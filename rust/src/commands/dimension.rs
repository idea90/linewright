use crate::model::ucs::Ucs;
use std::cell::RefCell;
use std::rc::Rc;

use super::defaults::Defaults;
use super::draw::line_preview;
use super::edit::objects_to_edit;
use super::format_general;
use super::prompts::{Answer, Io, Prompt, PromptKind, Step};
use crate::model::annotation::{AnnotationKind, angle_between_lines};
use crate::model::dimension::{DimensionLook, DimensionStyle};
use crate::model::entities::{Entity, HAlign, Shape, TextAlign, VAlign};
use crate::model::geometry::Point;

const SAME_POINT: &str = "The points are the same.";

/// Horizontal when the cursor sits above or below the two points more than beside them, otherwise vertical: along
/// the UCS's X axis or its Y axis.
fn linear_angle(start: Point, end: Point, cursor: Point, ucs: &Ucs) -> f64 {
    let (start, end, cursor) = (ucs.to_ucs(start), ucs.to_ucs(end), ucs.to_ucs(cursor));
    let beside = (start.x.min(end.x) - cursor.x).max(cursor.x - start.x.max(end.x)).max(0.0);
    let above = (start.y.min(end.y) - cursor.y).max(cursor.y - start.y.max(end.y)).max(0.0);
    ucs.angle + if above >= beside { 0.0 } else { 90.0 }
}

fn aligned_angle(start: Point, end: Point, _cursor: Point, _ucs: &Ucs) -> f64 {
    start.angle_to(end)
}

async fn second_origin(io: &Io, start: Point) -> Option<Point> {
    let mut message = "Specify second extension line origin:".to_string();
    loop {
        let Some(Answer::Point(end)) = io.ask(Prompt::point(message.clone()).base(start).preview(line_preview(start))).await else {
            return None;
        };
        if end != start {
            return Some(end);
        }
        message = format!("{SAME_POINT} Specify second extension line origin:");
    }
}

async fn dimension_command(io: Io, choose_angle: fn(Point, Point, Point, &Ucs) -> f64, defaults: Rc<RefCell<Defaults>>) {
    let (look, ucs) = (defaults.borrow().dimension_look.clone(), defaults.borrow().ucs);
    let Some(Answer::Point(start)) = io.ask(Prompt::point("Specify first extension line origin:")).await else { return };
    let Some(end) = second_origin(&io, start).await else { return };
    let dimension_at = move |cursor: Point| -> Entity {
        let angle = choose_angle(start, end, cursor, &ucs);
        Entity::new(Shape::Dimension { start, end, line_point: cursor, angle, look: look.clone() })
    };
    let dimension_at = Rc::new(dimension_at);
    let preview = dimension_at.clone();
    let prompt = Prompt::point("Specify dimension line location:").base(end).preview(move |cursor| vec![preview(cursor)]);
    if let Some(Answer::Point(location)) = io.ask(prompt).await {
        let drawn = dimension_at(location);
        defaults.borrow_mut().last_dimension = Some(drawn.clone());
        io.step(Step::Drawn(vec![drawn]));
    }
}

pub async fn linear(io: Io, defaults: Rc<RefCell<Defaults>>) {
    dimension_command(io, linear_angle, defaults).await;
}

pub async fn aligned(io: Io, defaults: Rc<RefCell<Defaults>>) {
    dimension_command(io, aligned_angle, defaults).await;
}

const SELECT: &str = "S";

/// Baseline dimensions step out from the one before by this many text heights, like DIMDLI (3.75 for 2.5 text).
const BASELINE_SPACING: f64 = 1.5;

