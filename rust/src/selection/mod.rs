pub mod picking;

use std::collections::HashSet;

use crate::model::entities::EntityId;

/// The selected objects, in the order they were selected.
#[derive(Clone, Debug, Default)]
pub struct Selection {
    order: Vec<EntityId>,
    members: HashSet<EntityId>,
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
    }

    pub fn remove(&mut self, ids: impl IntoIterator<Item = EntityId>) {
        let doomed: HashSet<EntityId> = ids.into_iter().collect();
        self.members.retain(|id| !doomed.contains(id));
        self.order.retain(|id| !doomed.contains(id));
    }

    pub fn clear(&mut self) {
        self.order.clear();
        self.members.clear();
    }

    /// Forget anything that is no longer among the given objects.
    pub fn keep_only(&mut self, alive: &HashSet<EntityId>) {
        self.members.retain(|id| alive.contains(id));
        self.order.retain(|id| alive.contains(id));
    }
}
