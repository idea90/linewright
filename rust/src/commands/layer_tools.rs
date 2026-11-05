//! AutoCAD's layer tools on the command line: LAYISO, LAYUNISO, LAYOFF, LAYON, LAYFRZ, LAYTHW, LAYMCUR, LAYLCK
//! and LAYULK. Also the linetype scale commands LTSCALE, PSLTSCALE and CELTSCALE.

use std::cell::RefCell;
use std::rc::Rc;

use super::defaults::Defaults;
use super::edit::objects_to_edit;
use super::prompts::{Answer, Io, Prompt, PromptKind, ScaleSetting, Step};
use crate::model::document::Layer;
use crate::model::entities::Entity;

fn layers_of(defaults: &Rc<RefCell<Defaults>>) -> (Vec<Layer>, String) {
    let defaults = defaults.borrow();
    (defaults.layers.clone(), defaults.current_layer.clone())
}

fn named<'a>(layers: &'a mut [Layer], name: &str) -> Option<&'a mut Layer> {
    layers.iter_mut().find(|l| l.name == name)
}

/// The layers of objects picked one at a time, until Enter.
async fn pick_layers(io: &Io, message: &str, mut each: impl FnMut(&Io, &str)) {
    while let Some(Answer::Picked(picked)) = io.ask(Prompt::new(PromptKind::Pick, message)).await {
        each(io, &picked.entity.layer);
    }
}

/// LAYISO: only the layers of the selected objects stay on. LAYUNISO puts the others back.
pub async fn isolate(io: Io, selected: Vec<Entity>, defaults: Rc<RefCell<Defaults>>) {
    let objects = objects_to_edit(&io, selected).await;
    if objects.is_empty() {
        return;
    }
    let (before, current) = layers_of(&defaults);
    let keep: Vec<&str> = objects.iter().map(|e| e.layer.as_str()).collect();
    let layers: Vec<Layer> = before.iter().map(|l| Layer { visible: l.visible && keep.contains(&l.name.as_str()), ..l.clone() }).collect();
    let current = if keep.contains(&current.as_str()) { current } else { keep[0].to_string() };
    defaults.borrow_mut().isolated_from = Some(before);
    let mut names: Vec<&str> = keep.clone();
    names.sort();
    names.dedup();
    io.step(Step::Layers { layers, current: Some(current) });
    io.step(Step::Report(format!("Isolated layer(s): {}", names.join(", "))));
}

pub async fn unisolate(io: Io, defaults: Rc<RefCell<Defaults>>) {
    let Some(before) = defaults.borrow_mut().isolated_from.take() else {
        io.step(Step::Report("No layers were isolated with LAYISO.".into()));
        return;
    };
    let (now, _) = layers_of(&defaults);
    // Only what LAYISO turned off comes back; other changes since are kept.
    let layers = now.into_iter().map(|l| match before.iter().find(|b| b.name == l.name) {
        Some(b) => Layer { visible: b.visible || l.visible, ..l },
        None => l,
    });
    io.step(Step::Layers { layers: layers.collect(), current: None });
    io.step(Step::Report("Layers are as they were before LAYISO.".into()));
}

/// LAYOFF: pick objects to turn their layers off. The current layer stays on.
pub async fn layer_off(io: Io, defaults: Rc<RefCell<Defaults>>) {
    let (mut layers, current) = layers_of(&defaults);
    pick_layers(&io, "Select an object on the layer to be turned off:", |io, name| {
        if name == current {
            io.step(Step::Report(format!("Layer \"{name}\" is current and stays on.")));
            return;
        }
        if let Some(layer) = named(&mut layers, name) {
            layer.visible = false;
            io.step(Step::Layers { layers: layers.clone(), current: None });
            io.step(Step::Report(format!("Layer \"{name}\" has been turned off.")));
        }
    })
    .await;
}

/// LAYON: every layer on.
pub async fn layer_on(io: Io, defaults: Rc<RefCell<Defaults>>) {
    let (layers, _) = layers_of(&defaults);
    io.step(Step::Layers { layers: layers.into_iter().map(|l| Layer { visible: true, ..l }).collect(), current: None });
    io.step(Step::Report("All layers have been turned on.".into()));
}

/// LAYMCUR: the picked object's layer becomes current.
pub async fn make_current(io: Io, defaults: Rc<RefCell<Defaults>>) {
    let (layers, _) = layers_of(&defaults);
    if let Some(Answer::Picked(picked)) = io.ask(Prompt::new(PromptKind::Pick, "Select object whose layer will become current:")).await {
        let name = picked.entity.layer.clone();
        io.step(Step::Layers { layers, current: Some(name.clone()) });
        io.step(Step::Report(format!("Layer \"{name}\" is now current.")));
    }
}

