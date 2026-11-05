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
