//! Commands run start to finish against a stand-in for the drawing.

use std::cell::RefCell;
use std::rc::Rc;

use super::defaults::Defaults;
use super::prompts::{Host, PromptKind, ScaleSetting, Session};
use super::registry::create;
use crate::model::entities::{Entity, Shape, circle, line, polyline_from};
use crate::model::geometry::{Point, pt};
use crate::model::index::{Bounds, entity_extent};

/// Keeps the drawing as a list, and undoes by going back to the list before each change or group of changes.
#[derive(Default)]
struct FakeHost {
    entities: Vec<Entity>,
    groups: Vec<crate::model::document::ObjectGroup>,
    layers: Option<(Vec<crate::model::document::Layer>, Option<String>)>,
    history: Vec<Vec<Entity>>,
    group_depth: usize,
    reports: Vec<String>,
    /// The WIPEOUTFRAME mode a command set, if one did.
    frame_mode: Option<u8>,
    /// The linetype scales commands set, in order.
    scales: Vec<ScaleSetting>,
    /// The blocks ATTSYNC asked to sync.
    synced: Vec<crate::model::entities::BlockRef>,
    /// The UCS typed points are read in, and what UCS asked to change.
    ucs: crate::model::ucs::Ucs,
    ucs_requests: Vec<super::prompts::UcsRequest>,
}

impl FakeHost {
    fn with(entities: Vec<Entity>) -> FakeHost {
        FakeHost { entities, ..FakeHost::default() }
    }

    fn checkpoint(&mut self) {
        if self.group_depth == 0 {
            self.history.push(self.entities.clone());
        }
    }

    fn shapes(&self) -> Vec<Shape> {
        self.entities.iter().map(|e| e.shape.clone()).collect()
    }
}

impl Host for FakeHost {
    fn add(&mut self, entities: Vec<Entity>) {
        self.checkpoint();
        self.entities.extend(entities);
    }
    fn add_new(&mut self, entities: Vec<Entity>) {
        self.add(entities);
    }
    fn remove(&mut self, entities: Vec<Entity>) {
        self.checkpoint();
        self.entities.retain(|e| !entities.iter().any(|gone| gone.id == e.id));
    }
    fn modify(&mut self, states: Vec<Entity>) {
        self.checkpoint();
        for state in states {
            if let Some(target) = self.entities.iter_mut().find(|e| e.id == state.id) {
                target.copy_state(&state);
            }
        }
    }
    fn retract(&mut self) {
        if let Some(before) = self.history.pop() {
            self.entities = before;
        }
    }
    fn begin_group(&mut self) {
        self.checkpoint();
        self.group_depth += 1;
    }
    fn end_group(&mut self) {
        self.group_depth -= 1;
    }
    fn report(&mut self, message: String) {
        self.reports.push(message);
    }
    fn visible_in(&self, area: &Bounds) -> Vec<Entity> {
        self.entities.iter().filter(|e| entity_extent(e).touches(area)).cloned().collect()
    }
    fn layers(&mut self, layers: Vec<crate::model::document::Layer>, current: Option<String>) {
        self.layers = Some((layers, current));
    }
    fn groups(&mut self, groups: Vec<crate::model::document::ObjectGroup>) {
        self.groups = groups;
    }
    fn wipeout_frame(&mut self, mode: u8) {
        self.frame_mode = Some(mode);
    }
    fn linetype_scale(&mut self, setting: ScaleSetting) {
        self.scales.push(setting);
    }
    fn extent(&self) -> Option<Bounds> {
        self.entities.iter().map(entity_extent).reduce(Bounds::union)
    }
    fn sync_attributes(&mut self, block: crate::model::entities::BlockRef) {
        self.synced.push(block);
    }
    fn ucs(&self) -> crate::model::ucs::Ucs {
        self.ucs
    }
    fn ucs_request(&mut self, request: super::prompts::UcsRequest) {
        self.ucs_requests.push(request);
    }
}

fn start(name: &str, host: &mut FakeHost, selected: Vec<Entity>) -> Session {
    start_with(name, host, selected, Rc::new(RefCell::new(Defaults::default())))
}

fn start_with(name: &str, host: &mut FakeHost, selected: Vec<Entity>, defaults: Rc<RefCell<Defaults>>) -> Session {
    Session::start(create(name, selected, defaults), host)
}

fn typed(session: &mut Session, host: &mut FakeHost, answers: &[&str]) {
    for answer in answers {
        assert!(session.active(), "the command ended before {answer}");
        assert_eq!(session.submit_text(answer, host), None, "{answer} was refused at {:?}", session.prompt);
    }
}

fn line_shape(start: Point, end: Point) -> Shape {
    Shape::Line { start, end }
}

#[test]
fn line_draws_a_chain_and_closes_it() {
    let mut host = FakeHost::default();
    let mut session = start("LINE", &mut host, vec![]);
    typed(&mut session, &mut host, &["0,0", "10,0", "@0,10", "c"]);
    assert!(!session.active());
    assert_eq!(
        host.shapes(),
        vec![line_shape(pt(0.0, 0.0), pt(10.0, 0.0)), line_shape(pt(10.0, 0.0), pt(10.0, 10.0)), line_shape(pt(10.0, 10.0), pt(0.0, 0.0))]
    );
}

#[test]
fn line_undo_takes_back_the_last_segment() {
    let mut host = FakeHost::default();
    let mut session = start("LINE", &mut host, vec![]);
    typed(&mut session, &mut host, &["0,0", "5,0", "5,5", "u", "0,5"]);
    session.cancel(&mut host);
    assert_eq!(host.shapes(), vec![line_shape(pt(0.0, 0.0), pt(5.0, 0.0)), line_shape(pt(5.0, 0.0), pt(0.0, 5.0))]);
}

#[test]
fn polyline_finishes_with_what_it_has_on_esc() {
    let mut host = FakeHost::default();
    let mut session = start("PLINE", &mut host, vec![]);
    typed(&mut session, &mut host, &["0,0", "1,0", "1,1"]);
    session.cancel(&mut host);
    assert!(!session.active());
    let Shape::Polyline { vertices, closed } = &host.shapes()[0] else { panic!() };
    assert_eq!(vertices.len(), 3);
    assert!(!closed);
}

#[test]
fn bad_input_is_explained_and_the_prompt_stays() {
    let mut host = FakeHost::default();
    let mut session = start("CIRCLE", &mut host, vec![]);
    assert_eq!(session.submit_text("nowhere", &mut host), Some("Enter a point as x,y or @dx,dy or @distance<angle."));
    typed(&mut session, &mut host, &["2,2"]);
    assert_eq!(session.submit_text("-1", &mut host), Some("Enter a number greater than zero."));
    typed(&mut session, &mut host, &["d", "6"]);
    assert_eq!(host.shapes(), vec![Shape::Circle { center: pt(2.0, 2.0), radius: 3.0 }]);
}

#[test]
fn clicking_a_radius_measures_from_the_center() {
    let mut host = FakeHost::default();
    let mut session = start("CIRCLE", &mut host, vec![]);
    session.submit_point(pt(0.0, 0.0), &mut host);
    assert_eq!(session.prompt.as_ref().unwrap().kind, PromptKind::Number);
    session.submit_point(pt(3.0, 4.0), &mut host);
    assert_eq!(host.shapes(), vec![Shape::Circle { center: pt(0.0, 0.0), radius: 5.0 }]);
}

#[test]
fn typing_a_point_for_a_radius_or_an_angle_measures_from_the_base_point() {
    let mut host = FakeHost::default();
    let mut session = start("CIRCLE", &mut host, vec![]);
    typed(&mut session, &mut host, &["10,10", "13,14"]);
    assert_eq!(host.shapes(), vec![Shape::Circle { center: pt(10.0, 10.0), radius: 5.0 }]);
    let mut session = start("CIRCLE", &mut host, vec![]);
    typed(&mut session, &mut host, &["10,10", "@0,2"]);
    assert_eq!(host.shapes()[1], Shape::Circle { center: pt(10.0, 10.0), radius: 2.0 });
    let mut session = start("CIRCLE", &mut host, vec![]);
    typed(&mut session, &mut host, &["10,10"]);
    assert_eq!(session.submit_text("10,10", &mut host), Some("Enter a number greater than zero."), "the center gives no radius");
    let target = line(pt(0.0, 0.0), pt(10.0, 0.0));
    let mut host = FakeHost::with(vec![target.clone()]);
    let mut session = start("ROTATE", &mut host, vec![target]);
    typed(&mut session, &mut host, &["0,0", "0,5"]);
    let Some(Shape::Line { end, .. }) = host.shapes().pop() else { panic!("no line") };
    assert!(end.dist(pt(0.0, 10.0)) < 1e-9, "turned to point up: {end:?}");
}

#[test]
fn pline_arcs_carry_on_from_the_segment_before() {
    let mut host = FakeHost::default();
    let mut session = start("PLINE", &mut host, vec![]);
    // East along the bottom, a half circle up and back, then a straight run west.
    typed(&mut session, &mut host, &["0,0", "10,0", "A", "10,10", "L", "0,10", ""]);
    let Some(Shape::Polyline { vertices, closed: false }) = host.shapes().pop() else { panic!("no polyline") };
    let bulges: Vec<f64> = vertices.iter().map(|v| (v.bulge * 1e9).round() / 1e9).collect();
    assert_eq!(bulges, vec![0.0, 1.0, 0.0, 0.0], "a half circle turning left has a bulge of 1");
    // An arc through a second point, and closing with an arc.
    let mut session = start("PLINE", &mut host, vec![]);
    typed(&mut session, &mut host, &["0,0", "A", "S", "5,5", "10,0", "0,-10", "CL"]);
    let Some(Shape::Polyline { vertices, closed: true }) = host.shapes().pop() else { panic!("no closed polyline") };
    assert!((vertices[0].bulge + 1.0).abs() < 1e-9, "through 5,5 the arc turns right: {}", vertices[0].bulge);
    assert!(vertices[2].bulge != 0.0, "the closing segment is an arc too");
}

#[test]
fn rotate_and_scale_take_copy_and_reference() {
    let target = line(pt(0.0, 0.0), pt(10.0, 0.0));
    let mut host = FakeHost::with(vec![target.clone()]);
    let mut session = start("ROTATE", &mut host, vec![target.clone()]);
    typed(&mut session, &mut host, &["0,0", "C", "90"]);
    assert_eq!(host.entities.len(), 2, "Copy keeps the original");
    let Some(Shape::Line { end, .. }) = host.shapes().pop() else { panic!("no line") };
    assert!(end.dist(pt(0.0, 10.0)) < 1e-9, "{end:?}");
    // From 30 degrees to 75 is a turn of 45.
    let mut session = start("ROTATE", &mut host, vec![target.clone()]);
    typed(&mut session, &mut host, &["0,0", "R", "30", "75"]);
    let Shape::Line { end, .. } = host.entities[0].shape else { panic!("no line") };
    assert!(end.dist(pt(50f64.sqrt(), 50f64.sqrt())) < 1e-9, "{end:?}");
    // A length of 10 brought to 25 is a factor of 2.5.
    let other = line(pt(0.0, 0.0), pt(0.0, 4.0));
    let mut host = FakeHost::with(vec![other.clone()]);
    let mut session = start("SCALE", &mut host, vec![other.clone()]);
    typed(&mut session, &mut host, &["0,0", "R", "10", "25"]);
    assert_eq!(host.shapes(), vec![Shape::Line { start: pt(0.0, 0.0), end: pt(0.0, 10.0) }]);
    let scaled = host.entities[0].clone();
    let mut session = start("SCALE", &mut host, vec![scaled]);
    typed(&mut session, &mut host, &["0,0", "C", "2"]);
    assert_eq!(host.entities.len(), 2, "Copy keeps the original");
}

