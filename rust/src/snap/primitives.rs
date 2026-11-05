use crate::model::dimension::dimension_parts;
use crate::model::entities::{Entity, Shape};
use crate::model::geometry::{Point, bulge_arc, direction, modulo, sweep_degrees};
use crate::model::index::entity_extent;
use crate::model::parts::{ellipse_points, entity_parts, flatten, spline_points};
use crate::model::transform::{IDENTITY, Similarity, insert_transform, transform_shape};
use crate::model::viewport::frame_corners;

pub const ANGLE_TOLERANCE: f64 = 1e-7; // degrees
pub const PARAMETER_TOLERANCE: f64 = 1e-9;

/// The simple shapes everything is made of, for snapping, picking and cutting.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Primitive {
    Line {
        start: Point,
        end: Point,
    },
    /// Angles in degrees, counter-clockwise from start to end.
    Arc {
        center: Point,
        radius: f64,
        start_angle: f64,
        end_angle: f64,
    },
    Circle {
        center: Point,
        radius: f64,
    },
}

pub fn seg(start: Point, end: Point) -> Primitive {
    Primitive::Line { start, end }
}

impl Primitive {
    pub fn round(&self) -> Option<(Point, f64)> {
        match *self {
            Primitive::Arc { center, radius, .. } | Primitive::Circle { center, radius } => Some((center, radius)),
            Primitive::Line { .. } => None,
        }
    }
}

/// The lines, arcs and circles an entity is made of. Polylines split into their segments.
pub fn primitives(entity: &Entity) -> Vec<Primitive> {
    match &entity.shape {
        Shape::Line { start, end } => vec![seg(*start, *end)],
        Shape::Circle { center, radius } => vec![Primitive::Circle { center: *center, radius: *radius }],
        Shape::Arc { center, radius, start_angle, end_angle } => {
            vec![Primitive::Arc { center: *center, radius: *radius, start_angle: *start_angle, end_angle: *end_angle }]
        }
        Shape::Polyline { vertices, closed } => {
            let count = if *closed { vertices.len() } else { vertices.len().saturating_sub(1) };
            (0..count)
                .map(|index| {
                    let here = vertices[index];
                    let next = vertices[(index + 1) % vertices.len()];
                    if here.bulge != 0.0 { bulge_segment(here.point(), next.point(), here.bulge) } else { seg(here.point(), next.point()) }
                })
                .collect()
        }
        Shape::Dimension { start, end, line_point, angle, look } => {
            let parts = dimension_parts(*start, *end, *line_point, *angle, look);
            parts.extension_lines.iter().chain([&parts.dimension_line]).chain(&parts.line_extension).map(|(a, b)| seg(*a, *b)).collect()
        }
        Shape::Text { .. } => vec![],
        Shape::Viewport { center, width, height, .. } => closed_chords(&frame_corners(*center, *width, *height)),
        Shape::Table { .. } => entity_parts(entity, f64::INFINITY).paths.iter().flat_map(|p| chords(p)).collect(),
        Shape::Image { position, u, v, .. } => closed_chords(&crate::model::entities::image_corners(*position, *u, *v)),
        Shape::XLine { base, direction, ray } => {
            let (start, end) = crate::model::xline::far_ends(*base, *direction, *ray);
            vec![seg(start, end)]
        }
        Shape::Annotation { .. } | Shape::Multileader { .. } => {
            entity_parts(entity, f64::INFINITY).paths.iter().flat_map(|p| chords(p)).collect()
        }
        Shape::Ellipse { center, major, ratio, start, end } => {
            chords(&ellipse_points(*center, *major, *ratio, *start, *end, major.length() * CURVE_TOLERANCE))
        }
        Shape::Spline { degree, control, knots, weights, .. } => chords(&spline_points(*degree, control, knots, weights)),
        Shape::Point { position } => vec![seg(*position, *position)],
        Shape::Solid { corners } | Shape::Wipeout { corners } => closed_chords(corners),
        Shape::Mline { vertices, closed, justification, scale, style } => {
            crate::model::mline::mline_paths(vertices, *closed, *justification, *scale, style)
                .iter()
                .flat_map(|(_, path)| path.windows(2).map(|w| seg(w[0], w[1])).collect::<Vec<_>>())
                .collect()
        }
        Shape::Hatch { loops, .. } => loops.iter().flat_map(|l| closed_chords(l)).collect(),
        Shape::Insert { .. } => insert_primitives(entity, &IDENTITY, 0),
    }
}

/// How far chords may stray from a curve, per its size, where primitives stand in for curves they cannot be.
const CURVE_TOLERANCE: f64 = 1e-3;
const MAX_BLOCK_DEPTH: usize = 16;

fn chords(points: &[Point]) -> Vec<Primitive> {
    points.windows(2).map(|w| seg(w[0], w[1])).collect()
}

fn closed_chords(points: &[Point]) -> Vec<Primitive> {
    (0..points.len()).map(|i| seg(points[i], points[(i + 1) % points.len()])).collect()
}

