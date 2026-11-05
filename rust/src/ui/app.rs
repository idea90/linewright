//! The main window: what it holds, what each frame does, and the actions its buttons and keys trigger.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};

use eframe::egui::{self, Key, KeyboardShortcut, Modifiers, Pos2, Rect};

use super::controller::Controller;
use super::undo::Change;
use super::{frame, layers, panels, theme};
use crate::canvas::plot::{FIT, PAPER_SIZES, export_layout_pdf, export_pdf, format_scale, parse_scale};
use crate::canvas::view::{Camera, ZOOM_STEP, viewport_camera, viewport_view};
use crate::commands::format_general;
use crate::commands::prompts::{PromptKind, ViewRequest};
use crate::fonts::{self, FontFile};
use crate::io::dxf::unit_names;
use crate::io::files::{DXF_SUFFIX, FileError, open_drawing, save_drawing};
use crate::model::dimension::DimensionStyle;
use crate::model::document::{Document, PlotSettings, TextStyleDef};
use crate::model::entities::{BlockDef, BlockRef, Entity, EntityId, HAlign, Shape, TextAlign, VAlign};
use crate::model::geometry::Point;
use crate::model::index::Bounds;
use crate::model::lineweight;
use crate::model::mline::MlineStyle;
use crate::model::mtext::{RichText, Run, RunStyle, has_formatting, plain, serialize, valid_rich};
use crate::model::multileader::MleaderStyle;
use crate::model::overkill::OverkillSettings;
use crate::model::properties::set_property;
use crate::snap::resolve::SnapMode;

pub const WINDOW_NAME: &str = "Linewright";
pub const UNTITLED: &str = "Drawing1";
pub const COMMAND_INPUT: &str = "command_input";
pub const RENAME_INPUT: &str = "layer_rename";
pub const PLOT_SCALE_INPUT: &str = "plot_scale";
const MAX_PREVIOUS_VIEWS: usize = 20;
/// ZOOM Window frames the window exactly, undoing the margin zoom extents leaves.
const EXTENTS_MARGIN: f64 = 0.9;

/// Something a button, menu or key asks for. Panels collect them while they are drawn; the window runs them after.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    New,
    Open,
    Save,
    SaveAs,
    ExportPdf,
    Undo,
    Redo,
    ZoomExtents,
    ZoomIn,
    ZoomOut,
    Command(&'static str),
    Input(String),
    ToggleSnap(SnapMode),
    TogglePalette,
    Cancel,
    EraseSelection,
    /// The toolbar's layer, color, linetype and lineweight: for the selection, or for what is drawn next.
    UseLayer(String),
    UseColor(i32),
    UseLinetype(String),
    UseLineweight(i16),
    /// Show lineweights on screen or not (LWDISPLAY).
    ToggleLineweights,
    NewLayer,
    DeleteLayer(String),
    CurrentLayer(String),
    RenameLayer(String, String),
    LayerVisible(String, bool),
    LayerLocked(String, bool),
    LayerFrozen(String, bool),
    LayerPlot(String, bool),
    LayerColor(String, i32),
    LayerLinetype(String, String),
    LayerLineweight(String, i16),
    /// Freeze a layer in the viewport worked through, or thaw it there.
    LayerViewportFrozen(String, bool),
    /// Enter, from the right mouse button or the space bar.
    Enter,
    Copy,
    Cut,
    Paste,
    SelectAll,
    ToggleProperties,
    /// Open the Select Color dialog for a color to go somewhere.
    ChooseColor(ColorTarget),
    /// Open a drawing from the start page's recent list.
    OpenRecent(PathBuf),
    /// Open model space (None) or a layout.
    OpenSpace(Option<usize>),
    NewLayout,
    DeleteLayout(usize),
    RenameLayout(usize),
    /// A typed value for one property of the selected objects, from the Properties palette.
    SetProperty(Vec<EntityId>, String, String),
    /// Work on model space through a viewport of the layout on screen.
    EnterViewport(EntityId),
    DeselectAll,
    ShowProperties,
    /// Snap the next point to one kind of point only.
    SnapOverride(crate::snap::osnap::SnapKind),
    /// Start a command and answer its first prompt, as DRAWORDER's Front.
    CommandWith(&'static str, &'static str),
    /// Type an answer to the running command's prompt, as choosing one of its options does.
    Answer(String),
}

/// Which right-click menu is open, as AutoCAD's shortcut menus: with nothing selected, with a selection, while a
/// command asks something with options, and the object snaps (Shift+right-click).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MenuKind {
    Default,
    Edit,
    Command,
    Snap,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ContextMenu {
    pub at: egui::Pos2,
    pub kind: MenuKind,
    /// Just opened this frame, so the click that opened it does not close it.
    pub fresh: bool,
}

/// What to do once unsaved changes are saved or discarded.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Then {
    New,
    Open,
    Close,
}

pub struct PlotForm {
    pub paper: usize,
    pub landscape: bool,
    pub scale: String,
    pub error: String,
    /// Plotting a layout: its sheet at 1:1, so paper and scale are not asked.
    pub layout: bool,
    /// The page setup's plot style table (empty for none) and whether objects' lineweights print.
    pub style_table: String,
    pub lineweights: bool,
    /// The tables to choose from.
    pub tables: Vec<String>,
}

/// The dimension style manager's working copy of the styles, applied on OK.
pub struct StyleForm {
    pub styles: Vec<DimensionStyle>,
    pub selected: usize,
    pub current: String,
    /// Styles some dimension follows, which cannot be deleted.
    pub used: Vec<String>,
    pub new_name: String,
    pub error: String,
}

/// What a color chosen in the Select Color dialog goes to.
#[derive(Clone, Debug, PartialEq)]
pub enum ColorTarget {
    /// The color new objects get (COLOR).
    Current,
    /// The selected objects, or new objects when nothing is selected (the ribbon).
    Selection,
    Layer(String),
}

/// The Select Color dialog: the color being picked, on the index or the true color tab.
pub struct ColorForm {
    pub target: ColorTarget,
    pub color: i32,
    pub true_color_tab: bool,
    pub typed: String,
}

/// QSELECT: objects of a kind whose layer, color or linetype is, or is not, a value.
pub struct QuickSelectForm {
    /// The object kind, empty for any.
    pub kind: String,
    pub property: usize,
    pub equals: bool,
    pub value: String,
    pub append: bool,
}

pub const QUICK_SELECT_PROPERTIES: [&str; 4] = ["Layer", "Color", "Linetype", "Lineweight"];

/// An object's layer, color or linetype, as QSELECT compares them.
pub fn quick_select_value(entity: &Entity, property: usize) -> String {
    match property {
        0 => entity.layer.clone(),
        1 => crate::canvas::aci::color_name(entity.color),
        2 => entity.linetype.clone(),
        _ => lineweight::name(entity.lineweight),
    }
}

pub struct FindForm {
    pub find: String,
    pub replace: String,
    pub match_case: bool,
    pub message: String,
}

/// PURGE: what nothing uses, each with whether to remove it.
pub struct PurgeForm {
    pub layers: Vec<(String, bool)>,
    pub styles: Vec<(String, bool)>,
    pub blocks: Vec<(String, bool)>,
}

/// $INSUNITS codes UNITS offers, with their names.
pub const UNIT_CHOICES: [(i64, &str); 6] =
    [(0, "Unitless"), (1, "Inches"), (2, "Feet"), (4, "Millimeters"), (5, "Centimeters"), (6, "Meters")];

pub struct UnitsForm {
    pub units: i64,
    pub precision: usize,
}

/// The Multileader Style Manager: the styles as edited, which is picked, and which new multileaders follow.
pub struct MleaderStyleForm {
    pub styles: Vec<MleaderStyle>,
    pub selected: usize,
    pub current: String,
    pub new_name: String,
    pub error: String,
}

/// The Multiline Style manager: the styles as edited, which one and which of its lines is picked, which new
/// multilines follow, and the styles multilines in the drawing use, which cannot be deleted.
pub struct MlineStyleForm {
    pub styles: Vec<MlineStyle>,
    pub selected: usize,
    pub element: usize,
    pub current: String,
    pub in_use: Vec<String>,
    pub new_name: String,
    pub error: String,
}

/// The Text Style manager: the styles as edited, and the fonts to choose from.
pub struct TextStyleForm {
    pub styles: Vec<TextStyleDef>,
    pub selected: usize,
    pub current: String,
    pub new_name: String,
    pub error: String,
    pub fonts: Vec<String>,
}

/// The multiline text editor: the text with each character's look, and what the toolbar applies next.
pub struct MtextForm {
    /// The object edited, or where new text goes.
    pub target: Option<EntityId>,
    pub position: crate::model::geometry::Point,
    pub height: f64,
    pub text: String,
    pub styles: Vec<RunStyle>,
    pub current: RunStyle,
    /// The characters selected, as last seen.
    pub selection: (usize, usize),
    pub fonts: Vec<String>,
}

impl MtextForm {
    /// The text's runs, one line of them for each line, stacked characters each a run of their own.
    pub fn lines(&self) -> RichText {
        let mut lines: RichText = vec![vec![]];
        for (c, style) in self.text.chars().zip(&self.styles) {
            if c == '\n' {
                lines.push(vec![]);
                continue;
            }
            let Some(line) = lines.last_mut() else { continue };
            match line.last_mut() {
                Some(run) if run.style == *style => run.text.push(c),
                _ => line.push(Run { text: c.to_string(), style: style.clone() }),
            }
        }
        lines
    }

    /// A style for every character after the text changed: what is kept keeps its look, what is new takes the
    /// toolbar's.
    pub fn follow_edit(&mut self, before: &str) {
        let (old, new): (Vec<char>, Vec<char>) = (before.chars().collect(), self.text.chars().collect());
        let start = old.iter().zip(&new).take_while(|(a, b)| a == b).count();
        let max_end = old.len().min(new.len()) - start;
        let end = old.iter().rev().zip(new.iter().rev()).take(max_end).take_while(|(a, b)| a == b).count();
        let inserted = new.len() - start - end;
        self.styles.splice(start..old.len() - end, std::iter::repeat_n(self.current.clone(), inserted));
    }

    /// Change the look of the selection, or of what is typed next when nothing is selected.
    pub fn apply(&mut self, change: impl Fn(&mut RunStyle)) {
        let (from, to) = self.selection;
        if from == to {
            change(&mut self.current);
        } else {
            for style in self.styles.iter_mut().take(to).skip(from) {
                change(style);
            }
        }
    }
}

/// The Lineweight Settings dialog: the lineweight new objects get, what Default means, and whether they show.
pub struct LineweightForm {
    pub current: i16,
    pub default: i16,
    pub show: bool,
}

