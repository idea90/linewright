//! Viewports: frames on a layout that show model space at a scale.

use super::entities::Shape;
use super::geometry::{Point, pt};
use super::index::Bounds;

/// The frame's corners, counter-clockwise from the lower left.
pub fn frame_corners(center: Point, width: f64, height: f64) -> [Point; 4] {
    let (w, h) = (width / 2.0, height / 2.0);
    [center + pt(-w, -h), center + pt(w, -h), center + pt(w, h), center + pt(-w, h)]
}

/// Sheet units per model unit.
pub fn viewport_scale(height: f64, view_height: f64) -> f64 {
    if view_height > 0.0 { height / view_height } else { 1.0 }
}

/// The same viewport with its frame moved to cover `frame`, showing model space at the same scale and keeping
/// the model point at the frame's middle where it would be had only the frame been dragged.
pub fn reframed(shape: &Shape, frame: Bounds) -> Option<Shape> {
    let Shape::Viewport { center, height, view_center, view_height, frozen, locked, .. } = shape else { return None };
    let scale = viewport_scale(*height, *view_height);
    let new_center = frame.center();
    Some(Shape::Viewport {
        center: new_center,
        width: frame.width(),
        height: frame.height(),
        view_center: *view_center + (new_center - *center) * (1.0 / scale),
        view_height: frame.height() / scale,
        frozen: frozen.clone(),
        locked: *locked,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bigger_frame_shows_more_at_the_same_scale() {
        let shape = Shape::Viewport {
            center: pt(0.0, 0.0),
            width: 100.0,
            height: 50.0,
            view_center: pt(500.0, 500.0),
            view_height: 5000.0,
            frozen: vec![],
            locked: false,
        };
        let Some(Shape::Viewport { width, height, view_height, view_center, .. }) =
            reframed(&shape, Bounds::from_corners(pt(-50.0, -25.0), pt(150.0, 75.0)))
        else {
            panic!()
        };
        assert_eq!((width, height, view_height), (200.0, 100.0, 10000.0));
        assert_eq!(view_center, pt(5500.0, 3000.0));
    }
}
