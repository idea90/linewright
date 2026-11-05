//! Radius, diameter, angle, ordinate, arc length and jogged radius dimensions, and leaders: what each draws from
//! its defining points.

use std::f64::consts::{PI, TAU};

use super::dimension::{Arrowhead, DimensionLook, HALF_CHARACTER_WIDTH, Segment, end_mark, normal, readable};
use super::geometry::{Point, direction, modulo, pt};
use super::parts::arc_points;

/// What an annotation is, and what its points mean.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnnotationKind {
    /// Points: the center, then the point on the curve the dimension points at.
    Radius,
    /// Points: the center, then the point on the curve the dimension points at; it spans to the opposite point.
    Diameter,
    /// Points: the vertex, a point on each of the two sides, then a point the dimension arc runs through. The
    /// angle measured is the one between the sides that holds that point.
    Angle,
    /// Points: from the arrow's tip along the leader line. Text that goes with it is a text object of its own.
    Leader { arrow: bool },
    /// Points: the datum (where 0 is), the feature measured, and the end of the leader where the text sits. An X
    /// datum measures across, the leader running up or down; a Y datum measures up, the leader running across.
    Ordinate { x_datum: bool },
    /// Points: the arc's center, its start and end (counter-clockwise), and a point the dimension arc runs through.
    ArcLength,
    /// Points: the center, the point on the curve pointed at, the center shown instead (for a center far off the
    /// sheet), and where the line jogs.
    JoggedRadius,
}

impl AnnotationKind {
    pub fn name(self) -> &'static str {
        match self {
            AnnotationKind::Radius => "Radial dimension",
            AnnotationKind::Diameter => "Diameter dimension",
            AnnotationKind::Angle => "Angular dimension",
            AnnotationKind::Leader { .. } => "Leader",
            AnnotationKind::Ordinate { .. } => "Ordinate dimension",
            AnnotationKind::ArcLength => "Arc length dimension",
            AnnotationKind::JoggedRadius => "Jogged radius dimension",
        }
    }
}

/// Text a dimension shows.
#[derive(Clone, Debug, PartialEq)]
pub struct AnnotationText {
    pub text: String,
    pub height: f64,
    pub anchor: Point, // middle of the baseline
    pub middle: Point,
    pub angle: f64, // degrees, readable
}

/// Everything an annotation draws, in drawing units.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AnnotationParts {
    pub paths: Vec<Vec<Point>>,
    pub arrows: Vec<[Point; 3]>,
    pub text: Option<AnnotationText>,
    /// A tolerance's two stacked lines beside the text.
    pub stack: Vec<AnnotationText>,
}

impl AnnotationParts {
    fn mark(&mut self, tip: Point, inward: Point, look: &DimensionLook) {
        let (arrows, ticks) = end_mark(tip, inward, look);
        self.arrows.extend(arrows);
        self.paths.extend(ticks.into_iter().map(|(a, b)| vec![a, b]));
    }

    fn line(&mut self, (a, b): Segment) {
        if a != b {
            self.paths.push(vec![a, b]);
        }
    }
}

/// What the dimension measures: a length, an angle in degrees, or nothing for a leader.
pub fn annotation_measurement(kind: AnnotationKind, points: &[Point]) -> Option<f64> {
    match (kind, points) {
        (AnnotationKind::Radius, [center, chord, ..]) => Some(center.dist(*chord)),
        (AnnotationKind::Diameter, [center, chord, ..]) => Some(2.0 * center.dist(*chord)),
        (AnnotationKind::Angle, [vertex, first, second, arc_point, ..]) => {
            Some(angle_sweep(*vertex, *first, *second, *arc_point).1.to_degrees())
        }
        (AnnotationKind::Ordinate { x_datum }, [origin, feature, ..]) => {
            Some(if x_datum { (feature.x - origin.x).abs() } else { (feature.y - origin.y).abs() })
        }
        (AnnotationKind::ArcLength, [center, start, end, ..]) => Some(center.dist(*start) * arc_sweep(*center, *start, *end)),
        (AnnotationKind::JoggedRadius, [center, chord, ..]) => Some(center.dist(*chord)),
        _ => None,
    }
}

