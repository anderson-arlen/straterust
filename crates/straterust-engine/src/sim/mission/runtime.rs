use super::*;

impl World {
    pub(in crate::sim) fn advance_rescue(&mut self) {
        if !self.state.tick.0.is_multiple_of(2)
            || self.state.winner.is_some()
            || self
                .state
                .mission
                .as_ref()
                .is_some_and(|mission| mission.paused)
        {
            return;
        }
        let Some(mission) = &self.map.mission else {
            return;
        };
        let rescuable = self
            .state
            .mission
            .as_ref()
            .map_or_else(Vec::new, |state| state.rescue_players.clone());
        let rescuers = mission.rescuers.clone();
        let candidates: Vec<_> = self
            .state
            .entities
            .iter()
            .filter(|entity| rescuable.contains(&entity.owner))
            .map(|entity| entity.id)
            .collect();
        for id in candidates {
            let Some(index) = self
                .state
                .entities
                .iter()
                .position(|entity| entity.id == id && rescuable.contains(&entity.owner))
            else {
                continue;
            };
            let source = &self.state.entities[index];
            let area = MissionLocation {
                excluded_elevations: 0,
                left: source.position.x - 64,
                top: source.position.y - 64,
                right: source.position.x + 64,
                bottom: source.position.y + 64,
            };
            let Some(owner) = self
                .state
                .entities
                .iter()
                .filter(|entity| {
                    rescuers.contains(&entity.owner)
                        && entity.hp != 0
                        && entity.garrisoned_in.is_none()
                        && !entity.gathering_inside
                        && !self.unit_type(entity.unit_type).unwrap().revealer
                })
                .find(|entity| {
                    area.overlaps(
                        entity.position,
                        self.unit_type(entity.unit_type).unwrap().footprint,
                    )
                })
                .map(|entity| entity.owner)
            else {
                continue;
            };
            let old_owner = source.owner;
            let unit = self.unit_type(source.unit_type).unwrap();
            let all = source.construction.is_none()
                && !source.airborne
                && source.flight_transition == 0
                && unit.structure
                && !unit.dropoff.is_empty()
                && unit.movement_class == MovementClass::Ground;
            let transfers: Vec<_> = self
                .state
                .entities
                .iter()
                .enumerate()
                .filter(|(_, entity)| entity.id == id || (all && entity.owner == old_owner))
                .map(|(index, _)| index)
                .collect();
            for index in transfers {
                self.cancel_research(index);
                {
                    let entity = &mut self.state.entities[index];
                    entity.owner = owner;
                    entity.order = UnitOrder::Idle;
                    entity.target = None;
                    entity.path.clear();
                    entity.queued_orders.clear();
                    entity.dropoff_target = None;
                    entity.harvest_progress = 0;
                    entity.repair_progress = 0;
                    entity.patrol_origin = None;
                    entity.patrol_returning = false;
                }
            }
        }
    }

