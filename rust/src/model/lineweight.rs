//! Lineweights as AutoCAD keeps them: hundredths of a millimeter from the standard list, or ByLayer, ByBlock or
//! Default (the drawing's LWDEFAULT). What an object prints and shows with comes from itself, its block insert,
//! its layer and the default, in that order.

pub const BYLAYER: i16 = -1;
pub const BYBLOCK: i16 = -2;
pub const DEFAULT: i16 = -3;
/// LWDEFAULT in a new drawing: 0.25 mm.
pub const DEFAULT_MM100: i16 = 25;

/// The lineweights AutoCAD offers, in hundredths of a millimeter. Objects can have no others.
pub const STANDARD: [i16; 24] = [0, 5, 9, 13, 15, 18, 20, 25, 30, 35, 40, 50, 53, 60, 70, 80, 90, 100, 106, 120, 140, 158, 200, 211];

/// Every choice a lineweight list offers, special ones first.
pub fn choices() -> impl Iterator<Item = i16> {
    [BYLAYER, BYBLOCK, DEFAULT].into_iter().chain(STANDARD)
}

/// How a lineweight reads in lists and the Properties palette.
pub fn name(lineweight: i16) -> String {
    match lineweight {
        BYLAYER => "ByLayer".into(),
        BYBLOCK => "ByBlock".into(),
        DEFAULT => "Default".into(),
        mm100 => format!("{:.2} mm", mm100 as f64 / 100.0),
    }
}

/// The standard lineweight nearest to some hundredths of a millimeter, as AutoCAD rounds one it is given.
pub fn nearest_standard(mm100: f64) -> i16 {
    STANDARD.iter().copied().min_by(|a, b| (*a as f64 - mm100).abs().total_cmp(&(*b as f64 - mm100).abs())).unwrap_or(0)
}

/// A lineweight from typed text: ByLayer, ByBlock, Default, or millimeters ("0.3", "0.30 mm"), rounded to the
/// nearest standard one.
pub fn parse(text: &str) -> Option<i16> {
    let lower = text.trim().to_lowercase();
    match lower.as_str() {
        "bylayer" => Some(BYLAYER),
        "byblock" => Some(BYBLOCK),
        "default" => Some(DEFAULT),
        _ => {
            let mm: f64 = lower.trim_end_matches("mm").trim().parse().ok()?;
            (mm.is_finite() && (0.0..=2.2).contains(&mm)).then(|| nearest_standard(mm * 100.0))
        }
    }
}

/// A lineweight as a DXF file gives it (code 370), kept to the values objects can have.
pub fn from_file(value: i64) -> i16 {
    match value {
        -1 => BYLAYER,
        -2 => BYBLOCK,
        -3 => DEFAULT,
        v if v >= 0 => nearest_standard(v as f64),
        _ => BYLAYER,
    }
}

/// The lineweight a part has in millimeters: its own, else its layer's, else the drawing's default. ByBlock left
/// over outside any block counts as default, as do a layer's ByLayer and ByBlock.
pub fn resolve_mm(own: i16, layer: Option<i16>, default_mm100: i16) -> f64 {
    let concrete = |lw: i16| (lw >= 0).then_some(lw);
    let mm100 = match own {
        BYLAYER => layer.and_then(concrete),
        lw => concrete(lw),
    }
    .unwrap_or(default_mm100.max(0));
    mm100 as f64 / 100.0
}

/// How many screen pixels a lineweight shows with in model space, where lineweights do not grow as you zoom:
/// a millimeter at 96 pixels an inch, and never under one pixel.
pub fn model_pixels(mm: f64) -> f32 {
    ((mm * 96.0 / 25.4) as f32).max(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_lineweights_round_to_standard_ones() {
        assert_eq!(parse("0.3"), Some(30));
        assert_eq!(parse("0.31 mm"), Some(30));
        assert_eq!(parse("0.52"), Some(53));
        assert_eq!(parse("ByLayer"), Some(BYLAYER));
        assert_eq!(parse("default"), Some(DEFAULT));
        assert_eq!(parse("5"), None, "past the largest lineweight");
        assert_eq!(parse("thick"), None);
        assert_eq!(name(30), "0.30 mm");
        assert_eq!(name(BYBLOCK), "ByBlock");
        assert_eq!(from_file(211), 211);
        assert_eq!(from_file(31), 30);
    }

    #[test]
    fn lineweights_come_from_the_object_then_the_layer_then_the_default() {
        assert_eq!(resolve_mm(50, Some(30), 25), 0.5);
        assert_eq!(resolve_mm(BYLAYER, Some(30), 25), 0.3);
        assert_eq!(resolve_mm(BYLAYER, Some(DEFAULT), 25), 0.25);
        assert_eq!(resolve_mm(DEFAULT, Some(100), 18), 0.18);
        assert_eq!(resolve_mm(BYBLOCK, Some(100), 25), 0.25, "ByBlock outside a block");
        assert_eq!(resolve_mm(0, None, 25), 0.0);
        assert_eq!(model_pixels(0.0), 1.0);
        assert!((model_pixels(1.0) - 3.78).abs() < 0.01);
    }
}
