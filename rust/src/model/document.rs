use super::multileader::{MleaderStyle, STANDARD_MLEADER_STYLE};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;

use super::dimension::{DimensionLook, DimensionStyle, STANDARD_STYLE};
use super::entities::{BYLAYER, BlockDef, BlockRef, DEFAULT_LAYER, Entity, EntityId, Shape};
use super::geometry::Point;
use super::index::{Bounds, SpatialIndex};
use super::styles::BYLAYER_LINETYPE;
use crate::io::dxf::DxfFile;

pub const DEFAULT_LAYER_COLOR: i32 = 7;
pub const DEFAULT_LINETYPE: &str = "Continuous";

#[derive(Clone, Debug, PartialEq)]
pub struct Layer {
    pub name: String,
    pub color: i32,
    pub linetype: String,
    /// Hundredths of a millimeter, or Default.
    pub lineweight: i16,
    /// On or off.
    pub visible: bool,
    /// Frozen layers are hidden too, and stay so whatever is turned on; the current layer cannot be frozen.
    pub frozen: bool,
    pub locked: bool,
    /// Whether what is on it prints.
    pub plot: bool,
}

impl Layer {
    pub fn new(name: &str) -> Layer {
        Layer {
            name: name.to_string(),
            color: DEFAULT_LAYER_COLOR,
            linetype: DEFAULT_LINETYPE.to_string(),
            lineweight: super::lineweight::DEFAULT,
            visible: true,
            frozen: false,
            locked: false,
            plot: true,
        }
    }

    /// Whether what is on it shows: on, and not frozen.
    pub fn shown(&self) -> bool {
        self.visible && !self.frozen
    }
}

/// One space's entities in drawing order, and the index that finds them by place.
#[derive(Default)]
pub struct EntityStore {
    entities: Vec<Entity>,
    positions: HashMap<EntityId, usize>,
    index: SpatialIndex,
}

impl EntityStore {
    fn with(entities: Vec<Entity>) -> EntityStore {
        let mut store = EntityStore::default();
        store.index.reload(&entities);
        store.positions = entities.iter().enumerate().map(|(position, e)| (e.id, position)).collect();
        store.entities = entities;
        store
    }

    pub fn entities(&self) -> &[Entity] {
        &self.entities
    }

    pub fn get(&self, id: EntityId) -> Option<&Entity> {
        self.positions.get(&id).map(|&position| &self.entities[position])
    }

    fn contains(&self, id: EntityId) -> bool {
        self.positions.contains_key(&id)
    }

    fn modify(&mut self, state: &Entity) {
        if let Some(&position) = self.positions.get(&state.id) {
            self.entities[position].copy_state(state);
            self.index.refresh(&self.entities[position]);
        }
    }

    /// Every entity whose box touches the area, in drawing order, on any layer.
    pub fn ids_in(&self, area: &Bounds) -> Vec<EntityId> {
        self.index.query(area)
    }

    pub fn bounds_of(&self, id: EntityId) -> Option<Bounds> {
        self.index.bounds_of(id)
    }

    /// The box around everything in the space, on any layer.
    pub fn extent(&self) -> Option<Bounds> {
        self.entities
            .iter()
            .filter(|e| !matches!(e.shape, Shape::XLine { .. }))
            .filter_map(|e| self.index.bounds_of(e.id))
            .reduce(Bounds::union)
    }
}

/// A named set of objects that select together, as AutoCAD's groups. Unnamed ones are named *A1, *A2… by AutoCAD.
#[derive(Clone, Debug, PartialEq)]
pub struct ObjectGroup {
    pub name: String,
    pub description: String,
    pub unnamed: bool,
    /// Picking one member picks them all (while group selection is on).
    pub selectable: bool,
    pub members: Vec<EntityId>,
}

/// A paper layout: a sheet with what is drawn on it, viewports onto model space among it.
#[derive(Default)]
pub struct Layout {
    pub name: String,
    /// The sheet in paper space units: where its lower-left corner sits, and its width and height.
    pub paper_origin: Point,
    pub paper_size: (f64, f64),
    /// Where it came from in the file: its paper space block record and its LAYOUT object, by handle.
    pub record: Option<String>,
    pub object: Option<String>,
    pub plot: PlotSettings,
    store: EntityStore,
}

/// A named text style: its font (an .shx or .ttf file, or a family), a fixed height or 0 to ask for one each time,
/// its letters' width and lean.
#[derive(Clone, Debug, PartialEq)]
pub struct TextStyleDef {
    pub name: String,
    pub font: String,
    pub height: f64,
    pub width_factor: f64,
    pub oblique: f64,
}

impl TextStyleDef {
    pub fn standard() -> TextStyleDef {
        TextStyleDef { name: STANDARD_STYLE.into(), font: String::new(), height: 0.0, width_factor: 1.0, oblique: 0.0 }
    }
}

