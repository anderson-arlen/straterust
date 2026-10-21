//! Town production, opening budget and gathering allocation.
use super::*;

impl World {
    pub(super) fn ai_economy(&mut self, controller: &AiController, state: &mut AiState) {
        if controller.takeover {
            self.refresh_navigation_geometry();
        }
        let radius = i64::from(controller.radius).pow(2);
        // An unpaid construction order must not reserve its budget forever
        // behind mobile traffic. Return that worker to the economy and choose
        // another reachable builder/site through ordinary orders.
        let stalled: BTreeSet<_> = self
            .state
            .entities
            .iter()
            .filter(|e| {
                controller.takeover
                    && e.owner == controller.player
                    && matches!(e.order, UnitOrder::PlaceBuilding { .. })
                    && e.route_wait.as_ref().is_some_and(|w| {
                        self.tick().0.saturating_sub(w.since.0) * u64::from(self.rules.tick_ms)
                            >= 3000
                    })
            })
            .map(|e| e.id)
            .collect();
        for &entity in &stalled {
            self.ai_order(controller, state, Order::Stop { entity });
        }
        if let Some(target) = self.ai_target(controller) {
            let idle: Vec<_> = self
                .state
                .entities
                .iter()
                .filter(|e| {
                    state.deployed.contains(&e.id)
                        && e.garrisoned_in.is_none()
                        && !state.transports.contains_key(&e.id)
                        && matches!(e.order, UnitOrder::Idle)
                        && rts::distance(e.position, target) > 32_i64.pow(2)
                })
                .map(|e| e.id)
                .collect();
            for entity in idle {
                self.ai_order(controller, state, Order::AttackMove { entity, target });
            }
        }
        // Resume construction if its original worker died. Do this before
        // gathering so a replacement worker does not leave a paid shell idle.
        let abandoned: Vec<_> = self
            .state
            .entities
            .iter()
            .filter(|e| {
                e.owner == controller.player
                    && e.construction.as_ref().is_some_and(|c| c.worker.is_none())
                    && !self
                        .unit_type(e.unit_type)
                        .is_some_and(|u| u.consumes_builder)
                    && self.ai_in_town(controller, state, e)
            })
            .map(|e| (e.id, e.unit_type))
            .collect();
        for (building, unit_type) in abandoned {
            let worker = self
                .state
                .entities
                .iter()
                .find(|e| {
                    e.owner == controller.player
                        && e.construction.is_none()
                        && !self.inside_structure(e)
                        && !matches!(
                            e.order,
                            UnitOrder::Build { .. } | UnitOrder::PlaceBuilding { .. }
                        )
                        && self
                            .unit_type(e.unit_type)
                            .is_some_and(|u| u.builds.contains(&unit_type))
                })
                .map(|e| e.id);
            if let Some(entity) = worker {
                self.ai_order(controller, state, Order::Resume { entity, building });
            }
        }
        let workers: Vec<_> = self
            .state
            .entities
            .iter()
            .filter(|e| {
                e.owner == controller.player
                    && e.construction.is_none()
                    && matches!(e.order, UnitOrder::Idle)
                    && e.auto_attack_target.is_none()
                    && self.ai_in_town(controller, state, e)
                    && self
                        .unit_type(e.unit_type)
                        .is_some_and(|u| u.worker.is_some())
            })
            .map(|e| e.id)
            .collect();
        let kinds: BTreeMap<_, _> = self
            .state
            .resources
            .iter()
            .map(|r| (r.id, r.kind.as_str()))
            .collect();
        let mut assigned_nodes: BTreeMap<ResourceId, u32> = BTreeMap::new();
        let mut assigned_kinds: BTreeMap<String, u32> = BTreeMap::new();
        for e in &self.state.entities {
            if e.owner == controller.player
                && let UnitOrder::Gather { resource } = e.order
            {
                *assigned_nodes.entry(resource).or_default() += 1;
                if let Some(kind) = kinds.get(&resource) {
                    *assigned_kinds.entry((*kind).into()).or_default() += 1;
                }
            }
        }
        drop(kinds);
        for entity in workers {
            let worker = self.state.entities.iter().find(|e| e.id == entity).unwrap();
            let resource = self
                .state
                .resources
                .iter()
                .filter(|r| {
                    r.amount != 0
                        && rts::distance(r.position, controller.home) <= radius
                        && self.gather_rejection(entity, r.id).is_none()
                })
                .min_by_key(|r| {
                    let weight = controller.harvest_weights.iter().find(|a| a.kind == r.kind);
                    let allocation = weight.map_or(0, |a| {
                        assigned_kinds.get(&r.kind).copied().unwrap_or(0) * 65536 / a.amount
                    });
                    (
                        allocation,
                        assigned_nodes.get(&r.id).copied().unwrap_or(0),
                        rts::distance(worker.position, r.position),
                        r.id,
                    )
                })
                .map(|r| (r.id, r.kind.clone()));
            if let Some((resource, kind)) = resource
                && self.ai_order(controller, state, Order::Gather { entity, resource })
            {
                *assigned_nodes.entry(resource).or_default() += 1;
                *assigned_kinds.entry(kind).or_default() += 1;
            }
        }
        let mut requests = state.requests.clone();
        for (&unit_type, &count) in &state.defense {
            if let Some(request) = requests.iter_mut().find(|r| r.unit_type == unit_type) {
                request.count = request.count.max(count);
            } else {
                requests.push(AiRequest {
                    unit_type,
                    count,
                    priority: 70,
                    include_deployed: false,
                });
            }
        }
        if state.prepared {
            for (&unit_type, &wave_count) in &state.attack {
                let count =
                    wave_count.saturating_add(state.defense.get(&unit_type).copied().unwrap_or(0));
                if let Some(request) = requests.iter_mut().find(|r| r.unit_type == unit_type) {
                    request.count = request.count.max(count);
                } else {
                    requests.push(AiRequest {
                        unit_type,
                        count,
                        priority: 60,
                        include_deployed: false,
                    });
                }
            }
        }
        self.ai_request_transports(controller, state, &mut requests);
        if controller.takeover
            && requests.iter().any(|r| {
                self.unit_type(r.unit_type).is_some_and(|u| {
                    !u.structure
                        && u.movement_class != crate::map::MovementClass::Ground
                        && u.garrison.is_some()
                })
            })
        {
            // An inaccessible town needs one boarding party, not two armies
            // filling every production exit while air/naval access is funded.
            for request in &mut requests {
                if request.include_deployed
                    && self.unit_type(request.unit_type).is_some_and(|u| {
                        !u.structure
                            && u.worker.is_none()
                            && u.weapon.is_some()
                            && u.movement_class == crate::map::MovementClass::Ground
                    })
                {
                    request.count = request
                        .count
                        .div_ceil(2)
                        .max(state.defense.get(&request.unit_type).copied().unwrap_or(0));
                }
            }
        }
        // A worker-only start must fund its economic center before farms or
        // military spending. Its workers must be able to build that center;
        // a secondary depot training haulers must not restart the opening.
        // Planned/incomplete centers still reserve this budget:
        // construction is paid only when the builder arrives.
        let opening = requests.iter().find_map(|request| {
            let unit = self.unit_type(request.unit_type)?;
            let produces_workers = unit.trains.iter().any(|id| {
                self.unit_type(*id)
                    .is_some_and(|u| u.worker.is_some() && u.builds.contains(&unit.id))
            }) || unit
                .offspring
                .as_ref()
                .and_then(|o| self.unit_type(o.unit_type))
                .is_some_and(|producer| {
                    producer.trains.iter().any(|id| {
                        self.unit_type(*id)
                            .is_some_and(|u| u.worker.is_some() && u.builds.contains(&unit.id))
                    })
                });
            if unit.dropoff.is_empty() || !produces_workers {
                return None;
            }
            let complete = self.state.entities.iter().any(|e| {
                e.owner == controller.player
                    && e.construction.is_none()
                    && !e.airborne
                    && self.ai_in_town(controller, state, e)
                    && (e.unit_type == self.researched_unit_type(controller.player, unit.id)
                        || self
                            .unit_type(e.unit_type)
                            .is_some_and(|u| u.provides_types.contains(&unit.id)))
            });
            (!complete).then_some(unit.id)
        });
        if let Some(center) = opening {
            requests.retain(|request| request.unit_type == center);
        }
        // Supply is built through the same builder/payment/placement path.
        let (used, provided) = self.supply(controller.player);
        if opening.is_none()
            && used.saturating_add(if controller.takeover { 2 } else { 4 }) >= provided
            && provided < self.rules.supply_limit
        {
            let supply = self.rules.units.iter().find(|u| {
                u.supply_provided > 0
                    && u.dropoff.is_empty()
                    && self.state.entities.iter().any(|e| {
                        e.owner == controller.player
                            && self.unit_type(e.unit_type).is_some_and(|builder| {
                                builder.builds.contains(&u.id) || builder.trains.contains(&u.id)
                            })
                    })
            });
            if let Some(supply) = supply {
                let count = self
                    .state
                    .entities
                    .iter()
                    .filter(|e| e.owner == controller.player && e.unit_type == supply.id)
                    .count();
                if !self.state.entities.iter().any(|e| {
                    e.owner == controller.player
                        && ((e.unit_type == supply.id && e.construction.is_some())
                            || matches!(e.order, UnitOrder::PlaceBuilding { unit_type, .. } if unit_type == supply.id))
                }) {
                    requests.insert(
                        0,
                        AiRequest {
                            unit_type: supply.id,
                            count: (count + 1).min(4096) as u16,
                            priority: 255,
                    include_deployed: false,
                        },
                    );
                }
            }
        }
        let requests = self.ai_dependencies(controller, state, requests);
        self.ai_balance_gathering(controller, state, &requests);
        self.ai_gather_shortfall(controller, state, &requests);
        let mut reserved = BTreeMap::<String, u64>::new();
        if controller.takeover {
            for actor in self
                .state
                .entities
                .iter()
                .filter(|e| e.owner == controller.player)
            {
                if let UnitOrder::PlaceBuilding { unit_type, .. } = actor.order
                    && let Some(unit) = self.unit_type(unit_type)
                {
                    for cost in &unit.cost {
                        *reserved.entry(cost.kind.clone()).or_default() += u64::from(cost.amount);
                    }
                }
            }
        }
        // Keep the clearing crew on one job. A remote coast must not consume
        // every woodcutter while the town still needs land-based facilities.
        let mut clearing_job: Option<(bool, Vec<(EntityId, ResourceId)>)> = None;
        for request in requests {
            if self.ai_requested_count(controller, state, &request) >= request.count {
                continue;
            }
            let unit = self
                .unit_type(self.researched_unit_type(controller.player, request.unit_type))
                .unwrap()
                .clone();
            // Missing technology/facilities cannot reserve the whole budget while
            // their prerequisites are still being constructed.
            if controller.takeover && !self.ai_producer_exists(controller, &unit) {
                continue;
            }
            // Expanding a town can cover a large island. Retry site searches
            // at one bounded interval instead of every town decision.
            if controller.takeover && unit.structure && !self.tick().0.is_multiple_of(64) {
                continue;
            }
            if !self.can_pay(controller.player, &unit.cost)
                || unit.cost.iter().any(|cost| {
                    self.resource_balance(controller.player, &cost.kind)
                        < u64::from(cost.amount) + reserved.get(&cost.kind).copied().unwrap_or(0)
                })
            {
                // Saving for the next priority purchase must not be defeated by
                // repeatedly spending that budget on cheaper recruits.
                let can_earn_shortfall = unit
                    .cost
                    .iter()
                    .filter(|cost| {
                        self.resource_balance(controller.player, &cost.kind)
                            < u64::from(cost.amount)
                    })
                    .all(|cost| self.ai_can_gather_kind(controller.player, &cost.kind));
                if controller.takeover && can_earn_shortfall {
                    break;
                }
                continue;
            }
            if let Some(entity) = self
                .state
                .entities
                .iter()
                .find(|e| {
                    e.owner == controller.player
                        && e.construction.is_none()
                        && e.production.is_empty()
                        && self.ai_in_town(controller, state, e)
                        && self.unit_type(e.unit_type).is_some_and(|u| {
                            u.transforms_on_production && u.trains.contains(&unit.id)
                        })
                })
                .map(|e| e.id)
            {
                self.ai_order(
                    controller,
                    state,
                    Order::Train {
                        entity,
                        unit_type: unit.id,
                    },
                );
                continue;
            }
            if unit.structure {
                let mut builders: Vec<_> = self
                    .state
                    .entities
                    .iter()
                    .filter(|e| {
                        e.owner == controller.player
                            && !stalled.contains(&e.id)
                            && e.construction.is_none()
                            && !self.inside_structure(e)
                            && !matches!(
                                e.order,
                                UnitOrder::Build { .. }
                                    | UnitOrder::PlaceBuilding { .. }
                                    | UnitOrder::Repair { .. }
                            )
                            && self.ai_in_town(controller, state, e)
                            && self
                                .unit_type(e.unit_type)
                                .is_some_and(|u| u.builds.contains(&unit.id))
                    })
                    .map(|e| e.id)
                    .collect();
                if builders.is_empty() {
                    continue;
                }
                let special: Vec<_> = if unit.extracts.is_some() {
                    self.state
                        .resources
                        .iter()
                        .filter(|r| {
                            r.requires_extractor
                                && rts::distance(r.position, controller.home) <= radius
                        })
                        .map(|r| r.position)
                        .collect()
                } else if let Some(parent) = unit.addon_parent {
                    self.state
                        .entities
                        .iter()
                        .filter(|e| e.owner == controller.player && e.unit_type == parent)
                        .filter_map(|e| self.addon_position(e.id))
                        .collect()
                } else {
                    Vec::new()
                };
                let mut positions = special;
                if unit.extracts.is_none() && unit.addon_parent.is_none() {
                    positions = self.ai_build_positions(controller, &unit);
                } else if controller.takeover {
                    positions.sort_by_key(|p| (rts::distance(*p, controller.home), p.x, p.y));
                }
                let mut clearing = None;
                let mut clearing_probes = 0;
                let mut placed = false;
                let mut sites = Vec::new();
                let shore = unit.placement_surface == crate::map::PlacementSurface::Shore;
                'placement: for position in positions {
                    if controller.takeover
                        && !self.ai_building_space(controller.player, &unit, position, false)
                    {
                        if clearing.is_none()
                            && clearing_probes < 4
                            && clearing_job
                                .as_ref()
                                .is_none_or(|(coast, _)| *coast && !shore)
                            && self.ai_building_space(controller.player, &unit, position, true)
                        {
                            clearing_probes += 1;
                            let orders = self.ai_clear_build_site(
                                controller.player,
                                &builders,
                                &unit,
                                position,
                            );
                            if !orders.is_empty() {
                                clearing = Some(orders);
                            }
                        }
                        continue;
                    }
                    if controller.takeover {
                        sites.push(position);
                        continue;
                    }
                    for &entity in &builders {
                        if self
                            .build_rejection(controller.player, entity, unit.id, position)
                            .is_none()
                            && self.ai_order(
                                controller,
                                state,
                                Order::Build {
                                    entity,
                                    unit_type: unit.id,
                                    position,
                                },
                            )
                        {
                            placed = true;
                            break 'placement;
                        }
                    }
                }
                if controller.takeover
                    && let Some((entity, position)) =
                        self.ai_build_site(controller.player, &mut builders, &unit, &sites)
                    && self.ai_order(
                        controller,
                        state,
                        Order::Build {
                            entity,
                            unit_type: unit.id,
                            position,
                        },
                    )
                {
                    self.refresh_navigation_geometry();
                    for cost in &unit.cost {
                        *reserved.entry(cost.kind.clone()).or_default() += u64::from(cost.amount);
                    }
                    placed = true;
                }
                if !placed && let Some(orders) = clearing {
                    clearing_job = Some((shore, orders));
                }
            } else {
                let producer = self
                    .state
                    .entities
                    .iter()
                    .find(|e| {
                        e.owner == controller.player
                            && e.construction.is_none()
                            && e.production.is_empty()
                            && !e.airborne
                            && e.research.is_none()
                            && self.ai_in_town(controller, state, e)
                            && self.can_train_type(e, unit.id)
                    })
                    .map(|e| e.id);
                if let Some(entity) = producer {
                    self.ai_order(
                        controller,
                        state,
                        Order::Train {
                            entity,
                            unit_type: unit.id,
                        },
                    );
                }
            }
        }
        if let Some((_, orders)) = clearing_job {
            for (entity, resource) in orders {
                let actor = &self.state.entities[self.index(entity).unwrap()];
                if matches!(actor.order, UnitOrder::Idle | UnitOrder::Gather { .. })
                    && actor.order != (UnitOrder::Gather { resource })
                {
                    self.ai_order(controller, state, Order::Gather { entity, resource });
                }
            }
        }
    }

    /// A one-worker town must switch resources when its next purchase is blocked.
    /// Interior workers use the normal free-exit search; builders keep their work.
    fn ai_gather_shortfall(
        &mut self,
        controller: &AiController,
        state: &mut AiState,
        requests: &[AiRequest],
    ) {
        let missing = requests
            .iter()
            .find(|r| {
                self.ai_requested_count(controller, state, r) < r.count
                    && (!controller.takeover
                        || self
                            .unit_type(r.unit_type)
                            .is_some_and(|u| self.ai_producer_exists(controller, u)))
            })
            .and_then(|r| self.unit_type(self.researched_unit_type(controller.player, r.unit_type)))
            .and_then(|u| {
                u.cost.iter().find(|cost| {
                    self.resource_balance(controller.player, &cost.kind) < u64::from(cost.amount)
                })
            })
            .map(|cost| cost.kind.clone());
        let Some(kind) = missing else {
            return;
        };
        let assigned = self.state.entities.iter().any(|e| e.owner == controller.player
            && self.ai_in_town(controller, state, e)
            && matches!(e.order, UnitOrder::Gather { resource } if self.state.resources.iter().any(|r| r.id == resource && r.kind == kind && r.amount > 0)));
        if assigned {
            return;
        }
        let mut best = None;
        for worker in self.state.entities.iter().filter(|e| {
            e.owner == controller.player
                && e.construction.is_none()
                && e.auto_attack_target.is_none()
                && matches!(e.order, UnitOrder::Idle | UnitOrder::Gather { .. })
                && self.ai_in_town(controller, state, e)
        }) {
            for node in self.state.resources.iter().filter(|r| {
                r.kind == kind
                    && r.amount > 0
                    && rts::distance(r.position, controller.home)
                        <= i64::from(controller.radius).pow(2)
                    && self.gather_rejection(worker.id, r.id).is_none()
            }) {
                let candidate = (
                    !matches!(worker.order, UnitOrder::Idle),
                    rts::distance(worker.position, node.position),
                    worker.id,
                    node.id,
                );
                if best.is_none_or(|old| candidate < old) {
                    best = Some(candidate);
                }
            }
        }
        if let Some((_, _, entity, resource)) = best {
            self.ai_order(controller, state, Order::Gather { entity, resource });
        }
    }
}
