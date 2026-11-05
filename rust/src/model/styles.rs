pub const BYLAYER_LINETYPE: &str = "ByLayer";
pub const BYLAYER_COLOR: i32 = 256;
pub const LINETYPES: [&str; 6] = ["Continuous", "Dashed", "Center", "Hidden", "DashDot", "Phantom"];

/// Dash pattern for drawing a linetype on screen, in pixels: dash, gap, dash, gap...
pub fn dash_pattern(linetype: &str) -> &'static [f32] {
    match linetype {
        "Dashed" => &[8.0, 5.0],
        "Center" => &[16.0, 4.0, 4.0, 4.0],
        "Hidden" => &[4.0, 4.0],
        "DashDot" => &[10.0, 4.0, 2.0, 4.0],
        "Phantom" => &[16.0, 4.0, 4.0, 4.0, 4.0, 4.0],
        _ => &[],
    }
}

/// AutoCAD color index, name. These are the standard colors.
pub const COLOR_CHOICES: [(i32, &str); 9] =
    [(1, "Red"), (2, "Yellow"), (3, "Green"), (4, "Cyan"), (5, "Blue"), (6, "Magenta"), (7, "White"), (8, "Gray"), (9, "Light Gray")];

/// The spelling Linewright uses for a linetype name from a file, which may be in any case. Unknown names stay as-is.
pub fn canonical_linetype(name: &str) -> String {
    std::iter::once(BYLAYER_LINETYPE).chain(LINETYPES).find(|known| known.eq_ignore_ascii_case(name)).unwrap_or(name).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linetype_names_from_files_take_linewright_spelling() {
        assert_eq!(canonical_linetype("DASHED"), "Dashed");
        assert_eq!(canonical_linetype("bylayer"), "ByLayer");
        assert_eq!(canonical_linetype("BORDER"), "BORDER");
    }
}
