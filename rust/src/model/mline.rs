//! Multilines: several parallel lines drawn along one path, as an MLINESTYLE sets them out.

use std::f64::consts::PI;

use super::entities::BYLAYER;
use super::geometry::{Point, pt};
use super::parts::arc_points;
use super::styles::BYLAYER_LINETYPE;

/// One of a multiline style's lines: how far it lies left of the path at scale 1, and how it looks.
#[derive(Clone, Debug, PartialEq)]
pub struct MlineElement {
    pub offset: f64,
    pub color: i32,
    pub linetype: String,
}

/// A multiline style: its lines, and whether its ends and the joints between segments are closed off with lines
/// across, rounded or filled in.
#[derive(Clone, Debug, PartialEq)]
pub struct MlineStyle {
    pub name: String,
    pub description: String,
    pub elements: Vec<MlineElement>,
    /// The color the space between the outermost lines is filled with, when the style fills it.
    pub fill: Option<i32>,
    pub start_cap: bool,
    pub end_cap: bool,
    /// A half circle closing the start with the outermost two lines, bulging out past the end.
    pub start_round: bool,
    pub end_round: bool,
    /// A half circle joining the next pair of lines inward at the start, bulging back into the multiline.
    pub start_inner_arcs: bool,
    pub end_inner_arcs: bool,
    pub joints: bool,
}

impl MlineStyle {
    /// AutoCAD's STANDARD style: two lines a unit apart, without caps.
    pub fn standard() -> MlineStyle {
        let element = |offset| MlineElement { offset, color: BYLAYER, linetype: BYLAYER_LINETYPE.to_string() };
        MlineStyle {
            name: "Standard".into(),
            description: String::new(),
            elements: vec![element(0.5), element(-0.5)],
            fill: None,
            start_cap: false,
            end_cap: false,
            start_round: false,
            end_round: false,
            start_inner_arcs: false,
            end_inner_arcs: false,
            joints: false,
        }
    }

    fn offsets(&self) -> impl Iterator<Item = f64> + '_ {
        self.elements.iter().map(|e| e.offset)
    }
}

/// Which of a multiline's lines runs along the points it was drawn through: the top one, the middle (offset
/// zero) or the bottom one. Top is AutoCAD's default.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Justification {
    #[default]
    Top,
    Zero,
    Bottom,
}

impl Justification {
    /// The DXF code 70 value.
    pub fn code(self) -> i64 {
        match self {
            Justification::Top => 0,
            Justification::Zero => 1,
            Justification::Bottom => 2,
        }
    }

    pub fn from_code(code: i64) -> Justification {
        match code {
            1 => Justification::Zero,
            2 => Justification::Bottom,
            _ => Justification::Top,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Justification::Top => "Top",
            Justification::Zero => "Zero",
            Justification::Bottom => "Bottom",
        }
    }
}

/// The lines a multiline draws: for each element, its path (closed back to its start when the multiline is),
/// then the caps, rounds, inner arcs and joints across, which take the multiline's own look (element None).
pub fn mline_paths(
    vertices: &[Point],
    closed: bool,
    justification: Justification,
    scale: f64,
    style: &MlineStyle,
) -> Vec<(Option<usize>, Vec<Point>)> {
    let points: Vec<Point> = dedup(vertices);
    if points.len() < 2 || style.elements.is_empty() {
        return vec![];
    }
    let closed = closed && points.len() > 2;
    let distances = element_distances(justification, scale, style);
    let miters = miters(&points, closed);
    let mut out: Vec<(Option<usize>, Vec<Point>)> =
        distances.iter().enumerate().map(|(i, d)| (Some(i), along_path(&points, &miters, closed, *d))).collect();
    let (high, low) = extremes(&distances);
    let across = |at: usize| vec![points[at] + miters[at] * high, points[at] + miters[at] * low];
    if high > low {
        if !closed && style.start_cap {
            out.push((None, across(0)));
        }
        if !closed && style.end_cap {
            out.push((None, across(points.len() - 1)));
        }
        if style.joints {
            let inner = if closed { 0..points.len() } else { 1..points.len() - 1 };
            out.extend(inner.map(|i| (None, across(i))));
        }
        if !closed {
            out.extend(end_arcs(&points, &miters, &distances, style));
        }
    }
    out
}