/// DIMCONTINUE and DIMBASELINE: more linear dimensions from the last one drawn (or one picked), each measuring
/// to a new point. Continued ones chain end to end on the same line; baseline ones all start where the first did,
/// each on a line further out.
async fn dimension_chain(io: Io, defaults: Rc<RefCell<Defaults>>, baseline: bool) {
    let mut base = defaults.borrow().last_dimension.clone();
    loop {
        let Some(Entity { shape: Shape::Dimension { start, end, line_point, angle, look }, .. }) = base.clone() else {
            let message = if baseline { "Select base dimension:" } else { "Select continued dimension:" };
            let Some(Answer::Picked(picked)) = io.ask(Prompt::new(PromptKind::Pick, message)).await else { return };
            match picked.entity.shape {
                Shape::Dimension { .. } => base = Some(picked.entity),
                _ => io.step(Step::Report("Pick a linear or aligned dimension.".into())),
            }
            continue;
        };
        let along = crate::model::geometry::direction(angle);
        let across = crate::model::geometry::pt(-along.y, along.x);
        let on_line = crate::model::dimension::dimension_parts(start, end, line_point, angle, &look).dimension_line.1;
        let side = if (on_line - start).dot(across) >= 0.0 { 1.0 } else { -1.0 };
        let spacing = look.text_height * look.scale * BASELINE_SPACING;
        let next = move |to: Point| -> Entity {
            let (from, line) = if baseline { (start, on_line + across * (side * spacing)) } else { (end, on_line) };
            Entity::new(Shape::Dimension {
                start: from,
                end: to,
                line_point: line,
                angle,
                look: DimensionLook { text_position: None, ..look.clone() },
            })
        };
        let from = if baseline { start } else { end };
        let preview = next.clone();
        let prompt = Prompt::point("Specify a second extension line origin or [Select] <Select>:")
            .base(from)
            .options(&[SELECT])
            .preview(move |cursor| vec![preview(cursor)]);
        match io.ask(prompt).await {
            Some(Answer::Point(to)) if to != from => {
                let drawn = next(to);
                defaults.borrow_mut().last_dimension = Some(drawn.clone());
                io.step(Step::Drawn(vec![drawn.clone()]));
                base = Some(drawn);
            }
            Some(answer) if answer.is(SELECT) => base = None,
            Some(_) => {}
            None => return,
        }
    }
}

pub async fn continue_dimension(io: Io, defaults: Rc<RefCell<Defaults>>) {
    dimension_chain(io, defaults, false).await;
}

pub async fn baseline_dimension(io: Io, defaults: Rc<RefCell<Defaults>>) {
    dimension_chain(io, defaults, true).await;
}

/// A dimension variable new dimensions take, typed like AutoCAD's: "Enter new value for DIMTXT <2.5>:".
async fn set_variable(io: Io, defaults: Rc<RefCell<Defaults>>, name: &str, field: fn(&mut DimensionLook) -> &mut f64) {
    let current = *field(&mut defaults.borrow_mut().dimension_look);
    let prompt = Prompt::new(PromptKind::Number, format!("Enter new value for {name} <{}>:", format_general(current)))
        .default(Answer::Number(current));
    if let Some(Answer::Number(value)) = io.ask(prompt).await {
        *field(&mut defaults.borrow_mut().dimension_look) = value;
    }
}

pub async fn set_scale(io: Io, defaults: Rc<RefCell<Defaults>>) {
    set_variable(io, defaults, "DIMSCALE", |look| &mut look.scale).await;
}

pub async fn set_text_height(io: Io, defaults: Rc<RefCell<Defaults>>) {
    set_variable(io, defaults, "DIMTXT", |look| &mut look.text_height).await;
}

pub async fn set_arrow_size(io: Io, defaults: Rc<RefCell<Defaults>>) {
    set_variable(io, defaults, "DIMASZ", |look| &mut look.arrow_size).await;
}

const APPLY: &str = "A";
const STATUS: &str = "ST";
const SAVE: &str = "S";
const RESTORE: &str = "R";
const LIST: &str = "?";
const YES: &str = "Y";
const NO: &str = "N";

