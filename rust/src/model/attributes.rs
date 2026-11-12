//! Attributes of block inserts kept in line with their block's definitions: what ATTSYNC, BATTMAN and the
//! Enhanced Attribute Editor change.

use super::entities::{Attribute, BlockRef, Entity, Shape};
use super::transform::{insert_transform, transform_shape};

/// The text of an attribute with another value, keeping its place and look.
pub fn with_value(text: &Shape, value: &str) -> Shape {
    match text {
        Shape::Text { .. } => {
            let mut shape = text.clone();
            if let Shape::Text { text, formatted, .. } = &mut shape {
                *text = value.to_string();
                formatted.clear();
            }
            shape
        }
        other => other.clone(),
    }
}

/// An attribute's value: the text it shows.
pub fn value_of(attribute: &Attribute) -> &str {
    match &attribute.text {
        Shape::Text { text, .. } => text,
        _ => "",
    }
}

/// An insert as its block now defines it: each attribute the block defines, in the block's order and placed as the
/// definition is, keeping the value the insert had under the same tag, or the definition's default for a new one.
/// Attributes the block no longer defines go. Anything else than an insert is given back as it is.
pub fn synced(entity: &Entity, block: &BlockRef) -> Entity {
    let Shape::Insert { position, scale_x, scale_y, rotation, attributes, .. } = &entity.shape else { return entity.clone() };
    let placed = insert_transform(block.0.base, *position, *scale_x, *scale_y, *rotation);
    let attributes = block
        .0
        .attribute_defs
        .iter()
        .map(|definition| {
            let kept = attributes.iter().find(|a| a.tag.eq_ignore_ascii_case(&definition.tag));
            let value = kept.map_or_else(|| value_of(definition).to_string(), |a| value_of(a).to_string());
            Attribute { text: with_value(&transform_shape(&definition.text, &placed), &value), ..definition.clone() }
        })
        .collect();
    let mut shape = entity.shape.clone();
    if let Shape::Insert { block: b, attributes: a, .. } = &mut shape {
        *b = block.clone();
        *a = attributes;
    }
    entity.with_shape(shape)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::entities::{BlockDef, text};
    use crate::model::geometry::pt;
    use std::sync::Arc;

    fn definition(tag: &str, default: &str, at: f64) -> Attribute {
        Attribute {
            tag: tag.into(),
            prompt: String::new(),
            layer: "0".into(),
            color: 256,
            text: text(pt(at, 0.0), default, 1.0, 0.0).shape,
            invisible: false,
        }
    }

    #[test]
    fn syncing_keeps_values_by_tag_adds_new_ones_and_drops_old_ones() {
        let old = BlockRef(Arc::new(BlockDef {
            name: "DOOR".into(),
            base: pt(0.0, 0.0),
            entities: vec![],
            attribute_defs: vec![definition("NUMBER", "?", 0.0), definition("GONE", "x", 5.0)],
            xref: None,
        }));
        let mut attributes: Vec<Attribute> = old.0.attribute_defs.clone();
        attributes[0].text = with_value(&attributes[0].text, "D12");
        let insert =
            Entity::new(Shape::Insert { block: old, position: pt(10.0, 0.0), scale_x: 2.0, scale_y: 2.0, rotation: 0.0, attributes });
        let new = BlockRef(Arc::new(BlockDef {
            name: "DOOR".into(),
            base: pt(0.0, 0.0),
            entities: vec![],
            attribute_defs: vec![definition("FIRE", "30min", 3.0), definition("number", "?", 1.0)],
            xref: None,
        }));
        let Shape::Insert { attributes, .. } = synced(&insert, &new).shape else { panic!("an insert") };
        let found: Vec<(&str, &str)> = attributes.iter().map(|a| (a.tag.as_str(), value_of(a))).collect();
        assert_eq!(found, [("FIRE", "30min"), ("number", "D12")]);
        let Shape::Text { position, height, .. } = &attributes[1].text else { panic!("text") };
        assert_eq!((*position, *height), (pt(12.0, 0.0), 2.0), "placed as the definition is, through the insert");
    }
}