/// The fill of a multiline whose style has one: the outermost two lines joined into one loop, going out along one
/// and back along the other. A closed multiline gives two loops, the outer one and the inner one as a hole.
/// Empty when the style fills nothing.
pub fn fill_loops(vertices: &[Point], closed: bool, justification: Justification, scale: f64, style: &MlineStyle) -> Vec<Vec<Point>> {
    let points: Vec<Point> = dedup(vertices);
    if style.fill.is_none() || points.len() < 2 || style.elements.is_empty() {
        return vec![];
    }
    let closed = closed && points.len() > 2;
    let distances = element_distances(justification, scale, style);
    let (high, low) = extremes(&distances);
    if high <= low {
        return vec![];
    }
    let miters = miters(&points, closed);
    let outer = along_path(&points, &miters, closed, high);
    let inner = along_path(&points, &miters, closed, low);
    if closed {
        // Each loop is closed by the first point, so the repeat at the end goes.
        let mut loops: Vec<Vec<Point>> = [outer, inner]
            .into_iter()
            .map(|mut l| {
                l.pop();
                l
            })
            .collect();
        if area(&loops[1]) > area(&loops[0]) {
            loops.swap(0, 1);
        }
        loops
    } else {
        let mut joined = outer;
        joined.extend(inner.into_iter().rev());
        vec![joined]
    }
}

/// The path of one element, `distance` to the left of the points, closed back to its start when the multiline is.
fn along_path(points: &[Point], miters: &[Point], closed: bool, distance: f64) -> Vec<Point> {
    let mut path: Vec<Point> = points.iter().zip(miters).map(|(p, m)| *p + *m * distance).collect();
    if closed {
        path.push(path[0]);
    }
    path
}

/// The highest and lowest of the element distances.
fn extremes(distances: &[f64]) -> (f64, f64) {
    (distances.iter().copied().fold(f64::NEG_INFINITY, f64::max), distances.iter().copied().fold(f64::INFINITY, f64::min))
}

/// The absolute area inside a loop of points.
fn area(loop_: &[Point]) -> f64 {
    let n = loop_.len();
    let twice: f64 = (0..n).map(|i| loop_[i].x * loop_[(i + 1) % n].y - loop_[(i + 1) % n].x * loop_[i].y).sum();
    twice.abs() / 2.0
}

/// The round caps and inner arcs of an open multiline, each a half circle across one of its ends.
fn end_arcs(points: &[Point], miters: &[Point], distances: &[f64], style: &MlineStyle) -> Vec<(Option<usize>, Vec<Point>)> {
    let last = points.len() - 1;
    let mut sorted = distances.to_vec();
    sorted.sort_by(|a, b| b.total_cmp(a));
    let n = sorted.len();
    let (high, low) = (sorted[0], sorted[n - 1]);
    let start = unit(points[1] - points[0]);
    let end = unit(points[last] - points[last - 1]);
    let mut out = vec![];
    if style.start_round {
        out.push((None, half_circle(points[0], miters[0], high, low, start * -1.0)));
    }
    if style.end_round {
        out.push((None, half_circle(points[last], miters[last], high, low, end)));
    }
    // Pairs from the second and second-from-last lines inward, a middle line left out when the count is odd.
    for i in (1..n).take_while(|i| *i < n - 1 - i) {
        let (outer, inner) = (sorted[i], sorted[n - 1 - i]);
        if outer - inner <= 1e-12 {
            continue;
        }
        if style.start_inner_arcs {
            out.push((None, half_circle(points[0], miters[0], outer, inner, start)));
        }
        if style.end_inner_arcs {
            out.push((None, half_circle(points[last], miters[last], outer, inner, end * -1.0)));
        }
    }
    out
}

/// A half circle through the lines at `from` and `to` across the end at `end`, bulging along `outward`.
fn half_circle(end: Point, miter: Point, from: f64, to: f64, outward: Point) -> Vec<Point> {
    let center = end + miter * ((from + to) / 2.0);
    let radius = (from - to).abs() / 2.0;
    let start = miter * ((from - to) / 2.0);
    // The arc passes a quarter turn from its start, and it bulges the way that quarter turn points.
    let quarter = pt(-start.y, start.x);
    let turn = if outward.dot(quarter) > 0.0 { PI } else { -PI };
    arc_points(center, radius, start.y.atan2(start.x), turn, radius * 0.01)
}

/// A vector of length one in the same direction.
fn unit(v: Point) -> Point {
    v * (1.0 / v.length())
}

