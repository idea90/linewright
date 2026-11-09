//! A picture of the drawing as a PNG, drawn the way a plot prints it: dark lines on white paper.

use tiny_skia::{
    FillRule, FilterQuality, IntSize, LineCap, LineJoin, Mask, Paint, PathBuilder, Pattern, Pixmap, SpreadMode, Stroke, StrokeDash,
    Transform,
};

use super::pdf::Page;
use super::plot::{draw_model, draw_sheet, open_sheet};
use crate::fonts::PathOp;
use crate::model::document::Document;
use crate::model::geometry::{Point, pt};
use crate::model::index::Bounds;

/// The largest picture made, in pixels on a side.
pub const MAX_SIDE: u32 = 4096;
/// Room left around the drawing when the picture shows all of it, as a share of its size.
const MARGIN: f64 = 0.04;
/// How far a curve's chords may stray from it, in pixels.
const CHORD_PIXELS: f64 = 0.25;

/// A page of pixels, y up like the plot's pages.
pub struct RasterPage {
    pixmap: Pixmap,
    /// The clipping in force, and those saved before it.
    clip: Option<Mask>,
    saved: Vec<Option<Mask>>,
}

impl RasterPage {
    pub fn new(width: u32, height: u32) -> Option<RasterPage> {
        let mut pixmap = Pixmap::new(width, height)?;
        pixmap.fill(tiny_skia::Color::WHITE);
        Some(RasterPage { pixmap, clip: None, saved: vec![] })
    }

    pub fn png(&self) -> Result<Vec<u8>, String> {
        self.pixmap.encode_png().map_err(|error| error.to_string())
    }

    fn device(&self, p: Point) -> (f32, f32) {
        (p.x as f32, (self.pixmap.height() as f64 - p.y) as f32)
    }

    fn polygon(&self, points: &[Point], closed: bool) -> Option<tiny_skia::Path> {
        let mut builder = PathBuilder::new();
        self.add_polygon(&mut builder, points, closed);
        builder.finish()
    }

    fn add_polygon(&self, builder: &mut PathBuilder, points: &[Point], closed: bool) {
        let mut corners = points.iter().map(|p| self.device(*p));
        let Some((x, y)) = corners.next() else { return };
        builder.move_to(x, y);
        for (x, y) in corners {
            builder.line_to(x, y);
        }
        if closed {
            builder.close();
        }
    }

    fn fill_shape(&mut self, path: &tiny_skia::Path, paint: &Paint, rule: FillRule) {
        self.pixmap.fill_path(path, paint, rule, Transform::identity(), self.clip.as_ref());
    }
}

fn paint((r, g, b): (u8, u8, u8)) -> Paint<'static> {
    let mut paint = Paint::default();
    paint.set_color_rgba8(r, g, b, 255);
    paint.anti_alias = true;
    paint
}

impl Page for RasterPage {
    fn save(&mut self) {
        self.saved.push(self.clip.clone());
    }

    fn restore(&mut self) {
        self.clip = self.saved.pop().flatten();
    }

    fn clip(&mut self, x: f64, y: f64, width: f64, height: f64) {
        let corners = [pt(x, y), pt(x + width, y), pt(x + width, y + height), pt(x, y + height)];
        let Some(path) = self.polygon(&corners, true) else { return };
        match &mut self.clip {
            Some(mask) => mask.intersect_path(&path, FillRule::Winding, false, Transform::identity()),
            None => {
                let Some(mut mask) = Mask::new(self.pixmap.width(), self.pixmap.height()) else { return };
                mask.fill_path(&path, FillRule::Winding, false, Transform::identity());
                self.clip = Some(mask);
            }
        }
    }

    fn stroke(&mut self, points: &[Point], rgb: (u8, u8, u8), width: f64, dashes: &[f64]) {
        if points.len() < 2 {
            return;
        }
        let Some(path) = self.polygon(points, false) else { return };
        // A dash pattern needs pairs of dash and gap: an odd one runs twice, as PDF does it.
        let mut pattern: Vec<f32> = dashes.iter().map(|d| *d as f32).collect();
        if pattern.len() % 2 == 1 {
            pattern.extend_from_slice(&pattern.clone());
        }
        let stroke = Stroke {
            width: width.max(1.0) as f32,
            line_cap: LineCap::Round,
            line_join: LineJoin::Round,
            dash: StrokeDash::new(pattern, 0.0),
            ..Stroke::default()
        };
        self.pixmap.stroke_path(&path, &paint(rgb), &stroke, Transform::identity(), self.clip.as_ref());
    }

    fn fill(&mut self, corners: &[Point], rgb: (u8, u8, u8)) {
        if corners.len() >= 3
            && let Some(path) = self.polygon(corners, true)
        {
            self.fill_shape(&path, &paint(rgb), FillRule::Winding);
        }
    }

    fn fill_path(&mut self, ops: &[PathOp], rgb: (u8, u8, u8)) {
        let mut builder = PathBuilder::new();
        for op in ops {
            match *op {
                PathOp::Move(p) => {
                    let (x, y) = self.device(p);
                    builder.move_to(x, y);
                }
                PathOp::Line(p) => {
                    let (x, y) = self.device(p);
                    builder.line_to(x, y);
                }
                PathOp::Curve(a, b, p) => {
                    let ((ax, ay), (bx, by), (x, y)) = (self.device(a), self.device(b), self.device(p));
                    builder.cubic_to(ax, ay, bx, by, x, y);
                }
                PathOp::Close => builder.close(),
            }
        }
        if let Some(path) = builder.finish() {
            self.fill_shape(&path, &paint(rgb), FillRule::Winding);
        }
    }