/// The Delete Duplicate Objects dialog: the options being set, the tolerance as typed, and what is wrong with it.
pub struct OverkillForm {
    pub settings: OverkillSettings,
    pub tolerance: String,
    pub error: String,
}

pub enum Dialog {
    Message(String),
    Discard(Then),
    Plot(PlotForm),
    DimensionStyles(StyleForm),
    RenameLayout {
        index: usize,
        name: String,
        error: String,
    },
    Color(ColorForm),
    /// Drafting Settings: object snap modes, tracking and the polar angle, changed as they are ticked.
    SnapSettings,
    /// BEDIT with nothing picked: which block to edit, as typed.
    EditBlock(String),
    /// BCLOSE: save the block's changes or not.
    CloseBlock,
    /// External References: each attached drawing, to reload or detach.
    Xrefs,
    QuickSelect(QuickSelectForm),
    Find(FindForm),
    Purge(PurgeForm),
    Units(UnitsForm),
    Lineweight(LineweightForm),
    /// Delete Duplicate Objects, which OVERKILL opens once the objects are picked.
    Overkill(OverkillForm),
    MleaderStyles(MleaderStyleForm),
    MlineStyles(MlineStyleForm),
    TextStyles(TextStyleForm),
    Mtext(MtextForm),
}

struct Opening {
    name: String,
    receiver: Receiver<Result<Document, FileError>>,
}

pub struct LinewrightApp {
    pub controller: Controller,
    pub camera: Camera,
    pub input: String,
    pub palette_open: bool,
    pub properties_open: bool,
    pub context_menu: Option<ContextMenu>,
    /// What is typed into the Properties palette's fields, by field name, before Enter applies it.
    pub property_edits: std::collections::HashMap<String, String>,
    /// The palette's fields belong to these objects; a different selection starts them afresh.
    pub property_target: Vec<EntityId>,
    previous_views: Vec<Camera>,
    /// The viewport last worked through, which MSPACE goes back to.
    last_viewport: Option<EntityId>,
    /// The view of each space that is not open, and which space the window last showed.
    space_cameras: std::collections::HashMap<Option<usize>, Camera>,
    shown_space: Option<usize>,
    shown_layouts: usize,
    /// The start page shows in place of the drawing until a drawing is opened or work begins.
    pub welcome: bool,
    /// AutoComplete: which suggestion is picked, for which typed text.
    pub suggestion: usize,
    pub suggestion_for: String,
    /// Dynamic input: the first field's value once Tab moved on to the second (length, or X).
    pub dynamic_first: Option<String>,
    /// Where the pointer is held for a screenshot, in place of the mouse.
    pub pinned_pointer: Option<Pos2>,
    pub recent: Vec<PathBuf>,
    /// A Properties palette field has the keyboard this frame, so the command line leaves it be.
    pub property_focus: bool,
    pub palette_row: Option<String>,
    /// The layer being renamed and the name typed so far.
    pub renaming: Option<(String, String)>,
    pub dialog: Option<Dialog>,
    pub actions: Vec<Action>,
    opening: Option<Opening>,
    zoom_extents_pending: bool,
    pub canvas_rect: Rect,
    pub panning: Option<Pos2>,
    pub pointer: Option<Pos2>,
    allow_close: bool,
    title: String,
    egui_ctx: egui::Context,
    #[cfg(debug_assertions)]
    capture: Option<super::debug_capture::Capture>,
    /// The drawing's fonts registered with egui, by lower-case name.
    pub drawing_fonts: HashSet<String>,
    /// Fonts handed to egui this frame, usable from the next.
    fonts_pending: Option<HashSet<String>>,
    fonts_installed: bool,
    /// The pointer is on the window's edge, where a press resizes the window instead of reaching the drawing.
    pub on_window_edge: bool,
}

impl LinewrightApp {
    pub fn new(cc: &eframe::CreationContext<'_>, path: Option<PathBuf>) -> LinewrightApp {
        egui_extras::install_image_loaders(&cc.egui_ctx);
        theme::apply(&cc.egui_ctx);
        let mut app = LinewrightApp::without_window(cc.egui_ctx.clone(), path.is_none());
        app.set_document(Document::new());
        app.announce_new();
        if let Some(path) = path {
            app.open_in_background(path);
        }
        app
    }

    /// The window's state with an empty drawing, drawing into `egui_ctx`. `new` builds it from eframe's context;
    /// tests build it from a bare one, without a window.
    fn without_window(egui_ctx: egui::Context, welcome: bool) -> LinewrightApp {
        LinewrightApp {
            controller: Controller::new(),
            camera: Camera::default(),
            input: String::new(),
            palette_open: false,
            properties_open: false,
            context_menu: None,
            property_edits: Default::default(),
            property_target: vec![],
            previous_views: vec![],
            last_viewport: None,
            space_cameras: Default::default(),
            shown_space: None,
            shown_layouts: 0,
            welcome,
            suggestion: 0,
            suggestion_for: String::new(),
            dynamic_first: None,
            pinned_pointer: None,
            recent: super::recent::load(),
            property_focus: false,
            palette_row: None,
            renaming: None,
            dialog: None,
            actions: vec![],
            opening: None,
            zoom_extents_pending: true,
            canvas_rect: Rect::NOTHING,
            panning: None,
            pointer: None,
            allow_close: false,
            title: String::new(),
            egui_ctx,
            #[cfg(debug_assertions)]
            capture: super::debug_capture::Capture::from_env(),
            drawing_fonts: HashSet::new(),
            fonts_pending: None,
            fonts_installed: true, // theme::apply installs the system fonts
            on_window_edge: false,
        }
    }

    /// An app with an empty drawing and no window, for tests that draw its dialogs into a bare egui context.
    #[cfg(test)]
    pub(crate) fn for_tests() -> LinewrightApp {
        let mut app = LinewrightApp::without_window(egui::Context::default(), false);
        app.set_document(Document::new());
        app.announce_new();
        app
    }

    pub fn document(&self) -> &Document {
        self.controller.document()
    }

    pub fn is_opening(&self) -> bool {
        self.opening.is_some()
    }

    pub fn opening_name(&self) -> Option<&str> {
        self.opening.as_ref().map(|o| o.name.as_str())
    }

    pub fn print(&mut self, message: impl Into<String>) {
        self.controller.drawing.print(message);
    }

    pub fn file_name(&self) -> String {
        self.document().path.as_deref().and_then(Path::file_name).map_or(UNTITLED.to_string(), |n| n.to_string_lossy().into_owned())
    }

    pub fn modified(&self) -> bool {
        !self.controller.drawing.is_clean()
    }

    fn set_document(&mut self, document: Document) {
        self.space_cameras.clear();
        self.shown_space = None;
        self.shown_layouts = document.layouts.len();
        let skipped = document.skipped.clone();
        self.controller.set_document(document);
        self.register_drawing_fonts();
        self.palette_row = None;
        self.renaming = None;
        self.zoom_extents_pending = true;
        if !skipped.is_empty() {
            let kinds: Vec<String> = skipped.iter().map(|(kind, count)| format!("{count} {kind}")).collect();
            self.print(format!("Not shown yet (kept when saving): {}", kinds.join(", ")));
        }
    }

    /// Make the fonts the drawing's text asks for available to the canvas, those that are installed.
    fn register_drawing_fonts(&mut self) {
        let wanted = drawing_font_names(self.document());
        let registered = self.fonts_pending.as_ref().unwrap_or(&self.drawing_fonts);
        if wanted.iter().all(|name| registered.contains(name)) && self.fonts_installed {
            return;
        }
        let catalog = fonts::catalog();
        let found: Vec<(String, FontFile)> = wanted.iter().filter_map(|name| Some((name.clone(), catalog.resolve(name)?))).collect();
        // egui takes new fonts at the start of the next frame; until then the text keeps the interface font.
        self.drawing_fonts.clear();
        self.fonts_pending = Some(found.iter().map(|(name, _)| name.clone()).collect());
        theme::install_fonts(&self.egui_ctx, &found);
        self.fonts_installed = true;
    }

    fn announce_new(&mut self) {
        let units = unit_names(self.document()).0;
        self.print(format!("{UNTITLED} created. Units: {units}."));
    }

    /// The viewport model space is open through, when zooming and panning change its view: not when its display
    /// is locked, when they move the sheet instead, as in AutoCAD.
    fn zoomable_viewport(&self) -> Option<Entity> {
        self.document().through_viewport().filter(|v| !matches!(v.shape, Shape::Viewport { locked: true, .. })).cloned()
    }

    /// How the space being worked in is shown: through the viewport worked through, or the window's own view.
    pub fn view_camera(&self) -> Camera {
        match self.zoomable_viewport().map(|v| v.shape) {
            Some(Shape::Viewport { center, height, view_center, view_height, .. }) => {
                viewport_camera(&self.camera, center, height, view_center, view_height)
            }
            _ => self.camera,
        }
    }

    /// Show the space being worked in as a camera does: through a viewport, its view changes to match.
    pub fn set_view_camera(&mut self, camera: Camera) {
        let Some(viewport) = self.zoomable_viewport() else {
            self.camera = camera;
            return;
        };
        let Shape::Viewport { center, height, .. } = viewport.shape else { return };
        let (new_center, new_height) = viewport_view(&self.camera, center, height, &camera);
        self.set_viewport_view(viewport, new_center, new_height);
    }

    fn set_viewport_view(&mut self, mut viewport: Entity, new_center: Point, new_height: f64) {
        if !(new_height.is_finite() && new_height > 0.0 && new_center.is_finite()) {
            return;
        }
        if let Shape::Viewport { view_center, view_height, .. } = &mut viewport.shape {
            (*view_center, *view_height) = (new_center, new_height);
        }
        self.controller.drawing.document.modify(&viewport);
        self.controller.drawing.note_change();
    }

    /// Fit a part of the space being worked in to the view, taking up `margin` of it.
    fn zoom_to(&mut self, bounds: Bounds, margin: f64) {
        match self.zoomable_viewport() {
            Some(viewport) => {
                let Shape::Viewport { width, height, .. } = viewport.shape else { return };
                let shown = bounds.height().max(bounds.width() * height / width.max(1e-12)).max(1e-9) / margin;
                self.set_viewport_view(viewport, bounds.center(), shown);
            }
            None => {
                let size = self.canvas_rect.size() * (margin / EXTENTS_MARGIN) as f32;
                self.camera.zoom_extents(bounds, size.x as f64, size.y as f64);
            }
        }
    }

    pub fn zoom_extents(&mut self) {
        if self.zoomable_viewport().is_some() {
            if let Some(extent) = self.document().model().extent() {
                self.zoom_to(extent, EXTENTS_MARGIN);
            }
            return;
        }
        let size = self.canvas_rect.size();
        let document = self.document();
        let sheet = document.shown_layout().map(|i| document.layouts[i].sheet());
        let extent = match (sheet, document.extent()) {
            (Some(sheet), Some(extent)) => Some(sheet.union(extent)),
            (sheet, extent) => sheet.or(extent),
        };
        if let Some(extent) = extent {
            self.camera.zoom_extents(extent, size.x as f64, size.y as f64);
        }
    }

