use super::intersect::intersections;
use super::primitives::{Primitive, arc_point, distance_to, lerp, line_parameter, on_primitive, primitives};
use crate::model::entities::{Entity, Shape};
use crate::model::geometry::{Point, sweep_degrees};

const MAX_INTERSECTION_PRIMITIVES: usize = 40;

/// The kinds of object snap. Earlier kinds win when two snap points are equally close to the cursor.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SnapKind {
    Endpoint,
    Intersection,
    Center,
    Quadrant,
    Node,
    Insertion,
    Midpoint,
    Perpendicular,
    Nearest,
}

impl SnapKind {
    pub const ALL: [SnapKind; 9] = [
        SnapKind::Endpoint,
        SnapKind::Midpoint,
        SnapKind::Center,
        SnapKind::Node,
        SnapKind::Quadrant,
        SnapKind::Intersection,
        SnapKind::Insertion,
        SnapKind::Perpendicular,
        SnapKind::Nearest,
    ];

    pub fn name(self) -> &'static str {
        match self {
            SnapKind::Endpoint => "Endpoint",
            SnapKind::Midpoint => "Midpoint",
            SnapKind::Center => "Center",
            SnapKind::Node => "Node",
            SnapKind::Quadrant => "Quadrant",
            SnapKind::Intersection => "Intersection",
            SnapKind::Insertion => "Insertion",
            SnapKind::Perpendicular => "Perpendicular",
            SnapKind::Nearest => "Nearest",
        }
    }

    /// The snap a typed override names, as AutoCAD takes them at a point prompt: END, MID, CEN and the like.
    pub fn from_override(word: &str) -> Option<SnapKind> {
        Some(match word.trim().to_uppercase().as_str() {
            "END" | "ENDP" | "ENDPOINT" => SnapKind::Endpoint,
            "MID" | "MIDPOINT" => SnapKind::Midpoint,
            "CEN" | "CENTER" => SnapKind::Center,
            "NOD" | "NODE" => SnapKind::Node,
            "QUA" | "QUAD" | "QUADRANT" => SnapKind::Quadrant,
            "INT" | "INTERSECTION" => SnapKind::Intersection,
            "INS" | "INSERT" | "INSERTION" => SnapKind::Insertion,
            "PER" | "PERP" | "PERPENDICULAR" => SnapKind::Perpendicular,
            "NEA" | "NEAR" | "NEAREST" => SnapKind::Nearest,
            _ => return None,
        })
    }

    /// The snaps AutoCAD has on out of the box.
    pub fn defaults() -> Vec<SnapKind> {
        vec![SnapKind::Endpoint, SnapKind::Midpoint, SnapKind::Center, SnapKind::Node, SnapKind::Intersection, SnapKind::Perpendicular]
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SnapPoint {
    pub point: Point,
    pub kind: SnapKind,
}

/// The object snap point nearest the cursor within the aperture, or None, of the kinds turned on.
///
/// A perpendicular snap needs a base point, and it and nearest are only used when nothing else is in range.
pub fn find_osnap(cursor: Point, aperture: f64, entities: &[Entity], base: Option<Point>, on: &[SnapKind]) -> Option<SnapPoint> {
    let every: Vec<Primitive> = entities.iter().flat_map(primitives).collect();
    let near: Vec<Primitive> = every.iter().copied().filter(|p| distance_to(p, cursor) <= aperture).collect();
    let mut found = centers(&every);
    found.extend(quadrants(&near));
    found.extend(ends_and_middles(&near));
    found.extend(crossings(&near));
    found.extend(nodes_and_insertions(entities));
    found.retain(|s| on.contains(&s.kind));
    if let Some(best) = nearest(&found, cursor, aperture) {
        return Some(best);
    }
    let foot = base.filter(|_| on.contains(&SnapKind::Perpendicular)).and_then(|base| nearest_foot(&near, cursor, base));
    foot.or_else(|| on.contains(&SnapKind::Nearest).then(|| nearest_on(&near, cursor)).flatten())
}

/// The four points of a circle at 0, 90, 180 and 270 degrees, or those an arc reaches.
fn quadrants(near: &[Primitive]) -> Vec<SnapPoint> {
    let mut found = vec![];
    for primitive in near {
        let Some((center, radius)) = primitive.round() else { continue };
        for angle in [0.0, 90.0, 180.0, 270.0] {
            let point = arc_point(center, radius, angle);
            if on_primitive(primitive, point) {
                found.push(SnapPoint { point, kind: SnapKind::Quadrant });
            }
        }
    }
    found
}

/// Point objects (nodes), and where text and blocks are placed (insertion points).
fn nodes_and_insertions(entities: &[Entity]) -> Vec<SnapPoint> {
    entities
        .iter()
        .filter_map(|e| match e.shape {
            Shape::Point { position } => Some(SnapPoint { point: position, kind: SnapKind::Node }),
            Shape::Text { position, .. } | Shape::Insert { position, .. } => Some(SnapPoint { point: position, kind: SnapKind::Insertion }),
            _ => None,
        })
        .collect()
}

/// The point on the objects nearest the cursor.
fn nearest_on(near: &[Primitive], cursor: Point) -> Option<SnapPoint> {
    near.iter()
        .filter_map(|p| closest_point(p, cursor))
        .min_by(|a, b| a.dist(cursor).total_cmp(&b.dist(cursor)))
        .map(|point| SnapPoint { point, kind: SnapKind::Nearest })
}

fn closest_point(primitive: &Primitive, cursor: Point) -> Option<Point> {
    match *primitive {
        Primitive::Line { start, end } => {
            let t = line_parameter(start, end, cursor)?.clamp(0.0, 1.0);
            Some(lerp(start, end, t))
        }
        _ => {
            let (center, radius) = primitive.round()?;
            let reach = center.dist(cursor);
            (reach > 0.0).then(|| center + (cursor - center) * (radius / reach)).filter(|p| on_primitive(primitive, *p))
        }
    }
}

fn centers(every: &[Primitive]) -> Vec<SnapPoint> {
    every.iter().filter_map(|p| p.round()).map(|(center, _)| SnapPoint { point: center, kind: SnapKind::Center }).collect()
}

fn ends_and_middles(near: &[Primitive]) -> Vec<SnapPoint> {
    let mut found = vec![];
    for primitive in near {
        let (first, last, middle) = match *primitive {
            // A point object is a line of no length: it snaps as a node, not as ends.
            Primitive::Line { start, end } if start == end => continue,
            Primitive::Line { start, end } => (start, end, start.midpoint(end)),
            Primitive::Arc { center, radius, start_angle, end_angle } => (
                arc_point(center, radius, start_angle),
                arc_point(center, radius, end_angle),
                arc_point(center, radius, start_angle + sweep_degrees(start_angle, end_angle) / 2.0),
            ),
            Primitive::Circle { .. } => continue,
        };
        found.push(SnapPoint { point: first, kind: SnapKind::Endpoint });
        found.push(SnapPoint { point: last, kind: SnapKind::Endpoint });
        found.push(SnapPoint { point: middle, kind: SnapKind::Midpoint });
    }
    found
}

fn crossings(near: &[Primitive]) -> Vec<SnapPoint> {
    let candidates = &near[..near.len().min(MAX_INTERSECTION_PRIMITIVES)];
    let mut found = vec![];
    for (index, first) in candidates.iter().enumerate() {
        for second in &candidates[index + 1..] {
            found.extend(intersections(first, second).into_iter().map(|point| SnapPoint { point, kind: SnapKind::Intersection }));
        }
    }
    found
}

fn nearest(found: &[SnapPoint], cursor: Point, aperture: f64) -> Option<SnapPoint> {
    found
        .iter()
        .filter(|s| s.point.dist(cursor) <= aperture)
        .min_by(|a, b| a.point.dist(cursor).total_cmp(&b.point.dist(cursor)).then(a.kind.cmp(&b.kind)))
        .copied()
}

fn nearest_foot(near: &[Primitive], cursor: Point, base: Point) -> Option<SnapPoint> {
    near.iter()
        .flat_map(|p| perpendicular_feet(p, base))
        .map(|point| SnapPoint { point, kind: SnapKind::Perpendicular })
        .min_by(|a, b| a.point.dist(cursor).total_cmp(&b.point.dist(cursor)))
}

/// Points on the primitive where a line from base meets it at a right angle.
fn perpendicular_feet(primitive: &Primitive, base: Point) -> Vec<Point> {
    match *primitive {
        Primitive::Line { start, end } => match line_parameter(start, end, base) {
            None => vec![],
            Some(t) => {
                let foot = lerp(start, end, t);
                if on_primitive(primitive, foot) { vec![foot] } else { vec![] }
            }
        },
        _ => {
            let (center, radius) = primitive.round().expect("round");
            let reach = center.dist(base);
            if reach == 0.0 {
                return vec![];
            }
            let unit = (base - center) * (1.0 / reach);
            [center + unit * radius, center - unit * radius].into_iter().filter(|p| on_primitive(primitive, *p)).collect()
        }
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn typed_snap_overrides_name_their_kind() {
        assert_eq!(SnapKind::from_override("end"), Some(SnapKind::Endpoint));
        assert_eq!(SnapKind::from_override(" PER "), Some(SnapKind::Perpendicular));
        assert_eq!(SnapKind::from_override("10,5"), None);
    }

    use super::*;
    use crate::model::entities::{circle, line};
    use crate::model::geometry::pt;

    #[test]
    fn endpoint_beats_a_farther_midpoint() {
        let entities = [line(pt(0.0, 0.0), pt(10.0, 0.0))];
        let hit = find_osnap(pt(0.5, 0.2), 1.0, &entities, None, &SnapKind::defaults()).unwrap();
        assert_eq!(hit, SnapPoint { point: pt(0.0, 0.0), kind: SnapKind::Endpoint });
    }

    #[test]
    fn intersection_of_two_lines() {
        let entities = [line(pt(-5.0, 0.0), pt(5.0, 0.0)), line(pt(0.0, -5.0), pt(0.0, 5.0))];
        let hit = find_osnap(pt(0.3, 0.3), 1.0, &entities, None, &SnapKind::defaults()).unwrap();
        assert_eq!(hit.kind, SnapKind::Intersection);
    }

    #[test]
    fn circle_center_is_found_from_its_rim() {
        let entities = [circle(pt(0.0, 0.0), 0.5)];
        let hit = find_osnap(pt(0.0, 0.6), 1.0, &entities, None, &SnapKind::defaults()).unwrap();
        assert_eq!(hit, SnapPoint { point: pt(0.0, 0.0), kind: SnapKind::Center });
    }

    #[test]
    fn perpendicular_needs_a_base_point() {
        let entities = [line(pt(0.0, 0.0), pt(100.0, 0.0))];
        assert!(find_osnap(pt(30.0, 0.5), 1.0, &entities, None, &SnapKind::defaults()).is_none());
        let hit = find_osnap(pt(30.5, 0.5), 1.0, &entities, Some(pt(30.0, 20.0)), &SnapKind::defaults()).unwrap();
        assert_eq!(hit, SnapPoint { point: pt(30.0, 0.0), kind: SnapKind::Perpendicular });
    }

    #[test]
    fn quadrants_nodes_and_nearest_when_turned_on() {
        let entities = [circle(pt(0.0, 0.0), 10.0), crate::model::entities::point(pt(50.0, 50.0))];
        let all = SnapKind::ALL;
        assert!(find_osnap(pt(0.3, 9.8), 1.0, &entities, None, &all).unwrap().point.dist(pt(0.0, 10.0)) < 1e-9);
        assert_eq!(find_osnap(pt(50.2, 50.1), 1.0, &entities, None, &all).unwrap().kind, SnapKind::Node);
        let near = find_osnap(pt(7.0, 7.3), 1.0, &entities, None, &[SnapKind::Nearest]).unwrap();
        assert!((near.point.dist(pt(0.0, 0.0)) - 10.0).abs() < 1e-9);
        assert!(find_osnap(pt(0.3, 9.8), 1.0, &entities, None, &[SnapKind::Endpoint]).is_none(), "quadrant off");
    }
}
