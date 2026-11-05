//! OVERKILL's clean-up: copies of an object lying on top of it are deleted, and lines along one line or arcs on one
//! circle that overlap are combined into one object. Objects only count as the same when their layer, color,
//! linetype and lineweight match, unless those are ignored.

use std::collections::HashMap;
use std::f64::consts::PI;

use super::entities::{Entity, Shape};
use super::geometry::{Point, modulo, sweep_degrees};
use super::index::entity_extent;

/// How OVERKILL compares and combines, as its options set it.
#[derive(Clone, Debug, PartialEq)]
pub struct OverkillSettings {
    /// Points closer than this count as the same point.
    pub tolerance: f64,
    pub ignore_color: bool,
    pub ignore_layer: bool,
    pub ignore_linetype: bool,
    pub ignore_lineweight: bool,
    /// Lines or arcs that partly overlap become one.
    pub combine_partial: bool,
    /// Lines or arcs that meet end to end, without overlapping, become one.
    pub combine_end_to_end: bool,
}

impl Default for OverkillSettings {
    fn default() -> Self {
        OverkillSettings {
            tolerance: 1e-6,
            ignore_color: false,
            ignore_layer: false,
            ignore_linetype: false,
            ignore_lineweight: false,
            combine_partial: true,
            combine_end_to_end: false,
        }
    }
}

/// What OVERKILL does to the drawing, and what it tells the user.
#[derive(Clone, Debug, Default)]
pub struct Cleanup {
    pub remove: Vec<Entity>,
    /// Objects that grew to cover the ones combined into them.
    pub modify: Vec<Entity>,
    pub duplicates: usize,
    pub overlaps: usize,
}

/// Lines further apart in direction than this, in radians, are never along one line.
const ANGLE_TOLERANCE: f64 = 1e-9;

pub fn overkill(entities: &[Entity], settings: &OverkillSettings) -> Cleanup {
    let mut cleanup = Cleanup::default();
    let mut alike: HashMap<(String, i32, String, i16), Vec<&Entity>> = HashMap::new();
    for entity in entities {
        let key = (
            if settings.ignore_layer { String::new() } else { entity.layer.clone() },
            if settings.ignore_color { 0 } else { entity.color },
            if settings.ignore_linetype { String::new() } else { entity.linetype.clone() },
            if settings.ignore_lineweight { 0 } else { entity.lineweight },
        );
        alike.entry(key).or_default().push(entity);
    }
    let mut groups: Vec<Vec<&Entity>> = alike.into_values().collect();
    // The same drawing always cleans up the same way.
    groups.sort_by_key(|group| group[0].id);
    for group in groups {
        let (lines, rest): (Vec<&Entity>, Vec<&Entity>) = group.into_iter().partition(|e| matches!(e.shape, Shape::Line { .. }));
        let (round, rest): (Vec<&Entity>, Vec<&Entity>) =
            rest.into_iter().partition(|e| matches!(e.shape, Shape::Arc { .. } | Shape::Circle { .. }));
        combine_lines(&lines, settings, &mut cleanup);
        combine_round(&round, settings, &mut cleanup);
        delete_copies(&rest, settings.tolerance, &mut cleanup);
    }
    cleanup
}

/// A stretch of a line or arc, from `lo` to `hi` along it.
#[derive(Clone, Copy)]
struct Span<'a> {
    lo: f64,
    hi: f64,
    entity: &'a Entity,
}

/// Spans along one line or circle that became one, the first of them the one that is kept.
struct Run<'a> {
    lo: f64,
    hi: f64,
    members: Vec<Span<'a>>,
}

