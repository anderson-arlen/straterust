//! Skirmish growth counts working deposits, not the number of bases ever built.
use super::*;
use crate::multiplayer::AiDifficulty;

impl World {
    pub(super) fn ai_recover_mining(&mut self, controller: &AiController, state: &mut AiState) {
        if !Self::ai_manages_resources(controller) || !self.tick().0.is_multiple_of(64) {
            return;
        }
        let Some(base) = self
            .ai_economic_center(controller.player)
            .and_then(|id| self.unit_type(id))
        else {
            return;
        };
        let kind = base.dropoff[0].clone();
        if !self.ai_mining_sites(controller, &kind).is_empty() {
            return;
        }
        // A healthy served mine suppresses replacement-base planning. It still
        // needs a worker if all previous miners took woodcutting jobs while it
        // was undiscovered/unavailable; waiting for an idle worker deadlocks.
        let mut workers: Vec<_> = self
            .state
            .entities
            .iter()
            .filter(|e| {
                e.owner == controller.player
                    && e.construction.is_none()
                    && e.garrisoned_in.is_none()
                    && e.cargo.is_none()
                    && e.auto_attack_target.is_none()
                    && e.queued_orders.is_empty()
                    && self.ai_in_town(controller, state, e)
                    && matches!(e.order, UnitOrder::Gather { .. })
                    && !state
                        .expansion
                        .as_ref()
                        .is_some_and(|p| p.scout == Some(e.id))
                    && self
                        .unit_type(e.unit_type)
                        .and_then(|u| u.worker.as_ref())
                        .is_some_and(|w| w.resource_kinds.contains(&kind))
            })
            .map(|e| (self.inside_structure(e), e.id))
            .collect();
        workers.sort_unstable();
        let mut assigned = 0;
        for (_, entity) in workers.into_iter().take(4) {
            let actor = &self.state.entities[self.index(entity).unwrap()];
            let mut nodes: Vec<_> = self
                .state
                .resources
                .iter()
                .filter(|r| {
                    r.kind == kind
                        && self.ai_gather_candidate(controller, actor, r)
                        && self.ai_mine_ready(controller.player, r)
                })
                .map(|r| (rts::distance(actor.position, r.position), r.id))
                .collect();
            nodes.sort_unstable();
            let ids: Vec<_> = nodes.into_iter().map(|(_, id)| id).collect();
            if let Some(resource) = self.ai_gather_choice(controller, state, actor, &ids)
                && self.ai_order(controller, state, Order::Gather { entity, resource })
            {
                assigned += 1;
                if assigned == 2 {
                    break;
                }
            }
        }
    }

    pub(super) fn ai_mine_limit(&self) -> Option<usize> {
        match self.map.multiplayer.as_ref()?.ai_difficulty? {
            AiDifficulty::Normal => Some(2),
            AiDifficulty::Hard => Some(usize::MAX),
            AiDifficulty::Easy => None,
        }
    }

    pub(super) fn ai_same_mine(&self, a: ResourceId, b: ResourceId) -> bool {
        if a == b {
            return true;
        }
        let (Some(a), Some(b)) = (self.collision_resource(a), self.collision_resource(b)) else {
            return false;
        };
        // Interior deposits (gold mines) are individual operations. Adjacent
        // outdoor patches (minerals) share one operation rather than using up
        // the difficulty's entire allowance on two patches at the same base.
        a.kind == b.kind
            && !self.rules.units.iter().any(|u| {
                u.harvest_profiles
                    .iter()
                    .any(|p| p.kind == a.kind && p.inside)
            })
            && rts::distance(a.position, b.position) <= 192_i64.pow(2)
    }

