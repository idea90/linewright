//! Creating, renaming and changing layers. Each change is one undo step; a refusal is a message for the user.

use super::controller::Controller;
use super::undo::{Change, LayerState};
use crate::model::document::Layer;
use crate::model::entities::{DEFAULT_LAYER, EntityId};

pub fn create(controller: &mut Controller) -> String {
    let document = controller.document();
    let mut number = 1;
    while document.layers.iter().any(|l| l.name.eq_ignore_ascii_case(&format!("layer{number}"))) {
        number += 1;
    }
    let name = format!("Layer{number}");
    let mut layers = document.layers.clone();
    layers.push(Layer::new(&name));
    push(controller, layers, None, None);
    name
}

pub fn rename(controller: &mut Controller, old: &str, new: &str) -> Result<(), String> {
    let new = new.trim();
    let document = controller.document();
    if old == DEFAULT_LAYER {
        return Err("Layer 0 cannot be renamed.".into());
    }
    if new.is_empty() {
        return Err("A layer needs a name.".into());
    }
    if new == old {
        return Ok(());
    }
    if document.layers.iter().any(|l| l.name.eq_ignore_ascii_case(new) && l.name != old) {
        return Err(format!("A layer called {new} already exists."));
    }
    let layers =
        document.layers.iter().map(|l| if l.name == old { Layer { name: new.to_string(), ..l.clone() } } else { l.clone() }).collect();
    let affected: Vec<EntityId> = document.entities().iter().filter(|e| e.layer == old).map(|e| e.id).collect();
    let current = if document.current_layer == old { Some(new.to_string()) } else { None };
    push(controller, layers, current, Some((affected, old.to_string(), new.to_string())));
    Ok(())
}

pub fn delete(controller: &mut Controller, name: &str) -> Result<(), String> {
    let document = controller.document();
    if name == DEFAULT_LAYER {
        return Err("Layer 0 cannot be deleted.".into());
    }
    if name == document.current_layer {
        return Err("The current layer cannot be deleted.".into());
    }
    if document.layer_in_use(name) {
        return Err(format!("Layer {name} has objects on it."));
    }
    let layers = document.layers.iter().filter(|l| l.name != name).cloned().collect();
    push(controller, layers, None, None);
    Ok(())
}

pub fn set_color(controller: &mut Controller, name: &str, color: i32) -> Result<(), String> {
    change(controller, name, |layer| layer.color = color)
}

pub fn set_linetype(controller: &mut Controller, name: &str, linetype: &str) -> Result<(), String> {
    change(controller, name, |layer| layer.linetype = linetype.to_string())
}

pub fn set_lineweight(controller: &mut Controller, name: &str, lineweight: i16) -> Result<(), String> {
    change(controller, name, |layer| layer.lineweight = lineweight)
}

pub fn set_visible(controller: &mut Controller, name: &str, visible: bool) -> Result<(), String> {
    if !visible && name == controller.document().current_layer {
        return Err("The current layer cannot be turned off.".into());
    }
    change(controller, name, |layer| layer.visible = visible)
}

pub fn set_frozen(controller: &mut Controller, name: &str, frozen: bool) -> Result<(), String> {
    if frozen && name == controller.document().current_layer {
        return Err("The current layer cannot be frozen.".into());
    }
    change(controller, name, |layer| layer.frozen = frozen)
}

pub fn set_plot(controller: &mut Controller, name: &str, plot: bool) -> Result<(), String> {
    change(controller, name, |layer| layer.plot = plot)
}

pub fn set_locked(controller: &mut Controller, name: &str, locked: bool) -> Result<(), String> {
    change(controller, name, |layer| layer.locked = locked)
}

pub fn set_current(controller: &mut Controller, name: &str) -> Result<(), String> {
    let document = controller.document();
    let layer = document.layer(name).ok_or_else(|| format!("There is no layer called {name}."))?;
    if !layer.visible {
        return Err("A layer that is turned off cannot be current.".into());
    }
    if name != document.current_layer {
        let layers = document.layers.clone();
        push(controller, layers, Some(name.to_string()), None);
    }
    Ok(())
}

fn change(controller: &mut Controller, name: &str, edit: impl Fn(&mut Layer)) -> Result<(), String> {
    let document = controller.document();
    let layer = document.layer(name).ok_or_else(|| format!("There is no layer called {name}."))?;
    let mut changed = layer.clone();
    edit(&mut changed);
    if changed != *layer {
        let layers = document.layers.iter().map(|l| if l.name == name { changed.clone() } else { l.clone() }).collect();
        push(controller, layers, None, None);
    }
    Ok(())
}

fn push(controller: &mut Controller, layers: Vec<Layer>, current: Option<String>, renamed: Option<(Vec<EntityId>, String, String)>) {
    let document = controller.document();
    let (affected, old, new) = renamed.unwrap_or_default();
    let before = LayerState {
        layers: document.layers.clone(),
        current: document.current_layer.clone(),
        entity_layers: affected.iter().map(|id| (*id, old.clone())).collect(),
    };
    let after = LayerState {
        layers,
        current: current.unwrap_or_else(|| document.current_layer.clone()),
        entity_layers: affected.iter().map(|id| (*id, new.clone())).collect(),
    };
    controller.push(Change::Layers { before, after });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::document::Document;
    use crate::model::entities::line;
    use crate::model::geometry::pt;

    fn controller() -> Controller {
        let mut walls_line = line(pt(0.0, 0.0), pt(1.0, 0.0));
        walls_line.layer = "Walls".into();
        let mut document = Document::with_entities(vec![walls_line]);
        document.layers.push(Layer::new("Walls"));
        let mut controller = Controller::new();
        controller.set_document(document);
        controller
    }

    #[test]
    fn new_layers_get_the_next_free_number() {
        let mut controller = controller();
        assert_eq!(create(&mut controller), "Layer1");
        assert_eq!(create(&mut controller), "Layer2");
        controller.undo();
        assert_eq!(controller.document().layers.len(), 3);
    }

    #[test]
    fn renaming_moves_the_objects_with_the_layer_and_undoes() {
        let mut controller = controller();
        rename(&mut controller, "Walls", "Partitions").unwrap();
        assert_eq!(controller.document().entities()[0].layer, "Partitions");
        assert!(controller.document().layer("Walls").is_none());
        controller.undo();
        assert_eq!(controller.document().entities()[0].layer, "Walls");
        assert_eq!(rename(&mut controller, "Walls", "0"), Err("A layer called 0 already exists.".into()));
        assert_eq!(rename(&mut controller, "0", "Base"), Err("Layer 0 cannot be renamed.".into()));
    }

    #[test]
    fn layers_in_use_or_current_stay() {
        let mut controller = controller();
        assert_eq!(delete(&mut controller, "Walls"), Err("Layer Walls has objects on it.".into()));
        assert_eq!(delete(&mut controller, "0"), Err("Layer 0 cannot be deleted.".into()));
        assert_eq!(set_visible(&mut controller, "0", false), Err("The current layer cannot be turned off.".into()));
        set_visible(&mut controller, "Walls", false).unwrap();
        assert_eq!(set_current(&mut controller, "Walls"), Err("A layer that is turned off cannot be current.".into()));
    }

    #[test]
    fn locking_a_layer_drops_its_objects_from_the_selection() {
        let mut controller = controller();
        controller.handle_input("all");
        assert_eq!(controller.selection.len(), 1);
        set_locked(&mut controller, "Walls", true).unwrap();
        assert!(controller.selection.is_empty());
    }
}