/// Combines spans that overlap, or meet end to end when the settings say so. Spans wholly inside another always
/// go. Counts what goes as duplicates or overlaps.
fn merge_spans<'a>(mut spans: Vec<Span<'a>>, tolerance: f64, settings: &OverkillSettings, cleanup: &mut Cleanup) -> Vec<Run<'a>> {
    // Longest first among those starting together, so the one kept covers the rest.
    spans.sort_by(|a, b| a.lo.total_cmp(&b.lo).then(b.hi.total_cmp(&a.hi)).then(a.entity.id.cmp(&b.entity.id)));
    let mut runs: Vec<Run> = vec![];
    for span in spans {
        if let Some(run) = runs.last_mut() {
            let keeper = run.members[0];
            if span.hi <= run.hi + tolerance && span.lo >= run.lo - tolerance {
                if (span.lo - keeper.lo).abs() <= tolerance && (span.hi - keeper.hi).abs() <= tolerance {
                    cleanup.duplicates += 1;
                } else {
                    cleanup.overlaps += 1;
                }
                run.members.push(span);
                continue;
            }
            let overlapping = span.lo < run.hi - tolerance;
            let touching = !overlapping && span.lo <= run.hi + tolerance;
            if (overlapping && settings.combine_partial) || (touching && settings.combine_end_to_end) {
                cleanup.overlaps += 1;
                run.hi = run.hi.max(span.hi);
                run.members.push(span);
                continue;
            }
        }
        runs.push(Run { lo: span.lo, hi: span.hi, members: vec![span] });
    }
    runs
}

/// Takes away every span of a run but the first, and stretches that one over the run when it has to grow.
fn settle_run(run: &Run, grown: impl Fn(&Entity, f64, f64) -> Shape, cleanup: &mut Cleanup) {
    let keeper = run.members[0];
    cleanup.remove.extend(run.members[1..].iter().map(|s| s.entity.clone()));
    if run.lo < keeper.lo || run.hi > keeper.hi {
        cleanup.modify.push(keeper.entity.with_shape(grown(keeper.entity, run.lo, run.hi)));
    }
}

/// Lines along one infinite line: grouped by direction, then by how far the line is from the origin, then
/// combined along it.
fn combine_lines(lines: &[&Entity], settings: &OverkillSettings, cleanup: &mut Cleanup) {
    let tolerance = settings.tolerance;
    struct Along<'a> {
        angle: f64,
        offset: f64,
        span: Span<'a>,
    }
    let mut along: Vec<Along> = lines
        .iter()
        .filter_map(|&entity| {
            let Shape::Line { start, end } = entity.shape else { return None };
            if start.dist(end) <= tolerance {
                return None;
            }
            // The direction folded onto a half turn, with lines just short of it counted as level ones.
            let mut angle = modulo((end.y - start.y).atan2(end.x - start.x), PI);
            if angle > PI - ANGLE_TOLERANCE {
                angle -= PI;
            }
            let u = Point { x: angle.cos(), y: angle.sin() };
            let (a, b) = (u.dot(start), u.dot(end));
            Some(Along { angle, offset: u.cross(start), span: Span { lo: a.min(b), hi: a.max(b), entity } })
        })
        .collect();
    along.sort_by(|a, b| a.angle.total_cmp(&b.angle));
    for same_angle in split_where(&mut along, |a, b| b.angle - a.angle > ANGLE_TOLERANCE) {
        same_angle.sort_by(|a, b| a.offset.total_cmp(&b.offset));
        for same_line in split_where(same_angle, |a, b| b.offset - a.offset > tolerance) {
            let spans = same_line.iter().map(|a| a.span).collect();
            let angle = same_line[0].angle;
            for run in merge_spans(spans, tolerance, settings, cleanup) {
                settle_run(&run, |entity, lo, hi| stretched_line(entity, angle, lo, hi), cleanup);
            }
        }
    }
}

/// The line stretched along itself to reach from `lo` to `hi`, measured along `angle`, keeping its direction.
fn stretched_line(entity: &Entity, angle: f64, lo: f64, hi: f64) -> Shape {
    let Shape::Line { start, end } = entity.shape else { return entity.shape.clone() };
    let u = Point { x: angle.cos(), y: angle.sin() };
    let (ta, tb) = (u.dot(start), u.dot(end));
    let at = |t: f64| start + (end - start) * ((t - ta) / (tb - ta));
    let (to_lo, to_hi) = (at(lo), at(hi));
    if ta <= tb {
        Shape::Line { start: if lo < ta { to_lo } else { start }, end: if hi > tb { to_hi } else { end } }
    } else {
        Shape::Line { start: if hi > ta { to_hi } else { start }, end: if lo < tb { to_lo } else { end } }
    }
}