    fn fill_area(&mut self, loops: &[Vec<Point>], rgb: (u8, u8, u8)) {
        let mut builder = PathBuilder::new();
        for points in loops.iter().filter(|l| l.len() >= 3) {
            self.add_polygon(&mut builder, points, true);
        }
        if let Some(path) = builder.finish() {
            self.fill_shape(&path, &paint(rgb), FillRule::EvenOdd);
        }
    }

    /// A picture has no font of its own: without fonts on the computer, text is left out.
    fn text(&mut self, _origin: Point, _rotation: f64, _size: f64, _value: &str, _rgb: (u8, u8, u8)) {}

    fn image(&mut self, corner: Point, along: Point, up: Point, (width, height): (u32, u32), rgb: &[u8]) {
        let Some(size) = IntSize::from_wh(width, height) else { return };
        let rgba: Vec<u8> = rgb.as_chunks::<3>().0.iter().flat_map(|[r, g, b]| [*r, *g, *b, 255]).collect();
        let Some(picture) = Pixmap::from_vec(rgba, size) else { return };
        // Pixel (x, y), counted from the top, lands at corner + along * x / width + up * (1 - y / height), then y flips.
        let (w, h, page_height) = (width as f64, height as f64, self.pixmap.height() as f64);
        let placing = Transform::from_row(
            (along.x / w) as f32,
            (-along.y / w) as f32,
            (-up.x / h) as f32,
            (up.y / h) as f32,
            (corner.x + up.x) as f32,
            (page_height - corner.y - up.y) as f32,
        );
        let shader = Pattern::new(picture.as_ref(), SpreadMode::Pad, FilterQuality::Bilinear, 1.0, placing);
        let mut paint = Paint { shader, ..Paint::default() };
        paint.anti_alias = true;
        let frame = [corner, corner + along, corner + along + up, corner + up];
        if let Some(path) = self.polygon(&frame, true) {
            self.fill_shape(&path, &paint, FillRule::Winding);
        }
    }
}

/// What a picture of the drawing shows: the open layout's sheet, or an area of model space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PictureView {
    /// The area shown, in drawing units (sheet units on a layout), and how many pixels a unit is.
    pub area: Bounds,
    pub pixels_per_unit: f64,
}

/// A PNG of the open space, at most `width` by `height` pixels, keeping the drawing's proportions. On a layout it
/// shows the sheet; in model space `window` (or everything there is) with a little room around it.
pub fn picture(document: &Document, width: u32, height: u32, window: Option<Bounds>) -> Result<(Vec<u8>, PictureView), String> {
    let sheet = open_sheet(document);
    let area = match (sheet, window) {
        (Some(sheet), None) => sheet,
        (_, Some(window)) => window,
        (None, None) => {
            let extent = document.extent().ok_or("There is nothing to show: the drawing is empty.")?;
            let room = extent.width().max(extent.height()).max(1e-9) * MARGIN;
            Bounds { x0: extent.x0 - room, y0: extent.y0 - room, x1: extent.x1 + room, y1: extent.y1 + room }
        }
    };
    let (area_width, area_height) = (area.width().max(1e-9), area.height().max(1e-9));
    let (width, height) = (width.clamp(16, MAX_SIDE), height.clamp(16, MAX_SIDE));
    let per_unit = (width as f64 / area_width).min(height as f64 / area_height);
    let size = |length: f64| ((length * per_unit).round() as u32).clamp(1, MAX_SIDE);
    let mut page = RasterPage::new(size(area_width), size(area_height)).ok_or("The picture could not be made.")?;
    let place = |p: Point| pt((p.x - area.x0) * per_unit, (p.y - area.y0) * per_unit);
    let tolerance = CHORD_PIXELS / per_unit;
    match sheet {
        Some(_) => draw_sheet(&mut page, document, &place, per_unit, tolerance),
        None => draw_model(&mut page, document, &area, &place, per_unit, tolerance),
    }
    Ok((page.png()?, PictureView { area, pixels_per_unit: per_unit }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::entities::line;

    fn dark_pixels(png: &[u8]) -> usize {
        let picture = image::load_from_memory(png).unwrap().to_rgb8();
        picture.pixels().filter(|p| p[0] < 128 && p[1] < 128 && p[2] < 128).count()
    }

    #[test]
    fn a_picture_shows_the_drawing_in_its_proportions() {
        let document = Document::with_entities(vec![line(pt(0.0, 0.0), pt(100.0, 0.0)), line(pt(0.0, 0.0), pt(0.0, 50.0))]);
        let (png, view) = picture(&document, 800, 800, None).unwrap();
        let shown = image::load_from_memory(&png).unwrap();
        assert_eq!(shown.width(), 800);
        assert_eq!(shown.height(), 430, "108 by 58 units with the room around them");
        assert!(view.area.x0 < 0.0 && view.area.x1 > 100.0);
        assert!(dark_pixels(&png) > 1000, "the lines are drawn dark on white");
    }

    #[test]
    fn a_window_shows_only_what_is_inside_it() {
        let document = Document::with_entities(vec![line(pt(0.0, 0.0), pt(100.0, 0.0))]);
        let window = Bounds { x0: 0.0, y0: 10.0, x1: 100.0, y1: 60.0 };
        let (png, _) = picture(&document, 400, 400, Some(window)).unwrap();
        assert_eq!(dark_pixels(&png), 0);
    }

    #[test]
    fn an_empty_drawing_has_no_picture() {
        assert!(picture(&Document::new(), 400, 400, None).is_err());
    }
}
