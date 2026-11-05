//! What an entity is made of, in drawing units, ready for any painter: paths to stroke, filled areas, points and
//! text. Block inserts are expanded into the parts of their block's entities, each with the layer, color and
//! linetype it shows in.

use std::f64::consts::TAU;

use std::sync::Arc;

use super::annotation::annotation_parts;
use super::dimension::dimension_parts;
use super::entities::{Entity, Gradient, HAlign, LINE_PITCH, PatternLine, Shape, TextAlign, VAlign, Vertex};
use super::geometry::{Point, bulge_arc, direction, modulo, pt};
use super::mtext::{RichText, valid_rich};
use super::transform::{IDENTITY, Similarity, insert_transform};
use super::viewport::frame_corners;

const MAX_ARC_SEGMENTS: f64 = 2048.0;
const MAX_SPLINE_SAMPLES: usize = 4096;
const MAX_HATCH_SEGMENTS: usize = 20000;
const MAX_BLOCK_DEPTH: usize = 16;
pub const BYBLOCK_COLOR: i32 = 0;
pub const BYBLOCK_LINETYPE: &str = "ByBlock";

#[derive(Clone, Debug, PartialEq)]
pub struct TextSpec {
    /// The point the text is aligned on, as `align` says.
    pub origin: Point,
    pub rotation: f64, // degrees, counter-clockwise
    pub height: f64,
    pub text: String,
    pub align: TextAlign,
    pub line_spacing: f64,
    /// The font the drawing asks for, empty for the interface font.
    pub font: String,
    pub look: TextLook,
    /// Runs with looks of their own, in place of `text` when there are any.
    pub rich: Option<Arc<RichText>>,
}

/// How letters are shaped: width, lean, bold and italic.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextLook {
    pub width_factor: f64,
    pub oblique: f64,
    pub bold: bool,
    pub italic: bool,
}

impl Default for TextLook {
    fn default() -> Self {
        TextLook { width_factor: 1.0, oblique: 0.0, bold: false, italic: false }
    }
}

impl TextLook {
    /// How far a letter's top leans over per its height. Italic leans 15 degrees more.
    pub fn shear(&self) -> f64 {
        (self.oblique + if self.italic { 15.0 } else { 0.0 }).to_radians().tan()
    }

    pub fn is_plain(&self) -> bool {
        *self == TextLook::default()
    }
}

/// Fake bold: how far the letters are drawn again to the side, per the text's height.
pub const BOLD_OFFSET: f64 = 0.035;

impl TextSpec {
    /// Where each line's baseline starts, given how wide each line is: lines step down from the first, and each
    /// lines up on the origin as the alignment says.
    pub fn line_origins(&self, widths: &[f64]) -> Vec<Point> {
        let lines = widths.len().max(1) as f64;
        let pitch = self.height * LINE_PITCH * self.line_spacing;
        let first = match self.align.v {
            VAlign::Baseline => 0.0,
            VAlign::Bottom => (lines - 1.0) * pitch,
            VAlign::Middle => (self.height + (lines - 1.0) * pitch) / 2.0 - self.height,
            VAlign::Top => -self.height,
        };
        let (along, up) = (direction(self.rotation), direction(self.rotation + 90.0));
        widths
            .iter()
            .enumerate()
            .map(|(i, width)| {
                let back = match self.align.h {
                    HAlign::Left => 0.0,
                    HAlign::Center => width / 2.0,
                    HAlign::Right => *width,
                };
                self.origin + up * (first - i as f64 * pitch) - along * back
            })
            .collect()
    }

    /// The corners of a box around the text, guessing each character `char_width` heights wide. Generous below
    /// the baseline for descenders.
    pub fn bounding_box(&self, char_width: f64) -> [Point; 4] {
        let lines: Vec<&str> = self.text.split('\n').collect();
        let widths: Vec<f64> = lines.iter().map(|l| l.chars().count().max(1) as f64 * char_width * self.height).collect();
        let origins = self.line_origins(&widths);
        let (along, up) = (direction(self.rotation), direction(self.rotation + 90.0));
        let descent = 0.3 * self.height;
        let local = |p: Point| ((p - self.origin).dot(along), (p - self.origin).dot(up));
        let (mut u0, mut v0, mut u1, mut v1) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
        for (origin, width) in origins.iter().zip(&widths) {
            let (u, v) = local(*origin);
            u0 = u0.min(u);
            u1 = u1.max(u + width);
            v0 = v0.min(v - descent);
            v1 = v1.max(v + self.height);
        }
        [(u0, v0), (u1, v0), (u1, v1), (u0, v1)].map(|(u, v)| self.origin + along * u + up * v)
    }