/// -DIMSTYLE, as AutoCAD's command line version of DIMSTYLE. Save keeps the current settings (what DIMSCALE,
/// DIMTXT and DIMASZ set) as a named style and makes it current; redefining a style carries the change over to the
/// dimensions that follow it. Restore makes a style current, by name or by picking a dimension. STatus lists the
/// current settings, Apply gives them to selected dimensions, ? lists the styles.
pub async fn style(io: Io, selected: Vec<Entity>, defaults: Rc<RefCell<Defaults>>) {
    let prompt = Prompt::point("Enter a dimension style option [Save/Restore/STatus/Apply/?] <Restore>:")
        .options(&[SAVE, RESTORE, STATUS, APPLY, LIST])
        .default(Answer::keyword(RESTORE));
    let current = defaults.borrow().dimension_look.clone();
    let styles = defaults.borrow().dimension_styles.clone();
    let names = || styles.iter().map(|s| s.name.as_str()).collect::<Vec<_>>().join(", ");
    match io.ask(prompt).await {
        Some(answer) if answer.is(STATUS) => io.step(Step::Report(format!(
            "Current style {}: DIMSCALE {}  DIMTXT {}  DIMASZ {}  DIMDEC {}",
            current.style,
            format_general(current.scale),
            format_general(current.text_height),
            format_general(current.arrow_size),
            current.decimals
        ))),
        Some(answer) if answer.is(LIST) => io.step(Step::Report(format!("Dimension styles: {}", names()))),
        Some(answer) if answer.is(SAVE) => {
            let Some(Answer::Text(name)) = io.ask(Prompt::new(PromptKind::Text, "Enter name for new dimension style:")).await else {
                return;
            };
            let name = name.trim().to_string();
            if name.is_empty() {
                return;
            }
            if styles.iter().any(|s| s.name.eq_ignore_ascii_case(&name)) {
                let prompt = Prompt::point("That name is already in use, redefine it? [Yes/No] <N>:")
                    .options(&[YES, NO])
                    .default(Answer::keyword(NO));
                if !io.ask(prompt).await.is_some_and(|a| a.is(YES)) {
                    return;
                }
            }
            let look = DimensionLook { style: name.clone(), text: String::new(), text_position: None, ..current };
            defaults.borrow_mut().dimension_look = look.clone();
            let annotative = defaults.borrow().dimension_styles.iter().any(|s| s.name.eq_ignore_ascii_case(&name) && s.annotative);
            io.step(Step::DimensionStyle { style: Some(DimensionStyle { name: name.clone(), look, annotative }), current: Some(name) });
        }
        Some(answer) if answer.is(RESTORE) => {
            let prompt = Prompt::new(PromptKind::Text, "Enter a dimension style name, [?] or <select dimension>:")
                .default(Answer::Text(String::new()));
            let style = match io.ask(prompt).await {
                Some(Answer::Text(name)) if name.trim() == LIST => {
                    io.step(Step::Report(format!("Dimension styles: {}", names())));
                    return;
                }
                Some(Answer::Text(name)) if !name.trim().is_empty() => {
                    let Some(style) = styles.iter().find(|s| s.name.eq_ignore_ascii_case(name.trim())) else {
                        io.step(Step::Report(format!("Dimension style {} not found.", name.trim())));
                        return;
                    };
                    style.clone()
                }
                Some(_) => {
                    let Some(Answer::Picked(picked)) = io.ask(Prompt::new(PromptKind::Pick, "Select dimension:")).await else { return };
                    let (Shape::Dimension { look, .. } | Shape::Annotation { look, .. }) = &picked.entity.shape else {
                        io.step(Step::Report("That is not a dimension.".into()));
                        return;
                    };
                    match styles.iter().find(|s| s.name.eq_ignore_ascii_case(&look.style)) {
                        Some(style) => style.clone(),
                        None => return,
                    }
                }
                None => return,
            };
            defaults.borrow_mut().dimension_look = style.look.clone();
            io.step(Step::Report(format!("Current dimension style: {}", style.name)));
            io.step(Step::DimensionStyle { style: None, current: Some(style.name) });
        }
        Some(answer) if answer.is(APPLY) => {
            let changed: Vec<Entity> = objects_to_edit(&io, selected)
                .await
                .into_iter()
                .filter_map(|entity| {
                    let mut shape = entity.shape.clone();
                    let (Shape::Dimension { look, .. } | Shape::Annotation { look, .. }) = &mut shape else { return None };
                    *look = DimensionLook { text: look.text.clone(), ..current.clone() };
                    Some(entity.with_shape(shape))
                })
                .collect();
            if !changed.is_empty() {
                let count = changed.len();
                io.step(Step::Modify(changed));
                io.step(Step::Report(format!("{count} dimension(s) updated.")));
            }
        }
        _ => {}
    }
}

