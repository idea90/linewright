use std::f64::consts::PI;

use super::entities::{Entity, Shape, Vertex};
use super::geometry::{Point, bulge_arc, modulo};
use super::intersect::line_line_meeting;

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

/// The radius of an arc or circle, as MEASUREGEOM's Radius option finds it.
pub fn radius_of(shape: &Shape) -> Option<f64> {
    match shape {
        Shape::Circle { radius, .. } | Shape::Arc { radius, .. } => Some(*radius),
        _ => None,
    }
}

pub fn radius_report(radius: f64) -> String {
    format!("Radius = {radius:.4}\nDiameter = {:.4}", radius * 2.0)
}

/// The angle in degrees, 0 to 180, between the directions from a vertex to two points.
pub fn angle_at(vertex: Point, first: Point, second: Point) -> f64 {
    let (a, b) = (first - vertex, second - vertex);
    a.cross(b).atan2(a.dot(b)).abs().to_degrees()
}

/// The angle between two lines, each given by its ends and the point it was picked at: measured from where the
/// lines meet, on the sides that were picked, as AutoCAD does. Parallel lines are 0°.
pub fn angle_between_lines(first: (Point, Point), first_pick: Point, second: (Point, Point), second_pick: Point) -> f64 {
    let Some(meet) = line_line_meeting(first.0, first.1, second.0, second.1) else { return 0.0 };
    let picked_side = |(start, end): (Point, Point), pick: Point| {
        let along = end - start;
        let side = (pick - meet).dot(along);
        // Picked right where the lines meet: the side the line lies on.
        let side = if side.abs() > 1e-12 { side } else { (start + along * 0.5 - meet).dot(along) };
        if side < 0.0 { meet - along } else { meet + along }
    };
    angle_at(meet, picked_side(first, first_pick), picked_side(second, second_pick))
}

/// The angle an arc sweeps through, in degrees.
pub fn included_angle(start_angle: f64, end_angle: f64) -> f64 {
    let sweep = modulo(end_angle - start_angle, 360.0);
    if sweep == 0.0 { 360.0 } else { sweep }
}

pub fn angle_report(degrees: f64) -> String {
    format!("Angle = {degrees:.0}°")
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
    fn angles_between_lines_arcs_and_points() {
        let horizontal = (pt(0.0, 0.0), pt(10.0, 0.0));
        let diagonal = (pt(0.0, 0.0), pt(10.0, 10.0));
        assert!((angle_between_lines(horizontal, pt(5.0, 0.0), diagonal, pt(5.0, 5.0)) - 45.0).abs() < 1e-9);
        // Lines that do not touch are measured where they would meet, on the picked sides.
        let left = (pt(-10.0, 0.0), pt(-1.0, 0.0));
        let up = (pt(0.0, 2.0), pt(0.0, 9.0));
        assert!((angle_between_lines(left, pt(-5.0, 0.0), up, pt(0.0, 5.0)) - 90.0).abs() < 1e-9);
        let reversed = (pt(10.0, 0.0), pt(0.0, 0.0));
        assert!(
            (angle_between_lines(reversed, pt(5.0, 0.0), diagonal, pt(5.0, 5.0)) - 45.0).abs() < 1e-9,
            "the ends' order does not matter"
        );
        let through = (pt(-10.0, 0.0), pt(10.0, 0.0));
        assert!((angle_between_lines(through, pt(-5.0, 0.0), diagonal, pt(5.0, 5.0)) - 135.0).abs() < 1e-9, "the picked side counts");
        assert_eq!(angle_between_lines(horizontal, pt(1.0, 0.0), (pt(0.0, 1.0), pt(5.0, 1.0)), pt(1.0, 1.0)), 0.0, "parallel");
        assert!((angle_at(pt(0.0, 0.0), pt(1.0, 0.0), pt(-1.0, 1.0)) - 135.0).abs() < 1e-9);
        assert!((angle_at(pt(0.0, 0.0), pt(1.0, 0.0), pt(0.0, -1.0)) - 90.0).abs() < 1e-9, "the smaller side");
        assert_eq!(included_angle(350.0, 20.0), 30.0);
        assert_eq!(angle_report(45.0), "Angle = 45°");
        assert_eq!(radius_report(2.5), "Radius = 2.5000\nDiameter = 5.0000");
        assert_eq!(radius_of(&circle(pt(0.0, 0.0), 3.0).shape), Some(3.0));
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