/// Arcs and circles on one circle: a circle covers everything else on it; arcs combine around it.
fn combine_round(round: &[&Entity], settings: &OverkillSettings, cleanup: &mut Cleanup) {
    let tolerance = settings.tolerance;
    let circle_of = |e: &Entity| match e.shape {
        Shape::Circle { center, radius } | Shape::Arc { center, radius, .. } => (center, radius),
        _ => unreachable!(),
    };
    let mut sorted: Vec<&Entity> = round.to_vec();
    sorted.sort_by(|a, b| circle_of(a).0.x.total_cmp(&circle_of(b).0.x));
    let mut taken = vec![false; sorted.len()];
    for i in 0..sorted.len() {
        if taken[i] {
            continue;
        }
        let (center, radius) = circle_of(sorted[i]);
        let mut same: Vec<&Entity> = vec![];
        for j in i..sorted.len() {
            let (c, r) = circle_of(sorted[j]);
            if c.x - center.x > tolerance {
                break;
            }
            if !taken[j] && c.dist(center) <= tolerance && (r - radius).abs() <= tolerance {
                taken[j] = true;
                same.push(sorted[j]);
            }
        }
        if same.len() > 1 {
            combine_on_circle(&same, radius, settings, cleanup);
        }
    }
}

fn combine_on_circle(same: &[&Entity], radius: f64, settings: &OverkillSettings, cleanup: &mut Cleanup) {
    if let Some(circle) = same.iter().find(|e| matches!(e.shape, Shape::Circle { .. })) {
        for other in same.iter().filter(|e| e.id != circle.id) {
            if matches!(other.shape, Shape::Circle { .. }) {
                cleanup.duplicates += 1;
            } else {
                cleanup.overlaps += 1;
            }
            cleanup.remove.push((*other).clone());
        }
        return;
    }
    // Arcs, as spans of degrees from where each starts, the first of them between 0 and 360.
    let spans: Vec<Span> = same
        .iter()
        .map(|&entity| {
            let Shape::Arc { start_angle, end_angle, .. } = entity.shape else { unreachable!() };
            let lo = modulo(start_angle, 360.0);
            Span { lo, hi: lo + sweep_degrees(start_angle, end_angle), entity }
        })
        .collect();
    let degrees = (settings.tolerance / radius).to_degrees();
    let mut runs = merge_spans(spans, degrees, settings, cleanup);
    // The last run may reach on round past 360 into the first.
    if runs.len() > 1 {
        let (first_lo, first_hi) = (runs[0].lo + 360.0, runs[0].hi + 360.0);
        let last = runs.last().unwrap();
        let inside = first_hi <= last.hi + degrees;
        let overlapping = first_lo < last.hi - degrees;
        let touching = !overlapping && first_lo <= last.hi + degrees;
        if inside || (overlapping && settings.combine_partial) || (touching && settings.combine_end_to_end) {
            let first = runs.remove(0);
            let last = runs.last_mut().unwrap();
            cleanup.overlaps += 1;
            last.hi = last.hi.max(first_hi);
            for member in first.members {
                last.members.push(Span { lo: member.lo + 360.0, hi: member.hi + 360.0, ..member });
            }
            // The first run's own leftovers were already counted when it was merged.
        }
    }
    for run in &runs {
        let whole = run.hi - run.lo >= 360.0 - degrees;
        settle_run(
            run,
            |entity, lo, hi| match entity.shape {
                Shape::Arc { center, radius, .. } if whole => Shape::Circle { center, radius },
                Shape::Arc { center, radius, .. } => {
                    Shape::Arc { center, radius, start_angle: modulo(lo, 360.0), end_angle: modulo(hi, 360.0) }
                }
                _ => entity.shape.clone(),
            },
            cleanup,
        );
    }
}

/// Any other object with a copy lying on it: the later copies go.
fn delete_copies(rest: &[&Entity], tolerance: f64, cleanup: &mut Cleanup) {
    let mut sorted: Vec<(Point, &Entity)> = rest
        .iter()
        .map(|&e| {
            let b = entity_extent(e);
            (Point { x: (b.x0 + b.x1) / 2.0, y: (b.y0 + b.y1) / 2.0 }, e)
        })
        .collect();
    sorted.sort_by(|a, b| a.0.x.total_cmp(&b.0.x).then(a.1.id.cmp(&b.1.id)));
    let mut gone = vec![false; sorted.len()];
    for i in 0..sorted.len() {
        if gone[i] {
            continue;
        }
        for j in i + 1..sorted.len() {
            // Copies' extents differ by no more than the tolerance at each side.
            if sorted[j].0.x - sorted[i].0.x > tolerance * 2.0 {
                break;
            }
            if !gone[j] && same_object(sorted[i].1, sorted[j].1, tolerance) {
                gone[j] = true;
                cleanup.duplicates += 1;
                cleanup.remove.push(sorted[j].1.clone());
            }
        }
    }
}

