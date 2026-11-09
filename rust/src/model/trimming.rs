use super::entities::{Entity, Shape, vertex};
use super::geometry::{Point, modulo, sweep_degrees};
use crate::model::intersect::intersections;
use crate::model::primitives::{Primitive, distance_to, lerp, line_parameter, primitives, seg};

const PARAMETER_EPSILON: f64 = 1e-9; // in the units of the parameter: fractions of a line, or degrees
const DISTANCE_EPSILON: f64 = 1e-9;
const EXTEND_REACH: f64 = 1e6; // how many lengths of the object an extension may reach when looking for a boundary

/// An entity seen as a path with a parameter: a line runs 0 to 1, an arc or circle in degrees from its start,
/// a polyline of straight segments from 0 to its number of segments.
enum Curve<'a> {
    Line { entity: &'a Entity, start: Point, end: Point },
    Arc { entity: &'a Entity, center: Point, radius: f64, start_angle: f64, span: f64 },
    Circle { entity: &'a Entity, center: Point, radius: f64 },
    Polyline { entity: &'a Entity, points: Vec<Point>, closed: bool },
}

impl<'a> Curve<'a> {
    fn of(entity: &'a Entity) -> Option<Curve<'a>> {
        Some(match &entity.shape {
            Shape::Line { start, end } => Curve::Line { entity, start: *start, end: *end },
            Shape::Arc { center, radius, start_angle, end_angle } => Curve::Arc {
                entity,
                center: *center,
                radius: *radius,
                start_angle: *start_angle,
                span: sweep_degrees(*start_angle, *end_angle),
            },
            Shape::Circle { center, radius } => Curve::Circle { entity, center: *center, radius: *radius },
            Shape::Polyline { vertices, closed } if vertices.len() >= 2 && vertices.iter().all(|v| v.bulge == 0.0) => {
                Curve::Polyline { entity, points: vertices.iter().map(|v| v.point()).collect(), closed: *closed }
            }
            _ => return None,
        })
    }

    fn closed(&self) -> bool {
        match self {
            Curve::Circle { .. } => true,
            Curve::Polyline { closed, .. } => *closed,
            _ => false,
        }
    }

    fn span(&self) -> f64 {
        match self {
            Curve::Line { .. } => 1.0,
            Curve::Arc { span, .. } => *span,
            Curve::Circle { .. } => 360.0,
            Curve::Polyline { points, closed, .. } => (if *closed { points.len() } else { points.len() - 1 }) as f64,
        }
    }

    fn segment(points: &[Point], index: usize) -> (Point, Point) {
        (points[index], points[(index + 1) % points.len()])
    }

    fn param_of(&self, point: Point) -> f64 {
        match self {
            Curve::Line { start, end, .. } => line_parameter(*start, *end, point).map_or(0.0, |t| t.clamp(0.0, 1.0)),
            Curve::Arc { center, start_angle, span, .. } => {
                let offset = modulo(center.angle_to(point) - start_angle, 360.0);
                if offset <= *span {
                    offset
                } else if offset - span < 360.0 - offset {
                    *span
                } else {
                    0.0
                }
            }
            Curve::Circle { center, .. } => modulo(center.angle_to(point), 360.0),
            Curve::Polyline { points, .. } => {
                let count = self.span() as usize;
                let distance = |i: usize| {
                    let (a, b) = Self::segment(points, i);
                    distance_to(&seg(a, b), point)
                };
                let nearest = (0..count).min_by(|a, b| distance(*a).total_cmp(&distance(*b))).unwrap_or(0);
                let (a, b) = Self::segment(points, nearest);
                nearest as f64 + line_parameter(a, b, point).map_or(0.0, |t| t.clamp(0.0, 1.0))
            }
        }
    }

    fn cut_params(&self, edges: &[Primitive]) -> Vec<f64> {
        match self {
            Curve::Line { start, end, .. } => {
                let me = seg(*start, *end);
                edges.iter().flat_map(|edge| intersections(&me, edge)).map(|p| line_parameter(*start, *end, p).unwrap_or(0.0)).collect()
            }
            Curve::Arc { center, radius, start_angle, span, .. } => {
                let me = Primitive::Arc { center: *center, radius: *radius, start_angle: *start_angle, end_angle: start_angle + span };
                edges
                    .iter()
                    .flat_map(|edge| intersections(&me, edge))
                    .map(|p| modulo(center.angle_to(p) - start_angle, 360.0).min(*span))
                    .collect()
            }
            Curve::Circle { center, radius, .. } => {
                let me = Primitive::Circle { center: *center, radius: *radius };
                edges.iter().flat_map(|edge| intersections(&me, edge)).map(|p| modulo(center.angle_to(p), 360.0)).collect()
            }
            Curve::Polyline { points, .. } => {
                let mut found = vec![];
                for index in 0..self.span() as usize {
                    let (a, b) = Self::segment(points, index);
                    if a == b {
                        continue;
                    }
                    for edge in edges {
                        found.extend(
                            intersections(&seg(a, b), edge).into_iter().map(|p| index as f64 + line_parameter(a, b, p).unwrap_or(0.0)),
                        );
                    }
                }
                found
            }
        }
    }

    fn polyline_point_at(&self, points: &[Point], mut u: f64) -> Point {
        let span = self.span();
        if self.closed() {
            u = modulo(u, span);
        }
        let index = (u.floor() as i64).clamp(0, span as i64 - 1) as usize;
        let (a, b) = Self::segment(points, index);
        lerp(a, b, u - index as f64)
    }

    fn piece(&self, start: f64, end: f64) -> Option<Entity> {
        match self {
            Curve::Line { entity, start: a, end: b } => {
                Some(entity.derive(Shape::Line { start: lerp(*a, *b, start), end: lerp(*a, *b, end) }))
            }
            Curve::Arc { entity, center, radius, start_angle, .. } => Some(entity.derive(Shape::Arc {
                center: *center,
                radius: *radius,
                start_angle: modulo(start_angle + start, 360.0),
                end_angle: modulo(start_angle + end, 360.0),
            })),
            Curve::Circle { entity, center, radius } => Some(entity.derive(Shape::Arc {
                center: *center,
                radius: *radius,
                start_angle: modulo(start, 360.0),
                end_angle: modulo(end, 360.0),
            })),
            Curve::Polyline { entity, points, .. } => {
                let mut path = vec![self.polyline_point_at(points, start)];
                let mut corner = start.floor() + 1.0;
                while corner < end - PARAMETER_EPSILON {
                    path.push(points[(corner as usize) % points.len()]);
                    corner += 1.0;
                }
                path.push(self.polyline_point_at(points, end));
                let mut distinct: Vec<Point> = vec![];
                for p in path {
                    if distinct.last().is_none_or(|last| p.dist(*last) > DISTANCE_EPSILON) {
                        distinct.push(p);
                    }
                }
                (distinct.len() >= 2)
                    .then(|| entity.derive(Shape::Polyline { vertices: distinct.into_iter().map(vertex).collect(), closed: false }))
            }
        }
    }
}

fn edges_for(target: &Entity, boundaries: &[Entity]) -> Vec<Primitive> {
    boundaries.iter().filter(|e| e.id != target.id).flat_map(primitives).collect()
}

fn distinct(mut values: Vec<f64>) -> Vec<f64> {
    values.sort_by(f64::total_cmp);
    let mut found: Vec<f64> = vec![];
    for value in values {
        if found.last().is_none_or(|last| value - last > PARAMETER_EPSILON) {
            found.push(value);
        }
    }
    found
}

/// What is left of the target after cutting away the stretch under the click, between the boundaries.
///
/// None means it cannot be trimmed there. An empty list means nothing is left.
pub fn trim_entity(target: &Entity, click: Point, boundaries: &[Entity]) -> Option<Vec<Entity>> {
    let curve = Curve::of(target)?;
    let edges = edges_for(target, boundaries);
    if edges.is_empty() {
        return None;
    }
    let span = curve.span();
    let cuts = curve.cut_params(&edges);
    if curve.closed() {
        return trim_closed(&curve, curve.param_of(click), distinct(cuts.into_iter().map(|c| modulo(c, span)).collect()));
    }
    let cuts = distinct(cuts.into_iter().filter(|c| PARAMETER_EPSILON < *c && *c < span - PARAMETER_EPSILON).collect());
    if cuts.is_empty() {
        return None;
    }
    let u = curve.param_of(click);
    let lower = cuts.iter().copied().filter(|c| *c < u).fold(0.0, f64::max);
    let upper = cuts.iter().copied().filter(|c| *c > u).fold(span, f64::min);
    let mut pieces = vec![];
    if lower > PARAMETER_EPSILON {
        pieces.extend(curve.piece(0.0, lower));
    }
    if upper < span - PARAMETER_EPSILON {
        pieces.extend(curve.piece(upper, span));
    }
    Some(pieces)
}

fn trim_closed(curve: &Curve, u: f64, cuts: Vec<f64>) -> Option<Vec<Entity>> {
    if cuts.len() < 2 {
        return None;
    }
    let span = curve.span();
    let lower = cuts.iter().copied().filter(|c| *c < u).reduce(f64::max).unwrap_or(cuts[cuts.len() - 1] - span);
    let upper = cuts.iter().copied().filter(|c| *c > u).reduce(f64::min).unwrap_or(cuts[0] + span);
    Some(curve.piece(upper, lower + span).into_iter().collect())
}

/// What is left of the target after cutting out the stretch between two points on it. On a circle or closed
/// polyline the stretch runs counter-clockwise (forward) from the first point to the second, as AutoCAD's BREAK.
/// The same point twice splits it there. None means it cannot be broken.
pub fn break_entity(target: &Entity, first: Point, second: Point) -> Option<Vec<Entity>> {
    let curve = Curve::of(target)?;
    let span = curve.span();
    let (u, v) = (curve.param_of(first), curve.param_of(second));
    if curve.closed() {
        let end = if v > u { v } else { v + span };
        if (end - u).abs() < PARAMETER_EPSILON {
            return None;
        }
        return Some(curve.piece(end, u + span).into_iter().collect());
    }
    let (low, high) = if u <= v { (u, v) } else { (v, u) };
    let mut pieces = vec![];
    if low > PARAMETER_EPSILON {
        pieces.extend(curve.piece(0.0, low));
    }
    if high < span - PARAMETER_EPSILON {
        pieces.extend(curve.piece(high, span));
    }
    Some(pieces)
}

/// The first point where the path from start through end, carried on past end, meets an edge.
fn beyond(start: Point, end: Point, edges: &[Primitive]) -> Option<Point> {
    let length = start.dist(end);
    if length == 0.0 {
        return None;
    }
    let unit = (end - start) * (1.0 / length);
    let reach = EXTEND_REACH * length.max(1.0);
    let carrier = seg(start, end + unit * reach);
    let mut best: Option<Point> = None;
    let mut best_distance = f64::INFINITY;
    for edge in edges {
        for point in intersections(&carrier, edge) {
            let ahead = (point - end).dot(unit);
            if DISTANCE_EPSILON * length.max(1.0) < ahead && ahead < best_distance {
                best = Some(point);
                best_distance = ahead;
            }
        }
    }
    best
}

/// The target with the end nearest the click carried on to the first boundary, or None if none is in the way.
pub fn extend_entity(target: &Entity, click: Point, boundaries: &[Entity]) -> Option<Entity> {
    let edges = edges_for(target, boundaries);
    if edges.is_empty() {
        return None;
    }
    match &target.shape {
        Shape::Line { start, end } => {
            if line_parameter(*start, *end, click).unwrap_or(0.0) >= 0.5 {
                let point = beyond(*start, *end, &edges)?;
                Some(target.with_shape(Shape::Line { start: *start, end: point }))
            } else {
                let point = beyond(*end, *start, &edges)?;
                Some(target.with_shape(Shape::Line { start: point, end: *end }))
            }
        }
        Shape::Arc { center, radius, start_angle, end_angle } => {
            let curve = Curve::of(target)?;
            let span = curve.span();
            let toward_end = curve.param_of(click) >= span / 2.0;
            let full = Primitive::Circle { center: *center, radius: *radius };
            let mut best: Option<f64> = None;
            for edge in &edges {
                for point in intersections(&full, edge) {
                    let angle = center.angle_to(point);
                    let gap = if toward_end { modulo(angle - end_angle, 360.0) } else { modulo(start_angle - angle, 360.0) };
                    if PARAMETER_EPSILON < gap && gap < 360.0 - span && best.is_none_or(|b| gap < b) {
                        best = Some(gap);
                    }
                }
            }
            let best = best?;
            let (start_angle, end_angle) =
                if toward_end { (*start_angle, modulo(end_angle + best, 360.0)) } else { (modulo(start_angle - best, 360.0), *end_angle) };
            Some(target.with_shape(Shape::Arc { center: *center, radius: *radius, start_angle, end_angle }))
        }
        Shape::Polyline { vertices, closed: false } => {
            let curve = Curve::of(target)?;
            let mut points: Vec<Point> = vertices.iter().map(|v| v.point()).collect();
            let last = points.len() - 1;
            if curve.param_of(click) >= curve.span() / 2.0 {
                points[last] = beyond(points[last - 1], points[last], &edges)?;
            } else {
                points[0] = beyond(points[1], points[0], &edges)?;
            }
            Some(target.with_shape(Shape::Polyline { vertices: points.into_iter().map(vertex).collect(), closed: false }))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::entities::{arc, circle, line, polyline_from};
    use crate::model::geometry::pt;

    fn line_ends(entity: &Entity) -> (Point, Point) {
        match entity.shape {
            Shape::Line { start, end } => (start, end),
            _ => panic!("not a line"),
        }
    }

    #[test]
    fn trim_the_middle_of_a_line_between_two_cutters() {
        let target = line(pt(0.0, 0.0), pt(10.0, 0.0));
        let cutters = [line(pt(3.0, -1.0), pt(3.0, 1.0)), line(pt(7.0, -1.0), pt(7.0, 1.0))];
        let pieces = trim_entity(&target, pt(5.0, 0.0), &cutters).unwrap();
        assert_eq!(pieces.len(), 2);
        assert_eq!(line_ends(&pieces[0]), (pt(0.0, 0.0), pt(3.0, 0.0)));
        assert_eq!(line_ends(&pieces[1]), (pt(7.0, 0.0), pt(10.0, 0.0)));
    }

    #[test]
    fn trim_the_end_of_a_line() {
        let target = line(pt(0.0, 0.0), pt(10.0, 0.0));
        let cutters = [line(pt(3.0, -1.0), pt(3.0, 1.0))];
        let pieces = trim_entity(&target, pt(8.0, 0.0), &cutters).unwrap();
        assert_eq!(pieces.len(), 1);
        assert_eq!(line_ends(&pieces[0]), (pt(0.0, 0.0), pt(3.0, 0.0)));
    }

    #[test]
    fn nothing_to_cut_against() {
        let target = line(pt(0.0, 0.0), pt(10.0, 0.0));
        assert!(trim_entity(&target, pt(5.0, 0.0), &[line(pt(20.0, 1.0), pt(20.0, 2.0))]).is_none());
        assert!(trim_entity(&target, pt(5.0, 0.0), std::slice::from_ref(&target)).is_none());
    }

    #[test]
    fn trimming_a_circle_leaves_an_arc() {
        let round = circle(pt(0.0, 0.0), 5.0);
        let cutter = line(pt(-10.0, 0.0), pt(10.0, 0.0));
        let pieces = trim_entity(&round, pt(0.0, 5.0), &[cutter]).unwrap();
        let Shape::Arc { start_angle, end_angle, .. } = pieces[0].shape else { panic!() };
        assert!((start_angle - 180.0).abs() < 1e-9 && end_angle.abs() < 1e-9);
    }

    #[test]
    fn trimming_a_closed_polyline_opens_it() {
        let square = polyline_from(&[pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 10.0), pt(0.0, 10.0)], true);
        let cutter = line(pt(5.0, -1.0), pt(5.0, 11.0));
        let pieces = trim_entity(&square, pt(10.0, 5.0), &[cutter]).unwrap();
        let Shape::Polyline { vertices, closed } = &pieces[0].shape else { panic!() };
        assert!(!closed);
        let points: Vec<Point> = vertices.iter().map(|v| v.point()).collect();
        assert_eq!(points, vec![pt(5.0, 10.0), pt(0.0, 10.0), pt(0.0, 0.0), pt(5.0, 0.0)]);
    }

    #[test]
    fn break_cuts_out_the_stretch_between_two_points() {
        let pieces = break_entity(&line(pt(0.0, 0.0), pt(10.0, 0.0)), pt(7.0, 0.0), pt(3.0, 0.0)).unwrap();
        assert_eq!(line_ends(&pieces[0]), (pt(0.0, 0.0), pt(3.0, 0.0)));
        assert_eq!(line_ends(&pieces[1]), (pt(7.0, 0.0), pt(10.0, 0.0)));
        let split = break_entity(&line(pt(0.0, 0.0), pt(10.0, 0.0)), pt(4.0, 0.0), pt(4.0, 0.0)).unwrap();
        assert_eq!(split.len(), 2, "the same point twice splits the line there");
        let left = break_entity(&circle(pt(0.0, 0.0), 1.0), pt(1.0, 0.0), pt(0.0, 1.0)).unwrap();
        let Shape::Arc { start_angle, end_angle, .. } = left[0].shape else { panic!() };
        assert!((start_angle - 90.0).abs() < 1e-9 && end_angle.abs() < 1e-9, "the quarter from 0 to 90 is gone");
    }

    #[test]
    fn extend_a_line_to_a_boundary_keeps_the_object() {
        let target = line(pt(0.0, 0.0), pt(5.0, 0.0));
        let boundary = line(pt(9.0, -1.0), pt(9.0, 1.0));
        let longer = extend_entity(&target, pt(4.0, 0.0), &[boundary]).unwrap();
        assert_eq!(longer.id, target.id);
        assert_eq!(line_ends(&longer), (pt(0.0, 0.0), pt(9.0, 0.0)));
        assert!(extend_entity(&target, pt(1.0, 0.0), &[line(pt(9.0, -1.0), pt(9.0, 1.0))]).is_none());
    }

    #[test]
    fn extend_an_arc_to_a_line() {
        let quarter = arc(pt(0.0, 0.0), 1.0, 0.0, 90.0);
        let boundary = line(pt(-2.0, 0.0), pt(0.0, 0.0));
        let longer = extend_entity(&quarter, pt(0.0, 1.0), &[boundary]).unwrap();
        let Shape::Arc { start_angle, end_angle, .. } = longer.shape else { panic!() };
        assert_eq!(start_angle, 0.0);
        assert!((end_angle - 180.0).abs() < 1e-9);
    }
}
