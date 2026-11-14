//! Paid primary-resource expansion, for extra income or impending depletion.
use super::*;

const REVIEW_TICKS: u64 = 64;
const DEPOT_RADIUS: i64 = 512;
const FORECAST_MS: u64 = 120_000;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiExpansion {
    pub unit_type: UnitTypeId,
    pub resource: Option<ResourceId>,
    pub scout: Option<EntityId>,
}

impl World {
    pub(super) fn ai_economic_center(&self, player: PlayerId) -> Option<UnitTypeId> {
        self.rules
            .units
            .iter()
            .find(|base| {
                !base.dropoff.is_empty()
                    && self.creation_allowed(player, base.id)
                    && self.state.entities.iter().any(|e| {
                        e.owner == player
                            && self.unit_type(e.unit_type).is_some_and(|u| {
                                u.worker.is_some()
                                    && u.builds.contains(&base.id)
                                    && (base.trains.contains(&u.id)
                                        || base
                                            .offspring
                                            .as_ref()
                                            .and_then(|o| self.unit_type(o.unit_type))
                                            .is_some_and(|p| p.trains.contains(&u.id)))
                            })
                    })
                    && self.state.entities.iter().any(|e| {
                        e.owner == player
                            && e.construction.is_none()
                            && !e.airborne
                            && self.ai_is_center(e, base.id)
                    })
            })
            .map(|u| u.id)
    }

    fn ai_is_center(&self, entity: &Entity, base: UnitTypeId) -> bool {
        entity.unit_type == base
            || self
                .unit_type(entity.unit_type)
                .is_some_and(|u| u.provides_types.contains(&base))
    }

    pub(super) fn ai_mine_served(&self, player: PlayerId, node: &ResourceNode) -> bool {
        self.state.entities.iter().any(|e| {
            e.owner == player
                && e.hp > 0
                && !e.airborne
                && rts::distance(e.position, node.position) <= DEPOT_RADIUS.pow(2)
                && self
                    .unit_type(e.unit_type)
                    .is_some_and(|u| u.dropoff.contains(&node.kind))
        })
    }

    fn ai_mines_running_low(
        &self,
        controller: &AiController,
        state: &AiState,
        base: &UnitType,
    ) -> bool {
        let kind = &base.dropoff[0];
        let probe = self.state.entities.iter().find(|e| {
            e.owner == controller.player
                && self.ai_in_town(controller, state, e)
                && !self.inside_structure(e)
                && e.garrisoned_in.is_none()
                && self
                    .unit_type(e.unit_type)
                    .and_then(|u| u.worker.as_ref())
                    .is_some_and(|w| w.resource_kinds.contains(kind))
        });
        let working: BTreeSet<_> = self
            .state
            .entities
            .iter()
            .filter(|e| {
                e.owner == controller.player && (e.gathering_inside || e.harvest_progress > 0)
            })
            .filter_map(|e| {
                if let UnitOrder::Gather { resource } = e.order {
                    Some(resource)
                } else {
                    None
                }
            })
            .collect();
        let remaining: u64 = self
            .state
            .resources
            .iter()
            .filter(|r| {
                r.kind == *kind
                    && self.visibility(controller.player, r.position) != Visibility::Unexplored
                    && self.ai_mine_served(controller.player, r)
                    && (working.contains(&r.id)
                        || probe.is_some_and(|e| self.ai_resource_approach(e, r).is_some()))
            })
            .map(|r| u64::from(r.amount))
            .sum();
        let horizon = FORECAST_MS / u64::from(self.rules.tick_ms);
        let mut consumption = 0;
        for actor in self
            .state
            .entities
            .iter()
            .filter(|e| e.owner == controller.player && self.ai_in_town(controller, state, e))
        {
            let UnitOrder::Gather { resource } = actor.order else {
                continue;
            };
            let Some(node) = self
                .collision_resource(resource)
                .filter(|r| r.kind == *kind)
            else {
                continue;
            };
            let unit = self.unit_type(actor.unit_type).unwrap();
            let Some(worker) = &unit.worker else { continue };
            let profile = unit.harvest_profiles.iter().find(|p| p.kind == *kind);
            let harvest = profile.map_or(worker.harvest_ticks, |p| p.ticks);
            let delivery = profile.map_or(0, |p| p.depot_ticks);
            let travel = self
                .state
                .entities
                .iter()
                .filter(|e| {
                    e.owner == actor.owner
                        && e.construction.is_none()
                        && !e.airborne
                        && self
                            .unit_type(e.unit_type)
                            .is_some_and(|u| u.dropoff.contains(kind))
                })
                .map(|e| rts::distance(e.position, node.position) as u64)
                .min()
                .unwrap_or(0)
                .isqrt()
                * 2
                / unit.speed.max(1) as u64;
            consumption += u64::from(worker.capacity) * horizon
                / (u64::from(harvest) + u64::from(delivery) + travel).max(1);
        }
        let construction = base
            .cost
            .iter()
            .find(|c| c.kind == *kind)
            .map_or(0, |c| u64::from(c.amount));
        remaining <= consumption.saturating_add(construction)
    }

