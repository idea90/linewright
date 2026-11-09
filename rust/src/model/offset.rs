use super::entities::{Entity, Shape, vertex};
use super::geometry::{Point, pt};
use crate::model::intersect::line_line_meeting;
use crate::model::primitives::{distance_to, seg};

/// A copy moved sideways by the distance, toward the side point. None if it cannot be offset.
pub fn offset_entity(entity: &Entity, distance: f64, side: Point) -> Option<Entity> {
    let shape = match &entity.shape {
        Shape::Line { start, end } => {
            if start == end {
                return None;
            }
            let (start, end) = shifted(*start, *end, distance * side_sign(*start, *end, side));
            Shape::Line { start, end }
        }
        Shape::Circle { center, radius } => Shape::Circle { center: *center, radius: offset_radius(*center, *radius, distance, side)? },
        Shape::Arc { center, radius, start_angle, end_angle } => Shape::Arc {
            center: *center,
            radius: offset_radius(*center, *radius, distance, side)?,
            start_angle: *start_angle,
            end_angle: *end_angle,
        },
        Shape::Polyline { vertices, closed } => offset_polyline(vertices, *closed, distance, side)?,
        _ => return None,
    };
    Some(entity.derive(shape))
}

fn left_normal(start: Point, end: Point) -> Point {
    let length = start.dist(end);
    pt(-(end.y - start.y) / length, (end.x - start.x) / length)
}

fn side_sign(start: Point, end: Point, side: Point) -> f64 {
    if (side - start).dot(left_normal(start, end)) >= 0.0 { 1.0 } else { -1.0 }
}

fn shifted(start: Point, end: Point, amount: f64) -> (Point, Point) {
    let shift = left_normal(start, end) * amount;
    (start + shift, end + shift)
}

fn offset_radius(center: Point, radius: f64, distance: f64, side: Point) -> Option<f64> {
    let inside = center.dist(side) < radius;
    let radius = if inside { radius - distance } else { radius + distance };
    (radius > 0.0).then_some(radius)
}

fn distinct_vertices(vertices: &[crate::model::entities::Vertex], closed: bool) -> Vec<Point> {
    let mut points: Vec<Point> = vec![];
    for v in vertices {
        if points.last() != Some(&v.point()) {
            points.push(v.point());
        }
    }
    if closed && points.len() > 1 && points.first() == points.last() {
        points.pop();
    }
    points
}

fn offset_polyline(vertices: &[crate::model::entities::Vertex], closed: bool, distance: f64, side: Point) -> Option<Shape> {
    // shortcut: straight segments only, joined with sharp corners and no trimming of loops that
    // collapse; curved segments need arc offsetting and trimming.
    if vertices.iter().any(|v| v.bulge != 0.0) {
        return None;
    }
    let points = distinct_vertices(vertices, closed);
    if points.len() < 2 {
        return None;
    }
    let count = if closed { points.len() } else { points.len() - 1 };
    let segments: Vec<(Point, Point)> = (0..count).map(|i| (points[i], points[(i + 1) % points.len()])).collect();
    let nearest =
        segments.iter().min_by(|a, b| distance_to(&seg(a.0, a.1), side).total_cmp(&distance_to(&seg(b.0, b.1), side))).copied()?;
    let amount = distance * side_sign(nearest.0, nearest.1, side);
    let moved: Vec<(Point, Point)> = segments.iter().map(|(start, end)| shifted(*start, *end, amount)).collect();
    let corners = (1..moved.len()).map(|i| corner(moved[i - 1], moved[i]));
    let result: Vec<Point> = if closed {
        std::iter::once(corner(moved[moved.len() - 1], moved[0])).chain(corners).collect()
    } else {
        std::iter::once(moved[0].0).chain(corners).chain(std::iter::once(moved[moved.len() - 1].1)).collect()
    };
    Some(Shape::Polyline { vertices: result.into_iter().map(vertex).collect(), closed })
}

/// Where two offset segments meet. Collinear segments just keep the end of the first.
fn corner(before: (Point, Point), after: (Point, Point)) -> Point {
    line_line_meeting(before.0, before.1, after.0, after.1).unwrap_or(before.1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::entities::{circle, line, polyline_from};

    #[test]
    fn line_moves_toward_the_side_point() {
        let moved = offset_entity(&line(pt(0.0, 0.0), pt(10.0, 0.0)), 2.0, pt(5.0, -7.0)).unwrap();
        assert_eq!(moved.shape, Shape::Line { start: pt(0.0, -2.0), end: pt(10.0, -2.0) });
    }

    #[test]
    fn circle_shrinks_from_inside_and_can_vanish() {
        let round = circle(pt(0.0, 0.0), 5.0);
        assert_eq!(offset_entity(&round, 2.0, pt(1.0, 0.0)).unwrap().shape, Shape::Circle { center: pt(0.0, 0.0), radius: 3.0 });
        assert!(offset_entity(&round, 6.0, pt(1.0, 0.0)).is_none());
    }

    #[test]
    fn closed_square_grows_outward() {
        let square = polyline_from(&[pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 10.0), pt(0.0, 10.0)], true);
        let Shape::Polyline { vertices, closed } = offset_entity(&square, 1.0, pt(-5.0, 5.0)).unwrap().shape else { panic!() };
        assert!(closed);
        let points: Vec<Point> = vertices.iter().map(|v| v.point()).collect();
        for expected in [pt(-1.0, -1.0), pt(11.0, -1.0), pt(11.0, 11.0), pt(-1.0, 11.0)] {
            assert!(points.iter().any(|p| p.dist(expected) < 1e-9), "{expected:?} missing from {points:?}");
        }
    }
}
