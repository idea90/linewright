use crate::model::geometry::{Point, direction, pt};

const POLAR_TOLERANCE: f64 = 5.0; // degrees either side of an increment that the cursor is pulled onto it

/// Python's round(): halves go to the even neighbour.
fn round_half_even(value: f64) -> f64 {
    let rounded = value.round();
    if (value - value.trunc()).abs() == 0.5 { 2.0 * (value / 2.0).round() } else { rounded }
}

pub fn snap_to_grid(point: Point, step: f64) -> Point {
    pt(round_half_even(point.x / step) * step, round_half_even(point.y / step) * step)
}

/// Keep whichever of horizontal or vertical the cursor is closer to.
pub fn constrain_ortho(base: Point, point: Point) -> Point {
    if (point.x - base.x).abs() >= (point.y - base.y).abs() { pt(point.x, base.y) } else { pt(base.x, point.y) }
}

/// Pull the cursor onto the nearest multiple of the increment angle when it is within tolerance.
pub fn constrain_polar(base: Point, point: Point, increment: f64) -> Point {
    let length = base.dist(point);
    if length == 0.0 {
        return point;
    }
    let angle = base.angle_to(point);
    let target = round_half_even(angle / increment) * increment;
    let error = (angle - target).abs();
    if error.min(360.0 - error) > POLAR_TOLERANCE {
        return point;
    }
    let reach = length * (angle - target).to_radians().cos();
    base + direction(target) * reach
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_rounds_to_the_nearest_step() {
        assert_eq!(snap_to_grid(pt(14.0, -6.0), 10.0), pt(10.0, -10.0));
    }

    #[test]
    fn ortho_keeps_the_longer_direction() {
        assert_eq!(constrain_ortho(pt(0.0, 0.0), pt(5.0, 2.0)), pt(5.0, 0.0));
        assert_eq!(constrain_ortho(pt(0.0, 0.0), pt(1.0, -3.0)), pt(0.0, -3.0));
    }

    #[test]
    fn polar_pulls_near_angles_only() {
        let near = constrain_polar(pt(0.0, 0.0), pt(10.0, 10.5), 45.0);
        assert!((near.x - near.y).abs() < 1e-9);
        let far = pt(10.0, 3.0);
        assert_eq!(constrain_polar(pt(0.0, 0.0), far, 45.0), far);
    }
}