    pub(in crate::sim) fn advance_mission(&mut self) {
        if self.map.mission.is_none() || self.state.winner.is_some() {
            return;
        }
        let Some(mut state) = self.state.mission.take() else {
            return;
        };
        let mission = self.map.mission.as_ref().expect("mission definition");
        if self.state.defeated.contains(&mission.player) {
            self.state.mission = Some(state);
            return;
        }
        state.countdown_ms = state.countdown_ms.saturating_sub(mission.wait_step_ms);
        if let Some(wait) = &mut state.wait {
            if wait.remaining_ms < mission.wait_step_ms {
                state.triggers[usize::from(wait.trigger)].action += 1;
                state.wait = None;
                state.poll_remaining = 0;
            } else {
                wait.remaining_ms -= mission.wait_step_ms;
            }
        }
        if state.poll_remaining > 0 {
            state.poll_remaining -= 1;
            self.state.mission = Some(state);
            return;
        }
        state.poll_remaining = mission.poll_ticks - 1;
        // Retail doodad toggles issue an asynchronous unit order. Copies of an
        // All Players trigger in the same pass therefore request the same state,
        // rather than immediately opening and closing the door again.
        let mut toggled_doodads = BTreeSet::new();
        for index in 0..mission.triggers.len() {
            if state.triggers[index].complete {
                continue;
            }
            if !state.triggers[index].started {
                let ready = self.map.mission.as_ref().unwrap().triggers[index]
                    .conditions
                    .iter()
                    .all(|condition| self.mission_condition(condition, &state));
                if !ready {
                    continue;
                }
                state.triggers[index].started = true;
            }
            loop {
                let definition = &self.map.mission.as_ref().unwrap().triggers[index];
                let Some(action) = definition
                    .actions
                    .get(usize::from(state.triggers[index].action))
                    .cloned()
                else {
                    if state.triggers[index].preserve {
                        state.triggers[index].action = 0;
                        state.triggers[index].started = false;
                    } else {
                        state.triggers[index].complete = true;
                    }
                    break;
                };
                if !self.mission_action(action, index as u16, &mut state, &mut toggled_doodads) {
                    break;
                }
                state.triggers[index].action += 1;
                if self.state.winner.is_some()
                    || self
                        .state
                        .defeated
                        .contains(&self.map.mission.as_ref().unwrap().player)
                {
                    break;
                }
            }
            if self.state.winner.is_some()
                || self
                    .state
                    .defeated
                    .contains(&self.map.mission.as_ref().unwrap().player)
            {
                break;
            }
        }
        self.state.mission = Some(state);
    }

