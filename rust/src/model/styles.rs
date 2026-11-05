pub const BYLAYER_LINETYPE: &str = "ByLayer";
pub const BYLAYER_COLOR: i32 = 256;
pub const LINETYPES: [&str; 6] = ["Continuous", "Dashed", "Center", "Hidden", "DashDot", "Phantom"];

/// Dash pattern of a linetype in drawing units, as AutoCAD's acadiso.lin gives it in millimeters: dash, gap, dash,
/// gap... A zero is a dot. Empty is a solid line.
pub fn dash_pattern(linetype: &str) -> &'static [f64] {
    match linetype {
        "Dashed" => &[12.7, 6.35],
        "Center" => &[31.75, 6.35, 6.35, 6.35],
        "Hidden" => &[6.35, 3.175],
        "DashDot" => &[12.7, 6.35, 0.0, 6.35],
        "Phantom" => &[31.75, 6.35, 6.35, 6.35, 6.35, 6.35],
        _ => &[],
    }
}

/// The dash pattern of a linetype, each length multiplied by `scale`: how many units a unit of the pattern is
/// drawn at. Empty, a solid line, when the shortest dash or gap would be shorter than `min_length`, as AutoCAD
/// draws a linetype too fine to see as solid.
pub fn scaled_pattern(linetype: &str, scale: f64, min_length: f64) -> Vec<f64> {
    let pattern = dash_pattern(linetype);
    let shortest = pattern.iter().filter(|d| **d > 0.0).fold(f64::INFINITY, |least, d| least.min(*d));
    if !shortest.is_finite() || shortest * scale < min_length {
        return vec![];
    }
    pattern.iter().map(|d| d * scale).collect()
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
    fn a_pattern_scales_and_goes_solid_when_too_fine() {
        assert_eq!(scaled_pattern("Dashed", 2.0, 1.0), vec![25.4, 12.7]);
        assert_eq!(scaled_pattern("DashDot", 1.0, 1.0), vec![12.7, 6.35, 0.0, 6.35], "a dot keeps its zero length");
        assert!(scaled_pattern("Dashed", 0.1, 1.0).is_empty(), "6.35 at 0.1 is under a pixel");
        assert!(scaled_pattern("Continuous", 10.0, 1.0).is_empty());
    }

    #[test]
    fn linetype_names_from_files_take_linewright_spelling() {
        assert_eq!(canonical_linetype("DASHED"), "Dashed");
        assert_eq!(canonical_linetype("bylayer"), "ByLayer");
        assert_eq!(canonical_linetype("BORDER"), "BORDER");
    }
}
