//! Drawing the canvas with egui: grid, entities, the selection, the rubber band and the drafting cursor.

use eframe::egui::epaint::{PathShape, TextShape};
use eframe::egui::{self, Color32, FontFamily, FontId, Pos2, Rect, Shape, Stroke, StrokeKind, Vec2, pos2, vec2};

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use super::aci::color_rgb;
use super::gpu::{Gpu, Placement};
use super::shapes::{Styled, TextSpec, flatten, resolve_color};
use super::view::{Camera, grid_spacing, viewport_camera};
use crate::fonts::display_text;
use crate::model::document::{Document, EntityStore, Layout};
use crate::model::entities::{Entity, EntityId, Shape as EntityShape};
use crate::model::entities::{HAlign, TextAlign, VAlign};
use crate::model::geometry::pt;
use crate::model::geometry::{Point, direction};
use crate::model::index::Bounds;
use crate::model::lineweight;
use crate::model::mtext::{RichText, RunStyle, layout};
use crate::model::parts::{BOLD_OFFSET, TextLook};
use crate::model::styles::{BYLAYER_LINETYPE, scaled_pattern};
use crate::model::ucs::Ucs;
use crate::model::viewport::{frame_corners, viewport_scale};
use crate::snap::osnap::SnapKind;
use crate::ui::controller::Controller;
use crate::ui::theme;
use crate::ui::theme::drawing_family;

const MAJOR_GRID_EVERY: f64 = 10.0;
const MIN_ENTITY_PIXELS: f64 = 1.0; // smaller than this is not drawn at all
const MIN_DASH_PIXELS: f64 = 1.0; // a dash or gap shorter than this makes the line solid
const MIN_TEXT_PIXELS: f64 = 3.0;
const MAX_TEXT_PIXELS: f64 = 2000.0;
const CHORD_PIXELS: f64 = 0.25; // how far a curve's chords may stray from it on screen
const CLIP_MARGIN: f32 = 8.0;
const SELECTED_WIDTH: f32 = 2.0;
const ACTIVE_VIEWPORT_WIDTH: f32 = 2.0;
const POINT_RADIUS: f32 = 1.5;
const GRIP_HALF: f32 = 4.5;
const PREVIEW_DASHES: [f32; 2] = [4.0, 4.0];
const MARKER_HALF: f32 = 6.0;
const PICKBOX_HALF: f32 = 5.0;
const CROSSHAIR_ARM_FRACTION: f32 = 0.04; // of the drawing area's width, each way from the cursor
const AXES_LENGTH: f32 = 42.0;
const AXES_MARGIN: Vec2 = vec2(48.0, 44.0); // from the left and bottom edges
const BOX_FILL_ALPHA: f32 = 0.12;

/// How the canvas maps the drawing to the screen.
#[derive(Clone, Copy)]
pub struct Screen<'a> {
    pub rect: Rect,
    pub camera: Camera,
    /// The drawing's fonts that are installed and registered with egui, by lower-case name.
    pub fonts: &'a HashSet<String>,
    /// A layout is open: the drawing sits on white paper, where white lines show black.
    pub paper: bool,
    /// Pixels a millimeter of lineweight shows as, when lineweights show (LWDISPLAY).
    pub lineweight_pixels: Option<f64>,
    /// PSLTSCALE: model space seen through a viewport keeps its dashes as long on the sheet as paper space ones.
    pub psltscale: bool,
    /// What linetype lengths are divided by before they are drawn: the viewport's scale when PSLTSCALE divides them
    /// through a viewport, else 1.
    pub linetype_divisor: f64,
}

impl<'a> Screen<'a> {
    /// How a document shows in a rectangle of the window. On a layout lineweights grow as you zoom, as on paper;
    /// in model space they keep their width on screen.
    pub fn new(rect: Rect, camera: Camera, fonts: &'a HashSet<String>, document: &Document) -> Screen<'a> {
        let paper = document.shown_layout().is_some();
        let lineweight_pixels = document
            .show_lineweights
            .then(|| if paper { camera.scale / crate::io::dxf::mm_per_unit(document) } else { lineweight::model_pixels(1.0) as f64 });
        Screen { rect, camera, fonts, paper, lineweight_pixels, psltscale: document.psltscale, linetype_divisor: 1.0 }
    }

    /// How wide a part's lines show: one pixel, or its lineweight when lineweights show.
    fn line_width(&self, style: &Styled, document: &Document) -> f32 {
        let Some(per_mm) = self.lineweight_pixels else { return 1.0 };
        let layer = document.layer(&style.layer).map(|l| l.lineweight);
        ((lineweight::resolve_mm(style.lineweight, layer, document.default_lineweight) * per_mm) as f32).max(1.0)
    }

    pub fn pos(&self, p: Point) -> Pos2 {
        let (x, y) = self.camera.to_screen(p);
        self.rect.center() + vec2(x as f32, y as f32)
    }

    pub fn world(&self, pos: Pos2) -> Point {
        let offset = pos - self.rect.center();
        self.camera.to_world(offset.x as f64, offset.y as f64)
    }
}

struct Pen<'a> {
    color: Color32,
    width: f32,
    dashes: &'a [f32],
}

/// The part of a segment inside the rectangle (Liang–Barsky), or None.
fn clip(a: Pos2, b: Pos2, area: Rect) -> Option<(Pos2, Pos2)> {
    let d = b - a;
    let (mut t0, mut t1) = (0.0f32, 1.0f32);
    for (p, q) in [(-d.x, a.x - area.min.x), (d.x, area.max.x - a.x), (-d.y, a.y - area.min.y), (d.y, area.max.y - a.y)] {
        if p == 0.0 {
            if q < 0.0 {
                return None;
            }
        } else {
            let t = q / p;
            if p < 0.0 {
                t0 = t0.max(t);
            } else {
                t1 = t1.min(t);
            }
            if t0 > t1 {
                return None;
            }
        }
    }
    Some((a + d * t0, a + d * t1))
}

/// Split runs of points into dashes, carrying the pattern on from one segment to the next.
fn dashed(run: &[Pos2], pattern: &[f32], out: &mut Vec<[Pos2; 2]>) {
    let total: f32 = pattern.iter().sum();
    if total <= 0.0 {
        return;
    }
    let (mut index, mut left) = (0usize, pattern[0]);
    for pair in run.windows(2) {
        let (mut from, to) = (pair[0], pair[1]);
        let mut remaining = from.distance(to);
        if remaining > 1e5 {
            continue; // never cut an enormous segment into dashes
        }
        let direction = if remaining > 0.0 { (to - from) / remaining } else { Vec2::ZERO };
        while remaining > 0.0 {
            let step = left.min(remaining);
            let next = from + direction * step;
            if index % 2 == 0 {
                out.push([from, next]);
            }
            from = next;
            remaining -= step;
            left -= step;
            if left <= 0.0 {
                index = (index + 1) % pattern.len();
                left = pattern[index];
            }
        }
    }
}

fn stroke_path(shapes: &mut Vec<Shape>, screen: &Screen, path: &[Point], pen: &Pen) {
    let area = screen.rect.expand(CLIP_MARGIN);
    let points: Vec<Pos2> = path.iter().map(|p| screen.pos(*p)).collect();
    let mut runs: Vec<Vec<Pos2>> = vec![];
    for pair in points.windows(2) {
        let Some((a, b)) = clip(pair[0], pair[1], area) else { continue };
        match runs.last_mut() {
            Some(run) if run.last() == Some(&a) => run.push(b),
            _ => runs.push(vec![a, b]),
        }
    }
    let stroke = Stroke::new(pen.width, pen.color);
    for run in runs {
        if pen.dashes.is_empty() {
            shapes.push(Shape::Path(PathShape::line(run, stroke)));
        } else {
            let mut dashes = vec![];
            dashed(&run, pen.dashes, &mut dashes);
            shapes.extend(dashes.into_iter().map(|segment| Shape::line_segment(segment, stroke)));
        }
    }
}

/// How long a piece of text runs in drawing units, as it will be drawn.
fn text_width(ctx: &egui::Context, screen: &Screen, text: &str, height: f64, font: &str, look: &TextLook) -> f64 {
    let truetype = screen.fonts.contains(&font.to_lowercase());
    if !truetype && let Some(font) = crate::shx::text_font(font) {
        return crate::shx::line_strokes(&font, &display_text(text), height, look.width_factor).1;
    }
    let family = if truetype { drawing_family(font) } else { FontFamily::Proportional };
    let size = rounded_text_size((height * screen.camera.scale).max(1.0));
    let galley = ctx.fonts(|fonts| fonts.layout_no_wrap(display_text(text), FontId::new(size, family), Color32::WHITE));
    galley.size().x as f64 / screen.camera.scale * (height * screen.camera.scale / size as f64) * look.width_factor
}