/// How a space plots, as its page setup says.
#[derive(Clone, Debug, PartialEq)]
pub struct PlotSettings {
    /// The plot style table (a .ctb file name), empty for none: then objects print in their own colors.
    pub style_table: String,
    /// Whether objects print with their lineweights; without, every line prints as thin as can be.
    pub lineweights: bool,
}

impl Default for PlotSettings {
    fn default() -> Self {
        PlotSettings { style_table: String::new(), lineweights: true }
    }
}

impl Layout {
    pub fn sheet(&self) -> Bounds {
        Bounds::from_corners(self.paper_origin, self.paper_origin + super::geometry::pt(self.paper_size.0, self.paper_size.1))
    }

    /// A layout with an ISO A3 sheet across, in millimeters.
    pub fn new(name: &str) -> Layout {
        Layout { name: name.into(), paper_size: (420.0, 297.0), ..Layout::default() }
    }
}

/// A drawing: its entities in drawing order, its layers and what new objects get.
pub struct Document {
    /// The open space's entities: model space, or the layout that is open.
    store: EntityStore,
    /// Model space while a layout is open.
    model_space: EntityStore,
    pub layouts: Vec<Layout>,
    active_layout: Option<usize>,
    pub layers: Vec<Layer>,
    pub current_layer: String,
    pub current_color: i32,
    pub current_linetype: String,
    /// LTSCALE: the scale every linetype's dashes are drawn at, times each object's own linetype scale.
    pub ltscale: f64,
    /// PSLTSCALE: whether model space objects seen through a viewport keep their dashes as long on the sheet as paper
    /// space ones (on, AutoCAD's start value) or are drawn at the viewport's scale (off).
    pub psltscale: bool,
    /// CELTSCALE: the linetype scale new objects get.
    pub celtscale: f64,
    /// CELWEIGHT: the lineweight new objects get.
    pub current_lineweight: i16,
    /// LWDEFAULT: what Default means, in hundredths of a millimeter.
    pub default_lineweight: i16,
    /// LWDISPLAY: whether lineweights show on screen (they always print).
    pub show_lineweights: bool,
    /// How model space plots: its plot style table and whether objects' lineweights print.
    pub model_plot: PlotSettings,
    /// WIPEOUTFRAME: whether wipeout frames show on screen and print. 0 hides them, 1 shows and prints them (AutoCAD's
    /// start value), and 2 shows them but does not print them.
    pub wipeout_frame: u8,
    pub path: Option<PathBuf>,
    /// The loaded DXF file without the entities this document holds. Saving writes the document into it, so
    /// content Linewright cannot edit yet is kept.
    pub source: Option<DxfFile>,
    pub skipped: BTreeMap<String, usize>,
    /// The named blocks the drawing's file defines, by upper-case name.
    pub blocks: BTreeMap<String, BlockRef>,
    /// The drawing's dimension styles, and the one new dimensions follow.
    pub dimension_styles: Vec<DimensionStyle>,
    pub current_dimension_style: String,
    /// The drawing's text styles, and the one new text follows.
    pub text_styles: Vec<TextStyleDef>,
    pub current_text_style: String,
    /// The drawing's multileader styles, and the one new multileaders follow.
    pub mleader_styles: Vec<MleaderStyle>,
    /// The multiline styles MLINE can draw with, from the file.
    pub mline_styles: Vec<crate::model::mline::MlineStyle>,
    pub current_mleader_style: String,
    pub groups: Vec<ObjectGroup>,
    /// PICKSTYLE: whether picking a grouped object picks its whole group.
    pub group_selection: bool,
    /// The drawing's units as $INSUNITS gives them, once set here (UNITS); None keeps the file's.
    pub units: Option<i64>,
    /// Decimal places coordinates and lengths are shown with ($LUPREC).
    pub precision: usize,
    /// Blocks PURGE removed, which saving takes out of the file too.
    pub purged_blocks: Vec<String>,
    /// Model space objects Linewright shows but cannot edit, such as AutoCAD's tables, by the drawing AutoCAD
    /// keeps for them. They stay in the file as they are.
    pub shown_only: Vec<Entity>,
    /// The block open in the block editor, with the space it was opened from.
    block_edit: Option<BlockEdit>,
    /// Blocks changed in the block editor, whose definitions in the file are written afresh on saving.
    pub redefined_blocks: Vec<String>,
    /// Model space open through a layout's viewport (MSPACE): the layout stays on screen.
    through: Option<Through>,
}

/// The layout on screen and the viewport whose model space is being worked on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Through {
    pub layout: usize,
    pub viewport: EntityId,
}

struct BlockEdit {
    block: BlockRef,
    store: EntityStore,
    layout: Option<usize>,
}

/// Where DRAWORDER puts objects: in front of or behind everything, or just above or under other objects.
#[derive(Clone, Debug, PartialEq)]
pub enum DrawOrder {
    Front,
    Back,
    Above(Vec<EntityId>),
    Under(Vec<EntityId>),
}

