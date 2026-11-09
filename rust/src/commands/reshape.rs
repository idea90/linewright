//! LENGTHEN, ALIGN and PEDIT.

use std::cell::RefCell;
use std::rc::Rc;

use super::defaults::{Defaults, Lengthen};
use super::edit::{copies, objects_to_edit};
use super::format_general;
use super::prompts::{Answer, Io, Picked, Prompt, PromptKind, Step};
use crate::model::editing::join;
use crate::model::entities::{Entity, Shape, Vertex};
use crate::model::geometry::{Point, modulo, pt, sweep_degrees};
use crate::model::parts::spline_points;
use crate::model::spline::SplineData;
use crate::model::transform::{rotation, scaling, transform_shape, translation};

const DELTA: &str = "DE";
const PERCENT: &str = "P";
const TOTAL: &str = "T";
const UNDO: &str = "U";
const WIDTH: &str = "W";
const YES: &str = "Y";
const NO: &str = "N";

/// A line's or an arc's length.
fn length(shape: &Shape) -> Option<f64> {
    match shape {
        Shape::Line { start, end } => Some(start.dist(*end)),
        Shape::Arc { radius, start_angle, end_angle, .. } => Some(radius * sweep_degrees(*start_angle, *end_angle).to_radians()),
        _ => None,
    }
}

/// A line or an arc made longer or shorter at the end nearer `near`. None for anything else, or a length of zero
/// or less (or a whole circle or more, for an arc).
pub fn lengthened(shape: &Shape, near: Point, how: Lengthen) -> Option<Shape> {
    let old = length(shape)?;
    let new = match how {
        Lengthen::Delta(delta) => old + delta,
        Lengthen::Percent(percent) => old * percent / 100.0,
        Lengthen::Total(total) => total,
    };
    if new <= 0.0 {
        return None;
    }
    Some(match *shape {
        Shape::Line { start, end } => {
            let along = (end - start) * (1.0 / old);
            if near.dist(start) < near.dist(end) {
                Shape::Line { start: end - along * new, end }
            } else {
                Shape::Line { start, end: start + along * new }
            }
        }
        Shape::Arc { center, radius, start_angle, end_angle } => {
            let sweep = (new / radius).to_degrees();
            if sweep >= 360.0 {
                return None;
            }
            let at = |a: f64| center + pt(a.to_radians().cos(), a.to_radians().sin()) * radius;
            if near.dist(at(start_angle)) < near.dist(at(end_angle)) {
                Shape::Arc { center, radius, start_angle: modulo(end_angle - sweep, 360.0), end_angle }
            } else {
                Shape::Arc { center, radius, start_angle, end_angle: modulo(start_angle + sweep, 360.0) }
            }
        }
        _ => return None,
    })
}

/// Enter at the first prompt: the way LENGTHEN worked last time.
const LAST: &str = "LAST";

fn describe(how: Lengthen) -> String {
    match how {
        Lengthen::Delta(d) => format!("DElta {}", format_general(d)),
        Lengthen::Percent(p) => format!("Percent {}", format_general(p)),
        Lengthen::Total(t) => format!("Total {}", format_general(t)),
    }
}

async fn amount(io: &Io, message: String, default: f64) -> Option<f64> {
    let prompt = Prompt::new(PromptKind::Text, message).default(Answer::Text(format_general(default)));
    match io.ask(prompt).await {
        Some(Answer::Text(typed)) => typed.trim().parse::<f64>().ok().filter(|v| v.is_finite()),
        _ => None,
    }
}

/// LENGTHEN: pick a line or arc to hear its length; choose DElta, Percent or Total, then pick the ends to change.
pub async fn lengthen_command(io: Io, defaults: Rc<RefCell<Defaults>>) {
    let last = defaults.borrow().lengthen;
    let how = loop {
        let prompt = Prompt::new(PromptKind::Pick, format!("Select an object to measure or [DElta/Percent/Total] <{}>:", describe(last)))
            .options(&[DELTA, PERCENT, TOTAL])
            .default(Answer::keyword(LAST));
        match io.ask(prompt).await {
            Some(Answer::Picked(picked)) => match length(&picked.entity.shape) {
                Some(value) => io.step(Step::Report(format!("Current length: {}", format_general(value)))),
                None => io.step(Step::Report("Pick a line or an arc.".into())),
            },
            Some(answer) if answer.is(DELTA) => {
                let Some(delta) = amount(&io, "Enter delta length <0>:".into(), 0.0).await else { return };
                break Lengthen::Delta(delta);
            }
            Some(answer) if answer.is(PERCENT) => {
                let Some(percent) = amount(&io, "Enter percentage length <100>:".into(), 100.0).await else { return };
                break Lengthen::Percent(percent);
            }
            Some(answer) if answer.is(TOTAL) => {
                let Some(total) = amount(&io, "Specify total length:".into(), 1.0).await else { return };
                break Lengthen::Total(total);
            }
            Some(answer) if answer.is(LAST) => break last,
            _ => return,
        }
    };
    defaults.borrow_mut().lengthen = how;
    let mut changed = 0;
    loop {
        let prompt = Prompt::new(PromptKind::Pick, "Select an object to change or [Undo]:").options(&[UNDO]);
        match io.ask(prompt).await {
            Some(Answer::Picked(Picked { entity, point, .. })) => match lengthened(&entity.shape, point, how) {
                Some(shape) => {
                    io.step(Step::Modify(vec![entity.with_shape(shape)]));
                    changed += 1;
                }
                None => io.step(Step::Report("That cannot be lengthened that way.".into())),
            },
            Some(answer) if answer.is(UNDO) && changed > 0 => {
                io.step(Step::Retract);
                changed -= 1;
            }
            Some(_) => {}
            None => return,
        }
    }
}

