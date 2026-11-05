use std::cell::RefCell;
use std::rc::Rc;

use super::defaults::Defaults;
use super::format_general;
use super::prompts::{Answer, Io, Picked, Prompt, PromptKind, Step};
use crate::model::entities::{Entity, Shape};
use crate::model::fillet::fillet_lines;
use crate::model::geometry::Point;
use crate::model::offset::offset_entity;
use crate::model::transform::{Similarity, reflection, rotation, scaling, transform_entity, transform_shape, translation};
use crate::model::trimming::{extend_entity, trim_entity};

pub(super) const MAX_PREVIEW_ENTITIES: usize = 200; // a rubber band of thousands of entities would make the cursor lag
const UNDO: &str = "U";
const YES: &str = "Y";
const NO: &str = "N";
const RADIUS: &str = "R";
const TRIM_PROMPT: &str = "Select object to trim or shift-select to extend or [Undo]:";
const EXTEND_PROMPT: &str = "Select object to extend or shift-select to trim or [Undo]:";

/// The objects with new values, keeping who they are.
fn changes(objects: &[Entity], sim: &Similarity) -> Vec<Entity> {
    objects.iter().map(|e| e.with_shape(transform_shape(&e.shape, sim))).collect()
}

pub(super) fn copies(objects: &[Entity], sim: &Similarity) -> Vec<Entity> {
    objects.iter().map(|e| transform_entity(e, sim)).collect()
}

fn preview<F: Fn(Point) -> Similarity + 'static>(objects: &[Entity], sim_for: F) -> impl Fn(Point) -> Vec<Entity> + use<F> {
    let shown: Vec<Entity> = objects.iter().take(MAX_PREVIEW_ENTITIES).cloned().collect();
    move |cursor| copies(&shown, &sim_for(cursor))
}

fn shift_from(base: Point) -> impl Fn(Point) -> Similarity {
    move |cursor| translation(cursor.x - base.x, cursor.y - base.y)
}

/// The objects to work on: the ones already selected, otherwise whatever the user selects now.
pub(super) async fn objects_to_edit(io: &Io, selected: Vec<Entity>) -> Vec<Entity> {
    if !selected.is_empty() {
        return selected;
    }
    match io.ask(Prompt::new(PromptKind::Select, "Select objects:")).await {
        Some(Answer::Objects(chosen)) => chosen,
        _ => vec![],
    }
}

pub(super) async fn base_point(io: &Io) -> Option<Point> {
    match io.ask(Prompt::point("Specify base point:")).await {
        Some(Answer::Point(base)) => Some(base),
        _ => None,
    }
}

pub async fn erase(io: Io, selected: Vec<Entity>, defaults: Rc<RefCell<Defaults>>) {
    let objects = objects_to_edit(&io, selected).await;
    if !objects.is_empty() {
        defaults.borrow_mut().last_erased = objects.clone();
        io.step(Step::Remove(objects));
    }
}

/// OOPS: brings back the objects the last ERASE or BLOCK took out, as one undo step, and forgets them.
pub async fn oops(io: Io, defaults: Rc<RefCell<Defaults>>) {
    let objects = std::mem::take(&mut defaults.borrow_mut().last_erased);
    if objects.is_empty() {
        io.step(Step::Report("Nothing to restore.".into()));
    } else {
        io.step(Step::Commit(objects));
    }
}

pub async fn move_command(io: Io, selected: Vec<Entity>) {
    let objects = objects_to_edit(&io, selected).await;
    if objects.is_empty() {
        return;
    }
    let Some(base) = base_point(&io).await else { return };
    let prompt = Prompt::point("Specify second point:").base(base).preview(preview(&objects, shift_from(base)));
    if let Some(Answer::Point(target)) = io.ask(prompt).await {
        io.step(Step::Modify(changes(&objects, &shift_from(base)(target))));
    }
}

pub async fn copy(io: Io, selected: Vec<Entity>) {
    let objects = objects_to_edit(&io, selected).await;
    if objects.is_empty() {
        return;
    }
    let Some(base) = base_point(&io).await else { return };
    let mut copies_made = 0;
    loop {
        let prompt =
            Prompt::point("Specify second point or [Undo]:").base(base).options(&[UNDO]).preview(preview(&objects, shift_from(base)));
        match io.ask(prompt).await {
            Some(Answer::Keyword(_)) => {
                if copies_made > 0 {
                    io.step(Step::Retract);
                    copies_made -= 1;
                }
            }
            Some(Answer::Point(target)) => {
                io.step(Step::Commit(copies(&objects, &shift_from(base)(target))));
                copies_made += 1;
            }
            _ => return,
        }
    }
}