/// The primitives of a block's entities where an insert puts them. A transform that keeps shapes keeps circles
/// and arcs (and their centers to snap to); one that stretches them leaves chords.
fn insert_primitives(entity: &Entity, transform: &Similarity, depth: usize) -> Vec<Primitive> {
    let Shape::Insert { block, position, scale_x, scale_y, rotation, .. } = &entity.shape else {
        if transform.is_uniform() {
            return primitives(&entity.with_shape(transform_shape(&entity.shape, transform)));
        }
        let size = entity_extent(entity).diagonal() * transform.determinant().abs().sqrt();
        return flatten(entity, (size * CURVE_TOLERANCE).max(1e-9))
            .into_iter()
            .flat_map(|(_, parts)| parts.paths.into_iter().map(|p| p.iter().map(|q| transform.point(*q)).collect::<Vec<_>>()))
            .flat_map(|path| chords(&path))
            .collect();
    };
    if depth >= MAX_BLOCK_DEPTH {
        return vec![];
    }
    let placed = transform.after(&insert_transform(block.0.base, *position, *scale_x, *scale_y, *rotation));
    block.0.entities.iter().flat_map(|child| insert_primitives(child, &placed, depth + 1)).collect()
}

fn bulge_segment(start: Point, end: Point, bulge: f64) -> Primitive {
    let arc = bulge_arc(start, end, bulge);
    let mut first = arc.start_angle.to_degrees();
    let mut last = first + arc.sweep.to_degrees();
    if arc.sweep < 0.0 {
        std::mem::swap(&mut first, &mut last);
    }
    Primitive::Arc { center: arc.center, radius: arc.radius, start_angle: modulo(first, 360.0), end_angle: modulo(last, 360.0) }
}

pub fn arc_point(center: Point, radius: f64, angle: f64) -> Point {
    center + direction(angle) * radius
}

pub fn on_arc(center: Point, start_angle: f64, end_angle: f64, point: Point) -> bool {
    let offset = modulo(center.angle_to(point) - start_angle, 360.0);
    offset <= sweep_degrees(start_angle, end_angle) + ANGLE_TOLERANCE || offset >= 360.0 - ANGLE_TOLERANCE
}

/// Where the point's projection falls along the line: 0 at the start, 1 at the end.
pub fn line_parameter(start: Point, end: Point, point: Point) -> Option<f64> {
    let d = end - start;
    let length_squared = d.dot(d);
    if length_squared == 0.0 {
        return None;
    }
    Some((point - start).dot(d) / length_squared)
}

pub fn lerp(start: Point, end: Point, t: f64) -> Point {
    start + (end - start) * t
}

/// Whether a point already known to lie on the primitive's full line or circle is on the primitive itself.
pub fn on_primitive(primitive: &Primitive, point: Point) -> bool {
    match *primitive {
        Primitive::Line { start, end } => {
            line_parameter(start, end, point).is_some_and(|t| (-PARAMETER_TOLERANCE..=1.0 + PARAMETER_TOLERANCE).contains(&t))
        }
        Primitive::Arc { center, start_angle, end_angle, .. } => on_arc(center, start_angle, end_angle, point),
        Primitive::Circle { .. } => true,
    }
}

pub fn distance_to(primitive: &Primitive, point: Point) -> f64 {
    match *primitive {
        Primitive::Line { start, end } => match line_parameter(start, end, point) {
            None => start.dist(point),
            Some(t) => lerp(start, end, t.clamp(0.0, 1.0)).dist(point),
        },
        Primitive::Circle { center, radius } => (center.dist(point) - radius).abs(),
        Primitive::Arc { center, radius, start_angle, end_angle } => {
            if on_arc(center, start_angle, end_angle, point) {
                (center.dist(point) - radius).abs()
            } else {
                let first = arc_point(center, radius, start_angle);
                let last = arc_point(center, radius, end_angle);
                first.dist(point).min(last.dist(point))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::entities::Vertex;
    use crate::model::entities::{arc, polyline};
    use crate::model::geometry::pt;

    #[test]
    fn distance_to_an_arc_beyond_its_ends_is_to_the_nearer_end() {
        let quarter = primitives(&arc(pt(0.0, 0.0), 1.0, 0.0, 90.0))[0];
        assert!((distance_to(&quarter, pt(2.0, 0.0)) - 1.0).abs() < 1e-12);
        assert!((distance_to(&quarter, pt(0.0, -1.0)) - 2f64.sqrt()).abs() < 1e-12);
    }

    #[test]
    fn a_bulged_polyline_segment_is_an_arc() {
        let shape = polyline(
            vec![
                Vertex { x: 0.0, y: 0.0, bulge: 1.0, start_width: 0.0, end_width: 0.0 },
                Vertex { x: 2.0, y: 0.0, bulge: 0.0, start_width: 0.0, end_width: 0.0 },
            ],
            false,
        );
        match primitives(&shape)[0] {
            Primitive::Arc { center, radius, .. } => {
                assert!(center.dist(pt(1.0, 0.0)) < 1e-9);
                assert!((radius - 1.0).abs() < 1e-9);
            }
            other => panic!("expected an arc, got {other:?}"),
        }
    }
}
