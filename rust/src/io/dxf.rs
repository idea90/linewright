//! Reading and writing DXF drawings at the level of group codes.
//!
//! A file is kept as its list of groups (each starts with a code 0 pair), so whatever Linewright does not understand —
//! blocks, hatches, layouts, objects, extended data — goes back into the file untouched when it is saved. Loading
//! takes the entities Linewright can edit out of that list; saving puts the document's entities back in.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

use crate::canvas::aci::{index_of, is_true_color, true_color};
use crate::model::annotation::{AnnotationKind, angle_between_lines, annotation_measurement, annotation_parts};
use crate::model::dimension::{AnnotationText, Arrowhead, DimensionLook, DimensionStyle, Tolerance, dimension_parts};
use crate::model::document::{DEFAULT_LINETYPE, Document, Layer, Layout, ObjectGroup, PlotSettings, TextStyleDef};
use crate::model::editing::explode;
use crate::model::entities::vertex;
use crate::model::entities::{
    Association, Attribute, AttributeInfo, BYLAYER, BlockDef, BlockRef, DEFAULT_LAYER, Entity, EntityId, Gradient, HAlign, PatternLine,
    Shape, TextAlign, VAlign, Vertex,
};
use crate::model::geometry::{Point, bulge_arc, direction, pt, sweep_degrees};
use crate::model::index::{Bounds, entity_extent};
use crate::model::lineweight;
use crate::model::mline::{Justification, MlineElement, MlineGap, MlineStyle, dedup, element_distances, miters};
use crate::model::mtext::decode_percent_codes;
use crate::model::multileader::MleaderStyle;
use crate::model::parts::{BYBLOCK_LINETYPE, TextSpec, clamped_knots, ellipse_point, entity_parts, spline_points};
use crate::model::styles::{BYLAYER_LINETYPE, canonical_linetype};

pub mod file;
use file::pair;
pub use file::{DxfError, DxfFile, Group, Pair};

/// A new drawing: an empty R2018 file in millimeters with the standard linetypes, text and dimension styles.
const DIMENSION_STYLE: &str = "Standard";
const LINEAR_DIMENSION: i64 = 0;
/// A block that is an external reference, and one that depends on another drawing, by its flags.
const BLOCK_XREF: i64 = 4;
const BLOCK_EXTERNAL: i64 = 16;
const ALIGNED_DIMENSION: i64 = 1;
const TWO_LINE_ANGULAR_DIMENSION: i64 = 2;
const DIAMETER_DIMENSION: i64 = 3;
const RADIUS_DIMENSION: i64 = 4;
const THREE_POINT_ANGULAR_DIMENSION: i64 = 5;
const ORDINATE_DIMENSION: i64 = 6;
const ORDINATE_X_TYPE: i64 = 64;
/// ARC_DIMENSION's type: 5 up to R2013, 8 from R2018 (outside the mask the other types use).
const ARC_DIMENSION_TYPE: i64 = 5;
const ARC_DIMENSION_TYPE_2018: i64 = 8;
const LARGE_RADIAL_DIMENSION_TYPE: i64 = 9;
/// Every kind of entity that is a dimension, with a block AutoCAD draws it with.
const DIMENSION_KINDS: [&str; 3] = ["DIMENSION", "ARC_DIMENSION", "LARGE_RADIAL_DIMENSION"];
const DIMENSION_TYPE_MASK: i64 = 7;
const BLOCK_REFERENCED_BY_THIS_DIMENSION_ONLY: i64 = 32;
const KEPT_LAYERS: [&str; 2] = ["0", "defpoints"];
const LAYER_FROZEN: i64 = 1;
const LAYER_LOCKED: i64 = 4;

/// Millimeters in one drawing unit for each $INSUNITS code Linewright knows. Unitless (0) drawings are read as millimeters.
fn mm_per_unit_code(code: i64) -> f64 {
    match code {
        1 => 25.4,
        2 => 304.8,
        5 => 10.0,
        6 => 1000.0,
        _ => 1.0,
    }
}

/// Full and short name of a drawing unit, by how many millimeters it is.
fn unit_names_for(mm: f64) -> (&'static str, &'static str) {
    match mm {
        10.0 => ("centimeters", "cm"),
        1000.0 => ("meters", "m"),
        25.4 => ("inches", "in"),
        304.8 => ("feet", "ft"),
        _ => ("millimeters", "mm"),
    }
}

/// Standard linetypes Linewright can add to a file that lacks them: name, description, pattern (total, then elements).
const STANDARD_LINETYPES: [(&str, &str, &[f64]); 6] = [
    ("CONTINUOUS", "Solid", &[0.0]),
    ("CENTER", "Center ____ _ ____ _ ____ _ ____ _ ____ _ ____", &[2.0, 1.25, -0.25, 0.25, -0.25]),
    ("DASHED", "Dashed __ __ __ __ __ __ __ __ __ __ __ __ __ _", &[0.6, 0.5, -0.1]),
    ("PHANTOM", "Phantom ______  __  __  ______  __  __  ______", &[2.5, 1.25, -0.25, 0.25, -0.25, 0.25, -0.25]),
    ("DASHDOT", "Dash dot __ . __ . __ . __ . __ . __ . __ . __", &[1.4, 1.0, -0.2, 0.0, -0.2]),
    ("HIDDEN", "Hidden __ __ __ __ __ __ __ __ __ __ __ __ __", &[0.375, 0.25, -0.125]),
];

fn number(value: f64) -> String {
    use std::fmt::Write as _;
    // Room for any number and the ".0" a whole one gets, so the text is made once.
    let mut text = String::with_capacity(24);
    let _ = write!(text, "{value}");
    if value.is_finite() && !text.contains(['.', 'e', 'E']) {
        text.push_str(".0");
    }
    text
}

fn point_pairs(x_code: i32, p: Point) -> [Pair; 3] {
    [pair(x_code, number(p.x)), pair(x_code + 10, number(p.y)), pair(x_code + 20, "0.0")]
}

/// The special characters back as AutoCAD's codes, for TEXT, which every program reads that way.
fn encode_percent_codes(text: &str) -> String {
    text.replace('⌀', "%%c").replace('°', "%%d").replace('±', "%%p")
}

// The file itself (pairs, groups, encodings, header) is in `file`.

// ---------------------------------------------------------------------------------------------------- reading

pub fn mm_per_unit(document: &Document) -> f64 {
    match document.units {
        Some(code) => mm_per_unit_code(code),
        None => document.source.as_ref().map_or(1.0, |source| mm_per_unit_code(source.insunits())),
    }
}

pub fn unit_names(document: &Document) -> (&'static str, &'static str) {
    unit_names_for(mm_per_unit(document))
}

/// An entity's layer, color and linetype. ByBlock means something only inside a block; elsewhere it is by layer.
fn style(group: &Group, in_block: bool) -> (String, i32, String) {
    let mut linetype = canonical_linetype(group.get(6).map_or(BYLAYER_LINETYPE, str::trim));
    if linetype.eq_ignore_ascii_case("byblock") {
        linetype = if in_block { BYBLOCK_LINETYPE.into() } else { BYLAYER_LINETYPE.into() };
    }
    let layer = group.text(8).unwrap_or_else(|| DEFAULT_LAYER.into());
    (layer, color_of(group, BYLAYER), linetype)
}

/// An entity's lineweight (code 370). ByBlock means something only inside a block; elsewhere it is by layer.
fn lineweight_of(group: &Group, in_block: bool) -> i16 {
    match group.int(370).map_or(lineweight::BYLAYER, lineweight::from_file) {
        lineweight::BYBLOCK if !in_block => lineweight::BYLAYER,
        lw => lw,
    }
}

/// An entity's linetype scale (code 48), which is 1 when the file leaves it out or gives no usable scale.
fn linetype_scale_of(group: &Group) -> f64 {
    group.float(48).filter(|scale| *scale > 0.0).unwrap_or(1.0)
}

/// The color a group gives: its true color (code 420) when it has one, else its index color (62).
fn color_of(group: &Group, otherwise: i32) -> i32 {
    match group.int(420) {
        Some(rgb) => true_color((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8),
        None => group.int(62).map_or(otherwise, |c| c as i32),
    }
}

/// A true color as code 420 writes it: 0xRRGGBB.
fn true_color_value(color: i32) -> Option<i64> {
    is_true_color(color).then_some((color & 0xFF_FFFF) as i64)
}

fn in_world_plane(group: &Group) -> bool {
    match (group.float(210), group.float(220), group.float(230)) {
        (None, None, None) => true,
        (x, y, z) => x.unwrap_or(0.0).abs() < 1e-9 && y.unwrap_or(0.0).abs() < 1e-9 && (z.unwrap_or(1.0) - 1.0).abs() < 1e-9,
    }
}

/// MTEXT without its formatting codes. Paragraph breaks become new lines.
pub fn plain_mtext(raw: &str) -> String {
    let mut out = String::new();
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' | '}' => {}
            '\\' => match chars.next() {
                Some('P') => out.push('\n'),
                Some('~') => out.push(' '),
                Some(escaped @ ('\\' | '{' | '}')) => out.push(escaped),
                Some('S') => {
                    // Stacked fractions: \Sup^down; shows as up/down.
                    for c in chars.by_ref() {
                        match c {
                            ';' => break,
                            '^' | '#' => out.push('/'),
                            other => out.push(other),
                        }
                    }
                }
                Some('L' | 'l' | 'O' | 'o' | 'K' | 'k' | 'N') => {}
                Some(_) => {
                    // Font, height, color and other settings run up to a semicolon.
                    for c in chars.by_ref() {
                        if c == ';' {
                            break;
                        }
                    }
                }
                None => {}
            },
            other => out.push(other),
        }
    }
    out
}

/// TEXT's alignment codes (72 horizontal, 73 vertical) as an alignment. Aligned and fit text (3, 5) stretch
/// between two points; they are shown left aligned on their first point.
fn text_align(horizontal: i64, vertical: i64) -> TextAlign {
    let h = match horizontal {
        1 | 4 => HAlign::Center,
        2 => HAlign::Right,
        _ => HAlign::Left,
    };
    let v = match (horizontal, vertical) {
        (4, _) => VAlign::Middle,
        (_, 1) => VAlign::Bottom,
        (_, 2) => VAlign::Middle,
        (_, 3) => VAlign::Top,
        _ => VAlign::Baseline,
    };
    TextAlign { h, v }
}

/// The TEXT alignment codes for an alignment.
fn text_align_codes(align: TextAlign) -> (i64, i64) {
    let h = match align.h {
        HAlign::Left => 0,
        HAlign::Center => 1,
        HAlign::Right => 2,
    };
    let v = match align.v {
        VAlign::Baseline => 0,
        VAlign::Bottom => 1,
        VAlign::Middle => 2,
        VAlign::Top => 3,
    };
    (h, v)
}

/// MTEXT's attachment point (71): 1 to 9 by rows from top left to bottom right.
fn mtext_align(attachment: i64) -> TextAlign {
    let h = match (attachment - 1).rem_euclid(3) {
        1 => HAlign::Center,
        2 => HAlign::Right,
        _ => HAlign::Left,
    };
    let v = match attachment {
        4..=6 => VAlign::Middle,
        7..=9 => VAlign::Bottom,
        _ => VAlign::Top,
    };
    TextAlign { h, v }
}

fn mtext_attachment(align: TextAlign) -> i64 {
    let row = match align.v {
        VAlign::Top => 0,
        VAlign::Middle => 1,
        VAlign::Bottom | VAlign::Baseline => 2,
    };
    let column = match align.h {
        HAlign::Left => 1,
        HAlign::Center => 2,
        HAlign::Right => 3,
    };
    row * 3 + column
}

/// Chords per whole turn for curved hatch boundaries.
const HATCH_ARC_SEGMENTS: f64 = 72.0;

/// Points along an arc of a hatch boundary, from its start to its end. Clockwise edges store their angles mirrored.
fn hatch_arc(center: Point, radius: f64, start: f64, end: f64, counter_clockwise: bool) -> Vec<Point> {
    let (from, sweep) = if counter_clockwise { (start, sweep_degrees(start, end)) } else { (-start, -sweep_degrees(start, end)) };
    let count = (sweep.abs() / 360.0 * HATCH_ARC_SEGMENTS).ceil().max(2.0) as usize;
    (0..=count).map(|i| center + direction(from + sweep * i as f64 / count as f64) * radius).collect()
}