/// Each element's distance left of the path, in drawing units, for this justification and scale.
pub fn element_distances(justification: Justification, scale: f64, style: &MlineStyle) -> Vec<f64> {
    let shift = match justification {
        Justification::Top => style.offsets().fold(f64::NEG_INFINITY, f64::max),
        Justification::Zero => 0.0,
        Justification::Bottom => style.offsets().fold(f64::INFINITY, f64::min),
    };
    style.offsets().map(|o| (o - shift) * scale).collect()
}

/// At each vertex, the vector that moves a point one unit of offset to the left of the path: the segment's own
/// normal at an open end, and along the bisector at a corner, lengthened so both segments' lines meet on it.
pub fn miters(points: &[Point], closed: bool) -> Vec<Point> {
    let n = points.len();
    let normal = |a: Point, b: Point| {
        let d = b - a;
        let length = d.length().max(f64::MIN_POSITIVE);
        Point { x: -d.y / length, y: d.x / length }
    };
    (0..n)
        .map(|i| {
            let before = if i > 0 {
                Some(normal(points[i - 1], points[i]))
            } else if closed {
                Some(normal(points[n - 1], points[0]))
            } else {
                None
            };
            let after = if i + 1 < n {
                Some(normal(points[i], points[i + 1]))
            } else if closed {
                Some(normal(points[i], points[0]))
            } else {
                None
            };
            match (before, after) {
                (Some(a), Some(b)) => {
                    let sum = a + b;
                    let length = sum.length();
                    // Doubling straight back: no corner to meet at, so the lines turn about the end.
                    if length < 1e-9 {
                        return a;
                    }
                    let bisector = sum * (1.0 / length);
                    bisector * (1.0 / bisector.dot(a))
                }
                (Some(a), None) | (None, Some(a)) => a,
                (None, None) => Point { x: 0.0, y: 0.0 },
            }
        })
        .collect()
}

