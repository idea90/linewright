use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use super::annotation::AnnotationKind;
use super::dimension::DimensionLook;
use super::geometry::Point;
use super::styles::{BYLAYER_COLOR, BYLAYER_LINETYPE};

pub const BYLAYER: i32 = BYLAYER_COLOR;
pub const DEFAULT_LAYER: &str = "0";

/// Which object an entity is. Copies get a new id; changing an object in place keeps it, so the selection,
/// the drawing order and anything else that refers to it stays valid.
pub type EntityId = u64;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

fn fresh_id() -> EntityId {
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vertex {
    pub x: f64,
    pub y: f64,
    pub bulge: f64, // of the segment that starts here
    /// The width of that segment at its start and end; zero draws a plain line.
    pub start_width: f64,
    pub end_width: f64,
}

impl Vertex {
    pub fn point(&self) -> Point {
        Point { x: self.x, y: self.y }
    }
}

pub fn vertex(point: Point) -> Vertex {
    Vertex { x: point.x, y: point.y, bulge: 0.0, start_width: 0.0, end_width: 0.0 }
}

/// Where text sits relative to its position point, horizontally.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HAlign {
    #[default]
    Left,
    Center,
    Right,
}

/// Where text sits relative to its position point, vertically: on its baseline, or by the bottom, middle or top
/// of the whole text.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum VAlign {
    #[default]
    Baseline,
    Bottom,
    Middle,
    Top,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TextAlign {
    pub h: HAlign,
    pub v: VAlign,
}

/// From one line of text to the next, per text height, at line spacing 1: AutoCAD's MTEXT spacing.
pub const LINE_PITCH: f64 = 5.0 / 3.0;

/// A block definition: entities drawn relative to its base point. Inserts share it.
#[derive(Clone, Debug)]
pub struct BlockDef {
    pub name: String,
    pub base: Point,
    pub entities: Vec<Entity>,
    /// The attributes an insert of the block asks for, with their default values, in block coordinates.
    pub attribute_defs: Vec<Attribute>,
    /// For an external reference, the drawing its entities come from, as the file names it.
    pub xref: Option<String>,
}

/// A shared block definition. Two are the same block when they have the same name.
#[derive(Clone, Debug)]
pub struct BlockRef(pub Arc<BlockDef>);

impl PartialEq for BlockRef {
    fn eq(&self, other: &BlockRef) -> bool {
        Arc::ptr_eq(&self.0, &other.0) || self.0.name == other.0.name
    }
}

/// A value an insert fills in for its block: a tagged text that moves with the insert.
#[derive(Clone, Debug, PartialEq)]
pub struct Attribute {
    pub tag: String,
    /// What INSERT asks when it wants this value; empty asks by the tag.
    pub prompt: String,
    pub layer: String,
    pub color: i32,
    /// Always a Shape::Text, in drawing coordinates.
    pub text: Shape,
    pub invisible: bool,
}

/// One family of parallel lines in a hatch pattern, in drawing units.
#[derive(Clone, Debug, PartialEq)]
pub struct PatternLine {
    pub angle: f64, // degrees
    pub origin: Point,
    /// From one line of the family to the next.
    pub offset: Point,
    /// Dash lengths along the line: positive draws, negative is a gap, zero a dot. Empty is a solid line.
    pub dashes: Vec<f64>,
}

/// A hatch's gradient: two colors along a direction.
#[derive(Clone, Debug, PartialEq)]
pub struct Gradient {
    /// AutoCAD's name for how the colors run: LINEAR, CYLINDER, INVCYLINDER and so on.
    pub name: String,
    pub first: (u8, u8, u8),
    pub second: (u8, u8, u8),
    /// The direction the colors change along, in degrees.
    pub angle: f64,
}

