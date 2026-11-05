use std::sync::Arc;

use crate::fonts::{self, FontFile};
use eframe::egui::{self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke, TextStyle, Visuals};

pub const CANVAS: Color32 = Color32::from_rgb(0x21, 0x21, 0x21);
pub const GRID_MINOR: Color32 = Color32::from_rgb(0x27, 0x27, 0x27);
pub const GRID_MAJOR: Color32 = Color32::from_rgb(0x30, 0x30, 0x30);
pub const PANEL: Color32 = Color32::from_rgb(0x2d, 0x2d, 0x2d);
pub const PANEL_RAISED: Color32 = Color32::from_rgb(0x3b, 0x3b, 0x3b);
pub const PANEL_DARK: Color32 = Color32::from_rgb(0x26, 0x26, 0x26);
pub const BORDER: Color32 = Color32::from_rgb(0x3d, 0x3d, 0x3d);
pub const BORDER_STRONG: Color32 = Color32::from_rgb(0x4b, 0x4b, 0x4b);
pub const TEXT: Color32 = Color32::from_rgb(0xe4, 0xe4, 0xe4);
pub const TEXT_DIM: Color32 = Color32::from_rgb(0x9a, 0x9a, 0x9a);
pub const ACCENT: Color32 = Color32::from_rgb(0xc8, 0xc8, 0xc8);
pub const ACCENT_TINT: Color32 = Color32::from_rgba_premultiplied(29, 29, 29, 36); // accent at 14%
pub const ACCENT_EDGE: Color32 = Color32::from_rgb(0x5c, 0x5c, 0x5c);
pub const SNAP_MARKER: Color32 = Color32::from_rgb(0xff, 0xd6, 0x00);
/// Selected objects and the window selection box: the one color the drawing area keeps besides snaps and grips.
pub const SELECTION: Color32 = Color32::from_rgb(0x4c, 0x8d, 0xff);
pub const CROSSING: Color32 = Color32::from_rgb(0x5f, 0xd4, 0x8a);
pub const CROSSHAIR: Color32 = Color32::from_rgb(0xf2, 0xf2, 0xf2);
pub const AXIS_X: Color32 = Color32::from_rgb(0x9a, 0x9a, 0x9a);
pub const AXIS_Y: Color32 = Color32::from_rgb(0x9a, 0x9a, 0x9a);
pub const ERROR: Color32 = Color32::from_rgb(0xff, 0x7b, 0x72);
pub const GRIP: Color32 = Color32::from_rgb(0x2f, 0x6b, 0xff);
pub const GRIP_HOT: Color32 = Color32::from_rgb(0xff, 0x45, 0x45);
pub const CLOSE_HOVER: Color32 = Color32::from_rgb(0xc4, 0x2b, 0x1c);
pub const OVERLAY: Color32 = Color32::from_rgba_premultiplied(26, 26, 26, 204); // the canvas at 80%

fn font_data(file: &FontFile) -> Option<FontData> {
    let bytes = fonts::load(file)?;
    let mut data = FontData::from_owned(bytes.as_ref().clone());
    data.index = file.index;
    Some(data)
}

/// The egui family a drawing's font is registered under.
pub fn drawing_family(font: &str) -> FontFamily {
    FontFamily::Name(format!("drawing:{}", font.to_lowercase()).into())
}

/// Use the system's fonts: its interface and monospace fonts first, then fonts for the scripts those lack, then
/// egui's own for anything left. Each font a drawing asks for gets a family of its own, which falls back the same way.
pub fn install_fonts(ctx: &egui::Context, drawing_fonts: &[(String, FontFile)]) {
    let catalog = fonts::catalog();
    let mut definitions = FontDefinitions::default();
    let add = |definitions: &mut FontDefinitions, key: String, file: &FontFile| -> Option<String> {
        let data = font_data(file)?;
        definitions.font_data.insert(key.clone(), Arc::new(data));
        Some(key)
    };
    let ui = catalog.ui_font().and_then(|f| add(&mut definitions, "system-ui".into(), &f));
    let mono = catalog.mono_font().and_then(|f| add(&mut definitions, "system-mono".into(), &f));
    let fallbacks: Vec<String> =
        catalog.fallbacks().iter().enumerate().filter_map(|(i, f)| add(&mut definitions, format!("fallback-{i}"), f)).collect();
    let egui_proportional = definitions.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    let egui_mono = definitions.families.get(&FontFamily::Monospace).cloned().unwrap_or_default();
    let proportional: Vec<String> = ui.into_iter().chain(fallbacks.iter().cloned()).chain(egui_proportional).collect();
    let monospace: Vec<String> = mono.into_iter().chain(egui_mono).chain(fallbacks.iter().cloned()).collect();
    for (font, file) in drawing_fonts {
        if let Some(key) = add(&mut definitions, format!("drawing:{}", font.to_lowercase()), file) {
            let family = std::iter::once(key).chain(proportional.iter().cloned()).collect();
            definitions.families.insert(drawing_family(font), family);
        }
    }
    definitions.families.insert(FontFamily::Proportional, proportional);
    definitions.families.insert(FontFamily::Monospace, monospace);
    ctx.set_fonts(definitions);
}

pub fn apply(ctx: &egui::Context) {
    install_fonts(ctx, &[]);
    let mut visuals = Visuals::dark();
    visuals.panel_fill = PANEL;
    visuals.window_fill = PANEL_RAISED;
    visuals.extreme_bg_color = CANVAS;
    visuals.faint_bg_color = PANEL;
    visuals.window_stroke = Stroke::new(1.0_f32, BORDER_STRONG);
    visuals.window_corner_radius = CornerRadius::same(10);
    visuals.menu_corner_radius = CornerRadius::same(6);
    visuals.override_text_color = Some(TEXT);
    visuals.hyperlink_color = ACCENT;
    // Selected items: a mid gray that white labels read well on.
    visuals.selection.bg_fill = ACCENT_EDGE;
    visuals.selection.stroke = Stroke::new(1.0_f32, TEXT);
    visuals.text_cursor.stroke = Stroke::new(2.0_f32, ACCENT);
    let widgets = &mut visuals.widgets;
    widgets.noninteractive.bg_fill = PANEL;
    widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, BORDER);
    widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, TEXT);
    for (state, fill, edge) in [
        (&mut widgets.inactive, PANEL_RAISED, BORDER_STRONG),
        (&mut widgets.hovered, PANEL_RAISED, ACCENT),
        (&mut widgets.active, ACCENT_EDGE, ACCENT),
        (&mut widgets.open, PANEL_RAISED, ACCENT),
    ] {
        state.bg_fill = fill;
        state.weak_bg_fill = fill;
        state.bg_stroke = Stroke::new(1.0_f32, edge);
        state.fg_stroke = Stroke::new(1.0_f32, TEXT);
        state.corner_radius = CornerRadius::same(6);
    }
    ctx.set_visuals(visuals);
    ctx.style_mut(|style| {
        style.spacing.item_spacing = egui::vec2(6.0, 4.0);
        style.spacing.button_padding = egui::vec2(10.0, 4.0);
        style.text_styles.insert(TextStyle::Body, FontId::new(13.5, FontFamily::Proportional));
        style.text_styles.insert(TextStyle::Button, FontId::new(13.5, FontFamily::Proportional));
        style.text_styles.insert(TextStyle::Small, FontId::new(11.5, FontFamily::Proportional));
        style.text_styles.insert(TextStyle::Monospace, FontId::new(13.0, FontFamily::Monospace));
    });
}
