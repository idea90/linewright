//! The area around a point that the drawing closes off, as HATCH's "pick internal point" finds it. The lines and
//! curves near the point are split where they cross or touch, loose ends are trimmed away, and the face of that
//! network holding the point is traced. Whatever lies wholly inside it comes along as islands.

use std::collections::{HashMap, HashSet};

use super::entities::{Entity, Shape};
use super::geometry::{Point, pt};
use super::index::{Bounds, entity_extent};
use super::parts::entity_parts;

/// The loops around a picked point: the outer boundary first, then the islands inside it.
#[derive(Clone, Debug, PartialEq)]
pub struct Region {
    pub outer: Vec<Point>,
    pub islands: Vec<Vec<Point>>,
}

impl Region {
    pub fn loops(self) -> Vec<Vec<Point>> {
        std::iter::once(self.outer).chain(self.islands).collect()
    }
}

/// How finely curves are cut into straight pieces, per size of the curve.
const CURVE_TOLERANCE: f64 = 1e-3;
/// Ends closer than this, per size of the area searched, count as touching.
const GAP_TOLERANCE: f64 = 1e-7;
/// The first area searched is this part of the drawing's size across; it grows fourfold until the region fits.
const FIRST_SEARCH: f64 = 1.0 / 256.0;
const SEARCH_GROWTH: f64 = 4.0;
/// The ray looking for the nearest boundary leaves at an angle no drawing lines up with, so it never runs along a
/// line or through a corner.
const RAY_ANGLE: f64 = 0.012_345_678_9;

/// The region around `point` in a drawing, given its extent and a way to find the objects in an area. Only lines,
/// arcs, circles, polylines, ellipses, splines and blocks bound it. The search starts small and widens until the
/// region found lies wholly inside the area searched, so far-off objects are never looked at.
pub fn region_in_drawing(point: Point, extent: Bounds, entities_in: impl Fn(&Bounds) -> Vec<Entity>) -> Option<Region> {
    let size = extent.diagonal().max(f64::MIN_POSITIVE);
    let mut radius = size * FIRST_SEARCH;
    loop {
        let area = Bounds::square_around(point, radius);
        let paths: Vec<Vec<Point>> = entities_in(&area).iter().filter(|e| bounds_area(e)).flat_map(boundary_paths).collect();
        let found = region_around(point, &paths, radius * GAP_TOLERANCE);
        let covers_drawing = area.x0 <= extent.x0 && area.y0 <= extent.y0 && area.x1 >= extent.x1 && area.y1 >= extent.y1;
        let inside = |region: &Region| {
            let b = Bounds::around(region.outer.iter().copied());
            b.x0 > area.x0 && b.y0 > area.y0 && b.x1 < area.x1 && b.y1 < area.y1
        };
        match found {
            Some(region) if covers_drawing || inside(&region) => return Some(region),
            _ if covers_drawing => return None,
            _ => radius *= SEARCH_GROWTH,
        }
    }
}

fn bounds_area(entity: &Entity) -> bool {
    !matches!(
        entity.shape,
        Shape::Text { .. } | Shape::Dimension { .. } | Shape::Annotation { .. } | Shape::Hatch { .. } | Shape::Point { .. }
    )
}

fn boundary_paths(entity: &Entity) -> Vec<Vec<Point>> {
    let size = entity_extent(entity).diagonal();
    entity_parts(entity, (size * CURVE_TOLERANCE).max(f64::MIN_POSITIVE)).paths
}

/// The region of these paths' network that holds `point`, or None when the point is not closed in. Ends closer
/// than `tolerance` join.
pub fn region_around(point: Point, paths: &[Vec<Point>], tolerance: f64) -> Option<Region> {
    // Work about the picked point, where coordinates are small and precise.
    let local: Vec<(Point, Point)> = paths
        .iter()
        .flat_map(|path| path.windows(2).map(|w| (w[0] - point, w[1] - point)))
        .filter(|(a, b)| a.dist(*b) > tolerance && a.is_finite() && b.is_finite())
        .collect();
    let graph = Graph::build(&local, tolerance.max(f64::MIN_POSITIVE));
    let region = graph.region_around_origin()?;
    let back = |loop_: Vec<Point>| loop_.into_iter().map(|p| p + point).collect();
    Some(Region { outer: back(region.outer), islands: region.islands.into_iter().map(back).collect() })
}

