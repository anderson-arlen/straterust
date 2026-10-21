//! A town budgets workers by resource demand, rather than by node count.
use super::*;

impl World {
    pub(super) fn ai_build_site(
        &self,
        player: PlayerId,
        builders: &mut [EntityId],
        unit: &UnitType,
        positions: &[Position],
    ) -> Option<(EntityId, Position)> {
        let first = *positions.first()?;
        builders.sort_by_key(|id| {
            let actor = &self.state.entities[self.index(*id).unwrap()];
            (rts::distance(actor.position, first), *id)
        });
        // Plan against terrain/resources/buildings, reusing the prepared grid.
        // A passing unit is temporary traffic, not an inaccessible building site.
        for &entity in builders.iter() {
            let actor = &self.state.entities[self.index(entity).unwrap()];
            let worker = self.unit_type(actor.unit_type).unwrap();
            let targets: Vec<_> = positions
                .iter()
                .filter(|&&position| {
                    self.build_rejection(player, entity, unit.id, position)
                        .is_none()
                })
                .flat_map(|&position| {
                    rts::perimeter(position, unit.footprint, worker.footprint, actor.position)
                        .into_iter()
                        .map(move |at| (at, position))
                })
                .collect();
            let goals: Vec<_> = targets.iter().map(|(at, _)| *at).collect();
            // One failed search proves the whole start component; testing
            // every plot separately repeats that flood thousands of times.
            if let Some((at, _)) = self.navigator().find_path_to_any(
                worker.footprint,
                worker.movement_class,
                actor.position,
                &goals,
            ) && let Some((_, position)) = targets.iter().find(|(goal, _)| *goal == at)
            {
                return Some((entity, *position));
            }
        }
        None
    }

    pub(super) fn ai_build_positions(
        &self,
        controller: &AiController,
        unit: &UnitType,
    ) -> Vec<Position> {
        let shore =
            controller.takeover && unit.placement_surface == crate::map::PlacementSurface::Shore;
        let limit = if controller.takeover {
            (controller.radius / 32).clamp(16, if shore { 64 } else { 24 }) as i32
        } else {
            16
        };
        let mut positions = Vec::new();
        for ring in 2_i32..=limit {
            for dy in -ring..=ring {
                for dx in -ring..=ring {
                    if dx.abs() != ring && dy.abs() != ring {
                        continue;
                    }
                    let position = Position {
                        x: controller.home.x / 32 * 32 + dx * 32,
                        y: controller.home.y / 32 * 32 + dy * 32,
                    };
                    if !controller.takeover
                        || self
                            .map
                            .can_build_on(position, unit.placement, unit.placement_surface)
                    {
                        positions.push(position);
                    }
                }
            }
        }
        if shore {
            positions.sort_by_key(|p| (rts::distance(*p, controller.home), p.x, p.y));
        } else if let Some(primary) = unit.dropoff.first().filter(|_| {
            controller.takeover
                && unit
                    .trains
                    .iter()
                    .any(|id| self.unit_type(*id).is_some_and(|u| u.worker.is_some()))
        }) {
            // Put the opening depot toward its nearby mine. Building on the
            // opposite side of town forces every delivery through future lanes.
            if let Some(mine) = self
                .state
                .resources
                .iter()
                .filter(|r| {
                    r.kind == *primary
                        && r.amount > 0
                        && rts::distance(r.position, controller.home) <= 512_i64.pow(2)
                })
                .min_by_key(|r| (rts::distance(r.position, controller.home), r.id))
            {
                positions.sort_by_key(|p| {
                    (
                        rts::distance(*p, mine.position),
                        rts::distance(*p, controller.home),
                        p.x,
                        p.y,
                    )
                });
            }
        }
        positions
    }