#[test]
fn join_closes_gaps_between_lines_in_a_line_and_area_measures_an_ellipse() {
    let (left, right) = (line(pt(0.0, 0.0), pt(3.0, 0.0)), line(pt(6.0, 0.0), pt(10.0, 0.0)));
    let up = line(pt(10.0, 0.0), pt(10.0, 5.0));
    let aside = line(pt(0.0, 1.0), pt(3.0, 1.0));
    let mut host = FakeHost::with(vec![left.clone(), right.clone(), up.clone(), aside.clone()]);
    let mut session = start("JOIN", &mut host, vec![left, right, up, aside]);
    typed(&mut session, &mut host, &[]);
    let shapes = host.shapes();
    assert_eq!(shapes.len(), 2, "the parallel line stays as it is: {shapes:?}");
    let Some(Shape::Polyline { vertices, closed: false }) = shapes.iter().find(|s| matches!(s, Shape::Polyline { .. })) else {
        panic!("the two lines in a line and the one that meets them make one polyline: {shapes:?}")
    };
    let corners: Vec<Point> = vertices.iter().map(|v| v.point()).collect();
    assert!(
        corners == [pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 5.0)] || corners == [pt(10.0, 5.0), pt(10.0, 0.0), pt(0.0, 0.0)],
        "{corners:?}"
    );
    let oval = crate::model::entities::ellipse(pt(0.0, 0.0), pt(4.0, 0.0), 0.5);
    let measured = crate::model::measure::measure_entity(&oval).expect("a whole ellipse has an area");
    assert!((measured.area - std::f64::consts::PI * 8.0).abs() < 1e-9);
    assert!((measured.perimeter - 19.3769).abs() < 1e-3, "{}", measured.perimeter);
}

#[test]
fn three_point_arc() {
    let mut host = FakeHost::default();
    let mut session = start("ARC", &mut host, vec![]);
    typed(&mut session, &mut host, &["1,0", "0,1", "0,1"]);
    assert!(session.prompt.as_ref().unwrap().message.starts_with("The points are in a line."));
    typed(&mut session, &mut host, &["-1,0"]);
    let Shape::Arc { radius, start_angle, end_angle, .. } = host.shapes()[0] else { panic!() };
    assert!((radius - 1.0).abs() < 1e-9 && start_angle.abs() < 1e-9 && (end_angle - 180.0).abs() < 1e-9);
}

#[test]
fn move_changes_the_selected_objects_in_place() {
    let target = line(pt(0.0, 0.0), pt(1.0, 0.0));
    let id = target.id;
    let mut host = FakeHost::with(vec![target.clone()]);
    let mut session = start("MOVE", &mut host, vec![target]);
    typed(&mut session, &mut host, &["0,0", "5,5"]);
    assert_eq!(host.entities[0].id, id);
    assert_eq!(host.entities[0].shape, line_shape(pt(5.0, 5.0), pt(6.0, 5.0)));
}

#[test]
fn copy_makes_copies_until_enter_and_undo_takes_one_back() {
    let target = circle(pt(0.0, 0.0), 1.0);
    let mut host = FakeHost::with(vec![target.clone()]);
    let mut session = start("COPY", &mut host, vec![target]);
    typed(&mut session, &mut host, &["0,0", "10,0", "20,0", "u", "0,10", ""]);
    assert!(!session.active());
    let centers: Vec<Shape> = host.shapes();
    assert_eq!(
        centers,
        vec![
            Shape::Circle { center: pt(0.0, 0.0), radius: 1.0 },
            Shape::Circle { center: pt(10.0, 0.0), radius: 1.0 },
            Shape::Circle { center: pt(0.0, 10.0), radius: 1.0 },
        ]
    );
}

#[test]
fn erase_asks_for_objects_when_nothing_is_selected() {
    let target = line(pt(0.0, 0.0), pt(1.0, 0.0));
    let mut host = FakeHost::with(vec![target.clone()]);
    let mut session = start("ERASE", &mut host, vec![]);
    assert_eq!(session.prompt.as_ref().unwrap().kind, PromptKind::Select);
    session.submit_objects(vec![target], &mut host);
    assert!(host.entities.is_empty());
}

#[test]
fn oops_brings_back_what_erase_took_out_and_keeps_the_new_line() {
    let first = line(pt(0.0, 0.0), pt(1.0, 0.0));
    let second = line(pt(0.0, 1.0), pt(1.0, 1.0));
    let mut host = FakeHost::with(vec![first.clone(), second.clone()]);
    let defaults = Rc::new(RefCell::new(Defaults::default()));
    let session = start_with("ERASE", &mut host, vec![first.clone(), second.clone()], defaults.clone());
    assert!(!session.active());
    assert!(host.entities.is_empty());

    let mut session = start_with("LINE", &mut host, vec![], defaults.clone());
    typed(&mut session, &mut host, &["0,5", "5,5", ""]);
    assert_eq!(host.shapes(), vec![line_shape(pt(0.0, 5.0), pt(5.0, 5.0))]);

    let session = start_with("OOPS", &mut host, vec![], defaults.clone());
    assert!(!session.active());
    assert_eq!(host.entities.len(), 3);
    assert!(host.entities.iter().any(|e| e.id == first.id));
    assert!(host.entities.iter().any(|e| e.id == second.id));
    assert_eq!(host.shapes()[0], line_shape(pt(0.0, 5.0), pt(5.0, 5.0)));
    assert!(defaults.borrow().last_erased.is_empty());

    start_with("OOPS", &mut host, vec![], defaults);
    assert_eq!(host.reports.last().unwrap(), "Nothing to restore.");
    assert_eq!(host.entities.len(), 3);
}

#[test]
fn mirror_keeps_the_source_by_default() {
    let target = line(pt(1.0, 0.0), pt(2.0, 0.0));
    let mut host = FakeHost::with(vec![target.clone()]);
    let mut session = start("MIRROR", &mut host, vec![target]);
    typed(&mut session, &mut host, &["0,0", "0,0"]);
    assert!(session.prompt.as_ref().unwrap().message.starts_with("The mirror line needs two different points."));
    typed(&mut session, &mut host, &["0,1", ""]);
    assert_eq!(host.entities.len(), 2);
    let Shape::Line { start, end } = host.shapes()[1] else { panic!() };
    assert!(start.dist(pt(-1.0, 0.0)) < 1e-9 && end.dist(pt(-2.0, 0.0)) < 1e-9);
}

#[test]
fn trim_then_undo_restores_the_object() {
    let target = line(pt(0.0, 0.0), pt(10.0, 0.0));
    let cutters = vec![line(pt(3.0, -1.0), pt(3.0, 1.0)), line(pt(7.0, -1.0), pt(7.0, 1.0))];
    let mut all = cutters.clone();
    all.push(target.clone());
    let mut host = FakeHost::with(all);
    let mut session = start("TRIM", &mut host, cutters);
    session.submit_pick(target.clone(), pt(5.0, 0.0), false, &mut host);
    assert_eq!(host.entities.len(), 4);
    assert!(!host.entities.iter().any(|e| e.id == target.id));
    typed(&mut session, &mut host, &["u"]);
    assert_eq!(host.entities.len(), 3);
    assert!(host.entities.iter().any(|e| e.id == target.id));
    session.cancel(&mut host);
    assert!(!session.active());
}

#[test]
fn shift_pick_extends_while_trimming() {
    let target = line(pt(0.0, 0.0), pt(5.0, 0.0));
    let boundary = line(pt(9.0, -1.0), pt(9.0, 1.0));
    let mut host = FakeHost::with(vec![target.clone(), boundary.clone()]);
    let mut session = start("TRIM", &mut host, vec![boundary]);
    session.submit_pick(target, pt(4.0, 0.0), true, &mut host);
    assert_eq!(host.shapes()[0], line_shape(pt(0.0, 0.0), pt(9.0, 0.0)));
}

#[test]
fn fillet_remembers_its_radius() {
    let defaults = Rc::new(RefCell::new(Defaults::default()));
    let horizontal = line(pt(-10.0, 0.0), pt(10.0, 0.0));
    let vertical = line(pt(0.0, -10.0), pt(0.0, 10.0));
    let mut host = FakeHost::with(vec![horizontal.clone(), vertical.clone()]);
    let mut session = start_with("FILLET", &mut host, vec![], defaults.clone());
    typed(&mut session, &mut host, &["r", "2"]);
    assert_eq!(session.prompt.as_ref().unwrap().message, "Select first object (radius 2) or [Polyline/Radius]:");
    session.submit_pick(horizontal, pt(5.0, 0.0), false, &mut host);
    session.submit_pick(vertical, pt(0.0, 5.0), false, &mut host);
    assert!(!session.active());
    assert_eq!(host.entities.len(), 3);
    assert_eq!(defaults.borrow().fillet_radius, 2.0);
    host.retract();
    assert_eq!(host.entities.len(), 2, "the fillet undoes as one step");
}

#[test]
fn fillet_rounds_a_corner_of_a_rectangle() {
    let rectangle = polyline_from(&[pt(0.0, 0.0), pt(20.0, 0.0), pt(20.0, 10.0), pt(0.0, 10.0)], true);
    let mut host = FakeHost::with(vec![rectangle.clone()]);
    let mut session = start("FILLET", &mut host, vec![]);
    typed(&mut session, &mut host, &["r", "2"]);
    session.submit_pick(rectangle.clone(), pt(0.0, 5.0), false, &mut host);
    session.submit_pick(rectangle, pt(10.0, 0.0), false, &mut host);
    assert!(!session.active());
    let Shape::Polyline { vertices, closed: true } = &host.shapes()[0] else { panic!("{:?}", host.shapes()) };
    assert_eq!(vertices.len(), 5, "the corner became two tangent points");
    assert!(vertices[0].point().dist(pt(0.0, 2.0)) < 1e-9 && vertices[1].point().dist(pt(2.0, 0.0)) < 1e-9);
    assert!((vertices[0].bulge - (std::f64::consts::PI / 8.0).tan()).abs() < 1e-9, "a quarter turn counter-clockwise");
}

#[test]
fn fillet_polyline_option_rounds_every_corner() {
    let rectangle = polyline_from(&[pt(0.0, 0.0), pt(20.0, 0.0), pt(20.0, 10.0), pt(0.0, 10.0)], true);
    let mut host = FakeHost::with(vec![rectangle.clone()]);
    let mut session = start("FILLET", &mut host, vec![]);
    typed(&mut session, &mut host, &["r", "3", "p"]);
    session.submit_pick(rectangle, pt(10.0, 0.0), false, &mut host);
    assert!(!session.active());
    let Shape::Polyline { vertices, .. } = &host.shapes()[0] else { panic!() };
    assert_eq!(vertices.len(), 8);
    assert_eq!(host.reports, vec!["4 lines were filleted"]);
}

