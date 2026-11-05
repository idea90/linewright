//! Linewright's mark, as on its website: a drafting square with a line drawn corner to corner and a grip at each
//! end, the line and grips in the accent. Drawn on 26 units: the square's edge runs 1 unit in from each side.

use eframe::egui::{self, Color32, ImageSource, Pos2, Rect, Stroke, StrokeKind, load::Bytes, pos2};

use super::theme;

const SIDE: f32 = 26.0;
const FRAME_WIDTH: f32 = 2.0;
const LINE: (Pos2, Pos2) = (pos2(5.0, 21.0), pos2(21.0, 5.0));
const LINE_WIDTH: f32 = 2.2;
/// The grips at the line's ends: their top left corners, each 5 units across.
const GRIPS: [Pos2; 2] = [pos2(2.5, 18.5), pos2(18.5, 2.5)];
const GRIP_SIZE: f32 = 5.0;

fn hex(color: Color32) -> String {
    format!("#{:02x}{:02x}{:02x}", color.r(), color.g(), color.b())
}

/// The mark as an SVG, in the theme's text and accent colors.
fn svg() -> String {
    let (ink, accent) = (hex(theme::TEXT), hex(theme::ACCENT));
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 26 26" width="64" height="64" fill="none"><rect x="1" y="1" width="24" height="24" stroke="{ink}" stroke-width="{FRAME_WIDTH}"/><path d="M5 21 L21 5" stroke="{accent}" stroke-width="{LINE_WIDTH}"/><rect x="2.5" y="18.5" width="5" height="5" fill="{accent}"/><rect x="18.5" y="2.5" width="5" height="5" fill="{accent}"/></svg>"#
    )
}

/// The mark as an image, for buttons.
pub fn image() -> egui::Image<'static> {
    let bytes: Bytes = svg().into_bytes().into();
    egui::Image::new(ImageSource::Bytes { uri: "bytes://linewright-logo.svg".into(), bytes })
}

/// Paint the mark to fill a square.
pub fn paint(painter: &egui::Painter, square: Rect) {
    let unit = square.width() / SIDE;
    let at = |p: Pos2| square.min + p.to_vec2() * unit;
    let frame = Rect::from_min_max(at(pos2(1.0, 1.0)), at(pos2(25.0, 25.0)));
    painter.rect_stroke(frame, 0.0, Stroke::new(FRAME_WIDTH * unit, theme::TEXT), StrokeKind::Middle);
    painter.line_segment([at(LINE.0), at(LINE.1)], Stroke::new(LINE_WIDTH * unit, theme::ACCENT));
    for corner in GRIPS {
        painter.rect_filled(Rect::from_min_size(at(corner), egui::vec2(GRIP_SIZE, GRIP_SIZE) * unit), 0.0, theme::ACCENT);
    }
}

/// The mark on a dark rounded tile, as the window's icon in the taskbar and title bar.
pub fn window_icon(size: u32) -> egui::IconData {
    const SAMPLES: u32 = 4;
    let side = size as f32;
    let radius = side * 0.2;
    let inset = side * 0.16;
    let unit = (side - 2.0 * inset) / SIDE;
    let mut rgba = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            // Each pixel is sampled several times across, so edges come out smooth.
            let (mut sum, mut covered) = ([0.0_f32; 3], 0.0_f32);
            for sy in 0..SAMPLES {
                for sx in 0..SAMPLES {
                    let p = pos2(x as f32 + (sx as f32 + 0.5) / SAMPLES as f32, y as f32 + (sy as f32 + 0.5) / SAMPLES as f32);
                    if !inside_rounded(p, side, radius) {
                        continue;
                    }
                    let color = color_at(pos2((p.x - inset) / unit, (p.y - inset) / unit));
                    for (total, channel) in sum.iter_mut().zip([color.r(), color.g(), color.b()]) {
                        *total += f32::from(channel);
                    }
                    covered += 1.0;
                }
            }
            let alpha = covered / (SAMPLES * SAMPLES) as f32;
            let average = |total: f32| if covered > 0.0 { (total / covered).round() as u8 } else { 0 };
            rgba.extend([average(sum[0]), average(sum[1]), average(sum[2]), (alpha * 255.0).round() as u8]);
        }
    }
    egui::IconData { rgba, width: size, height: size }
}

/// Whether a point lies on the tile: a square `side` across with corners rounded by `radius`.
fn inside_rounded(p: Pos2, side: f32, radius: f32) -> bool {
    let nearest = pos2(p.x.clamp(radius, side - radius), p.y.clamp(radius, side - radius));
    p.distance(nearest) <= radius
}

/// The mark's color at a point in its own 26 units, or the tile's where the mark leaves it.
fn color_at(p: Pos2) -> Color32 {
    let on_grip = GRIPS.iter().any(|c| Rect::from_min_size(*c, egui::vec2(GRIP_SIZE, GRIP_SIZE)).contains(p));
    if on_grip || distance_to_segment(p, LINE.0, LINE.1) <= LINE_WIDTH / 2.0 {
        return theme::ACCENT;
    }
    let outer = Rect::from_min_max(pos2(0.0, 0.0), pos2(SIDE, SIDE));
    let inner = outer.shrink(FRAME_WIDTH);
    if outer.contains(p) && !inner.contains(p) {
        return theme::TEXT;
    }
    theme::PANEL_RAISED
}

fn distance_to_segment(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let along = b - a;
    let t = ((p - a).dot(along) / along.length_sq()).clamp(0.0, 1.0);
    p.distance(a + along * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_icon_shows_the_mark_on_a_rounded_tile() {
        let icon = window_icon(64);
        assert_eq!(icon.rgba.len(), 64 * 64 * 4);
        let pixel = |x: usize, y: usize| &icon.rgba[(y * 64 + x) * 4..(y * 64 + x) * 4 + 4];
        assert_eq!(pixel(0, 0)[3], 0, "the corners are rounded off");
        let middle = pixel(32, 32);
        assert_eq!(
            (middle[0], middle[1], middle[2]),
            (theme::ACCENT.r(), theme::ACCENT.g(), theme::ACCENT.b()),
            "the line crosses the middle"
        );
        assert!(svg().contains(&hex(theme::ACCENT)));
    }
}
