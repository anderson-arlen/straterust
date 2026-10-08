use super::*;

impl World {
    /// Travel does not reserve resources, create an entity or obstruct navigation.
    pub(in crate::sim) fn place_building(
        &mut self,
        index: usize,
        unit_type: UnitTypeId,
        target: Position,
    ) {
        let unit = self
            .unit_type(unit_type)
            .expect("validated build type")
            .clone();
        if unit.addon_parent.is_none() && !self.approach(index, target, unit.footprint, 1) {
            return;
        }
        if unit.addon_parent.is_some() {
            // Reuse relocation validation if a queued landing could not finish.
            let actor = &self.state.entities[index];
            let command = Command {
                tick: self.tick(),
                player: actor.owner,
                sequence: 0,
                order: Order::Build {
                    entity: actor.id,
                    unit_type,
                    position: target,
                },
            };
            if self.apply(&command).is_some() {
                self.finish(index);
            }
            return;
        }
        let actor = &self.state.entities[index];
        match self.build_rejection(actor.owner, actor.id, unit_type, target) {
            None => {}
            // Funds may have been spent while travelling. Wait without creating
            // or charging for a foundation; another order can interrupt this.
            Some(Rejection::InsufficientResources) => return,
            Some(_) => {
                self.finish(index);
                return;
            }
        }
        let building = self.start_building(index, &unit, target);
        self.assign(index, UnitOrder::Build { building }, false);
        self.construct(index, building);
    }

    pub(super) fn start_building(
        &mut self,
        index: usize,
        unit: &UnitType,
        position: Position,
    ) -> EntityId {
        let owner = self.state.entities[index].owner;
        let worker = self.state.entities[index].id;
        self.pay(owner, &unit.cost, false);
        let id = EntityId(self.state.next_entity_id);
        self.state.next_entity_id += 1;
        self.state.entities.push(Entity {
            id,
            owner,
            unit_type: unit.id,
            position,
            hp: 1,
            parent: unit.addon_parent.map(|_| worker),
            construction: Some(Construction {
                worker: Some(worker),
                remaining: unit.build_ticks,
                total: unit.build_ticks,
                work_position: None,
                work_ticks: 0,
            }),
            ..Entity::default()
        });
        {
            // Source addon placement ignores mobile traffic. Move overlapping
            // bodies to a free edge so our solid foundation cannot trap them.
            for passenger in 0..self.state.entities.len() - 1 {
                let actor = &self.state.entities[passenger];
                let actor_type = self.unit_at(passenger);
                if (unit.addon_parent.is_none() && actor.id != worker)
                    || actor_type.speed == 0
                    || actor.airborne
                    || self.movement_locked(actor)
                    || actor.garrisoned_in.is_some()
                    || actor.gathering_inside
                    || !overlaps(
                        position,
                        unit.footprint,
                        actor.position,
                        actor_type.footprint,
                    )
                {
                    continue;
                }
                let exit = perimeter(
                    position,
                    unit.footprint,
                    actor_type.footprint,
                    actor.position,
                )
                .into_iter()
                .find(|point| {
                    self.can_place(
                        *point,
                        actor_type.footprint,
                        actor_type.movement_class,
                        Some(actor.id),
                    )
                });
                if let Some(exit) = exit {
                    let actor = &mut self.state.entities[passenger];
                    actor.position = exit;
                    actor.motion_fraction = [0, 0];
                    actor.path.clear();
                    actor.path_retry = self.state.tick;
                }
            }
        }
        id
    }
}
