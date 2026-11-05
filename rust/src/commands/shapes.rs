//! Drawing ellipses, splines, points and hatches.

use std::cell::RefCell;
use std::rc::Rc;

use super::defaults::Defaults;
use super::draw::line_preview;
use super::edit::objects_to_edit;
use super::format_general;
use super::prompts::{Answer, Io, Prompt, PromptKind, Step};
use crate::canvas::aci::{color_rgb, parse_color};
use crate::model::boundary::fit_arcs;
use crate::model::editing::{closed_loop, hatch_pattern, hatch_pattern_names, rehatched};
use crate::model::entities::{Association, Entity, EntityId, Gradient, Shape, Vertex, ellipse, point};
use crate::model::geometry::{Point, pt};
use crate::model::parts::clamped_knots;

const CENTER: &str = "C";
const UNDO: &str = "U";

/// The ellipse with this center, one end of an axis, and the other axis reaching `distance` from the center.
/// Whichever axis is longer is the major one.
fn ellipse_from(center: Point, axis_end: Point, distance: f64) -> Option<Entity> {
    let half = axis_end - center;
    let length = half.length();
    if length == 0.0 || distance <= 0.0 {
        return None;
    }
    Some(if distance <= length {
        ellipse(center, half, distance / length)
    } else {
        ellipse(center, pt(-half.y, half.x) * (distance / length), length / distance)
    })
}

pub async fn ellipse_command(io: Io) {
    let first = io.ask(Prompt::point("Specify axis endpoint of ellipse or [Center]:").options(&[CENTER])).await;
    let (center, axis_end) = match first {
        Some(Answer::Keyword(_)) => {
            let Some(Answer::Point(center)) = io.ask(Prompt::point("Specify center of ellipse:")).await else { return };
            let prompt = Prompt::point("Specify endpoint of axis:").base(center).preview(line_preview(center));
            let Some(Answer::Point(end)) = io.ask(prompt).await else { return };
            (center, end)
        }
        Some(Answer::Point(start)) => {
            let prompt = Prompt::point("Specify other endpoint of axis:").base(start).preview(line_preview(start));
            let Some(Answer::Point(end)) = io.ask(prompt).await else { return };
            if end == start {
                return;
            }
            (start.midpoint(end), end)
        }
        _ => return,
    };
    let prompt = Prompt::new(PromptKind::Number, "Specify distance to other axis:")
        .base(center)
        .preview(move |cursor| ellipse_from(center, axis_end, center.dist(cursor)).into_iter().collect());
    if let Some(Answer::Number(distance)) = io.ask(prompt).await
        && let Some(shape) = ellipse_from(center, axis_end, distance)
    {
        io.step(Step::Drawn(vec![shape]));
    }
}

fn spline_through(control: &[Point]) -> Entity {
    let degree = 3.min(control.len().saturating_sub(1)).max(1);
    Entity::new(Shape::Spline {
        degree,
        control: control.to_vec(),
        knots: clamped_knots(degree, control.len()),
        weights: vec![],
        closed: false,
    })
}

/// A spline by its control vertices, as AutoCAD's SPLINE with Method CV.
pub async fn spline_command(io: Io) {
    let Some(Answer::Point(first)) = io.ask(Prompt::point("Specify first control vertex:")).await else { return };
    let mut points = vec![first];
    loop {
        let shown = points.clone();
        let prompt = Prompt::point("Specify next control vertex or [Undo] <finish>:")
            .base(points[points.len() - 1])
            .options(&[UNDO])
            .preview(move |cursor| {
                let mut control = shown.clone();
                control.push(cursor);
                vec![spline_through(&control)]
            });
        match io.ask(prompt).await {
            Some(Answer::Point(next)) => points.push(next),
            Some(Answer::Keyword(_)) => {
                if points.len() > 1 {
                    points.pop();
                }
            }
            _ => break,
        }
    }
    if points.len() >= 2 {
        io.step(Step::Drawn(vec![spline_through(&points)]));
    }
}