    pub(in crate::sim) fn mission_matches(
        &self,
        entity: &Entity,
        players: &[PlayerId],
        units: MissionUnits,
        location: Option<MissionLocation>,
    ) -> bool {
        players.contains(&entity.owner)
            && match units {
                MissionUnits::Men => {
                    !self.unit_type(entity.unit_type).unwrap().structure
                        && self.unit_type(entity.unit_type).unwrap().speed > 0
                        && !self.unit_type(entity.unit_type).unwrap().revealer
                }
                MissionUnits::Any => true,
                MissionUnits::Structures => self.unit_type(entity.unit_type).unwrap().structure,
                MissionUnits::Type(id) => entity.unit_type == id,
            }
            && location.is_none_or(|location| {
                location.excluded_elevations
                    & (1 << (self.map.height_at(entity.position).unwrap_or(0).min(2)
                        + if self.movement_class(entity) == MovementClass::Air {
                            0
                        } else {
                            3
                        }))
                    == 0
                    && location.overlaps(
                        entity.position,
                        self.unit_type(entity.unit_type).unwrap().footprint,
                    )
            })
    }
    pub(in crate::sim) fn mission_condition(
        &self,
        condition: &MissionCondition,
        state: &MissionState,
    ) -> bool {
        match condition {
            MissionCondition::Resources {
                players,
                kinds,
                comparison,
                amount,
            } => comparison.test(
                players
                    .iter()
                    .flat_map(|p| kinds.iter().map(|kind| self.resource_balance(*p, kind)))
                    .sum::<u64>()
                    .min(u64::from(u32::MAX)) as u32,
                *amount,
            ),
            MissionCondition::Kills {
                players,
                units,
                comparison,
                amount,
            } => {
                let actual = players
                    .iter()
                    .filter_map(|p| self.state.kills.get(p))
                    .flat_map(|types| types.iter())
                    .filter(|(id, _)| match units {
                        MissionUnits::Any => true,
                        MissionUnits::Men => self
                            .unit_type(**id)
                            .is_some_and(|u| !u.structure && !u.revealer),
                        MissionUnits::Structures => {
                            self.unit_type(**id).is_some_and(|u| u.structure)
                        }
                        MissionUnits::Type(unit) => *id == unit,
                    })
                    .fold(0_u32, |total, (_, n)| total.saturating_add(*n));
                comparison.test(actual, *amount)
            }
            MissionCondition::Elapsed {
                comparison,
                milliseconds,
            } => comparison.test(
                // Mission elapsed-time conditions observe a whole-second clock.
                // In particular, AtMost(0) must survive the first trigger poll.
                self.tick()
                    .0
                    .saturating_mul(u64::from(self.rules.tick_ms))
                    .div_euclid(1000)
                    .saturating_mul(1000)
                    .min(u64::from(u32::MAX)) as u32,
                *milliseconds,
            ),
            MissionCondition::Countdown {
                comparison,
                milliseconds,
            } => comparison.test(state.countdown_ms, *milliseconds),
            MissionCondition::Switch { index, set } => state.switches[usize::from(*index)] == *set,
            MissionCondition::Count {
                players,
                units,
                location,
                comparison,
                amount,
            } => {
                let count = self
                    .state
                    .entities
                    .iter()
                    .filter(|entity| {
                        // AtMost counts unfinished units too; the other comparisons count completed units.
                        (*comparison == MissionComparison::AtMost || entity.construction.is_none())
                            && self.mission_matches(
                                entity,
                                players,
                                *units,
                                location.map(|id| state.locations[usize::from(id)]),
                            )
                    })
                    .count() as u32;
                comparison.test(count, *amount)
            }
        }
    }
    /// False suspends this trigger on its current action, using the owner's shared wait.
    pub(in crate::sim) fn mission_action(
        &mut self,
        action: MissionAction,
        trigger: u16,
        state: &mut MissionState,
        toggled_doodads: &mut BTreeSet<EntityId>,
    ) -> bool {
        let player = self.map.mission.as_ref().unwrap().player;
        match action {
            MissionAction::Resume => state.paused = false,
            MissionAction::Preserve => state.triggers[usize::from(trigger)].preserve = true,
            MissionAction::Cosmetic => {}
            MissionAction::Countdown { milliseconds } => state.countdown_ms = milliseconds,
            MissionAction::Rescue { players } => {
                state.rescue_players.extend(players);
                state.rescue_players.sort();
                state.rescue_players.dedup();
            }
            MissionAction::Assault { players } => {
                let actors: Vec<_> = self
                    .state
                    .entities
                    .iter()
                    .filter(|e| {
                        players.contains(&e.owner)
                            && self.unit_type(e.unit_type).is_some_and(|u| {
                                u.weapon.is_some() && !u.structure && u.worker.is_none()
                            })
                    })
                    .map(|e| (e.id, e.owner))
                    .collect();
                for (id, owner) in actors {
                    if let Some(target) = self
                        .map
                        .start_locations
                        .iter()
                        .find(|s| self.is_enemy(owner, s.player))
                        .map(|s| s.position)
                        && let Some(index) = self.index(id)
                    {
                        self.assign(index, UnitOrder::AttackMove { target }, true);
                        for (controller, ai) in self.map.ai.iter().zip(&mut self.state.ai) {
                            if controller.player == owner {
                                ai.deployed.insert(id);
                            }
                        }
                    }
                }
            }
            MissionAction::EnterBunkers { players, location } => {
                let area = state.locations[usize::from(location)];
                let bunkers: Vec<_> = self
                    .state
                    .entities
                    .iter()
                    .filter(|e| {
                        self.mission_matches(e, &players, MissionUnits::Any, Some(area))
                            && self
                                .unit_type(e.unit_type)
                                .is_some_and(|u| u.garrison.is_some())
                    })
                    .map(|e| e.id)
                    .collect();
                for target in bunkers {
                    let actors: Vec<_> = self
                        .state
                        .entities
                        .iter()
                        .filter(|e| {
                            self.mission_matches(e, &players, MissionUnits::Men, Some(area))
                                && self.load_rejection(e.id, target).is_none()
                        })
                        .map(|e| e.id)
                        .collect();
                    for id in actors {
                        if let Some(index) = self.index(id) {
                            self.assign(index, UnitOrder::Load { target }, true);
                        }
                    }
                }
            }
            MissionAction::Remove {
                players,
                units,
                location,
            } => {
                let remove: BTreeSet<_> = self
                    .state
                    .entities
                    .iter()
                    .filter(|e| {
                        self.mission_matches(
                            e,
                            &players,
                            units,
                            location.map(|l| state.locations[usize::from(l)]),
                        )
                    })
                    .map(|e| e.id)
                    .collect();
                self.state.entities.retain(|e| !remove.contains(&e.id));
                self.clear_dead_references();
            }
            MissionAction::ToggleDoodad {
                players,
                units,
                location,
            } => {
                let actors: Vec<_> = self
                    .state
                    .entities
                    .iter()
                    .filter(|e| {
                        self.mission_matches(
                            e,
                            &players,
                            units,
                            Some(state.locations[usize::from(location)]),
                        )
                    })
                    .map(|e| e.id)
                    .collect();
                for id in actors {
                    if !toggled_doodads.insert(id) {
                        continue;
                    }
                    if let Some(index) = self.index(id) {
                        let e = &mut self.state.entities[index];
                        e.doodad_enabled = Some(!e.doodad_enabled.unwrap_or(true));
                    }
                }
            }
            MissionAction::Teleport {
                players,
                units,
                location,
                destination,
            } => {
                let actors: Vec<_> = self
                    .state
                    .entities
                    .iter()
                    .filter(|e| {
                        self.mission_matches(
                            e,
                            &players,
                            units,
                            Some(state.locations[usize::from(location)]),
                        )
                    })
                    .map(|e| e.id)
                    .collect();
                let target = state.locations[usize::from(destination)].center();
                for id in actors {
                    if let Some(index) = self.index(id) {
                        let unit = self.unit_at(index).clone();
                        let position = (0_i32..=16)
                            .flat_map(|r| {
                                (-r..=r).flat_map(move |y| {
                                    (-r..=r).filter(move |x| x.abs() == r || y.abs() == r).map(
                                        move |x| Position {
                                            x: target.x + x * 8,
                                            y: target.y + y * 8,
                                        },
                                    )
                                })
                            })
                            .find(|p| {
                                self.can_place(*p, unit.footprint, unit.movement_class, Some(id))
                            });
                        if let Some(position) = position {
                            self.assign(index, UnitOrder::Idle, true);
                            self.state.entities[index].position = position;
                        }
                    }
                }
            }
            MissionAction::StartAi { controller } => {
                self.state.ai[usize::from(controller)].active = true
            }
            MissionAction::Victory => {
                self.state.winner = Some(player);
            }
            MissionAction::Defeat => {
                if !self.state.defeated.contains(&player) {
                    self.state.defeated.push(player);
                    self.state.defeated.sort();
                }
            }
            MissionAction::Wait { milliseconds } => {
                if state.wait.is_none() {
                    state.wait = Some(MissionWait {
                        trigger,
                        remaining_ms: milliseconds,
                    });
                }
                return false;
            }
            MissionAction::Pause => state.paused = true,
            MissionAction::SetSwitch { index, set } => state.switches[usize::from(index)] = set,
            MissionAction::SetResources { players, resources } => {
                for player in players {
                    for resource in &resources {
                        self.state.players[usize::from(player.0)]
                            .resources
                            .insert(resource.kind.clone(), u64::from(resource.amount));
                    }
                }
            }
            MissionAction::Create {
                player,
                unit_type,
                location,
            } => {
                self.mission_spawn(
                    player,
                    unit_type,
                    state.locations[usize::from(location)].center(),
                );
            }
            MissionAction::Kill {
                players,
                units,
                location,
            } => {
                let ids: BTreeSet<_> = self
                    .state
                    .entities
                    .iter()
                    .filter(|entity| {
                        self.mission_matches(
                            entity,
                            &players,
                            units,
                            Some(state.locations[usize::from(location)]),
                        )
                    })
                    .map(|entity| entity.id)
                    .collect();
                let garrisons: Vec<_> = self
                    .state
                    .entities
                    .iter()
                    .enumerate()
                    .filter(|(_, entity)| {
                        ids.contains(&entity.id)
                            && self.unit_type(entity.unit_type).unwrap().garrison.is_some()
                    })
                    .map(|(index, _)| index)
                    .collect();
                for index in garrisons {
                    self.unload_garrison(index, true);
                }
                self.record_losses(|e| ids.contains(&e.id) || e.hp == 0);
                self.state
                    .entities
                    .retain(|entity| !ids.contains(&entity.id) && entity.hp != 0);
                self.clear_dead_references();
            }
            MissionAction::Invincibility {
                players,
                units,
                location,
                enabled,
            } => {
                let ids: BTreeSet<_> = self
                    .state
                    .entities
                    .iter()
                    .filter(|entity| {
                        self.mission_matches(
                            entity,
                            &players,
                            units,
                            Some(state.locations[usize::from(location)]),
                        )
                    })
                    .map(|entity| entity.id)
                    .collect();
                for entity in &mut self.state.entities {
                    if ids.contains(&entity.id) {
                        entity.invincible = enabled;
                    }
                }
            }
            MissionAction::MoveLocation {
                location,
                players,
                units,
                search_location,
            } => {
                let search = state.locations[usize::from(search_location)];
                let center = self
                    .state
                    .entities
                    .iter()
                    .find(|entity| self.mission_matches(entity, &players, units, Some(search)))
                    .map_or(search.center(), |entity| entity.position);
                let target = &mut state.locations[usize::from(location)];
                let width = target.right - target.left;
                let height = target.bottom - target.top;
                target.left = (center.x - width / 2).clamp(0, self.map.width - width);
                target.top = (center.y - height / 2).clamp(0, self.map.height - height);
                target.right = target.left + width;
                target.bottom = target.top + height;
            }
            MissionAction::Objectives { text } => state.emit(MissionEvent::Objectives { text }),
            MissionAction::Text { text } => state.emit(MissionEvent::Text { text }),
            MissionAction::Sound { sound } => state.emit(MissionEvent::Sound { sound }),
            MissionAction::CenterView { location } => state.emit(MissionEvent::CenterView {
                position: state.locations[usize::from(location)].center(),
            }),
            MissionAction::Speech { muted } => state.emit(MissionEvent::Speech { muted }),
            MissionAction::Transmission {
                text,
                sound,
                portrait,
                location,
                milliseconds,
            } => {
                if state.wait.is_some() {
                    return false;
                }
                state.emit(MissionEvent::Transmission {
                    text,
                    sound,
                    portrait,
                    position: state.locations[usize::from(location)].center(),
                    milliseconds,
                });
                state.wait = Some(MissionWait {
                    trigger,
                    remaining_ms: milliseconds,
                });
                return false;
            }
        }
        true
    }

