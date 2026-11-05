//! LAYOUT and MVIEW: paper layouts and the viewports on them.

use std::cell::RefCell;
use std::rc::Rc;

use super::defaults::Defaults;
use super::prompts::{Answer, Io, LayoutRequest, Prompt, PromptKind, Step};
use crate::model::entities::{Entity, Shape};
use crate::model::geometry::Point;
use crate::model::index::Bounds;

const NEW: &str = "N";
const RENAME: &str = "R";
const DELETE: &str = "D";
const SET: &str = "S";
const LIST: &str = "?";
const FIT: &str = "F";
const LOCK: &str = "L";
const ON: &str = "ON";
const FREEZE: &str = "F";
const THAW: &str = "T";
const RESET: &str = "R";
const ALL: &str = "A";
const SELECT: &str = "S";
const CURRENT: &str = "C";
const OFF: &str = "OFF";
const MODEL: &str = "Model";

/// The layout a typed name means: an index, None for model space. Names match whatever their case.
fn find(layouts: &[String], name: &str) -> Option<Option<usize>> {
    if name.eq_ignore_ascii_case(MODEL) {
        return Some(None);
    }
    layouts.iter().position(|l| l.eq_ignore_ascii_case(name.trim())).map(Some)
}

async fn ask_name(io: &Io, message: String, default: String) -> Option<String> {
    match io.ask(Prompt::new(PromptKind::Text, message).default(Answer::Text(default))).await {
        Some(Answer::Text(name)) if !name.trim().is_empty() => Some(name.trim().to_string()),
        _ => None,
    }
}

/// LAYOUT, as AutoCAD's command line version: New, Rename, Delete, Set (open one, or Model) and ? to list.
pub async fn layout(io: Io, defaults: Rc<RefCell<Defaults>>) {
    let (layouts, active) = (defaults.borrow().layouts.clone(), defaults.borrow().active_layout);
    let current = active.map_or(MODEL.to_string(), |i| layouts[i].clone());
    let prompt = Prompt::point("Enter layout option [New/Rename/Delete/Set/?] <Set>:")
        .options(&[NEW, RENAME, DELETE, SET, LIST])
        .default(Answer::keyword(SET));
    let missing = |name: &str| Step::Report(format!("Layout \"{name}\" not found."));
    match io.ask(prompt).await {
        Some(answer) if answer.is(LIST) => {
            io.step(Step::Report(format!("Layouts: {}", std::iter::once(MODEL.to_string()).chain(layouts).collect::<Vec<_>>().join(", "))));
        }
        Some(answer) if answer.is(NEW) => {
            let free = (1..).map(|n| format!("Layout{n}")).find(|n| find(&layouts, n).is_none()).unwrap_or_default();
            let Some(name) = ask_name(&io, format!("Enter name of new layout <{free}>:"), free).await else { return };
            io.step(Step::Layout(LayoutRequest::New(name)));
        }
        Some(answer) if answer.is(RENAME) => {
            let Some(old) = ask_name(&io, format!("Enter layout to rename <{current}>:"), current).await else { return };
            let Some(Some(index)) = find(&layouts, &old) else {
                io.step(missing(&old));
                return;
            };
            let Some(new) = ask_name(&io, "Enter new layout name:".into(), String::new()).await else { return };
            io.step(Step::Layout(LayoutRequest::Rename(index, new)));
        }
        Some(answer) if answer.is(DELETE) => {
            let Some(name) = ask_name(&io, format!("Enter name of layout to delete <{current}>:"), current).await else { return };
            match find(&layouts, &name) {
                Some(Some(index)) => io.step(Step::Layout(LayoutRequest::Delete(index))),
                Some(None) => io.step(Step::Report("The Model tab cannot be deleted.".into())),
                None => io.step(missing(&name)),
            }
        }
        Some(answer) if answer.is(SET) => {
            let Some(name) = ask_name(&io, format!("Enter layout to make current <{current}>:"), current).await else { return };
            match find(&layouts, &name) {
                Some(space) => io.step(Step::Layout(LayoutRequest::Open(space))),
                None => io.step(missing(&name)),
            }
        }
        _ => {}
    }
}