/// A hatch's boundary loops and pattern, read in order: its group codes repeat, so their order is what counts.
fn read_hatch(group: &Group) -> Option<Shape> {
    let pairs: Vec<&Pair> = group.0.iter().take_while(|p| p.code < 1000).collect();
    let mut at = 0;
    let float = |p: &Pair| p.value.trim().parse::<f64>().unwrap_or(0.0);
    let int = |p: &Pair| p.value.trim().parse::<f64>().unwrap_or(0.0) as i64;
    // The next pair with this code from where reading is, as a number.
    let next = |code: i32, at: &mut usize| -> Option<f64> {
        let found = pairs[*at..].iter().position(|p| p.code == code)? + *at;
        *at = found + 1;
        Some(float(pairs[found]))
    };
    let name = group.text(2).unwrap_or_default().trim().to_string();
    let solid = group.int(70).unwrap_or(0) == 1;
    let path_count = next(91, &mut at)? as usize;
    let mut loops = vec![];
    for _ in 0..path_count {
        let flags = next(92, &mut at)? as i64;
        let mut points: Vec<Point> = vec![];
        if flags & 2 != 0 {
            let has_bulge = next(72, &mut at)? != 0.0;
            next(73, &mut at)?;
            let count = next(93, &mut at)? as usize;
            let mut vertices = vec![];
            for _ in 0..count {
                let (x, y) = (next(10, &mut at)?, next(20, &mut at)?);
                let bulge = if has_bulge && pairs.get(at).is_some_and(|p| p.code == 42) { next(42, &mut at)? } else { 0.0 };
                vertices.push(Vertex { x, y, bulge, start_width: 0.0, end_width: 0.0 });
            }
            for (i, v) in vertices.iter().enumerate() {
                let following = vertices[(i + 1) % vertices.len()];
                points.push(v.point());
                if v.bulge != 0.0 && following.point() != v.point() {
                    let arc = bulge_arc(v.point(), following.point(), v.bulge);
                    let count = (arc.sweep.abs() / std::f64::consts::TAU * HATCH_ARC_SEGMENTS).ceil().max(2.0) as usize;
                    points.extend((1..count).map(|k| {
                        arc.center + direction((arc.start_angle + arc.sweep * k as f64 / count as f64).to_degrees()) * arc.radius
                    }));
                }
            }
        } else {
            let edges = next(93, &mut at)? as usize;
            for _ in 0..edges {
                let kind = pairs[at..].iter().position(|p| p.code == 72).map(|k| int(pairs[at + k]))?;
                next(72, &mut at)?;
                let edge: Vec<Point> = match kind {
                    1 => vec![pt(next(10, &mut at)?, next(20, &mut at)?), pt(next(11, &mut at)?, next(21, &mut at)?)],
                    2 => {
                        let center = pt(next(10, &mut at)?, next(20, &mut at)?);
                        let (radius, start, end) = (next(40, &mut at)?, next(50, &mut at)?, next(51, &mut at)?);
                        hatch_arc(center, radius, start, end, next(73, &mut at)? != 0.0)
                    }
                    3 => {
                        let center = pt(next(10, &mut at)?, next(20, &mut at)?);
                        let major = pt(next(11, &mut at)?, next(21, &mut at)?);
                        let (ratio, start, end) = (next(40, &mut at)?, next(50, &mut at)?, next(51, &mut at)?);
                        let ccw = next(73, &mut at)? != 0.0;
                        let (from, to) = if ccw { (start, end) } else { (-start, -end) };
                        let sweep = if ccw { sweep_degrees(from, to) } else { -sweep_degrees(-from, -to) };
                        let count = (sweep.abs() / 360.0 * HATCH_ARC_SEGMENTS).ceil().max(2.0) as usize;
                        (0..=count)
                            .map(|k| ellipse_point(center, major, ratio, (from + sweep * k as f64 / count as f64).to_radians()))
                            .collect()
                    }
                    4 => {
                        let degree = next(94, &mut at)? as usize;
                        let (knot_count, control_count) = (next(95, &mut at)? as usize, next(96, &mut at)? as usize);
                        let knots: Option<Vec<f64>> = (0..knot_count).map(|_| next(40, &mut at)).collect();
                        let control: Option<Vec<Point>> =
                            (0..control_count).map(|_| Some(pt(next(10, &mut at)?, next(20, &mut at)?))).collect();
                        spline_points(degree, &control?, &knots?, &[])
                    }
                    _ => return None,
                };
                if points.last() == edge.first() {
                    points.extend(edge.into_iter().skip(1));
                } else {
                    points.extend(edge);
                }
            }
        }
        if points.len() >= 3 {
            loops.push(points);
        }
    }
    let mut pattern = vec![];
    if !solid && let Some(lines) = next(78, &mut at) {
        for _ in 0..lines as usize {
            let angle = next(53, &mut at)?;
            let origin = pt(next(43, &mut at)?, next(44, &mut at)?);
            let offset = pt(next(45, &mut at)?, next(46, &mut at)?);
            let count = next(79, &mut at)? as usize;
            let dashes: Option<Vec<f64>> = (0..count).map(|_| next(49, &mut at)).collect();
            pattern.push(PatternLine { angle, origin, offset, dashes: dashes? });
        }
    }
    // Scale and angle come just before the pattern lines; a gradient's colors come after the seed points.
    let (scale, angle) = (group.float(41).unwrap_or(1.0), group.float(52).unwrap_or(0.0));
    let gradient = (group.int(450).unwrap_or(0) != 0).then(|| {
        let mut colors = group.0.iter().take_while(|p| p.code < 1000).filter(|p| p.code == 421).map(|p| {
            let rgb = p.value.trim().parse::<i64>().unwrap_or(0);
            ((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
        });
        let first = colors.next().unwrap_or((0, 0, 255));
        Gradient {
            name: group.text(470).unwrap_or_else(|| "LINEAR".into()).trim().to_string(),
            first,
            second: colors.next().unwrap_or(first),
            angle: group.float(460).unwrap_or(0.0).to_degrees(),
        }
    });
    (!loops.is_empty()).then_some(Shape::Hatch { loops, solid, pattern, name, scale, angle, gradient, association: None })
}

/// A TEXT or ATTRIB as text: where it sits and how, its value with AutoCAD's codes decoded, and its style's font.
fn read_text(group: &Group, styles: &DimensionStyles) -> Option<Shape> {
    let style = group.text(7).unwrap_or_default();
    let (horizontal, vertical) = (group.int(72).unwrap_or(0), group.int(74).or_else(|| group.int(73)).unwrap_or(0));
    let align = text_align(horizontal, vertical);
    let aligned = !matches!(horizontal, 0 | 3 | 5) || vertical != 0;
    let position = if aligned { group.point(11).or_else(|| group.point(10))? } else { group.point(10)? };
    Some(Shape::Text {
        position,
        text: decode_percent_codes(&group.text(1).unwrap_or_default()),
        height: group.float(40)?,
        rotation: group.float(50).unwrap_or(0.0),
        font: styles.font_of_style(&style),
        width_factor: group.float(41).filter(|w| *w > 0.0).unwrap_or(1.0),
        oblique: group.float(51).unwrap_or(0.0),
        style,
        align,
        line_spacing: 1.0,
        formatted: String::new(),
    })
}

/// Reads entities, wherever they are: in model space or inside a block definition. Blocks are read the first time
/// an insert asks for them.
struct Reader<'a> {
    mleader_style_names: HashMap<String, String>,
    file: &'a DxfFile,
    /// Layer names by upper-case handle, for the layers a viewport freezes.
    layers_by_handle: HashMap<String, String>,
    styles: DimensionStyles,
    /// Each block definition by upper-case name: its groups (BLOCK to ENDBLK) and name as written.
    definitions: HashMap<String, (usize, usize)>,
    /// The blocks read so far. A lock, not a cell, so that entities can be read on several threads at once.
    built: std::sync::RwLock<HashMap<String, Option<BlockRef>>>,
}

impl<'a> Reader<'a> {
    fn new(file: &'a DxfFile) -> Reader<'a> {
        let mut definitions = HashMap::new();
        if let Some((start, end)) = file.section("BLOCKS") {
            let mut index = start + 1;
            while index < end {
                if file.groups[index].kind() == "BLOCK" {
                    let finish = (index..end).find(|&i| file.groups[i].kind() == "ENDBLK").unwrap_or(end);
                    if let Some(name) = file.groups[index].get(2) {
                        definitions.insert(name.trim().to_uppercase(), (index, finish));
                    }
                    index = finish;
                }
                index += 1;
            }
        }
        let layers_by_handle = file
            .table_entries("LAYER")
            .into_iter()
            .filter_map(|i| Some((file.groups[i].handle()?.trim().to_uppercase(), file.groups[i].get(2)?.trim().to_string())))
            .collect();
        // Multileader styles by the handle a MULTILEADER names them with.
        let mleader_style_names = mleader_dictionary(file)
            .map(|at| {
                let pairs = &file.groups[at].0;
                pairs
                    .windows(2)
                    .filter(|w| w[0].code == 3 && matches!(w[1].code, 350 | 360))
                    .map(|w| (w[1].value.trim().to_uppercase(), w[0].value.trim().to_string()))
                    .collect()
            })
            .unwrap_or_default();
        Reader { file, layers_by_handle, styles: DimensionStyles::read(file), definitions, built: Default::default(), mleader_style_names }
    }

    /// A block definition by name, read the first time. A block that (through others) holds itself gets nothing
    /// for the inner copy rather than going round for ever.
    fn block(&self, name: &str) -> Option<BlockRef> {
        let key = name.trim().to_uppercase();
        let found = self.built.read().unwrap_or_else(|poisoned| poisoned.into_inner()).get(&key).cloned();
        if let Some(found) = found {
            return found;
        }
        let &(start, finish) = self.definitions.get(&key)?;
        self.built.write().unwrap_or_else(|poisoned| poisoned.into_inner()).insert(key.clone(), None);
        let header = &self.file.groups[start];
        let base = header.point(10).unwrap_or_default();
        let body = &self.file.groups[start + 1..finish];
        let entities = entity_runs(body).into_iter().filter_map(|(a, b)| self.entity(&body[a..b], true)).collect();
        let attribute_defs = body
            .iter()
            .filter(|g| g.kind() == "ATTDEF")
            .filter_map(|g| {
                let (layer, color, _) = style(g, true);
                Some(Attribute {
                    tag: g.text(2).unwrap_or_default().trim().to_string(),
                    prompt: g.text(3).unwrap_or_default(),
                    layer,
                    color,
                    text: read_text(g, &self.styles)?,
                    invisible: g.int(70).unwrap_or(0) & 1 != 0,
                })
            })
            .collect();
        let name = header.text(2).map_or(key.clone(), |n| n.trim().to_string());
        // An external reference keeps its drawing's path; what it shows is read from there once the file is open.
        let xref = (header.int(70).unwrap_or(0) & BLOCK_XREF != 0).then(|| header.text(1).unwrap_or_default().trim().to_string());
        let block = BlockRef(std::sync::Arc::new(BlockDef { name, base, entities, attribute_defs, xref }));
        self.built.write().unwrap_or_else(|poisoned| poisoned.into_inner()).insert(key, Some(block.clone()));
        Some(block)
    }

    /// Read every block the file defines now. While one is being read it counts as empty, which is how a block
    /// that holds itself stops; with them all read first, entities read on several threads never meet one half
    /// read.
    fn read_every_block(&self) {
        let mut names: Vec<&String> = self.definitions.keys().collect();
        names.sort();
        for name in names {
            self.block(name);
        }
    }

    fn entity(&self, run: &[Group], in_block: bool) -> Option<Entity> {
        // An attribute definition on its own (inside a block it is one of the block's attributes): text showing its
        // tag, with its prompt, default value and visibility.
        if run[0].kind() == "ATTDEF" && !in_block {
            let group = &run[0];
            let mut shape = read_text(group, &self.styles)?;
            let Shape::Text { text, .. } = &mut shape else { return None };
            let default = std::mem::replace(text, group.text(2).unwrap_or_default().trim().to_string());
            let (layer, color, linetype) = style(group, false);
            let info = AttributeInfo { prompt: group.text(3).unwrap_or_default(), default, invisible: group.int(70).unwrap_or(0) & 1 != 0 };
            let lineweight = lineweight_of(group, false);
            let linetype_scale = linetype_scale_of(group);
            return Some(Entity { layer, color, linetype, linetype_scale, lineweight, attribute: Some(info), ..Entity::new(shape) });
        }
        let shape = self.shape(run)?;
        let (layer, color, linetype) = style(&run[0], in_block);
        let linetype_scale = linetype_scale_of(&run[0]);
        Some(Entity { layer, color, linetype, linetype_scale, lineweight: lineweight_of(&run[0], in_block), ..Entity::new(shape) })
    }

    fn shape(&self, groups: &[Group]) -> Option<Shape> {
        let group = &groups[0];
        let styles = &self.styles;
        if !in_world_plane(group) {
            return None;
        }
        Some(match group.kind() {
            "LINE" => Shape::Line { start: group.point(10)?, end: group.point(11)? },
            "CIRCLE" => Shape::Circle { center: group.point(10)?, radius: group.float(40)? },
            "ARC" => Shape::Arc {
                center: group.point(10)?,
                radius: group.float(40)?,
                start_angle: group.float(50)?,
                end_angle: group.float(51)?,
            },
            "LWPOLYLINE" => {
                let mut vertices: Vec<Vertex> = vec![];
                for p in group.0.iter().take_while(|p| p.code < 1000) {
                    let value = || p.value.trim().parse::<f64>().ok();
                    match p.code {
                        10 => vertices.push(Vertex { x: value()?, y: 0.0, bulge: 0.0, start_width: 0.0, end_width: 0.0 }),
                        20 => vertices.last_mut()?.y = value()?,
                        42 => vertices.last_mut()?.bulge = value()?,
                        40 => vertices.last_mut()?.start_width = value()?,
                        41 => vertices.last_mut()?.end_width = value()?,
                        _ => {}
                    }
                }
                if vertices.is_empty() {
                    return None;
                }
                // A constant width for the whole polyline.
                if let Some(width) = group.float(43).filter(|w| *w > 0.0) {
                    for vertex in &mut vertices {
                        (vertex.start_width, vertex.end_width) = (width, width);
                    }
                }
                Shape::Polyline { vertices, closed: group.int(70).unwrap_or(0) & 1 == 1 }
            }
            "POLYLINE" => {
                let flags = group.int(70).unwrap_or(0);
                if flags & (8 | 16 | 64) != 0 {
                    return None; // 3D polylines and meshes
                }
                let vertices: Option<Vec<Vertex>> = groups[1..]
                    .iter()
                    .filter(|g| g.kind() == "VERTEX")
                    .map(|v| {
                        Some(Vertex {
                            x: v.float(10)?,
                            y: v.float(20)?,
                            bulge: v.float(42).unwrap_or(0.0),
                            start_width: v.float(40).or(group.float(40)).unwrap_or(0.0),
                            end_width: v.float(41).or(group.float(41)).unwrap_or(0.0),
                        })
                    })
                    .collect();
                let vertices = vertices?;
                if vertices.is_empty() {
                    return None;
                }
                Shape::Polyline { vertices, closed: flags & 1 == 1 }
            }
            "TEXT" => read_text(group, styles)?,
            "MTEXT" => {
                let mut raw: String =
                    group.0.iter().take_while(|p| p.code < 1000).filter(|p| p.code == 3).map(|p| p.value.as_str()).collect();
                raw.push_str(group.get(1).unwrap_or(""));
                let rotation = group.float(50).or_else(|| Some(group.float(21)?.atan2(group.float(11)?).to_degrees())).unwrap_or(0.0);
                let style = group.text(7).unwrap_or_default();
                let height = group.float(40)?;
                // Formatting is kept as the file has it; its text is what shows.
                let parsed = crate::model::mtext::parse(&raw, height);
                let (text, formatted) = if crate::model::mtext::has_formatting(&parsed) {
                    (crate::model::mtext::plain(&parsed), raw.clone())
                } else {
                    (decode_percent_codes(&plain_mtext(&raw)), String::new())
                };
                let (width_factor, oblique) = styles.look_of_style(&style);
                Shape::Text {
                    formatted,
                    width_factor,
                    oblique,
                    position: group.point(10)?,
                    text,
                    height,
                    rotation,
                    font: mtext_font(&raw).unwrap_or_else(|| styles.font_of_style(&style)),
                    style,
                    align: mtext_align(group.int(71).unwrap_or(1)),
                    line_spacing: group.float(44).filter(|f| *f > 0.0).unwrap_or(1.0),
                }
            }
            "DIMENSION" if ![LINEAR_DIMENSION, ALIGNED_DIMENSION].contains(&(group.int(70).unwrap_or(0) & DIMENSION_TYPE_MASK)) => {
                let (kind, points) = match group.int(70).unwrap_or(0) & DIMENSION_TYPE_MASK {
                    RADIUS_DIMENSION => (AnnotationKind::Radius, vec![group.point(10)?, group.point(15)?]),
                    DIAMETER_DIMENSION => {
                        let (far, chord) = (group.point(10)?, group.point(15)?);
                        (AnnotationKind::Diameter, vec![far.midpoint(chord), chord])
                    }
                    THREE_POINT_ANGULAR_DIMENSION => {
                        (AnnotationKind::Angle, vec![group.point(15)?, group.point(13)?, group.point(14)?, group.point(10)?])
                    }
                    TWO_LINE_ANGULAR_DIMENSION => (
                        AnnotationKind::Angle,
                        angle_between_lines((group.point(13)?, group.point(14)?), (group.point(15)?, group.point(10)?), group.point(16)?)?,
                    ),
                    ORDINATE_DIMENSION => (
                        AnnotationKind::Ordinate { x_datum: group.int(70).unwrap_or(0) & ORDINATE_X_TYPE != 0 },
                        vec![group.point(10)?, group.point(13)?, group.point(14)?],
                    ),
                    _ => return None,
                };
                let mut look = styles.look_of(group);
                let centered = annotation_parts(kind, &points, &look).text.map(|t| t.middle);
                look.text_position = group.point(11).filter(|p| centered.is_none_or(|c| p.dist(c) > look.text_height * look.scale * 1e-6));
                Shape::Annotation { kind, points, look }
            }
            "MULTILEADER" | "MLEADER" => read_multileader(group, &self.mleader_style_names)?,
            "ARC_DIMENSION" | "LARGE_RADIAL_DIMENSION" => {
                let (kind, points) = if group.kind() == "ARC_DIMENSION" {
                    let arc = group.subclass("AcDbArcDimension");
                    (AnnotationKind::ArcLength, vec![arc.point(15)?, arc.point(13)?, arc.point(14)?, group.point(10)?])
                } else {
                    let large = group.subclass("AcDbRadialDimensionLarge");
                    (AnnotationKind::JoggedRadius, vec![group.point(10)?, large.point(13)?, large.point(14)?, large.point(15)?])
                };
                let mut look = styles.look_of(group);
                let centered = annotation_parts(kind, &points, &look).text.map(|t| t.middle);
                look.text_position = group.point(11).filter(|p| centered.is_none_or(|c| p.dist(c) > look.text_height * look.scale * 1e-6));
                Shape::Annotation { kind, points, look }
            }
            "ACAD_TABLE" => {
                let after =
                    group.0.iter().skip_while(|p| !(p.code == 100 && p.value.trim() == "AcDbTable")).skip(1).take_while(|p| p.code < 1000);
                let (mut rows, mut columns, mut cells): (Vec<f64>, Vec<f64>, Vec<String>) = (vec![], vec![], vec![]);
                // Each cell's text: its value's display text (302) where it has one, as AutoCAD 2008 and later write
                // it, else the text older files give in codes 2 and 1.
                let (mut spans, mut text, mut shown) = (vec![], String::new(), String::new());
                let mut in_cell = false;
                let finish = |text: &mut String, shown: &mut String| {
                    let older = std::mem::take(text);
                    let newer = std::mem::take(shown);
                    if newer.is_empty() { older } else { newer }
                };
                for p in after {
                    let value = p.value.trim_end_matches(['\r', '\n']);
                    match p.code {
                        141 if !in_cell => rows.push(value.trim().parse().ok()?),
                        142 if !in_cell => columns.push(value.trim().parse().ok()?),
                        171 => {
                            if in_cell {
                                cells.push(finish(&mut text, &mut shown));
                            }
                            in_cell = true;
                        }
                        175 if in_cell => spans.push(value.trim().parse::<usize>().unwrap_or(1)),
                        2 | 1 if in_cell => text.push_str(value),
                        302 if in_cell => shown.push_str(value),
                        _ => {}
                    }
                }
                if in_cell {
                    cells.push(finish(&mut text, &mut shown));
                }
                // Newer files keep cell text elsewhere; without it, the table is shown as AutoCAD drew it instead.
                if rows.is_empty() || columns.is_empty() || cells.len() != rows.len() * columns.len() {
                    return None;
                }
                let grid: Vec<Vec<String>> = cells.chunks(columns.len()).map(|row| row.iter().map(|t| plain_mtext(t)).collect()).collect();
                let title = spans.first().is_some_and(|span| *span == columns.len() && columns.len() > 1);
                let text_height = rows.iter().copied().fold(f64::INFINITY, f64::min) * 0.36;
                Shape::Table { position: group.point(10)?, columns, rows, cells: grid, text_height, title }
            }
            "IMAGE" => {
                let pixels = (group.float(13)?.max(1.0), group.float(23)?.max(1.0));
                let definition = object_index(self.file, group.get(340)?.trim())?;
                Shape::Image {
                    path: self.file.groups[definition].text(1)?.trim().to_string(),
                    position: group.point(10)?,
                    u: group.point(11)? * pixels.0,
                    v: group.point(12)? * pixels.1,
                    pixels: (pixels.0 as u32, pixels.1 as u32),
                }
            }
            "WIPEOUT" => read_wipeout(group)?,
            "MLINE" => {
                let style = group
                    .get(340)
                    .and_then(|handle| object_index(self.file, handle.trim()))
                    .and_then(|at| read_mline_style(&self.file.groups[at]))
                    .unwrap_or_else(|| MlineStyle {
                        name: group.text(2).map_or("Standard".into(), |n| n.trim().to_string()),
                        ..MlineStyle::standard()
                    });
                let mut vertices: Vec<Point> = vec![];
                // Each vertex's elements: code 74 starts one, its 41s are the miter distance, then where its line
                // turns on and off along the segment (starting on at 0).
                let mut params: Vec<Vec<Vec<f64>>> = vec![];
                for p in group.0.iter().take_while(|p| p.code < 1000) {
                    let value = || p.value.trim().parse::<f64>().ok();
                    match p.code {
                        11 => {
                            vertices.push(pt(value()?, 0.0));
                            params.push(vec![]);
                        }
                        21 => vertices.last_mut()?.y = value()?,
                        74 => params.last_mut()?.push(vec![]),
                        41 => params.last_mut()?.last_mut()?.push(value()?),
                        _ => {}
                    }
                }
                if vertices.len() < 2 {
                    return None;
                }
                let gaps = params
                    .iter()
                    .enumerate()
                    .flat_map(|(segment, elements)| {
                        elements.iter().enumerate().flat_map(move |(element, values)| {
                            values.get(2..).unwrap_or_default().as_chunks::<2>().0.iter().map(move |pair| MlineGap {
                                segment,
                                element,
                                from: pair[0],
                                to: pair[1],
                            })
                        })
                    })
                    .collect();
                Shape::Mline {
                    gaps,
                    vertices,
                    closed: group.int(71).unwrap_or(0) & 2 != 0,
                    justification: Justification::from_code(group.int(70).unwrap_or(0)),
                    scale: group.float(40).unwrap_or(1.0),
                    style,
                }
            }
            "XLINE" | "RAY" => {
                let direction = group.point(11)?;
                let length = direction.length();
                if length == 0.0 {
                    return None;
                }
                Shape::XLine { base: group.point(10)?, direction: direction * (1.0 / length), ray: group.kind() == "RAY" }
            }
            "VIEWPORT" => Shape::Viewport {
                center: group.point(10)?,
                width: group.float(40)?,
                height: group.float(41)?,
                view_center: group.point(12)?,
                view_height: group.float(45)?,
                frozen: group
                    .0
                    .iter()
                    .take_while(|p| p.code < 1000)
                    .filter(|p| p.code == 331 || p.code == 341)
                    .filter_map(|p| self.layers_by_handle.get(&p.value.trim().to_uppercase()).cloned())
                    .collect(),
                locked: group.int(90).unwrap_or(0) & VIEWPORT_LOCKED != 0,
            },
            "LEADER" => {
                let points: Vec<Point> = group
                    .0
                    .iter()
                    .take_while(|p| p.code < 1000)
                    .filter(|p| p.code == 10 || p.code == 20)
                    .collect::<Vec<_>>()
                    .chunks(2)
                    .filter_map(|xy| Some(pt(xy[0].value.trim().parse().ok()?, xy.get(1)?.value.trim().parse().ok()?)))
                    .collect();
                if points.len() < 2 {
                    return None;
                }
                let arrow = group.int(71).unwrap_or(1) != 0;
                Shape::Annotation { kind: AnnotationKind::Leader { arrow }, points, look: styles.look_of(group) }
            }
            "DIMENSION" => {
                let kind = group.int(70).unwrap_or(0) & DIMENSION_TYPE_MASK;
                let (start, end) = (group.point(13)?, group.point(14)?);
                let angle = if kind == ALIGNED_DIMENSION { start.angle_to(end) } else { group.float(50).unwrap_or(0.0) };
                let mut look = styles.look_of(group);
                let line_point = group.point(10)?;
                // Text where it would be centered anyway stays centered, so it follows later changes of size.
                let centered = dimension_parts(start, end, line_point, angle, &look).text_middle;
                look.text_position = group.point(11).filter(|p| p.dist(centered) > look.text_height * look.scale * 1e-6);
                Shape::Dimension { start, end, line_point, angle, look }
            }
            "ELLIPSE" => Shape::Ellipse {
                center: group.point(10)?,
                major: group.point(11)?,
                ratio: group.float(40)?,
                start: group.float(41).unwrap_or(0.0),
                end: group.float(42).unwrap_or(std::f64::consts::TAU),
            },
            "SPLINE" => {
                let codes = |code: i32| -> Vec<f64> {
                    group
                        .0
                        .iter()
                        .take_while(|p| p.code < 1000)
                        .filter(|p| p.code == code)
                        .filter_map(|p| p.value.trim().parse().ok())
                        .collect()
                };
                let (xs, ys) = (codes(10), codes(20));
                let (control, knots) = if xs.is_empty() {
                    // Only fit points: they stand in for control points.
                    (codes(11).into_iter().zip(codes(21)).map(|(x, y)| pt(x, y)).collect::<Vec<_>>(), vec![])
                } else {
                    (xs.into_iter().zip(ys).map(|(x, y)| pt(x, y)).collect(), codes(40))
                };
                if control.len() < 2 {
                    return None;
                }
                let degree = group.int(71).unwrap_or(3).max(1) as usize;
                Shape::Spline { degree, control, knots, weights: codes(41), closed: group.int(70).unwrap_or(0) & 1 == 1 }
            }
            "POINT" => Shape::Point { position: group.point(10)? },
            "SOLID" | "TRACE" => {
                // The third and fourth corners are stored the other way round; equal ones make a triangle.
                let corners = [group.point(10)?, group.point(11)?, group.point(13).or(group.point(12))?, group.point(12)?];
                let mut corners = corners.to_vec();
                if corners[2] == corners[3] {
                    corners.pop();
                }
                Shape::Solid { corners }
            }
            "HATCH" => read_hatch(group)?,
            "INSERT" => {
                if group.int(70).unwrap_or(1) > 1 || group.int(71).unwrap_or(1) > 1 {
                    return None; // arrays of inserts (MINSERT)
                }
                let block = self.block(group.get(2)?)?;
                let attributes = groups[1..]
                    .iter()
                    .filter(|g| g.kind() == "ATTRIB")
                    .filter_map(|g| {
                        let (layer, color, _) = style(g, false);
                        Some(Attribute {
                            tag: g.text(2).unwrap_or_default().trim().to_string(),
                            prompt: String::new(),
                            layer,
                            color,
                            text: read_text(g, styles)?,
                            invisible: g.int(70).unwrap_or(0) & 1 != 0,
                        })
                    })
                    .collect();
                Shape::Insert {
                    block,
                    position: group.point(10)?,
                    scale_x: group.float(41).filter(|s| *s != 0.0).unwrap_or(1.0),
                    scale_y: group.float(42).filter(|s| *s != 0.0).unwrap_or(1.0),
                    rotation: group.float(50).unwrap_or(0.0),
                    attributes,
                }
            }
            _ => return None,
        })
    }
}

/// The dimension variables Linewright draws with, by group code, as styles set them and dimensions override them.
const DIMSCALE: i32 = 40;
const DIMASZ: i32 = 41;
const DIMEXO: i32 = 42;
const DIMEXE: i32 = 44;
const DIMTXT: i32 = 140;
const DIMTSZ: i32 = 142;
const DIMLFAC: i32 = 144;
const DIMGAP: i32 = 147;
const DIMDEC: i32 = 271;
const DIMADEC: i32 = 179;
const DIMTAD: i32 = 77;
const DIMZIN: i32 = 78;
const DIMZIN_TRAILING_ZEROS: i64 = 8;
const DIMSAH: i32 = 173;
const DIMBLK_NAME: i32 = 5; // older files name the arrowhead block
const DIMBLK_HANDLES: [i32; 3] = [342, 343, 344]; // newer ones point at its block record
const DIMTOL: i32 = 71;
const DIMLIM: i32 = 72;
const DIMTP: i32 = 47;
const DIMTM: i32 = 48;
const DIMTFAC: i32 = 146;

type ToleranceVariables = ([(i32, i64); 2], [(i32, f64); 3]);

/// The tolerance settings as dimension variables: DIMTOL, DIMLIM, DIMTP, DIMTM and DIMTFAC.
fn tolerance_variables(look: &DimensionLook) -> ToleranceVariables {
    let tol = matches!(look.tolerance, Tolerance::Symmetric | Tolerance::Deviation);
    let lim = look.tolerance == Tolerance::Limits;
    // Symmetric is a deviation whose minus equals its plus.
    let minus = if look.tolerance == Tolerance::Symmetric { look.tolerance_plus } else { look.tolerance_minus };
    ([(DIMTOL, i64::from(tol)), (DIMLIM, i64::from(lim))], [(DIMTP, look.tolerance_plus), (DIMTM, minus), (DIMTFAC, look.tolerance_height)])
}

/// The styles of a file: each dimension style's variables, block names for the arrowhead handles, and the font
/// of each text style.
struct DimensionStyles {
    styles: HashMap<String, BTreeMap<i32, String>>,
    /// Each style's name as the file spells it, in table order.
    names: Vec<String>,
    blocks: HashMap<String, String>,
    text_fonts: HashMap<String, String>,
    text_looks: HashMap<String, (f64, f64)>,
}

impl DimensionStyles {
    fn read(file: &DxfFile) -> DimensionStyles {
        let variables =
            |group: &Group| group.0.iter().skip(1).take_while(|p| p.code < 1000).map(|p| (p.code, p.value.trim().to_string())).collect();
        let names: Vec<String> =
            file.table_entries("DIMSTYLE").into_iter().filter_map(|i| Some(file.groups[i].get(2)?.trim().to_string())).collect();
        let styles = file
            .table_entries("DIMSTYLE")
            .into_iter()
            .filter_map(|i| Some((file.groups[i].get(2)?.trim().to_lowercase(), variables(&file.groups[i]))))
            .collect();
        let blocks = file
            .table_entries("BLOCK_RECORD")
            .into_iter()
            .filter_map(|i| Some((file.groups[i].handle()?.trim().to_uppercase(), file.groups[i].get(2)?.trim().to_string())))
            .collect();
        let text_looks = read_text_styles(file).into_iter().map(|s| (s.name.to_lowercase(), (s.width_factor, s.oblique))).collect();
        DimensionStyles { styles, names, blocks, text_fonts: text_style_fonts(file), text_looks }
    }

    /// A text style's letter width and lean.
    fn look_of_style(&self, style: &str) -> (f64, f64) {
        self.text_looks.get(&style.trim().to_lowercase()).copied().unwrap_or((1.0, 0.0))
    }

    fn font_of_style(&self, style: &str) -> String {
        self.text_fonts.get(&style.trim().to_lowercase()).cloned().unwrap_or_default()
    }

    /// The look a dimension has: AutoCAD's defaults, then its style's variables, then its own overrides.
    fn look_of(&self, dimension: &Group) -> DimensionLook {
        let style = dimension.get(3).map_or(DIMENSION_STYLE.to_lowercase(), |s| s.trim().to_lowercase());
        let mut look = self.style_look(&style, dimension_overrides(dimension));
        look.style = self.display_name(&style);
        // Text typed over the measurement; "<>" in it stands for the measurement.
        if let Some(text) = dimension.get(1).filter(|t| !t.is_empty() && *t != "<>") {
            look.text = plain_mtext(text);
        }
        // Text turned with DIMTEDIT's Angle.
        look.text_rotation = dimension.float(53).filter(|a| *a != 0.0).map(f64::to_degrees);
        look
    }

    /// A style's name as the file spells it; Standard for one the file lacks.
    fn display_name(&self, key: &str) -> String {
        self.names.iter().find(|n| n.to_lowercase() == key).cloned().unwrap_or_else(|| DIMENSION_STYLE.into())
    }

    /// Every style the file defines, in its order.
    fn all(&self) -> Vec<DimensionStyle> {
        self.names
            .iter()
            .map(|name| {
                let mut look = self.style_look(&name.to_lowercase(), vec![]);
                look.style = name.clone();
                DimensionStyle { name: name.clone(), look }
            })
            .collect()
    }

    /// AutoCAD's defaults, then a style's variables, then the overrides given.
    fn style_look(&self, style: &str, overrides: Vec<(i32, String)>) -> DimensionLook {
        let mut variables = self.styles.get(style).cloned().unwrap_or_default();
        variables.extend(overrides);
        let number = |code: i32| variables.get(&code).and_then(|v| v.parse::<f64>().ok()).filter(|v| v.is_finite());
        let positive = |code: i32| number(code).filter(|v| *v > 0.0);
        let mut look = DimensionLook::autocad_defaults();
        look.scale = positive(DIMSCALE).unwrap_or(look.scale); // 0 means "fit to the layout": shown at 1
        look.text_height = positive(DIMTXT).unwrap_or(look.text_height);
        look.arrow_size = positive(DIMASZ).unwrap_or(look.arrow_size);
        look.extension_gap = number(DIMEXO).unwrap_or(look.extension_gap);
        look.extension_overshoot = number(DIMEXE).unwrap_or(look.extension_overshoot);
        look.text_gap = number(DIMGAP).map(f64::abs).unwrap_or(look.text_gap);
        look.decimals = number(DIMDEC).map_or(look.decimals, |d| d.clamp(0.0, 8.0) as u8);
        // -1 means as many as lengths have.
        look.angle_decimals = number(DIMADEC).map_or(look.angle_decimals, |d| if d < 0.0 { look.decimals } else { d.min(8.0) as u8 });
        look.length_factor = number(DIMLFAC).filter(|f| *f != 0.0).map(f64::abs).unwrap_or(look.length_factor);
        look.trim_zeros = number(DIMZIN).is_some_and(|z| z as i64 & DIMZIN_TRAILING_ZEROS != 0);
        look.tolerance_plus = number(DIMTP).unwrap_or(0.0);
        look.tolerance_minus = number(DIMTM).unwrap_or(0.0);
        look.tolerance_height = positive(DIMTFAC).unwrap_or(1.0);
        look.tolerance = if number(DIMLIM).is_some_and(|v| v != 0.0) {
            Tolerance::Limits
        } else if number(DIMTOL).is_some_and(|v| v != 0.0) {
            if look.tolerance_plus == look.tolerance_minus { Tolerance::Symmetric } else { Tolerance::Deviation }
        } else {
            Tolerance::None
        };
        // With DIMSAH the two ends have their own blocks (343, 344), otherwise both use DIMBLK (342).
        let separate = number(DIMSAH).is_some_and(|v| v != 0.0);
        let codes: &[i32] = if separate { &DIMBLK_HANDLES[1..] } else { &DIMBLK_HANDLES[..1] };
        let block = codes
            .iter()
            .filter_map(|code| variables.get(code))
            .filter_map(|handle| self.blocks.get(&handle.to_uppercase()))
            .chain(variables.get(&DIMBLK_NAME))
            .map(|name| name.to_uppercase())
            .find(|name| !name.is_empty());
        if let Some(tick_size) = positive(DIMTSZ) {
            look.arrowhead = Arrowhead::Tick;
            look.arrow_size = tick_size;
        } else if let Some(block) = block {
            if block.contains("ARCHTICK") || block.contains("OBLIQUE") {
                look.arrowhead = Arrowhead::Tick;
            } else if block.contains("DOT") {
                look.arrowhead = Arrowhead::Dot;
            }
        }
        look
    }
}

/// The font of each text style, by lower-case style name: a TrueType family from the style's extended data, or
/// its font file ("times.ttf", "romans.shx").
fn text_style_fonts(file: &DxfFile) -> HashMap<String, String> {
    file.table_entries("STYLE")
        .into_iter()
        .filter_map(|i| {
            let entry = &file.groups[i];
            let name = entry.get(2)?.trim().to_lowercase();
            let xdata = entry.0.iter().skip_while(|p| !(p.code == 1001 && p.value.trim() == "ACAD")).skip(1);
            let family = xdata.take_while(|p| p.code != 1001).find(|p| p.code == 1000).map(|p| p.value.trim().to_string());
            let mut font = family.filter(|f| !f.is_empty()).or_else(|| entry.text(3).map(|f| f.trim().to_string()))?;
            // A big font beside an SHX font goes with it, as AutoCAD shows the pair: "txt.shx,gbcbig.shx".
            if let Some(big) = entry.text(4).map(|b| b.trim().to_string()).filter(|b| !b.is_empty())
                && !font.to_lowercase().ends_with(".ttf")
            {
                font = format!("{font},{big}");
            }
            Some((name, font))
        })
        .collect()
}

/// The file's text styles, without the entries that hold shape files.
fn read_text_styles(file: &DxfFile) -> Vec<TextStyleDef> {
    let fonts = text_style_fonts(file);
    file.table_entries("STYLE")
        .into_iter()
        .filter_map(|i| {
            let entry = &file.groups[i];
            let name = entry.get(2)?.trim().to_string();
            if name.is_empty() || entry.int(70).unwrap_or(0) & 1 != 0 {
                return None;
            }
            Some(TextStyleDef {
                font: fonts.get(&name.to_lowercase()).cloned().unwrap_or_default(),
                height: entry.float(40).unwrap_or(0.0),
                width_factor: entry.float(41).filter(|w| *w > 0.0).unwrap_or(1.0),
                oblique: entry.float(50).unwrap_or(0.0),
                name,
            })
        })
        .collect()
}

/// Make the file's text styles match the document. A style's font is only rewritten when it changed, so a TrueType
/// family's extended data stays as the file has it.
fn write_text_styles(file: &mut DxfFile, writer: &mut Writer, document: &Document) {
    let fonts = text_style_fonts(file);
    for style in &document.text_styles {
        let entries = file.table_entries("STYLE");
        let existing = entries.iter().copied().find(|&i| file.groups[i].get(2).is_some_and(|n| n.trim().eq_ignore_ascii_case(&style.name)));
        let at = match existing {
            Some(at) => at,
            None => {
                let Some((_, end)) = file.table("STYLE") else { continue };
                let mut entry = entries
                    .iter()
                    .copied()
                    .find(|&i| file.groups[i].get(2).is_some_and(|n| n.trim().eq_ignore_ascii_case("Standard")))
                    .map_or_else(
                        || {
                            Group(vec![
                                pair(0, "STYLE"),
                                pair(2, ""),
                                pair(70, 0),
                                pair(71, 0),
                                pair(42, "2.5"),
                                pair(3, "txt"),
                                pair(4, ""),
                            ])
                        },
                        |i| file.groups[i].clone(),
                    );
                entry.0.retain(|p| p.code < 1000);
                entry.set(5, writer.handle());
                entry.set(2, style.name.clone());
                file.groups.insert(end, entry);
                end
            }
        };
        let entry = &mut file.groups[at];
        entry.set(40, number(style.height));
        entry.set(41, number(style.width_factor));
        entry.set(50, number(style.oblique));
        let same_font = fonts.get(&style.name.to_lowercase()).is_some_and(|f| f.eq_ignore_ascii_case(&style.font));
        if !same_font {
            entry.0.retain(|p| p.code < 1000);
            // A family goes in as its file's usual name; AutoCAD finds fonts by file. A big font goes in beside it.
            let (main, big) = style.font.split_once(',').map_or((style.font.as_str(), ""), |(main, big)| (main.trim(), big.trim()));
            let lower = main.to_lowercase();
            let file_name = if main.is_empty() {
                "txt".to_string()
            } else if lower.ends_with(".shx") || lower.ends_with(".ttf") || lower.ends_with(".ttc") || !big.is_empty() {
                main.to_string()
            } else {
                format!("{main}.ttf")
            };
            entry.set(3, file_name);
            entry.set(4, big.to_string());
        }
        writer.text_styles.insert(style.name.to_lowercase(), (style.name.clone(), style.font.clone()));
    }
    file.set_header("$TEXTSTYLE", 7, document.current_text_style.clone());
}

/// The first font an MTEXT's formatting asks for: \fFamily|b0|i0; for TrueType, \Ffile; for others.
fn mtext_font(raw: &str) -> Option<String> {
    let at = raw.find("\\f").or_else(|| raw.find("\\F"))?;
    let rest = &raw[at + 2..];
    let name: String = rest.chars().take_while(|c| *c != '|' && *c != ';').collect();
    let name = name.trim();
    (!name.is_empty()).then(|| name.to_string())
}

/// The dimension variables a dimension sets for itself, kept in its ACAD extended data as code, value pairs.
fn dimension_overrides(group: &Group) -> Vec<(i32, String)> {
    let pairs = &group.0;
    let Some(start) = pairs.iter().position(|p| p.code == 1001 && p.value.trim() == "ACAD") else { return vec![] };
    let data: Vec<&Pair> = pairs[start + 1..].iter().take_while(|p| p.code != 1001).collect();
    let Some(open) = data.iter().position(|p| p.code == 1000 && p.value.trim() == "DSTYLE") else { return vec![] };
    let mut found = vec![];
    let mut iter = data[open + 1..].iter().skip_while(|p| p.code == 1002 && p.value.trim() == "{");
    while let Some(p) = iter.next() {
        if p.code != 1070 {
            break;
        }
        let (Ok(code), Some(value)) = (p.value.trim().parse::<i32>(), iter.next()) else { break };
        found.push((code, value.value.trim().to_string()));
    }
    found
}

/// Entity groups that belong together: a POLYLINE or INSERT with its VERTEX/ATTRIB groups and SEQEND.
fn entity_runs(groups: &[Group]) -> Vec<(usize, usize)> {
    let mut runs = vec![];
    let mut index = 0;
    while index < groups.len() {
        let start = index;
        index += 1;
        let followed = matches!(groups[start].kind(), "POLYLINE" | "INSERT")
            && groups[start].int(66).unwrap_or(if groups[start].kind() == "POLYLINE" { 1 } else { 0 }) == 1;
        if followed {
            while index < groups.len() && matches!(groups[index].kind(), "VERTEX" | "ATTRIB") {
                index += 1;
            }
            if index < groups.len() && groups[index].kind() == "SEQEND" {
                index += 1;
            }
        }
        runs.push((start, index));
    }
    runs
}

fn read_layers(file: &DxfFile) -> Vec<Layer> {
    file.table_entries("LAYER")
        .into_iter()
        .map(|i| {
            let entry = &file.groups[i];
            let name = entry.text(2).unwrap_or_default();
            let color = entry.int(62).unwrap_or(7);
            let flags = entry.int(70).unwrap_or(0);
            Layer {
                name,
                color: match color_of(entry, 7) {
                    true_color if is_true_color(true_color) => true_color,
                    _ => color.abs() as i32,
                },
                linetype: canonical_linetype(entry.get(6).map_or(DEFAULT_LINETYPE, str::trim)),
                lineweight: match entry.int(370).map(lineweight::from_file) {
                    Some(lw) if lw >= 0 => lw,
                    _ => lineweight::DEFAULT,
                },
                visible: color >= 0,
                frozen: flags & LAYER_FROZEN != 0,
                locked: flags & LAYER_LOCKED != 0,
                plot: entry.int(290) != Some(0),
            }
        })
        .collect()
}

/// DXF layer names ignore case, so an object may say "walls" for a layer defined as "WALLS".
pub fn layer_name_as_defined(layers: &[Layer], name: &str) -> String {
    if layers.iter().any(|l| l.name == name) {
        return name.to_string();
    }
    layers.iter().find(|l| l.name.eq_ignore_ascii_case(name)).map_or(name.to_string(), |l| l.name.clone())
}

/// The entity each handle in the file names, by the handle in upper case. Every entity is noted here as it is read,
/// so it hashes with the quick hasher.
type HandleMap = HashMap<String, EntityId, crate::model::entities::IdBuildHasher>;

/// Fewer entities than this are read on the one thread; more are shared among several.
const ENTITIES_PER_THREAD: usize = 5000;

/// The first code of extended data: what programs attach to an object for themselves, after everything else it has.
const FIRST_XDATA_CODE: i32 = 1000;
/// Each program's extended data starts with its name under this code.
const XDATA_APPLICATION_CODE: i32 = 1001;
/// AutoCAD's own name there. What it keeps there goes with how it draws the object (a dimension's overrides, say),
/// and is written afresh with the object rather than kept.
const AUTOCAD_APPLICATION: &str = "ACAD";
/// The application name Linewright's own extended data goes under: a region's loops, written as polylines, carry
/// `REGION` and the region's number, so they come back as one region.
const LINEWRIGHT_APPLICATION: &str = "LINEWRIGHT";
const REGION_MARK: &str = "REGION";

/// The number of the region a polyline is a loop of, from Linewright's extended data on it.
fn region_mark(group: &Group) -> Option<i64> {
    let start =
        group.0.iter().position(|p| p.code == XDATA_APPLICATION_CODE && p.value.trim().eq_ignore_ascii_case(LINEWRIGHT_APPLICATION))?;
    let data: Vec<&Pair> = group.0[start + 1..].iter().take_while(|p| p.code != XDATA_APPLICATION_CODE).collect();
    let marked = data.iter().any(|p| p.code == 1000 && p.value.trim() == REGION_MARK);
    marked.then(|| data.iter().find(|p| p.code == 1071).and_then(|p| p.value.trim().parse().ok())).flatten()
}

/// The loops of regions, read as closed polylines, put back together: one region for each number, in the place of
/// its first loop.
fn merge_regions(entities: Vec<Entity>, marks: &HashMap<EntityId, i64>) -> Vec<Entity> {
    if marks.is_empty() {
        return entities;
    }
    let mut first: HashMap<i64, usize> = HashMap::new();
    let mut merged: Vec<Entity> = Vec::with_capacity(entities.len());
    for entity in entities {
        let (Some(&number), Shape::Polyline { vertices, .. }) = (marks.get(&entity.id), &entity.shape) else {
            merged.push(entity);
            continue;
        };
        match first.get(&number) {
            Some(&at) => {
                if let Shape::Region { loops } = &mut merged[at].shape {
                    loops.push(vertices.clone());
                }
            }
            None => {
                first.insert(number, merged.len());
                let loops = vec![vertices.clone()];
                merged.push(entity.with_shape(Shape::Region { loops }));
            }
        }
    }
    merged
}

/// The named plot style an entity's own part names (code 390), as the handle of the style's entry in the file.
fn plot_style_handle(group: &Group) -> Option<String> {
    let own = group.0.iter().skip_while(|p| !(p.code == 100 && p.value.trim() == "AcDbEntity")).skip(1);
    own.take_while(|p| p.code != 100).find(|p| p.code == 390).map(|p| p.value.trim().to_uppercase())
}

/// Note what an entity being read has that Linewright writes back as it found it: its named plot style, and
/// extended data other programs attached to it.
fn keep_entity_extras(document: &mut Document, id: EntityId, group: &Group) {
    if let Some(handle) = plot_style_handle(group) {
        document.object_plot_styles.insert(id, handle);
    }
    keep_foreign_xdata(document, id, group);
}

/// The named plot styles in the file: each style's entry (an ACDBPLACEHOLDER) by its handle, with its name.
fn read_plot_style_names(file: &DxfFile) -> HashMap<String, String> {
    let Some((start, _)) = file.section("OBJECTS") else { return HashMap::new() };
    let root = &file.groups[start + 1].0;
    let Some(at) = root.iter().position(|p| p.code == 3 && p.value.trim() == "ACAD_PLOTSTYLENAME") else { return HashMap::new() };
    let Some(dictionary) = root.get(at + 1).and_then(|p| object_index(file, p.value.trim())) else { return HashMap::new() };
    let pairs = &file.groups[dictionary].0;
    pairs
        .windows(2)
        .filter(|w| w[0].code == 3 && (w[1].code == 350 || w[1].code == 360))
        .map(|w| (w[1].value.trim().to_uppercase(), w[0].value.trim().to_string()))
        .collect()
}

/// Note the extended data other programs attached to an entity being read, so that saving puts it back.
fn keep_foreign_xdata(document: &mut Document, id: EntityId, group: &Group) {
    let Some(start) = group.0.iter().position(|p| p.code >= FIRST_XDATA_CODE) else { return };
    let (mut kept, mut foreign) = (vec![], false);
    for pair in &group.0[start..] {
        if pair.code == XDATA_APPLICATION_CODE {
            let name = pair.value.trim();
            foreign = !name.eq_ignore_ascii_case(AUTOCAD_APPLICATION) && !name.eq_ignore_ascii_case(LINEWRIGHT_APPLICATION);
        }
        if foreign {
            kept.push(pair.clone());
        }
    }
    if !kept.is_empty() {
        document.kept_xdata.insert(id, kept);
    }
}

pub fn document_from_dxf(mut file: DxfFile) -> Document {
    let mut document = Document::new();
    document.layers = read_layers(&file);
    if document.layers.is_empty() {
        document.layers.push(Layer::new(DEFAULT_LAYER));
    }
    if let Some(precision) = file.header("$LUPREC").and_then(|v| v.trim().parse::<usize>().ok()) {
        document.precision = precision.min(8);
    }
    let header_int = |name: &str| file.header(name).and_then(|v| v.trim().parse::<i64>().ok());
    if let Some(lw) = header_int("$CELWEIGHT") {
        document.current_lineweight = lineweight::from_file(lw);
    }
    if let Some(lw) = header_int("$LWDEFAULT").filter(|lw| *lw >= 0) {
        document.default_lineweight = lineweight::nearest_standard(lw as f64);
    }
    document.show_lineweights = header_int("$LWDISPLAY") == Some(1);
    // The linetype scales are real numbers, and PSLTSCALE is on unless the file turns it off.
    let header_scale = |name: &str| file.header(name).and_then(|v| v.trim().parse::<f64>().ok()).filter(|scale| *scale > 0.0);
    document.ltscale = header_scale("$LTSCALE").unwrap_or(1.0);
    document.psltscale = header_int("$PSLTSCALE").is_none_or(|on| on != 0);
    document.celtscale = header_scale("$CELTSCALE").unwrap_or(1.0);
    document.model_plot = model_plot_settings(&file);
    document.wipeout_frame = wipeout_frame_mode(&file).unwrap_or(1);
    document.named_plot_styles = header_int("$PSTYLEMODE") == Some(0);
    document.plot_style_names = read_plot_style_names(&file);
    document.layer_plot_styles = file
        .table_entries("LAYER")
        .into_iter()
        .filter_map(|i| Some((file.groups[i].get(2)?.trim().to_string(), file.groups[i].get(390)?.trim().to_uppercase())))
        .collect();
    let reader = Reader::new(&file);
    let styles = reader.styles.all();
    if !styles.is_empty() {
        let current = file.header("$DIMSTYLE").map(|s| s.trim().to_lowercase());
        document.current_dimension_style =
            styles.iter().find(|s| Some(s.name.to_lowercase()) == current).unwrap_or(&styles[0]).name.clone();
        document.dimension_styles = styles;
    }
    let text_styles = read_text_styles(&file);
    if !text_styles.is_empty() {
        let current = file.header("$TEXTSTYLE").map(|s| s.trim().to_lowercase());
        document.current_text_style =
            text_styles.iter().find(|s| Some(s.name.to_lowercase()) == current).unwrap_or(&text_styles[0]).name.clone();
        document.text_styles = text_styles;
    }
    let mline_styles = read_mline_styles(&file);
    if !mline_styles.is_empty() {
        document.mline_styles = mline_styles;
    }
    let mleader_styles = read_mleader_styles(&file);
    if !mleader_styles.is_empty() {
        let current = file.header("$CMLEADERSTYLE").map(|s| s.trim().to_lowercase());
        document.current_mleader_style =
            mleader_styles.iter().find(|s| Some(s.name.to_lowercase()) == current).unwrap_or(&mleader_styles[0]).name.clone();
        document.mleader_styles = mleader_styles;
    }
    let mut entities = vec![];
    let mut handles: HandleMap = HandleMap::default();
    let mut hatch_links: Vec<(EntityId, Vec<String>)> = vec![];
    let mut taken: HashSet<usize, crate::model::entities::IdBuildHasher> = HashSet::default();
    let mut stale_blocks: Vec<String> = vec![];
    // Polylines that are loops of a region Linewright wrote, by the region's number.
    let mut region_marks: HashMap<EntityId, i64> = HashMap::new();
    if let Some((start, end)) = file.section("ENTITIES") {
        let body = &file.groups[start + 1..end];
        let runs = entity_runs(body);
        // Reading each entity is most of opening a large drawing, and each is read on its own: shared among threads.
        // What follows from each (layers it names, handles, what is kept) is then noted in the file's order.
        if crate::threads::threads_for(runs.len(), ENTITIES_PER_THREAD) > 1 {
            reader.read_every_block();
        }
        let in_paper_space = |run: &[Group]| run[0].int(67) == Some(1);
        let mut read = crate::threads::map_runs(&runs, ENTITIES_PER_THREAD, |_, some| {
            let entity = |&(first, last): &(usize, usize)| {
                let run = &body[first..last];
                if in_paper_space(run) { None } else { reader.entity(run, false) }
            };
            some.iter().map(entity).collect()
        })
        .into_iter();
        for (first, last) in runs {
            let run = &body[first..last];
            let entity = read.next().flatten();
            if in_paper_space(run) {
                continue; // paper space, read with its layout
            }
            match entity {
                Some(mut entity) => {
                    // Nearly always the layer is defined under this very name, which is quick to see.
                    if !document.layers.iter().any(|l| l.name == entity.layer) {
                        entity.layer = defined_layer(&mut document.layers, &entity.layer);
                    }
                    if let Some(handle) = run[0].handle() {
                        handles.insert(handle.trim().to_uppercase(), entity.id);
                    }
                    keep_entity_extras(&mut document, entity.id, &run[0]);
                    if let Some(number) = region_mark(&run[0]) {
                        region_marks.insert(entity.id, number);
                    }
                    if run[0].kind() == "HATCH" && run[0].int(71).unwrap_or(0) != 0 {
                        // One closed object for each loop, or the hatch follows nothing here.
                        let codes = run[0].0.iter().take_while(|p| p.code < 1000);
                        let linked: Vec<String> = codes.filter(|p| p.code == 330).skip(1).map(|p| p.value.trim().to_uppercase()).collect();
                        if let Shape::Hatch { loops, .. } = &entity.shape
                            && loops.len() == linked.len()
                        {
                            hatch_links.push((entity.id, linked));
                        }
                    }
                    entities.push(entity);
                    if DIMENSION_KINDS.contains(&run[0].kind()) || run[0].kind() == "ACAD_TABLE" {
                        stale_blocks.extend(run[0].text(2).map(|n| n.trim().to_string()));
                    }
                    taken.extend(start + 1 + first..start + 1 + last);
                }
                None => {
                    *document.skipped.entry(run[0].kind().to_string()).or_default() += 1;
                    // A table shows by the block AutoCAD draws it with.
                    if run[0].kind() == "ACAD_TABLE"
                        && let (Some(name), Some(position)) = (run[0].get(2), run[0].point(10))
                        && let Some(block) = reader.block(name)
                    {
                        let (layer, color, linetype) = style(&run[0], false);
                        let shape = Shape::Insert { block, position, scale_x: 1.0, scale_y: 1.0, rotation: 0.0, attributes: vec![] };
                        document.shown_only.push(Entity { layer, color, linetype, ..Entity::new(shape) });
                    }
                }
            }
        }
    }
    // Each layout with what is on it: the active one's in the entities section on paper space, the others' in
    // their paper space blocks. The viewport that is the sheet itself (number 1) stays in the file as it is.
    let layouts = read_layouts(&file);
    if !layouts.is_empty() {
        document.layouts.clear();
    }
    for (layout, record_name) in layouts {
        let range = if record_name.eq_ignore_ascii_case(PAPER_SPACE) {
            file.section("ENTITIES").map(|(start, end)| (start + 1, end))
        } else {
            reader.definitions.get(&record_name.to_uppercase()).map(|&(begin, finish)| (begin + 1, finish))
        };
        let mut on_layout = vec![];
        if let Some((from, to)) = range {
            let body = &file.groups[from..to];
            for (first, last) in entity_runs(body) {
                let run = &body[first..last];
                let paper = run[0].int(67) == Some(1) || !record_name.eq_ignore_ascii_case(PAPER_SPACE);
                if !paper || (run[0].kind() == "VIEWPORT" && run[0].int(69) == Some(1)) {
                    continue;
                }
                match reader.entity(run, false) {
                    Some(mut entity) => {
                        // Nearly always the layer is defined under this very name, which is quick to see.
                        if !document.layers.iter().any(|l| l.name == entity.layer) {
                            entity.layer = defined_layer(&mut document.layers, &entity.layer);
                        }
                        if let Some(handle) = run[0].handle() {
                            handles.insert(handle.trim().to_uppercase(), entity.id);
                        }
                        keep_entity_extras(&mut document, entity.id, &run[0]);
                        if let Some(number) = region_mark(&run[0]) {
                            region_marks.insert(entity.id, number);
                        }
                        on_layout.push(entity);
                        if DIMENSION_KINDS.contains(&run[0].kind()) || run[0].kind() == "ACAD_TABLE" {
                            stale_blocks.extend(run[0].text(2).map(|n| n.trim().to_string()));
                        }
                        taken.extend(from + first..from + last);
                    }
                    None => *document.skipped.entry(run[0].kind().to_string()).or_default() += 1,
                }
            }
        }
        document.layouts.push(layout);
        let space = document.layouts.len() - 1;
        document.set_space_entities(Some(space), merge_regions(on_layout, &region_marks));
    }
    // Every named block the file defines, for INSERT. Anonymous ones (*D1, *U2) belong to the objects using them.
    let names: Vec<String> = reader.definitions.keys().filter(|n| !n.starts_with('*')).cloned().collect();
    document.blocks = names.into_iter().filter_map(|n| reader.block(&n)).map(|b| (b.0.name.to_uppercase(), b)).collect();
    drop(reader);
    // Images read are written afresh with their definitions and reactors, so the old ones go.
    let mut image_objects: HashSet<String> = HashSet::new();
    for &at in &taken {
        let group = &file.groups[at];
        if group.kind() == "IMAGE" {
            image_objects.extend([group.get(340), group.get(360)].into_iter().flatten().map(|h| h.trim().to_uppercase()));
        }
    }
    if let Some((start, end)) = file.section("OBJECTS") {
        taken.extend(
            (start + 1..end).filter(|&i| file.groups[i].handle().is_some_and(|h| image_objects.contains(&h.trim().to_uppercase()))),
        );
    }
    // Groups of what was read are the drawing's now, and are written afresh on saving.
    let groups = read_groups(&file, &handles);
    let read_group_handles: HashSet<String> =
        groups.iter().filter_map(|(at, _)| file.groups[*at].handle().map(|h| h.trim().to_uppercase())).collect();
    taken.extend(groups.iter().map(|(at, _)| *at));
    document.groups = groups.into_iter().map(|(_, group)| group).collect();
    let all = std::mem::take(&mut file.groups);
    let mut read = Vec::with_capacity(taken.len());
    file.groups = Vec::with_capacity(all.len() - taken.len().min(all.len()));
    for (index, group) in all.into_iter().enumerate() {
        if taken.contains(&index) { read.push(group) } else { file.groups.push(group) }
    }
    // Letting go of the groups of a large drawing's entities, a little memory at a time, takes a while: it is
    // done aside, so the drawing opens meanwhile.
    if read.len() > ENTITIES_PER_THREAD {
        std::thread::spawn(move || drop(read));
    }
    for name in stale_blocks {
        remove_block(&mut file, &name);
    }
    if let Some(dictionary) = image_dictionary(&file) {
        let pairs = &mut file.groups[dictionary].0;
        let mut kept = vec![];
        let mut i = 0;
        while i < pairs.len() {
            if pairs[i].code == 3 && pairs.get(i + 1).is_some_and(|p| image_objects.contains(&p.value.trim().to_uppercase())) {
                i += 2;
            } else {
                kept.push(pairs[i].clone());
                i += 1;
            }
        }
        *pairs = kept;
    }
    if let Some(dictionary) = group_dictionary(&file) {
        let pairs = &mut file.groups[dictionary].0;
        let mut kept = vec![];
        let mut i = 0;
        while i < pairs.len() {
            let entry = pairs[i].code == 3 && pairs.get(i + 1).is_some_and(|p| read_group_handles.contains(&p.value.trim().to_uppercase()));
            if entry {
                i += 2;
            } else {
                kept.push(pairs[i].clone());
                i += 1;
            }
        }
        *pairs = kept;
    }
    // Hatches that follow objects, now every object has its id.
    for entity in &mut entities {
        let Some(found) = hatch_links.iter().find(|(id, _)| *id == entity.id) else { continue };
        let ids: Option<Vec<EntityId>> = found.1.iter().map(|h| handles.get(h).copied()).collect();
        if let (Some(ids), Shape::Hatch { association, .. }) = (ids, &mut entity.shape) {
            *association = Some(Association::Objects(ids));
        }
    }
    document.set_entities(merge_regions(entities, &region_marks));
    document.source = Some(file);
    document
}

const PAPER_SPACE: &str = "*Paper_Space";
/// VIEWPORT status flags (code 90): the ones AutoCAD sets on a plain viewport, and display locked.
const VIEWPORT_STATUS: i64 = 32864;
const VIEWPORT_LOCKED: i64 = 16384;
const MODEL_SPACE: &str = "*Model_Space";

/// The file's paper layouts in tab order, each with the name of the block record holding what is on it. The
/// sheet is the layout's limits, or its paper when it has none.
/// PLOTSETTINGS flags: plot with plot styles, and print objects' lineweights.
const PLOT_PLOT_STYLES: i64 = 32;
const PRINT_LINEWEIGHTS: i64 = 128;

/// A page setup's plot style table and whether lineweights print.
fn plot_settings(plot: &Group) -> PlotSettings {
    let flags = plot.int(70).unwrap_or(PRINT_LINEWEIGHTS);
    let table = plot.text(7).unwrap_or_default().trim().to_string();
    PlotSettings {
        style_table: if flags & PLOT_PLOT_STYLES != 0 { table } else { String::new() },
        lineweights: flags & PRINT_LINEWEIGHTS != 0 || flags & PLOT_PLOT_STYLES != 0,
    }
}

/// The LAYOUT object of model space, by its index in the file.
fn model_layout_object(file: &DxfFile) -> Option<usize> {
    let model = record_handle(file, MODEL_SPACE)?;
    let (start, end) = file.section("OBJECTS")?;
    (start + 1..end).find(|&i| {
        let group = &file.groups[i];
        group.kind() == "LAYOUT" && group.subclass("AcDbLayout").get(330).is_some_and(|r| r.trim().eq_ignore_ascii_case(&model))
    })
}

fn model_plot_settings(file: &DxfFile) -> PlotSettings {
    model_layout_object(file).map_or_else(PlotSettings::default, |i| plot_settings(&file.groups[i].subclass("AcDbPlotSettings")))
}

/// Write a page setup's plot style table and lineweight choice into a LAYOUT object.
fn set_plot_settings(group: &mut Group, plot: &PlotSettings) {
    let mut flags = group.subclass("AcDbPlotSettings").int(70).unwrap_or(0);
    for (flag, on) in [(PLOT_PLOT_STYLES, !plot.style_table.is_empty()), (PRINT_LINEWEIGHTS, plot.lineweights)] {
        flags = if on { flags | flag } else { flags & !flag };
    }
    group.set_in("AcDbPlotSettings", 70, flags.to_string());
    group.set_in("AcDbPlotSettings", 7, plot.style_table.clone());
}

fn read_layouts(file: &DxfFile) -> Vec<(Layout, String)> {
    let records: HashMap<String, String> = file
        .table_entries("BLOCK_RECORD")
        .into_iter()
        .filter_map(|i| Some((file.groups[i].handle()?.trim().to_uppercase(), file.groups[i].get(2)?.trim().to_string())))
        .collect();
    let Some((start, end)) = file.section("OBJECTS") else { return vec![] };
    let mut layouts: Vec<(i64, Layout, String)> = (start + 1..end)
        .filter(|&i| file.groups[i].kind() == "LAYOUT")
        .filter_map(|i| {
            let group = &file.groups[i];
            let layout = group.subclass("AcDbLayout");
            let plot = group.subclass("AcDbPlotSettings");
            let record_handle = layout.get(330)?.trim().to_uppercase();
            let record_name = records.get(&record_handle)?.clone();
            if !record_name.to_uppercase().starts_with("*PAPER_SPACE") {
                return None;
            }
            let (low, high) = (layout.point(10).unwrap_or_default(), layout.point(11).unwrap_or_default());
            let (paper_origin, paper_size) = if high.x > low.x && high.y > low.y {
                (low, (high.x - low.x, high.y - low.y))
            } else {
                // Paper sizes are in millimeters; a quarter turn swaps them.
                let inches = plot.int(72) == Some(0);
                let to_units = if inches { 1.0 / 25.4 } else { 1.0 };
                let (w, h) = (plot.float(44).unwrap_or(420.0) * to_units, plot.float(45).unwrap_or(297.0) * to_units);
                let turned = matches!(plot.int(73), Some(1 | 3));
                let size = if turned { (h, w) } else { (w, h) };
                let margin = pt(plot.float(40).unwrap_or(0.0), plot.float(41).unwrap_or(0.0)) * to_units;
                (pt(0.0, 0.0) - margin, size)
            };
            let mut item = Layout::new(layout.get(1).unwrap_or("Layout").trim());
            (item.paper_origin, item.paper_size) = (paper_origin, paper_size);
            item.record = Some(record_handle);
            item.object = group.handle().map(|h| h.trim().to_uppercase());
            item.plot = plot_settings(&plot);
            Some((layout.int(71).unwrap_or(0), item, record_name))
        })
        .collect();
    layouts.sort_by_key(|(order, _, _)| *order);
    layouts.into_iter().map(|(_, layout, record)| (layout, record)).collect()
}

/// The layer name as the table spells it. A layer used but never defined is added.
fn defined_layer(layers: &mut Vec<Layer>, name: &str) -> String {
    let defined = layer_name_as_defined(layers, name);
    if !layers.iter().any(|l| l.name == defined) {
        layers.push(Layer::new(&defined));
    }
    defined
}

/// Remove a block and its block record. A dimension leaves its drawn block behind when it is replaced.
fn remove_block(file: &mut DxfFile, name: &str) {
    if name.is_empty() {
        return;
    }
    if let Some((start, end)) = file.section("BLOCKS")
        && let Some(begin) = (start + 1..end)
            .find(|&i| file.groups[i].kind() == "BLOCK" && file.groups[i].get(2).is_some_and(|n| n.trim().eq_ignore_ascii_case(name)))
    {
        let finish = (begin..end).find(|&i| file.groups[i].kind() == "ENDBLK").map_or(end, |i| i + 1);
        file.groups.drain(begin..finish);
    }
    if let Some(record) =
        file.table_entries("BLOCK_RECORD").into_iter().find(|&i| file.groups[i].get(2).is_some_and(|n| n.trim().eq_ignore_ascii_case(name)))
    {
        file.groups.remove(record);
    }
}

// ---------------------------------------------------------------------------------------------------- writing

/// How MTEXT is written: its font when it has its own, its style, alignment and line spacing.
struct MText<'a> {
    font: &'a str,
    style: Option<String>,
    align: TextAlign,
    line_spacing: f64,
    /// Formatting codes to write as they are.
    raw: Option<&'a str>,
}

/// A block written into the file: its name, the handle of its block record, and its groups from BLOCK to ENDBLK.
struct DimensionBlock {
    name: String,
    record: String,
    groups: Vec<Group>,
}

/// Handles a MULTILEADER refers to: styles by lower-case name, the text style and the ByBlock linetype.
#[derive(Clone)]
struct MleaderIds {
    styles: HashMap<String, String>,
    text_style: String,
    linetype: String,
}

/// How MULTILEADER writes a color that goes by block.
const BY_BLOCK_COLOR_VALUE: i64 = -1056964608;

/// The file's multileader styles, in its dictionary's order.
fn read_mleader_styles(file: &DxfFile) -> Vec<MleaderStyle> {
    let Some((start, end)) = file.section("OBJECTS") else { return vec![] };
    let objects = &file.groups[start + 1..end];
    let Some(dictionary) = mleader_dictionary(file) else { return vec![] };
    let pairs = &file.groups[dictionary].0;
    let mut styles = vec![];
    for at in 0..pairs.len().saturating_sub(1) {
        if pairs[at].code != 3 || !matches!(pairs[at + 1].code, 350 | 360) {
            continue;
        }
        let handle = pairs[at + 1].value.trim();
        let Some(group) =
            objects.iter().find(|g| g.kind() == "MLEADERSTYLE" && g.handle().is_some_and(|h| h.trim().eq_ignore_ascii_case(handle)))
        else {
            continue;
        };
        let standard = MleaderStyle::standard();
        styles.push(MleaderStyle {
            name: pairs[at].value.trim().to_string(),
            text_height: group.float(45).unwrap_or(standard.text_height),
            arrow_size: group.float(44).unwrap_or(standard.arrow_size),
            dogleg: group.float(43).unwrap_or(standard.dogleg),
            landing_gap: group.float(42).unwrap_or(standard.landing_gap),
        });
    }
    styles
}

/// The ACAD_MLEADERSTYLE dictionary's place among the file's groups.
fn mleader_dictionary(file: &DxfFile) -> Option<usize> {
    let (start, end) = file.section("OBJECTS")?;
    let root = &file.groups[start + 1];
    let pairs = &root.0;
    let at = pairs.iter().position(|p| p.code == 3 && p.value.trim() == "ACAD_MLEADERSTYLE")?;
    let handle = pairs.get(at + 1)?.value.trim().to_string();
    (start + 1..end)
        .find(|&i| file.groups[i].kind() == "DICTIONARY" && file.groups[i].handle().is_some_and(|h| h.trim().eq_ignore_ascii_case(&handle)))
}

/// Make the file's multileader styles match the document, and note what multileaders will point at.
fn write_mleader_styles(file: &mut DxfFile, writer: &mut Writer, document: &Document) {
    if !writer.r2007 {
        return;
    }
    let Some(dictionary) = mleader_dictionary(file) else { return };
    let Some((start, end)) = file.section("OBJECTS") else { return };
    let Some(template) = (start + 1..end).find(|&i| file.groups[i].kind() == "MLEADERSTYLE").map(|i| file.groups[i].clone()) else {
        return;
    };
    let mut styles = HashMap::new();
    for style in &document.mleader_styles {
        let pairs = &file.groups[dictionary].0;
        let named = pairs.iter().position(|p| p.code == 3 && p.value.trim().eq_ignore_ascii_case(&style.name));
        let handle = match named.and_then(|at| pairs.get(at + 1)).map(|p| p.value.trim().to_string()) {
            Some(handle) => handle,
            None => {
                let handle = writer.handle();
                let mut group = template.clone();
                group.set(5, handle.clone());
                group.set(3, style.name.clone());
                let Some((_, end)) = file.section("OBJECTS") else { return };
                file.groups.insert(end, group);
                let pairs = &mut file.groups[dictionary].0;
                let tail = pairs.iter().position(|p| p.code >= 1000).unwrap_or(pairs.len());
                pairs.splice(tail..tail, [pair(3, &style.name), pair(350, &handle)]);
                handle
            }
        };
        if let Some(at) = object_index(file, &handle) {
            let group = &mut file.groups[at];
            for (code, value) in [(45, style.text_height), (44, style.arrow_size), (43, style.dogleg), (42, style.landing_gap)] {
                group.set(code, number(value));
            }
        }
        styles.insert(style.name.to_lowercase(), handle);
    }
    let entry = |file: &DxfFile, table: &str, name: &str| {
        file.table_entries(table)
            .into_iter()
            .find(|&i| file.groups[i].get(2).is_some_and(|n| n.trim().eq_ignore_ascii_case(name)))
            .and_then(|i| file.groups[i].handle().map(|h| h.trim().to_string()))
    };
    if let (Some(text_style), Some(linetype)) = (entry(file, "STYLE", "Standard"), entry(file, "LTYPE", "ByBlock")) {
        writer.mleader_ids = Some(MleaderIds { styles, text_style, linetype });
    }
    file.set_header("$CMLEADERSTYLE", 2, document.current_mleader_style.clone());
}

/// A MULTILEADER with text. One with a block for its content stays in the file as it is.
fn read_multileader(group: &Group, styles: &HashMap<String, String>) -> Option<Shape> {
    let body = group.subclass("AcDbMLeader");
    let mut in_context = false;
    let (mut in_leader, mut in_line) = (false, false);
    let (mut text, mut text_x, mut text_y, mut height, mut arrow) = (String::new(), None, None, 2.5, 2.5);
    let (mut landing, mut landing_set, mut dogleg, mut side) = (pt(0.0, 0.0), false, 0.0, 1.0);
    let mut leaders: Vec<Vec<Point>> = vec![];
    let mut line: Vec<Point> = vec![];
    let mut pending_x = 0.0;
    let mut content = None;
    let mut style = String::new();
    for p in body.0.iter().take_while(|p| p.code < 1000) {
        let value = p.value.trim();
        let float = value.parse::<f64>().unwrap_or(0.0);
        match (p.code, in_context, in_leader, in_line) {
            (300, ..) if value == "CONTEXT_DATA{" => in_context = true,
            (301, ..) => in_context = false,
            (302, ..) => in_leader = true,
            (303, ..) => in_leader = false,
            (304, _, true, _) if value == "LEADER_LINE{" => {
                in_line = true;
                line = vec![];
            }
            (305, ..) => {
                in_line = false;
                leaders.push(std::mem::take(&mut line));
            }
            (10, _, _, true) => pending_x = float,
            (20, _, _, true) => line.push(pt(pending_x, float)),
            (10, _, true, false) if !landing_set => landing.x = float,
            (20, _, true, false) if !landing_set => {
                landing.y = float;
                landing_set = true;
            }
            (11, _, true, false) => side = if float < 0.0 { -1.0 } else { 1.0 },
            (40, _, true, false) => dogleg = float,
            (41, true, false, false) => height = float,
            (140, true, false, false) => arrow = float,
            (304, true, false, false) => text = p.value.clone(),
            (12, true, false, false) => text_x = Some(float),
            (22, true, false, false) => text_y = Some(float),
            (172, false, false, false) => content = value.parse::<i64>().ok(),
            (340, false, false, false) => style = styles.get(&value.to_uppercase()).cloned().unwrap_or_default(),
            _ => {}
        }
    }
    if content == Some(1) || !landing_set || leaders.is_empty() {
        return None; // block content, or nothing to draw
    }
    let dogleg = dogleg * side;
    let shown = decode_percent_codes(&plain_mtext(&text));
    let text_position = match (text_x, text_y) {
        (Some(x), Some(y)) => pt(x, y),
        _ => crate::model::multileader::text_corner(landing, dogleg, height, 0.0),
    };
    Some(Shape::Multileader { leaders, landing, dogleg, text: shown, text_position, text_height: height, arrow_size: arrow, style })
}

struct Writer {
    modern: bool,
    /// R2018 or later, where ARC_DIMENSION has a type of its own.
    r2018: bool,
    /// R2004 or later, which has arc length and jogged radius dimensions.
    r2004: bool,
    /// R2007 or later, which has multileaders.
    r2007: bool,
    /// What multileaders point at in this file, once it has their styles.
    mleader_ids: Option<MleaderIds>,
    next_handle: u64,
    model_space: String,
    block_names: HashSet<String>, // upper case, so new dimension blocks get names nothing else has
    blocks: Vec<DimensionBlock>,
    /// The file's text styles by lower-case name: their name as written and their font.
    text_styles: HashMap<String, (String, String)>,
    /// Blocks inserts use that the file does not define yet, such as ones pasted from another drawing.
    blocks_to_define: Vec<BlockRef>,
    /// Layer handles by lower-case name, for the layers a viewport freezes.
    layer_handles: HashMap<String, String>,
    /// The number the next viewport gets; 1 is each layout's sheet.
    next_viewport: i64,
    /// The handle each entity was written with, for the groups that list it.
    entity_handles: HashMap<EntityId, String>,
    /// Image files written: path, IMAGEDEF handle and pixel size; and each image's reactor, image and definition.
    image_defs: Vec<(String, String, (u32, u32))>,
    image_reactors: Vec<(String, String, String)>,
    /// Whether any wipeout was written, which AutoCAD needs its class declared for.
    wipeouts: bool,
    /// How many regions were written, each numbered in the extended data of its loops.
    regions: i64,
    /// The file's multiline styles' handles by lower-case name.
    mline_styles: HashMap<String, String>,
    /// The table style tables are written with: its handle, and the text height to make it with when the file
    /// has none yet.
    table_style: Option<(String, Option<f64>)>,
    /// The lineweight of the entity being written, which every group it is written as carries.
    lineweight: i16,
    /// The linetype scale of the entity being written, which its groups carry when it is not 1.
    linetype_scale: f64,
}

impl Writer {
    fn handle(&mut self) -> String {
        let handle = format!("{:X}", self.next_handle);
        self.next_handle += 1;
        handle
    }

    /// A name for a table's block that nothing else has: *T1, *T2…
    fn table_block_name(&mut self) -> String {
        let number = (1..).find(|n| !self.block_names.contains(&format!("*T{n}"))).unwrap_or(1);
        let name = format!("*T{number}");
        self.block_names.insert(name.clone());
        name
    }

    fn block_name(&mut self) -> String {
        let mut number = self.block_names.len() + 1;
        while self.block_names.contains(&format!("*D{number}")) {
            number += 1;
        }
        let name = format!("*D{number}");
        self.block_names.insert(name.clone());
        name
    }

    /// The start of an entity: its type, handle, owner, layer and look, then its subclass.
    fn head(&mut self, kind: &str, owner: &str, layer: &str, color: i32, linetype: &str, subclass: &[&str]) -> Vec<Pair> {
        let mut pairs = vec![pair(0, kind), pair(5, self.handle())];
        if self.modern {
            pairs.push(pair(330, owner));
            pairs.push(pair(100, "AcDbEntity"));
        }
        pairs.push(pair(8, layer));
        if linetype != BYLAYER_LINETYPE {
            pairs.push(pair(6, linetype));
        }
        if color != BYLAYER {
            pairs.push(pair(62, index_of(color)));
            if self.modern
                && let Some(value) = true_color_value(color)
            {
                pairs.push(pair(420, value));
            }
        }
        if self.modern && self.lineweight != lineweight::BYLAYER {
            pairs.push(pair(370, self.lineweight));
        }
        if self.modern && self.linetype_scale != 1.0 {
            pairs.push(pair(48, number(self.linetype_scale)));
        }
        if self.modern {
            pairs.extend(subclass.iter().map(|s| pair(100, s)));
        }
        pairs
    }

    /// A viewport as AutoCAD writes one: looking straight down at model space, on, with no clipping of its own.
    /// R12 files get its frame.
    #[allow(clippy::too_many_arguments)]
    fn viewport(
        &mut self,
        owner: &str,
        layer: &str,
        color: i32,
        center: Point,
        (width, height): (f64, f64),
        view_center: Point,
        view_height: f64,
        frozen: &[String],
        locked: bool,
    ) -> Vec<Group> {
        if !self.modern {
            let corners = crate::model::viewport::frame_corners(center, width, height);
            return (0..4).map(|i| self.line(owner, layer, color, BYLAYER_LINETYPE, corners[i], corners[(i + 1) % 4])).collect();
        }
        let id = self.next_viewport;
        self.next_viewport += 1;
        let mut pairs = self.head("VIEWPORT", owner, layer, color, BYLAYER_LINETYPE, &["AcDbViewport"]);
        pairs.extend(point_pairs(10, center));
        pairs.extend([pair(40, number(width)), pair(41, number(height)), pair(68, id), pair(69, id)]);
        pairs.extend([pair(12, number(view_center.x)), pair(22, number(view_center.y))]);
        pairs.extend([pair(13, "0.0"), pair(23, "0.0"), pair(14, "10.0"), pair(24, "10.0"), pair(15, "10.0"), pair(25, "10.0")]);
        pairs.extend([pair(16, "0.0"), pair(26, "0.0"), pair(36, "1.0")]);
        pairs.extend([pair(17, "0.0"), pair(27, "0.0"), pair(37, "0.0")]);
        pairs.extend([pair(42, "50.0"), pair(43, "0.0"), pair(44, "0.0"), pair(45, number(view_height))]);
        pairs.extend([pair(50, "0.0"), pair(51, "0.0"), pair(72, 1000)]);
        pairs.extend(frozen.iter().filter_map(|name| self.layer_handles.get(&name.to_lowercase())).map(|h| pair(331, h)));
        let status = if locked { VIEWPORT_STATUS | VIEWPORT_LOCKED } else { VIEWPORT_STATUS };
        pairs.extend([pair(90, status), pair(281, 0), pair(71, 1), pair(74, 0)]);
        pairs.extend(point_pairs(110, pt(0.0, 0.0)));
        pairs.extend([pair(111, "1.0"), pair(121, "0.0"), pair(131, "0.0"), pair(112, "0.0"), pair(122, "1.0"), pair(132, "0.0")]);
        pairs.extend([pair(79, 0), pair(146, "0.0"), pair(170, 0), pair(61, 5)]);
        vec![Group(pairs)]
    }

    fn line(&mut self, owner: &str, layer: &str, color: i32, linetype: &str, start: Point, end: Point) -> Group {
        let mut pairs = self.head("LINE", owner, layer, color, linetype, &["AcDbLine"]);
        pairs.extend(point_pairs(10, start));
        pairs.extend(point_pairs(11, end));
        Group(pairs)
    }

    fn entity(&mut self, entity: &Entity, linetype: &str, dimension_style: &str, owner: &str) -> Vec<Group> {
        let outer = std::mem::replace(&mut self.lineweight, entity.lineweight);
        let outer_scale = std::mem::replace(&mut self.linetype_scale, entity.linetype_scale);
        let groups = self.entity_groups(entity, linetype, dimension_style, owner);
        self.lineweight = outer;
        self.linetype_scale = outer_scale;
        groups
    }

    fn entity_groups(&mut self, entity: &Entity, linetype: &str, dimension_style: &str, owner: &str) -> Vec<Group> {
        let (layer, color) = (entity.layer.as_str(), entity.color);
        if let (Some(info), Shape::Text { position, text, height, rotation, align, .. }) = (&entity.attribute, &entity.shape) {
            let value = encode_percent_codes(&info.default);
            let mut pairs = self.text("ATTDEF", owner, layer, color, linetype, *position, &value, *height, *rotation, *align);
            if self.modern {
                pairs.push(pair(100, "AcDbAttributeDefinition"));
            }
            let asked = if info.prompt.is_empty() { text.as_str() } else { info.prompt.as_str() };
            pairs.extend([pair(3, asked), pair(2, text.trim()), pair(70, i32::from(info.invisible))]);
            if let Some(at) = pairs.iter().position(|p| p.code == 74) {
                let vertical = pairs.remove(at);
                pairs.push(vertical);
            }
            return vec![Group(pairs)];
        }
        match &entity.shape {
            Shape::Line { start, end } => vec![self.line(owner, layer, color, linetype, *start, *end)],
            // AutoCAD keeps a region as ACIS solid-modelling data. Here each loop is a closed polyline, which any
            // program shows, marked as a loop of the region so that Linewright reads them back as one.
            Shape::Region { loops } => {
                self.regions += 1;
                let number = self.regions;
                let mut groups = vec![];
                for shape in crate::model::region::loop_shapes(loops) {
                    let mut written = self.entity_groups(&entity.derive(shape), linetype, dimension_style, owner);
                    if let Some(first) = written.first_mut() {
                        first.0.extend([pair(XDATA_APPLICATION_CODE, LINEWRIGHT_APPLICATION), pair(1000, REGION_MARK), pair(1071, number)]);
                    }
                    groups.extend(written);
                }
                groups
            }
            Shape::Circle { center, radius } => {
                let mut pairs = self.head("CIRCLE", owner, layer, color, linetype, &["AcDbCircle"]);
                pairs.extend(point_pairs(10, *center));
                pairs.push(pair(40, number(*radius)));
                vec![Group(pairs)]
            }
            Shape::Arc { center, radius, start_angle, end_angle } => {
                let mut pairs = self.head("ARC", owner, layer, color, linetype, &["AcDbCircle"]);
                pairs.extend(point_pairs(10, *center));
                pairs.push(pair(40, number(*radius)));
                if self.modern {
                    pairs.push(pair(100, "AcDbArc"));
                }
                pairs.push(pair(50, number(*start_angle)));
                pairs.push(pair(51, number(*end_angle)));
                vec![Group(pairs)]
            }
            Shape::Polyline { vertices, closed } if self.modern => {
                let mut pairs = self.head("LWPOLYLINE", owner, layer, color, linetype, &["AcDbPolyline"]);
                pairs.push(pair(90, vertices.len()));
                pairs.push(pair(70, if *closed { 1 } else { 0 }));
                for v in vertices {
                    pairs.push(pair(10, number(v.x)));
                    pairs.push(pair(20, number(v.y)));
                    if v.start_width != 0.0 || v.end_width != 0.0 {
                        pairs.extend([pair(40, number(v.start_width)), pair(41, number(v.end_width))]);
                    }
                    if v.bulge != 0.0 {
                        pairs.push(pair(42, number(v.bulge)));
                    }
                }
                vec![Group(pairs)]
            }
            Shape::Polyline { vertices, closed } => {
                // R12 has no light-weight polyline: a POLYLINE, its VERTEX groups and a SEQEND.
                let mut head = self.head("POLYLINE", owner, layer, color, linetype, &[]);
                head.push(pair(66, 1));
                head.extend(point_pairs(10, pt(0.0, 0.0)));
                head.push(pair(70, if *closed { 1 } else { 0 }));
                let mut groups = vec![Group(head)];
                for v in vertices {
                    let mut pairs = self.head("VERTEX", owner, layer, color, linetype, &[]);
                    pairs.extend(point_pairs(10, v.point()));
                    if v.start_width != 0.0 || v.end_width != 0.0 {
                        pairs.extend([pair(40, number(v.start_width)), pair(41, number(v.end_width))]);
                    }
                    if v.bulge != 0.0 {
                        pairs.push(pair(42, number(v.bulge)));
                    }
                    groups.push(Group(pairs));
                }
                groups.push(Group(self.head("SEQEND", owner, layer, color, linetype, &[])));
                groups
            }
            Shape::Text { position, text, height, rotation, font, style, align, line_spacing, formatted, width_factor, oblique } => {
                let rich = crate::model::mtext::valid_rich(text, formatted, *height).is_some();
                let style = self.text_styles.get(&style.trim().to_lowercase()).cloned();
                let style_font = style.as_ref().map_or("", |(_, font)| font.as_str());
                // TEXT holds one line in its style's font; anything else keeps its lines and font as MTEXT.
                if rich || text.contains('\n') || (!font.is_empty() && !font.eq_ignore_ascii_case(style_font)) {
                    let raw = rich.then_some(formatted.as_str());
                    let look = MText { font, style: style.map(|(name, _)| name), align: *align, line_spacing: *line_spacing, raw };
                    vec![self.mtext(owner, layer, color, linetype, *position, text, *height, *rotation, look)]
                } else {
                    let value = encode_percent_codes(text);
                    let mut pairs = self.text("TEXT", owner, layer, color, linetype, *position, &value, *height, *rotation, *align);
                    if let Some((name, _)) = style {
                        let at = pairs.iter().position(|p| p.code == 1).map_or(pairs.len(), |i| i + 1);
                        pairs.insert(at, pair(7, name));
                    }
                    if *width_factor != 1.0 || *oblique != 0.0 {
                        let at = pairs.iter().position(|p| p.code == 1).map_or(pairs.len(), |i| i + 1);
                        pairs.splice(at..at, [pair(41, number(*width_factor)), pair(51, number(*oblique))]);
                    }
                    vec![Group(pairs)]
                }
            }
            Shape::Dimension { start, end, line_point, angle, look } => {
                let block_name = self.block_name();
                let parts = dimension_parts(*start, *end, *line_point, *angle, look);
                let paths: Vec<Vec<Point>> = parts.lines().map(|(a, b)| vec![*a, *b]).collect();
                let text = AnnotationText {
                    text: parts.text.clone(),
                    height: parts.text_height,
                    anchor: parts.text_anchor,
                    middle: parts.text_middle,
                    angle: parts.text_angle,
                };
                let texts: Vec<AnnotationText> = std::iter::once(text).chain(parts.stack.iter().cloned()).collect();
                let block = self.dimension_block(&block_name, &paths, &parts.arrows, &texts);
                self.blocks.push(block);
                let mut pairs = self.head("DIMENSION", owner, layer, color, linetype, &["AcDbDimension"]);
                if self.modern {
                    pairs.push(pair(280, 0));
                }
                pairs.push(pair(2, &block_name));
                pairs.extend(point_pairs(10, parts.dimension_line.1));
                pairs.extend(point_pairs(11, parts.text_middle));
                pairs.push(pair(70, LINEAR_DIMENSION | BLOCK_REFERENCED_BY_THIS_DIMENSION_ONLY));
                if self.modern {
                    pairs.push(pair(71, 5));
                }
                pairs.push(pair(42, number(crate::model::dimension::measurement(*start, *end, *angle))));
                if !look.text.is_empty() {
                    pairs.push(pair(1, &look.text));
                }
                if let Some(rotation) = look.text_rotation {
                    pairs.push(pair(53, number(rotation.to_radians())));
                }
                pairs.push(pair(3, dimension_style));
                if self.modern {
                    pairs.push(pair(100, "AcDbAlignedDimension"));
                }
                pairs.extend(point_pairs(13, *start));
                pairs.extend(point_pairs(14, *end));
                pairs.push(pair(50, number(*angle)));
                if self.modern {
                    pairs.push(pair(100, "AcDbRotatedDimension"));
                }
                pairs.extend(dimension_style_overrides(look));
                vec![Group(pairs)]
            }
            Shape::Table { .. } if !self.modern => {
                // R12 has no tables: their lines and text go in instead.
                let pieces = explode(entity).unwrap_or_default();
                pieces.iter().flat_map(|piece| self.entity(piece, linetype, dimension_style, owner)).collect()
            }
            Shape::Table { position, columns, rows, cells, text_height, title } => {
                let name = self.table_block_name();
                let parts = entity_parts(entity, f64::INFINITY);
                let block = self.anonymous_block(&name, &parts.paths, &[], &parts.texts);
                let record = block.record.clone();
                self.blocks.push(block);
                let style = match &self.table_style {
                    Some((handle, _)) => handle.clone(),
                    None => {
                        let handle = self.handle();
                        self.table_style = Some((handle.clone(), Some(*text_height)));
                        handle
                    }
                };
                let mut pairs = self.head("ACAD_TABLE", owner, layer, color, linetype, &["AcDbBlockReference"]);
                pairs.push(pair(2, &name));
                pairs.extend(point_pairs(10, *position));
                pairs.extend([pair(100, "AcDbTable"), pair(280, 0), pair(342, &style), pair(343, &record)]);
                pairs.extend(point_pairs(11, pt(1.0, 0.0)));
                pairs.extend([
                    pair(90, 22),
                    pair(91, rows.len()),
                    pair(92, columns.len()),
                    pair(93, 0),
                    pair(94, 0),
                    pair(95, 0),
                    pair(96, 0),
                ]);
                pairs.extend(rows.iter().map(|r| pair(141, number(*r))));
                pairs.extend(columns.iter().map(|c| pair(142, number(*c))));
                for (r, _) in rows.iter().enumerate() {
                    for (c, _) in columns.iter().enumerate() {
                        let across = *title && r == 0;
                        // A title is one cell across the row; the cells it covers are merged into it.
                        let (merged, span) = if across { (i32::from(c > 0), if c == 0 { columns.len() } else { 1 }) } else { (0, 1) };
                        pairs.extend([pair(171, 1), pair(172, 0), pair(173, merged), pair(174, 0), pair(175, span), pair(176, 1)]);
                        pairs.extend([pair(91, 0), pair(178, 0), pair(145, "0.0")]);
                        let text = cells.get(r).and_then(|row| row.get(c)).cloned().unwrap_or_default();
                        let text = encode_percent_codes(&text).replace('\n', "\\P");
                        // The cell's value as AutoCAD 2008 and later keep it: a string, shown as itself.
                        pairs.extend([pair(92, 0), pair(301, "CELL_VALUE"), pair(93, 2), pair(90, 4), pair(91, 0), pair(1, &text)]);
                        pairs.extend([pair(94, 0), pair(300, ""), pair(302, &text), pair(304, "ACVALUE_END")]);
                    }
                }
                vec![Group(pairs)]
            }
            Shape::Image { path, position, u, v, pixels } if self.modern => {
                let mut pairs = self.head("IMAGE", owner, layer, color, linetype, &["AcDbRasterImage"]);
                let image = pairs.iter().find(|p| p.code == 5).map(|p| p.value.clone()).unwrap_or_default();
                let definition = match self.image_defs.iter().find(|(p, _, _)| p == path) {
                    Some((_, handle, _)) => handle.clone(),
                    None => {
                        let handle = self.handle();
                        self.image_defs.push((path.clone(), handle.clone(), *pixels));
                        handle
                    }
                };
                let reactor = self.handle();
                self.image_reactors.push((reactor.clone(), image, definition.clone()));
                let (w, h) = (pixels.0.max(1) as f64, pixels.1.max(1) as f64);
                pairs.push(pair(90, 0));
                pairs.extend(point_pairs(10, *position));
                pairs.extend(point_pairs(11, *u * (1.0 / w)));
                pairs.extend(point_pairs(12, *v * (1.0 / h)));
                pairs.extend([pair(13, number(w)), pair(23, number(h)), pair(340, &definition), pair(70, 3), pair(280, 0)]);
                pairs.extend([pair(281, 50), pair(282, 50), pair(283, 0), pair(360, &reactor), pair(71, 1), pair(91, 2)]);
                pairs.extend([pair(14, "-0.5"), pair(24, "-0.5"), pair(14, number(w - 0.5)), pair(24, number(h - 0.5)), pair(290, 0)]);
                vec![Group(pairs)]
            }
            // R12 has no images: their frames stand in.
            Shape::Image { position, u, v, .. } => {
                let corners = crate::model::entities::image_corners(*position, *u, *v);
                (0..4).map(|i| self.line(owner, layer, color, linetype, corners[i], corners[(i + 1) % 4])).collect()
            }
            Shape::XLine { base, direction, ray } if self.modern => {
                let (kind, subclass) = if *ray { ("RAY", "AcDbRay") } else { ("XLINE", "AcDbXline") };
                let mut pairs = self.head(kind, owner, layer, color, linetype, &[subclass]);
                pairs.extend(point_pairs(10, *base));
                pairs.extend(point_pairs(11, *direction));
                vec![Group(pairs)]
            }
            // R12 has no construction lines: a long line stands in.
            Shape::XLine { base, direction, ray } => {
                let (start, end) = crate::model::xline::far_ends(*base, *direction, *ray);
                vec![self.line(owner, layer, color, linetype, start, end)]
            }
            Shape::Viewport { center, width, height, view_center, view_height, frozen, locked } => {
                self.viewport(owner, layer, color, *center, (*width, *height), *view_center, *view_height, frozen, *locked)
            }
            Shape::Multileader { .. } if !self.r2007 || self.mleader_ids.is_none() => {
                // Files before R2007 have no multileaders: what they draw goes in instead.
                let pieces = explode(entity).unwrap_or_default();
                pieces.iter().flat_map(|piece| self.entity(piece, linetype, dimension_style, owner)).collect()
            }
            Shape::Multileader { leaders, landing, dogleg, text, text_position, text_height, arrow_size, style } => {
                let Some(ids) = self.mleader_ids.clone() else { return vec![] };
                let style_handle =
                    ids.styles.get(&style.to_lowercase()).or_else(|| ids.styles.values().next()).cloned().unwrap_or_default();
                let right = *dogleg >= 0.0;
                let gap = ((text_position.x - landing.x).abs() - dogleg.abs()).max(0.0);
                let raw = encode_percent_codes(text).replace('\n', "\\P");
                let mut pairs = self.head("MULTILEADER", owner, layer, color, linetype, &["AcDbMLeader"]);
                pairs.extend([pair(270, 2), pair(300, "CONTEXT_DATA{"), pair(40, "1.0")]);
                pairs.extend(point_pairs(10, *landing + pt(*dogleg, 0.0)));
                pairs.extend([pair(41, number(*text_height)), pair(140, number(*arrow_size)), pair(145, number(gap))]);
                pairs.extend([
                    pair(174, 1),
                    pair(175, 1),
                    pair(176, if right { 0 } else { 2 }),
                    pair(177, 0),
                    pair(290, 1),
                    pair(304, &raw),
                ]);
                pairs.extend([pair(11, "0.0"), pair(21, "0.0"), pair(31, "1.0"), pair(340, &ids.text_style)]);
                pairs.extend(point_pairs(12, *text_position));
                pairs.extend([pair(13, "1.0"), pair(23, "0.0"), pair(33, "0.0")]);
                pairs.extend([
                    pair(42, "0.0"),
                    pair(43, "0.0"),
                    pair(44, "0.0"),
                    pair(45, "1.0"),
                    pair(170, 1),
                    pair(90, BY_BLOCK_COLOR_VALUE),
                ]);
                pairs.extend([pair(171, if right { 1 } else { 3 }), pair(172, 1), pair(91, -939524096), pair(141, "1.5"), pair(92, 0)]);
                pairs.extend([pair(291, 0), pair(292, 0), pair(173, 0), pair(293, 0), pair(142, "0.0"), pair(143, "0.0")]);
                pairs.extend([pair(294, 0), pair(295, 1), pair(296, 0)]);
                pairs.extend(point_pairs(110, pt(0.0, 0.0)));
                pairs.extend([
                    pair(111, "1.0"),
                    pair(121, "0.0"),
                    pair(131, "0.0"),
                    pair(112, "0.0"),
                    pair(122, "1.0"),
                    pair(132, "0.0"),
                    pair(297, 0),
                ]);
                pairs.extend([pair(302, "LEADER{"), pair(290, 1), pair(291, 1)]);
                pairs.extend(point_pairs(10, *landing));
                pairs.extend([
                    pair(11, if right { "1.0" } else { "-1.0" }),
                    pair(21, "0.0"),
                    pair(31, "0.0"),
                    pair(90, 0),
                    pair(40, number(dogleg.abs())),
                ]);
                for (index, leader) in leaders.iter().enumerate() {
                    pairs.push(pair(304, "LEADER_LINE{"));
                    for p in leader {
                        pairs.extend(point_pairs(10, *p));
                    }
                    pairs.extend([pair(91, index), pair(92, BY_BLOCK_COLOR_VALUE), pair(305, "}")]);
                }
                pairs.extend([pair(271, 0), pair(303, "}"), pair(272, 9), pair(273, 9), pair(301, "}")]);
                pairs.extend([
                    pair(340, &style_handle),
                    pair(90, 2147483647),
                    pair(170, 1),
                    pair(91, BY_BLOCK_COLOR_VALUE),
                    pair(341, &ids.linetype),
                    pair(171, -2),
                ]);
                pairs.extend([
                    pair(290, 1),
                    pair(291, 1),
                    pair(41, number(dogleg.abs())),
                    pair(42, number(*arrow_size)),
                    pair(172, 2),
                    pair(343, &ids.text_style),
                ]);
                pairs.extend([
                    pair(173, 1),
                    pair(95, 1),
                    pair(174, 1),
                    pair(175, if right { 0 } else { 2 }),
                    pair(92, BY_BLOCK_COLOR_VALUE),
                    pair(292, 0),
                    pair(93, BY_BLOCK_COLOR_VALUE),
                ]);
                pairs.extend([
                    pair(10, "1.0"),
                    pair(20, "1.0"),
                    pair(30, "1.0"),
                    pair(43, "0.0"),
                    pair(176, 0),
                    pair(293, 0),
                    pair(294, 0),
                    pair(178, 0),
                    pair(179, 1),
                    pair(45, "1.0"),
                ]);
                pairs.extend([pair(271, 0), pair(272, 9), pair(273, 9), pair(295, 0)]);
                vec![Group(pairs)]
            }
            Shape::Annotation { kind: AnnotationKind::ArcLength | AnnotationKind::JoggedRadius, .. } if !self.r2004 => {
                // Files before R2004 have no arc length or jogged dimensions: what they draw goes in instead.
                let pieces = explode(entity).unwrap_or_default();
                pieces.iter().flat_map(|piece| self.entity(piece, linetype, dimension_style, owner)).collect()
            }
            Shape::Annotation { .. } if !self.modern => {
                // R12 has no leaders, and these dimensions go in as what they draw.
                let pieces = explode(entity).unwrap_or_default();
                pieces.iter().flat_map(|piece| self.entity(piece, linetype, dimension_style, owner)).collect()
            }
            Shape::Annotation { kind: AnnotationKind::Leader { arrow }, points, look } => {
                let mut pairs = self.head("LEADER", owner, layer, color, linetype, &["AcDbLeader"]);
                pairs.push(pair(3, dimension_style));
                pairs.extend([pair(71, i32::from(*arrow)), pair(72, 0), pair(73, 3), pair(74, 0), pair(75, 0)]);
                pairs.push(pair(40, number(look.text_height * look.scale)));
                pairs.push(pair(41, "0.0"));
                pairs.push(pair(76, points.len()));
                for p in points {
                    pairs.extend(point_pairs(10, *p));
                }
                pairs.extend(dimension_style_overrides(look));
                vec![Group(pairs)]
            }
            Shape::Annotation { kind, points, look } => {
                let block_name = self.block_name();
                let parts = annotation_parts(*kind, points, look);
                let texts: Vec<AnnotationText> = parts.text.iter().chain(&parts.stack).cloned().collect();
                let block = self.dimension_block(&block_name, &parts.paths, &parts.arrows, &texts);
                self.blocks.push(block);
                let middle = parts.text.as_ref().map_or(points[0], |t| t.middle);
                // The whole text's middle, its tolerance stack included, as AutoCAD keeps it.
                let middle = look.text_position.unwrap_or(middle);
                let arc_type = if self.r2018 { ARC_DIMENSION_TYPE_2018 } else { ARC_DIMENSION_TYPE };
                let (entity_kind, code, subclass, definition) = match (kind, &points[..]) {
                    (AnnotationKind::Radius, [center, chord, ..]) => {
                        ("DIMENSION", RADIUS_DIMENSION, "AcDbRadialDimension", [(10, *center), (15, *chord)].to_vec())
                    }
                    (AnnotationKind::Diameter, [center, chord, ..]) => {
                        ("DIMENSION", DIAMETER_DIMENSION, "AcDbDiametricDimension", vec![(10, *center - (*chord - *center)), (15, *chord)])
                    }
                    (AnnotationKind::Ordinate { x_datum }, [origin, feature, leader_end, ..]) => (
                        "DIMENSION",
                        ORDINATE_DIMENSION | if *x_datum { ORDINATE_X_TYPE } else { 0 },
                        "AcDbOrdinateDimension",
                        vec![(10, *origin), (13, *feature), (14, *leader_end)],
                    ),
                    (AnnotationKind::ArcLength, [center, start, end, arc_point, ..]) => {
                        ("ARC_DIMENSION", arc_type, "AcDbArcDimension", vec![(10, *arc_point), (13, *start), (14, *end), (15, *center)])
                    }
                    (AnnotationKind::JoggedRadius, [center, chord, shown_center, jog, ..]) => (
                        "LARGE_RADIAL_DIMENSION",
                        LARGE_RADIAL_DIMENSION_TYPE,
                        "AcDbRadialDimensionLarge",
                        vec![(10, *center), (13, *chord), (14, *shown_center), (15, *jog)],
                    ),
                    (AnnotationKind::Angle, [vertex, first, second, arc_point, ..]) => (
                        "DIMENSION",
                        THREE_POINT_ANGULAR_DIMENSION,
                        "AcDb3PointAngularDimension",
                        vec![(10, *arc_point), (13, *first), (14, *second), (15, *vertex)],
                    ),
                    _ => return vec![],
                };
                let mut pairs = self.head(entity_kind, owner, layer, color, linetype, &["AcDbDimension"]);
                pairs.push(pair(280, 0));
                pairs.push(pair(2, &block_name));
                pairs.extend(point_pairs(10, definition[0].1));
                pairs.extend(point_pairs(11, middle));
                pairs.push(pair(70, code | BLOCK_REFERENCED_BY_THIS_DIMENSION_ONLY));
                pairs.push(pair(71, 5));
                let measured = annotation_measurement(*kind, points).unwrap_or(0.0);
                let measured = if *kind == AnnotationKind::Angle { measured.to_radians() } else { measured };
                pairs.push(pair(42, number(measured)));
                if !look.text.is_empty() {
                    pairs.push(pair(1, &look.text));
                }
                if let Some(rotation) = look.text_rotation {
                    pairs.push(pair(53, number(rotation.to_radians())));
                }
                pairs.push(pair(3, dimension_style));
                pairs.push(pair(100, subclass));
                for &(code, point) in &definition[1..] {
                    pairs.extend(point_pairs(code, point));
                }
                match kind {
                    AnnotationKind::Radius | AnnotationKind::Diameter => pairs.push(pair(40, "0.0")),
                    AnnotationKind::ArcLength => {
                        // The arc's start and end angles, not partial, no leader.
                        let (center, start, end) = (points[0], points[1], points[2]);
                        let angle = |p: Point| (p.y - center.y).atan2(p.x - center.x);
                        pairs.extend([pair(40, number(angle(start))), pair(41, number(angle(end))), pair(70, 0), pair(71, 0)]);
                        pairs.extend(point_pairs(16, pt(0.0, 0.0)));
                        pairs.extend(point_pairs(17, pt(0.0, 0.0)));
                    }
                    AnnotationKind::JoggedRadius => pairs.push(pair(40, number(std::f64::consts::FRAC_PI_4))),
                    _ => {}
                }
                pairs.extend(dimension_style_overrides(look));
                vec![Group(pairs)]
            }
            Shape::Ellipse { .. } | Shape::Spline { .. } | Shape::Hatch { .. } if !self.modern => {
                // R12 has no ellipses, splines or hatches: their outlines go in as polylines.
                let parts = entity_parts(entity, entity_extent(entity).diagonal() * 1e-3);
                let outlines: Vec<Vec<Point>> = match &entity.shape {
                    Shape::Hatch { loops, .. } => loops.clone(),
                    _ => parts.paths,
                };
                let mut groups = vec![];
                for points in outlines {
                    let shape = Shape::Polyline { vertices: points.into_iter().map(vertex).collect(), closed: false };
                    groups.extend(self.entity(&entity.with_shape(shape), linetype, dimension_style, owner));
                }
                groups
            }
            Shape::Ellipse { center, major, ratio, start, end } => {
                let mut pairs = self.head("ELLIPSE", owner, layer, color, linetype, &["AcDbEllipse"]);
                pairs.extend(point_pairs(10, *center));
                pairs.extend(point_pairs(11, *major));
                pairs.extend([pair(40, number(*ratio)), pair(41, number(*start)), pair(42, number(*end))]);
                vec![Group(pairs)]
            }
            Shape::Spline { degree, control, knots, weights, closed } => {
                let mut pairs = self.head("SPLINE", owner, layer, color, linetype, &["AcDbSpline"]);
                let knots = if knots.len() == control.len() + degree + 1 { knots.clone() } else { clamped_knots(*degree, control.len()) };
                let rational = !weights.is_empty();
                let flags = 8 | if *closed { 1 } else { 0 } | if rational { 4 } else { 0 };
                pairs.extend([pair(70, flags), pair(71, degree), pair(72, knots.len()), pair(73, control.len()), pair(74, 0)]);
                pairs.extend(knots.iter().map(|k| pair(40, number(*k))));
                pairs.extend(weights.iter().map(|w| pair(41, number(*w))));
                for p in control {
                    pairs.extend(point_pairs(10, *p));
                }
                vec![Group(pairs)]
            }
            Shape::Point { position } => {
                let mut pairs = self.head("POINT", owner, layer, color, linetype, &["AcDbPoint"]);
                pairs.extend(point_pairs(10, *position));
                vec![Group(pairs)]
            }
            Shape::Wipeout { corners } if self.modern => {
                let bounds = Bounds::around(corners.iter().copied());
                let (width, height) = (bounds.width().max(1e-12), bounds.height().max(1e-12));
                let mut pairs = self.head("WIPEOUT", owner, layer, color, linetype, &["AcDbWipeout"]);
                pairs.push(pair(90, 0));
                pairs.extend(point_pairs(10, pt(bounds.x0, bounds.y0)));
                pairs.extend(point_pairs(11, pt(width, 0.0)));
                pairs.extend(point_pairs(12, pt(0.0, height)));
                pairs.extend([pair(13, "1.0"), pair(23, "1.0"), pair(340, "0"), pair(70, 7), pair(280, 1)]);
                pairs.extend([pair(281, 50), pair(282, 50), pair(283, 0), pair(71, 2), pair(91, corners.len() + 1)]);
                // The clip boundary is in the picture's own terms: a unit square about its middle, y downwards.
                for corner in corners.iter().chain(corners.first()) {
                    let x = (corner.x - bounds.x0) / width - 0.5;
                    let y = 0.5 - (corner.y - bounds.y0) / height;
                    pairs.extend([pair(14, number(x)), pair(24, number(y))]);
                }
                pairs.push(pair(290, 0));
                self.wipeouts = true;
                vec![Group(pairs)]
            }
            Shape::Mline { vertices, closed, justification, scale, style, gaps } if self.modern => {
                let points = dedup(vertices);
                let miters = miters(&points, *closed && points.len() > 2);
                let distances = element_distances(*justification, *scale, style);
                let handle = self
                    .mline_styles
                    .get(&style.name.to_lowercase())
                    .or_else(|| self.mline_styles.get("standard"))
                    .cloned()
                    .unwrap_or_else(|| "0".into());
                let mut pairs = self.head("MLINE", owner, layer, color, linetype, &["AcDbMline"]);
                pairs.extend([pair(2, &style.name), pair(340, &handle), pair(40, number(*scale)), pair(70, justification.code())]);
                pairs.extend([pair(71, if *closed { 3 } else { 1 }), pair(72, points.len()), pair(73, distances.len())]);
                pairs.extend(point_pairs(10, points.first().copied().unwrap_or_default()));
                pairs.extend([pair(210, "0.0"), pair(220, "0.0"), pair(230, "1.0")]);
                for (i, (point, miter)) in points.iter().zip(&miters).enumerate() {
                    let next = points.get(i + 1).or(if *closed { points.first() } else { None });
                    let along = match next {
                        Some(next) => *next - *point,
                        None => *point - points[i.saturating_sub(1)],
                    };
                    let unit = |v: Point| v * (1.0 / v.length().max(f64::MIN_POSITIVE));
                    pairs.extend(point_pairs(11, *point));
                    pairs.extend(point_pairs(12, unit(along)));
                    pairs.extend(point_pairs(13, unit(*miter)));
                    // Each line's distance from the vertex along the miter, then where it turns off and back on for
                    // each gap MLEDIT cut on this segment; no fill of its own.
                    for (element, distance) in distances.iter().enumerate() {
                        let mut cuts: Vec<(f64, f64)> =
                            gaps.iter().filter(|g| g.segment == i && g.element == element).map(|g| (g.from, g.to)).collect();
                        cuts.sort_by(|a, b| a.0.total_cmp(&b.0));
                        pairs.extend([pair(74, 2 + 2 * cuts.len()), pair(41, number(distance * miter.length())), pair(41, "0.0")]);
                        pairs.extend(cuts.iter().flat_map(|(from, to)| [pair(41, number(*from)), pair(41, number(*to))]));
                        pairs.push(pair(75, 0));
                    }
                }
                vec![Group(pairs)]
            }
            // R12 has no multilines: their lines go in instead.
            Shape::Mline { .. } => {
                let pieces = explode(entity).unwrap_or_default();
                pieces.iter().flat_map(|piece| self.entity(piece, linetype, dimension_style, owner)).collect()
            }
            // R12 has no wipeouts: the frame stands in.
            Shape::Wipeout { corners } => {
                (0..corners.len()).map(|i| self.line(owner, layer, color, linetype, corners[i], corners[(i + 1) % corners.len()])).collect()
            }
            Shape::Solid { corners } => {
                // The file keeps the third and fourth corners the other way round.
                let mut pairs = self.head("SOLID", owner, layer, color, linetype, &["AcDbTrace"]);
                let last = corners.len().saturating_sub(1);
                let at = |i: usize| corners.get(i.min(last)).copied().unwrap_or_default();
                let (third, fourth) = if corners.len() >= 4 { (at(3), at(2)) } else { (at(2), at(2)) };
                for (code, corner) in [(10, at(0)), (11, at(1)), (12, third), (13, fourth)] {
                    pairs.extend(point_pairs(code, corner));
                }
                vec![Group(pairs)]
            }
            Shape::Hatch { loops, solid, pattern, name, scale, angle, gradient, association } => {
                // Objects it follows that are already written; only then can the file say so.
                let linked: Option<Vec<String>> = match association {
                    Some(Association::Objects(ids)) if ids.len() == loops.len() => {
                        ids.iter().map(|id| self.entity_handles.get(id).cloned()).collect()
                    }
                    _ => None,
                };
                let mut pairs = self.head("HATCH", owner, layer, color, linetype, &["AcDbHatch"]);
                pairs.extend(point_pairs(10, pt(0.0, 0.0)));
                pairs.extend([pair(210, "0.0"), pair(220, "0.0"), pair(230, "1.0")]);
                let name = if name.is_empty() { if *solid { "SOLID" } else { "USER" } } else { name.as_str() };
                pairs.extend([
                    pair(2, name),
                    pair(70, if *solid { 1 } else { 0 }),
                    pair(71, i32::from(linked.is_some())),
                    pair(91, loops.len()),
                ]);
                for (index, points) in loops.iter().enumerate() {
                    pairs.extend([pair(92, 2), pair(72, 0), pair(73, 1), pair(93, points.len())]);
                    for p in points {
                        pairs.extend([pair(10, number(p.x)), pair(20, number(p.y))]);
                    }
                    match &linked {
                        Some(handles) => pairs.extend([pair(97, 1), pair(330, &handles[index])]),
                        None => pairs.push(pair(97, 0)),
                    }
                }
                pairs.extend([pair(75, 0), pair(76, 1)]);
                if !*solid {
                    pairs.extend([pair(52, number(*angle)), pair(41, number(*scale)), pair(77, 0), pair(78, pattern.len())]);
                    for line in pattern {
                        pairs.push(pair(53, number(line.angle)));
                        pairs.extend([pair(43, number(line.origin.x)), pair(44, number(line.origin.y))]);
                        pairs.extend([pair(45, number(line.offset.x)), pair(46, number(line.offset.y))]);
                        pairs.push(pair(79, line.dashes.len()));
                        pairs.extend(line.dashes.iter().map(|d| pair(49, number(*d))));
                    }
                }
                pairs.push(pair(98, 0));
                if let Some(gradient) = gradient.as_ref().filter(|_| self.r2004) {
                    let value = |(r, g, b): (u8, u8, u8)| ((r as i64) << 16) | ((g as i64) << 8) | b as i64;
                    pairs.extend([
                        pair(450, 1),
                        pair(451, 0),
                        pair(460, number(gradient.angle.to_radians())),
                        pair(461, "0.0"),
                        pair(452, 0),
                    ]);
                    pairs.extend([pair(462, "1.0"), pair(453, 2), pair(463, "0.0"), pair(63, 5), pair(421, value(gradient.first))]);
                    pairs.extend([pair(463, "1.0"), pair(63, 2), pair(421, value(gradient.second)), pair(470, &gradient.name)]);
                }
                vec![Group(pairs)]
            }
            Shape::Insert { block, position, scale_x, scale_y, rotation, attributes } => {
                if !self.block_names.contains(&block.0.name.to_uppercase()) {
                    self.block_names.insert(block.0.name.to_uppercase());
                    self.blocks_to_define.push(block.clone());
                }
                let mut pairs = self.head("INSERT", owner, layer, color, linetype, &["AcDbBlockReference"]);
                let handle = pairs.iter().find(|p| p.code == 5).map(|p| p.value.clone()).unwrap_or_default();
                if !attributes.is_empty() {
                    pairs.push(pair(66, 1));
                }
                pairs.push(pair(2, &block.0.name));
                pairs.extend(point_pairs(10, *position));
                pairs.extend([pair(41, number(*scale_x)), pair(42, number(*scale_y)), pair(43, "1.0"), pair(50, number(*rotation))]);
                let mut groups = vec![Group(pairs)];
                for attribute in attributes {
                    let Shape::Text { position, text, height, rotation, style, align, .. } = &attribute.text else { continue };
                    let value = encode_percent_codes(text);
                    let mut pairs = self.text(
                        "ATTRIB",
                        &handle,
                        &attribute.layer,
                        attribute.color,
                        BYLAYER_LINETYPE,
                        *position,
                        &value,
                        *height,
                        *rotation,
                        *align,
                    );
                    if let Some((name, _)) = self.text_styles.get(&style.trim().to_lowercase()) {
                        pairs.push(pair(7, name));
                    }
                    if self.modern {
                        pairs.push(pair(100, "AcDbAttribute"));
                    }
                    pairs.extend([pair(2, &attribute.tag), pair(70, if attribute.invisible { 1 } else { 0 })]);
                    // The vertical alignment of an attribute comes after its tag.
                    if let Some(at) = pairs.iter().position(|p| p.code == 74) {
                        let vertical = pairs.remove(at);
                        pairs.push(vertical);
                    }
                    groups.push(Group(pairs));
                }
                if !attributes.is_empty() {
                    groups.push(Group(self.head("SEQEND", &handle, layer, color, linetype, &[])));
                }
                groups
            }
        }
    }

    /// Text with several lines or its own font, as MTEXT attached as it is aligned.
    #[allow(clippy::too_many_arguments)]
    fn mtext(
        &mut self,
        owner: &str,
        layer: &str,
        color: i32,
        linetype: &str,
        position: Point,
        value: &str,
        height: f64,
        rotation: f64,
        look: MText,
    ) -> Group {
        let mut pairs = self.head("MTEXT", owner, layer, color, linetype, &["AcDbMText"]);
        pairs.extend(point_pairs(10, position));
        pairs.push(pair(40, number(height)));
        pairs.push(pair(41, "0.0"));
        pairs.push(pair(71, mtext_attachment(look.align)));
        pairs.push(pair(72, 1));
        let escaped = value.replace('\\', "\\\\").replace('{', "\\{").replace('}', "\\}").replace('\n', "\\P");
        let content = if let Some(raw) = look.raw {
            raw.to_string()
        } else if look.font.is_empty() {
            escaped
        } else {
            let family = Path::new(look.font).file_stem().map_or(look.font.to_string(), |s| s.to_string_lossy().into_owned());
            format!("{{\\f{family}|b0|i0|c0|p0;{escaped}}}")
        };
        // MTEXT text longer than 250 characters goes in pieces: code 3 for all but the last, code 1 for the last.
        let chunks: Vec<String> = content.chars().collect::<Vec<_>>().chunks(250).map(|c| c.iter().collect()).collect();
        for (index, chunk) in chunks.iter().enumerate() {
            pairs.push(pair(if index + 1 == chunks.len() { 1 } else { 3 }, chunk));
        }
        if chunks.is_empty() {
            pairs.push(pair(1, ""));
        }
        if let Some(style) = look.style {
            pairs.push(pair(7, style));
        }
        pairs.extend(point_pairs(11, direction(rotation)));
        if look.line_spacing != 1.0 {
            pairs.push(pair(73, 2)); // exactly the spacing asked for
            pairs.push(pair(44, number(look.line_spacing)));
        }
        Group(pairs)
    }

    /// One line of text. An aligned text keeps its alignment point in 11 and says how it lines up there.
    #[allow(clippy::too_many_arguments)]
    fn text(
        &mut self,
        kind: &str,
        owner: &str,
        layer: &str,
        color: i32,
        linetype: &str,
        position: Point,
        value: &str,
        height: f64,
        rotation: f64,
        align: TextAlign,
    ) -> Vec<Pair> {
        let mut pairs = self.head(kind, owner, layer, color, linetype, &["AcDbText"]);
        pairs.extend(point_pairs(10, position));
        pairs.push(pair(40, number(height)));
        pairs.push(pair(1, value));
        if rotation != 0.0 {
            pairs.push(pair(50, number(rotation)));
        }
        let (horizontal, vertical) = text_align_codes(align);
        if (horizontal, vertical) != (0, 0) {
            pairs.push(pair(72, horizontal));
            pairs.extend(point_pairs(11, position));
        }
        if self.modern && kind == "TEXT" {
            pairs.push(pair(100, "AcDbText"));
        }
        if vertical != 0 {
            // TEXT says how it lines up vertically in 73, an attribute in 74.
            pairs.push(pair(if kind == "TEXT" { 73 } else { 74 }, vertical));
        }
        pairs
    }

    /// The block that draws a dimension: its lines, arrowheads or ticks and measurement, colored by the dimension.
    fn dimension_block(&mut self, name: &str, paths: &[Vec<Point>], arrows: &[[Point; 3]], texts: &[AnnotationText]) -> DimensionBlock {
        let texts: Vec<TextSpec> = texts.iter().map(crate::model::parts::dimension_text).collect();
        self.anonymous_block(name, paths, arrows, &texts)
    }

    /// An anonymous block of lines, filled triangles and text, by block in color and lineweight, so it shows as
    /// what uses it does.
    fn anonymous_block(&mut self, name: &str, paths: &[Vec<Point>], arrows: &[[Point; 3]], texts: &[TextSpec]) -> DimensionBlock {
        let outer = std::mem::replace(&mut self.lineweight, lineweight::BYBLOCK);
        let block = self.anonymous_block_groups(name, paths, arrows, texts);
        self.lineweight = outer;
        block
    }

    fn anonymous_block_groups(&mut self, name: &str, paths: &[Vec<Point>], arrows: &[[Point; 3]], texts: &[TextSpec]) -> DimensionBlock {
        const BY_BLOCK: i32 = 0;
        let record = self.handle();
        let mut begin = vec![pair(0, "BLOCK"), pair(5, self.handle())];
        if self.modern {
            begin.extend([pair(330, &record), pair(100, "AcDbEntity")]);
        }
        begin.push(pair(8, DEFAULT_LAYER));
        if self.modern {
            begin.push(pair(100, "AcDbBlockBegin"));
        }
        begin.extend([pair(2, name), pair(70, 1)]);
        begin.extend(point_pairs(10, pt(0.0, 0.0)));
        begin.extend([pair(3, name), pair(1, "")]);
        let mut groups = vec![Group(begin)];
        for path in paths {
            for w in path.windows(2) {
                groups.push(self.line(&record, DEFAULT_LAYER, BY_BLOCK, "ByBlock", w[0], w[1]));
            }
        }
        for &[tip, left, right] in arrows {
            let mut pairs = self.head("SOLID", &record, DEFAULT_LAYER, BY_BLOCK, "ByBlock", &["AcDbTrace"]);
            for (code, corner) in [(10, tip), (11, left), (12, right), (13, right)] {
                pairs.extend(point_pairs(code, corner));
            }
            groups.push(Group(pairs));
        }
        for text in texts {
            let lines: Vec<&str> = text.text.split('\n').collect();
            if lines.len() == 1 {
                groups.push(Group(self.text(
                    "TEXT",
                    &record,
                    DEFAULT_LAYER,
                    BY_BLOCK,
                    "ByBlock",
                    text.origin,
                    &text.text,
                    text.height,
                    text.rotation,
                    text.align,
                )));
                continue;
            }
            // TEXT holds one line: each line goes in on its own, where it sits, from its left end.
            let widths: Vec<f64> = lines.iter().map(|l| l.chars().count() as f64 * text.height * 0.6).collect();
            let left = TextAlign { h: HAlign::Left, v: VAlign::Baseline };
            for (line, origin) in lines.iter().zip(text.line_origins(&widths)) {
                groups.push(Group(self.text(
                    "TEXT",
                    &record,
                    DEFAULT_LAYER,
                    BY_BLOCK,
                    "ByBlock",
                    origin,
                    line,
                    text.height,
                    text.rotation,
                    left,
                )));
            }
        }
        let mut finish = vec![pair(0, "ENDBLK"), pair(5, self.handle())];
        if self.modern {
            finish.extend([pair(330, &record), pair(100, "AcDbEntity"), pair(8, DEFAULT_LAYER), pair(100, "AcDbBlockEnd")]);
        } else {
            finish.push(pair(8, DEFAULT_LAYER));
        }
        groups.push(Group(finish));
        DimensionBlock { name: name.to_string(), record, groups }
    }
}

/// The dimension's look, as overrides in its extended data, so it shows the same in other programs.
fn dimension_style_overrides(look: &DimensionLook) -> Vec<Pair> {
    let mut pairs = vec![pair(1001, "ACAD"), pair(1000, "DSTYLE"), pair(1002, "{")];
    let tick_size = if look.arrowhead == Arrowhead::Tick { look.arrow_size } else { 0.0 };
    for (code, value) in [
        (DIMSCALE, look.scale),
        (DIMTXT, look.text_height),
        (DIMASZ, look.arrow_size),
        (DIMEXO, look.extension_gap),
        (DIMEXE, look.extension_overshoot),
        (DIMGAP, look.text_gap),
        (DIMLFAC, look.length_factor),
        (DIMTSZ, tick_size),
    ] {
        pairs.push(pair(1070, code));
        pairs.push(pair(1040, number(value)));
    }
    let zeros = if look.trim_zeros { DIMZIN_TRAILING_ZEROS } else { 0 };
    for (code, value) in [(DIMDEC, look.decimals as i64), (DIMADEC, look.angle_decimals as i64), (DIMTAD, 1), (DIMZIN, zeros)] {
        pairs.push(pair(1070, code));
        pairs.push(pair(1070, value));
    }
    let (switches, sizes) = tolerance_variables(look);
    for (code, value) in switches {
        pairs.push(pair(1070, code));
        pairs.push(pair(1070, value));
    }
    for (code, value) in sizes {
        pairs.push(pair(1070, code));
        pairs.push(pair(1040, number(value)));
    }
    pairs.push(pair(1002, "}"));
    pairs
}

/// Finds, or adds from the standard set, the table entry for a linetype and gives its name in the file.
fn linetype_name(file: &mut DxfFile, writer: &mut Writer, linetype: &str) -> String {
    for i in file.table_entries("LTYPE") {
        if let Some(name) = file.groups[i].get(2)
            && name.trim().eq_ignore_ascii_case(linetype)
        {
            return name.trim().to_string();
        }
    }
    let Some((name, description, pattern)) = STANDARD_LINETYPES.iter().find(|(name, _, _)| name.eq_ignore_ascii_case(linetype)) else {
        return if linetype.eq_ignore_ascii_case(DEFAULT_LINETYPE) {
            DEFAULT_LINETYPE.into()
        } else {
            linetype_name(file, writer, DEFAULT_LINETYPE)
        };
    };
    let Some((table, end)) = file.table("LTYPE") else { return name.to_string() };
    let mut pairs = vec![pair(0, "LTYPE"), pair(5, writer.handle())];
    if writer.modern {
        pairs.push(pair(330, file.groups[table].handle().unwrap_or("0").trim()));
        pairs.extend([pair(100, "AcDbSymbolTableRecord"), pair(100, "AcDbLinetypeTableRecord")]);
    }
    pairs.extend([
        pair(2, name),
        pair(70, 0),
        pair(3, description),
        pair(72, 65),
        pair(73, pattern.len() - 1),
        pair(40, number(pattern[0])),
    ]);
    for element in &pattern[1..] {
        pairs.push(pair(49, number(*element)));
        if writer.modern {
            pairs.push(pair(74, 0));
        }
    }
    file.groups.insert(end, Group(pairs));
    name.to_string()
}

/// Make the file's dimension style table hold the document's styles with their values, adding the ones it lacks as
/// copies of `base`, and name the current one in the header. Gives each style's name in the file by lower-case name.
fn write_dimension_styles(file: &mut DxfFile, writer: &mut Writer, document: &Document, base: &str) -> HashMap<String, String> {
    let find = |file: &DxfFile, name: &str| {
        file.table_entries("DIMSTYLE").into_iter().find(|&i| file.groups[i].get(2).is_some_and(|n| n.trim().eq_ignore_ascii_case(name)))
    };
    let mut names = HashMap::new();
    for style in &document.dimension_styles {
        let at = match find(file, &style.name) {
            Some(at) => at,
            None => {
                let Some((_, end)) = file.table("DIMSTYLE") else { continue };
                let mut entry = find(file, base).map_or_else(|| Group(vec![pair(0, "DIMSTYLE"), pair(70, 0)]), |i| file.groups[i].clone());
                let handle_code = if entry.get(105).is_some() { 105 } else { 5 };
                entry.set(handle_code, writer.handle());
                entry.set(2, style.name.clone());
                file.groups.insert(end, entry);
                end
            }
        };
        let look = &style.look;
        let entry = &mut file.groups[at];
        let tick_size = if look.arrowhead == Arrowhead::Tick { look.arrow_size } else { 0.0 };
        for (code, value) in [
            (DIMSCALE, look.scale),
            (DIMTXT, look.text_height),
            (DIMASZ, look.arrow_size),
            (DIMEXO, look.extension_gap),
            (DIMEXE, look.extension_overshoot),
            (DIMGAP, look.text_gap),
            (DIMLFAC, look.length_factor),
            (DIMTSZ, tick_size),
        ] {
            entry.set(code, number(value));
        }
        let zeros = if look.trim_zeros { DIMZIN_TRAILING_ZEROS } else { 0 };
        for (code, value) in [(DIMDEC, look.decimals as i64), (DIMADEC, look.angle_decimals as i64), (DIMZIN, zeros)] {
            entry.set(code, value.to_string());
        }
        let (switches, sizes) = tolerance_variables(look);
        for (code, value) in switches {
            entry.set(code, value.to_string());
        }
        for (code, value) in sizes {
            entry.set(code, number(value));
        }
        names.insert(style.name.to_lowercase(), style.name.clone());
    }
    if let Some(current) = names.get(&document.current_dimension_style.to_lowercase()) {
        file.set_header("$DIMSTYLE", 2, current.clone());
    }
    names
}

/// Make the file's layer table match the document. Layers the user deleted go if nothing else uses them.
fn write_layers(file: &mut DxfFile, writer: &mut Writer, document: &Document) {
    for layer in &document.layers {
        let linetype = linetype_name(file, writer, &layer.linetype);
        let entries = file.table_entries("LAYER");
        let existing = entries
            .iter()
            .copied()
            .find(|&i| file.groups[i].get(2).is_some_and(|n| n == layer.name))
            .or_else(|| entries.iter().copied().find(|&i| file.groups[i].get(2).is_some_and(|n| n.eq_ignore_ascii_case(&layer.name))));
        let index = match existing {
            Some(index) => index,
            None => {
                let Some((_, end)) = file.table("LAYER") else { continue };
                let template = entries.iter().copied().find(|&i| file.groups[i].get(2).is_some_and(|n| n.trim() == DEFAULT_LAYER));
                let mut entry = match template {
                    Some(i) => file.groups[i].clone(),
                    None => Group(vec![pair(0, "LAYER"), pair(2, ""), pair(70, 0), pair(62, 7), pair(6, DEFAULT_LINETYPE)]),
                };
                entry.set(5, writer.handle());
                entry.0.retain(|p| p.code < 1000); // no extended data from layer 0
                entry.set(2, layer.name.clone());
                entry.set(70, "0".into());
                file.groups.insert(end, entry);
                end
            }
        };
        let entry = &mut file.groups[index];
        let mut flags = entry.int(70).unwrap_or(0);
        let shown = layer.visible;
        if layer.frozen {
            flags |= LAYER_FROZEN;
        } else {
            flags &= !LAYER_FROZEN;
        }
        if writer.modern {
            entry.set(290, i32::from(layer.plot).to_string());
        }
        if layer.locked {
            flags |= LAYER_LOCKED;
        } else {
            flags &= !LAYER_LOCKED;
        }
        entry.set(70, flags.to_string());
        let index = index_of(layer.color);
        entry.set(62, (if shown { index } else { -index }).to_string());
        match true_color_value(layer.color).filter(|_| writer.modern) {
            Some(value) => entry.set(420, value.to_string()),
            None => entry.0.retain(|p| p.code != 420),
        }
        entry.set(6, linetype);
        if writer.modern {
            entry.set(370, (if layer.lineweight >= 0 { layer.lineweight } else { lineweight::DEFAULT }).to_string());
        }
    }
    remove_deleted_layers(file, document);
}

fn remove_deleted_layers(file: &mut DxfFile, document: &Document) {
    let defined: HashSet<String> = document.layers.iter().map(|l| l.name.to_lowercase()).collect();
    let mut in_use: HashSet<String> = document.entities().iter().map(|e| e.layer.to_lowercase()).collect();
    for section in ["ENTITIES", "BLOCKS"] {
        if let Some((start, end)) = file.section(section) {
            in_use.extend(file.groups[start + 1..end].iter().filter_map(|g| g.get(8)).map(|n| n.trim().to_lowercase()));
        }
    }
    let doomed: Vec<usize> = file
        .table_entries("LAYER")
        .into_iter()
        .filter(|&i| {
            let name = file.groups[i].get(2).unwrap_or("").trim().to_lowercase();
            !defined.contains(&name) && !in_use.contains(&name) && !KEPT_LAYERS.contains(&name.as_str())
        })
        .collect();
    for index in doomed.into_iter().rev() {
        file.groups.remove(index);
    }
}

fn find_model_space(file: &DxfFile) -> String {
    file.table_entries("BLOCK_RECORD")
        .into_iter()
        .find(|&i| file.groups[i].get(2).is_some_and(|n| n.trim().eq_ignore_ascii_case("*Model_Space")))
        .and_then(|i| file.groups[i].handle().map(|h| h.trim().to_string()))
        .unwrap_or_else(|| "0".into())
}

/// Entities written for one owner, each linetype and dimension style as the file names them.
/// A space's entities to write, with what the file had on them that Linewright keeps: other programs' extended data
/// and named plot styles, by entity.
type SpaceToWrite<'a> = (&'a [Entity], &'a HashMap<EntityId, Vec<Pair>>, &'a crate::model::entities::IdMap<EntityId, String>);

