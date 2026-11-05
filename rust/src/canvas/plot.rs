//! Plotting the drawing to a PDF page at a scale, the way it would come out of a plotter.

use std::path::Path;
use std::sync::Arc;

use super::aci::{BY_BLOCK, BY_LAYER, color_rgb, index_of};
use super::pdf::{PdfPage, helvetica_width};
use super::shapes::{Styled, TextSpec, flatten, resolve_color};
use crate::commands::format_significant;
use crate::fonts::{self, FontFile, PathOp};
use crate::io::ctb::{self, PlotStyleTable};
use crate::io::dxf::mm_per_unit;
use crate::model::document::{Document, PlotSettings};
use crate::model::entities::{Entity, HAlign, Shape, TextAlign, VAlign};
use crate::model::geometry::{Point, direction, pt};
use crate::model::index::Bounds;
use crate::model::lineweight;
use crate::model::mtext::{self, RunStyle};
use crate::model::parts::{BOLD_OFFSET, TextLook};
use crate::model::styles::{BYLAYER_LINETYPE, dash_pattern};
use crate::model::viewport::{frame_corners, viewport_scale};

pub const MM_PER_INCH: f64 = 25.4;
pub const POINTS_PER_INCH: f64 = 72.0;
pub const MARGIN_MM: f64 = 10.0;
/// The length on paper of one unit of a linetype's dash pattern.
const DASH_UNIT_MM: f64 = 0.25;
/// Points (the objects) print as a dot at least this wide, in PDF points, even with no lineweight.
const MIN_POINT_DOT: f64 = 0.7;
pub const FIT: &str = "Fit";
pub const SCALE_HELP: &str = "Enter a scale such as 1:50 or 2:1, or Fit.";
pub const NOTHING_TO_PLOT: &str = "There is nothing to plot.";
/// How far apart a gradient's lines print, in PDF points.
const GRADIENT_STEP_POINTS: f64 = 1.0;
const CHORD_MM: f64 = 0.01; // how far a curve's chords may stray from it on paper

/// Paper sizes in millimeters, portrait.
pub const PAPER_SIZES: [(&str, f64, f64); 8] = [
    ("A4", 210.0, 297.0),
    ("A3", 297.0, 420.0),
    ("A2", 420.0, 594.0),
    ("A1", 594.0, 841.0),
    ("A0", 841.0, 1189.0),
    ("Letter", 215.9, 279.4),
    ("Legal", 215.9, 355.6),
    ("Tabloid", 279.4, 431.8),
];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlotLayout {
    pub scale: f64,     // paper length per real length, so 0.02 is 1:50
    pub source: Bounds, // the part of the drawing that lands on the page, in drawing units
    pub fits: bool,     // whether the whole drawing is on the page
}

/// Paper per real length from "1:50" or "2:1". None means fit the page. The error is SCALE_HELP.
pub fn parse_scale(text: &str) -> Result<Option<f64>, &'static str> {
    let text = text.trim();
    if text.eq_ignore_ascii_case(FIT) {
        return Ok(None);
    }
    let (paper, real) = text.split_once(':').ok_or(SCALE_HELP)?;
    let scale = match (paper.trim().parse::<f64>(), real.trim().parse::<f64>()) {
        (Ok(paper), Ok(real)) if real != 0.0 => paper / real,
        _ => 0.0,
    };
    if scale > 0.0 && scale.is_finite() { Ok(Some(scale)) } else { Err(SCALE_HELP) }
}

pub fn format_scale(scale: f64) -> String {
    if scale >= 1.0 { format!("{}:1", format_significant(scale, 4)) } else { format!("1:{}", format_significant(1.0 / scale, 4)) }
}

/// Where the drawing lands on a page of the given size. Fit picks the largest scale that holds it all.
pub fn plot_layout(extent: Bounds, page: (f64, f64), points_per_mm: f64, mm_per_drawing_unit: f64, scale: Option<f64>) -> PlotLayout {
    let (width, height) = page;
    let per_real_length = points_per_mm * mm_per_drawing_unit;
    let per_unit = match scale {
        None => [(width, extent.width()), (height, extent.height())]
            .iter()
            .filter(|(_, size)| *size > 0.0)
            .map(|(room, size)| room / size)
            .reduce(f64::min)
            .unwrap_or(per_real_length),
        Some(scale) => per_real_length * scale,
    };
    let center = extent.center();
    let half = pt(width / per_unit / 2.0, height / per_unit / 2.0);
    let source = Bounds { x0: center.x - half.x, y0: center.y - half.y, x1: center.x + half.x, y1: center.y + half.y };
    let fits = extent.width() * per_unit <= width + 1e-6 && extent.height() * per_unit <= height + 1e-6;
    PlotLayout { scale: per_unit / per_real_length, source, fits }
}

