//! Blocks and the clipboard: placing blocks, making them, copying or cutting objects, and pasting what was copied.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use super::defaults::Defaults;
use super::edit::{MAX_PREVIEW_ENTITIES, copies, objects_to_edit};
use super::format_general;
use super::prompts::{Answer, Io, Prompt, PromptKind, Step};
use crate::model::entities::{Attribute, AttributeInfo, BlockDef, BlockRef, Entity, Shape, attribute_of};
use crate::model::geometry::{Point, pt};
use crate::model::index::{Bounds, entity_extent};
use crate::model::transform::{insert_transform, transform_shape, translation};

const LIST: &str = "?";

fn insert_of(block: &BlockRef, position: Point, scale: f64, rotation: f64, attributes: Vec<Attribute>) -> Entity {
    Entity::new(Shape::Insert { block: block.clone(), position, scale_x: scale, scale_y: scale, rotation, attributes })
}

/// The block's attribute definitions where an insert puts them, with the values given.
fn placed_attributes(block: &BlockRef, position: Point, scale: f64, rotation: f64, values: &[String]) -> Vec<Attribute> {
    let placed = insert_transform(block.0.base, position, scale, scale, rotation);
    block
        .0
        .attribute_defs
        .iter()
        .zip(values)
        .map(|(definition, value)| {
            let text = match transform_shape(&definition.text, &placed) {
                Shape::Text { position, height, rotation, font, style, align, line_spacing, width_factor, oblique, .. } => Shape::Text {
                    position,
                    text: value.clone(),
                    height,
                    rotation,
                    font,
                    style,
                    align,
                    line_spacing,
                    formatted: String::new(),
                    width_factor,
                    oblique,
                },
                other => other,
            };
            Attribute { text, ..definition.clone() }
        })
        .collect()
}

pub async fn insert_command(io: Io, defaults: Rc<RefCell<Defaults>>) {
    let (blocks, last) = {
        let d = defaults.borrow();
        (d.blocks.clone(), d.last_block.clone())
    };
    if blocks.is_empty() {
        io.step(Step::Report("This drawing has no blocks. Make one with BLOCK.".into()));
        return;
    }
    let default = if blocks.iter().any(|b| b.0.name.eq_ignore_ascii_case(&last)) { last } else { blocks[0].0.name.clone() };
    let block = loop {
        let prompt = Prompt::new(PromptKind::Text, format!("Enter block name or [?] <{default}>:")).default(Answer::Text(default.clone()));
        let Some(Answer::Text(name)) = io.ask(prompt).await else { return };
        if name.trim() == LIST {
            let names: Vec<&str> = blocks.iter().map(|b| b.0.name.as_str()).collect();
            io.step(Step::Report(format!("Blocks: {}", names.join(", "))));
            continue;
        }
        match blocks.iter().find(|b| b.0.name.eq_ignore_ascii_case(name.trim())) {
            Some(block) => break block.clone(),
            None => io.step(Step::Report(format!("There is no block called {}.", name.trim()))),
        }
    };
    defaults.borrow_mut().last_block = block.0.name.clone();
    let shown = block.clone();
    let prompt = Prompt::point("Specify insertion point:").preview(move |cursor| vec![insert_of(&shown, cursor, 1.0, 0.0, vec![])]);
    let Some(Answer::Point(position)) = io.ask(prompt).await else { return };
    let prompt = Prompt::new(PromptKind::Number, "Enter scale factor <1>:").base(position).default(Answer::Number(1.0));
    let Some(Answer::Number(scale)) = io.ask(prompt).await else { return };
    let shown = block.clone();
    let prompt = Prompt::new(PromptKind::Angle, "Specify rotation angle <0>:")
        .base(position)
        .default(Answer::Number(0.0))
        .preview(move |cursor| vec![insert_of(&shown, position, scale, position.angle_to(cursor), vec![])]);
    let Some(Answer::Number(rotation)) = io.ask(prompt).await else { return };
    let mut values = vec![];
    for definition in &block.0.attribute_defs {
        let Shape::Text { text, .. } = &definition.text else { continue };
        let asked = if definition.prompt.is_empty() { format!("Enter {}", definition.tag) } else { definition.prompt.clone() };
        let prompt = Prompt::new(PromptKind::Text, format!("{asked} <{text}>:")).default(Answer::Text(text.clone()));
        match io.ask(prompt).await {
            Some(Answer::Text(value)) => values.push(value),
            _ => return,
        }
    }
    let attributes = placed_attributes(&block, position, scale, rotation, &values);
    io.step(Step::Drawn(vec![insert_of(&block, position, scale, rotation, attributes)]));
}

