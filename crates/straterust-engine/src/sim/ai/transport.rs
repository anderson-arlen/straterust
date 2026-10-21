//! Carry scripted ground attack parties across disconnected terrain.
use super::*;
use crate::map::MovementClass;

// Rules and terrain are immutable within a World. Landing validity depends on
// the carrier, its position, destination and the types currently aboard; mobile
// occupants are deliberately left to ordinary unload validation on arrival.
pub(in crate::sim) type LandingKey = (UnitTypeId, Position, Position, Vec<UnitTypeId>);
const LANDING_CACHE_LIMIT: usize = 128;

#[cfg(test)]
mod tests;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiTransport {
    pub passengers: Vec<EntityId>,
    /// Fixed attack destination; never follows a concealed enemy.
    pub target: Position,
    pub departed: bool,
}

impl World {
    pub(super) fn ai_needs_transport(&self, controller: &AiController, state: &AiState) -> bool {
        let Some(target) = self.ai_target(controller) else {
            return false;
        };
        // One static probe at preparation, not a full-map search every AI tick.
        let ground = state
            .attack
            .keys()
            .filter_map(|id| self.unit_type(*id))
            .filter(|u| u.movement_class == MovementClass::Ground)
            .max_by_key(|u| u.footprint.width.max(u.footprint.height));
        ground.is_some_and(|unit| {
            crate::path::find_path(
                &self.map,
                unit.footprint,
                MovementClass::Ground,
                controller.home,
                target,
                &[],
            )
            .is_none()
        })
    }

    fn ai_transport_type(&self, controller: &AiController, state: &AiState) -> Option<UnitTypeId> {
        let producible = self.ai_production_types(controller.player);
        self.rules
            .units
            .iter()
            .filter(|u| !u.structure && u.movement_class != MovementClass::Ground)
            .filter(|u| producible.contains(&u.id))
            .find(|u| {
                u.garrison.as_ref().is_some_and(|g| {
                    state
                        .attack
                        .keys()
                        .filter(|id| {
                            self.unit_type(**id).unwrap().movement_class == MovementClass::Ground
                        })
                        .all(|id| g.passengers.contains(id))
                })
            })
            .map(|u| u.id)
    }

    pub(super) fn ai_request_transports(
        &self,
        controller: &AiController,
        state: &AiState,
        requests: &mut Vec<AiRequest>,
    ) {
        if !state.prepared || !state.transport_needed {
            return;
        }
        let Some(unit_type) = self.ai_transport_type(controller, state) else {
            return;
        };
        let capacity = u32::from(
            self.unit_type(unit_type)
                .unwrap()
                .garrison
                .as_ref()
                .unwrap()
                .capacity,
        );
        let space: u32 = state
            .attack
            .iter()
            .filter_map(|(id, count)| {
                let unit = self.unit_type(*id).unwrap();
                (unit.movement_class == MovementClass::Ground)
                    .then_some(u32::from(unit.cargo_size) * u32::from(*count))
            })
            .sum();
        let count = space.div_ceil(capacity).min(4096) as u16;
        if let Some(request) = requests.iter_mut().find(|r| r.unit_type == unit_type) {
            request.count = request.count.max(count);
        } else {
            requests.insert(
                0,
                AiRequest {
                    unit_type,
                    count,
                    priority: 90,
                    include_deployed: false,
                },
            );
        }
    }