fn annotation(kind: AnnotationKind, points: Vec<Point>, look: &DimensionLook) -> Entity {
    Entity::new(Shape::Annotation { kind, points, look: look.clone() })
}

/// Ask for where the dimension goes, showing it at the cursor, and draw it there.
async fn place(io: &Io, message: &str, base: Point, at: impl Fn(Point) -> Option<Entity> + 'static) {
    let at = Rc::new(at);
    let preview = at.clone();
    let prompt = Prompt::point(message).base(base).preview(move |cursor| preview(cursor).into_iter().collect());
    if let Some(Answer::Point(location)) = io.ask(prompt).await
        && let Some(entity) = at(location)
    {
        io.step(Step::Drawn(vec![entity]));
    }
}

/// The center and radius of a picked arc or circle, asking again until one is picked.
async fn pick_curve(io: &Io, message: &str) -> Option<(Point, f64)> {
    loop {
        let Some(Answer::Picked(picked)) = io.ask(Prompt::new(PromptKind::Pick, message)).await else { return None };
        match picked.entity.shape {
            Shape::Circle { center, radius } | Shape::Arc { center, radius, .. } => return Some((center, radius)),
            _ => io.step(Step::Report("Object selected is not a circle or arc.".into())),
        }
    }
}

/// DIMRADIUS and DIMDIAMETER: pick an arc or circle, then place the text; the dimension points at the curve where
/// the text lies.
async fn radial(io: Io, defaults: Rc<RefCell<Defaults>>, kind: AnnotationKind) {
    let Some((center, radius)) = pick_curve(&io, "Select arc or circle:").await else { return };
    let look = defaults.borrow().dimension_look.clone();
    place(&io, "Specify dimension line location:", center, move |cursor| {
        let toward = cursor - center;
        let toward = if toward.length() > 0.0 { toward * (1.0 / toward.length()) } else { crate::model::geometry::pt(1.0, 0.0) };
        let look = DimensionLook { text_position: Some(cursor), ..look.clone() };
        Some(annotation(kind, vec![center, center + toward * radius], &look))
    })
    .await;
}

pub async fn radius(io: Io, defaults: Rc<RefCell<Defaults>>) {
    radial(io, defaults, AnnotationKind::Radius).await;
}

pub async fn diameter(io: Io, defaults: Rc<RefCell<Defaults>>) {
    radial(io, defaults, AnnotationKind::Diameter).await;
}

const VERTEX: &str = "VERTEX";