/// Text with runs of its own looks: each run drawn on its own, with its underlines and fraction bars.
fn draw_rich_text(shapes: &mut Vec<Shape>, ctx: &egui::Context, screen: &Screen, spec: &TextSpec, rich: &RichText, pen: &Pen) {
    let run_font = |style: &RunStyle| style.font.clone().unwrap_or_else(|| spec.font.clone());
    let run_look = |style: &RunStyle| TextLook { bold: style.bold, italic: style.italic, ..spec.look };
    let measure = |text: &str, height: f64, style: &RunStyle| text_width(ctx, screen, text, height, &run_font(style), &run_look(style));
    let (placed, rules) = layout(rich, spec.height, spec.rotation, |widths| spec.line_origins(widths), &measure);
    for run in placed {
        let color = match (run.style.color.and_then(color_rgb), pen.dashes.is_empty() && pen.color != theme::SELECTION) {
            (Some(rgb), true) => on_paper(color32(rgb), screen.paper),
            _ => pen.color,
        };
        let piece = TextSpec {
            origin: run.origin,
            rotation: spec.rotation,
            height: run.height,
            text: run.text,
            align: TextAlign { h: HAlign::Left, v: VAlign::Baseline },
            line_spacing: 1.0,
            font: run_font(&run.style),
            look: run_look(&run.style),
            rich: None,
        };
        draw_text(shapes, ctx, screen, &piece, &Pen { color, width: pen.width, dashes: &[] });
    }
    for [a, b] in rules {
        shapes.push(Shape::line_segment([screen.pos(a), screen.pos(b)], Stroke::new(pen.width, pen.color)));
    }
}

fn draw_text(shapes: &mut Vec<Shape>, ctx: &egui::Context, screen: &Screen, spec: &TextSpec, pen: &Pen) {
    if let Some(rich) = &spec.rich {
        draw_rich_text(shapes, ctx, screen, spec, rich, pen);
        return;
    }
    // Bold is the letters drawn twice, a little apart.
    if spec.look.bold {
        let offset = direction(spec.rotation) * (spec.height * BOLD_OFFSET);
        for origin in [spec.origin, spec.origin + offset] {
            let piece = TextSpec { origin, look: TextLook { bold: false, ..spec.look }, ..spec.clone() };
            draw_text(shapes, ctx, screen, &piece, pen);
        }
        return;
    }
    let color = pen.color;
    let pixels = spec.height * screen.camera.scale;
    if !text_shows(screen, spec) {
        return;
    }
    let truetype = screen.fonts.contains(&spec.font.to_lowercase());
    if !truetype && let Some(font) = crate::shx::text_font(&spec.font) {
        let stroke = Stroke::new(pen.width, color);
        for points in crate::shx::text_strokes(&font, spec) {
            shapes.push(Shape::line(points.into_iter().map(|p| screen.pos(p)).collect(), stroke));
        }
        return;
    }
    let size = rounded_text_size(pixels);
    let family = if truetype { drawing_family(&spec.font) } else { FontFamily::Proportional };
    // Each line is laid out on its own, so lines step down by the drawing's spacing and line up as aligned.
    let galleys: Vec<_> = spec
        .text
        .split('\n')
        .map(|line| ctx.fonts(|fonts| fonts.layout_no_wrap(display_text(line), FontId::new(size, family.clone()), color)))
        .collect();
    let widths: Vec<f64> = galleys.iter().map(|g| g.size().x as f64 / screen.camera.scale * spec.look.width_factor).collect();
    let angle = -(spec.rotation.to_radians() as f32);
    let rotate = |v: Vec2| vec2(v.x * angle.cos() - v.y * angle.sin(), v.x * angle.sin() + v.y * angle.cos());
    for (galley, origin) in galleys.into_iter().zip(spec.line_origins(&widths)) {
        let baseline = galley.rows.first().and_then(|row| row.glyphs.first()).map_or(size * 0.8, |glyph| glyph.pos.y);
        let top_left = screen.pos(origin) + rotate(vec2(0.0, -baseline));
        if spec.look.is_plain() {
            shapes.push(Shape::Text(TextShape::new(top_left, galley, color).with_angle(angle)));
        } else {
            // Stretched or leaning letters: the laid out glyphs reshaped around the baseline's start.
            let (stretch, shear) = (spec.look.width_factor as f32, spec.look.shear() as f32);
            // A galley's texture coordinates are in pixels of the font atlas until it is drawn as text.
            let atlas = ctx.fonts(|fonts| fonts.font_image_size());
            for row in &galley.rows {
                let mut mesh = row.visuals.mesh.clone();
                for vertex in &mut mesh.vertices {
                    let at = vertex.pos.to_vec2();
                    let above = baseline - at.y;
                    let local = vec2((at.x + above * shear) * stretch, -above);
                    vertex.pos = screen.pos(origin) + rotate(local);
                    vertex.color = color;
                    vertex.uv = pos2(vertex.uv.x / atlas[0] as f32, vertex.uv.y / atlas[1] as f32);
                }
                shapes.push(Shape::mesh(mesh));
            }
        }
    }
}

/// Fill an area whose loops may hold holes, as one-pixel scanlines across it (inside by the even-odd rule).
fn fill_area(shapes: &mut Vec<Shape>, screen: &Screen, loops: &[Vec<Point>], color: Color32) {
    let loops: Vec<Vec<Pos2>> = loops.iter().map(|l| l.iter().map(|p| screen.pos(*p)).collect()).collect();
    let (top, bottom) = loops.iter().flatten().fold((f32::INFINITY, f32::NEG_INFINITY), |(t, b), p| (t.min(p.y), b.max(p.y)));
    let (top, bottom) = (top.max(screen.rect.top()), bottom.min(screen.rect.bottom()));
    let stroke = Stroke::new(1.0_f32, color);
    let mut y = top.floor() + 0.5;
    while y <= bottom {
        let mut crossings: Vec<f32> = vec![];
        for points in &loops {
            for i in 0..points.len() {
                let (a, b) = (points[i], points[(i + 1) % points.len()]);
                if (a.y <= y) != (b.y <= y) {
                    crossings.push(a.x + (y - a.y) * (b.x - a.x) / (b.y - a.y));
                }
            }
        }
        crossings.sort_by(f32::total_cmp);
        for pair in crossings.as_chunks::<2>().0 {
            let (x0, x1) = (pair[0].max(screen.rect.left() - 1.0), pair[1].min(screen.rect.right() + 1.0));
            if x1 > x0 {
                shapes.push(Shape::line_segment([pos2(x0, y), pos2(x1, y)], stroke));
            }
        }
        y += 1.0;
    }
}

/// Text sizes are rounded so zooming does not fill the font atlas with a size for every step: whole pixels when
/// small, steps of 3% when large.
fn rounded_text_size(pixels: f64) -> f32 {
    (if pixels < 40.0 { pixels.round() } else { ((pixels.ln() / 0.03).round() * 0.03).exp() }) as f32
}

/// How an entity is drawn: in its own colors and linetypes, in one pen (the selection), or in its own colors
/// dashed (the rubber band).
enum Look<'a> {
    Own,
    Pen(&'a Pen<'a>),
    Preview,
}

/// The dashes a part is drawn with on screen, in pixels: its linetype's pattern at the drawing's LTSCALE and the
/// part's own linetype scale, divided as PSLTSCALE says through a viewport. Empty draws the part solid, as when
/// its dashes would be shorter than a pixel.
fn screen_dashes(screen: &Screen, document: &Document, style: &Styled) -> Vec<f32> {
    let per_unit = document.ltscale * style.linetype_scale * screen.camera.scale / screen.linetype_divisor;
    scaled_pattern(&effective_linetype(style, document), per_unit, MIN_DASH_PIXELS).into_iter().map(|d| d as f32).collect()
}

/// Text put off while a picture of the drawing is made: laid out afresh every frame the picture is shown, since
/// laid out text points into the font atlas, which egui makes anew now and then.
#[derive(Clone)]
struct LaterText {
    spec: TextSpec,
    color: Color32,
    width: f32,
    dashes: Vec<f32>,
}

fn draw_entity(shapes: &mut Vec<Shape>, ctx: &egui::Context, screen: &Screen, document: &Document, entity: &Entity, look: &Look) {
    draw_entity_to(shapes, ctx, screen, document, entity, look, None);
}

