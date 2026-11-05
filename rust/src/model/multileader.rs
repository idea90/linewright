//! Multileaders: several leader lines to one landing, a dogleg and text, and the named styles they follow.

use super::dimension::{DimensionLook, end_mark};
use super::geometry::{Point, pt};

/// The multileader style every drawing has.
pub const STANDARD_MLEADER_STYLE: &str = "Standard";

/// A named set of multileader sizes, as a drawing's MLEADERSTYLE objects keep it.
#[derive(Clone, Debug, PartialEq)]
pub struct MleaderStyle {
    pub name: String,
    pub text_height: f64,
    pub arrow_size: f64,
    pub dogleg: f64,
    pub landing_gap: f64,
}

impl MleaderStyle {
    pub fn standard() -> MleaderStyle {
        MleaderStyle { name: STANDARD_MLEADER_STYLE.into(), text_height: 2.5, arrow_size: 2.5, dogleg: 2.5, landing_gap: 0.625 }
    }
}

/// Where the text's upper corner goes: past the dogleg's end by the gap, its first line level with the landing.
pub fn text_corner(landing: Point, dogleg: f64, height: f64, gap: f64) -> Point {
    let side = if dogleg < 0.0 { -1.0 } else { 1.0 };
    landing + pt(dogleg + side * gap, height / 2.0)
}

/// The lines (the dogleg last) and the arrowheads a multileader draws.
pub fn multileader_lines(leaders: &[Vec<Point>], landing: Point, dogleg: f64, arrow_size: f64) -> (Vec<Vec<Point>>, Vec<[Point; 3]>) {
    let look = DimensionLook { arrow_size, scale: 1.0, ..DimensionLook::default() };
    let mut paths = vec![];
    let mut arrows = vec![];
    for leader in leaders.iter().filter(|l| !l.is_empty()) {
        let mut path = leader.clone();
        path.push(landing);
        let toward = path[1] - path[0];
        if toward.length() > 0.0 {
            arrows.extend(end_mark(path[0], toward * (1.0 / toward.length()), &look).0);
        }
        paths.push(path);
    }
    if dogleg != 0.0 {
        paths.push(vec![landing, landing + pt(dogleg, 0.0)]);
    }
    (paths, arrows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_leader_ends_at_the_landing_and_the_dogleg_runs_on() {
        let leaders = vec![vec![pt(0.0, 0.0), pt(5.0, 10.0)], vec![pt(0.0, 20.0)]];
        let (paths, arrows) = multileader_lines(&leaders, pt(10.0, 14.0), 8.0, 4.0);
        assert_eq!(paths.len(), 3);
        assert_eq!(*paths[1].last().unwrap(), pt(10.0, 14.0));
        assert_eq!(paths[2][1], pt(18.0, 14.0));
        assert_eq!(arrows.len(), 2);
        assert_eq!(arrows[1][0], pt(0.0, 20.0));
        assert_eq!(text_corner(pt(10.0, 14.0), 8.0, 2.5, 2.0), pt(20.0, 15.25));
        assert_eq!(text_corner(pt(10.0, 14.0), -8.0, 2.5, 2.0), pt(0.0, 15.25));
    }
}