pub async fn point_command(io: Io) {
    if let Some(Answer::Point(position)) = io.ask(Prompt::point("Specify a point:")).await {
        io.step(Step::Drawn(vec![point(position)]));
    }
}

/// Fill closed objects with a pattern. Loops inside loops stay empty, as islands.
pub async fn hatch_command(io: Io, selected: Vec<Entity>, defaults: Rc<RefCell<Defaults>>) {
    let current = defaults.borrow().hatch_pattern.clone();
    let Some(pattern) = ask_pattern(&io, &current).await else { return };
    defaults.borrow_mut().hatch_pattern = pattern.clone();
    let (mut scale, mut angle) = (defaults.borrow().hatch_scale, defaults.borrow().hatch_angle);
    if pattern != "SOLID" {
        let prompt =
            Prompt::new(PromptKind::Number, format!("Specify pattern scale <{}>:", format_general(scale))).default(Answer::Number(scale));
        let Some(Answer::Number(s)) = io.ask(prompt).await else { return };
        let prompt =
            Prompt::new(PromptKind::Angle, format!("Specify pattern angle <{}>:", format_general(angle))).default(Answer::Number(angle));
        let Some(Answer::Number(a)) = io.ask(prompt).await else { return };
        (scale, angle) = (s, a);
        let mut remembered = defaults.borrow_mut();
        remembered.hatch_scale = scale;
        remembered.hatch_angle = angle;
    }
    let found = if selected.is_empty() { boundaries(&io).await } else { selected_boundaries(&io, selected).await };
    let Some((loops, association)) = found.filter(|(l, _)| !l.is_empty()) else { return };
    let Some((solid, lines)) = hatch_pattern(&pattern, scale, angle) else { return };
    io.step(Step::Drawn(vec![Entity::new(Shape::Hatch {
        loops,
        solid,
        pattern: lines,
        name: pattern,
        scale,
        angle,
        gradient: None,
        association,
    })]));
}

/// A pattern by name, any AutoCAD's pattern file has; ? lists them.
async fn ask_pattern(io: &Io, current: &str) -> Option<String> {
    loop {
        let prompt =
            Prompt::new(PromptKind::Text, format!("Enter a pattern name or [?] <{current}>:")).default(Answer::Text(current.to_string()));
        let Some(Answer::Text(typed)) = io.ask(prompt).await else { return None };
        let typed = typed.trim().to_uppercase();
        if typed == "?" {
            io.step(Step::Report(hatch_pattern_names().join(", ")));
        } else if hatch_pattern(&typed, 1.0, 0.0).is_some() {
            return Some(typed);
        } else {
            io.step(Step::Report(format!("There is no hatch pattern called {typed}. Type ? to list them.")));
        }
    }
}

const LINEAR: &str = "L";
const CYLINDER: &str = "C";
const INVERSE: &str = "I";

/// GRADIENT: two colors running across a boundary.
pub async fn gradient_command(io: Io, selected: Vec<Entity>) {
    let ask_color = |message: &'static str, default: &'static str| {
        let io = io.clone();
        async move {
            loop {
                let prompt = Prompt::new(PromptKind::Text, format!("{message} <{default}>:")).default(Answer::Text(default.to_string()));
                let Some(Answer::Text(typed)) = io.ask(prompt).await else { return None };
                match parse_color(&typed).and_then(color_rgb) {
                    Some(rgb) => return Some(rgb),
                    None => io.step(Step::Report("Enter a color name, an index (1-255) or red,green,blue.".into())),
                }
            }
        }
    };
    let Some(first) = ask_color("Enter the first color", "blue").await else { return };
    let Some(second) = ask_color("Enter the second color", "yellow").await else { return };
    let prompt = Prompt::new(PromptKind::Text, "Enter a gradient type [Linear/Cylinder/Inverse cylinder] <Linear>:")
        .options(&[LINEAR, CYLINDER, INVERSE])
        .default(Answer::keyword(LINEAR));
    let name = match io.ask(prompt).await {
        Some(answer) if answer.is(CYLINDER) => "CYLINDER",
        Some(answer) if answer.is(INVERSE) => "INVCYLINDER",
        Some(_) => "LINEAR",
        None => return,
    };
    let prompt = Prompt::new(PromptKind::Angle, "Specify the gradient's direction <0>:").default(Answer::Number(0.0));
    let Some(Answer::Number(angle)) = io.ask(prompt).await else { return };
    let found = if selected.is_empty() { boundaries(&io).await } else { selected_boundaries(&io, selected).await };
    let Some((loops, association)) = found.filter(|(l, _)| !l.is_empty()) else { return };
    let gradient = Some(Gradient { name: name.to_string(), first, second, angle });
    io.step(Step::Drawn(vec![Entity::new(Shape::Hatch {
        loops,
        solid: true,
        pattern: vec![],
        name: name.to_string(),
        scale: 1.0,
        angle: 0.0,
        gradient,
        association,
    })]));
}

