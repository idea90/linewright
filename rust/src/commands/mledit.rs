//! MLEDIT on the command line, as AutoCAD's -MLEDIT: crosses and tees where two multilines meet, corner joints,
//! adding and deleting vertices, and cutting and welding a multiline's lines.

use super::prompts::{Answer, Io, Picked, Prompt, PromptKind, Step};
use crate::model::entities::{Entity, Shape};
use crate::model::geometry::Point;
use crate::model::mline::{MlineGap, dedup, element_segments};

const CLOSED_CROSS: &str = "CC";
const OPEN_CROSS: &str = "OC";
const MERGED_CROSS: &str = "MC";
const CLOSED_TEE: &str = "CT";
const OPEN_TEE: &str = "OT";
const MERGED_TEE: &str = "MT";
const CORNER_JOINT: &str = "CJ";
const ADD_VERTEX: &str = "AV";
const DELETE_VERTEX: &str = "DV";
const CUT_SINGLE: &str = "CS";
const CUT_ALL: &str = "CA";
const WELD_ALL: &str = "WA";
const UNDO: &str = "U";

pub async fn mledit_command(io: Io) {
    let prompt = Prompt::point("Enter mline editing option [CC/OC/MC/CT/OT/MT/CJ/AV/DV/CS/CA/WA]:").options(&[
        CLOSED_CROSS,
        OPEN_CROSS,
        MERGED_CROSS,
        CLOSED_TEE,
        OPEN_TEE,
        MERGED_TEE,
        CORNER_JOINT,
        ADD_VERTEX,
        DELETE_VERTEX,
        CUT_SINGLE,
        CUT_ALL,
        WELD_ALL,
    ]);
    let Some(Answer::Keyword(tool)) = io.ask(prompt).await else { return };
    match tool.as_str() {
        CLOSED_CROSS | OPEN_CROSS | MERGED_CROSS => joint(&io, &tool).await,
        CLOSED_TEE | OPEN_TEE | MERGED_TEE => joint(&io, &tool).await,
        CORNER_JOINT => corner_joint(&io).await,
        ADD_VERTEX | DELETE_VERTEX => vertices(&io, tool == ADD_VERTEX).await,
        _ => cut_or_weld(&io, &tool).await,
    }
}

/// What a multiline is made of, for working out where to cut it.
struct Lines<'a> {
    entity: &'a Entity,
    points: Vec<Point>,
    closed: bool,
    /// Each element's line on each segment: `[element][segment]`.
    segments: Vec<Vec<(Point, Point)>>,
    gaps: Vec<MlineGap>,
    /// The outermost two elements: the one furthest left of the path, then the one furthest right.
    outer: (usize, usize),
}

fn lines(entity: &Entity) -> Option<Lines<'_>> {
    let Shape::Mline { vertices, closed, justification, scale, style, gaps } = &entity.shape else { return None };
    let segments = element_segments(vertices, *closed, *justification, *scale, style);
    let offsets: Vec<f64> = style.elements.iter().map(|e| e.offset).collect();
    let by = |better: fn(f64, f64) -> bool| (0..offsets.len()).reduce(|a, b| if better(offsets[b], offsets[a]) { b } else { a });
    let outer = (by(|x, y| x > y)?, by(|x, y| x < y)?);
    Some(Lines { entity, points: dedup(vertices), closed: *closed && vertices.len() > 2, segments, gaps: gaps.clone(), outer })
}

