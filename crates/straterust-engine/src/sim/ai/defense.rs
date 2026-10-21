//! Damage calls for help independently of the victim's ability to retaliate.
use super::*;

impl World {
    pub(in crate::sim) fn ai_help_on_damage(
        &mut self,
        index: usize,
        sources: &BTreeMap<EntityId, u64>,
    ) {
        let victim = &self.state.entities[index];
        if victim.invincible {
            return;
        }
        let Some(town) = self.map.ai.iter().enumerate().position(|(i, c)| {
            c.player == victim.owner && (!c.takeover || self.state.ai[i].active)
        }) else {
            return;
        };
        let Some((&attacker, &amount)) = sources
            .iter()
            .filter(|(_, amount)| **amount > 0)
            .filter(|(id, _)| {
                self.index(**id).is_some_and(|other| {
                    let enemy = &self.state.entities[other];
                    enemy.hp > 0 && self.is_automatic_target(victim.owner, enemy)
                })
            })
            .max_by_key(|(id, amount)| (**amount, std::cmp::Reverse(**id)))
        else {
            return;
        };
        let enemy = self.state.entities[self.index(attacker).unwrap()].clone();
        let owner = victim.owner;
        let position = victim.position;
        // Buildings summon a wider defense, as in the source hit routine.
        let radius = if self.unit_at(index).structure {
            512_i64
        } else {
            256
        };
        let helpers: Vec<_> = self
            .state
            .entities
            .iter()
            .filter(|e| {
                e.owner == owner
                    && e.id != victim.id
                    && e.hp > 0
                    && e.construction.is_none()
                    && e.garrisoned_in.is_none()
                    && rts::distance(position, e.position) <= radius.pow(2)
                    && matches!(
                        e.order,
                        UnitOrder::Idle | UnitOrder::AttackMove { .. } | UnitOrder::Patrol { .. }
                    )
                    && self
                        .unit_type(e.unit_type)
                        .is_some_and(|u| u.worker.is_none() && u.weapon.is_some())
                    && self.can_target_entity(e, &enemy)
                    && !self.undetected(owner, &enemy)
            })
            .map(|e| e.id)
            .collect();
        let hit = BTreeMap::from([(attacker, amount)]);
        for id in helpers {
            let helper = self.index(id).unwrap();
            let deployed = self.state.ai.iter().any(|s| s.deployed.contains(&id));
            if !deployed && !self.unit_at(helper).structure {
                let home = *self.state.ai[town]
                    .guards
                    .entry(id)
                    .or_insert(self.state.entities[helper].position);
                let range = self
                    .weapon_for(&self.state.entities[helper], &enemy)
                    .unwrap()
                    .range;
                if rts::distance(home, enemy.position) > i64::from(range + 192).pow(2) {
                    continue;
                }
            }
            if self.movement_locked(&self.state.entities[helper]) {
                self.assign(
                    helper,
                    UnitOrder::AttackMove {
                        target: enemy.position,
                    },
                    false,
                );
            }
            self.react_to_damage(helper, &hit);
        }
    }

    pub(super) fn ai_defend(&mut self, controller: &AiController, state: &mut AiState) {
        if controller.takeover {
            self.ai_yield_to_workers(controller, state);
        }
        let guards: Vec<_> = self
            .state
            .entities
            .iter()
            .filter(|e| {
                e.owner == controller.player
                    && e.hp > 0
                    && rts::distance(e.position, controller.home)
                        <= i64::from(controller.radius).pow(2)
                    && e.construction.is_none()
                    && !state.deployed.contains(&e.id)
                    && !self.state.ai.iter().any(|s| s.deployed.contains(&e.id))
                    && e.garrisoned_in.is_none()
                    && self
                        .unit_type(e.unit_type)
                        .is_some_and(|u| u.weapon.is_some() && u.worker.is_none() && !u.structure)
            })
            .map(|e| {
                (
                    e.id,
                    e.position,
                    e.order.clone(),
                    e.auto_attack_target,
                    self.movement_locked(e),
                )
            })
            .collect();
        for (entity, position, order, target, movement_locked) in guards {
            let home = *state.guards.entry(entity).or_insert(position);
            if target.is_none()
                && !movement_locked
                && matches!(order, UnitOrder::Idle)
                && rts::distance(position, home) > 32_i64.pow(2)
            {
                self.ai_order(
                    controller,
                    state,
                    Order::AttackMove {
                        entity,
                        target: home,
                    },
                );
            }
        }
    }

    fn ai_yield_to_workers(&mut self, controller: &AiController, state: &mut AiState) {
        let blocked: Vec<_> = self
            .state
            .entities
            .iter()
            .filter(|e| {
                e.owner == controller.player
                    && e.route_wait.as_ref().is_some_and(|w| {
                        self.tick().0.saturating_sub(w.since.0) * u64::from(self.rules.tick_ms)
                            >= 1000
                    })
                    && matches!(
                        e.order,
                        UnitOrder::Gather { .. } | UnitOrder::PlaceBuilding { .. }
                    )
                    && !self.inside_structure(e)
            })
            .map(|e| (e.position, self.movement_class(e)))
            .collect();
        for (origin, class) in blocked {
            let mut guards: Vec<_> = self
                .state
                .entities
                .iter()
                .filter(|e| {
                    e.owner == controller.player
                        && e.auto_attack_target.is_none()
                        && matches!(e.order, UnitOrder::Idle)
                        && e.garrisoned_in.is_none()
                        && self.movement_class(e) == class
                        && rts::distance(e.position, origin) <= 128_i64.pow(2)
                        && self.unit_type(e.unit_type).is_some_and(|u| {
                            !u.structure && u.worker.is_none() && u.weapon.is_some()
                        })
                })
                .map(|e| e.id)
                .collect();
            guards.sort_by_key(|id| {
                (
                    rts::distance(
                        self.state.entities[self.index(*id).unwrap()].position,
                        origin,
                    ),
                    *id,
                )
            });
            for entity in guards {
                let actor = &self.state.entities[self.index(entity).unwrap()];
                let unit = self.unit_type(actor.unit_type).unwrap();
                let away = Position {
                    x: actor.position.x * 2 - origin.x,
                    y: actor.position.y * 2 - origin.y,
                };
                let goals: Vec<_> = [64, 128, 192]
                    .into_iter()
                    .flat_map(|size| {
                        rts::perimeter(
                            actor.position,
                            Footprint {
                                width: size,
                                height: size,
                            },
                            unit.footprint,
                            away,
                        )
                    })
                    .filter(|&at| {
                        self.can_place(at, unit.footprint, class, Some(entity))
                            && rts::distance(at, origin) > rts::distance(actor.position, origin)
                    })
                    .collect();
                let obstacles = self.obstacles(entity);
                let data = crate::path::NavigationData::default();
                if let Some((target, _)) = crate::path::Pathfinder::new(
                    &self.map, &obstacles, &data,
                )
                .find_path_to_any(unit.footprint, class, actor.position, &goals)
                    && self.ai_order(controller, state, Order::AttackMove { entity, target })
                {
                    // Do not send it straight back into the worker's lane.
                    state.guards.insert(entity, target);
                    return;
                }
            }
        }
    }
}
