use crate::model::dimension::{DimensionLook, DimensionStyle};
use crate::model::document::ObjectGroup;
use crate::model::entities::{BlockRef, Entity};
use crate::model::index::Bounds;

/// How many minutes pass between autosaves when nothing has been set.
pub const DEFAULT_SAVETIME: u32 = 10;
/// The longest autosave interval SAVETIME takes, in minutes. Zero turns autosave off.
pub const MAX_SAVETIME: u32 = 600;

/// How LENGTHEN changes what it is given.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Lengthen {
    Delta(f64),
    Percent(f64),
    Total(f64),
}

/// What commands remember from one use to the next, and what the drawing offers them.
#[derive(Clone, Debug)]
pub struct Defaults {
    pub fillet_radius: f64,
    /// The look new dimensions take: the drawing's dimension style, as DIMSCALE, DIMTXT and DIMASZ change it.
    pub dimension_look: DimensionLook,
    /// The dimension drawn last, which DIMCONTINUE and DIMBASELINE carry on from.
    pub last_dimension: Option<Entity>,
    /// A table and the point in it double-clicked, for TABLEDIT to start from.
    pub table_cell: Option<(Entity, crate::model::geometry::Point)>,
    /// The image file IMAGEATTACH places next, and its size in pixels.
    pub image: Option<(String, (u32, u32))>,
    /// The chord of each revision cloud arc, once REVCLOUD has one.
    pub cloud_arc: Option<f64>,
    /// How LENGTHEN last changed lengths.
    pub lengthen: Lengthen,
    /// How MLINE draws: which line runs along the points, how far apart the lines are, and in which style.
    pub mline_justification: crate::model::mline::Justification,
    pub mline_scale: f64,
    pub mline_style: crate::model::mline::MlineStyle,
    /// The drawing's multiline styles, for MLINE's STyle option. The window keeps it up to date.
    pub mline_styles: Vec<crate::model::mline::MlineStyle>,
    /// How OVERKILL last compared and combined objects.
    pub overkill: crate::model::overkill::OverkillSettings,
    /// The layers as they were before LAYISO, for LAYUNISO.
    pub isolated_from: Option<Vec<crate::model::document::Layer>>,
    /// The drawing's layers and current layer, for the layer tools. The window keeps them up to date.
    pub layers: Vec<crate::model::document::Layer>,
    pub current_layer: String,
    /// The drawing's dimension styles, which -DIMSTYLE lists and restores. The window keeps it up to date.
    pub dimension_styles: Vec<DimensionStyle>,
    /// The layouts by name, which one is open, its sheet, and the extent of model space, for LAYOUT and MVIEW.
    pub layouts: Vec<String>,
    /// The drawing's groups, for GROUP, UNGROUP and GROUPEDIT.
    pub groups: Vec<ObjectGroup>,
    pub active_layout: Option<usize>,
    /// The viewports of the layout on screen, and the one model space is open through, for VPLAYER.
    pub viewports: Vec<Entity>,
    pub through_viewport: Option<crate::model::entities::EntityId>,
    pub sheet: Option<Bounds>,
    pub model_extent: Option<Bounds>,
    /// The text style new text follows.
    pub text_style: crate::model::document::TextStyleDef,
    /// Corners given for new multiline text, for the window to open its editor with.
    pub mtext_request: Option<crate::model::geometry::Point>,
    /// The multileader style new multileaders follow.
    pub mleader_style: crate::model::multileader::MleaderStyle,
    pub chamfer_distances: (f64, f64),
    /// The drawing's WIPEOUTFRAME mode, which WIPEOUT's Frames option shows. The window keeps it up to date.
    pub wipeout_frame: u8,
    /// The drawing's LTSCALE, PSLTSCALE and CELTSCALE, which their commands show as the current values.
    pub ltscale: f64,
    pub psltscale: bool,
    pub celtscale: f64,
    pub hatch_pattern: String,
    pub hatch_scale: f64,
    pub hatch_angle: f64,
    pub last_block: String,
    /// The drawing's blocks, which INSERT can place. The window keeps it up to date.
    pub blocks: Vec<BlockRef>,
    /// What COPYCLIP and CUTCLIP last copied, with the point it is pasted by.
    pub clipboard: Vec<Entity>,
    pub clipboard_base: crate::model::geometry::Point,
    /// The objects the last ERASE or BLOCK took out of the drawing, which OOPS brings back.
    pub last_erased: Vec<Entity>,
    /// The SAVETIME autosave interval in minutes, which the window keeps up to date.
    pub autosave_minutes: u32,
    /// A SAVETIME value typed, for the window to set and remember.
    pub autosave_request: Option<u32>,
}

impl Default for Defaults {
    fn default() -> Self {
        Defaults {
            fillet_radius: 0.0,
            dimension_look: DimensionLook::default(),
            last_dimension: None,
            cloud_arc: None,
            image: None,
            table_cell: None,
            lengthen: Lengthen::Total(1.0),
            overkill: crate::model::overkill::OverkillSettings::default(),
            mline_justification: crate::model::mline::Justification::Top,
            mline_scale: 20.0,
            mline_style: crate::model::mline::MlineStyle::standard(),
            mline_styles: vec![crate::model::mline::MlineStyle::standard()],
            isolated_from: None,
            layers: vec![],
            current_layer: "0".into(),
            dimension_styles: vec![DimensionStyle::standard()],
            layouts: vec![],
            groups: vec![],
            active_layout: None,
            viewports: vec![],
            through_viewport: None,
            sheet: None,
            model_extent: None,
            text_style: crate::model::document::TextStyleDef::standard(),
            mtext_request: None,
            mleader_style: crate::model::multileader::MleaderStyle::standard(),
            chamfer_distances: (0.0, 0.0),
            wipeout_frame: 1,
            ltscale: 1.0,
            psltscale: true,
            celtscale: 1.0,
            hatch_pattern: "SOLID".into(),
            hatch_scale: 1.0,
            hatch_angle: 0.0,
            last_block: String::new(),
            blocks: vec![],
            clipboard: vec![],
            clipboard_base: Default::default(),
            last_erased: vec![],
            autosave_minutes: DEFAULT_SAVETIME,
            autosave_request: None,
        }
    }
}
