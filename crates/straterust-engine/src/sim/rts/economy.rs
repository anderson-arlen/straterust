use super::*;

impl World {
    /// Interior workers share entrances without occupying them. Choose a free
    /// place on emergence so another entrant cannot trap them or expose a stack.
    pub(super) fn leave_interior(
        &mut self,
        index: usize,
        position: Position,
        footprint: Footprint,
    ) -> bool {
        self.leave_interior_toward(index, position, footprint, None)
    }
    fn leave_interior_toward(
        &mut self,
        index: usize,
        position: Position,
        footprint: Footprint,
        destination: Option<(Position, Footprint)>,
    ) -> bool {
        let actor = &self.state.entities[index];
        let unit = self.unit_at(index);
        let toward = destination.map_or(actor.position, |(position, _)| position);
        let goals = destination
            .map(|(position, footprint)| perimeter(position, footprint, unit.footprint, toward));
        let mut candidates = perimeter(position, footprint, unit.footprint, toward);
        candidates.push(actor.position);
        // Exit toward the next destination, rather than back into an entrance
        // pocket that may be cut off from the depot by the structure itself.
        candidates.sort_by_key(|p| (distance(*p, toward), p.y, p.x));
        let exit = candidates.into_iter().find(|p| {
            self.can_place(*p, unit.footprint, unit.movement_class, Some(actor.id))
                && goals.as_ref().is_none_or(|goals| {
                    crate::path::find_path_to_any(
                        &self.map,
                        unit.footprint,
                        unit.movement_class,
                        *p,
                        goals,
                        &self.navigation_geometry,
                    )
                    .is_some()
                })
        });
        let Some(exit) = exit else { return false };
        self.state.entities[index].position = exit;
        self.state.entities[index].motion_fraction = [0, 0];
        self.state.entities[index].gathering_inside = false;
        self.state.entities[index].path.clear();
        self.state.entities[index].target = None;
        true
    }
    pub(in crate::sim) fn extractor(&self, player: PlayerId, node: &ResourceNode) -> Option<usize> {
        self.state.entities.iter().position(|entity| {
            entity.owner == player
                && entity.position == node.position
                && entity.construction.is_none()
                && self.unit_type(entity.unit_type).is_some_and(|unit| {
                    unit.extracts
                        .as_ref()
                        .is_some_and(|extraction| extraction.resource == node.kind)
                })
        })
    }
    pub fn gather_rejection(&self, worker: EntityId, resource: ResourceId) -> Option<Rejection> {
        let Some(index) = self.index(worker) else {
            return Some(Rejection::UnknownEntity);
        };
        self.action_rejection(index, &UnitOrder::Gather { resource })
    }
    pub(in crate::sim) fn retarget_gather(&mut self, index: usize, previous: &ResourceNode) {
        let actor = &self.state.entities[index];
        let unit = self.unit_at(index);
        let obstacles = self.obstacles(actor.id);
        let origin = actor.gather_origin.unwrap_or(previous.position);
        let radius = unit
            .worker
            .as_ref()
            .map_or(0, |worker| worker.idle_resource_radius);
        // The reference searches twelve tiles around the exhausted patch.
        // Only known resources participate; path probes must not reveal new nodes.
        let mut candidates: Vec<_> = self
            .state
            .resources
            .iter()
            .filter(|node| {
                node.id != previous.id
                    && node.kind == previous.kind
                    && (node.position.x - previous.position.x).abs() <= 384
                    && (node.position.y - previous.position.y).abs() <= 384
                    && (radius == 0 || distance(origin, node.position) <= i64::from(radius).pow(2))
                    && self.map.height_at(node.position) == self.map.height_at(actor.position)
                    && self.visibility(actor.owner, node.position) != Visibility::Unexplored
                    && self.gather_rejection(actor.id, node.id).is_none()
            })
            .collect();
        candidates.sort_by_key(|node| (distance(previous.position, node.position), node.id));
        let resource = candidates
            .into_iter()
            .find(|node| {
                let footprint = self
                    .extractor(actor.owner, node)
                    .map_or(node.footprint, |other| self.unit_at(other).footprint);
                in_range(actor.position, unit.footprint, node.position, footprint, 1)
                    || crate::path::find_path_to_any(
                        &self.map,
                        unit.footprint,
                        unit.movement_class,
                        actor.position,
                        &perimeter(node.position, footprint, unit.footprint, actor.position),
                        &obstacles,
                    )
                    .is_some()
            })
            .map(|node| node.id);
        if let Some(resource) = resource {
            let origin = self.state.entities[index].gather_origin;
            self.assign(index, UnitOrder::Gather { resource }, false);
            self.state.entities[index].gather_origin = origin;
        } else if previous.amount == 0 {
            self.finish(index);
        }
    }
    pub(in crate::sim) fn gather(&mut self, index: usize, mut resource: ResourceId) {
        let actor = &self.state.entities[index];
        if actor.cargo.is_none()
            && actor.harvest_progress == 0
            && actor.harvest_spot.is_none()
            && self.state.tick >= actor.path_retry
        {
            resource = self.redistribute_gather(index, resource, true);
        }
        let Some(node_index) = self
            .state
            .resources
            .iter()
            .position(|node| node.id == resource)
        else {
            self.finish(index);
            return;
        };
        let node = self.state.resources[node_index].clone();
        let actor = self.state.entities[index].clone();
        let profile = self
            .unit_at(index)
            .harvest_profiles
            .iter()
            .find(|p| p.kind == node.kind)
            .cloned();
        let worker = self
            .unit_at(index)
            .worker
            .as_ref()
            .expect("validated gather order")
            .clone();
        let entry_range = profile.as_ref().map_or(1, |p| p.entry_range);
        let returning = actor.cargo.as_ref().is_some_and(|cargo| {
            cargo.amount >= worker.capacity || cargo.kind != node.kind || node.amount == 0
        });
        if returning {
            self.state.entities[index].harvest_spot = None;
            let cargo = actor.cargo.expect("returning cargo");
            let mut dropoffs: Vec<_> = self
                .state
                .entities
                .iter()
                .enumerate()
                .filter(|(_, entity)| {
                    entity.owner == actor.owner && entity.construction.is_none() && !entity.airborne
                })
                .filter(|(other, _)| self.unit_at(*other).dropoff.contains(&cargo.kind))
                .map(|(other, entity)| (entity.id, entity.position, self.unit_at(other).footprint))
                .collect();
            dropoffs.sort_by_key(|(id, position, _)| (distance(actor.position, *position), *id));
            let mut current = actor
                .dropoff_target
                .and_then(|id| dropoffs.iter().copied().find(|(other, _, _)| *other == id));
            let mut arrived = false;
            if let Some((_, position, footprint)) = current {
                let retry_due = self.state.tick >= actor.path_retry;
                arrived = actor.gathering_inside
                    || self.approach(index, position, footprint, entry_range);
                if !arrived
                    && retry_due
                    && self.state.entities[index].path.is_empty()
                    && !in_range(
                        self.state.entities[index].position,
                        self.unit_at(index).footprint,
                        position,
                        footprint,
                        entry_range,
                    )
                {
                    self.state.entities[index].dropoff_target = None;
                }
            } else {
                self.state.entities[index].dropoff_target = None;
                if self.state.tick >= actor.path_retry {
                    for (id, position, footprint) in dropoffs {
                        self.state.entities[index].path_retry = self.state.tick;
                        self.state.entities[index].target = None;
                        self.state.entities[index].path.clear();
                        arrived = self.approach(index, position, footprint, entry_range);
                        if arrived
                            || !self.state.entities[index].path.is_empty()
                            || in_range(
                                self.state.entities[index].position,
                                self.unit_at(index).footprint,
                                position,
                                footprint,
                                entry_range,
                            )
                        {
                            self.state.entities[index].dropoff_target = Some(id);
                            current = Some((id, position, footprint));
                            break;
                        }
                    }
                }
            }
            if arrived {
                if let Some(profile) = &profile
                    && profile.depot_ticks > 0
                {
                    let delivery = &mut self.state.entities[index];
                    delivery.gathering_inside = profile.depot_inside;
                    delivery.harvest_progress = delivery.harvest_progress.saturating_add(1);
                    if delivery.harvest_progress < profile.depot_ticks {
                        return;
                    }
                    if profile.depot_inside {
                        let Some((_, position, footprint)) = current else {
                            return;
                        };
                        if !self.leave_interior_toward(
                            index,
                            position,
                            footprint,
                            Some((node.position, node.footprint)),
                        ) {
                            return;
                        }
                    }
                }
                let bonus = self
                    .state
                    .entities
                    .iter()
                    .filter(|e| {
                        e.owner == actor.owner
                            && e.hp > 0
                            && e.construction.is_none()
                            && !e.airborne
                    })
                    .filter_map(|e| {
                        self.unit_type(e.unit_type)
                            .unwrap()
                            .harvest_bonus_percent
                            .iter()
                            .find(|a| a.kind == cargo.kind)
                    })
                    .map(|a| a.amount)
                    .max()
                    .unwrap_or(0);
                let deposited = u64::from(cargo.amount) * u64::from(100 + bonus) / 100;
                *self.state.statistics[usize::from(actor.owner.0)]
                    .resources_collected
                    .entry(cargo.kind.clone())
                    .or_default() += deposited;
                *self.state.players[usize::from(actor.owner.0)]
                    .resources
                    .entry(cargo.kind)
                    .or_default() += deposited;
                self.state.entities[index].cargo = None;
                self.state.entities[index].gathering_inside = false;
                self.state.entities[index].dropoff_target = None;
                self.state.entities[index].harvest_progress = 0;
                if node.amount == 0 && !node.requires_extractor {
                    self.retarget_gather(index, &node);
                }
                if let UnitOrder::Gather { resource } = self.state.entities[index].order {
                    self.redistribute_gather(index, resource, false);
                }
            }
            return;
        }
        if node.amount == 0 && !node.requires_extractor {
            if actor.gathering_inside && !self.leave_interior(index, node.position, node.footprint)
            {
                return;
            }
            self.retarget_gather(index, &node);
            return;
        }
        let extraction = if node.requires_extractor {
            let Some(other) = self.extractor(actor.owner, &node) else {
                self.finish(index);
                return;
            };
            self.unit_at(other).extracts.clone()
        } else {
            None
        };
        let footprint = self
            .extractor(actor.owner, &node)
            .map_or(node.footprint, |other| self.unit_at(other).footprint);
        let inside = extraction.is_some() || profile.as_ref().is_some_and(|p| p.inside);
        if inside {
            // Interior entry does not reserve an outdoor harvesting position.
            // This also releases reservations inherited from older saves.
            self.state.entities[index].harvest_spot = None;
        }
        let arrived = actor.gathering_inside
            || if inside {
                self.approach(index, node.position, footprint, entry_range)
            } else {
                self.approach_resource(index, &node)
            };
        if !arrived {
            // Only reconsider after an actual failed route, not while walking
            // or waiting for a busy harvest slot. Keep gather intent if every
            // nearby patch is temporarily blocked.
            if !node.requires_extractor
                && self.state.tick >= actor.path_retry
                && self.state.entities[index].path.is_empty()
            {
                self.retarget_gather(index, &node);
            }
            return;
        }
        let since = *self.state.entities[index]
            .harvest_waiting_since
            .get_or_insert(self.state.tick);
        let active = self
            .state
            .entities
            .iter()
            .filter(|entity| {
                entity.id != actor.id
                    && entity.dropoff_target.is_none()
                    && entity.order == (UnitOrder::Gather { resource })
                    && (entity.harvest_progress > 0 || entity.gathering_inside)
            })
            .count();
        if actor.harvest_progress == 0
            && !actor.gathering_inside
            && (active >= profile.as_ref().map_or(1, |p| usize::from(p.capacity))
                || self.state.entities.iter().any(|entity| {
                    entity.id != actor.id
                        && entity.order == (UnitOrder::Gather { resource })
                        && entity.harvest_progress == 0
                        && !entity.gathering_inside
                        && entity
                            .harvest_waiting_since
                            .is_some_and(|tick| (tick, entity.id) < (since, actor.id))
                }))
        {
            if !node.requires_extractor && self.state.tick >= self.state.entities[index].path_retry
            {
                self.state.entities[index].path_retry =
                    Tick(self.state.tick.0.saturating_add(PATH_RETRY_TICKS));
                self.redistribute_gather(index, resource, false);
            }
            return;
        }
        if inside {
            self.state.entities[index].gathering_inside = true;
            self.state.entities[index].harvest_spot = None;
        }
        self.state.entities[index].harvest_progress += 1;
        if self.state.entities[index].harvest_progress
            >= extraction.as_ref().map_or(
                profile.as_ref().map_or(worker.harvest_ticks, |p| p.ticks),
                |extraction| extraction.harvest_ticks,
            )
        {
            if inside {
                let destination = self
                    .state
                    .entities
                    .iter()
                    .filter(|e| {
                        e.owner == actor.owner
                            && e.construction.is_none()
                            && !e.airborne
                            && self
                                .unit_type(e.unit_type)
                                .unwrap()
                                .dropoff
                                .contains(&node.kind)
                    })
                    .min_by_key(|e| (distance(actor.position, e.position), e.id))
                    .map(|e| (e.position, self.unit_type(e.unit_type).unwrap().footprint));
                if !self.leave_interior_toward(index, node.position, footprint, destination) {
                    return;
                }
            }
            self.state.entities[index].gathering_inside = false;
            self.state.entities[index].harvest_progress = 0;
            self.state.entities[index].harvest_waiting_since = None;
            let held = self.state.entities[index]
                .cargo
                .as_ref()
                .map_or(0, |cargo| cargo.amount);
            let amount = if let Some(extraction) = &extraction {
                if self.state.resources[node_index].amount < worker.harvest_amount {
                    extraction.depleted_amount.min(worker.capacity - held)
                } else {
                    worker.harvest_amount.min(worker.capacity - held)
                }
            } else {
                worker
                    .harvest_amount
                    .min(worker.capacity - held)
                    .min(self.state.resources[node_index].amount)
            };
            let amount = if extraction.is_none() {
                profile.as_ref().map_or(amount, |p| {
                    p.amount
                        .min(worker.capacity - held)
                        .min(self.state.resources[node_index].amount)
                })
            } else {
                amount
            };
            if extraction.is_some()
                && self.state.resources[node_index].amount < worker.harvest_amount
            {
                self.state.resources[node_index].amount = 0;
            } else {
                self.state.resources[node_index].amount = self.state.resources[node_index]
                    .amount
                    .saturating_sub(amount);
            }
            self.state.entities[index].cargo = Some(ResourceAmount {
                kind: node.kind,
                amount: held + amount,
            });
            if held + amount >= worker.capacity {
                self.state.entities[index].harvest_spot = None;
            }
            if self.state.resources[node_index].amount == 0
                && self
                    .map
                    .resources
                    .get(node_index)
                    .is_some_and(|r| r.terrain_corners.is_some())
            {
                self.reconnect_resource_terrain();
            }
        }
    }
    pub(in crate::sim) fn repair(&mut self, index: usize, target: EntityId) {
        let actor = &self.state.entities[index];
        if self.repair_rejection(actor.id, target).is_some() {
            self.finish(index);
            return;
        }
        let owner = actor.owner;
        let other = self.index(target).expect("validated repair target");
        let position = self.state.entities[other].position;
        let unit = self.unit_at(other).clone();
        let repair = self
            .rules
            .repair
            .as_ref()
            .expect("validated repair rules")
            .clone();
        if !self.approach(index, position, unit.footprint, repair.range) {
            return;
        }
        if self.state.entities[other].construction.is_some() {
            let rate = self
                .unit_at(index)
                .worker
                .as_ref()
                .map_or(1, |w| w.build_rate);
            self.state.entities[index].repair_progress += u64::from(rate);
            if self.progress_construction(other, rate) {
                self.finish(index);
            }
            return;
        }
        let time = u64::from(unit.build_ticks) * u64::from(repair.rate_denominator);
        let progress = self.state.entities[index].repair_progress
            + u64::from(unit.max_hp) * u64::from(repair.rate_numerator);
        let missing = unit.max_hp - self.state.entities[other].hp
            + u32::from(self.state.entities[other].damage_fraction != 0);
        let healed = (progress / time).min(u64::from(missing));
        let denominator = u64::from(unit.max_hp) * u64::from(repair.cost_divisor);
        // Check the next HP's price even during fractional work. An unfunded
        // order waits without accumulating work or emitting working effects.
        let prices: Vec<_> = unit
            .cost
            .iter()
            .enumerate()
            .map(|(i, cost)| {
                let credit = self.state.entities[other]
                    .repair_credit
                    .get(i)
                    .copied()
                    .unwrap_or(0);
                (healed.max(1) * u64::from(cost.amount))
                    .saturating_sub(credit)
                    .div_ceil(denominator)
            })
            .collect();
        if unit
            .cost
            .iter()
            .zip(&prices)
            .any(|(cost, price)| self.resource_balance(owner, &cost.kind) < *price)
        {
            return;
        }
        self.state.entities[index].repair_progress = progress % time;
        if healed == 0 {
            return;
        }
        self.state.entities[other]
            .repair_credit
            .resize(unit.cost.len(), 0);
        for (i, (cost, price)) in unit.cost.iter().zip(prices).enumerate() {
            if price != 0 {
                *self.state.players[usize::from(owner.0)]
                    .resources
                    .entry(cost.kind.clone())
                    .or_default() -= price;
            }
            self.state.entities[other].repair_credit[i] += price * denominator;
            self.state.entities[other].repair_credit[i] -= healed * u64::from(cost.amount);
        }
        self.state.entities[other].hp =
            (self.state.entities[other].hp + healed as u32).min(unit.max_hp);
        if self.state.entities[other].hp == unit.max_hp {
            self.state.entities[other].damage_fraction = 0;
        }
        if self.state.entities[other].hp == unit.max_hp {
            self.finish(index);
        }
    }

