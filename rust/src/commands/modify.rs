//! More editing commands: explode, stretch, array, chamfer, break, join and match properties.

use std::cell::RefCell;
use std::rc::Rc;

use super::defaults::Defaults;
use super::edit::{MAX_PREVIEW_ENTITIES, base_point, copies, objects_to_edit};
use super::format_general;
use super::prompts::{Answer, Io, Picked, Prompt, PromptKind, Step};
use crate::model::document::DrawOrder;
use crate::model::editing::{explode, join, stretch};
use crate::model::entities::{Entity, Shape};
use crate::model::fillet::chamfer_lines;
use crate::model::geometry::Point;
use crate::model::index::{Bounds, entity_extent};
use crate::model::transform::{rotation, translation};
use crate::model::trimming::break_entity;

const RECTANGULAR: &str = "R";
const POLAR: &str = "P";
const DISTANCE: &str = "D";
const FIRST: &str = "F";

pub async fn explode_command(io: Io, selected: Vec<Entity>) {
    let objects = objects_to_edit(&io, selected).await;
    let (mut gone, mut pieces, mut kept) = (vec![], vec![], 0);
    for object in objects {
        match explode(&object) {
            Some(parts) => {
                pieces.extend(parts);
                gone.push(object);
            }
            None => kept += 1,
        }
    }
    if kept > 0 {
        io.step(Step::Report(format!("{kept} object(s) cannot be exploded.")));
    }
    if !gone.is_empty() {
        io.step(Step::Compound(vec![Step::Remove(gone), Step::Commit(pieces)]));
    }
}

pub async fn stretch_command(io: Io) {
    let Some(Answer::Point(first)) = io.ask(Prompt::point("Select objects to stretch by crossing-window. Specify first corner:")).await
    else {
        return;
    };
    let Some(Answer::Window { first, second, objects }) =
        io.ask(Prompt::new(PromptKind::Window, "Specify opposite corner:").base(first)).await
    else {
        return;
    };
    if objects.is_empty() {
        io.step(Step::Report("No objects found.".into()));
        return;
    }
    io.step(Step::Report(format!("{} found", objects.len())));
    let window = Bounds::from_corners(first, second);
    let Some(base) = base_point(&io).await else { return };
    let shown: Vec<Entity> = objects.iter().take(MAX_PREVIEW_ENTITIES).cloned().collect();
    let preview = move |cursor: Point| shown.iter().filter_map(|e| stretch(e, &window, cursor - base)).map(|e| e.duplicate()).collect();
    if let Some(Answer::Point(target)) = io.ask(Prompt::point("Specify second point:").base(base).preview(preview)).await {
        let changed: Vec<Entity> = objects.iter().filter_map(|e| stretch(e, &window, target - base)).collect();
        if !changed.is_empty() {
            io.step(Step::Modify(changed));
        }
    }
}

/// The objects' box, for a sensible spacing between copies.
fn extent_of(objects: &[Entity]) -> Option<Bounds> {
    objects.iter().map(entity_extent).filter(|b| b.x0.is_finite()).reduce(Bounds::union)
}

async fn count(io: &Io, message: &str, default: f64) -> Option<usize> {
    let prompt = Prompt::new(PromptKind::Number, format!("{message} <{}>:", format_general(default))).default(Answer::Number(default));
    match io.ask(prompt).await {
        Some(Answer::Number(n)) => Some(n.round().max(1.0) as usize),
        _ => None,
    }
}

async fn length(io: &Io, message: &str, default: f64) -> Option<f64> {
    let prompt = Prompt::new(PromptKind::Length, format!("{message} <{}>:", format_general(default))).default(Answer::Number(default));
    match io.ask(prompt).await {
        Some(Answer::Number(n)) => Some(n),
        _ => None,
    }
}

