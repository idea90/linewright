//! Connects typed input, clicks and the cursor to the running command, and its results to the drawing.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use super::undo::{Change, LayerState, UndoStack};
use crate::canvas::view::grid_spacing;
use crate::commands::defaults::Defaults;
use crate::commands::prompts::{Host, LayoutRequest, Prompt, PromptKind, SELECT_HINT, ScaleSetting, Session, ViewRequest};
use crate::commands::registry::{create, grip, resolve};
use crate::model::annotation::AnnotationKind;
use crate::model::dimension::DimensionStyle;
use crate::model::document::{Document, DrawOrder, Layer, Layout, ObjectGroup};
use crate::model::editing::grip_points;
use crate::model::entities::{BlockRef, Entity, EntityId, Shape, VAlign};
use crate::model::geometry::Point;
use crate::model::index::Bounds;
use crate::selection::Selection;
use crate::selection::picking::{pick_entity, select_in_box};
use crate::snap::osnap::SnapKind;
use crate::snap::resolve::{SnapResult, SnapScene, SnapSettings, resolve_point};

pub const IDLE_PROMPT: &str = "Command:";
pub const IDLE_HINT: &str = "Type a command, e.g. line, circle, move";
const UNDO_WORDS: [&str; 2] = ["U", "UNDO"];
const REDO_WORDS: [&str; 1] = ["REDO"];
const SELECT_ALL_WORD: &str = "ALL";
/// SELECTALL and its alias: every object in the current space, as Ctrl+A.
const SELECT_ALL_COMMANDS: [&str; 2] = ["SELECTALL", "AI_SELALL"];
const SNAP_APERTURE_PIXELS: f64 = 12.0;
const PICK_APERTURE_PIXELS: f64 = 8.0;
const DRAG_THRESHOLD_PIXELS: f64 = 4.0;
const MAX_HISTORY_LINES: usize = 500;

fn is_point_prompt(kind: PromptKind) -> bool {
    matches!(kind, PromptKind::Point | PromptKind::Number | PromptKind::Length | PromptKind::Angle | PromptKind::Window)
}

const GRIP_PIXELS: f64 = 6.0;
const ACQUIRE_AFTER: std::time::Duration = std::time::Duration::from_millis(400);
const MAX_ACQUIRED: usize = 5;
/// With more objects selected than this, grips are not shown: they would cover the drawing.
const MAX_GRIP_OBJECTS: usize = 100;
const MAX_RECENT_COMMANDS: usize = 8;

fn is_selecting_prompt(kind: PromptKind) -> bool {
    matches!(kind, PromptKind::Select | PromptKind::Pick)
}

/// The document with its undo history: what a running command acts on. Every change goes through the undo stack.
#[derive(Default)]
pub struct Drawing {
    pub document: Document,
    pub undo: UndoStack,
    /// The command line history, oldest first.
    pub log: Vec<String>,
    /// A change of view a command asked for, for the window to make.
    pub view_request: Option<ViewRequest>,
    /// The undo history of each space that is not open: model space (None) or a layout.
    stashed_undo: HashMap<Option<usize>, UndoStack>,
    /// The undo history of each deleted layout, by its name, for undoing its deletion to bring back.
    parked_undo: HashMap<String, UndoStack>,
    /// Layouts were added, renamed or deleted since the drawing was saved.
    layouts_changed: bool,
    /// A layout change a command asked for, made once the command step is done.
    pub layout_request: Option<LayoutRequest>,
    /// The drawing's undo history while the block editor has one of its own.
    block_undo: Option<UndoStack>,
}

impl Drawing {
    /// Whether everything is as saved, in every space.
    pub fn is_clean(&self) -> bool {
        self.undo.is_clean() && self.stashed_undo.values().all(UndoStack::is_clean) && !self.layouts_changed
    }

    /// Something outside the undo history changed (purged blocks, units), so the drawing needs saving.
    pub fn note_change(&mut self) {
        self.layouts_changed = true;
    }

    pub fn set_clean(&mut self) {
        self.undo.set_clean();
        self.stashed_undo.values_mut().for_each(UndoStack::set_clean);
        self.layouts_changed = false;
    }

    pub fn print(&mut self, message: impl Into<String>) {
        self.log.push(message.into());
        if self.log.len() > MAX_HISTORY_LINES {
            self.log.drain(..self.log.len() - MAX_HISTORY_LINES);
        }
    }

    /// Make a change, and let associative hatches follow it, undoing together.
    pub fn push(&mut self, change: Change) {
        let (mut ids, mut areas) = (vec![], vec![]);
        change.touched(&self.document, &mut ids, &mut areas);
        let followers =
            !ids.is_empty() && self.document.entities().iter().any(|e| matches!(e.shape, Shape::Hatch { association: Some(_), .. }));
        if !followers {
            self.undo.push(change, &mut self.document);
            return;
        }
        self.undo.begin_group();
        self.undo.push(change, &mut self.document);
        let states = crate::model::boundary::hatches_following(&self.document, &ids, &areas);
        if !states.is_empty() {
            let follow = Change::modify(&self.document, states);
            self.undo.push(follow, &mut self.document);
        }
        self.undo.end_group();
    }
}

impl Host for Drawing {
    fn view(&mut self, request: ViewRequest) {
        self.view_request = Some(request);
    }

    fn visible_in(&self, area: &Bounds) -> Vec<Entity> {
        self.document.entities_in(area).into_iter().filter(|e| self.document.is_visible(e)).collect()
    }

    fn extent(&self) -> Option<Bounds> {
        self.document.extent()
    }

    fn wipeout_frame(&mut self, mode: u8) {
        self.document.wipeout_frame = mode;
        self.note_change();
    }

    fn linetype_scale(&mut self, setting: ScaleSetting) {
        match setting {
            ScaleSetting::Global(scale) => self.document.ltscale = scale,
            ScaleSetting::Paper(on) => self.document.psltscale = on,
            ScaleSetting::Current(scale) => self.document.celtscale = scale,
        }
        self.note_change();
    }

    fn draw_order(&mut self, objects: Vec<Entity>, placement: DrawOrder) {
        let ids: Vec<EntityId> = objects.iter().map(|e| e.id).collect();
        let change = Change::Reorder { before: self.document.order(), after: self.document.reordered(&ids, &placement) };
        self.push(change);
    }

    fn layers(&mut self, layers: Vec<Layer>, current: Option<String>) {
        let before =
            LayerState { layers: self.document.layers.clone(), current: self.document.current_layer.clone(), entity_layers: vec![] };
        let after = LayerState { layers, current: current.unwrap_or_else(|| before.current.clone()), entity_layers: vec![] };
        self.push(Change::Layers { before, after });
    }

    fn groups(&mut self, groups: Vec<ObjectGroup>) {
        let change = Change::Groups { before: self.document.groups.clone(), after: groups };
        self.push(change);
    }

    fn layout(&mut self, request: LayoutRequest) {
        self.layout_request = Some(request);
    }