/// DIMANGULAR: pick an arc (its angle), a circle (from the picked point to a second one), two lines (the angle
/// between them), or press Enter to give the vertex and the two endpoints. The arc's place picks which angle.
pub async fn angular(io: Io, defaults: Rc<RefCell<Defaults>>) {
    let look = defaults.borrow().dimension_look.clone();
    let prompt = Prompt::new(PromptKind::Pick, "Select arc, circle, line, or <specify vertex>:").default(Answer::keyword(VERTEX));
    let ask_point = |message: &'static str, base: Option<Point>| {
        let io = io.clone();
        async move {
            let prompt = match base {
                Some(base) => Prompt::point(message).base(base).preview(line_preview(base)),
                None => Prompt::point(message),
            };
            match io.ask(prompt).await {
                Some(Answer::Point(p)) => Some(p),
                _ => None,
            }
        }
    };
    type Sides = Box<dyn Fn(Point) -> Option<Vec<Point>>>;
    let (vertex, sides): (Point, Sides) = match io.ask(prompt).await {
        Some(Answer::Picked(picked)) => match picked.entity.shape {
            Shape::Arc { center, radius, start_angle, end_angle } => {
                let at = |a: f64| center + crate::model::geometry::direction(a) * radius;
                let (first, second) = (at(start_angle), at(end_angle));
                (center, Box::new(move |cursor| Some(vec![center, first, second, cursor])))
            }
            Shape::Circle { center, radius } => {
                let toward = picked.point - center;
                let first = center + toward * (radius / toward.length().max(f64::MIN_POSITIVE));
                let Some(second) = ask_point("Specify second angle endpoint:", Some(center)).await else { return };
                (center, Box::new(move |cursor| Some(vec![center, first, second, cursor])))
            }
            Shape::Line { start, end } => {
                let second = loop {
                    let Some(Answer::Picked(other)) = io.ask(Prompt::new(PromptKind::Pick, "Select second line:")).await else { return };
                    match other.entity.shape {
                        Shape::Line { start, end } => break (start, end),
                        _ => io.step(Step::Report("Object selected is not a line.".into())),
                    }
                };
                let Some(sides) = angle_between_lines((start, end), second, end) else {
                    io.step(Step::Report("Lines are parallel.".into()));
                    return;
                };
                (sides[0], Box::new(move |cursor| angle_between_lines((start, end), second, cursor)))
            }
            _ => {
                io.step(Step::Report("Select an arc, circle or line.".into()));
                return;
            }
        },
        Some(answer) if answer.is(VERTEX) => {
            let Some(vertex) = ask_point("Specify angle vertex:", None).await else { return };
            let Some(first) = ask_point("Specify first angle endpoint:", Some(vertex)).await else { return };
            let Some(second) = ask_point("Specify second angle endpoint:", Some(vertex)).await else { return };
            (vertex, Box::new(move |cursor| Some(vec![vertex, first, second, cursor])))
        }
        _ => return,
    };
    place(&io, "Specify dimension arc line location:", vertex, move |cursor| {
        sides(cursor).map(|points| annotation(AnnotationKind::Angle, points, &look))
    })
    .await;
}

/// LEADER and QLEADER: points from the arrow's tip, then lines of text, written beside the last point.
pub async fn leader(io: Io, defaults: Rc<RefCell<Defaults>>) {
    let look = defaults.borrow().dimension_look.clone();
    let Some(Answer::Point(tip)) = io.ask(Prompt::point("Specify leader start point:")).await else { return };
    let mut points = vec![tip];
    loop {
        let last = points[points.len() - 1];
        let shown = points.clone();
        let preview_look = look.clone();
        let message = if points.len() == 1 { "Specify next point:" } else { "Specify next point or [Undo] <Annotation>:" };
        let prompt = Prompt::point(message).base(last).options(&[UNDO]).preview(move |cursor| {
            let mut points = shown.clone();
            points.push(cursor);
            vec![annotation(AnnotationKind::Leader { arrow: true }, points, &preview_look)]
        });
        match io.ask(prompt).await {
            Some(Answer::Point(p)) if p != last => points.push(p),
            Some(answer) if answer.is(UNDO) => {
                if points.len() > 1 {
                    points.pop();
                }
            }
            Some(_) => {}
            None if points.len() >= 2 => break,
            None => return,
        }
    }
    let mut lines: Vec<String> = vec![];
    loop {
        let message = if lines.is_empty() { "Enter first line of annotation text:" } else { "Enter next line of annotation text:" };
        match io.ask(Prompt::new(PromptKind::Text, message).default(Answer::Text(String::new()))).await {
            Some(Answer::Text(line)) if !line.trim().is_empty() => lines.push(line),
            Some(_) => break,
            None => return,
        }
    }
    let (end, before) = (points[points.len() - 1], points[points.len() - 2]);
    let rightward = end.x >= before.x;
    let ahead = if rightward { 1.0 } else { -1.0 };
    let size = look.arrow_size * look.scale;
    // A short level landing where the text sits, unless the leader already ends level.
    let landing_end = if (end.y - before.y).abs() > 1e-9 * end.dist(before) {
        let landing = end + crate::model::geometry::pt(ahead * size, 0.0);
        points.push(landing);
        landing
    } else {
        end
    };
    let mut drawn = vec![annotation(AnnotationKind::Leader { arrow: true }, points, &look)];
    if !lines.is_empty() {
        let gap = look.text_gap * look.scale;
        drawn.push(Entity::new(Shape::Text {
            position: landing_end + crate::model::geometry::pt(ahead * gap, 0.0),
            text: lines.join("\n"),
            height: look.text_height * look.scale,
            rotation: 0.0,
            font: String::new(),
            style: String::new(),
            align: TextAlign { h: if rightward { HAlign::Left } else { HAlign::Right }, v: VAlign::Middle },
            line_spacing: 1.0,
            formatted: String::new(),
            width_factor: 1.0,
            oblique: 0.0,
        }));
    }
    io.step(Step::Drawn(drawn));
}