    pub(super) fn ai_load_wave(
        &mut self,
        controller: &AiController,
        state: &mut AiState,
        wave: &[EntityId],
        target: Position,
        partial: bool,
    ) -> bool {
        let Some(unit_type) = self.ai_transport_type(controller, state) else {
            return false;
        };
        let capacity = self
            .unit_type(unit_type)
            .unwrap()
            .garrison
            .as_ref()
            .unwrap()
            .capacity;
        let mut ships: Vec<_> = self
            .state
            .entities
            .iter()
            .filter(|e| {
                e.owner == controller.player
                    && e.unit_type == unit_type
                    && e.hp > 0
                    && e.construction.is_none()
                    && !state.deployed.contains(&e.id)
                    && !self.state.ai.iter().any(|s| s.deployed.contains(&e.id))
                    && !self
                        .state
                        .entities
                        .iter()
                        .any(|p| p.garrisoned_in == Some(e.id))
            })
            .map(|e| (e.id, capacity, Vec::new()))
            .collect();
        let mut boarding = wave.to_vec();
        if partial {
            boarding.sort_by_key(|id| {
                let at = self.state.entities[self.index(*id).unwrap()].position;
                (
                    ships
                        .iter()
                        .map(|(ship, _, _)| {
                            rts::distance(
                                at,
                                self.state.entities[self.index(*ship).unwrap()].position,
                            )
                        })
                        .min()
                        .unwrap_or(i64::MAX),
                    *id,
                )
            });
        }
        for id in boarding {
            let passenger = &self.state.entities[self.index(id).unwrap()];
            let unit = self.unit_type(passenger.unit_type).unwrap();
            if unit.movement_class != MovementClass::Ground {
                continue;
            }
            let Some((_, free, passengers)) = ships
                .iter_mut()
                .find(|(_, free, _)| *free >= unit.cargo_size)
            else {
                if partial {
                    continue;
                }
                return false;
            };
            *free -= unit.cargo_size;
            passengers.push(id);
        }
        if ships.iter().all(|(_, _, passengers)| passengers.is_empty()) {
            return false;
        }
        for (ship, _, passengers) in ships.into_iter().filter(|(_, _, p)| !p.is_empty()) {
            for &entity in &passengers {
                if !self.ai_order(
                    controller,
                    state,
                    Order::Load {
                        entity,
                        target: ship,
                    },
                ) {
                    return false;
                }
                state.deployed.insert(entity);
            }
            state.deployed.insert(ship);
            state.transports.insert(
                ship,
                AiTransport {
                    passengers,
                    target,
                    departed: false,
                },
            );
        }
        true
    }

    fn ai_drop_point(&mut self, ship: EntityId, target: Position) -> Option<Position> {
        let unit = self.unit_at(self.index(ship)?);
        if unit.movement_class == MovementClass::Water {
            let mut passengers: Vec<_> = self
                .state
                .entities
                .iter()
                .filter(|e| e.garrisoned_in == Some(ship))
                .map(|e| e.unit_type)
                .collect();
            passengers.sort_unstable();
            passengers.dedup();
            let key = (
                unit.id,
                self.state.entities[self.index(ship)?].position,
                target,
                passengers,
            );
            if let Some(point) = self.ai_drop_points.get(&key) {
                return *point;
            }
            let point = self.ai_water_drop_point(ship, target);
            if self.ai_drop_points.len() >= LANDING_CACHE_LIMIT {
                self.ai_drop_points.clear();
            }
            self.ai_drop_points.insert(key, point);
            return point;
        }
        // Drop outside the known base center. Do not inspect hidden occupants
        // to choose a magically clear landing zone; ordinary unload retries apply.
        for ring in 4_i32..=8 {
            for dy in -ring..=ring {
                for dx in -ring..=ring {
                    if dx.abs() != ring && dy.abs() != ring {
                        continue;
                    }
                    let point = Position {
                        x: target.x + dx * 32,
                        y: target.y + dy * 32,
                    };
                    if self.map.height_at(point) == self.map.height_at(target)
                        && self.unload_at_rejection(ship, point).is_none()
                    {
                        return Some(point);
                    }
                }
            }
        }
        None
    }

    fn ai_water_drop_point(&self, ship: EntityId, target: Position) -> Option<Position> {
        let actor = &self.state.entities[self.index(ship)?];
        let unit = self.unit_at(self.index(ship)?);
        let boarding = self.boarding_footprint(ship);
        let passengers: Vec<_> = self
            .state
            .entities
            .iter()
            .filter(|e| e.garrisoned_in == Some(ship))
            .filter_map(|e| self.unit_type(e.unit_type))
            .collect();
        let terrain = self.map.terrain.as_ref()?;
        let cell = terrain.cell_size as i32;
        let mut coast: Vec<_> = (0..terrain.rows)
            .flat_map(|y| {
                (0..terrain.columns).map(move |x| Position {
                    x: x as i32 * cell + cell / 2,
                    y: y as i32 * cell + cell / 2,
                })
            })
            .filter(|&p| {
                self.map.can_move(p, unit.footprint, unit.movement_class)
                    && passengers.iter().all(|passenger| {
                        rts::perimeter(p, boarding, passenger.footprint, p)
                            .into_iter()
                            .any(|exit| {
                                self.map.can_move(
                                    exit,
                                    passenger.footprint,
                                    passenger.movement_class,
                                )
                            })
                    })
            })
            .collect();
        coast.sort_by_key(|p| (rts::distance(*p, target), p.x, p.y));
        let ground = Footprint {
            width: 24,
            height: 24,
        };
        let data = crate::path::NavigationData::default();
        let finder = crate::path::Pathfinder::new(&self.map, &[], &data);
        coast.into_iter().find(|p| {
            let exit = rts::perimeter(*p, boarding, ground, target)
                .into_iter()
                .find(|&e| self.map.can_move(e, ground, MovementClass::Ground));
            exit.is_some_and(|exit| {
                finder
                    .find_path(ground, MovementClass::Ground, exit, target)
                    .is_some()
            }) && finder
                .find_path(unit.footprint, MovementClass::Water, actor.position, *p)
                .is_some()
        })
    }