/// What makes a hatch's boundary: the closed objects it was made from, or the points picked inside the areas.
#[derive(Clone, Debug, PartialEq)]
pub enum Association {
    Objects(Vec<EntityId>),
    Seeds(Vec<Point>),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Shape {
    Line {
        start: Point,
        end: Point,
    },
    Circle {
        center: Point,
        radius: f64,
    },
    /// Angles in degrees, running counter-clockwise from start to end.
    Arc {
        center: Point,
        radius: f64,
        start_angle: f64,
        end_angle: f64,
    },
    Polyline {
        vertices: Vec<Vertex>,
        closed: bool,
    },
    /// `position` is the start of the baseline; rotation in degrees. `font` is the font the drawing asks for
    /// (a family such as "Alice4 2000" or a file such as "times.ttf"), empty for the interface font; `style` is the
    /// DXF text style it came with, kept for saving.
    /// `align` says how the text sits on its position; text with several lines steps down by
    /// `line_spacing` × LINE_PITCH × height.
    Text {
        position: Point,
        text: String,
        height: f64,
        rotation: f64,
        font: String,
        style: String,
        align: TextAlign,
        line_spacing: f64,
        /// MTEXT's formatting codes, for text with a look of its own in places; empty for plain text.
        formatted: String,
        /// Letters' width per their height's usual, and how far they lean, in degrees.
        width_factor: f64,
        oblique: f64,
    },
    /// A linear dimension: the distance between two points measured along the direction `angle` (degrees).
    /// `line_point` is any point on the dimension line. `look` holds its sizes and how it writes the measurement.
    Dimension {
        start: Point,
        end: Point,
        line_point: Point,
        angle: f64,
        look: DimensionLook,
    },
    /// A radius, diameter or angle dimension, or a leader, drawn from its points as `kind` says.
    Annotation {
        kind: AnnotationKind,
        points: Vec<Point>,
        look: DimensionLook,
    },
    /// A table: its top left corner at `position`, column widths left to right, row heights top to bottom, and
    /// each row's cell text. With `title`, the first row is one cell across the whole table.
    Table {
        position: Point,
        columns: Vec<f64>,
        rows: Vec<f64>,
        cells: Vec<Vec<String>>,
        text_height: f64,
        title: bool,
    },
    /// A raster image from a file: its lower left corner at `position`, `u` along its full width and `v` up its
    /// full height, `pixels` across and up.
    Image {
        path: String,
        position: Point,
        u: Point,
        v: Point,
        pixels: (u32, u32),
    },
    /// A construction line through `base` along `direction` (a unit vector): without end both ways (XLINE), or
    /// from the base on (RAY).
    XLine {
        base: Point,
        direction: Point,
        ray: bool,
    },
    /// A window on a layout onto model space: a `width` by `height` frame around `center` on the sheet, showing
    /// model space around `view_center`, `view_height` of it across the frame's height. Layers in `frozen` stay
    /// hidden in it.
    Viewport {
        center: Point,
        width: f64,
        height: f64,
        view_center: Point,
        view_height: f64,
        frozen: Vec<String>,
        /// Display locked: its view cannot be zoomed or panned.
        locked: bool,
    },
    /// A multileader: leader lines from their arrowheads to one landing, a dogleg from it (negative runs left), and
    /// the text at its end.
    Multileader {
        /// Each line's points from the arrowhead, not counting the landing they all end at.
        leaders: Vec<Vec<Point>>,
        landing: Point,
        dogleg: f64,
        text: String,
        /// The text's upper corner on the dogleg's side.
        text_position: Point,
        text_height: f64,
        arrow_size: f64,
        /// The name of the multileader style it follows.
        style: String,
    },
    /// An ellipse or elliptical arc: `major` is the vector from the center to one end of the major axis, `ratio`
    /// the minor axis per major, and it runs counter-clockwise between two parameters in radians (0 to 2π whole).
    Ellipse {
        center: Point,
        major: Point,
        ratio: f64,
        start: f64,
        end: f64,
    },
    /// A NURBS curve. Empty weights mean every weight is one.
    Spline {
        degree: usize,
        control: Vec<Point>,
        knots: Vec<f64>,
        weights: Vec<f64>,
        closed: bool,
    },
    Point {
        position: Point,
    },
    /// A filled triangle or quadrilateral, corners in drawing order.
    Solid {
        corners: Vec<Point>,
    },
    /// A wipeout: an area painted in the background color, hiding what is drawn before it, with its frame shown
    /// on screen. Corners in order, the last joining the first.
    Wipeout {
        corners: Vec<Point>,
    },
    /// A multiline: the lines of `style` drawn along the path through `vertices`, `scale` times as far apart as the
    /// style sets them, with the line `justification` names running along the path.
    Mline {
        vertices: Vec<Point>,
        closed: bool,
        justification: super::mline::Justification,
        scale: f64,
        style: super::mline::MlineStyle,
        /// Where MLEDIT has cut its lines.
        gaps: Vec<super::mline::MlineGap>,
    },
    /// Areas bounded by closed loops, filled solid or with a pattern of lines. Loops inside loops are holes.
    Hatch {
        loops: Vec<Vec<Point>>,
        solid: bool,
        pattern: Vec<PatternLine>,
        name: String,
        /// The pattern's scale and angle in degrees, which HATCHEDIT starts from.
        scale: f64,
        angle: f64,
        /// A gradient fill in place of the solid color.
        gradient: Option<Gradient>,
        /// What it follows when it changes; None for a hatch that keeps its loops whatever happens.
        association: Option<Association>,
    },
    /// A block placed in the drawing: moved to `position`, scaled, then turned by `rotation` degrees.
    Insert {
        block: BlockRef,
        position: Point,
        scale_x: f64,
        scale_y: f64,
        rotation: f64,
        attributes: Vec<Attribute>,
    },
}

/// An image's corners, counter-clockwise from its lower left.
pub fn image_corners(position: Point, u: Point, v: Point) -> [Point; 4] {
    [position, position + u, position + u + v, position + v]
}

impl Shape {
    pub fn kind(&self) -> &'static str {
        match self {
            Shape::Line { .. } => "Line",
            Shape::Circle { .. } => "Circle",
            Shape::Arc { .. } => "Arc",
            Shape::Polyline { .. } => "Polyline",
            Shape::Text { .. } => "Text",
            Shape::Dimension { .. } => "Dimension",
            Shape::Annotation { kind, .. } => kind.name(),
            Shape::Viewport { .. } => "Viewport",
            Shape::Multileader { .. } => "Multileader",
            Shape::Image { .. } => "Raster image",
            Shape::Table { .. } => "Table",
            Shape::XLine { ray: false, .. } => "Construction line",
            Shape::XLine { ray: true, .. } => "Ray",
            Shape::Ellipse { .. } => "Ellipse",
            Shape::Spline { .. } => "Spline",
            Shape::Point { .. } => "Point",
            Shape::Solid { .. } => "Solid",
            Shape::Wipeout { .. } => "Wipeout",
            Shape::Mline { .. } => "MLine",
            Shape::Hatch { .. } => "Hatch",
            Shape::Insert { .. } => "Block reference",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Entity {
    pub id: EntityId,
    pub layer: String,
    pub color: i32, // AutoCAD color index, 256 = by layer
    pub linetype: String,
    /// Multiplies the linetype's dashes on top of the drawing's LTSCALE (DXF code 48). 1 is the usual.
    pub linetype_scale: f64,
    /// Hundredths of a millimeter, or ByLayer, ByBlock or Default (see `lineweight`).
    pub lineweight: i16,
    pub shape: Shape,
    /// Set on text that is an attribute definition (ATTDEF), whose text is the tag.
    pub attribute: Option<AttributeInfo>,
}

/// What makes text an attribute definition: what INSERT asks, the value offered, and whether it shows.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AttributeInfo {
    pub prompt: String,
    pub default: String,
    pub invisible: bool,
}

/// An attribute definition as a block keeps it: the tag its text shows, the default value as its text.
pub fn attribute_of(entity: &Entity) -> Option<Attribute> {
    let info = entity.attribute.as_ref()?;
    let Shape::Text { text, .. } = &entity.shape else { return None };
    let mut shape = entity.shape.clone();
    if let Shape::Text { text: value, .. } = &mut shape {
        *value = info.default.clone();
    }
    Some(Attribute {
        tag: text.trim().to_uppercase(),
        prompt: info.prompt.clone(),
        layer: entity.layer.clone(),
        color: entity.color,
        text: shape,
        invisible: info.invisible,
    })
}

/// A block's attribute as an attribute definition to edit: text showing its tag.
pub fn definition_of(attribute: &Attribute) -> Entity {
    let mut shape = attribute.text.clone();
    let default = match &mut shape {
        Shape::Text { text, .. } => std::mem::replace(text, attribute.tag.clone()),
        _ => String::new(),
    };
    let mut entity = Entity::new(shape);
    (entity.layer, entity.color) = (attribute.layer.clone(), attribute.color);
    entity.attribute = Some(AttributeInfo { prompt: attribute.prompt.clone(), default, invisible: attribute.invisible });
    entity
}

impl Entity {
    /// A brand-new object on layer 0, by layer.
    pub fn new(shape: Shape) -> Entity {
        Entity {
            id: fresh_id(),
            layer: DEFAULT_LAYER.to_string(),
            color: BYLAYER,
            linetype: BYLAYER_LINETYPE.to_string(),
            linetype_scale: 1.0,
            lineweight: super::lineweight::BYLAYER,
            shape,
            attribute: None,
        }
    }

