use std::f64::consts::PI;
use std::ops::{Add, Mul, Sub};

/// A point or a vector in drawing units.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

pub const fn pt(x: f64, y: f64) -> Point {
    Point { x, y }
}

impl Point {
    pub fn dist(self, other: Point) -> f64 {
        (self.x - other.x).hypot(self.y - other.y)
    }

    pub fn dot(self, other: Point) -> f64 {
        self.x * other.x + self.y * other.y
    }

    /// The z of the cross product: positive when `other` turns counter-clockwise from `self`.
    pub fn cross(self, other: Point) -> f64 {
        self.x * other.y - self.y * other.x
    }

    pub fn length(self) -> f64 {
        self.x.hypot(self.y)
    }

    pub fn midpoint(self, other: Point) -> Point {
        pt((self.x + other.x) / 2.0, (self.y + other.y) / 2.0)
    }

    /// The direction from this point to the other, in degrees counter-clockwise from +X (-180 to 180).
    pub fn angle_to(self, other: Point) -> f64 {
        (other.y - self.y).atan2(other.x - self.x).to_degrees()
    }

    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }
}

impl Add for Point {
    type Output = Point;
    fn add(self, other: Point) -> Point {
        pt(self.x + other.x, self.y + other.y)
    }
}

impl Sub for Point {
    type Output = Point;
    fn sub(self, other: Point) -> Point {
        pt(self.x - other.x, self.y - other.y)
    }
}

impl Mul<f64> for Point {
    type Output = Point;
    fn mul(self, factor: f64) -> Point {
        pt(self.x * factor, self.y * factor)
    }
}

/// Python's `%` for floats: the result takes the sign of the divisor, so angles land in 0..360.
pub fn modulo(value: f64, divisor: f64) -> f64 {
    let result = value % divisor;
    if result != 0.0 && (result < 0.0) != (divisor < 0.0) { result + divisor } else { result }
}