impl Default for Document {
    fn default() -> Self {
        Document::new()
    }
}

impl Document {
    pub fn new() -> Document {
        Document {
            store: EntityStore::default(),
            model_space: EntityStore::default(),
            layouts: vec![Layout::new("Layout1")],
            active_layout: None,
            layers: vec![Layer::new(DEFAULT_LAYER)],
            current_layer: DEFAULT_LAYER.to_string(),
            current_color: BYLAYER,
            current_linetype: BYLAYER_LINETYPE.to_string(),
            ltscale: 1.0,
            psltscale: true,
            celtscale: 1.0,
            current_lineweight: super::lineweight::BYLAYER,
            default_lineweight: super::lineweight::DEFAULT_MM100,
            show_lineweights: false,
            model_plot: PlotSettings::default(),
            wipeout_frame: 1,
            path: None,
            source: None,
            skipped: BTreeMap::new(),
            blocks: BTreeMap::new(),
            dimension_styles: vec![DimensionStyle::standard()],
            current_dimension_style: STANDARD_STYLE.into(),
            text_styles: vec![TextStyleDef::standard()],
            current_text_style: STANDARD_STYLE.into(),
            mleader_styles: vec![MleaderStyle::standard()],
            mline_styles: vec![crate::model::mline::MlineStyle::standard()],
            current_mleader_style: STANDARD_MLEADER_STYLE.into(),
            groups: vec![],
            group_selection: true,
            units: None,
            precision: 4,
            purged_blocks: vec![],
            shown_only: vec![],
            block_edit: None,
            redefined_blocks: vec![],
            through: None,
        }
    }

    pub fn with_entities(entities: Vec<Entity>) -> Document {
        let mut document = Document::new();
        document.set_entities(entities);
        document
    }

    pub fn entities(&self) -> &[Entity] {
        &self.store.entities
    }

    pub fn len(&self) -> usize {
        self.store.entities.len()
    }

    pub fn is_empty(&self) -> bool {
        self.store.entities.is_empty()
    }

    pub fn set_entities(&mut self, entities: Vec<Entity>) {
        self.store.entities = entities;
        self.store.index.reload(&self.store.entities);
        self.reposition();
    }

    pub fn dimension_style(&self, name: &str) -> Option<&DimensionStyle> {
        self.dimension_styles.iter().find(|s| s.name.eq_ignore_ascii_case(name))
    }

    /// The dimensions following a style, as they become when it changes from `old` to `new`.
    pub fn restyled_dimensions(&self, old: &DimensionStyle, new: &DimensionStyle) -> Vec<Entity> {
        let mut changed = vec![];
        for entity in &self.store.entities {
            let mut shape = entity.shape.clone();
            let (Shape::Dimension { look, .. } | Shape::Annotation { look, .. }) = &mut shape else { continue };
            if !look.style.eq_ignore_ascii_case(&old.name) {
                continue;
            }
            let restyled = look.restyled(&old.look, &new.look);
            if restyled != *look {
                *look = restyled;
                changed.push(entity.with_shape(shape));
            }
        }
        changed
    }

    /// The look new dimensions take: the current dimension style's.
    pub fn current_dimension_look(&self) -> DimensionLook {
        self.dimension_style(&self.current_dimension_style).map_or_else(DimensionLook::default, |s| s.look.clone())
    }

    /// An entity of the open space, or of another one (as a layer change reaches them all).
    pub fn get(&self, id: EntityId) -> Option<&Entity> {
        self.store.get(id).or_else(|| self.other_stores().find_map(|store| store.get(id)))
    }

    pub fn contains(&self, id: EntityId) -> bool {
        self.store.positions.contains_key(&id)
    }

    pub fn add(&mut self, entity: Entity) {
        self.store.index.add(&entity);
        self.store.positions.insert(entity.id, self.store.entities.len());
        self.store.entities.push(entity);
    }

    pub fn insert(&mut self, position: usize, entity: Entity) {
        self.store.index.add(&entity);
        self.store.entities.insert(position.min(self.store.entities.len()), entity);
        self.reposition();
    }

    /// Where each entity sits. None for one that is not in the drawing.
    pub fn positions_of(&self, ids: &[EntityId]) -> Vec<Option<usize>> {
        ids.iter().map(|id| self.store.positions.get(id).copied()).collect()
    }

    /// Remove many entities in one pass, returning them.
    pub fn remove_all(&mut self, ids: &[EntityId]) -> Vec<Entity> {
        let doomed: HashSet<EntityId> = ids.iter().copied().collect();
        let (removed, kept): (Vec<Entity>, Vec<Entity>) =
            std::mem::take(&mut self.store.entities).into_iter().partition(|e| doomed.contains(&e.id));
        self.store.entities = kept;
        for entity in &removed {
            self.store.index.remove(entity.id);
        }
        self.reposition();
        removed
    }

