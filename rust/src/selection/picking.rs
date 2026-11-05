use crate::model::entities::{Entity, Shape};
use crate::model::geometry::{Point, pt};
use crate::model::parts::flatten;
use crate::snap::intersect::intersections;
use crate::snap::primitives::{Primitive, arc_point, distance_to, on_arc, primitives, seg};

const TEXT_WIDTH_PER_HEIGHT: f64 = 0.6; // rough average glyph advance; the real width depends on the font

/// The boxes around an entity's text, its own or (for a block insert) its block's and attributes'.
fn text_boxes(entity: &Entity) -> Vec<[Point; 4]> {
    if !matches!(entity.shape, Shape::Text { .. } | Shape::Insert { .. }) {
        return vec![];
    }
    flatten(entity, f64::INFINITY).iter().flat_map(|(_, parts)| parts.texts.iter().map(|t| t.bounding_box(TEXT_WIDTH_PER_HEIGHT))).collect()
}

/// What the entity looks like to the mouse. Text is treated as the box around it.
pub fn outline(entity: &Entity) -> Vec<Primitive> {
    let mut shapes = primitives(entity);
    for corners in text_boxes(entity) {
        shapes.extend((0..4).map(|i| seg(corners[i], corners[(i + 1) % 4])));
    }
    shapes
}

pub fn entity_distance(entity: &Entity, point: Point) -> f64 {
    let shapes = outline(entity);
    if shapes.is_empty() {
        return f64::INFINITY;
    }
    if text_boxes(entity).iter().any(|corners| inside(corners, point)) {
        return 0.0;
    }
    shapes.iter().map(|shape| distance_to(shape, point)).fold(f64::INFINITY, f64::min)
}

/// Whether the point is inside a convex polygon given in order.
fn inside(corners: &[Point], point: Point) -> bool {
    let signs: Vec<f64> = (0..corners.len()).map(|i| (corners[(i + 1) % corners.len()] - corners[i]).cross(point - corners[i])).collect();
    signs.iter().all(|s| *s >= 0.0) || signs.iter().all(|s| *s <= 0.0)
}

/// The entity nearest the point within the aperture. Later (upper) entities win ties.
pub fn pick_entity(entities: &[Entity], point: Point, aperture: f64) -> Option<&Entity> {
    let mut best = None;
    let mut best_distance = aperture;
    for entity in entities {
        let distance = entity_distance(entity, point);
        if distance <= best_distance {
            best = Some(entity);
            best_distance = distance;
        }
    }
    best
}

/// Points that bound the shape: its ends, plus the extreme points of a round shape.
fn extent_points(shape: &Primitive) -> Vec<Point> {
    match *shape {
        Primitive::Line { start, end } => vec![start, end],
        Primitive::Circle { center, radius } | Primitive::Arc { center, radius, .. } => {
            let quadrants = [(1.0, 0.0), (0.0, 1.0), (-1.0, 0.0), (0.0, -1.0)].map(|(dx, dy)| center + pt(dx, dy) * radius);
            match *shape {
                Primitive::Arc { start_angle, end_angle, .. } => {
                    let mut points = vec![arc_point(center, radius, start_angle), arc_point(center, radius, end_angle)];
                    points.extend(quadrants.into_iter().filter(|p| on_arc(center, start_angle, end_angle, *p)));
                    points
                }
                _ => quadrants.to_vec(),
            }
        }
    }
}

fn in_rect(p: Point, low: Point, high: Point) -> bool {
    low.x <= p.x && p.x <= high.x && low.y <= p.y && p.y <= high.y
}

fn touches_rect(shape: &Primitive, low: Point, high: Point) -> bool {
    if extent_points(shape).into_iter().any(|p| in_rect(p, low, high)) {
        return true;
    }
    let corners = [low, pt(high.x, low.y), high, pt(low.x, high.y)];
    (0..4).any(|i| !intersections(shape, &seg(corners[i], corners[(i + 1) % 4])).is_empty())
}

/// Window (dragged left to right): entities entirely inside. Crossing (right to left): inside or touching.
pub fn select_in_box(entities: &[Entity], first: Point, second: Point) -> Vec<Entity> {
    let low = pt(first.x.min(second.x), first.y.min(second.y));
    let high = pt(first.x.max(second.x), first.y.max(second.y));
    let crossing = second.x < first.x;
    entities
        .iter()
        .filter(|entity| {
            let shapes = outline(entity);
            if shapes.is_empty() {
                return false;
            }
            if crossing {
                shapes.iter().any(|shape| touches_rect(shape, low, high))
            } else {
                shapes.iter().flat_map(extent_points).all(|p| in_rect(p, low, high))
            }
        })
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::entities::{circle, line, text};

    #[test]
    fn later_entities_win_ties() {
        let entities = [line(pt(0.0, 0.0), pt(10.0, 0.0)), line(pt(0.0, 0.0), pt(10.0, 0.0))];
        assert_eq!(pick_entity(&entities, pt(5.0, 0.1), 1.0).unwrap().id, entities[1].id);
        assert!(pick_entity(&entities, pt(5.0, 3.0), 1.0).is_none());
    }

    #[test]
    fn clicking_inside_text_picks_it() {
        let entities = [text(pt(0.0, 0.0), "HELLO", 2.0, 0.0)];
        assert!(pick_entity(&entities, pt(3.0, 1.0), 0.1).is_some());
    }

    #[test]
    fn window_needs_everything_inside_crossing_needs_a_touch() {
        let entities = [line(pt(1.0, 1.0), pt(2.0, 2.0)), circle(pt(5.0, 5.0), 3.0)];
        let window = select_in_box(&entities, pt(0.0, 0.0), pt(4.0, 4.0));
        assert_eq!(window.len(), 1);
        let crossing = select_in_box(&entities, pt(4.0, 4.0), pt(0.0, 0.0));
        assert_eq!(crossing.len(), 2);
    }
}
