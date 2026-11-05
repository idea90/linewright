//! Multilines: several parallel lines drawn along one path, as an MLINESTYLE sets them out.

use super::entities::BYLAYER;
use super::geometry::Point;
use super::styles::BYLAYER_LINETYPE;

/// One of a multiline style's lines: how far it lies left of the path at scale 1, and how it looks.
#[derive(Clone, Debug, PartialEq)]
pub struct MlineElement {
    pub offset: f64,
    pub color: i32,
    pub linetype: String,
}

/// A multiline style: its lines, and whether its ends and the joints between segments are closed off with lines
/// across.
#[derive(Clone, Debug, PartialEq)]
pub struct MlineStyle {
    pub name: String,
    pub description: String,
    pub elements: Vec<MlineElement>,
    pub start_cap: bool,
    pub end_cap: bool,
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
            start_cap: false,
            end_cap: false,
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
/// then the caps and joints across, which take the multiline's own look (element None).
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
    let along = |distance: f64| -> Vec<Point> {
        let mut path: Vec<Point> = points.iter().zip(&miters).map(|(p, m)| *p + *m * distance).collect();
        if closed {
            path.push(path[0]);
        }
        path
    };
    let mut out: Vec<(Option<usize>, Vec<Point>)> = distances.iter().enumerate().map(|(i, d)| (Some(i), along(*d))).collect();
    let (high, low) =
        (distances.iter().copied().fold(f64::NEG_INFINITY, f64::max), distances.iter().copied().fold(f64::INFINITY, f64::min));
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
    }
    out
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
}