fn write_entities(
    file: &mut DxfFile,
    writer: &mut Writer,
    linetypes: &mut BTreeMap<String, String>,
    (entities, kept_xdata, plot_styles): SpaceToWrite,
    owner: &str,
    (style_names, dimension_style): (&HashMap<String, String>, &str),
) -> Vec<Group> {
    let mut groups = vec![];
    for entity in entities {
        let linetype = if entity.linetype == BYLAYER_LINETYPE {
            BYLAYER_LINETYPE.to_string()
        } else {
            match linetypes.get(&entity.linetype) {
                Some(name) => name.clone(),
                None => {
                    let name = linetype_name(file, writer, &entity.linetype);
                    linetypes.insert(entity.linetype.clone(), name.clone());
                    name
                }
            }
        };
        let style = match &entity.shape {
            Shape::Dimension { look, .. } | Shape::Annotation { look, .. } => {
                style_names.get(&look.style.to_lowercase()).map_or(dimension_style, |s| s.as_str())
            }
            _ => dimension_style,
        };
        let mut written = writer.entity(entity, &linetype, style, owner);
        // What other programs attached to the object goes back after everything Linewright writes for it.
        if let (Some(first), Some(kept)) = (written.first_mut(), kept_xdata.get(&entity.id)) {
            first.0.extend(kept.iter().cloned());
        }
        // The named plot style it had, in its own part, before the part of its kind starts.
        if writer.modern
            && let (Some(first), Some(handle)) = (written.first_mut(), plot_styles.get(&entity.id))
            && let Some(at) = first.0.iter().enumerate().filter(|(_, p)| p.code == 100).nth(1).map(|(i, _)| i)
        {
            first.0.insert(at, pair(390, handle));
        }
        if let Some(handle) = written.first().and_then(|g| g.handle()) {
            writer.entity_handles.insert(entity.id, handle.trim().to_string());
        }
        groups.extend(written);
    }
    groups
}