/// Draw an entity. With `later`, its text is not drawn but noted there, a `Shape::Noop` holding its place among
/// the shapes. Tells whether it covers an area (a fill, a mask or an image), hiding what was drawn before it.
fn draw_entity_to(
    shapes: &mut Vec<Shape>,
    ctx: &egui::Context,
    screen: &Screen,
    document: &Document,
    entity: &Entity,
    look: &Look,
    mut later: Option<&mut Vec<LaterText>>,
) -> bool {
    let mut covers = false;
    // A construction line has no ends: only the part on screen is drawn, cut in drawing units where it is precise.
    let clipped_xline = match entity.shape {
        EntityShape::XLine { base, direction, ray } => {
            let area = crate::model::index::Bounds::from_corners(screen.world(screen.rect.min), screen.world(screen.rect.max));
            let Some((start, end)) =
                crate::model::xline::clipped(base, direction, ray, &area.grow(CLIP_MARGIN as f64 / screen.camera.scale))
            else {
                return false;
            };
            Some(entity.with_shape(EntityShape::Line { start, end }))
        }
        _ => None,
    };
    let entity = clipped_xline.as_ref().unwrap_or(entity);
    if let EntityShape::Image { path, position, u, v, .. } = &entity.shape
        && matches!(look, Look::Own)
    {
        draw_image(shapes, ctx, screen, document, path, crate::model::entities::image_corners(*position, *u, *v));
        covers = true;
    }
    for (style, parts) in flatten(entity, CHORD_PIXELS / screen.camera.scale) {
        // Inside a block, entities on a layer that is turned off stay hidden.
        if document.layer(&style.layer).is_some_and(|layer| !layer.shown()) {
            continue;
        }
        let own_dashes: Vec<f32>;
        let own;
        let pen = match look {
            Look::Pen(pen) => *pen,
            Look::Own => {
                own_dashes = screen_dashes(screen, document, &style);
                own = Pen {
                    color: on_paper(color32(resolve_color(&style, document)), screen.paper),
                    width: screen.line_width(&style, document),
                    dashes: &own_dashes,
                };
                &own
            }
            Look::Preview => {
                own = Pen { color: color32(resolve_color(&style, document)), width: 1.0, dashes: &PREVIEW_DASHES };
                &own
            }
        };
        covers |= !(parts.masks.is_empty() && parts.fills.is_empty() && parts.areas.is_empty() && parts.gradients.is_empty());
        // A wipeout hides what is under it in the background's color; its frame shows in its own, unless WIPEOUTFRAME is 0.
        for mask in &parts.masks {
            if !matches!(look, Look::Preview) {
                fill_area(shapes, screen, std::slice::from_ref(mask), if screen.paper { Color32::WHITE } else { theme::CANVAS });
            }
            if document.wipeout_frame != 0 {
                let frame: Vec<Point> = mask.iter().chain(mask.first()).copied().collect();
                stroke_path(shapes, screen, &frame, pen);
            }
        }
        for path in &parts.paths {
            stroke_path(shapes, screen, path, pen);
        }
        for triangle in &parts.fills {
            shapes.push(Shape::convex_polygon(triangle.iter().map(|p| screen.pos(*p)).collect(), pen.color, Stroke::NONE));
        }
        for area in &parts.areas {
            fill_area(shapes, screen, area, pen.color);
        }
        for (loops, gradient) in &parts.gradients {
            // A selected or previewed gradient shows in its pen, as a solid area does.
            if matches!(look, Look::Own) {
                let step = 1.0 / screen.camera.scale;
                for (a, b, color) in crate::model::parts::gradient_strips(loops, gradient, step) {
                    if let Some((a, b)) = clip(screen.pos(a), screen.pos(b), screen.rect.expand(CLIP_MARGIN)) {
                        shapes.push(Shape::line_segment([a, b], Stroke::new(1.5_f32, color32(color))));
                    }
                }
            } else {
                fill_area(shapes, screen, loops, pen.color);
            }
        }
        for point in &parts.points {
            shapes.push(Shape::circle_filled(screen.pos(*point), POINT_RADIUS, pen.color));
        }
        for spec in &parts.texts {
            match later.as_deref_mut() {
                None => draw_text(shapes, ctx, screen, spec, pen),
                // Text too small or too large to show needs no place kept for it.
                Some(_) if spec.rich.is_none() && !text_shows(screen, spec) => {}
                Some(later) => {
                    later.push(LaterText { spec: spec.clone(), color: pen.color, width: pen.width, dashes: pen.dashes.to_vec() });
                    shapes.push(Shape::Noop);
                }
            }
        }
    }
    covers
}

/// Whether plain text is of a size that is drawn at all, and has anything to draw.
fn text_shows(screen: &Screen, spec: &TextSpec) -> bool {
    (MIN_TEXT_PIXELS..=MAX_TEXT_PIXELS).contains(&(spec.height * screen.camera.scale)) && !spec.text.trim().is_empty()
}

/// One piece of a picture of the drawing, in drawing order.
enum Piece {
    /// Lines, arcs and filled areas, already cut into the triangles the screen is painted with. `first` is where
    /// its corners start among those of all the picture's meshes, and `covers` whether it fills any area, hiding
    /// what was drawn before.
    Mesh {
        mesh: Arc<egui::Mesh>,
        first: u32,
        covers: bool,
    },
    Text(LaterText),
    /// A shape kept as it is: an image, with a texture of its own.
    Shape(Shape),
}

/// What a picture of the drawing depends on. While all of it stays the same, so does the picture.
#[derive(Clone, PartialEq)]
struct PictureKey {
    revision: u64,
    /// The selection's own revision, for the picture of the selected objects; 0 for the drawing's.
    selection: u64,
    look: u64,
    rect: Rect,
    camera: Camera,
    paper: bool,
    lineweight_pixels: Option<f64>,
    psltscale: bool,
    linetype_divisor: f64,
    pixels_per_point: f32,
    fonts: u64,
}

impl PictureKey {
    /// The key of a picture of the objects of the store at `revision`, drawn as `screen` shows them. `selection`
    /// is the selection's revision for the picture of the selected objects, else 0, and `also` a number for
    /// anything else the picture depends on.
    fn of(ctx: &egui::Context, screen: &Screen, document: &Document, revision: u64, selection: u64, also: u64) -> PictureKey {
        PictureKey {
            revision,
            selection,
            look: look_of(document) ^ also,
            rect: screen.rect,
            camera: screen.camera,
            paper: screen.paper,
            lineweight_pixels: screen.lineweight_pixels,
            psltscale: screen.psltscale,
            linetype_divisor: screen.linetype_divisor,
            pixels_per_point: ctx.pixels_per_point(),
            fonts: fonts_of(screen),
        }
    }

    /// Whether the other is the same picture seen from somewhere else: only the view was panned or zoomed.
    fn moved_only(&self, other: &PictureKey) -> bool {
        PictureKey { camera: other.camera, ..self.clone() } == *other
    }
}

/// A picture of some of the drawing's objects as one view shows them, kept from frame to frame: most frames only
/// move the crosshair over it.
struct Picture {
    /// A number of its own, by which the graphics card knows its copy.
    id: u64,
    key: PictureKey,
    pieces: Vec<Piece>,
    /// Large enough to be drawn from the graphics card's own copy rather than handed to egui every frame.
    on_card: bool,
    /// It took long enough to make that a pan or zoom shows it moved along, and makes it anew once the view rests.
    slow: bool,
}

impl Picture {
    fn meshes(&self) -> Vec<&egui::Mesh> {
        self.pieces.iter().filter_map(|piece| if let Piece::Mesh { mesh, .. } = piece { Some(mesh.as_ref()) } else { None }).collect()
    }
}

/// One kept picture, and the view last asked of it when that was not the one it was made for, with when it was
/// first asked.
#[derive(Default)]
struct Kept {
    picture: Option<Arc<Picture>>,
    moved: Option<(Camera, f64)>,
    /// The frame it was last asked for: one not asked for any more, as of a viewport that is gone, is let go.
    asked: u64,
}

