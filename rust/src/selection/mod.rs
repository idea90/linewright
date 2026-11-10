pub mod picking;

use crate::model::entities::{EntityId, IdSet};

/// The selected objects, in the order they were selected.
#[derive(Clone, Debug, Default)]
pub struct Selection {
    order: Vec<EntityId>,
    members: IdSet,
    /// Goes up whenever the selection changes, so what was worked out from it can be kept until then.
    revision: u64,
}

impl Selection {
    pub fn ids(&self) -> &[EntityId] {
        &self.order
    }

    pub fn len(&self) -> usize {
        self.order.len()
    }

    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }

    /// A number that is different after any change to what is selected.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn contains(&self, id: EntityId) -> bool {
        self.members.contains(&id)
    }

    pub fn replace(&mut self, ids: impl IntoIterator<Item = EntityId>) {
        self.clear();
        self.add(ids);
    }

    pub fn add(&mut self, ids: impl IntoIterator<Item = EntityId>) {
        for id in ids {
            if self.members.insert(id) {
                self.order.push(id);
            }
        }
        self.revision += 1;
    }

    pub fn remove(&mut self, ids: impl IntoIterator<Item = EntityId>) {
        let doomed: IdSet = ids.into_iter().collect();
        self.members.retain(|id| !doomed.contains(id));
        self.order.retain(|id| !doomed.contains(id));
        self.revision += 1;
    }

    pub fn clear(&mut self) {
        self.order.clear();
        self.members.clear();
        self.revision += 1;
    }

    /// Forget anything that is no longer there to select, as `alive` tells of each. It asks about the selected
    /// objects only, so it costs nothing when little is selected, however large the drawing.
    pub fn keep_only(&mut self, alive: impl Fn(EntityId) -> bool) {
        if self.order.iter().all(|id| alive(*id)) {
            return;
        }
        self.members.retain(|id| alive(*id));
        self.order.retain(|id| alive(*id));
        self.revision += 1;
    }
}