/// The page with the visible part of a non-empty document on it. Lines print at their lineweights, not at
/// drawing scale, and white lines (meant for a dark screen) print black.
pub fn plot_page(
    document: &Document,
    paper: (f64, f64),
    landscape: bool,
    scale: Option<f64>,
) -> Result<(PdfPage, PlotLayout), &'static str> {
    let extent = document.extent().ok_or(NOTHING_TO_PLOT)?;
    let points_per_mm = POINTS_PER_INCH / MM_PER_INCH;
    let (paper_width, paper_height) = if landscape { (paper.1, paper.0) } else { paper };
    let (page_width, page_height) = (paper_width * points_per_mm, paper_height * points_per_mm);
    let margin = MARGIN_MM * points_per_mm;
    let printable = (page_width - 2.0 * margin, page_height - 2.0 * margin);
    let layout = plot_layout(extent, printable, points_per_mm, mm_per_unit(document), scale);
    let per_unit = printable.0 / layout.source.width();
    let place = |p: Point| pt(margin + (p.x - layout.source.x0) * per_unit, margin + (p.y - layout.source.y0) * per_unit);

    let mut page = PdfPage::new(page_width, page_height);
    page.clip(margin, margin, printable.0, printable.1);
    let pens = Pens::new(document, &document.model_plot);
    let tolerance = CHORD_MM * points_per_mm / per_unit;
    for entity in document.entities() {
        if !document.is_visible(entity) || !document.bounds_of(entity.id).is_some_and(|b| b.touches(&layout.source)) {
            continue;
        }
        plot_entity(&mut page, document, entity, &place, per_unit, tolerance, &pens);
    }
    Ok((page, layout))
}

/// How a plot inks what it prints, as the page setup says: through a plot style table or not, with objects'
/// lineweights or every line as thin as can be.
struct Pens {
    table: Option<Arc<PlotStyleTable>>,
    lineweights: bool,
    points_per_mm: f64,
}

impl Pens {
    fn new(document: &Document, settings: &PlotSettings) -> Pens {
        let beside = document.path.as_deref().and_then(Path::parent);
        let table = ctb::load(&settings.style_table, beside);
        // Plotting with plot styles always prints lineweights: the table's own, or the objects' where it says so.
        Pens { lineweights: settings.lineweights || table.is_some(), table, points_per_mm: POINTS_PER_INCH / MM_PER_INCH }
    }

    /// The color a part prints in and its line width in points. White (meant for a dark screen) prints black;
    /// a plot style table then picks by the part's index color.
    fn ink(&self, style: &Styled, document: &Document) -> ((u8, u8, u8), f64) {
        let mut rgb = resolve_color(style, document);
        if rgb == (255, 255, 255) {
            rgb = (0, 0, 0);
        }
        let layer = document.layer(&style.layer);
        let pen = self.table.as_ref().map(|table| {
            let color = if style.color == BY_LAYER || style.color == BY_BLOCK { layer.map_or(7, |l| l.color) } else { style.color };
            table.pen(index_of(color))
        });
        let own =
            || self.lineweights.then(|| lineweight::resolve_mm(style.lineweight, layer.map(|l| l.lineweight), document.default_lineweight));
        let mm = pen.as_ref().and_then(|p| p.lineweight).or_else(own).unwrap_or(0.0);
        (pen.map_or(rgb, |p| p.apply(rgb)), mm * self.points_per_mm)
    }
}