    fn ai_expansion_worker(
        &self,
        controller: &AiController,
        state: &AiState,
        base: UnitTypeId,
        preferred: Option<EntityId>,
    ) -> Option<EntityId> {
        self.state
            .entities
            .iter()
            .filter(|e| {
                e.owner == controller.player
                    && e.hp > 0
                    && e.construction.is_none()
                    && e.garrisoned_in.is_none()
                    && e.cargo.is_none()
                    && e.auto_attack_target.is_none()
                    && self.ai_in_town(controller, state, e)
                    && matches!(
                        e.order,
                        UnitOrder::Idle | UnitOrder::Gather { .. } | UnitOrder::Move { .. }
                    )
                    && self
                        .unit_type(e.unit_type)
                        .is_some_and(|u| u.worker.is_some() && u.builds.contains(&base))
            })
            .min_by_key(|e| {
                (
                    Some(e.id) != preferred,
                    self.inside_structure(e),
                    !matches!(e.order, UnitOrder::Idle),
                    e.id,
                )
            })
            .map(|e| e.id)
    }

    pub(super) fn ai_mine_threatened(&self, player: PlayerId, position: Position) -> bool {
        self.state.entities.iter().any(|e| {
            self.is_automatic_target(player, e)
                && rts::distance(e.position, position) < 384_i64.pow(2)
                && self.entity_visible(player, e.id)
                && self
                    .unit_type(e.unit_type)
                    .is_some_and(|u| u.structure || u.weapon.is_some())
        })
    }

    fn ai_next_mine(
        &self,
        controller: &AiController,
        worker: &Entity,
        kind: &str,
    ) -> Option<ResourceId> {
        let limit = self.ai_mine_limit();
        let sites = if limit.is_some() {
            self.ai_mining_sites(controller, kind)
        } else {
            Vec::new()
        };
        let mut candidates: Vec<_> = self
            .state
            .resources
            .iter()
            .filter(|r| {
                r.kind == kind
                    && r.amount > 0
                    && !r.requires_extractor
                    && match limit {
                        Some(limit) => {
                            if sites.iter().any(|(id, _)| self.ai_same_mine(*id, r.id)) {
                                !self.ai_mine_served(controller.player, r)
                            } else {
                                sites.len() < limit
                            }
                        }
                        None => !self.ai_mine_served(controller.player, r),
                    }
                    && self.visibility(controller.player, r.position) != Visibility::Unexplored
                    && !self.ai_mine_threatened(controller.player, r.position)
            })
            .collect();
        candidates.sort_by_key(|r| {
            (
                !sites.iter().any(|(id, _)| self.ai_same_mine(*id, r.id)),
                rts::distance(controller.home, r.position),
                r.id,
            )
        });
        let offset = (self.tick().0 / REVIEW_TICKS % candidates.len().div_ceil(4).max(1) as u64)
            as usize
            * 4;
        candidates
            .iter()
            .take(4)
            .chain(candidates.iter().skip(4 + offset).take(4))
            .find(|r| self.ai_resource_approach(worker, r).is_some())
            .map(|r| r.id)
    }

    fn ai_should_expand(
        &self,
        controller: &AiController,
        state: &AiState,
        base: &UnitType,
    ) -> bool {
        if let Some(limit) = self.ai_mine_limit() {
            self.ai_growth_needed(controller, state, base)
                || (self.ai_mining_sites(controller, &base.dropoff[0]).len() < limit
                    && self.ai_mines_running_low(controller, state, base))
        } else {
            self.ai_mines_running_low(controller, state, base)
        }
    }

