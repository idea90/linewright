//! The icon family, after AutoCAD's ribbon on a dark theme: 24x24 white line art (1.6 stroke, round caps and joins)
//! with colored parts: blue grips and construction, yellow layers, lamps and suns, gold padlocks, a red eraser.
//!
//! In a snippet, `white` is line art and takes the tint it is shown with (hovered, checked, disabled); a `#rrggbb`
//! color is art that keeps its color and is only darkened as much as a dim tint darkens. Both happen in the SVG
//! before it is rasterized, rather than through egui's image tint, which multiplies every pixel and would turn a
//! yellow lamp green on a checked button. Each tint is a texture of its own, cached under a URI naming the tint.

use std::sync::Arc;

use eframe::egui::{self, Color32, ImageSource, Rect, Ui, load::Bytes};

const RENDER_SIZE: u32 = 64; // rasterized this large so they stay sharp on high-DPI screens

/// A yellow pencil in the lower right corner: editing what the rest of the icon shows.
macro_rules! pencil {
    () => {
        r##"<path d="m12.5 21.5.7-3.1 6.6-6.6 2.4 2.4-6.6 6.6Z" fill="#f5c04a" stroke="#a8740f" stroke-width="1"/><path d="m12.5 21.5.7-3.1 2.4 2.4Z" fill="#4b3a1a" stroke="none"/>"##
    };
}

/// A small gear in the lower right corner: the styles or settings of what the rest shows.
macro_rules! gear {
    () => {
        r##"<g stroke="#b9c2c6" stroke-width="1.4"><circle cx="18" cy="18" r="2.3" fill="#2d353a"/><path d="M18 13.4v1.5M18 21.1v1.5M13.4 18h1.5M21.1 18h1.5M14.7 14.7l1.1 1.1M20.2 20.2l1.1 1.1M21.3 14.7l-1.1 1.1M15.8 20.2l-1.1 1.1"/></g>"##
    };
}

/// A yellow sparkle in the upper right corner: something new is made.
macro_rules! star {
    () => {
        r##"<path d="M18 .5 18.57 4.61 20.26 3.74 19.39 5.43 23.5 6 19.39 6.57 20.26 8.26 18.57 7.39 18 11.5 17.43 7.39 15.74 8.26 16.61 6.57 12.5 6 16.61 5.43 15.74 3.74 17.43 4.61Z" fill="#f7c843" stroke="#d08a16" stroke-width=".6"/>"##
    };
}

macro_rules! plus_badge {
    () => {
        r##"<circle cx="18" cy="18" r="4.8" fill="#3e9b48" stroke="none"/><path d="M18 15.3v5.4M15.3 18h5.4" stroke="#ffffff"/>"##
    };
}

macro_rules! minus_badge {
    () => {
        r##"<circle cx="18" cy="18" r="4.8" fill="#d6463d" stroke="none"/><path d="M15.3 18h5.4" stroke="#ffffff"/>"##
    };
}

macro_rules! cross_badge {
    () => {
        r##"<circle cx="18" cy="18" r="4.8" fill="#d6463d" stroke="none"/><path d="m16 16 4 4M20 16l-4 4" stroke="#ffffff"/>"##
    };
}

macro_rules! check_badge {
    () => {
        r##"<circle cx="18" cy="18" r="4.8" fill="#3e9b48" stroke="none"/><path d="m15.6 18.1 1.7 1.7 3.2-3.4" stroke="#ffffff"/>"##
    };
}

/// A white mouse pointer in the lower right corner.
macro_rules! pointer {
    () => {
        r##"<path d="m13.5 12.5 8 3.2-3.4 1.2 2.6 3.6-1.6 1.2-2.6-3.6-2.2 2.8Z" fill="#f2f4f5" stroke="#1c2226" stroke-width="1"/>"##
    };
}

/// Three stacked yellow layer sheets in the upper left, for the layer tools that add a glyph below right.
macro_rules! small_sheets {
    () => {
        r##"<g stroke="#a86f12" stroke-width=".8"><path d="m1.5 13.5 4.5-3.2h10.5l-4.5 3.2Z" fill="#e09a2a"/><path d="m1.5 10 4.5-3.2h10.5L12 10Z" fill="#f0b43c"/><path d="m1.5 6.5 4.5-3.2h10.5L12 6.5Z" fill="#f7cf5a"/></g>"##
    };
}

