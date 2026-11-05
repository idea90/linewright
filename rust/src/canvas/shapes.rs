//! The colors parts are drawn in. What entities are made of lives in `model::parts`.

use super::aci::{aci_rgb, color_rgb};
use crate::model::document::Document;
pub use crate::model::parts::{Parts, Styled, TextSpec, flatten};

const FALLBACK_ACI: i32 = 7;

/// The color something shows in: its own, or its layer's when it is by layer (or by block outside any block).
pub fn resolve_color(style: &Styled, document: &Document) -> (u8, u8, u8) {
    color_rgb(style.color)
        .or_else(|| document.layer(&style.layer).and_then(|l| color_rgb(l.color)))
        .unwrap_or_else(|| aci_rgb(FALLBACK_ACI))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::entities::circle;
    use crate::model::geometry::pt;

    #[test]
    fn layer_color_is_used_by_layer() {
        let mut document = Document::new();
        document.layers[0].color = 1;
        assert_eq!(resolve_color(&Styled::of(&circle(pt(0.0, 0.0), 1.0)), &document), (255, 0, 0));
    }
}
