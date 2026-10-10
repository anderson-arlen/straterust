use super::*;

impl World {
    pub(super) fn gather_completed_extractor(&mut self, building: usize, worker: EntityId) {
        let Some(index) = self.index(worker) else {
            return;
        };
        let target = &self.state.entities[building];
        let actor = &self.state.entities[index];
        if actor.order
            != (UnitOrder::Build {
                building: target.id,
            })
            || !actor.queued_orders.is_empty()
        {
            return;
        }
        let extraction = self
            .unit_at(building)
            .extracts
            .as_ref()
            .expect("validated extractor");
        let Some(resource) = self
            .state
            .resources
            .iter()
            .find(|node| {
                node.position == target.position
                    && node.kind == extraction.resource
                    && node.requires_extractor
            })
            .map(|node| node.id)
        else {
            return;
        };
        let order = UnitOrder::Gather { resource };
        if self.action_rejection(index, &order).is_none() {
            self.assign(index, order, false);
        }
    }

    pub(super) fn builder_is_inside(&self, actor: &Entity) -> bool {
        let UnitOrder::Build { building } = actor.order else {
            return false;
        };
        self.state
            .entities
            .iter()
            .find(|e| e.id == building)
            .is_some_and(|e| {
                self.unit_type(e.unit_type)
                    .is_some_and(|u| u.builder_inside)
                    && e.construction
                        .as_ref()
                        .is_some_and(|c| c.worker == Some(actor.id) && c.work_position.is_some())
            })
    }

    /// Interior work is derived from the saved order and the foundation's job.
    /// There is no second timer or hidden state to migrate in old checkpoints.
    pub(in crate::sim) fn inside_structure(&self, actor: &Entity) -> bool {
        actor.gathering_inside || self.builder_is_inside(actor)
    }
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
                    || self.inside_structure(actor)
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
