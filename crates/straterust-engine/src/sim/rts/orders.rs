use super::*;

impl World {
    pub(in crate::sim) fn has_prerequisites(&self, player: PlayerId, unit: &UnitType) -> bool {
        unit.prerequisites.iter().all(|id| {
            self.state.entities.iter().any(|entity| {
                entity.owner == player
                    && entity.unit_type == *id
                    && entity.construction.is_none()
                    && !entity.airborne
            })
        })
    }
    pub(in crate::sim) fn can_pay(&self, player: PlayerId, cost: &[ResourceAmount]) -> bool {
        cost.iter()
            .all(|amount| self.resource_balance(player, &amount.kind) >= u64::from(amount.amount))
    }
    pub(in crate::sim) fn pay(&mut self, player: PlayerId, cost: &[ResourceAmount], refund: bool) {
        let resources = &mut self.state.players[usize::from(player.0)].resources;
        for amount in cost {
            let value = resources.entry(amount.kind.clone()).or_default();
            if refund {
                *value += u64::from(amount.amount);
            } else {
                *value -= u64::from(amount.amount);
            }
        }
    }
    /// Shared command/placement-preview validation. It makes no state changes.
    pub fn build_rejection(
        &self,
        player: PlayerId,
        worker: EntityId,
        unit_type: UnitTypeId,
        position: Position,
    ) -> Option<Rejection> {
        if player.0 >= self.map.players {
            return Some(Rejection::UnknownPlayer);
        }
        if self.state.winner.is_some() || self.state.defeated.contains(&player) {
            return Some(Rejection::GameOver);
        }
        let Some(index) = self.index(worker) else {
            return Some(Rejection::UnknownEntity);
        };
        let actor = &self.state.entities[index];
        if actor.owner != player {
            return Some(Rejection::NotOwner);
        }
        if actor.construction.is_some() {
            return Some(Rejection::Unfinished);
        }
        let Some(unit) = self.unit_type(unit_type) else {
            return Some(Rejection::InvalidTarget);
        };
        if (actor.airborne && unit.addon_parent.is_none()) || actor.flight_transition != 0 {
            return Some(Rejection::UnsupportedOrder);
        }
        if !self.creation_allowed(player, unit_type) {
            return Some(Rejection::UnsupportedOrder);
        }
        if !unit.structure || !self.unit_at(index).builds.contains(&unit_type) {
            return Some(Rejection::UnsupportedOrder);
        }
        if !self.has_prerequisites(player, unit) {
            return Some(Rejection::MissingPrerequisite);
        }
        if unit.addon_parent.is_some()
            && (!actor.production.is_empty()
                || actor.research.is_some()
                || self.constructing_addon(worker))
        {
            return Some(Rejection::QueueFull);
        }
        if !self.can_pay(player, &unit.cost) {
            return Some(Rejection::InsufficientResources);
        }
        if self.state.entities.len() >= MAX_ENTITIES || self.state.next_entity_id == u32::MAX {
            return Some(Rejection::EntityLimit);
        }
        if let Some(parent) = unit.addon_parent
            && (actor.unit_type != parent
                || self
                    .state
                    .entities
                    .iter()
                    .any(|entity| entity.parent == Some(worker)))
        {
            return Some(Rejection::InvalidPlacement);
        }
        if let Some(parent) = unit.addon_parent {
            let parent_position = self.addon_parent_position(unit_type, position)?;
            let parent_unit = self.unit_type(parent)?;
            if ((actor.airborne || parent_position != actor.position)
                && parent_unit.flight.is_none())
                || ((actor.airborne || parent_position != actor.position)
                    && !self.resource_clearance_allowed(parent_unit, parent_position))
                || !self.map.can_build(parent_position, parent_unit.placement)
                || self.placement_occupied_except(
                    parent_position,
                    parent_unit.placement,
                    None,
                    false,
                    Some(worker),
                    Some(worker),
                )
                || self.placement_occupied_except(
                    parent_position,
                    parent_unit.footprint,
                    None,
                    true,
                    Some(worker),
                    Some(worker),
                )
            {
                return Some(Rejection::InvalidPlacement);
            }
        }
        let source = unit.extracts.as_ref().and_then(|extraction| {
            self.state.resources.iter().find(|node| {
                node.kind == extraction.resource
                    && node.requires_extractor
                    && node.position == position
            })
        });
        if unit.extracts.is_some() && source.is_none() {
            return Some(Rejection::InvalidPlacement);
        }
        if !self.resource_clearance_allowed(unit, position)
            || !self.creep_placement_allowed(unit, position)
            || (source.is_none() && !self.map.can_build(position, unit.placement))
            || self.placement_occupied_except(
                position,
                unit.placement,
                source.map(|node| node.id),
                false,
                unit.addon_parent.map(|_| worker),
                Some(worker),
            )
            || self.placement_occupied_except(
                position,
                unit.footprint,
                source.map(|node| node.id),
                true,
                unit.addon_parent.map(|_| worker),
                Some(worker),
            )
        {
            return Some(Rejection::InvalidPlacement);
        }
        None
    }
    pub(in crate::sim) fn resource_clearance_allowed(
        &self,
        unit: &UnitType,
        position: Position,
    ) -> bool {
        if unit.resource_clearance == 0 {
            return true;
        }
        let margin = i64::from(unit.resource_clearance) * 2;
        !self.state.resources.iter().any(|resource| {
            (resource.amount > 0 || resource.requires_extractor)
                && (i64::from(position.x) - i64::from(resource.position.x)).abs() * 2
                    < i64::from(unit.placement.width) + i64::from(resource.footprint.width) + margin
                && (i64::from(position.y) - i64::from(resource.position.y)).abs() * 2
                    < i64::from(unit.placement.height)
                        + i64::from(resource.footprint.height)
                        + margin
        })
    }
    pub fn addon_position(&self, parent: EntityId) -> Option<Position> {
        let entity = &self.state.entities[self.index(parent)?];
        let footprint = self.unit_type(entity.unit_type)?.placement;
        Some(Position {
            x: entity.position.x + i32::from(footprint.width) / 2 + 32,
            y: entity.position.y + i32::from(footprint.height) / 2 - 32,
        })
    }
    pub fn constructing_addon(&self, parent: EntityId) -> bool {
        self.state
            .entities
            .iter()
            .any(|entity| entity.parent == Some(parent) && entity.construction.is_some())
    }
    pub fn addon_pending(&self, parent: EntityId) -> bool {
        self.constructing_addon(parent)
            || self.index(parent).is_some_and(|index| {
                let actor = &self.state.entities[index];
                matches!(actor.order, UnitOrder::PlaceAddon { .. })
                    || actor
                        .queued_orders
                        .iter()
                        .any(|order| matches!(order, UnitOrder::PlaceAddon { .. }))
            })
    }
    pub fn addon_parent_position(&self, addon: UnitTypeId, position: Position) -> Option<Position> {
        let parent = self.unit_type(self.unit_type(addon)?.addon_parent?)?;
        Some(Position {
            x: position.x - i32::from(parent.placement.width) / 2 - 32,
            y: position.y - i32::from(parent.placement.height) / 2 + 32,
        })
    }
    pub(in crate::sim) fn placement_occupied_except(
        &self,
        position: Position,
        footprint: Footprint,
        except: Option<ResourceId>,
        collision: bool,
        addon_parent: Option<EntityId>,
        ignored_entity: Option<EntityId>,
    ) -> bool {
        self.state.entities.iter().any(|entity| {
            let unit = self.unit_type(entity.unit_type).expect("validated type");
            addon_parent != Some(entity.id)
                && ignored_entity != Some(entity.id)
                && (addon_parent.is_none()
                    || unit.speed == 0
                    || self.phases_collision(entity)
                    || entity.cloak_transition > 0)
                && !unit.revealer
                && unit.blocks_movement
                && !entity.airborne
                && !entity.gathering_inside
                && entity.garrisoned_in.is_none()
                && unit.movement_class == MovementClass::Ground
                && overlaps(
                    position,
                    footprint,
                    entity.position,
                    if unit.structure && !collision {
                        unit.placement
                    } else {
                        unit.footprint
                    },
                )
        }) || self.state.resources.iter().any(|resource| {
            Some(resource.id) != except
                && (resource.amount > 0 || resource.requires_extractor)
                && overlaps(position, footprint, resource.position, resource.footprint)
        })
    }
    /// Repair orders may wait for resources; eligibility excludes self, hostile,
    /// unfinished, healthy and unsupported targets before changing any orders.
    pub fn repair_rejection(&self, worker: EntityId, target: EntityId) -> Option<Rejection> {
        let Some(index) = self.index(worker) else {
            return Some(Rejection::UnknownEntity);
        };
        let actor = &self.state.entities[index];
        let unit = self.unit_at(index);
        if unit.repairs.is_empty() || self.rules.repair.is_none() {
            return Some(Rejection::UnsupportedOrder);
        }
        if actor.construction.is_some() {
            return Some(Rejection::Unfinished);
        }
        let Some(other) = self.index(target) else {
            return Some(Rejection::InvalidTarget);
        };
        let entity = &self.state.entities[other];
        if target == worker
            || actor.owner != entity.owner
            || entity.construction.is_some()
            || (entity.hp >= self.unit_at(other).max_hp && entity.damage_fraction == 0)
            || !unit.repairs.contains(&entity.unit_type)
        {
            return Some(Rejection::InvalidTarget);
        }
        None
    }

