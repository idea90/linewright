use std::sync::Arc;

use crate::fonts::{self, FontFile};
use eframe::egui::{self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke, TextStyle, Visuals};

// The palette of the Linewright website's dark theme: blue-grey neutrals, light ink and a teal accent.
pub const CANVAS: Color32 = Color32::from_rgb(0x15, 0x1a, 0x1d);
pub const GRID_MINOR: Color32 = Color32::from_rgb(0x1b, 0x21, 0x25);
pub const GRID_MAJOR: Color32 = Color32::from_rgb(0x24, 0x2b, 0x30);
pub const PANEL: Color32 = Color32::from_rgb(0x1c, 0x22, 0x26);
pub const PANEL_RAISED: Color32 = Color32::from_rgb(0x27, 0x2f, 0x34);
pub const PANEL_DARK: Color32 = Color32::from_rgb(0x14, 0x18, 0x1b);
pub const BORDER: Color32 = Color32::from_rgb(0x2e, 0x36, 0x3b);
pub const BORDER_STRONG: Color32 = Color32::from_rgb(0x3c, 0x46, 0x4c);
pub const TEXT: Color32 = Color32::from_rgb(0xe4, 0xe9, 0xea);
pub const TEXT_DIM: Color32 = Color32::from_rgb(0x97, 0xa3, 0xa7);
pub const ACCENT: Color32 = Color32::from_rgb(0x45, 0xc4, 0xcf);
pub const ACCENT_TINT: Color32 = Color32::from_rgba_premultiplied(10, 27, 29, 36); // accent at 14%
/// The accent darkened so white labels read well on it: selected list items and pressed buttons.
pub const ACCENT_EDGE: Color32 = Color32::from_rgb(0x1d, 0x5e, 0x65);
pub const SNAP_MARKER: Color32 = Color32::from_rgb(0xf0, 0xb4, 0x3c);
/// Selected objects and the window selection box: the one color the drawing area keeps besides snaps and grips.
/// It stays blue, as in AutoCAD, so it never looks like a cyan layer.
pub const SELECTION: Color32 = Color32::from_rgb(0x4c, 0x8d, 0xff);
pub const CROSSING: Color32 = Color32::from_rgb(0x3f, 0xd0, 0x6a);
pub const CROSSHAIR: Color32 = Color32::from_rgb(0xe9, 0xee, 0xf0);
pub const AXIS_X: Color32 = Color32::from_rgb(0x97, 0xa3, 0xa7);
pub const AXIS_Y: Color32 = Color32::from_rgb(0x97, 0xa3, 0xa7);
pub const ERROR: Color32 = Color32::from_rgb(0xf0, 0x7a, 0x6a);
pub const GRIP: Color32 = Color32::from_rgb(0x2f, 0x6b, 0xff);
pub const GRIP_HOT: Color32 = Color32::from_rgb(0xff, 0x45, 0x45);
pub const CLOSE_HOVER: Color32 = Color32::from_rgb(0xc4, 0x2b, 0x1c);
pub const OVERLAY: Color32 = Color32::from_rgba_premultiplied(17, 21, 23, 204); // the canvas at 80%

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
    // Selected items: the dark accent, which white labels read well on.
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
