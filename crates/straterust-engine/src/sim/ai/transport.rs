//! Carry scripted ground attack parties across disconnected terrain.
use super::*;
use crate::map::MovementClass;

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
        self.rules
            .units
            .iter()
            .filter(|u| !u.structure && u.movement_class == MovementClass::Air)
            .filter(|u| self.creation_allowed(controller.player, u.id))
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
        for &id in wave {
            let passenger = &self.state.entities[self.index(id).unwrap()];
            let unit = self.unit_type(passenger.unit_type).unwrap();
            if unit.movement_class != MovementClass::Ground {
                continue;
            }
            let Some((_, free, passengers)) = ships
                .iter_mut()
                .find(|(_, free, _)| *free >= unit.cargo_size)
            else {
                return false;
            };
            *free -= unit.cargo_size;
            passengers.push(id);
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

    fn ai_drop_point(&self, ship: EntityId, target: Position) -> Option<Position> {
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

    pub(super) fn ai_transports(&mut self, controller: &AiController, state: &mut AiState) {
        let ships: Vec<_> = state.transports.keys().copied().collect();
        for ship in ships {
            let Some(index) = self.index(ship) else {
                state.transports.remove(&ship);
                continue;
            };
            let mut trip = state.transports[&ship].clone();
            trip.passengers.retain(|id| self.index(*id).is_some());
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