    /// Leave room for two workers between buildings, including foundations that
    /// have only been ordered. Dense legal placement can seal an economic route.
    /// Clearing candidates may overlap harvestable forest, never mines or bases.
    pub(super) fn ai_building_space(
        &self,
        player: PlayerId,
        unit: &UnitType,
        position: Position,
        clearing: bool,
    ) -> bool {
        if unit.addon_parent.is_some() || unit.extracts.is_some() {
            return true;
        }
        // A forest edge needs one worker passage; opposing building entrances
        // need two navigation lanes. Do not reserve a whole courtyard per tree.
        let gap = self.ai_building_gap(player, unit);
        // Legal foundations can still choke off a mine or the working edge of
        // a forest. Leave the same worker passage beside resources as buildings.
        if self
            .state
            .resources
            .iter()
            .filter(|r| r.amount > 0)
            .any(|node| {
                if (i64::from(position.x) - i64::from(node.position.x)).abs() * 2
                    >= i64::from(unit.placement.width)
                        + i64::from(node.footprint.width)
                        + i64::from(gap) * 2
                    || (i64::from(position.y) - i64::from(node.position.y)).abs() * 2
                        >= i64::from(unit.placement.height)
                            + i64::from(node.footprint.height)
                            + i64::from(gap) * 2
                {
                    return false;
                }
                let harvestable = self.ai_clearable_resource(node);
                if clearing && harvestable {
                    return false;
                }
                let gap = if harvestable { gap / 2 } else { gap };
                (i64::from(position.x) - i64::from(node.position.x)).abs() * 2
                    < i64::from(unit.placement.width)
                        + i64::from(node.footprint.width)
                        + i64::from(gap) * 2
                    && (i64::from(position.y) - i64::from(node.position.y)).abs() * 2
                        < i64::from(unit.placement.height)
                            + i64::from(node.footprint.height)
                            + i64::from(gap) * 2
            })
        {
            return false;
        }
        self.state
            .entities
            .iter()
            .filter(|e| e.owner == player)
            .all(|actor| {
                let Some(other) = self.unit_type(actor.unit_type) else {
                    return true;
                };
                let planned = match actor.order {
                    UnitOrder::PlaceBuilding { unit_type, target } => {
                        self.unit_type(unit_type).map(|u| (u, target))
                    }
                    _ => None,
                };
                let placed =
                    (other.structure && !actor.airborne).then_some((other, actor.position));
                placed.into_iter().chain(planned).all(|(other, at)| {
                    (i64::from(position.x) - i64::from(at.x)).abs() * 2
                        >= i64::from(unit.placement.width)
                            + i64::from(other.placement.width)
                            + i64::from(gap) * 2
                        || (i64::from(position.y) - i64::from(at.y)).abs() * 2
                            >= i64::from(unit.placement.height)
                                + i64::from(other.placement.height)
                                + i64::from(gap) * 2
                })
            })
    }

    pub(super) fn ai_building_gap(&self, player: PlayerId, building: &UnitType) -> u16 {
        let worker_width = self
            .state
            .entities
            .iter()
            .filter(|e| e.owner == player)
            .filter_map(|e| self.unit_type(e.unit_type))
            .filter(|u| u.worker.is_some() && u.builds.contains(&building.id))
            .map(|u| u.footprint.width.max(u.footprint.height))
            .max()
            .unwrap_or(16);
        // Two complete grid lanes let opposing workers pass. Naval haulers do
        // not enlarge the access lanes of buildings constructed by land workers.
        let cell = self.map.terrain.as_ref().map_or(8, |t| t.cell_size) as u16;
        worker_width.saturating_mul(2).max(cell.saturating_mul(2))
    }

