use super::*;

impl World {
    pub(in crate::sim) fn retreat_position(
        &self,
        index: usize,
        origin: Position,
    ) -> Option<Position> {
        let actor = &self.state.entities[index];
        let unit = self.unit_at(index);
        if unit.speed == 0 && !actor.airborne || self.movement_locked(actor) {
            return None;
        }
        // Use the hit's origin once. A Move order does not track a concealed
        // attacker, and ordinary player orders can interrupt the retreat.
        let mut dx = i64::from(actor.position.x) - i64::from(origin.x);
        let dy = i64::from(actor.position.y) - i64::from(origin.y);
        if dx == 0 && dy == 0 {
            dx = 1;
        }
        let obstacles = self.obstacles(actor.id);
        let footprint = unit.footprint;
        for (x, y) in [
            (dx, dy),
            (dx - dy, dy + dx),
            (dx + dy, dy - dx),
            (-dy, dx),
            (dy, -dx),
        ] {
            let length = ((x * x + y * y) as u64).isqrt().max(1) as i64;
            for travel in [192, 128, 64, 32, 16] {
                let target = Position {
                    x: (actor.position.x + (x * travel / length) as i32).clamp(
                        i32::from(footprint.width / 2),
                        self.map.width - i32::from(footprint.width.div_ceil(2)),
                    ),
                    y: (actor.position.y + (y * travel / length) as i32).clamp(
                        i32::from(footprint.height / 2),
                        self.map.height - i32::from(footprint.height.div_ceil(2)),
                    ),
                };
                if distance(target, origin) > distance(actor.position, origin)
                    && find_path(
                        &self.map,
                        footprint,
                        self.movement_class(actor),
                        actor.position,
                        target,
                        &obstacles,
                    )
                    .is_some()
                {
                    return Some(target);
                }
            }
        }
        None
    }