impl Lines<'_> {
    fn with_gaps(&self, gaps: Vec<MlineGap>) -> Entity {
        let mut shape = self.entity.shape.clone();
        if let Shape::Mline { gaps: g, .. } = &mut shape {
            *g = gaps;
        }
        self.entity.with_shape(shape)
    }

    fn segment_near(&self, point: Point) -> usize {
        nearest_segment(&self.points, self.closed, point)
    }

    /// The element whose line passes nearest the point on a segment.
    fn element_near(&self, segment: usize, point: Point) -> usize {
        (0..self.segments.len())
            .min_by(|&a, &b| {
                let d = |e: usize| {
                    let (start, end) = self.segments[e][segment];
                    distance_to_segment(point, start, end)
                };
                d(a).total_cmp(&d(b))
            })
            .unwrap_or(0)
    }

    /// How far along an element's line on a segment the point lies, from where that line starts.
    fn along(&self, element: usize, segment: usize, point: Point) -> f64 {
        let (start, end) = self.segments[element][segment];
        let direction = end - start;
        (point - start).dot(direction) / direction.length().max(f64::MIN_POSITIVE)
    }

    /// The gaps that cut each of these elements on a segment where it passes between the other multiline's
    /// outermost lines on its segment.
    fn cut_by(&self, segment: usize, elements: &[usize], other: &Lines, other_segment: usize) -> Vec<MlineGap> {
        let band = (other.segments[other.outer.0][other_segment], other.segments[other.outer.1][other_segment]);
        elements
            .iter()
            .filter_map(|&element| {
                let (start, end) = self.segments[element][segment];
                let (from, to) = between(start, end, band)?;
                Some(MlineGap { segment, element, from, to })
            })
            .collect()
    }
}

/// Where a line from `start` to `end` runs between two parallel lines, as distances along it from its start;
/// None when it doesn't, or runs alongside them.
fn between(start: Point, end: Point, (first, second): ((Point, Point), (Point, Point))) -> Option<(f64, f64)> {
    let length = start.dist(end);
    let across = |(a, b): (Point, Point)| {
        let direction = b - a;
        let rate = direction.cross(end - start);
        (rate.abs() > 1e-12 * direction.length() * length).then(|| -direction.cross(start - a) / rate)
    };
    let (t0, t1) = (across(first)?, across(second)?);
    let (from, to) = (t0.min(t1).max(0.0), t0.max(t1).min(1.0));
    (to > from).then_some((from * length, to * length))
}

/// The crosses and tees: the first multiline picked is cut (a tee also ends it at the second), the second is cut
/// as the tool says.
async fn joint(io: &Io, tool: &str) {
    let Some(first) = pick_mline(io, "Select first mline:").await else { return };
    let Some(second) = pick_mline(io, "Select second mline:").await else { return };
    if first.entity.id == second.entity.id {
        io.step(Step::Report("Select two different multilines.".into()));
        return;
    }
    let (Some(a), Some(b)) = (lines(&first.entity), lines(&second.entity)) else { return };
    let (sa, sb) = (a.segment_near(first.point), b.segment_near(second.point));
    let every = |lines: &Lines| (0..lines.segments.len()).collect::<Vec<_>>();
    let outer = |lines: &Lines| vec![lines.outer.0, lines.outer.1];
    let tee = matches!(tool, CLOSED_TEE | OPEN_TEE | MERGED_TEE);
    if tee {
        let Some(changed) = tee_joint(&a, sa, first.point, &b, sb, tool) else {
            io.step(Step::Report("The multilines are parallel.".into()));
            return;
        };
        io.step(Step::Modify(changed));
        return;
    }
    // A closed cross cuts only the first through the second; an open one also opens the second's outer lines;
    // a merged one opens only the outer lines of both.
    let (cut_a, cut_b) = match tool {
        CLOSED_CROSS => (every(&a), vec![]),
        OPEN_CROSS => (every(&a), outer(&b)),
        _ => (outer(&a), outer(&b)),
    };
    let gaps_a = a.cut_by(sa, &cut_a, &b, sb);
    let gaps_b = b.cut_by(sb, &cut_b, &a, sa);
    if gaps_a.is_empty() && gaps_b.is_empty() {
        io.step(Step::Report("The multilines do not cross there.".into()));
        return;
    }
    let mut changed = vec![a.with_gaps(a.gaps.iter().copied().chain(gaps_a).collect())];
    if !gaps_b.is_empty() {
        changed.push(b.with_gaps(b.gaps.iter().copied().chain(gaps_b).collect()));
    }
    io.step(Step::Modify(changed));
}