    pub(super) fn ai_balance_gathering(
        &mut self,
        controller: &AiController,
        state: &mut AiState,
        requests: &[AiRequest],
    ) {
        if !controller.takeover || !controller.harvest_weights.is_empty() {
            return;
        }
        let mut weights = BTreeMap::<String, u64>::new();
        for request in requests {
            let missing = request
                .count
                .saturating_sub(self.ai_requested_count(controller, state, request));
            if missing == 0 {
                continue;
            }
            if let Some(unit) = self.unit_type(request.unit_type) {
                for cost in &unit.cost {
                    if self.ai_can_gather_kind(controller.player, &cost.kind) {
                        *weights.entry(cost.kind.clone()).or_default() +=
                            u64::from(cost.amount) * u64::from(missing);
                    }
                }
            }
        }
        // Existing armies and stockpiled gold are already funded. Gathering
        // their full cost forever can starve the wood needed to leave a forest.
        weights.retain(|kind, value| {
            *value = value
                .saturating_sub(self.resource_balance(controller.player, kind))
                .isqrt();
            *value > 0
        });
        // Oil-only haulers cannot take over a peasant's gold/wood job. Balance
        // each set of resource capabilities independently, including both races.
        let mut groups = BTreeMap::<Vec<String>, BTreeSet<EntityId>>::new();
        for actor in self
            .state
            .entities
            .iter()
            .filter(|e| e.owner == controller.player && self.ai_in_town(controller, state, e))
        {
            if let Some(worker) = self
                .unit_type(actor.unit_type)
                .and_then(|u| u.worker.as_ref())
            {
                let mut kinds = worker.resource_kinds.clone();
                kinds.sort();
                kinds.dedup();
                groups.entry(kinds).or_default().insert(actor.id);
            }
        }
        for (kinds, workers) in groups {
            let weights = weights
                .iter()
                .filter(|(kind, _)| kinds.contains(kind))
                .map(|(kind, weight)| (kind.clone(), *weight))
                .collect();
            self.ai_balance_workers(controller, state, &weights, &workers);
        }
    }

    pub(super) fn ai_can_gather_kind(&self, player: PlayerId, kind: &str) -> bool {
        self.state.entities.iter().any(|actor| {
            actor.owner == player
                && self
                    .unit_type(actor.unit_type)
                    .and_then(|u| u.worker.as_ref())
                    .is_some_and(|worker| worker.resource_kinds.iter().any(|k| k == kind))
                && self.state.resources.iter().any(|r| {
                    r.kind == kind
                        && r.amount > 0
                        && self.gather_rejection(actor.id, r.id).is_none()
                })
        })
    }

    fn ai_balance_workers(
        &mut self,
        controller: &AiController,
        state: &mut AiState,
        weights: &BTreeMap<String, u64>,
        workers: &BTreeSet<EntityId>,
    ) {
        let mut assigned = BTreeMap::<String, u64>::new();
        for actor in self
            .state
            .entities
            .iter()
            .filter(|e| workers.contains(&e.id))
        {
            if let UnitOrder::Gather { resource } = actor.order
                && let Some(node) = self.state.resources.iter().find(|r| r.id == resource)
            {
                *assigned.entry(node.kind.clone()).or_default() += 1;
            }
        }
        let total: u64 = assigned.values().sum();
        let weight: u64 = weights.values().sum();
        if total < 2 || weight == 0 {
            return;
        }
        let under = weights
            .iter()
            .filter(|(kind, demand)| {
                assigned.get(*kind).copied().unwrap_or(0) * weight < **demand * total
            })
            .min_by_key(|(kind, demand)| {
                assigned.get(*kind).copied().unwrap_or(0) * 65536 / **demand
            })
            .map(|(kind, _)| kind.clone());
        let Some(under) = under else {
            return;
        };
        let candidates: Vec<_> = self
            .state
            .entities
            .iter()
            .filter(|e| {
                workers.contains(&e.id)
                    && e.cargo.is_none()
                    && !e.gathering_inside
                    && self.ai_in_town(controller, state, e)
            })
            .filter_map(|actor| {
                let UnitOrder::Gather { resource } = actor.order else {
                    return None;
                };
                let node = self.state.resources.iter().find(|r| r.id == resource)?;
                let count = assigned.get(&node.kind).copied().unwrap_or(0);
                let desired = weights.get(&node.kind).copied().unwrap_or(0) * total;
                (node.kind != under && count > 1 && (count - 1) * weight >= desired)
                    .then_some(actor.id)
            })
            .collect();
        for entity in candidates {
            let position = self.state.entities[self.index(entity).unwrap()].position;
            let resource = self
                .state
                .resources
                .iter()
                .filter(|r| {
                    r.kind == under && r.amount > 0 && self.gather_rejection(entity, r.id).is_none()
                })
                .min_by_key(|r| (rts::distance(position, r.position), r.id))
                .map(|r| r.id);
            if let Some(resource) = resource
                && self.ai_order(controller, state, Order::Gather { entity, resource })
            {
                break;
            }
        }
    }
}