pub async fn rotate(io: Io, selected: Vec<Entity>) {
    let objects = objects_to_edit(&io, selected).await;
    if objects.is_empty() {
        return;
    }
    let Some(base) = base_point(&io).await else { return };
    let prompt = Prompt::new(PromptKind::Angle, "Specify rotation angle:")
        .base(base)
        .preview(preview(&objects, move |cursor| rotation(base, base.angle_to(cursor))));
    if let Some(Answer::Number(angle)) = io.ask(prompt).await {
        io.step(Step::Modify(changes(&objects, &rotation(base, angle))));
    }
}

pub async fn scale(io: Io, selected: Vec<Entity>) {
    let objects = objects_to_edit(&io, selected).await;
    if objects.is_empty() {
        return;
    }
    let Some(base) = base_point(&io).await else { return };
    let shown: Vec<Entity> = objects.iter().take(MAX_PREVIEW_ENTITIES).cloned().collect();
    let prompt = Prompt::new(PromptKind::Number, "Specify scale factor:").base(base).preview(move |cursor| {
        let factor = base.dist(cursor);
        if factor > 0.0 { copies(&shown, &scaling(base, factor)) } else { vec![] }
    });
    if let Some(Answer::Number(factor)) = io.ask(prompt).await {
        io.step(Step::Modify(changes(&objects, &scaling(base, factor))));
    }
}

pub async fn mirror(io: Io, selected: Vec<Entity>) {
    let objects = objects_to_edit(&io, selected).await;
    if objects.is_empty() {
        return;
    }
    let Some(Answer::Point(first)) = io.ask(Prompt::point("Specify first point of mirror line:")).await else { return };
    let mut message = "Specify second point of mirror line:".to_string();
    let second = loop {
        let shown: Vec<Entity> = objects.iter().take(MAX_PREVIEW_ENTITIES).cloned().collect();
        let prompt = Prompt::point(message.clone())
            .base(first)
            .preview(move |cursor| if cursor != first { copies(&shown, &reflection(first, cursor)) } else { vec![] });
        let Some(Answer::Point(second)) = io.ask(prompt).await else { return };
        if second != first {
            break second;
        }
        message = "The mirror line needs two different points. Specify second point of mirror line:".into();
    };
    let prompt = Prompt::point("Erase source objects? [Yes/No] <N>:").options(&[YES, NO]).default(Answer::keyword(NO));
    let sim = reflection(first, second);
    match io.ask(prompt).await {
        Some(answer) if answer.is(YES) => io.step(Step::Modify(changes(&objects, &sim))),
        Some(answer) if answer.is(NO) => io.step(Step::Commit(copies(&objects, &sim))),
        _ => {}
    }
}

pub async fn offset(io: Io) {
    let Some(Answer::Number(distance)) = io.ask(Prompt::new(PromptKind::Number, "Specify offset distance:")).await else { return };
    let mut message = "Select object to offset or <exit>:".to_string();
    loop {
        let Some(Answer::Picked(picked)) = io.ask(Prompt::new(PromptKind::Pick, message.clone())).await else { return };
        let entity = picked.entity;
        let shown = entity.clone();
        let prompt = Prompt::point("Specify point on side to offset:")
            .preview(move |cursor| offset_entity(&shown, distance, cursor).into_iter().collect());
        let Some(Answer::Point(side)) = io.ask(prompt).await else { return };
        match offset_entity(&entity, distance, side) {
            None => message = "That object cannot be offset by that distance. Select object to offset or <exit>:".into(),
            Some(result) => {
                io.step(Step::Commit(vec![result]));
                message = "Select object to offset or <exit>:".into();
            }
        }
    }
}

/// The objects to cut or extend against: the ones already selected, otherwise chosen now (Enter takes all).
async fn boundary_objects(io: &Io, selected: Vec<Entity>, message: &str) -> Vec<Entity> {
    if !selected.is_empty() {
        return selected;
    }
    match io.ask(Prompt::new(PromptKind::Select, message).or_all()).await {
        Some(Answer::Objects(chosen)) => chosen,
        _ => vec![],
    }
}

/// The step that trims the picked object, and the boundaries after it (the object's pieces replace it).
fn trim_step(picked: &Picked, boundaries: &[Entity]) -> Option<(Step, Vec<Entity>)> {
    let target = &picked.entity;
    let pieces = trim_entity(target, picked.point, boundaries)?;
    let step = if pieces.is_empty() {
        Step::Remove(vec![target.clone()])
    } else {
        Step::Compound(vec![Step::Remove(vec![target.clone()]), Step::Commit(pieces.clone())])
    };
    if !boundaries.iter().any(|e| e.id == target.id) {
        return Some((step, boundaries.to_vec()));
    }
    let mut after: Vec<Entity> = boundaries.iter().filter(|e| e.id != target.id).cloned().collect();
    after.extend(pieces);
    Some((step, after))
}