    /// A new object with this one's layer, color and linetype but a different shape.
    pub fn derive(&self, shape: Shape) -> Entity {
        Entity { id: fresh_id(), shape, ..self.clone() }.without_association()
    }

    /// A new object's hatch follows nothing: the boundary it was copied from is the original's.
    fn without_association(mut self) -> Entity {
        if let Shape::Hatch { association, .. } = &mut self.shape {
            *association = None;
        }
        self
    }

    /// The same object with its values changed, keeping its identity.
    pub fn with_shape(&self, shape: Shape) -> Entity {
        Entity { shape, ..self.clone() }
    }

    /// A copy that is a different object.
    pub fn duplicate(&self) -> Entity {
        Entity { id: fresh_id(), ..self.clone() }.without_association()
    }

    /// Whether the two hold the same values, whichever objects they are.
    pub fn same_state(&self, other: &Entity) -> bool {
        self.layer == other.layer
            && self.color == other.color
            && self.linetype == other.linetype
            && self.linetype_scale == other.linetype_scale
            && self.shape == other.shape
            && self.attribute == other.attribute
    }

    /// Take the other's values, keeping this object's identity.
    pub fn copy_state(&mut self, source: &Entity) {
        let id = self.id;
        *self = source.clone();
        self.id = id;
    }
}

pub fn line(start: Point, end: Point) -> Entity {
    Entity::new(Shape::Line { start, end })
}

pub fn circle(center: Point, radius: f64) -> Entity {
    Entity::new(Shape::Circle { center, radius })
}

pub fn arc(center: Point, radius: f64, start_angle: f64, end_angle: f64) -> Entity {
    Entity::new(Shape::Arc { center, radius, start_angle, end_angle })
}

pub fn polyline(vertices: Vec<Vertex>, closed: bool) -> Entity {
    Entity::new(Shape::Polyline { vertices, closed })
}

pub fn polyline_from(points: &[Point], closed: bool) -> Entity {
    polyline(points.iter().copied().map(vertex).collect(), closed)
}

pub fn text(position: Point, value: &str, height: f64, rotation: f64) -> Entity {
    Entity::new(Shape::Text {
        position,
        text: value.to_string(),
        height,
        rotation,
        font: String::new(),
        style: String::new(),
        align: TextAlign::default(),
        line_spacing: 1.0,
        formatted: String::new(),
        width_factor: 1.0,
        oblique: 0.0,
    })
}

/// A dimension in Linewright's own look, at a scale.
pub fn dimension(start: Point, end: Point, line_point: Point, angle: f64, scale: f64) -> Entity {
    Entity::new(Shape::Dimension { start, end, line_point, angle, look: DimensionLook::scaled(scale) })
}

pub fn ellipse(center: Point, major: Point, ratio: f64) -> Entity {
    Entity::new(Shape::Ellipse { center, major, ratio, start: 0.0, end: std::f64::consts::TAU })
}

pub fn point(position: Point) -> Entity {
    Entity::new(Shape::Point { position })
}