#[test]
fn area_of_typed_corners_is_reported() {
    let mut host = FakeHost::default();
    let mut session = start("AREA", &mut host, vec![]);
    typed(&mut session, &mut host, &["0,0", "4,0", "4,3", ""]);
    assert_eq!(host.reports, vec!["Area = 6.0000, Perimeter = 12.0000"]);
}

#[test]
fn dimscale_is_remembered_for_the_next_dimension() {
    let defaults = Rc::new(RefCell::new(Defaults::default()));
    let mut host = FakeHost::default();
    let mut session = start_with("DIMSCALE", &mut host, vec![], defaults.clone());
    typed(&mut session, &mut host, &["4"]);
    let mut session = start_with("DIMLINEAR", &mut host, vec![], defaults);
    typed(&mut session, &mut host, &["0,0", "10,0", "5,5"]);
    assert_eq!(
        host.shapes(),
        vec![Shape::Dimension {
            start: pt(0.0, 0.0),
            end: pt(10.0, 0.0),
            line_point: pt(5.0, 5.0),
            angle: 0.0,
            look: crate::model::dimension::DimensionLook::scaled(4.0)
        }]
    );
}

#[test]
fn dimtxt_and_dimasz_size_the_next_dimension() {
    let defaults = Rc::new(RefCell::new(Defaults::default()));
    let mut host = FakeHost::default();
    let mut session = start_with("DIMTXT", &mut host, vec![], defaults.clone());
    typed(&mut session, &mut host, &["0.2"]);
    let mut session = start_with("DIMASZ", &mut host, vec![], defaults.clone());
    typed(&mut session, &mut host, &["0.1"]);
    let mut session = start_with("DIMLINEAR", &mut host, vec![], defaults);
    typed(&mut session, &mut host, &["0,0", "10,0", "5,5"]);
    let [Shape::Dimension { look, .. }] = &host.shapes()[..] else { panic!() };
    assert_eq!((look.text_height, look.arrow_size), (0.2, 0.1));
}

#[test]
fn dimstyle_apply_gives_dimensions_the_current_settings() {
    use crate::model::dimension::DimensionLook;
    let big = DimensionLook { text_height: 1.8, text: "<> m".into(), text_position: Some(pt(20.0, 3.0)), ..DimensionLook::default() };
    let old = Entity::new(Shape::Dimension { start: pt(0.0, 0.0), end: pt(2.6, 0.0), line_point: pt(0.0, 1.0), angle: 0.0, look: big });
    let mut host = FakeHost::with(vec![old.clone(), line(pt(0.0, 0.0), pt(1.0, 0.0))]);
    let defaults = Rc::new(RefCell::new(Defaults::default()));
    defaults.borrow_mut().dimension_look.text_height = 0.2;
    let selected = host.entities.clone();
    let mut session = start_with("-DIMSTYLE", &mut host, selected, defaults);
    typed(&mut session, &mut host, &["A"]);
    let Shape::Dimension { look, .. } = &host.entities[0].shape else { panic!() };
    assert_eq!(look.text_height, 0.2);
    assert_eq!(look.text, "<> m", "text typed over the measurement stays");
    assert_eq!(look.text_position, None, "the text is laid out afresh");
    assert_eq!(host.reports, vec!["1 dimension(s) updated."]);
}

#[test]
fn text_takes_defaults_on_enter() {
    let mut host = FakeHost::default();
    let mut session = start("TEXT", &mut host, vec![]);
    typed(&mut session, &mut host, &["1,1", "", "", "Hello world"]);
    assert_eq!(
        host.shapes(),
        vec![Shape::Text {
            position: pt(1.0, 1.0),
            text: "Hello world".into(),
            height: 2.5,
            rotation: 0.0,
            font: String::new(),
            style: String::new(),
            align: Default::default(),
            line_spacing: 1.0,
            formatted: String::new(),
            width_factor: 1.0,
            oblique: 0.0,
        }]
    );
}

#[test]
fn previews_follow_the_cursor() {
    let mut host = FakeHost::default();
    let mut session = start("RECTANGLE", &mut host, vec![]);
    typed(&mut session, &mut host, &["0,0"]);
    let preview = session.prompt.as_ref().unwrap().preview.clone().unwrap();
    let Shape::Polyline { vertices, closed } = &preview(pt(2.0, 3.0))[0].shape else { panic!() };
    assert!(*closed);
    assert_eq!(vertices[2].point(), pt(2.0, 3.0));
}

#[test]
fn hatch_fills_the_room_clicked_in_and_leaves_its_column_empty() {
    // Two rooms sharing a wall, drawn as loose lines; the right one holds a round column.
    let walls = [
        ((0.0, 0.0), (20.0, 0.0)),
        ((20.0, 0.0), (20.0, 10.0)),
        ((20.0, 10.0), (0.0, 10.0)),
        ((0.0, 10.0), (0.0, 0.0)),
        ((10.0, 0.0), (10.0, 10.0)),
    ];
    let mut entities: Vec<Entity> = walls.iter().map(|&((x0, y0), (x1, y1))| line(pt(x0, y0), pt(x1, y1))).collect();
    entities.push(circle(pt(15.0, 5.0), 1.0));
    let mut host = FakeHost::with(entities);
    let mut session = start("HATCH", &mut host, vec![]);
    typed(&mut session, &mut host, &["SOLID", "30,5", "12,2", ""]);
    assert_eq!(host.reports, vec!["Valid hatch boundary not found.", "Boundary found, with 1 island(s) left unfilled."]);
    let Some(Shape::Hatch { loops, solid: true, .. }) = host.shapes().last().cloned() else { panic!("no hatch") };
    assert_eq!(loops.len(), 2);
    let xs: Vec<f64> = loops[0].iter().map(|p| p.x).collect();
    assert!(xs.iter().all(|x| (10.0..=20.0).contains(x)), "the right room only: {xs:?}");
}

#[test]
fn dimradius_points_at_the_circle_where_its_text_goes() {
    use crate::model::annotation::AnnotationKind;
    let target = circle(pt(0.0, 0.0), 5.0);
    let mut host = FakeHost::with(vec![target.clone()]);
    let mut session = start("DIMRADIUS", &mut host, vec![]);
    session.submit_pick(target, pt(5.0, 0.0), false, &mut host);
    typed(&mut session, &mut host, &["0,12"]);
    let Some(Shape::Annotation { kind: AnnotationKind::Radius, points, look }) = host.shapes().last().cloned() else { panic!() };
    assert_eq!(points, vec![pt(0.0, 0.0), pt(0.0, 5.0)]);
    assert_eq!(look.text_position, Some(pt(0.0, 12.0)));
}

#[test]
fn dimangular_between_two_lines_measures_the_side_clicked() {
    use crate::model::annotation::{AnnotationKind, annotation_measurement};
    let (a, b) = (line(pt(0.0, 0.0), pt(10.0, 0.0)), line(pt(0.0, 0.0), pt(10.0, 10.0)));
    let mut host = FakeHost::with(vec![a.clone(), b.clone()]);
    let mut session = start("DIMANGULAR", &mut host, vec![]);
    session.submit_pick(a, pt(5.0, 0.0), false, &mut host);
    session.submit_pick(b, pt(5.0, 5.0), false, &mut host);
    typed(&mut session, &mut host, &["8,3"]);
    let Some(Shape::Annotation { kind: AnnotationKind::Angle, points, .. }) = host.shapes().last().cloned() else { panic!() };
    assert!((annotation_measurement(AnnotationKind::Angle, &points).unwrap() - 45.0).abs() < 1e-9);
    let mut session = start("DIMANGULAR", &mut host, vec![]);
    typed(&mut session, &mut host, &["", "0,0", "10,0", "0,10", "-3,-3"]);
    let Some(Shape::Annotation { points, .. }) = host.shapes().last().cloned() else { panic!() };
    assert!((annotation_measurement(AnnotationKind::Angle, &points).unwrap() - 270.0).abs() < 1e-9);
}

#[test]
fn qleader_draws_a_leader_and_its_text_beside_a_landing() {
    use crate::model::annotation::AnnotationKind;
    let mut host = FakeHost::default();
    let mut session = start("QLEADER", &mut host, vec![]);
    typed(&mut session, &mut host, &["0,0", "10,10", "", "First", "Second", ""]);
    let shapes = host.shapes();
    let [Shape::Annotation { kind: AnnotationKind::Leader { arrow: true }, points, .. }, Shape::Text { text, position, .. }] = &shapes[..]
    else {
        panic!("{shapes:?}")
    };
    assert_eq!(points.len(), 3, "a level landing after the slanted line");
    assert_eq!(points[2].y, 10.0);
    assert_eq!(text, "First\nSecond");
    assert!(position.x > points[2].x);
}

#[test]
fn dimstyle_save_and_restore_name_the_current_settings() {
    let defaults = Rc::new(RefCell::new(Defaults::default()));
    let mut host = FakeHost::default();
    defaults.borrow_mut().dimension_look.text_height = 0.3;
    let mut session = start_with("-DIMSTYLE", &mut host, vec![], defaults.clone());
    typed(&mut session, &mut host, &["S", "Metric"]);
    assert_eq!(defaults.borrow().dimension_look.style, "Metric");
    let mut session = start_with("-DIMSTYLE", &mut host, vec![], defaults.clone());
    typed(&mut session, &mut host, &["R", "standard"]);
    assert_eq!(host.reports, vec!["Current dimension style: Standard"]);
    assert_eq!(defaults.borrow().dimension_look.text_height, 2.5);
}

#[test]
fn group_ungroup_and_groupedit_work_on_named_and_unnamed_groups() {
    let (a, b, c) = (line(pt(0.0, 0.0), pt(1.0, 0.0)), line(pt(0.0, 1.0), pt(1.0, 1.0)), circle(pt(5.0, 5.0), 1.0));
    let mut host = FakeHost::with(vec![a.clone(), b.clone(), c.clone()]);
    let defaults = Rc::new(RefCell::new(Defaults::default()));
    let mut session = start_with("GROUP", &mut host, vec![a.clone(), b.clone()], defaults.clone());
    typed(&mut session, &mut host, &[""]);
    assert_eq!(host.groups.len(), 1);
    assert_eq!((host.groups[0].name.as_str(), host.groups[0].unnamed), ("*A1", true));
    defaults.borrow_mut().groups = host.groups.clone();
    let mut session = start_with("GROUP", &mut host, vec![c.clone()], defaults.clone());
    typed(&mut session, &mut host, &["D", "The well", "Well"]);
    assert_eq!((host.groups[1].name.as_str(), host.groups[1].description.as_str()), ("Well", "The well"));
    defaults.borrow_mut().groups = host.groups.clone();
    let mut session = start_with("GROUPEDIT", &mut host, vec![], defaults.clone());
    session.submit_pick(a.clone(), pt(0.5, 0.0), false, &mut host);
    typed(&mut session, &mut host, &["REN", "Edges"]);
    assert_eq!((host.groups[0].name.as_str(), host.groups[0].unnamed), ("Edges", false));
    defaults.borrow_mut().groups = host.groups.clone();
    let mut session = start_with("UNGROUP", &mut host, vec![c], defaults);
    typed(&mut session, &mut host, &[]);
    assert_eq!(host.groups.iter().map(|g| g.name.as_str()).collect::<Vec<_>>(), vec!["Edges"]);
    assert!(host.reports.contains(&"Group \"Well\" has been exploded.".to_string()));
}