/// A tee: the first multiline ends where its path meets the near outer line of the second, keeping the part picked.
/// An open tee opens that line of the second where the first meets it; a merged tee opens its inner lines too.
fn tee_joint(a: &Lines, sa: usize, picked: Point, b: &Lines, sb: usize, tool: &str) -> Option<Vec<Entity>> {
    // The second's outer line on the side the first comes from.
    let (b_start, b_end) = (b.points[sb], b.points[(sb + 1) % b.points.len()]);
    let left = (b_end - b_start).cross(picked - b_start) > 0.0;
    let near = if left { b.outer.0 } else { b.outer.1 };
    let (n0, n1) = b.segments[near][sb];
    let (a_start, a_end) = (a.points[sa], a.points[(sa + 1) % a.points.len()]);
    let end = crossing(a_start, a_end, n0, n1)?;
    let vertices = cut_at(&a.points, sa, picked, end);
    // The first keeps the gaps on the segments it keeps, numbered from its new start.
    let kept_head = (end - a_start).dot(a_end - a_start) > (picked - a_start).dot(a_end - a_start);
    let gaps_a: Vec<MlineGap> = if kept_head {
        a.gaps.iter().filter(|g| g.segment < sa).copied().collect()
    } else {
        a.gaps.iter().filter(|g| g.segment > sa).map(|g| MlineGap { segment: g.segment - sa, ..*g }).collect()
    };
    let mut first = a.entity.shape.clone();
    if let Shape::Mline { vertices: v, gaps, .. } = &mut first {
        *v = vertices;
        *gaps = gaps_a;
    }
    let mut changed = vec![a.entity.with_shape(first)];
    let far = if left { b.outer.1 } else { b.outer.0 };
    let opened: Vec<usize> = match tool {
        OPEN_TEE => vec![near],
        MERGED_TEE => (0..b.segments.len()).filter(|&e| e != far).collect(),
        _ => vec![],
    };
    let gaps_b = b.cut_by(sb, &opened, a, sa);
    if !gaps_b.is_empty() {
        changed.push(b.with_gaps(b.gaps.iter().copied().chain(gaps_b).collect()));
    }
    Some(changed)
}

/// Cut Single, Cut All and Weld All: between two points on a multiline, one of its lines (the one nearest the
/// first point) or all of them are cut, or every cut there is closed again.
async fn cut_or_weld(io: &Io, tool: &str) {
    let Some(picked) = pick_mline(io, "Select mline:").await else { return };
    let prompt = Prompt::point("Select second point:").base(picked.point);
    let Some(Answer::Point(second)) = io.ask(prompt).await else { return };
    let Some(lines) = lines(&picked.entity) else { return };
    let (s1, s2) = (lines.segment_near(picked.point), lines.segment_near(second));
    let ((first_segment, first), (last_segment, last)) =
        if s1 <= s2 { ((s1, picked.point), (s2, second)) } else { ((s2, second), (s1, picked.point)) };
    let elements: Vec<usize> = match tool {
        CUT_SINGLE => vec![lines.element_near(s1, picked.point)],
        _ => (0..lines.segments.len()).collect(),
    };
    // The stretch between the points on each segment of each element, from where it starts to where it ends.
    let mut stretch: Vec<MlineGap> = vec![];
    for &element in &elements {
        for segment in first_segment..=last_segment {
            let (start, end) = lines.segments[element][segment];
            let length = start.dist(end);
            let from = if segment == first_segment { lines.along(element, segment, first).clamp(0.0, length) } else { 0.0 };
            let to = if segment == last_segment { lines.along(element, segment, last).clamp(0.0, length) } else { length };
            let (from, to) = if first_segment == last_segment { (from.min(to), from.max(to)) } else { (from, to) };
            if to > from {
                stretch.push(MlineGap { segment, element, from, to });
            }
        }
    }
    let gaps = if tool == WELD_ALL { welded(&lines.gaps, &stretch) } else { lines.gaps.iter().copied().chain(stretch).collect() };
    io.step(Step::Modify(vec![lines.with_gaps(gaps)]));
}

/// The gaps with the stretches taken out of them: what is cut there is joined again.
fn welded(gaps: &[MlineGap], stretches: &[MlineGap]) -> Vec<MlineGap> {
    let mut out = vec![];
    for gap in gaps {
        let mut pieces = vec![(gap.from, gap.to)];
        for weld in stretches.iter().filter(|w| w.segment == gap.segment && w.element == gap.element) {
            pieces = pieces
                .into_iter()
                .flat_map(|(from, to)| [(from, to.min(weld.from)), (from.max(weld.to), to)])
                .filter(|(from, to)| to > from)
                .collect();
        }
        out.extend(pieces.into_iter().map(|(from, to)| MlineGap { from, to, ..*gap }));
    }
    out
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