pub async fn array_command(io: Io, selected: Vec<Entity>) {
    let objects = objects_to_edit(&io, selected).await;
    if objects.is_empty() {
        return;
    }
    let kind = Prompt::point("Enter array type [Rectangular/Polar] <Rectangular>:")
        .options(&[RECTANGULAR, POLAR])
        .default(Answer::keyword(RECTANGULAR));
    match io.ask(kind).await {
        Some(answer) if answer.is(POLAR) => polar_array(&io, &objects).await,
        Some(answer) if answer.is(RECTANGULAR) => rectangular_array(&io, &objects).await,
        _ => {}
    }
}

async fn rectangular_array(io: &Io, objects: &[Entity]) {
    let size = extent_of(objects).map_or((1.0, 1.0), |b| (b.width().max(1e-9), b.height().max(1e-9)));
    let Some(rows) = count(io, "Enter the number of rows", 3.0).await else { return };
    let Some(columns) = count(io, "Enter the number of columns", 4.0).await else { return };
    let Some(row_spacing) = length(io, "Specify the distance between rows", size.1 * 1.5).await else { return };
    let Some(column_spacing) = length(io, "Specify the distance between columns", size.0 * 1.5).await else { return };
    let mut made = vec![];
    for row in 0..rows {
        for column in 0..columns {
            if row == 0 && column == 0 {
                continue;
            }
            made.extend(copies(objects, &translation(column as f64 * column_spacing, row as f64 * row_spacing)));
        }
    }
    if !made.is_empty() {
        io.step(Step::Commit(made));
    }
}

async fn polar_array(io: &Io, objects: &[Entity]) {
    let Some(Answer::Point(center)) = io.ask(Prompt::point("Specify center point of array:")).await else { return };
    let Some(items) = count(io, "Enter the number of items", 6.0).await else { return };
    let prompt = Prompt::new(PromptKind::Angle, "Specify the angle to fill (+=ccw, -=cw) <360>:").default(Answer::Number(360.0));
    let Some(Answer::Number(fill)) = io.ask(prompt).await else { return };
    // A full circle shares its turn among all the items; anything less puts the last item at the end of it.
    let step = if (fill.abs() - 360.0).abs() < 1e-9 { fill / items as f64 } else { fill / (items.max(2) - 1) as f64 };
    let made: Vec<Entity> = (1..items).flat_map(|i| copies(objects, &rotation(center, step * i as f64))).collect();
    if !made.is_empty() {
        io.step(Step::Commit(made));
    }
}

pub async fn chamfer_command(io: Io, defaults: Rc<RefCell<Defaults>>) {
    let message = |d: (f64, f64)| format!("Select first line (distances {}, {}) or [Distance]:", format_general(d.0), format_general(d.1));
    let mut prompt = message(defaults.borrow().chamfer_distances);
    loop {
        let first = match io.ask(Prompt::new(PromptKind::Pick, prompt.clone()).options(&[DISTANCE])).await {
            Some(Answer::Picked(picked)) => picked,
            Some(Answer::Keyword(_)) => {
                let (one, two) = defaults.borrow().chamfer_distances;
                let Some(one) = length(&io, "Specify first chamfer distance", one).await else { return };
                let Some(two) = length(&io, "Specify second chamfer distance", if two == 0.0 { one } else { two }).await else { return };
                defaults.borrow_mut().chamfer_distances = (one, two);
                prompt = message((one, two));
                continue;
            }
            _ => return,
        };
        let Some(Answer::Picked(second)) = io.ask(Prompt::new(PromptKind::Pick, "Select second line:")).await else { return };
        match chamfer_step(&first, &second, defaults.borrow().chamfer_distances) {
            Ok(step) => {
                io.step(step);
                return;
            }
            Err(error) => prompt = format!("{error} Select first line or [Distance]:"),
        }
    }
}

fn chamfer_step(first: &Picked, second: &Picked, distances: (f64, f64)) -> Result<Step, String> {
    let is_line = |p: &Picked| matches!(p.entity.shape, Shape::Line { .. });
    if !is_line(first) || !is_line(second) {
        return Err("Chamfer works on lines.".into());
    }
    if first.entity.id == second.entity.id {
        return Err("Select two different lines.".into());
    }
    let (a, b, bevel) = chamfer_lines(&first.entity, first.point, &second.entity, second.point, distances)?;
    let modify = Step::Modify(vec![a, b]);
    Ok(match bevel {
        None => modify,
        Some(line) => Step::Compound(vec![modify, Step::Commit(vec![line])]),
    })
}

