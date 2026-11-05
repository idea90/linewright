//! QDIM: many dimensions at once from the geometry selected.

use std::cell::RefCell;
use std::rc::Rc;

use super::defaults::Defaults;
use super::prompts::{Answer, Io, Prompt, PromptKind, Step};
use crate::model::annotation::AnnotationKind;
use crate::model::dimension::DimensionLook;
use crate::model::entities::{Entity, Shape};
use crate::model::geometry::{Point, direction, pt};
use crate::model::index::Bounds;

const CONTINUOUS: &str = "C";
const BASELINE: &str = "B";
const ORDINATE: &str = "O";
const RADIUS: &str = "R";
const DIAMETER: &str = "D";
/// Baseline dimensions step out by this many text heights, as DIMBASELINE's do.
const BASELINE_SPACING: f64 = 1.5;
/// Where radius and diameter dimensions point on their curves, in degrees.
const RADIAL_ANGLE: f64 = 45.0;

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Continuous,
    Baseline,
    Ordinate,
    Radius,
    Diameter,
}

/// The points QDIM measures between: ends of lines and arcs, polyline vertices, centers of arcs and circles.
fn feature_points(entities: &[Entity]) -> Vec<Point> {
    let mut points = vec![];
    for entity in entities {
        match &entity.shape {
            Shape::Line { start, end } => points.extend([*start, *end]),
            Shape::Polyline { vertices, .. } => points.extend(vertices.iter().map(|v| v.point())),
            Shape::Circle { center, .. } => points.push(*center),
            Shape::Arc { center, radius, start_angle, end_angle } => {
                points.extend([*center + direction(*start_angle) * *radius, *center + direction(*end_angle) * *radius, *center]);
            }
            _ => {}
        }
    }
    points
}

/// The points in order along an axis, keeping one for each distinct position.
fn along_axis(points: &[Point], horizontal: bool) -> Vec<Point> {
    let at = |p: &Point| if horizontal { p.x } else { p.y };
    let mut sorted = points.to_vec();
    sorted.sort_by(|a, b| at(a).total_cmp(&at(b)));
    sorted.dedup_by(|a, b| (at(a) - at(b)).abs() < 1e-9);
    sorted
}

/// The dimensions a mode makes with the dimension line through the cursor. They measure across when the cursor is
/// above or below the geometry more than beside it, and up otherwise.
fn dimensions(mode: Mode, entities: &[Entity], points: &[Point], cursor: Point, look: &DimensionLook) -> Vec<Entity> {
    let annotate = |kind, points: Vec<Point>| Entity::new(Shape::Annotation { kind, points, look: look.clone() });
    if matches!(mode, Mode::Radius | Mode::Diameter) {
        let kind = if mode == Mode::Radius { AnnotationKind::Radius } else { AnnotationKind::Diameter };
        return entities
            .iter()
            .filter_map(|e| match e.shape {
                Shape::Circle { center, radius } | Shape::Arc { center, radius, .. } => {
                    Some(annotate(kind, vec![center, center + direction(RADIAL_ANGLE) * radius]))
                }
                _ => None,
            })
            .collect();
    }
    let Some(bounds) = points.iter().map(|p| Bounds::from_corners(*p, *p)).reduce(Bounds::union) else { return vec![] };
    let beside = (bounds.x0 - cursor.x).max(cursor.x - bounds.x1).max(0.0);
    let above = (bounds.y0 - cursor.y).max(cursor.y - bounds.y1).max(0.0);
    let horizontal = above >= beside;
    let angle = if horizontal { 0.0 } else { 90.0 };
    let line = |from: Point, to: Point, through: Point| {
        Entity::new(Shape::Dimension { start: from, end: to, line_point: through, angle, look: look.clone() })
    };
    let ordered = along_axis(points, horizontal);
    match mode {
        Mode::Continuous => ordered.windows(2).map(|pair| line(pair[0], pair[1], cursor)).collect(),
        Mode::Baseline => {
            let Some((first, rest)) = ordered.split_first() else { return vec![] };
            let spacing = look.text_height * look.scale * BASELINE_SPACING;
            let across = if horizontal { pt(0.0, 1.0) } else { pt(1.0, 0.0) };
            let middle = bounds.center();
            let side = if (cursor - middle).dot(across) >= 0.0 { 1.0 } else { -1.0 };
            rest.iter().enumerate().map(|(i, to)| line(*first, *to, cursor + across * (side * spacing * i as f64))).collect()
        }
        _ => ordered
            .iter()
            .map(|p| {
                let end = if horizontal { pt(p.x, cursor.y) } else { pt(cursor.x, p.y) };
                annotate(AnnotationKind::Ordinate { x_datum: horizontal }, vec![pt(0.0, 0.0), *p, end])
            })
            .collect(),
    }
}

