use super::entities::{Entity, Shape, arc};
use super::geometry::{Point, modulo};
use crate::model::intersect::line_line_meeting;

const EPSILON: f64 = 1e-9;

pub struct FilletResult {
    pub first: Entity, // the first line with its new values, same object
    pub second: Entity,
    pub arc: Option<Entity>, // None when the radius is zero: the lines just meet at the corner
}

fn ends(line: &Entity) -> (Point, Point) {
    match line.shape {
        Shape::Line { start, end } => (start, end),
        _ => panic!("fillet works on lines"),
    }
}

/// The unit direction from the corner along the line, toward the side that was picked.
fn toward_pick(line: (Point, Point), pick: Point, corner: Point) -> Point {
    let (start, end) = line;
    let unit = (end - start) * (1.0 / start.dist(end));
    let mut side = (pick - corner).dot(unit);
    if side.abs() < EPSILON {
        let reaches = [(start - corner).dot(unit), (end - corner).dot(unit)];
        side = if reaches[0].abs() >= reaches[1].abs() { reaches[0] } else { reaches[1] };
    }
    unit * if side >= 0.0 { 1.0 } else { -1.0 }
}

/// The line cut back to the tangent point, keeping the end that lies in the picked direction.
fn keep_side(line: &Entity, corner: Point, toward: Point, tangent: Point, name: &str) -> Result<Entity, String> {
    let (start, end) = ends(line);
    let reach = [(start - corner).dot(toward), (end - corner).dot(toward)];
    let far_is_end = reach[0] < reach[1];
    if reach[if far_is_end { 1 } else { 0 }] <= (tangent - corner).dot(toward) + EPSILON {
        return Err(format!("The {name} line is too short for that radius."));
    }
    let shape = if far_is_end { Shape::Line { start: tangent, end } } else { Shape::Line { start, end: tangent } };
    Ok(line.with_shape(shape))
}

/// Join two lines with a corner or, for a radius above zero, a rounded corner. Both are cut back to fit.
/// The error is a message for the user.
pub fn fillet_lines(first: &Entity, first_pick: Point, second: &Entity, second_pick: Point, radius: f64) -> Result<FilletResult, String> {
    let (a, b) = ends(first);
    let (c, d) = ends(second);
    if a == b || c == d {
        return Err("A line has no length.".into());
    }
    let corner = line_line_meeting(a, b, c, d).ok_or("The lines are parallel.")?;
    let toward_first = toward_pick((a, b), first_pick, corner);
    let toward_second = toward_pick((c, d), second_pick, corner);
    let angle = toward_first.dot(toward_second).clamp(-1.0, 1.0).acos();
    if angle < EPSILON || angle > std::f64::consts::PI - EPSILON {
        return Err("The lines are parallel.".into());
    }
    let tangent_length = radius / (angle / 2.0).tan();
    let tangent_first = corner + toward_first * tangent_length;
    let tangent_second = corner + toward_second * tangent_length;
    let new_first = keep_side(first, corner, toward_first, tangent_first, "first")?;
    let new_second = keep_side(second, corner, toward_second, tangent_second, "second")?;
    if radius == 0.0 {
        return Ok(FilletResult { first: new_first, second: new_second, arc: None });
    }
    let bisector = toward_first + toward_second;
    let center = corner + bisector * (radius / (angle / 2.0).sin() / bisector.length());
    let mut rounded = arc_between(center, radius, tangent_first, tangent_second);
    rounded.layer = first.layer.clone();
    rounded.color = first.color;
    Ok(FilletResult { first: new_first, second: new_second, arc: Some(rounded) })
}

/// Join two lines with a bevel: each is cut back from their corner by its distance, and a line closes the gap.
/// With both distances zero they just meet at the corner. Returns the two lines' new states and the bevel line.
pub fn chamfer_lines(
    first: &Entity,
    first_pick: Point,
    second: &Entity,
    second_pick: Point,
    distances: (f64, f64),
) -> Result<(Entity, Entity, Option<Entity>), String> {
    let (a, b) = ends(first);
    let (c, d) = ends(second);
    if a == b || c == d {
        return Err("A line has no length.".into());
    }
    let corner = line_line_meeting(a, b, c, d).ok_or("The lines are parallel.")?;
    let toward_first = toward_pick((a, b), first_pick, corner);
    let toward_second = toward_pick((c, d), second_pick, corner);
    let cut_first = corner + toward_first * distances.0;
    let cut_second = corner + toward_second * distances.1;
    let new_first = keep_side(first, corner, toward_first, cut_first, "first")?;
    let new_second = keep_side(second, corner, toward_second, cut_second, "second")?;
    let bevel = (cut_first != cut_second).then(|| first.derive(Shape::Line { start: cut_first, end: cut_second }));
    Ok((new_first, new_second, bevel))
}

