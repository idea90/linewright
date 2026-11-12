use super::prompts::{Answer, Io, Prompt, PromptKind, Step};
use crate::model::entities::{Entity, Shape, Vertex, line, polyline, polyline_from, text as text_entity, vertex};
use crate::model::geometry::{Point, pt};

pub const DEFAULT_TEXT_HEIGHT: f64 = 2.5;
pub const DEFAULT_TEXT_ROTATION: f64 = 0.0;
const CLOSE: &str = "C";
const WIDTH: &str = "W";
const HALFWIDTH: &str = "H";
const UNDO: &str = "U";

/// A rectangle with opposite corners at two points, its sides along the UCS's axes.
fn rectangle_shape(first: Point, other: Point, ucs: &crate::model::ucs::Ucs) -> Entity {
    let (a, b) = (ucs.to_ucs(first), ucs.to_ucs(other));
    polyline_from(&[first, ucs.to_world(pt(b.x, a.y)), other, ucs.to_world(pt(a.x, b.y))], true)
}

pub fn line_preview(start: Point) -> impl Fn(Point) -> Vec<Entity> {
    move |cursor| vec![line(start, cursor)]
}

/// The prompt for the next point of a chain: Undo from the first segment, Close from the third point.
fn next_point_prompt(points: &[Point]) -> Prompt {
    let keywords: Vec<&str> = if points.len() >= 3 { vec![CLOSE, UNDO] } else { vec![UNDO] };
    let names: Vec<&str> = keywords.iter().map(|k| if *k == CLOSE { "Close" } else { "Undo" }).collect();
    Prompt::point(format!("Specify next point or [{}]:", names.join("/"))).base(points[points.len() - 1]).options(&keywords)
}

pub async fn line_command(io: Io) {
    let mut points: Vec<Point> = vec![];
    loop {
        if points.is_empty() {
            match io.ask(Prompt::point("Specify first point:")).await {
                Some(Answer::Point(first)) => points.push(first),
                _ => return,
            }
            continue;
        }
        let last = points[points.len() - 1];
        match io.ask(next_point_prompt(&points).preview(line_preview(last))).await {
            Some(Answer::Keyword(k)) if k == UNDO => {
                if points.len() > 1 {
                    io.step(Step::Retract);
                }
                points.pop();
            }
            Some(Answer::Keyword(k)) if k == CLOSE => {
                io.step(Step::Drawn(vec![line(last, points[0])]));
                return;
            }
            Some(Answer::Point(next)) => {
                io.step(Step::Drawn(vec![line(last, next)]));
                points.push(next);
            }
            _ => return,
        }
    }
}

/// A point of a polyline being drawn: where it is, the widths its segment starts and ends at, and that segment's
/// bulge (0 for a straight one).
#[derive(Clone, Copy)]
struct Corner {
    point: Point,
    start_width: f64,
    end_width: f64,
    bulge: f64,
}

/// A polyline with each point's segment at its widths and bulge.
fn wide_polyline(corners: &[Corner], closed: bool) -> Entity {
    let vertices =
        corners.iter().map(|c| Vertex { start_width: c.start_width, end_width: c.end_width, bulge: c.bulge, ..vertex(c.point) }).collect();
    polyline(vertices, closed)
}

/// The direction, in radians, a polyline is heading where its last segment ends: along a line, or along an arc's
/// tangent there. East before there is any segment.
fn heading(corners: &[Corner]) -> f64 {
    let [.., from, to] = corners else { return 0.0 };
    let chord = (to.point.y - from.point.y).atan2(to.point.x - from.point.x);
    chord + 2.0 * from.bulge.atan()
}

/// The bulge of the arc that leaves `from` heading `direction` (radians) and ends at `to`: PLINE's arcs carry on
/// from the segment before, as in AutoCAD.
fn tangent_bulge(from: Point, direction: f64, to: Point) -> f64 {
    let chord = (to.y - from.y).atan2(to.x - from.x);
    // The arc turns through twice the angle from its start direction to its chord.
    let half_turn = (chord - direction + std::f64::consts::PI).rem_euclid(2.0 * std::f64::consts::PI) - std::f64::consts::PI;
    (half_turn / 2.0).tan()
}

