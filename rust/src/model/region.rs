//! Regions: closed areas made from closed objects (REGION), and combined by the Boolean commands UNION, SUBTRACT
//! and INTERSECT. A region is a set of loops, each a closed run of straight and arc segments; a point is inside
//! when an odd number of loops surround it, so a loop inside another is a hole.

use i_overlay::core::fill_rule::FillRule;
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::float::single::SingleFloatOverlay;

use super::boundary::fit_arcs;
use super::editing::closed_loop;
use super::entities::{Entity, Shape, Vertex, vertex};
use super::geometry::{Point, bulge_arc, pt};
use super::measure::{Measured, polygon};

/// What a Boolean command does with the regions it is given.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Boolean {
    Union,
    Subtract,
    Intersect,
}

/// Arcs are cut into chords this many degrees apart for the Boolean, few enough that the result's arcs are found
/// again (see `fit_arcs`, which takes chords turning up to 20 degrees as an arc).
const CHORD_DEGREES: f64 = 6.0;

/// The closed loop an object makes, as vertices with bulges: a closed polyline as it is, a circle as two half
/// circles, and other closed objects (ellipses, splines, solids) as their outline's points. None for an open object.
pub fn loop_of(entity: &Entity) -> Option<Vec<Vertex>> {
    match &entity.shape {
        Shape::Polyline { vertices, closed } if vertices.len() >= 2 => {
            let mut vertices: Vec<Vertex> = vertices.iter().map(|v| Vertex { start_width: 0.0, end_width: 0.0, ..*v }).collect();
            let ends_meet = vertices.first().map(|v| v.point()) == vertices.last().map(|v| v.point());
            if !*closed && !ends_meet {
                return None;
            }
            if ends_meet && vertices.len() > 2 {
                vertices.pop();
            }
            (vertices.len() >= 2).then_some(vertices)
        }
        Shape::Circle { center, radius } if *radius > 0.0 => {
            let (left, right) = (*center - pt(*radius, 0.0), *center + pt(*radius, 0.0));
            Some(vec![Vertex { bulge: 1.0, ..vertex(right) }, Vertex { bulge: 1.0, ..vertex(left) }])
        }
        Shape::Region { .. } => None,
        _ => closed_loop(entity).map(|points| points.into_iter().map(vertex).collect()),
    }
}

/// A loop's points, its arcs cut into short chords; the last point is not the first again.
fn flattened(vertices: &[Vertex]) -> Vec<Point> {
    let mut points = vec![];
    for (index, here) in vertices.iter().enumerate() {
        let next = vertices[(index + 1) % vertices.len()];
        points.push(here.point());
        if here.bulge != 0.0 && here.point() != next.point() {
            let arc = bulge_arc(here.point(), next.point(), here.bulge);
            let steps = ((arc.sweep.abs().to_degrees() / CHORD_DEGREES).ceil() as usize).max(2);
            for step in 1..steps {
                let angle = arc.start_angle + arc.sweep * step as f64 / steps as f64;
                points.push(arc.center + pt(angle.cos(), angle.sin()) * arc.radius);
            }
        }
    }
    points
}

/// Combine two sets of loops. Each set is read as its loops' filled area, holes counted out; the result's loops
/// come back with their arcs found again, outer loops counter-clockwise and holes clockwise.
pub fn combine(first: &[Vec<Vertex>], second: &[Vec<Vertex>], boolean: Boolean) -> Vec<Vec<Vertex>> {
    let contours = |loops: &[Vec<Vertex>]| -> Vec<Vec<[f64; 2]>> {
        loops.iter().map(|l| flattened(l).into_iter().map(|p| [p.x, p.y]).collect()).filter(|c: &Vec<[f64; 2]>| c.len() >= 3).collect()
    };
    let rule = match boolean {
        Boolean::Union => OverlayRule::Union,
        Boolean::Subtract => OverlayRule::Difference,
        Boolean::Intersect => OverlayRule::Intersect,
    };
    let shapes = contours(first).overlay(&contours(second), rule, FillRule::EvenOdd);
    shapes
        .into_iter()
        .flatten()
        .map(|contour| {
            let points: Vec<Point> = contour.into_iter().map(|[x, y]| pt(x, y)).collect();
            fit_arcs(&points, true)
        })
        .filter(|l| l.len() >= 2)
        .collect()
}

/// The loops of several sets combined into one by union, as REGION's loops overlapping become one area.
pub fn union_all(sets: &[Vec<Vec<Vertex>>]) -> Vec<Vec<Vertex>> {
    sets.iter().fold(vec![], |all, set| if all.is_empty() { set.clone() } else { combine(&all, set, Boolean::Union) })
}