    /// Give an entity new values, keeping its identity and place.
    pub fn modify(&mut self, state: &Entity) {
        if self.store.contains(state.id) {
            self.store.modify(state);
            return;
        }
        let active = self.active_layout;
        let others = std::iter::once(&mut self.model_space)
            .filter(|_| active.is_some())
            .chain(self.layouts.iter_mut().enumerate().filter(|(i, _)| Some(*i) != active).map(|(_, l)| &mut l.store));
        for store in others {
            if store.contains(state.id) {
                store.modify(state);
                return;
            }
        }
    }

    /// The stores of the spaces that are not open.
    fn other_stores(&self) -> impl Iterator<Item = &EntityStore> {
        let active = self.active_layout;
        std::iter::once(&self.model_space)
            .filter(move |_| active.is_some())
            .chain(self.layouts.iter().enumerate().filter(move |(i, _)| Some(*i) != active).map(|(_, l)| &l.store))
    }

    /// The objects picked, with the rest of every selectable group they belong to, when group selection is on.
    /// Only members in the open space come along.
    pub fn with_groups(&self, ids: Vec<EntityId>) -> Vec<EntityId> {
        if !self.group_selection {
            return ids;
        }
        let mut all = ids.clone();
        for group in self.groups.iter().filter(|g| g.selectable && g.members.iter().any(|m| ids.contains(m))) {
            all.extend(group.members.iter().filter(|m| self.store.contains(**m) && !all.contains(m)).copied().collect::<Vec<_>>());
        }
        all
    }

    /// The name the next unnamed group gets: *A1, *A2…
    pub fn next_unnamed_group(&self) -> String {
        (1..).map(|n| format!("*A{n}")).find(|n| !self.groups.iter().any(|g| g.name.eq_ignore_ascii_case(n))).unwrap_or_default()
    }

    /// A block's new definition put in place: in the block list and in every insert of it, in every space.
    pub fn replace_block(&mut self, block: BlockRef) {
        let key = block.0.name.to_uppercase();
        if self.blocks.contains_key(&key) {
            self.blocks.insert(key.clone(), block.clone());
        }
        let renew = |store: &mut EntityStore| {
            let uses = store
                .entities
                .iter()
                .any(|e| matches!(&e.shape, Shape::Insert { block: b, .. } if b.0.name.eq_ignore_ascii_case(&block.0.name)));
            if !uses {
                return;
            }
            let entities: Vec<Entity> = std::mem::take(&mut store.entities)
                .into_iter()
                .map(|mut e| {
                    if let Shape::Insert { block: b, .. } = &mut e.shape
                        && b.0.name.eq_ignore_ascii_case(&block.0.name)
                    {
                        *b = block.clone();
                    }
                    e
                })
                .collect();
            *store = EntityStore::with(entities);
        };
        renew(&mut self.store);
        renew(&mut self.model_space);
        for layout in &mut self.layouts {
            renew(&mut layout.store);
        }
    }

    /// The external references in the drawing, inserted or not.
    pub fn xrefs(&self) -> Vec<BlockRef> {
        let mut found: BTreeMap<String, BlockRef> =
            self.blocks.iter().filter(|(_, b)| b.0.xref.is_some()).map(|(k, b)| (k.clone(), b.clone())).collect();
        for entity in self.all_entities() {
            if let Shape::Insert { block, .. } = &entity.shape
                && block.0.xref.is_some()
            {
                found.entry(block.0.name.to_uppercase()).or_insert_with(|| block.clone());
            }
        }
        found.into_values().collect()
    }

    /// The open space's drawing order, every entity by id.
    pub fn order(&self) -> Vec<EntityId> {
        self.store.entities.iter().map(|e| e.id).collect()
    }

    /// The drawing order with `moving` put where DRAWORDER says, the rest keeping theirs.
    pub fn reordered(&self, moving: &[EntityId], placement: &DrawOrder) -> Vec<EntityId> {
        let order = self.order();
        let rest: Vec<EntityId> = order.iter().copied().filter(|id| !moving.contains(id)).collect();
        let moved: Vec<EntityId> = order.iter().copied().filter(|id| moving.contains(id)).collect();
        let at = match placement {
            DrawOrder::Front => rest.len(),
            DrawOrder::Back => 0,
            DrawOrder::Above(others) => rest.iter().rposition(|id| others.contains(id)).map_or(rest.len(), |i| i + 1),
            DrawOrder::Under(others) => rest.iter().position(|id| others.contains(id)).unwrap_or(0),
        };
        rest[..at].iter().chain(&moved).chain(&rest[at..]).copied().collect()
    }

