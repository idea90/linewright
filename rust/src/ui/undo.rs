//! Undo and redo. Every change to the drawing is a `Change` pushed here, which also makes it.

use crate::model::dimension::DimensionStyle;
use crate::model::document::ObjectGroup;
use crate::model::document::{Document, Layer};
use crate::model::entities::{Entity, EntityId};

/// Everything a layer change can touch: the layers, which one is current, and objects renamed with a layer.
#[derive(Clone, Debug, PartialEq)]
pub struct LayerState {
    pub layers: Vec<Layer>,
    pub current: String,
    pub entity_layers: Vec<(EntityId, String)>,
}

#[derive(Clone, Debug)]
pub enum Change {
    Add(Vec<Entity>),
    Remove {
        entities: Vec<Entity>,
        positions: Vec<usize>,
    },
    /// Changes entities in place, so everything that refers to them (selection, drawing order) stays valid.
    Modify {
        before: Vec<Entity>,
        after: Vec<Entity>,
    },
    Layers {
        before: LayerState,
        after: LayerState,
    },
    /// The dimension style table and which style is current.
    DimensionStyles {
        before: (Vec<DimensionStyle>, String),
        after: (Vec<DimensionStyle>, String),
    },
    /// The open space's drawing order, as entity ids front-most last.
    Reorder {
        before: Vec<EntityId>,
        after: Vec<EntityId>,
    },
    /// The drawing's groups.
    Groups {
        before: Vec<ObjectGroup>,
        after: Vec<ObjectGroup>,
    },
    Group(Vec<Change>),
}

impl Change {
    pub fn remove(entities: Vec<Entity>) -> Change {
        Change::Remove { entities, positions: vec![] }
    }

    /// The change to these dimension styles, with this one current.
    pub fn dimension_styles(document: &Document, styles: Vec<DimensionStyle>, current: String) -> Change {
        Change::DimensionStyles {
            before: (document.dimension_styles.clone(), document.current_dimension_style.clone()),
            after: (styles, current),
        }
    }

    /// The change for these new states, remembering what the entities held before. They may be in a space that is
    /// not open, as a layout's viewports are while model space is worked on through one.
    pub fn modify(document: &Document, after: Vec<Entity>) -> Change {
        let before = after.iter().filter_map(|state| document.get(state.id).cloned()).collect();
        let after = after.into_iter().filter(|state| document.get(state.id).is_some()).collect();
        Change::Modify { before, after }
    }

    /// The objects this change touches: their ids, and the places they were and are.
    pub fn touched(&self, document: &Document, ids: &mut Vec<EntityId>, areas: &mut Vec<crate::model::index::Bounds>) {
        use crate::model::index::entity_extent;
        match self {
            Change::Add(entities) | Change::Remove { entities, .. } => {
                ids.extend(entities.iter().map(|e| e.id));
                areas.extend(entities.iter().map(entity_extent));
            }
            Change::Modify { after, .. } => {
                for state in after {
                    ids.push(state.id);
                    areas.push(entity_extent(state));
                    areas.extend(document.get(state.id).map(entity_extent));
                }
            }
            Change::Group(changes) => changes.iter().for_each(|c| c.touched(document, ids, areas)),
            _ => {}
        }
    }

    fn redo(&mut self, document: &mut Document) {
        match self {
            Change::Add(entities) => {
                for entity in entities.iter() {
                    document.add(entity.clone());
                }
            }
            Change::Remove { entities, positions } => {
                // Only what is still in the drawing is removed, and its current values are what undo puts back.
                let ids: Vec<EntityId> = entities.iter().map(|e| e.id).collect();
                let present: Vec<(usize, Entity)> = document
                    .positions_of(&ids)
                    .into_iter()
                    .zip(&ids)
                    .filter_map(|(position, id)| Some((position?, document.get(*id)?.clone())))
                    .collect();
                (*positions, *entities) = present.into_iter().unzip();
                document.remove_all(&ids);
            }
            Change::Modify { after, .. } => {
                for state in after.iter() {
                    document.modify(state);
                }
            }
            Change::Layers { after, .. } => apply_layers(document, after),
            Change::Groups { after, .. } => document.groups = after.clone(),
            Change::Reorder { after, .. } => document.set_order(after),
            Change::DimensionStyles { after, .. } => (document.dimension_styles, document.current_dimension_style) = after.clone(),
            Change::Group(changes) => {
                for change in changes.iter_mut() {
                    change.redo(document);
                }
            }
        }
    }

    fn undo(&mut self, document: &mut Document) {
        match self {
            Change::Add(entities) => {
                let ids: Vec<EntityId> = entities.iter().map(|e| e.id).collect();
                document.remove_all(&ids);
            }
            Change::Remove { entities, positions } => {
                // Put them back where they were, lowest position first so the later ones land correctly.
                let mut back: Vec<(usize, &Entity)> = positions.iter().copied().zip(entities.iter()).collect();
                back.sort_by_key(|(position, _)| *position);
                for (position, entity) in back {
                    document.insert(position, entity.clone());
                }
            }
            Change::Modify { before, .. } => {
                for state in before.iter() {
                    document.modify(state);
                }
            }
            Change::Layers { before, .. } => apply_layers(document, before),
            Change::Groups { before, .. } => document.groups = before.clone(),
            Change::Reorder { before, .. } => document.set_order(before),
            Change::DimensionStyles { before, .. } => (document.dimension_styles, document.current_dimension_style) = before.clone(),
            Change::Group(changes) => {
                for change in changes.iter_mut().rev() {
                    change.undo(document);
                }
            }
        }
    }
}