/// One entity on the page, each point placed by `place`, which makes `per_unit` points of a drawing unit.
fn plot_entity(
    page: &mut PdfPage,
    document: &Document,
    entity: &Entity,
    place: &dyn Fn(Point) -> Point,
    per_unit: f64,
    tolerance: f64,
    pens: &Pens,
) {
    if let Shape::Image { path, position, u, v, .. } = &entity.shape
        && document.layer(&entity.layer).is_none_or(|layer| layer.shown() && layer.plot)
    {
        let corner = place(*position);
        plot_image(page, document, path, corner, place(*position + *u) - corner, place(*position + *v) - corner);
    }
    for (style, parts) in flatten(entity, tolerance) {
        // Layers that are off, frozen or set not to plot stay off the page.
        if document.layer(&style.layer).is_some_and(|layer| !layer.shown() || !layer.plot) {
            continue;
        }
        let (rgb, weight) = pens.ink(&style, document);
        let linetype = if style.linetype.eq_ignore_ascii_case(BYLAYER_LINETYPE) {
            document.layer(&style.layer).map_or(String::new(), |l| l.linetype.clone())
        } else {
            style.linetype.clone()
        };
        // Dashes are as long on paper whatever the scale, as AutoCAD's are at a linetype scale of 1 on a sheet.
        let dashes: Vec<f64> = dash_pattern(&linetype).iter().map(|d| *d as f64 * DASH_UNIT_MM * pens.points_per_mm).collect();
        for path in &parts.paths {
            page.stroke(&path.iter().map(|p| place(*p)).collect::<Vec<_>>(), rgb, weight, &dashes);
        }
        for triangle in &parts.fills {
            page.fill(&triangle.map(place), rgb);
        }
        for (loops, gradient) in &parts.gradients {
            for (a, b, color) in crate::model::parts::gradient_strips(loops, gradient, GRADIENT_STEP_POINTS / per_unit) {
                page.stroke(&[place(a), place(b)], color, GRADIENT_STEP_POINTS * 1.3, &[]);
            }
        }
        for area in &parts.areas {
            page.fill_area(&area.iter().map(|l| l.iter().map(|p| place(*p)).collect()).collect::<Vec<_>>(), rgb);
        }
        for point in &parts.points {
            let p = place(*point);
            page.stroke(&[p, p + pt(0.01, 0.0)], rgb, (weight * 2.0).max(MIN_POINT_DOT), &[]);
        }
        for spec in &parts.texts {
            let on_page = TextSpec { origin: place(spec.origin), height: spec.height * per_unit, ..spec.clone() };
            plot_text(page, &on_page, rgb, weight);
        }
    }
}

/// A picture on the page, read from its file (beside the drawing when its path is relative). Its transparent parts
/// print white. A file that cannot be read leaves just the frame.
fn plot_image(page: &mut PdfPage, document: &Document, path: &str, corner: Point, along: Point, up: Point) {
    let written = std::path::Path::new(path);
    let file = match document.path.as_deref().and_then(std::path::Path::parent) {
        Some(folder) if written.is_relative() => folder.join(written),
        _ => written.to_path_buf(),
    };
    let Ok(picture) = image::open(&file) else { return };
    let picture = picture.to_rgba8();
    let rgb: Vec<u8> = picture
        .pixels()
        .flat_map(|p| {
            let alpha = p[3] as u32;
            [0, 1, 2].map(|c| ((p[c] as u32 * alpha + 255 * (255 - alpha)) / 255) as u8)
        })
        .collect();
    page.image(corner, along, up, (picture.width(), picture.height()), &rgb);
}

/// An open layout's sheet as a page of the same size, at 1:1: what is drawn on it, and in each viewport the part
/// of model space it shows, cut off at its frame. Sheets are in millimeters, or inches when they are that small.
pub fn plot_layout_page(document: &Document) -> Result<PdfPage, &'static str> {
    let index = document.shown_layout().ok_or(NOTHING_TO_PLOT)?;
    let layout = document.layouts.get(index).ok_or(NOTHING_TO_PLOT)?;
    let sheet = layout.sheet();
    let mm_per_unit = if sheet.width().max(sheet.height()) < 50.0 { MM_PER_INCH } else { 1.0 };
    let per_unit = POINTS_PER_INCH / MM_PER_INCH * mm_per_unit;
    let place = |p: Point| (p - pt(sheet.x0, sheet.y0)) * per_unit;
    let mut page = PdfPage::new(sheet.width() * per_unit, sheet.height() * per_unit);
    let pens = Pens::new(document, &layout.plot);
    let tolerance = CHORD_MM / mm_per_unit;
    let model = document.model();
    for entity in document.space_entities(Some(index)).iter().filter(|e| document.is_visible(e)) {
        if let Shape::Viewport { center, width, height, view_center, view_height, frozen, .. } = &entity.shape {
            let scale = viewport_scale(*height, *view_height);
            let frame = Bounds::around(frame_corners(*center, *width, *height));
            let half = pt(width / scale / 2.0, view_height / 2.0);
            let seen = Bounds::from_corners(*view_center - half, *view_center + half);
            let low = place(pt(frame.x0, frame.y0));
            page.save();
            page.clip(low.x, low.y, frame.width() * per_unit, frame.height() * per_unit);
            let into_sheet = |m: Point| place(*center + (m - *view_center) * scale);
            for id in model.ids_in(&seen) {
                let Some(shown) = model.get(id) else { continue };
                if document.is_visible(shown) && !frozen.iter().any(|l| l.eq_ignore_ascii_case(&shown.layer)) {
                    plot_entity(&mut page, document, shown, &into_sheet, per_unit * scale, tolerance / scale, &pens);
                }
            }
            page.restore();
        }
        plot_entity(&mut page, document, entity, &place, per_unit, tolerance, &pens);
    }
    Ok(page)
}