/// The planar network: nodes, and each edge as two half-edges running opposite ways (2k and 2k+1).
struct Graph {
    nodes: Vec<Point>,
    from: Vec<usize>,
    /// Each node's outgoing half-edges, counter-clockwise by direction.
    outgoing: Vec<Vec<usize>>,
    /// Where each half-edge sits in its node's `outgoing`.
    slot: Vec<usize>,
}

impl Graph {
    fn build(segments: &[(Point, Point)], tolerance: f64) -> Graph {
        let cuts = split_parameters(segments, tolerance);
        let mut nodes = Nodes::new(tolerance);
        let mut edges: HashSet<(usize, usize)> = HashSet::new();
        for (&(a, b), ts) in segments.iter().zip(&cuts) {
            let ids: Vec<usize> = ts.iter().map(|t| nodes.id(a + (b - a) * *t)).collect();
            for w in ids.windows(2) {
                if w[0] != w[1] {
                    edges.insert((w[0].min(w[1]), w[0].max(w[1])));
                }
            }
        }
        let points = nodes.points;
        let mut edges: Vec<(usize, usize)> = prune_loose_ends(points.len(), edges);
        edges.sort_unstable();
        let mut from = Vec::with_capacity(edges.len() * 2);
        for &(a, b) in &edges {
            from.extend([a, b]);
        }
        let mut outgoing: Vec<Vec<usize>> = vec![vec![]; points.len()];
        for (h, &f) in from.iter().enumerate() {
            outgoing[f].push(h);
        }
        let to = |h: usize| from[h ^ 1];
        for list in &mut outgoing {
            list.sort_by(|&g, &h| {
                let angle = |e: usize| {
                    let d = points[to(e)] - points[from[e]];
                    d.y.atan2(d.x)
                };
                angle(g).total_cmp(&angle(h))
            });
        }
        let mut slot = vec![0; from.len()];
        for list in &outgoing {
            for (i, &h) in list.iter().enumerate() {
                slot[h] = i;
            }
        }
        Graph { nodes: points, from, outgoing, slot }
    }

    fn to(&self, h: usize) -> usize {
        self.from[h ^ 1]
    }

    /// The half-edge after `h` around the face on its left.
    fn next(&self, h: usize) -> usize {
        let back = h ^ 1;
        let list = &self.outgoing[self.from[back]];
        list[(self.slot[back] + list.len() - 1) % list.len()]
    }

    /// The face on the left of `h`, as its corners, and the half-edges it uses.
    fn face(&self, start: usize) -> (Vec<Point>, Vec<usize>) {
        let (mut corners, mut used) = (vec![], vec![]);
        let mut h = start;
        loop {
            corners.push(self.nodes[self.from[h]]);
            used.push(h);
            h = self.next(h);
            if h == start || used.len() > self.from.len() {
                return (corners, used);
            }
        }
    }

    /// Which connected piece of the network each node belongs to.
    fn components(&self) -> Vec<usize> {
        let mut parent: Vec<usize> = (0..self.nodes.len()).collect();
        fn find(parent: &mut [usize], mut i: usize) -> usize {
            while parent[i] != i {
                parent[i] = parent[parent[i]];
                i = parent[i];
            }
            i
        }
        for h in (0..self.from.len()).step_by(2) {
            let (a, b) = (find(&mut parent, self.from[h]), find(&mut parent, self.to(h)));
            parent[a] = b;
        }
        (0..self.nodes.len()).map(|i| find(&mut parent, i)).collect()
    }

    /// The outline of one connected piece: its outer face, which runs clockwise.
    fn outline(&self, component: usize, components: &[usize]) -> Vec<Point> {
        let mut seen: HashSet<usize> = HashSet::new();
        let mut outline: (f64, Vec<Point>) = (0.0, vec![]);
        for h in (0..self.from.len()).filter(|&h| components[self.from[h]] == component) {
            if seen.contains(&h) {
                continue;
            }
            let (corners, used) = self.face(h);
            seen.extend(used);
            let area = signed_area(&corners);
            if area < outline.0 {
                outline = (area, corners);
            }
        }
        outline.1
    }