const UNDO: &str = "U";

const X_DATUM: &str = "X";
const Y_DATUM: &str = "Y";

/// DIMORDINATE: how far a feature is from the origin, across (X datum) or up (Y datum), at the end of a leader. The
/// leader's direction picks which, as in AutoCAD, unless Xdatum or Ydatum says.
pub async fn ordinate(io: Io, defaults: Rc<RefCell<Defaults>>) {
    let look = defaults.borrow().dimension_look.clone();
    let Some(Answer::Point(feature)) = io.ask(Prompt::point("Specify feature location:")).await else { return };
    let origin = crate::model::geometry::pt(0.0, 0.0);
    let mut forced: Option<bool> = None;
    loop {
        let look = look.clone();
        let at = move |end: Point| {
            let x_datum = forced.unwrap_or_else(|| (end - feature).y.abs() >= (end - feature).x.abs());
            annotation(AnnotationKind::Ordinate { x_datum }, vec![origin, feature, end], &look)
        };
        let shown = at.clone();
        let prompt = Prompt::point("Specify leader endpoint location or [Xdatum/Ydatum]:")
            .base(feature)
            .options(&[X_DATUM, Y_DATUM])
            .preview(move |cursor| vec![shown(cursor)]);
        match io.ask(prompt).await {
            Some(Answer::Point(end)) if end != feature => {
                io.step(Step::Drawn(vec![at(end)]));
                return;
            }
            Some(answer) if answer.is(X_DATUM) => forced = Some(true),
            Some(answer) if answer.is(Y_DATUM) => forced = Some(false),
            Some(_) => {}
            None => return,
        }
    }
}

/// DIMARC: the length along an arc, with a dimension arc beside it where the cursor puts it.
pub async fn arc_length(io: Io, defaults: Rc<RefCell<Defaults>>) {
    let look = defaults.borrow().dimension_look.clone();
    let (center, start, end) = loop {
        let Some(Answer::Picked(picked)) = io.ask(Prompt::new(PromptKind::Pick, "Select arc segment:")).await else { return };
        match picked.entity.shape {
            Shape::Arc { center, radius, start_angle, end_angle } => {
                let at = |a: f64| center + crate::model::geometry::direction(a) * radius;
                break (center, at(start_angle), at(end_angle));
            }
            _ => io.step(Step::Report("Object selected is not an arc.".into())),
        }
    };
    place(&io, "Specify arc length dimension location:", center, move |cursor| {
        Some(annotation(AnnotationKind::ArcLength, vec![center, start, end, cursor], &look))
    })
    .await;
}

/// DIMJOGGED: a radius whose center is too far off to show: the line starts from a center shown nearer, and jogs.
pub async fn jogged(io: Io, defaults: Rc<RefCell<Defaults>>) {
    let Some((center, radius)) = pick_curve(&io, "Select arc or circle:").await else { return };
    let look = defaults.borrow().dimension_look.clone();
    let Some(Answer::Point(shown_center)) = io.ask(Prompt::point("Specify center location override:")).await else { return };
    let chord_toward = move |cursor: Point| {
        let toward = cursor - center;
        center + toward * (radius / toward.length().max(f64::MIN_POSITIVE))
    };
    let preview_look = look.clone();
    let prompt = Prompt::point("Specify dimension line location:").base(shown_center).preview(move |cursor| {
        let chord = chord_toward(cursor);
        let look = DimensionLook { text_position: Some(cursor), ..preview_look.clone() };
        vec![annotation(AnnotationKind::JoggedRadius, vec![center, chord, shown_center, chord.midpoint(center)], &look)]
    });
    let Some(Answer::Point(location)) = io.ask(prompt).await else { return };
    let chord = chord_toward(location);
    let look = DimensionLook { text_position: Some(location), ..look };
    place(&io, "Specify jog location:", chord, move |jog| {
        Some(annotation(AnnotationKind::JoggedRadius, vec![center, chord, shown_center, jog], &look))
    })
    .await;
}