    // ------------------------------------------------------------------------------------------- files

    /// Open a drawing without freezing the window. Everything is held still until it arrives or fails.
    pub fn open_in_background(&mut self, path: PathBuf) {
        if self.opening.is_some() {
            return;
        }
        let name = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let opened = open_drawing(&path);
            // Finding the drawing's fonts among the system's reads every font's names: do it here, not in the window.
            if let Ok(document) = &opened {
                if let Some(folder) = path.parent() {
                    crate::shx::add_drawing_folder(folder);
                }
                for name in drawing_font_names(document) {
                    if fonts::catalog().resolve(&name).is_none() {
                        crate::shx::text_font(&name);
                    }
                }
            }
            let _ = sender.send(opened);
        });
        self.opening = Some(Opening { name, receiver });
    }

    fn poll_opening(&mut self, ctx: &egui::Context) {
        let Some(opening) = &self.opening else { return };
        match opening.receiver.try_recv() {
            Err(TryRecvError::Empty) => ctx.request_repaint_after(std::time::Duration::from_millis(50)),
            Err(TryRecvError::Disconnected) => {
                let name = opening.name.clone();
                self.opening = None;
                self.dialog = Some(Dialog::Message(format!("Could not open {name}.")));
            }
            Ok(result) => {
                self.opening = None;
                match result {
                    Ok(document) => {
                        if let Some(path) = &document.path {
                            self.recent = super::recent::remember(path);
                        }
                        self.welcome = false;
                        self.set_document(document);
                    }
                    Err(error) => self.dialog = Some(Dialog::Message(error.0)),
                }
            }
        }
    }

    fn save_path(&mut self, path: &Path) -> bool {
        match save_drawing(&mut self.controller.drawing.document, path) {
            Ok(()) => {
                self.controller.drawing.set_clean();
                self.recent = super::recent::remember(path);
                true
            }
            Err(error) => {
                self.dialog = Some(Dialog::Message(error.0));
                false
            }
        }
    }

    fn save(&mut self) -> bool {
        match self.document().path.clone() {
            Some(path) => self.save_path(&path),
            None => self.save_as(),
        }
    }

    fn save_as(&mut self) -> bool {
        let stem =
            self.document().path.as_deref().and_then(Path::file_stem).map_or(UNTITLED.to_string(), |s| s.to_string_lossy().into_owned());
        let dialog = drawing_filters(rfd::FileDialog::new().set_title("Save drawing")).set_file_name(format!("{stem}.{DXF_SUFFIX}"));
        let Some(path) = dialog.save_file() else { return false };
        let path = if path.extension().is_some() { path } else { path.with_extension(DXF_SUFFIX) };
        self.save_path(&path)
    }

    fn choose_and_open(&mut self) {
        if let Some(path) = drawing_filters(rfd::FileDialog::new().set_title("Open drawing")).pick_file() {
            self.open_in_background(path);
        }
    }

    /// Ask about unsaved changes first, unless there are none.
    fn confirm_discard(&mut self, then: Then, ctx: &egui::Context) {
        if self.modified() {
            self.dialog = Some(Dialog::Discard(then));
        } else {
            self.proceed(then, ctx);
        }
    }

    pub fn proceed(&mut self, then: Then, ctx: &egui::Context) {
        match then {
            Then::New => {
                self.set_document(Document::new());
                self.announce_new();
            }
            Then::Open => self.choose_and_open(),
            Then::Close => {
                self.allow_close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    pub fn discard_answered(&mut self, then: Then, save_first: bool, ctx: &egui::Context) {
        self.dialog = None;
        if !save_first || self.save() {
            self.proceed(then, ctx);
        }
    }

    /// Open a block in the block editor, viewing all of it.
    pub fn edit_block(&mut self, block: BlockRef) {
        self.dialog = None;
        let name = block.0.name.clone();
        self.space_cameras.insert(Some(usize::MAX), self.camera);
        self.controller.begin_block_edit(block);
        self.zoom_extents();
        self.print(format!("Block editor: {name}. BCLOSE saves or discards the changes."));
    }

    /// Close the block editor, saving or not, and go back to the view it was opened from.
    pub fn close_block(&mut self, save: bool) {
        self.dialog = None;
        let name = self.document().editing_block().map(|b| b.0.name.clone()).unwrap_or_default();
        let saved = self.controller.end_block_edit(save);
        if let Some(camera) = self.space_cameras.remove(&Some(usize::MAX)) {
            self.camera = camera;
        }
        self.print(if saved.is_some() { format!("Block {name} saved.") } else { format!("Changes to block {name} discarded.") });
    }

    /// XATTACH: choose a drawing, then insert it as an external reference, named after its file.
    fn attach_xref(&mut self) {
        let Some(path) = drawing_filters(rfd::FileDialog::new().set_title("Attach external reference")).pick_file() else { return };
        let entities = match crate::io::files::xref_entities(&path) {
            Ok(entities) => entities,
            Err(error) => {
                self.dialog = Some(Dialog::Message(error.0));
                return;
            }
        };
        let stem = path.file_stem().map_or_else(|| "xref".to_string(), |s| s.to_string_lossy().into_owned());
        let document = self.document();
        let name = std::iter::once(stem.clone())
            .chain((2..).map(|n| format!("{stem}_{n}")))
            .find(|n| !document.blocks.contains_key(&n.to_uppercase()))
            .unwrap_or(stem);
        let block = BlockRef(std::sync::Arc::new(BlockDef {
            name: name.clone(),
            base: crate::model::geometry::pt(0.0, 0.0),
            entities,
            attribute_defs: vec![],
            xref: Some(path.display().to_string()),
        }));
        self.controller.drawing.document.blocks.insert(name.to_uppercase(), block);
        self.controller.defaults.borrow_mut().last_block = name;
        self.controller.drawing.note_change();
        self.controller.start_command("INSERT");
    }

    /// Reload an external reference from its drawing, or detach it: its inserts go, and so does it.
    pub fn xref_action(&mut self, name: &str, detach: bool) {
        let Some(block) = self.document().xrefs().into_iter().find(|b| b.0.name == name) else { return };
        if detach {
            let inserts: Vec<Entity> = self
                .document()
                .entities()
                .iter()
                .filter(|e| matches!(&e.shape, Shape::Insert { block, .. } if block.0.name == name))
                .cloned()
                .collect();
            if !inserts.is_empty() {
                self.controller.push(Change::remove(inserts));
            }
            let document = &mut self.controller.drawing.document;
            document.blocks.remove(&name.to_uppercase());
            document.purged_blocks.push(name.to_string());
            self.controller.drawing.note_change();
            self.print(format!("External reference {name} detached."));
            return;
        }
        let written = block.0.xref.clone().unwrap_or_default();
        let path = crate::io::files::xref_path(self.document().path.as_deref(), &written);
        match crate::io::files::xref_entities(&path) {
            Ok(entities) => {
                self.controller.drawing.document.replace_block(BlockRef(std::sync::Arc::new(BlockDef { entities, ..(*block.0).clone() })));
                self.print(format!("External reference {name} reloaded."));
            }
            Err(error) => self.print(error.0),
        }
    }

    /// SELECTSIMILAR: everything of the same kind on the same layer as what is selected.
    fn select_similar(&mut self) {
        let selected = self.controller.selected();
        if selected.is_empty() {
            self.print("Select objects first, then SELECTSIMILAR.");
            return;
        }
        let like: Vec<(&'static str, String)> = selected.iter().map(|e| (e.shape.kind(), e.layer.clone())).collect();
        let document = self.document();
        let ids: Vec<EntityId> = document
            .entities()
            .iter()
            .filter(|e| document.is_editable(e) && like.iter().any(|(kind, layer)| e.shape.kind() == *kind && e.layer == *layer))
            .map(|e| e.id)
            .collect();
        let count = ids.len();
        self.controller.selection.replace(ids);
        self.print(format!("{count} similar object(s) selected."));
    }

    /// Apply in Quick Select: the matching objects are selected, or added to what is.
    pub fn quick_select(&mut self) {
        let Some(Dialog::QuickSelect(form)) = &self.dialog else { return };
        let document = self.document();
        let ids: Vec<EntityId> = document
            .entities()
            .iter()
            .filter(|e| document.is_editable(e))
            .filter(|e| form.kind.is_empty() || e.shape.kind() == form.kind)
            .filter(|e| (quick_select_value(e, form.property).eq_ignore_ascii_case(form.value.trim())) == form.equals)
            .map(|e| e.id)
            .collect();
        let (count, append) = (ids.len(), form.append);
        if append {
            self.controller.selection.add(ids);
        } else {
            self.controller.selection.replace(ids);
        }
        self.dialog = None;
        self.print(format!("{count} item(s) selected."));
    }

    /// Find: the objects holding the text, selected and zoomed to.
    pub fn find_select(&mut self) {
        let Some(Dialog::Find(form)) = &self.dialog else { return };
        let (needle, case) = (form.find.clone(), form.match_case);
        let document = self.document();
        let found: Vec<Entity> = document
            .entities()
            .iter()
            .filter(|e| document.is_editable(e) && crate::model::find::holds(e, &needle, case))
            .cloned()
            .collect();
        let message = format!("{} object(s) found.", found.len());
        if let Some(bounds) = found.iter().map(crate::model::index::entity_extent).reduce(Bounds::union) {
            self.remember_view();
            self.zoom_to(bounds.grow(bounds.diagonal().max(1.0) * 0.2), EXTENTS_MARGIN);
        }
        self.controller.selection.replace(found.iter().map(|e| e.id));
        if let Some(Dialog::Find(form)) = &mut self.dialog {
            form.message = message;
        }
    }

    /// Replace All: every match changed, as one undo step.
    pub fn find_replace(&mut self) {
        let Some(Dialog::Find(form)) = &self.dialog else { return };
        let (needle, with, case) = (form.find.clone(), form.replace.clone(), form.match_case);
        let document = self.document();
        let changed: Vec<Entity> = document
            .entities()
            .iter()
            .filter(|e| document.is_editable(e))
            .filter_map(|e| crate::model::find::replaced(e, &needle, &with, case))
            .collect();
        let message = format!("{} object(s) changed.", changed.len());
        if !changed.is_empty() {
            let change = Change::modify(self.document(), changed);
            self.controller.push(change);
        }
        if let Some(Dialog::Find(form)) = &mut self.dialog {
            form.message = message;
        }
    }

    /// Purge: the ticked layers, dimension styles and blocks go. Layers and styles undo together; blocks are
    /// taken out of the file when it is saved.
    pub fn purge(&mut self) {
        let Some(Dialog::Purge(form)) = self.dialog.take() else { return };
        let chosen = |list: &[(String, bool)]| list.iter().filter(|(_, on)| *on).map(|(n, _)| n.clone()).collect::<Vec<_>>();
        let (layers, styles, blocks) = (chosen(&form.layers), chosen(&form.styles), chosen(&form.blocks));
        let document = self.document();
        let before =
            super::undo::LayerState { layers: document.layers.clone(), current: document.current_layer.clone(), entity_layers: vec![] };
        let after = super::undo::LayerState {
            layers: document.layers.iter().filter(|l| !layers.contains(&l.name)).cloned().collect(),
            current: document.current_layer.clone(),
            entity_layers: vec![],
        };
        let kept_styles = document.dimension_styles.iter().filter(|s| !styles.contains(&s.name)).cloned().collect();
        let changes = vec![
            Change::Layers { before, after },
            Change::dimension_styles(document, kept_styles, document.current_dimension_style.clone()),
        ];
        self.controller.push(Change::Group(changes));
        let document = &mut self.controller.drawing.document;
        for name in &blocks {
            document.blocks.remove(&name.to_uppercase());
            document.purged_blocks.push(name.clone());
        }
        if !blocks.is_empty() {
            self.controller.drawing.note_change();
        }
        self.print(format!("Purged {} layer(s), {} dimension style(s), {} block(s).", layers.len(), styles.len(), blocks.len()));
    }

    /// OK in Drawing Units.
    pub fn set_units(&mut self) {
        let Some(Dialog::Units(form)) = self.dialog.take() else { return };
        let document = &mut self.controller.drawing.document;
        (document.units, document.precision) = (Some(form.units), form.precision);
        self.controller.drawing.note_change();
        let (name, _) = unit_names(self.document());
        self.print(format!("Drawing units: {name}, {} decimal places.", form.precision));
    }

    /// MSPACE: work on model space through the layout's viewport, the one last worked through if it is still
    /// there, else the first.
    fn model_space_through_viewport(&mut self) {
        let document = self.document();
        let Some(layout) = document.shown_layout() else {
            self.print("MSPACE works on a layout. Model space is already open.");
            return;
        };
        if document.through().is_some() {
            return;
        }
        let viewports: Vec<EntityId> = document
            .space_store(Some(layout))
            .entities()
            .iter()
            .filter(|e| matches!(e.shape, Shape::Viewport { .. }) && document.is_visible(e))
            .map(|e| e.id)
            .collect();
        let chosen = self.last_viewport.filter(|id| viewports.contains(id)).or(viewports.first().copied());
        match chosen {
            Some(viewport) => self.actions.push(Action::EnterViewport(viewport)),
            None => self.print("There are no viewports on this layout. Make one with MVIEW."),
        }
    }

    /// The Multiline Style manager, opened on the style new multilines follow.
    pub fn open_mline_styles(&mut self) {
        let current = self.controller.defaults.borrow().mline_style.name.clone();
        let document = self.document();
        let mut in_use: Vec<String> = vec![];
        let spaces = std::iter::once(None).chain((0..document.layouts.len()).map(Some));
        for space in spaces {
            for entity in document.space_entities(space) {
                if let Shape::Mline { style, .. } = &entity.shape
                    && !in_use.iter().any(|n| n.eq_ignore_ascii_case(&style.name))
                {
                    in_use.push(style.name.clone());
                }
            }
        }
        let styles = document.mline_styles.clone();
        let selected = styles.iter().position(|s| s.name.eq_ignore_ascii_case(&current)).unwrap_or(0);
        self.dialog = Some(Dialog::MlineStyles(MlineStyleForm {
            styles,
            selected,
            element: 0,
            current,
            in_use,
            new_name: String::new(),
            error: String::new(),
        }));
    }

    /// OK in the Multiline Style manager: the drawing keeps the styles as edited, multilines drawn in a style that
    /// changed take its new look in one undo, and the current style is what MLINE draws with next.
    pub fn set_mline_styles(&mut self) {
        let Some(Dialog::MlineStyles(form)) = self.dialog.take() else { return };
        let document = self.document();
        let mut changed = vec![];
        let spaces = std::iter::once(None).chain((0..document.layouts.len()).map(Some));
        for space in spaces {
            for entity in document.space_entities(space) {
                let Shape::Mline { style, .. } = &entity.shape else { continue };
                let Some(after) = form.styles.iter().find(|s| s.name.eq_ignore_ascii_case(&style.name)) else { continue };
                if after == style {
                    continue;
                }
                let mut state = entity.clone();
                if let Shape::Mline { style, .. } = &mut state.shape {
                    *style = after.clone();
                }
                changed.push(state);
            }
        }
        if let Some(current) = form.styles.iter().find(|s| s.name == form.current) {
            self.controller.defaults.borrow_mut().mline_style = current.clone();
        }
        self.controller.drawing.document.mline_styles = form.styles;
        self.controller.drawing.note_change();
        if !changed.is_empty() {
            let change = Change::modify(self.document(), changed);
            self.controller.push(change);
        }
    }

    /// OK in the Multileader Style Manager: multileaders that follow a style take its new sizes, in one undo.
    pub fn set_mleader_styles(&mut self) {
        let Some(Dialog::MleaderStyles(form)) = self.dialog.take() else { return };
        let old = self.document().mleader_styles.clone();
        let document = self.document();
        let mut changed = vec![];
        let spaces = std::iter::once(None).chain((0..document.layouts.len()).map(Some));
        for space in spaces {
            for entity in document.space_entities(space) {
                let Shape::Multileader { style, landing, dogleg, .. } = &entity.shape else {
                    continue;
                };
                let name = if style.is_empty() { document.current_mleader_style.as_str() } else { style.as_str() };
                let (Some(before), Some(after)) = (
                    old.iter().find(|s| s.name.eq_ignore_ascii_case(name)),
                    form.styles.iter().find(|s| s.name.eq_ignore_ascii_case(name)),
                ) else {
                    continue;
                };
                if before == after {
                    continue;
                }
                let new_dogleg = if *dogleg < 0.0 { -after.dogleg } else { after.dogleg };
                let mut state = entity.clone();
                if let Shape::Multileader { dogleg, text_position, text_height, arrow_size, .. } = &mut state.shape {
                    (*dogleg, *text_height, *arrow_size) = (new_dogleg, after.text_height, after.arrow_size);
                    *text_position = crate::model::multileader::text_corner(*landing, new_dogleg, after.text_height, after.landing_gap);
                }
                changed.push(state);
            }
        }
        let document = &mut self.controller.drawing.document;
        document.mleader_styles = form.styles;
        document.current_mleader_style = form.current;
        self.controller.drawing.note_change();
        if !changed.is_empty() {
            let change = Change::modify(self.document(), changed);
            self.controller.push(change);
        }
    }

    /// Open a text object in the multiline text editor.
    pub fn open_text_editor(&mut self, id: EntityId) {
        let Some(entity) = self.document().get(id).cloned() else { return };
        let Shape::Text { position, text, height, formatted, .. } = &entity.shape else { return };
        let lines = valid_rich(text, formatted, *height).unwrap_or_else(|| {
            text.split('\n')
                .map(|l| if l.is_empty() { vec![] } else { vec![Run { text: l.to_string(), style: RunStyle::default() }] })
                .collect()
        });
        let mut styles = vec![];
        for (index, line) in lines.iter().enumerate() {
            if index > 0 {
                styles.push(RunStyle::default());
            }
            for run in line {
                styles.extend(run.text.chars().map(|_| run.style.clone()));
            }
        }
        self.dialog = Some(Dialog::Mtext(MtextForm {
            target: Some(id),
            position: *position,
            height: *height,
            text: text.clone(),
            styles,
            current: RunStyle::default(),
            selection: (0, 0),
            fonts: editor_fonts(),
        }));
    }

    /// OK in the multiline text editor: the text with its formatting, a new object or the one being edited.
    pub fn mtext_accepted(&mut self) {
        let Some(Dialog::Mtext(form)) = self.dialog.take() else { return };
        let lines = form.lines();
        let text = plain(&lines);
        let formatted = if has_formatting(&lines) { serialize(&lines) } else { String::new() };
        match form.target {
            Some(id) => {
                let Some(entity) = self.document().get(id).cloned() else { return };
                let mut shape = entity.shape.clone();
                if let Shape::Text { text: t, formatted: f, height, .. } = &mut shape {
                    (*t, *f, *height) = (text, formatted, form.height);
                }
                let change = Change::modify(self.document(), vec![entity.with_shape(shape)]);
                self.controller.push(change);
            }
            None if !text.trim().is_empty() => {
                let style = self
                    .document()
                    .text_styles
                    .iter()
                    .find(|s| s.name == self.document().current_text_style)
                    .cloned()
                    .unwrap_or_else(TextStyleDef::standard);
                let shape = Shape::Text {
                    position: form.position,
                    text,
                    height: form.height,
                    rotation: 0.0,
                    // A style with no font of its own leaves the text in the interface font, as it reopens.
                    style: if style.font.is_empty() { String::new() } else { style.name },
                    font: style.font,
                    align: TextAlign { h: HAlign::Left, v: VAlign::Top },
                    line_spacing: 1.0,
                    formatted,
                    width_factor: style.width_factor,
                    oblique: style.oblique,
                };
                let document = self.document();
                let mut entity = Entity::new(shape);
                entity.layer = document.current_layer.clone();
                entity.color = document.current_color;
                entity.linetype = document.current_linetype.clone();
                entity.lineweight = document.current_lineweight;
                self.controller.push(Change::Add(vec![entity]));
            }
            None => {}
        }
    }

    /// OK in the Text Style manager: text that follows a style takes its new font, width and lean.
    pub fn set_text_styles(&mut self) {
        let Some(Dialog::TextStyles(form)) = self.dialog.take() else { return };
        let document = self.document();
        let mut changed = vec![];
        for space in std::iter::once(None).chain((0..document.layouts.len()).map(Some)) {
            for entity in document.space_entities(space) {
                let Shape::Text { style, .. } = &entity.shape else { continue };
                let name = if style.is_empty() { "Standard" } else { style.as_str() };
                let (Some(before), Some(after)) = (
                    document.text_styles.iter().find(|s| s.name.eq_ignore_ascii_case(name)),
                    form.styles.iter().find(|s| s.name.eq_ignore_ascii_case(name)),
                ) else {
                    continue;
                };
                if before == after {
                    continue;
                }
                let mut state = entity.clone();
                if let Shape::Text { font, width_factor, oblique, .. } = &mut state.shape {
                    (*font, *width_factor, *oblique) = (after.font.clone(), after.width_factor, after.oblique);
                }
                changed.push(state);
            }
        }
        let document = &mut self.controller.drawing.document;
        document.text_styles = form.styles;
        document.current_text_style = form.current;
        self.controller.drawing.note_change();
        if !changed.is_empty() {
            let change = Change::modify(self.document(), changed);
            self.controller.push(change);
        }
        self.fonts_installed = false;
    }

    /// OK in Lineweight Settings.
    pub fn set_lineweights(&mut self) {
        let Some(Dialog::Lineweight(form)) = self.dialog.take() else { return };
        let document = &mut self.controller.drawing.document;
        document.current_lineweight = form.current;
        document.default_lineweight = form.default;
        document.show_lineweights = form.show;
        self.controller.drawing.note_change();
        self.print(format!(
            "Current lineweight: {}. Default: {}. Lineweights {}.",
            lineweight::name(form.current),
            lineweight::name(form.default),
            if form.show { "shown" } else { "hidden" }
        ));
    }

    /// Open the dialog a running command's prompt asks for, when no other dialog is open.
    fn open_prompt_dialogs(&mut self) {
        if self.dialog.is_none() && self.controller.prompt().is_some_and(|p| p.kind == PromptKind::OverkillDialog) {
            self.open_overkill();
        }
    }

    /// Open Delete Duplicate Objects with the options last used.
    pub fn open_overkill(&mut self) {
        let settings = self.controller.defaults.borrow().overkill.clone();
        let tolerance = format_general(settings.tolerance);
        self.dialog = Some(Dialog::Overkill(OverkillForm { settings, tolerance, error: String::new() }));
    }

    /// OK in Delete Duplicate Objects: the options are remembered, and the clean-up runs with them.
    pub fn set_overkill(&mut self, settings: OverkillSettings) {
        self.dialog = None;
        self.controller.defaults.borrow_mut().overkill = settings;
        self.controller.accept_default();
    }

    /// Cancel in Delete Duplicate Objects: the command ends and the drawing is left as it was.
    pub fn cancel_overkill(&mut self) {
        self.dialog = None;
        self.controller.cancel();
    }

    /// OK in the Select Color dialog.
    pub fn color_chosen(&mut self) {
        let Some(Dialog::Color(form)) = self.dialog.take() else { return };
        let action = match form.target {
            ColorTarget::Current => {
                self.controller.drawing.document.current_color = form.color;
                self.print(format!("Current color: {}", crate::canvas::aci::color_name(form.color)));
                return;
            }
            ColorTarget::Selection => Action::UseColor(form.color),
            ColorTarget::Layer(name) => Action::LayerColor(name, form.color),
        };
        self.actions.push(action);
    }

    /// PLOT on a layout: its sheet at 1:1, as AutoCAD plots a layout.
    fn plot_layout(&mut self) {
        let document = self.document();
        let layout = document.shown_layout().map(|i| document.layouts[i].name.clone()).unwrap_or_default();
        let stem = document.path.as_deref().and_then(Path::file_stem).map_or(String::new(), |s| s.to_string_lossy().into_owned());
        let default = format!("{stem}-{layout}.pdf");
        let Some(path) =
            rfd::FileDialog::new().set_title("Plot layout to PDF").add_filter("PDF", &["pdf"]).set_file_name(default).save_file()
        else {
            return;
        };
        let path = if path.extension().is_some() { path } else { path.with_extension("pdf") };
        match export_layout_pdf(self.document(), &path) {
            Ok(()) => {
                let name = path.file_name().map_or(String::new(), |n| n.to_string_lossy().into_owned());
                self.print(format!("Plotted {layout} to {name} at 1:1"));
            }
            Err(error) => self.dialog = Some(Dialog::Message(error)),
        }
    }

    /// Keep the plot dialog's plot style choices in the page setup of the space being plotted, saved with the
    /// drawing as AutoCAD saves them.
    fn keep_plot_settings(&mut self, settings: PlotSettings) {
        let document = &mut self.controller.drawing.document;
        let target = match document.shown_layout() {
            Some(index) => &mut document.layouts[index].plot,
            None => &mut document.model_plot,
        };
        if *target != settings {
            *target = settings;
            self.controller.drawing.note_change();
        }
    }

    pub fn plot_accepted(&mut self) {
        let Some(Dialog::Plot(form)) = &mut self.dialog else { return };
        let settings = PlotSettings { style_table: form.style_table.clone(), lineweights: form.lineweights };
        if form.layout {
            self.dialog = None;
            self.keep_plot_settings(settings);
            self.plot_layout();
            return;
        }
        let scale = match parse_scale(&form.scale) {
            Ok(scale) => scale,
            Err(help) => {
                form.error = help.to_string();
                return;
            }
        };
        let (_, width, height) = PAPER_SIZES[form.paper];
        let landscape = form.landscape;
        self.dialog = None;
        self.keep_plot_settings(settings);
        let default =
            self.document().path.as_deref().and_then(Path::file_stem).map_or(String::new(), |s| format!("{}.pdf", s.to_string_lossy()));
        let Some(path) = rfd::FileDialog::new().set_title("Export PDF").add_filter("PDF", &["pdf"]).set_file_name(default).save_file()
        else {
            return;
        };
        let path = if path.extension().is_some() { path } else { path.with_extension("pdf") };
        match export_pdf(self.document(), &path, (width, height), landscape, scale) {
            Ok(layout) => {
                let name = path.file_name().map_or(String::new(), |n| n.to_string_lossy().into_owned());
                self.print(format!("Exported {name} at {}", format_scale(layout.scale)));
                if !layout.fits {
                    self.print("Part of the drawing falls outside the page at this scale.");
                }
            }
            Err(error) => self.dialog = Some(Dialog::Message(error)),
        }
    }

    // ------------------------------------------------------------------------------------------- actions

    fn run(&mut self, action: Action, ctx: &egui::Context) {
        // Anything but choosing a file to open starts work on the drawing, so the start page gives way.
        if !matches!(action, Action::Open | Action::OpenRecent(_)) {
            self.welcome = false;
        }
        if self.opening.is_some() && !matches!(action, Action::ZoomExtents | Action::ZoomIn | Action::ZoomOut | Action::TogglePalette) {
            return; // the window holds still while a drawing opens
        }
        match action {
            Action::New => self.confirm_discard(Then::New, ctx),
            Action::Open => self.confirm_discard(Then::Open, ctx),
            Action::Save => {
                self.save();
            }
            Action::SaveAs => {
                self.save_as();
            }
            Action::ExportPdf => {
                let document = self.document();
                let layout = document.shown_layout().and_then(|i| document.layouts.get(i));
                let settings = layout.map_or(&document.model_plot, |l| &l.plot).clone();
                self.dialog = Some(Dialog::Plot(PlotForm {
                    paper: 0,
                    landscape: true,
                    scale: FIT.to_string(),
                    error: String::new(),
                    layout: layout.is_some(),
                    style_table: settings.style_table,
                    lineweights: settings.lineweights,
                    tables: crate::io::ctb::available(),
                }));
            }
            Action::Undo => self.controller.undo(),
            Action::Redo => self.controller.redo(),
            Action::ZoomExtents => self.zoom_extents(),
            Action::ZoomIn => self.zoom_by(ZOOM_STEP),
            Action::ZoomOut => self.zoom_by(1.0 / ZOOM_STEP),
            Action::Command("DIMSTYLE") => self.open_dimension_styles(),
            Action::Command("OSNAP") => self.dialog = Some(Dialog::SnapSettings),
            Action::Command("QSELECT") => {
                self.dialog = Some(Dialog::QuickSelect(QuickSelectForm {
                    kind: String::new(),
                    property: 0,
                    equals: true,
                    value: self.document().current_layer.clone(),
                    append: false,
                }))
            }
            Action::Command("FIND") => {
                let form = FindForm { find: String::new(), replace: String::new(), match_case: false, message: String::new() };
                self.dialog = Some(Dialog::Find(form));
            }
            Action::Command("PURGE") => {
                let document = self.document();
                let unchecked = |names: Vec<String>| names.into_iter().map(|n| (n, true)).collect();
                let form = PurgeForm {
                    layers: unchecked(document.unused_layers()),
                    styles: unchecked(document.unused_dimension_styles()),
                    blocks: unchecked(document.unused_blocks()),
                };
                self.dialog = Some(Dialog::Purge(form));
            }
            Action::Command("MSPACE") => self.model_space_through_viewport(),
            Action::Command("PSPACE") => {
                if self.document().through().is_some() {
                    self.controller.leave_viewport();
                    self.previous_views.clear();
                } else if self.document().active_layout().is_none() {
                    self.print("PSPACE works on a layout. Open one from its tab.");
                }
            }
            Action::Command("MLSTYLE") => self.open_mline_styles(),
            Action::Command("MLEADERSTYLE") => {
                let document = self.document();
                self.dialog = Some(Dialog::MleaderStyles(MleaderStyleForm {
                    styles: document.mleader_styles.clone(),
                    selected: document.mleader_styles.iter().position(|s| s.name == document.current_mleader_style).unwrap_or(0),
                    current: document.current_mleader_style.clone(),
                    new_name: String::new(),
                    error: String::new(),
                }));
            }
            Action::Command("MTEXT") => self.controller.start_command("MTEXTBOX"),
            Action::Command("MTEDIT") => {
                let id = self.controller.selected().iter().find(|e| matches!(e.shape, Shape::Text { .. })).map(|e| e.id);
                match id {
                    Some(id) => self.open_text_editor(id),
                    None => self.print("Select the text to edit first."),
                }
            }
            Action::Command("STYLE") => {
                let document = self.document();
                let mut fonts = crate::shx::available();
                fonts.extend(fonts::catalog().family_names());
                self.dialog = Some(Dialog::TextStyles(TextStyleForm {
                    styles: document.text_styles.clone(),
                    selected: document.text_styles.iter().position(|s| s.name == document.current_text_style).unwrap_or(0),
                    current: document.current_text_style.clone(),
                    new_name: String::new(),
                    error: String::new(),
                    fonts,
                }));
            }
            Action::Command("LWEIGHT") => {
                let document = self.document();
                self.dialog = Some(Dialog::Lineweight(LineweightForm {
                    current: document.current_lineweight,
                    default: document.default_lineweight,
                    show: document.show_lineweights,
                }));
            }
            Action::Command("UNITS") => {
                let document = self.document();
                let units = document.units.or_else(|| document.source.as_ref().map(|s| s.insunits())).unwrap_or(4);
                self.dialog = Some(Dialog::Units(UnitsForm { units, precision: document.precision }));
            }
            Action::Command("SELECTSIMILAR") => self.select_similar(),
            Action::Command("XATTACH") => self.attach_xref(),
            Action::Command("BEDIT") => {
                let picked = self.controller.selected().iter().find_map(|e| match &e.shape {
                    Shape::Insert { block, .. } if block.0.xref.is_none() && !block.0.name.starts_with('*') => Some(block.clone()),
                    _ => None,
                });
                match picked {
                    Some(block) => self.edit_block(block),
                    None => self.dialog = Some(Dialog::EditBlock(String::new())),
                }
            }
            Action::Command("BCLOSE") => {
                if self.document().editing_block().is_some() {
                    self.dialog = Some(Dialog::CloseBlock);
                } else {
                    self.print("The block editor is not open.");
                }
            }
            Action::Command("XREF") => self.dialog = Some(Dialog::Xrefs),
            Action::Command("IMAGEATTACH") => {
                let Some(path) =
                    rfd::FileDialog::new().set_title("Attach image").add_filter("Images", &["png", "jpg", "jpeg", "bmp"]).pick_file()
                else {
                    return;
                };
                match image::image_dimensions(&path) {
                    Ok(pixels) => {
                        self.controller.defaults.borrow_mut().image = Some((path.display().to_string(), pixels));
                        self.controller.start_command("IMAGEPLACE");
                    }
                    Err(error) => self.dialog = Some(Dialog::Message(format!("Could not read {}: {error}", path.display()))),
                }
            }
            Action::Command("PICKSTYLE") => {
                let document = &mut self.controller.drawing.document;
                document.group_selection = !document.group_selection;
                let state = if document.group_selection { "on" } else { "off" };
                self.print(format!("Group selection {state}"));
            }
            Action::OpenSpace(space) => self.controller.switch_space(space),
            Action::EnterViewport(viewport) => {
                if self.controller.enter_viewport(viewport) {
                    self.previous_views.clear();
                    self.last_viewport = Some(viewport);
                }
            }
            Action::OpenRecent(path) => self.open_in_background(path),
            Action::ChooseColor(target) => {
                let document = self.document();
                let color = match &target {
                    ColorTarget::Layer(name) => document.layer(name).map_or(7, |l| l.color),
                    _ => self.controller.selected().first().map_or(document.current_color, |e| e.color),
                };
                let true_color_tab = crate::canvas::aci::is_true_color(color);
                self.dialog =
                    Some(Dialog::Color(ColorForm { target, color, true_color_tab, typed: crate::canvas::aci::color_name(color) }));
            }
            Action::NewLayout => {
                let layouts = &self.document().layouts;
                let name = (1..)
                    .map(|n| format!("Layout{n}"))
                    .find(|n| !layouts.iter().any(|l| l.name.eq_ignore_ascii_case(n)))
                    .unwrap_or_default();
                if let Err(message) = self.controller.new_layout(&name) {
                    self.print(message);
                }
            }
            Action::DeleteLayout(index) => {
                if let Err(message) = self.controller.delete_layout(index) {
                    self.print(message);
                }
            }
            Action::RenameLayout(index) => {
                let name = self.document().layouts.get(index).map(|l| l.name.clone()).unwrap_or_default();
                self.dialog = Some(Dialog::RenameLayout { index, name, error: String::new() });
            }
            // Commands that work on the sheet itself go back to paper space first, as in AutoCAD.
            Action::Command(name @ ("MVIEW" | "LAYOUT")) if self.document().through().is_some() => {
                self.controller.leave_viewport();
                self.previous_views.clear();
                self.controller.start_command(name);
            }
            Action::Command(name) => self.controller.start_command(name),
            Action::Input(text) => match file_command(&text).filter(|_| self.controller.running_command().is_none()) {
                Some(action) => {
                    self.controller.drawing.print(format!("Command: {}", text.trim().to_uppercase()));
                    self.run(action, ctx);
                }
                None => self.controller.handle_input(&text),
            },
            Action::ToggleSnap(mode) => {
                let on = !self.controller.snap.get(mode);
                self.controller.snap.set(mode, on);
                self.controller.reapply_cursor();
            }
            Action::TogglePalette => self.palette_open = !self.palette_open,
            Action::Cancel => {
                if self.renaming.take().is_none() {
                    self.controller.cancel();
                }
            }
            Action::EraseSelection => self.controller.erase_selection(),
            Action::UseLayer(name) => {
                if self.controller.selection.is_empty() {
                    let result = layers::set_current(&mut self.controller, &name);
                    self.report(result);
                } else {
                    self.controller.change_selected(|e: &mut Entity| e.layer = name.clone());
                }
            }
            Action::UseColor(aci) => {
                if self.controller.selection.is_empty() {
                    self.controller.drawing.document.current_color = aci;
                } else {
                    self.controller.change_selected(|e: &mut Entity| e.color = aci);
                }
            }
            Action::UseLinetype(name) => {
                if self.controller.selection.is_empty() {
                    self.controller.drawing.document.current_linetype = name;
                } else {
                    self.controller.change_selected(|e: &mut Entity| e.linetype = name.clone());
                }
            }
            Action::UseLineweight(lineweight) => {
                if self.controller.selection.is_empty() {
                    self.controller.drawing.document.current_lineweight = lineweight;
                } else {
                    self.controller.change_selected(|e: &mut Entity| e.lineweight = lineweight);
                }
            }
            Action::ToggleLineweights => {
                let document = &mut self.controller.drawing.document;
                document.show_lineweights = !document.show_lineweights;
                let shown = document.show_lineweights;
                self.controller.drawing.note_change();
                self.print(if shown { "Lineweights shown." } else { "Lineweights hidden." });
            }
            Action::NewLayer => {
                let name = layers::create(&mut self.controller);
                self.palette_row = Some(name.clone());
                self.renaming = Some((name.clone(), name));
            }
            Action::DeleteLayer(name) => {
                let result = layers::delete(&mut self.controller, &name);
                self.report(result);
            }
            Action::CurrentLayer(name) => {
                let result = layers::set_current(&mut self.controller, &name);
                self.report(result);
            }
            Action::RenameLayer(old, new) => {
                let result = layers::rename(&mut self.controller, &old, &new);
                if result.is_ok() && self.palette_row.as_deref() == Some(old.as_str()) {
                    self.palette_row = Some(new.trim().to_string());
                }
                self.report(result);
            }
            Action::LayerVisible(name, on) => {
                let result = layers::set_visible(&mut self.controller, &name, on);
                self.report(result);
            }
            Action::LayerFrozen(name, on) => {
                let result = layers::set_frozen(&mut self.controller, &name, on);
                self.report(result);
            }
            Action::LayerPlot(name, on) => {
                let result = layers::set_plot(&mut self.controller, &name, on);
                self.report(result);
            }
            Action::LayerLocked(name, on) => {
                let result = layers::set_locked(&mut self.controller, &name, on);
                self.report(result);
            }
            Action::LayerColor(name, aci) => {
                let result = layers::set_color(&mut self.controller, &name, aci);
                self.report(result);
            }
            Action::LayerLinetype(name, linetype) => {
                let result = layers::set_linetype(&mut self.controller, &name, &linetype);
                self.report(result);
            }
            Action::LayerViewportFrozen(name, frozen_here) => {
                let Some(mut viewport) = self.document().through_viewport().cloned() else { return };
                if let Shape::Viewport { frozen, .. } = &mut viewport.shape {
                    frozen.retain(|f| !f.eq_ignore_ascii_case(&name));
                    if frozen_here {
                        frozen.push(name);
                    }
                }
                let change = Change::modify(self.document(), vec![viewport]);
                self.controller.push(change);
            }
            Action::LayerLineweight(name, lineweight) => {
                let result = layers::set_lineweight(&mut self.controller, &name, lineweight);
                self.report(result);
            }
            Action::Enter => self.controller.enter(),
            Action::Copy => self.controller.copy_selection(false),
            Action::Cut => self.controller.copy_selection(true),
            Action::Paste => self.controller.start_command("PASTECLIP"),
            Action::SelectAll => self.controller.select_all(),
            Action::ToggleProperties => self.properties_open = !self.properties_open,
            Action::ShowProperties => self.properties_open = true,
            Action::DeselectAll => self.controller.selection.clear(),
            Action::SnapOverride(kind) => self.controller.override_snap(kind),
            Action::CommandWith(name, answer) => {
                self.controller.start_command(name);
                self.controller.handle_input(answer);
            }
            Action::Answer(text) => self.controller.handle_input(&text),
            Action::SetProperty(ids, name, value) => {
                let mut changed = vec![];
                for entity in ids.iter().filter_map(|id| self.controller.document().get(*id)) {
                    match set_property(entity, &name, &value) {
                        Ok(state) if !state.same_state(entity) => changed.push(state),
                        Ok(_) => {}
                        Err(message) => return self.print(message),
                    }
                }
                if !changed.is_empty() {
                    let change = Change::modify(self.controller.document(), changed);
                    self.controller.push(change);
                }
            }
        }
    }

    fn open_dimension_styles(&mut self) {
        self.controller.cancel();
        let document = self.controller.document();
        let mut used: Vec<String> = document
            .entities()
            .iter()
            .filter_map(|e| match &e.shape {
                Shape::Dimension { look, .. } | Shape::Annotation { look, .. } => Some(look.style.to_lowercase()),
                _ => None,
            })
            .collect();
        used.sort();
        used.dedup();
        let current = document.current_dimension_style.clone();
        let selected = document.dimension_styles.iter().position(|s| s.name.eq_ignore_ascii_case(&current)).unwrap_or(0);
        self.dialog = Some(Dialog::DimensionStyles(StyleForm {
            styles: document.dimension_styles.clone(),
            selected,
            current,
            used,
            new_name: String::new(),
            error: String::new(),
        }));
    }

    /// OK in the dimension style manager: the styles as edited, and every dimension following a changed style
    /// brought up to date, as one undo step.
    pub fn dimension_styles_accepted(&mut self) {
        let Some(Dialog::DimensionStyles(form)) = self.dialog.take() else { return };
        let document = self.controller.document();
        let mut changed = vec![];
        for style in &form.styles {
            if let Some(old) = document.dimension_style(&style.name)
                && old.look != style.look
            {
                changed.extend(document.restyled_dimensions(old, style));
            }
        }
        let mut changes = vec![Change::dimension_styles(document, form.styles, form.current)];
        if !changed.is_empty() {
            changes.push(Change::modify(document, changed));
        }
        self.controller.push(Change::Group(changes));
        let look = self.controller.document().current_dimension_look();
        self.controller.defaults.borrow_mut().dimension_look = look;
    }

    /// When another space opens, keep the old one's view and show the new one as it was left, or all of it.
    fn follow_space(&mut self) {
        let (space, count) = (self.document().shown_layout(), self.document().layouts.len());
        let removed = count < self.shown_layouts;
        if removed {
            // A layout went: views of layouts are forgotten rather than shifted.
            self.space_cameras.retain(|space, _| space.is_none());
        }
        self.shown_layouts = count;
        if space == self.shown_space && !removed {
            return;
        }
        if !removed || self.shown_space.is_none() {
            self.space_cameras.insert(self.shown_space, self.camera);
        }
        self.shown_space = space;
        self.previous_views.clear();
        match self.space_cameras.remove(&space) {
            Some(camera) => self.camera = camera,
            None => self.zoom_extents(),
        }
    }

    /// Change the view as a command asked: ZOOM's extents, window or previous view.
    fn apply_view_request(&mut self) {
        let Some(request) = self.controller.drawing.view_request.take() else { return };
        match request {
            ViewRequest::Extents => {
                self.remember_view();
                self.zoom_extents();
            }
            ViewRequest::Window(first, second) => {
                self.remember_view();
                self.zoom_to(Bounds::from_corners(first, second), 1.0);
            }
            ViewRequest::Previous => match self.previous_views.pop() {
                Some(camera) => self.set_view_camera(camera),
                None => self.print("No previous view saved."),
            },
            ViewRequest::Scale(factor) => {
                self.remember_view();
                self.zoom_by(factor);
            }
        }
    }

    fn remember_view(&mut self) {
        self.previous_views.push(self.view_camera());
        if self.previous_views.len() > MAX_PREVIOUS_VIEWS {
            self.previous_views.remove(0);
        }
    }

    fn report(&mut self, result: Result<(), String>) {
        if let Err(message) = result {
            self.print(message);
        }
    }

    /// Zoom about the middle of the view: of the drawing area, or of the viewport worked through.
    pub fn zoom_by(&mut self, factor: f64) {
        let middle = match self.zoomable_viewport().map(|v| v.shape) {
            Some(Shape::Viewport { center, .. }) => self.camera.to_screen(center),
            _ => (0.0, 0.0),
        };
        let mut camera = self.view_camera();
        camera.zoom_at(factor, middle.0, middle.1);
        self.set_view_camera(camera);
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        let command_line = egui::Id::new(COMMAND_INPUT);
        let typing_here = ctx.memory(|m| m.focused()) == Some(command_line);
        // As in AutoCAD the space bar is Enter on the command line, except where text is being typed.
        let typing_text = self.controller.prompt().is_some_and(|p| p.kind == PromptKind::Text);
        if typing_here && !typing_text && ctx.input(|i| i.key_pressed(Key::Space)) {
            ctx.input_mut(|i| {
                i.events.retain(|e| !matches!(e, egui::Event::Text(t) if t == " "));
                i.consume_key(Modifiers::NONE, Key::Space);
            });
            let typed = std::mem::take(&mut self.input);
            self.actions.push(Action::Input(typed.trim().to_string()));
        }
        // With nothing typed, the clipboard keys copy and paste drawing objects rather than command line text.
        if self.input.is_empty() {
            let command = |key| KeyboardShortcut::new(Modifiers::COMMAND, key);
            ctx.input_mut(|input| {
                for (key, action) in [(Key::C, Action::Copy), (Key::X, Action::Cut), (Key::V, Action::Paste), (Key::A, Action::SelectAll)] {
                    if input.consume_shortcut(&command(key)) {
                        self.actions.push(action);
                    }
                }
                // Text editing turns copy, cut and paste keys into events of their own.
                input.events.retain(|e| match e {
                    egui::Event::Copy => {
                        self.actions.push(Action::Copy);
                        false
                    }
                    egui::Event::Cut => {
                        self.actions.push(Action::Cut);
                        false
                    }
                    egui::Event::Paste(_) => {
                        self.actions.push(Action::Paste);
                        false
                    }
                    _ => true,
                });
            });
        }
        // Esc closes a right-click menu before it cancels anything.
        if self.context_menu.is_some() && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
            self.context_menu = None;
        }
        if ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::Num1))) {
            self.actions.push(Action::ToggleProperties);
        }
        let command = |key| KeyboardShortcut::new(Modifiers::COMMAND, key);
        let command_shift = |key| KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, key);
        let table: [(KeyboardShortcut, Action); 11] = [
            (command_shift(Key::S), Action::SaveAs),
            (command_shift(Key::A), Action::Command("PICKSTYLE")),
            (command_shift(Key::Z), Action::Redo),
            (command(Key::N), Action::New),
            (command(Key::O), Action::Open),
            (command(Key::S), Action::Save),
            (command(Key::P), Action::ExportPdf),
            (command(Key::Z), Action::Undo),
            (command(Key::Y), Action::Redo),
            (command(Key::E), Action::ZoomExtents),
            (KeyboardShortcut::new(Modifiers::NONE, Key::Escape), Action::Cancel),
        ];
        let toggles = [
            (Key::F3, SnapMode::Osnap),
            (Key::F7, SnapMode::ShowGrid),
            (Key::F8, SnapMode::Ortho),
            (Key::F9, SnapMode::Grid),
            (Key::F10, SnapMode::Polar),
            (Key::F11, SnapMode::Tracking),
            (Key::F12, SnapMode::Dynamic),
        ];
        ctx.input_mut(|input| {
            for (shortcut, action) in table {
                if input.consume_shortcut(&shortcut) {
                    self.actions.push(action);
                }
            }
            for (key, mode) in toggles {
                if input.consume_key(Modifiers::NONE, key) {
                    self.actions.push(Action::ToggleSnap(mode));
                }
            }
        });
    }

    fn keep_typing_on_the_command_line(&self, ctx: &egui::Context) {
        if self.dialog.is_some() || self.renaming.is_some() || self.opening.is_some() || self.property_focus {
            return;
        }
        let command = egui::Id::new(COMMAND_INPUT);
        if ctx.memory(|m| m.focused()) != Some(command) {
            ctx.memory_mut(|m| m.request_focus(command));
        }
    }

    fn update_title(&mut self, ctx: &egui::Context) {
        let title = format!("{}{} - {WINDOW_NAME}", self.file_name(), if self.modified() { "*" } else { "" });
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }
}

