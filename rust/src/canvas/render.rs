//! Drawing the canvas with egui: grid, entities, the selection, the rubber band and the drafting cursor.

use eframe::egui::epaint::{PathShape, TextShape};
use eframe::egui::{self, Color32, FontFamily, FontId, Pos2, Rect, Shape, Stroke, StrokeKind, Vec2, pos2, vec2};

use std::collections::HashSet;

use super::aci::color_rgb;
use super::shapes::{Styled, TextSpec, flatten, resolve_color};
use super::view::{Camera, grid_spacing, viewport_camera};
use crate::fonts::display_text;
use crate::model::document::{Document, Layout};
use crate::model::entities::{Entity, Shape as EntityShape};
use crate::model::entities::{HAlign, TextAlign, VAlign};
use crate::model::geometry::{Point, direction};
use crate::model::lineweight;
use crate::model::mtext::{RichText, RunStyle, layout};
use crate::model::parts::{BOLD_OFFSET, TextLook};
use crate::model::styles::{BYLAYER_LINETYPE, dash_pattern};
use crate::model::viewport::frame_corners;
use crate::snap::osnap::SnapKind;
use crate::ui::controller::Controller;
use crate::ui::theme;
use crate::ui::theme::drawing_family;

const MAJOR_GRID_EVERY: f64 = 10.0;
const MIN_ENTITY_PIXELS: f64 = 1.0; // smaller than this is not drawn at all
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
}

