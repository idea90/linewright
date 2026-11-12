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

/// COPYBASE: objects go to the clipboard to be pasted by a base point chosen first, as Ctrl+Shift+C does.
pub async fn copybase_command(io: Io, selected: Vec<Entity>, defaults: Rc<RefCell<Defaults>>) {
    let Some(Answer::Point(base)) = io.ask(Prompt::point("Specify base point:")).await else { return };
    let objects = objects_to_edit(&io, selected).await;
    if objects.is_empty() {
        return;
    }
    let count = objects.len();
    {
        let mut defaults = defaults.borrow_mut();
        defaults.clipboard = objects;
        defaults.clipboard_base = base;
    }
    io.step(Step::Report(format!("{count} object(s) copied to the clipboard.")));
}

/// PASTEORIG: what is on the clipboard goes back at the coordinates it was copied from.
pub async fn pasteorig_command(io: Io, defaults: Rc<RefCell<Defaults>>) {
    let objects = defaults.borrow().clipboard.clone();
    if objects.is_empty() {
        io.step(Step::Report("The clipboard is empty. Copy objects with Ctrl+C first.".into()));
        return;
    }
    io.step(Step::Commit(copies(&objects, &translation(0.0, 0.0))));
    io.step(Step::Report(format!("{} object(s) pasted at their original coordinates.", objects.len())));
}

/// A name for a block no one named, as AutoCAD gives pasted blocks: A$C and eight hexadecimal digits, unused.
fn anonymous_block_name(taken: &[BlockRef]) -> String {
    let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64);
    (0u64..)
        .map(|n| format!("A$C{:08X}", (seed.wrapping_add(n.wrapping_mul(0x9E37_79B9))) as u32))
        .find(|name| !taken.iter().any(|b| b.0.name.eq_ignore_ascii_case(name)))
        .expect("a free name")
}

/// PASTEBLOCK: what is on the clipboard goes in as one block, by the point it was copied by, as Ctrl+Shift+V does.
pub async fn pasteblock_command(io: Io, defaults: Rc<RefCell<Defaults>>) {
    let (objects, base, blocks) = {
        let d = defaults.borrow();
        (d.clipboard.clone(), d.clipboard_base, d.blocks.clone())
    };
    if objects.is_empty() {
        io.step(Step::Report("The clipboard is empty. Copy objects with Ctrl+C first.".into()));
        return;
    }
    let name = anonymous_block_name(&blocks);
    let entities = objects.iter().map(|e| e.duplicate()).collect();
    let block = BlockRef(Arc::new(BlockDef { name: name.clone(), base, entities, attribute_defs: vec![], xref: None }));
    let shown = block.clone();
    let prompt = Prompt::point("Specify insertion point:").preview(move |cursor| vec![insert_of(&shown, cursor, 1.0, 0.0, vec![])]);
    let Some(Answer::Point(position)) = io.ask(prompt).await else { return };
    defaults.borrow_mut().blocks.push(block.clone());
    io.step(Step::Drawn(vec![insert_of(&block, position, 1.0, 0.0, vec![])]));
    io.step(Step::Report(format!("Pasted as block {name}.")));
}

/// The piece of an insert nearest a point, looking into blocks inside blocks: in drawing coordinates, with the
/// layer and color it shows in. None for anything not an insert.
fn nested_at(insert: &Entity, point: Point) -> Option<Entity> {
    let pieces = crate::model::editing::explode(insert)?;
    let distance = |e: &Entity| crate::selection::picking::entity_distance(e, point);
    let nearest = pieces.into_iter().min_by(|a, b| distance(a).total_cmp(&distance(b)))?;
    match nearest.shape {
        Shape::Insert { .. } => nested_at(&nearest, point).or(Some(nearest)),
        _ => Some(nearest),
    }
}

/// NCOPY: copy objects from inside blocks and external references, as they show in the drawing, then move the
/// copies as COPY does.
pub async fn ncopy_command(io: Io) {
    let mut objects: Vec<Entity> = vec![];
    loop {
        let message = if objects.is_empty() { "Select nested objects to copy:" } else { "Select nested objects to copy <done>:" };
        let prompt = Prompt::new(PromptKind::Pick, message).default(Answer::keyword(""));
        match io.ask(prompt).await {
            Some(Answer::Picked(picked)) => match nested_at(&picked.entity, picked.point) {
                Some(found) => {
                    io.step(Step::Report(format!("1 found ({}).", found.shape.kind())));
                    objects.push(found);
                }
                None => io.step(Step::Report("That object is not in a block. Use COPY for it.".into())),
            },
            Some(_) => break,
            None => return,
        }
    }
    if objects.is_empty() {
        return;
    }
    let Some(Answer::Point(base)) = io.ask(Prompt::point("Specify base point:")).await else { return };
    let shown: Vec<Entity> = objects.iter().take(MAX_PREVIEW_ENTITIES).cloned().collect();
    let prompt = Prompt::point("Specify second point:")
        .base(base)
        .preview(move |cursor| copies(&shown, &translation(cursor.x - base.x, cursor.y - base.y)));
    let Some(Answer::Point(target)) = io.ask(prompt).await else { return };
    io.step(Step::Commit(copies(&objects, &translation(target.x - base.x, target.y - base.y))));
}