    /// The face holding the origin, found by following a ray out from it to the first edge whose face closes
    /// around the origin. Pieces the ray meets first that do not close around it are islands.
    fn region_around_origin(&self) -> Option<Region> {
        let ray = pt(RAY_ANGLE.cos(), RAY_ANGLE.sin());
        let mut hits: Vec<(f64, usize)> = (0..self.from.len())
            .step_by(2)
            .filter_map(|h| {
                let (a, b) = (self.nodes[self.from[h]], self.nodes[self.to(h)]);
                let edge = b - a;
                let denominator = ray.cross(edge);
                if denominator == 0.0 {
                    return None;
                }
                let along_ray = a.cross(edge) / denominator;
                let along_edge = a.cross(ray) / denominator;
                (along_ray > 0.0 && (0.0..1.0).contains(&along_edge)).then_some((along_ray, h))
            })
            .collect();
        hits.sort_by(|x, y| x.0.total_cmp(&y.0));
        let components = self.components();
        let mut outside_of: HashSet<usize> = HashSet::new();
        let (outer, outer_component) = hits.iter().find_map(|&(_, h)| {
            let component = components[self.from[h]];
            if outside_of.contains(&component) {
                return None;
            }
            let (a, b) = (self.nodes[self.from[h]], self.nodes[self.to(h)]);
            let left = if (b - a).cross(pt(0.0, 0.0) - a) > 0.0 { h } else { h ^ 1 };
            let (corners, _) = self.face(left);
            if signed_area(&corners) > 0.0 && contains(&corners, pt(0.0, 0.0)) {
                Some((corners, component))
            } else {
                outside_of.insert(component);
                None
            }
        })?;
        let mut firsts: HashMap<usize, usize> = HashMap::new();
        for (node, &component) in components.iter().enumerate() {
            if component != outer_component && !self.outgoing[node].is_empty() {
                firsts.entry(component).or_insert(node);
            }
        }
        let mut inner: Vec<(usize, usize)> = firsts.into_iter().collect();
        inner.sort_unstable();
        let islands = inner
            .into_iter()
            .filter(|&(_, node)| contains(&outer, self.nodes[node]))
            .map(|(component, _)| self.outline(component, &components))
            .filter(|outline| outline.len() >= 3)
            .collect();
        Some(Region { outer, islands })
    }
}

/// Snaps points closer than the tolerance onto one node.
struct Nodes {
    tolerance: f64,
    cells: HashMap<(i64, i64), Vec<usize>>,
    points: Vec<Point>,
}

impl Nodes {
    fn new(tolerance: f64) -> Nodes {
        Nodes { tolerance, cells: HashMap::new(), points: vec![] }
    }

    fn cell(&self, p: Point) -> (i64, i64) {
        ((p.x / self.tolerance).floor() as i64, (p.y / self.tolerance).floor() as i64)
    }

    fn id(&mut self, p: Point) -> usize {
        let (cx, cy) = self.cell(p);
        for dx in -1..=1 {
            for dy in -1..=1 {
                if let Some(ids) = self.cells.get(&(cx + dx, cy + dy))
                    && let Some(&id) = ids.iter().find(|&&id| self.points[id].dist(p) <= self.tolerance)
                {
                    return id;
                }
            }
        }
        self.points.push(p);
        let id = self.points.len() - 1;
        self.cells.entry((cx, cy)).or_default().push(id);
        id
    }
}

/// Where along each segment (0 to 1) it meets another, ends included, in order.
fn split_parameters(segments: &[(Point, Point)], tolerance: f64) -> Vec<Vec<f64>> {
    let mut cuts: Vec<Vec<f64>> = vec![vec![0.0, 1.0]; segments.len()];
    if segments.is_empty() {
        return cuts;
    }
    let bounds = Bounds::around(segments.iter().flat_map(|(a, b)| [*a, *b]));
    let cell = (bounds.diagonal() / (segments.len() as f64).sqrt()).max(tolerance * 16.0).max(f64::MIN_POSITIVE);
    let key = |v: f64, origin: f64| ((v - origin) / cell).floor() as i64;
    let mut grid: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    for (i, (a, b)) in segments.iter().enumerate() {
        let (x0, x1) = (key(a.x.min(b.x) - tolerance, bounds.x0), key(a.x.max(b.x) + tolerance, bounds.x0));
        let (y0, y1) = (key(a.y.min(b.y) - tolerance, bounds.y0), key(a.y.max(b.y) + tolerance, bounds.y0));
        for x in x0..=x1 {
            for y in y0..=y1 {
                grid.entry((x, y)).or_default().push(i);
            }
        }
    }
    let mut tested: HashSet<(usize, usize)> = HashSet::new();
    for members in grid.values() {
        for (k, &i) in members.iter().enumerate() {
            for &j in &members[k + 1..] {
                if !tested.insert((i.min(j), i.max(j))) {
                    continue;
                }
                for (t, u) in meetings(segments[i], segments[j], tolerance) {
                    cuts[i].extend(t);
                    cuts[j].extend(u);
                }
            }
        }
    }
    for ts in &mut cuts {
        ts.sort_by(f64::total_cmp);
        ts.dedup();
    }
    cuts
}