/// Where the arc of an angle dimension starts and how far it sweeps counter-clockwise, in radians: between the two
/// sides, on the side that holds `arc_point`. True when it starts from the second side.
fn angle_sweep(vertex: Point, first: Point, second: Point, arc_point: Point) -> (f64, f64, bool) {
    let angle = |p: Point| (p.y - vertex.y).atan2(p.x - vertex.x);
    let (a1, a2, aq) = (angle(first), angle(second), angle(arc_point));
    let sweep = modulo(a2 - a1, TAU);
    if modulo(aq - a1, TAU) <= sweep { (a1, sweep, false) } else { (a2, TAU - sweep, true) }
}

/// How far an arc sweeps counter-clockwise from start to end, in radians.
fn arc_sweep(center: Point, start: Point, end: Point) -> f64 {
    let angle = |p: Point| (p.y - center.y).atan2(p.x - center.x);
    let sweep = modulo(angle(end) - angle(start), TAU);
    if sweep == 0.0 { TAU } else { sweep }
}

fn unit(v: Point) -> Point {
    let length = v.length();
    if length > 0.0 { v * (1.0 / length) } else { pt(1.0, 0.0) }
}

/// The text, its height and how far it runs each side of its middle.
fn text_size(text: &str, look: &DimensionLook) -> (f64, f64) {
    let height = look.text_height * look.scale;
    (height, text.chars().count() as f64 * height * HALF_CHARACTER_WIDTH)
}

/// The text and its tolerance stack, placed around a middle, reading along `angle` unless DIMTEDIT turned it.
fn place_text(parts: &mut AnnotationParts, main: String, stack: Option<(String, String)>, middle: Point, angle: f64, look: &DimensionLook) {
    let angle = look.text_rotation.unwrap_or_else(|| readable(angle));
    let (text, stacked) = look.layout_text(main, stack, middle, angle);
    parts.text = Some(text);
    parts.stack = stacked;
}

pub fn annotation_parts(kind: AnnotationKind, points: &[Point], look: &DimensionLook) -> AnnotationParts {
    match (kind, points) {
        (AnnotationKind::Radius | AnnotationKind::Diameter, [center, chord, ..]) => {
            radial_parts(*center, *chord, kind == AnnotationKind::Diameter, look)
        }
        (AnnotationKind::Angle, [vertex, first, second, arc_point, ..]) => angle_parts(*vertex, *first, *second, *arc_point, look),
        (AnnotationKind::Ordinate { x_datum }, [origin, feature, leader_end, ..]) => {
            ordinate_parts(*origin, *feature, *leader_end, x_datum, look)
        }
        (AnnotationKind::ArcLength, [center, start, end, arc_point, ..]) => arc_length_parts(*center, *start, *end, *arc_point, look),
        (AnnotationKind::JoggedRadius, [center, chord, shown_center, jog, ..]) => jogged_parts(*center, *chord, *shown_center, *jog, look),
        (AnnotationKind::Leader { arrow }, [tip, rest @ ..]) if !rest.is_empty() => {
            let mut parts = AnnotationParts { paths: vec![points.to_vec()], ..AnnotationParts::default() };
            if arrow {
                // Leaders keep their own arrowhead, a filled one unless the dimensions use dots.
                let arrowhead = if look.arrowhead == Arrowhead::Dot { Arrowhead::Dot } else { Arrowhead::Filled };
                parts.mark(*tip, unit(rest[0] - *tip), &DimensionLook { arrowhead, ..look.clone() });
            }
            parts
        }
        _ => AnnotationParts::default(),
    }
}

/// A radius or diameter dimension. Text outside the curve takes the dimension line out to it, with the arrow
/// pointing in at the curve; text inside leaves the line across the circle, arrows pointing out.
fn radial_parts(center: Point, chord: Point, diameter: bool, look: &DimensionLook) -> AnnotationParts {
    let radius = center.dist(chord);
    let along = unit(chord - center);
    let (text, stack) = if diameter { look.measured(2.0 * radius, "Ø") } else { look.measured(radius, "R") };
    let (height, _) = text_size(&text, look);
    let half_width = look.text_half_width(&text, &stack);
    let text_angle = readable(along.y.atan2(along.x).to_degrees());
    let above = normal(direction(text_angle));
    let size = look.arrow_size * look.scale;
    let middle = look
        .text_position
        .unwrap_or_else(|| center + along * (radius + 2.0 * size + half_width) + above * (look.text_gap * look.scale + height / 2.0));
    let reach = (middle - center).dot(along);
    let far = center - along * radius;
    let back = pt(0.0, 0.0) - along;
    let mut parts = AnnotationParts::default();
    if diameter {
        parts.line((far, chord));
    }
    if reach > radius {
        parts.line((chord, center + along * (reach + half_width)));
        parts.mark(chord, along, look);
        if diameter {
            parts.mark(far, along, look);
        }
    } else if diameter && reach < -radius {
        // Text beyond the far side: the line runs out to it there.
        parts.line((far, center + along * (reach - half_width)));
        parts.mark(far, back, look);
        parts.mark(chord, back, look);
    } else {
        if diameter {
            parts.mark(far, along, look);
        } else {
            parts.line((center, chord));
        }
        parts.mark(chord, back, look);
    }
    place_text(&mut parts, text, stack, middle, text_angle, look);
    parts
}