/// Make a block of objects and put an insert of it in their place, as BLOCK does with "Convert to block".
pub async fn block_command(io: Io, selected: Vec<Entity>, defaults: Rc<RefCell<Defaults>>) {
    let name = loop {
        let Some(Answer::Text(name)) = io.ask(Prompt::new(PromptKind::Text, "Enter block name:")).await else { return };
        let name = name.trim().to_string();
        if name.is_empty() || name.contains(['<', '>', '/', '\\', '"', ':', ';', '?', '*', '|', ',', '=', '`']) {
            io.step(Step::Report("A block name cannot be empty or use < > / \\ \" : ; ? * | , = `.".into()));
        } else if defaults.borrow().blocks.iter().any(|b| b.0.name.eq_ignore_ascii_case(&name)) {
            io.step(Step::Report(format!("A block called {name} already exists.")));
        } else {
            break name;
        }
    };
    let Some(Answer::Point(base)) = io.ask(Prompt::point("Specify base point:")).await else { return };
    let objects = objects_to_edit(&io, selected).await;
    if objects.is_empty() {
        return;
    }
    // Attribute definitions among the objects become the block's attributes; the rest is what it draws.
    let (definitions, drawn): (Vec<&Entity>, Vec<&Entity>) = objects.iter().partition(|e| e.attribute.is_some());
    let entities = drawn.iter().map(|e| e.duplicate()).collect();
    let attribute_defs = definitions.iter().filter_map(|e| attribute_of(e)).collect();
    let block = BlockRef(Arc::new(BlockDef { name: name.clone(), base, entities, attribute_defs, xref: None }));
    defaults.borrow_mut().blocks.push(block.clone());
    defaults.borrow_mut().last_block = name.clone();
    defaults.borrow_mut().last_erased = objects.clone();
    io.step(Step::Compound(vec![Step::Remove(objects), Step::Drawn(vec![insert_of(&block, base, 1.0, 0.0, vec![])])]));
    io.step(Step::Report(format!("Block {name} made.")));
}

/// COPYCLIP and CUTCLIP: the objects go to the clipboard, to paste with PASTECLIP by their lower left corner.
/// Cutting erases them too.
pub async fn copyclip_command(io: Io, selected: Vec<Entity>, defaults: Rc<RefCell<Defaults>>, cut: bool) {
    let objects = objects_to_edit(&io, selected).await;
    let Some(extent) = objects.iter().map(entity_extent).reduce(Bounds::union) else { return };
    let count = objects.len();
    {
        let mut defaults = defaults.borrow_mut();
        defaults.clipboard = objects.clone();
        defaults.clipboard_base = pt(extent.x0, extent.y0);
    }
    io.step(Step::Report(format!("{count} object(s) copied to the clipboard.")));
    if cut {
        io.step(Step::Remove(objects));
    }
}

pub async fn paste_command(io: Io, defaults: Rc<RefCell<Defaults>>) {
    let (objects, base) = {
        let d = defaults.borrow();
        (d.clipboard.clone(), d.clipboard_base)
    };
    if objects.is_empty() {
        io.step(Step::Report("The clipboard is empty. Copy objects with Ctrl+C first.".into()));
        return;
    }
    let shown: Vec<Entity> = objects.iter().take(MAX_PREVIEW_ENTITIES).cloned().collect();
    let prompt =
        Prompt::point("Specify insertion point:").preview(move |cursor| copies(&shown, &translation(cursor.x - base.x, cursor.y - base.y)));
    if let Some(Answer::Point(target)) = io.ask(prompt).await {
        io.step(Step::Commit(copies(&objects, &translation(target.x - base.x, target.y - base.y))));
        io.step(Step::Report(format!("{} object(s) pasted at {},{}.", objects.len(), format_general(target.x), format_general(target.y))));
    }
}

const INVISIBLE: &str = "I";

/// ATTDEF: an attribute definition, shown by its tag, for a block made with BLOCK or edited with BEDIT. Its
/// prompt is what INSERT asks, and its default the value offered.
pub async fn attdef_command(io: Io) {
    let mut invisible = false;
    loop {
        let prompt = Prompt::point(format!(
            "Current attribute modes: Invisible={}. Enter an option to change [Invisible] <done>:",
            if invisible { "Y" } else { "N" }
        ))
        .options(&[INVISIBLE])
        .default(Answer::keyword(""));
        match io.ask(prompt).await {
            Some(answer) if answer.is(INVISIBLE) => invisible = !invisible,
            Some(_) => break,
            None => return,
        }
    }
    let tag = loop {
        let Some(Answer::Text(tag)) = io.ask(Prompt::new(PromptKind::Text, "Enter attribute tag name:")).await else { return };
        let tag = tag.trim().to_uppercase();
        if tag.is_empty() || tag.contains(char::is_whitespace) {
            io.step(Step::Report("A tag cannot be empty or have spaces.".into()));
        } else {
            break tag;
        }
    };
    let prompt = Prompt::new(PromptKind::Text, "Enter attribute prompt:").default(Answer::Text(String::new()));
    let Some(Answer::Text(asked)) = io.ask(prompt).await else { return };
    let prompt = Prompt::new(PromptKind::Text, "Enter default attribute value:").default(Answer::Text(String::new()));
    let Some(Answer::Text(default)) = io.ask(prompt).await else { return };
    let Some(Answer::Point(position)) = io.ask(Prompt::point("Specify start point of text:")).await else { return };
    let prompt = Prompt::new(PromptKind::Number, "Specify text height <2.5>:").base(position).default(Answer::Number(2.5));
    let Some(Answer::Number(height)) = io.ask(prompt).await else { return };
    let mut definition = crate::model::entities::text(position, &tag, height, 0.0);
    definition.attribute = Some(AttributeInfo { prompt: asked, default, invisible });
    io.step(Step::Drawn(vec![definition]));
}