/// The bulge of the arc from `from` to `to` that passes through `through`, or None when the three are in line.
fn three_point_bulge(from: Point, through: Point, to: Point) -> Option<f64> {
    let (a, b) = (through - from, to - through);
    let cross = a.cross(b);
    if cross.abs() < 1e-12 {
        return None;
    }
    // The angle at `through` between the chords to both ends is half the arc's remaining turn.
    let inscribed = (cross).atan2(a.dot(b));
    Some((inscribed / 2.0).tan())
}

const ARC: &str = "A";
const LINE: &str = "L";
const SECOND: &str = "S";

/// PLINE: straight segments, and with Arc, arcs that carry on from the segment before (or pass through a Second
/// point); Line goes back to straight ones. Width and Halfwidth taper the next segment.
pub async fn polyline_command(io: Io) {
    let Some(Answer::Point(first)) = io.ask(Prompt::point("Specify start point:")).await else { return };
    // Width and Halfwidth set the next segment's widths.
    let (mut start_width, mut end_width) = (0.0, 0.0);
    let mut corners = vec![Corner { point: first, start_width, end_width, bulge: 0.0 }];
    let mut arcs = false;
    loop {
        let shown = corners.clone();
        let closable = corners.len() >= 3;
        let (message, options): (&str, &[&str]) = match (arcs, closable) {
            (false, true) => ("Specify next point or [Arc/Halfwidth/Width/Close/Undo]:", &[ARC, HALFWIDTH, WIDTH, CLOSE, UNDO]),
            (false, false) => ("Specify next point or [Arc/Halfwidth/Width/Undo]:", &[ARC, HALFWIDTH, WIDTH, UNDO]),
            (true, true) => {
                ("Specify endpoint of arc or [Line/Second pt/Halfwidth/Width/CLose/Undo]:", &[LINE, SECOND, HALFWIDTH, WIDTH, "CL", UNDO])
            }
            (true, false) => ("Specify endpoint of arc or [Line/Second pt/Halfwidth/Width/Undo]:", &[LINE, SECOND, HALFWIDTH, WIDTH, UNDO]),
        };
        let base = corners[corners.len() - 1].point;
        let direction = heading(&corners);
        let prompt = Prompt::point(message).base(base).options(options).preview(move |cursor| {
            let mut path = shown.clone();
            if arcs && let Some(last) = path.last_mut() {
                last.bulge = tangent_bulge(last.point, direction, cursor);
            }
            path.push(Corner { point: cursor, start_width: 0.0, end_width: 0.0, bulge: 0.0 });
            vec![wide_polyline(&path, false)]
        });
        // The segment to `next` leaves the last corner with this bulge.
        let add = |corners: &mut Vec<Corner>, next: Point, bulge: f64| {
            if let Some(last) = corners.last_mut() {
                last.bulge = bulge;
            }
            corners.push(Corner { point: next, start_width, end_width, bulge: 0.0 });
        };
        match io.ask(prompt).await {
            Some(Answer::Keyword(k)) if k == ARC && !arcs => arcs = true,
            Some(Answer::Keyword(k)) if k == LINE && arcs => arcs = false,
            Some(Answer::Keyword(k)) if k == SECOND && arcs => {
                let Some(Answer::Point(through)) = io.ask(Prompt::point("Specify second point on arc:").base(base)).await else { return };
                let Some(Answer::Point(end)) = io.ask(Prompt::point("Specify end point of arc:").base(through)).await else { return };
                match three_point_bulge(base, through, end) {
                    Some(bulge) => add(&mut corners, end, bulge),
                    None => io.step(Step::Report("The three points are in a line.".into())),
                }
            }
            Some(Answer::Keyword(k)) if k == WIDTH || k == HALFWIDTH => {
                let factor = if k == WIDTH { 1.0 } else { 2.0 };
                let name = if k == WIDTH { "width" } else { "half-width" };
                let ask = |message: String, default: f64| {
                    let io = io.clone();
                    async move {
                        let prompt = Prompt::new(PromptKind::Length, format!("{message} <{}>:", super::format_general(default)))
                            .default(Answer::Number(default));
                        match io.ask(prompt).await {
                            Some(Answer::Number(value)) => Some(value.abs()),
                            _ => None,
                        }
                    }
                };
                let Some(start) = ask(format!("Specify starting {name}"), end_width / factor).await else { return };
                let Some(end) = ask(format!("Specify ending {name}"), start).await else { return };
                (start_width, end_width) = (start * factor, end * factor);
                if let Some(last) = corners.last_mut() {
                    (last.start_width, last.end_width) = (start_width, end_width);
                }
                // After a taper, later segments carry on at the ending width, as in AutoCAD.
                start_width = end_width;
            }
            Some(Answer::Keyword(k)) if k == CLOSE || k == "CL" => {
                // Closing with an arc carries on from the last segment, back to the start.
                if arcs && let Some(last) = corners.last_mut() {
                    last.bulge = tangent_bulge(last.point, direction, first);
                }
                io.step(Step::Drawn(vec![wide_polyline(&corners, true)]));
                return;
            }
            Some(Answer::Keyword(k)) if k == UNDO => {
                if corners.len() > 1 {
                    corners.pop();
                    if let Some(last) = corners.last_mut() {
                        last.bulge = 0.0;
                    }
                }
            }
            Some(Answer::Point(next)) => {
                let bulge = if arcs && next != base { tangent_bulge(base, direction, next) } else { 0.0 };
                add(&mut corners, next, bulge);
            }
            _ => {
                if corners.len() >= 2 {
                    io.step(Step::Drawn(vec![wide_polyline(&corners, false)]));
                }
                return;
            }
        }
    }
}