/// An angle dimension: an arc between the two sides, extension lines out to it where the sides stop short.
fn angle_parts(vertex: Point, first: Point, second: Point, arc_point: Point, look: &DimensionLook) -> AnnotationParts {
    let radius = vertex.dist(arc_point);
    let (start, sweep, from_second) = angle_sweep(vertex, first, second, arc_point);
    let text = look.with_override(look.angle(sweep.to_degrees()));
    let (height, _) = text_size(&text, look);
    let mut parts = AnnotationParts::default();
    parts.paths.push(arc_points(vertex, radius, start, sweep, (radius * 1e-3).max(f64::MIN_POSITIVE)));
    let end = start + sweep;
    let at = |a: f64| vertex + pt(a.cos(), a.sin()) * radius;
    parts.mark(at(start), pt(-start.sin(), start.cos()), look);
    parts.mark(at(end), pt(end.sin(), -end.cos()), look);
    let (at_start, at_end) = if from_second { (second, first) } else { (first, second) };
    for (side, a) in [(at_start, start), (at_end, end)] {
        let reach = side.dist(vertex);
        let out = pt(a.cos(), a.sin());
        if radius > reach + look.extension_gap * look.scale {
            parts.line((
                vertex + out * (reach + look.extension_gap * look.scale),
                vertex + out * (radius + look.extension_overshoot * look.scale),
            ));
        }
    }
    let middle_angle = start + sweep / 2.0;
    let middle = look
        .text_position
        .unwrap_or_else(|| vertex + pt(middle_angle.cos(), middle_angle.sin()) * (radius + look.text_gap * look.scale + height / 2.0));
    place_text(&mut parts, text, None, middle, (middle_angle - PI / 2.0).to_degrees(), look);
    parts
}

/// An ordinate dimension: a leader from the feature out to where its text sits, square to the datum it measures
/// from, jogging over near its end when the end is not straight out from the feature.
fn ordinate_parts(origin: Point, feature: Point, leader_end: Point, x_datum: bool, look: &DimensionLook) -> AnnotationParts {
    let value = annotation_measurement(AnnotationKind::Ordinate { x_datum }, &[origin, feature]).unwrap_or(0.0);
    let (text, stack) = look.measured(value, "");
    let half_width = look.text_half_width(&text, &stack);
    // The leader runs along y for an X datum, along x for a Y datum, toward its end.
    let (reach, sideways) =
        if x_datum { (leader_end.y - feature.y, leader_end.x - feature.x) } else { (leader_end.x - feature.x, leader_end.y - feature.y) };
    let out = |d: f64| if x_datum { pt(0.0, d) } else { pt(d, 0.0) };
    let across = |d: f64| if x_datum { pt(d, 0.0) } else { pt(0.0, d) };
    let sign = if reach >= 0.0 { 1.0 } else { -1.0 };
    let gap = look.extension_gap * look.scale;
    let start = feature + out(sign * gap);
    let mut leader = vec![start];
    if sideways.abs() > 1e-9 * reach.abs().max(1.0) {
        // Straight out, a jog across, then straight out again to the end.
        let jog = (look.arrow_size * look.scale * 2.0).min(reach.abs() / 3.0);
        leader.push(feature + out(reach - sign * 2.0 * jog));
        leader.push(feature + out(reach - sign * jog) + across(sideways));
    }
    leader.push(leader_end);
    let mut parts = AnnotationParts { paths: vec![leader], ..AnnotationParts::default() };
    let direction_out = unit(out(sign));
    let angle = direction_out.y.atan2(direction_out.x).to_degrees();
    let middle = look.text_position.unwrap_or_else(|| leader_end + direction_out * (look.text_gap * look.scale + half_width));
    place_text(&mut parts, text, stack, middle, angle, look);
    parts
}

