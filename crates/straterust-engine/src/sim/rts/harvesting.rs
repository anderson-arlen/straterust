//! Exclusive outdoor resource access points and their approach routes.
use super::*;

impl World {
    pub(super) fn redistribute_gather(
        &mut self,
        index: usize,
        resource: ResourceId,
        prefer_current: bool,
    ) -> ResourceId {
        let Some(node) = self.state.resources.iter().find(|node| node.id == resource) else {
            return resource;
        };
        let radius = self
            .unit_at(index)
            .worker
            .as_ref()
            .expect("gathering worker")
            .idle_resource_radius;
        if node.requires_extractor || radius == 0 {
            return resource;
        }
        let origin = self.state.entities[index]
            .gather_origin
            .unwrap_or(node.position);
        self.state.entities[index].gather_origin = Some(origin);
        let actor = &self.state.entities[index];
        let radius_squared = i64::from(radius) * i64::from(radius);
        let mut candidates: Vec<_> = self
            .state
            .resources
            .iter()
            .filter(|other| {
                other.kind == node.kind
                    && !other.requires_extractor
                    && other.amount > 0
                    && distance(origin, other.position) <= radius_squared
                    && self.map.height_at(other.position) == self.map.height_at(actor.position)
                    && self.visibility(actor.owner, other.position) != Visibility::Unexplored
                    && !self.state.entities.iter().any(|worker| {
                        worker.id != actor.id
                            && worker.order == (UnitOrder::Gather { resource: other.id })
                            && (worker.harvest_spot.is_some()
                                || worker.harvest_waiting_since.is_some()
                                || worker.harvest_progress > 0)
                    })
            })
            .cloned()
            .collect();
        candidates.sort_by_key(|node| {
            (
                prefer_current && node.id != resource,
                distance(actor.position, node.position),
                node.id,
            )
        });
        for node in candidates {
            if node.id == resource && self.state.entities[index].harvest_spot.is_some() {
                return resource;
            }
            if let Some((spot, path)) = self.resource_route(index, &node) {
                if node.id != resource {
                    self.assign(index, UnitOrder::Gather { resource: node.id }, false);
                    self.state.entities[index].gather_origin = Some(origin);
                }
                // Claim the patch in the same tick as choosing it, before the
                // next worker searches. Inbound claims prevent another queue.
                self.reserve_resource_route(index, spot, path);
                return node.id;
            }
        }
        resource
    }

    pub(super) fn add_harvest_spot_obstacles(
        &self,
        except: EntityId,
        obstacles: &mut Vec<Obstacle>,
    ) {
        obstacles.extend(self.state.entities.iter().filter_map(|entity| {
            let position = entity.harvest_spot.filter(|_| entity.id != except)?;
            Some(Obstacle {
                position,
                footprint: self.unit_type(entity.unit_type)?.footprint,
                movement_class: self.movement_class(entity),
            })
        }));
    }

    pub(super) fn approach_resource(&mut self, index: usize, node: &ResourceNode) -> bool {
        let actor = &self.state.entities[index];
        let unit = self.unit_at(index);
        let mut obstacles = self.navigation_geometry.clone();
        self.add_harvest_spot_obstacles(actor.id, &mut obstacles);
        if actor.harvest_spot.is_some_and(|spot| {
            !segment_clear(
                &self.map,
                unit.footprint,
                unit.movement_class,
                spot,
                spot,
                &obstacles,
            )
        }) {
            let actor = &mut self.state.entities[index];
            actor.harvest_spot = None;
            actor.harvest_waiting_since = None;
            actor.harvest_progress = 0;
            actor.target = None;
            actor.path.clear();
            actor.route_wait = None;
            actor.path_retry = self.state.tick;
        }
        let actor = &self.state.entities[index];
        if actor.harvest_spot.is_none() {
            if self.state.tick < actor.path_retry {
                return false;
            }
            let route = self.resource_route(index, node);
            let actor = &mut self.state.entities[index];
            actor.path_retry = Tick(self.state.tick.0.saturating_add(PATH_RETRY_TICKS));
            let Some((spot, path)) = route else {
                actor.motion_speed = 0;
                actor.motion_phase = 0;
                return false;
            };
            self.reserve_resource_route(index, spot, path);
        }
        let spot = self.state.entities[index]
            .harvest_spot
            .expect("reserved edge");
        let retry_due = self.state.tick >= self.state.entities[index].path_retry;
        if self.navigate(index, spot, false) {
            let actor = &self.state.entities[index];
            // Mobile phasing is still useful en route. It must never authorize
            // harvesting while another visible body occupies the stopping spot.
            return self.can_place(
                spot,
                self.unit_at(index).footprint,
                self.movement_class(actor),
                Some(actor.id),
            );
        }
        if retry_due && self.state.entities[index].path.is_empty() {
            // A building/terrain change can disconnect a previously valid edge.
            // Try the remaining edges on the next normal retry.
            self.state.entities[index].harvest_spot = None;
        }
        false
    }