/// The fonts the drawing's text asks for, by lower-case name.
fn drawing_font_names(document: &Document) -> HashSet<String> {
    document
        .entities()
        .iter()
        .filter_map(|e| match &e.shape {
            Shape::Text { font, .. } if !font.is_empty() => Some(font.to_lowercase()),
            _ => None,
        })
        .collect()
}

fn editor_fonts() -> Vec<String> {
    let mut fonts = crate::shx::available();
    fonts.extend(fonts::catalog().family_names());
    fonts
}

fn drawing_filters(dialog: rfd::FileDialog) -> rfd::FileDialog {
    dialog.add_filter("Drawings", &["dwg", "dxf"]).add_filter("DWG", &["dwg"]).add_filter("DXF", &["dxf"])
}

impl eframe::App for LinewrightApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Some(fonts) = self.fonts_pending.take() {
            self.drawing_fonts = fonts;
            ctx.request_repaint();
        }
        self.poll_opening(ctx);
        if ctx.input(|i| i.viewport().close_requested()) && !self.allow_close && self.modified() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.dialog = Some(Dialog::Discard(Then::Close));
        }
        if self.dialog.is_none() {
            self.shortcuts(ctx);
        }
        self.on_window_edge = frame::resize_from_edges(ctx);

        panels::header(self, ctx);
        panels::ribbon(self, ctx);
        panels::document_tab(self, ctx);
        panels::status_bar(self, ctx);
        panels::command_panel(self, ctx);
        panels::layout_tabs(self, ctx);
        if self.palette_open {
            panels::layer_palette(self, ctx);
        }
        self.property_focus = false;
        if self.properties_open {
            panels::properties_palette(self, ctx);
        }
        panels::canvas(self, ctx);
        panels::context_menu(self, ctx);
        if self.zoom_extents_pending && self.canvas_rect.width() > 0.0 {
            self.zoom_extents_pending = false;
            self.zoom_extents();
        }
        self.open_prompt_dialogs();
        panels::dialogs(self, ctx);
        if let Some(id) = self.controller.edit_text_request.take() {
            self.open_text_editor(id);
        }
        let new_text = self.controller.defaults.borrow_mut().mtext_request.take();
        if let Some(position) = new_text {
            let document = self.document();
            let height = document.text_styles.iter().find(|s| s.name == document.current_text_style).map_or(0.0, |s| s.height);
            self.dialog = Some(Dialog::Mtext(MtextForm {
                target: None,
                position,
                height: if height > 0.0 { height } else { 2.5 },
                text: String::new(),
                styles: vec![],
                current: RunStyle::default(),
                selection: (0, 0),
                fonts: editor_fonts(),
            }));
        }

        for action in std::mem::take(&mut self.actions) {
            self.run(action, ctx);
        }
        self.controller.apply_layout_request();
        if self.controller.tick() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        self.follow_space();
        self.apply_view_request();
        self.keep_typing_on_the_command_line(ctx);
        self.update_title(ctx);
        #[cfg(debug_assertions)]
        if let Some(capture) = self.capture.as_mut().filter(|_| self.opening.is_none() && self.fonts_pending.is_none()) {
            capture.step(ctx);
            match capture.next_input().as_deref() {
                Some("PROPERTIES") => self.properties_open = true,
                Some("ESC") => self.controller.cancel(),
                // HOVER puts the pointer in the middle of the drawing, as if the mouse rested there.
                Some("HOVER") => self.pinned_pointer = Some(self.canvas_rect.center()),
                // MENU and SHIFTMENU right-click (with Shift) in the middle of the drawing.
                Some(word @ ("MENU" | "SHIFTMENU")) => {
                    let at = self.canvas_rect.center() + egui::vec2(-300.0, -200.0);
                    panels::open_context_menu(self, at, word == "SHIFTMENU");
                }
                // TYPE leaves text in the command line, as if typed, without pressing Enter.
                Some(input) if input.starts_with("TYPE ") => {
                    self.input = input["TYPE ".len()..].to_string();
                    ctx.memory_mut(|m| m.request_focus(egui::Id::new(COMMAND_INPUT)));
                }
                Some(input) => self.actions.push(Action::Input(input.to_string())),
                None => {}
            }
        } else if self.capture.is_some() {
            ctx.request_repaint();
        }
        frame::paint_border(ctx);
        if self.on_window_edge {
            frame::resize_from_edges(ctx); // the edge's resize cursor wins over the drawing's crosshair
        }
    }
}