/// The points without repeats of the one before, which have no direction to offset along.
pub fn dedup(points: &[Point]) -> Vec<Point> {
    let mut out: Vec<Point> = vec![];
    for p in points {
        if out.last().is_none_or(|last| last.dist(*p) > 1e-12) {
            out.push(*p);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::geometry::pt;

    fn close(a: Point, b: Point) -> bool {
        a.dist(b) < 1e-9
    }

    #[test]
    fn top_justified_lines_hang_below_a_path_drawn_left_to_right() {
        let paths = mline_paths(&[pt(0.0, 0.0), pt(10.0, 0.0)], false, Justification::Top, 20.0, &MlineStyle::standard());
        assert_eq!(paths.len(), 2);
        assert!(close(paths[0].1[0], pt(0.0, 0.0)) && close(paths[0].1[1], pt(10.0, 0.0)));
        assert!(close(paths[1].1[0], pt(0.0, -20.0)) && close(paths[1].1[1], pt(10.0, -20.0)));
        let zero = mline_paths(&[pt(0.0, 0.0), pt(10.0, 0.0)], false, Justification::Zero, 20.0, &MlineStyle::standard());
        assert!(close(zero[0].1[0], pt(0.0, 10.0)) && close(zero[1].1[0], pt(0.0, -10.0)));
    }

    #[test]
    fn corners_meet_on_the_miter() {
        let path = [pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 10.0)];
        let paths = mline_paths(&path, false, Justification::Bottom, 2.0, &MlineStyle::standard());
        // Bottom justified: the top line lies two units to the left of the path, inside the left turn.
        assert!(close(paths[0].1[1], pt(8.0, 2.0)), "{:?}", paths[0].1);
        assert!(close(paths[1].1[1], pt(10.0, 0.0)));
    }

    #[test]
    fn a_closed_multiline_comes_round_and_caps_and_joints_draw_across() {
        let square = [pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 10.0), pt(0.0, 10.0)];
        let paths = mline_paths(&square, true, Justification::Zero, 2.0, &MlineStyle::standard());
        assert_eq!(paths[0].1.len(), 5);
        assert!(close(paths[0].1[0], pt(1.0, 1.0)), "inside corner: {:?}", paths[0].1[0]);
        assert!(close(paths[1].1[0], pt(-1.0, -1.0)), "outside corner: {:?}", paths[1].1[0]);
        let capped = MlineStyle { start_cap: true, end_cap: true, joints: true, ..MlineStyle::standard() };
        let open = mline_paths(&square[..3], false, Justification::Zero, 2.0, &capped);
        assert_eq!(open.iter().filter(|(element, _)| element.is_none()).count(), 3, "two caps and one joint");
    }

    fn rounded(start: bool, end: bool) -> MlineStyle {
        MlineStyle { start_round: start, end_round: end, ..MlineStyle::standard() }
    }

    fn with_offsets(offsets: &[f64]) -> MlineStyle {
        let elements = offsets.iter().map(|&offset| MlineElement { offset, color: BYLAYER, linetype: BYLAYER_LINETYPE.to_string() });
        MlineStyle { elements: elements.collect(), ..MlineStyle::standard() }
    }

    #[test]
    fn a_round_cap_joins_the_outer_lines_and_reaches_half_the_width_past_the_end() {
        let line = [pt(0.0, 0.0), pt(10.0, 0.0)];
        let paths = mline_paths(&line, false, Justification::Zero, 2.0, &rounded(true, true));
        let arcs: Vec<&Vec<Point>> = paths.iter().filter(|(e, _)| e.is_none()).map(|(_, p)| p).collect();
        assert_eq!(arcs.len(), 2);
        let start = arcs[0];
        assert!(close(start[0], pt(0.0, 1.0)) && close(*start.last().unwrap(), pt(0.0, -1.0)), "{start:?}");
        // The width is 2, so the cap bulges one unit out past the start, away from the path.
        assert!(start.iter().any(|p| close(*p, pt(-1.0, 0.0))), "{start:?}");
        let end = arcs[1];
        assert!(close(end[0], pt(10.0, 1.0)) && close(*end.last().unwrap(), pt(10.0, -1.0)), "{end:?}");
        assert!(end.iter().any(|p| close(*p, pt(11.0, 0.0))), "{end:?}");
    }

    #[test]
    fn a_closed_multiline_has_no_round_caps() {
        let square = [pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 10.0), pt(0.0, 10.0)];
        let paths = mline_paths(&square, true, Justification::Zero, 2.0, &rounded(true, true));
        assert_eq!(paths.len(), 2);
    }

    #[test]
    fn four_lines_give_one_inner_arc_joining_the_middle_pair() {
        let style = MlineStyle { start_inner_arcs: true, ..with_offsets(&[1.5, 0.5, -0.5, -1.5]) };
        let paths = mline_paths(&[pt(0.0, 0.0), pt(10.0, 0.0)], false, Justification::Zero, 1.0, &style);
        let arcs: Vec<&Vec<Point>> = paths.iter().filter(|(e, _)| e.is_none()).map(|(_, p)| p).collect();
        assert_eq!(arcs.len(), 1);
        assert!(close(arcs[0][0], pt(0.0, 0.5)) && close(*arcs[0].last().unwrap(), pt(0.0, -0.5)), "{:?}", arcs[0]);
        // The arc bulges back into the multiline, so its middle is half a unit along from the end.
        assert!(arcs[0].iter().any(|p| close(*p, pt(0.5, 0.0))), "{:?}", arcs[0]);
    }

    #[test]
    fn three_lines_have_no_inner_arcs() {
        let style = MlineStyle { start_inner_arcs: true, end_inner_arcs: true, ..with_offsets(&[1.0, 0.0, -1.0]) };
        let paths = mline_paths(&[pt(0.0, 0.0), pt(10.0, 0.0)], false, Justification::Zero, 1.0, &style);
        assert_eq!(paths.len(), 3);
    }

    #[test]
    fn an_open_multiline_fills_the_outer_lines_as_one_loop() {
        let style = MlineStyle { fill: Some(3), ..MlineStyle::standard() };
        let loops = fill_loops(&[pt(0.0, 0.0), pt(10.0, 0.0)], false, Justification::Zero, 2.0, &style);
        assert_eq!(loops, vec![vec![pt(0.0, 1.0), pt(10.0, 1.0), pt(10.0, -1.0), pt(0.0, -1.0)]]);
        let unfilled = MlineStyle { fill: None, ..MlineStyle::standard() };
        assert!(fill_loops(&[pt(0.0, 0.0), pt(10.0, 0.0)], false, Justification::Zero, 2.0, &unfilled).is_empty());
    }

    #[test]
    fn a_closed_multiline_fills_a_ring_with_its_inside_as_a_hole() {
        let style = MlineStyle { fill: Some(3), ..MlineStyle::standard() };
        let square = [pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 10.0), pt(0.0, 10.0)];
        let loops = fill_loops(&square, true, Justification::Zero, 2.0, &style);
        assert_eq!(loops.len(), 2);
        assert!(area(&loops[0]) > area(&loops[1]));
        assert!(close(loops[1][0], pt(1.0, 1.0)) && close(loops[0][0], pt(-1.0, -1.0)), "{loops:?}");
    }
}