fn points_of(host: &FakeHost) -> Vec<Point> {
    host.shapes().into_iter().filter_map(|s| if let Shape::Point { position } = s { Some(position) } else { None }).collect()
}

#[test]
fn polygon_by_center_and_by_edge() {
    let mut host = FakeHost::default();
    let mut session = start("POLYGON", &mut host, vec![]);
    typed(&mut session, &mut host, &["6", "0,0", "I", "10,0"]);
    let Some(Shape::Polyline { vertices, closed: true }) = host.shapes().pop() else { panic!() };
    assert_eq!(vertices.len(), 6);
    assert!(vertices.iter().all(|v| (v.point().dist(pt(0.0, 0.0)) - 10.0).abs() < 1e-9));
    let mut session = start("POLYGON", &mut host, vec![]);
    typed(&mut session, &mut host, &["4", "E", "0,0", "10,0"]);
    let Some(Shape::Polyline { vertices, .. }) = host.shapes().pop() else { panic!() };
    let corners: Vec<Point> = vertices.iter().map(|v| v.point()).collect();
    assert!(corners.iter().any(|c| c.dist(pt(10.0, 10.0)) < 1e-9), "{corners:?}");
}

#[test]
fn polygon_radius_typed_as_a_number_levels_the_bottom_edge() {
    let mut host = FakeHost::default();
    let mut session = start("POLYGON", &mut host, vec![]);
    typed(&mut session, &mut host, &["6", "0,0", "I", "10"]);
    let Some(Shape::Polyline { vertices, closed: true }) = host.shapes().pop() else { panic!("no polygon") };
    assert!(vertices.iter().all(|v| (v.point().dist(pt(0.0, 0.0)) - 10.0).abs() < 1e-9), "corners on the circle");
    let lowest = vertices.iter().map(|v| v.y).fold(f64::INFINITY, f64::min);
    let on_bottom = vertices.iter().filter(|v| (v.y - lowest).abs() < 1e-9).count();
    assert_eq!(on_bottom, 2, "a level bottom edge");
    let mut session = start("POLYGON", &mut host, vec![]);
    typed(&mut session, &mut host, &["4", "0,0", "C", "10"]);
    let Some(Shape::Polyline { vertices, .. }) = host.shapes().pop() else { panic!("no polygon") };
    let corners: Vec<Point> = vertices.iter().map(|v| v.point()).collect();
    assert!(corners.iter().any(|c| c.dist(pt(-10.0, -10.0)) < 1e-9), "a square around the circle: {corners:?}");
}

#[test]
fn mtext_hangs_its_lines_from_the_top_left_corner() {
    let mut host = FakeHost::default();
    let mut session = start("MTEXT", &mut host, vec![]);
    typed(&mut session, &mut host, &["0,0", "50,20", "3", "First line", "Second line", ""]);
    let Some(Shape::Text { position, text, height, .. }) = host.shapes().pop() else { panic!() };
    assert_eq!((position, text.as_str(), height), (pt(0.0, 20.0), "First line\nSecond line", 3.0));
}

#[test]
fn divide_and_measure_place_points_along_a_line() {
    let target = line(pt(0.0, 0.0), pt(10.0, 0.0));
    let mut host = FakeHost::with(vec![target.clone()]);
    let mut session = start("DIVIDE", &mut host, vec![]);
    session.submit_pick(target.clone(), pt(1.0, 0.0), false, &mut host);
    typed(&mut session, &mut host, &["4"]);
    let xs: Vec<f64> = points_of(&host).iter().map(|p| (p.x * 1e6).round() / 1e6).collect();
    assert_eq!(xs, vec![2.5, 5.0, 7.5]);
    let mut host = FakeHost::with(vec![target.clone()]);
    let mut session = start("MEASURE", &mut host, vec![]);
    session.submit_pick(target, pt(1.0, 0.0), false, &mut host);
    typed(&mut session, &mut host, &["3"]);
    assert_eq!(points_of(&host).len(), 3, "at 3, 6 and 9");
}

#[test]
fn lengthen_by_delta_at_the_end_picked() {
    let target = line(pt(0.0, 0.0), pt(10.0, 0.0));
    let mut host = FakeHost::with(vec![target.clone()]);
    let mut session = start("LENGTHEN", &mut host, vec![]);
    typed(&mut session, &mut host, &["DE", "5"]);
    session.submit_pick(target, pt(9.0, 0.0), false, &mut host);
    assert_eq!(host.shapes()[0], Shape::Line { start: pt(0.0, 0.0), end: pt(15.0, 0.0) });
}

#[test]
fn align_moves_turns_and_scales_onto_two_points() {
    let target = line(pt(0.0, 0.0), pt(10.0, 0.0));
    let mut host = FakeHost::with(vec![target.clone()]);
    let mut session = start("ALIGN", &mut host, vec![target]);
    typed(&mut session, &mut host, &["0,0", "5,5", "10,0", "5,25", "Y"]);
    let Shape::Line { start, end } = host.shapes()[0] else { panic!() };
    assert!(start.dist(pt(5.0, 5.0)) < 1e-9 && end.dist(pt(5.0, 25.0)) < 1e-9, "{start:?} {end:?}");
}

#[test]
fn pedit_turns_a_line_into_a_polyline_and_closes_it() {
    let target = line(pt(0.0, 0.0), pt(10.0, 0.0));
    let mut host = FakeHost::with(vec![target.clone()]);
    let mut session = start("PEDIT", &mut host, vec![]);
    session.submit_pick(target, pt(5.0, 0.0), false, &mut host);
    typed(&mut session, &mut host, &["", "C", ""]);
    assert!(matches!(&host.shapes()[0], Shape::Polyline { vertices, closed: true } if vertices.len() == 2));
}

#[test]
fn dimcontinue_chains_and_dimbaseline_steps_out() {
    let defaults = Rc::new(RefCell::new(Defaults::default()));
    let mut host = FakeHost::default();
    let mut session = start_with("DIMLINEAR", &mut host, vec![], defaults.clone());
    typed(&mut session, &mut host, &["0,0", "10,0", "5,5"]);
    let mut session = start_with("DIMCONTINUE", &mut host, vec![], defaults.clone());
    typed(&mut session, &mut host, &["20,0", "35,0"]);
    let dims: Vec<(Point, Point, Point)> = host
        .shapes()
        .into_iter()
        .filter_map(|s| if let Shape::Dimension { start, end, line_point, .. } = s { Some((start, end, line_point)) } else { None })
        .collect();
    assert_eq!(dims.len(), 3);
    assert_eq!((dims[1].0, dims[1].1), (pt(10.0, 0.0), pt(20.0, 0.0)));
    assert_eq!((dims[2].0, dims[2].1), (pt(20.0, 0.0), pt(35.0, 0.0)));
    assert!(dims.iter().all(|d| (d.2.y - 5.0).abs() < 1e-9), "all on one line");
    let mut host = FakeHost::default();
    let mut session = start_with("DIMLINEAR", &mut host, vec![], defaults.clone());
    typed(&mut session, &mut host, &["0,0", "10,0", "5,5"]);
    let mut session = start_with("DIMBASELINE", &mut host, vec![], defaults);
    typed(&mut session, &mut host, &["20,0"]);
    let Some(Shape::Dimension { start, end, line_point, .. }) = host.shapes().pop() else { panic!() };
    assert_eq!((start, end), (pt(0.0, 0.0), pt(20.0, 0.0)));
    assert!((line_point.y - (5.0 + 3.75)).abs() < 1e-9, "one step further out: {line_point:?}");
}

#[test]
fn layiso_keeps_only_the_layers_picked_and_layuniso_brings_the_rest_back() {
    use crate::model::document::Layer;
    let mut wall = line(pt(0.0, 0.0), pt(1.0, 0.0));
    wall.layer = "Walls".into();
    let defaults = Rc::new(RefCell::new(Defaults::default()));
    defaults.borrow_mut().layers = vec![Layer::new("0"), Layer::new("Walls"), Layer::new("Doors")];
    let mut host = FakeHost::with(vec![wall.clone()]);
    let mut session = start_with("LAYISO", &mut host, vec![wall], defaults.clone());
    typed(&mut session, &mut host, &[]);
    let (layers, current) = host.layers.clone().unwrap();
    let on: Vec<&str> = layers.iter().filter(|l| l.visible).map(|l| l.name.as_str()).collect();
    assert_eq!((on, current.as_deref()), (vec!["Walls"], Some("Walls")));
    defaults.borrow_mut().layers = layers;
    let mut session = start_with("LAYUNISO", &mut host, vec![], defaults);
    typed(&mut session, &mut host, &[]);
    assert!(host.layers.clone().unwrap().0.iter().all(|l| l.visible));
}

#[test]
fn xline_ray_donut_and_revcloud_draw_what_autocad_does() {
    let mut host = FakeHost::default();
    let mut session = start("XLINE", &mut host, vec![]);
    typed(&mut session, &mut host, &["H", "0,5", "0,10", ""]);
    assert_eq!(host.shapes().len(), 2);
    assert!(host.shapes().iter().all(|s| matches!(s, Shape::XLine { direction, ray: false, .. } if *direction == pt(1.0, 0.0))));
    let mut session = start("RAY", &mut host, vec![]);
    typed(&mut session, &mut host, &["0,0", "3,4", ""]);
    assert!(matches!(host.shapes().last(), Some(Shape::XLine { ray: true, direction, .. }) if (direction.x - 0.6).abs() < 1e-12));
    let mut session = start("DONUT", &mut host, vec![]);
    typed(&mut session, &mut host, &["2", "4", "10,10", ""]);
    assert!(matches!(host.shapes().last(), Some(Shape::Hatch { loops, solid: true, .. }) if loops.len() == 2));
    let mut session = start("REVCLOUD", &mut host, vec![]);
    typed(&mut session, &mut host, &["A", "2", "0,0", "20,10"]);
    let Some(Shape::Polyline { vertices, closed: true }) = host.shapes().pop() else { panic!() };
    assert_eq!(vertices.len(), 30, "60 around at about 2 each");
    assert!(vertices.iter().all(|v| v.bulge > 0.0), "arcs along a counter-clockwise outline bow out of it");
    let mut session = start("REVCLOUD", &mut host, vec![]);
    typed(&mut session, &mut host, &["A", "0.000001", "0,0", "1000,1000"]);
    let Some(Shape::Polyline { vertices, .. }) = host.shapes().pop() else { panic!() };
    assert_eq!(vertices.len(), 10_000, "an arc length far too short for the outline gives the most arcs a cloud has");
}

