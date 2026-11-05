//! Commands run start to finish against a stand-in for the drawing.

use std::cell::RefCell;
use std::rc::Rc;

use super::defaults::Defaults;
use super::prompts::{Host, PromptKind, Session};
use super::registry::create;
use crate::model::entities::{Entity, Shape, circle, line};
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
    fn extent(&self) -> Option<Bounds> {
        self.entities.iter().map(entity_extent).reduce(Bounds::union)
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
    assert_eq!(session.prompt.as_ref().unwrap().message, "Select first line (radius 2) or [Radius]:");
    session.submit_pick(horizontal, pt(5.0, 0.0), false, &mut host);
    session.submit_pick(vertical, pt(0.0, 5.0), false, &mut host);
    assert!(!session.active());
    assert_eq!(host.entities.len(), 3);
    assert_eq!(defaults.borrow().fillet_radius, 2.0);
    host.retract();
    assert_eq!(host.entities.len(), 2, "the fillet undoes as one step");
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
    assert!(column.iter().all(|v| (v.point().dist(pt(15.0, 5.0)) - 1.0).abs() < 1e-2), "the column");
}

#[test]
fn overkill_deletes_copies_and_combines_overlaps_as_one_undo_step() {
    let a = line(pt(0.0, 0.0), pt(10.0, 0.0));
    let entities = vec![a.clone(), a.duplicate(), line(pt(5.0, 0.0), pt(15.0, 0.0)), circle(pt(0.0, 5.0), 1.0)];
    let mut host = FakeHost::with(entities.clone());
    let mut session = start("OVERKILL", &mut host, entities);
    typed(&mut session, &mut host, &["T", "0.001", "E", "Y", ""]);
    assert!(!session.active());
    assert_eq!(host.shapes(), vec![line_shape(pt(0.0, 0.0), pt(15.0, 0.0)), Shape::Circle { center: pt(0.0, 5.0), radius: 1.0 }]);
    assert_eq!(host.reports.last().unwrap(), "1 duplicate(s) deleted\n1 overlapping object(s) or segment(s) deleted");
    assert!(host.reports.iter().any(|r| r.contains("Tolerance=0.001") && r.contains("Combine end to end=Yes")));
    host.retract();
    assert_eq!(host.entities.len(), 4, "undo brings all of it back");
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
fn mline_draws_with_the_justification_and_scale_it_is_given_and_remembers_them() {
    use crate::model::mline::Justification;
    let defaults = Rc::new(RefCell::new(Defaults::default()));
    let mut host = FakeHost::default();
    let mut session = start_with("MLINE", &mut host, vec![], defaults.clone());
    typed(&mut session, &mut host, &["J", "Z", "S", "240", "ST", "?", "0,0", "5000,0", "@0,3000", "U", "@0,4000", ""]);
    assert!(!session.active());
    let Some(Shape::Mline { vertices, closed, justification, scale, style }) = host.shapes().last().cloned() else { panic!() };
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
        Entity::new(Shape::Mline { vertices, closed: false, justification: Justification::Zero, scale: 1.0, style: MlineStyle::standard() })
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