/// Where two segments meet, as parameters along each: a crossing, or an end of one lying on the other.
fn meetings(first: (Point, Point), second: (Point, Point), tolerance: f64) -> Vec<(Option<f64>, Option<f64>)> {
    let (a, b) = first;
    let (c, d) = second;
    let (r, s) = (b - a, d - c);
    let mut found = vec![];
    let denominator = r.cross(s);
    if denominator.abs() > f64::EPSILON * r.length() * s.length() {
        let t = (c - a).cross(s) / denominator;
        let u = (c - a).cross(r) / denominator;
        let (slack_t, slack_u) = (tolerance / r.length(), tolerance / s.length());
        if (-slack_t..=1.0 + slack_t).contains(&t) && (-slack_u..=1.0 + slack_u).contains(&u) {
            found.push((Some(t.clamp(0.0, 1.0)), Some(u.clamp(0.0, 1.0))));
        }
    }
    let on = |p: Point, (start, end): (Point, Point)| -> Option<f64> {
        let run = end - start;
        let t = ((p - start).dot(run) / run.dot(run)).clamp(0.0, 1.0);
        ((start + run * t).dist(p) <= tolerance && t > 0.0 && t < 1.0).then_some(t)
    };
    for p in [c, d] {
        found.extend(on(p, first).map(|t| (Some(t), None)));
    }
    for p in [a, b] {
        found.extend(on(p, second).map(|u| (None, Some(u))));
    }
    found
}

/// Take away edges that lead nowhere, again and again, so only edges on closed paths remain.
fn prune_loose_ends(node_count: usize, edges: HashSet<(usize, usize)>) -> Vec<(usize, usize)> {
    let mut degree = vec![0usize; node_count];
    let mut touching: Vec<Vec<(usize, usize)>> = vec![vec![]; node_count];
    for &(a, b) in &edges {
        degree[a] += 1;
        degree[b] += 1;
        touching[a].push((a, b));
        touching[b].push((a, b));
    }
    let mut alive = edges;
    let mut ends: Vec<usize> = (0..node_count).filter(|&n| degree[n] == 1).collect();
    while let Some(node) = ends.pop() {
        for edge in std::mem::take(&mut touching[node]) {
            if alive.remove(&edge) {
                for end in [edge.0, edge.1] {
                    degree[end] -= 1;
                    if degree[end] == 1 {
                        ends.push(end);
                    }
                }
            }
        }
    }
    alive.into_iter().collect()
}

/// Positive when the corners run counter-clockwise.
fn signed_area(corners: &[Point]) -> f64 {
    let n = corners.len();
    (0..n).map(|i| corners[i].cross(corners[(i + 1) % n])).sum::<f64>() / 2.0
}

fn contains(corners: &[Point], p: Point) -> bool {
    let mut inside = false;
    let n = corners.len();
    for i in 0..n {
        let (a, b) = (corners[i], corners[(i + n - 1) % n]);
        if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (b.x - a.x) * (p.y - a.y) / (b.y - a.y) {
            inside = !inside;
        }
    }
    inside
}