fn record_handle(file: &DxfFile, name: &str) -> Option<String> {
    file.table_entries("BLOCK_RECORD")
        .into_iter()
        .find(|&i| file.groups[i].get(2).is_some_and(|n| n.trim().eq_ignore_ascii_case(name)))
        .and_then(|i| file.groups[i].handle().map(|h| h.trim().to_uppercase()))
}

fn record_name(file: &DxfFile, handle: &str) -> Option<String> {
    file.table_entries("BLOCK_RECORD")
        .into_iter()
        .find(|&i| file.groups[i].handle().is_some_and(|h| h.trim().eq_ignore_ascii_case(handle)))
        .and_then(|i| file.groups[i].get(2).map(|n| n.trim().to_string()))
}

/// Where a block's BLOCK and ENDBLK groups are.
fn block_range(file: &DxfFile, name: &str) -> Option<(usize, usize)> {
    let (start, end) = file.section("BLOCKS")?;
    let begin = (start + 1..end)
        .find(|&i| file.groups[i].kind() == "BLOCK" && file.groups[i].get(2).is_some_and(|n| n.trim().eq_ignore_ascii_case(name)))?;
    let finish = (begin..end).find(|&i| file.groups[i].kind() == "ENDBLK")?;
    Some((begin, finish))
}