#[test]
fn an_array_too_large_to_make_is_refused() {
    let target = line(pt(0.0, 0.0), pt(1.0, 0.0));
    let mut host = FakeHost::with(vec![target.clone()]);
    let mut session = start("ARRAY", &mut host, vec![target.clone()]);
    typed(&mut session, &mut host, &["R", "100000", "100000"]);
    assert!(!session.active(), "it asks no more");
    assert!(host.reports.iter().any(|r| r.starts_with("Too many elements in the array")), "{:?}", host.reports);
    let mut session = start("ARRAY", &mut host, vec![target.clone()]);
    typed(&mut session, &mut host, &["P", "0,0", "1e300"]);
    assert!(!session.active() && host.entities.len() == 1, "nothing made");
    let mut session = start("ARRAY", &mut host, vec![target]);
    typed(&mut session, &mut host, &["R", "2", "3", "5", "5"]);
    assert_eq!(host.entities.len(), 6, "an ordinary array still works");
}

#[test]
fn attdef_then_block_gives_a_block_whose_inserts_ask_for_the_value() {
    let mut host = FakeHost::default();
    let mut session = start("ATTDEF", &mut host, vec![]);
    typed(&mut session, &mut host, &["", "room", "Room number?", "101", "0,0", "2"]);
    let definition = host.entities[0].clone();
    assert!(matches!(&definition.shape, Shape::Text { text, .. } if text == "ROOM"), "shown by its tag");
    let frame = line(pt(-5.0, -5.0), pt(5.0, -5.0));
    host.entities.push(frame.clone());
    let defaults = Rc::new(RefCell::new(Defaults::default()));
    let mut session = start_with("BLOCK", &mut host, vec![definition, frame], defaults.clone());
    typed(&mut session, &mut host, &["Door", "0,0"]);
    let block = defaults.borrow().blocks[0].clone();
    assert_eq!(block.0.entities.len(), 1);
    assert_eq!((block.0.attribute_defs[0].tag.as_str(), block.0.attribute_defs[0].prompt.as_str()), ("ROOM", "Room number?"));
    let mut session = start_with("INSERT", &mut host, vec![], defaults);
    assert!(session.prompt.as_ref().is_some());
    typed(&mut session, &mut host, &["Door", "50,50", "1", "0"]);
    assert_eq!(session.prompt.as_ref().unwrap().message, "Room number? <101>:");
    typed(&mut session, &mut host, &["204"]);
    let Some(Shape::Insert { attributes, .. }) = host.shapes().pop() else { panic!() };
    assert!(matches!(&attributes[0].text, Shape::Text { text, .. } if text == "204"));
}

#[test]
fn table_makes_a_table_and_tabledit_changes_a_cell() {
    let mut host = FakeHost::default();
    let mut session = start("TABLE", &mut host, vec![]);
    typed(&mut session, &mut host, &["2", "2", "20", "8", "0,0", "Doors", "D1", "900"]);
    session.cancel(&mut host);
    let Some(Shape::Table { cells, title: true, rows, .. }) = host.shapes().pop() else { panic!("{:?}", host.shapes()) };
    assert_eq!(rows.len(), 3, "a title row and two data rows");
    assert_eq!(cells[0][0], "Doors");
    assert_eq!((cells[1][0].as_str(), cells[1][1].as_str()), ("D1", "900"));
    let mut session = start("TABLEDIT", &mut host, vec![]);
    typed(&mut session, &mut host, &["30,-20"]);
    typed(&mut session, &mut host, &["1000"]);
    let Some(Shape::Table { cells, .. }) = host.shapes().pop() else { panic!() };
    assert_eq!(cells[2][1], "1000", "the cell picked, in the last row and second column");
}

#[test]
fn boundary_outlines_the_room_clicked_in_and_its_column() {
    let walls = [
        ((0.0, 0.0), (20.0, 0.0)),
        ((20.0, 0.0), (20.0, 10.0)),
        ((20.0, 10.0), (0.0, 10.0)),
        ((0.0, 10.0), (0.0, 0.0)),
        ((10.0, 0.0), (10.0, 10.0)),
    ];
    let mut entities: Vec<Entity> = walls.iter().map(|&((x0, y0), (x1, y1))| line(pt(x0, y0), pt(x1, y1))).collect();
    entities.push(circle(pt(15.0, 5.0), 1.0));
    let mut host = FakeHost::with(entities);
    let mut session = start("BOUNDARY", &mut host, vec![]);
    typed(&mut session, &mut host, &["30,5", "12,2", "5,5", "U", ""]);
    assert!(!session.active());
    assert_eq!(host.reports, vec!["Valid boundary not found.", "BOUNDARY created 2 polylines"]);
    let made: Vec<Shape> = host.shapes().split_off(6);
    assert_eq!(made.len(), 2, "the room and the column, not the undone left room");
    let Shape::Polyline { vertices, closed: true } = &made[0] else { panic!("{:?}", made[0]) };
    let xs: Vec<f64> = vertices.iter().map(|v| v.x).collect();
    assert!(xs.iter().all(|x| (10.0..=20.0).contains(x)), "the right room only: {xs:?}");
    assert_eq!(vertices.len(), 4, "a rectangle's corners");
    let Shape::Polyline { vertices: column, closed: true } = &made[1] else { panic!() };
    assert_eq!(column.len(), 2, "the column is two half-circle arcs");
    assert!(column.iter().all(|v| (v.bulge.abs() - 1.0).abs() < 1e-6), "{column:?}");
    assert!(column.iter().all(|v| (v.point().dist(pt(15.0, 5.0)) - 1.0).abs() < 1e-2), "the column");
}

#[test]
fn overkill_deletes_copies_and_combines_overlaps_as_one_undo_step() {
    let a = line(pt(0.0, 0.0), pt(10.0, 0.0));
    let entities = vec![a.clone(), a.duplicate(), line(pt(5.0, 0.0), pt(15.0, 0.0)), circle(pt(0.0, 5.0), 1.0)];
    let mut host = FakeHost::with(entities.clone());
    let mut session = start("-OVERKILL", &mut host, entities);
    typed(&mut session, &mut host, &["T", "0.001", "E", "Y", ""]);
    assert!(!session.active());
    assert_eq!(host.shapes(), vec![line_shape(pt(0.0, 0.0), pt(15.0, 0.0)), Shape::Circle { center: pt(0.0, 5.0), radius: 1.0 }]);
    assert_eq!(host.reports.last().unwrap(), "1 duplicate(s) deleted\n1 overlapping object(s) or segment(s) deleted");
    assert!(host.reports.iter().any(|r| r.contains("Tolerance=0.001") && r.contains("Combine end to end=Yes")));
    host.retract();
    assert_eq!(host.entities.len(), 4, "undo brings all of it back");
}

#[test]
fn overkill_in_the_window_takes_its_options_from_the_dialog_and_typed_it_takes_the_remembered_ones() {
    let a = line(pt(0.0, 0.0), pt(10.0, 0.0));
    let entities = vec![a.clone(), a.duplicate(), line(pt(5.0, 0.0), pt(15.0, 0.0))];
    let defaults = Rc::new(RefCell::new(Defaults::default()));
    let mut host = FakeHost::with(entities.clone());
    let mut session = start_with("OVERKILL", &mut host, entities.clone(), defaults.clone());
    assert!(session.active());
    assert_eq!(session.prompt.as_ref().unwrap().kind, PromptKind::OverkillDialog);
    // The window saves what the dialog chose, then answers the prompt with Enter.
    defaults.borrow_mut().overkill.combine_partial = false;
    typed(&mut session, &mut host, &[""]);
    assert!(!session.active());
    assert_eq!(host.shapes().len(), 2, "the copy goes, and the partial overlap stays when the dialog says so");
    assert_eq!(host.reports.last().unwrap(), "1 duplicate(s) deleted\n0 overlapping object(s) or segment(s) deleted");

    // Cancel changes nothing.
    let mut host = FakeHost::with(entities.clone());
    let mut session = start("OVERKILL", &mut host, entities);
    session.cancel(&mut host);
    assert!(!session.active());
    assert_eq!(host.entities.len(), 3);
}

#[test]
fn minus_overkill_turns_polylines_tidy_with_the_o_option() {
    let square = crate::model::entities::polyline_from(&[pt(0.0, 0.0), pt(4.0, 0.0), pt(4.0, 0.0), pt(4.0, 4.0), pt(0.0, 4.0)], true);
    let mut host = FakeHost::with(vec![square.clone()]);
    let mut session = start("-OVERKILL", &mut host, vec![square]);
    typed(&mut session, &mut host, &["O", "Y", ""]);
    assert!(!session.active());
    let Shape::Polyline { vertices, closed: true } = &host.shapes()[0] else { panic!("{:?}", host.shapes()) };
    assert_eq!(vertices.len(), 4, "the repeated corner is gone");
    assert_eq!(host.reports.last().unwrap(), "0 duplicate(s) deleted\n1 overlapping object(s) or segment(s) deleted");
    assert!(host.reports.iter().any(|r| r.contains("Optimize polylines=Yes")));
}

#[test]
fn wipeout_from_clicked_corners_or_a_closed_polyline() {
    let mut host = FakeHost::default();
    let mut session = start("WIPEOUT", &mut host, vec![]);
    typed(&mut session, &mut host, &["0,0", "10,0", "@0,5", "C"]);
    assert!(!session.active());
    assert_eq!(host.shapes(), vec![Shape::Wipeout { corners: vec![pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 5.0)] }]);

    let frame = crate::model::entities::polyline_from(&[pt(0.0, 0.0), pt(4.0, 0.0), pt(4.0, 4.0), pt(0.0, 4.0)], true);
    let mut host = FakeHost::with(vec![frame.clone()]);
    let mut session = start("WIPEOUT", &mut host, vec![]);
    typed(&mut session, &mut host, &[""]);
    session.submit_pick(frame, pt(4.0, 2.0), false, &mut host);
    typed(&mut session, &mut host, &["Y"]);
    assert_eq!(host.shapes().len(), 1, "the polyline is erased");
    let Shape::Wipeout { corners } = &host.shapes()[0] else { panic!() };
    assert_eq!(corners.len(), 4);
}

#[test]
fn wipeout_frames_sets_the_drawings_frame_mode_and_ends() {
    let defaults = Rc::new(RefCell::new(Defaults { wipeout_frame: 1, ..Defaults::default() }));
    let mut host = FakeHost::default();
    let mut session = start_with("WIPEOUT", &mut host, vec![], defaults.clone());
    assert_eq!(session.prompt.as_ref().unwrap().message, "Specify first point or [Frames/Polyline] <Polyline>:");
    typed(&mut session, &mut host, &["F"]);
    assert_eq!(session.prompt.as_ref().unwrap().message, "Enter mode [ON/OFF/Display but not plot] <ON>:");
    typed(&mut session, &mut host, &["OFF"]);
    assert!(!session.active(), "Frames ends the command");
    assert_eq!(host.frame_mode, Some(0));
    assert!(host.shapes().is_empty(), "no wipeout is drawn");

    let mut host = FakeHost::default();
    let defaults = Rc::new(RefCell::new(Defaults { wipeout_frame: 2, ..Defaults::default() }));
    let mut session = start_with("WIPEOUT", &mut host, vec![], defaults.clone());
    typed(&mut session, &mut host, &["F"]);
    assert_eq!(session.prompt.as_ref().unwrap().message, "Enter mode [ON/OFF/Display but not plot] <Display but not plot>:");
    typed(&mut session, &mut host, &["ON"]);
    assert_eq!(host.frame_mode, Some(1));

    let mut host = FakeHost::default();
    let mut session = start_with("WIPEOUT", &mut host, vec![], defaults);
    typed(&mut session, &mut host, &["F", "D"]);
    assert_eq!(host.frame_mode, Some(2));
}