/// The file commands AutoCAD users type, which the window rather than the drawing carries out.
pub(crate) fn file_command(text: &str) -> Option<Action> {
    Some(match text.trim().to_uppercase().as_str() {
        "QSAVE" | "SAVE" => Action::Save,
        "SAVEAS" => Action::SaveAs,
        "OPEN" => Action::Open,
        "NEW" | "QNEW" => Action::New,
        "PLOT" | "PRINT" | "EXPORTPDF" => Action::ExportPdf,
        "DIMSTYLE" | "D" | "DST" | "DDIM" => Action::Command("DIMSTYLE"),
        "PICKSTYLE" => Action::Command("PICKSTYLE"),
        "OSNAP" | "OS" | "DSETTINGS" | "SE" | "DDOSNAP" => Action::Command("OSNAP"),
        "QSELECT" => Action::Command("QSELECT"),
        "XATTACH" | "XA" => Action::Command("XATTACH"),
        "BEDIT" | "BE" => Action::Command("BEDIT"),
        "BCLOSE" | "BC" => Action::Command("BCLOSE"),
        "XREF" | "XR" | "EXTERNALREFERENCES" => Action::Command("XREF"),
        "IMAGEATTACH" | "IAT" | "IM" | "IMAGE" => Action::Command("IMAGEATTACH"),
        "FIND" => Action::Command("FIND"),
        "PURGE" | "PU" => Action::Command("PURGE"),
        "UNITS" | "UN" | "DDUNITS" => Action::Command("UNITS"),
        "LWEIGHT" | "LW" | "LINEWEIGHT" => Action::Command("LWEIGHT"),
        "STYLE" | "ST" | "DDSTYLE" => Action::Command("STYLE"),
        "MTEXT" | "MT" | "T" => Action::Command("MTEXT"),
        "MTEDIT" => Action::Command("MTEDIT"),
        "MLEADERSTYLE" | "MLS" => Action::Command("MLEADERSTYLE"),
        "MLSTYLE" => Action::Command("MLSTYLE"),
        "MSPACE" | "MS" => Action::Command("MSPACE"),
        "PSPACE" | "PS" => Action::Command("PSPACE"),
        "LWDISPLAY" | "LWT" => Action::ToggleLineweights,
        "SELECTSIMILAR" => Action::Command("SELECTSIMILAR"),
        "COLOR" | "COL" | "COLOUR" | "DDCOLOR" => Action::ChooseColor(ColorTarget::Current),
        _ => return None,
    })
}