/// A viewport filling `frame`, showing all of model space at the largest scale that fits.
pub fn viewport_onto(frame: Bounds, model: Option<Bounds>) -> Shape {
    let model = model.unwrap_or_else(|| Bounds::square_around(frame.center(), frame.height() / 2.0));
    let fit = (frame.width() / model.width().max(1e-9)).min(frame.height() / model.height().max(1e-9)) * 0.95;
    Shape::Viewport {
        center: frame.center(),
        width: frame.width(),
        height: frame.height(),
        view_center: model.center(),
        view_height: frame.height() / fit.max(1e-12),
        frozen: vec![],
        locked: false,
    }
}

/// The layers a typed list names: names separated by commas, each of which may use * and ? as wildcards.
fn named_layers(typed: &str, layers: &[crate::model::document::Layer]) -> Vec<String> {
    let patterns: Vec<String> = typed.split(',').map(|p| p.trim().to_lowercase()).filter(|p| !p.is_empty()).collect();
    layers
        .iter()
        .filter(|layer| patterns.iter().any(|p| crate::model::find::wildcard_match(p, &layer.name.to_lowercase())))
        .map(|layer| layer.name.clone())
        .collect()
}

/// VPLAYER: freeze or thaw layers in some of the layout's viewports only, so each viewport can show its own part of
/// model space. Through a viewport, Current means that one; on paper space, viewports are picked.
pub async fn vplayer(io: Io, defaults: Rc<RefCell<Defaults>>) {
    let (mut viewports, through, layers) = {
        let d = defaults.borrow();
        (d.viewports.clone(), d.through_viewport, d.layers.clone())
    };
    if viewports.is_empty() {
        io.step(Step::Report("VPLAYER works on a layout with viewports. Make one with MVIEW.".into()));
        return;
    }
    loop {
        let prompt = Prompt::new(PromptKind::Text, "Enter an option [?/Freeze/Thaw/Reset]:").options(&[LIST, FREEZE, THAW, RESET]);
        let option = match io.ask(prompt).await {
            Some(Answer::Keyword(k)) => k,
            _ => return,
        };
        if option == LIST {
            for (number, viewport) in viewports.iter().enumerate() {
                if let Shape::Viewport { frozen, .. } = &viewport.shape {
                    let shown = if frozen.is_empty() { "none".to_string() } else { frozen.join(", ") };
                    let mark = if Some(viewport.id) == through { " (current)" } else { "" };
                    io.step(Step::Report(format!("Viewport {}{mark}: frozen layers {shown}", number + 1)));
                }
            }
            continue;
        }
        let verb = if option == FREEZE { "freeze" } else { "thaw" };
        let typed = match io.ask(Prompt::new(PromptKind::Text, format!("Enter layer name(s) to {verb}:"))).await {
            Some(Answer::Text(t)) if !t.trim().is_empty() => t,
            _ => return,
        };
        let names = named_layers(&typed, &layers);
        if names.is_empty() {
            io.step(Step::Report(format!("No layer matches \"{}\".", typed.trim())));
            continue;
        }
        // Which viewports: through one, the current one or all; on paper space, all or those picked.
        let (options, default, message) = if through.is_some() {
            (vec![ALL, CURRENT], CURRENT, "Enter an option [All/Current] <Current>:")
        } else {
            (vec![ALL, SELECT], SELECT, "Enter an option [All/Select] <Select>:")
        };
        let which = match io.ask(Prompt::new(PromptKind::Text, message).options(&options).default(Answer::keyword(default))).await {
            Some(Answer::Keyword(k)) => k,
            _ => return,
        };
        let chosen: Vec<_> = if which == ALL {
            viewports.iter().map(|v| v.id).collect()
        } else if which == CURRENT {
            through.into_iter().collect()
        } else {
            match io.ask(Prompt::new(PromptKind::Select, "Select viewports:")).await {
                Some(Answer::Objects(picked)) => {
                    picked.iter().filter(|e| matches!(e.shape, Shape::Viewport { .. })).map(|e| e.id).collect()
                }
                _ => return,
            }
        };
        let mut changed = vec![];
        for viewport in viewports.iter_mut().filter(|v| chosen.contains(&v.id)) {
            let Shape::Viewport { frozen, .. } = &mut viewport.shape else { continue };
            let before = frozen.clone();
            if option == FREEZE {
                frozen.extend(names.iter().filter(|n| !before.iter().any(|f| f.eq_ignore_ascii_case(n))).cloned());
            } else {
                frozen.retain(|f| !names.iter().any(|n| n.eq_ignore_ascii_case(f)));
            }
            if *frozen != before {
                changed.push(viewport.clone());
            }
        }
        let done = if option == FREEZE { "frozen" } else { "thawed" };
        io.step(Step::Report(format!("{} layer(s) {done} in {} viewport(s).", names.len(), changed.len())));
        if !changed.is_empty() {
            io.step(Step::Modify(changed));
        }
    }
}

