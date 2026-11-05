use super::osnap::{SnapKind, find_osnap};
use super::tracking::{constrain_ortho, constrain_polar, snap_to_grid};
use crate::model::entities::Entity;
use crate::model::geometry::Point;

pub const DEFAULT_POLAR_INCREMENT: f64 = 45.0;

/// A drafting aid on the status bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapMode {
    ShowGrid,
    Grid,
    Ortho,
    Polar,
    Osnap,
    Tracking,
    Dynamic,
}

/// The drafting aids on the status bar. show_grid only draws the grid; grid is snapping to it.
#[derive(Clone, Debug, PartialEq)]
pub struct SnapSettings {
    pub show_grid: bool,
    pub osnap: bool,
    pub ortho: bool,
    pub polar: bool,
    pub grid: bool,
    pub polar_increment: f64,
    /// The object snaps that are on.
    pub kinds: Vec<SnapKind>,
    /// Object snap tracking: lines through acquired snap points pull the cursor onto them.
    pub tracking: bool,
    /// Dynamic input: the prompt and measurements follow the cursor.
    pub dynamic: bool,
}

impl Default for SnapSettings {
    fn default() -> Self {
        SnapSettings {
            show_grid: true,
            osnap: true,
            ortho: false,
            polar: false,
            grid: false,
            polar_increment: DEFAULT_POLAR_INCREMENT,
            kinds: SnapKind::defaults(),
            tracking: true,
            dynamic: true,
        }
    }
}

impl SnapSettings {
    pub fn get(&self, mode: SnapMode) -> bool {
        match mode {
            SnapMode::ShowGrid => self.show_grid,
            SnapMode::Grid => self.grid,
            SnapMode::Ortho => self.ortho,
            SnapMode::Polar => self.polar,
            SnapMode::Osnap => self.osnap,
            SnapMode::Tracking => self.tracking,
            SnapMode::Dynamic => self.dynamic,
        }
    }

    /// Turn a mode on or off. Ortho and polar tracking exclude each other.
    pub fn set(&mut self, mode: SnapMode, on: bool) {
        match mode {
            SnapMode::ShowGrid => self.show_grid = on,
            SnapMode::Grid => self.grid = on,
            SnapMode::Ortho => self.ortho = on,
            SnapMode::Polar => self.polar = on,
            SnapMode::Osnap => self.osnap = on,
            SnapMode::Tracking => self.tracking = on,
            SnapMode::Dynamic => self.dynamic = on,
        }
        if on && mode == SnapMode::Ortho {
            self.polar = false;
        }
        if on && mode == SnapMode::Polar {
            self.ortho = false;
        }
    }
}

/// What the cursor can snap to, in world units.
pub struct SnapScene<'a> {
    pub aperture: f64,
    pub grid_step: f64,
    pub entities: &'a [Entity],
    /// Snap points acquired for object snap tracking.
    pub acquired: &'a [Point],
}

#[derive(Clone, Debug, PartialEq)]
pub struct SnapResult {
    pub point: Point,
    pub kind: Option<SnapKind>, // the object snap that was used, if any
    /// The acquired points whose tracking lines the point lies on, to draw the lines from.
    pub tracked: Vec<Point>,
}

/// Object snaps win. Otherwise the grid rounds the cursor, then ortho or polar constrains it to the base.
pub fn resolve_point(cursor: Point, base: Option<Point>, settings: &SnapSettings, scene: &SnapScene) -> SnapResult {
    if settings.osnap
        && let Some(hit) = find_osnap(cursor, scene.aperture, scene.entities, base, &settings.kinds)
    {
        return SnapResult { point: hit.point, kind: Some(hit.kind), tracked: vec![] };
    }
    if settings.tracking
        && let Some((point, tracked)) = track(cursor, scene.acquired, scene.aperture)
    {
        return SnapResult { point, kind: None, tracked };
    }
    let mut point = if settings.grid { snap_to_grid(cursor, scene.grid_step) } else { cursor };
    if let Some(base) = base {
        if settings.ortho {
            point = constrain_ortho(base, point);
        } else if settings.polar {
            point = constrain_polar(base, point, settings.polar_increment);
        }
    }
    SnapResult { point, kind: None, tracked: vec![] }
}

/// Object snap tracking: the cursor on the horizontal or vertical through an acquired point, or where two such
/// lines cross, when it is within the aperture of them. Gives the point and the acquired points it lines up with.
pub fn track(cursor: Point, acquired: &[Point], aperture: f64) -> Option<(Point, Vec<Point>)> {
    let horizontal = acquired
        .iter()
        .filter(|a| (cursor.y - a.y).abs() <= aperture)
        .min_by(|a, b| (cursor.y - a.y).abs().total_cmp(&(cursor.y - b.y).abs()));
    let vertical = acquired
        .iter()
        .filter(|a| (cursor.x - a.x).abs() <= aperture)
        .min_by(|a, b| (cursor.x - a.x).abs().total_cmp(&(cursor.x - b.x).abs()));
    match (horizontal, vertical) {
        (Some(h), Some(v)) if h != v => Some((crate::model::geometry::pt(v.x, h.y), vec![*h, *v])),
        (Some(h), _) => Some((crate::model::geometry::pt(cursor.x, h.y), vec![*h])),
        (None, Some(v)) => Some((crate::model::geometry::pt(v.x, cursor.y), vec![*v])),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::entities::line;
    use crate::model::geometry::pt;

    #[test]
    fn object_snap_wins_over_ortho() {
        let entities = [line(pt(3.0, 3.0), pt(10.0, 10.0))];
        let settings = SnapSettings { ortho: true, ..SnapSettings::default() };
        let scene = SnapScene { aperture: 0.5, grid_step: 1.0, entities: &entities, acquired: &[] };
        let hit = resolve_point(pt(3.1, 3.1), Some(pt(0.0, 0.0)), &settings, &scene);
        assert_eq!(hit.point, pt(3.0, 3.0));
        let free = resolve_point(pt(6.0, 1.0), Some(pt(0.0, 0.0)), &settings, &scene);
        assert_eq!(free, SnapResult { point: pt(6.0, 0.0), kind: None, tracked: vec![] });
    }

    #[test]
    fn ortho_and_polar_exclude_each_other() {
        let mut settings = SnapSettings::default();
        settings.set(SnapMode::Ortho, true);
        settings.set(SnapMode::Polar, true);
        assert!(!settings.ortho && settings.polar);
    }

    #[test]
    fn tracking_lines_up_with_acquired_points() {
        let acquired = [pt(0.0, 0.0), pt(10.0, 20.0)];
        let (point, from) = track(pt(5.0, 0.3), &acquired, 0.5).unwrap();
        assert_eq!((point, from), (pt(5.0, 0.0), vec![pt(0.0, 0.0)]));
        let (corner, both) = track(pt(10.2, 0.2), &acquired, 0.5).unwrap();
        assert_eq!((corner, both.len()), (pt(10.0, 0.0), 2), "where the two lines cross");
        assert!(track(pt(5.0, 5.0), &acquired, 0.5).is_none());
    }
}