pub fn export_layout_pdf(document: &Document, path: &Path) -> Result<(), String> {
    let page = plot_layout_page(document).map_err(str::to_string)?;
    let name = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
    std::fs::write(path, page.to_bytes()).map_err(|error| format!("Could not write {name}: {error}"))
}

/// How long a piece of text runs on the page.
fn plot_text_width(text: &str, size: f64, font: &str, look: &TextLook) -> f64 {
    let catalog = fonts::catalog();
    if catalog.resolve(font).is_none()
        && let Some(shx) = crate::shx::text_font(font)
    {
        return crate::shx::line_strokes(&shx, &fonts::display_text(text), size, look.width_factor).1;
    }
    let chain = font_chain(font);
    if chain.is_empty() {
        helvetica_width(text, size) * look.width_factor
    } else {
        fonts::outline_line(text, &chain, size).1 * look.width_factor
    }
}

fn font_chain(font: &str) -> Vec<FontFile> {
    let catalog = fonts::catalog();
    (!font.is_empty()).then(|| catalog.resolve(font)).flatten().into_iter().chain(catalog.ui_font()).chain(catalog.fallbacks()).collect()
}

/// Text in an SHX font as its pen strokes, `weight` wide like the drawing's lines. Other text as the outlines of
/// its glyphs, from the font the drawing asks for and the fallback fonts, so every script prints as it shows on
/// screen. Without any fonts on the computer it falls back to the PDF's Helvetica. Runs with looks of their own
/// print one by one, and bold is the letters printed twice, a little apart.
fn plot_text(page: &mut PdfPage, spec: &TextSpec, rgb: (u8, u8, u8), weight: f64) {
    if let Some(rich) = &spec.rich {
        let run_font = |style: &RunStyle| style.font.clone().unwrap_or_else(|| spec.font.clone());
        let run_look = |style: &RunStyle| TextLook { bold: style.bold, italic: style.italic, ..spec.look };
        let measure = |text: &str, size: f64, style: &RunStyle| plot_text_width(text, size, &run_font(style), &run_look(style));
        let (placed, rules) = mtext::layout(rich, spec.height, spec.rotation, |widths| spec.line_origins(widths), &measure);
        for run in placed {
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
            let color = run.style.color.and_then(color_rgb).filter(|c| *c != (255, 255, 255)).unwrap_or(rgb);
            plot_text(page, &piece, color, weight);
        }
        for rule in rules {
            page.stroke(&rule, rgb, weight, &[]);
        }
        return;
    }
    if spec.look.bold {
        let offset = direction(spec.rotation) * (spec.height * BOLD_OFFSET);
        for origin in [spec.origin, spec.origin + offset] {
            plot_text(page, &TextSpec { origin, look: TextLook { bold: false, ..spec.look }, ..spec.clone() }, rgb, weight);
        }
        return;
    }
    let catalog = fonts::catalog();
    let truetype = catalog.resolve(&spec.font).is_some();
    if !truetype && let Some(font) = crate::shx::text_font(&spec.font) {
        for points in crate::shx::text_strokes(&font, spec) {
            page.stroke(&points, rgb, weight, &[]);
        }
        return;
    }
    let chain = font_chain(&spec.font);
    let size = spec.height;
    let along = direction(spec.rotation);
    let up = direction(spec.rotation + 90.0);
    let lines: Vec<&str> = spec.text.split('\n').collect();
    if chain.is_empty() {
        let widths: Vec<f64> = lines.iter().map(|l| helvetica_width(l, size)).collect();
        for (line, start) in lines.iter().zip(spec.line_origins(&widths)) {
            page.text(start, spec.rotation, size, line, rgb);
        }
        return;
    }
    let (stretch, shear) = (spec.look.width_factor, spec.look.shear());
    let outlines: Vec<(Vec<PathOp>, f64)> = lines.iter().map(|line| fonts::outline_line(line, &chain, size)).collect();
    let widths: Vec<f64> = outlines.iter().map(|(_, width)| *width * stretch).collect();
    for ((ops, _), start) in outlines.iter().zip(spec.line_origins(&widths)) {
        let place = |p: Point| start + along * ((p.x + p.y * shear) * stretch) + up * p.y;
        let placed: Vec<PathOp> = ops
            .iter()
            .map(|op| match *op {
                PathOp::Move(p) => PathOp::Move(place(p)),
                PathOp::Line(p) => PathOp::Line(place(p)),
                PathOp::Curve(a, b, p) => PathOp::Curve(place(a), place(b), place(p)),
                PathOp::Close => PathOp::Close,
            })
            .collect();
        page.fill_path(&placed, rgb);
    }
}