    pub(super) fn ai_mining_sites(
        &self,
        controller: &AiController,
        kind: &str,
    ) -> Vec<(ResourceId, usize)> {
        let mut sites: Vec<(ResourceId, usize)> = Vec::new();
        for actor in self
            .state
            .entities
            .iter()
            .filter(|e| e.owner == controller.player && e.hp > 0 && e.garrisoned_in.is_none())
        {
            let UnitOrder::Gather { resource } = actor.order else {
                continue;
            };
            if !self
                .collision_resource(resource)
                .is_some_and(|r| r.kind == kind && r.amount > 0 && !r.requires_extractor)
            {
                continue;
            }
            if let Some((_, count)) = sites
                .iter_mut()
                .find(|(id, _)| self.ai_same_mine(*id, resource))
            {
                *count += 1;
            } else {
                sites.push((resource, 1));
            }
        }
        sites
    }

    pub(super) fn ai_growth_needed(
        &self,
        controller: &AiController,
        state: &AiState,
        base: &UnitType,
    ) -> bool {
        let Some(limit) = self.ai_mine_limit() else {
            return false;
        };
        let sites = self.ai_mining_sites(controller, &base.dropoff[0]);
        // A remote operation still needs its depot even after filling the
        // active-site allowance. Depleted deposits disappear from this count.
        if sites.iter().any(|(id, _)| {
            self.collision_resource(*id)
                .is_some_and(|r| !self.ai_mine_served(controller.player, r))
        }) {
            return true;
        }
        let workers = self
            .state
            .entities
            .iter()
            .filter(|e| {
                e.owner == controller.player
                    && self.ai_in_town(controller, state, e)
                    && e.construction.is_none()
                    && self
                        .unit_type(e.unit_type)
                        .is_some_and(|u| u.worker.is_some() && u.builds.contains(&base.id))
            })
            .count();
        !sites.is_empty() && sites.len() < limit && workers >= (sites.len() * 3).max(4)
    }

    pub(super) fn ai_mine_ready(&self, player: PlayerId, node: &ResourceNode) -> bool {
        self.state.entities.iter().any(|e| {
            e.owner == player
                && e.hp > 0
                && e.construction.is_none()
                && !e.airborne
                && rts::distance(e.position, node.position) <= 512_i64.pow(2)
                && self
                    .unit_type(e.unit_type)
                    .is_some_and(|u| u.dropoff.contains(&node.kind))
        })
    }

    pub(super) fn ai_allowed_mines(
        &self,
        controller: &AiController,
        state: &AiState,
        actor: &Entity,
        candidates: &[ResourceId],
    ) -> Vec<ResourceId> {
        let policy = self.ai_mine_limit().and_then(|limit| {
            let base = self.unit_type(self.ai_economic_center(controller.player)?)?;
            Some((limit, &base.dropoff[0]))
        });
        let Some((limit, kind)) = policy else {
            return candidates.to_vec();
        };
        let mut sites = self.ai_mining_sites(controller, kind);
        if let Some(id) = state.expansion.as_ref().and_then(|p| p.resource)
            && self.collision_resource(id).is_some_and(|r| r.amount > 0)
            && !sites.iter().any(|(other, _)| self.ai_same_mine(*other, id))
        {
            sites.push((id, 0));
        }
        candidates
            .iter()
            .copied()
            .filter(|id| {
                self.collision_resource(*id).is_some_and(|r| {
                    r.kind != *kind
                        || r.requires_extractor
                        || (self.ai_mine_ready(actor.owner, r)
                            && (sites.len() < limit
                                || sites
                                    .iter()
                                    .any(|(other, _)| self.ai_same_mine(*other, *id))))
                })
            })
            .collect()
    }

