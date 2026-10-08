//! Paired building entrances; content chooses eligible passengers and the exit body.
use super::*;

pub(in crate::sim) fn initialize_spawn_links(map: &Map, state: &mut State) {
    for spawn in &map.spawns {
        if let Some(point) = spawn.linked_to {
            let source = state.entities.iter().position(|e| {
                e.position == spawn.position
                    && e.owner == spawn.owner
                    && e.unit_type == spawn.unit_type
                    && e.linked_to.is_none()
            });
            let peer = state.entities.iter().position(|e| {
                e.position == point
                    && e.owner == spawn.owner
                    && e.unit_type == spawn.unit_type
                    && e.linked_to.is_none()
            });
            if let (Some(a), Some(b)) = (source, peer)
                && a != b
            {
                state.entities[a].linked_to = Some(state.entities[b].id);
                state.entities[b].linked_to = Some(state.entities[a].id);
            }
        }
    }
}

impl World {
    pub(in crate::sim) fn linked_exit(&self, actor: &Entity) -> Option<usize> {
        let i = self.index(actor.linked_to?)?;
        let peer = &self.state.entities[i];
        (peer.hp > 0
            && peer.owner == actor.owner
            && peer.construction.is_none()
            && peer.linked_to == Some(actor.id)
            && !self.disabled(peer))
        .then_some(i)
    }

    pub(in crate::sim) fn link_placement_rejection(
        &self,
        actor: &Entity,
        exit: UnitTypeId,
        point: Position,
    ) -> Option<Rejection> {
        if actor.linked_to.is_some() {
            return Some(Rejection::QueueFull);
        }
        let unit = self.unit_type(exit).unwrap();
        if !self.map.contains(point)
            || self.visibility(actor.owner, point) == Visibility::Unexplored
            || !self.map.can_build(point, unit.placement)
            || !self.creep_placement_allowed(unit, point)
            || !self.powered_position(actor.owner, unit, point)
            || !self.resource_clearance_allowed(unit, point)
            || self.placement_occupied_except(point, unit.placement, None, false, None, None)
            || self.placement_occupied_except(point, unit.footprint, None, true, None, None)
        {
            return Some(Rejection::InvalidPlacement);
        }
        if self.state.entities.len() >= rts::MAX_ENTITIES || self.state.next_entity_id == u32::MAX {
            return Some(Rejection::EntityLimit);
        }
        None
    }

    pub(in crate::sim) fn create_linked_exit(
        &mut self,
        index: usize,
        unit: UnitTypeId,
        point: Position,
    ) {
        let source = self.state.entities[index].clone();
        if self
            .link_placement_rejection(&source, unit, point)
            .is_some()
        {
            return;
        }
        let definition = self.unit_type(unit).unwrap().clone();
        let Some(id) = self.spawn_offspring(source.owner, unit, point, None) else {
            return;
        };
        let exit = self.index(id).unwrap();
        self.state.entities[index].linked_to = Some(id);
        self.state.entities[exit].linked_to = Some(source.id);
        self.state.entities[exit].hp = (definition.max_hp / 10).max(1);
        self.state.entities[exit].construction = Some(Construction {
            worker: None,
            remaining: definition.build_ticks,
            total: definition.build_ticks,
            work_position: Some(point),
            work_ticks: 0,
        });
    }

    pub(in crate::sim) fn advance_linked_transport(
        &mut self,
        index: usize,
        provider: usize,
        ability: AbilityId,
    ) {
        let entrance = self.state.entities[provider].clone();
        if !self.approach(
            index,
            entrance.position,
            self.unit_at(provider).footprint,
            1,
        ) {
            return;
        }
        let Some(exit) = self.linked_exit(&entrance) else {
            return;
        };
        let destination = self.state.entities[exit].clone();
        let actor = self.state.entities[index].clone();
        let unit = self.unit_at(index);
        let spot = rts::perimeter(
            destination.position,
            self.unit_at(exit).footprint,
            unit.footprint,
            actor.position,
        )
        .into_iter()
        .find(|p| self.can_place(*p, unit.footprint, unit.movement_class, Some(actor.id)));
        let Some(spot) = spot else {
            return;
        }; // A blocked exit retains the order at the entrance.
        self.finish(index);
        self.state.entities[index].position = spot;
        self.state.entities[index].motion_fraction = [0; 2];
        for endpoint in [provider, exit] {
            self.state.entities[endpoint].last_cast = Some(CastAppearance {
                ability,
                tick: self.tick(),
                position: self.state.entities[endpoint].position,
                origin: self.state.entities[endpoint].position,
            });
        }
    }

    pub(in crate::sim) fn remove_dead_links(&mut self) {
        let dead: BTreeSet<_> = self
            .state
            .entities
            .iter()
            .filter(|e| e.hp == 0)
            .map(|e| e.id)
            .collect();
        for entity in &mut self.state.entities {
            if entity.linked_to.is_some_and(|id| dead.contains(&id)) {
                entity.hp = 0;
                entity.linked_to = None;
            }
        }
    }
}