#[cfg(test)]
mod editor_tests {
    use super::*;

    #[test]
    fn typing_keeps_each_character_s_look_and_lines_become_runs() {
        let bold = RunStyle { bold: true, ..RunStyle::default() };
        let mut form = MtextForm {
            target: None,
            position: Default::default(),
            height: 2.5,
            text: "abcd".into(),
            styles: vec![RunStyle::default(), bold.clone(), bold.clone(), RunStyle::default()],
            current: RunStyle::default(),
            selection: (0, 0),
            fonts: vec![],
        };
        form.text = "abXcd".into();
        form.follow_edit("abcd");
        assert_eq!(form.styles.len(), 5);
        assert_eq!(form.styles[1], bold);
        assert_eq!(form.styles[2], RunStyle::default(), "new text takes the toolbar's look");
        assert_eq!(form.styles[3], bold);
        form.text = "abcd".into();
        form.follow_edit("abXcd");
        assert_eq!(form.styles.len(), 4);
        form.text = "ab\ncd".into();
        form.follow_edit("abcd");
        let lines = form.lines();
        assert_eq!(lines.len(), 2);
        assert_eq!(plain(&lines), "ab\ncd");
        assert_eq!(crate::model::mtext::parse(&serialize(&lines), 2.5), lines);
        form.selection = (0, 2);
        form.apply(|s| s.italic = true);
        assert!(form.styles[0].italic && form.styles[1].italic && !form.styles[3].italic);
    }
}