/// An arc length dimension: an arc beside the arc, extension lines out to it (square to the chord for arcs up to a
/// right angle, along the radius beyond that, as AutoCAD draws them) and an arc mark before the length.
fn arc_length_parts(center: Point, start: Point, end: Point, arc_point: Point, look: &DimensionLook) -> AnnotationParts {
    let radius = center.dist(start);
    let sweep = arc_sweep(center, start, end);
    let a0 = (start.y - center.y).atan2(start.x - center.x);
    let reach = center.dist(arc_point).max(f64::MIN_POSITIVE);
    let outward = if reach >= radius { 1.0 } else { -1.0 };
    let gap = look.extension_gap * look.scale;
    let overshoot = look.extension_overshoot * look.scale;
    let bisector = pt((a0 + sweep / 2.0).cos(), (a0 + sweep / 2.0).sin());
    // Where each extension line meets the dimension arc.
    let meet = |p: Point| -> Point {
        if sweep <= PI / 2.0 {
            // Along the bisector from p to the circle of the dimension arc.
            let d = bisector * outward;
            let f = p - center;
            let b = f.dot(d);
            let c = f.dot(f) - reach * reach;
            let t = -b + (b * b - c).max(0.0).sqrt();
            p + d * t
        } else {
            center + unit(p - center) * reach
        }
    };
    let (m0, m1) = (meet(start), meet(end));
    let b0 = (m0.y - center.y).atan2(m0.x - center.x);
    let b1 = (m1.y - center.y).atan2(m1.x - center.x);
    let dim_sweep = modulo(b1 - b0, TAU);
    let mut parts = AnnotationParts::default();
    parts.paths.push(arc_points(center, reach, b0, dim_sweep, (reach * 1e-3).max(f64::MIN_POSITIVE)));
    for (p, m) in [(start, m0), (end, m1)] {
        let along = unit(m - p);
        if p.dist(m) > gap {
            parts.line((p + along * gap, m + along * overshoot));
        }
    }
    let b_end = b0 + dim_sweep;
    parts.mark(m0, pt(-b0.sin(), b0.cos()), look);
    parts.mark(m1, pt(b_end.sin(), -b_end.cos()), look);
    let (text, stack) = look.measured(radius * sweep, "");
    let (height, _) = text_size(&text, look);
    let middle_angle = b0 + dim_sweep / 2.0;
    let middle = look.text_position.unwrap_or_else(|| {
        center + pt(middle_angle.cos(), middle_angle.sin()) * (reach + outward * (look.text_gap * look.scale + height / 2.0))
    });
    place_text(&mut parts, text, stack, middle, (middle_angle - PI / 2.0).to_degrees(), look);
    // The arc mark before the length, as DIMARCSYM's default puts it.
    if let Some(text) = &parts.text {
        let along = direction(text.angle);
        let up = normal(along);
        let half = text.text.chars().count() as f64 * height * HALF_CHARACTER_WIDTH;
        let mark_middle = text.anchor - along * (half + height * 0.55) + up * (height * 0.35);
        let mark_radius = height * 0.4;
        let mark: Vec<Point> = (0..=12)
            .map(|i| {
                let a = (20.0 + 140.0 * i as f64 / 12.0).to_radians();
                mark_middle + along * (a.cos() * mark_radius) + up * (a.sin() * mark_radius - mark_radius * 0.5)
            })
            .collect();
        parts.paths.push(mark);
    }
    parts
}

/// A jogged radius: for a center too far off to show, the line starts at a center shown nearer instead, runs out
/// parallel to the true radius, jogs over onto it and goes on to the curve, the arrow pointing in at it.
fn jogged_parts(center: Point, chord: Point, shown_center: Point, jog: Point, look: &DimensionLook) -> AnnotationParts {
    let radius = center.dist(chord);
    let along = unit(chord - center);
    // The jog crosses from the shown center's line to the true radius' line, at 45 degrees.
    let offset = (shown_center - center).cross(along);
    let on_radius = center + along * (jog - center).dot(along);
    let back = on_radius - along * offset.abs();
    let from = shown_center + along * ((back - shown_center).dot(along));
    let mut parts = AnnotationParts::default();
    parts.paths.push(vec![shown_center, from, on_radius, chord]);
    parts.mark(chord, pt(0.0, 0.0) - along, look);
    let (text, stack) = look.measured(radius, "R");
    let (height, _) = text_size(&text, look);
    let text_angle = readable(along.y.atan2(along.x).to_degrees());
    let above = normal(direction(text_angle));
    let middle = look.text_position.unwrap_or_else(|| on_radius.midpoint(chord) + above * (look.text_gap * look.scale + height / 2.0));
    place_text(&mut parts, text, stack, middle, text_angle, look);
    parts
}