/// Whether two objects are copies of one another, their points no further apart than the tolerance.
fn same_object(a: &Entity, b: &Entity, tolerance: f64) -> bool {
    if a.attribute != b.attribute {
        return false;
    }
    let near = |p: Point, q: Point| p.dist(q) <= tolerance;
    match (&a.shape, &b.shape) {
        (Shape::Point { position: p }, Shape::Point { position: q }) => near(*p, *q),
        (Shape::Polyline { vertices: v, closed: c }, Shape::Polyline { vertices: w, closed: d }) => {
            let vertex_alike = |x: &super::entities::Vertex, y: &super::entities::Vertex, bulge: f64| {
                near(x.point(), y.point())
                    && (x.bulge - bulge).abs() <= 1e-9
                    && (x.start_width - y.start_width).abs() <= tolerance
                    && (x.end_width - y.end_width).abs() <= tolerance
            };
            if c != d || v.len() != w.len() {
                return false;
            }
            let forward = v.iter().zip(w).all(|(x, y)| vertex_alike(x, y, y.bulge));
            // An open polyline drawn the other way round: each segment's bulge turns the other way.
            let backward = !c
                && v.iter().enumerate().all(|(i, x)| {
                    let y = &w[w.len() - 1 - i];
                    let bulge = if i + 1 < v.len() { -w[w.len() - 2 - i].bulge } else { x.bulge };
                    near(x.point(), y.point()) && (x.bulge - bulge).abs() <= 1e-9
                });
            forward || backward
        }
        (Shape::Text { position: p, .. }, Shape::Text { position: q, .. })
        | (Shape::Insert { position: p, .. }, Shape::Insert { position: q, .. }) => {
            // Everything but where they sit has to match exactly.
            near(*p, *q) && placed_at(&b.shape, *p) == a.shape
        }
        (x, y) => x == y,
    }
}

fn placed_at(shape: &Shape, at: Point) -> Shape {
    let mut shape = shape.clone();
    if let Shape::Text { position, .. } | Shape::Insert { position, .. } = &mut shape {
        *position = at;
    }
    shape
}