pub async fn break_command(io: Io) {
    let Some(Answer::Picked(picked)) = io.ask(Prompt::new(PromptKind::Pick, "Select object:")).await else { return };
    let mut first = picked.point;
    let prompt = Prompt::point("Specify second break point or [First point]:").base(first).options(&[FIRST]);
    let second = match io.ask(prompt).await {
        Some(Answer::Point(point)) => point,
        Some(Answer::Keyword(_)) => {
            let Some(Answer::Point(point)) = io.ask(Prompt::point("Specify first break point:")).await else { return };
            first = point;
            let Some(Answer::Point(point)) = io.ask(Prompt::point("Specify second break point:").base(first)).await else { return };
            point
        }
        _ => return,
    };
    match break_entity(&picked.entity, first, second) {
        Some(pieces) if pieces.is_empty() => io.step(Step::Remove(vec![picked.entity])),
        Some(pieces) => io.step(Step::Compound(vec![Step::Remove(vec![picked.entity]), Step::Commit(pieces)])),
        None => io.step(Step::Report("That object cannot be broken.".into())),
    }
}

pub async fn join_command(io: Io, selected: Vec<Entity>) {
    let objects = objects_to_edit(&io, selected).await;
    match join(&objects) {
        Some((removed, added)) => {
            let message = format!("{} objects joined into {}.", removed.len(), added.len());
            io.step(Step::Compound(vec![Step::Remove(removed), Step::Commit(added)]));
            io.step(Step::Report(message));
        }
        None => io.step(Step::Report("Nothing to join: the objects must meet end to end.".into())),
    }
}

pub async fn matchprop_command(io: Io) {
    let Some(Answer::Picked(source)) = io.ask(Prompt::new(PromptKind::Pick, "Select source object:")).await else { return };
    let Some(Answer::Objects(targets)) = io.ask(Prompt::new(PromptKind::Select, "Select destination object(s):")).await else { return };
    let source = source.entity;
    let changed: Vec<Entity> = targets
        .into_iter()
        .filter(|t| t.id != source.id)
        .map(|t| Entity {
            layer: source.layer.clone(),
            color: source.color,
            linetype: source.linetype.clone(),
            lineweight: source.lineweight,
            ..t
        })
        .collect();
    if !changed.is_empty() {
        io.step(Step::Modify(changed));
    }
}

const ABOVE: &str = "A";
const UNDER: &str = "U";
const FRONT: &str = "F";
const BACK: &str = "B";

/// DRAWORDER: bring objects to the front, send them to the back, or put them just above or under others.
pub async fn draworder_command(io: Io, selected: Vec<Entity>) {
    let objects = objects_to_edit(&io, selected).await;
    if objects.is_empty() {
        return;
    }
    let prompt = Prompt::point("Enter object ordering option [Above objects/Under objects/Front/Back] <Back>:")
        .options(&[ABOVE, UNDER, FRONT, BACK])
        .default(Answer::keyword(BACK));
    let placement = match io.ask(prompt).await {
        Some(answer) if answer.is(FRONT) => DrawOrder::Front,
        Some(answer) if answer.is(BACK) => DrawOrder::Back,
        Some(answer) if answer.is(ABOVE) || answer.is(UNDER) => {
            let above = answer.is(ABOVE);
            let Some(Answer::Objects(reference)) = io.ask(Prompt::new(PromptKind::Select, "Select reference objects:")).await else {
                return;
            };
            let ids = reference.iter().map(|e| e.id).collect();
            if above { DrawOrder::Above(ids) } else { DrawOrder::Under(ids) }
        }
        _ => return,
    };
    io.step(Step::DrawOrder(objects, placement));
}
