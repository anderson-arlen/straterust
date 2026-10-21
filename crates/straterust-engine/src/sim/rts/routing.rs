//! Route retention and distance-proportional waiting for temporary traffic.
use super::*;

const DETOUR_WAIT_MS: u64 = 100;

impl World {
    pub(in crate::sim) fn navigator(&self) -> crate::path::Pathfinder<'_> {
        crate::path::Pathfinder::new(&self.map, &self.navigation_geometry, &self.navigation_data)
    }

    pub(in crate::sim) fn refresh_navigation_geometry(&mut self) {
        self.navigation_geometry = self.static_navigation_obstacles();
        let mut hash = blake3::Hasher::new();
        for obstacle in &self.navigation_geometry {
            hash.update(&obstacle.position.x.to_le_bytes());
            hash.update(&obstacle.position.y.to_le_bytes());
            hash.update(&obstacle.footprint.width.to_le_bytes());
            hash.update(&obstacle.footprint.height.to_le_bytes());
            hash.update(&[match obstacle.movement_class {
                MovementClass::Ground => 0,
                MovementClass::Air => 1,
                MovementClass::Water => 2,
            }]);
        }
        let fingerprint = *hash.finalize().as_bytes();
        if fingerprint != self.navigation_geometry_hash {
            self.navigation_data = Arc::default();
        }
        self.navigation_geometry_hash = fingerprint;
    }

    pub(super) fn validate_route_geometry(&mut self, index: usize, unit: &UnitType) {
        let actor = &self.state.entities[index];
        if actor.path_geometry == self.navigation_geometry_hash {
            return;
        }
        let clear = route_clear(
            &self.map,
            unit,
            actor.position,
            &actor.path,
            &self.navigation_geometry,
        );
        // Removing a building can also reopen a direct route. Keep unrelated
        // valid routes, rather than rerunning every search after every change.
        let shortcut = actor.target.is_some_and(|end| {
            (actor.path.back() != Some(&end)
                || route_cost(actor.position, actor.path.iter().copied())
                    > travel_cost(actor.position, end))
                && segment_clear(
                    &self.map,
                    unit.footprint,
                    unit.movement_class,
                    actor.position,
                    end,
                    &self.navigation_geometry,
                )
        });
        let actor = &mut self.state.entities[index];
        actor.path_geometry = self.navigation_geometry_hash;
        if !clear || shortcut {
            actor.path.clear();
            actor.route_wait = None;
            actor.path_retry = self.state.tick;
        }
    }

    pub(super) fn consider_route_detour(
        &mut self,
        index: usize,
        unit: &UnitType,
        target: Position,
        allow_near: bool,
        obstacles: &[Obstacle],
    ) {
        let actor = &self.state.entities[index];
        if actor.path.front().is_none_or(|&next| {
            segment_clear(
                &self.map,
                unit.footprint,
                unit.movement_class,
                actor.position,
                next,
                obstacles,
            )
        }) {
            self.state.entities[index].route_wait = None;
            return;
        }
        if actor.route_wait.is_none() {
            let actor = &mut self.state.entities[index];
            actor.route_wait = Some(RouteWait {
                since: self.state.tick,
                origin: actor.position,
                alternate: VecDeque::new(),
                ready_at: self.state.tick,
            });
            actor.path_retry = self.state.tick;
        }
        let actor = &self.state.entities[index];
        let wait = actor.route_wait.as_ref().expect("blocked route");
        let Some(endpoint) = actor.path.back().copied() else {
            self.state.entities[index].route_wait = None;
            return;
        };
        // Nearby arrival points may become occupied while travelling. Always
        // find their replacements relative to the original requested goal.
        let goal = if allow_near { target } else { endpoint };
        let since = wait.since;
        if self.state.tick >= actor.path_retry
            || wait.origin != actor.position
                && (!first_step_clear(&self.map, unit, actor, &actor.path, obstacles)
                    || !wait.alternate.is_empty() && self.state.tick >= wait.ready_at)
        {
            let start = actor.position;
            let ideal = self.navigator().route_estimate(
                unit.footprint,
                unit.movement_class,
                start,
                goal,
                allow_near,
            );
            let (ideal_travel, ideal_end) =
                ideal.unwrap_or_else(|| (route_cost(start, actor.path.iter().copied()), endpoint));
            let ideal_cost = ideal_travel + travel_cost(ideal_end, goal);
            // Traffic may justify a local detour, not a tour of the map. Allow
            // twice the static route or eight extra cells, whichever is longer.
            // Even fine grids leave room for two body widths around traffic.
            // Permanent terrain remains part of the unrestricted ideal route.
            let cell = self
                .map
                .terrain
                .as_ref()
                .map_or(8, |t| u64::from(t.cell_size));
            let allowance =
                (cell * 8).max(u64::from(unit.footprint.width.max(unit.footprint.height)) * 2);
            let limit = ideal_travel.saturating_add(ideal_travel.max(allowance * 10));
            let data = crate::path::NavigationData::default();
            let traffic =
                crate::path::Pathfinder::new(&self.map, obstacles, &data).with_cost_limit(limit);
            let alternate = if allow_near {
                traffic.find_path_near(unit.footprint, unit.movement_class, start, goal)
            } else {
                traffic.find_path(unit.footprint, unit.movement_class, start, goal)
            }
            .and_then(|mut route| {
                let end = route.last().copied().unwrap_or(start);
                if end == goal
                    || allow_near
                        && (travel_cost(end, goal) < travel_cost(start, goal)
                            || self.near_arrival_from(index, end, target))
                {
                    if route.is_empty() {
                        route.push(start);
                    }
                    Some(route)
                } else {
                    None
                }
            });
            let mut ready_at = since;
            let alternate: VecDeque<_> = alternate.unwrap_or_default().into();
            if let Some(&end) = alternate.back() {
                // Compare the same destination even when an occupied goal
                // requires the alternate to stop at an adjacent free point.
                let cost = route_cost(start, alternate.iter().copied()) + travel_cost(end, goal);
                let ticks = detour_wait_ticks(ideal_cost, cost, self.rules.tick_ms);
                ready_at = Tick(since.0.saturating_add(ticks));
            }
            let actor = &mut self.state.entities[index];
            actor.route_wait = Some(RouteWait {
                since,
                origin: start,
                alternate,
                ready_at,
            });
            actor.path_retry = Tick(self.state.tick.0.saturating_add(PATH_RETRY_TICKS));
        }
        let actor = &self.state.entities[index];
        let wait = actor.route_wait.as_ref().expect("retained collision");
        if !wait.alternate.is_empty()
            && self.state.tick >= wait.ready_at
            && first_step_clear(&self.map, unit, actor, &wait.alternate, obstacles)
        {
            let actor = &mut self.state.entities[index];
            actor.path = actor.route_wait.take().unwrap().alternate;
        }
    }
}

