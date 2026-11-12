//! UCS and UCSICON: setting the user coordinate system that points are typed in and the grid, snap and ortho
//! follow, and showing its icon. Linewright draws in the XY plane, so a UCS can move its origin and turn about Z;
//! the options that tilt it out of the plane are refused.

use std::cell::RefCell;
use std::rc::Rc;

use super::defaults::Defaults;
use super::prompts::{Answer, Io, Prompt, PromptKind, Step, UcsRequest};
use crate::model::entities::Shape;
use crate::model::geometry::{Point, bulge_arc};
use crate::model::ucs::Ucs;

const FACE: &str = "F";
const NAMED: &str = "NA";
const OBJECT: &str = "OB";
const PREVIOUS: &str = "P";
const VIEW: &str = "V";
const WORLD: &str = "W";
const X: &str = "X";
const Y: &str = "Y";
const Z: &str = "Z";
const Z_AXIS: &str = "ZA";
const RESTORE: &str = "R";
const SAVE: &str = "S";
const DELETE: &str = "D";
const LIST: &str = "?";

const OUT_OF_PLANE: &str = "Linewright draws in the XY plane: a UCS can move its origin and turn about Z, not tilt.";

/// The UCS an object sets with UCS OBject: along a line from its end nearer the pick, at an arc's or circle's
/// center pointing at the pick (an arc's start), along a polyline's segment nearest the pick, at text's or a
/// block's insertion point along its rotation.
fn object_ucs(shape: &Shape, picked: Point) -> Option<Ucs> {
    let toward = |origin: Point, to: Point| (to != origin).then(|| Ucs::new(origin, origin.angle_to(to)));
    match shape {
        Shape::Line { start, end } => {
            let (origin, other) = if picked.dist(*start) <= picked.dist(*end) { (*start, *end) } else { (*end, *start) };
            toward(origin, other)
        }
        Shape::Circle { center, .. } => toward(*center, picked),
        Shape::Arc { center, start_angle, .. } => Some(Ucs::new(*center, *start_angle)),
        Shape::Polyline { vertices, closed } => {
            let count = if *closed { vertices.len() } else { vertices.len().saturating_sub(1) };
            let nearest = (0..count).min_by(|&a, &b| {
                let distance = |i: usize| {
                    let (here, next) = (vertices[i].point(), vertices[(i + 1) % vertices.len()].point());
                    distance_to_segment(picked, here, next)
                };
                distance(a).total_cmp(&distance(b))
            })?;
            let (here, next) = (vertices[nearest], vertices[(nearest + 1) % vertices.len()]);
            if here.bulge != 0.0 && here.point() != next.point() {
                Some(Ucs { origin: bulge_arc(here.point(), next.point(), here.bulge).center, angle: 0.0 }.pointing(here.point()))
            } else {
                toward(here.point(), next.point())
            }
        }
        Shape::Text { position, rotation, .. } => Some(Ucs::new(*position, *rotation)),
        Shape::Insert { position, rotation, .. } => Some(Ucs::new(*position, *rotation)),
        Shape::Dimension { line_point, angle, .. } => Some(Ucs::new(*line_point, *angle)),
        _ => None,
    }
}

/// How far a point is from a segment.
fn distance_to_segment(p: Point, a: Point, b: Point) -> f64 {
    let along = b - a;
    let length = along.dot(along);
    let t = if length == 0.0 { 0.0 } else { ((p - a).dot(along) / length).clamp(0.0, 1.0) };
    p.dist(a + along * t)
}

trait Pointing {
    fn pointing(self, at: Point) -> Ucs;
}

impl Pointing for Ucs {
    /// The UCS turned so its X axis points from its origin at a point.
    fn pointing(self, at: Point) -> Ucs {
        if at == self.origin { self } else { Ucs::new(self.origin, self.origin.angle_to(at)) }
    }
}