/// The unit vector pointing at an angle given in degrees.
pub fn direction(degrees: f64) -> Point {
    let radians = degrees.to_radians();
    pt(radians.cos(), radians.sin())
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BulgeArc {
    pub center: Point,
    pub radius: f64,
    pub start_angle: f64, // radians, counter-clockwise from +X
    pub sweep: f64,       // radians, positive counter-clockwise
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ThreePointArc {
    pub center: Point,
    pub radius: f64,
    pub start_angle: f64, // degrees, counter-clockwise from +X
    pub end_angle: f64,
}

/// Counter-clockwise arc that runs through the three points in order, or None if they are in a line.
pub fn arc_through_points(first: Point, second: Point, third: Point) -> Option<ThreePointArc> {
    let (bx, by) = (second.x - first.x, second.y - first.y);
    let (cx, cy) = (third.x - first.x, third.y - first.y);
    let turn = bx * (cy - by) - by * (cx - bx);
    if turn.abs() <= 1e-9 * bx.hypot(by) * (cx - bx).hypot(cy - by) {
        return None;
    }
    let (b_sq, c_sq) = (bx * bx + by * by, cx * cx + cy * cy);
    let d = 2.0 * (bx * cy - by * cx);
    let center = pt(first.x + (cy * b_sq - by * c_sq) / d, first.y + (bx * c_sq - cx * b_sq) / d);
    let angle_to = |point: Point| modulo(center.angle_to(point), 360.0);
    let radius = first.dist(center);
    Some(if turn > 0.0 {
        ThreePointArc { center, radius, start_angle: angle_to(first), end_angle: angle_to(third) }
    } else {
        ThreePointArc { center, radius, start_angle: angle_to(third), end_angle: angle_to(first) }
    })
}

/// Arc through start and end for a DXF polyline bulge (tan of a quarter of the sweep).
pub fn bulge_arc(start: Point, end: Point, bulge: f64) -> BulgeArc {
    let (dx, dy) = (end.x - start.x, end.y - start.y);
    let chord = dx.hypot(dy);
    let offset = (chord / 2.0) * (1.0 - bulge * bulge) / (2.0 * bulge);
    let center = pt((start.x + end.x) / 2.0 - dy / chord * offset, (start.y + end.y) / 2.0 + dx / chord * offset);
    let radius = (chord / 2.0) * (1.0 + bulge * bulge) / (2.0 * bulge.abs());
    let start_angle = (start.y - center.y).atan2(start.x - center.x);
    BulgeArc { center, radius, start_angle, sweep: 4.0 * bulge.atan() }
}

/// How far an arc running counter-clockwise from start to end turns, in degrees. A closed arc is a full turn.
pub fn sweep_degrees(start_angle: f64, end_angle: f64) -> f64 {
    let sweep = modulo(end_angle - start_angle, 360.0);
    if sweep == 0.0 { 360.0 } else { sweep }
}

pub const TAU: f64 = 2.0 * PI;

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn modulo_follows_the_divisor_sign() {
        assert!(close(modulo(-90.0, 360.0), 270.0));
        assert!(close(modulo(370.0, 360.0), 10.0));
        assert_eq!(modulo(0.0, 360.0), 0.0);
    }

    #[test]
    fn three_points_counter_clockwise() {
        let arc = arc_through_points(pt(1.0, 0.0), pt(0.0, 1.0), pt(-1.0, 0.0)).unwrap();
        assert!(arc.center.dist(pt(0.0, 0.0)) < 1e-9);
        assert!(close(arc.radius, 1.0));
        assert!(close(arc.start_angle, 0.0));
        assert!(close(arc.end_angle, 180.0));
    }

    #[test]
    fn three_points_clockwise_swaps_the_ends() {
        let arc = arc_through_points(pt(-1.0, 0.0), pt(0.0, 1.0), pt(1.0, 0.0)).unwrap();
        assert!(close(arc.start_angle, 0.0));
        assert!(close(arc.end_angle, 180.0));
    }

    #[test]
    fn points_in_a_line_make_no_arc() {
        assert!(arc_through_points(pt(0.0, 0.0), pt(1.0, 1.0), pt(2.0, 2.0)).is_none());
    }

    #[test]
    fn half_circle_bulge() {
        let arc = bulge_arc(pt(0.0, 0.0), pt(2.0, 0.0), 1.0);
        assert!(arc.center.dist(pt(1.0, 0.0)) < 1e-9);
        assert!(close(arc.radius, 1.0));
        assert!(close(arc.sweep, PI));
    }
}

/// The most chords an arc is drawn with, however fine the tolerance.
pub const MAX_ARC_SEGMENTS: f64 = 2048.0;

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

/// An axis-aligned box: x0, y0, x1, y1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

impl Bounds {
    pub fn around(points: impl IntoIterator<Item = Point>) -> Bounds {
        let mut bounds = Bounds { x0: f64::INFINITY, y0: f64::INFINITY, x1: f64::NEG_INFINITY, y1: f64::NEG_INFINITY };
        for p in points {
            bounds.x0 = bounds.x0.min(p.x);
            bounds.y0 = bounds.y0.min(p.y);
            bounds.x1 = bounds.x1.max(p.x);
            bounds.y1 = bounds.y1.max(p.y);
        }
        bounds
    }

    /// The box with these two corners, in any order.
    pub fn from_corners(first: Point, second: Point) -> Bounds {
        Bounds::around([first, second])
    }

    pub fn square_around(center: Point, radius: f64) -> Bounds {
        Bounds { x0: center.x - radius, y0: center.y - radius, x1: center.x + radius, y1: center.y + radius }
    }

    pub fn union(self, other: Bounds) -> Bounds {
        Bounds { x0: self.x0.min(other.x0), y0: self.y0.min(other.y0), x1: self.x1.max(other.x1), y1: self.y1.max(other.y1) }
    }

    pub fn touches(&self, other: &Bounds) -> bool {
        !(self.x1 < other.x0 || self.x0 > other.x1 || self.y1 < other.y0 || self.y0 > other.y1)
    }

    pub fn contains(&self, p: Point) -> bool {
        self.x0 <= p.x && p.x <= self.x1 && self.y0 <= p.y && p.y <= self.y1
    }

    pub fn width(&self) -> f64 {
        self.x1 - self.x0
    }

    pub fn height(&self) -> f64 {
        self.y1 - self.y0
    }

    pub fn center(&self) -> Point {
        pt((self.x0 + self.x1) / 2.0, (self.y0 + self.y1) / 2.0)
    }

    pub fn diagonal(&self) -> f64 {
        self.width().hypot(self.height())
    }

    pub fn grow(self, margin: f64) -> Bounds {
        Bounds { x0: self.x0 - margin, y0: self.y0 - margin, x1: self.x1 + margin, y1: self.y1 + margin }
    }

    /// Damaged files can hold coordinates that are not numbers. They are treated as sitting at the origin.
    pub(super) fn finite(self) -> Bounds {
        if [self.x0, self.y0, self.x1, self.y1].iter().all(|v| v.is_finite()) {
            self
        } else {
            Bounds { x0: 0.0, y0: 0.0, x1: 0.0, y1: 0.0 }
        }
    }
}
