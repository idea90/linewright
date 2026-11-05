//! MLEDIT on the command line: adding and deleting a multiline's vertices, and joining two multilines at a corner.

use super::prompts::{Answer, Io, Picked, Prompt, PromptKind, Step};
use crate::model::entities::Shape;
use crate::model::geometry::Point;

const CORNER_JOINT: &str = "CJ";
const ADD_VERTEX: &str = "AV";
const DELETE_VERTEX: &str = "DV";
const UNDO: &str = "U";

pub async fn mledit_command(io: Io) {
    let prompt = Prompt::point("Enter mline editing option [Corner Joint/Add Vertex/Delete Vertex]:").options(&[
        CORNER_JOINT,
        ADD_VERTEX,
        DELETE_VERTEX,
    ]);
    match io.ask(prompt).await {
        Some(answer) if answer.is(CORNER_JOINT) => corner_joint(&io).await,
        Some(answer) if answer.is(ADD_VERTEX) || answer.is(DELETE_VERTEX) => vertices(&io, answer.is(ADD_VERTEX)).await,
        _ => {}
    }
}

/// A multiline picked at a prompt; None on Enter or Esc, or when something else was picked.
async fn pick_mline(io: &Io, message: &str) -> Option<Picked> {
    match io.ask(Prompt::new(PromptKind::Pick, message)).await {
        Some(Answer::Picked(picked)) if matches!(picked.entity.shape, Shape::Mline { .. }) => Some(picked),
        Some(Answer::Picked(_)) => {
            io.step(Step::Report("The object is not a multiline.".into()));
            None
        }
        _ => None,
    }
}

/// Add Vertex or Delete Vertex, on one multiline after another until Enter. Undo takes back the last edit.
async fn vertices(io: &Io, add: bool) {
    let mut edits = 0;
    loop {
        let message = if edits > 0 { "Select mline or [Undo]:" } else { "Select mline:" };
        let prompt = Prompt::new(PromptKind::Pick, message).options(&[UNDO]);
        let picked = match io.ask(prompt).await {
            Some(Answer::Picked(picked)) => picked,
            Some(answer) if answer.is(UNDO) && edits > 0 => {
                io.step(Step::Retract);
                edits -= 1;
                continue;
            }
            Some(_) => continue,
            None => return,
        };
        let Shape::Mline { vertices, closed, .. } = &picked.entity.shape else {
            io.step(Step::Report("The object is not a multiline.".into()));
            continue;
        };
        let changed =
            if add { with_vertex_added(vertices, *closed, picked.point) } else { with_vertex_deleted(vertices, *closed, picked.point) };
        match changed {
            Some(changed) => {
                let mut shape = picked.entity.shape.clone();
                if let Shape::Mline { vertices, .. } = &mut shape {
                    *vertices = changed;
                }
                io.step(Step::Modify(vec![picked.entity.with_shape(shape)]));
                edits += 1;
            }
            None => io.step(Step::Report("A multiline needs at least two vertices.".into())),
        }
    }
}

/// Corner Joint: two multilines trimmed or extended to meet where their paths cross, each keeping the part picked.
async fn corner_joint(io: &Io) {
    let Some(first) = pick_mline(io, "Select first mline:").await else { return };
    let Some(second) = pick_mline(io, "Select second mline:").await else { return };
    if first.entity.id == second.entity.id {
        io.step(Step::Report("Select two different multilines.".into()));
        return;
    }
    let (Shape::Mline { vertices: a, closed: false, .. }, Shape::Mline { vertices: b, closed: false, .. }) =
        (&first.entity.shape, &second.entity.shape)
    else {
        io.step(Step::Report("Closed multilines cannot be joined at a corner.".into()));
        return;
    };
    let (i, j) = (nearest_segment(a, false, first.point), nearest_segment(b, false, second.point));
    let Some(corner) = crossing(a[i], a[i + 1], b[j], b[j + 1]) else {
        io.step(Step::Report("The multilines are parallel.".into()));
        return;
    };
    let joined = |picked: &Picked, vertices: &[Point], at: usize| {
        let mut shape = picked.entity.shape.clone();
        if let Shape::Mline { vertices: v, .. } = &mut shape {
            *v = cut_at(vertices, at, picked.point, corner);
        }
        picked.entity.with_shape(shape)
    };
    io.step(Step::Modify(vec![joined(&first, a, i), joined(&second, b, j)]));
}