/// UCS: a new origin and X axis, World, Previous, turning about Z, an object's, or one saved by name.
pub async fn ucs_command(io: Io, defaults: Rc<RefCell<Defaults>>) {
    let (current, named) = {
        let d = defaults.borrow();
        (d.ucs, d.named_ucs.clone())
    };
    let name = if current.is_world() {
        "World".to_string()
    } else {
        named.iter().find(|(_, u)| *u == current).map_or("*NO NAME*".into(), |(n, _)| n.clone())
    };
    io.step(Step::Report(format!("Current ucs name:  {name}")));
    let prompt = Prompt::point("Specify origin of UCS or [Face/NAmed/OBject/Previous/View/World/X/Y/Z/ZAxis] <World>:")
        .options(&[FACE, NAMED, OBJECT, PREVIOUS, VIEW, WORLD, X, Y, Z, Z_AXIS])
        .default(Answer::keyword(WORLD));
    match io.ask(prompt).await {
        Some(Answer::Point(origin)) => {
            // The X axis points where asked, or stays as it was.
            let prompt = Prompt::point("Specify point on X-axis or <Accept>:").base(origin).default(Answer::keyword(""));
            let ucs = match io.ask(prompt).await {
                Some(Answer::Point(on_axis)) if on_axis != origin => Ucs::new(origin, origin.angle_to(on_axis)),
                Some(_) => Ucs { origin, ..current },
                None => return,
            };
            io.step(Step::Ucs(UcsRequest::Set(ucs)));
        }
        Some(answer) if answer.is(WORLD) || answer.is(VIEW) => {
            // The view is never turned here, so its UCS is the world's directions at the current origin.
            let ucs = if answer.is(WORLD) { Ucs::WORLD } else { Ucs { angle: 0.0, ..current } };
            io.step(Step::Ucs(UcsRequest::Set(ucs)));
        }
        Some(answer) if answer.is(PREVIOUS) => io.step(Step::Ucs(UcsRequest::Previous)),
        Some(answer) if answer.is(Z) => {
            let prompt = Prompt::new(PromptKind::Angle, "Specify rotation angle about Z axis <90>:").turn().default(Answer::Number(90.0));
            let Some(Answer::Number(turn)) = io.ask(prompt).await else { return };
            io.step(Step::Ucs(UcsRequest::Set(current.rotated(turn))));
        }
        Some(answer) if answer.is(OBJECT) => loop {
            let prompt = Prompt::new(PromptKind::Pick, "Select object to align UCS:");
            let Some(Answer::Picked(picked)) = io.ask(prompt).await else { return };
            match object_ucs(&picked.entity.shape, picked.point) {
                Some(ucs) => {
                    io.step(Step::Ucs(UcsRequest::Set(ucs)));
                    return;
                }
                None => io.step(Step::Report(format!("A UCS cannot be aligned to a {}.", picked.entity.shape.kind().to_lowercase()))),
            }
        },
        Some(answer) if answer.is(NAMED) => named_ucs(&io, &named).await,
        Some(answer) if [FACE, X, Y, Z_AXIS].iter().any(|k| answer.is(k)) => io.step(Step::Report(OUT_OF_PLANE.into())),
        _ => {}
    }
}

/// UCS NAmed: restore, save or delete a UCS by name, or list them.
async fn named_ucs(io: &Io, named: &[(String, Ucs)]) {
    let prompt = Prompt::new(PromptKind::Text, "Enter an option [Restore/Save/Delete/?]:").options(&[RESTORE, SAVE, DELETE, LIST]);
    match io.ask(prompt).await {
        Some(answer) if answer.is(LIST) => {
            let lines: Vec<String> = named
                .iter()
                .map(|(name, ucs)| format!("  {name}  origin {:.4},{:.4}  X axis {:.4} degrees", ucs.origin.x, ucs.origin.y, ucs.angle))
                .collect();
            io.step(Step::Report(if lines.is_empty() { "No UCS is saved.".into() } else { lines.join("\n") }));
        }
        Some(answer) if answer.is(SAVE) => {
            let Some(Answer::Text(name)) = io.ask(Prompt::new(PromptKind::Text, "Enter name to save current UCS or [?]:")).await else {
                return;
            };
            let name = name.trim().to_string();
            if name.is_empty() || name.contains(['<', '>', '/', '\\', '"', ':', ';', '?', '*', '|', ',', '=', '`']) {
                io.step(Step::Report("A UCS name cannot be empty or use < > / \\ \" : ; ? * | , = `.".into()));
            } else {
                io.step(Step::Ucs(UcsRequest::Save(name)));
            }
        }
        Some(answer) if answer.is(RESTORE) || answer.is(DELETE) => {
            let restore = answer.is(RESTORE);
            let message = if restore { "Enter name of UCS to restore:" } else { "Enter UCS name(s) to delete:" };
            let Some(Answer::Text(name)) = io.ask(Prompt::new(PromptKind::Text, message)).await else { return };
            match named.iter().find(|(n, _)| n.eq_ignore_ascii_case(name.trim())) {
                Some((name, ucs)) if restore => {
                    io.step(Step::Ucs(UcsRequest::Set(*ucs)));
                    io.step(Step::Report(format!("UCS {name} restored.")));
                }
                Some((name, _)) => io.step(Step::Ucs(UcsRequest::Delete(name.clone()))),
                None => io.step(Step::Report(format!("Cannot find UCS {}.", name.trim()))),
            }
        }
        _ => {}
    }
}

const ON: &str = "ON";
const OFF: &str = "OFF";
const ORIGIN: &str = "OR";
const NO_ORIGIN: &str = "N";

/// UCSICON: show or hide the UCS icon, and put it at the origin or in the corner.
pub async fn ucsicon_command(io: Io) {
    let prompt = Prompt::new(PromptKind::Text, "Enter an option [ON/OFF/Noorigin/ORigin] <ON>:")
        .options(&[ON, OFF, NO_ORIGIN, ORIGIN])
        .default(Answer::keyword(ON));
    let request = match io.ask(prompt).await {
        Some(answer) if answer.is(OFF) => UcsRequest::Icon(Some(false), None),
        Some(answer) if answer.is(ON) => UcsRequest::Icon(Some(true), None),
        Some(answer) if answer.is(NO_ORIGIN) => UcsRequest::Icon(None, Some(false)),
        Some(answer) if answer.is(ORIGIN) => UcsRequest::Icon(None, Some(true)),
        _ => return,
    };
    io.step(Step::Ucs(request));
}
