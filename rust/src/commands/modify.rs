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
use crate::model::overkill::{OverkillSettings, overkill};
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

/// The most items an array may have, AutoCAD's MAXARRAY.
const MAX_ARRAY_ITEMS: usize = 100_000;
/// The most objects one ARRAY may make, however few items hold them.
const MAX_ARRAY_OBJECTS: usize = 1_000_000;

/// Whether an array of so many items of these objects is small enough to make; it says so when it is not.
fn array_fits(io: &Io, items: usize, objects: &[Entity]) -> bool {
    let fits = items <= MAX_ARRAY_ITEMS && items.saturating_mul(objects.len()) <= MAX_ARRAY_OBJECTS;
    if !fits {
        io.step(Step::Report(format!(
            "Too many elements in the array: the most is {MAX_ARRAY_ITEMS} items and {MAX_ARRAY_OBJECTS} objects."
        )));
    }
    fits
}

async fn rectangular_array(io: &Io, objects: &[Entity]) {
    let size = extent_of(objects).map_or((1.0, 1.0), |b| (b.width().max(1e-9), b.height().max(1e-9)));
    let Some(rows) = count(io, "Enter the number of rows", 3.0).await else { return };
    let Some(columns) = count(io, "Enter the number of columns", 4.0).await else { return };
    if !array_fits(io, rows.saturating_mul(columns), objects) {
        return;
    }
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
    if !array_fits(io, items, objects) {
        return;
    }
    let prompt = Prompt::new(PromptKind::Angle, "Specify the angle to fill (+=ccw, -=cw) <360>:").turn().default(Answer::Number(360.0));
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

const DONE: &str = "D";
const IGNORE: &str = "I";
const TOLERANCE: &str = "T";
const PARTIAL: &str = "P";
const END_TO_END: &str = "E";
const OPTIMIZE: &str = "O";
const YES: &str = "Y";
const NO: &str = "N";
const OK: &str = "OK";

/// OVERKILL in the window: the Delete Duplicate Objects dialog gives the options, and the clean-up runs with them.
/// The window shows the dialog for the prompt and saves its choices to the defaults before answering. Typed where
/// there is no window, Enter takes the options remembered.
pub async fn overkill_dialog_command(io: Io, selected: Vec<Entity>, defaults: Rc<RefCell<Defaults>>) {
    let objects = objects_to_edit(&io, selected).await;
    if objects.is_empty() {
        return;
    }
    let prompt = Prompt::new(PromptKind::OverkillDialog, "Delete Duplicate Objects").default(Answer::keyword(OK));
    if io.ask(prompt).await.is_none() {
        return;
    }
    let settings = defaults.borrow().overkill.clone();
    clean_up(&io, &objects, &settings);
}

/// -OVERKILL: deletes copies lying on top of other objects and combines lines and arcs that overlap, after the
/// options are set as the command line asks for them.
pub async fn overkill_command(io: Io, selected: Vec<Entity>, defaults: Rc<RefCell<Defaults>>) {
    let objects = objects_to_edit(&io, selected).await;
    if objects.is_empty() {
        return;
    }
    let mut settings = defaults.borrow().overkill.clone();
    loop {
        io.step(Step::Report(overkill_status(&settings)));
        let prompt =
            Prompt::point("Enter an option to change [Done/Ignore/Tolerance/Partial overlap/End to end/Optimize polylines] <Done>:")
                .options(&[DONE, IGNORE, TOLERANCE, PARTIAL, END_TO_END, OPTIMIZE])
                .default(Answer::keyword(DONE));
        match io.ask(prompt).await {
            Some(answer) if answer.is(DONE) => break,
            Some(answer) if answer.is(IGNORE) => {
                let prompt = Prompt::point("Ignore property [None/All/Color/LAyer/LType/LWeight] <None>:")
                    .options(&["N", "A", "C", "LA", "LT", "LW"])
                    .default(Answer::keyword("N"));
                let Some(Answer::Keyword(which)) = io.ask(prompt).await else { continue };
                let all = which == "A";
                match which.as_str() {
                    "C" => settings.ignore_color = !settings.ignore_color,
                    "LA" => settings.ignore_layer = !settings.ignore_layer,
                    "LT" => settings.ignore_linetype = !settings.ignore_linetype,
                    "LW" => settings.ignore_lineweight = !settings.ignore_lineweight,
                    _ => {
                        settings.ignore_color = all;
                        settings.ignore_layer = all;
                        settings.ignore_linetype = all;
                        settings.ignore_lineweight = all;
                    }
                }
            }
            Some(answer) if answer.is(TOLERANCE) => {
                let prompt = Prompt::new(PromptKind::Length, format!("Specify tolerance <{}>:", format_general(settings.tolerance)))
                    .default(Answer::Number(settings.tolerance));
                if let Some(Answer::Number(tolerance)) = io.ask(prompt).await {
                    settings.tolerance = tolerance;
                }
            }
            Some(answer) if answer.is(OPTIMIZE) => {
                let current = settings.optimize_polylines;
                let prompt = Prompt::point(format!("Optimize polylines? [Yes/No] <{}>:", yes_no(current)))
                    .options(&[YES, NO])
                    .default(Answer::keyword(if current { YES } else { NO }));
                if let Some(Answer::Keyword(choice)) = io.ask(prompt).await {
                    settings.optimize_polylines = choice == YES;
                }
            }
            Some(answer) if answer.is(PARTIAL) || answer.is(END_TO_END) => {
                let partial = answer.is(PARTIAL);
                let current = if partial { settings.combine_partial } else { settings.combine_end_to_end };
                let question = if partial { "that partially overlap" } else { "when aligned end to end" };
                let prompt = Prompt::point(format!("Combine co-linear objects {question}? [Yes/No] <{}>:", yes_no(current)))
                    .options(&[YES, NO])
                    .default(Answer::keyword(if current { YES } else { NO }));
                if let Some(Answer::Keyword(choice)) = io.ask(prompt).await {
                    let flag = if partial { &mut settings.combine_partial } else { &mut settings.combine_end_to_end };
                    *flag = choice == YES;
                }
            }
            Some(_) => {}
            None => return,
        }
    }
    defaults.borrow_mut().overkill = settings.clone();
    clean_up(&io, &objects, &settings);
}

/// Runs the clean-up with these settings, and makes its changes as one undo step.
fn clean_up(io: &Io, objects: &[Entity], settings: &OverkillSettings) {
    let cleanup = overkill(objects, settings);
    let mut steps = vec![];
    if !cleanup.remove.is_empty() {
        steps.push(Step::Remove(cleanup.remove));
    }
    if !cleanup.modify.is_empty() {
        steps.push(Step::Modify(cleanup.modify));
    }
    if !steps.is_empty() {
        io.step(Step::Compound(steps));
    }
    io.step(Step::Report(format!(
        "{} duplicate(s) deleted\n{} overlapping object(s) or segment(s) deleted",
        cleanup.duplicates, cleanup.overlaps
    )));
}

fn yes_no(value: bool) -> &'static str {
    if value { "Yes" } else { "No" }
}

fn overkill_status(settings: &OverkillSettings) -> String {
    let ignored: Vec<&str> = [
        (settings.ignore_color, "Color"),
        (settings.ignore_layer, "Layer"),
        (settings.ignore_linetype, "Linetype"),
        (settings.ignore_lineweight, "Lineweight"),
    ]
    .into_iter()
    .filter_map(|(on, name)| on.then_some(name))
    .collect();
    format!(
        "Current settings: Tolerance={}, Ignore={}, Combine partial overlap={}, Combine end to end={}, Optimize polylines={}",
        format_general(settings.tolerance),
        if ignored.is_empty() { "None".to_string() } else { ignored.join(",") },
        yes_no(settings.combine_partial),
        yes_no(settings.combine_end_to_end),
        yes_no(settings.optimize_polylines)
    )
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
            linetype_scale: source.linetype_scale,
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