    /// Put the open space's entities in this order, as DRAWORDER and its undo do.
    pub fn set_order(&mut self, order: &[EntityId]) {
        let mut by_id: HashMap<EntityId, Entity> = std::mem::take(&mut self.store.entities).into_iter().map(|e| (e.id, e)).collect();
        let mut entities: Vec<Entity> = order.iter().filter_map(|id| by_id.remove(id)).collect();
        entities.extend(by_id.into_values());
        self.set_entities(entities);
    }

    /// Every entity in every space: model space and each layout.
    pub fn all_entities(&self) -> impl Iterator<Item = &Entity> {
        std::iter::once(&self.store).chain(self.other_stores()).flat_map(|store| store.entities.iter())
    }

    /// Layers nothing is drawn on, in any space or block, that may go: not 0, Defpoints or the current one.
    pub fn unused_layers(&self) -> Vec<String> {
        let mut used: HashSet<&str> = self.all_entities().map(|e| e.layer.as_str()).collect();
        for block in self.blocks.values() {
            used.extend(block.0.entities.iter().map(|e| e.layer.as_str()));
        }
        self.layers
            .iter()
            .map(|l| l.name.clone())
            .filter(|n| {
                n != DEFAULT_LAYER && !n.eq_ignore_ascii_case("Defpoints") && *n != self.current_layer && !used.contains(n.as_str())
            })
            .collect()
    }

    /// Dimension styles no dimension follows, other than Standard and the current one.
    pub fn unused_dimension_styles(&self) -> Vec<String> {
        let used: HashSet<String> = self
            .all_entities()
            .filter_map(|e| match &e.shape {
                Shape::Dimension { look, .. } | Shape::Annotation { look, .. } => Some(look.style.to_lowercase()),
                _ => None,
            })
            .collect();
        self.dimension_styles
            .iter()
            .map(|s| s.name.clone())
            .filter(|n| {
                !n.eq_ignore_ascii_case(STANDARD_STYLE)
                    && !n.eq_ignore_ascii_case(&self.current_dimension_style)
                    && !used.contains(&n.to_lowercase())
            })
            .collect()
    }

    /// Named blocks nothing inserts, directly or inside another block that is inserted.
    pub fn unused_blocks(&self) -> Vec<String> {
        let mut used: HashSet<String> = HashSet::new();
        let mut waiting: Vec<BlockRef> = self
            .all_entities()
            .filter_map(|e| if let Shape::Insert { block, .. } = &e.shape { Some(block.clone()) } else { None })
            .collect();
        while let Some(block) = waiting.pop() {
            if used.insert(block.0.name.to_uppercase()) {
                waiting.extend(
                    block
                        .0
                        .entities
                        .iter()
                        .filter_map(|e| if let Shape::Insert { block, .. } = &e.shape { Some(block.clone()) } else { None }),
                );
            }
        }
        self.blocks.iter().filter(|(key, _)| !used.contains(*key)).map(|(_, block)| block.0.name.clone()).collect()
    }

    /// The block open in the block editor, if one is.
    pub fn editing_block(&self) -> Option<&BlockRef> {
        self.block_edit.as_ref().map(|edit| &edit.block)
    }

    /// Open a block's contents in place of the open space, for editing.
    pub fn begin_block_edit(&mut self, block: BlockRef) {
        if self.block_edit.is_some() {
            return;
        }
        // Its attributes open as attribute definitions, showing their tags.
        let mut opened = block.0.entities.clone();
        opened.extend(block.0.attribute_defs.iter().map(super::entities::definition_of));
        let store = std::mem::replace(&mut self.store, EntityStore::with(opened));
        self.block_edit = Some(BlockEdit { block, store, layout: self.active_layout });
        self.active_layout = None;
    }

    /// Close the block editor, putting back the space it was opened from. Saved, the block takes what was drawn,
    /// in every insert of it; the new definition is given back.
    pub fn end_block_edit(&mut self, save: bool) -> Option<BlockRef> {
        let edit = self.block_edit.take()?;
        let entities = std::mem::replace(&mut self.store, edit.store).entities;
        self.active_layout = edit.layout;
        if !save {
            return None;
        }
        let (definitions, entities): (Vec<Entity>, Vec<Entity>) = entities.into_iter().partition(|e| e.attribute.is_some());
        let attribute_defs = definitions.iter().filter_map(super::entities::attribute_of).collect();
        let block = BlockRef(std::sync::Arc::new(BlockDef { entities, attribute_defs, ..(*edit.block.0).clone() }));
        self.blocks.insert(block.0.name.to_uppercase(), block.clone());
        self.replace_block(block.clone());
        if !self.redefined_blocks.iter().any(|n| n.eq_ignore_ascii_case(&block.0.name)) {
            self.redefined_blocks.push(block.0.name.clone());
        }
        Some(block)
    }

    /// Which layout is open; None for model space, also when it is open through a viewport.
    pub fn active_layout(&self) -> Option<usize> {
        self.active_layout
    }

    /// The layout on screen: the open one, or the one whose viewport model space is open through.
    pub fn shown_layout(&self) -> Option<usize> {
        self.active_layout.or(self.through.map(|t| t.layout))
    }