    pub(in crate::sim) fn construct(&mut self, index: usize, building: EntityId) {
        let Some(other) = self.index(building) else {
            self.finish(index);
            return;
        };
        let target = self.state.entities[other].clone();
        let Some(progress) = target.construction else {
            self.finish(index);
            return;
        };
        if progress.worker != Some(self.state.entities[index].id) {
            self.finish(index);
            return;
        }
        let unit = self.unit_at(other).clone();
        if unit.addon_parent.is_none() && progress.work_position.is_none() {
            if !self.approach(index, target.position, unit.footprint, 1) {
                return;
            }
            let position = self.state.entities[index].position;
            self.state.entities[other]
                .construction
                .as_mut()
                .expect("construction")
                .work_position = Some(position);
        }
        if unit.addon_parent.is_none() {
            if unit.autonomous_construction {
                self.state.entities[other]
                    .construction
                    .as_mut()
                    .unwrap()
                    .worker = None;
                self.finish(index);
                return;
            }
            if unit.consumes_builder {
                let id = self.state.entities[index].id;
                let mut transformed = self.state.entities[other].clone();
                transformed.id = id;
                transformed.construction.as_mut().unwrap().worker = None;
                self.state.entities[index] = transformed;
                self.state.entities.remove(other);
                return;
            }
            if !unit.builder_inside {
                self.reposition_builder(index, other);
            }
        }
        let rate = self
            .unit_at(index)
            .worker
            .as_ref()
            .map_or(1, |worker| worker.build_rate);
        if self.progress_construction(other, rate)
            && self.state.entities[index].order == (UnitOrder::Build { building })
        {
            self.finish(index);
        }
    }
    pub(in crate::sim) fn progress_construction(&mut self, other: usize, rate: u32) -> bool {
        let unit = self.unit_at(other).clone();
        let progress = self.state.entities[other].construction.clone().unwrap();
        let remaining = progress.remaining.saturating_sub(rate);
        if remaining == 0
            && unit.builder_inside
            && let Some(worker) = progress.worker.and_then(|id| self.index(id))
            && !self.leave_interior(worker, self.state.entities[other].position, unit.footprint)
        {
            return false;
        }
        // Add only the HP earned this tick, preserving damage suffered during construction.
        let before = u64::from(unit.max_hp - 1) * u64::from(progress.total - progress.remaining)
            / u64::from(progress.total);
        let after = u64::from(unit.max_hp - 1) * u64::from(progress.total - remaining)
            / u64::from(progress.total);
        self.state.entities[other].hp = (u64::from(self.state.entities[other].hp) + after - before)
            .min(u64::from(unit.max_hp)) as u32;
        if remaining == 0 {
            self.record_created(self.state.entities[other].owner, unit.id);
            self.state.entities[other].construction = None;
            self.state.entities[other].energy = unit.initial_energy();
            self.state.entities[other].shields = unit.max_shields * 256;
            if unit.builder_gathers_resource
                && let Some(worker) = progress.worker
            {
                self.gather_completed_extractor(other, worker);
            }
        } else {
            self.state.entities[other]
                .construction
                .as_mut()
                .expect("construction")
                .remaining = remaining;
        }
        remaining == 0
    }
    pub(in crate::sim) fn reposition_builder(&mut self, index: usize, building: usize) {
        // OpenBW's ConstructingBuilding order alternates random work positions
        // with 30..=93 frame pauses, and builds during travel. The supplied 1.00
        // executable confirms the timer at 0x415396..0x4153ae. Mapping frames
        // to engine ticks is provisional. We retain normal collision outside
        // the building; the source's collision override is not implemented.
        let progress = self.state.entities[building]
            .construction
            .as_ref()
            .expect("construction");
        let target = progress.work_position.expect("work started");
        if self.state.entities[index].position != target {
            if self.navigate(index, target, false) {
                let ticks = 30 + (splitmix64(&mut self.state.rng_state) & 63) as u32;
                self.state.entities[building]
                    .construction
                    .as_mut()
                    .expect("construction")
                    .work_ticks = ticks;
            } else if self.state.entities[index].path.is_empty() {
                // A moving obstacle may invalidate a route. Work from the
                // current safe position and retry later, without a busy search.
                let position = self.state.entities[index].position;
                self.state.entities[index].target = None;
                let progress = self.state.entities[building]
                    .construction
                    .as_mut()
                    .expect("construction");
                progress.work_position = Some(position);
                progress.work_ticks = 30;
            }
            return;
        }
        if progress.work_ticks > 0 {
            self.state.entities[building]
                .construction
                .as_mut()
                .expect("construction")
                .work_ticks -= 1;
            return;
        }
        let actor = &self.state.entities[index];
        let unit = self.unit_at(index);
        let start = actor.position;
        let id = actor.id;
        let footprint = unit.footprint;
        let class = unit.movement_class;
        let mut candidates = perimeter(
            self.state.entities[building].position,
            self.unit_at(building).footprint,
            footprint,
            start,
        );
        candidates.retain(|point| {
            distance(*point, start) >= 64 && self.can_place(*point, footprint, class, Some(id))
        });
        let mut destination = start;
        if !candidates.is_empty() {
            let offset = (splitmix64(&mut self.state.rng_state) % candidates.len() as u64) as usize;
            let obstacles = self.obstacles(id);
            // Bound expensive searches even when terrain partitions the work
            // perimeter. An inaccessible point must never stall construction.
            for attempt in 0..candidates.len().min(4) {
                let point = candidates[(offset + attempt) % candidates.len()];
                if let Some(path) = find_path(&self.map, footprint, class, start, point, &obstacles)
                {
                    destination = point;
                    let actor = &mut self.state.entities[index];
                    actor.target = Some(point);
                    actor.path = path.into();
                    actor.path_retry = Tick(self.state.tick.0.saturating_add(PATH_RETRY_TICKS));
                    break;
                }
            }
        }
        let progress = self.state.entities[building]
            .construction
            .as_mut()
            .expect("construction");
        progress.work_position = Some(destination);
        progress.work_ticks = if destination == start { 30 } else { 0 };
        if destination != start && self.navigate(index, destination, false) {
            let ticks = 30 + (splitmix64(&mut self.state.rng_state) & 63) as u32;
            self.state.entities[building]
                .construction
                .as_mut()
                .expect("construction")
                .work_ticks = ticks;
        }
    }
    /// A body changing in place has no producer exit. Mobile traffic can
    /// overlap the emerging body; terrain, structures and resources still block.
    fn morph_position_clear(&self, position: Position, unit: &UnitType, actor: EntityId) -> bool {
        if unit.structure {
            return self.can_place(position, unit.footprint, unit.movement_class, Some(actor));
        }
        self.map
            .can_move(position, unit.footprint, unit.movement_class)
            && !self.state.entities.iter().any(|other| {
                let definition = self.unit_type(other.unit_type).unwrap();
                other.id != actor
                    && definition.structure
                    && definition.blocks_movement
                    && !other.airborne
                    && self.movement_class(other) == unit.movement_class
                    && overlaps(
                        position,
                        unit.footprint,
                        other.position,
                        definition.footprint,
                    )
            })
            && (unit.movement_class != MovementClass::Ground
                || !self.state.resources.iter().any(|resource| {
                    self.resource_blocks_movement(resource)
                        && overlaps(
                            position,
                            unit.footprint,
                            resource.position,
                            resource.footprint,
                        )
                }))
    }