fn detour_wait_ticks(ideal: u64, alternate: u64, tick_ms: u32) -> u64 {
    let milliseconds = alternate
        .saturating_sub(ideal)
        .saturating_mul(DETOUR_WAIT_MS)
        .div_ceil(ideal.max(1));
    milliseconds.div_ceil(u64::from(tick_ms))
}

fn travel_cost(a: Position, b: Position) -> u64 {
    let dx = (i64::from(a.x) - i64::from(b.x)).unsigned_abs();
    let dy = (i64::from(a.y) - i64::from(b.y)).unsigned_abs();
    10 * dx.max(dy) + 4 * dx.min(dy)
}

fn route_cost(start: Position, route: impl IntoIterator<Item = Position>) -> u64 {
    let mut previous = start;
    route
        .into_iter()
        .map(|point| {
            let cost = travel_cost(previous, point);
            previous = point;
            cost
        })
        .sum()
}

fn route_clear(
    map: &Map,
    unit: &UnitType,
    start: Position,
    route: &VecDeque<Position>,
    obstacles: &[Obstacle],
) -> bool {
    let mut previous = start;
    route.iter().all(|&point| {
        let clear = segment_clear(
            map,
            unit.footprint,
            unit.movement_class,
            previous,
            point,
            obstacles,
        );
        previous = point;
        clear
    })
}

fn first_step_clear(
    map: &Map,
    unit: &UnitType,
    actor: &Entity,
    route: &VecDeque<Position>,
    obstacles: &[Obstacle],
) -> bool {
    let from = [
        i64::from(actor.position.x) * 256 + i64::from(actor.motion_fraction[0]),
        i64::from(actor.position.y) * 256 + i64::from(actor.motion_fraction[1]),
    ];
    let Some(next) = route
        .iter()
        .find(|point| [i64::from(point.x) * 256, i64::from(point.y) * 256] != from)
    else {
        return true;
    };
    let budget = if let Some(motion) = &unit.motion {
        let speed = if motion.steps.is_empty() {
            if motion.acceleration == 0 {
                motion.speed
            } else {
                actor
                    .motion_speed
                    .saturating_add(motion.acceleration)
                    .min(motion.speed)
            }
        } else {
            u32::from(motion.steps[actor.motion_phase as usize % motion.steps.len()]) * 256
        };
        i64::from(speed) * if actor.stim_remaining > 0 { 3 } else { 2 } / 2
    } else {
        i64::from(unit.speed) * 256
    };
    let delta = [
        i64::from(next.x) * 256 - from[0],
        i64::from(next.y) * 256 - from[1],
    ];
    let square = (delta[0] * delta[0] + delta[1] * delta[1]) as u64;
    let root = square.isqrt();
    let length = (root + u64::from(root * root != square)) as i64;
    let spent = budget.min(length);
    let step = Position {
        x: ((from[0] + delta[0] * spent / length) / 256) as i32,
        y: ((from[1] + delta[1] * spent / length) / 256) as i32,
    };
    segment_clear(
        map,
        unit.footprint,
        unit.movement_class,
        actor.position,
        step,
        obstacles,
    )
}

#[cfg(test)]
mod tests;