    pub(in crate::sim) fn obstacles(&self, except: EntityId) -> Vec<Obstacle> {
        self.navigation_obstacles(except, None)
    }
    pub(in crate::sim) fn navigation_obstacles(
        &self,
        except: EntityId,
        arrival: Option<(Position, Footprint)>,
    ) -> Vec<Obstacle> {
        self.collect_navigation_obstacles(except, arrival, false)
    }
    fn collect_navigation_obstacles(
        &self,
        except: EntityId,
        arrival: Option<(Position, Footprint)>,
        static_only: bool,
    ) -> Vec<Obstacle> {
        let actor = self.state.entities.iter().find(|e| e.id == except);
        let phasing_worker = actor
            .and_then(|e| self.unit_type(e.unit_type))
            .is_some_and(|u| u.phases_while_gathering);
        // Harvesting can leave a worker inside a packed group. Keep mobile
        // collision disabled until it has left that overlap, even if its
        // gathering order was replaced by construction or movement.
        let phasing = phasing_worker
            && actor.is_some_and(|a| {
                matches!(a.order, UnitOrder::Gather { .. })
                    || self.state.entities.iter().any(|e| {
                        e.id != except
                            && !self.phases_collision(e)
                            && !e.gathering_inside
                            && e.garrisoned_in.is_none()
                            && e.doodad_enabled != Some(false)
                            && self.movement_class(a) == self.movement_class(e)
                            && self.unit_type(e.unit_type).is_some_and(|u| {
                                u.speed > 0
                                    && u.blocks_movement
                                    && overlaps(
                                        a.position,
                                        self.unit_type(a.unit_type).unwrap().footprint,
                                        e.position,
                                        u.footprint,
                                    )
                            })
                    })
            });
        let mut obstacles: Vec<_> = self
            .state
            .entities
            .iter()
            .filter(|entity| {
                entity.id != except
                    && (!static_only
                        || self
                            .unit_type(entity.unit_type)
                            .expect("validated type")
                            .speed
                            == 0
                            && !entity.airborne)
                    && (!phasing
                        || self
                            .unit_type(entity.unit_type)
                            .expect("validated type")
                            .speed
                            == 0)
                    && !self.phases_collision(entity)
                    && entity.doodad_enabled != Some(false)
                    && self
                        .unit_type(entity.unit_type)
                        .expect("validated type")
                        .blocks_movement
                    && !entity.gathering_inside
                    && entity.garrisoned_in.is_none()
                    && !self
                        .unit_type(entity.unit_type)
                        .expect("validated type")
                        .revealer
                    && arrival.is_none_or(|(target, footprint)| {
                        let unit = self.unit_type(entity.unit_type).expect("validated type");
                        (unit.speed == 0 && !entity.airborne)
                            || in_range(
                                entity.position,
                                unit.footprint,
                                target,
                                footprint,
                                u32::from(footprint.width.max(footprint.height)),
                            )
                    })
            })
            .map(|entity| {
                let unit = self.unit_type(entity.unit_type).expect("validated type");
                Obstacle {
                    position: entity.position,
                    footprint: unit.footprint,
                    movement_class: self.movement_class(entity),
                }
            })
            .collect();
        obstacles.extend(
            self.state
                .resources
                .iter()
                .filter(|node| self.resource_blocks_movement(node))
                .map(|node| Obstacle {
                    position: node.position,
                    footprint: node.footprint,
                    movement_class: MovementClass::Ground,
                }),
        );
        if !static_only && actor.is_some_and(|a| matches!(a.order, UnitOrder::Gather { .. })) {
            // Harvesting may phase through ordinary mobile traffic, but the
            // reserved endpoints must stay separate even before workers arrive.
            self.add_harvest_spot_obstacles(except, &mut obstacles);
        }
        obstacles
    }
    pub(super) fn static_navigation_obstacles(&self) -> Vec<Obstacle> {
        self.collect_navigation_obstacles(EntityId(0), None, true)
    }
    pub(in crate::sim) fn near_arrival(&self, index: usize, target: Position) -> bool {
        self.near_arrival_from(index, self.state.entities[index].position, target)
    }
    pub(super) fn near_arrival_from(
        &self,
        index: usize,
        position: Position,
        target: Position,
    ) -> bool {
        let actor = &self.state.entities[index];
        let unit = self.unit_at(index);
        // Mobile traffic away from the destination cannot complete an order.
        // Keep occupants at the goal so groups can still settle beside each
        // other, and keep terrain, buildings and resources for unreachable goals.
        let obstacles = self.navigation_obstacles(actor.id, Some((target, unit.footprint)));
        find_path_near(
            &self.map,
            unit.footprint,
            self.movement_class(actor),
            position,
            target,
            &obstacles,
        )
        .is_some_and(|route| route.is_empty())
    }
    pub(in crate::sim) fn navigate(
        &mut self,
        index: usize,
        target: Position,
        allow_near: bool,
    ) -> bool {
        if self.movement_locked(&self.state.entities[index]) {
            return false;
        }
        let mut unit = self.unit_at(index).clone();
        if let Some((percent, acceleration)) = self.researched_motion(&self.state.entities[index]) {
            unit.speed = unit.speed * i32::from(percent) / 100;
            if let Some(motion) = &mut unit.motion {
                motion.speed = motion.speed * u32::from(percent) / 100;
                motion.acceleration = motion.acceleration * u32::from(acceleration) / 100;
                for step in &mut motion.steps {
                    *step = (u32::from(*step) * u32::from(percent) / 100).min(u32::from(u16::MAX))
                        as u16;
                }
            }
        }
        if self.state.entities[index].airborne {
            unit.speed = unit.flight.as_ref().expect("validated flight").speed;
            unit.movement_class = MovementClass::Air;
        }
        if self.state.entities[index].stim_remaining > 0 {
            unit.speed = unit.speed * 3 / 2;
        }
        let actor = &self.state.entities[index];
        if actor.position == target && actor.motion_fraction == [0, 0] {
            self.state.entities[index].target = None;
            self.state.entities[index].route_wait = None;
            self.state.entities[index].motion_speed = 0;
            self.state.entities[index].motion_phase = 0;
            return true;
        }
        if unit.speed == 0 {
            return false;
        }
        let obstacles = self.obstacles(actor.id);
        let changed = actor.target != Some(target);
        if changed {
            let actor = &mut self.state.entities[index];
            if actor.target.is_none() {
                actor.motion_speed = 0;
                actor.motion_phase = 0;
            }
            actor.path.clear();
            actor.path_retry = self.state.tick;
            actor.route_wait = None;
        }
        self.state.entities[index].target = Some(target);
        self.validate_route_geometry(index, &unit);
        let actor = &self.state.entities[index];
        if actor.path.is_empty() && self.state.tick >= actor.path_retry {
            let search = if allow_near {
                find_path_near
            } else {
                find_path
            };
            let route = search(
                &self.map,
                unit.footprint,
                unit.movement_class,
                actor.position,
                target,
                &self.navigation_geometry,
            );
            let empty = route.as_ref().is_some_and(Vec::is_empty);
            let actor = &mut self.state.entities[index];
            actor.path = route.unwrap_or_default().into();
            actor.path_geometry = self.navigation_geometry_hash;
            actor.path_retry = Tick(self.state.tick.0.saturating_add(PATH_RETRY_TICKS));
            if empty {
                actor.motion_speed = 0;
                actor.motion_phase = 0;
                if allow_near && !self.near_arrival(index, target) {
                    return false;
                }
                let actor = &mut self.state.entities[index];
                actor.target = None;
                actor.motion_speed = 0;
                actor.motion_phase = 0;
                return true;
            }
        }
        self.consider_route_detour(index, &unit, target, allow_near, &obstacles);
        let effect_speed = self.effect_speed_percent(&self.state.entities[index]);
        let actor = &mut self.state.entities[index];
        let endpoint = actor.path.back().copied();
        let mut budget = if actor.path.is_empty() {
            actor.motion_speed = 0;
            actor.motion_phase = 0;
            0
        } else if let Some(motion) = &unit.motion {
            if motion.steps.is_empty() {
                actor.motion_speed = if motion.acceleration == 0 {
                    motion.speed
                } else {
                    actor
                        .motion_speed
                        .saturating_add(motion.acceleration)
                        .min(motion.speed)
                };
            } else {
                actor.motion_speed =
                    u32::from(motion.steps[actor.motion_phase as usize % motion.steps.len()]) * 256;
                actor.motion_phase = (actor.motion_phase + 1) % motion.steps.len() as u32;
            }
            i64::from(actor.motion_speed) * if actor.stim_remaining > 0 { 3 } else { 2 } / 2
        } else {
            i64::from(unit.speed) * 256
        };
        budget = budget * i64::from(effect_speed) / 100;
        // Consume one scalar distance budget across route corners. Keep the
        // fractional position so diagonal travel and small accelerations cannot
        // gain speed or lose a fraction of a pixel on every simulation tick.
        while budget > 0
            && let Some(next) = actor.path.front().copied()
        {
            let from = [
                i64::from(actor.position.x) * 256 + i64::from(actor.motion_fraction[0]),
                i64::from(actor.position.y) * 256 + i64::from(actor.motion_fraction[1]),
            ];
            let delta = [
                i64::from(next.x) * 256 - from[0],
                i64::from(next.y) * 256 - from[1],
            ];
            let square = (delta[0] * delta[0] + delta[1] * delta[1]) as u64;
            let root = square.isqrt();
            let length = (root + u64::from(root * root != square)) as i64;
            let spent = budget.min(length);
            let precise = if spent == length {
                [i64::from(next.x) * 256, i64::from(next.y) * 256]
            } else {
                [
                    from[0] + delta[0] * spent / length,
                    from[1] + delta[1] * spent / length,
                ]
            };
            let step = Position {
                x: (precise[0] / 256) as i32,
                y: (precise[1] / 256) as i32,
            };
            if segment_clear(
                &self.map,
                unit.footprint,
                unit.movement_class,
                actor.position,
                step,
                &obstacles,
            ) {
                actor.position = step;
                actor.motion_fraction = [(precise[0] % 256) as i32, (precise[1] % 256) as i32];
                budget -= spent;
                if spent == length {
                    actor.path.pop_front();
                }
            } else {
                // Preserve the route and check it again next tick. Only taking
                // a longer alternative has to wait; physical clearance does not.
                if actor.route_wait.is_none() {
                    actor.route_wait = Some(RouteWait {
                        since: self.state.tick,
                        origin: actor.position,
                        alternate: VecDeque::new(),
                        ready_at: self.state.tick,
                    });
                    actor.path_retry = self.state.tick;
                }
                actor.motion_speed = 0;
                actor.motion_phase = 0;
                break;
            }
        }
        if actor.path.is_empty()
            && (actor.position == target || (allow_near && endpoint == Some(actor.position)))
        {
            if actor.position != target && !self.near_arrival(index, target) {
                let actor = &mut self.state.entities[index];
                actor.motion_speed = 0;
                actor.motion_phase = 0;
                actor.path_retry = Tick(self.state.tick.0.saturating_add(PATH_RETRY_TICKS));
                return false;
            }
            let actor = &mut self.state.entities[index];
            actor.target = None;
            actor.route_wait = None;
            actor.path.clear();
            actor.motion_speed = 0;
            actor.motion_phase = 0;
            true
        } else {
            false
        }
    }
    pub(in crate::sim) fn approach(
        &mut self,
        index: usize,
        position: Position,
        footprint: Footprint,
        reach: u32,
    ) -> bool {
        let actor = &self.state.entities[index];
        let unit = self.unit_at(index);
        if in_range(actor.position, unit.footprint, position, footprint, reach) {
            self.state.entities[index].target = None;
            self.state.entities[index].path.clear();
            self.state.entities[index].route_wait = None;
            self.state.entities[index].motion_speed = 0;
            self.state.entities[index].motion_phase = 0;
            return true;
        }
        if unit.speed == 0 {
            return false;
        }
        let mut candidates = perimeter(position, footprint, unit.footprint, actor.position);
        candidates.retain(|target| {
            self.can_place(*target, unit.footprint, unit.movement_class, Some(actor.id))
        });
        // Retain a valid route; when blocked, try the other sides in stable
        // distance order. A reachable far edge must not be hidden by a wall at
        // the closest edge. Failed searches retry at a fixed tick interval.
        if let Some(target) = actor.target.filter(|target| candidates.contains(target))
            && (!actor.path.is_empty() || self.state.tick < actor.path_retry)
        {
            self.navigate(index, target, false);
            return false;
        }
        if self.state.tick < actor.path_retry && actor.path.is_empty() {
            return false;
        }
        let footprint = unit.footprint;
        let class = unit.movement_class;
        let start = actor.position;
        let obstacles = &self.navigation_geometry;
        if let Some((target, path)) = crate::path::find_path_to_any(
            &self.map,
            footprint,
            class,
            start,
            &candidates,
            obstacles,
        ) {
            let actor = &mut self.state.entities[index];
            actor.target = Some(target);
            actor.path = path.into();
            actor.path_geometry = self.navigation_geometry_hash;
            actor.route_wait = None;
            actor.path_retry = Tick(self.state.tick.0.saturating_add(PATH_RETRY_TICKS));
            self.navigate(index, target, false);
            return false;
        }
        let actor = &mut self.state.entities[index];
        actor.path.clear();
        actor.target = None;
        actor.motion_speed = 0;
        actor.motion_phase = 0;
        actor.path_retry = Tick(self.state.tick.0.saturating_add(PATH_RETRY_TICKS));
        false
    }
}
