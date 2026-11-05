//! Where the drawing sits on screen: the world point at the middle of the drawing area and how many pixels one
//! drawing unit takes. Drawings are y-up, screens are y-down.

use crate::model::geometry::{Point, pt};
use crate::model::index::Bounds;

pub const ZOOM_STEP: f64 = 1.2;
pub const MIN_SCALE: f64 = 1e-6;
pub const MAX_SCALE: f64 = 1e6;
const EXTENTS_MARGIN: f64 = 0.9;
const MIN_GRID_PIXELS: f64 = 12.0;
const START_SCALE: f64 = 4.0;

/// Smallest power of ten whose on-screen size is at least MIN_GRID_PIXELS.
pub fn grid_spacing(scale: f64) -> f64 {
    10f64.powf((MIN_GRID_PIXELS / scale).log10().ceil())
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    pub center: Point,
    pub scale: f64, // pixels per drawing unit
}

impl Default for Camera {
    fn default() -> Self {
        Camera { center: pt(0.0, 0.0), scale: START_SCALE }
    }
}

/// Screen positions are relative to the middle of the drawing area, in pixels, y down.
impl Camera {
    pub fn to_screen(&self, p: Point) -> (f64, f64) {
        ((p.x - self.center.x) * self.scale, -(p.y - self.center.y) * self.scale)
    }

    pub fn to_world(&self, x: f64, y: f64) -> Point {
        pt(self.center.x + x / self.scale, self.center.y - y / self.scale)
    }

    /// The part of the drawing a viewport of this size shows.
    pub fn visible(&self, width: f64, height: f64) -> Bounds {
        let half = pt(width / 2.0 / self.scale, height / 2.0 / self.scale);
        Bounds { x0: self.center.x - half.x, y0: self.center.y - half.y, x1: self.center.x + half.x, y1: self.center.y + half.y }
    }

    /// Zoom by a factor, keeping the world point under the given screen position where it is.
    pub fn zoom_at(&mut self, factor: f64, x: f64, y: f64) {
        let anchor = self.to_world(x, y);
        self.scale = (self.scale * factor).clamp(MIN_SCALE, MAX_SCALE);
        let moved = self.to_world(x, y);
        self.center = self.center + (anchor - moved);
    }

    /// Move the drawing with the mouse by a screen distance.
    pub fn pan(&mut self, dx: f64, dy: f64) {
        self.center = self.center + pt(-dx / self.scale, dy / self.scale);
    }

    /// Frame the box in a viewport of this size, with a little room around it.
    pub fn zoom_extents(&mut self, bounds: Bounds, width: f64, height: f64) {
        if width <= 0.0 || height <= 0.0 {
            return;
        }
        let fit = (width / bounds.width().max(1e-9)).min(height / bounds.height().max(1e-9)) * EXTENTS_MARGIN;
        self.scale = fit.clamp(MIN_SCALE, MAX_SCALE);
        self.center = bounds.center();
    }
}

/// The camera a viewport shows model space through, on a sheet shown by `paper`: its view's middle at the
/// frame's middle, at the scale it shows model space at.
pub fn viewport_camera(paper: &Camera, center: Point, height: f64, view_center: Point, view_height: f64) -> Camera {
    let scale = paper.scale * crate::model::viewport::viewport_scale(height, view_height);
    let (x, y) = paper.to_screen(center);
    Camera { scale, center: pt(view_center.x - x / scale, view_center.y + y / scale) }
}

/// What a viewport must show (its view's middle and the height of model space it shows) to show model space
/// as `model` does, its frame staying where `paper` shows it.
pub fn viewport_view(paper: &Camera, center: Point, height: f64, model: &Camera) -> (Point, f64) {
    let (x, y) = paper.to_screen(center);
    (model.to_world(x, y), height * paper.scale / model.scale)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_viewport_s_camera_and_view_turn_into_each_other() {
        let paper = Camera { center: pt(150.0, 100.0), scale: 3.0 };
        let camera = viewport_camera(&paper, pt(100.0, 120.0), 80.0, pt(5000.0, 2000.0), 4000.0);
        assert!((camera.scale - 0.06).abs() < 1e-12, "1:50 on a sheet shown 3 pixels a unit");
        let (x, y) = paper.to_screen(pt(100.0, 120.0));
        assert!(camera.to_world(x, y).dist(pt(5000.0, 2000.0)) < 1e-9, "the frame's middle shows the view's middle");
        let mut zoomed = camera;
        zoomed.zoom_at(2.0, x, y);
        let (view_center, view_height) = viewport_view(&paper, pt(100.0, 120.0), 80.0, &zoomed);
        assert!(view_center.dist(pt(5000.0, 2000.0)) < 1e-9 && (view_height - 2000.0).abs() < 1e-9);
    }

    #[test]
    fn screen_and_world_round_trip() {
        let camera = Camera { center: pt(10.0, 20.0), scale: 2.0 };
        assert_eq!(camera.to_screen(pt(11.0, 21.0)), (2.0, -2.0));
        assert_eq!(camera.to_world(2.0, -2.0), pt(11.0, 21.0));
    }

    #[test]
    fn zoom_keeps_the_point_under_the_cursor() {
        let mut camera = Camera::default();
        let before = camera.to_world(100.0, 50.0);
        camera.zoom_at(ZOOM_STEP, 100.0, 50.0);
        let after = camera.to_world(100.0, 50.0);
        assert!(before.dist(after) < 1e-9);
    }

    #[test]
    fn extents_fit_the_box() {
        let mut camera = Camera::default();
        camera.zoom_extents(Bounds { x0: 0.0, y0: 0.0, x1: 100.0, y1: 50.0 }, 1000.0, 1000.0);
        assert!((camera.scale - 9.0).abs() < 1e-9);
        assert_eq!(camera.center, pt(50.0, 25.0));
    }

    #[test]
    fn grid_is_a_power_of_ten_at_least_twelve_pixels() {
        assert_eq!(grid_spacing(4.0), 10.0);
        assert_eq!(grid_spacing(1.5), 10.0);
        assert_eq!(grid_spacing(1.0), 100.0);
    }
}