/// The step that extends the picked object, and the boundaries with the object at its new length.
fn extend_step(picked: &Picked, boundaries: &[Entity]) -> Option<(Step, Vec<Entity>)> {
    let longer = extend_entity(&picked.entity, picked.point, boundaries)?;
    let after = boundaries.iter().map(|e| if e.id == longer.id { longer.clone() } else { e.clone() }).collect();
    Some((Step::Modify(vec![longer]), after))
}

/// Pick objects one after another. Holding Shift does the opposite of the command.
async fn trim_or_extend(io: &Io, mut boundaries: Vec<Entity>, extend_by_default: bool) {
    let prompt = if extend_by_default { EXTEND_PROMPT } else { TRIM_PROMPT };
    let mut message = prompt.to_string();
    let mut earlier: Vec<Vec<Entity>> = vec![];
    loop {
        let picked = match io.ask(Prompt::new(PromptKind::Pick, message.clone()).options(&[UNDO])).await {
            Some(Answer::Picked(picked)) => picked,
            Some(Answer::Keyword(_)) => {
                if let Some(before) = earlier.pop() {
                    io.step(Step::Retract);
                    boundaries = before;
                }
                continue;
            }
            _ => return,
        };
        let extending = extend_by_default != picked.shift;
        let outcome = if extending { extend_step(&picked, &boundaries) } else { trim_step(&picked, &boundaries) };
        let Some((step, after)) = outcome else {
            let reason = if extending { "No boundary edge is in the way." } else { "Nothing to trim against there." };
            message = format!("{reason} {prompt}");
            continue;
        };
        earlier.push(std::mem::replace(&mut boundaries, after));
        message = prompt.to_string();
        io.step(step);
    }
}

pub async fn trim(io: Io, selected: Vec<Entity>) {
    let boundaries = boundary_objects(&io, selected, "Select cutting edges or <select all>:").await;
    if !boundaries.is_empty() {
        trim_or_extend(&io, boundaries, false).await;
    }
}

pub async fn extend(io: Io, selected: Vec<Entity>) {
    let boundaries = boundary_objects(&io, selected, "Select boundary edges or <select all>:").await;
    if !boundaries.is_empty() {
        trim_or_extend(&io, boundaries, true).await;
    }
}

fn fillet_step(first: &Picked, second: &Picked, radius: f64) -> Result<Step, String> {
    let is_line = |p: &Picked| matches!(p.entity.shape, Shape::Line { .. });
    if !is_line(first) || !is_line(second) {
        return Err("Fillet works on lines only for now.".into());
    }
    if first.entity.id == second.entity.id {
        return Err("Select two different lines.".into());
    }
    let result = fillet_lines(&first.entity, first.point, &second.entity, second.point, radius)?;
    let modify = Step::Modify(vec![result.first, result.second]);
    Ok(match result.arc {
        None => modify,
        Some(arc) => Step::Compound(vec![modify, Step::Commit(vec![arc])]),
    })
}

pub async fn fillet(io: Io, defaults: Rc<RefCell<Defaults>>) {
    let first_prompt = |radius: f64| format!("Select first line (radius {}) or [Radius]:", format_general(radius));
    let mut message = first_prompt(defaults.borrow().fillet_radius);
    loop {
        let first = match io.ask(Prompt::new(PromptKind::Pick, message.clone()).options(&[RADIUS])).await {
            Some(Answer::Picked(picked)) => picked,
            Some(Answer::Keyword(_)) => {
                let current = defaults.borrow().fillet_radius;
                let prompt = Prompt::new(PromptKind::Length, format!("Specify fillet radius <{}>:", format_general(current)))
                    .default(Answer::Number(current));
                if let Some(Answer::Number(radius)) = io.ask(prompt).await {
                    defaults.borrow_mut().fillet_radius = radius;
                }
                message = first_prompt(defaults.borrow().fillet_radius);
                continue;
            }
            _ => return,
        };
        let Some(Answer::Picked(second)) = io.ask(Prompt::new(PromptKind::Pick, "Select second line:")).await else { return };
        match fillet_step(&first, &second, defaults.borrow().fillet_radius) {
            Ok(step) => {
                io.step(step);
                return;
            }
            Err(error) => message = format!("{error} Select first line or [Radius]:"),
        }
    }
}
