use super::entities::{Entity, Shape, Vertex, arc};
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

/// What filleting a whole polyline did: the polyline with its corners rounded, how many corners were rounded and
/// how many had segments too short for the radius.
pub struct PolylineFillet {
    pub polyline: Entity,
    pub filleted: usize,
    pub too_short: usize,
}

/// A corner of a polyline that can be rounded: the vertex where two straight segments meet at an angle, with the
/// length cut from each segment for the radius and the bulge of the arc that replaces the corner.
struct Corner {
    index: usize,
    tangent_length: f64,
    bulge: f64,
}

/// The polyline's segment from vertex `index` to the next one, if it has one.
fn segment_end(vertices: &[Vertex], closed: bool, index: usize) -> Option<usize> {
    if index + 1 < vertices.len() { Some(index + 1) } else { (closed && vertices.len() > 2).then_some(0) }
}

/// The corner at vertex `index` for this radius, if two straight segments that are not in line meet there.
fn corner_at(vertices: &[Vertex], closed: bool, index: usize, radius: f64) -> Option<Corner> {
    let count = vertices.len();
    let previous = if index > 0 {
        index - 1
    } else if closed && count > 2 {
        count - 1
    } else {
        return None;
    };
    let next = segment_end(vertices, closed, index)?;
    if vertices[previous].bulge != 0.0 || vertices[index].bulge != 0.0 {
        return None;
    }
    let (before, at, after) = (vertices[previous].point(), vertices[index].point(), vertices[next].point());
    let (incoming, outgoing) = (at - before, after - at);
    if incoming.length() < EPSILON || outgoing.length() < EPSILON {
        return None;
    }
    let sweep = incoming.cross(outgoing).atan2(incoming.dot(outgoing));
    if sweep.abs() < EPSILON || sweep.abs() > std::f64::consts::PI - EPSILON {
        return None;
    }
    Some(Corner { index, tangent_length: radius * (sweep.abs() / 2.0).tan(), bulge: (sweep / 4.0).tan() })
}

/// Rounds the given corners, each with an arc of the radius they were found for. Each corner becomes two vertices,
/// the tangent points, with the arc between them; the arc segment takes the corner's starting width.
fn round_corners(polyline: &Entity, vertices: &[Vertex], closed: bool, corners: &[Corner]) -> Entity {
    let mut rounded = Vec::with_capacity(vertices.len() + corners.len());
    for (index, vertex) in vertices.iter().enumerate() {
        let Some(corner) = corners.iter().find(|c| c.index == index) else {
            rounded.push(*vertex);
            continue;
        };
        let previous = if index > 0 { index - 1 } else { vertices.len() - 1 };
        let next = segment_end(vertices, closed, index).unwrap();
        let at = vertex.point();
        let back = (vertices[previous].point() - at) * (1.0 / vertices[previous].point().dist(at));
        let ahead = (vertices[next].point() - at) * (1.0 / vertices[next].point().dist(at));
        let (first, second) = (at + back * corner.tangent_length, at + ahead * corner.tangent_length);
        let width = vertex.start_width;
        rounded.push(Vertex { x: first.x, y: first.y, bulge: corner.bulge, start_width: width, end_width: width });
        rounded.push(Vertex { x: second.x, y: second.y, ..*vertex });
    }
    polyline.with_shape(Shape::Polyline { vertices: rounded, closed })
}

/// The polyline's vertices, or a message that the object is not a polyline.
fn polyline_vertices(polyline: &Entity) -> Result<(&[Vertex], bool), String> {
    match &polyline.shape {
        Shape::Polyline { vertices, closed } => Ok((vertices, *closed)),
        _ => Err("The object is not a polyline.".into()),
    }
}

/// Rounds every corner of a polyline where two straight segments meet, as FILLET's Polyline option does. A corner
/// is left as it is when the segments are too short for the radius, counting the arcs already put at their other
/// ends.
pub fn fillet_polyline(polyline: &Entity, radius: f64) -> Result<PolylineFillet, String> {
    let (vertices, closed) = polyline_vertices(polyline)?;
    let lengths: Vec<f64> = (0..vertices.len())
        .map(|i| segment_end(vertices, closed, i).map_or(0.0, |next| vertices[i].point().dist(vertices[next].point())))
        .collect();
    // How much of each segment's start and end the arcs rounded so far have taken.
    let (mut used_start, mut used_end) = (vec![0.0; vertices.len()], vec![0.0; vertices.len()]);
    let (mut corners, mut too_short) = (Vec::new(), 0);
    for index in 0..vertices.len() {
        let Some(corner) = corner_at(vertices, closed, index, radius) else { continue };
        let previous = if index > 0 { index - 1 } else { vertices.len() - 1 };
        let fits = corner.tangent_length + used_start[previous] <= lengths[previous] + EPSILON
            && corner.tangent_length + used_end[index] <= lengths[index] + EPSILON;
        if !fits {
            too_short += 1;
            continue;
        }
        used_end[previous] = corner.tangent_length;
        used_start[index] = corner.tangent_length;
        corners.push(corner);
    }
    let filleted = if radius > 0.0 { corners.len() } else { 0 };
    let polyline = if filleted > 0 { round_corners(polyline, vertices, closed, &corners) } else { polyline.clone() };
    Ok(PolylineFillet { polyline, filleted, too_short })
}