/// ALIGN: move, turn and if wanted scale objects so one or two source points land on destination points.
pub async fn align_command(io: Io, selected: Vec<Entity>) {
    let objects = objects_to_edit(&io, selected).await;
    if objects.is_empty() {
        return;
    }
    let ask = |message: &'static str, base: Option<Point>| {
        let io = io.clone();
        async move {
            let prompt = match base {
                Some(base) => Prompt::point(message).base(base).preview(super::draw::line_preview(base)),
                None => Prompt::point(message),
            };
            match io.ask(prompt).await {
                Some(Answer::Point(p)) => Some(p),
                _ => None,
            }
        }
    };
    let Some(source) = ask("Specify first source point:", None).await else { return };
    let Some(target) = ask("Specify first destination point:", Some(source)).await else { return };
    let moved = translation(target.x - source.x, target.y - source.y);
    let Some(second_source) = ask("Specify second source point or <continue>:", Some(target)).await else {
        io.step(Step::Modify(objects.iter().map(|e| e.with_shape(transform_shape(&e.shape, &moved))).collect()));
        return;
    };
    let Some(second_target) = ask("Specify second destination point:", Some(second_source)).await else { return };
    let prompt = Prompt::point("Scale objects based on alignment points? [Yes/No] <N>:").options(&[YES, NO]).default(Answer::keyword(NO));
    let scale = matches!(io.ask(prompt).await, Some(answer) if answer.is(YES));
    let turn = target.angle_to(second_target) - source.angle_to(second_source);
    let factor = if scale { target.dist(second_target) / source.dist(second_source).max(f64::MIN_POSITIVE) } else { 1.0 };
    let sim = moved.after(&rotation(source, turn).after(&scaling(source, factor)));
    let placed = copies(&objects, &sim);
    let kept: Vec<Entity> = objects.iter().zip(placed).map(|(e, p)| e.with_shape(p.shape)).collect();
    io.step(Step::Modify(kept));
}

const CLOSE: &str = "C";
const OPEN: &str = "O";
const JOIN: &str = "J";
const REVERSE: &str = "R";
const DECURVE: &str = "D";

/// A line or an arc as a polyline, the arc's curve kept as a bulge.
fn as_polyline(shape: &Shape) -> Option<Shape> {
    Some(match *shape {
        Shape::Line { start, end } => Shape::Polyline { vertices: vec![vertex(start, 0.0), vertex(end, 0.0)], closed: false },
        Shape::Arc { center, radius, start_angle, end_angle } => {
            let at = |a: f64| center + pt(a.to_radians().cos(), a.to_radians().sin()) * radius;
            let bulge = (sweep_degrees(start_angle, end_angle).to_radians() / 4.0).tan();
            Shape::Polyline { vertices: vec![vertex(at(start_angle), bulge), vertex(at(end_angle), 0.0)], closed: false }
        }
        _ => return None,
    })
}

fn vertex(p: Point, bulge: f64) -> Vertex {
    Vertex { x: p.x, y: p.y, bulge, start_width: 0.0, end_width: 0.0 }
}

/// The same polyline run the other way: each segment's bulge moves to its new start and turns the other way.
pub fn reversed(vertices: &[Vertex], closed: bool) -> Vec<Vertex> {
    let n = vertices.len();
    (0..n)
        .map(|i| {
            let original = n - 1 - i;
            // The segment now starting here ran into this vertex before: from the one before it.
            let before = if original == 0 { if closed { n - 1 } else { usize::MAX } } else { original - 1 };
            let (bulge, start_width, end_width) = if before == usize::MAX {
                (0.0, 0.0, 0.0)
            } else {
                (-vertices[before].bulge, vertices[before].end_width, vertices[before].start_width)
            };
            Vertex { bulge, start_width, end_width, ..vertices[original] }
        })
        .collect()
}