const LEFT: &str = "L";
const RIGHT: &str = "R";
const CENTER: &str = "C";
const HOME: &str = "H";
const ANGLE: &str = "A";

/// The dimension text's look and where its middle sits now.
fn dimension_text_middle(entity: &Entity) -> Option<(DimensionLook, Point)> {
    match &entity.shape {
        Shape::Dimension { start, end, line_point, angle, look } => {
            Some((look.clone(), crate::model::dimension::dimension_parts(*start, *end, *line_point, *angle, look).text_middle))
        }
        Shape::Annotation { kind, points, look } if !matches!(kind, AnnotationKind::Leader { .. }) => {
            let parts = crate::model::annotation::annotation_parts(*kind, points, look);
            Some((look.clone(), look.text_position.or(parts.text.map(|t| t.middle))?))
        }
        _ => None,
    }
}

fn with_look(entity: &Entity, look: DimensionLook) -> Entity {
    let shape = match entity.shape.clone() {
        Shape::Dimension { start, end, line_point, angle, .. } => Shape::Dimension { start, end, line_point, angle, look },
        Shape::Annotation { kind, points, .. } => Shape::Annotation { kind, points, look },
        other => other,
    };
    entity.with_shape(shape)
}

/// DIMTEDIT: move a dimension's text where the cursor puts it, or to the left or right end of the dimension line,
/// back to the middle, back home where it started, or turn it.
pub async fn text_edit(io: Io, selected: Vec<Entity>) {
    let dimension = match selected.iter().find(|e| dimension_text_middle(e).is_some()) {
        Some(found) => found.clone(),
        None => loop {
            let Some(Answer::Picked(picked)) = io.ask(Prompt::new(PromptKind::Pick, "Select dimension:")).await else { return };
            if dimension_text_middle(&picked.entity).is_some() {
                break picked.entity;
            }
            io.step(Step::Report("Pick a dimension.".into()));
        },
    };
    let Some((look, middle)) = dimension_text_middle(&dimension) else { return };
    let moved = {
        let (dimension, look) = (dimension.clone(), look.clone());
        move |at: Point| with_look(&dimension, DimensionLook { text_position: Some(at), ..look.clone() })
    };
    let shown = moved.clone();
    let prompt = Prompt::point("Specify new location for dimension text or [Left/Right/Center/Home/Angle]:")
        .base(middle)
        .options(&[LEFT, RIGHT, CENTER, HOME, ANGLE])
        .preview(move |cursor| vec![shown(cursor)]);
    let changed = match io.ask(prompt).await {
        Some(Answer::Point(at)) => moved(at),
        Some(answer) if answer.is(HOME) => with_look(&dimension, DimensionLook { text_position: None, text_rotation: None, ..look }),
        Some(answer) if answer.is(CENTER) => with_look(&dimension, DimensionLook { text_position: None, ..look }),
        Some(answer) if answer.is(LEFT) || answer.is(RIGHT) => {
            let Shape::Dimension { start, end, line_point, angle, .. } = &dimension.shape else {
                io.step(Step::Report("Left and Right move the text of linear dimensions.".into()));
                return;
            };
            let centered = DimensionLook { text_position: None, ..look.clone() };
            let parts = crate::model::dimension::dimension_parts(*start, *end, *line_point, *angle, &centered);
            // Along the text's reading direction, the end of the line it should sit by.
            let along = crate::model::geometry::direction(parts.text_angle);
            let (a, b) = parts.dimension_line;
            let (low, high) = if (b - a).dot(along) >= 0.0 { (a, b) } else { (b, a) };
            let half = centered.text_half_width(&parts.text, &None) + centered.arrow_size * centered.scale * 1.5;
            let target = if answer.is(LEFT) { low + along * half } else { high - along * half };
            moved(parts.text_middle + along * (target - parts.text_middle).dot(along))
        }
        Some(answer) if answer.is(ANGLE) => {
            let prompt = Prompt::new(PromptKind::Angle, "Specify angle for dimension text:").base(middle);
            let Some(Answer::Number(degrees)) = io.ask(prompt).await else { return };
            with_look(&dimension, DimensionLook { text_rotation: Some(degrees), ..look })
        }
        _ => return,
    };
    io.step(Step::Modify(vec![changed]));
}