    fn ai_scout_resources(
        &mut self,
        controller: &AiController,
        state: &mut AiState,
        entity: EntityId,
    ) -> bool {
        let actor = &self.state.entities[self.index(entity).unwrap()];
        if matches!(actor.order, UnitOrder::Move { .. })
            && (!actor.path.is_empty() || actor.route_wait.is_some())
        {
            return true;
        }
        let unit = self.unit_type(actor.unit_type).unwrap();
        let mut targets = Vec::new();
        // Scan coarse fog coordinates, not undisclosed resource locations.
        for y in (64..self.map.height).step_by(128) {
            for x in (64..self.map.width).step_by(128) {
                let p = Position { x, y };
                if self.visibility(controller.player, p) == Visibility::Unexplored
                    && self.map.can_move(p, unit.footprint, unit.movement_class)
                {
                    targets.push(p);
                }
            }
        }
        targets.sort_by_key(|p| (rts::distance(actor.position, *p), p.y, p.x));
        let Some((target, _)) = self.navigator().find_path_to_any(
            unit.footprint,
            unit.movement_class,
            actor.position,
            &targets,
        ) else {
            return false;
        };
        self.ai_order(controller, state, Order::Move { entity, target })
    }

    pub(super) fn ai_prepare_expansion(
        &mut self,
        controller: &AiController,
        state: &mut AiState,
    ) -> Option<AiRequest> {
        if !Self::ai_manages_resources(controller) {
            return None;
        }
        let review = self.tick().0.is_multiple_of(REVIEW_TICKS);
        if review {
            self.refresh_navigation_geometry();
        }
        if state.expansion.is_none() {
            if !review {
                return None;
            }
            let unit_type = self.ai_economic_center(controller.player)?;
            if !self.ai_should_expand(controller, state, self.unit_type(unit_type).unwrap()) {
                return None;
            }
            state.expansion = Some(AiExpansion {
                unit_type,
                resource: None,
                scout: None,
            });
        }
        let mut plan = state.expansion.take().unwrap();
        let base = self.unit_type(plan.unit_type)?;
        let kind = base.dropoff.first()?.clone();
        // A scout may reveal a rich mine already served by the existing base.
        // Release the reserve instead of continuing an unnecessary expansion.
        if review && plan.resource.is_none() && !self.ai_should_expand(controller, state, base) {
            if let Some(scout) = plan.scout
                && self.index(scout).is_some_and(|at| {
                    matches!(self.state.entities[at].order, UnitOrder::Move { .. })
                })
            {
                self.ai_order(controller, state, Order::Stop { entity: scout });
            }
            return None;
        }
        let mut building = false;
        if !self.state.entities.iter().any(|e| {
            e.owner == controller.player
                && self
                    .unit_type(e.unit_type)
                    .is_some_and(|u| u.worker.is_some() && u.builds.contains(&plan.unit_type))
        }) {
            return None;
        }
        if let Some(node) = plan
            .resource
            .and_then(|id| self.collision_resource(id))
            .filter(|r| r.amount > 0)
        {
            let bases: Vec<_> = self
                .state
                .entities
                .iter()
                .filter(|e| {
                    e.owner == controller.player
                        && self.ai_is_center(e, plan.unit_type)
                        && !e.airborne
                        && rts::distance(e.position, node.position) <= DEPOT_RADIUS.pow(2)
                })
                .map(|e| (e.id, e.construction.is_none()))
                .collect();
            for &(id, _) in &bases {
                state.members.insert(id);
            }
            building = !bases.is_empty() || self.state.entities.iter().any(|e| e.owner == controller.player
                && matches!(e.order, UnitOrder::PlaceBuilding { unit_type, target }
                    if unit_type == plan.unit_type && rts::distance(target, node.position) <= DEPOT_RADIUS.pow(2)));
            if bases.iter().any(|(_, complete)| *complete) {
                if let Some(scout) = plan
                    .scout
                    .filter(|&id| self.gather_rejection(id, node.id).is_none())
                {
                    self.ai_order(
                        controller,
                        state,
                        Order::Gather {
                            entity: scout,
                            resource: node.id,
                        },
                    );
                }
                return None;
            }
        } else {
            plan.resource = None;
        }
        if review
            && !building
            && let Some(worker) =
                self.ai_expansion_worker(controller, state, plan.unit_type, plan.scout)
        {
            let index = self.index(worker).unwrap();
            if !self.inside_structure(&self.state.entities[index])
                || self.ai_order(controller, state, Order::Stop { entity: worker })
            {
                plan.scout = Some(worker);
                if plan.resource.is_none() {
                    plan.resource =
                        self.ai_next_mine(controller, &self.state.entities[index], &kind);
                }
                if plan.resource.is_none() && !self.ai_scout_resources(controller, state, worker) {
                    return None;
                }
                // A second deposit can share an existing depot. Activate it
                // immediately instead of requesting a redundant town hall.
                if let Some(resource) = plan.resource
                    && self
                        .collision_resource(resource)
                        .is_some_and(|r| self.ai_mine_ready(controller.player, r))
                {
                    self.ai_order(
                        controller,
                        state,
                        Order::Gather {
                            entity: worker,
                            resource,
                        },
                    );
                    return None;
                }
                if let Some(resource) = plan.resource
                    && matches!(self.state.entities[index].order, UnitOrder::Move { .. })
                {
                    self.ai_order(
                        controller,
                        state,
                        Order::Gather {
                            entity: worker,
                            resource,
                        },
                    );
                }
            }
        }
        let complete = self
            .state
            .entities
            .iter()
            .filter(|e| {
                e.owner == controller.player
                    && e.construction.is_none()
                    && self.ai_is_center(e, plan.unit_type)
            })
            .count();
        let request = AiRequest {
            unit_type: plan.unit_type,
            count: (complete + 1).min(4096) as u16,
            priority: 255,
            include_deployed: true,
        };
        state.expansion = Some(plan);
        Some(request)
    }