/// LAYLCK: pick objects to lock their layers.
pub async fn lock(io: Io, defaults: Rc<RefCell<Defaults>>) {
    let (mut layers, _) = layers_of(&defaults);
    pick_layers(&io, "Select an object on the layer to be locked:", |io, name| {
        if let Some(layer) = named(&mut layers, name) {
            layer.locked = true;
            io.step(Step::Layers { layers: layers.clone(), current: None });
            io.step(Step::Report(format!("Layer \"{name}\" has been locked.")));
        }
    })
    .await;
}

/// LAYULK: unlock a layer by name, since objects on a locked layer cannot be picked.
pub async fn unlock(io: Io, defaults: Rc<RefCell<Defaults>>) {
    let (mut layers, _) = layers_of(&defaults);
    let locked: Vec<String> = layers.iter().filter(|l| l.locked).map(|l| l.name.clone()).collect();
    if locked.is_empty() {
        io.step(Step::Report("No layer is locked.".into()));
        return;
    }
    let default = locked[0].clone();
    let prompt =
        Prompt::new(PromptKind::Text, format!("Enter layer to unlock ({}) <{default}>:", locked.join(", "))).default(Answer::Text(default));
    let Some(Answer::Text(name)) = io.ask(prompt).await else { return };
    let Some(name) = layers.iter().find(|l| l.name.eq_ignore_ascii_case(name.trim())).map(|l| l.name.clone()) else {
        io.step(Step::Report(format!("There is no layer named \"{}\".", name.trim())));
        return;
    };
    if let Some(layer) = named(&mut layers, &name) {
        layer.locked = false;
    }
    io.step(Step::Layers { layers, current: None });
    io.step(Step::Report(format!("Layer \"{name}\" has been unlocked.")));
}

/// LAYFRZ: pick objects to freeze their layers. The current layer stays thawed.
pub async fn freeze(io: Io, defaults: Rc<RefCell<Defaults>>) {
    let (mut layers, current) = layers_of(&defaults);
    pick_layers(&io, "Select an object on the layer to be frozen:", |io, name| {
        if name == current {
            io.step(Step::Report(format!("Layer \"{name}\" is current and cannot be frozen.")));
            return;
        }
        if let Some(layer) = named(&mut layers, name) {
            layer.frozen = true;
            io.step(Step::Layers { layers: layers.clone(), current: None });
            io.step(Step::Report(format!("Layer \"{name}\" has been frozen.")));
        }
    })
    .await;
}

/// LAYTHW: every layer thawed.
pub async fn thaw(io: Io, defaults: Rc<RefCell<Defaults>>) {
    let (layers, _) = layers_of(&defaults);
    io.step(Step::Layers { layers: layers.into_iter().map(|l| Layer { frozen: false, ..l }).collect(), current: None });
    io.step(Step::Report("All layers have been thawed.".into()));
}

/// LTSCALE: the drawing's linetype scale, which multiplies every linetype's dashes.
pub async fn ltscale_command(io: Io, defaults: Rc<RefCell<Defaults>>) {
    let current = defaults.borrow().ltscale;
    let prompt =
        Prompt::new(PromptKind::Number, format!("Enter new linetype scale factor <{current:.4}>:")).default(Answer::Number(current));
    if let Some(Answer::Number(scale)) = io.ask(prompt).await {
        io.step(Step::LinetypeScale(ScaleSetting::Global(scale)));
    }
}

/// PSLTSCALE: whether model space objects in viewports keep their dashes as long on the sheet as paper space ones.
pub async fn psltscale_command(io: Io, defaults: Rc<RefCell<Defaults>>) {
    let current = defaults.borrow().psltscale;
    let prompt = Prompt::new(PromptKind::Number, format!("Enter new value for PSLTSCALE <{}>:", u8::from(current)))
        .options(&["0", "1"])
        .default(Answer::keyword(if current { "1" } else { "0" }));
    let on = match io.ask(prompt).await {
        Some(answer) if answer.is("0") => false,
        Some(answer) if answer.is("1") => true,
        _ => return,
    };
    io.step(Step::LinetypeScale(ScaleSetting::Paper(on)));
}

/// CELTSCALE: the linetype scale new objects get.
pub async fn celtscale_command(io: Io, defaults: Rc<RefCell<Defaults>>) {
    let current = defaults.borrow().celtscale;
    let prompt = Prompt::new(PromptKind::Number, format!("Enter new value for CELTSCALE <{current:.4}>:")).default(Answer::Number(current));
    if let Some(Answer::Number(scale)) = io.ask(prompt).await {
        io.step(Step::LinetypeScale(ScaleSetting::Current(scale)));
    }
}