/// The index of the segment nearest the point; segment i runs from vertex i to the next (the first, when closed).
fn nearest_segment(vertices: &[Point], closed: bool, point: Point) -> usize {
    let count = if closed { vertices.len() } else { vertices.len().saturating_sub(1) };
    (0..count)
        .min_by(|&x, &y| {
            let d = |i: usize| distance_to_segment(point, vertices[i], vertices[(i + 1) % vertices.len()]);
            d(x).total_cmp(&d(y))
        })
        .unwrap_or(0)
}

fn distance_to_segment(point: Point, a: Point, b: Point) -> f64 {
    point.dist(project(point, a, b))
}

/// The point of the segment nearest `point`.
fn project(point: Point, a: Point, b: Point) -> Point {
    let along = b - a;
    let t = ((point - a).dot(along) / along.dot(along).max(f64::MIN_POSITIVE)).clamp(0.0, 1.0);
    a + along * t
}

/// Where the lines through two segments cross, or None when they are parallel.
fn crossing(a: Point, b: Point, c: Point, d: Point) -> Option<Point> {
    let (r, s) = (b - a, d - c);
    let denominator = r.cross(s);
    if denominator.abs() <= 1e-12 * r.length() * s.length() {
        return None;
    }
    Some(a + r * ((c - a).cross(s) / denominator))
}

/// The vertices with the path cut or stretched to end at `corner` on segment `at`, keeping the side where `picked`
/// lies.
fn cut_at(vertices: &[Point], at: usize, picked: Point, corner: Point) -> Vec<Point> {
    let (a, b) = (vertices[at], vertices[at + 1]);
    let along = b - a;
    let t = |p: Point| (p - a).dot(along);
    if t(corner) > t(picked) {
        vertices[..=at].iter().copied().chain([corner]).collect()
    } else {
        std::iter::once(corner).chain(vertices[at + 1..].iter().copied()).collect()
    }
}

/// The vertices with a new one on the segment nearest the point.
fn with_vertex_added(vertices: &[Point], closed: bool, point: Point) -> Option<Vec<Point>> {
    if vertices.len() < 2 {
        return None;
    }
    let i = nearest_segment(vertices, closed, point);
    let on = project(point, vertices[i], vertices[(i + 1) % vertices.len()]);
    let mut changed = vertices.to_vec();
    changed.insert(i + 1, on);
    Some(changed)
}

/// The vertices without the one nearest the point. None when too few would be left: two for an open multiline,
/// three for a closed one.
fn with_vertex_deleted(vertices: &[Point], closed: bool, point: Point) -> Option<Vec<Point>> {
    let fewest = if closed { 3 } else { 2 };
    if vertices.len() <= fewest {
        return None;
    }
    let nearest = (0..vertices.len()).min_by(|&x, &y| vertices[x].dist(point).total_cmp(&vertices[y].dist(point)))?;
    let mut changed = vertices.to_vec();
    changed.remove(nearest);
    Some(changed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::geometry::pt;

    #[test]
    fn a_vertex_goes_on_the_segment_picked_and_the_nearest_one_comes_off() {
        let path = [pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 10.0)];
        assert_eq!(with_vertex_added(&path, false, pt(4.0, 0.3)), Some(vec![pt(0.0, 0.0), pt(4.0, 0.0), pt(10.0, 0.0), pt(10.0, 10.0)]));
        assert_eq!(with_vertex_deleted(&path, false, pt(9.0, 1.0)), Some(vec![pt(0.0, 0.0), pt(10.0, 10.0)]));
        assert_eq!(with_vertex_deleted(&path[..2], false, pt(0.0, 0.0)), None);
    }

    #[test]
    fn a_corner_joint_keeps_the_picked_sides() {
        // A horizontal wall running past a vertical one, and a vertical wall stopping short of the horizontal one.
        let horizontal = [pt(0.0, 0.0), pt(20.0, 0.0)];
        let vertical = [pt(10.0, 3.0), pt(10.0, 15.0)];
        let corner = crossing(horizontal[0], horizontal[1], vertical[0], vertical[1]).unwrap();
        assert_eq!(corner, pt(10.0, 0.0));
        assert_eq!(cut_at(&horizontal, 0, pt(2.0, 0.0), corner), vec![pt(0.0, 0.0), pt(10.0, 0.0)]);
        assert_eq!(cut_at(&vertical, 0, pt(10.0, 12.0), corner), vec![pt(10.0, 0.0), pt(10.0, 15.0)]);
    }
}
