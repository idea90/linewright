use super::primitives::{Primitive, on_primitive};
use crate::model::geometry::{Point, pt};

const TOLERANCE: f64 = 1e-9;

/// Points where two lines, arcs or circles cross, limited to the parts that actually exist.
pub fn intersections(first: &Primitive, second: &Primitive) -> Vec<Point> {
    let points = match (first, second) {
        (Primitive::Line { start: a, end: b }, Primitive::Line { start: c, end: d }) => {
            line_line_meeting(*a, *b, *c, *d).into_iter().collect()
        }
        (Primitive::Line { start, end }, round) | (round, Primitive::Line { start, end }) => {
            let (center, radius) = round.round().expect("not a line");
            line_circle(*start, *end, center, radius)
        }
        _ => {
            let (c1, r1) = first.round().expect("round");
            let (c2, r2) = second.round().expect("round");
            circle_circle(c1, r1, c2, r2)
        }
    };
    points.into_iter().filter(|p| on_primitive(first, *p) && on_primitive(second, *p)).collect()
}

/// Where the two lines would meet if extended without end (nothing if parallel).
pub fn line_line_meeting(a: Point, b: Point, c: Point, d: Point) -> Option<Point> {
    let d1 = b - a;
    let d2 = d - c;
    let denominator = d1.cross(d2);
    if denominator.abs() <= TOLERANCE * d1.length() * d2.length() {
        return None;
    }
    let along = (c - a).cross(d2) / denominator;
    Some(a + d1 * along)
}

fn line_circle(start: Point, end: Point, center: Point, radius: f64) -> Vec<Point> {
    let d = end - start;
    let f = start - center;
    let a = d.dot(d);
    if a == 0.0 {
        return vec![];
    }
    let b = 2.0 * f.dot(d);
    let c = f.dot(f) - radius * radius;
    let discriminant = b * b - 4.0 * a * c;
    if discriminant < -TOLERANCE * (b * b).max(4.0 * a * c.abs()) {
        return vec![];
    }
    let root = discriminant.max(0.0).sqrt();
    let mut parameters = vec![(-b - root) / (2.0 * a), (-b + root) / (2.0 * a)];
    parameters.sort_by(f64::total_cmp);
    parameters.dedup();
    parameters.into_iter().map(|t| start + d * t).collect()
}

fn circle_circle(c1: Point, r1: f64, c2: Point, r2: f64) -> Vec<Point> {
    let distance = c1.dist(c2);
    let slack = TOLERANCE * (r1 + r2 + distance);
    if distance == 0.0 || distance > r1 + r2 + slack || distance < (r1 - r2).abs() - slack {
        return vec![];
    }
    let along = (r1 * r1 - r2 * r2 + distance * distance) / (2.0 * distance);
    let height = (r1 * r1 - along * along).max(0.0).sqrt();
    let (ux, uy) = ((c2.x - c1.x) / distance, (c2.y - c1.y) / distance);
    let middle = pt(c1.x + along * ux, c1.y + along * uy);
    if height <= slack {
        return vec![middle];
    }
    vec![pt(middle.x - height * uy, middle.y + height * ux), pt(middle.x + height * uy, middle.y - height * ux)]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::primitives::seg;

    #[test]
    fn crossing_lines() {
        let found = intersections(&seg(pt(0.0, 0.0), pt(2.0, 2.0)), &seg(pt(0.0, 2.0), pt(2.0, 0.0)));
        assert_eq!(found, vec![pt(1.0, 1.0)]);
    }

    #[test]
    fn lines_that_would_meet_only_if_extended_do_not_cross() {
        assert!(intersections(&seg(pt(0.0, 0.0), pt(1.0, 0.0)), &seg(pt(2.0, -1.0), pt(2.0, 1.0))).is_empty());
    }

    #[test]
    fn line_through_a_circle() {
        let circle = Primitive::Circle { center: pt(0.0, 0.0), radius: 1.0 };
        let found = intersections(&seg(pt(-2.0, 0.0), pt(2.0, 0.0)), &circle);
        assert_eq!(found, vec![pt(-1.0, 0.0), pt(1.0, 0.0)]);
    }

    #[test]
    fn only_the_existing_part_of_an_arc_counts() {
        let upper = Primitive::Arc { center: pt(0.0, 0.0), radius: 1.0, start_angle: 0.0, end_angle: 180.0 };
        let found = intersections(&seg(pt(0.0, -2.0), pt(0.0, 2.0)), &upper);
        assert_eq!(found.len(), 1);
        assert!(found[0].dist(pt(0.0, 1.0)) < 1e-12);
    }

    #[test]
    fn two_circles() {
        let a = Primitive::Circle { center: pt(0.0, 0.0), radius: 1.0 };
        let b = Primitive::Circle { center: pt(1.0, 0.0), radius: 1.0 };
        assert_eq!(intersections(&a, &b).len(), 2);
    }
}
