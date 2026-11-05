//! The AutoCAD color index palette, as ezdxf and AutoCAD define it.

/// RGB of each ACI color, 0xRRGGBB. Index 0 (by block) and 256 (by layer) have no color of their own.
pub const ACI_RGB: [u32; 256] = [
    0x000000, 0xff0000, 0xffff00, 0x00ff00, 0x00ffff, 0x0000ff, 0xff00ff, 0xffffff, 0x808080, 0xc0c0c0, 0xff0000, 0xff7f7f, 0xa50000,
    0xa55252, 0x7f0000, 0x7f3f3f, 0x4c0000, 0x4c2626, 0x260000, 0x261313, 0xff3f00, 0xff9f7f, 0xa52900, 0xa56752, 0x7f1f00, 0x7f4f3f,
    0x4c1300, 0x4c2f26, 0x260900, 0x261713, 0xff7f00, 0xffbf7f, 0xa55200, 0xa57c52, 0x7f3f00, 0x7f5f3f, 0x4c2600, 0x4c3926, 0x261300,
    0x261c13, 0xffbf00, 0xffdf7f, 0xa57c00, 0xa59152, 0x7f5f00, 0x7f6f3f, 0x4c3900, 0x4c4226, 0x261c00, 0x262113, 0xffff00, 0xffff7f,
    0xa5a500, 0xa5a552, 0x7f7f00, 0x7f7f3f, 0x4c4c00, 0x4c4c26, 0x262600, 0x262613, 0xbfff00, 0xdfff7f, 0x7ca500, 0x91a552, 0x5f7f00,
    0x6f7f3f, 0x394c00, 0x424c26, 0x1c2600, 0x212613, 0x7fff00, 0xbfff7f, 0x52a500, 0x7ca552, 0x3f7f00, 0x5f7f3f, 0x264c00, 0x394c26,
    0x132600, 0x1c2613, 0x3fff00, 0x9fff7f, 0x29a500, 0x67a552, 0x1f7f00, 0x4f7f3f, 0x134c00, 0x2f4c26, 0x092600, 0x172613, 0x00ff00,
    0x7fff7f, 0x00a500, 0x52a552, 0x007f00, 0x3f7f3f, 0x004c00, 0x264c26, 0x002600, 0x132613, 0x00ff3f, 0x7fff9f, 0x00a529, 0x52a567,
    0x007f1f, 0x3f7f4f, 0x004c13, 0x264c2f, 0x002609, 0x135817, 0x00ff7f, 0x7fffbf, 0x00a552, 0x52a57c, 0x007f3f, 0x3f7f5f, 0x004c26,
    0x264c39, 0x002613, 0x13581c, 0x00ffbf, 0x7fffdf, 0x00a57c, 0x52a591, 0x007f5f, 0x3f7f6f, 0x004c39, 0x264c42, 0x00261c, 0x135858,
    0x00ffff, 0x7fffff, 0x00a5a5, 0x52a5a5, 0x007f7f, 0x3f7f7f, 0x004c4c, 0x264c4c, 0x002626, 0x135858, 0x00bfff, 0x7fdfff, 0x007ca5,
    0x5291a5, 0x005f7f, 0x3f6f7f, 0x00394c, 0x26427e, 0x001c26, 0x135858, 0x007fff, 0x7fbfff, 0x0052a5, 0x527ca5, 0x003f7f, 0x3f5f7f,
    0x00264c, 0x26397e, 0x001326, 0x131c58, 0x003fff, 0x7f9fff, 0x0029a5, 0x5267a5, 0x001f7f, 0x3f4f7f, 0x00134c, 0x262f7e, 0x000926,
    0x131758, 0x0000ff, 0x7f7fff, 0x0000a5, 0x5252a5, 0x00007f, 0x3f3f7f, 0x00004c, 0x26267e, 0x000026, 0x131358, 0x3f00ff, 0x9f7fff,
    0x2900a5, 0x6752a5, 0x1f007f, 0x4f3f7f, 0x13004c, 0x2f267e, 0x090026, 0x171358, 0x7f00ff, 0xbf7fff, 0x5200a5, 0x7c52a5, 0x3f007f,
    0x5f3f7f, 0x26004c, 0x39267e, 0x130026, 0x1c1358, 0xbf00ff, 0xdf7fff, 0x7c00a5, 0x9152a5, 0x5f007f, 0x6f3f7f, 0x39004c, 0x42264c,
    0x1c0026, 0x581358, 0xff00ff, 0xff7fff, 0xa500a5, 0xa552a5, 0x7f007f, 0x7f3f7f, 0x4c004c, 0x4c264c, 0x260026, 0x581358, 0xff00bf,
    0xff7fdf, 0xa5007c, 0xa55291, 0x7f005f, 0x7f3f6f, 0x4c0039, 0x4c2642, 0x26001c, 0x581358, 0xff007f, 0xff7fbf, 0xa50052, 0xa5527c,
    0x7f003f, 0x7f3f5f, 0x4c0026, 0x4c2639, 0x260013, 0x58131c, 0xff003f, 0xff7f9f, 0xa50029, 0xa55267, 0x7f001f, 0x7f3f4f, 0x4c0013,
    0x4c262f, 0x260009, 0x581317, 0x000000, 0x656565, 0x666666, 0x999999, 0xcccccc, 0xffffff,
];