/// PEDIT: pick a polyline (or a line or arc, which becomes one), then Close, Open, Join, Reverse or Decurve it,
/// as often as wanted.
pub async fn pedit_command(io: Io) {
    let Some(Answer::Picked(picked)) = io.ask(Prompt::new(PromptKind::Pick, "Select polyline:")).await else { return };
    let mut entity = picked.entity;
    if !matches!(entity.shape, Shape::Polyline { .. }) {
        let Some(shape) = as_polyline(&entity.shape) else {
            io.step(Step::Report("Pick a polyline, line or arc.".into()));
            return;
        };
        let prompt = Prompt::point("Object selected is not a polyline. Do you want to turn it into one? [Yes/No] <Y>:")
            .options(&[YES, NO])
            .default(Answer::keyword(YES));
        if !matches!(io.ask(prompt).await, Some(answer) if answer.is(YES)) {
            return;
        }
        entity = entity.with_shape(shape);
        io.step(Step::Modify(vec![entity.clone()]));
    }
    loop {
        let Shape::Polyline { vertices, closed } = entity.shape.clone() else { return };
        let message = format!("Enter an option [{}/Join/Width/Reverse/Decurve]:", if closed { "Open" } else { "Close" });
        let prompt = Prompt::point(message).options(&[if closed { OPEN } else { CLOSE }, JOIN, WIDTH, REVERSE, DECURVE]);
        let shape = match io.ask(prompt).await {
            Some(answer) if answer.is(CLOSE) => Shape::Polyline { vertices, closed: true },
            Some(answer) if answer.is(OPEN) => Shape::Polyline { vertices, closed: false },
            Some(answer) if answer.is(REVERSE) => Shape::Polyline { vertices: reversed(&vertices, closed), closed },
            Some(answer) if answer.is(WIDTH) => {
                let current = vertices.first().map_or(0.0, |v| v.start_width);
                let prompt = Prompt::new(PromptKind::Length, format!("Specify new width for all segments <{}>:", format_general(current)))
                    .default(Answer::Number(current));
                let Some(Answer::Number(width)) = io.ask(prompt).await else { continue };
                let width = width.abs();
                Shape::Polyline {
                    vertices: vertices.iter().map(|v| Vertex { start_width: width, end_width: width, ..*v }).collect(),
                    closed,
                }
            }
            Some(answer) if answer.is(DECURVE) => {
                Shape::Polyline { vertices: vertices.iter().map(|v| Vertex { bulge: 0.0, ..*v }).collect(), closed }
            }
            Some(answer) if answer.is(JOIN) => {
                let Some(Answer::Objects(others)) = io.ask(Prompt::new(PromptKind::Select, "Select objects to join:")).await else {
                    continue;
                };
                let mut pool = vec![entity.clone()];
                pool.extend(others.into_iter().filter(|o| o.id != entity.id));
                match join(&pool) {
                    Some((removed, added)) => {
                        let count = removed.len().saturating_sub(1);
                        if let Some(joined) = added.iter().find(|a| matches!(a.shape, Shape::Polyline { .. })).cloned() {
                            entity = joined;
                        }
                        io.step(Step::Compound(vec![Step::Remove(removed), Step::Commit(added)]));
                        io.step(Step::Report(format!("{count} segment(s) added to polyline.")));
                    }
                    None => io.step(Step::Report("Nothing joined: the objects must meet the polyline's ends.".into())),
                }
                continue;
            }
            _ => return,
        };
        entity = entity.with_shape(shape);
        io.step(Step::Modify(vec![entity.clone()]));
    }
}

const MOVE_POINT: &str = "M";
const ADD_POINT: &str = "A";
const DELETE_POINT: &str = "D";
const WEIGHT: &str = "W";
const TO_POLYLINE: &str = "P";

