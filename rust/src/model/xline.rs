//! Construction lines: XLINE runs without end both ways, RAY one way from its base point.

use super::geometry::{Bounds, Point};

/// How far either way an endless line is taken for what needs ends: snapping, the index, plotting. Far beyond
/// any drawing, yet small enough for coordinates to stay precise.
pub const FAR: f64 = 1e7;

/// The ends used for an endless line: `FAR` along it each way (from the base only, for a ray).
pub fn far_ends(base: Point, direction: Point, ray: bool) -> (Point, Point) {
    let start = if ray { base } else { base - direction * FAR };
    (start, base + direction * FAR)
}

/// The part of an endless line inside an area, for drawing it. None when it misses.
pub fn clipped(base: Point, direction: Point, ray: bool, area: &Bounds) -> Option<(Point, Point)> {
    let (mut t0, mut t1) = (if ray { 0.0 } else { f64::NEG_INFINITY }, f64::INFINITY);
    for (d, low, high, p) in [(direction.x, area.x0, area.x1, base.x), (direction.y, area.y0, area.y1, base.y)] {
        if d == 0.0 {
            if p < low || p > high {
                return None;
            }
        } else {
            let (a, b) = ((low - p) / d, (high - p) / d);
            t0 = t0.max(a.min(b));
            t1 = t1.min(a.max(b));
        }
    }
    (t0 <= t1).then(|| (base + direction * t0, base + direction * t1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::geometry::pt;

    #[test]
    fn an_endless_line_is_cut_to_the_area_and_a_ray_starts_at_its_base() {
        let area = Bounds::from_corners(pt(-10.0, -10.0), pt(10.0, 10.0));
        assert_eq!(clipped(pt(0.0, 0.0), pt(1.0, 0.0), false, &area), Some((pt(-10.0, 0.0), pt(10.0, 0.0))));
        assert_eq!(clipped(pt(0.0, 0.0), pt(1.0, 0.0), true, &area), Some((pt(0.0, 0.0), pt(10.0, 0.0))));
        assert_eq!(clipped(pt(0.0, 20.0), pt(1.0, 0.0), false, &area), None);
        assert_eq!(clipped(pt(20.0, 0.0), pt(1.0, 0.0), true, &area), None, "pointing away");
    }
}