    /// The viewport model space is open through, if it is.
    pub fn through(&self) -> Option<Through> {
        self.through
    }

    /// The viewport model space is open through, with its frame and view.
    pub fn through_viewport(&self) -> Option<&Entity> {
        let through = self.through?;
        self.layouts.get(through.layout)?.store.get(through.viewport)
    }

    /// An entity in any space, by id.
    pub fn find_anywhere(&self, id: EntityId) -> Option<&Entity> {
        self.store
            .get(id)
            .or_else(|| std::iter::once(&self.model_space).chain(self.layouts.iter().map(|l| &l.store)).find_map(|store| store.get(id)))
    }

    /// MSPACE: open model space through one of the open layout's viewports, keeping the layout on screen.
    /// From another viewport of the same layout, that one becomes the one worked through.
    pub fn enter_viewport(&mut self, viewport: EntityId) -> bool {
        let layout = match (self.active_layout, self.through) {
            (Some(layout), None) => layout,
            (None, Some(through)) => through.layout,
            _ => return false,
        };
        let is_viewport = |e: &Entity| matches!(e.shape, Shape::Viewport { .. });
        let found = if self.active_layout.is_some() { self.store.get(viewport) } else { self.layouts[layout].store.get(viewport) };
        if self.block_edit.is_some() || !found.is_some_and(is_viewport) {
            return false;
        }
        if self.active_layout.is_some() {
            self.activate(None);
        }
        self.through = Some(Through { layout, viewport });
        true
    }

    /// PSPACE: back to the layout's paper space from working through a viewport.
    pub fn leave_viewport(&mut self) {
        if let Some(through) = self.through.take() {
            self.activate(Some(through.layout));
        }
    }

    /// Open model space (None) or a layout. The open space's entities are what everything else works on.
    pub fn activate(&mut self, target: Option<usize>) {
        if self.block_edit.is_some() || target.is_some_and(|i| i >= self.layouts.len()) {
            return;
        }
        // Model space open through a viewport is model space already: the layout just leaves the screen.
        self.through = None;
        if target == self.active_layout {
            return;
        }
        let open = std::mem::take(&mut self.store);
        match self.active_layout {
            None => self.model_space = open,
            Some(i) => self.layouts[i].store = open,
        }
        self.store = match target {
            None => std::mem::take(&mut self.model_space),
            Some(i) => std::mem::take(&mut self.layouts[i].store),
        };
        self.active_layout = target;
    }

    /// The entities of a space, open or not, with their index.
    pub fn space_store(&self, space: Option<usize>) -> &EntityStore {
        if space == self.active_layout {
            &self.store
        } else {
            match space {
                None => &self.model_space,
                Some(i) => &self.layouts[i].store,
            }
        }
    }

    /// Model space, open or not, for viewports to show.
    pub fn model(&self) -> &EntityStore {
        if self.active_layout.is_none() { &self.store } else { &self.model_space }
    }

    /// The entities of a space: model space (None) or a layout.
    pub fn space_entities(&self, space: Option<usize>) -> &[Entity] {
        if space == self.active_layout {
            &self.store.entities
        } else {
            match space {
                None => &self.model_space.entities,
                Some(i) => &self.layouts[i].store.entities,
            }
        }
    }

    /// Fill a space that is not open, as reading a file does.
    pub fn set_space_entities(&mut self, space: Option<usize>, entities: Vec<Entity>) {
        if space == self.active_layout {
            self.set_entities(entities);
            return;
        }
        let store = match space {
            None => &mut self.model_space,
            Some(i) => &mut self.layouts[i].store,
        };
        *store = EntityStore::with(entities);
    }

    /// Take a layout out, with what is on it. Model space opens if it was the open one.
    pub fn remove_layout(&mut self, index: usize) -> Option<Layout> {
        if index >= self.layouts.len() {
            return None;
        }
        if self.shown_layout() == Some(index) {
            self.activate(None);
        }
        if let Some(through) = self.through.as_mut().filter(|t| t.layout > index) {
            through.layout -= 1;
        }
        let layout = self.layouts.remove(index);
        if let Some(active) = self.active_layout.filter(|&a| a > index) {
            self.active_layout = Some(active - 1);
        }
        Some(layout)
    }

    fn reposition(&mut self) {
        self.store.positions = self.store.entities.iter().enumerate().map(|(position, e)| (e.id, position)).collect();
    }

    pub fn layer(&self, name: &str) -> Option<&Layer> {
        self.layers.iter().find(|layer| layer.name == name)
    }

    pub fn layer_mut(&mut self, name: &str) -> Option<&mut Layer> {
        self.layers.iter_mut().find(|layer| layer.name == name)
    }

    pub fn layer_of(&self, entity: &Entity) -> Layer {
        self.layer(&entity.layer).or_else(|| self.layer(DEFAULT_LAYER)).cloned().unwrap_or_else(|| Layer::new(DEFAULT_LAYER))
    }