/// SPLINEDIT: close or open a spline, move, add or delete control points, weight them, reverse it, or turn it into a
/// polyline, as often as wanted.
pub async fn splinedit_command(io: Io) {
    let mut entity = loop {
        let Some(Answer::Picked(picked)) = io.ask(Prompt::new(PromptKind::Pick, "Select spline:")).await else { return };
        if matches!(picked.entity.shape, Shape::Spline { .. }) {
            break picked.entity;
        }
        io.step(Step::Report("Object selected is not a spline.".into()));
    };
    loop {
        let Shape::Spline { degree, control, knots, weights, closed } = entity.shape.clone() else { return };
        let mut data = SplineData::new(degree, &control, &knots, &weights);
        let message = format!(
            "Enter an option [{}/Move vertex/Add vertex/Delete vertex/Weight/Reverse/Polyline]:",
            if closed { "Open" } else { "Close" }
        );
        let prompt = Prompt::new(PromptKind::Text, message).options(&[
            if closed { OPEN } else { CLOSE },
            MOVE_POINT,
            ADD_POINT,
            DELETE_POINT,
            WEIGHT,
            REVERSE,
            TO_POLYLINE,
        ]);
        let mut now_closed = closed;
        let Some(answer) = io.ask(prompt).await else { return };
        if answer.is(CLOSE) {
            data.close();
            now_closed = true;
        } else if answer.is(OPEN) {
            data.open();
            now_closed = false;
        } else if answer.is(REVERSE) {
            data.reverse();
        } else if answer.is(MOVE_POINT) || answer.is(DELETE_POINT) || answer.is(WEIGHT) {
            let Some(Answer::Point(near)) = io.ask(Prompt::point("Specify a control point:")).await else { continue };
            let index = data.nearest_control(near);
            if answer.is(MOVE_POINT) {
                let from = data.control[index];
                let Some(Answer::Point(to)) = io.ask(Prompt::point("Specify new location:").base(from)).await else { continue };
                data.control[index] = to;
            } else if answer.is(DELETE_POINT) {
                if !data.remove_control(index) {
                    io.step(Step::Report(format!("A spline of degree {} keeps at least {} control points.", data.degree, data.degree + 1)));
                    continue;
                }
            } else {
                let current = data.weights[index];
                let prompt = Prompt::new(PromptKind::Number, format!("Enter new weight <{}>:", format_general(current)))
                    .default(Answer::Number(current));
                let Some(Answer::Number(weight)) = io.ask(prompt).await else { continue };
                if weight <= 0.0 {
                    io.step(Step::Report("A weight must be more than zero.".into()));
                    continue;
                }
                data.weights[index] = weight;
            }
        } else if answer.is(ADD_POINT) {
            let Some(Answer::Point(near)) = io.ask(Prompt::point("Specify a point on the spline to add a control point:")).await else {
                continue;
            };
            data.insert_knot(data.nearest_parameter(near));
        } else if answer.is(TO_POLYLINE) {
            let points = spline_points(data.degree, &data.control, &data.knots, &data.weights);
            let mut points = points;
            if closed && points.len() > 2 && points.first() == points.last() {
                points.pop();
            }
            let vertices = points.into_iter().map(crate::model::entities::vertex).collect();
            io.step(Step::Modify(vec![entity.with_shape(Shape::Polyline { vertices, closed })]));
            return;
        } else {
            return;
        }
        let weights = data.stored_weights();
        entity =
            entity.with_shape(Shape::Spline { degree: data.degree, control: data.control, knots: data.knots, weights, closed: now_closed });
        io.step(Step::Modify(vec![entity.clone()]));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lengthen_changes_the_end_picked_near() {
        let line = Shape::Line { start: pt(0.0, 0.0), end: pt(10.0, 0.0) };
        assert_eq!(lengthened(&line, pt(9.0, 0.0), Lengthen::Delta(5.0)), Some(Shape::Line { start: pt(0.0, 0.0), end: pt(15.0, 0.0) }));
        assert_eq!(lengthened(&line, pt(1.0, 0.0), Lengthen::Total(4.0)), Some(Shape::Line { start: pt(6.0, 0.0), end: pt(10.0, 0.0) }));
        assert_eq!(lengthened(&line, pt(9.0, 0.0), Lengthen::Percent(50.0)), Some(Shape::Line { start: pt(0.0, 0.0), end: pt(5.0, 0.0) }));
        assert_eq!(lengthened(&line, pt(9.0, 0.0), Lengthen::Delta(-20.0)), None);
        let arc = Shape::Arc { center: pt(0.0, 0.0), radius: 1.0, start_angle: 0.0, end_angle: 90.0 };
        let Some(Shape::Arc { end_angle, .. }) = lengthened(&arc, pt(0.0, 1.0), Lengthen::Total(std::f64::consts::PI)) else { panic!() };
        assert!((end_angle - 180.0).abs() < 1e-9);
    }

    #[test]
    fn reversing_a_polyline_keeps_its_curves() {
        let vertices = vec![vertex(pt(0.0, 0.0), 0.5), vertex(pt(1.0, 0.0), 0.0), vertex(pt(1.0, 1.0), 0.0)];
        let back = reversed(&vertices, false);
        assert_eq!(back.iter().map(|v| v.point()).collect::<Vec<_>>(), vec![pt(1.0, 1.0), pt(1.0, 0.0), pt(0.0, 0.0)]);
        assert_eq!(back.iter().map(|v| v.bulge).collect::<Vec<_>>(), vec![0.0, -0.5, 0.0]);
        assert_eq!(reversed(&back, false), vertices);
    }
}