impl<'a> Screen<'a> {
    /// How a document shows in a rectangle of the window. On a layout lineweights grow as you zoom, as on paper;
    /// in model space they keep their width on screen.
    pub fn new(rect: Rect, camera: Camera, fonts: &'a HashSet<String>, document: &Document) -> Screen<'a> {
        let paper = document.shown_layout().is_some();
        let lineweight_pixels = document
            .show_lineweights
            .then(|| if paper { camera.scale / crate::io::dxf::mm_per_unit(document) } else { lineweight::model_pixels(1.0) as f64 });
        Screen { rect, camera, fonts, paper, lineweight_pixels }
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
    if !(MIN_TEXT_PIXELS..=MAX_TEXT_PIXELS).contains(&pixels) || spec.text.trim().is_empty() {
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

fn draw_entity(shapes: &mut Vec<Shape>, ctx: &egui::Context, screen: &Screen, document: &Document, entity: &Entity, look: &Look) {
    // A construction line has no ends: only the part on screen is drawn, cut in drawing units where it is precise.
    let clipped_xline = match entity.shape {
        EntityShape::XLine { base, direction, ray } => {
            let area = crate::model::index::Bounds::from_corners(screen.world(screen.rect.min), screen.world(screen.rect.max));
            let Some((start, end)) =
                crate::model::xline::clipped(base, direction, ray, &area.grow(CLIP_MARGIN as f64 / screen.camera.scale))
            else {
                return;
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
    }
    for (style, parts) in flatten(entity, CHORD_PIXELS / screen.camera.scale) {
        // Inside a block, entities on a layer that is turned off stay hidden.
        if document.layer(&style.layer).is_some_and(|layer| !layer.shown()) {
            continue;
        }
        let own;
        let pen = match look {
            Look::Pen(pen) => *pen,
            Look::Own => {
                own = Pen {
                    color: on_paper(color32(resolve_color(&style, document)), screen.paper),
                    width: screen.line_width(&style, document),
                    dashes: dash_pattern(&effective_linetype(&style, document)),
                };
                &own
            }
            Look::Preview => {
                own = Pen { color: color32(resolve_color(&style, document)), width: 1.0, dashes: &PREVIEW_DASHES };
                &own
            }
        };
        // A wipeout hides what is under it in the background's color; its frame shows in its own.
        for mask in &parts.masks {
            if !matches!(look, Look::Preview) {
                fill_area(shapes, screen, std::slice::from_ref(mask), if screen.paper { Color32::WHITE } else { theme::CANVAS });
            }
            let frame: Vec<Point> = mask.iter().chain(mask.first()).copied().collect();
            stroke_path(shapes, screen, &frame, pen);
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
            draw_text(shapes, ctx, screen, spec, pen);
        }
    }
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
fn draw_viewport(ui: &egui::Ui, screen: &Screen, document: &Document, viewport: &EntityShape) {
    let EntityShape::Viewport { center, width, height, view_center, view_height, frozen, .. } = viewport else { return };
    let corners = frame_corners(*center, *width, *height);
    let clip = Rect::from_two_pos(screen.pos(corners[0]), screen.pos(corners[2])).intersect(screen.rect);
    if clip.width() <= 0.0 || clip.height() <= 0.0 {
        return;
    }
    let camera = viewport_camera(&screen.camera, *center, *height, *view_center, *view_height);
    let scale = camera.scale;
    let inside = Screen { rect: screen.rect, camera, fonts: screen.fonts, paper: true, lineweight_pixels: screen.lineweight_pixels };
    let area = crate::model::index::Bounds::from_corners(inside.world(clip.min), inside.world(clip.max));
    let model = document.model();
    let mut shapes = vec![];
    for id in model.ids_in(&area.grow(CLIP_MARGIN as f64 / scale)) {
        let (Some(entity), Some(bounds)) = (model.get(id), model.bounds_of(id)) else { continue };
        if !document.is_visible(entity) || frozen.iter().any(|layer| layer.eq_ignore_ascii_case(&entity.layer)) {
            continue;
        }
        if bounds.diagonal() * scale < MIN_ENTITY_PIXELS && !matches!(entity.shape, EntityShape::Point { .. }) {
            continue;
        }
        draw_entity(&mut shapes, ui.ctx(), &inside, document, entity, &Look::Own);
    }
    ui.painter().with_clip_rect(clip).extend(shapes);
}

fn color32((r, g, b): (u8, u8, u8)) -> Color32 {
    Color32::from_rgb(r, g, b)
}

fn draw_grid(shapes: &mut Vec<Shape>, screen: &Screen) {
    let visible = screen.camera.visible(screen.rect.width() as f64, screen.rect.height() as f64);
    let minor = grid_spacing(screen.camera.scale);
    for (step, color) in [(minor, theme::GRID_MINOR), (minor * MAJOR_GRID_EVERY, theme::GRID_MAJOR)] {
        let stroke = Stroke::new(1.0_f32, color);
        let mut x = (visible.x0 / step).floor() * step;
        while x <= visible.x1 {
            let sx = screen.pos(Point { x, y: 0.0 }).x;
            shapes.push(Shape::line_segment([pos2(sx, screen.rect.top()), pos2(sx, screen.rect.bottom())], stroke));
            x += step;
        }
        let mut y = (visible.y0 / step).floor() * step;
        while y <= visible.y1 {
            let sy = screen.pos(Point { x: 0.0, y }).y;
            shapes.push(Shape::line_segment([pos2(screen.rect.left(), sy), pos2(screen.rect.right(), sy)], stroke));
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
    top_left.x = top_left.x.min(screen.rect.right() - width - 4.0);
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

fn draw_axes_icon(shapes: &mut Vec<Shape>, ctx: &egui::Context, rect: Rect) {
    let origin = pos2(rect.left() + AXES_MARGIN.x, rect.bottom() - AXES_MARGIN.y);
    for (color, vector, label_offset, label) in
        [(theme::AXIS_X, vec2(AXES_LENGTH, 0.0), vec2(5.0, 5.0), "X"), (theme::AXIS_Y, vec2(0.0, -AXES_LENGTH), vec2(-4.0, -12.0), "Y")]
    {
        let tip = origin + vector;
        shapes.push(Shape::line_segment([origin, tip], Stroke::new(1.6_f32, color)));
        let galley = ctx.fonts(|f| f.layout_no_wrap(label.to_string(), FontId::proportional(12.0), color));
        shapes.push(Shape::galley(tip + label_offset - vec2(0.0, 6.0), galley, color));
    }
    shapes.push(Shape::rect_stroke(
        Rect::from_center_size(origin, vec2(6.0, 6.0)),
        0.0,
        Stroke::new(1.0_f32, theme::TEXT_DIM),
        StrokeKind::Middle,
    ));
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

fn draw_crosshair(shapes: &mut Vec<Shape>, rect: Rect, center: Pos2, pickbox: bool) {
    let stroke = Stroke::new(1.0_f32, theme::CROSSHAIR);
    let arm = rect.width() * CROSSHAIR_ARM_FRACTION;
    shapes.push(Shape::line_segment([center - vec2(arm, 0.0), center + vec2(arm, 0.0)], stroke));
    shapes.push(Shape::line_segment([center - vec2(0.0, arm), center + vec2(0.0, arm)], stroke));
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
    Some((Screen { camera, ..*screen }, frame))
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
                    draw_viewport(ui, screen, document, &entity.shape);
                }
            }
        }
        None if overlay.show_grid => draw_grid(&mut shapes, screen),
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
        draw_axes_icon(&mut shapes, ctx, screen.rect);
        ui.painter().with_clip_rect(screen.rect).extend(std::mem::take(&mut shapes));
        let selected: Vec<&Entity> = controller.selection.ids().iter().filter_map(|id| document.get(*id)).collect();
        draw_work(&mut shapes, ctx, inside, controller, overlay, selected);
        let pointer_inside = overlay.pointer.filter(|p| frame.contains(*p));
        if let (Some(pointer), false) = (pointer_inside, overlay.panning) {
            draw_crosshair(&mut shapes, *frame, pointer, controller.picking());
        }
        ui.painter().with_clip_rect(frame.intersect(screen.rect)).extend(shapes);
        return;
    }
    let scale = screen.camera.scale;
    let ids = document.ids_in(&visible.grow(CLIP_MARGIN as f64 / scale));
    let mut selected = vec![];
    for id in ids {
        let Some(bounds) = document.bounds_of(id) else { continue };
        let Some(entity) = document.get(id) else { continue };
        // Too small to see, except points, which have no size and are drawn as a dot.
        if bounds.diagonal() * scale < MIN_ENTITY_PIXELS && !matches!(entity.shape, EntityShape::Point { .. }) {
            continue;
        }
        draw_entity(&mut shapes, ctx, screen, document, entity, &Look::Own);
        if controller.selection.contains(id) {
            selected.push(entity);
        }
    }
    if shown.is_none() {
        for entity in document.shown_only.iter().filter(|e| document.is_visible(e)) {
            draw_entity(&mut shapes, ctx, screen, document, entity, &Look::Own);
        }
    }
    draw_axes_icon(&mut shapes, ctx, screen.rect);
    draw_work(&mut shapes, ctx, screen, controller, overlay, selected);
    if let (Some(pointer), false) = (overlay.pointer, overlay.panning) {
        draw_crosshair(&mut shapes, screen.rect, pointer, controller.picking());
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