fn object_index(file: &DxfFile, handle: &str) -> Option<usize> {
    let (start, end) = file.section("OBJECTS")?;
    (start + 1..end).find(|&i| file.groups[i].handle().is_some_and(|h| h.trim().eq_ignore_ascii_case(handle)))
}

/// The dictionary that lists the layouts by name.
fn layout_dictionary(file: &DxfFile) -> Option<usize> {
    let (start, end) = file.section("OBJECTS")?;
    let root = &file.groups[start + 1];
    let pairs = &root.0;
    let at = pairs.iter().position(|p| p.code == 3 && p.value.trim() == "ACAD_LAYOUT")?;
    let handle = pairs.get(at + 1).filter(|p| p.code == 350 || p.code == 360)?.value.trim().to_string();
    (start + 1..end).find(|&i| file.groups[i].handle().is_some_and(|h| h.trim().eq_ignore_ascii_case(&handle)))
}

/// Make the file's layouts match the document's: deleted ones go with their paper space, renamed and reordered
/// ones are updated, new ones are copied from an existing layout. What is on each layout is written to its paper
/// space: the first's (AutoCAD's *Paper_Space) goes with the model's entities, marked as paper space, which this
/// gives back; the others' go into their blocks. R12 files have one paper space, which holds every layout's.
fn write_layouts(
    file: &mut DxfFile,
    writer: &mut Writer,
    linetypes: &mut BTreeMap<String, String>,
    document: &Document,
    styles: (&HashMap<String, String>, &str),
) -> Vec<Group> {
    let mut paper = vec![];
    let dictionary = layout_dictionary(file);
    let paper_record = record_handle(file, PAPER_SPACE);
    let (Some(paper_record), Some(_), true) = (paper_record.clone(), dictionary, writer.modern) else {
        let owner = paper_record.unwrap_or_else(|| "0".into());
        for (index, _) in document.layouts.iter().enumerate() {
            paper.extend(write_entities(
                file,
                writer,
                linetypes,
                (document.space_entities(Some(index)), &document.kept_xdata, &document.object_plot_styles),
                &owner,
                styles,
            ));
        }
        for group in &mut paper {
            group.on_paper();
        }
        return paper;
    };
    // A layout the drawing made takes over the file's layout of the same name, as a new drawing's Layout1 does.
    let in_file: Vec<(Layout, String)> = read_layouts(file);
    let attached: Vec<(Option<String>, Option<String>)> = document
        .layouts
        .iter()
        .map(|layout| match &layout.object {
            Some(object) => (Some(object.clone()), layout.record.clone()),
            None => in_file
                .iter()
                .find(|(l, _)| l.name.eq_ignore_ascii_case(&layout.name))
                .map_or((None, None), |(l, _)| (l.object.clone(), l.record.clone())),
        })
        .collect();
    // Layouts the drawing no longer has.
    let kept: HashSet<String> = attached.iter().filter_map(|(object, _)| object.clone()).collect();
    let in_file: Vec<(String, String)> = in_file.into_iter().filter_map(|(l, record)| Some((l.object?, record))).collect();
    for (object, record) in in_file.iter().filter(|(object, _)| !kept.contains(object)) {
        if let Some(at) = object_index(file, object) {
            file.groups.remove(at);
        }
        if let Some(at) = layout_dictionary(file) {
            let pairs = &mut file.groups[at].0;
            if let Some(i) = pairs.iter().position(|p| (p.code == 350 || p.code == 360) && p.value.trim().eq_ignore_ascii_case(object)) {
                pairs.drain(i.saturating_sub(1)..=i);
            }
        }
        if record.eq_ignore_ascii_case(PAPER_SPACE) {
            if let Some((start, end)) = file.section("ENTITIES") {
                let body: Vec<Group> = file.groups.drain(start + 1..end).collect();
                let runs = entity_runs(&body);
                let mut keep = vec![];
                for (first, last) in runs {
                    if body[first].int(67) != Some(1) {
                        keep.extend(body[first..last].iter().cloned());
                    }
                }
                file.groups.splice(start + 1..start + 1, keep);
            }
        } else {
            remove_block(file, record);
        }
    }
    // The first layout holds *Paper_Space when none of the drawing's still does.
    let holder = attached.iter().position(|(_, r)| r.as_deref().is_some_and(|r| r.eq_ignore_ascii_case(&paper_record))).unwrap_or(0);
    let template = read_layouts(file).into_iter().find_map(|(l, _)| l.object).or_else(|| {
        let (start, end) = file.section("OBJECTS")?;
        (start + 1..end).find(|&i| file.groups[i].kind() == "LAYOUT").and_then(|i| file.groups[i].handle().map(|h| h.trim().to_string()))
    });
    let mut used_names: HashSet<String> =
        file.table_entries("BLOCK_RECORD").into_iter().filter_map(|i| file.groups[i].get(2).map(|n| n.trim().to_uppercase())).collect();
    for (index, (layout, (object, record))) in document.layouts.iter().zip(attached).enumerate() {
        let record = if index == holder {
            if let Some(old) = record.as_deref().filter(|r| !r.eq_ignore_ascii_case(&paper_record))
                && let Some(name) = record_name(file, old)
            {
                remove_block(file, &name);
            }
            paper_record.clone()
        } else if let Some(record) = record.filter(|r| !r.eq_ignore_ascii_case(&paper_record) && record_name(file, r).is_some()) {
            record
        } else {
            let name = (0..).map(|n| format!("*Paper_Space{n}")).find(|n| !used_names.contains(&n.to_uppercase())).unwrap_or_default();
            used_names.insert(name.to_uppercase());
            new_paper_block(file, writer, &name)
        };
        let object = match object.filter(|o| object_index(file, o).is_some()) {
            Some(object) => object,
            None => {
                let Some(source) = template.as_deref().and_then(|t| object_index(file, t)) else { continue };
                let mut group = file.groups[source].clone();
                let handle = writer.handle();
                group.set(5, handle.clone());
                group.0.retain(|p| p.code != 331);
                let at = layout_dictionary(file).map_or(source + 1, |d| d.max(source) + 1);
                file.groups.insert(at, group);
                if let Some(d) = layout_dictionary(file) {
                    let pairs = &mut file.groups[d].0;
                    let end = pairs.iter().position(|p| p.code >= 1000).unwrap_or(pairs.len());
                    pairs.splice(end..end, [pair(3, &layout.name), pair(350, &handle)]);
                }
                handle
            }
        };
        if let Some(at) = object_index(file, &object) {
            let group = &mut file.groups[at];
            group.set_in("AcDbLayout", 1, layout.name.clone());
            group.set_in("AcDbLayout", 71, (index + 1).to_string());
            group.set_in("AcDbLayout", 330, record.clone());
            let (low, high) = (layout.paper_origin, layout.paper_origin + pt(layout.paper_size.0, layout.paper_size.1));
            for (code, value) in [(10, low.x), (20, low.y), (11, high.x), (21, high.y)] {
                group.set_in("AcDbLayout", code, number(value));
            }
            set_plot_settings(group, &layout.plot);
        }
        if let Some(d) = layout_dictionary(file) {
            let pairs = &mut file.groups[d].0;
            if let Some(i) = pairs.iter().position(|p| (p.code == 350 || p.code == 360) && p.value.trim().eq_ignore_ascii_case(&object))
                && i > 0
                && pairs[i - 1].code == 3
            {
                pairs[i - 1].value = layout.name.clone();
            }
        }
        if let Some(at) = file
            .table_entries("BLOCK_RECORD")
            .into_iter()
            .find(|&i| file.groups[i].handle().is_some_and(|h| h.trim().eq_ignore_ascii_case(&record)))
        {
            file.groups[at].set(340, object.clone());
        }
        let mut groups = write_entities(
            file,
            writer,
            linetypes,
            (document.space_entities(Some(index)), &document.kept_xdata, &document.object_plot_styles),
            &record,
            styles,
        );
        if record.eq_ignore_ascii_case(&paper_record) {
            for group in &mut groups {
                group.on_paper();
            }
            paper.extend(groups);
        } else if let Some(name) = record_name(file, &record)
            && let Some((_, finish)) = block_range(file, &name)
        {
            file.groups.splice(finish..finish, groups);
        }
    }
    paper
}