    /// The text after a transform. Mirrored text turns round and lines up from the other side, so it stays
    /// readable where the mirrored text would be.
    fn mapped(&self, transform: &Similarity) -> TextSpec {
        let along = transform.linear(direction(self.rotation));
        let up = transform.linear(direction(self.rotation + 90.0));
        let mut rotation = along.y.atan2(along.x).to_degrees();
        let mut align = self.align;
        if transform.reflects() {
            rotation += 180.0;
            align.h = match align.h {
                HAlign::Left => HAlign::Right,
                HAlign::Right => HAlign::Left,
                HAlign::Center => HAlign::Center,
            };
        }
        TextSpec { origin: transform.point(self.origin), rotation, height: self.height * up.length(), align, ..self.clone() }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Parts {
    pub paths: Vec<Vec<Point>>,
    /// Filled triangles, such as arrowheads.
    pub fills: Vec<[Point; 3]>,
    /// Filled areas, each a set of loops where loops inside loops are holes.
    pub areas: Vec<Vec<Vec<Point>>>,
    pub points: Vec<Point>,
    pub texts: Vec<TextSpec>,
    /// Gradient fills: the loops, as for areas, and the colors.
    pub gradients: Vec<(Vec<Vec<Point>>, Gradient)>,
}

impl Parts {
    fn map(mut self, transform: &Similarity) -> Parts {
        let point = |p: &mut Point| *p = transform.point(*p);
        self.paths.iter_mut().flatten().for_each(point);
        self.fills.iter_mut().flatten().for_each(point);
        self.areas.iter_mut().flatten().flatten().for_each(point);
        self.points.iter_mut().for_each(point);
        for (loops, gradient) in &mut self.gradients {
            loops.iter_mut().flatten().for_each(point);
            let along = transform.linear(direction(gradient.angle));
            gradient.angle = along.y.atan2(along.x).to_degrees();
        }
        self.texts = self.texts.iter().map(|t| t.mapped(transform)).collect();
        self
    }
}

/// Points along an arc, close enough to it that no chord strays more than `tolerance` from it.
pub fn arc_points(center: Point, radius: f64, start: f64, sweep: f64, tolerance: f64) -> Vec<Point> {
    let step = if radius > tolerance { 2.0 * (1.0 - tolerance / radius).acos() } else { TAU };
    let count = (sweep.abs() / step.max(1e-9)).ceil().clamp(1.0, MAX_ARC_SEGMENTS) as usize;
    (0..=count)
        .map(|i| {
            let angle = start + sweep * i as f64 / count as f64;
            pt(center.x + radius * angle.cos(), center.y + radius * angle.sin())
        })
        .collect()
}

/// The point of an ellipse at a parameter (radians).
pub fn ellipse_point(center: Point, major: Point, ratio: f64, t: f64) -> Point {
    let minor = pt(-major.y, major.x) * ratio;
    center + major * t.cos() + minor * t.sin()
}

/// How far an ellipse runs from start to end, 0 to 2π; equal ends make a whole ellipse.
pub fn ellipse_sweep(start: f64, end: f64) -> f64 {
    let sweep = modulo(end - start, TAU);
    if sweep <= 1e-12 { TAU } else { sweep }
}

pub fn ellipse_points(center: Point, major: Point, ratio: f64, start: f64, end: f64, tolerance: f64) -> Vec<Point> {
    let sweep = ellipse_sweep(start, end);
    let radius = major.length();
    let step = if radius > tolerance { 2.0 * (1.0 - tolerance / radius).acos() } else { TAU };
    let count = (sweep / step.max(1e-9)).ceil().clamp(4.0, MAX_ARC_SEGMENTS) as usize;
    (0..=count).map(|i| ellipse_point(center, major, ratio, start + sweep * i as f64 / count as f64)).collect()
}

/// Knots for a curve that starts and ends at its first and last control points, evenly spaced between.
pub fn clamped_knots(degree: usize, count: usize) -> Vec<f64> {
    let inner = count.saturating_sub(degree + 1);
    let mut knots = vec![0.0; degree + 1];
    knots.extend((1..=inner).map(|i| i as f64 / (inner + 1) as f64));
    knots.extend(vec![1.0; degree + 1]);
    knots
}

/// Points along a NURBS curve, by de Boor's algorithm on homogeneous points.
pub fn spline_points(degree: usize, control: &[Point], knots: &[f64], weights: &[f64]) -> Vec<Point> {
    if control.len() < 2 {
        return control.to_vec();
    }
    let degree = degree.clamp(1, control.len() - 1);
    let knots: Vec<f64> = if knots.len() == control.len() + degree + 1 { knots.to_vec() } else { clamped_knots(degree, control.len()) };
    let weight = |i: usize| weights.get(i).copied().filter(|w| *w > 0.0).unwrap_or(1.0);
    let (low, high) = (knots[degree], knots[control.len()]);
    let samples = ((control.len() - 1) * 16).clamp(16, MAX_SPLINE_SAMPLES);
    (0..=samples)
        .map(|s| {
            let u = low + (high - low) * s as f64 / samples as f64;
            let span =
                (degree..control.len()).rev().find(|&k| knots[k] <= u && (u < knots[k + 1] || k == control.len() - 1)).unwrap_or(degree);
            let mut d: Vec<(f64, f64, f64)> = (0..=degree)
                .map(|j| {
                    let i = span + j - degree;
                    let w = weight(i);
                    (control[i].x * w, control[i].y * w, w)
                })
                .collect();
            for r in 1..=degree {
                for j in (r..=degree).rev() {
                    let i = span + j - degree;
                    let denominator = knots[i + degree + 1 - r] - knots[i];
                    let alpha = if denominator.abs() < 1e-300 { 0.0 } else { (u - knots[i]) / denominator };
                    let (a, b) = (d[j - 1], d[j]);
                    d[j] = (a.0 + alpha * (b.0 - a.0), a.1 + alpha * (b.1 - a.1), a.2 + alpha * (b.2 - a.2));
                }
            }
            let (x, y, w) = d[degree];
            pt(x / w, y / w)
        })
        .collect()
}

/// The pattern lines of a hatch: each family's lines clipped to the inside of the loops (even-odd, so islands stay
/// empty) and cut into dashes. Families closer than `min_spacing` apart are left out, as too dense to see.
pub fn hatch_lines(loops: &[Vec<Point>], pattern: &[PatternLine], min_spacing: f64) -> Vec<Vec<Point>> {
    let edges: Vec<(Point, Point)> =
        loops.iter().filter(|l| l.len() >= 2).flat_map(|l| (0..l.len()).map(move |i| (l[i], l[(i + 1) % l.len()]))).collect();
    let corners: Vec<Point> = loops.iter().flatten().copied().collect();
    let mut segments = vec![];
    for line in pattern {
        let along = direction(line.angle);
        let across = pt(-along.y, along.x);
        let spacing = line.offset.dot(across);
        if spacing.abs() < min_spacing.max(1e-12) {
            continue;
        }
        let reach: Vec<f64> = corners.iter().map(|c| (*c - line.origin).dot(across) / spacing).collect();
        let (Some(first), Some(last)) =
            (reach.iter().copied().reduce(f64::min).map(f64::floor), reach.iter().copied().reduce(f64::max).map(f64::ceil))
        else {
            continue;
        };
        let cycle: f64 = line.dashes.iter().map(|d| d.abs()).sum();
        for k in (first as i64)..=(last as i64) {
            let start = line.origin + line.offset * k as f64;
            let mut hits: Vec<f64> = edges
                .iter()
                .filter_map(|(a, b)| {
                    let (da, db) = ((*a - start).dot(across), (*b - start).dot(across));
                    if (da > 0.0) == (db > 0.0) {
                        return None;
                    }
                    let crossing = *a + (*b - *a) * (da / (da - db));
                    Some((crossing - start).dot(along))
                })
                .collect();
            hits.sort_by(f64::total_cmp);
            for pair in hits.as_chunks::<2>().0 {
                let (t0, t1) = (pair[0], pair[1]);
                let at = |t: f64| start + along * t;
                if cycle <= 0.0 {
                    segments.push(vec![at(t0), at(t1)]);
                    continue;
                }
                let mut t = (t0 / cycle).floor() * cycle;
                'dashes: while t < t1 {
                    for dash in &line.dashes {
                        let next = t + dash.abs();
                        if *dash >= 0.0 && next > t0 {
                            let (a, b) = (t.max(t0), next.min(t1));
                            segments.push(if *dash == 0.0 { vec![at(a), at(a)] } else { vec![at(a), at(b)] });
                        }
                        t = next;
                        if t >= t1 || segments.len() >= MAX_HATCH_SEGMENTS {
                            break 'dashes;
                        }
                    }
                }
            }
            if segments.len() >= MAX_HATCH_SEGMENTS {
                return segments;
            }
        }
    }
    segments
}

/// The entity's parts. Curves are split into chords no further than `tolerance` drawing units from the curve.
/// A block insert gives no parts here: `flatten` expands it.
pub fn entity_parts(entity: &Entity, tolerance: f64) -> Parts {
    let path = |points: Vec<Point>| Parts { paths: vec![points], ..Parts::default() };
    match &entity.shape {
        Shape::Line { start, end } => path(vec![*start, *end]),
        Shape::Circle { center, radius } => path(arc_points(*center, *radius, 0.0, TAU, tolerance)),
        Shape::Arc { center, radius, start_angle, end_angle } => {
            let sweep = modulo(end_angle - start_angle, 360.0);
            let sweep = if sweep == 0.0 { 360.0 } else { sweep };
            path(arc_points(*center, *radius, start_angle.to_radians(), sweep.to_radians(), tolerance))
        }
        Shape::Polyline { vertices, closed } => {
            let Some(first) = vertices.first() else { return Parts::default() };
            let mut points = vec![first.point()];
            let count = if *closed { vertices.len() } else { vertices.len() - 1 };
            for index in 0..count {
                let here = vertices[index];
                let next = vertices[(index + 1) % vertices.len()];
                if here.bulge != 0.0 && here.point() != next.point() {
                    let arc = bulge_arc(here.point(), next.point(), here.bulge);
                    points.extend(arc_points(arc.center, arc.radius, arc.start_angle, arc.sweep, tolerance).into_iter().skip(1));
                } else {
                    points.push(next.point());
                }
            }
            let areas = wide_polyline_areas(vertices, *closed);
            Parts { paths: vec![points], areas, ..Parts::default() }
        }
        Shape::Text { position, text, height, rotation, font, align, line_spacing, formatted, width_factor, oblique, .. } => Parts {
            texts: vec![TextSpec {
                look: TextLook { width_factor: *width_factor, oblique: *oblique, ..TextLook::default() },
                rich: valid_rich(text, formatted, *height).map(Arc::new),
                origin: *position,
                rotation: *rotation,
                height: *height,
                text: text.clone(),
                align: *align,
                line_spacing: *line_spacing,
                font: font.clone(),
            }],
            ..Parts::default()
        },
        Shape::Table { position, columns, rows, cells, text_height, title } => {
            crate::model::table::table_parts(*position, columns, rows, cells, *text_height, *title)
        }
        // The frame; the picture itself is drawn by the canvas.
        Shape::Image { position, u, v, .. } => {
            let [a, b, c, d] = crate::model::entities::image_corners(*position, *u, *v);
            path(vec![a, b, c, d, a])
        }
        Shape::XLine { base, direction, ray } => {
            let (start, end) = crate::model::xline::far_ends(*base, *direction, *ray);
            path(vec![start, end])
        }
        Shape::Viewport { center, width, height, .. } => {
            let corners = frame_corners(*center, *width, *height);
            path(vec![corners[0], corners[1], corners[2], corners[3], corners[0]])
        }
        Shape::Annotation { kind, points, look } => {
            let parts = annotation_parts(*kind, points, look);
            Parts {
                paths: parts.paths,
                fills: parts.arrows,
                texts: parts.text.iter().chain(&parts.stack).map(dimension_text).collect(),
                ..Parts::default()
            }
        }
        Shape::Multileader { leaders, landing, dogleg, text, text_position, text_height, arrow_size, .. } => {
            let (paths, fills) = super::multileader::multileader_lines(leaders, *landing, *dogleg, *arrow_size);
            let h = if *dogleg < 0.0 { HAlign::Right } else { HAlign::Left };
            let spec = TextSpec {
                origin: *text_position,
                rotation: 0.0,
                height: *text_height,
                text: text.clone(),
                align: TextAlign { h, v: VAlign::Top },
                line_spacing: 1.0,
                font: String::new(),
                look: TextLook::default(),
                rich: None,
            };
            Parts { paths, fills, texts: if text.is_empty() { vec![] } else { vec![spec] }, ..Parts::default() }
        }
        Shape::Dimension { start, end, line_point, angle, look } => {
            let parts = dimension_parts(*start, *end, *line_point, *angle, look);
            Parts {
                paths: parts.lines().map(|(a, b)| vec![*a, *b]).collect(),
                fills: parts.arrows.clone(),
                texts: std::iter::once(TextSpec {
                    origin: parts.text_anchor,
                    rotation: parts.text_angle,
                    height: parts.text_height,
                    text: parts.text,
                    align: TextAlign { h: HAlign::Center, v: VAlign::Baseline },
                    line_spacing: 1.0,
                    font: String::new(),
                    look: TextLook::default(),
                    rich: None,
                })
                .chain(parts.stack.iter().map(dimension_text))
                .collect(),
                ..Parts::default()
            }
        }
        Shape::Ellipse { center, major, ratio, start, end } => path(ellipse_points(*center, *major, *ratio, *start, *end, tolerance)),
        Shape::Spline { degree, control, knots, weights, closed } => {
            let mut points = spline_points(*degree, control, knots, weights);
            if *closed && points.first() != points.last() {
                points.extend(points.first().copied());
            }
            path(points)
        }
        Shape::Point { position } => Parts { points: vec![*position], ..Parts::default() },
        // One area, not two triangles, so no seam shows where they would meet.
        Shape::Solid { corners } => Parts { areas: vec![vec![corners.clone()]], ..Parts::default() },
        Shape::Hatch { loops, solid, pattern, gradient, .. } => {
            let mut parts = Parts::default();
            if let Some(gradient) = gradient {
                parts.gradients.push((loops.clone(), gradient.clone()));
            } else if *solid {
                parts.areas.push(loops.clone());
            } else {
                // Pattern lines closer than about two pixels (eight chord tolerances) would only make a grey blur.
                parts.paths = hatch_lines(loops, pattern, tolerance * 8.0);
            }
            parts
        }
        Shape::Insert { .. } => Parts::default(),
    }
}

/// The filled pieces of a polyline with width: a band along each wide segment (tapering, and bent round its arc),
/// and at each corner a mitre filling the outside where two bands of the same width meet.
fn wide_polyline_areas(vertices: &[Vertex], closed: bool) -> Vec<Vec<Vec<Point>>> {
    const ARC_STEPS: f64 = 64.0;
    const MITRE_LIMIT: f64 = 4.0;
    if !vertices.iter().any(|v| v.start_width > 0.0 || v.end_width > 0.0) {
        return vec![];
    }
    let count = if closed { vertices.len() } else { vertices.len().saturating_sub(1) };
    let mut areas = vec![];
    // Each segment's direction leaving its start and arriving at its end.
    let mut tangents: Vec<(Point, Point)> = vec![];
    for index in 0..count {
        let (here, next) = (vertices[index], vertices[(index + 1) % vertices.len()]);
        let (a, b) = (here.point(), next.point());
        let (w0, w1) = (here.start_width / 2.0, here.end_width / 2.0);
        if here.bulge != 0.0 && a != b {
            let arc = bulge_arc(a, b, here.bulge);
            let steps = (arc.sweep.abs() / TAU * ARC_STEPS).ceil().max(4.0) as usize;
            let at = |i: usize| arc.start_angle + arc.sweep * i as f64 / steps as f64;
            let width = |i: usize| w0 + (w1 - w0) * i as f64 / steps as f64;
            let side = |i: usize, sign: f64| arc.center + pt(at(i).cos(), at(i).sin()) * (arc.radius + sign * width(i));
            let mut outline: Vec<Point> = (0..=steps).map(|i| side(i, 1.0)).collect();
            outline.extend((0..=steps).rev().map(|i| side(i, -1.0)));
            if w0 > 0.0 || w1 > 0.0 {
                areas.push(vec![outline]);
            }
            let turn = if arc.sweep >= 0.0 { 1.0 } else { -1.0 };
            let tangent = |angle: f64| pt(-angle.sin(), angle.cos()) * turn;
            tangents.push((tangent(at(0)), tangent(at(steps))));
        } else {
            let length = a.dist(b);
            let along = if length > 0.0 { (b - a) * (1.0 / length) } else { pt(1.0, 0.0) };
            let across = pt(-along.y, along.x);
            if w0 > 0.0 || w1 > 0.0 {
                areas.push(vec![vec![a + across * w0, b + across * w1, b - across * w1, a - across * w0]]);
            }
            tangents.push((along, along));
        }
    }
    let joints = if closed { count } else { count.saturating_sub(1) };
    for index in 0..joints {
        let following = (index + 1) % count;
        let half = vertices[index].end_width / 2.0;
        if half <= 0.0 || (vertices[following].start_width / 2.0 - half).abs() > half * 1e-6 {
            continue;
        }
        let at = vertices[following].point();
        let (t1, t2) = (tangents[index].1, tangents[following].0);
        let turn = t1.cross(t2);
        if turn.abs() < 1e-9 {
            continue;
        }
        let side = if turn > 0.0 { -1.0 } else { 1.0 };
        let (n1, n2) = (pt(-t1.y, t1.x) * side, pt(-t2.y, t2.x) * side);
        let (c1, c2) = (at + n1 * half, at + n2 * half);
        let middle = n1 + n2;
        let mut corner = vec![at, c1];
        if middle.length() > 1e-9 {
            let reach = half / (middle.length() / 2.0);
            if reach <= half * MITRE_LIMIT {
                corner.push(at + middle * (reach / middle.length()));
            }
        }
        corner.push(c2);
        areas.push(vec![corner]);
    }
    areas
}

/// A gradient as lines across its direction, each all one color, `step` apart: inside the loops by the even-odd
/// rule. CYLINDER runs to the second color at the middle and back, INVCYLINDER the other way round.
pub fn gradient_strips(loops: &[Vec<Point>], gradient: &Gradient, step: f64) -> Vec<(Point, Point, (u8, u8, u8))> {
    const MAX_STRIPS: f64 = 4000.0;
    let (along, across) = (direction(gradient.angle), direction(gradient.angle + 90.0));
    let (mut low, mut high) = (f64::INFINITY, f64::NEG_INFINITY);
    for p in loops.iter().flatten() {
        let t = p.dot(along);
        (low, high) = (low.min(t), high.max(t));
    }
    if high <= low || !high.is_finite() || step <= 0.0 {
        return vec![];
    }
    let step = step.max((high - low) / MAX_STRIPS);
    let mix = |t: f64| {
        let t = match gradient.name.to_uppercase().as_str() {
            "CYLINDER" => 1.0 - (2.0 * t - 1.0).abs(),
            "INVCYLINDER" => (2.0 * t - 1.0).abs(),
            _ => t,
        };
        let channel = |a: u8, b: u8| (a as f64 + (b as f64 - a as f64) * t).round() as u8;
        (
            channel(gradient.first.0, gradient.second.0),
            channel(gradient.first.1, gradient.second.1),
            channel(gradient.first.2, gradient.second.2),
        )
    };
    let mut strips = vec![];
    let mut t = low + step / 2.0;
    while t < high {
        let mut crossings: Vec<f64> = vec![];
        for points in loops {
            for i in 0..points.len() {
                let (a, b) = (points[i], points[(i + 1) % points.len()]);
                let (ta, tb) = (a.dot(along), b.dot(along));
                if (ta <= t) != (tb <= t) {
                    crossings.push(a.dot(across) + (b.dot(across) - a.dot(across)) * (t - ta) / (tb - ta));
                }
            }
        }
        crossings.sort_by(f64::total_cmp);
        let color = mix((t - low) / (high - low));
        for pair in crossings.as_chunks::<2>().0 {
            strips.push((along * t + across * pair[0], along * t + across * pair[1], color));
        }
        t += step;
    }
    strips
}

/// A piece of a dimension's text, centered on the middle of its baseline.
pub fn dimension_text(text: &super::annotation::AnnotationText) -> TextSpec {
    TextSpec {
        origin: text.anchor,
        rotation: text.angle,
        height: text.height,
        text: text.text.clone(),
        align: TextAlign { h: HAlign::Center, v: VAlign::Baseline },
        line_spacing: 1.0,
        font: String::new(),
        look: TextLook::default(),
        rich: None,
    }
}

/// The layer, color and linetype a part shows in, after what it takes from the block insert it is in.
#[derive(Clone, Debug, PartialEq)]
pub struct Styled {
    pub layer: String,
    pub color: i32,
    pub linetype: String,
    pub lineweight: i16,
}

impl Styled {
    pub fn of(entity: &Entity) -> Styled {
        Styled { layer: entity.layer.clone(), color: entity.color, linetype: entity.linetype.clone(), lineweight: entity.lineweight }
    }