#[cfg(test)]
mod dialog_tests {
    use super::*;
    use crate::model::entities::{Entity, Shape, line};
    use crate::model::geometry::pt;
    use crate::model::mline::{Justification, MlineStyle};

    /// Runs one frame of the window's dialogs in the app's egui context, with `events` as the frame's input.
    fn frame(app: &mut LinewrightApp, events: Vec<egui::Event>) {
        let ctx = app.egui_ctx.clone();
        let _ = ctx.run(egui::RawInput { events, ..Default::default() }, |ctx| panels::dialogs(app, ctx));
    }

    fn press(key: egui::Key) -> egui::Event {
        egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }
    }

    fn app_with(entities: Vec<Entity>) -> LinewrightApp {
        let mut app = LinewrightApp::for_tests();
        app.controller.set_document(Document::with_entities(entities));
        app
    }

    /// Two copies of one line, which OVERKILL should reduce to one.
    fn two_copies() -> Vec<Entity> {
        let a = line(pt(0.0, 0.0), pt(10.0, 0.0));
        vec![a.clone(), a.duplicate()]
    }

    /// Select everything and start OVERKILL, which asks for its dialog.
    fn start_overkill(app: &mut LinewrightApp) {
        app.controller.handle_input("all");
        app.controller.handle_input("overkill");
        assert_eq!(app.controller.prompt().map(|p| p.kind), Some(PromptKind::OverkillDialog));
        app.open_prompt_dialogs();
        assert!(matches!(app.dialog, Some(Dialog::Overkill(_))));
    }

    #[test]
    fn overkill_dialog_opens_and_enter_accepts_the_changed_form() {
        let mut app = app_with(two_copies());
        start_overkill(&mut app);
        frame(&mut app, vec![]);
        assert!(matches!(app.dialog, Some(Dialog::Overkill(_))), "a frame with no key pressed keeps it open");
        let Some(Dialog::Overkill(form)) = &mut app.dialog else { panic!() };
        form.settings.combine_partial = false;
        form.tolerance = "0.5".into();
        frame(&mut app, vec![press(egui::Key::Enter)]);

        assert!(app.dialog.is_none());
        assert!(app.controller.running_command().is_none());
        assert_eq!(app.document().len(), 1, "the copy is deleted");
        assert!(matches!(app.document().entities()[0].shape, Shape::Line { .. }));
        let remembered = app.controller.defaults.borrow().overkill.clone();
        assert_eq!(remembered.tolerance, 0.5, "the form's tolerance is remembered");
        assert!(!remembered.combine_partial, "the form's options are remembered");
    }

    #[test]
    fn overkill_escape_cancels_and_leaves_the_drawing_unchanged() {
        let mut app = app_with(two_copies());
        start_overkill(&mut app);
        // A modal only takes Escape once it has been drawn, as it is by the time a user presses it.
        frame(&mut app, vec![]);
        frame(&mut app, vec![press(egui::Key::Escape)]);

        assert!(app.dialog.is_none());
        assert!(app.controller.running_command().is_none());
        assert!(app.controller.prompt().is_none());
        assert_eq!(app.document().len(), 2, "nothing is deleted");
        assert!(!app.modified());
    }

    #[test]
    fn overkill_bad_tolerance_shows_the_error_and_does_not_run() {
        let mut app = app_with(two_copies());
        start_overkill(&mut app);
        let Some(Dialog::Overkill(form)) = &mut app.dialog else { panic!() };
        form.tolerance = "a lot".into();
        frame(&mut app, vec![press(egui::Key::Enter)]);

        let Some(Dialog::Overkill(form)) = &app.dialog else { panic!("the dialog stays open") };
        assert_eq!(form.error, "Enter a tolerance, zero or greater.");
        assert_eq!(app.controller.prompt().map(|p| p.kind), Some(PromptKind::OverkillDialog), "the command waits");
        assert_eq!(app.document().len(), 2, "nothing is deleted");

        let Some(Dialog::Overkill(form)) = &mut app.dialog else { panic!() };
        form.tolerance = "0".into();
        frame(&mut app, vec![press(egui::Key::Enter)]);
        assert!(app.dialog.is_none());
        assert_eq!(app.document().len(), 1, "a valid tolerance then runs it");
    }

    fn wall() -> Entity {
        let shape = Shape::Mline {
            vertices: vec![pt(0.0, 0.0), pt(20.0, 0.0)],
            closed: false,
            justification: Justification::Zero,
            scale: 1.0,
            style: MlineStyle::standard(),
            gaps: vec![],
        };
        Entity::new(shape)
    }

    fn drawn_style(app: &LinewrightApp) -> MlineStyle {
        match &app.document().entities()[0].shape {
            Shape::Mline { style, .. } => style.clone(),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn mline_styles_copy_edit_set_current_and_ok_restyles_the_drawn_multiline_as_one_undo() {
        let mut app = app_with(vec![wall()]);
        app.open_mline_styles();
        frame(&mut app, vec![]);
        let Some(Dialog::MlineStyles(form)) = &mut app.dialog else { panic!("the manager is open") };
        assert_eq!(form.current, "Standard");

        // New: a copy of the selected style under the typed name.
        form.new_name = "Thin".into();
        let copy = MlineStyle { name: form.new_name.trim().to_string(), ..form.styles[form.selected].clone() };
        form.styles.push(copy);
        form.selected = form.styles.len() - 1;
        form.new_name.clear();
        // Set Current on the new style, then edit an element of Standard, which the drawn multiline uses.
        form.current = form.styles[form.selected].name.clone();
        form.selected = 0;
        form.element = 0;
        form.styles[0].elements[0].offset = 0.75;
        frame(&mut app, vec![]);
        app.set_mline_styles();

        assert!(app.dialog.is_none());
        assert!(app.document().mline_styles.iter().any(|s| s.name == "Thin"));
        assert_eq!(app.controller.defaults.borrow().mline_style.name, "Thin", "MLINE draws with it next");
        let drawn = drawn_style(&app);
        assert_eq!(drawn.elements[0].offset, 0.75, "the multiline took the change");
        assert_eq!(drawn.elements[1].offset, -0.5);

        app.controller.undo();
        assert_eq!(drawn_style(&app).elements[0].offset, 0.5, "one undo restores the multiline");
        app.controller.redo();
        assert_eq!(drawn_style(&app).elements[0].offset, 0.75);
    }

    #[test]
    fn mline_style_dialog_keeps_the_picked_element_when_offsets_reorder() {
        let mut app = app_with(vec![wall()]);
        app.open_mline_styles();
        for _ in 0..3 {
            frame(&mut app, vec![]);
        }
        let Some(Dialog::MlineStyles(form)) = &mut app.dialog else { panic!() };
        // The picked element is the top line (offset 0.5); moved below the other line it is listed second.
        form.element = 0;
        form.styles[0].elements[0].offset = -0.8;
        frame(&mut app, vec![]);

        let Some(Dialog::MlineStyles(form)) = &app.dialog else { panic!() };
        let offsets: Vec<f64> = form.styles[0].elements.iter().map(|e| e.offset).collect();
        assert_eq!(offsets, vec![-0.5, -0.8], "listed from the top one down");
        assert_eq!(form.element, 1, "the element picked stays picked");
    }
}