const PATTERN: &str = "P";
const SCALE: &str = "S";
const ANGLE: &str = "A";
const REMOVE_LINK: &str = "U";

/// HATCHEDIT: change a hatch's pattern, scale or angle on the same boundary, or stop it following its boundary.
pub async fn hatchedit_command(io: Io, selected: Vec<Entity>) {
    let mut target = match selected.into_iter().find(|e| matches!(e.shape, Shape::Hatch { .. })) {
        Some(found) => found,
        None => loop {
            let Some(Answer::Picked(picked)) = io.ask(Prompt::new(PromptKind::Pick, "Select hatch object:")).await else { return };
            if matches!(picked.entity.shape, Shape::Hatch { .. }) {
                break picked.entity;
            }
            io.step(Step::Report("Object selected is not a hatch.".into()));
        },
    };
    loop {
        let Shape::Hatch { name, scale, angle, .. } = target.shape.clone() else { return };
        let prompt = Prompt::new(PromptKind::Text, "Enter hatch option [Pattern/Scale/Angle/Unassociate]:").options(&[
            PATTERN,
            SCALE,
            ANGLE,
            REMOVE_LINK,
        ]);
        let new_shape = match io.ask(prompt).await {
            Some(answer) if answer.is(PATTERN) => {
                let Some(pattern) = ask_pattern(&io, &name).await else { return };
                rehatched(&target.shape, &pattern, scale, angle)
            }
            Some(answer) if answer.is(SCALE) => {
                let prompt = Prompt::new(PromptKind::Number, format!("Specify pattern scale <{}>:", format_general(scale)))
                    .default(Answer::Number(scale));
                let Some(Answer::Number(new_scale)) = io.ask(prompt).await else { return };
                rehatched(&target.shape, &name, new_scale, angle)
            }
            Some(answer) if answer.is(ANGLE) => {
                let prompt = Prompt::new(PromptKind::Angle, format!("Specify pattern angle <{}>:", format_general(angle)))
                    .default(Answer::Number(angle));
                let Some(Answer::Number(new_angle)) = io.ask(prompt).await else { return };
                rehatched(&target.shape, &name, scale, new_angle)
            }
            Some(answer) if answer.is(REMOVE_LINK) => {
                let mut shape = target.shape.clone();
                if let Shape::Hatch { association, .. } = &mut shape {
                    *association = None;
                }
                Some(shape)
            }
            _ => return,
        };
        if let Some(shape) = new_shape {
            target = target.with_shape(shape);
            io.step(Step::Modify(vec![target.clone()]));
        }
    }
}

const SELECT_OBJECTS: &str = "S";