    pub(in crate::sim) fn action_rejection(
        &self,
        index: usize,
        order: &UnitOrder,
    ) -> Option<Rejection> {
        let actor = &self.state.entities[index];
        let unit = self.unit_at(index);
        match *order {
            UnitOrder::UnloadAt { target } => return self.unload_at_rejection(actor.id, target),
            UnitOrder::Pickup { target } => {
                if unit.structure || unit.speed == 0 {
                    return Some(Rejection::UnsupportedOrder);
                }
                return self.load_rejection(target, actor.id);
            }
            UnitOrder::PlaceMine { target } => return self.mine_rejection(actor.id, target),
            UnitOrder::Land { target } => return self.land_rejection(actor.id, target),
            UnitOrder::Load { target } => return self.load_rejection(actor.id, target),
            UnitOrder::Move { target }
            | UnitOrder::AttackMove { target }
            | UnitOrder::Patrol { target } => {
                if unit.speed == 0 && !actor.airborne {
                    return Some(Rejection::UnsupportedOrder);
                }
                if !self.map.contains(target) {
                    return Some(Rejection::OutOfBounds);
                }
            }
            UnitOrder::Attack { target } => {
                if unit.weapon.is_none() {
                    return Some(Rejection::UnsupportedOrder);
                }
                if self.index(target).is_none_or(|other| {
                    let target = &self.state.entities[other];
                    !self.can_attack_entity(actor, target)
                }) {
                    return Some(Rejection::InvalidTarget);
                }
            }
            UnitOrder::Gather { resource } => {
                let Some(worker) = &unit.worker else {
                    return Some(Rejection::UnsupportedOrder);
                };
                if !self.state.resources.iter().any(|node| {
                    node.id == resource
                        && (node.amount > 0 || node.requires_extractor)
                        && worker.resource_kinds.contains(&node.kind)
                        && (!node.requires_extractor || self.extractor(actor.owner, node).is_some())
                }) {
                    return Some(Rejection::InvalidTarget);
                }
            }
            UnitOrder::Build { building } => {
                let Some(other) = self.index(building) else {
                    return Some(Rejection::InvalidTarget);
                };
                let target = &self.state.entities[other];
                if target.owner != actor.owner
                    || target.construction.is_none()
                    || !unit.builds.contains(&target.unit_type)
                {
                    return Some(Rejection::InvalidTarget);
                }
                if target
                    .construction
                    .as_ref()
                    .and_then(|progress| progress.worker)
                    .is_some_and(|worker| worker != actor.id)
                {
                    return Some(Rejection::InvalidTarget);
                }
            }
            UnitOrder::Repair { target } => return self.repair_rejection(actor.id, target),
            UnitOrder::PlaceAddon { unit_type, target } => {
                return self.build_rejection(actor.owner, actor.id, unit_type, target);
            }
            UnitOrder::Idle | UnitOrder::Hold => {}
        }
        None
    }
    pub(in crate::sim) fn apply(&mut self, command: &Command) -> Option<Rejection> {
        if self
            .state
            .mission
            .as_ref()
            .is_some_and(|mission| mission.paused)
        {
            return Some(Rejection::UnsupportedOrder);
        }
        if self.state.winner.is_some() || self.state.defeated.contains(&command.player) {
            return Some(Rejection::GameOver);
        }
        let Some(index) = self.index(command.order.entity()) else {
            return Some(Rejection::UnknownEntity);
        };
        if self.state.entities[index].owner != command.player {
            return Some(Rejection::NotOwner);
        }
        if self.unit_at(index).mine.is_some() {
            return Some(Rejection::UnsupportedOrder);
        }
        if self.state.entities[index].garrisoned_in.is_some()
            && !matches!(command.order, Order::Stim { .. })
        {
            return Some(Rejection::UnsupportedOrder);
        }
        if self.state.entities[index].construction.is_some()
            && !matches!(command.order, Order::Cancel { .. })
        {
            return Some(Rejection::Unfinished);
        }
        let action = match &command.order {
            Order::PlaceMine { target, .. } => UnitOrder::PlaceMine { target: *target },
            Order::Lift { .. } => return self.start_lift(index),
            Order::Land { target, .. } => UnitOrder::Land { target: *target },
            Order::Load { target, .. } => UnitOrder::Load { target: *target },
            Order::UnloadAt { target, .. } => UnitOrder::UnloadAt { target: *target },
            Order::Unload { .. } => {
                if let Some(reason) = self.unload_rejection(self.state.entities[index].id) {
                    return Some(reason);
                }
                if !self.unit_at(index).structure {
                    let target = self.state.entities[index].position;
                    self.assign(index, UnitOrder::UnloadAt { target }, true);
                    return None;
                }
                self.unload_garrison(index, false);
                return None;
            }
            Order::UnloadPassenger { passenger, .. } => {
                if let Some(reason) =
                    self.unload_passenger_rejection(self.state.entities[index].id, *passenger)
                {
                    return Some(reason);
                }
                self.unload_passenger(index, *passenger, false);
                return None;
            }
            Order::Research { research, .. } => return self.start_research(index, *research),
            Order::Stim { .. } => return self.use_stim(index),
            Order::Cloak { enabled, .. } => return self.toggle_cloak(index, *enabled),
            Order::Scan { target, .. } => return self.start_scan(index, *target),
            Order::Move { target, .. } => UnitOrder::Move { target: *target },
            Order::Stop { .. } => UnitOrder::Idle,
            Order::Hold { .. } => UnitOrder::Hold,
            Order::Attack { target, .. } => UnitOrder::Attack { target: *target },
            Order::AttackMove { target, .. } => UnitOrder::AttackMove { target: *target },
            Order::Patrol { target, .. } => UnitOrder::Patrol { target: *target },
            Order::Gather { resource, .. } => UnitOrder::Gather {
                resource: *resource,
            },
            Order::Resume { building, .. } => UnitOrder::Build {
                building: *building,
            },
            Order::Repair { target, .. } => UnitOrder::Repair { target: *target },
            Order::Wander { .. } => {
                if self.unit_at(index).speed == 0 {
                    return Some(Rejection::UnsupportedOrder);
                }
                let footprint = self.unit_at(index).footprint;
                let target = Position {
                    x: i32::from(footprint.width / 2)
                        + (splitmix64(&mut self.state.rng_state)
                            % (self.map.width as u64 - u64::from(footprint.width) + 1))
                            as i32,
                    y: i32::from(footprint.height / 2)
                        + (splitmix64(&mut self.state.rng_state)
                            % (self.map.height as u64 - u64::from(footprint.height) + 1))
                            as i32,
                };
                // A random obstructed cell means staying put. Occupied valid
                // destinations use the same path/retry behavior as Move.
                let target =
                    if self
                        .map
                        .can_move(target, footprint, self.unit_at(index).movement_class)
                    {
                        target
                    } else {
                        self.state.entities[index].position
                    };
                self.assign(index, UnitOrder::Move { target }, true);
                return None;
            }
            Order::Queue { order, .. } => {
                if let Some(reason) = self.action_rejection(index, order) {
                    return Some(reason);
                }
                if matches!(order, UnitOrder::Build { .. }) {
                    return Some(Rejection::UnsupportedOrder);
                }
                if self.state.entities[index].queued_orders.len() >= MAX_QUEUED_ORDERS {
                    return Some(Rejection::QueueFull);
                }
                if self.state.entities[index].order == UnitOrder::Idle {
                    self.assign(index, order.clone(), false);
                } else {
                    self.state.entities[index]
                        .queued_orders
                        .push_back(order.clone());
                }
                return None;
            }
            Order::Rally { target, .. } => {
                if self.unit_at(index).trains.is_empty() {
                    return Some(Rejection::UnsupportedOrder);
                }
                if !self.map.contains(*target) {
                    return Some(Rejection::OutOfBounds);
                }
                self.state.entities[index].rally = Some(*target);
                self.state.entities[index].rally_resource = None;
                return None;
            }
            Order::RallyResource { resource, .. } => {
                if self.unit_at(index).trains.is_empty() {
                    return Some(Rejection::UnsupportedOrder);
                }
                let Some(node) = self.state.resources.iter().find(|node| {
                    node.id == *resource
                        && (node.amount != 0 || node.requires_extractor)
                        && self.visibility(command.player, node.position) != Visibility::Unexplored
                }) else {
                    return Some(Rejection::InvalidTarget);
                };
                self.state.entities[index].rally = Some(node.position);
                self.state.entities[index].rally_resource = Some(*resource);
                return None;
            }
            Order::Train { unit_type, .. } => {
                if !self.creation_allowed(command.player, *unit_type) {
                    return Some(Rejection::UnsupportedOrder);
                }
                if self.state.entities[index].airborne
                    || self.state.entities[index].flight_transition != 0
                {
                    return Some(Rejection::UnsupportedOrder);
                }
                if self.state.entities[index].research.is_some()
                    || self.addon_pending(self.state.entities[index].id)
                {
                    return Some(Rejection::QueueFull);
                }
                if !self.unit_at(index).trains.contains(unit_type) {
                    return Some(Rejection::UnsupportedOrder);
                }
                if self.state.entities[index].production.len() >= MAX_PRODUCTION {
                    return Some(Rejection::QueueFull);
                }
                let unit = self
                    .unit_type(*unit_type)
                    .expect("validated train type")
                    .clone();
                if !self.has_prerequisites(command.player, &unit) {
                    return Some(Rejection::MissingPrerequisite);
                }
                if unit.prerequisites.iter().any(|id| {
                    self.unit_type(*id)
                        .is_some_and(|u| u.addon_parent == Some(self.unit_at(index).id))
                        && !self.state.entities.iter().any(|addon| {
                            addon.parent == Some(self.state.entities[index].id)
                                && addon.unit_type == *id
                                && addon.construction.is_none()
                        })
                }) {
                    return Some(Rejection::MissingPrerequisite);
                }
                if !self.can_pay(command.player, &unit.cost) {
                    return Some(Rejection::InsufficientResources);
                }
                self.pay(command.player, &unit.cost, false);
                self.state.entities[index]
                    .production
                    .push_back(ProductionJob {
                        unit_type: *unit_type,
                        remaining: unit.build_ticks,
                        total: unit.build_ticks,
                        started: false,
                    });
                return None;
            }
            Order::Build {
                entity,
                unit_type,
                position,
            } => {
                if let Some(reason) =
                    self.build_rejection(command.player, *entity, *unit_type, *position)
                {
                    return Some(reason);
                }
                let unit = self
                    .unit_type(*unit_type)
                    .expect("validated build type")
                    .clone();
                if unit.addon_parent.is_some()
                    && (self.state.entities[index].airborne
                        || self.addon_position(*entity) != Some(*position))
                {
                    let target = self.addon_parent_position(*unit_type, *position).unwrap();
                    if !self.state.entities[index].airborne
                        && let Some(reason) = self.start_lift(index)
                    {
                        return Some(reason);
                    }
                    // Lift completion interprets an active Land as a landing
                    // transition. Travel first, then activate Land at arrival.
                    self.assign(index, UnitOrder::Move { target }, true);
                    self.state.entities[index]
                        .queued_orders
                        .push_back(UnitOrder::Land { target });
                    self.state.entities[index]
                        .queued_orders
                        .push_back(UnitOrder::PlaceAddon {
                            unit_type: *unit_type,
                            target: *position,
                        });
                    return None;
                }
                self.pay(command.player, &unit.cost, false);
                let id = EntityId(self.state.next_entity_id);
                self.state.next_entity_id += 1;
                self.state.entities.push(Entity {
                    id,
                    owner: command.player,
                    unit_type: *unit_type,
                    position: *position,
                    hp: 1,
                    parent: unit.addon_parent.map(|_| *entity),
                    construction: Some(Construction {
                        worker: Some(*entity),
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
                        if (unit.addon_parent.is_none() && actor.id != *entity)
                            || actor_type.speed == 0
                            || actor.airborne
                            || self.movement_locked(actor)
                            || actor.garrisoned_in.is_some()
                            || actor.gathering_inside
                            || !overlaps(
                                *position,
                                unit.footprint,
                                actor.position,
                                actor_type.footprint,
                            )
                        {
                            continue;
                        }
                        let exit = perimeter(
                            *position,
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
                UnitOrder::Build { building: id }
            }
            Order::Cancel { .. } => {
                if self.cancel_research(index) {
                    return None;
                }
                let actor = self.state.entities[index].clone();
                if self.addon_pending(actor.id) && !self.constructing_addon(actor.id) {
                    self.assign(index, UnitOrder::Idle, true);
                    return None;
                }
                if let Some(addon) = self.state.entities.iter().position(|entity| {
                    entity.parent == Some(actor.id) && entity.construction.is_some()
                }) {
                    let cost = self.unit_at(addon).cost.clone();
                    self.pay(command.player, &cost, true);
                    self.state.entities.remove(addon);
                    self.clear_dead_references();
                    return None;
                }
                if actor.construction.is_some() {
                    let cost = self.unit_at(index).cost.clone();
                    self.pay(command.player, &cost, true);
                    self.state.entities.remove(index);
                    self.clear_dead_references();
                } else if let Some(job) = self.state.entities[index].production.pop_back() {
                    let cost = self
                        .unit_type(job.unit_type)
                        .expect("validated type")
                        .cost
                        .clone();
                    self.pay(command.player, &cost, true);
                } else {
                    return Some(Rejection::UnsupportedOrder);
                }
                return None;
            }
        };
        if let Some(reason) = self.action_rejection(index, &action) {
            return Some(reason);
        }
        self.assign(index, action, true);
        None
    }
    pub(in crate::sim) fn assign(&mut self, index: usize, order: UnitOrder, clear_queue: bool) {
        let id = self.state.entities[index].id;
        let pickup = if let UnitOrder::Load { target } = order {
            self.index(target)
        } else {
            None
        };
        if let UnitOrder::Build { building } = self.state.entities[index].order
            && let Some(other) = self.index(building)
            && let Some(progress) = &mut self.state.entities[other].construction
            && progress.worker == Some(id)
        {
            progress.worker = None;
            progress.work_position = None;
            progress.work_ticks = 0;
        }
        if let UnitOrder::Build { building } = order
            && let Some(other) = self.index(building)
            && let Some(progress) = &mut self.state.entities[other].construction
        {
            progress.worker = Some(id);
            progress.work_position = None;
            progress.work_ticks = 0;
        }
        if self.state.entities[index].cloaked
            && self
                .unit_at(index)
                .cloak
                .as_ref()
                .is_some_and(|c| c.reveal_on_order)
            && matches!(
                order,
                UnitOrder::Move { .. }
                    | UnitOrder::AttackMove { .. }
                    | UnitOrder::Attack { .. }
                    | UnitOrder::Patrol { .. }
            )
        {
            self.reveal(index);
        }
        let actor = &mut self.state.entities[index];
        actor.order = order;
        actor.auto_attack_target = None;
        actor.retaliation_position = None;
        actor.target = None;
        actor.path.clear();
        actor.path_retry = self.state.tick;
        actor.harvest_progress = 0;
        actor.harvest_waiting_since = None;
        actor.motion_speed = 0;
        actor.motion_phase = 0;
        actor.gathering_inside = false;
        actor.repair_progress = 0;
        actor.strikes.clear();
        actor.dropoff_target = None;
        actor.patrol_origin = if matches!(actor.order, UnitOrder::Patrol { .. }) {
            Some(actor.position)
        } else {
            None
        };
        actor.patrol_returning = false;
        if clear_queue {
            actor.queued_orders.clear();
        }
        if let Some(container) = pickup {
            self.request_pickup(container, id);
        }
    }
    pub(in crate::sim) fn finish(&mut self, index: usize) {
        let order = self.state.entities[index]
            .queued_orders
            .pop_front()
            .unwrap_or_default();
        self.assign(index, order, false);
    }
}