    pub(in crate::sim) fn mission_spawn(
        &mut self,
        owner: PlayerId,
        unit_type: UnitTypeId,
        center: Position,
    ) -> Option<EntityId> {
        if self.state.entities.len() >= 4096 || self.state.next_entity_id == u32::MAX {
            return None;
        }
        let unit = self.unit_type(unit_type)?.clone();
        let step = self
            .map
            .terrain
            .as_ref()
            .map_or(8, |terrain| terrain.cell_size as i32);
        let mut position = (unit.revealer
            || self.can_place(center, unit.footprint, unit.movement_class, None))
        .then_some(center);
        // A bounded deterministic expanding perimeter handles reinforcements created
        // inside an occupied structure without searching the whole map per action.
        'search: for radius in 1..=32 {
            if position.is_some() {
                break;
            }
            for side in 0..4 {
                for offset in -radius..radius {
                    let (x, y) = match side {
                        0 => (offset, -radius),
                        1 => (radius, offset),
                        2 => (-offset, radius),
                        _ => (-radius, -offset),
                    };
                    let candidate = Position {
                        x: center.x + x * step,
                        y: center.y + y * step,
                    };
                    if self.can_place(candidate, unit.footprint, unit.movement_class, None) {
                        position = Some(candidate);
                        break 'search;
                    }
                }
            }
        }
        let position = position?;
        let id = EntityId(self.state.next_entity_id);
        self.state.next_entity_id += 1;
        self.state.entities.push(Entity {
            id,
            owner,
            unit_type,
            position,
            hp: unit.max_hp,
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
        });
        self.record_created(owner, unit_type);
        Some(id)
    }
}