/// Cuts a sorted list into runs, between neighbours for which `apart` holds.
fn split_where<T>(items: &mut [T], apart: impl Fn(&T, &T) -> bool) -> Vec<&mut [T]> {
    let mut runs = vec![];
    let mut rest = items;
    while !rest.is_empty() {
        let cut = (1..rest.len()).find(|&i| apart(&rest[i - 1], &rest[i])).unwrap_or(rest.len());
        let (run, tail) = rest.split_at_mut(cut);
        runs.push(run);
        rest = tail;
    }
    runs
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::entities::{arc, circle, line, point, polyline_from, text};
    use crate::model::geometry::pt;

    fn run(entities: &[Entity]) -> Cleanup {
        overkill(entities, &OverkillSettings::default())
    }

    #[test]
    fn a_line_drawn_twice_and_backwards_is_one_line() {
        let a = line(pt(0.0, 0.0), pt(10.0, 0.0));
        let b = line(pt(10.0, 0.0), pt(0.0, 1e-9));
        let cleanup = run(&[a.clone(), b.clone()]);
        assert_eq!((cleanup.duplicates, cleanup.overlaps), (1, 0));
        assert_eq!(cleanup.remove.len(), 1);
        assert!(cleanup.modify.is_empty());
    }

    #[test]
    fn overlapping_lines_become_one_that_reaches_both_ends() {
        let a = line(pt(0.0, 0.0), pt(6.0, 6.0));
        let b = line(pt(10.0, 10.0), pt(4.0, 4.0));
        let inside = line(pt(1.0, 1.0), pt(2.0, 2.0));
        let cleanup = run(&[a.clone(), b, inside]);
        assert_eq!((cleanup.duplicates, cleanup.overlaps), (0, 2));
        assert_eq!(cleanup.remove.len(), 2);
        let Shape::Line { start, end } = cleanup.modify[0].shape else { panic!() };
        assert_eq!(cleanup.modify[0].id, a.id, "the first line stays and grows");
        assert!(start.dist(pt(0.0, 0.0)) < 1e-9 && end.dist(pt(10.0, 10.0)) < 1e-9, "{start:?} {end:?}");
    }

    #[test]
    fn lines_meeting_end_to_end_combine_only_when_asked() {
        let lines = [line(pt(0.0, 0.0), pt(5.0, 0.0)), line(pt(5.0, 0.0), pt(9.0, 0.0))];
        assert!(run(&lines).remove.is_empty());
        let settings = OverkillSettings { combine_end_to_end: true, ..OverkillSettings::default() };
        let cleanup = overkill(&lines, &settings);
        assert_eq!(cleanup.remove.len(), 1);
        assert_eq!(cleanup.modify[0].shape, Shape::Line { start: pt(0.0, 0.0), end: pt(9.0, 0.0) });
    }

    #[test]
    fn parallel_lines_and_other_layers_stay() {
        let a = line(pt(0.0, 0.0), pt(10.0, 0.0));
        let parallel = line(pt(0.0, 1.0), pt(10.0, 1.0));
        let mut elsewhere = a.duplicate();
        elsewhere.layer = "WALLS".into();
        assert!(run(&[a.clone(), parallel.clone(), elsewhere.clone()]).remove.is_empty());
        let settings = OverkillSettings { ignore_layer: true, ..OverkillSettings::default() };
        assert_eq!(overkill(&[a, parallel, elsewhere], &settings).duplicates, 1);
    }

    #[test]
    fn a_circle_covers_arcs_on_it_and_arcs_combine_round_zero() {
        let c = circle(pt(3.0, 3.0), 2.0);
        let on_it = arc(pt(3.0, 3.0), 2.0, 10.0, 80.0);
        let cleanup = run(&[on_it.clone(), c.clone(), c.duplicate()]);
        assert_eq!((cleanup.duplicates, cleanup.overlaps), (1, 1));
        assert!(cleanup.remove.iter().all(|e| e.id != c.id));

        let a = arc(pt(0.0, 0.0), 1.0, 300.0, 30.0);
        let b = arc(pt(0.0, 0.0), 1.0, 20.0, 90.0);
        let cleanup = run(&[a.clone(), b.clone()]);
        assert_eq!(cleanup.remove.len(), 1);
        let Shape::Arc { start_angle, end_angle, .. } = cleanup.modify[0].shape else { panic!("{:?}", cleanup.modify) };
        assert!((start_angle - 300.0).abs() < 1e-9 && (end_angle - 90.0).abs() < 1e-9, "{start_angle} {end_angle}");
    }

    #[test]
    fn arcs_that_close_the_circle_become_one() {
        let a = arc(pt(0.0, 0.0), 1.0, 0.0, 200.0);
        let b = arc(pt(0.0, 0.0), 1.0, 180.0, 10.0);
        let cleanup = run(&[a, b]);
        assert_eq!(cleanup.remove.len(), 1);
        assert_eq!(cleanup.modify[0].shape, Shape::Circle { center: pt(0.0, 0.0), radius: 1.0 });
    }

    #[test]
    fn copies_of_other_objects_go() {
        let p = polyline_from(&[pt(0.0, 0.0), pt(4.0, 0.0), pt(4.0, 3.0)], false);
        let reversed = polyline_from(&[pt(4.0, 3.0), pt(4.0, 0.0), pt(0.0, 0.0)], false);
        let label = text(pt(1.0, 1.0), "A", 2.5, 0.0);
        let other_label = text(pt(1.0, 1.0), "B", 2.5, 0.0);
        let dot = point(pt(9.0, 9.0));
        let entities = [p, reversed, label.clone(), label.duplicate(), other_label, dot.clone(), dot.duplicate()];
        let cleanup = run(&entities);
        assert_eq!((cleanup.duplicates, cleanup.overlaps), (3, 0));
    }
}