    /// An entity inside a block: ByBlock color, linetype and lineweight come from the insert, and layer 0 takes the
    /// insert's layer, as in AutoCAD.
    pub fn inside(entity: &Entity, insert: &Styled) -> Styled {
        Styled {
            layer: if entity.layer == "0" { insert.layer.clone() } else { entity.layer.clone() },
            color: if entity.color == BYBLOCK_COLOR { insert.color } else { entity.color },
            linetype: if entity.linetype.eq_ignore_ascii_case(BYBLOCK_LINETYPE) {
                insert.linetype.clone()
            } else {
                entity.linetype.clone()
            },
            lineweight: if entity.lineweight == super::lineweight::BYBLOCK { insert.lineweight } else { entity.lineweight },
        }
    }
}

/// Every part an entity draws, with the style each shows in. A block insert gives its block's entities, placed
/// and styled as the insert says, and its visible attributes.
pub fn flatten(entity: &Entity, tolerance: f64) -> Vec<(Styled, Parts)> {
    let mut out = vec![];
    flatten_into(entity, &Styled::of(entity), &IDENTITY, tolerance, 0, &mut out);
    out
}

fn flatten_into(entity: &Entity, style: &Styled, transform: &Similarity, tolerance: f64, depth: usize, out: &mut Vec<(Styled, Parts)>) {
    let Shape::Insert { block, position, scale_x, scale_y, rotation, attributes } = &entity.shape else {
        // The chords must stay close to the curve after the transform has scaled it.
        let local = tolerance / transform.determinant().abs().sqrt().max(1e-300);
        out.push((style.clone(), entity_parts(entity, local).map(transform)));
        return;
    };
    if depth >= MAX_BLOCK_DEPTH {
        return;
    }
    let placed = transform.after(&insert_transform(block.0.base, *position, *scale_x, *scale_y, *rotation));
    for child in &block.0.entities {
        flatten_into(child, &Styled::inside(child, style), &placed, tolerance, depth + 1, out);
    }
    for attribute in attributes.iter().filter(|a| !a.invisible) {
        let shown = Entity { layer: attribute.layer.clone(), color: attribute.color, shape: attribute.text.clone(), ..entity.clone() };
        flatten_into(&shown, &Styled::inside(&shown, style), transform, tolerance, depth + 1, out);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::model::entities::{BlockDef, BlockRef, arc, circle, line};

    #[test]
    fn arcs_get_more_chords_when_closer_is_needed() {
        let coarse = arc_points(pt(0.0, 0.0), 10.0, 0.0, TAU, 1.0).len();
        let fine = arc_points(pt(0.0, 0.0), 10.0, 0.0, TAU, 0.01).len();
        assert!(fine > coarse * 5);
        assert!(arc_points(pt(0.0, 0.0), 10.0, 0.0, TAU, 0.01).iter().all(|p| (p.dist(pt(0.0, 0.0)) - 10.0).abs() < 1e-9));
    }

    #[test]
    fn an_arc_runs_counter_clockwise_across_zero() {
        let parts = entity_parts(&arc(pt(0.0, 0.0), 1.0, 270.0, 90.0), 0.001);
        let path = &parts.paths[0];
        assert!(path[0].dist(pt(0.0, -1.0)) < 1e-9);
        assert!(path[path.len() / 2].dist(pt(1.0, 0.0)) < 1e-3);
        assert!(path[path.len() - 1].dist(pt(0.0, 1.0)) < 1e-9);
    }

    #[test]
    fn lines_of_text_line_up_as_aligned() {
        let spec = TextSpec {
            origin: pt(0.0, 0.0),
            rotation: 0.0,
            height: 3.0,
            text: "ab\ncd".into(),
            align: TextAlign { h: HAlign::Right, v: VAlign::Top },
            line_spacing: 1.0,
            font: String::new(),
            look: TextLook::default(),
            rich: None,
        };
        let origins = spec.line_origins(&[10.0, 4.0]);
        assert_eq!(origins[0], pt(-10.0, -3.0));
        assert!(origins[1].dist(pt(-4.0, -8.0)) < 1e-12);
        let bottom = TextSpec { align: TextAlign { h: HAlign::Left, v: VAlign::Bottom }, ..spec };
        assert!(bottom.line_origins(&[1.0, 1.0])[1].dist(pt(0.0, 0.0)) < 1e-12, "the last line sits on a bottom-aligned point");
    }

    #[test]
    fn a_clamped_spline_starts_and_ends_on_its_control_points() {
        let control = [pt(0.0, 0.0), pt(1.0, 2.0), pt(3.0, 2.0), pt(4.0, 0.0)];
        let points = spline_points(3, &control, &[], &[]);
        assert!(points[0].dist(control[0]) < 1e-12 && points[points.len() - 1].dist(control[3]) < 1e-12);
        let middle = points[points.len() / 2];
        assert!((middle.x - 2.0).abs() < 1e-9 && (middle.y - 1.5).abs() < 1e-9, "{middle:?}");
    }

    #[test]
    fn hatch_lines_stay_inside_and_skip_islands() {
        let outer = vec![pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 10.0), pt(0.0, 10.0)];
        let island = vec![pt(4.0, 4.0), pt(6.0, 4.0), pt(6.0, 6.0), pt(4.0, 6.0)];
        let horizontal = PatternLine { angle: 0.0, origin: pt(0.0, 0.5), offset: pt(0.0, 1.0), dashes: vec![] };
        let lines = hatch_lines(&[outer, island], &[horizontal], 0.0);
        assert_eq!(lines.len(), 10 + 2, "two of the ten rows cross the island and split in two");
        assert!(lines.iter().flatten().all(|p| p.x >= -1e-9 && p.x <= 10.0 + 1e-9));
        assert!(lines.iter().all(|l| !(l[0].x < 5.0 && l[1].x > 5.0 && (4.0..6.0).contains(&l[0].y))));
    }

    #[test]
    fn a_block_is_drawn_where_its_insert_puts_it_in_the_insert_s_colors() {
        let mut by_block = line(pt(0.0, 0.0), pt(1.0, 0.0));
        by_block.color = BYBLOCK_COLOR;
        let mut own = circle(pt(0.0, 0.0), 1.0);
        own.layer = "Walls".into();
        own.color = 3;
        let block = BlockRef(Arc::new(BlockDef {
            name: "B".into(),
            base: pt(0.0, 0.0),
            entities: vec![by_block, own],
            attribute_defs: vec![],
            xref: None,
        }));
        let mut insert =
            Entity::new(Shape::Insert { block, position: pt(10.0, 0.0), scale_x: 2.0, scale_y: 2.0, rotation: 90.0, attributes: vec![] });
        insert.layer = "Doors".into();
        insert.color = 5;
        let pieces = flatten(&insert, 0.01);
        assert_eq!(pieces.len(), 2);
        assert_eq!((pieces[0].0.layer.as_str(), pieces[0].0.color), ("Doors", 5));
        assert_eq!((pieces[1].0.layer.as_str(), pieces[1].0.color), ("Walls", 3));
        let end = pieces[0].1.paths[0][1];
        assert!(end.dist(pt(10.0, 2.0)) < 1e-12, "{end:?}");
    }
}