/// The dictionary of table styles, and the handle of its Standard style if it has one.
fn table_style_dictionary(file: &DxfFile) -> Option<usize> {
    let (start, _) = file.section("OBJECTS")?;
    let pairs = &file.groups[start + 1].0;
    let at = pairs.iter().position(|p| p.code == 3 && p.value.trim() == "ACAD_TABLESTYLE")?;
    object_index(file, pairs.get(at + 1).filter(|p| p.code == 350 || p.code == 360)?.value.trim())
}

fn standard_table_style(file: &DxfFile) -> Option<String> {
    let pairs = &file.groups[table_style_dictionary(file)?].0;
    let at = pairs.iter().position(|p| p.code == 3 && p.value.trim().eq_ignore_ascii_case("Standard"))?;
    Some(pairs.get(at + 1)?.value.trim().to_string())
}

/// AutoCAD's Standard table style, made when tables were written and the file had none: data, header and title
/// cells, middle centered, text in the Standard style at the tables' height (the title a quarter bigger).
fn write_table_style(file: &mut DxfFile, writer: &Writer) {
    let Some((handle, Some(height))) = &writer.table_style else { return };
    let Some(dictionary) = table_style_dictionary(file) else { return };
    let owner = file.groups[dictionary].handle().unwrap_or("0").trim().to_string();
    let mut pairs = vec![
        pair(0, "TABLESTYLE"),
        pair(5, handle),
        pair(102, "{ACAD_REACTORS"),
        pair(330, &owner),
        pair(102, "}"),
        pair(330, &owner),
        pair(100, "AcDbTableStyle"),
        pair(3, "Standard"),
        pair(70, 0),
        pair(71, 0),
        pair(40, number(height * 0.6)),
        pair(41, number(height * 0.6)),
        pair(280, 0),
        pair(281, 0),
    ];
    for cell_height in [*height, *height, height * 1.25] {
        pairs.extend([pair(7, "Standard"), pair(140, number(cell_height)), pair(170, 5), pair(62, 0), pair(63, 7), pair(283, 0)]);
        pairs.extend([pair(90, 512), pair(91, 0), pair(1, "")]);
        pairs.extend((274..=279).map(|code| pair(code, -2)));
        pairs.extend((284..=289).map(|code| pair(code, 1)));
        pairs.extend((64..=69).map(|code| pair(code, 0)));
    }
    let entries = &mut file.groups[dictionary].0;
    let end = entries.iter().position(|p| p.code >= 1000).unwrap_or(entries.len());
    entries.splice(end..end, [pair(3, "Standard"), pair(350, handle)]);
    file.groups.insert(dictionary + 1, Group(pairs));
}