    fn dimension_style(&mut self, style: Option<DimensionStyle>, current: Option<String>) {
        let mut styles = self.document.dimension_styles.clone();
        let mut changed = vec![];
        if let Some(style) = style {
            match styles.iter().position(|s| s.name.eq_ignore_ascii_case(&style.name)) {
                Some(at) => {
                    changed = self.document.restyled_dimensions(&styles[at], &style);
                    styles[at] = style;
                }
                None => styles.push(style),
            }
        }
        let current = current.unwrap_or_else(|| self.document.current_dimension_style.clone());
        let mut changes = vec![Change::dimension_styles(&self.document, styles, current)];
        if !changed.is_empty() {
            changes.push(Change::modify(&self.document, changed));
        }
        self.push(Change::Group(changes));
    }

    fn add(&mut self, mut entities: Vec<Entity>) {
        // Objects already in the drawing stay as they are, such as ones OOPS brings back after their ERASE was undone.
        entities.retain(|e| !self.document.contains(e.id));
        if !entities.is_empty() {
            self.push(Change::Add(entities));
        }
    }

    fn add_new(&mut self, mut entities: Vec<Entity>) {
        for entity in &mut entities {
            entity.layer = self.document.current_layer.clone();
            entity.color = self.document.current_color;
            entity.linetype = self.document.current_linetype.clone();
            entity.lineweight = self.document.current_lineweight;
            entity.linetype_scale = self.document.celtscale;
        }
        self.add(entities);
    }

    fn remove(&mut self, entities: Vec<Entity>) {
        self.push(Change::remove(entities));
    }

    fn modify(&mut self, states: Vec<Entity>) {
        let change = Change::modify(&self.document, states);
        self.push(change);
    }

    fn retract(&mut self) {
        self.undo.undo(&mut self.document);
    }

    fn begin_group(&mut self) {
        self.undo.begin_group();
    }

    fn end_group(&mut self) {
        self.undo.end_group();
    }

    fn report(&mut self, message: String) {
        self.print(message);
    }
}

pub struct Controller {
    pub drawing: Drawing,
    pub defaults: Rc<RefCell<Defaults>>,
    pub selection: Selection,
    pub snap: SnapSettings,
    /// Pixels per drawing unit, kept up to date by the view. Apertures and the grid follow it.
    pub scale: f64,
    session: Option<Session>,
    running: Option<&'static str>,
    last_command: Option<&'static str>,
    /// The commands run lately, latest first, for the right-click menu's Recent Input.
    recent_commands: Vec<&'static str>,
    /// Multiline text double-clicked, for the window to open in its editor.
    pub edit_text_request: Option<EntityId>,
    /// A one-time object snap (from Shift+right-click or typing END, MID…) for the next point only.
    snap_override: Option<SnapKind>,
    raw_cursor: Option<Point>,
    /// The cursor after snapping, and the object snap that moved it there.
    pub cursor: Option<SnapResult>,
    press: Option<Point>,
    /// The rubber-band box being dragged, first corner first.
    pub selection_box: Option<(Point, Point)>,
    pub preview: Vec<Entity>,
    /// Snap points acquired for object snap tracking, and the one the cursor rests on with since when.
    pub acquired: Vec<Point>,
    hovered: Option<(Point, std::time::Instant)>,
}

impl Default for Controller {
    fn default() -> Self {
        Controller::new()
    }
}

impl Controller {
    pub fn new() -> Controller {
        Controller {
            drawing: Drawing::default(),
            defaults: Rc::new(RefCell::new(Defaults::default())),
            selection: Selection::default(),
            snap: SnapSettings::default(),
            scale: 4.0,
            session: None,
            running: None,
            last_command: None,
            recent_commands: vec![],
            snap_override: None,
            edit_text_request: None,
            raw_cursor: None,
            cursor: None,
            press: None,
            selection_box: None,
            preview: vec![],
            acquired: vec![],
            hovered: None,
        }
    }

    pub fn document(&self) -> &Document {
        &self.drawing.document
    }

    /// Open model space (None) or a layout, with its own undo history.
    pub fn switch_space(&mut self, target: Option<usize>) {
        if self.drawing.document.editing_block().is_some() {
            self.drawing.print("Close the block editor first (BCLOSE).");
            return;
        }
        let from = self.drawing.document.active_layout();
        let through = self.drawing.document.through().is_some();
        if (target == from && !through) || target.is_some_and(|i| i >= self.drawing.document.layouts.len()) {
            return;
        }
        if through && target.is_none() {
            // Model space is open already, through the viewport: only the layout leaves the screen.
            self.stop_session();
            self.selection.clear();
            self.drawing.document.activate(None);
            self.refresh();
            return;
        }
        self.stop_session();
        self.selection.clear();
        let open = std::mem::take(&mut self.drawing.undo);
        self.drawing.stashed_undo.insert(from, open);
        self.drawing.document.activate(target);
        self.drawing.undo = self.drawing.stashed_undo.remove(&target).unwrap_or_else(|| {
            let mut fresh = UndoStack::default();
            fresh.set_clean();
            fresh
        });
        self.refresh();
    }

    /// MSPACE: work on model space through a viewport of the open layout, with model space's undo history.
    /// From another viewport of the layout, that one becomes the one worked through, the command going on.
    pub fn enter_viewport(&mut self, viewport: EntityId) -> bool {
        let document = &self.drawing.document;
        if document.editing_block().is_some() || document.through().is_some_and(|t| t.viewport == viewport) {
            return false;
        }
        let from = document.active_layout();
        if from.is_some() {
            self.stop_session();
            self.selection.clear();
        }
        if !self.drawing.document.enter_viewport(viewport) {
            return false;
        }
        if from.is_some() {
            self.swap_undo(from, None);
        }
        self.refresh();
        true
    }

    /// PSPACE: back to the layout's paper space from a viewport.
    pub fn leave_viewport(&mut self) {
        let Some(through) = self.drawing.document.through() else { return };
        self.stop_session();
        self.selection.clear();
        self.drawing.document.leave_viewport();
        self.swap_undo(None, Some(through.layout));
        self.refresh();
    }

    /// Put away one space's undo history and take out another's.
    fn swap_undo(&mut self, from: Option<usize>, to: Option<usize>) {
        let open = std::mem::take(&mut self.drawing.undo);
        self.drawing.stashed_undo.insert(from, open);
        self.drawing.undo = self.drawing.stashed_undo.remove(&to).unwrap_or_else(|| {
            let mut fresh = UndoStack::default();
            fresh.set_clean();
            fresh
        });
    }

    /// Open a block in the block editor, with an undo history of its own.
    pub fn begin_block_edit(&mut self, block: BlockRef) {
        if self.drawing.document.editing_block().is_some() {
            return;
        }
        self.leave_viewport();
        self.stop_session();
        self.selection.clear();
        let mut fresh = UndoStack::default();
        fresh.set_clean();
        self.drawing.block_undo = Some(std::mem::replace(&mut self.drawing.undo, fresh));
        self.drawing.document.begin_block_edit(block);
        self.refresh();
    }

    /// Close the block editor, saving the block or not, and go back to where it was opened from.
    pub fn end_block_edit(&mut self, save: bool) -> Option<BlockRef> {
        self.drawing.document.editing_block()?;
        self.stop_session();
        self.selection.clear();
        let saved = self.drawing.document.end_block_edit(save);
        if let Some(undo) = self.drawing.block_undo.take() {
            self.drawing.undo = undo;
        }
        if saved.is_some() {
            self.drawing.note_change();
        }
        self.refresh();
        saved
    }