    pub(in crate::sim) fn produce(&mut self, index: usize) {
        if self.state.entities[index].airborne || self.state.entities[index].flight_transition != 0
        {
            return;
        }
        if self.state.entities[index].construction.is_some() {
            return;
        }
        if !self.powered(&self.state.entities[index]) {
            return;
        }
        let Some(job) = self.state.entities[index].production.front().cloned() else {
            return;
        };
        let unit = self
            .unit_type(self.researched_unit_type(self.state.entities[index].owner, job.unit_type))
            .expect("validated production")
            .clone();
        let actor = self.state.entities[index].clone();
        if !job.started {
            let (used, provided) = self.supply(actor.owner);
            if !self.has_prerequisites(actor.owner, &unit)
                || used
                    + (unit.supply_used * u32::from(unit.production_count)).saturating_sub(
                        if job.producer_type.is_some() {
                            self.unit_at(index).supply_used
                        } else {
                            0
                        },
                    )
                    > provided
            {
                return;
            }
            self.state.entities[index]
                .production
                .front_mut()
                .expect("job")
                .started = true;
            if let Some(original) = job.producer_type
                && let Some(form) = self.unit_type(original).unwrap().production_form
            {
                self.state.entities[index].unit_type = form;
                self.state.entities[index].hp = self.unit_type(form).unwrap().max_hp;
            }
        }
        let progress = self.state.entities[index]
            .production
            .front_mut()
            .expect("job");
        progress.remaining = progress.remaining.saturating_sub(1);
        if progress.remaining > 0
            || self.state.entities.len() >= MAX_ENTITIES
            || self.state.next_entity_id == u32::MAX
        {
            return;
        }
        if job.producer_type.is_some() {
            let position = if self.morph_position_clear(actor.position, &unit, actor.id) {
                actor.position
            } else {
                let Some(point) = perimeter(
                    actor.position,
                    self.unit_at(index).footprint,
                    unit.footprint,
                    actor.position,
                )
                .into_iter()
                .find(|p| self.morph_position_clear(*p, &unit, actor.id)) else {
                    return;
                };
                point
            };
            let extra = if unit.production_count > 1 {
                let candidate = perimeter(position, unit.footprint, unit.footprint, position)
                    .into_iter()
                    .find(|p| {
                        self.can_place(*p, unit.footprint, unit.movement_class, Some(actor.id))
                    });
                Some(candidate.unwrap_or(position))
            } else {
                None
            };
            self.state.entities[index].production.pop_front();
            let missing_hp = self.unit_at(index).max_hp.saturating_sub(actor.hp);
            self.state.entities[index].position = position;
            self.state.entities[index].unit_type = unit.id;
            self.state.entities[index].hp = unit.max_hp.saturating_sub(missing_hp).max(1);
            self.state.entities[index].shields = unit.max_shields * 256;
            self.state.entities[index].energy = unit.initial_energy();
            self.state.entities[index].order = actor
                .rally
                .map_or(UnitOrder::Idle, |target| UnitOrder::Move { target });
            self.state.entities[index].parent = None;
            self.state.entities[index].path.clear();
            self.state.entities[index].cooldown = 0;
            self.state.entities[index].strikes.clear();
            self.state.entities[index].auto_attack_target = None;
            self.record_created(actor.owner, unit.id);
            if let Some(extra) = extra
                && let Some(id) = self.spawn_offspring(actor.owner, unit.id, extra, None)
            {
                let second = self.index(id).unwrap();
                self.state.entities[second].order = self.state.entities[index].order.clone();
                self.state.entities[second].rally = actor.rally;
                self.ai_produced(actor.id, id);
            }
            self.ai_produced(actor.id, actor.id);
            if let Some(resource) = actor.rally_resource
                && unit.worker.is_some()
                && self.gather_rejection(actor.id, resource).is_none()
            {
                self.assign(index, UnitOrder::Gather { resource }, false);
            }
            return;
        }
        if self.production_capacity(&actor) > 0 {
            self.state.entities[index].production.pop_front();
            if let Some(id) =
                self.spawn_offspring(actor.owner, unit.id, actor.position, Some(actor.id))
            {
                let stored = self.index(id).unwrap();
                self.state.entities[stored].garrisoned_in = Some(actor.id);
            }
            return;
        }
        let reference = actor.rally.unwrap_or(Position {
            x: actor.position.x,
            y: actor.position.y + i32::from(self.unit_at(index).footprint.height),
        });
        let exit = perimeter(
            actor.position,
            self.unit_at(index).footprint,
            unit.footprint,
            reference,
        )
        .into_iter()
        .find(|position| self.can_place(*position, unit.footprint, unit.movement_class, None));
        let Some(position) = exit else { return };
        self.state.entities[index].production.pop_front();
        let id = EntityId(self.state.next_entity_id);
        self.state.next_entity_id += 1;
        let mut entity = Entity {
            id,
            owner: actor.owner,
            unit_type: unit.id,
            position,
            hp: unit.max_hp,
            shields: unit.max_shields * 256,
            energy: unit.initial_energy(),
            mine_count: unit
                .mine_layer
                .as_ref()
                .map_or(0, |layer| layer.initial_count),
            mine_state: unit.mine.as_ref().map(|mine| MineState {
                phase: MinePhase::Arming,
                remaining: mine.arm_ticks,
                target: None,
            }),
            ..Entity::default()
        };
        if let Some(target) = actor.rally {
            entity.order = UnitOrder::Move { target };
        }
        let produced = entity.id;
        let producer = self.state.entities[index].id;
        self.state.entities.push(entity);
        self.record_created(actor.owner, unit.id);
        self.ai_produced(producer, produced);
        let newborn = self.state.entities.len() - 1;
        if let Some(resource) = actor.rally_resource
            && let Some(node) = self
                .state
                .resources
                .iter()
                .find(|node| node.id == resource)
                .cloned()
            && unit
                .worker
                .as_ref()
                .is_some_and(|worker| worker.resource_kinds.contains(&node.kind))
        {
            if self.gather_rejection(id, resource).is_none() {
                self.assign(newborn, UnitOrder::Gather { resource }, false);
            } else if node.amount == 0 && !node.requires_extractor {
                self.retarget_gather(newborn, &node);
            }
        }
    }
}
