use std::f64::consts::PI;

use super::entities::{Entity, Shape, Vertex};
use super::geometry::{Point, bulge_arc, modulo};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Measured {
    pub area: f64,
    pub perimeter: f64, // the circumference for a circle
}

pub fn distance_report(first: Point, second: Point) -> String {
    let (dx, dy) = (second.x - first.x, second.y - first.y);
    let angle = modulo(first.angle_to(second), 360.0);
    let length = dx.hypot(dy);
    format!("Distance = {length:.4}, Angle in XY Plane = {angle:.0}, Delta X = {dx:.4}, Delta Y = {dy:.4}")
}

/// Signed area under the segment (counter-clockwise positive, with the bulge's cap) and its length.
fn segment(start: &Vertex, end: &Vertex) -> (f64, f64) {
    let cross = (start.x * end.y - end.x * start.y) / 2.0;
    if start.bulge == 0.0 {
        return (cross, start.point().dist(end.point()));
    }
    let arc = bulge_arc(start.point(), end.point(), start.bulge);
    let cap = arc.radius.powi(2) / 2.0 * (arc.sweep - arc.sweep.sin());
    (cross + cap, arc.radius * arc.sweep.abs())
}

/// The area and perimeter of the shape the vertices enclose when the last joins back to the first.
pub fn polygon(vertices: &[Vertex]) -> Option<Measured> {
    if vertices.len() < 2 {
        return None;
    }
    let (mut area, mut perimeter) = (0.0, 0.0);
    for (index, start) in vertices.iter().enumerate() {
        let (segment_area, length) = segment(start, &vertices[(index + 1) % vertices.len()]);
        area += segment_area;
        perimeter += length;
    }
    Some(Measured { area: area.abs(), perimeter })
}

/// Area and perimeter of a circle or closed polyline, or None for anything else.
pub fn measure_entity(entity: &Entity) -> Option<Measured> {
    match &entity.shape {
        Shape::Circle { radius, .. } => Some(Measured { area: PI * radius.powi(2), perimeter: 2.0 * PI * radius }),
        Shape::Polyline { vertices, closed: true } => polygon(vertices),
        _ => None,
    }
}

pub fn area_report(measured: Measured) -> String {
    format!("Area = {:.4}, Perimeter = {:.4}", measured.area, measured.perimeter)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::entities::{circle, polyline_from, vertex};
    use crate::model::geometry::pt;

    #[test]
    fn distance_report_reads_like_autocad() {
        assert_eq!(
            distance_report(pt(0.0, 0.0), pt(3.0, 4.0)),
            "Distance = 5.0000, Angle in XY Plane = 53, Delta X = 3.0000, Delta Y = 4.0000"
        );
    }

    #[test]
    fn square_and_circle() {
        let square = polyline_from(&[pt(0.0, 0.0), pt(2.0, 0.0), pt(2.0, 2.0), pt(0.0, 2.0)], true);
        assert_eq!(measure_entity(&square), Some(Measured { area: 4.0, perimeter: 8.0 }));
        let round = measure_entity(&circle(pt(0.0, 0.0), 1.0)).unwrap();
        assert!((round.area - PI).abs() < 1e-12);
    }

    #[test]
    fn bulges_add_their_cap() {
        // A half disc of radius 1: a straight edge and a half circle.
        let half = [
            Vertex { x: -1.0, y: 0.0, bulge: 0.0, start_width: 0.0, end_width: 0.0 },
            Vertex { x: 1.0, y: 0.0, bulge: 1.0, start_width: 0.0, end_width: 0.0 },
        ];
        let measured = polygon(&half).unwrap();
        assert!((measured.area - PI / 2.0).abs() < 1e-9);
        assert!((measured.perimeter - (2.0 + PI)).abs() < 1e-9);
        assert!(polygon(&[vertex(pt(0.0, 0.0))]).is_none());
    }
}