#[test]
fn ltscale_psltscale_and_celtscale_set_the_drawings_linetype_scales() {
    let defaults = Rc::new(RefCell::new(Defaults { ltscale: 1.0, psltscale: true, celtscale: 1.0, ..Defaults::default() }));
    let mut host = FakeHost::default();
    let mut session = start_with("LTSCALE", &mut host, vec![], defaults.clone());
    assert_eq!(session.prompt.as_ref().unwrap().message, "Enter new linetype scale factor <1.0000>:");
    typed(&mut session, &mut host, &["2.5"]);
    assert!(!session.active());
    assert_eq!(host.scales, vec![ScaleSetting::Global(2.5)]);

    let mut host = FakeHost::default();
    assert_eq!(super::registry::resolve("LTS"), Some("LTSCALE"), "LTS is the alias");
    let mut session = start_with("LTSCALE", &mut host, vec![], defaults.clone());
    assert_eq!(session.submit_text("0", &mut host), Some("Enter a number greater than zero."), "zero is refused");
    typed(&mut session, &mut host, &["0.5"]);
    assert_eq!(host.scales, vec![ScaleSetting::Global(0.5)], "LTS is the alias");

    let mut host = FakeHost::default();
    let mut session = start_with("PSLTSCALE", &mut host, vec![], defaults.clone());
    assert_eq!(session.prompt.as_ref().unwrap().message, "Enter new value for PSLTSCALE <1>:");
    typed(&mut session, &mut host, &["0"]);
    assert_eq!(host.scales, vec![ScaleSetting::Paper(false)]);

    let mut host = FakeHost::default();
    let mut session = start_with("CELTSCALE", &mut host, vec![], defaults.clone());
    assert_eq!(session.prompt.as_ref().unwrap().message, "Enter new value for CELTSCALE <1.0000>:");
    typed(&mut session, &mut host, &["4"]);
    assert_eq!(host.scales, vec![ScaleSetting::Current(4.0)]);

    // Enter keeps the current value, and nothing is set.
    let mut host = FakeHost::default();
    let defaults = Rc::new(RefCell::new(Defaults { psltscale: false, ..Defaults::default() }));
    let mut session = start_with("PSLTSCALE", &mut host, vec![], defaults);
    typed(&mut session, &mut host, &[""]);
    assert_eq!(host.scales, vec![ScaleSetting::Paper(false)], "Enter keeps 0 as it is");
}

#[test]
fn mline_draws_with_the_justification_and_scale_it_is_given_and_remembers_them() {
    use crate::model::mline::Justification;
    let defaults = Rc::new(RefCell::new(Defaults::default()));
    let mut host = FakeHost::default();
    let mut session = start_with("MLINE", &mut host, vec![], defaults.clone());
    typed(&mut session, &mut host, &["J", "Z", "S", "240", "ST", "?", "0,0", "5000,0", "@0,3000", "U", "@0,4000", ""]);
    assert!(!session.active());
    let Some(Shape::Mline { vertices, closed, justification, scale, style, .. }) = host.shapes().last().cloned() else { panic!() };
    assert_eq!(vertices, vec![pt(0.0, 0.0), pt(5000.0, 0.0), pt(5000.0, 4000.0)]);
    assert!(!closed);
    assert_eq!((justification, scale, style.name.as_str()), (Justification::Zero, 240.0, "Standard"));
    assert!(host.reports.iter().any(|r| r == "Loaded mline styles: Standard"));
    assert!(host.reports.iter().any(|r| r.contains("Justification = Zero, Scale = 240.00")));
    assert_eq!(defaults.borrow().mline_scale, 240.0);
}

#[test]
fn an_exploded_mline_is_its_lines() {
    let mut host = FakeHost::default();
    let mut session = start("MLINE", &mut host, vec![]);
    typed(&mut session, &mut host, &["0,0", "100,0", "100,100", "0,100", "C"]);
    let mline = host.entities[0].clone();
    let mut session = start("EXPLODE", &mut host, vec![mline]);
    assert!(!session.active() || session.submit_text("", &mut host).is_none());
    assert_eq!(host.entities.len(), 8, "four sides, two lines each");
    assert!(host.shapes().iter().all(|s| matches!(s, Shape::Line { .. })));
}