    pub(super) fn ai_staff_mines(&mut self, controller: &AiController, state: &mut AiState) {
        if self.ai_mine_limit().is_none() || !self.tick().0.is_multiple_of(64) {
            return;
        }
        let Some(base) = self
            .ai_economic_center(controller.player)
            .and_then(|id| self.unit_type(id))
        else {
            return;
        };
        let mut sites = self.ai_mining_sites(controller, &base.dropoff[0]);
        sites.retain(|(id, _)| {
            self.collision_resource(*id)
                .is_some_and(|r| self.ai_mine_ready(controller.player, r))
        });
        sites.sort_by_key(|(id, count)| (*count, *id));
        let (Some(&(target, low)), Some(&(source, high))) = (sites.first(), sites.last()) else {
            return;
        };
        if high <= low + 1 {
            return;
        }
        let mut workers: Vec<_> = self.state.entities.iter().filter(|e| {
            e.owner == controller.player && e.cargo.is_none() && e.auto_attack_target.is_none()
                && e.queued_orders.is_empty() && self.ai_in_town(controller, state, e)
                && !state.expansion.as_ref().is_some_and(|p| p.scout == Some(e.id))
                && matches!(e.order, UnitOrder::Gather { resource } if self.ai_same_mine(source, resource))
        }).map(|e| (self.inside_structure(e), e.id)).collect();
        workers.sort_unstable();
        for (_, entity) in workers.into_iter().take(4) {
            let actor = &self.state.entities[self.index(entity).unwrap()];
            if self
                .ai_gather_choice(controller, state, actor, &[target])
                .is_some()
                && self.ai_order(
                    controller,
                    state,
                    Order::Gather {
                        entity,
                        resource: target,
                    },
                )
            {
                break;
            }
        }
    }

    pub(super) fn ai_hire_miners(
        &mut self,
        controller: &AiController,
        state: &mut AiState,
        reserved: &BTreeMap<String, u64>,
    ) {
        if self.ai_mine_limit().is_none() || !self.tick().0.is_multiple_of(64) {
            return;
        }
        let Some(base) = self
            .ai_economic_center(controller.player)
            .and_then(|id| self.unit_type(id))
        else {
            return;
        };
        let sites = self.ai_mining_sites(controller, &base.dropoff[0]);
        let pending = state
            .expansion
            .as_ref()
            .and_then(|p| p.resource)
            .is_some_and(|id| !sites.iter().any(|(other, _)| self.ai_same_mine(*other, id)));
        let target = (sites.len() + usize::from(pending)) * 5 + 4;
        let worker_types: Vec<_> = self
            .rules
            .units
            .iter()
            .filter(|u| u.worker.is_some() && u.builds.contains(&base.id))
            .map(|u| u.id)
            .collect();
        let mut count = self
            .state
            .entities
            .iter()
            .filter(|e| e.owner == controller.player)
            .map(|e| {
                usize::from(worker_types.contains(&e.unit_type))
                    + e.production
                        .iter()
                        .filter(|p| worker_types.contains(&p.unit_type))
                        .count()
            })
            .sum::<usize>();
        // Ordinary production requests spend first. Scale the economy with
        // surplus funds, without turning every new miner into a higher-priority
        // purchase than the military or bypassing supply/production validation.
        let producers: Vec<_> = self
            .state
            .entities
            .iter()
            .filter(|e| {
                e.owner == controller.player
                    && e.construction.is_none()
                    && e.production.is_empty()
                    && !e.airborne
                    && e.research.is_none()
                    && self.ai_in_town(controller, state, e)
            })
            .filter_map(|e| {
                worker_types
                    .iter()
                    .find(|&&id| self.can_train_type(e, id))
                    .map(|&id| (e.id, id))
            })
            .collect();
        for (entity, unit_type) in producers {
            if count >= target {
                break;
            }
            let unit = self.unit_type(unit_type).unwrap();
            if unit.cost.iter().all(|c| {
                self.resource_balance(controller.player, &c.kind)
                    >= u64::from(c.amount) + reserved.get(&c.kind).copied().unwrap_or(0)
            }) && self
                .ai_expansion_reserve(controller, state)
                .iter()
                .all(|c| {
                    let extra = unit
                        .cost
                        .iter()
                        .find(|cost| cost.kind == c.kind)
                        .map_or(0, |cost| cost.amount);
                    self.resource_balance(controller.player, &c.kind)
                        >= u64::from(c.amount) + u64::from(extra)
                })
                && self.ai_order(controller, state, Order::Train { entity, unit_type })
            {
                count += 1
            }
        }
    }
}