/// The pictures kept between frames, by what they are of, and the graphics card's copies of them.
#[derive(Clone, Default)]
struct Pictures {
    kept: Arc<Mutex<HashMap<(&'static str, u64), Kept>>>,
    gpu: Arc<Mutex<Gpu>>,
}

impl Pictures {
    fn of(ctx: &egui::Context) -> Pictures {
        ctx.data_mut(|d| d.get_temp_mut_or_default::<Pictures>(egui::Id::new("pictures")).clone())
    }
}

/// The picture of all the open space's objects, that of the selected ones drawn over it, and that of model space
/// through a layout's viewport, of which there is one for each viewport, told apart by the viewport's id.
const DRAWING_PICTURE: (&str, u64) = ("drawing", 0);
const SELECTION_PICTURE: (&str, u64) = ("selection", 0);
const VIEWPORT_PICTURE: &str = "viewport";
/// Frames a picture may go unasked for before it is let go.
const FRAMES_PICTURES_ARE_KEPT: u64 = 2;
/// Fewer objects than this are drawn on the one thread; more are shared among several.
const OBJECTS_PER_THREAD: usize = 4000;
/// A picture of more corners than this is drawn from the graphics card's own copy.
const CORNERS_FOR_CARD: usize = 100_000;
/// A picture that took longer than this to make is not made anew on every frame of a pan or zoom.
const SLOW_PICTURE_SECONDS: f64 = 0.03;
/// How long the view rests before a slow picture is made anew for it.
const VIEW_RESTS_SECONDS: f64 = 0.12;
/// A selection of more objects than this is drawn as a kept picture rather than afresh on every frame.
const OBJECTS_FOR_SELECTION_PICTURE: usize = 2000;

/// LINEWRIGHT_NO_GPU set to anything keeps every picture off the graphics card, for a driver that draws it wrong.
fn card_turned_off() -> bool {
    static OFF: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *OFF.get_or_init(|| std::env::var_os("LINEWRIGHT_NO_GPU").is_some())
}

fn next_picture_id() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

/// A number that changes with anything outside the objects themselves that changes how they show: the layers'
/// colors, linetypes, lineweights and whether they are on, and the drawing's settings for linetypes, lineweights
/// and wipeout frames.
fn look_of(document: &Document) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for layer in &document.layers {
        (&layer.name, layer.color, &layer.linetype, layer.lineweight, layer.shown()).hash(&mut hasher);
    }
    (document.ltscale.to_bits(), document.default_lineweight, document.wipeout_frame, document.shown_layout()).hash(&mut hasher);
    document.path.hash(&mut hasher);
    hasher.finish()
}

fn fonts_of(screen: &Screen) -> u64 {
    use std::hash::{Hash, Hasher};
    // The same whatever order the set gives its names in.
    screen.fonts.iter().fold(screen.fonts.len() as u64, |sum, name| {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        name.hash(&mut hasher);
        sum.wrapping_add(hasher.finish())
    })
}

/// A drawing and the space of it whose objects a picture is of: the open one, or model space for a viewport.
type Space<'a> = (&'a Document, &'a EntityStore);

/// The picture of some of a space's objects, in the order given: each drawn as `draw_entity` draws it, and
/// what it draws cut into triangles here rather than by egui on every frame.
fn pieces_of(ctx: &egui::Context, screen: &Screen, (document, store): Space, ids: &[EntityId], look: &Look) -> Vec<Piece> {
    let options = ctx.tessellation_options(|options| *options);
    // No discs from the font atlas, which a kept picture must not point into: round dots are cut into triangles.
    let mut tessellator = egui::epaint::Tessellator::new(ctx.pixels_per_point(), options, [1, 1], vec![]);
    tessellator.set_clip_rect(screen.rect.expand(CLIP_MARGIN));
    let (mut pieces, mut mesh, mut covering, mut shapes, mut later) = (vec![], egui::Mesh::default(), false, vec![], vec![]);
    let keep = |mesh: &mut egui::Mesh, covering: &mut bool, pieces: &mut Vec<Piece>| {
        if !mesh.is_empty() {
            pieces.push(Piece::Mesh { mesh: Arc::new(std::mem::take(mesh)), first: 0, covers: *covering });
        }
        *covering = false;
    };
    let scale = screen.camera.scale;
    for id in ids {
        let (Some(bounds), Some(entity)) = (store.bounds_of(*id), store.get(*id)) else { continue };
        // Too small to see, except points, which have no size and are drawn as a dot.
        if bounds.diagonal() * scale < MIN_ENTITY_PIXELS && !matches!(entity.shape, EntityShape::Point { .. }) {
            continue;
        }
        if draw_entity_to(&mut shapes, ctx, screen, document, entity, look, Some(&mut later)) {
            // What covers starts a mesh of its own, so text waiting to be drawn can go under it.
            keep(&mut mesh, &mut covering, &mut pieces);
            covering = true;
        }
        let mut texts = later.drain(..);
        for shape in shapes.drain(..) {
            match shape {
                Shape::Noop => {
                    let was = covering;
                    keep(&mut mesh, &mut covering, &mut pieces);
                    covering = was;
                    pieces.extend(texts.next().map(Piece::Text));
                }
                Shape::Mesh(_) => {
                    keep(&mut mesh, &mut covering, &mut pieces);
                    pieces.push(Piece::Shape(shape));
                }
                shape => tessellator.tessellate_shape(shape, &mut mesh),
            }
        }
        if covering {
            keep(&mut mesh, &mut covering, &mut pieces);
        }
    }
    keep(&mut mesh, &mut covering, &mut pieces);
    pieces
}

/// Make the picture of the objects `ids` in this view, on several threads when there is much to draw.
fn make_picture(ctx: &egui::Context, screen: &Screen, space: Space, key: PictureKey, ids: &[EntityId], look: &Look) -> Picture {
    let started = std::time::Instant::now();
    // Each thread draws a run of the objects; the runs go back together in drawing order.
    let mut pieces = crate::threads::map_runs(ids, OBJECTS_PER_THREAD, |_, run| pieces_of(ctx, screen, space, run, look));
    let (mut corners, mut triangle_corners) = (0usize, 0usize);
    for piece in &mut pieces {
        if let Piece::Mesh { mesh, first, .. } = piece {
            *first = triangle_corners as u32;
            corners += mesh.vertices.len();
            triangle_corners += mesh.indices.len();
        }
    }
    let on_card = corners > CORNERS_FOR_CARD
        && triangle_corners <= i32::MAX as usize
        && corners * size_of::<egui::epaint::Vertex>() <= i32::MAX as usize;
    Picture { id: next_picture_id(), key, pieces, on_card, slow: started.elapsed().as_secs_f64() > SLOW_PICTURE_SECONDS }
}

/// Where a picture made for one view goes on screen in another.
fn placement_from(made: &Camera, now: &Camera, rect: Rect) -> Placement {
    let scale = now.scale / made.scale;
    let moved = vec2(((made.center.x - now.center.x) * now.scale) as f32, (-(made.center.y - now.center.y) * now.scale) as f32);
    Placement { scale: scale as f32, offset: rect.center().to_vec2() * (1.0 - scale as f32) + moved }
}