#[test]
fn mledit_joins_two_walls_at_a_corner_and_adds_a_vertex() {
    use crate::model::mline::{Justification, MlineStyle};
    let wall = |vertices: Vec<Point>| {
        Entity::new(Shape::Mline {
            vertices,
            closed: false,
            justification: Justification::Zero,
            scale: 1.0,
            style: MlineStyle::standard(),
            gaps: vec![],
        })
    };
    let (horizontal, vertical) = (wall(vec![pt(0.0, 0.0), pt(20.0, 0.0)]), wall(vec![pt(10.0, 3.0), pt(10.0, 15.0)]));
    let mut host = FakeHost::with(vec![horizontal.clone(), vertical.clone()]);
    let mut session = start("MLEDIT", &mut host, vec![]);
    typed(&mut session, &mut host, &["CJ"]);
    session.submit_pick(horizontal, pt(2.0, 0.0), false, &mut host);
    session.submit_pick(vertical, pt(10.0, 12.0), false, &mut host);
    assert!(!session.active());
    let paths: Vec<Vec<Point>> = host
        .shapes()
        .into_iter()
        .map(|s| match s {
            Shape::Mline { vertices, .. } => vertices,
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(paths, vec![vec![pt(0.0, 0.0), pt(10.0, 0.0)], vec![pt(10.0, 0.0), pt(10.0, 15.0)]]);

    let target = host.entities[0].clone();
    let mut session = start("MLEDIT", &mut host, vec![]);
    typed(&mut session, &mut host, &["AV"]);
    session.submit_pick(target, pt(5.0, 0.2), false, &mut host);
    typed(&mut session, &mut host, &[""]);
    let Shape::Mline { vertices, .. } = &host.entities[0].shape else { panic!() };
    assert_eq!(vertices, &vec![pt(0.0, 0.0), pt(5.0, 0.0), pt(10.0, 0.0)]);
}

/// A wall two units thick along a path, its lines one unit each side.
fn wall(vertices: Vec<Point>) -> Entity {
    use crate::model::mline::{Justification, MlineStyle};
    Entity::new(Shape::Mline {
        vertices,
        closed: false,
        justification: Justification::Zero,
        scale: 2.0,
        style: MlineStyle::standard(),
        gaps: vec![],
    })
}

fn gaps_of(entity: &Entity) -> Vec<(usize, usize, f64, f64)> {
    let Shape::Mline { gaps, .. } = &entity.shape else { panic!() };
    let mut out: Vec<(usize, usize, f64, f64)> =
        gaps.iter().map(|g| (g.segment, g.element, (g.from * 1e6).round() / 1e6, (g.to * 1e6).round() / 1e6)).collect();
    out.sort_by(|a, b| a.partial_cmp(b).unwrap());
    out
}

fn mledit(host: &mut FakeHost, tool: &str, picks: &[(Entity, Point)], typed_after: &[&str]) {
    let mut session = start("MLEDIT", host, vec![]);
    typed(&mut session, host, &[tool]);
    for (entity, point) in picks {
        session.submit_pick(entity.clone(), *point, false, host);
    }
    typed(&mut session, host, typed_after);
    assert!(!session.active());
}

#[test]
fn mledit_crosses_cut_the_first_wall_through_the_second() {
    let (a, b) = (wall(vec![pt(0.0, 0.0), pt(20.0, 0.0)]), wall(vec![pt(10.0, -5.0), pt(10.0, 5.0)]));
    let mut host = FakeHost::with(vec![a.clone(), b.clone()]);
    mledit(&mut host, "CC", &[(a.clone(), pt(3.0, 0.0)), (b.clone(), pt(10.0, 3.0))], &[]);
    assert_eq!(gaps_of(&host.entities[0]), vec![(0, 0, 9.0, 11.0), (0, 1, 9.0, 11.0)]);
    assert!(gaps_of(&host.entities[1]).is_empty(), "a closed cross leaves the second whole");

    let mut host = FakeHost::with(vec![a.clone(), b.clone()]);
    mledit(&mut host, "OC", &[(a, pt(3.0, 0.0)), (b, pt(10.0, 3.0))], &[]);
    assert_eq!(gaps_of(&host.entities[1]), vec![(0, 0, 4.0, 6.0), (0, 1, 4.0, 6.0)], "an open cross opens the second too");
}

#[test]
fn mledit_tees_end_the_first_wall_at_the_second() {
    let (a, b) = (wall(vec![pt(10.0, 10.0), pt(10.0, 3.0)]), wall(vec![pt(0.0, 0.0), pt(20.0, 0.0)]));
    let mut host = FakeHost::with(vec![a.clone(), b.clone()]);
    mledit(&mut host, "OT", &[(a, pt(10.0, 8.0)), (b, pt(5.0, 0.0))], &[]);
    let Shape::Mline { vertices, .. } = &host.entities[0].shape else { panic!() };
    assert_eq!(vertices, &vec![pt(10.0, 10.0), pt(10.0, 1.0)], "the first ends at the second's near line");
    assert_eq!(gaps_of(&host.entities[1]), vec![(0, 0, 9.0, 11.0)], "the near line opens where the first meets it");
}

#[test]
fn mledit_cuts_one_line_and_welds_part_of_it_back() {
    let a = wall(vec![pt(0.0, 0.0), pt(20.0, 0.0)]);
    let mut host = FakeHost::with(vec![a.clone()]);
    mledit(&mut host, "CS", &[(a, pt(5.0, 1.0))], &["8,1"]);
    assert_eq!(gaps_of(&host.entities[0]), vec![(0, 0, 5.0, 8.0)]);
    let cut = host.entities[0].clone();
    mledit(&mut host, "WA", &[(cut, pt(6.0, 0.0))], &["7,0"]);
    assert_eq!(gaps_of(&host.entities[0]), vec![(0, 0, 5.0, 6.0), (0, 0, 7.0, 8.0)]);
}

#[test]
fn laymch_moves_objects_to_a_picked_or_named_layer_in_one_step() {
    use crate::model::document::Layer;
    let mut door = line(pt(0.0, 5.0), pt(1.0, 5.0));
    door.layer = "Doors".into();
    let (a, b) = (line(pt(0.0, 0.0), pt(1.0, 0.0)), circle(pt(5.0, 5.0), 1.0));
    let defaults = Rc::new(RefCell::new(Defaults::default()));
    defaults.borrow_mut().layers = vec![Layer::new("0"), Layer::new("Walls"), Layer::new("Doors")];
    let mut host = FakeHost::with(vec![a.clone(), b.clone(), door.clone()]);
    let mut session = start_with("LAYMCH", &mut host, vec![], defaults.clone());
    assert_eq!(session.prompt.as_ref().unwrap().message, "Select objects to be changed:");
    session.submit_objects(vec![a.clone(), b], &mut host);
    session.submit_pick(door, pt(0.5, 5.0), false, &mut host);
    assert!(!session.active());
    let layers: Vec<&str> = host.entities.iter().map(|e| e.layer.as_str()).collect();
    assert_eq!(layers, vec!["Doors", "Doors", "Doors"]);
    assert_eq!(host.reports, vec!["2 objects changed to layer \"Doors\"."]);
    host.retract();
    assert!(host.entities[..2].iter().all(|e| e.layer == "0"), "undone as one");

    let mut session = start_with("LAYMCH", &mut host, vec![a], defaults);
    typed(&mut session, &mut host, &["N", "walls"]);
    assert!(!session.active());
    assert_eq!(host.entities[0].layer, "Walls");
    assert_eq!(host.reports[1], "1 object changed to layer \"Walls\".");
}

#[test]
fn copyclip_and_cutclip_fill_the_clipboard_for_pasteclip() {
    let defaults = Rc::new(RefCell::new(Defaults::default()));
    let round = circle(pt(5.0, 5.0), 1.0);
    let mut host = FakeHost::with(vec![round.clone()]);
    let mut session = start_with("COPYCLIP", &mut host, vec![], defaults.clone());
    session.submit_objects(vec![round.clone()], &mut host);
    assert!(!session.active());
    assert_eq!((defaults.borrow().clipboard.len(), defaults.borrow().clipboard_base), (1, pt(4.0, 4.0)));
    assert_eq!(host.entities.len(), 1, "copying leaves the objects");

    let session = start_with("CUTCLIP", &mut host, vec![round], defaults.clone());
    assert!(!session.active(), "the selection is used without asking");
    assert!(host.entities.is_empty(), "cutting erases them");
    host.retract();
    assert_eq!(host.entities.len(), 1, "undone");
    let mut session = start_with("PASTECLIP", &mut host, vec![], defaults);
    typed(&mut session, &mut host, &["100,100"]);
    assert_eq!(host.shapes()[1], Shape::Circle { center: pt(101.0, 101.0), radius: 1.0 });
}

#[test]
fn measuregeom_measures_until_exit() {
    let arc = Entity::new(Shape::Arc { center: pt(0.0, 0.0), radius: 2.0, start_angle: 0.0, end_angle: 90.0 });
    let (horizontal, diagonal) = (line(pt(0.0, 0.0), pt(10.0, 0.0)), line(pt(0.0, 0.0), pt(10.0, 10.0)));
    let mut host = FakeHost::with(vec![arc.clone(), horizontal.clone(), diagonal.clone()]);
    let mut session = start("MEASUREGEOM", &mut host, vec![]);
    typed(&mut session, &mut host, &["", "0,0", "3,4"]);
    assert_eq!(host.reports[0], "Distance = 5.0000, Angle in XY Plane = 53, Delta X = 3.0000, Delta Y = 4.0000");
    assert_eq!(session.prompt.as_ref().unwrap().message, "Enter an option [Distance/Radius/Angle/ARea/eXit] <eXit>:");
    typed(&mut session, &mut host, &["R"]);
    session.submit_pick(arc.clone(), pt(2.0, 0.0), false, &mut host);
    assert_eq!(host.reports[1..], ["Radius = 2.0000", "Diameter = 4.0000"]);
    typed(&mut session, &mut host, &["A"]);
    session.submit_pick(arc, pt(2.0, 0.0), false, &mut host);
    assert_eq!(host.reports[3], "Angle = 90°", "an arc's included angle");
    typed(&mut session, &mut host, &["A"]);
    session.submit_pick(horizontal, pt(5.0, 0.0), false, &mut host);
    session.submit_pick(diagonal, pt(5.0, 5.0), false, &mut host);
    assert_eq!(host.reports[4], "Angle = 45°", "between two lines");
    typed(&mut session, &mut host, &["A", "", "0,0", "1,0", "0,1"]);
    assert_eq!(host.reports[5], "Angle = 90°", "at a vertex");
    typed(&mut session, &mut host, &["AR", "0,0", "4,0", "4,3", ""]);
    assert_eq!(host.reports[6], "Area = 6.0000, Perimeter = 12.0000");
    typed(&mut session, &mut host, &[""]);
    assert!(!session.active(), "Enter is eXit");
}

#[test]
fn cal_evaluates_an_expression() {
    let mut host = FakeHost::default();
    let cases = [
        ("2*(3+4)^2", "98"),
        ("sqrt(2)/2", "0.707107"),
        ("-sin(30)", "-0.5"),
        ("1/0", "Division by zero."),
        ("2+*3", "Invalid expression."),
    ];
    for (expression, result) in cases {
        let mut session = start("CAL", &mut host, vec![]);
        assert_eq!(session.prompt.as_ref().unwrap().message, ">> Expression:");
        typed(&mut session, &mut host, &[expression]);
        assert!(!session.active());
        assert_eq!(host.reports.last().unwrap(), result, "{expression}");
    }
}

/// A block DOOR of a square with corners 0,0 and 1,1 on layer 0, and a NUMBER attribute, inserted at 10,10 at
/// twice its size on layer WALLS with the value D1.
fn door() -> (crate::model::entities::BlockRef, Entity) {
    use crate::model::entities::{Attribute, BlockDef, BlockRef, text};
    let square = polyline_from(&[pt(0.0, 0.0), pt(1.0, 0.0), pt(1.0, 1.0), pt(0.0, 1.0)], true);
    let number = Attribute {
        tag: "NUMBER".into(),
        prompt: "Door number".into(),
        layer: "0".into(),
        color: 256,
        text: text(pt(0.0, 2.0), "?", 0.5, 0.0).shape,
        invisible: false,
    };
    let block = BlockRef(std::sync::Arc::new(BlockDef {
        name: "DOOR".into(),
        base: pt(0.0, 0.0),
        entities: vec![square],
        attribute_defs: vec![number.clone()],
        xref: None,
    }));
    let placed = crate::model::attributes::synced(
        &Entity::new(Shape::Insert {
            block: block.clone(),
            position: pt(10.0, 10.0),
            scale_x: 2.0,
            scale_y: 2.0,
            rotation: 0.0,
            attributes: vec![],
        }),
        &block,
    );
    let mut insert = placed;
    if let Shape::Insert { attributes, .. } = &mut insert.shape {
        attributes[0].text = crate::model::attributes::with_value(&attributes[0].text, "D1");
    }
    insert.layer = "WALLS".into();
    (block, insert)
}

#[test]
fn copybase_copies_by_the_point_chosen_and_pasteorig_puts_back_in_place() {
    let defaults = Rc::new(RefCell::new(Defaults::default()));
    let round = circle(pt(5.0, 5.0), 1.0);
    let mut host = FakeHost::with(vec![round.clone()]);
    let mut session = start_with("COPYBASE", &mut host, vec![round], defaults.clone());
    typed(&mut session, &mut host, &["5,5"]);
    assert!(!session.active(), "the selection is used once the base point is given");
    assert_eq!(defaults.borrow().clipboard_base, pt(5.0, 5.0));
    let mut session = start_with("PASTECLIP", &mut host, vec![], defaults.clone());
    typed(&mut session, &mut host, &["0,0"]);
    assert_eq!(host.shapes()[1], Shape::Circle { center: pt(0.0, 0.0), radius: 1.0 }, "pasted by its base point");
    start_with("PASTEORIG", &mut host, vec![], defaults);
    assert_eq!(host.shapes()[2], Shape::Circle { center: pt(5.0, 5.0), radius: 1.0 });
    assert_ne!(host.entities[2].id, host.entities[0].id, "a copy, not the original");
}

#[test]
fn pasteblock_pastes_the_clipboard_as_one_new_block() {
    let defaults = Rc::new(RefCell::new(Defaults::default()));
    let objects = vec![circle(pt(5.0, 5.0), 1.0), line(pt(4.0, 4.0), pt(6.0, 6.0))];
    let mut host = FakeHost::with(objects.clone());
    start_with("COPYCLIP", &mut host, objects, defaults.clone());
    let mut session = start_with("PASTEBLOCK", &mut host, vec![], defaults.clone());
    typed(&mut session, &mut host, &["20,0"]);
    let Shape::Insert { block, position, .. } = &host.entities[2].shape else { panic!("an insert") };
    assert!(block.0.name.starts_with("A$C") && block.0.name.len() == 11, "{}", block.0.name);
    assert_eq!((block.0.entities.len(), block.0.base, *position), (2, pt(4.0, 4.0), pt(20.0, 0.0)));
    assert!(defaults.borrow().blocks.iter().any(|b| b.0.name == block.0.name), "the drawing knows the block");
}

#[test]
fn ncopy_copies_an_object_out_of_a_block_as_it_shows() {
    let (_, insert) = door();
    let mut host = FakeHost::with(vec![insert.clone()]);
    let mut session = start("NCOPY", &mut host, vec![]);
    session.submit_pick(insert, pt(11.0, 10.0), false, &mut host);
    assert_eq!(host.reports[0], "1 found (Polyline).");
    typed(&mut session, &mut host, &["", "0,0", "100,0"]);
    assert!(!session.active());
    let copy = &host.entities[1];
    let Shape::Polyline { vertices, closed: true } = &copy.shape else { panic!("the square, {:?}", copy.shape) };
    assert_eq!(vertices[0].point(), pt(110.0, 10.0), "placed as the insert shows it, then moved");
    assert_eq!(vertices[2].point(), pt(112.0, 12.0), "at the insert's scale");
    assert_eq!(copy.layer, "WALLS", "on layer 0 in the block, it shows on the insert's layer");
    assert_eq!(host.entities.len(), 2, "the block is left as it is");
}

#[test]
fn burst_keeps_attribute_values_as_text() {
    let (_, insert) = door();
    let mut host = FakeHost::with(vec![insert.clone()]);
    start("BURST", &mut host, vec![insert]);
    let kinds: Vec<&str> = host.entities.iter().map(|e| e.shape.kind()).collect();
    assert_eq!(kinds, ["Polyline", "Text"]);
    let Shape::Text { text, .. } = &host.entities[1].shape else { panic!("text") };
    assert_eq!(text, "D1", "the value, not the tag");
    assert_eq!(host.entities[0].layer, "WALLS", "what is on layer 0 in the block shows on the insert's layer");
    assert_eq!(host.entities[1].layer, "0", "an attribute keeps its own layer");
    host.retract();
    assert_eq!(host.entities.len(), 1, "undone in one step");
}

#[test]
fn attsync_syncs_a_picked_block_by_name_or_selection() {
    let (block, insert) = door();
    let defaults = Rc::new(RefCell::new(Defaults::default()));
    defaults.borrow_mut().blocks = vec![block];
    let mut host = FakeHost::with(vec![insert.clone()]);
    let mut session = start_with("ATTSYNC", &mut host, vec![], defaults.clone());
    typed(&mut session, &mut host, &["?"]);
    assert_eq!(host.reports[0], "Blocks with attributes: DOOR");
    typed(&mut session, &mut host, &["N", "nope"]);
    assert_eq!(host.reports[1], "There is no block called nope.");
    typed(&mut session, &mut host, &["S"]);
    session.submit_pick(insert, pt(10.0, 10.0), false, &mut host);
    typed(&mut session, &mut host, &[""]);
    assert!(!session.active());
    assert_eq!(host.synced.iter().map(|b| b.0.name.as_str()).collect::<Vec<_>>(), ["DOOR"]);
}

#[test]
fn eattedit_asks_for_a_block_with_attributes() {
    let (_, insert) = door();
    let plain = circle(pt(0.0, 0.0), 1.0);
    let defaults = Rc::new(RefCell::new(Defaults::default()));
    let mut host = FakeHost::with(vec![insert.clone(), plain.clone()]);
    let mut session = start_with("EATTEDIT", &mut host, vec![], defaults.clone());
    session.submit_pick(plain, pt(1.0, 0.0), false, &mut host);
    assert_eq!(host.reports[0], "That is not a block reference.");
    session.submit_pick(insert.clone(), pt(10.0, 10.0), false, &mut host);
    assert!(!session.active());
    assert_eq!(defaults.borrow().attribute_edit_request, Some(insert.id));
}

fn square(x: f64, y: f64, side: f64) -> Entity {
    polyline_from(&[pt(x, y), pt(x + side, y), pt(x + side, y + side), pt(x, y + side)], true)
}

fn area_of(entity: &Entity) -> f64 {
    let Shape::Region { loops } = &entity.shape else { panic!("a region, not {:?}", entity.shape) };
    crate::model::region::measure(loops).area
}

#[test]
fn region_makes_regions_of_closed_objects_and_of_lines_closing_a_loop() {
    let triangle = vec![line(pt(20.0, 0.0), pt(30.0, 0.0)), line(pt(30.0, 0.0), pt(20.0, 10.0)), line(pt(20.0, 10.0), pt(20.0, 0.0))];
    let (box_, round, stray) = (square(0.0, 0.0, 10.0), circle(pt(50.0, 0.0), 1.0), line(pt(0.0, 50.0), pt(5.0, 50.0)));
    let mut all = vec![box_, round, stray.clone()];
    all.extend(triangle);
    let mut host = FakeHost::with(all.clone());
    start("REGION", &mut host, all);
    assert_eq!(host.reports[0], "3 loop(s) extracted. 3 Region(s) created.");
    let regions: Vec<f64> = host.entities.iter().filter(|e| matches!(e.shape, Shape::Region { .. })).map(area_of).collect();
    assert_eq!(regions.len(), 3);
    assert!((regions[0] - 100.0).abs() < 1e-9 && (regions[1] - std::f64::consts::PI).abs() < 1e-9 && (regions[2] - 50.0).abs() < 1e-6);
    assert_eq!(host.entities.len(), 4, "the line that closes nothing stays");
    assert!(host.entities.iter().any(|e| e.id == stray.id));
    host.retract();
    assert_eq!(host.entities.len(), 6, "undone in one step");
}

#[test]
fn union_subtract_and_intersect_combine_regions() {
    let regions = |host: &mut FakeHost| {
        let squares = vec![square(0.0, 0.0, 10.0), square(5.0, 5.0, 10.0)];
        *host = FakeHost::with(squares.clone());
        start("REGION", host, squares);
        host.entities.clone()
    };
    let mut host = FakeHost::default();
    let made = regions(&mut host);
    start("UNION", &mut host, made);
    assert_eq!(host.entities.len(), 1);
    assert!((area_of(&host.entities[0]) - 175.0).abs() < 1e-6);

    let made = regions(&mut host);
    let mut session = start("SUBTRACT", &mut host, vec![made[0].clone()]);
    assert_eq!(session.prompt.as_ref().unwrap().message, "Select solids, surfaces, and regions to subtract ..");
    session.submit_objects(vec![made[1].clone()], &mut host);
    assert_eq!(host.entities.len(), 1);
    assert!((area_of(&host.entities[0]) - 75.0).abs() < 1e-6);

    let made = regions(&mut host);
    start("INTERSECT", &mut host, made);
    assert!((area_of(&host.entities[0]) - 25.0).abs() < 1e-6);

    let mut host = FakeHost::with(vec![square(0.0, 0.0, 1.0), square(5.0, 5.0, 1.0)]);
    let squares = host.entities.clone();
    start("REGION", &mut host, squares);
    let made = host.entities.clone();
    start("INTERSECT", &mut host, made);
    assert!(host.entities.is_empty());
    assert_eq!(host.reports.last().map(String::as_str), Some("Null region created - deleted."));
}

#[test]
fn boolean_commands_leave_out_what_is_not_a_region() {
    let mut host = FakeHost::with(vec![square(0.0, 0.0, 1.0)]);
    let chosen = host.entities.clone();
    start("UNION", &mut host, chosen);
    assert_eq!(
        host.reports,
        ["1 object(s) are not regions and are left out. Make regions of them with REGION.", "At least 2 regions are required."]
    );
    assert!(matches!(host.entities[0].shape, Shape::Polyline { .. }), "left as it was");
}

/// A UCS at 10,0 turned a quarter turn: its X axis is the world's Y axis.
fn turned_ucs() -> crate::model::ucs::Ucs {
    crate::model::ucs::Ucs { origin: pt(10.0, 0.0), angle: 90.0 }
}

fn near(a: Point, b: Point) -> bool {
    a.dist(b) < 1e-9
}

#[test]
fn points_and_angles_are_typed_in_the_ucs() {
    let mut host = FakeHost { ucs: turned_ucs(), ..FakeHost::default() };
    let mut session = start("LINE", &mut host, vec![]);
    typed(&mut session, &mut host, &["0,0", "@5<0", "*0,0", ""]);
    let Shape::Line { start, end } = host.shapes()[0] else { panic!("a line") };
    assert!(near(start, pt(10.0, 0.0)) && near(end, pt(10.0, 5.0)), "{start:?} {end:?}");
    let Shape::Line { end, .. } = host.shapes()[1] else { panic!("a line") };
    assert!(near(end, pt(0.0, 0.0)), "a starred point is in world coordinates");

    // A direction typed is from the UCS's X axis; a turn is taken as it is.
    let defaults = Rc::new(RefCell::new(Defaults { ucs: turned_ucs(), ..Defaults::default() }));
    let mut host = FakeHost { ucs: turned_ucs(), ..FakeHost::default() };
    let mut session = start_with("TEXT", &mut host, vec![], defaults.clone());
    typed(&mut session, &mut host, &["0,0", "1", "", "Hi"]);
    let Shape::Text { rotation, .. } = host.shapes()[0] else { panic!("text") };
    assert!((rotation - 90.0).abs() < 1e-9, "the default rotation 0 runs along the UCS's X axis: {rotation}");
    let target = line(pt(0.0, 0.0), pt(1.0, 0.0));
    let mut host = FakeHost { entities: vec![target.clone()], ucs: turned_ucs(), ..FakeHost::default() };
    let mut session = start_with("ROTATE", &mut host, vec![target], defaults);
    typed(&mut session, &mut host, &["*0,0", "90"]);
    let Shape::Line { end, .. } = host.shapes()[0] else { panic!("a line") };
    assert!(near(end, pt(0.0, 1.0)), "rotated by a quarter turn, not by the UCS's turn too: {end:?}");
}

#[test]
fn rectangles_dimensions_and_construction_lines_follow_the_ucs() {
    let defaults = Rc::new(RefCell::new(Defaults { ucs: turned_ucs(), ..Defaults::default() }));
    let mut host = FakeHost { ucs: turned_ucs(), ..FakeHost::default() };
    let mut session = start_with("RECTANGLE", &mut host, vec![], defaults.clone());
    typed(&mut session, &mut host, &["0,0", "4,2"]);
    let Shape::Polyline { vertices, .. } = &host.shapes()[0] else { panic!("a rectangle") };
    let corners: Vec<Point> = vertices.iter().map(|v| v.point()).collect();
    for (corner, expected) in corners.iter().zip([pt(10.0, 0.0), pt(10.0, 4.0), pt(8.0, 4.0), pt(8.0, 0.0)]) {
        assert!(near(*corner, expected), "{corners:?}");
    }
    let mut session = start_with("ID", &mut host, vec![], defaults.clone());
    typed(&mut session, &mut host, &["*10,3"]);
    assert_eq!(host.reports.last().map(String::as_str), Some("X = 3.0000  Y = 0.0000"));
    let mut session = start_with("DIMLINEAR", &mut host, vec![], defaults.clone());
    typed(&mut session, &mut host, &["0,0", "3,1", "1.5,5"]);
    let Shape::Dimension { angle, .. } = host.shapes()[1] else { panic!("a dimension") };
    assert!((angle - 90.0).abs() < 1e-9, "horizontal in the UCS is the world's vertical: {angle}");
    let mut session = start_with("XLINE", &mut host, vec![], defaults);
    typed(&mut session, &mut host, &["H", "0,0", ""]);
    let Shape::XLine { direction, .. } = host.shapes()[2] else { panic!("an xline") };
    assert!(near(direction, pt(0.0, 1.0)), "{direction:?}");
}

#[test]
fn ucs_sets_an_origin_and_axis_and_its_other_options() {
    use super::prompts::UcsRequest;
    use crate::model::ucs::Ucs;
    let defaults = Rc::new(RefCell::new(Defaults::default()));
    let mut host = FakeHost::default();
    let mut session = start_with("UCS", &mut host, vec![], defaults.clone());
    assert_eq!(host.reports[0], "Current ucs name:  World");
    typed(&mut session, &mut host, &["5,5", "5,10"]);
    assert_eq!(host.ucs_requests, [UcsRequest::Set(Ucs { origin: pt(5.0, 5.0), angle: 90.0 })]);
    defaults.borrow_mut().ucs = Ucs { origin: pt(5.0, 5.0), angle: 90.0 };
    for (answers, expected) in [
        (vec!["Z", ""], UcsRequest::Set(Ucs { origin: pt(5.0, 5.0), angle: 180.0 })),
        (vec![""], UcsRequest::Set(Ucs::WORLD)),
        (vec!["P"], UcsRequest::Previous),
        (vec!["NA", "S", "Plan"], UcsRequest::Save("Plan".into())),
    ] {
        host.ucs_requests.clear();
        let mut session = start_with("UCS", &mut host, vec![], defaults.clone());
        typed(&mut session, &mut host, &answers);
        assert_eq!(host.ucs_requests, [expected], "{answers:?}");
    }
    let target = line(pt(1.0, 1.0), pt(1.0, 9.0));
    host.ucs_requests.clear();
    let mut session = start_with("UCS", &mut host, vec![], defaults.clone());
    typed(&mut session, &mut host, &["OB"]);
    session.submit_pick(target, pt(1.0, 8.0), false, &mut host);
    assert_eq!(host.ucs_requests, [UcsRequest::Set(Ucs { origin: pt(1.0, 9.0), angle: 270.0 })], "from the end nearer the pick");
    let mut session = start_with("UCS", &mut host, vec![], defaults);
    typed(&mut session, &mut host, &["X"]);
    assert!(host.reports.last().unwrap().contains("XY plane"));
    let mut session = start("UCSICON", &mut host, vec![]);
    typed(&mut session, &mut host, &["OFF"]);
    assert_eq!(host.ucs_requests.last(), Some(&UcsRequest::Icon(Some(false), None)));
}
