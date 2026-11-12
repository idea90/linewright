//! User coordinate systems: an origin and the direction of the X axis, in the drawing's world coordinates. Points
//! typed and shown, the grid, snap, ortho and polar tracking all follow the current UCS; the drawing itself is
//! always kept in world coordinates. Linewright draws in the XY plane, so a UCS turns only about the Z axis.

use super::geometry::{Point, direction, pt};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ucs {
    pub origin: Point,
    /// The X axis's direction, in degrees counter-clockwise from the world's X axis.
    pub angle: f64,
}

impl Default for Ucs {
    fn default() -> Ucs {
        Ucs::WORLD
    }
}

impl Ucs {
    pub const WORLD: Ucs = Ucs { origin: Point { x: 0.0, y: 0.0 }, angle: 0.0 };

    /// A UCS at an origin with its X axis at an angle, the angle brought into 0 to 360.
    pub fn new(origin: Point, angle: f64) -> Ucs {
        Ucs { origin, angle: angle.rem_euclid(360.0) }
    }

    pub fn is_world(&self) -> bool {
        self.origin == Point::default() && self.angle.rem_euclid(360.0) < 1e-12
    }

    /// The UCS's X axis, as a world direction of length one.
    pub fn x_axis(&self) -> Point {
        direction(self.angle)
    }

    pub fn y_axis(&self) -> Point {
        direction(self.angle + 90.0)
    }

    /// A direction given in the UCS, in world terms.
    pub fn vector_to_world(&self, v: Point) -> Point {
        // Square to the world, a direction is exact: no rounding from the sines of right angles.
        match self.quarter_turns() {
            Some(0) => v,
            Some(1) => pt(-v.y, v.x),
            Some(2) => pt(-v.x, -v.y),
            Some(3) => pt(v.y, -v.x),
            _ => self.x_axis() * v.x + self.y_axis() * v.y,
        }
    }

    /// How many quarter turns the X axis is from the world's, when it is a whole number of them.
    fn quarter_turns(&self) -> Option<i64> {
        let turns = self.angle.rem_euclid(360.0) / 90.0;
        (turns.fract() == 0.0).then_some(turns as i64 % 4)
    }

    /// A world direction, in the UCS's terms.
    pub fn vector_to_ucs(&self, v: Point) -> Point {
        match self.quarter_turns() {
            Some(0) => v,
            Some(1) => pt(v.y, -v.x),
            Some(2) => pt(-v.x, -v.y),
            Some(3) => pt(-v.y, v.x),
            _ => pt(v.dot(self.x_axis()), v.dot(self.y_axis())),
        }
    }

    /// A point given in UCS coordinates, in world coordinates.
    pub fn to_world(&self, p: Point) -> Point {
        self.origin + self.vector_to_world(p)
    }

    /// A world point, in UCS coordinates.
    pub fn to_ucs(&self, p: Point) -> Point {
        self.vector_to_ucs(p - self.origin)
    }

    /// The UCS with its X axis turned about its origin.
    pub fn rotated(&self, degrees: f64) -> Ucs {
        Ucs { angle: (self.angle + degrees).rem_euclid(360.0), ..*self }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn points_go_to_the_world_and_back() {
        let ucs = Ucs { origin: pt(10.0, 5.0), angle: 90.0 };
        let world = ucs.to_world(pt(2.0, 1.0));
        assert!(world.dist(pt(9.0, 7.0)) < 1e-12, "{world:?}");
        assert!(ucs.to_ucs(world).dist(pt(2.0, 1.0)) < 1e-12);
        assert!(Ucs::WORLD.is_world() && !ucs.is_world());
        assert!(ucs.rotated(270.0).angle.abs() < 1e-12);
    }
}