/// An angle dimension between two lines, as vertex, a point on each side and the arc point: the sides are the
/// halves of the lines, out from where they meet, between which the arc point lies.
pub fn angle_between_lines(first: (Point, Point), second: (Point, Point), arc_point: Point) -> Option<Vec<Point>> {
    let (u, v) = (first.1 - first.0, second.1 - second.0);
    let denominator = u.cross(v);
    if denominator.abs() < f64::EPSILON * u.length() * v.length() {
        return None;
    }
    let vertex = first.0 + u * ((second.0 - first.0).cross(v) / denominator);
    let toward = arc_point - vertex;
    // Each line runs both ways from the vertex; the sides are the two halves whose angle holds the arc point.
    let within = |r1: Point, r2: Point| {
        let turn = r1.cross(r2);
        r1.cross(toward) * turn >= 0.0 && toward.cross(r2) * turn >= 0.0
    };
    let back = |d: Point| pt(0.0, 0.0) - d;
    let (r1, r2) = [(u, v), (back(u), v), (u, back(v)), (back(u), back(v))].into_iter().find(|&(r1, r2)| within(r1, r2))?;
    // Each side point as far out along its half as the line reaches, or near the vertex when it lies behind.
    let side = |(a, b): (Point, Point), r: Point| {
        let reach = [a, b].iter().map(|p| (*p - vertex).dot(r) / r.length()).fold(0.0_f64, f64::max);
        vertex + r * (reach.max(toward.length() * 1e-3) / r.length())
    };
    Some(vec![vertex, side(first, r1), side(second, r2), arc_point])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_radius_outside_the_circle_points_in_at_it() {
        let look = DimensionLook { text_position: Some(pt(20.0, 0.5)), ..DimensionLook::default() };
        let parts = annotation_parts(AnnotationKind::Radius, &[pt(0.0, 0.0), pt(10.0, 0.0)], &look);
        let text = parts.text.unwrap();
        assert_eq!(text.text, "R10.00");
        assert_eq!(parts.paths[0][0], pt(10.0, 0.0));
        assert!(parts.paths[0][1].x > 20.0, "the line runs on under the text");
        let [tip, back, _] = parts.arrows[0];
        assert_eq!(tip, pt(10.0, 0.0));
        assert!(back.x > 10.0, "the arrow sits outside, pointing in");
    }

    #[test]
    fn a_diameter_inside_spans_the_circle() {
        let look = DimensionLook { text_position: Some(pt(0.0, 1.0)), ..DimensionLook::default() };
        let parts = annotation_parts(AnnotationKind::Diameter, &[pt(0.0, 0.0), pt(5.0, 0.0)], &look);
        assert_eq!(parts.text.unwrap().text, "Ø10.00");
        assert_eq!(parts.paths[0], vec![pt(-5.0, 0.0), pt(5.0, 0.0)]);
        assert_eq!(parts.arrows.len(), 2);
    }

    #[test]
    fn an_angle_takes_the_side_its_arc_point_is_on() {
        let points = [pt(0.0, 0.0), pt(10.0, 0.0), pt(0.0, 10.0), pt(5.0, 5.0)];
        assert!((annotation_measurement(AnnotationKind::Angle, &points).unwrap() - 90.0).abs() < 1e-9);
        let reflex = [pt(0.0, 0.0), pt(10.0, 0.0), pt(0.0, 10.0), pt(-5.0, -5.0)];
        assert!((annotation_measurement(AnnotationKind::Angle, &reflex).unwrap() - 270.0).abs() < 1e-9);
        let look = DimensionLook::default();
        let parts = annotation_parts(AnnotationKind::Angle, &points, &look);
        assert_eq!(parts.text.unwrap().text, "90°");
        // Sides 10 long, arc at about 7: no extension lines, just the arc.
        assert_eq!(parts.paths.len(), 1);
        let far = annotation_parts(AnnotationKind::Angle, &[pt(0.0, 0.0), pt(2.0, 0.0), pt(0.0, 2.0), pt(10.0, 10.0)], &look);
        assert_eq!(far.paths.len(), 3, "the arc and an extension line out to it from each side");
    }

    #[test]
    fn an_ordinate_measures_from_the_datum_along_its_axis() {
        let points = [pt(10.0, 20.0), pt(35.0, 50.0), pt(35.0, 70.0)];
        let kind = AnnotationKind::Ordinate { x_datum: true };
        assert_eq!(annotation_measurement(kind, &points), Some(25.0));
        assert_eq!(annotation_measurement(AnnotationKind::Ordinate { x_datum: false }, &points), Some(30.0));
        let parts = annotation_parts(kind, &points, &DimensionLook::default());
        assert_eq!(parts.text.unwrap().text, "25.00");
        assert_eq!(parts.paths[0].len(), 2, "straight out when the end is above the feature");
        let jogged = annotation_parts(kind, &[pt(0.0, 0.0), pt(5.0, 0.0), pt(9.0, 30.0)], &DimensionLook::default());
        assert_eq!(jogged.paths[0].len(), 4);
        assert_eq!(*jogged.paths[0].last().unwrap(), pt(9.0, 30.0));
    }

    #[test]
    fn an_arc_length_measures_along_the_arc() {
        let points = [pt(0.0, 0.0), pt(10.0, 0.0), pt(0.0, 10.0), pt(14.0, 14.0)];
        let length = annotation_measurement(AnnotationKind::ArcLength, &points).unwrap();
        assert!((length - 5.0 * PI).abs() < 1e-9);
        let parts = annotation_parts(AnnotationKind::ArcLength, &points, &DimensionLook::default());
        assert_eq!(parts.text.unwrap().text, "15.71");
        assert_eq!(parts.arrows.len(), 2);
    }

    #[test]
    fn a_jogged_radius_runs_from_the_shown_center_to_the_curve() {
        let points = [pt(0.0, 0.0), pt(100.0, 0.0), pt(70.0, 10.0), pt(85.0, 0.0)];
        let parts = annotation_parts(AnnotationKind::JoggedRadius, &points, &DimensionLook::default());
        assert_eq!(parts.text.unwrap().text, "R100.00");
        let path = &parts.paths[0];
        assert_eq!((path[0], *path.last().unwrap()), (pt(70.0, 10.0), pt(100.0, 0.0)));
        assert_eq!(parts.arrows[0][0], pt(100.0, 0.0));
    }

    #[test]
    fn tolerances_stack_beside_the_size_or_replace_it() {
        use crate::model::dimension::Tolerance;
        let look = DimensionLook { tolerance_plus: 0.1, tolerance_minus: 0.05, ..DimensionLook::default() };
        assert_eq!(DimensionLook { tolerance: Tolerance::Symmetric, ..look.clone() }.measured(25.0, "").0, "25.00±0.10");
        let (main, stack) = DimensionLook { tolerance: Tolerance::Deviation, ..look.clone() }.measured(25.0, "");
        assert_eq!((main.as_str(), stack), ("25.00", Some(("+0.10".to_string(), "-0.05".to_string()))));
        let (main, stack) = DimensionLook { tolerance: Tolerance::Limits, ..look }.measured(25.0, "");
        assert_eq!((main.as_str(), stack), ("", Some(("25.10".to_string(), "24.95".to_string()))));
    }

    #[test]
    fn a_leader_has_its_arrow_at_the_first_point() {
        let parts = annotation_parts(
            AnnotationKind::Leader { arrow: true },
            &[pt(0.0, 0.0), pt(5.0, 5.0), pt(10.0, 5.0)],
            &DimensionLook::default(),
        );
        assert_eq!(parts.paths[0].len(), 3);
        assert_eq!(parts.arrows[0][0], pt(0.0, 0.0));
        assert!(parts.text.is_none());
        let bare = annotation_parts(AnnotationKind::Leader { arrow: false }, &[pt(0.0, 0.0), pt(5.0, 5.0)], &DimensionLook::default());
        assert!(bare.arrows.is_empty());
    }
}
