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
/// Small section labels, as the website's: its warm orange.
pub const LABEL: Color32 = Color32::from_rgb(0xf0, 0x8a, 0x5d);
pub const OVERLAY: Color32 = Color32::from_rgba_premultiplied(17, 21, 23, 204); // the canvas at 80%

// The ribbon, in the colors of AutoCAD's dark theme: a blue-grey slate under a darker strip of tabs.
/// The panels, and the tab that is open.
pub const RIBBON: Color32 = Color32::from_rgb(0x3b, 0x44, 0x53);
/// The strip the tabs sit on.
pub const RIBBON_TABS: Color32 = Color32::from_rgb(0x22, 0x29, 0x33);
/// The title strip at the bottom of each panel.
pub const RIBBON_TITLE: Color32 = Color32::from_rgb(0x33, 0x3b, 0x48);
pub const RIBBON_TITLE_TEXT: Color32 = Color32::from_rgb(0xb4, 0xbc, 0xc8);
/// The groove between panels and the ribbon's lower edge.
pub const RIBBON_EDGE: Color32 = Color32::from_rgb(0x1d, 0x24, 0x2d);
pub const RIBBON_TEXT: Color32 = Color32::from_rgb(0xe6, 0xe8, 0xeb);
pub const RIBBON_TAB_TEXT: Color32 = Color32::from_rgb(0xc4, 0xc9, 0xd1);
/// A button under the pointer.
pub const RIBBON_HOVER: Color32 = Color32::from_rgb(0x4a, 0x55, 0x68);
pub const RIBBON_HOVER_EDGE: Color32 = Color32::from_rgb(0x62, 0x6e, 0x84);
/// A button that is on, or whose command runs: AutoCAD's blue.
pub const RIBBON_CHECKED: Color32 = Color32::from_rgb(0x46, 0x5d, 0x75);
pub const RIBBON_CHECKED_EDGE: Color32 = Color32::from_rgb(0x74, 0xc8, 0xff);
/// The drop-down lists of layers, colors, lineweights and linetypes.
pub const RIBBON_FIELD: Color32 = Color32::from_rgb(0x4e, 0x5a, 0x6e);
pub const RIBBON_FIELD_EDGE: Color32 = Color32::from_rgb(0x5c, 0x68, 0x7e);

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

/// The website's typefaces, built in: Public Sans for the interface and its headings, Martian Mono for the command
/// line and labels. Both are under the SIL Open Font License (see assets/fonts).
const INTERFACE_FONT: &[u8] = include_bytes!("../../assets/fonts/PublicSans-Regular.ttf");
const HEADING_FONT: &[u8] = include_bytes!("../../assets/fonts/PublicSans-SemiBold.ttf");
const MONO_FONT: &[u8] = include_bytes!("../../assets/fonts/MartianMono.ttf");

/// The family dialog titles and other headings are set in.
pub fn heading() -> FontFamily {
    FontFamily::Name("heading".into())
}

/// A small section label as the website sets them: uppercase, in the monospace face, spaced out.
pub fn label(text: &str, color: Color32) -> egui::RichText {
    egui::RichText::new(text.to_uppercase()).family(FontFamily::Monospace).size(9.5).extra_letter_spacing(1.2).color(color)
}

/// The interface in the built-in faces, then the system's interface and monospace fonts, then fonts for the scripts
/// those lack, then egui's own for anything left. Each font a drawing asks for gets a family of its own, which falls
/// back to the system's fonts, so drawings look as they do elsewhere.
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
    let system: Vec<String> = ui.into_iter().chain(fallbacks.iter().cloned()).chain(egui_proportional).collect();
    for (key, bytes) in [("interface", INTERFACE_FONT), ("heading", HEADING_FONT), ("mono", MONO_FONT)] {
        definitions.font_data.insert(key.into(), Arc::new(FontData::from_static(bytes)));
    }
    let proportional: Vec<String> = std::iter::once("interface".to_string()).chain(system.iter().cloned()).collect();
    let headings: Vec<String> = std::iter::once("heading".to_string()).chain(proportional.iter().cloned()).collect();
    let monospace: Vec<String> = ["mono".to_string()].into_iter().chain(mono).chain(egui_mono).chain(fallbacks.iter().cloned()).collect();
    for (font, file) in drawing_fonts {
        if let Some(key) = add(&mut definitions, format!("drawing:{}", font.to_lowercase()), file) {
            let family = std::iter::once(key).chain(system.iter().cloned()).collect();
            definitions.families.insert(drawing_family(font), family);
        }
    }
    definitions.families.insert(FontFamily::Proportional, proportional);
    definitions.families.insert(heading(), headings);
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
    visuals.window_corner_radius = CornerRadius::same(6);
    visuals.menu_corner_radius = CornerRadius::same(4);
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
        state.corner_radius = CornerRadius::same(3);
    }
    ctx.set_visuals(visuals);
    ctx.style_mut(|style| {
        style.spacing.item_spacing = egui::vec2(6.0, 4.0);
        style.spacing.button_padding = egui::vec2(10.0, 4.0);
        style.text_styles.insert(TextStyle::Body, FontId::new(13.5, FontFamily::Proportional));
        style.text_styles.insert(TextStyle::Button, FontId::new(13.5, FontFamily::Proportional));
        style.text_styles.insert(TextStyle::Small, FontId::new(11.5, FontFamily::Proportional));
        style.text_styles.insert(TextStyle::Monospace, FontId::new(11.0, FontFamily::Monospace));
        style.text_styles.insert(TextStyle::Heading, FontId::new(17.0, heading()));
    });
}