/// Boundaries picked as AutoCAD does: a click inside each area, until Enter. Select objects switches to choosing
/// closed objects instead. None when cancelled.
async fn boundaries(io: &Io) -> Option<(Vec<Vec<Point>>, Option<Association>)> {
    let mut picked: Vec<Vec<Vec<Point>>> = vec![];
    let mut seeds: Vec<Point> = vec![];
    loop {
        let shown: Vec<Entity> = picked.iter().flatten().map(|l| outline(l)).collect();
        let prompt = Prompt::point("Pick internal point or [Select objects/Undo]:")
            .options(&[SELECT_OBJECTS, UNDO])
            .region()
            .preview(move |_| shown.clone());
        match io.ask(prompt).await {
            Some(Answer::Region { loops, .. }) if loops.is_empty() => {
                io.step(Step::Report("Valid hatch boundary not found.".into()));
            }
            Some(Answer::Region { point, loops }) => {
                seeds.push(point);
                let islands = loops.len() - 1;
                if islands > 0 {
                    io.step(Step::Report(format!("Boundary found, with {islands} island(s) left unfilled.")));
                }
                picked.push(loops);
            }
            Some(answer) if answer.is(UNDO) => {
                picked.pop();
                seeds.pop();
            }
            Some(answer) if answer.is(SELECT_OBJECTS) => {
                let mut loops: Vec<Vec<Point>> = picked.into_iter().flatten().collect();
                let (chosen, association) = selected_boundaries(io, vec![]).await?;
                // Picked areas and chosen objects together follow nothing: neither alone makes the whole boundary.
                let association = if loops.is_empty() { association } else { None };
                loops.extend(chosen);
                return Some((loops, association));
            }
            _ => return Some((picked.into_iter().flatten().collect(), (!seeds.is_empty()).then_some(Association::Seeds(seeds)))),
        }
    }
}

/// Closed objects chosen as boundaries; any that are not closed are left out, with a word about it.
async fn selected_boundaries(io: &Io, selected: Vec<Entity>) -> Option<(Vec<Vec<Point>>, Option<Association>)> {
    let objects = objects_to_edit(io, selected).await;
    let closed: Vec<(EntityId, Vec<Point>)> = objects.iter().filter_map(|e| Some((e.id, closed_loop(e)?))).collect();
    let (ids, loops): (Vec<EntityId>, Vec<Vec<Point>>) = closed.into_iter().unzip();
    if loops.len() < objects.len() {
        io.step(Step::Report(format!("{} object(s) are not closed and were left out.", objects.len() - loops.len())));
    }
    Some((loops, (!ids.is_empty()).then_some(Association::Objects(ids))))
}

/// BOUNDARY: a closed polyline around each area clicked inside, and around the islands within it, as AutoCAD
/// makes them with island detection on. They are drawn when Enter ends the picking.
pub async fn boundary_command(io: Io) {
    let mut picked: Vec<Vec<Vec<Point>>> = vec![];
    loop {
        let shown: Vec<Entity> = picked.iter().flatten().map(|l| fitted_outline(l)).collect();
        let prompt = Prompt::point("Pick internal point or [Undo]:").options(&[UNDO]).region().preview(move |_| shown.clone());
        match io.ask(prompt).await {
            Some(Answer::Region { loops, .. }) if loops.is_empty() => {
                io.step(Step::Report("Valid boundary not found.".into()));
            }
            Some(Answer::Region { loops, .. }) => picked.push(loops),
            Some(answer) if answer.is(UNDO) => {
                picked.pop();
            }
            _ => break,
        }
    }
    let polylines: Vec<Entity> = picked.iter().flatten().map(|l| fitted_outline(l)).collect();
    let count = polylines.len();
    if count > 0 {
        io.step(Step::Drawn(polylines));
    }
    io.step(Step::Report(format!("BOUNDARY created {count} polyline{}", if count == 1 { "" } else { "s" })));
}

/// A picked boundary drawn while more are picked.
fn outline(corners: &[Point]) -> Entity {
    let vertices = corners.iter().map(|p| Vertex { x: p.x, y: p.y, bulge: 0.0, start_width: 0.0, end_width: 0.0 }).collect();
    Entity::new(Shape::Polyline { vertices, closed: true })
}

/// A boundary as BOUNDARY makes it, where its edge follows a circle or arc as one arc segment.
fn fitted_outline(corners: &[Point]) -> Entity {
    Entity::new(Shape::Polyline { vertices: fit_arcs(corners, true), closed: true })
}