/// The index of the polyline's segment nearest a point, and whether it is straight.
fn nearest_segment(vertices: &[Vertex], closed: bool, point: Point) -> Option<(usize, bool)> {
    let distance = |a: Point, b: Point| {
        let along = b - a;
        let t = if along.length() < EPSILON { 0.0 } else { ((point - a).dot(along) / along.dot(along)).clamp(0.0, 1.0) };
        point.dist(a + along * t)
    };
    (0..vertices.len())
        .filter_map(|i| segment_end(vertices, closed, i).map(|next| (i, distance(vertices[i].point(), vertices[next].point()))))
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| (i, vertices[i].bulge == 0.0))
}

/// Rounds the corner between two neighbouring straight segments of a polyline, each picked at a point.
pub fn fillet_polyline_corner(polyline: &Entity, first_pick: Point, second_pick: Point, radius: f64) -> Result<Entity, String> {
    let (vertices, closed) = polyline_vertices(polyline)?;
    let (first, first_straight) = nearest_segment(vertices, closed, first_pick).ok_or("The polyline has no segments.")?;
    let (second, second_straight) = nearest_segment(vertices, closed, second_pick).ok_or("The polyline has no segments.")?;
    if !first_straight || !second_straight {
        return Err("Cannot fillet an arc segment of a polyline.".into());
    }
    let corner = if segment_end(vertices, closed, first) == Some(second) {
        second
    } else if segment_end(vertices, closed, second) == Some(first) {
        first
    } else {
        return Err("Select two segments of the polyline that meet.".into());
    };
    let corner = corner_at(vertices, closed, corner, radius).ok_or("The segments are in line.")?;
    let previous = if corner.index > 0 { corner.index - 1 } else { vertices.len() - 1 };
    let next = segment_end(vertices, closed, corner.index).unwrap();
    let at = vertices[corner.index].point();
    if corner.tangent_length > at.dist(vertices[previous].point()) + EPSILON
        || corner.tangent_length > at.dist(vertices[next].point()) + EPSILON
    {
        return Err("The segments are too short for that radius.".into());
    }
    if radius == 0.0 {
        return Ok(polyline.clone());
    }
    Ok(round_corners(polyline, vertices, closed, &[corner]))
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
    use crate::model::entities::{line, polyline_from};
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

    #[test]
    fn polyline_corners_too_short_for_the_radius_are_left_and_counted() {
        // A 10 by 4 rectangle: a radius of 3 needs 6 along each short side, so only two corners on one side fit.
        let rectangle = polyline_from(&[pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 4.0), pt(0.0, 4.0)], true);
        let result = fillet_polyline(&rectangle, 3.0).unwrap();
        assert_eq!((result.filleted, result.too_short), (2, 2));
        let result = fillet_polyline(&rectangle, 2.0).unwrap();
        assert_eq!((result.filleted, result.too_short), (4, 0));
    }

    #[test]
    fn an_open_polyline_rounds_only_its_inner_corners_and_turns_the_right_way() {
        // A clockwise bend: the arc's bulge is negative.
        let bend = polyline_from(&[pt(0.0, 0.0), pt(0.0, 10.0), pt(10.0, 10.0)], false);
        let result = fillet_polyline(&bend, 1.0).unwrap();
        assert_eq!(result.filleted, 1);
        let Shape::Polyline { vertices, closed: false } = result.polyline.shape else { panic!() };
        assert_eq!(vertices.len(), 4);
        assert!(vertices[1].point().dist(pt(0.0, 9.0)) < 1e-9 && vertices[2].point().dist(pt(1.0, 10.0)) < 1e-9);
        assert!(vertices[1].bulge < 0.0);
    }

    #[test]
    fn polyline_segments_that_do_not_meet_are_refused() {
        let rectangle = polyline_from(&[pt(0.0, 0.0), pt(20.0, 0.0), pt(20.0, 10.0), pt(0.0, 10.0)], true);
        let message = fillet_polyline_corner(&rectangle, pt(10.0, 0.0), pt(10.0, 10.0), 1.0).err().unwrap();
        assert!(message.contains("meet"), "{message}");
    }
}