    fn resource_route(
        &self,
        index: usize,
        node: &ResourceNode,
    ) -> Option<(Position, Vec<Position>)> {
        let actor = &self.state.entities[index];
        let unit = self.unit_at(index);
        let mut obstacles = self.navigation_geometry.clone();
        self.add_harvest_spot_obstacles(actor.id, &mut obstacles);
        let mut candidates = perimeter(
            node.position,
            node.footprint,
            unit.footprint,
            actor.position,
        );
        // Preserve existing free, off-grid stopping points beside the resource.
        if in_range(
            actor.position,
            unit.footprint,
            node.position,
            node.footprint,
            1,
        ) {
            candidates.insert(0, actor.position);
        }
        candidates.retain(|&spot| {
            self.can_place(spot, unit.footprint, unit.movement_class, Some(actor.id))
                && segment_clear(
                    &self.map,
                    unit.footprint,
                    unit.movement_class,
                    spot,
                    spot,
                    &obstacles,
                )
        });
        resource_approach_route(
            &self.map,
            unit,
            actor.position,
            node,
            &candidates,
            &obstacles,
        )
    }

    fn reserve_resource_route(&mut self, index: usize, spot: Position, path: Vec<Position>) {
        let actor = &mut self.state.entities[index];
        actor.harvest_spot = Some(spot);
        actor.target = Some(spot);
        actor.path = path.into();
        actor.path_geometry = self.navigation_geometry_hash;
        actor.path_retry = Tick(self.state.tick.0.saturating_add(PATH_RETRY_TICKS));
        actor.route_wait = None;
    }
}

fn resource_approach_route(
    map: &Map,
    unit: &UnitType,
    start: Position,
    node: &ResourceNode,
    spots: &[Position],
    obstacles: &[Obstacle],
) -> Option<(Position, Vec<Position>)> {
    if spots.contains(&start) {
        return Some((start, Vec::new()));
    }
    let [left, top, right, bottom] = node.footprint.bounds(node.position);
    let mut approaches = Vec::new();
    for &spot in spots {
        if segment_clear(
            map,
            unit.footprint,
            unit.movement_class,
            start,
            spot,
            obstacles,
        ) {
            approaches.push((spot, spot));
            continue;
        }
        let [l, t, r, b] = unit.footprint.bounds(spot);
        let mut entries = Vec::new();
        if r <= left {
            entries.push(Position {
                x: spot.x - i32::from(unit.footprint.width),
                ..spot
            });
        }
        if l >= right {
            entries.push(Position {
                x: spot.x + i32::from(unit.footprint.width),
                ..spot
            });
        }
        if b <= top {
            entries.push(Position {
                y: spot.y - i32::from(unit.footprint.height),
                ..spot
            });
        }
        if t >= bottom {
            entries.push(Position {
                y: spot.y + i32::from(unit.footprint.height),
                ..spot
            });
        }
        entries.sort_by_key(|point| (distance(*point, start), point.y, point.x));
        // Grid centers need not line up with packed edge slots. Reach a point
        // outside the row first, then enter perpendicular to the resource edge.
        for entry in entries {
            if segment_clear(
                map,
                unit.footprint,
                unit.movement_class,
                entry,
                spot,
                obstacles,
            ) {
                approaches.push((entry, spot));
            }
        }
    }
    let entries: Vec<_> = approaches.iter().map(|(entry, _)| *entry).collect();
    let (entry, mut path) = crate::path::find_path_to_any(
        map,
        unit.footprint,
        unit.movement_class,
        start,
        &entries,
        obstacles,
    )?;
    let spot = approaches.iter().find(|(point, _)| *point == entry)?.1;
    if spot != entry {
        path.push(spot);
    }
    Some((spot, path))
}
