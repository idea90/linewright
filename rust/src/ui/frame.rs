//! The window has no system frame: the header is its title bar. This lets it be resized from its edges, using the
//! system's own resize, and draws the thin border the system frame would have.

use eframe::egui::{self, CursorIcon, LayerId, Order, Pos2, Rect, Stroke, StrokeKind, ViewportCommand, viewport::ResizeDirection};

use super::theme;

const EDGE_WIDTH: f32 = 6.0; // pixels inside the window border that grab a resize

/// Which window edges a position grabs. Nothing away from the border.
pub fn edges_at(position: Pos2, window: Rect, width: f32) -> Option<ResizeDirection> {
    if !window.contains(position) {
        return None;
    }
    let left = position.x < window.left() + width;
    let right = position.x > window.right() - width;
    let top = position.y < window.top() + width;
    let bottom = position.y > window.bottom() - width;
    Some(match (left, right, top, bottom) {
        (true, _, true, _) => ResizeDirection::NorthWest,
        (_, true, true, _) => ResizeDirection::NorthEast,
        (true, _, _, true) => ResizeDirection::SouthWest,
        (_, true, _, true) => ResizeDirection::SouthEast,
        (true, ..) => ResizeDirection::West,
        (_, true, ..) => ResizeDirection::East,
        (_, _, true, _) => ResizeDirection::North,
        (_, _, _, true) => ResizeDirection::South,
        _ => return None,
    })
}

fn cursor_for(direction: ResizeDirection) -> CursorIcon {
    match direction {
        ResizeDirection::North => CursorIcon::ResizeNorth,
        ResizeDirection::South => CursorIcon::ResizeSouth,
        ResizeDirection::East => CursorIcon::ResizeEast,
        ResizeDirection::West => CursorIcon::ResizeWest,
        ResizeDirection::NorthEast => CursorIcon::ResizeNorthEast,
        ResizeDirection::SouthEast => CursorIcon::ResizeSouthEast,
        ResizeDirection::NorthWest => CursorIcon::ResizeNorthWest,
        ResizeDirection::SouthWest => CursorIcon::ResizeSouthWest,
    }
}

pub fn is_maximized(ctx: &egui::Context) -> bool {
    ctx.input(|i| i.viewport().maximized.unwrap_or(false) || i.viewport().fullscreen.unwrap_or(false))
}

pub fn toggle_maximized(ctx: &egui::Context) {
    ctx.send_viewport_cmd(ViewportCommand::Maximized(!is_maximized(ctx)));
}

/// The edge under the pointer, if it grabs a resize. A press there starts the system's resize.
/// Returns whether the pointer is on an edge, so the drawing underneath ignores it.
pub fn resize_from_edges(ctx: &egui::Context) -> bool {
    if is_maximized(ctx) {
        return false;
    }
    let (position, pressed) = ctx.input(|i| (i.pointer.hover_pos(), i.pointer.primary_pressed()));
    let Some(direction) = position.and_then(|p| edges_at(p, ctx.screen_rect(), EDGE_WIDTH)) else { return false };
    ctx.set_cursor_icon(cursor_for(direction));
    if pressed {
        ctx.send_viewport_cmd(ViewportCommand::BeginResize(direction));
    }
    true
}

/// The thin line around the window, where the system frame would be.
pub fn paint_border(ctx: &egui::Context) {
    if !is_maximized(ctx) {
        let painter = ctx.layer_painter(LayerId::new(Order::Foreground, egui::Id::new("window_border")));
        painter.rect_stroke(ctx.screen_rect(), 0.0, Stroke::new(1.0_f32, theme::BORDER_STRONG), StrokeKind::Inside);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::pos2;

    #[test]
    fn edges_and_corners_grab_a_resize() {
        let window = Rect::from_min_max(pos2(0.0, 0.0), pos2(800.0, 600.0));
        assert_eq!(edges_at(pos2(2.0, 300.0), window, 6.0), Some(ResizeDirection::West));
        assert_eq!(edges_at(pos2(798.0, 598.0), window, 6.0), Some(ResizeDirection::SouthEast));
        assert_eq!(edges_at(pos2(400.0, 1.0), window, 6.0), Some(ResizeDirection::North));
        assert_eq!(edges_at(pos2(400.0, 300.0), window, 6.0), None);
        assert_eq!(edges_at(pos2(900.0, 300.0), window, 6.0), None);
    }
}