pub async fn qdim(io: Io, selected: Vec<Entity>, defaults: Rc<RefCell<Defaults>>) {
    let look = defaults.borrow().dimension_look.clone();
    let entities = if selected.is_empty() {
        let Some(Answer::Objects(picked)) = io.ask(Prompt::new(PromptKind::Select, "Select geometry to dimension:")).await else { return };
        picked
    } else {
        selected
    };
    let points = feature_points(&entities);
    if points.is_empty() {
        io.step(Step::Report("None of these objects can be dimensioned: pick lines, polylines, arcs or circles.".into()));
        return;
    }
    let mut mode = Mode::Continuous;
    loop {
        let (shown_entities, shown_points, shown_look) = (entities.clone(), points.clone(), look.clone());
        let prompt = Prompt::point("Specify dimension line position, or [Continuous/Baseline/Ordinate/Radius/Diameter]:")
            .options(&[CONTINUOUS, BASELINE, ORDINATE, RADIUS, DIAMETER])
            .preview(move |cursor| dimensions(mode, &shown_entities, &shown_points, cursor, &shown_look));
        match io.ask(prompt).await {
            Some(Answer::Point(at)) => {
                let made = dimensions(mode, &entities, &points, at, &look);
                if made.is_empty() {
                    io.step(Step::Report("There is nothing to dimension that way.".into()));
                } else {
                    io.step(Step::Report(format!("{} dimension(s) made.", made.len())));
                    io.step(Step::Drawn(made));
                }
                return;
            }
            Some(answer) if answer.is(CONTINUOUS) => mode = Mode::Continuous,
            Some(answer) if answer.is(BASELINE) => mode = Mode::Baseline,
            Some(answer) if answer.is(ORDINATE) => mode = Mode::Ordinate,
            Some(answer) if answer.is(RADIUS) => mode = Mode::Radius,
            Some(answer) if answer.is(DIAMETER) => mode = Mode::Diameter,
            Some(_) => {}
            None => return,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::entities::{circle, line};

    #[test]
    fn continuous_dimensions_chain_across_the_features_in_order() {
        let entities = [line(pt(0.0, 0.0), pt(10.0, 0.0)), line(pt(10.0, 0.0), pt(10.0, 5.0)), line(pt(25.0, 0.0), pt(25.0, 5.0))];
        let points = feature_points(&entities);
        let made = dimensions(Mode::Continuous, &entities, &points, pt(5.0, 20.0), &DimensionLook::default());
        assert_eq!(made.len(), 2, "0 to 10, then 10 to 25");
        let Shape::Dimension { start, end, angle, .. } = &made[1].shape else { panic!() };
        assert_eq!((start.x, end.x, *angle), (10.0, 25.0, 0.0));
    }

    #[test]
    fn beside_the_geometry_measures_up_and_baselines_step_out() {
        let entities = [line(pt(0.0, 0.0), pt(0.0, 10.0)), line(pt(0.0, 20.0), pt(5.0, 20.0))];
        let points = feature_points(&entities);
        let look = DimensionLook::default();
        let made = dimensions(Mode::Baseline, &entities, &points, pt(30.0, 5.0), &look);
        assert_eq!(made.len(), 2);
        let (Shape::Dimension { angle, line_point: first, .. }, Shape::Dimension { line_point: second, .. }) =
            (&made[0].shape, &made[1].shape)
        else {
            panic!()
        };
        assert_eq!(*angle, 90.0);
        assert!(second.x > first.x);
    }

    #[test]
    fn radius_mode_takes_only_curves() {
        let entities = [circle(pt(0.0, 0.0), 2.0), line(pt(0.0, 0.0), pt(1.0, 0.0))];
        let made = dimensions(Mode::Radius, &entities, &feature_points(&entities), pt(9.0, 9.0), &DimensionLook::default());
        assert_eq!(made.len(), 1);
    }
}