    fn layer_flags(&self, name: &str) -> (bool, bool) {
        match self.layer(name).or_else(|| self.layer(DEFAULT_LAYER)) {
            Some(layer) => (layer.shown(), layer.locked),
            None => (true, false),
        }
    }

    pub fn effective_linetype(&self, entity: &Entity) -> String {
        if entity.linetype == BYLAYER_LINETYPE { self.layer_of(entity).linetype } else { entity.linetype.clone() }
    }

    pub fn is_visible(&self, entity: &Entity) -> bool {
        self.layer_flags(&entity.layer).0
    }

    /// Whether a layer is frozen in the viewport model space is open through, so nothing on it shows there.
    pub fn frozen_through(&self, layer: &str) -> bool {
        matches!(self.through_viewport().map(|v| &v.shape), Some(Shape::Viewport { frozen, .. }) if frozen.iter().any(|f| f.eq_ignore_ascii_case(layer)))
    }

    /// Everything on a layer that is turned on (and not frozen in the viewport worked through). This is what can
    /// be seen and snapped to.
    pub fn visible_entities(&self) -> Vec<Entity> {
        self.store.entities.iter().filter(|e| self.is_visible(e) && !self.frozen_through(&e.layer)).cloned().collect()
    }

    pub fn is_editable(&self, entity: &Entity) -> bool {
        let (visible, locked) = self.layer_flags(&entity.layer);
        visible && !locked && !self.frozen_through(&entity.layer)
    }

    /// Everything that can be picked and changed: visible and not on a locked layer.
    pub fn editable_entities(&self) -> Vec<Entity> {
        self.store.entities.iter().filter(|e| self.is_editable(e)).cloned().collect()
    }

    pub fn layer_in_use(&self, name: &str) -> bool {
        std::iter::once(&self.store).chain(self.other_stores()).any(|store| store.entities.iter().any(|e| e.layer == name))
    }

    /// The visible entities whose box touches the area, in drawing order. Fast for any drawing size.
    pub fn ids_in(&self, area: &Bounds) -> Vec<EntityId> {
        self.store.index.query(area).into_iter().filter(|id| self.get(*id).is_some_and(|e| self.is_visible(e))).collect()
    }

    pub fn entities_in(&self, area: &Bounds) -> Vec<Entity> {
        self.ids_in(area).into_iter().filter_map(|id| self.get(id).cloned()).collect()
    }

    pub fn bounds_of(&self, id: EntityId) -> Option<Bounds> {
        self.store.index.bounds_of(id)
    }

    pub fn draw_order(&self, id: EntityId) -> u64 {
        self.store.index.order_of(id)
    }

    pub fn cell_size(&self) -> f64 {
        self.store.index.cell_size
    }

