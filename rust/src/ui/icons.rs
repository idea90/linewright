//! One 24x24 line-icon family: 1.6 stroke, round caps and joins. Icons are drawn white and tinted when shown.

use std::sync::Arc;

use eframe::egui::{self, Color32, ImageSource, Rect, Ui, load::Bytes};

const RENDER_SIZE: u32 = 64; // rasterized this large so they stay sharp on high-DPI screens

fn shape(name: &str) -> &'static str {
    match name {
        "LINE" => r#"<path d="M5 19 19 5"/><circle cx="5" cy="19" r="1.6" fill="white"/><circle cx="19" cy="5" r="1.6" fill="white"/>"#,
        "PLINE" => {
            r#"<path d="M4 17 9 8l5 6 6-8"/><circle cx="4" cy="17" r="1.5" fill="white"/><circle cx="9" cy="8" r="1.5" fill="white"/><circle cx="14" cy="14" r="1.5" fill="white"/><circle cx="20" cy="6" r="1.5" fill="white"/>"#
        }
        "RECTANGLE" => r#"<rect x="4.5" y="7" width="15" height="10" rx="1"/>"#,
        "CIRCLE" => r#"<circle cx="12" cy="12" r="7.5"/><circle cx="12" cy="12" r="1" fill="white"/>"#,
        "ARC" => {
            r#"<path d="M4.5 18a8.5 8.5 0 0 1 15 0"/><circle cx="4.5" cy="18" r="1.5" fill="white"/><circle cx="19.5" cy="18" r="1.5" fill="white"/>"#
        }
        "TEXT" => r#"<path d="M6 7V5.5h12V7M12 5.5v13M9 18.5h6"/>"#,
        "ERASE" => {
            r#"<path d="M8 19 4.5 15.5a1.5 1.5 0 0 1 0-2.1L13 5a1.5 1.5 0 0 1 2.1 0l4 4a1.5 1.5 0 0 1 0 2.1L11 19Z"/><path d="M8 19h11M8.5 9.5l6 6"/>"#
        }
        "MOVE" => r#"<path d="M12 4v16M4 12h16M9 7l3-3 3 3M9 17l3 3 3-3M7 9l-3 3 3 3M17 9l3 3-3 3"/>"#,
        "COPY" => r#"<rect x="9" y="9" width="10.5" height="10.5" rx="1.5"/><path d="M5.5 15V6.5A1.5 1.5 0 0 1 7 5h8.5"/>"#,
        "ROTATE" => r#"<path d="M19 12a7 7 0 1 1-2.2-5.1"/><path d="M19.5 4v4.5H15"/>"#,
        "SCALE" => r#"<rect x="4.5" y="11.5" width="8" height="8" rx="1"/><path d="M12.5 11.5 19.5 4.5M14 4.5h5.5V10"/>"#,
        "MIRROR" => r#"<path d="M12 3.5v17" stroke-dasharray="2 2.5"/><path d="M9 8 4 18h5ZM15 8l5 10h-5Z"/>"#,
        "OFFSET" => r#"<path d="M5 19a14 14 0 0 1 14-14"/><path d="M5 13.5a8.5 8.5 0 0 1 8.5-8.5"/><path d="M5 8.5A3.5 3.5 0 0 1 8.5 5"/>"#,
        "TRIM" => r#"<path d="M12 4v16"/><path d="M4 12h8"/><path d="M12 12h8" stroke-dasharray="2 2.5"/>"#,
        "EXTEND" => r#"<path d="M19 5v14"/><path d="M4 12h9"/><path d="M13 12h6" stroke-dasharray="2 2.5"/>"#,
        "FILLET" => r#"<path d="M5 4v7.5A6.5 6.5 0 0 0 11.5 18H20"/>"#,
        "DIMLINEAR" => r#"<path d="M5 5v11M19 5v11M5 10.5h14"/><path d="m8.5 8.5-3.5 2 3.5 2M15.5 8.5l3.5 2-3.5 2"/>"#,
        "DIMALIGNED" => {
            r#"<g transform="rotate(-30 12 12)"><path d="M5 5v11M19 5v11M5 10.5h14"/><path d="m8.5 8.5-3.5 2 3.5 2M15.5 8.5l3.5 2-3.5 2"/></g>"#
        }
        "DIMSCALE" => {
            r#"<path d="M4 17h16M4 17v-4M8 17v-2M12 17v-4M16 17v-2M20 17v-4"/><path d="M7 8h10M9.5 5.5 7 8l2.5 2.5M14.5 5.5 17 8l-2.5 2.5"/>"#
        }
        "DISTANCE" => {
            r#"<g transform="rotate(-35 12 12)"><rect x="2.5" y="8.5" width="19" height="7" rx="1"/><path d="M6 8.5v3M9.5 8.5v2M13 8.5v3M16.5 8.5v2"/></g>"#
        }
        "DIMRADIUS" => r#"<circle cx="10" cy="14" r="6.5"/><path d="M10 14 20 4"/><path d="m14.6 9.4.5-2.6 2.1 2.1Z" fill="white"/>"#,
        "DIMDIAMETER" => r#"<circle cx="12" cy="12" r="7.5"/><path d="M6.7 17.3 17.3 6.7"/><circle cx="12" cy="12" r="1" fill="white"/>"#,
        "DIMANGULAR" => r#"<path d="M4 19h16M4 19 15 6"/><path d="M12 19a8 8 0 0 0-2.3-5.7"/>"#,
        "DIMSTYLE" => {
            r#"<path d="M4 8h16M4 8l2-1.5M4 8l2 1.5M20 8l-2-1.5M20 8l-2 1.5"/><path d="M5 14h7M5 18h5"/><circle cx="17" cy="16" r="2.5"/><path d="M17 12v1.5M17 18.5V20M13 16h1.5M19.5 16H21"/>"#
        }
        "POLYGON" => r#"<path d="m12 3.5 8 5.8-3 9.4H7l-3-9.4Z"/>"#,
        "MTEXT" => r#"<path d="M5 7V5.5h10V7M10 5.5v13M7.5 18.5h5"/><path d="M15.5 11h4.5M15.5 14.5h4.5M15.5 18h4.5"/>"#,
        "GROUP" => {
            r#"<path d="M7 4H4v16h3M17 4h3v16h-3"/><rect x="8.5" y="6.5" width="5" height="5" rx="0.5"/><circle cx="12.5" cy="15.5" r="2.6"/>"#
        }
        "UNGROUP" => {
            r#"<path d="M7 4H4v16h3M17 4h3v16h-3"/><rect x="7.5" y="6.5" width="5" height="5" rx="0.5"/><path d="m13.5 13.5 5 5M18.5 13.5l-5 5"/>"#
        }
        "GROUPEDIT" => {
            r#"<path d="M7 4H4v16h3M17 4h3v16h-3"/><rect x="8.5" y="6.5" width="5" height="5" rx="0.5"/><path d="m11 18 6-6 1.8 1.8-6 6H11Z"/>"#
        }
        "PICKSTYLE" => {
            r#"<path d="M7 4H4v16h3M17 4h3v16h-3"/><rect x="8" y="6.5" width="4.5" height="4.5" rx="0.5"/><circle cx="10.3" cy="15.2" r="2.2"/><path d="m14 11 5.5 3-2.6.8 1.6 2.9-1.3.7-1.6-2.9L14 17.4Z" fill="white"/>"#
        }
        "MVIEW" => {
            r#"<rect x="3.5" y="5" width="17" height="14" rx="1"/><rect x="6.5" y="8" width="7" height="8"/><path d="m8 14 2-3 2 2"/>"#
        }
        "LAYOUT" => r#"<path d="M5 4h10l4 4v12H5Z"/><path d="M15 4v4h4M8 17h8M12 13v4"/>"#,
        "QLEADER" => r#"<path d="m4 19 8-8h8"/><path d="m4 19 1-3.6 2.6 2.6Z" fill="white"/><path d="M14 7h6"/>"#,
        "AREA" => r#"<path d="M5 17.5 7.5 6l11.5 3.5-2 8.5Z"/><path d="m9 14 5-5M12 16.5l4.5-4.5"/>"#,
        "PLOT" => r#"<path d="M7 9V4h10v5"/><rect x="4" y="9" width="16" height="8" rx="1.5"/><path d="M7 14h10v6H7Z"/>"#,
        "LAYERS" => r#"<path d="m12 4 8.5 4.5L12 13 3.5 8.5Z"/><path d="m3.5 12.5 8.5 4.5 8.5-4.5"/><path d="m3.5 16.5 8.5 4.5 8.5-4.5"/>"#,
        "MENU" => r#"<path d="M4 7h16M4 12h16M4 17h16"/>"#,
        "NEW" => {
            r#"<path d="M6.5 3.5h7.5l4.5 4.5v11.5a1 1 0 0 1-1 1h-11a1 1 0 0 1-1-1v-15a1 1 0 0 1 1-1Z"/><path d="M14 3.5V8h4.5M12 11.5v6M9 14.5h6"/>"#
        }
        "OPEN" => r#"<path d="M3.5 7v11a1 1 0 0 0 1 1h15a1 1 0 0 0 1-1V9.5a1 1 0 0 0-1-1h-7.5l-2-2.5h-5.5a1 1 0 0 0-1 1Z"/>"#,
        "SAVE" => {
            r#"<path d="M5.5 4h10.5l3.5 3.5V19a1 1 0 0 1-1 1h-13a1 1 0 0 1-1-1V5a1 1 0 0 1 1-1Z"/><path d="M8 4v4.5h7V4M8 20v-6h8v6"/>"#
        }
        "UNDO" => r#"<path d="M9 6 4.5 10.5 9 15M4.5 10.5H15a4.5 4.5 0 0 1 0 9H10"/>"#,
        "REDO" => r#"<path d="m15 6 4.5 4.5L15 15M19.5 10.5H9a4.5 4.5 0 0 0 0 9h5"/>"#,
        "ZOOM_EXTENTS" => r#"<path d="M4 9V4h5M15 4h5v5M20 15v5h-5M9 20H4v-5"/><rect x="8.5" y="8.5" width="7" height="7" rx="1"/>"#,
        "ELLIPSE" => r#"<ellipse cx="12" cy="12" rx="8.5" ry="5"/><circle cx="12" cy="12" r="1" fill="white"/>"#,
        "SPLINE" => {
            r#"<path d="M3.5 17c3-9 6-9 8.5-5s5.5 4 8.5-5"/><circle cx="3.5" cy="17" r="1.4" fill="white"/><circle cx="20.5" cy="7" r="1.4" fill="white"/>"#
        }
        "HATCH" => r#"<rect x="4" y="4" width="16" height="16" rx="1"/><path d="M4 12 12 4M4 20 20 4M12 20l8-8"/>"#,
        "POINT" => r#"<circle cx="12" cy="12" r="2" fill="white"/><path d="M12 4v4M12 16v4M4 12h4M16 12h4"/>"#,
        "STRETCH" => r#"<path d="M4 16h8l6-8"/><path d="M14 4h6v6" stroke-dasharray="2 2"/><path d="m15 13 3-5 1 4"/>"#,
        "CHAMFER" => r#"<path d="M5 4v9l6 7h9"/><path d="M5 13V20h6" stroke-dasharray="2 2.5"/>"#,
        "ARRAY" => {
            r#"<rect x="3.5" y="3.5" width="6" height="6" rx="1"/><rect x="14.5" y="3.5" width="6" height="6" rx="1"/><rect x="3.5" y="14.5" width="6" height="6" rx="1"/><rect x="14.5" y="14.5" width="6" height="6" rx="1"/>"#
        }
        "BREAK" => r#"<path d="M3.5 12h6M14.5 12h6"/><path d="m10.5 9 3 6" stroke-dasharray="1.5 2"/>"#,
        "EXPLODE" => r#"<path d="M12 3v5M12 16v5M3 12h5M16 12h5M5.5 5.5l3.5 3.5M15 15l3.5 3.5M18.5 5.5 15 9M9 15l-3.5 3.5"/>"#,
        "JOIN" => r#"<path d="M4 16h6a4 4 0 0 0 4-4V8h6"/><circle cx="12" cy="12" r="1.5" fill="white"/>"#,
        "MATCHPROP" => r#"<path d="M6 4h10v5H6Z"/><path d="M16 6.5h3v5h-7v3"/><path d="M11 14.5h2V20h-2Z"/>"#,
        "LIST" => {
            r#"<path d="M8 6h12M8 12h12M8 18h12"/><circle cx="4.5" cy="6" r="1" fill="white"/><circle cx="4.5" cy="12" r="1" fill="white"/><circle cx="4.5" cy="18" r="1" fill="white"/>"#
        }
        "INSERT" => r#"<rect x="9" y="9" width="11" height="11" rx="1.5"/><path d="M3.5 3.5 11 11M11 6v5H6"/>"#,
        "BLOCK" => {
            r#"<rect x="4" y="4" width="16" height="16" rx="1.5" stroke-dasharray="2.5 2"/><circle cx="10" cy="10" r="3"/><path d="M12 17h5v-5"/>"#
        }
        "PASTECLIP" => r#"<rect x="5" y="5" width="14" height="16" rx="1.5"/><path d="M9 5V3.5h6V5M9 11h6M9 15h4"/>"#,
        "ID" => r#"<path d="M12 4v16M4 12h16"/><circle cx="12" cy="12" r="3"/>"#,
        "ZOOM" => r#"<circle cx="10.5" cy="10.5" r="6"/><path d="m15 15 5 5"/>"#,
        "TEXTEDIT" => r#"<path d="M5 7V5.5h10V7M10 5.5v13M7.5 18.5h5"/><path d="m14 18 6-6 1.5 1.5-6 6H14Z"/>"#,
        "WIN_MINIMIZE" => r#"<path d="M6 12h12"/>"#,
        "WIN_MAXIMIZE" => r#"<rect x="6" y="6" width="12" height="12" rx="1"/>"#,
        "WIN_RESTORE" => {
            r#"<rect x="5.5" y="9.5" width="9" height="9" rx="1"/><path d="M9.5 9.5v-3a1 1 0 0 1 1-1h8a1 1 0 0 1 1 1v8a1 1 0 0 1-1 1h-3"/>"#
        }
        "WIN_CLOSE" => r#"<path d="m6 6 12 12M18 6 6 18"/>"#,
        "PLUS" => r#"<path d="M12 5v14M5 12h14"/>"#,
        "GRID" => r#"<rect x="4" y="4" width="16" height="16" rx="1.5"/><path d="M4 9.3h16M4 14.7h16M9.3 4v16M14.7 4v16"/>"#,
        "SNAP" => r#"<path d="m12 4 8 8-8 8-8-8Z"/><circle cx="12" cy="12" r="1.3" fill="white"/>"#,
        "ORTHO" => r#"<rect x="5" y="5" width="14" height="14" rx="1.5"/>"#,
        "POLAR" => r#"<circle cx="12" cy="12" r="8"/><path d="m8.5 15.5 7-7"/>"#,
        "OTRACK" => {
            r#"<circle cx="8" cy="8" r="2.2"/><path d="M10.5 8H21" stroke-dasharray="2 2"/><path d="M8 10.5V21" stroke-dasharray="2 2"/>"#
        }
        "DYN" => r#"<path d="M4 4l6 14 2-6 6-2Z"/><rect x="13" y="15" width="8" height="5" rx="1"/>"#,
        "MSPACE" => r#"<rect x="3.5" y="4.5" width="17" height="15" rx="1.5"/><rect x="7" y="8" width="10" height="8" rx="0.5"/>"#,
        "LWT" => r#"<path d="M4 6h16" stroke-width="1"/><path d="M4 11.5h16" stroke-width="2.2"/><path d="M4 18h16" stroke-width="3.6"/>"#,
        "LWEIGHT" => {
            r#"<path d="M4 6h16" stroke-width="1"/><path d="M4 11.5h16" stroke-width="2.2"/><path d="M4 18h16" stroke-width="3.6"/>"#
        }
        "OSNAP" => r#"<circle cx="12" cy="12" r="6.5"/><path d="M12 2.5v5M12 16.5v5M2.5 12h5M16.5 12h5"/>"#,
        "ZOOM_IN" => r#"<circle cx="10.5" cy="10.5" r="6"/><path d="m15 15 5 5M10.5 8v5M8 10.5h5"/>"#,
        "ZOOM_OUT" => r#"<circle cx="10.5" cy="10.5" r="6"/><path d="m15 15 5 5M8 10.5h5"/>"#,
        _ => r#"<circle cx="12" cy="12" r="2"/>"#,
    }
}

fn svg(name: &str) -> Bytes {
    let svg = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{RENDER_SIZE}" height="{RENDER_SIZE}" viewBox="0 0 24 24" fill="none" stroke="white" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">{}</svg>"##,
        shape(name)
    );
    Bytes::Shared(Arc::from(svg.into_bytes().into_boxed_slice()))
}

pub fn image(name: &str, tint: Color32) -> egui::Image<'static> {
    egui::Image::new(ImageSource::Bytes { uri: format!("bytes://icon/{name}.svg").into(), bytes: svg(name) }).tint(tint)
}

/// Paint an icon into a square of the given size centered in the rectangle.
pub fn paint(ui: &Ui, name: &str, rect: Rect, size: f32, tint: Color32) {
    let square = Rect::from_center_size(rect.center(), egui::vec2(size, size));
    image(name, tint).paint_at(ui, square);
}