/// The new states of associative hatches after objects changed (`ids`, at `areas`): those made from closed objects
/// take their new outlines; those from points inside areas find their areas again. A hatch whose boundary is gone
/// keeps its loops and stops following. A hatch that changed itself has moved along already.
pub fn hatches_following(
    document: &crate::model::document::Document,
    ids: &[crate::model::entities::EntityId],
    areas: &[Bounds],
) -> Vec<Entity> {
    use crate::model::entities::Association;
    let mut states = vec![];
    for entity in document.entities() {
        let Shape::Hatch { association: Some(association), loops, .. } = &entity.shape else { continue };
        if ids.contains(&entity.id) {
            continue;
        }
        let new_loops: Option<Vec<Vec<Point>>> = match association {
            Association::Objects(members) => {
                if !members.iter().any(|m| ids.contains(m)) {
                    continue;
                }
                members
                    .iter()
                    .map(|m| document.get(*m).filter(|e| document.contains(e.id)).and_then(crate::model::editing::closed_loop))
                    .collect()
            }
            Association::Seeds(seeds) => {
                let bounds = Bounds::around(loops.iter().flatten().copied());
                if !areas.iter().any(|a| a.touches(&bounds)) {
                    continue;
                }
                let Some(extent) = document.extent() else { continue };
                let visible = |area: &Bounds| -> Vec<Entity> {
                    document
                        .entities_in(area)
                        .into_iter()
                        .filter(|e| document.is_visible(e) && !matches!(e.shape, Shape::Hatch { .. }))
                        .collect()
                };
                let found: Vec<Vec<Vec<Point>>> = seeds
                    .iter()
                    .filter_map(|seed| region_in_drawing(*seed, extent, visible).map(Region::loops))
                    .filter(|l| !l.is_empty())
                    .collect();
                (found.len() == seeds.len()).then(|| found.into_iter().flatten().collect())
            }
        };
        let mut state = entity.clone();
        if let Shape::Hatch { loops, association, .. } = &mut state.shape {
            match new_loops {
                Some(new_loops) if !new_loops.is_empty() => *loops = new_loops,
                _ => *association = None,
            }
        }
        if state.shape != entity.shape {
            states.push(state);
        }
    }
    states
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(x: f64, y: f64, size: f64) -> Vec<Point> {
        vec![pt(x, y), pt(x + size, y), pt(x + size, y + size), pt(x, y + size), pt(x, y)]
    }

    fn area(corners: &[Point]) -> f64 {
        signed_area(corners).abs()
    }

    #[test]
    fn lines_that_cross_close_off_the_area_between_them() {
        // A # shape: the middle square is closed in by four long crossing lines.
        let paths = vec![
            vec![pt(-5.0, 0.0), pt(15.0, 0.0)],
            vec![pt(-5.0, 10.0), pt(15.0, 10.0)],
            vec![pt(0.0, -5.0), pt(0.0, 15.0)],
            vec![pt(10.0, -5.0), pt(10.0, 15.0)],
        ];
        let region = region_around(pt(5.0, 5.0), &paths, 1e-9).unwrap();
        assert!((area(&region.outer) - 100.0).abs() < 1e-9);
        assert!(region.islands.is_empty());
        assert!(region_around(pt(20.0, 5.0), &paths, 1e-9).is_none());
    }

    #[test]
    fn rooms_side_by_side_pick_one_and_islands_stay_out() {
        // Two rooms sharing a wall; the right one holds a column.
        let paths = vec![
            vec![pt(0.0, 0.0), pt(20.0, 0.0), pt(20.0, 10.0), pt(0.0, 10.0), pt(0.0, 0.0)],
            vec![pt(10.0, 0.0), pt(10.0, 10.0)],
            square(14.0, 4.0, 2.0),
        ];
        let left = region_around(pt(5.0, 5.0), &paths, 1e-9).unwrap();
        assert!((area(&left.outer) - 100.0).abs() < 1e-9);
        assert!(left.islands.is_empty());
        let right = region_around(pt(12.0, 2.0), &paths, 1e-9).unwrap();
        assert!((area(&right.outer) - 100.0).abs() < 1e-9);
        assert_eq!(right.islands.len(), 1);
        assert!((area(&right.islands[0]) - 4.0).abs() < 1e-9);
        // Inside the column, the column itself is the region.
        let column = region_around(pt(15.0, 5.0), &paths, 1e-9).unwrap();
        assert!((area(&column.outer) - 4.0).abs() < 1e-9);
    }

    #[test]
    fn a_wall_ending_short_of_the_far_wall_does_not_split_the_room() {
        let paths = vec![square(0.0, 0.0, 10.0), vec![pt(5.0, 0.0), pt(5.0, 6.0)]];
        let region = region_around(pt(2.0, 5.0), &paths, 1e-9).unwrap();
        assert!((area(&region.outer) - 100.0).abs() < 1e-9);
    }

    #[test]
    fn far_from_the_origin_the_search_widens_until_the_region_fits() {
        let base = pt(239_280.0, 1_989_570.0);
        let room = Entity::new(Shape::Polyline {
            vertices: square(base.x, base.y, 30.0)
                .iter()
                .map(|p| super::super::entities::Vertex { x: p.x, y: p.y, bulge: 0.0, start_width: 0.0, end_width: 0.0 })
                .collect(),
            closed: true,
        });
        let far = super::super::entities::circle(pt(base.x + 5000.0, base.y), 1.0);
        let extent = Bounds::around([base, pt(base.x + 5001.0, base.y + 30.0)]);
        let all = [room, far];
        let region =
            region_in_drawing(base + pt(3.0, 3.0), extent, |area| all.iter().filter(|e| entity_extent(e).touches(area)).cloned().collect())
                .unwrap();
        assert!((area(&region.outer) - 900.0).abs() < 1e-6);
    }
}