    /// The box around everything visible, or None for an empty drawing.
    /// Construction lines have no end, so they are left out.
    pub fn extent(&self) -> Option<Bounds> {
        self.store
            .entities
            .iter()
            .filter(|e| self.is_visible(e) && !matches!(e.shape, Shape::XLine { .. }))
            .filter_map(|e| self.store.index.bounds_of(e.id))
            .reduce(Bounds::union)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::entities::line;
    use crate::model::geometry::pt;

    fn viewport() -> Entity {
        Entity::new(Shape::Viewport {
            center: pt(100.0, 100.0),
            width: 100.0,
            height: 50.0,
            view_center: pt(0.0, 0.0),
            view_height: 500.0,
            frozen: vec!["Hidden".into()],
            locked: false,
        })
    }

    #[test]
    fn working_through_a_viewport_edits_model_space_with_the_layout_on_screen() {
        let mut document = Document::with_entities(vec![line(pt(0.0, 0.0), pt(10.0, 0.0))]);
        let frame = viewport();
        let title = line(pt(0.0, 0.0), pt(400.0, 0.0));
        document.set_space_entities(Some(0), vec![frame.clone(), title.clone()]);
        document.activate(Some(0));
        assert!(!document.enter_viewport(title.id), "only a viewport can be worked through");
        assert!(document.enter_viewport(frame.id));
        assert_eq!((document.active_layout(), document.shown_layout()), (None, Some(0)));
        assert_eq!(document.len(), 1, "model space is open");
        document.add(line(pt(0.0, 5.0), pt(10.0, 5.0)));
        assert_eq!(document.through_viewport().map(|v| v.id), Some(frame.id));
        let mut hidden = line(pt(0.0, 0.0), pt(1.0, 1.0));
        hidden.layer = "Hidden".into();
        assert!(!document.is_editable(&hidden), "a layer frozen in the viewport cannot be picked through it");
        document.leave_viewport();
        assert_eq!((document.active_layout(), document.through()), (Some(0), None));
        assert_eq!(document.len(), 2, "the sheet's own objects are open again");
        assert_eq!(document.model().entities().len(), 2, "what was drawn through the viewport is in model space");
        assert!(document.is_editable(&hidden));
        // The Model tab from inside a viewport just takes the layout off the screen.
        document.enter_viewport(frame.id);
        document.activate(None);
        assert_eq!((document.shown_layout(), document.through()), (None, None));
    }

    #[test]
    fn removing_and_inserting_keeps_positions_right() {
        let entities: Vec<Entity> = (0..5).map(|i| line(pt(i as f64, 0.0), pt(i as f64, 1.0))).collect();
        let ids: Vec<EntityId> = entities.iter().map(|e| e.id).collect();
        let mut document = Document::with_entities(entities);
        let removed = document.remove_all(&[ids[1], ids[3]]);
        assert_eq!(removed.len(), 2);
        assert_eq!(document.positions_of(&[ids[4]]), vec![Some(2)]);
        document.insert(1, removed[0].clone());
        assert_eq!(document.positions_of(&[ids[1], ids[4]]), vec![Some(1), Some(3)]);
    }

    #[test]
    fn hidden_layers_are_neither_seen_nor_edited() {
        let mut document = Document::new();
        let mut layer = Layer::new("Hidden");
        layer.visible = false;
        document.layers.push(layer);
        let mut on_hidden = line(pt(0.0, 0.0), pt(1.0, 1.0));
        on_hidden.layer = "Hidden".into();
        document.add(on_hidden);
        document.add(line(pt(0.0, 0.0), pt(2.0, 2.0)));
        assert_eq!(document.visible_entities().len(), 1);
        assert_eq!(document.ids_in(&Bounds::square_around(pt(0.0, 0.0), 1.0)).len(), 1);
        assert_eq!(document.extent().unwrap().x1, 2.0);
    }

    #[test]
    fn picking_one_member_picks_its_group_while_group_selection_is_on() {
        let (a, b, c) = (line(pt(0.0, 0.0), pt(1.0, 0.0)), line(pt(0.0, 1.0), pt(1.0, 1.0)), line(pt(0.0, 2.0), pt(1.0, 2.0)));
        let mut document = Document::with_entities(vec![a.clone(), b.clone(), c.clone()]);
        document.groups.push(ObjectGroup {
            name: "*A1".into(),
            description: String::new(),
            unnamed: true,
            selectable: true,
            members: vec![a.id, b.id],
        });
        assert_eq!(document.with_groups(vec![a.id]), vec![a.id, b.id]);
        assert_eq!(document.with_groups(vec![c.id]), vec![c.id]);
        document.group_selection = false;
        assert_eq!(document.with_groups(vec![a.id]), vec![a.id]);
        assert_eq!(document.next_unnamed_group(), "*A2");
    }

    #[test]
    fn draw_order_moves_objects_front_back_above_and_under() {
        let lines: Vec<Entity> = (0..4).map(|i| line(pt(0.0, i as f64), pt(1.0, i as f64))).collect();
        let ids: Vec<EntityId> = lines.iter().map(|e| e.id).collect();
        let mut document = Document::with_entities(lines);
        assert_eq!(document.reordered(&[ids[0]], &DrawOrder::Front), vec![ids[1], ids[2], ids[3], ids[0]]);
        assert_eq!(document.reordered(&[ids[3]], &DrawOrder::Back), vec![ids[3], ids[0], ids[1], ids[2]]);
        assert_eq!(document.reordered(&[ids[0]], &DrawOrder::Above(vec![ids[2]])), vec![ids[1], ids[2], ids[0], ids[3]]);
        assert_eq!(document.reordered(&[ids[3]], &DrawOrder::Under(vec![ids[1]])), vec![ids[0], ids[3], ids[1], ids[2]]);
        document.set_order(&[ids[3], ids[2], ids[1], ids[0]]);
        assert_eq!(document.order(), vec![ids[3], ids[2], ids[1], ids[0]]);
        assert!(document.draw_order(ids[0]) > document.draw_order(ids[3]), "the index draws them in the new order");
    }

    #[test]
    fn unused_layers_and_styles_are_found_in_every_space() {
        let mut document = Document::new();
        document.layers.extend([Layer::new("Walls"), Layer::new("Spare"), Layer::new("Paper")]);
        let mut wall = line(pt(0.0, 0.0), pt(1.0, 0.0));
        wall.layer = "Walls".into();
        let mut sheet = line(pt(0.0, 0.0), pt(1.0, 0.0));
        sheet.layer = "Paper".into();
        document.add(wall);
        document.set_space_entities(Some(0), vec![sheet]);
        assert_eq!(document.unused_layers(), vec!["Spare".to_string()]);
        document.dimension_styles.push(DimensionStyle { name: "Old".into(), look: DimensionLook::default() });
        assert_eq!(document.unused_dimension_styles(), vec!["Old".to_string()]);
    }
}