    /// Add a layout with a fresh A3 sheet and open it.
    pub fn new_layout(&mut self, name: &str) -> Result<usize, String> {
        self.check_layout_name(name, None)?;
        self.drawing.document.layouts.push(Layout::new(name.trim()));
        self.drawing.layouts_changed = true;
        let index = self.drawing.document.layouts.len() - 1;
        self.switch_space(Some(index));
        Ok(index)
    }

    pub fn rename_layout(&mut self, index: usize, name: &str) -> Result<(), String> {
        self.check_layout_name(name, Some(index))?;
        let layout = self.drawing.document.layouts.get(index).ok_or("There is no such layout.")?;
        let (before, after) = (layout.name.clone(), name.trim().to_string());
        if before != after {
            self.push(Change::LayoutRenamed { before, after });
        }
        Ok(())
    }

    /// Delete a layout and what is on it. A drawing keeps at least one.
    pub fn delete_layout(&mut self, index: usize) -> Result<(), String> {
        if self.drawing.document.layouts.len() <= 1 {
            return Err("A drawing keeps at least one layout.".into());
        }
        if self.drawing.document.active_layout() == Some(index) {
            self.switch_space(None);
        }
        // An undoable change in the history of the space that is open, so U there brings the layout back whole.
        let layout = self.drawing.document.layouts.get(index).ok_or("There is no such layout.")?.saved();
        let names = self.layout_names();
        self.drawing.push(Change::LayoutRemoved { index, layout });
        self.follow_layouts(&names);
        self.refresh();
        Ok(())
    }

    fn layout_names(&self) -> Vec<String> {
        self.drawing.document.layouts.iter().map(|l| l.name.clone()).collect()
    }

    /// Keep each layout's undo history with its layout after one was taken out or put back: the histories are
    /// kept by the layouts' places, which have moved. `before` is the layouts' names before the change.
    fn follow_layouts(&mut self, before: &[String]) {
        let after = self.layout_names();
        let at = before.iter().zip(&after).position(|(a, b)| a != b).unwrap_or(before.len().min(after.len()));
        let mut stashed = std::mem::take(&mut self.drawing.stashed_undo);
        if after.len() + 1 == before.len() {
            // The deleted layout's history is kept by its name, to come back with it.
            if let Some(history) = stashed.remove(&Some(at)) {
                self.drawing.parked_undo.insert(before[at].clone(), history);
            }
            stashed = stashed.into_iter().map(|(space, stack)| (space.map(|i| if i > at { i - 1 } else { i }), stack)).collect();
        } else if after.len() == before.len() + 1 {
            stashed = stashed.into_iter().map(|(space, stack)| (space.map(|i| if i >= at { i + 1 } else { i }), stack)).collect();
            if let Some(history) = self.drawing.parked_undo.remove(&after[at]) {
                stashed.insert(Some(at), history);
            }
        }
        self.drawing.stashed_undo = stashed;
    }

    fn check_layout_name(&self, name: &str, renaming: Option<usize>) -> Result<(), String> {
        let name = name.trim();
        if name.is_empty() || name.eq_ignore_ascii_case("Model") || name.chars().any(|c| "<>/\\\":;?*|,=`".contains(c)) {
            return Err(format!("\"{name}\" cannot name a layout."));
        }
        let taken = self.drawing.document.layouts.iter().enumerate().any(|(i, l)| Some(i) != renaming && l.name.eq_ignore_ascii_case(name));
        if taken { Err(format!("There is already a layout named {name}.")) } else { Ok(()) }
    }

    /// Carry out a layout change a command asked for.
    pub fn apply_layout_request(&mut self) {
        let Some(request) = self.drawing.layout_request.take() else { return };
        let result = match request {
            LayoutRequest::New(name) => self.new_layout(&name).map(|_| ()),
            LayoutRequest::Rename(index, name) => self.rename_layout(index, &name),
            LayoutRequest::Delete(index) => self.delete_layout(index),
            LayoutRequest::Open(space) => {
                self.switch_space(space);
                Ok(())
            }
        };
        if let Err(message) = result {
            self.drawing.print(message);
        }
    }

    pub fn set_document(&mut self, document: Document) {
        self.stop_session();
        self.selection.clear();
        self.drawing.stashed_undo.clear();
        self.drawing.parked_undo.clear();
        self.drawing.layouts_changed = false;
        self.defaults.borrow_mut().dimension_look = document.current_dimension_look();
        self.drawing.document = document;
        self.drawing.undo.clear();
        self.drawing.undo.set_clean();
        self.refresh();
    }

    pub fn prompt(&self) -> Option<&Prompt> {
        self.session.as_ref().and_then(|s| s.prompt.as_ref())
    }