/// The class AutoCAD reads wipeouts with, from the Express Tools that brought them.
const WIPEOUT_CLASS: (&str, &str, &str, i32, i32) =
    ("WIPEOUT", "AcDbWipeout", "WipeOut|AutoCAD Express Tool|expresstools@autodesk.com", 127, 1);

/// The class AutoCAD reads the wipeout frame setting with.
const WIPEOUT_VARIABLES_CLASS: (&str, &str, &str, i32, i32) =
    ("WIPEOUTVARIABLES", "AcDbWipeoutVariables", "WipeOut|AutoCAD Express Tool|expresstools@autodesk.com", 0, 0);

/// Declares a class in the CLASSES section, unless the file already does: name, C++ class, application, proxy
/// flags and whether it is an entity.
/// Add an application to the APPID table, which every name extended data is filed under must be in.
fn register_application(file: &mut DxfFile, writer: &mut Writer, name: &str) {
    if file.table_entries("APPID").into_iter().any(|i| file.groups[i].get(2).is_some_and(|n| n.trim().eq_ignore_ascii_case(name))) {
        return;
    }
    let Some((table, end)) = file.table("APPID") else { return };
    let owner = file.groups[table].handle().map_or("0".to_string(), |h| h.trim().to_string());
    let mut entry = vec![pair(0, "APPID"), pair(5, writer.handle())];
    if writer.modern {
        entry.extend([pair(330, owner), pair(100, "AcDbSymbolTableRecord"), pair(100, "AcDbRegAppTableRecord")]);
    }
    entry.extend([pair(2, name), pair(70, 0)]);
    file.groups.insert(end, Group(entry));
}

fn declare_class(file: &mut DxfFile, (name, class, application, flags, entity): (&str, &str, &str, i32, i32)) {
    let Some((_, end)) = file.section("CLASSES") else { return };
    if file.groups.iter().any(|g| g.kind() == "CLASS" && g.get(1).is_some_and(|n| n.trim() == name)) {
        return;
    }
    let group = Group(vec![
        pair(0, "CLASS"),
        pair(1, name),
        pair(2, class),
        pair(3, application),
        pair(90, flags),
        pair(91, 0),
        pair(280, 0),
        pair(281, entity),
    ]);
    file.groups.insert(end, group);
}

/// A multiline style from its MLINESTYLE object: the fill (flag 1, in the color that comes before the first element),
/// square, round and inner-arc ends at either end (flags 16, 64 and 32 at the start, 256, 1024 and 512 at the end),
/// lines across the joints (flag 2), and its elements, each starting at its offset (code 49).
fn read_mline_style(group: &Group) -> Option<MlineStyle> {
    if group.kind() != "MLINESTYLE" {
        return None;
    }
    let flags = group.int(70).unwrap_or(0);
    let mut fill_color = BYLAYER;
    let mut elements: Vec<MlineElement> = vec![];
    for p in group.0.iter().take_while(|p| p.code < 1000) {
        match p.code {
            49 => {
                elements.push(MlineElement { offset: p.value.trim().parse().ok()?, color: BYLAYER, linetype: BYLAYER_LINETYPE.to_string() })
            }
            62 if !elements.is_empty() => elements.last_mut()?.color = p.value.trim().parse().unwrap_or(BYLAYER),
            62 => fill_color = p.value.trim().parse().unwrap_or(BYLAYER),
            6 if !elements.is_empty() => elements.last_mut()?.linetype = canonical_linetype(p.value.trim()),
            _ => {}
        }
    }
    Some(MlineStyle {
        name: group.text(2)?.trim().to_string(),
        description: group.text(3).unwrap_or_default().trim().to_string(),
        elements,
        fill: (flags & 1 != 0).then_some(fill_color),
        start_cap: flags & 16 != 0,
        end_cap: flags & 256 != 0,
        start_round: flags & 64 != 0,
        end_round: flags & 1024 != 0,
        start_inner_arcs: flags & 32 != 0,
        end_inner_arcs: flags & 512 != 0,
        joints: flags & 2 != 0,
    })
}

/// The ACAD_MLINESTYLE dictionary's place among the file's groups.
fn mline_dictionary(file: &DxfFile) -> Option<usize> {
    let (start, end) = file.section("OBJECTS")?;
    let pairs = &file.groups[start + 1].0;
    let at = pairs.iter().position(|p| p.code == 3 && p.value.trim() == "ACAD_MLINESTYLE")?;
    let handle = pairs.get(at + 1)?.value.trim().to_string();
    (start + 1..end)
        .find(|&i| file.groups[i].kind() == "DICTIONARY" && file.groups[i].handle().is_some_and(|h| h.trim().eq_ignore_ascii_case(&handle)))
}

/// What an MLINESTYLE object says after its subclass marker: name, flags, description, fill color, cap angles,
/// then each element's offset, color and linetype.
fn mline_style_pairs(style: &MlineStyle) -> Vec<Pair> {
    let flags = [
        (style.fill.is_some(), 1),
        (style.joints, 2),
        (style.start_cap, 16),
        (style.start_inner_arcs, 32),
        (style.start_round, 64),
        (style.end_cap, 256),
        (style.end_inner_arcs, 512),
        (style.end_round, 1024),
    ]
    .iter()
    .filter(|(on, _)| *on)
    .map(|(_, bit)| bit)
    .sum::<i32>();
    let mut pairs = vec![pair(2, &style.name), pair(70, flags), pair(3, &style.description)];
    pairs.extend([pair(62, style.fill.unwrap_or(BYLAYER)), pair(51, "90.0"), pair(52, "90.0"), pair(71, style.elements.len())]);
    for element in &style.elements {
        pairs.extend([pair(49, number(element.offset)), pair(62, element.color), pair(6, &element.linetype)]);
    }
    pairs
}

/// Make the file's multiline styles match the document: each one written afresh, new ones added to the
/// ACAD_MLINESTYLE dictionary, and ones the drawing no longer has taken out (Standard always stays). Notes each
/// style's handle for the multilines that use it.
fn write_mline_styles(file: &mut DxfFile, writer: &mut Writer, document: &Document) {
    if !writer.modern {
        return;
    }
    let Some(mut dictionary) = mline_dictionary(file) else { return };
    let dictionary_handle = file.groups[dictionary].handle().unwrap_or("0").trim().to_string();
    let entries = |file: &DxfFile, dictionary: usize| -> Vec<(String, String)> {
        let pairs = &file.groups[dictionary].0;
        pairs
            .windows(2)
            .filter(|w| w[0].code == 3 && matches!(w[1].code, 350 | 360))
            .map(|w| (w[0].value.trim().to_string(), w[1].value.trim().to_string()))
            .collect()
    };
    // Styles that are gone leave the dictionary and the file.
    for (name, handle) in entries(file, dictionary) {
        let kept = name.eq_ignore_ascii_case("Standard") || document.mline_styles.iter().any(|s| s.name.eq_ignore_ascii_case(&name));
        if !kept {
            let pairs = &mut file.groups[dictionary].0;
            if let Some(at) = pairs.iter().position(|p| p.code == 3 && p.value.trim() == name) {
                pairs.drain(at..at + 2);
            }
            if let Some(at) = object_index(file, &handle) {
                file.groups.remove(at);
            }
            let Some(moved) = mline_dictionary(file) else { return };
            dictionary = moved;
        }
    }
    for style in &document.mline_styles {
        let existing = entries(file, dictionary).into_iter().find(|(name, _)| name.eq_ignore_ascii_case(&style.name)).map(|(_, h)| h);
        let handle = match existing.as_ref().and_then(|h| object_index(file, h)) {
            Some(at) => {
                // Keep what comes before the style's own values (handle, reactors, owner), and write those afresh.
                let group = &mut file.groups[at];
                let body =
                    group.0.iter().position(|p| p.code == 100 && p.value.trim() == "AcDbMlineStyle").map_or(group.0.len(), |i| i + 1);
                group.0.truncate(body);
                group.0.extend(mline_style_pairs(style));
                existing.unwrap_or_default()
            }
            None => {
                let handle = writer.handle();
                let mut pairs = vec![pair(0, "MLINESTYLE"), pair(5, &handle), pair(102, "{ACAD_REACTORS"), pair(330, &dictionary_handle)];
                pairs.extend([pair(102, "}"), pair(330, &dictionary_handle), pair(100, "AcDbMlineStyle")]);
                pairs.extend(mline_style_pairs(style));
                let Some((_, end)) = file.section("OBJECTS") else { return };
                file.groups.insert(end, Group(pairs));
                let entry = &mut file.groups[dictionary].0;
                let tail = entry.iter().position(|p| p.code >= 1000).unwrap_or(entry.len());
                entry.splice(tail..tail, [pair(3, &style.name), pair(350, &handle)]);
                handle
            }
        };
        writer.mline_styles.insert(style.name.to_lowercase(), handle);
    }
    for (name, handle) in entries(file, dictionary) {
        writer.mline_styles.entry(name.to_lowercase()).or_insert(handle);
    }
}