    pub(super) fn ai_transports(&mut self, controller: &AiController, state: &mut AiState) {
        let ships: Vec<_> = state.transports.keys().copied().collect();
        for ship in ships {
            let Some(index) = self.index(ship) else {
                state.transports.remove(&ship);
                continue;
            };
            let mut trip = state.transports[&ship].clone();
            trip.passengers.retain(|id| self.index(*id).is_some());
            if controller.takeover && !trip.departed {
                let blocked: Vec<_> = trip
                    .passengers
                    .iter()
                    .copied()
                    .filter(|id| {
                        let passenger = &self.state.entities[self.index(*id).unwrap()];
                        passenger.garrisoned_in.is_none()
                            && passenger.route_wait.as_ref().is_some_and(|w| {
                                self.tick().0.saturating_sub(w.since.0)
                                    * u64::from(self.rules.tick_ms)
                                    >= 5000
                            })
                    })
                    .collect();
                for id in blocked {
                    trip.passengers.retain(|p| *p != id);
                    state.deployed.remove(&id);
                    self.ai_order(controller, state, Order::Stop { entity: id });
                }
            }
            if trip.departed {
                for &id in &trip.passengers {
                    let passenger = &self.state.entities[self.index(id).unwrap()];
                    if passenger.garrisoned_in.is_none()
                        && matches!(passenger.order, UnitOrder::Idle)
                    {
                        self.ai_order(
                            controller,
                            state,
                            Order::AttackMove {
                                entity: id,
                                target: trip.target,
                            },
                        );
                    }
                }
            }
            if trip.passengers.is_empty()
                || (trip.departed
                    && trip.passengers.iter().all(|id| {
                        self.state.entities[self.index(*id).unwrap()].garrisoned_in != Some(ship)
                    }))
            {
                state.transports.remove(&ship);
                state.deployed.remove(&ship);
                self.ai_order(
                    controller,
                    state,
                    Order::Move {
                        entity: ship,
                        target: controller.home,
                    },
                );
                continue;
            }
            if !trip.departed {
                let loaded = trip.passengers.iter().all(|id| {
                    self.state.entities[self.index(*id).unwrap()].garrisoned_in == Some(ship)
                });
                if loaded {
                    if let Some(target) = self.ai_drop_point(ship, trip.target)
                        && self.ai_order(
                            controller,
                            state,
                            Order::UnloadAt {
                                entity: ship,
                                target,
                            },
                        )
                    {
                        trip.departed = true;
                    }
                } else {
                    for &id in &trip.passengers {
                        let passenger = &self.state.entities[self.index(id).unwrap()];
                        if passenger.garrisoned_in.is_none()
                            && passenger.order != (UnitOrder::Load { target: ship })
                        {
                            self.ai_order(
                                controller,
                                state,
                                Order::Load {
                                    entity: id,
                                    target: ship,
                                },
                            );
                        }
                    }
                }
            } else if matches!(self.state.entities[index].order, UnitOrder::Idle)
                && self
                    .state
                    .entities
                    .iter()
                    .any(|e| e.garrisoned_in == Some(ship))
                && let Some(target) = self.ai_drop_point(ship, trip.target)
            {
                self.ai_order(
                    controller,
                    state,
                    Order::UnloadAt {
                        entity: ship,
                        target,
                    },
                );
            }
            state.transports.insert(ship, trip);
        }
    }
}