pub fn aci_rgb(aci: i32) -> (u8, u8, u8) {
    let value = ACI_RGB[aci.clamp(0, 255) as usize];
    ((value >> 16) as u8, (value >> 8) as u8, value as u8)
}

/// A true color keeps its red, green and blue in the low 24 bits, above which this flag sets it apart from
/// AutoCAD's index colors (1 to 255), by block (0) and by layer (256).
pub const TRUE_COLOR: i32 = 0x0100_0000;
pub const BY_BLOCK: i32 = 0;
pub const BY_LAYER: i32 = 256;

/// The names AutoCAD gives the first seven index colors, and the two grays after them.
pub const NAMED_COLORS: [(i32, &str); 9] =
    [(1, "Red"), (2, "Yellow"), (3, "Green"), (4, "Cyan"), (5, "Blue"), (6, "Magenta"), (7, "White"), (8, "Gray"), (9, "Light Gray")];

pub fn true_color(r: u8, g: u8, b: u8) -> i32 {
    TRUE_COLOR | (r as i32) << 16 | (g as i32) << 8 | b as i32
}

/// Negative numbers (how files mark a layer that is off) are never true colors.
pub fn is_true_color(color: i32) -> bool {
    color > 0 && color & TRUE_COLOR != 0
}

/// A color's own red, green and blue: a true color's or an index color's. None for by layer or by block.
pub fn color_rgb(color: i32) -> Option<(u8, u8, u8)> {
    if is_true_color(color) {
        Some(((color >> 16) as u8, (color >> 8) as u8, color as u8))
    } else {
        (1..=255).contains(&color).then(|| aci_rgb(color))
    }
}

/// The index color nearest a true color, which files keep beside it for programs without true colors.
pub fn nearest_aci((r, g, b): (u8, u8, u8)) -> i32 {
    let distance = |aci: i32| {
        let (ar, ag, ab) = aci_rgb(aci);
        let d = |x: u8, y: u8| (x as i32 - y as i32).pow(2);
        d(r, ar) + d(g, ag) + d(b, ab)
    };
    (1..=255).min_by_key(|&aci| distance(aci)).unwrap_or(7)
}

/// The index color to write for a color: its own, the nearest to a true color, or 256/0 for by layer/by block.
pub fn index_of(color: i32) -> i32 {
    match color_rgb(color) {
        Some(rgb) if is_true_color(color) => nearest_aci(rgb),
        _ => color,
    }
}

/// How AutoCAD names a color: ByLayer, ByBlock, Red, Color 30, or 255,128,0 for a true color.
pub fn color_name(color: i32) -> String {
    match color {
        BY_LAYER => "ByLayer".into(),
        BY_BLOCK => "ByBlock".into(),
        _ if is_true_color(color) => {
            let (r, g, b) = color_rgb(color).unwrap_or_default();
            format!("{r},{g},{b}")
        }
        _ => NAMED_COLORS.iter().find(|(c, _)| *c == color).map_or(format!("Color {color}"), |(_, n)| n.to_string()),
    }
}

/// A color as typed: ByLayer, ByBlock, a name, an index (1-255), red,green,blue or #rrggbb.
pub fn parse_color(text: &str) -> Option<i32> {
    let text = text.trim();
    let lower = text.to_lowercase();
    match lower.as_str() {
        "bylayer" => return Some(BY_LAYER),
        "byblock" => return Some(BY_BLOCK),
        _ => {}
    }
    if let Some((aci, _)) = NAMED_COLORS.iter().find(|(_, n)| n.eq_ignore_ascii_case(text)) {
        return Some(*aci);
    }
    if let Some(hex) = lower.strip_prefix('#').filter(|h| h.len() == 6) {
        let value = u32::from_str_radix(hex, 16).ok()?;
        return Some(true_color((value >> 16) as u8, (value >> 8) as u8, value as u8));
    }
    let parts: Vec<&str> = text.split(',').map(str::trim).collect();
    match parts[..] {
        [one] => one.parse::<i32>().ok().filter(|aci| (1..=255).contains(aci)),
        [r, g, b] => Some(true_color(r.parse().ok()?, g.parse().ok()?, b.parse().ok()?)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn true_colors_keep_their_values_apart_from_index_colors() {
        let orange = true_color(255, 128, 0);
        assert!(is_true_color(orange) && !is_true_color(30) && !is_true_color(BY_LAYER) && !is_true_color(-3));
        assert_eq!(color_rgb(orange), Some((255, 128, 0)));
        assert_eq!(color_rgb(1), Some((255, 0, 0)));
        assert_eq!(color_rgb(BY_LAYER), None);
        assert_eq!(index_of(true_color(250, 0, 0)), 1);
        assert_eq!(index_of(5), 5);
    }

    #[test]
    fn colors_read_and_name_as_autocad_does() {
        assert_eq!(parse_color("red"), Some(1));
        assert_eq!(parse_color("ByLayer"), Some(BY_LAYER));
        assert_eq!(parse_color("30"), Some(30));
        assert_eq!(parse_color("256"), None);
        assert_eq!(parse_color("255, 128, 0"), Some(true_color(255, 128, 0)));
        assert_eq!(parse_color("#FF8000"), Some(true_color(255, 128, 0)));
        assert_eq!(color_name(true_color(255, 128, 0)), "255,128,0");
        assert_eq!(color_name(1), "Red");
        assert_eq!(color_name(30), "Color 30");
    }
}