/// The file's multiline styles, in the order the file keeps them.
fn read_mline_styles(file: &DxfFile) -> Vec<MlineStyle> {
    let Some((start, end)) = file.section("OBJECTS") else { return vec![] };
    file.groups[start..end].iter().filter_map(read_mline_style).collect()
}

/// A wipeout's corners from its clip boundary, which is in the terms of its picture: pixels from the top left
/// corner, about pixel centers. A rectangular boundary gives two opposite corners.
fn read_wipeout(group: &Group) -> Option<Shape> {
    let (position, u, v) = (group.point(10)?, group.point(11)?, group.point(12)?);
    let height = group.float(23).unwrap_or(1.0);
    let mut boundary: Vec<Point> = vec![];
    for p in group.0.iter().take_while(|p| p.code < 1000) {
        let value = || p.value.trim().parse::<f64>().ok();
        match p.code {
            14 => boundary.push(pt(value()?, 0.0)),
            24 => boundary.last_mut()?.y = value()?,
            _ => {}
        }
    }
    let at = |b: Point| position + u * (b.x + 0.5) + v * (height - b.y - 0.5);
    let mut corners: Vec<Point> = match boundary.as_slice() {
        [a, b] => vec![at(*a), at(pt(b.x, a.y)), at(*b), at(pt(a.x, b.y))],
        _ => boundary.iter().map(|b| at(*b)).collect(),
    };
    if corners.len() > 1 && corners.first()?.dist(*corners.last()?) < 1e-12 {
        corners.pop();
    }
    (corners.len() >= 3).then_some(Shape::Wipeout { corners })
}

/// The classes AutoCAD needs declared before it reads images, as name, C++ class and proxy flags.
const IMAGE_CLASSES: [(&str, &str, i32, i32); 3] = [
    ("IMAGE", "AcDbRasterImage", 2175, 1),
    ("IMAGEDEF", "AcDbRasterImageDef", 0, 0),
    ("IMAGEDEF_REACTOR", "AcDbRasterImageDefReactor", 1, 0),
];

/// What images need beside their entities: each file's IMAGEDEF in the image dictionary (made if the file has
/// none), a reactor for each image tying it to its definition, and the classes declared.
fn write_images(file: &mut DxfFile, writer: &mut Writer) {
    if writer.image_defs.is_empty() {
        return;
    }
    if let Some((_, end)) = file.section("CLASSES") {
        let missing: Vec<Group> = IMAGE_CLASSES
            .iter()
            .filter(|(name, ..)| !file.groups.iter().any(|g| g.kind() == "CLASS" && g.get(1).is_some_and(|n| n.trim() == *name)))
            .map(|(name, class, flags, entity)| {
                Group(vec![
                    pair(0, "CLASS"),
                    pair(1, name),
                    pair(2, class),
                    pair(3, "ISM"),
                    pair(90, flags),
                    pair(91, 0),
                    pair(280, 0),
                    pair(281, entity),
                ])
            })
            .collect();
        file.groups.splice(end..end, missing);
    }
    let Some((start, _)) = file.section("OBJECTS") else { return };
    let root = start + 1;
    let root_handle = file.groups[root].handle().unwrap_or("0").trim().to_string();
    let dictionary = match image_dictionary(file) {
        Some(at) => at,
        None => {
            let handle = writer.handle();
            let group =
                Group(vec![pair(0, "DICTIONARY"), pair(5, &handle), pair(330, &root_handle), pair(100, "AcDbDictionary"), pair(281, 1)]);
            let pairs = &mut file.groups[root].0;
            let end = pairs.iter().position(|p| p.code >= 1000).unwrap_or(pairs.len());
            pairs.splice(end..end, [pair(3, "ACAD_IMAGE_DICT"), pair(350, &handle)]);
            file.groups.insert(root + 1, group);
            root + 1
        }
    };
    let owner = file.groups[dictionary].handle().unwrap_or("0").trim().to_string();
    let mut objects = vec![];
    let mut entries = vec![];
    for (path, handle, (w, h)) in &writer.image_defs {
        let reactors: Vec<&(String, String, String)> = writer.image_reactors.iter().filter(|(_, _, def)| def == handle).collect();
        let mut pairs = vec![pair(0, "IMAGEDEF"), pair(5, handle), pair(102, "{ACAD_REACTORS"), pair(330, &owner)];
        pairs.extend(reactors.iter().map(|(reactor, _, _)| pair(330, reactor)));
        pairs.extend([pair(102, "}"), pair(330, &owner), pair(100, "AcDbRasterImageDef"), pair(90, 0), pair(1, path)]);
        pairs.extend([
            pair(10, number(*w as f64)),
            pair(20, number(*h as f64)),
            pair(11, "0.01"),
            pair(21, "0.01"),
            pair(280, 1),
            pair(281, 0),
        ]);
        objects.push(Group(pairs));
        for (reactor, image, _) in reactors {
            objects.push(Group(vec![
                pair(0, "IMAGEDEF_REACTOR"),
                pair(5, reactor),
                pair(330, image),
                pair(100, "AcDbRasterImageDefReactor"),
                pair(90, 2),
                pair(330, image),
            ]));
        }
        let stem = std::path::Path::new(path).file_stem().map_or_else(|| "image".to_string(), |s| s.to_string_lossy().into_owned());
        entries.extend([pair(3, format!("{stem}_{handle}")), pair(350, handle)]);
    }
    let pairs = &mut file.groups[dictionary].0;
    let end = pairs.iter().position(|p| p.code >= 1000).unwrap_or(pairs.len());
    pairs.splice(end..end, entries);
    file.groups.splice(dictionary + 1..dictionary + 1, objects);
}

/// The WIPEOUTVARIABLES object that the named objects dictionary lists under ACAD_WIPEOUT_VARS, by its index.
fn wipeout_variables(file: &DxfFile) -> Option<usize> {
    let (start, _) = file.section("OBJECTS")?;
    let pairs = &file.groups[start + 1].0;
    let at = pairs.iter().position(|p| p.code == 3 && p.value.trim() == "ACAD_WIPEOUT_VARS")?;
    object_index(file, pairs.get(at + 1).filter(|p| p.code == 350 || p.code == 360)?.value.trim())
}

/// The frame mode in the drawing's WIPEOUTVARIABLES object (code 70), when it has a valid one.
fn wipeout_frame_mode(file: &DxfFile) -> Option<u8> {
    let at = wipeout_variables(file)?;
    u8::try_from(file.groups[at].subclass("AcDbWipeoutVariables").int(70)?).ok().filter(|mode| *mode <= 2)
}

/// The frame mode AutoCAD reads from WIPEOUTVARIABLES. An existing object is updated; one is made when the mode is
/// not the default or the drawing has wipeouts, so files without either stay as they were.
fn write_wipeout_variables(file: &mut DxfFile, writer: &mut Writer, document: &Document) {
    if let Some(at) = wipeout_variables(file) {
        file.groups[at].set_in("AcDbWipeoutVariables", 70, document.wipeout_frame.to_string());
        return;
    }
    if document.wipeout_frame == 1 && !writer.wipeouts {
        return;
    }
    let Some((start, _)) = file.section("OBJECTS") else { return };
    let root = start + 1;
    let root_handle = file.groups[root].handle().unwrap_or("0").trim().to_string();
    let handle = writer.handle();
    let object = Group(vec![
        pair(0, "WIPEOUTVARIABLES"),
        pair(5, &handle),
        pair(102, "{ACAD_REACTORS"),
        pair(330, &root_handle),
        pair(102, "}"),
        pair(330, &root_handle),
        pair(100, "AcDbWipeoutVariables"),
        pair(70, document.wipeout_frame),
    ]);
    let pairs = &mut file.groups[root].0;
    let end = pairs.iter().position(|p| p.code >= 1000).unwrap_or(pairs.len());
    pairs.splice(end..end, [pair(3, "ACAD_WIPEOUT_VARS"), pair(350, &handle)]);
    file.groups.insert(root + 1, object);
    declare_class(file, WIPEOUT_VARIABLES_CLASS);
}

/// The dictionary that lists the image files.
fn image_dictionary(file: &DxfFile) -> Option<usize> {
    let (start, _) = file.section("OBJECTS")?;
    let pairs = &file.groups[start + 1].0;
    let at = pairs.iter().position(|p| p.code == 3 && p.value.trim() == "ACAD_IMAGE_DICT")?;
    object_index(file, pairs.get(at + 1).filter(|p| p.code == 350 || p.code == 360)?.value.trim())
}

/// Dimension styles the drawing no longer has, unless something left in the file still names one.
fn remove_unused_dimension_styles(file: &mut DxfFile, document: &Document) {
    let named: HashSet<String> = file
        .groups
        .iter()
        .filter(|g| DIMENSION_KINDS.contains(&g.kind()) || matches!(g.kind(), "LEADER" | "TOLERANCE" | "MULTILEADER" | "ACAD_TABLE"))
        .filter_map(|g| g.get(3).map(|n| n.trim().to_lowercase()))
        .collect();
    let keep = |name: &str| {
        let name = name.trim().to_lowercase();
        name == DIMENSION_STYLE.to_lowercase()
            || named.contains(&name)
            || document.dimension_styles.iter().any(|s| s.name.to_lowercase() == name)
    };
    let gone: Vec<usize> =
        file.table_entries("DIMSTYLE").into_iter().filter(|&i| file.groups[i].get(2).is_some_and(|n| !keep(n))).collect();
    for at in gone.into_iter().rev() {
        file.groups.remove(at);
    }
}

/// The dictionary that lists the groups by name.
fn group_dictionary(file: &DxfFile) -> Option<usize> {
    let (start, _) = file.section("OBJECTS")?;
    let pairs = &file.groups[start + 1].0;
    let at = pairs.iter().position(|p| p.code == 3 && p.value.trim() == "ACAD_GROUP")?;
    object_index(file, pairs.get(at + 1).filter(|p| p.code == 350 || p.code == 360)?.value.trim())
}

/// The file's groups whose members are all objects read into the drawing, with where each GROUP object sits.
/// Groups holding something left as it is in the file stay there untouched.
fn read_groups(file: &DxfFile, handles: &HandleMap) -> Vec<(usize, ObjectGroup)> {
    let Some(dictionary) = group_dictionary(file) else { return vec![] };
    let pairs = &file.groups[dictionary].0;
    let entries: Vec<(String, String)> = pairs
        .windows(2)
        .filter(|w| w[0].code == 3 && (w[1].code == 350 || w[1].code == 360))
        .map(|w| (w[0].value.trim().to_string(), w[1].value.trim().to_string()))
        .collect();
    entries
        .into_iter()
        .filter_map(|(name, handle)| {
            let at = object_index(file, &handle)?;
            let group = &file.groups[at];
            if group.kind() != "GROUP" {
                return None;
            }
            let listed: Vec<&str> =
                group.0.iter().take_while(|p| p.code < 1000).filter(|p| p.code == 340).map(|p| p.value.trim()).collect();
            let members: Vec<EntityId> = listed.iter().filter_map(|h| handles.get(&h.to_uppercase()).copied()).collect();
            if members.len() < listed.len() {
                return None;
            }
            let item = ObjectGroup {
                name,
                description: group.text(300).unwrap_or_default(),
                unnamed: group.int(70) == Some(1),
                selectable: group.int(71) != Some(0),
                members,
            };
            Some((at, item))
        })
        .collect()
}

/// The drawing's groups as GROUP objects in the group dictionary, each member pointing back at its groups as
/// AutoCAD expects. Members not in the file any more are left out, and groups left empty go.
fn write_groups(file: &mut DxfFile, writer: &mut Writer, document: &Document) {
    let Some(dictionary) = group_dictionary(file).filter(|_| writer.modern) else { return };
    let owner = file.groups[dictionary].handle().unwrap_or("0").trim().to_string();
    let mut reactors: HashMap<String, Vec<String>> = HashMap::new();
    let mut objects = vec![];
    let mut entries = vec![];
    for group in &document.groups {
        let members: Vec<String> = group.members.iter().filter_map(|id| writer.entity_handles.get(id).cloned()).collect();
        if members.is_empty() {
            continue;
        }
        let handle = writer.handle();
        let mut pairs = vec![
            pair(0, "GROUP"),
            pair(5, &handle),
            pair(102, "{ACAD_REACTORS"),
            pair(330, &owner),
            pair(102, "}"),
            pair(330, &owner),
            pair(100, "AcDbGroup"),
            pair(300, &group.description),
            pair(70, i32::from(group.unnamed)),
            pair(71, i32::from(group.selectable)),
        ];
        for member in &members {
            pairs.push(pair(340, member));
            reactors.entry(member.to_uppercase()).or_default().push(handle.clone());
        }
        objects.push(Group(pairs));
        entries.extend([pair(3, &group.name), pair(350, &handle)]);
    }
    let dictionary_pairs = &mut file.groups[dictionary].0;
    let end = dictionary_pairs.iter().position(|p| p.code >= 1000).unwrap_or(dictionary_pairs.len());
    dictionary_pairs.splice(end..end, entries);
    file.groups.splice(dictionary + 1..dictionary + 1, objects);
    if reactors.is_empty() {
        return;
    }
    for group in &mut file.groups {
        let Some(groups) = group.handle().and_then(|h| reactors.get(&h.trim().to_uppercase())) else { continue };
        if group.kind() == "GROUP" {
            continue;
        }
        let at = group.0.iter().position(|p| p.code == 5).map_or(1, |i| i + 1);
        let pointers =
            std::iter::once(pair(102, "{ACAD_REACTORS")).chain(groups.iter().map(|g| pair(330, g))).chain(std::iter::once(pair(102, "}")));
        group.0.splice(at..at, pointers.collect::<Vec<_>>());
    }
}

/// An empty paper space block and its record, for a new layout. Gives the record's handle.
fn new_paper_block(file: &mut DxfFile, writer: &mut Writer, name: &str) -> String {
    let record = writer.handle();
    let owner = file
        .table("BLOCK_RECORD")
        .and_then(|(table, _)| file.groups[table].handle().map(|h| h.trim().to_string()))
        .unwrap_or_else(|| "0".into());
    if let Some((_, end)) = file.table("BLOCK_RECORD") {
        let entry = vec![
            pair(0, "BLOCK_RECORD"),
            pair(5, &record),
            pair(330, owner),
            pair(100, "AcDbSymbolTableRecord"),
            pair(100, "AcDbBlockTableRecord"),
            pair(2, name),
            pair(340, "0"),
            pair(70, 0),
            pair(280, 1),
            pair(281, 0),
        ];
        file.groups.insert(end, Group(entry));
    }
    let begin = vec![
        pair(0, "BLOCK"),
        pair(5, writer.handle()),
        pair(330, &record),
        pair(100, "AcDbEntity"),
        pair(67, 1),
        pair(8, DEFAULT_LAYER),
        pair(100, "AcDbBlockBegin"),
        pair(2, name),
        pair(70, 0),
    ]
    .into_iter()
    .chain(point_pairs(10, pt(0.0, 0.0)))
    .chain([pair(3, name), pair(1, "")])
    .collect();
    let finish = vec![
        pair(0, "ENDBLK"),
        pair(5, writer.handle()),
        pair(330, &record),
        pair(100, "AcDbEntity"),
        pair(67, 1),
        pair(8, DEFAULT_LAYER),
        pair(100, "AcDbBlockEnd"),
    ];
    if let Some((_, end)) = file.section("BLOCKS") {
        file.groups.splice(end..end, [Group(begin), Group(finish)]);
    }
    record
}

/// The document written into its source file, or into a new one.
pub fn dxf_from_document(document: &Document) -> DxfFile {
    let mut file = document.source.clone().unwrap_or_else(DxfFile::template);
    // Blocks changed in the block editor are defined afresh from the drawing.
    for name in &document.redefined_blocks {
        remove_block(&mut file, name);
    }
    let block_names =
        file.table_entries("BLOCK_RECORD").into_iter().filter_map(|i| file.groups[i].get(2)).map(|n| n.trim().to_uppercase()).collect();
    let mut writer = Writer {
        modern: file.modern(),
        r2018: file.version().as_str() >= "AC1032",
        r2004: file.version().as_str() >= "AC1018",
        r2007: file.version().as_str() >= "AC1021",
        mleader_ids: None,
        next_handle: file.next_free_handle(),
        model_space: find_model_space(&file),
        block_names,
        blocks: vec![],
        blocks_to_define: vec![],
        layer_handles: HashMap::new(),
        next_viewport: 2,
        entity_handles: HashMap::new(),
        image_defs: vec![],
        image_reactors: vec![],
        wipeouts: false,
        regions: 0,
        mline_styles: HashMap::new(),
        table_style: None,
        lineweight: lineweight::BYLAYER,
        linetype_scale: 1.0,
        text_styles: text_style_fonts(&file)
            .into_iter()
            .filter_map(|(key, font)| {
                let name = file
                    .table_entries("STYLE")
                    .into_iter()
                    .filter_map(|i| file.groups[i].text(2))
                    .find(|n| n.trim().to_lowercase() == key)?;
                Some((key, (name.trim().to_string(), font)))
            })
            .collect(),
    };
    for block in document.redefined_blocks.iter().filter_map(|n| document.blocks.get(&n.to_uppercase())) {
        if writer.block_names.insert(block.0.name.to_uppercase()) {
            writer.blocks_to_define.push(block.clone());
        }
    }
    write_layers(&mut file, &mut writer, document);
    writer.table_style = standard_table_style(&file).map(|handle| (handle, None));
    writer.layer_handles = file
        .table_entries("LAYER")
        .into_iter()
        .filter_map(|i| Some((file.groups[i].get(2)?.trim().to_lowercase(), file.groups[i].handle()?.trim().to_string())))
        .collect();

    let dimension_style = file
        .table_entries("DIMSTYLE")
        .into_iter()
        .filter_map(|i| file.groups[i].text(2))
        .find(|n| n.trim().eq_ignore_ascii_case(DIMENSION_STYLE))
        .or_else(|| file.table_entries("DIMSTYLE").into_iter().find_map(|i| file.groups[i].text(2)))
        .unwrap_or_else(|| DIMENSION_STYLE.into());
    let style_names = write_dimension_styles(&mut file, &mut writer, document, &dimension_style);
    write_mleader_styles(&mut file, &mut writer, document);
    write_mline_styles(&mut file, &mut writer, document);
    write_text_styles(&mut file, &mut writer, document);
    let mut linetypes: BTreeMap<String, String> = BTreeMap::new();
    let styles = (&style_names, dimension_style.as_str());
    let owner = writer.model_space.clone();
    let mut entity_groups = write_entities(
        &mut file,
        &mut writer,
        &mut linetypes,
        (document.space_entities(None), &document.kept_xdata, &document.object_plot_styles),
        &owner,
        styles,
    );
    entity_groups.extend(write_layouts(&mut file, &mut writer, &mut linetypes, document, styles));
    // Blocks the file lacks, with their entities (which may insert further blocks the file lacks).
    while let Some(block) = writer.blocks_to_define.pop() {
        let record = writer.handle();
        let mut begin = vec![pair(0, "BLOCK"), pair(5, writer.handle())];
        if writer.modern {
            begin.extend([pair(330, &record), pair(100, "AcDbEntity")]);
        }
        begin.push(pair(8, DEFAULT_LAYER));
        if writer.modern {
            begin.push(pair(100, "AcDbBlockBegin"));
        }
        // An external reference names its drawing and holds nothing of its own.
        let flags = if block.0.xref.is_some() { BLOCK_XREF | BLOCK_EXTERNAL } else { 0 };
        begin.extend([pair(2, &block.0.name), pair(70, flags)]);
        begin.extend(point_pairs(10, block.0.base));
        begin.extend([pair(3, &block.0.name), pair(1, block.0.xref.clone().unwrap_or_default())]);
        let mut groups = vec![Group(begin)];
        let own = if block.0.xref.is_some() { &[][..] } else { &block.0.entities[..] };
        for child in own {
            let linetype = if child.linetype == BYLAYER_LINETYPE {
                BYLAYER_LINETYPE.to_string()
            } else {
                linetype_name(&mut file, &mut writer, &child.linetype)
            };
            groups.extend(writer.entity(child, &linetype, &dimension_style, &record));
        }
        for definition in &block.0.attribute_defs {
            let Shape::Text { position, text, height, rotation, align, .. } = &definition.text else { continue };
            let value = encode_percent_codes(text);
            let mut pairs = writer.text(
                "ATTDEF",
                &record,
                &definition.layer,
                definition.color,
                BYLAYER_LINETYPE,
                *position,
                &value,
                *height,
                *rotation,
                *align,
            );
            if writer.modern {
                pairs.push(pair(100, "AcDbAttributeDefinition"));
            }
            let asked = if definition.prompt.is_empty() { &definition.tag } else { &definition.prompt };
            pairs.extend([pair(3, asked), pair(2, &definition.tag), pair(70, if definition.invisible { 1 } else { 0 })]);
            if let Some(at) = pairs.iter().position(|p| p.code == 74) {
                let vertical = pairs.remove(at);
                pairs.push(vertical);
            }
            groups.push(Group(pairs));
        }
        let mut finish = vec![pair(0, "ENDBLK"), pair(5, writer.handle())];
        if writer.modern {
            finish.extend([pair(330, &record), pair(100, "AcDbEntity"), pair(8, DEFAULT_LAYER), pair(100, "AcDbBlockEnd")]);
        } else {
            finish.push(pair(8, DEFAULT_LAYER));
        }
        groups.push(Group(finish));
        writer.blocks.push(DimensionBlock { name: block.0.name.clone(), record, groups });
    }

    match file.section("ENTITIES") {
        Some((_, end)) => {
            file.groups.splice(end..end, entity_groups);
        }
        None => {
            let at = file.section("OBJECTS").map_or_else(|| file.groups.len().saturating_sub(1), |(start, _)| start);
            let mut section = vec![Group(vec![pair(0, "SECTION"), pair(2, "ENTITIES")])];
            section.extend(entity_groups);
            section.push(Group(vec![pair(0, "ENDSEC")]));
            file.groups.splice(at..at, section);
        }
    }

    let block_table = file.table("BLOCK_RECORD").and_then(|(table, _)| file.groups[table].handle().map(|h| h.trim().to_string()));
    for DimensionBlock { name, record: record_handle, groups } in std::mem::take(&mut writer.blocks) {
        if writer.modern
            && let Some((_, end)) = file.table("BLOCK_RECORD")
        {
            let owner = block_table.clone().unwrap_or_else(|| "0".into());
            let entry = vec![
                pair(0, "BLOCK_RECORD"),
                pair(5, &record_handle),
                pair(330, owner),
                pair(100, "AcDbSymbolTableRecord"),
                pair(100, "AcDbBlockTableRecord"),
                pair(2, &name),
                pair(340, "0"),
                pair(70, 0),
                pair(280, 1),
                pair(281, 0),
            ];
            file.groups.insert(end, Group(entry));
        }
        if let Some((_, end)) = file.section("BLOCKS") {
            file.groups.splice(end..end, groups);
        }
    }
    write_groups(&mut file, &mut writer, document);
    write_images(&mut file, &mut writer);
    if writer.wipeouts {
        declare_class(&mut file, WIPEOUT_CLASS);
    }
    if writer.regions > 0 {
        register_application(&mut file, &mut writer, LINEWRIGHT_APPLICATION);
    }
    if writer.modern {
        write_wipeout_variables(&mut file, &mut writer, document);
    }
    write_table_style(&mut file, &writer);
    if let Some(units) = document.units {
        file.set_header("$INSUNITS", 70, units.to_string());
    }
    file.set_header("$LUPREC", 70, document.precision.to_string());
    if writer.modern {
        file.set_header("$CELWEIGHT", 370, document.current_lineweight.to_string());
        file.set_header("$LWDEFAULT", 370, document.default_lineweight.to_string());
        file.set_header("$LWDISPLAY", 290, i32::from(document.show_lineweights).to_string());
        file.set_header("$LTSCALE", 40, number(document.ltscale));
        file.set_header("$PSLTSCALE", 70, i32::from(document.psltscale).to_string());
        file.set_header("$CELTSCALE", 40, number(document.celtscale));
        if let Some(at) = model_layout_object(&file) {
            set_plot_settings(&mut file.groups[at], &document.model_plot);
        }
    }
    for name in &document.purged_blocks {
        remove_block(&mut file, name);
    }
    remove_unused_dimension_styles(&mut file, document);
    let seed = format!("{:X}", writer.next_handle);
    file.set_header("$HANDSEED", 5, seed);
    file
}

#[cfg(test)]
mod tests;