/// Whether a point is inside a loop, its arcs followed closely.
fn inside(point: Point, vertices: &[Vertex]) -> bool {
    let ring = flattened(vertices);
    let mut inside = false;
    for (index, a) in ring.iter().enumerate() {
        let b = ring[(index + 1) % ring.len()];
        if (a.y > point.y) != (b.y > point.y) && point.x < a.x + (point.y - a.y) * (b.x - a.x) / (b.y - a.y) {
            inside = !inside;
        }
    }
    inside
}

/// A region's area (holes taken out) and the length of all its loops.
pub fn measure(loops: &[Vec<Vertex>]) -> Measured {
    let (mut area, mut perimeter) = (0.0, 0.0);
    for (index, l) in loops.iter().enumerate() {
        let Some(measured) = polygon(l) else { continue };
        perimeter += measured.perimeter;
        // A loop inside an odd number of others is a hole.
        let depth = loops.iter().enumerate().filter(|(other, o)| *other != index && inside(l[0].point(), o)).count();
        area += if depth % 2 == 0 { measured.area } else { -measured.area };
    }
    Measured { area: area.abs(), perimeter }
}

/// The region's loops as closed polylines, for drawing, snapping and exploding.
pub fn loop_shapes(loops: &[Vec<Vertex>]) -> impl Iterator<Item = Shape> + '_ {
    loops.iter().map(|l| Shape::Polyline { vertices: l.clone(), closed: true })
}

/// How far a full turn's worth of chords strays from a circle of this radius, for tests.
#[cfg(test)]
fn chord_error(radius: f64) -> f64 {
    radius * (1.0 - (CHORD_DEGREES.to_radians() / 2.0).cos())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::entities::{circle, polyline_from};
    use std::f64::consts::PI;

    fn square(x: f64, y: f64, side: f64) -> Vec<Vertex> {
        loop_of(&polyline_from(&[pt(x, y), pt(x + side, y), pt(x + side, y + side), pt(x, y + side)], true)).expect("closed")
    }

    #[test]
    fn closed_objects_make_loops_and_open_ones_do_not() {
        assert_eq!(square(0.0, 0.0, 1.0).len(), 4);
        let round = loop_of(&circle(pt(0.0, 0.0), 2.0)).expect("a circle is closed");
        assert!((measure(&[round]).area - PI * 4.0).abs() < 1e-9, "a circle keeps its true area");
        assert!(loop_of(&crate::model::entities::line(pt(0.0, 0.0), pt(1.0, 0.0))).is_none());
        let open = polyline_from(&[pt(0.0, 0.0), pt(1.0, 0.0), pt(1.0, 1.0)], false);
        assert!(loop_of(&open).is_none());
    }

    #[test]
    fn union_subtract_and_intersect_of_two_squares() {
        let (a, b) = (vec![square(0.0, 0.0, 10.0)], vec![square(5.0, 5.0, 10.0)]);
        let area = |loops: Vec<Vec<Vertex>>| measure(&loops).area;
        assert!((area(combine(&a, &b, Boolean::Union)) - 175.0).abs() < 1e-6);
        assert!((area(combine(&a, &b, Boolean::Subtract)) - 75.0).abs() < 1e-6);
        assert!((area(combine(&a, &b, Boolean::Intersect)) - 25.0).abs() < 1e-6);
        assert!(combine(&a, &[square(20.0, 20.0, 1.0)], Boolean::Intersect).is_empty(), "apart, nothing is common");
    }

    #[test]
    fn a_hole_is_counted_out_and_arcs_come_back_as_arcs() {
        let plate = vec![square(0.0, 0.0, 10.0)];
        let hole = vec![loop_of(&circle(pt(5.0, 5.0), 2.0)).expect("closed")];
        let result = combine(&plate, &hole, Boolean::Subtract);
        assert_eq!(result.len(), 2, "the outline and the hole");
        let expected = 100.0 - PI * 4.0;
        assert!((measure(&result).area - expected).abs() < 1e-6, "{} against {expected}", measure(&result).area);
        let ring = result.iter().find(|l| l.iter().any(|v| v.bulge != 0.0)).expect("the hole is round again");
        assert!(ring.len() <= 4, "a few arcs, not {} chords", ring.len());
        assert!(chord_error(2.0) > 1e-3, "the chords alone would be visibly off");
    }

    #[test]
    fn overlapping_loops_unite() {
        let joined = union_all(&[vec![square(0.0, 0.0, 2.0)], vec![square(1.0, 0.0, 2.0)], vec![square(10.0, 0.0, 1.0)]]);
        assert_eq!(joined.len(), 2, "two areas: the overlapping pair and the one apart");
        assert!((measure(&joined).area - 7.0).abs() < 1e-6);
    }
}