/// Puts a full-size glyph small in the lower right corner, keeping its lines readable.
macro_rules! corner {
    ($glyph:expr) => {
        concat!(r#"<g transform="translate(10.5 9.5) scale(.58)" stroke-width="2.6">"#, $glyph, "</g>")
    };
}

macro_rules! bulb_on {
    () => {
        r##"<path d="M12 2.5a6.5 6.5 0 0 0-3.9 11.7c.8.6 1.4 1.6 1.4 2.6v.7h5v-.7c0-1 .6-2 1.4-2.6A6.5 6.5 0 0 0 12 2.5Z" fill="#f7d04a" stroke="#c99a1d" stroke-width="1"/><path d="M9.7 19.6h4.6M10.6 21.8h2.8" stroke="#b9c2c6"/>"##
    };
}

macro_rules! bulb_off {
    () => {
        r##"<path d="M12 2.5a6.5 6.5 0 0 0-3.9 11.7c.8.6 1.4 1.6 1.4 2.6v.7h5v-.7c0-1 .6-2 1.4-2.6A6.5 6.5 0 0 0 12 2.5Z" fill="#5d7487" stroke="#8fa4b4" stroke-width="1"/><path d="M9.7 19.6h4.6M10.6 21.8h2.8" stroke="#8a9599"/>"##
    };
}

macro_rules! sun {
    () => {
        r##"<circle cx="12" cy="12" r="4.3" fill="#f7c843" stroke="#d39a1c" stroke-width="1"/><path d="M12 2.5v3M12 18.5v3M2.5 12h3M18.5 12h3M5.3 5.3l2.1 2.1M16.6 16.6l2.1 2.1M18.7 5.3l-2.1 2.1M7.4 16.6l-2.1 2.1" stroke="#f7c843"/>"##
    };
}

macro_rules! snowflake {
    () => {
        r##"<g stroke="#62b6f2"><path d="M12 3v18m-2.6-16.4 2.6 2 2.6-2m-5.2 14.8 2.6-2 2.6 2"/><path transform="rotate(60 12 12)" d="M12 3v18m-2.6-16.4 2.6 2 2.6-2m-5.2 14.8 2.6-2 2.6 2"/><path transform="rotate(120 12 12)" d="M12 3v18m-2.6-16.4 2.6 2 2.6-2m-5.2 14.8 2.6-2 2.6 2"/></g>"##
    };
}

macro_rules! lock_closed {
    () => {
        r##"<path d="M8 11V8a4 4 0 0 1 8 0v3" stroke="#d8dde0" stroke-width="2"/><rect x="5.5" y="10.5" width="13" height="10.5" rx="1.5" fill="#f2b632" stroke="#a8740f" stroke-width="1"/><path d="M12 14.3v3.2" stroke="#7a5310" stroke-width="2"/>"##
    };
}

macro_rules! lock_open {
    () => {
        r##"<path d="M8 11V6.5a4 4 0 0 1 7.7-1.5" stroke="#d8dde0" stroke-width="2"/><rect x="5.5" y="10.5" width="13" height="10.5" rx="1.5" fill="#f2b632" stroke="#a8740f" stroke-width="1"/><path d="M12 14.3v3.2" stroke="#7a5310" stroke-width="2"/>"##
    };
}

/// A block: a frame holding a circle and a line, the contents a block definition names.
macro_rules! block_art {
    () => {
        r#"<rect x="3.5" y="5.5" width="13" height="13" rx="1"/><circle cx="8.5" cy="10.5" r="2.8"/><path d="M6 15.5h7.5V9"/>"#
    };
}

/// A sheet of paper with a folded corner, outline only.
macro_rules! page {
    () => {
        r##"<path d="M5.5 2.5h8.5l4.5 4.5v14.5h-13Z" fill="#f2f4f5" stroke="#8a9599" stroke-width="1"/><path d="M14 2.5V7h4.5" stroke="#8a9599" stroke-width="1"/>"##
    };
}

/// The bracketed frame of a group, holding a square and a circle.
macro_rules! group_art {
    () => {
        r#"<path d="M6.5 4H3.5v16h3M17.5 4h3v4.5M17.5 20h3v-2"/><rect x="6.5" y="7" width="5.5" height="5.5" rx=".5"/><circle cx="10.5" cy="15.5" r="2.7"/>"#
    };
}

/// The multileader: a blue arrowhead, a leader line and two lines of text.
macro_rules! leader_art {
    () => {
        r##"<path d="M4.5 19.5 11 9h3"/><path d="m3.6 21 .6-4.6 3 1.9Z" fill="#4ea3f1" stroke="#4ea3f1" stroke-width="1"/><path d="M15.5 7.5h5.5M15.5 11h4"/>"##
    };
}

macro_rules! linear_dim {
    () => {
        r##"<path d="M4.5 6v12.5M19.5 6v12.5"/><path d="M5 11h14" stroke="#4ea3f1"/><path d="M4.5 11 8.5 9v4ZM19.5 11l-4-2v4Z" fill="#4ea3f1" stroke="none"/><path d="M10 7h4" stroke-width="2"/>"##
    };
}

macro_rules! spline_path {
    () => {
        r#"<path d="M3.5 18C8 4 13 21 20.5 6"/>"#
    };
}

macro_rules! hatch_art {
    () => {
        r##"<path d="M3.5 9.5l6-6M3.5 15.5l12-12M3.5 20.5l17-17M9.5 20.5l11-11M15.5 20.5l5-5" stroke="#4ea3f1" stroke-width="1.2"/><rect x="3.5" y="3.5" width="17" height="17" rx=".5"/>"##
    };
}

macro_rules! table_art {
    () => {
        r##"<rect x="3" y="4" width="18" height="4.5" fill="#4ea3f1" stroke="none"/><rect x="3" y="4" width="18" height="16" rx=".5"/><path d="M3 12.5h18M3 16.3h18M9 8.5V20M15 8.5V20" stroke-width="1.2"/>"##
    };
}

macro_rules! mline_art {
    () => {
        r##"<path d="M4 9h11v11M4 13h7v7"/><path d="M4 11h9v9" stroke="#4ea3f1" stroke-width="1" stroke-dasharray="1.5 1.5"/>"##
    };
}

macro_rules! scissors {
    () => {
        r#"<circle cx="7.5" cy="18" r="2.6"/><circle cx="16.5" cy="18" r="2.6"/><path d="M9.2 16 17 3.5M14.8 16 7 3.5"/>"#
    };
}

macro_rules! dashes {
    () => {
        r#"<path d="M3 6h4M10 6h4M17 6h4" stroke-width="1.8"/><path d="M3 12h7M13 12h1.5M17.5 12H21" stroke-width="1.8"/><path d="M3 18h.5M7 18h.5M11 18h.5M15 18h.5M19 18h.5" stroke-width="1.8"/>"#
    };
}

macro_rules! weights {
    () => {
        r#"<path d="M4 6h16" stroke-width="1"/><path d="M4 11.5h16" stroke-width="2.2"/><path d="M4 18h16" stroke-width="3.6"/>"#
    };
}

macro_rules! magnifier {
    () => {
        r##"<circle cx="10.5" cy="10.5" r="6.3" fill="#4ea3f1" fill-opacity=".25"/><path d="m15.2 15.2 5.3 5.3" stroke-width="2.4"/>"##
    };
}

/// The snippet drawing the named icon, or None when there is none of its own.
fn shape(name: &str) -> Option<&'static str> {
    Some(match name {
        // Draw.
        "LINE" => {
            r##"<path d="M5.5 18.5 18.5 5.5"/><g fill="#4ea3f1" stroke="none"><rect x="3.6" y="16.6" width="3.8" height="3.8"/><rect x="16.6" y="3.6" width="3.8" height="3.8"/></g>"##
        }
        "PLINE" => {
            r##"<path d="M3.5 17.5H13a4.75 4.75 0 0 0 0-9.5h-2"/><g fill="#4ea3f1" stroke="none"><rect x="1.7" y="15.7" width="3.6" height="3.6"/><rect x="11.2" y="15.7" width="3.6" height="3.6"/><rect x="9.2" y="6.2" width="3.6" height="3.6"/></g>"##
        }
        "RECTANGLE" => {
            r##"<rect x="4" y="6.5" width="16" height="11"/><g fill="#4ea3f1" stroke="none"><rect x="2.2" y="15.7" width="3.6" height="3.6"/><rect x="18.2" y="4.7" width="3.6" height="3.6"/></g>"##
        }
        "CIRCLE" => {
            r##"<circle cx="12" cy="12" r="8.5"/><path d="M12 12 18 6" stroke="#4ea3f1"/><rect x="10.2" y="10.2" width="3.6" height="3.6" fill="#4ea3f1" stroke="none"/>"##
        }
        "ARC" => {
            r##"<path d="M4.5 19.5A14 14 0 0 1 19.5 4.5"/><g fill="#4ea3f1" stroke="none"><rect x="2.7" y="17.7" width="3.6" height="3.6"/><rect x="6.8" y="6.8" width="3.6" height="3.6"/><rect x="17.7" y="2.7" width="3.6" height="3.6"/></g>"##
        }
        "ELLIPSE" => {
            r##"<path d="M3 12h18M12 6.5v11" stroke="#4ea3f1" stroke-width="1" stroke-dasharray="1.5 1.5"/><ellipse cx="12" cy="12" rx="9" ry="5.5"/><rect x="10.3" y="10.3" width="3.4" height="3.4" fill="#4ea3f1" stroke="none"/>"##
        }
        "SPLINE" => concat!(
            r##"<path d="M3.5 18 8 4.5l5 16 7.5-14.5" stroke="#4ea3f1" stroke-width="1" stroke-dasharray="1.5 1.5"/>"##,
            spline_path!(),
            r##"<g fill="#4ea3f1" stroke="none"><rect x="1.8" y="16.3" width="3.4" height="3.4"/><rect x="18.8" y="4.3" width="3.4" height="3.4"/></g>"##
        ),
        "POINT" => {
            r##"<path d="M12 3.5v17M3.5 12h17" stroke-width="1"/><circle cx="12" cy="12" r="5.5"/><circle cx="12" cy="12" r="2" fill="#4ea3f1" stroke="none"/>"##
        }
        "HATCH" => hatch_art!(),
        "GRADIENT" => {
            r##"<defs><linearGradient id="shade" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#1f5c9e"/><stop offset="1" stop-color="#cfe6fa"/></linearGradient></defs><rect x="3.5" y="3.5" width="17" height="17" rx=".5" fill="url(#shade)"/>"##
        }
        "HATCHEDIT" => concat!(hatch_art!(), pencil!()),
        "BOUNDARY" => {
            r##"<path d="M3 7h18M3 17h18M7 3v18M17 3v18" stroke-width="1"/><rect x="7" y="7" width="10" height="10" fill="#4ea3f1" fill-opacity=".3" stroke="#4ea3f1" stroke-width="2.2"/>"##
        }
        "REGION" => {
            r##"<path d="M4 6h10a6 6 0 0 1 6 6v6H4Z" fill="#4ea3f1" fill-opacity=".3" stroke="#4ea3f1"/><circle cx="10" cy="13" r="2.5"/>"##
        }
        "UNION" => r##"<path d="M3.5 5.5h10v4h7v9h-10v-4h-7Z" fill="#4ea3f1" fill-opacity=".3" stroke="#4ea3f1"/>"##,
        "SUBTRACT" => {
            r##"<path d="M3.5 5.5h10v4h-3v5h-7Z" fill="#4ea3f1" fill-opacity=".3" stroke="#4ea3f1"/><rect x="10.5" y="9.5" width="10" height="9" stroke-dasharray="2 1.5"/>"##
        }
        "INTERSECT" => {
            r##"<rect x="3.5" y="5.5" width="10" height="9" stroke-dasharray="2 1.5"/><rect x="10.5" y="9.5" width="10" height="9" stroke-dasharray="2 1.5"/><rect x="10.5" y="9.5" width="3" height="5" fill="#4ea3f1" stroke="#4ea3f1"/>"##
        }
        "POLYGON" => {
            r##"<path d="M12 3.5 19.36 7.75v8.5L12 20.5l-7.36-4.25v-8.5Z"/><path d="M12 12 19.36 7.75" stroke="#4ea3f1"/><rect x="10.2" y="10.2" width="3.6" height="3.6" fill="#4ea3f1" stroke="none"/>"##
        }
        "REVCLOUD" => {
            r#"<path d="M5 16a2.5 2.5 0 0 1 0-5 3 3 0 0 1 4-3.5 3 3 0 0 1 5-1.5 3 3 0 0 1 5 2.5 2.5 2.5 0 0 1 0 5 3 3 0 0 1-4.5 3 3 3 0 0 1-5 0A3 3 0 0 1 5 16Z"/>"#
        }
        "WIPEOUT" => {
            r##"<path d="M2.5 6h19M2.5 12h19M2.5 18h19" stroke-width="1.2"/><path d="M6 4.5h11l3 7-4 8H7l-2.5-7Z" fill="#20272c" stroke="#4ea3f1"/>"##
        }
        "XLINE" => {
            r##"<path d="M2 18 22 6"/><g fill="#4ea3f1" stroke="none"><rect x="6.2" y="12.6" width="3.6" height="3.6"/><rect x="14.2" y="7.8" width="3.6" height="3.6"/></g>"##
        }
        "RAY" => {
            r##"<path d="M5 17.5 21 6.5M16.6 6.4l4.4.1-1.6 4"/><rect x="3.2" y="15.7" width="3.6" height="3.6" fill="#4ea3f1" stroke="none"/>"##
        }
        "DONUT" => {
            r##"<circle cx="12" cy="12" r="6.3" stroke-width="4.2"/><circle cx="12" cy="12" r="1.5" fill="#4ea3f1" stroke="none"/>"##
        }
        "DIVIDE" => {
            r##"<path d="M3 15h18M3 12.5v5M21 12.5v5"/><path d="m6.8 7.8 2.4 2.4m0-2.4-2.4 2.4M10.8 7.8l2.4 2.4m0-2.4-2.4 2.4M14.8 7.8l2.4 2.4m0-2.4-2.4 2.4" stroke="#4ea3f1" stroke-width="1.3"/><path d="M8 13.5v3M12 13.5v3M16 13.5v3" stroke="#4ea3f1"/>"##
        }
        "MEASURE" => {
            r##"<path d="M3 16h18"/><path d="M3 6v5M9 6v5M3 8.5h6" stroke-width="1"/><path d="M9 14v4M15 14v4M21 14v4" stroke="#4ea3f1"/><circle cx="3" cy="16" r="1.6" fill="#4ea3f1" stroke="none"/>"##
        }
        "MLINE" => mline_art!(),
        "MLEDIT" => concat!(r#"<path d="M3 8h18M3 12h7v8M14 12h7"/><path d="M14 12v3" stroke-dasharray="1.5 1.5"/>"#, pencil!()),
        "MLSTYLE" => concat!(mline_art!(), gear!()),
        "TEXT" => r#"<path d="M5.5 20.5 12 3.5l6.5 17M8 14.5h8"/>"#,
        "MTEXT" => r#"<path d="M3 12.5 7 3.5l4 9M4.5 9.5h5"/><path d="M14 5h7M14 9h7M3 16h18M3 20h13" stroke-width="1.4"/>"#,
        "TEXTEDIT" => concat!(r#"<path d="M3 17.5 8.5 3.5l5.5 14M5 12.5h7"/>"#, pencil!()),
        "STYLE" => concat!(r#"<path d="M3 18 8.5 3.5 14 18M5 13h7"/>"#, gear!()),
        "TABLE" => table_art!(),
        "TABLEDIT" => concat!(table_art!(), pencil!()),
        // Modify.
        "MOVE" => {
            r##"<path d="M12 3v18M3 12h18M9 6l3-3 3 3M9 18l3 3 3-3M6 9l-3 3 3 3M18 9l3 3-3 3"/><rect x="10.2" y="10.2" width="3.6" height="3.6" fill="#4ea3f1" stroke="none"/>"##
        }
        "COPY" | "NCOPY" => r##"<circle cx="8.5" cy="8.5" r="5"/><circle cx="15.5" cy="15.5" r="5" stroke="#4ea3f1"/>"##,
        "ROTATE" => {
            r##"<path d="M19 12a7 7 0 1 1-2.2-5.1"/><path d="M19.5 3.5V8H15"/><path d="M12 12 16.5 16.5" stroke="#4ea3f1"/><rect x="10.2" y="10.2" width="3.6" height="3.6" fill="#4ea3f1" stroke="none"/>"##
        }
        "SCALE" => {
            r##"<rect x="3.5" y="3.5" width="17" height="17" rx=".5"/><rect x="3.5" y="11.5" width="9" height="9" fill="#4ea3f1" fill-opacity=".3" stroke="#4ea3f1"/><path d="M12.5 11.5 18 6M14 6h4v4"/>"##
        }
        "MIRROR" => {
            r##"<path d="M12 2.5v19" stroke-width="1" stroke-dasharray="2 2"/><path d="M9.5 6 3 18h6.5Z"/><path d="M14.5 6 21 18h-6.5Z" stroke="#4ea3f1" fill="#4ea3f1" fill-opacity=".3"/>"##
        }
        "OFFSET" => {
            r##"<path d="M4 20A16 16 0 0 1 20 4" stroke="#4ea3f1"/><path d="M4 14.5A10.5 10.5 0 0 1 14.5 4"/><path d="M4 9a5 5 0 0 1 5-5" stroke="#4ea3f1"/>"##
        }
        "TRIM" => concat!(r##"<path d="M2.5 9.5h19" stroke="#4ea3f1"/>"##, scissors!()),
        "CUTCLIP" => concat!(r#"<path d="M2.5 9.5h4M17.5 9.5h4" stroke-width="1" stroke-dasharray="1.5 1.5"/>"#, scissors!()),
        "EXTEND" => r##"<path d="M20 3v18" stroke="#4ea3f1"/><path d="M3 12h9"/><path d="M12 12h6M15 9l3 3-3 3" stroke-dasharray="2 2"/>"##,
        "FILLET" => r##"<path d="M5 21v-8M12 5h8"/><path d="M5 13a8 8 0 0 1 7-8" stroke="#4ea3f1"/>"##,
        "CHAMFER" => {
            r##"<path d="M5 21v-9M12 5h8"/><path d="M5 12l7-7" stroke="#4ea3f1"/><path d="M5 12V5h7" stroke-width="1" stroke-dasharray="1.5 1.5"/>"##
        }
        "ARRAY" => {
            r##"<rect x="3.5" y="3.5" width="6.5" height="6.5" rx="1"/><g stroke="#4ea3f1"><rect x="14" y="3.5" width="6.5" height="6.5" rx="1"/><rect x="3.5" y="14" width="6.5" height="6.5" rx="1"/><rect x="14" y="14" width="6.5" height="6.5" rx="1"/></g>"##
        }
        "ERASE" => {
            r##"<g transform="rotate(-45 12 12)"><rect x="3" y="8" width="6.5" height="8" rx="1" fill="#e86a6a" stroke="#f4a3a3" stroke-width="1"/><path d="M9.5 8H20a1 1 0 0 1 1 1v6a1 1 0 0 1-1 1H9.5Z"/></g><path d="M8 21.5h12" stroke-width="1"/>"##
        }
        "OOPS" => {
            r##"<g transform="translate(-2 -2) scale(.8)"><g transform="rotate(-45 12 12)"><rect x="3" y="8" width="6.5" height="8" rx="1" fill="#e86a6a" stroke="#f4a3a3" stroke-width="1"/><path d="M9.5 8H20a1 1 0 0 1 1 1v6a1 1 0 0 1-1 1H9.5Z"/></g></g><path d="M21 17.5a4.5 4.5 0 1 1-1.3-3.2M20.3 11.8v2.9h-2.9" stroke="#5fbf5a"/>"##
        }
        "EXPLODE" | "BURST" => {
            r##"<path d="M3.5 8V3.5H8M16 3.5h4.5V8M20.5 16v4.5H16M8 20.5H3.5V16"/><path d="M12 4.5 12.77 10.15 15.18 8.82 13.85 11.23 19.5 12 13.85 12.77 15.18 15.18 12.77 13.85 12 19.5 11.23 13.85 8.82 15.18 10.15 12.77 4.5 12 10.15 11.23 8.82 8.82 11.23 10.15Z" fill="#f7c843" stroke="#d08a16" stroke-width=".8"/>"##
        }
        "JOIN" => r##"<path d="M3.5 18 10 11.5h10.5"/><rect x="8.2" y="9.7" width="3.6" height="3.6" fill="#4ea3f1" stroke="none"/>"##,
        "BREAK" => {
            r##"<path d="M3 12h6M15 12h6"/><g fill="#4ea3f1" stroke="none"><rect x="7.3" y="10.3" width="3.4" height="3.4"/><rect x="13.3" y="10.3" width="3.4" height="3.4"/></g><path d="M12 6v12" stroke-width="1" stroke-dasharray="1.5 1.5"/>"##
        }
        "LENGTHEN" => {
            r##"<path d="M3 15h11"/><path d="M14 15h7" stroke="#4ea3f1" stroke-dasharray="2 2"/><path d="M13 9h7m-2.5-2.5L20 9l-2.5 2.5" stroke="#4ea3f1"/><rect x="1.3" y="13.3" width="3.4" height="3.4" fill="#4ea3f1" stroke="none"/>"##
        }
        "ALIGN" => {
            r##"<rect x="3.5" y="12.5" width="7" height="7" transform="rotate(-25 7 16)"/><rect x="13.5" y="3.5" width="7" height="7" stroke="#4ea3f1" stroke-dasharray="2 1.5"/><path d="M9 11.5 13.5 7M10 7h3.5v3.5" stroke="#4ea3f1"/>"##
        }
        "STRETCH" => {
            r##"<path d="M3 20.5h9l3-7"/><rect x="11.5" y="3.5" width="9.5" height="12" stroke="#4ea3f1" stroke-width="1" stroke-dasharray="2 1.5"/><path d="M15 13.5 18 6.5M14.5 7h3.5v3.5"/>"##
        }
        "PEDIT" => concat!(r#"<path d="M2.5 16H10a4 4 0 0 0 0-8H8"/>"#, pencil!()),
        "SPLINEDIT" => concat!(spline_path!(), pencil!()),
        "DRAWORDER" => {
            r##"<rect x="3.5" y="3.5" width="11" height="11" rx="1"/><rect x="9.5" y="9.5" width="11" height="11" rx="1" fill="#2f6fb3" stroke="#6bb5f5"/>"##
        }
        "OVERKILL" => r##"<path d="M3 8.5h12M3 12h12"/><path d="m15.5 15.5 5 5M20.5 15.5l-5 5" stroke="#e5534b" stroke-width="2"/>"##,
        "MATCHPROP" => {
            r##"<rect x="2.5" y="2.5" width="11" height="8.5" rx="1" fill="#4ea3f1" fill-opacity=".3" stroke="#4ea3f1"/><g transform="rotate(-45 15 15)" stroke-width="1"><rect x="13.8" y="16" width="2.4" height="8" rx="1" fill="#c49358" stroke="#8f6534"/><rect x="13" y="12.5" width="4" height="3.5" fill="#b9c2c6" stroke="#7d878c"/><path d="M13 12.5v-3c0-1.5 1-3 2-4 1 1 2 2.5 2 4v3Z" fill="#f5c04a" stroke="#a8740f"/></g>"##
        }
        "COPYCLIP" | "COPYBASE" => {
            r##"<path d="M3.5 2.5h8l3 3V16h-11Z" fill="#b9c2c6" stroke="#7d878c" stroke-width="1"/><path d="M9.5 8h8l3 3v10.5h-11Z" fill="#f2f4f5" stroke="#8a9599" stroke-width="1"/><path d="M12 14h6M12 17h6" stroke="#8a9599" stroke-width="1"/>"##
        }
        "PASTECLIP" | "PASTEORIG" | "PASTEBLOCK" => {
            r##"<rect x="3" y="4.5" width="13" height="17" rx="1.2" fill="#c8955a" stroke="#8f6534" stroke-width="1"/><rect x="6.5" y="2.5" width="6" height="4" rx="1" fill="#b9c2c6" stroke="#6d777b" stroke-width="1"/><path d="M10 10h7.5l3 3v8.5H10Z" fill="#f2f4f5" stroke="#8a9599" stroke-width="1"/><path d="M12.5 15.5h5.5M12.5 18.5h5.5" stroke="#8a9599" stroke-width="1"/>"##
        }
        "SELECTALL" => {
            r##"<rect x="3" y="3" width="18" height="18" rx="1" stroke="#4ea3f1" stroke-dasharray="2 1.8"/><circle cx="9" cy="10" r="3"/><path d="M13 7.5h4.5v4.5M7 17l10-1"/>"##
        }
        "QSELECT" => concat!(r##"<path d="M3 4h15l-5.5 6.5v6l-4 2.5v-8.5Z" fill="#4ea3f1" fill-opacity=".3"/>"##, pointer!()),
        "SELECTSIMILAR" => concat!(
            r##"<rect x="2.5" y="3" width="7" height="7" rx="1" fill="#3fd06a" fill-opacity=".3" stroke="#3fd06a"/><rect x="11" y="3" width="7" height="7" rx="1" fill="#3fd06a" fill-opacity=".3" stroke="#3fd06a"/><circle cx="6" cy="16.5" r="3.5"/>"##,
            pointer!()
        ),
        // Inquiry and utilities.
        "LIST" => {
            r##"<path d="M8 6h12M8 12h12M8 18h12"/><g fill="#4ea3f1" stroke="none"><circle cx="4.5" cy="6" r="1.4"/><circle cx="4.5" cy="12" r="1.4"/><circle cx="4.5" cy="18" r="1.4"/></g>"##
        }
        "ID" => {
            r##"<path d="M12 3v18M3 12h18" stroke-width="1"/><circle cx="12" cy="12" r="4"/><circle cx="12" cy="12" r="1.8" fill="#4ea3f1" stroke="none"/>"##
        }
        "AREA" => {
            r##"<path d="M4.5 17.5 7 5.5l12 3.5-2 9.5Z" fill="#4ea3f1" fill-opacity=".35"/><g fill="#4ea3f1" stroke="none"><rect x="2.8" y="15.8" width="3.4" height="3.4"/><rect x="5.3" y="3.8" width="3.4" height="3.4"/><rect x="17.3" y="7.3" width="3.4" height="3.4"/><rect x="15.3" y="16.8" width="3.4" height="3.4"/></g>"##
        }
        "DISTANCE" => {
            r##"<g transform="rotate(-35 12 12)"><rect x="2" y="8.5" width="20" height="7" rx=".5" fill="#f5c04a" stroke="#a8740f" stroke-width="1"/><path d="M5 8.5v3M8 8.5v2M11 8.5v3M14 8.5v2M17 8.5v3M20 8.5v2" stroke="#6b4c0c" stroke-width="1"/></g>"##
        }
        "MEASUREGEOM" => {
            r##"<rect x="1.5" y="8.5" width="21" height="7" rx=".5" fill="#f5c04a" stroke="#a8740f" stroke-width="1"/><path d="M4.5 8.5v3M7.5 8.5v2M10.5 8.5v3M13.5 8.5v2M16.5 8.5v3M19.5 8.5v2" stroke="#6b4c0c" stroke-width="1"/>"##
        }
        "QUICKCALC" | "CAL" => {
            r##"<rect x="5" y="2.5" width="14" height="19" rx="1.5"/><rect x="7.5" y="5" width="9" height="4" fill="#4ea3f1" fill-opacity=".45" stroke="#4ea3f1" stroke-width="1"/><g fill="white" stroke="none"><rect x="7.5" y="11.5" width="2.2" height="2"/><rect x="10.9" y="11.5" width="2.2" height="2"/><rect x="14.3" y="11.5" width="2.2" height="2"/><rect x="7.5" y="15" width="2.2" height="2"/><rect x="10.9" y="15" width="2.2" height="2"/><rect x="7.5" y="18" width="2.2" height="1.5"/><rect x="10.9" y="18" width="2.2" height="1.5"/></g><rect x="14.3" y="15" width="2.2" height="4.5" fill="#f08a32" stroke="none"/>"##
        }
        "FIND" => concat!(r#"<path d="M3 21h9M17 21h4" stroke-width="1.2"/>"#, magnifier!()),
        "ZOOM" => magnifier!(),
        "ZOOM_IN" => concat!(magnifier!(), r#"<path d="M10.5 7.5v6M7.5 10.5h6"/>"#),
        "ZOOM_OUT" => concat!(magnifier!(), r#"<path d="M7.5 10.5h6"/>"#),
        "ZOOM_EXTENTS" => {
            r##"<path d="M4 9V4h5M15 4h5v5M20 15v5h-5M9 20H4v-5"/><rect x="8" y="8" width="8" height="8" rx="1" fill="#4ea3f1" fill-opacity=".3" stroke="#4ea3f1"/>"##
        }
        "UNITS" => {
            r##"<path d="M4 3.5h3.5v13h13V20H4Z"/><path d="M7.5 7h-2M7.5 10.5h-2M7.5 14h-2M11 16.5v2M14.5 16.5v2M18 16.5v2" stroke-width="1"/><path d="M11 8a6 6 0 0 1 6 6" stroke="#4ea3f1"/><path d="M11 14 17.5 6" stroke="#4ea3f1" stroke-width="1"/>"##
        }
        "LTSCALE" => concat!(
            r#"<path d="M3 7h4M10 7h4M17 7h4" stroke-width="1.8"/>"#,
            r##"<path d="M4 15h16M7 12l-3 3 3 3M17 12l3 3-3 3" stroke="#4ea3f1"/>"##
        ),
        "PSLTSCALE" => {
            r##"<path d="M4 3.5h11.5l4 4V20.5H4Z"/><path d="M7 11h3M12.5 11h3M7 15.5h3M12.5 15.5h3" stroke="#4ea3f1" stroke-width="1.8"/>"##
        }
        "CELTSCALE" => concat!(
            r#"<path d="M3 7h4M10 7h4M17 7h4" stroke-width="1.8"/>"#,
            r##"<path d="M3 16h2.5M8.5 16H11" stroke="#4ea3f1" stroke-width="1.8"/>"##,
            star!()
        ),
        // Dimensions and leaders.
        "DIMLINEAR" => linear_dim!(),
        "DIMALIGNED" => concat!(r#"<g transform="rotate(-30 12 12)">"#, linear_dim!(), "</g>"),
        "DIMANGULAR" => {
            r##"<path d="M4 19h16M4 19 14.5 5.5"/><path d="M15 19A11 11 0 0 0 10.75 10.32" stroke="#4ea3f1"/><g fill="#4ea3f1" stroke="none"><circle cx="15" cy="19" r="1.5"/><circle cx="10.75" cy="10.32" r="1.5"/></g>"##
        }
        "DIMRADIUS" => {
            r##"<circle cx="10" cy="14" r="7"/><path d="M10 14 20.5 3.5" stroke="#4ea3f1"/><path d="m15 9-.7 3.7 3-3Z" fill="#4ea3f1" stroke="#4ea3f1" stroke-width="1"/><circle cx="10" cy="14" r="1.5" fill="#4ea3f1" stroke="none"/>"##
        }
        "DIMDIAMETER" => {
            r##"<circle cx="12" cy="12" r="8"/><path d="M6.5 17.5 17.5 6.5" stroke="#4ea3f1"/><path d="m6.3 17.7 1-3.6 2.6 2.6ZM17.7 6.3l-1 3.6-2.6-2.6Z" fill="#4ea3f1" stroke="#4ea3f1" stroke-width="1"/>"##
        }
        "DIMARC" => {
            r##"<path d="M5 20A8 8 0 0 1 19 20"/><path d="M3 15.5A10.5 10.5 0 0 1 21 15.5" stroke="#4ea3f1"/><path d="M9.5 6.5a3 3 0 0 1 5 0" stroke-width="1.2"/>"##
        }
        "DIMORDINATE" => {
            r##"<path d="M4 4v16h16" stroke-width="1"/><path d="M4 14h6l6-5"/><path d="M10 13V4.5M16 8V4.5" stroke="#4ea3f1"/><circle cx="4" cy="20" r="1.8" fill="#4ea3f1" stroke="none"/>"##
        }
        "DIMJOGGED" => {
            r##"<path d="M4 4A17 17 0 0 1 20 20"/><path d="M4 20 9 15l.5-3.5L15 9" stroke="#4ea3f1"/><path d="m15.4 8.6-1.1 3.5-2.4-2.4Z" fill="#4ea3f1" stroke="#4ea3f1" stroke-width="1"/>"##
        }
        "DIMCONTINUE" => {
            r##"<path d="M3.5 5v14M12 5v14M20.5 5v14"/><path d="M3.5 10h17" stroke="#4ea3f1"/><g fill="#4ea3f1" stroke="none"><circle cx="3.5" cy="10" r="1.6"/><circle cx="12" cy="10" r="1.6"/><circle cx="20.5" cy="10" r="1.6"/></g>"##
        }
        "DIMBASELINE" => {
            r##"<path d="M3.5 4v16M11 12v8M20.5 4v16"/><path d="M3.5 7h17M3.5 14h7.5" stroke="#4ea3f1"/><g fill="#4ea3f1" stroke="none"><circle cx="3.5" cy="7" r="1.6"/><circle cx="20.5" cy="7" r="1.6"/><circle cx="3.5" cy="14" r="1.6"/><circle cx="11" cy="14" r="1.6"/></g>"##
        }
        "QDIM" => concat!(
            r##"<path d="M3.5 8v13M10 13v8M16.5 13v8"/><path d="M3.5 16h13" stroke="#4ea3f1"/><g fill="#4ea3f1" stroke="none"><circle cx="3.5" cy="16" r="1.5"/><circle cx="10" cy="16" r="1.5"/><circle cx="16.5" cy="16" r="1.5"/></g>"##,
            star!()
        ),
        "DIMTEDIT" => {
            r##"<path d="M4.5 10v10M19.5 10v10"/><path d="M4.5 17h15" stroke="#4ea3f1"/><path d="M12 7h7" stroke-width="2.2"/><path d="M10 15c0-4 0-8 3-9.5M11.2 5.2l1.8.3-.3 1.8" stroke="#4ea3f1" stroke-width="1.2"/>"##
        }
        "DIMSCALE" => {
            r##"<path d="M4 18h16M4 18v-4M8 18v-2M12 18v-4M16 18v-2M20 18v-4"/><path d="M7 8h10M9.5 5.5 7 8l2.5 2.5M14.5 5.5 17 8l-2.5 2.5" stroke="#4ea3f1"/>"##
        }
        "DIMTXT" => r##"<path d="M4 19.5h16" stroke="#4ea3f1"/><path d="M4 16v7M20 16v7"/><path d="M6 4h12M12 4v11" stroke-width="2.2"/>"##,
        "DIMASZ" => {
            r##"<path d="M4 3.5v17"/><path d="M4 12h16" stroke="#4ea3f1"/><path d="m4.5 12 9-5v10Z" fill="#4ea3f1" stroke="#4ea3f1" stroke-width="1"/>"##
        }
        "DIMSTYLE" => concat!(
            r##"<path d="M4 4v9M20 4v9"/><path d="M4 8h16" stroke="#4ea3f1"/><path d="M4 8l3.5-2v4ZM20 8l-3.5-2v4Z" fill="#4ea3f1" stroke="none"/>"##,
            gear!()
        ),
        "QLEADER" | "LEADER" => {
            r##"<path d="M4.5 19.5 12 10h8"/><path d="m3.6 21 .7-4.8 3.1 2.2Z" fill="#4ea3f1" stroke="#4ea3f1" stroke-width="1"/><path d="M13 6h7" stroke-width="1.2"/>"##
        }
        "MLEADER" => leader_art!(),
        "MLEADERADD" => concat!(leader_art!(), plus_badge!()),
        "MLEADERREMOVE" => concat!(leader_art!(), minus_badge!()),
        "MLEADERSTYLE" => concat!(leader_art!(), gear!()),
        // Layers.
        "LAYER" | "LAYERS" => {
            r##"<g stroke="#a86f12" stroke-width=".8"><path d="m1.5 17 5-3.5h11l-5 3.5Z" fill="#e09a2a"/><path d="m1.5 12.5 5-3.5h11l-5 3.5Z" fill="#f0b43c"/><path d="m1.5 8 5-3.5h11l-5 3.5Z" fill="#f7cf5a"/></g><rect x="13" y="11.5" width="9.5" height="10.5" rx=".8" fill="#f2f4f5" stroke="#8a9599" stroke-width="1"/><rect x="13" y="11.5" width="9.5" height="2.5" fill="#4ea3f1" stroke="none"/><path d="M15 16.5h5.5M15 19.2h5.5" stroke="#7d878c" stroke-width="1"/>"##
        }
        "LAYON" => concat!(small_sheets!(), corner!(bulb_on!())),
        "LAYOFF" => concat!(small_sheets!(), corner!(bulb_off!())),
        "LAYFRZ" => concat!(small_sheets!(), corner!(snowflake!())),
        "LAYTHW" => concat!(small_sheets!(), corner!(sun!())),
        "LAYLCK" => concat!(small_sheets!(), corner!(lock_closed!())),
        "LAYULK" => concat!(small_sheets!(), corner!(lock_open!())),
        "LAYMCUR" => concat!(small_sheets!(), check_badge!()),
        "LAYMCH" => concat!(
            small_sheets!(),
            r##"<path d="M21 21H13M15.5 18.5 13 21l2.5 2.5" stroke="#4ea3f1"/><path d="M21 21v-6" stroke="#4ea3f1"/>"##
        ),
        "LAYISO" => {
            r##"<g stroke-width=".8"><path d="m1.5 17 5-3.5h11l-5 3.5Z" fill="#5e676c" stroke="#3f474b"/><path d="m1.5 12.5 5-3.5h11l-5 3.5Z" fill="#f7cf5a" stroke="#a86f12"/><path d="m1.5 8 5-3.5h11l-5 3.5Z" fill="#5e676c" stroke="#3f474b"/></g><path d="M22 11h-3.5M20 9l-2 2 2 2" stroke="#4ea3f1"/>"##
        }
        "LAYUNISO" => concat!(small_sheets!(), r##"<path d="M21 20a4.5 4.5 0 1 1-1.3-6.2M20.3 11v3h-3" stroke="#4ea3f1"/>"##),
        "VPLAYER" => concat!(
            r##"<rect x="2.5" y="2.5" width="19" height="19" rx="1" stroke="#4ea3f1"/>"##,
            r##"<g stroke="#a86f12" stroke-width=".8"><path d="m5 16.5 4-3h9l-4 3Z" fill="#f0b43c"/><path d="m5 12 4-3h9l-4 3Z" fill="#f7cf5a"/></g>"##
        ),
        "LAYER_ON" => bulb_on!(),
        "LAYER_OFF" => bulb_off!(),
        "LAYER_THAWED" => sun!(),
        "LAYER_FROZEN" => snowflake!(),
        "LAYER_LOCKED" => lock_closed!(),
        "LAYER_UNLOCKED" => lock_open!(),
        "COLOR_WHEEL" | "COLOR" => {
            r##"<g stroke="none"><path d="M12 12 12 3.5A8.5 8.5 0 0 1 16.25 4.64Z" fill="#e8473f"/><path d="M12 12 16.25 4.64A8.5 8.5 0 0 1 19.36 7.75Z" fill="#f08a32"/><path d="M12 12 19.36 7.75A8.5 8.5 0 0 1 20.5 12Z" fill="#f5c842"/><path d="M12 12 20.5 12A8.5 8.5 0 0 1 19.36 16.25Z" fill="#b5d33f"/><path d="M12 12 19.36 16.25A8.5 8.5 0 0 1 16.25 19.36Z" fill="#4fbf5a"/><path d="M12 12 16.25 19.36A8.5 8.5 0 0 1 12 20.5Z" fill="#35b89a"/><path d="M12 12 12 20.5A8.5 8.5 0 0 1 7.75 19.36Z" fill="#3fc0d8"/><path d="M12 12 7.75 19.36A8.5 8.5 0 0 1 4.64 16.25Z" fill="#3f95e0"/><path d="M12 12 4.64 16.25A8.5 8.5 0 0 1 3.5 12Z" fill="#4a62d8"/><path d="M12 12 3.5 12A8.5 8.5 0 0 1 4.64 7.75Z" fill="#7a52cc"/><path d="M12 12 4.64 7.75A8.5 8.5 0 0 1 7.75 4.64Z" fill="#b84cc0"/><path d="M12 12 7.75 4.64A8.5 8.5 0 0 1 12 3.5Z" fill="#e04880"/></g>"##
        }
        "LINEWEIGHT" | "LWEIGHT" | "LWT" | "LWDISPLAY" => weights!(),
        "LINETYPE" => dashes!(),
        // Groups.
        "GROUP" => concat!(group_art!(), star!()),
        "UNGROUP" => concat!(group_art!(), cross_badge!()),
        "GROUPEDIT" => concat!(group_art!(), pencil!()),
        "PICKSTYLE" => concat!(group_art!(), pointer!()),
        // Blocks, attributes and references.
        "INSERT" => {
            r##"<rect x="9" y="3" width="12" height="12" rx="1" fill="#2f6fb3" stroke="#6bb5f5"/><circle cx="13.5" cy="7.5" r="2.4" stroke="#e8f2fb" stroke-width="1.3"/><path d="M11.5 12.5h7V6" stroke="#e8f2fb" stroke-width="1.3"/><path d="M3.5 20.5 10 14M5.8 14H10v4.2"/>"##
        }
        "BLOCK" => concat!(
            r##"<rect x="2.5" y="4.5" width="15" height="15" rx="1" stroke="#4ea3f1" stroke-dasharray="2 1.6"/><circle cx="8" cy="10" r="2.8"/><path d="M5.5 15.5h8V9"/>"##,
            star!()
        ),
        "BEDIT" | "REFEDIT" => concat!(block_art!(), pencil!()),
        "BCLOSE" | "REFCLOSE" => concat!(block_art!(), cross_badge!()),
        "ATTDEF" => concat!(
            r##"<path d="M2.5 8h11l4.5 4.5-4.5 4.5h-11Z"/><circle cx="13.5" cy="12.5" r="1.4" fill="#4ea3f1" stroke="none"/>"##,
            star!()
        ),
        "ATTEDIT" | "EATTEDIT" | "ATTSYNC" | "BATTMAN" => concat!(
            r##"<path d="M2.5 5h11l4.5 4.5-4.5 4.5h-11Z"/><circle cx="13.5" cy="9.5" r="1.4" fill="#4ea3f1" stroke="none"/>"##,
            pencil!()
        ),
        "XATTACH" => concat!(page!(), r##"<path d="M15 9v9a3 3 0 0 1-6 0V6.5a2 2 0 0 1 4 0V17" stroke="#2f7fd0" stroke-width="1.5"/>"##),
        "XREF" => concat!(
            page!(),
            r##"<g stroke="#2f7fd0" stroke-width="1.6"><rect x="7" y="9" width="7" height="4" rx="2" transform="rotate(-40 10.5 11)"/><rect x="10.5" y="13" width="7" height="4" rx="2" transform="rotate(-40 14 15)"/></g>"##
        ),
        "IMAGEATTACH" => {
            r##"<rect x="2.5" y="4" width="19" height="16" rx="1"/><path d="m3 19.5 6-8 4 5 3-3 5.5 6Z" fill="#4fbf5a" stroke="none"/><circle cx="16" cy="8.5" r="2" fill="#f7c843" stroke="none"/>"##
        }
        // Layouts, viewports and plotting.
        "LAYOUT" => {
            r##"<rect x="2.5" y="4" width="19" height="16" rx=".5"/><rect x="5" y="6.5" width="10" height="11" stroke="#4ea3f1" stroke-width="1.2"/><path d="M17 17.5h2.5M17 14.5h2.5" stroke-width="1.2"/>"##
        }
        "MVIEW" => {
            r##"<rect x="2.5" y="4" width="19" height="16" rx=".5"/><rect x="5.5" y="7" width="13" height="10" stroke="#4ea3f1" stroke-width="2"/><path d="m8 15 3-4 2.5 3 2-2" stroke-width="1.2"/>"##
        }
        "MSPACE" => {
            r##"<rect x="2.5" y="4" width="19" height="16" rx=".5" stroke-width="1"/><rect x="5" y="6.5" width="14" height="11" fill="#4ea3f1" fill-opacity=".25" stroke="#4ea3f1" stroke-width="2"/><path d="M12 9v6M9 12h6" stroke-width="1.2"/>"##
        }
        "PSPACE" => r##"<path d="M4.5 2.5h10l5 5v14h-15Z"/><path d="M14.5 2.5v5h5"/><path d="M7.5 18.5v-7l7 7Z" stroke="#4ea3f1"/>"##,
        "PLOT" => {
            r##"<path d="M7 9V3h10v6" fill="#f2f4f5" stroke="#8a9599" stroke-width="1"/><rect x="3" y="9" width="18" height="8" rx="1.5"/><path d="M7 14h10v7H7Z" fill="#f2f4f5" stroke="#8a9599" stroke-width="1"/><path d="M9 17h6M9 19h4" stroke="#4ea3f1" stroke-width="1"/><circle cx="18" cy="11.5" r="1" fill="#5fbf5a" stroke="none"/>"##
        }
        // Files and drawing settings.
        "NEW" => concat!(page!(), star!()),
        "OPEN" => {
            r##"<path d="M2.5 6.5v12a1 1 0 0 0 1 1h16a1 1 0 0 0 1-1V9a1 1 0 0 0-1-1h-8l-2-2.5h-6a1 1 0 0 0-1 1Z" fill="#e8a83a" stroke="#a8740f" stroke-width="1"/><path d="M2.6 19 5.5 11h17l-2.9 8Z" fill="#f7cf5a" stroke="#a8740f" stroke-width="1"/>"##
        }
        "SAVE" | "QSAVE" => {
            r##"<path d="M4 3.5h13l3.5 3.5v13.5H4Z" fill="#2f6fb3" stroke="#6bb5f5" stroke-width="1"/><rect x="7" y="3.5" width="9" height="6" fill="#f2f4f5" stroke="none"/><rect x="13" y="4.5" width="2" height="4" fill="#2f6fb3" stroke="none"/><rect x="6.5" y="13" width="11" height="7.5" fill="#d8dde0" stroke="none"/>"##
        }
        "SAVEAS" => concat!(
            r##"<path d="M3 2.5h11l3 3V17H3Z" fill="#2f6fb3" stroke="#6bb5f5" stroke-width="1"/><rect x="5.5" y="2.5" width="7.5" height="5" fill="#f2f4f5" stroke="none"/><rect x="5.5" y="11" width="9" height="6" fill="#d8dde0" stroke="none"/>"##,
            pencil!()
        ),
        "AUDIT" => concat!(page!(), check_badge!()),
        "RECOVER" => concat!(
            page!(),
            r##"<circle cx="18" cy="18" r="4.8" fill="#d6463d" stroke="none"/><path d="M18 15v6M15 18h6" stroke="#ffffff" stroke-width="1.8"/>"##
        ),
        "DRAWINGRECOVERY" => concat!(
            r##"<path d="M2.5 1.5h9.5v3" stroke="#8a9599" stroke-width="1"/>"##,
            page!(),
            r##"<circle cx="18" cy="18" r="4.8" fill="#3e9b48" stroke="none"/><path d="M20.3 18a2.3 2.3 0 1 1-.7-1.6M19.8 15.3v1.4h-1.4" stroke="#ffffff" stroke-width="1.3"/>"##
        ),
        "PURGE" => {
            r##"<path d="M21 3 12.5 11.5" stroke="#c49358" stroke-width="2"/><path d="m10.5 9.5 4 4-2 2.5c-2 2.5-5.5 4.5-9 5 .5-3.5 2.5-7 5-9Z" fill="#f5c04a" stroke="#a8740f" stroke-width="1"/><path d="m6 18 3-3M8.5 19.5l2.5-2.5" stroke="#a8740f" stroke-width="1"/>"##
        }
        "SAVETIME" => {
            r##"<circle cx="12" cy="12" r="8.5"/><path d="M12 6.5V12l3.5 2.5" stroke="#4ea3f1"/><circle cx="12" cy="12" r="1.2" fill="#4ea3f1" stroke="none"/>"##
        }
        "OSNAP" => r#"<circle cx="12" cy="12" r="6.5"/><path d="M12 2.5v5M12 16.5v5M2.5 12h5M16.5 12h5"/>"#,
        // The window, status bar and plain marks: line art only, so the tint alone shows on and off.
        "MENU" => r#"<path d="M4 7h16M4 12h16M4 17h16"/>"#,
        "UNDO" | "U" => r#"<path d="M9 6 4.5 10.5 9 15M4.5 10.5H15a4.5 4.5 0 0 1 0 9H10"/>"#,
        "REDO" => r#"<path d="m15 6 4.5 4.5L15 15M19.5 10.5H9a4.5 4.5 0 0 0 0 9h5"/>"#,
        "WIN_MINIMIZE" => r#"<path d="M6 12h12"/>"#,
        "WIN_MAXIMIZE" => r#"<rect x="6" y="6" width="12" height="12" rx="1"/>"#,
        "WIN_RESTORE" => {
            r#"<rect x="5.5" y="9.5" width="9" height="9" rx="1"/><path d="M9.5 9.5v-3a1 1 0 0 1 1-1h8a1 1 0 0 1 1 1v8a1 1 0 0 1-1 1h-3"/>"#
        }
        "WIN_CLOSE" => r#"<path d="m6 6 12 12M18 6 6 18"/>"#,
        "PLUS" => r#"<path d="M12 5v14M5 12h14"/>"#,
        "GRID" => r#"<rect x="4" y="4" width="16" height="16" rx="1.5"/><path d="M4 9.3h16M4 14.7h16M9.3 4v16M14.7 4v16"/>"#,
        "SNAP" => r#"<path d="m12 4 8 8-8 8-8-8Z"/><circle cx="12" cy="12" r="1.3" fill="white"/>"#,
        "ORTHO" => r#"<path d="M5 4v15h15"/><path d="M5 19 16 8" stroke-dasharray="2 2"/>"#,
        "POLAR" => r#"<circle cx="12" cy="12" r="8"/><path d="m8.5 15.5 7-7"/>"#,
        "OTRACK" => {
            r#"<circle cx="8" cy="8" r="2.2"/><path d="M10.5 8H21" stroke-dasharray="2 2"/><path d="M8 10.5V21" stroke-dasharray="2 2"/>"#
        }
        "DYN" => r#"<path d="M4 4l6 14 2-6 6-2Z"/><rect x="13" y="15" width="8" height="5" rx="1"/>"#,
        _ => return None,
    })
}

/// Shown for a name with no icon of its own: a plain dot.
const FALLBACK: &str = r#"<circle cx="12" cy="12" r="2"/>"#;

/// Darkens every `#rrggbb` color in the snippet by the factor, from 0 (black) to 1 (unchanged).
fn darken(snippet: &str, factor: f32) -> String {
    let mut out = String::with_capacity(snippet.len());
    let mut rest = snippet;
    while let Some(at) = rest.find('#') {
        out.push_str(&rest[..at]);
        let hex = rest.get(at + 1..at + 7).filter(|h| h.bytes().all(|b| b.is_ascii_hexdigit()));
        match hex.and_then(|h| u32::from_str_radix(h, 16).ok()) {
            Some(rgb) => {
                let channel = |shift: u32| (((rgb >> shift) & 0xff) as f32 * factor).round() as u8;
                out.push_str(&format!("#{:02x}{:02x}{:02x}", channel(16), channel(8), channel(0)));
                rest = &rest[at + 7..];
            }
            None => {
                out.push('#');
                rest = &rest[at + 1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// The whole SVG of the named icon as shown in the given tint: line art in the tint, art darkened as the tint is
/// darker than the usual text, and all of it as transparent as the tint.
fn svg(name: &str, tint: Color32) -> String {
    let [r, g, b, a] = tint.to_srgba_unmultiplied();
    let factor = (f32::from(r.max(g).max(b)) / 200.0).min(1.0);
    let line = format!("#{r:02x}{g:02x}{b:02x}");
    let body = darken(shape(name.trim_start_matches('-')).unwrap_or(FALLBACK), factor).replace("\"white\"", &format!("\"{line}\""));
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{RENDER_SIZE}" height="{RENDER_SIZE}" viewBox="0 0 24 24" fill="none" stroke="{line}" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><g opacity="{:.3}">{body}</g></svg>"##,
        f32::from(a) / 255.0
    )
}

/// The named icon in the given tint, ready to size and show.
pub fn image(name: &str, tint: Color32) -> egui::Image<'static> {
    let [r, g, b, a] = tint.to_srgba_unmultiplied();
    let uri = format!("bytes://icon/{name}-{r:02x}{g:02x}{b:02x}{a:02x}.svg");
    egui::Image::new(ImageSource::Bytes {
        uri: uri.into(),
        bytes: Bytes::Shared(Arc::from(svg(name, tint).into_bytes().into_boxed_slice())),
    })
}

/// Paint an icon into a square of the given size centered in the rectangle.
pub fn paint(ui: &Ui, name: &str, rect: Rect, size: f32, tint: Color32) {
    let square = Rect::from_center_size(rect.center(), egui::vec2(size, size));
    image(name, tint).paint_at(ui, square);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::registry::{COMMANDS, WINDOW_COMMANDS};

    /// Icons the ribbon and the layer and property controls ask for besides the commands.
    const OTHERS: &[&str] = &[
        "LAYER",
        "LAYMCH",
        "COPYCLIP",
        "CUTCLIP",
        "PASTECLIP",
        "SELECTALL",
        "QSELECT",
        "QUICKCALC",
        "CAL",
        "MEASUREGEOM",
        "ATTEDIT",
        "EATTEDIT",
        "ZOOM_EXTENTS",
        "ZOOM_IN",
        "ZOOM_OUT",
        "LAYER_ON",
        "LAYER_OFF",
        "LAYER_THAWED",
        "LAYER_FROZEN",
        "LAYER_UNLOCKED",
        "LAYER_LOCKED",
        "COLOR_WHEEL",
        "LINEWEIGHT",
        "LINETYPE",
        "LAYERS",
        "MENU",
        "SAVE",
        "UNDO",
        "REDO",
        "WIN_MINIMIZE",
        "WIN_MAXIMIZE",
        "WIN_RESTORE",
        "WIN_CLOSE",
        "PLUS",
        "GRID",
        "SNAP",
        "ORTHO",
        "POLAR",
        "OTRACK",
        "DYN",
        "LWT",
    ];

    fn names() -> impl Iterator<Item = &'static str> {
        COMMANDS.iter().chain(WINDOW_COMMANDS).map(|(name, _)| *name).chain(OTHERS.iter().copied())
    }

    #[test]
    fn every_command_has_an_icon_that_draws() {
        let missing: Vec<_> = names().filter(|name| shape(name.trim_start_matches('-')).is_none()).collect();
        assert!(missing.is_empty(), "no icon for {missing:?}");
        for name in names() {
            for tint in [Color32::WHITE, Color32::from_rgb(0x45, 0xc4, 0xcf), Color32::from_rgb(0x97, 0xa3, 0xa7).gamma_multiply(0.5)] {
                let image = egui_extras::image::load_svg_bytes(svg(name, tint).as_bytes()).unwrap_or_else(|e| panic!("{name}: {e}"));
                assert_eq!(image.size, [RENDER_SIZE as usize; 2], "{name}");
                assert!(image.pixels.iter().filter(|p| p.a() > 0).count() > 40, "{name} draws nothing");
            }
        }
    }

    #[test]
    fn line_art_takes_the_tint_and_art_keeps_its_color() {
        let checked = svg("LAYER_ON", Color32::from_rgb(0x45, 0xc4, 0xcf));
        assert!(checked.contains("#f7d04a"), "the lamp stays yellow");
        assert!(checked.contains("stroke=\"#45c4cf\""));
        assert!(svg("SNAP", Color32::from_rgb(1, 2, 3)).contains("fill=\"#010203\""));
        assert!(!svg("LAYER_ON", Color32::from_rgb(0x50, 0x50, 0x50)).contains("#f7d04a"), "dim tints darken the art");
        assert_eq!(darken("url(#shade) #ff8000", 0.5), "url(#shade) #804000");
    }
}