    /// The running command's name, or None when idle.
    pub fn running_command(&self) -> Option<&'static str> {
        self.running
    }

    /// The commands run lately, latest first.
    pub fn recent_commands(&self) -> &[&'static str] {
        &self.recent_commands
    }

    /// The prompt's options as its message names them ("[Center/End]"), each with what to type for it.
    pub fn prompt_options(&self) -> Vec<(String, String)> {
        let Some(prompt) = self.prompt() else { return vec![] };
        let message = &prompt.message;
        let Some(names) = message.find('[').and_then(|start| Some(&message[start + 1..start + message[start..].find(']')?])) else {
            return prompt.options.iter().map(|o| (o.clone(), o.clone())).collect();
        };
        names
            .split('/')
            .filter(|name| !name.trim().is_empty())
            .map(|name| {
                let name = name.trim();
                let typed = prompt.options.iter().find(|o| name.to_uppercase().starts_with(&o.to_uppercase())).cloned();
                (name.to_string(), typed.unwrap_or_else(|| name.to_string()))
            })
            .collect()
    }

    /// Whether the prompt asks for a point, where object snaps apply.
    pub fn wants_point(&self) -> bool {
        self.prompt().is_some_and(|p| is_point_prompt(p.kind))
    }

    /// Snap the next point to this kind of point only, whatever snaps are on.
    pub fn override_snap(&mut self, kind: SnapKind) {
        if self.wants_point() {
            self.snap_override = Some(kind);
            self.drawing.print(format!("{} of", kind.name()));
            self.reapply_cursor();
        }
    }

    /// Whether the small pick box shows in the crosshair: objects can be picked now.
    pub fn picking(&self) -> bool {
        self.prompt().is_none_or(|p| is_selecting_prompt(p.kind))
    }

    pub fn start_command(&mut self, name: &'static str) {
        self.stop_session();
        self.drawing.print(format!("Command: {name}"));
        self.last_command = Some(name);
        self.recent_commands.retain(|n| *n != name);
        self.recent_commands.insert(0, name);
        self.recent_commands.truncate(MAX_RECENT_COMMANDS);
        self.running = Some(name);
        self.defaults.borrow_mut().blocks = self.blocks();
        {
            let document = &self.drawing.document;
            let mut defaults = self.defaults.borrow_mut();
            defaults.dimension_styles = document.dimension_styles.clone();
            defaults.mline_styles = document.mline_styles.clone();
            // The style MLINE draws with takes any change made to it since.
            if let Some(style) = document.mline_styles.iter().find(|s| s.name.eq_ignore_ascii_case(&defaults.mline_style.name)) {
                defaults.mline_style = style.clone();
            }
            defaults.wipeout_frame = document.wipeout_frame;
            defaults.ltscale = document.ltscale;
            defaults.psltscale = document.psltscale;
            defaults.celtscale = document.celtscale;
            if let Some(style) = document.text_styles.iter().find(|s| s.name == document.current_text_style) {
                defaults.text_style = style.clone();
            }
            if let Some(style) = document.mleader_styles.iter().find(|s| s.name == document.current_mleader_style) {
                defaults.mleader_style = style.clone();
            }
            defaults.groups = document.groups.clone();
            defaults.layers = document.layers.clone();
            defaults.current_layer = document.current_layer.clone();
            defaults.layouts = document.layouts.iter().map(|l| l.name.clone()).collect();
            defaults.active_layout = document.active_layout();
            defaults.viewports = document
                .shown_layout()
                .map(|layout| {
                    let store = document.space_store(Some(layout));
                    store.entities().iter().filter(|e| matches!(e.shape, Shape::Viewport { .. })).cloned().collect()
                })
                .unwrap_or_default();
            defaults.through_viewport = document.through().map(|t| t.viewport);
            defaults.sheet = document.active_layout().map(|i| document.layouts[i].sheet());
            defaults.model_extent = document.model().extent();
        }
        let selected = self.selected();
        let build = create(name, selected, self.defaults.clone());
        self.session = Some(Session::start(build, &mut self.drawing));
        self.refresh();
    }

    /// The drawing's named blocks: those its file defines and those its inserts use, by name.
    pub fn blocks(&self) -> Vec<BlockRef> {
        let document = &self.drawing.document;
        let mut found: std::collections::BTreeMap<String, BlockRef> = document.blocks.clone();
        for entity in document.entities() {
            if let Shape::Insert { block, .. } = &entity.shape
                && !block.0.name.starts_with('*')
            {
                found.entry(block.0.name.to_uppercase()).or_insert_with(|| block.clone());
            }
        }
        for block in &self.defaults.borrow().blocks {
            found.entry(block.0.name.to_uppercase()).or_insert_with(|| block.clone());
        }
        found.into_values().collect()
    }

    /// Enter, from the keyboard or the right mouse button: answers the prompt as Enter does, or repeats the last
    /// command when none runs.
    pub fn enter(&mut self) {
        self.handle_input("");
    }

    /// The grips of the selected objects, when no command runs: which object, which grip, where.
    pub fn grips(&self) -> Vec<(EntityId, usize, Point)> {
        if self.session.is_some() || self.selection.len() > MAX_GRIP_OBJECTS {
            return vec![];
        }
        self.selected().iter().flat_map(|e| grip_points(&e.shape).into_iter().enumerate().map(move |(i, p)| (e.id, i, p))).collect()
    }

    /// The grip under a point, within a few pixels.
    pub fn grip_at(&self, point: Point) -> Option<(EntityId, usize, Point)> {
        self.grips()
            .into_iter()
            .filter(|(_, _, p)| p.dist(point) * self.scale <= GRIP_PIXELS)
            .min_by(|a, b| a.2.dist(point).total_cmp(&b.2.dist(point)))
    }

    /// Ctrl+C and Ctrl+X: COPYCLIP or CUTCLIP, on the selected objects or those it asks for.
    pub fn copy_selection(&mut self, cut: bool) {
        self.start_command(if cut { "CUTCLIP" } else { "COPYCLIP" });
    }

    /// A double click edits what was clicked: a text, an insert's attributes, a dimension's text.
    /// The object a click at a point would pick, if any.
    pub fn hit(&self, point: Point) -> Option<EntityId> {
        let aperture = PICK_APERTURE_PIXELS / self.scale;
        pick_entity(&self.editable_in(&Bounds::square_around(point, aperture)), point, aperture).map(|e| e.id)
    }

    pub fn double_click(&mut self, point: Point) {
        if self.session.is_some() {
            return;
        }
        let aperture = PICK_APERTURE_PIXELS / self.scale;
        let candidates = self.editable_in(&Bounds::square_around(point, aperture));
        // Inside a table's cell, edit the cell.
        if let Some(table) = candidates.iter().find(|e| crate::model::table::cell_at(&e.shape, point).is_some()) {
            self.defaults.borrow_mut().table_cell = Some((table.clone(), point));
            self.selection.clear();
            self.start_command("TABLEDIT");
            return;
        }
        let Some(entity) = pick_entity(&candidates, point, aperture).cloned() else { return };
        let editable = match &entity.shape {
            Shape::Text { .. } | Shape::Dimension { .. } | Shape::Multileader { .. } => true,
            Shape::Annotation { kind, .. } => !matches!(kind, AnnotationKind::Leader { .. }),
            Shape::Insert { attributes, .. } => !attributes.is_empty(),
            _ => false,
        };
        // Multiline text opens in the editor, as in AutoCAD; a single TEXT is edited on the command line.
        if let Shape::Text { align, text, formatted, .. } = &entity.shape
            && (align.v == VAlign::Top || text.contains('\n') || !formatted.is_empty())
            && entity.attribute.is_none()
        {
            self.selection.replace([entity.id]);
            self.edit_text_request = Some(entity.id);
            return;
        }
        if editable {
            self.selection.replace([entity.id]);
            self.start_command("TEXTEDIT");
        }
    }

    pub fn handle_input(&mut self, text: &str) {
        if self.session.is_some() {
            self.answer_with_text(text);
            return;
        }
        let word = text.trim().to_uppercase();
        if UNDO_WORDS.contains(&word.as_str()) {
            self.undo();
        } else if REDO_WORDS.contains(&word.as_str()) {
            self.redo();
        } else if word == SELECT_ALL_WORD || SELECT_ALL_COMMANDS.contains(&word.as_str()) {
            self.select_all();
        } else {
            let name = if word.is_empty() { self.last_command } else { resolve(&word) };
            match name {
                Some(name) => self.start_command(name),
                None if !word.is_empty() => self.drawing.print(format!("Unknown command: {word}")),
                None => {}
            }
        }
    }

    pub fn undo(&mut self) {
        let names = self.layout_names();
        if self.drawing.undo.undo(&mut self.drawing.document) {
            self.follow_layouts(&names);
            self.after_change();
        }
    }

    pub fn redo(&mut self) {
        let names = self.layout_names();
        if self.drawing.undo.redo(&mut self.drawing.document) {
            self.follow_layouts(&names);
            self.after_change();
        }
    }

    /// The selected objects where they are in the drawing, in the order they were selected: for looking at them
    /// without copying each one, which matters when a great many are selected.
    pub fn selected_in_place(&self) -> impl Iterator<Item = &Entity> {
        self.selection.ids().iter().filter_map(|id| self.drawing.document.get(*id))
    }

    /// The selected objects as they are now, in the order they were selected.
    pub fn selected(&self) -> Vec<Entity> {
        self.selection.ids().iter().filter_map(|id| self.drawing.document.get(*id).cloned()).collect()
    }

    /// Give every selected object new values, as one undo step.
    pub fn change_selected(&mut self, change: impl Fn(&mut Entity)) {
        let states: Vec<Entity> = self
            .selected()
            .into_iter()
            .filter_map(|old| {
                let mut new = old.clone();
                change(&mut new);
                (!new.same_state(&old)).then_some(new)
            })
            .collect();
        if !states.is_empty() {
            self.drawing.modify(states);
            self.after_change();
        }
    }

    /// A layer or other change made outside a command, through the undo stack.
    pub fn push(&mut self, change: Change) {
        self.drawing.push(change);
        self.after_change();
    }

    pub fn erase_selection(&mut self) {
        if self.session.is_none() && !self.selection.is_empty() {
            self.start_command("ERASE");
        }
    }

    /// Answer the prompt with what Enter gives, for a dialog that has saved what the user chose.
    pub fn accept_default(&mut self) {
        if let Some(session) = &mut self.session {
            session.submit_text("", &mut self.drawing);
        }
        self.refresh();
    }

    /// Esc: stop the running command, or when idle clear the selection.
    pub fn cancel(&mut self) {
        match &mut self.session {
            Some(session) => session.cancel(&mut self.drawing),
            None => self.selection.clear(),
        }
        self.refresh();
    }

    /// The left button went down on the drawing.
    pub fn click(&mut self, point: Point) {
        if self.session.is_none()
            && let Some((id, index, _)) = self.grip_at(point)
            && let Some(target) = self.drawing.document.get(id).cloned()
        {
            self.drawing.print("** STRETCH **");
            self.running = Some("GRIP");
            self.session = Some(Session::start(grip(target, index), &mut self.drawing));
            self.refresh();
            return;
        }
        match self.prompt().map(|p| (p.kind, p.message.clone())) {
            Some((PromptKind::Window, message)) => {
                let snapped = self.snap_point(point).point;
                self.drawing.print(format!("{message} {:.4},{:.4}", snapped.x, snapped.y));
                self.answer_window(snapped);
                self.refresh();
            }
            Some((kind, message)) if is_point_prompt(kind) => {
                let snapped = self.snap_point(point).point;
                self.snap_override = None;
                self.drawing.print(format!("{message} {:.4},{:.4}", snapped.x, snapped.y));
                if let Some(session) = &mut self.session {
                    session.submit_point(snapped, &mut self.drawing);
                }
                self.refresh();
            }
            Some((kind, _)) if !is_selecting_prompt(kind) => {}
            _ => self.press = Some(point),
        }
    }

    /// The left button came up: a click picks, a drag selects in a box.
    pub fn release(&mut self, point: Point, shift: bool) {
        let Some(press) = self.press.take() else { return };
        self.selection_box = None;
        if press.dist(point) * self.scale < DRAG_THRESHOLD_PIXELS {
            self.pick_at(press, shift);
        } else {
            self.pick_in_box(press, point, shift);
        }
        self.refresh();
    }

    pub fn move_cursor(&mut self, point: Option<Point>) {
        self.raw_cursor = point;
        if let (Some(press), Some(point)) = (self.press, point) {
            self.selection_box = Some((press, point));
        }
        self.reapply_cursor();
    }

    /// Snap the last cursor position again, after the prompt, drawing or snap settings changed.
    pub fn reapply_cursor(&mut self) {
        let Some(raw) = self.raw_cursor else {
            self.cursor = None;
            self.preview.clear();
            return;
        };
        let result = self.snap_point(raw);
        // A snap point held under the cursor a moment is acquired for tracking.
        match result.kind {
            Some(_) if self.hovered.is_none_or(|(p, _)| p != result.point) => {
                self.hovered = Some((result.point, std::time::Instant::now()))
            }
            Some(_) => {}
            None => self.hovered = None,
        }
        self.preview = match self.prompt().and_then(|p| p.preview.clone()) {
            Some(preview) => preview(result.point),
            None => vec![],
        };
        self.cursor = Some(result);
    }

    /// Call each frame: acquires a snap point once the cursor has rested on it long enough. True while one is
    /// waiting, so the window keeps checking.
    pub fn tick(&mut self) -> bool {
        let Some((point, since)) = self.hovered else { return false };
        if !self.snap.tracking || self.prompt().is_none_or(|p| !is_point_prompt(p.kind)) {
            return false;
        }
        if since.elapsed() < ACQUIRE_AFTER {
            return true;
        }
        self.hovered = None;
        // Resting on an acquired point again lets it go, as in AutoCAD.
        match self.acquired.iter().position(|a| *a == point) {
            Some(at) => {
                self.acquired.remove(at);
            }
            None => {
                self.acquired.push(point);
                if self.acquired.len() > MAX_ACQUIRED {
                    self.acquired.remove(0);
                }
            }
        }
        false
    }

    pub fn snap_marker(&self) -> Option<(Point, SnapKind)> {
        self.cursor.as_ref().and_then(|c| c.kind.map(|kind| (c.point, kind)))
    }

    fn stop_session(&mut self) {
        if let Some(mut session) = self.session.take() {
            session.cancel(&mut self.drawing);
        }
        self.running = None;
        self.acquired.clear();
        self.snap_override = None;
    }

    fn snap_point(&self, cursor: Point) -> SnapResult {
        let Some(prompt) = self.prompt().filter(|p| is_point_prompt(p.kind)) else {
            return SnapResult { point: cursor, kind: None, tracked: vec![] };
        };
        let aperture = SNAP_APERTURE_PIXELS / self.scale;
        let near = self.drawing.document.entities_in(&Bounds::square_around(cursor, aperture));
        let scene = SnapScene { aperture, grid_step: grid_spacing(self.scale), entities: &near, acquired: &self.acquired };
        match self.snap_override {
            // An override snaps to its kind alone, even with running snaps off.
            Some(kind) => {
                let settings = SnapSettings { osnap: true, kinds: vec![kind], tracking: false, ..self.snap.clone() };
                resolve_point(cursor, prompt.base, &settings, &scene)
            }
            None => resolve_point(cursor, prompt.base, &self.snap, &scene),
        }
    }

    fn editable_in(&self, area: &Bounds) -> Vec<Entity> {
        let document = &self.drawing.document;
        document.entities_in(area).into_iter().filter(|e| document.is_editable(e)).collect()
    }

    fn pick_at(&mut self, point: Point, shift: bool) {
        let aperture = PICK_APERTURE_PIXELS / self.scale;
        let candidates = self.editable_in(&Bounds::square_around(point, aperture));
        let entity = pick_entity(&candidates, point, aperture).cloned();
        if self.prompt().is_some_and(|p| p.kind == PromptKind::Pick) {
            if let (Some(entity), Some(session)) = (entity, &mut self.session) {
                session.submit_pick(entity, point, shift, &mut self.drawing);
            }
            return;
        }
        self.change_selection(entity.into_iter().collect(), shift);
    }

    fn pick_in_box(&mut self, first: Point, second: Point, shift: bool) {
        if self.prompt().is_none_or(|p| p.kind == PromptKind::Select) {
            let candidates = self.editable_in(&Bounds::from_corners(first, second));
            let found = select_in_box(&candidates, first, second);
            self.change_selection(found, shift);
        }
    }

    /// Apply a click or box to the selection. Shift toggles. An empty pick clears it, but not mid-command.
    fn change_selection(&mut self, found: Vec<Entity>, shift: bool) {
        let in_command = self.prompt().is_some();
        let ids: Vec<EntityId> = found.iter().map(|e| e.id).collect();
        // GROUPEDIT picks single members to add or remove, whatever group selection says.
        let ids = if self.running == Some("GROUPEDIT") { ids } else { self.drawing.document.with_groups(ids) };
        if shift {
            let (toggled, fresh): (Vec<EntityId>, Vec<EntityId>) = ids.iter().partition(|id| self.selection.contains(**id));
            self.selection.add(fresh);
            self.selection.remove(toggled);
        } else if in_command {
            self.selection.add(ids.iter().copied());
        } else if !ids.is_empty() {
            self.selection.replace(ids.iter().copied());
        } else {
            self.selection.clear();
        }
        if !ids.is_empty() && in_command {
            let total = self.selection.len();
            self.drawing.print(format!("{} found, {total} total", ids.len()));
        }
    }

    pub fn select_all(&mut self) {
        let everything: Vec<EntityId> = self.drawing.document.editable_entities().iter().map(|e| e.id).collect();
        let count = everything.len();
        self.selection.add(everything);
        self.drawing.print(format!("{count} found"));
    }

    /// Finish a window prompt: the objects a crossing window from its base to this corner touches.
    fn answer_window(&mut self, second: Point) {
        let Some(first) = self.prompt().and_then(|p| p.base) else { return };
        let window = Bounds::from_corners(first, second);
        let crossing_from_right = crate::model::geometry::pt(window.x1, window.y1);
        let crossing_to_left = crate::model::geometry::pt(window.x0, window.y0);
        let objects = select_in_box(&self.editable_in(&window), crossing_from_right, crossing_to_left);
        if let Some(session) = &mut self.session {
            session.submit_window(second, objects, &mut self.drawing);
        }
    }

    fn answer_with_text(&mut self, text: &str) {
        let Some((kind, message, or_all)) = self.prompt().map(|p| (p.kind, p.message.clone(), p.or_all)) else { return };
        self.drawing.print(format!("{message} {text}"));
        // END, MID and the like at a point prompt snap the next point only, as in AutoCAD.
        // The prompt's own options come first: CENTER at "[Center/End]" picks the option.
        let is_option = self.prompt().is_some_and(|p| p.options.iter().any(|o| text.trim().to_uppercase().starts_with(&o.to_uppercase())));
        if is_point_prompt(kind)
            && !is_option
            && let Some(snap) = SnapKind::from_override(text)
        {
            self.snap_override = Some(snap);
            self.drawing.print(format!("{} of", snap.name()));
            self.reapply_cursor();
            return;
        }
        self.snap_override = None;
        if kind == PromptKind::Window {
            match crate::commands::parse::parse_point(text, self.prompt().and_then(|p| p.base)) {
                Some(point) => self.answer_window(point),
                None if text.trim().is_empty() => {
                    if let Some(session) = &mut self.session {
                        session.cancel(&mut self.drawing);
                    }
                }
                None => self.drawing.print("Enter the other corner as x,y or @dx,dy."),
            }
        } else if kind == PromptKind::Select {
            self.answer_select_prompt(or_all, &text.trim().to_uppercase());
        } else if let Some(session) = &mut self.session
            && let Some(error) = session.submit_text(text, &mut self.drawing)
        {
            self.drawing.print(error);
        }
        self.refresh();
    }

    fn answer_select_prompt(&mut self, or_all: bool, word: &str) {
        if word == SELECT_ALL_WORD {
            self.select_all();
            return;
        }
        if !word.is_empty() {
            self.drawing.print(SELECT_HINT);
            return;
        }
        let objects = if self.selection.is_empty() && or_all { self.drawing.document.visible_entities() } else { self.selected() };
        let Some(session) = &mut self.session else { return };
        if objects.is_empty() {
            session.cancel(&mut self.drawing);
        } else {
            session.submit_objects(objects, &mut self.drawing);
        }
    }

    /// After undo, redo or any other change: forget selected objects that are gone or can no longer be edited.
    pub fn after_change(&mut self) {
        let document = &self.drawing.document;
        self.selection.keep_only(|id| document.get(id).is_some_and(|e| document.is_editable(e)));
        self.reapply_cursor();
    }

    fn refresh(&mut self) {
        if self.session.as_ref().is_some_and(|s| !s.active()) {
            self.session = None;
            self.running = None;
            self.selection.clear();
        }
        self.after_change();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::entities::Association;
    use crate::model::entities::{circle, line};
    use crate::model::geometry::pt;

    fn run(controller: &mut Controller, inputs: &[&str]) {
        for input in inputs {
            controller.handle_input(input);
        }
    }

    fn shapes(controller: &Controller) -> Vec<Shape> {
        controller.document().entities().iter().map(|e| e.shape.clone()).collect()
    }

    fn controller_with(entities: Vec<Entity>) -> Controller {
        let mut controller = Controller::new();
        controller.set_document(Document::with_entities(entities));
        controller.scale = 10.0;
        controller
    }

    #[test]
    fn typed_commands_draw_and_undo() {
        let mut controller = controller_with(vec![]);
        for input in ["l", "0,0", "10,0", ""] {
            controller.handle_input(input);
        }
        assert_eq!(controller.document().len(), 1);
        assert!(controller.running_command().is_none());
        assert!(!controller.drawing.undo.is_clean());
        controller.handle_input("u");
        assert!(controller.document().is_empty());
        controller.handle_input("redo");
        assert_eq!(controller.document().len(), 1);
        controller.handle_input("");
        assert_eq!(controller.running_command(), Some("LINE"), "Enter repeats the last command");
    }

    #[test]
    fn new_objects_take_the_current_layer_and_look() {
        let mut controller = controller_with(vec![]);
        controller.drawing.document.layers.push(crate::model::document::Layer::new("Walls"));
        controller.drawing.document.current_layer = "Walls".into();
        controller.drawing.document.current_color = 3;
        for input in ["c", "0,0", "5"] {
            controller.handle_input(input);
        }
        let circle = &controller.document().entities()[0];
        assert_eq!((circle.layer.as_str(), circle.color), ("Walls", 3));
    }

    #[test]
    fn clicking_picks_and_dragging_selects() {
        let mut controller = controller_with(vec![line(pt(0.0, 0.0), pt(10.0, 0.0)), line(pt(0.0, 5.0), pt(10.0, 5.0))]);
        controller.click(pt(5.0, 0.1));
        controller.release(pt(5.0, 0.1), false);
        assert_eq!(controller.selection.len(), 1);
        controller.click(pt(-1.0, -1.0));
        controller.move_cursor(Some(pt(11.0, 6.0)));
        assert!(controller.selection_box.is_some());
        controller.release(pt(11.0, 6.0), false);
        assert_eq!(controller.selection.len(), 2);
        assert!(controller.selection_box.is_none());
        controller.cancel();
        assert!(controller.selection.is_empty());
    }

    #[test]
    fn delete_erases_the_selection() {
        let mut controller = controller_with(vec![line(pt(0.0, 0.0), pt(10.0, 0.0))]);
        controller.handle_input("all");
        controller.erase_selection();
        assert!(controller.document().is_empty());
        assert!(controller.selection.is_empty());
    }

    #[test]
    fn clicks_snap_to_endpoints_while_a_command_asks_for_a_point() {
        let mut controller = controller_with(vec![line(pt(0.0, 0.0), pt(10.0, 0.0))]);
        controller.handle_input("line");
        controller.move_cursor(Some(pt(10.3, 0.4)));
        assert_eq!(controller.snap_marker(), Some((pt(10.0, 0.0), SnapKind::Endpoint)));
        controller.click(pt(10.3, 0.4));
        controller.click(pt(20.0, 20.0));
        let Shape::Line { start, .. } = controller.document().entities()[1].shape else { panic!() };
        assert_eq!(start, pt(10.0, 0.0));
        assert!(!controller.preview.is_empty(), "the next segment follows the cursor");
    }

    #[test]
    fn select_prompt_takes_the_selection_on_enter() {
        let mut controller = controller_with(vec![line(pt(0.0, 0.0), pt(10.0, 0.0))]);
        controller.handle_input("move");
        assert_eq!(controller.prompt().unwrap().kind, PromptKind::Select);
        controller.click(pt(5.0, 0.0));
        controller.release(pt(5.0, 0.0), false);
        controller.handle_input("");
        assert_eq!(controller.prompt().unwrap().message, "Specify base point:");
        for input in ["0,0", "0,10"] {
            controller.handle_input(input);
        }
        let Shape::Line { start, .. } = controller.document().entities()[0].shape else { panic!() };
        assert_eq!(start, pt(0.0, 10.0));
        assert!(controller.selection.is_empty(), "a finished command clears the selection");
    }

    #[test]
    fn unknown_words_are_reported() {
        let mut controller = controller_with(vec![]);
        controller.handle_input("frobnicate");
        assert_eq!(controller.drawing.log.last().unwrap(), "Unknown command: FROBNICATE");
    }

    #[test]
    fn stretch_moves_the_ends_inside_the_window() {
        let mut controller = controller_with(vec![line(pt(0.0, 0.0), pt(10.0, 0.0)), circle(pt(0.0, 5.0), 1.0)]);
        run(&mut controller, &["s", "11,-1", "9,1", "0,0", "5,0"]);
        assert_eq!(shapes(&controller)[0], Shape::Line { start: pt(0.0, 0.0), end: pt(15.0, 0.0) });
        assert_eq!(shapes(&controller)[1], Shape::Circle { center: pt(0.0, 5.0), radius: 1.0 }, "the circle was outside");
    }

    #[test]
    fn rectangular_and_polar_arrays() {
        let mut controller = controller_with(vec![circle(pt(0.0, 0.0), 1.0)]);
        run(&mut controller, &["all", "ar", "", "2", "3", "5", "4"]);
        assert_eq!(controller.document().len(), 6);
        assert!(shapes(&controller).contains(&Shape::Circle { center: pt(8.0, 5.0), radius: 1.0 }));
        run(&mut controller, &["u", "all", "ar", "p", "0,10", "4", ""]);
        assert_eq!(controller.document().len(), 4);
    }

    #[test]
    fn overkill_waits_for_the_dialog_and_cancel_changes_nothing() {
        let a = line(pt(0.0, 0.0), pt(10.0, 0.0));
        let mut controller = controller_with(vec![a.clone(), a.duplicate()]);
        run(&mut controller, &["all", "overkill"]);
        assert_eq!(controller.prompt().map(|p| p.kind), Some(PromptKind::OverkillDialog));
        assert_eq!(controller.document().len(), 2, "nothing changes until the dialog answers");
        controller.cancel();
        assert!(controller.running_command().is_none());
        assert_eq!(controller.document().len(), 2);

        run(&mut controller, &["all", "overkill"]);
        controller.defaults.borrow_mut().overkill.tolerance = 0.5;
        controller.accept_default();
        assert!(controller.running_command().is_none());
        assert_eq!(shapes(&controller), vec![Shape::Line { start: pt(0.0, 0.0), end: pt(10.0, 0.0) }]);
        assert_eq!(controller.defaults.borrow().overkill.tolerance, 0.5, "the settings the dialog saved are kept");
    }

    #[test]
    fn join_then_explode() {
        let mut controller = controller_with(vec![line(pt(0.0, 0.0), pt(5.0, 0.0)), line(pt(5.0, 0.0), pt(5.0, 5.0))]);
        run(&mut controller, &["all", "j"]);
        assert!(matches!(shapes(&controller)[..], [Shape::Polyline { .. }]));
        run(&mut controller, &["all", "x"]);
        assert_eq!(
            shapes(&controller),
            vec![Shape::Line { start: pt(0.0, 0.0), end: pt(5.0, 0.0) }, Shape::Line { start: pt(5.0, 0.0), end: pt(5.0, 5.0) },]
        );
    }

    #[test]
    fn a_hatch_follows_its_boundary_and_undo_takes_both_back() {
        let mut controller = controller_with(vec![]);
        run(&mut controller, &["c", "0,0", "5", "all", "h", "solid", ""]);
        controller.selection.clear();
        let circle_id = controller.document().entities()[0].id;
        let loops_of = |c: &Controller| match shapes(c).into_iter().last() {
            Some(Shape::Hatch { loops, association, .. }) => (loops, association),
            _ => panic!("no hatch"),
        };
        assert_eq!(loops_of(&controller).1, Some(Association::Objects(vec![circle_id])));
        let before = loops_of(&controller).0;
        let mut moved = controller.document().get(circle_id).unwrap().clone();
        moved.shape = Shape::Circle { center: pt(100.0, 0.0), radius: 5.0 };
        controller.drawing.modify(vec![moved]);
        let (after, _) = loops_of(&controller);
        assert!(after[0].iter().all(|p| p.x > 90.0), "the hatch went with the circle");
        controller.undo();
        assert_eq!(loops_of(&controller).0, before, "one undo takes both back");
    }

    #[test]
    fn a_hatch_picked_inside_lines_finds_its_area_again() {
        let mut controller = controller_with(vec![]);
        run(&mut controller, &["l", "0,0", "10,0", "10,10", "0,10", "c", "h", "solid", "5,5", ""]);
        let line_id = controller.document().entities()[1].id;
        let mut wider = controller.document().get(line_id).unwrap().clone();
        wider.shape = Shape::Line { start: pt(20.0, 0.0), end: pt(20.0, 10.0) };
        // The sides reaching the far corner stay as they were, so the area is no longer closed in.
        controller.drawing.modify(vec![wider]);
        let Some(Shape::Hatch { association, .. }) = shapes(&controller).into_iter().last() else { panic!() };
        assert_eq!(association, None, "with its boundary open it stops following");
    }

    #[test]
    fn hatch_a_circle_and_a_rectangle_as_one_area_with_an_island() {
        let mut controller = controller_with(vec![]);
        run(&mut controller, &["rec", "0,0", "10,10", "c", "5,5", "2", "all", "h", "ansi31", "", ""]);
        let Some(Shape::Hatch { loops, solid, pattern, .. }) = shapes(&controller).into_iter().last() else { panic!() };
        assert!(!solid && loops.len() == 2 && pattern.len() == 1);
    }

    #[test]
    fn make_a_block_then_insert_it() {
        let mut controller = controller_with(vec![line(pt(0.0, 0.0), pt(1.0, 0.0)), circle(pt(0.0, 0.0), 1.0)]);
        run(&mut controller, &["all", "b", "Bolt", "0,0"]);
        assert!(matches!(&shapes(&controller)[..], [Shape::Insert { .. }]));
        run(&mut controller, &["i", "bolt", "10,0", "2", "90"]);
        let Shape::Insert { block, position, scale_x, rotation, .. } = &shapes(&controller)[1] else { panic!() };
        assert_eq!((block.0.name.as_str(), *position, *scale_x, *rotation), ("Bolt", pt(10.0, 0.0), 2.0, 90.0));
    }

    #[test]
    fn selectall_then_cutclip_erases_and_undo_brings_back() {
        let mut controller = controller_with(vec![circle(pt(5.0, 5.0), 1.0), circle(pt(9.0, 5.0), 1.0)]);
        run(&mut controller, &["selectall", "cutclip"]);
        assert!(shapes(&controller).is_empty());
        run(&mut controller, &["u"]);
        assert_eq!(shapes(&controller).len(), 2);
        assert_eq!(controller.defaults.borrow().clipboard.len(), 2);
    }

    #[test]
    fn copy_and_paste() {
        let mut controller = controller_with(vec![circle(pt(5.0, 5.0), 1.0)]);
        run(&mut controller, &["all"]);
        controller.copy_selection(false);
        run(&mut controller, &["pasteclip", "100,100"]);
        assert_eq!(shapes(&controller)[1], Shape::Circle { center: pt(101.0, 101.0), radius: 1.0 }, "pasted by the copy's lower left");
    }

    #[test]
    fn dragging_a_grip_stretches_the_object() {
        let mut controller = controller_with(vec![line(pt(0.0, 0.0), pt(10.0, 0.0))]);
        controller.click(pt(5.0, 0.0));
        controller.release(pt(5.0, 0.0), false);
        assert_eq!(controller.grips().len(), 3);
        controller.click(pt(10.05, 0.0));
        assert!(controller.prompt().is_some_and(|p| p.message == "Specify stretch point:"));
        controller.click(pt(10.0, 10.0));
        assert_eq!(shapes(&controller)[0], Shape::Line { start: pt(0.0, 0.0), end: pt(10.0, 10.0) });
    }

    #[test]
    fn double_clicking_text_edits_it() {
        let mut controller = controller_with(vec![crate::model::entities::text(pt(0.0, 0.0), "Old", 2.0, 0.0)]);
        controller.double_click(pt(1.0, 1.0));
        assert_eq!(controller.running_command(), Some("TEXTEDIT"));
        run(&mut controller, &["New\\Pline"]);
        let Shape::Text { text, .. } = &shapes(&controller)[0] else { panic!() };
        assert_eq!(text, "New\nline");
    }

    #[test]
    fn break_and_chamfer() {
        let mut controller = controller_with(vec![line(pt(0.0, 0.0), pt(10.0, 0.0))]);
        controller.handle_input("br");
        controller.click(pt(3.0, 0.0));
        controller.release(pt(3.0, 0.0), false);
        run(&mut controller, &["7,0"]);
        assert_eq!(controller.document().len(), 2);
        let mut controller = controller_with(vec![line(pt(0.0, 0.0), pt(10.0, 0.0)), line(pt(12.0, -2.0), pt(12.0, 10.0))]);
        run(&mut controller, &["cha", "d", "1", "1"]);
        for at in [pt(2.0, 0.0), pt(12.0, 8.0)] {
            controller.click(at);
            controller.release(at, false);
        }
        assert_eq!(controller.document().len(), 3, "the two lines and the bevel");
    }

    #[test]
    fn zoom_asks_the_window_for_a_new_view() {
        let mut controller = controller_with(vec![]);
        run(&mut controller, &["z", "e"]);
        assert_eq!(controller.drawing.view_request, Some(ViewRequest::Extents));
    }

    #[test]
    fn each_space_keeps_its_own_objects_and_undo() {
        let mut controller = Controller::new();
        controller.handle_input("LINE");
        for point in ["0,0", "10,0", ""] {
            controller.handle_input(point);
        }
        assert_eq!(controller.document().len(), 1);
        controller.switch_space(Some(0));
        assert_eq!(controller.document().len(), 0, "the layout starts empty");
        controller.handle_input("U");
        assert_eq!(controller.document().model().entities().len(), 1, "undo on the layout leaves model space be");
        controller.handle_input("CIRCLE");
        for answer in ["5,5", "2"] {
            controller.handle_input(answer);
        }
        assert!(!controller.drawing.is_clean());
        controller.switch_space(None);
        assert_eq!(controller.document().len(), 1);
        assert_eq!(controller.document().space_entities(Some(0)).len(), 1);
        controller.handle_input("U");
        assert_eq!(controller.document().len(), 0);
        assert_eq!(controller.document().space_entities(Some(0)).len(), 1);
        controller.drawing.set_clean();
        assert!(controller.drawing.is_clean());
    }

    #[test]
    fn resting_on_a_snap_point_acquires_it_for_tracking() {
        let mut controller = Controller::new();
        controller
            .drawing
            .document
            .add(crate::model::entities::line(crate::model::geometry::pt(0.0, 0.0), crate::model::geometry::pt(10.0, 0.0)));
        controller.scale = 10.0;
        controller.handle_input("LINE");
        controller.handle_input("50,50");
        controller.move_cursor(Some(crate::model::geometry::pt(10.05, 0.05)));
        assert!(controller.tick(), "waiting to acquire");
        std::thread::sleep(ACQUIRE_AFTER + std::time::Duration::from_millis(50));
        controller.tick();
        assert_eq!(controller.acquired, vec![crate::model::geometry::pt(10.0, 0.0)]);
        // Straight above the endpoint, the cursor is pulled onto its vertical.
        controller.move_cursor(Some(crate::model::geometry::pt(10.3, 30.0)));
        let cursor = controller.cursor.clone().unwrap();
        assert_eq!(cursor.point, crate::model::geometry::pt(10.0, 30.0));
        assert_eq!(cursor.tracked, vec![crate::model::geometry::pt(10.0, 0.0)]);
    }

    #[test]
    fn deleting_or_renaming_a_layout_undoes_with_what_was_on_it() {
        let mut controller = Controller::new();
        controller.set_document(Document::new());
        let sheet = controller.new_layout("Sheet").unwrap();
        run(&mut controller, &["LINE", "0,0", "10,10", ""]);
        // A second edit in the layout, to see its own undo history survive the round trip.
        run(&mut controller, &["CIRCLE", "5,5", "2"]);
        controller.switch_space(None);
        let count = controller.document().layouts.len();
        controller.delete_layout(sheet).unwrap();
        assert_eq!(controller.document().layouts.len(), count - 1);
        controller.undo();
        assert_eq!(controller.document().layouts.len(), count, "U brings the layout back");
        let back = controller.document().layouts.iter().position(|l| l.name == "Sheet").expect("named as before");
        assert_eq!(back, sheet, "in its old place");
        controller.switch_space(Some(back));
        assert_eq!(controller.document().len(), 2, "with what was on it");
        controller.undo();
        assert_eq!(controller.document().len(), 1, "and its own undo history");
        controller.switch_space(None);
        controller.redo();
        assert!(!controller.document().layouts.iter().any(|l| l.name == "Sheet"), "REDO deletes it again");
        controller.undo();
        controller.rename_layout(sheet, "Plan").unwrap();
        assert_eq!(controller.document().layouts[sheet].name, "Plan");
        controller.undo();
        assert_eq!(controller.document().layouts[sheet].name, "Sheet", "a rename undoes too");
    }
}