pub async fn rectangle_command(io: Io, defaults: std::rc::Rc<std::cell::RefCell<super::defaults::Defaults>>) {
    let ucs = defaults.borrow().ucs;
    let Some(Answer::Point(first)) = io.ask(Prompt::point("Specify first corner point:")).await else { return };
    let prompt = Prompt::point("Specify other corner point:").base(first).preview(move |cursor| vec![rectangle_shape(first, cursor, &ucs)]);
    if let Some(Answer::Point(other)) = io.ask(prompt).await {
        io.step(Step::Drawn(vec![rectangle_shape(first, other, &ucs)]));
    }
}

pub async fn text_command(io: Io, defaults: std::rc::Rc<std::cell::RefCell<super::defaults::Defaults>>) {
    let style = defaults.borrow().text_style.clone();
    let Some(Answer::Point(position)) = io.ask(Prompt::point("Specify start point of text:")).await else { return };
    // A style with a height of its own does not ask for one.
    let height = if style.height > 0.0 {
        Some(Answer::Number(style.height))
    } else {
        io.ask(
            Prompt::new(PromptKind::Number, format!("Specify height <{DEFAULT_TEXT_HEIGHT}>:"))
                .base(position)
                .default(Answer::Number(DEFAULT_TEXT_HEIGHT)),
        )
        .await
    };
    let rotation = io
        .ask(
            Prompt::new(PromptKind::Angle, format!("Specify rotation angle of text <{}>:", super::format_general(DEFAULT_TEXT_ROTATION)))
                .base(position)
                .default(Answer::Number(DEFAULT_TEXT_ROTATION))
                .preview(line_preview(position)),
        )
        .await;
    let content = io.ask(Prompt::new(PromptKind::Text, "Enter text:")).await;
    if let (Some(Answer::Number(height)), Some(Answer::Number(rotation)), Some(Answer::Text(content))) = (height, rotation, content) {
        let mut entity = text_entity(position, &content, height, rotation);
        if let Shape::Text { font, style: name, width_factor, oblique, .. } = &mut entity.shape {
            // A style with no font of its own leaves the text in the interface font, as it reopens.
            if !style.font.is_empty() {
                (*font, *name) = (style.font, style.name);
            }
            (*width_factor, *oblique) = (style.width_factor, style.oblique);
        }
        io.step(Step::Drawn(vec![entity]));
    }
}
