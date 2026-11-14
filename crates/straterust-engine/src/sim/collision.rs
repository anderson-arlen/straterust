//! Shared broad-phase occupancy. Exact collision rules still read current state.
use super::*;
use std::sync::Mutex;

mod navigation;
mod queries;
mod spatial;
#[cfg(test)]
mod tests;
pub(in crate::sim) use queries::{CollisionPolicy, CollisionView};
use spatial::SpatialIndex;

pub(in crate::sim) type Bounds = [i64; 4];

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(in crate::sim) enum ColliderId {
    Entity(EntityId),
    Resource(ResourceId),
    Reservation(EntityId),
}

#[derive(Debug, Default)]
struct CacheData {
    index: Option<SpatialIndex>,
    dirty: Vec<ColliderId>,
    navigation: navigation::StaticNavigation,
}

#[derive(Debug, Default)]
pub(super) struct CollisionCache {
    data: Mutex<CacheData>,
}

// Confirmed baselines and snapshots should not copy a large derived hash map.
impl Clone for CollisionCache {
    fn clone(&self) -> Self {
        Self::default()
    }
}

impl CollisionCache {
    pub(in crate::sim) fn changed(&self, id: EntityId) {
        let mut data = self.data.lock().unwrap();
        data.dirty.push(ColliderId::Entity(id));
        data.navigation.changed(ColliderId::Entity(id));
    }
    pub(in crate::sim) fn resource_changed(&self, id: ResourceId) {
        let mut data = self.data.lock().unwrap();
        data.dirty.push(ColliderId::Resource(id));
        data.navigation.changed(ColliderId::Resource(id));
    }
    pub(in crate::sim) fn navigation_current(&self, world: &World) -> bool {
        self.data.lock().unwrap().navigation.current(world)
    }
    pub(in crate::sim) fn navigation_geometry(
        &self,
        world: &World,
    ) -> Option<Vec<crate::path::Obstacle>> {
        self.data.lock().unwrap().navigation.refresh(world)
    }
    fn candidates(&self, world: &World, bounds: Bounds) -> Vec<ColliderId> {
        let mut data = self.data.lock().unwrap();
        let CacheData {
            index: slot, dirty, ..
        } = &mut *data;
        if slot.is_none() {
            let mut index = SpatialIndex::new(world);
            index.reconcile(world);
            *slot = Some(index);
        } else {
            dirty.sort_unstable();
            dirty.dedup();
            let index = slot.as_mut().unwrap();
            for &id in dirty.iter() {
                match id {
                    ColliderId::Entity(id) | ColliderId::Reservation(id) => {
                        index.sync_entity(world, id)
                    }
                    ColliderId::Resource(id) => index.sync_resource(world, id),
                }
            }
        }
        dirty.clear();
        slot.as_ref().unwrap().candidates(bounds)
    }
}

impl World {
    pub(in crate::sim) fn collision_candidates(&self, bounds: Bounds) -> Vec<ColliderId> {
        self.collision.candidates(self, bounds)
    }
    pub(in crate::sim) fn collision_entities(
        &self,
        bounds: Bounds,
    ) -> impl Iterator<Item = &Entity> {
        self.collision_candidates(bounds)
            .into_iter()
            .filter_map(move |id| {
                let ColliderId::Entity(id) = id else {
                    return None;
                };
                self.index(id).map(|at| &self.state.entities[at])
            })
    }
    pub(in crate::sim) fn collision_resources(
        &self,
        bounds: Bounds,
    ) -> impl Iterator<Item = &ResourceNode> {
        self.collision_candidates(bounds)
            .into_iter()
            .filter_map(move |id| {
                let ColliderId::Resource(id) = id else {
                    return None;
                };
                self.collision_resource(id)
            })
    }
    pub(in crate::sim) fn collision_resource(&self, id: ResourceId) -> Option<&ResourceNode> {
        id.0.checked_sub(1)
            .and_then(|at| self.state.resources.get(at as usize))
            .filter(|node| node.id == id)
            .or_else(|| {
                self.state
                    .resources
                    .binary_search_by_key(&id, |node| node.id)
                    .ok()
                    .map(|at| &self.state.resources[at])
            })
    }
    /// Transfer only same-player/map derived occupancy, then reconcile it to
    /// corrected positions. Authoritative and filtered worlds never share it.
    pub fn reuse_collision_cache(&mut self, previous: &mut Self) {
        if self.rules_hash != previous.rules_hash
            || self.map_hash != previous.map_hash
            || self.view.as_ref().map(|v| v.player) != previous.view.as_ref().map(|v| v.player)
        {
            return;
        }
        let mut cache = std::mem::take(&mut previous.collision);
        {
            let data = cache.data.get_mut().unwrap();
            if let Some(index) = &mut data.index {
                index.reconcile(self);
            }
            data.dirty.clear();
            // The incoming world's published static baseline is already based
            // on its corrected/disclosed state; transfer only spatial occupancy.
            std::mem::swap(
                &mut data.navigation,
                &mut self.collision.data.get_mut().unwrap().navigation,
            );
        }
        self.collision = cache;
    }
}

pub(in crate::sim) fn intersects(a: Bounds, b: Bounds) -> bool {
    a[0] < b[2] && a[2] > b[0] && a[1] < b[3] && a[3] > b[1]
}