    pub(super) fn ai_expansion_build(&mut self, controller: &AiController, state: &mut AiState) {
        if !self.tick().0.is_multiple_of(REVIEW_TICKS) {
            return;
        }
        let Some(plan) = state.expansion.clone() else {
            return;
        };
        let Some(node) = plan
            .resource
            .and_then(|id| self.collision_resource(id))
            .cloned()
        else {
            return;
        };
        let base = self.unit_type(plan.unit_type).unwrap().clone();
        let mut builders: Vec<_> = self
            .state
            .entities
            .iter()
            .filter(|e| {
                e.owner == controller.player
                    && e.construction.is_none()
                    && !self.inside_structure(e)
                    && e.garrisoned_in.is_none()
                    && !matches!(
                        e.order,
                        UnitOrder::Build { .. }
                            | UnitOrder::PlaceBuilding { .. }
                            | UnitOrder::Repair { .. }
                    )
                    && self.ai_in_town(controller, state, e)
                    && self
                        .unit_type(e.unit_type)
                        .is_some_and(|u| u.builds.contains(&base.id))
            })
            .map(|e| e.id)
            .collect();
        builders.sort_by_key(|id| {
            (
                Some(*id) != plan.scout,
                rts::distance(
                    self.state.entities[self.index(*id).unwrap()].position,
                    node.position,
                ),
                *id,
            )
        });
        builders.truncate(2);
        if builders.is_empty() {
            return;
        }
        let mut sites = Vec::new();
        for ring in 3_i32..=16 {
            for dy in -ring..=ring {
                for dx in -ring..=ring {
                    if dx.abs() != ring && dy.abs() != ring {
                        continue;
                    }
                    sites.push(Position {
                        x: node.position.x / 32 * 32 + dx * 32,
                        y: node.position.y / 32 * 32 + dy * 32,
                    });
                }
            }
        }
        sites.sort_by_key(|p| {
            (
                rts::distance(*p, node.position),
                rts::distance(*p, controller.home),
                p.y,
                p.x,
            )
        });
        let first = builders[0];
        let sites: Vec<_> = sites
            .into_iter()
            .filter(|p| {
                self.map.contains(*p)
                    && rts::distance(*p, node.position) <= DEPOT_RADIUS.pow(2)
                    && self.visibility(controller.player, *p) != Visibility::Unexplored
                    && self
                        .build_rejection(controller.player, first, base.id, *p)
                        .is_none()
                    && self.ai_building_space(controller.player, &base, *p, false)
            })
            .take(64)
            .collect();
        if let Some((entity, position)) =
            self.ai_build_site(controller.player, &mut builders, &base, &sites)
        {
            self.ai_order(
                controller,
                state,
                Order::Build {
                    entity,
                    unit_type: base.id,
                    position,
                },
            );
        }
    }

    pub(super) fn ai_expansion_reserve<'a>(
        &'a self,
        controller: &AiController,
        state: &AiState,
    ) -> &'a [ResourceAmount] {
        let Some(plan) = &state.expansion else {
            return &[];
        };
        let paid = plan
            .resource
            .and_then(|id| self.collision_resource(id))
            .is_some_and(|node| {
                self.state.entities.iter().any(|e| {
                    e.owner == controller.player
                        && self.ai_is_center(e, plan.unit_type)
                        && e.construction.is_some()
                        && rts::distance(e.position, node.position) <= DEPOT_RADIUS.pow(2)
                })
            });
        if paid {
            &[]
        } else {
            &self.unit_type(plan.unit_type).unwrap().cost
        }
    }
}