/// The picture to show for `key`, and where on screen it goes: the one kept from the frame before when nothing it
/// depends on has changed, else one made now of the objects `ids` gives. While a view that is slow to draw is
/// panned or zoomed, the picture kept is shown moved along, and a new one made when the view has rested.
fn picture(
    ctx: &egui::Context,
    screen: &Screen,
    space: Space,
    (slot, key): ((&'static str, u64), PictureKey),
    ids: impl FnOnce() -> Vec<EntityId>,
    look: &Look,
) -> (Arc<Picture>, Placement) {
    let pictures = Pictures::of(ctx);
    let card_works = !card_turned_off() && !pictures.gpu.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).broken();
    let mut all = pictures.kept.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let frame = ctx.cumulative_pass_nr();
    all.retain(|_, kept| kept.asked + FRAMES_PICTURES_ARE_KEPT >= frame);
    let kept = all.entry(slot).or_default();
    kept.asked = frame;
    if let Some(picture) = &kept.picture {
        if picture.key == key && (card_works || !picture.on_card) {
            kept.moved = None;
            return (picture.clone(), Placement::AS_MADE);
        }
        if picture.slow && picture.on_card && card_works && picture.key.moved_only(&key) {
            let now = ctx.input(|input| input.time);
            let since = match kept.moved {
                Some((camera, since)) if camera == key.camera => since,
                _ => now,
            };
            kept.moved = Some((key.camera, since));
            if now - since < VIEW_RESTS_SECONDS {
                ctx.request_repaint_after(std::time::Duration::from_secs_f64(VIEW_RESTS_SECONDS - (now - since)));
                return (picture.clone(), placement_from(&picture.key.camera, &key.camera, screen.rect));
            }
        }
    }
    let mut picture = make_picture(ctx, screen, space, key, &ids(), look);
    picture.on_card &= card_works;
    let picture = Arc::new(picture);
    *kept = Kept { picture: Some(picture.clone()), moved: None, asked: frame };
    (picture, Placement::AS_MADE)
}

/// Put a picture among the shapes to paint. Its meshes go as they are, or, for a picture on the graphics card, as
/// a call that draws them from there. Text is laid out for the view as it is now; it waits until the next piece
/// that covers an area, so that lines alone, which hide nothing, are drawn in as few calls as can be.
fn show_picture(shapes: &mut Vec<Shape>, ctx: &egui::Context, screen: &Screen, picture: &Arc<Picture>, placement: Placement) {
    let gpu = Pictures::of(ctx).gpu;
    let frame = ctx.cumulative_pass_nr();
    // A run of meshes whose corners follow on from one another: where it starts and how many corners it has.
    let mut run: Option<(u32, u32)> = None;
    let mut waiting: Vec<&LaterText> = vec![];
    let draw_run = |shapes: &mut Vec<Shape>, run: &mut Option<(u32, u32)>| {
        let Some(range) = run.take() else { return };
        let (gpu, picture, ctx) = (gpu.clone(), picture.clone(), ctx.clone());
        let call = eframe::egui_glow::CallbackFn::new(move |info, painter| {
            let mut gpu = gpu.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            gpu.draw(painter.gl(), &info, (picture.id, frame), || picture.meshes(), range, placement);
            if gpu.broken() {
                ctx.request_repaint(); // the next frame draws through egui instead
            }
        });
        shapes.push(Shape::Callback(egui::PaintCallback { rect: screen.rect, callback: Arc::new(call) }));
    };
    let draw_waiting = |shapes: &mut Vec<Shape>, waiting: &mut Vec<&LaterText>| {
        for text in waiting.drain(..) {
            draw_text(shapes, ctx, screen, &text.spec, &Pen { color: text.color, width: text.width, dashes: &text.dashes });
        }
    };
    for piece in &picture.pieces {
        match piece {
            Piece::Mesh { mesh, first, covers } => {
                if *covers && !waiting.is_empty() {
                    draw_run(shapes, &mut run);
                    draw_waiting(shapes, &mut waiting);
                }
                if picture.on_card {
                    let (start, count) = run.unwrap_or((*first, 0));
                    run = Some((start, count + mesh.indices.len() as u32));
                } else {
                    shapes.push(Shape::Mesh(mesh.clone()));
                }
            }
            Piece::Text(text) => waiting.push(text),
            Piece::Shape(shape) => {
                draw_run(shapes, &mut run);
                draw_waiting(shapes, &mut waiting);
                let mut shape = shape.clone();
                if placement != Placement::AS_MADE {
                    shape.transform(egui::emath::TSTransform::new(placement.offset, placement.scale));
                }
                shapes.push(shape);
            }
        }
    }
    draw_run(shapes, &mut run);
    draw_waiting(shapes, &mut waiting);
}

/// Image files read into textures, by the path they were looked for at. None for one that could not be read.
#[derive(Clone, Default)]
struct ImageCache(std::sync::Arc<std::sync::Mutex<std::collections::HashMap<std::path::PathBuf, Option<egui::TextureHandle>>>>);

/// Where an image's file is: as written when absolute, else beside the drawing.
fn image_file(document: &Document, path: &str) -> std::path::PathBuf {
    let written = std::path::Path::new(path);
    match document.path.as_deref().and_then(std::path::Path::parent) {
        Some(folder) if written.is_relative() => folder.join(written),
        _ => written.to_path_buf(),
    }
}

/// The picture of an image, stretched over its four corners. A file that cannot be read leaves just the frame.
fn draw_image(shapes: &mut Vec<Shape>, ctx: &egui::Context, screen: &Screen, document: &Document, path: &str, corners: [Point; 4]) {
    let file = image_file(document, path);
    let cache = ctx.data_mut(|d| d.get_temp_mut_or_default::<ImageCache>(egui::Id::new("image_cache")).clone());
    let texture = {
        let mut known = cache.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        known
            .entry(file.clone())
            .or_insert_with(|| {
                let picture = image::open(&file).ok()?.to_rgba8();
                let size = [picture.width() as usize, picture.height() as usize];
                let colors = egui::ColorImage::from_rgba_unmultiplied(size, picture.as_raw());
                Some(ctx.load_texture(file.display().to_string(), colors, egui::TextureOptions::LINEAR))
            })
            .clone()
    };
    let Some(texture) = texture else { return };
    let mut mesh = egui::Mesh::with_texture(texture.id());
    // The picture's top row is at the image's top edge: corners 3 and 2.
    for (corner, uv) in corners.iter().zip([pos2(0.0, 1.0), pos2(1.0, 1.0), pos2(1.0, 0.0), pos2(0.0, 0.0)]) {
        mesh.vertices.push(egui::epaint::Vertex { pos: screen.pos(*corner), uv, color: Color32::WHITE });
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    shapes.push(Shape::mesh(mesh));
}

/// A part's linetype by name: its own, or its layer's when it is by layer.
fn effective_linetype(style: &Styled, document: &Document) -> String {
    if style.linetype.eq_ignore_ascii_case(BYLAYER_LINETYPE) {
        document.layer(&style.layer).map_or(String::new(), |layer| layer.linetype.clone())
    } else {
        style.linetype.clone()
    }
}

/// On white paper, white and near-white show black, as they plot.
fn on_paper(color: Color32, paper: bool) -> Color32 {
    if paper && color.r() > 230 && color.g() > 230 && color.b() > 230 { Color32::BLACK } else { color }
}

/// The sheet of an open layout, with a shadow under it.
fn draw_sheet(shapes: &mut Vec<Shape>, screen: &Screen, layout: &Layout) {
    let low = layout.paper_origin;
    let high = low + crate::model::geometry::pt(layout.paper_size.0, layout.paper_size.1);
    let sheet = Rect::from_two_pos(screen.pos(low), screen.pos(high));
    shapes.push(Shape::rect_filled(sheet.translate(vec2(4.0, 4.0)), 0.0, theme::PANEL_DARK));
    shapes.push(Shape::rect_filled(sheet, 0.0, Color32::WHITE));
}

/// What a viewport shows: model space at its scale, clipped to its frame, without the layers it freezes.
fn draw_viewport(ui: &egui::Ui, screen: &Screen, document: &Document, viewport: &Entity) {
    let EntityShape::Viewport { center, width, height, view_center, view_height, frozen, .. } = &viewport.shape else { return };
    let corners = frame_corners(*center, *width, *height);
    let clip = Rect::from_two_pos(screen.pos(corners[0]), screen.pos(corners[2])).intersect(screen.rect);
    if clip.width() <= 0.0 || clip.height() <= 0.0 {
        return;
    }
    let camera = viewport_camera(&screen.camera, *center, *height, *view_center, *view_height);
    let scale = camera.scale;
    let inside = Screen {
        rect: screen.rect,
        camera,
        fonts: screen.fonts,
        paper: true,
        lineweight_pixels: screen.lineweight_pixels,
        psltscale: screen.psltscale,
        linetype_divisor: if screen.psltscale { viewport_scale(*height, *view_height) } else { 1.0 },
    };
    let area = crate::model::index::Bounds::from_corners(inside.world(clip.min), inside.world(clip.max));
    let model = document.model();
    let shown = || -> Vec<EntityId> {
        let ids = model.ids_in(&area.grow(CLIP_MARGIN as f64 / scale)).into_iter();
        let seen = |entity: &Entity| document.is_visible(entity) && !frozen.iter().any(|layer| layer.eq_ignore_ascii_case(&entity.layer));
        ids.filter(|id| model.get(*id).is_some_and(seen)).collect()
    };
    // Model space through this viewport is a picture kept like the drawing's own: it depends also on the viewport's
    // frame, which cuts it off, and on the layers frozen in it. What it looks at is in the camera.
    let also = {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        (frozen, center.x.to_bits(), center.y.to_bits(), width.to_bits(), height.to_bits()).hash(&mut hasher);
        hasher.finish()
    };
    let ctx = ui.ctx();
    let key = PictureKey::of(ctx, &inside, document, model.revision(), 0, also);
    let (seen, placement) = picture(ctx, &inside, (document, model), ((VIEWPORT_PICTURE, viewport.id), key), shown, &Look::Own);
    let mut shapes = vec![];
    show_picture(&mut shapes, ctx, &inside, &seen, placement);
    ui.painter().with_clip_rect(clip).extend(shapes);
}

fn color32((r, g, b): (u8, u8, u8)) -> Color32 {
    Color32::from_rgb(r, g, b)
}

/// The grid, along the UCS's axes from its origin.
fn draw_grid(shapes: &mut Vec<Shape>, screen: &Screen, ucs: &Ucs) {
    let visible = screen.camera.visible(screen.rect.width() as f64, screen.rect.height() as f64);
    // What is in view, as the UCS sees it: the box around the view's corners in UCS coordinates.
    let corners = [pt(visible.x0, visible.y0), pt(visible.x1, visible.y0), pt(visible.x1, visible.y1), pt(visible.x0, visible.y1)];
    let seen = Bounds::around(corners.map(|c| ucs.to_ucs(c)));
    let minor = grid_spacing(screen.camera.scale);
    for (step, color) in [(minor, theme::GRID_MINOR), (minor * MAJOR_GRID_EVERY, theme::GRID_MAJOR)] {
        let stroke = Stroke::new(1.0_f32, color);
        let line = |shapes: &mut Vec<Shape>, a: Point, b: Point| {
            shapes.push(Shape::line_segment([screen.pos(ucs.to_world(a)), screen.pos(ucs.to_world(b))], stroke));
        };
        let mut x = (seen.x0 / step).floor() * step;
        while x <= seen.x1 {
            line(shapes, pt(x, seen.y0), pt(x, seen.y1));
            x += step;
        }
        let mut y = (seen.y0 / step).floor() * step;
        while y <= seen.y1 {
            line(shapes, pt(seen.x0, y), pt(seen.x1, y));
            y += step;
        }
    }
}

fn draw_marker(shapes: &mut Vec<Shape>, center: Pos2, kind: SnapKind) {
    let stroke = Stroke::new(2.0_f32, theme::SNAP_MARKER);
    let h = MARKER_HALF;
    let (x, y) = (center.x, center.y);
    match kind {
        SnapKind::Endpoint => {
            shapes.push(Shape::rect_stroke(Rect::from_center_size(center, vec2(2.0 * h, 2.0 * h)), 0.0, stroke, StrokeKind::Middle))
        }
        SnapKind::Midpoint => shapes.push(Shape::closed_line(vec![pos2(x, y - h), pos2(x - h, y + h), pos2(x + h, y + h)], stroke)),
        SnapKind::Center => shapes.push(Shape::circle_stroke(center, h, stroke)),
        SnapKind::Intersection => {
            shapes.push(Shape::line_segment([pos2(x - h, y - h), pos2(x + h, y + h)], stroke));
            shapes.push(Shape::line_segment([pos2(x - h, y + h), pos2(x + h, y - h)], stroke));
        }
        SnapKind::Perpendicular => shapes.push(Shape::line(vec![pos2(x - h, y - h), pos2(x - h, y + h), pos2(x + h, y + h)], stroke)),
        SnapKind::Quadrant => shapes.push(Shape::closed_line(vec![pos2(x, y - h), pos2(x + h, y), pos2(x, y + h), pos2(x - h, y)], stroke)),
        SnapKind::Node => {
            shapes.push(Shape::circle_stroke(center, h, stroke));
            shapes.push(Shape::line_segment([pos2(x - h * 0.7, y - h * 0.7), pos2(x + h * 0.7, y + h * 0.7)], stroke));
            shapes.push(Shape::line_segment([pos2(x - h * 0.7, y + h * 0.7), pos2(x + h * 0.7, y - h * 0.7)], stroke));
        }
        SnapKind::Insertion => {
            shapes
                .push(Shape::closed_line(vec![pos2(x - h, y - h), pos2(x + h * 0.2, y - h), pos2(x + h * 0.2, y), pos2(x - h, y)], stroke));
            shapes
                .push(Shape::closed_line(vec![pos2(x - h * 0.2, y), pos2(x + h, y), pos2(x + h, y + h), pos2(x - h * 0.2, y + h)], stroke));
        }
        SnapKind::Nearest => {
            shapes.push(Shape::closed_line(vec![pos2(x - h, y - h), pos2(x + h, y - h), pos2(x - h, y + h), pos2(x + h, y + h)], stroke))
        }
    }
}

/// Object snap tracking: a small cross on each acquired point, and dotted lines from those the cursor lines up with.
fn draw_tracking(shapes: &mut Vec<Shape>, screen: &Screen, controller: &Controller) {
    let stroke = Stroke::new(1.0_f32, theme::SNAP_MARKER);
    for point in &controller.acquired {
        let at = screen.pos(*point);
        shapes.push(Shape::line_segment([at - vec2(4.0, 0.0), at + vec2(4.0, 0.0)], stroke));
        shapes.push(Shape::line_segment([at - vec2(0.0, 4.0), at + vec2(0.0, 4.0)], stroke));
    }
    let Some(cursor) = &controller.cursor else { return };
    let to = screen.pos(cursor.point);
    for from in &cursor.tracked {
        let mut dashes = vec![];
        dashed(&[screen.pos(*from), to], &[3.0, 3.0], &mut dashes);
        shapes.extend(dashes.into_iter().map(|segment| Shape::line_segment(segment, Stroke::new(1.0_f32, theme::TEXT_DIM))));
    }
}

/// Room kept free along the drawing area's right edge for the zoom buttons (`panels::navigation_bar`).
const NAV_BAR_ROOM: f32 = 62.0;

/// Dynamic input: by the cursor, what the command asks and, at a point prompt, two fields as AutoCAD shows them:
/// length and angle from the base point, or X and Y. What is typed goes into the highlighted field; Tab moves on.
fn draw_dynamic_input(
    shapes: &mut Vec<Shape>,
    ctx: &egui::Context,
    screen: &Screen,
    controller: &Controller,
    pointer: Pos2,
    typed: &str,
    first_field: Option<&str>,
) {
    let Some(prompt) = controller.prompt() else { return };
    let font = FontId::proportional(12.0);
    let galley = |text: String, color: Color32| ctx.fonts(|f| f.layout_no_wrap(text, font.clone(), color));
    let message = galley(prompt.message.clone(), theme::TEXT);
    let cursor = controller.cursor.as_ref().map(|c| c.point);
    let typing = |value: String, active: bool| if active && !typed.is_empty() { typed.to_string() } else { value };
    let fields: Vec<(&str, String, bool)> = match (prompt.kind == crate::commands::prompts::PromptKind::Point, prompt.base, cursor) {
        (true, Some(base), Some(point)) => {
            let angle = crate::model::geometry::modulo(base.angle_to(point), 360.0);
            let length = first_field.map_or_else(|| typing(format!("{:.4}", base.dist(point)), true), str::to_string);
            vec![
                ("Length", length, first_field.is_none()),
                ("Angle", typing(format!("{angle:.0}°"), first_field.is_some()), first_field.is_some()),
            ]
        }
        (true, None, Some(point)) => {
            let x = first_field.map_or_else(|| typing(format!("{:.4}", point.x), true), str::to_string);
            vec![("X", x, first_field.is_none()), ("Y", typing(format!("{:.4}", point.y), first_field.is_some()), first_field.is_some())]
        }
        _ => vec![],
    };
    let (field_width, row_height) = (76.0, 20.0);
    let width = message.size().x.max(fields.len() as f32 * (field_width + 60.0)) + 14.0;
    let extra = if !fields.is_empty() || !typed.is_empty() { row_height + 4.0 } else { 0.0 };
    let height = message.size().y + 8.0 + extra;
    let mut top_left = pointer + vec2(18.0, 18.0);
    // Keep it inside the drawing area.
    top_left.x = top_left.x.min(screen.rect.right() - width - NAV_BAR_ROOM);
    top_left.y = top_left.y.min(screen.rect.bottom() - height - 4.0);
    let frame = Rect::from_min_size(top_left, vec2(width, height));
    shapes.push(Shape::rect_filled(frame, 4.0, theme::PANEL_RAISED.gamma_multiply(0.94)));
    shapes.push(Shape::rect_stroke(frame, 4.0, Stroke::new(1.0_f32, theme::BORDER_STRONG), StrokeKind::Inside));
    let y = top_left.y + message.size().y + 8.0;
    shapes.push(Shape::galley(top_left + vec2(7.0, 4.0), message, theme::TEXT));
    let mut x = top_left.x + 7.0;
    if fields.is_empty() && !typed.is_empty() {
        shapes.push(Shape::galley(pos2(x, y), galley(format!("› {typed}"), theme::TEXT), theme::TEXT));
    }
    for (label, value, active) in fields {
        let label = galley(label.to_string(), theme::TEXT_DIM);
        let label_width = label.size().x;
        shapes.push(Shape::galley(pos2(x, y + 2.0), label, theme::TEXT_DIM));
        x += label_width + 6.0;
        let field = Rect::from_min_size(pos2(x, y), vec2(field_width, row_height - 2.0));
        shapes.push(Shape::rect_filled(field, 3.0, if active { theme::CANVAS } else { theme::PANEL }));
        let edge = Stroke::new(1.0_f32, if active { theme::TEXT_DIM } else { theme::BORDER });
        shapes.push(Shape::rect_stroke(field, 3.0, edge, StrokeKind::Inside));
        let color = if active { theme::TEXT } else { theme::TEXT_DIM };
        shapes.push(Shape::galley(pos2(x + 5.0, y + 2.0), galley(value, color), color));
        x += field_width + 14.0;
    }
}

/// The UCS icon: the UCS's X and Y axes, at its origin when that is in view (UCSICON ORigin), else in the lower left
/// corner. The square at its corner says it is the world coordinate system.
fn draw_axes_icon(shapes: &mut Vec<Shape>, ctx: &egui::Context, screen: &Screen, document: &Document) {
    let (shown, at_origin) = document.ucs_icon;
    if !shown {
        return;
    }
    let (rect, ucs) = (screen.rect, document.ucs);
    let corner = pos2(rect.left() + AXES_MARGIN.x, rect.bottom() - AXES_MARGIN.y);
    let inner = rect.shrink2(AXES_MARGIN);
    let origin = Some(screen.pos(ucs.origin)).filter(|o| at_origin && inner.contains(*o)).unwrap_or(corner);
    // The screen's Y runs down.
    let on_screen = |v: Point| vec2(v.x as f32, -v.y as f32);
    let (x_axis, y_axis) = (on_screen(ucs.x_axis()), on_screen(ucs.y_axis()));
    for (color, axis, label) in [(theme::AXIS_X, x_axis, "X"), (theme::AXIS_Y, y_axis, "Y")] {
        let tip = origin + axis * AXES_LENGTH;
        shapes.push(Shape::line_segment([origin, tip], Stroke::new(1.6_f32, color)));
        let galley = ctx.fonts(|f| f.layout_no_wrap(label.to_string(), FontId::proportional(12.0), color));
        let at = tip + axis * 9.0 - galley.size() / 2.0;
        shapes.push(Shape::galley(at, galley, color));
    }
    if ucs.is_world() {
        shapes.push(Shape::rect_stroke(
            Rect::from_center_size(origin, vec2(6.0, 6.0)),
            0.0,
            Stroke::new(1.0_f32, theme::TEXT_DIM),
            StrokeKind::Middle,
        ));
    }
}

fn draw_selection_box(shapes: &mut Vec<Shape>, screen: &Screen, first: Point, second: Point) {
    let crossing = second.x < first.x;
    let color = if crossing { theme::CROSSING } else { theme::SELECTION };
    let rect = Rect::from_two_pos(screen.pos(first), screen.pos(second));
    shapes.push(Shape::rect_filled(rect, 0.0, color.gamma_multiply(BOX_FILL_ALPHA)));
    let corners = vec![rect.left_top(), rect.right_top(), rect.right_bottom(), rect.left_bottom(), rect.left_top()];
    if crossing {
        shapes.extend(Shape::dashed_line(&corners, Stroke::new(1.0_f32, color), 4.0, 3.0));
    } else {
        shapes.push(Shape::line(corners, Stroke::new(1.0_f32, color)));
    }
}

/// The crosshair, its arms along the UCS's axes (`angle`, in degrees).
fn draw_crosshair(shapes: &mut Vec<Shape>, rect: Rect, center: Pos2, pickbox: bool, angle: f64) {
    let stroke = Stroke::new(1.0_f32, theme::CROSSHAIR);
    let arm = rect.width() * CROSSHAIR_ARM_FRACTION;
    let (sin, cos) = angle.to_radians().sin_cos();
    let (along, across) = (vec2(cos as f32, -sin as f32) * arm, vec2(-sin as f32, -cos as f32) * arm);
    shapes.push(Shape::line_segment([center - along, center + along], stroke));
    shapes.push(Shape::line_segment([center - across, center + across], stroke));
    if pickbox {
        shapes.push(Shape::rect_stroke(
            Rect::from_center_size(center, vec2(2.0 * PICKBOX_HALF, 2.0 * PICKBOX_HALF)),
            0.0,
            stroke,
            StrokeKind::Middle,
        ));
    }
}

pub struct Overlay {
    pub show_grid: bool,
    pub pointer: Option<Pos2>,
    pub panning: bool,
    /// What is typed on the command line, which dynamic input shows by the cursor.
    pub typed: String,
    /// Dynamic input's first field, once Tab has moved on from it.
    pub first_field: Option<String>,
}

/// How model space shows through a viewport of the layout on screen, and the part of the screen its frame
/// covers there.
pub fn viewport_screen<'a>(screen: &Screen<'a>, viewport: &Entity) -> Option<(Screen<'a>, Rect)> {
    let EntityShape::Viewport { center, width, height, view_center, view_height, .. } = &viewport.shape else { return None };
    let corners = frame_corners(*center, *width, *height);
    let frame = Rect::from_two_pos(screen.pos(corners[0]), screen.pos(corners[2]));
    let camera = viewport_camera(&screen.camera, *center, *height, *view_center, *view_height);
    let linetype_divisor = if screen.psltscale { viewport_scale(*height, *view_height) } else { 1.0 };
    Some((Screen { camera, linetype_divisor, ..*screen }, frame))
}

pub fn paint(ui: &egui::Ui, screen: &Screen, controller: &Controller, overlay: &Overlay) {
    let ctx = ui.ctx();
    let document = controller.document();
    let mut shapes: Vec<Shape> = vec![Shape::rect_filled(screen.rect, 0.0, theme::CANVAS)];
    let shown = document.shown_layout();
    let visible = screen.camera.visible(screen.rect.width() as f64, screen.rect.height() as f64);
    match shown.and_then(|i| document.layouts.get(i)) {
        Some(layout) => {
            draw_sheet(&mut shapes, screen, layout);
            // Viewports show model space under what is drawn on the sheet.
            ui.painter().with_clip_rect(screen.rect).extend(std::mem::take(&mut shapes));
            let sheet = document.space_store(shown);
            for id in sheet.ids_in(&visible) {
                if let Some(entity) = sheet.get(id).filter(|e| matches!(e.shape, EntityShape::Viewport { .. }) && document.is_visible(e)) {
                    draw_viewport(ui, screen, document, entity);
                }
            }
        }
        None if overlay.show_grid => draw_grid(&mut shapes, screen, &document.ucs),
        None => {}
    }
    let through = document.through_viewport().and_then(|viewport| viewport_screen(screen, viewport));
    if let Some((inside, frame)) = &through {
        // Working through a viewport: the sheet's own objects show but are not what is worked on, and the
        // viewport's frame stands out.
        let sheet = document.space_store(shown);
        for id in sheet.ids_in(&visible.grow(CLIP_MARGIN as f64 / screen.camera.scale)) {
            if let Some(entity) = sheet.get(id).filter(|e| document.is_visible(e)) {
                draw_entity(&mut shapes, ctx, screen, document, entity, &Look::Own);
            }
        }
        shapes.push(Shape::rect_stroke(*frame, 0.0, Stroke::new(ACTIVE_VIEWPORT_WIDTH, theme::ACCENT), StrokeKind::Outside));
        draw_axes_icon(&mut shapes, ctx, screen, document);
        ui.painter().with_clip_rect(screen.rect).extend(std::mem::take(&mut shapes));
        let selected: Vec<&Entity> = controller.selection.ids().iter().filter_map(|id| document.get(*id)).collect();
        draw_work(&mut shapes, ctx, inside, controller, overlay, selected);
        let pointer_inside = overlay.pointer.filter(|p| frame.contains(*p));
        if let (Some(pointer), false) = (pointer_inside, overlay.panning) {
            draw_crosshair(&mut shapes, *frame, pointer, controller.picking(), document.ucs.angle);
        }
        ui.painter().with_clip_rect(frame.intersect(screen.rect)).extend(shapes);
        return;
    }
    let in_sight = visible.grow(CLIP_MARGIN as f64 / screen.camera.scale);
    let space = (document, document.space_store(document.active_layout()));
    let key = PictureKey::of(ctx, screen, document, document.revision(), 0, 0);
    let (drawing, placement) = picture(ctx, screen, space, (DRAWING_PICTURE, key), || document.ids_in(&in_sight), &Look::Own);
    show_picture(&mut shapes, ctx, screen, &drawing, placement);
    // The selected objects in view are drawn over in the selection's pen: a few afresh on every frame, a great
    // many as a picture kept like the drawing's.
    let selected_in_view = || -> Vec<EntityId> {
        let ids = controller.selection.ids().iter();
        ids.filter(|id| {
            document.bounds_of(**id).is_some_and(|b| b.touches(&in_sight)) && document.get(**id).is_some_and(|e| document.is_visible(e))
        })
        .copied()
        .collect()
    };
    let mut selected: Vec<&Entity> = vec![];
    let mut selection_picture = None;
    if controller.selection.len() > OBJECTS_FOR_SELECTION_PICTURE {
        let pen = Pen { color: theme::SELECTION, width: SELECTED_WIDTH, dashes: &[] };
        let key = PictureKey::of(ctx, screen, document, document.revision(), controller.selection.revision(), 0);
        selection_picture = Some(picture(ctx, screen, space, (SELECTION_PICTURE, key), selected_in_view, &Look::Pen(&pen)));
    } else {
        selected = selected_in_view().into_iter().filter_map(|id| document.get(id)).collect();
    }
    if shown.is_none() {
        for entity in document.shown_only.iter().filter(|e| document.is_visible(e)) {
            draw_entity(&mut shapes, ctx, screen, document, entity, &Look::Own);
        }
    }
    draw_axes_icon(&mut shapes, ctx, screen, document);
    if let Some((picture, placement)) = &selection_picture {
        show_picture(&mut shapes, ctx, screen, picture, *placement);
    }
    draw_work(&mut shapes, ctx, screen, controller, overlay, selected);
    if let (Some(pointer), false) = (overlay.pointer, overlay.panning) {
        draw_crosshair(&mut shapes, screen.rect, pointer, controller.picking(), document.ucs.angle);
    }
    ui.painter().with_clip_rect(screen.rect).extend(shapes);
}

/// What the work in hand shows over the drawing, in the space worked on: the selection, previews, grips, the
/// selection box, snaps, tracking and dynamic input.
fn draw_work(
    shapes: &mut Vec<Shape>,
    ctx: &egui::Context,
    screen: &Screen,
    controller: &Controller,
    overlay: &Overlay,
    selected: Vec<&Entity>,
) {
    let document = controller.document();
    let selected_pen = Pen { color: theme::SELECTION, width: SELECTED_WIDTH, dashes: &[] };
    for entity in selected {
        draw_entity(shapes, ctx, screen, document, entity, &Look::Pen(&selected_pen));
    }
    for entity in &controller.preview {
        draw_entity(shapes, ctx, screen, document, entity, &Look::Preview);
    }
    for (_, _, point) in controller.grips() {
        let at = screen.pos(point);
        let hot = overlay.pointer.is_some_and(|p| p.distance(at) <= GRIP_HALF + 1.0);
        let square = Rect::from_center_size(at, vec2(2.0 * GRIP_HALF, 2.0 * GRIP_HALF));
        shapes.push(Shape::rect_filled(square, 0.0, if hot { theme::GRIP_HOT } else { theme::GRIP }));
        shapes.push(Shape::rect_stroke(square, 0.0, Stroke::new(1.0_f32, theme::CANVAS), StrokeKind::Inside));
    }
    if let Some((first, second)) = controller.selection_box {
        draw_selection_box(shapes, screen, first, second);
    }
    if let Some((point, kind)) = controller.snap_marker() {
        draw_marker(shapes, screen.pos(point), kind);
    }
    draw_tracking(shapes, screen, controller);
    if controller.snap.dynamic
        && let (Some(pointer), false) = (overlay.pointer, overlay.panning)
    {
        draw_dynamic_input(shapes, ctx, screen, controller, pointer, &overlay.typed, overlay.first_field.as_deref());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::geometry::pt;

    /// A screen of one pixel per drawing unit, with the drawing's own settings.
    fn screen_with<'a>(fonts: &'a HashSet<String>, scale: f64, divisor: f64) -> Screen<'a> {
        let rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(100.0, 100.0));
        Screen {
            rect,
            camera: Camera { center: pt(0.0, 0.0), scale },
            fonts,
            paper: false,
            lineweight_pixels: None,
            psltscale: true,
            linetype_divisor: divisor,
        }
    }

    fn dashed_part(linetype_scale: f64) -> Styled {
        Styled { layer: "0".into(), color: 256, linetype: "Dashed".into(), linetype_scale, lineweight: 0 }
    }

    #[test]
    fn dashes_follow_ltscale_and_the_objects_own_linetype_scale() {
        let fonts = HashSet::new();
        let mut document = Document::new();
        document.ltscale = 2.0;
        let dashes = screen_dashes(&screen_with(&fonts, 1.0, 1.0), &document, &dashed_part(0.5));
        assert_eq!(dashes.len(), 2);
        assert!((dashes[0] - 12.7).abs() < 1e-4 && (dashes[1] - 6.35).abs() < 1e-4, "{dashes:?}: 12.7 and 6.35 at 2 x 0.5");
    }

    #[test]
    fn through_a_viewport_psltscale_makes_dashes_as_long_on_the_sheet_as_paper_space_ones() {
        let fonts = HashSet::new();
        let document = Document::new();
        // A viewport at 4 pixels per unit, which is 4 times the sheet's scale: the dash divided by 4 is still 12.7 px.
        let dashes = screen_dashes(&screen_with(&fonts, 4.0, 4.0), &document, &dashed_part(1.0));
        assert!((dashes[0] - 12.7).abs() < 1e-4, "{dashes:?}");
        let undivided = screen_dashes(&screen_with(&fonts, 4.0, 1.0), &document, &dashed_part(1.0));
        assert!((undivided[0] - 50.8).abs() < 1e-3, "without PSLTSCALE the dash grows with the viewport");
    }

    #[test]
    fn a_linetype_too_fine_to_see_is_drawn_solid() {
        let fonts = HashSet::new();
        let document = Document::new();
        assert!(screen_dashes(&screen_with(&fonts, 0.1, 1.0), &document, &dashed_part(1.0)).is_empty(), "6.35 px at 0.1 is under a pixel");
        assert_eq!(screen_dashes(&screen_with(&fonts, 0.2, 1.0), &document, &dashed_part(1.0)).len(), 2, "1.27 px is not");
    }

    #[test]
    fn a_picture_made_for_one_view_lands_where_another_view_puts_the_drawing() {
        let fonts = HashSet::new();
        let made = screen_with(&fonts, 2.0, 1.0);
        let mut now = made;
        now.camera = Camera { center: pt(30.0, -12.0), scale: 5.0 };
        let placement = placement_from(&made.camera, &now.camera, made.rect);
        for point in [pt(0.0, 0.0), pt(17.5, 40.0), pt(-300.0, 2.25)] {
            let moved = (made.pos(point).to_vec2() * placement.scale + placement.offset).to_pos2();
            assert!(moved.distance(now.pos(point)) < 1e-2, "{point:?}: {moved:?} for {:?}", now.pos(point));
        }
        assert_eq!(placement_from(&made.camera, &made.camera, made.rect), Placement::AS_MADE);
    }

    #[test]
    fn a_kept_picture_is_shown_again_until_the_drawing_or_the_view_changes() {
        use crate::model::entities::{circle, line};
        let ctx = egui::Context::default();
        let fonts = HashSet::new();
        let screen = screen_with(&fonts, 1.0, 1.0);
        let mut document = Document::with_entities(vec![line(pt(-20.0, 0.0), pt(20.0, 0.0)), circle(pt(0.0, 0.0), 10.0)]);
        let key =
            |document: &Document, screen: &Screen| (DRAWING_PICTURE, PictureKey::of(&ctx, screen, document, document.revision(), 0, 0));
        let ids = |document: &Document| document.ids_in(&crate::model::index::Bounds::square_around(pt(0.0, 0.0), 100.0));
        let (first, placement) =
            picture(&ctx, &screen, (&document, document.space_store(None)), key(&document, &screen), || ids(&document), &Look::Own);
        assert_eq!(placement, Placement::AS_MADE);
        assert!(matches!(first.pieces[..], [Piece::Mesh { .. }]), "two objects without text or fills make one mesh");
        let (again, _) = picture(
            &ctx,
            &screen,
            (&document, document.space_store(None)),
            key(&document, &screen),
            || panic!("not made anew"),
            &Look::Own,
        );
        assert!(Arc::ptr_eq(&first, &again));
        // A small picture is quick to make, so a new view makes it anew rather than moving the old one along.
        let mut zoomed = screen;
        zoomed.camera.scale = 2.0;
        let (other, placement) =
            picture(&ctx, &zoomed, (&document, document.space_store(None)), key(&document, &zoomed), || ids(&document), &Look::Own);
        assert!(!Arc::ptr_eq(&first, &other) && placement == Placement::AS_MADE);
        document.add(line(pt(0.0, -5.0), pt(0.0, 5.0)));
        let (changed, _) =
            picture(&ctx, &zoomed, (&document, document.space_store(None)), key(&document, &zoomed), || ids(&document), &Look::Own);
        assert!(!Arc::ptr_eq(&other, &changed), "a change to the drawing makes a new picture");
        document.layers[0].color = 3;
        let (recolored, _) =
            picture(&ctx, &zoomed, (&document, document.space_store(None)), key(&document, &zoomed), || ids(&document), &Look::Own);
        assert!(!Arc::ptr_eq(&changed, &recolored), "and so does a change to a layer");
    }

    #[test]
    fn clipping_keeps_the_inside_part() {
        let area = Rect::from_min_max(pos2(0.0, 0.0), pos2(10.0, 10.0));
        let (a, b) = clip(pos2(-10.0, 5.0), pos2(20.0, 5.0), area).unwrap();
        assert_eq!((a, b), (pos2(0.0, 5.0), pos2(10.0, 5.0)));
        assert!(clip(pos2(-10.0, -5.0), pos2(20.0, -5.0), area).is_none());
    }

    #[test]
    fn dashes_carry_on_across_corners() {
        let mut out = vec![];
        dashed(&[pos2(0.0, 0.0), pos2(6.0, 0.0), pos2(6.0, 6.0)], &[4.0, 4.0], &mut out);
        assert_eq!(out, vec![[pos2(0.0, 0.0), pos2(4.0, 0.0)], [pos2(6.0, 2.0), pos2(6.0, 6.0)]]);
    }
}