fn apply_layers(document: &mut Document, state: &LayerState) {
    document.layers = state.layers.clone();
    document.current_layer = state.current.clone();
    for (id, name) in &state.entity_layers {
        if let Some(entity) = document.get(*id) {
            let mut renamed = entity.clone();
            renamed.layer = name.clone();
            document.modify(&renamed);
        }
    }
}

#[derive(Default)]
pub struct UndoStack {
    done: Vec<Change>,
    undone: Vec<Change>,
    clean: Option<usize>, // how many changes were done when the drawing was last saved
    groups: Vec<Vec<Change>>,
}

impl UndoStack {
    /// Make the change and remember it.
    pub fn push(&mut self, mut change: Change, document: &mut Document) {
        change.redo(document);
        if let Some(group) = self.groups.last_mut() {
            group.push(change);
            return;
        }
        if self.clean.is_some_and(|clean| clean > self.done.len()) {
            self.clean = None; // the saved state was undone and can no longer be reached
        }
        self.done.push(change);
        self.undone.clear();
    }

    /// Changes pushed until the matching `end_group` undo together as one.
    pub fn begin_group(&mut self) {
        self.groups.push(vec![]);
    }

    pub fn end_group(&mut self) {
        let Some(changes) = self.groups.pop() else { return };
        if changes.is_empty() {
            return;
        }
        let group = Change::Group(changes);
        match self.groups.last_mut() {
            Some(outer) => outer.push(group),
            None => {
                if self.clean.is_some_and(|clean| clean > self.done.len()) {
                    self.clean = None;
                }
                self.done.push(group);
                self.undone.clear();
            }
        }
    }

    pub fn undo(&mut self, document: &mut Document) -> bool {
        let Some(mut change) = self.done.pop() else { return false };
        change.undo(document);
        self.undone.push(change);
        true
    }

    pub fn redo(&mut self, document: &mut Document) -> bool {
        let Some(mut change) = self.undone.pop() else { return false };
        change.redo(document);
        self.done.push(change);
        true
    }

    pub fn can_undo(&self) -> bool {
        !self.done.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.undone.is_empty()
    }

    pub fn is_clean(&self) -> bool {
        self.clean == Some(self.done.len())
    }

    pub fn set_clean(&mut self) {
        self.clean = Some(self.done.len());
    }

    pub fn clear(&mut self) {
        *self = UndoStack::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::entities::line;
    use crate::model::geometry::pt;

    fn ids(document: &Document) -> Vec<EntityId> {
        document.entities().iter().map(|e| e.id).collect()
    }

    #[test]
    fn removed_entities_come_back_where_they_were() {
        let entities: Vec<Entity> = (0..5).map(|i| line(pt(i as f64, 0.0), pt(i as f64, 1.0))).collect();
        let mut document = Document::with_entities(entities.clone());
        let original = ids(&document);
        let mut stack = UndoStack::default();
        stack.push(Change::remove(vec![entities[3].clone(), entities[1].clone()]), &mut document);
        assert_eq!(ids(&document), vec![original[0], original[2], original[4]]);
        stack.undo(&mut document);
        assert_eq!(ids(&document), original);
        stack.redo(&mut document);
        assert_eq!(document.len(), 3);
    }

    #[test]
    fn modify_undoes_to_the_values_before() {
        let target = line(pt(0.0, 0.0), pt(1.0, 0.0));
        let mut document = Document::with_entities(vec![target.clone()]);
        let mut stack = UndoStack::default();
        let moved = target.with_shape(crate::model::entities::Shape::Line { start: pt(5.0, 5.0), end: pt(6.0, 5.0) });
        let change = Change::modify(&document, vec![moved.clone()]);
        stack.push(change, &mut document);
        assert!(document.get(target.id).unwrap().same_state(&moved));
        stack.undo(&mut document);
        assert!(document.get(target.id).unwrap().same_state(&target));
    }

    #[test]
    fn a_group_undoes_as_one_and_cleanliness_follows_saves() {
        let mut document = Document::new();
        let mut stack = UndoStack::default();
        stack.set_clean();
        stack.begin_group();
        stack.push(Change::Add(vec![line(pt(0.0, 0.0), pt(1.0, 0.0))]), &mut document);
        stack.push(Change::Add(vec![line(pt(0.0, 1.0), pt(1.0, 1.0))]), &mut document);
        stack.end_group();
        assert!(!stack.is_clean());
        assert!(stack.undo(&mut document));
        assert!(document.is_empty());
        assert!(stack.is_clean());
        assert!(!stack.undo(&mut document));
    }
}