/// The short arc from one tangent point to the other, as a counter-clockwise arc.
fn arc_between(center: Point, radius: f64, tangent_first: Point, tangent_second: Point) -> Entity {
    let (a, b) = (tangent_first - center, tangent_second - center);
    let first_angle = modulo(a.y.atan2(a.x).to_degrees(), 360.0);
    let second_angle = modulo(b.y.atan2(b.x).to_degrees(), 360.0);
    if a.cross(b) > 0.0 { arc(center, radius, first_angle, second_angle) } else { arc(center, radius, second_angle, first_angle) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::entities::line;
    use crate::model::geometry::pt;

    #[test]
    fn rounded_corner_between_crossing_lines() {
        let horizontal = line(pt(-10.0, 0.0), pt(10.0, 0.0));
        let vertical = line(pt(0.0, -10.0), pt(0.0, 10.0));
        let result = fillet_lines(&horizontal, pt(5.0, 0.0), &vertical, pt(0.0, 5.0), 2.0).unwrap();
        assert_eq!(result.first.id, horizontal.id);
        let Shape::Line { start, end } = result.first.shape else { panic!() };
        assert!(start.dist(pt(2.0, 0.0)) < 1e-9 && end == pt(10.0, 0.0));
        let Shape::Line { start, end } = result.second.shape else { panic!() };
        assert!(start.dist(pt(0.0, 2.0)) < 1e-9 && end == pt(0.0, 10.0));
        let Shape::Arc { center, radius, start_angle, end_angle } = result.arc.unwrap().shape else { panic!() };
        assert!(center.dist(pt(2.0, 2.0)) < 1e-9 && (radius - 2.0).abs() < 1e-12);
        assert!((start_angle - 180.0).abs() < 1e-9 && (end_angle - 270.0).abs() < 1e-9);
    }

    #[test]
    fn chamfer_bevels_the_corner() {
        let a = line(pt(0.0, 0.0), pt(10.0, 0.0));
        let b = line(pt(12.0, -2.0), pt(12.0, 10.0));
        let (first, second, bevel) = chamfer_lines(&a, pt(1.0, 0.0), &b, pt(12.0, 8.0), (2.0, 3.0)).unwrap();
        assert_eq!(first.shape, Shape::Line { start: pt(0.0, 0.0), end: pt(10.0, 0.0) });
        assert_eq!(second.shape, Shape::Line { start: pt(12.0, 3.0), end: pt(12.0, 10.0) });
        assert_eq!(bevel.unwrap().shape, Shape::Line { start: pt(10.0, 0.0), end: pt(12.0, 3.0) });
    }

    #[test]
    fn zero_radius_just_meets_at_the_corner() {
        let a = line(pt(0.0, 0.0), pt(5.0, 0.0));
        let b = line(pt(7.0, 1.0), pt(7.0, 5.0));
        let result = fillet_lines(&a, pt(1.0, 0.0), &b, pt(7.0, 4.0), 0.0).unwrap();
        assert_eq!(result.first.shape, Shape::Line { start: pt(0.0, 0.0), end: pt(7.0, 0.0) });
        assert_eq!(result.second.shape, Shape::Line { start: pt(7.0, 0.0), end: pt(7.0, 5.0) });
        assert!(result.arc.is_none());
    }

    #[test]
    fn parallel_lines_and_short_lines_are_refused() {
        let a = line(pt(0.0, 0.0), pt(5.0, 0.0));
        let b = line(pt(0.0, 1.0), pt(5.0, 1.0));
        assert_eq!(fillet_lines(&a, pt(1.0, 0.0), &b, pt(1.0, 1.0), 1.0).err().unwrap(), "The lines are parallel.");
        let c = line(pt(0.0, 0.5), pt(0.0, 1.0));
        let message = fillet_lines(&a, pt(1.0, 0.0), &c, pt(0.0, 0.8), 3.0).err().unwrap();
        assert!(message.contains("too short"), "{message}");
    }
}