/// BURST: block inserts come apart into what they draw, as EXPLODE does, but their attributes stay as text with
/// the values they had rather than going back to their tags. Each piece keeps the layer and color it showed in.
pub async fn burst_command(io: Io, selected: Vec<Entity>) {
    let objects = objects_to_edit(&io, selected).await;
    let inserts: Vec<Entity> = objects.into_iter().filter(|e| matches!(e.shape, Shape::Insert { .. })).collect();
    if inserts.is_empty() {
        io.step(Step::Report("Nothing to burst: select block references.".into()));
        return;
    }
    let pieces: Vec<Entity> = inserts.iter().filter_map(crate::model::editing::explode).flatten().collect();
    let count = inserts.len();
    io.step(Step::Compound(vec![Step::Remove(inserts), Step::Commit(pieces)]));
    io.step(Step::Report(format!("{count} block reference(s) burst.")));
}

const NAME: &str = "N";
const SELECT: &str = "S";

/// ATTSYNC: inserts of a block take its attributes as it now defines them, keeping their values.
pub async fn attsync_command(io: Io, defaults: Rc<RefCell<Defaults>>) {
    let blocks: Vec<BlockRef> =
        defaults.borrow().blocks.iter().filter(|b| !b.0.attribute_defs.is_empty() || b.0.xref.is_none()).cloned().collect();
    let block = loop {
        let prompt = Prompt::new(PromptKind::Text, "Enter an option [?/Name/Select] <Select>:")
            .options(&[LIST, NAME, SELECT])
            .default(Answer::keyword(SELECT));
        match io.ask(prompt).await {
            Some(answer) if answer.is(LIST) => {
                let names: Vec<&str> = blocks.iter().filter(|b| !b.0.attribute_defs.is_empty()).map(|b| b.0.name.as_str()).collect();
                io.step(Step::Report(if names.is_empty() {
                    "No block has attributes.".into()
                } else {
                    format!("Blocks with attributes: {}", names.join(", "))
                }));
            }
            Some(answer) if answer.is(NAME) => {
                let Some(Answer::Text(name)) = io.ask(Prompt::new(PromptKind::Text, "Enter name of block to sync:")).await else { return };
                match blocks.iter().find(|b| b.0.name.eq_ignore_ascii_case(name.trim())) {
                    Some(block) => break block.clone(),
                    None => io.step(Step::Report(format!("There is no block called {}.", name.trim()))),
                }
            }
            Some(answer) if answer.is(SELECT) => {
                let Some(Answer::Picked(picked)) = io.ask(Prompt::new(PromptKind::Pick, "Select a block:")).await else { return };
                match &picked.entity.shape {
                    // The block as the drawing defines it now, not as the insert last saw it.
                    Shape::Insert { block, .. } => {
                        break blocks
                            .iter()
                            .find(|b| b.0.name.eq_ignore_ascii_case(&block.0.name))
                            .cloned()
                            .unwrap_or_else(|| block.clone());
                    }
                    _ => io.step(Step::Report("That is not a block reference.".into())),
                }
            }
            _ => return,
        }
    };
    if block.0.attribute_defs.is_empty() {
        io.step(Step::Report(format!("Block {} has no attributes to sync.", block.0.name)));
        return;
    }
    let prompt = Prompt::new(PromptKind::Text, format!("ATTSYNC block {}? [Yes/No] <Yes>:", block.0.name))
        .options(&["Y", "N"])
        .default(Answer::keyword("Y"));
    if io.ask(prompt).await.is_some_and(|answer| answer.is("Y")) {
        io.step(Step::SyncAttributes(block));
    }
}

/// EATTEDIT: pick a block with attributes, whose values the Enhanced Attribute Editor then edits.
pub async fn eattedit_command(io: Io, defaults: Rc<RefCell<Defaults>>) {
    loop {
        let Some(Answer::Picked(picked)) = io.ask(Prompt::new(PromptKind::Pick, "Select a block:")).await else { return };
        match &picked.entity.shape {
            Shape::Insert { attributes, .. } if !attributes.is_empty() => {
                defaults.borrow_mut().attribute_edit_request = Some(picked.entity.id);
                return;
            }
            Shape::Insert { .. } => io.step(Step::Report("That block has no editable attributes.".into())),
            _ => io.step(Step::Report("That is not a block reference.".into())),
        }
    }
}
