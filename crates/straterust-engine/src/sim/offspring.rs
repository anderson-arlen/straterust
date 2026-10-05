//! Configured automatic production around a parent.
use super::*;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Offspring {
    pub unit_type: UnitTypeId,
    pub interval: u32,
    pub maximum: u8,
    pub initial: u8,
}

impl World {
    pub(in crate::sim) fn spawn_offspring(
        &mut self,
        owner: PlayerId,
        unit_type: UnitTypeId,
        position: Position,
        parent: Option<EntityId>,
    ) -> Option<EntityId> {
        if self.state.entities.len() >= rts::MAX_ENTITIES || self.state.next_entity_id == u32::MAX {
            return None;
        }
        let unit = self.unit_type(unit_type).unwrap();
        let entity = Entity {
            id: EntityId(self.state.next_entity_id),
            owner,
            unit_type,
            position,
            parent,
            hp: unit.max_hp,
            shields: unit.max_shields * 256,
            energy: unit.initial_energy(),
            ..Entity::default()
        };
        self.state.next_entity_id += 1;
        let id = entity.id;
        self.state.entities.push(entity);
        self.record_created(owner, unit_type);
        Some(id)
    }
    pub(in crate::sim) fn initialize_offspring(&mut self) {
        // Preplaced children belong to nearby matching providers too; otherwise
        // starting units are counted twice.
        let children: Vec<_> = self
            .state
            .entities
            .iter()
            .filter(|e| {
                e.parent.is_none()
                    && self.rules.units.iter().any(|unit| {
                        unit.offspring
                            .as_ref()
                            .is_some_and(|config| config.unit_type == e.unit_type)
                    })
            })
            .map(|e| e.id)
            .collect();
        for id in children {
            let index = self.index(id).unwrap();
            let child = &self.state.entities[index];
            let definition = self.unit_at(index);
            let parent = self
                .state
                .entities
                .iter()
                .filter(|e| e.owner == child.owner)
                .filter(|e| {
                    self.unit_type(e.unit_type)
                        .unwrap()
                        .offspring
                        .as_ref()
                        .is_some_and(|config| config.unit_type == child.unit_type)
                })
                .filter(|e| {
                    let unit = self.unit_type(e.unit_type).unwrap();
                    rts::in_range(
                        child.position,
                        definition.footprint,
                        e.position,
                        unit.footprint,
                        u32::from(unit.footprint.width.max(unit.footprint.height)),
                    )
                })
                .min_by_key(|e| (rts::distance(child.position, e.position), e.id))
                .map(|e| e.id);
            self.state.entities[index].parent = parent;
        }
        let parents: Vec<_> = self
            .state
            .entities
            .iter()
            .filter_map(|e| {
                self.unit_type(e.unit_type)
                    .unwrap()
                    .offspring
                    .as_ref()
                    .map(|config| (e.id, config.initial))
            })
            .collect();
        for (parent, count) in parents {
            for _ in 0..count {
                self.create_offspring(parent);
            }
        }
    }
    fn create_offspring(&mut self, parent: EntityId) {
        let Some(index) = self.index(parent) else {
            return;
        };
        let actor = self.state.entities[index].clone();
        let definition = self.unit_at(index);
        let Some(config) = definition.offspring.clone() else {
            return;
        };
        let count = self
            .state
            .entities
            .iter()
            .filter(|e| e.parent == Some(parent) && e.unit_type == config.unit_type)
            .count();
        if count >= usize::from(config.maximum) {
            self.state.entities[index].offspring_remaining = config.interval;
            return;
        }
        let child = self.unit_type(config.unit_type).unwrap();
        let point = rts::perimeter(
            actor.position,
            definition.footprint,
            child.footprint,
            actor.position,
        )
        .into_iter()
        .find(|p| self.can_place(*p, child.footprint, child.movement_class, None));
        if let Some(point) = point
            && let Some(id) =
                self.spawn_offspring(actor.owner, config.unit_type, point, Some(parent))
        {
            let child_index = self.index(id).unwrap();
            let child = &mut self.state.entities[child_index];
            child.rally = actor.rally;
            child.rally_resource = actor.rally_resource;
        }
        self.state.entities[index].offspring_remaining = config.interval;
    }
    pub(in crate::sim) fn advance_offspring(&mut self) {
        let parents: Vec<_> = self
            .state
            .entities
            .iter()
            .filter(|e| {
                e.construction.is_none() && self.unit_type(e.unit_type).unwrap().offspring.is_some()
            })
            .map(|e| e.id)
            .collect();
        for parent in parents {
            let index = self.index(parent).unwrap();
            self.state.entities[index].offspring_remaining = self.state.entities[index]
                .offspring_remaining
                .saturating_sub(1);
            if self.state.entities[index].offspring_remaining == 0 {
                self.create_offspring(parent);
            }
        }
    }
}
