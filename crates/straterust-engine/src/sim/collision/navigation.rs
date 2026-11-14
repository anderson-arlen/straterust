//! Static layout changes come from collider mutations, never a per-tick map scan.
use super::*;
use crate::path::Obstacle;

#[cfg(test)]
mod tests;

#[derive(Debug, Default)]
pub(super) struct StaticNavigation {
    initialized: bool,
    published: bool,
    dirty: bool,
    pending: Vec<ColliderId>,
    geometry: BTreeMap<ColliderId, Obstacle>,
    extractors: BTreeMap<EntityId, (Position, String)>,
}

impl StaticNavigation {
    pub fn changed(&mut self, id: ColliderId) {
        if self.initialized {
            self.pending.push(id);
        }
    }
    fn entity(&mut self, world: &World, id: EntityId) -> bool {
        let entity = world.index(id).map(|at| &world.state.entities[at]);
        self.obstacle(
            ColliderId::Entity(id),
            entity.and_then(|e| world.static_entity_obstacle(e)),
        );
        let extractor = entity.filter(|e| !e.airborne).and_then(|e| {
            world
                .unit_type(e.unit_type)?
                .extracts
                .as_ref()
                .map(|x| (e.position, x.resource.clone()))
        });
        if self.extractors.get(&id) == extractor.as_ref() {
            return false;
        }
        if let Some(extractor) = extractor {
            self.extractors.insert(id, extractor);
        } else {
            self.extractors.remove(&id);
        }
        true
    }
    fn resource(&mut self, node: &ResourceNode) {
        let blocks = (node.amount > 0 || node.requires_extractor)
            && !(node.requires_extractor
                && self
                    .extractors
                    .values()
                    .any(|(position, kind)| *position == node.position && *kind == node.kind));
        self.obstacle(
            ColliderId::Resource(node.id),
            blocks.then_some(Obstacle {
                position: node.position,
                footprint: node.footprint,
                movement_class: MovementClass::Ground,
            }),
        );
    }
    fn obstacle(&mut self, id: ColliderId, current: Option<Obstacle>) {
        if self.geometry.get(&id).copied() == current {
            return;
        }
        self.dirty = true;
        if let Some(obstacle) = current {
            self.geometry.insert(id, obstacle);
        } else {
            self.geometry.remove(&id);
        }
    }
    fn sync(&mut self, world: &World) {
        if !self.initialized {
            for entity in &world.state.entities {
                self.entity(world, entity.id);
            }
            for node in &world.state.resources {
                self.resource(node);
            }
            self.initialized = true;
            return;
        }
        let mut pending = std::mem::take(&mut self.pending);
        pending.sort_unstable();
        pending.dedup();
        let mut extractors_changed = false;
        for &id in &pending {
            if let ColliderId::Entity(id) = id {
                extractors_changed |= self.entity(world, id);
            }
        }
        for &id in &pending {
            if let ColliderId::Resource(id) = id {
                if let Some(node) = world.collision_resource(id) {
                    self.resource(node);
                } else {
                    self.obstacle(ColliderId::Resource(id), None);
                }
            }
        }
        if extractors_changed {
            // Only extractor creation/removal/type/lift/position changes can
            // expose or cover a field. Ordinary resource quantities cannot.
            for node in world
                .state
                .resources
                .iter()
                .filter(|n| n.requires_extractor)
            {
                self.resource(node);
            }
        }
        pending.clear();
        self.pending = pending;
    }
    pub fn current(&mut self, world: &World) -> bool {
        self.sync(world);
        // Compare layout only after a real static contribution changed. This
        // also accepts a change undone within the tick, or an identical body
        // replaced under a new ID, without invalidating nearest-stop proofs.
        if self.published
            && self.dirty
            && self
                .geometry
                .values()
                .copied()
                .eq(world.navigation_geometry.iter().copied())
        {
            self.dirty = false;
        }
        self.published && !self.dirty
    }
    pub fn refresh(&mut self, world: &World) -> Option<Vec<Obstacle>> {
        if self.current(world) {
            return None;
        }
        self.published = true;
        self.dirty = false;
        // Entity IDs precede resource IDs, matching the original slice order.
        Some(self.geometry.values().copied().collect())
    }
}

impl World {
    fn static_entity_obstacle(&self, entity: &Entity) -> Option<Obstacle> {
        let unit = self.unit_type(entity.unit_type).expect("validated type");
        (unit.speed == 0
            && !entity.airborne
            && unit.blocks_movement
            && !unit.revealer
            && !self.phases_collision(entity)
            && entity.doodad_enabled != Some(false)
            && !self.inside_structure(entity)
            && entity.garrisoned_in.is_none())
        .then_some(Obstacle {
            position: entity.position,
            footprint: unit.footprint,
            movement_class: self.movement_class(entity),
        })
    }
}