/// MVIEW's Lock: lock viewports' display, so zooming and panning through them leave their scale alone, or unlock.
async fn lock_viewports(io: &Io) {
    let prompt = Prompt::new(PromptKind::Text, "Viewport View Locking [ON/OFF]:").options(&[ON, OFF]);
    let lock = match io.ask(prompt).await {
        Some(Answer::Keyword(k)) => k == ON,
        Some(Answer::Text(t)) if t.trim().eq_ignore_ascii_case(ON) || t.trim().eq_ignore_ascii_case(OFF) => {
            t.trim().eq_ignore_ascii_case(ON)
        }
        _ => return,
    };
    let Some(Answer::Objects(picked)) = io.ask(Prompt::new(PromptKind::Select, "Select objects:")).await else { return };
    let changed: Vec<Entity> = picked
        .into_iter()
        .filter_map(|mut entity| match &mut entity.shape {
            Shape::Viewport { locked, .. } if *locked != lock => {
                *locked = lock;
                Some(entity)
            }
            _ => None,
        })
        .collect();
    let message = format!("{} viewport(s) {}.", changed.len(), if lock { "locked" } else { "unlocked" });
    if !changed.is_empty() {
        io.step(Step::Modify(changed));
    }
    io.step(Step::Report(message));
}

/// MVIEW: a viewport onto model space, between two corners or filling the sheet (Fit).
pub async fn mview(io: Io, defaults: Rc<RefCell<Defaults>>) {
    let (sheet, model) = (defaults.borrow().sheet, defaults.borrow().model_extent);
    let Some(sheet) = sheet else {
        io.step(Step::Report("MVIEW works on a layout. Open one from its tab, or with LAYOUT.".into()));
        return;
    };
    let prompt = Prompt::point("Specify corner of viewport or [Fit/Lock] <Fit>:").options(&[FIT, LOCK]).default(Answer::keyword(FIT));
    let frame = match io.ask(prompt).await {
        Some(Answer::Keyword(k)) if k == LOCK => {
            lock_viewports(&io).await;
            return;
        }
        Some(Answer::Point(first)) => {
            let preview = move |cursor: Point| vec![Entity::new(viewport_onto(Bounds::from_corners(first, cursor), model))];
            let prompt = Prompt::point("Specify opposite corner:").base(first).preview(preview);
            let Some(Answer::Point(second)) = io.ask(prompt).await else { return };
            if first.x == second.x || first.y == second.y {
                io.step(Step::Report("A viewport needs a width and a height.".into()));
                return;
            }
            Bounds::from_corners(first, second)
        }
        // The sheet, less a margin all round.
        Some(_) => sheet.grow(-sheet.width().min(sheet.height()) * 0.03),
        None => return,
    };
    io.step(Step::Drawn(vec![Entity::new(viewport_onto(frame, model))]));
}