pub fn export_pdf(document: &Document, path: &Path, paper: (f64, f64), landscape: bool, scale: Option<f64>) -> Result<PlotLayout, String> {
    let (page, layout) = plot_page(document, paper, landscape, scale).map_err(str::to_string)?;
    let name = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
    std::fs::write(path, page.to_bytes()).map_err(|error| format!("Could not write {name}: {error}"))?;
    Ok(layout)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::entities::line;

    #[test]
    fn scales_read_and_print() {
        assert_eq!(parse_scale("1:50"), Ok(Some(0.02)));
        assert_eq!(parse_scale(" fit "), Ok(None));
        assert_eq!(parse_scale("2:1"), Ok(Some(2.0)));
        for bad in ["50", "1:0", "a:b", "-1:2"] {
            assert_eq!(parse_scale(bad), Err(SCALE_HELP), "{bad}");
        }
        assert_eq!(format_scale(0.02), "1:50");
        assert_eq!(format_scale(2.0), "2:1");
    }

    #[test]
    fn fit_uses_the_tighter_direction() {
        let extent = Bounds { x0: 0.0, y0: 0.0, x1: 1000.0, y1: 100.0 };
        let layout = plot_layout(extent, (100.0, 100.0), 1.0, 1.0, None);
        assert!((layout.scale - 0.1).abs() < 1e-12);
        assert!(layout.fits);
        let at_one = plot_layout(extent, (100.0, 100.0), 1.0, 1.0, Some(1.0));
        assert!(!at_one.fits);
        assert_eq!(at_one.source.center(), extent.center());
    }

    #[test]
    fn an_empty_drawing_cannot_be_plotted() {
        assert_eq!(plot_page(&Document::new(), (210.0, 297.0), true, None).err(), Some(NOTHING_TO_PLOT));
    }

    #[test]
    fn a_drawing_at_one_to_one_plots_true_size() {
        let document = Document::with_entities(vec![line(pt(0.0, 0.0), pt(100.0, 0.0))]);
        let (_, layout) = plot_page(&document, (210.0, 297.0), false, Some(1.0)).unwrap();
        assert!((layout.source.width() - 190.0).abs() < 1e-9, "the printable width of A4 in millimeters");
    }

    #[test]
    fn a_layout_plots_its_sheet_with_model_space_inside_its_viewports() {
        let mut document = Document::with_entities(vec![line(pt(0.0, 0.0), pt(5000.0, 0.0)), line(pt(0.0, 9000.0), pt(5000.0, 9000.0))]);
        let viewport = Shape::Viewport {
            center: pt(100.0, 100.0),
            width: 100.0,
            height: 50.0,
            view_center: pt(2500.0, 0.0),
            view_height: 5000.0,
            frozen: vec![],
            locked: false,
        };
        document.set_space_entities(Some(0), vec![Entity::new(viewport)]);
        document.activate(Some(0));
        let page = plot_layout_page(&document).unwrap();
        let pdf = String::from_utf8_lossy(&page.to_bytes()).into_owned();
        assert!(pdf.contains("/MediaBox [0 0 1190.55"), "an A3 sheet across, at 1:1");
        assert_eq!(pdf.matches(" re W n").count(), 1, "one viewport, clipped");
        assert!(pdf.contains("q\n") && pdf.contains("Q\n"));
    }
}