/// MLEADER: an arrowhead, a landing, then the text, sized by the current multileader style.
pub async fn mleader(io: Io, defaults: Rc<RefCell<Defaults>>) {
    let style = defaults.borrow().mleader_style.clone();
    let Some(Answer::Point(tip)) = io.ask(Prompt::point("Specify leader arrowhead location:")).await else { return };
    let made = Rc::new(move |landing: Point, text: String| {
        let dogleg = if landing.x < tip.x { -style.dogleg } else { style.dogleg };
        Entity::new(Shape::Multileader {
            leaders: vec![vec![tip]],
            landing,
            dogleg,
            text,
            text_position: crate::model::multileader::text_corner(landing, dogleg, style.text_height, style.landing_gap),
            text_height: style.text_height,
            arrow_size: style.arrow_size,
            style: style.name.clone(),
        })
    });
    let shown = made.clone();
    let prompt = Prompt::point("Specify leader landing location:").base(tip).preview(move |cursor| vec![shown(cursor, String::new())]);
    let Some(Answer::Point(landing)) = io.ask(prompt).await else { return };
    let Some(Answer::Text(typed)) = io.ask(Prompt::new(PromptKind::Text, "Enter text (\\P for a new line):")).await else { return };
    io.step(Step::Drawn(vec![made(landing, typed.replace("\\P", "\n"))]));
}

async fn pick_multileader(io: &Io) -> Option<Entity> {
    loop {
        let Some(Answer::Picked(picked)) = io.ask(Prompt::new(PromptKind::Pick, "Select a multileader:")).await else { return None };
        if matches!(picked.entity.shape, Shape::Multileader { .. }) {
            return Some(picked.entity);
        }
        io.step(Step::Report("Object selected is not a multileader.".into()));
    }
}

/// MLEADERADD: more leader lines to a multileader's landing, one for each point given.
pub async fn mleader_add(io: Io) {
    let Some(mut target) = pick_multileader(&io).await else { return };
    let Shape::Multileader { landing, .. } = target.shape else { return };
    while let Some(Answer::Point(tip)) =
        io.ask(Prompt::point("Specify leader arrowhead location:").base(landing).preview(line_preview(landing))).await
    {
        if let Shape::Multileader { leaders, .. } = &mut target.shape {
            leaders.push(vec![tip]);
        }
        io.step(Step::Modify(vec![target.clone()]));
    }
}

/// MLEADERREMOVE: take away the leader line whose arrowhead is nearest each point given. One always stays.
pub async fn mleader_remove(io: Io) {
    let Some(mut target) = pick_multileader(&io).await else { return };
    while let Some(Answer::Point(near)) = io.ask(Prompt::point("Specify leaders to remove:")).await {
        let Shape::Multileader { leaders, .. } = &mut target.shape else { return };
        if leaders.len() < 2 {
            io.step(Step::Report("A multileader keeps at least one leader.".into()));
            continue;
        }
        let nearest = (0..leaders.len()).min_by(|a, b| {
            let distance = |i: &usize| leaders[*i].first().map_or(f64::INFINITY, |p| p.dist(near));
            distance(a).total_cmp(&distance(b))
        });
        if let Some(index) = nearest {
            leaders.remove(index);
        }
        io.step(Step::Modify(vec![target.clone()]));
    }
}
