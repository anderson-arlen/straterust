//! Deterministic native town AI. Importers translate source scripts into this
//! bounded instruction set; decisions use ordinary validated unit orders.
use super::*;
use anyhow::Context;

const MAX_STEPS: usize = 64;
const THINK_TICKS: u64 = 8;

mod defense;
mod transport;
pub use transport::AiTransport;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AiController {
    pub player: PlayerId,
    pub home: Position,
    /// Town membership/resource search radius, in world coordinates.
    pub radius: u32,
    #[serde(default)]
    pub active: bool,
    #[serde(default)]
    pub program: Vec<AiInstruction>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AiInstruction {
    Wait(u32),
    Request {
        unit_type: UnitTypeId,
        count: u16,
        priority: u8,
    },
    Defense {
        unit_type: UnitTypeId,
        count: u16,
    },
    AttackClear,
    AttackAdd {
        unit_type: UnitTypeId,
        count: u16,
    },
    AttackPrepare,
    Attack,
    Jump(u16),
    Stop,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiRequest {
    pub unit_type: UnitTypeId,
    pub count: u16,
    pub priority: u8,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiState {
    pub active: bool,
    pub instruction: u16,
    pub wake: Tick,
    pub requests: Vec<AiRequest>,
    pub attack: BTreeMap<UnitTypeId, u16>,
    pub prepared: bool,
    #[serde(default)]
    pub defense: BTreeMap<UnitTypeId, u16>,
    #[serde(default)]
    pub transport_needed: bool,
    #[serde(default)]
    pub transports: BTreeMap<EntityId, AiTransport>,
    pub deployed: BTreeSet<EntityId>,
    #[serde(default)]
    pub members: BTreeSet<EntityId>,
    pub guards: BTreeMap<EntityId, Position>,
    pub accepted_orders: u64,
}
impl AiState {
    pub(super) fn new(controller: &AiController) -> Self {
        Self {
            active: controller.active,
            instruction: 0,
            wake: Tick(0),
            requests: Vec::new(),
            attack: BTreeMap::new(),
            prepared: false,
            defense: BTreeMap::new(),
            transport_needed: false,
            transports: BTreeMap::new(),
            deployed: BTreeSet::new(),
            members: BTreeSet::new(),
            guards: BTreeMap::new(),
            accepted_orders: 0,
        }
    }
    fn request(&mut self, unit_type: UnitTypeId, count: u16, priority: u8) {
        if let Some(request) = self.requests.iter_mut().find(|r| r.unit_type == unit_type) {
            request.count = request.count.max(count);
            request.priority = request.priority.max(priority);
        } else {
            self.requests.push(AiRequest {
                unit_type,
                count,
                priority,
            });
        }
        self.requests
            .sort_by_key(|r| (std::cmp::Reverse(r.priority), r.unit_type));
    }
}

pub(super) fn validate_ai(rules: &Rules, map: &Map) -> Result<()> {
    ensure!(map.ai.len() <= 32, "too many AI towns");
    for controller in &map.ai {
        ensure!(controller.player.0 < map.players, "unknown AI player");
        ensure!(map.contains(controller.home), "AI home outside map");
        ensure!(
            (1..=32768).contains(&controller.radius),
            "invalid AI town radius"
        );
        ensure!(
            controller.program.len() <= 4096,
            "AI program exceeds 4096 instructions"
        );
        for instruction in &controller.program {
            match instruction {
                AiInstruction::Wait(ticks) => ensure!(*ticks > 0, "zero AI wait"),
                AiInstruction::Jump(target) => ensure!(
                    usize::from(*target) < controller.program.len(),
                    "invalid AI jump"
                ),
                AiInstruction::Request {
                    unit_type, count, ..
                }
                | AiInstruction::Defense { unit_type, count }
                | AiInstruction::AttackAdd { unit_type, count } => {
                    ensure!(*count > 0 && *count <= 4096, "invalid AI request count");
                    let unit = rules
                        .units
                        .iter()
                        .find(|u| u.id == *unit_type)
                        .context("unknown AI unit type")?;
                    if matches!(
                        instruction,
                        AiInstruction::AttackAdd { .. } | AiInstruction::Defense { .. }
                    ) {
                        ensure!(
                            unit.weapon.is_some() && !unit.structure && unit.worker.is_none(),
                            "AI attack requires a combat unit"
                        );
                    }
                }
                _ => {}
            }
        }
    }
    Ok(())
}

impl World {
    /// AI is part of the simulation, so replays contain human inputs only. Its
    /// decisions are regenerated on every peer and its full state is hashed.
    pub(super) fn advance_ai(&mut self) {
        if self.state.winner.is_some() || self.state.mission.as_ref().is_some_and(|m| m.paused) {
            return;
        }
        let controllers = self.map.ai.clone();
        for (index, controller) in controllers.iter().enumerate() {
            let mut state = self.state.ai[index].clone();
            let living: BTreeSet<_> = self
                .state
                .entities
                .iter()
                .filter(|e| e.owner == controller.player)
                .map(|e| e.id)
                .collect();
            state.members.retain(|id| living.contains(id));
            state.members.extend(
                self.state
                    .entities
                    .iter()
                    .filter(|e| {
                        e.owner == controller.player
                            && rts::distance(e.position, controller.home)
                                <= i64::from(controller.radius).pow(2)
                    })
                    .map(|e| e.id),
            );
            state.deployed.retain(|id| living.contains(id));
            state.guards.retain(|id, _| living.contains(id));
            if self.tick().0.is_multiple_of(THINK_TICKS) {
                if !controllers[..index]
                    .iter()
                    .any(|c| c.player == controller.player)
                {
                    self.ai_defend(controller, &mut state);
                }
                self.ai_transports(controller, &mut state);
                if state.active {
                    self.ai_program(controller, &mut state);
                    self.ai_economy(controller, &mut state);
                }
            }
            self.state.ai[index] = state;
        }
    }

    fn ai_order(&mut self, controller: &AiController, state: &mut AiState, order: Order) -> bool {
        let sequence = &mut self.state.last_sequences[usize::from(controller.player.0)];
        let Some(next) = sequence.checked_add(1) else {
            return false;
        };
        *sequence = next;
        let command = Command {
            tick: self.tick(),
            player: controller.player,
            sequence: next,
            order,
        };
        let accepted = self.apply(&command).is_none();
        if accepted {
            state.accepted_orders = state.accepted_orders.saturating_add(1);
        }
        accepted
    }

    fn ai_in_town(&self, controller: &AiController, state: &AiState, entity: &Entity) -> bool {
        state.members.contains(&entity.id)
            || rts::distance(entity.position, controller.home)
                <= i64::from(controller.radius).pow(2)
            || entity
                .parent
                .and_then(|id| self.index(id))
                .is_some_and(|parent| {
                    rts::distance(self.state.entities[parent].position, controller.home)
                        <= i64::from(controller.radius).pow(2)
                })
    }

    pub(in crate::sim) fn ai_produced(&mut self, producer: EntityId, unit: EntityId) {
        let Some(index) = self.index(producer) else {
            return;
        };
        let parent = &self.state.entities[index];
        for (town, controller) in self.map.ai.iter().enumerate() {
            if parent.owner == controller.player
                && self.ai_in_town(controller, &self.state.ai[town], parent)
            {
                self.state.ai[town].members.insert(unit);
            }
        }
    }

    fn ai_count(&self, controller: &AiController, state: &AiState, unit_type: UnitTypeId) -> u16 {
        self.state
            .entities
            .iter()
            .filter(|e| {
                e.owner == controller.player
                    && !state.deployed.contains(&e.id)
                    && !self.state.ai.iter().any(|s| s.deployed.contains(&e.id))
            })
            .filter(|e| self.ai_in_town(controller, state, e))
            .map(|e| {
                usize::from(e.unit_type == unit_type)
                    + e.production
                        .iter()
                        .filter(|j| j.unit_type == unit_type)
                        .count()
            })
            .sum::<usize>()
            .min(4096) as u16
    }

    fn ai_program(&mut self, controller: &AiController, state: &mut AiState) {
        if self.tick() < state.wake {
            return;
        }
        for _ in 0..MAX_STEPS {
            let Some(instruction) = controller.program.get(usize::from(state.instruction)) else {
                return;
            };
            match *instruction {
                AiInstruction::Wait(ticks) => {
                    state.wake = Tick(self.tick().0.saturating_add(u64::from(ticks)));
                    state.instruction += 1;
                    return;
                }
                AiInstruction::Request {
                    unit_type,
                    count,
                    priority,
                } => state.request(unit_type, count, priority),
                AiInstruction::Defense { unit_type, count } => {
                    let total = state.defense.entry(unit_type).or_default();
                    *total = (*total).max(count);
                }
                AiInstruction::AttackClear => {
                    state.transport_needed = false;
                    state.attack.clear();
                    state.prepared = false;
                }
                AiInstruction::AttackAdd { unit_type, count } => {
                    let total = state.attack.entry(unit_type).or_default();
                    *total = total.saturating_add(count).min(4096);
                }
                AiInstruction::AttackPrepare => {
                    state.prepared = true;
                    state.transport_needed = self.ai_needs_transport(controller, state);
                }
                AiInstruction::Attack => {
                    // A depleted town waits for the requested real production.
                    // It never creates free attackers to satisfy a wave.
                    if state.attack.iter().any(|(id, count)| {
                        self.ai_count(controller, state, *id)
                            < count.saturating_add(state.defense.get(id).copied().unwrap_or(0))
                    }) {
                        return;
                    }
                    let target = self.ai_target(controller);
                    let Some(target) = target else {
                        return;
                    };
                    let mut wave = Vec::new();
                    for (&unit_type, &count) in &state.attack {
                        let members: Vec<_> = self
                            .state
                            .entities
                            .iter()
                            .filter(|e| {
                                e.owner == controller.player
                                    && e.unit_type == unit_type
                                    && e.construction.is_none()
                                    && e.garrisoned_in.is_none()
                                    && self.ai_in_town(controller, state, e)
                                    && !state.deployed.contains(&e.id)
                                    && !self.state.ai.iter().any(|s| s.deployed.contains(&e.id))
                            })
                            .skip(usize::from(
                                state.defense.get(&unit_type).copied().unwrap_or(0),
                            ))
                            .take(usize::from(count))
                            .map(|e| e.id)
                            .collect();
                        if members.len() < usize::from(count) {
                            return;
                        }
                        wave.extend(members);
                    }
                    if state.transport_needed
                        && !self.ai_load_wave(controller, state, &wave, target)
                    {
                        return;
                    }
                    for entity in wave {
                        if state.transport_needed
                            && self
                                .unit_type(
                                    self.state.entities[self.index(entity).unwrap()].unit_type,
                                )
                                .is_some_and(|u| u.movement_class == MovementClass::Ground)
                        {
                            continue;
                        }
                        if self.ai_order(controller, state, Order::AttackMove { entity, target }) {
                            state.deployed.insert(entity);
                        }
                    }
                    state.prepared = false;
                }
                AiInstruction::Jump(target) => {
                    state.instruction = target;
                    continue;
                }
                AiInstruction::Stop => {
                    state.active = false;
                    return;
                }
            }
            state.instruction += 1;
        }
        // Even a wait-free loop gets only a bounded amount of work per decision.
    }

    fn ai_target(&self, controller: &AiController) -> Option<Position> {
        // Original campaign scripts know opponents' starting bases. Prefer a
        // visible enemy; absent one, attack that fixed map-authored position.
        self.state
            .entities
            .iter()
            .filter(|e| {
                self.is_enemy(controller.player, e.owner)
                    && self.entity_visible(controller.player, e.id)
                    && !self.unit_type(e.unit_type).is_some_and(|u| u.revealer)
            })
            .min_by_key(|e| (rts::distance(controller.home, e.position), e.id))
            .map(|e| e.position)
            .or_else(|| {
                self.map
                    .start_locations
                    .iter()
                    .filter(|s| self.is_enemy(controller.player, s.player))
                    .min_by_key(|s| (rts::distance(controller.home, s.position), s.player))
                    .map(|s| s.position)
            })
    }

    fn ai_economy(&mut self, controller: &AiController, state: &mut AiState) {
        let radius = i64::from(controller.radius).pow(2);
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
                        && !matches!(e.order, UnitOrder::Build { .. })
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
                    let assigned = self
                        .state
                        .entities
                        .iter()
                        .filter(|e| {
                            e.owner == controller.player
                                && e.order == (UnitOrder::Gather { resource: r.id })
                        })
                        .count();
                    (assigned, rts::distance(worker.position, r.position), r.id)
                })
                .map(|r| r.id);
            if let Some(resource) = resource {
                self.ai_order(controller, state, Order::Gather { entity, resource });
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
                    });
                }
            }
        }
        self.ai_request_transports(controller, state, &mut requests);
        // Supply is built through the same builder/payment/placement path.
        let (used, provided) = self.supply(controller.player);
        if used.saturating_add(4) >= provided && provided < self.rules.supply_limit {
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
                        && e.unit_type == supply.id
                        && e.construction.is_some()
                }) {
                    requests.insert(
                        0,
                        AiRequest {
                            unit_type: supply.id,
                            count: (count + 1).min(4096) as u16,
                            priority: 255,
                        },
                    );
                }
            }
        }
        for request in requests {
            if self.ai_count(controller, state, request.unit_type) >= request.count {
                continue;
            }
            let unit = self.unit_type(request.unit_type).unwrap().clone();
            if !self.can_pay(controller.player, &unit.cost) {
                continue;
            }
            if unit.structure {
                let builders: Vec<_> = self
                    .state
                    .entities
                    .iter()
                    .filter(|e| {
                        e.owner == controller.player
                            && e.construction.is_none()
                            && !matches!(
                                e.order,
                                UnitOrder::Build { .. } | UnitOrder::Repair { .. }
                            )
                            && self.ai_in_town(controller, state, e)
                            && self
                                .unit_type(e.unit_type)
                                .is_some_and(|u| u.builds.contains(&unit.id))
                    })
                    .map(|e| e.id)
                    .collect();
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
                    for ring in 2_i32..=16 {
                        for dy in -ring..=ring {
                            for dx in -ring..=ring {
                                if dx.abs() != ring && dy.abs() != ring {
                                    continue;
                                }
                                let position = Position {
                                    x: controller.home.x / 32 * 32 + dx * 32,
                                    y: controller.home.y / 32 * 32 + dy * 32,
                                };
                                positions.push(position);
                            }
                        }
                    }
                }
                'placement: for position in positions {
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
                            break 'placement;
                        }
                    }
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
                            && self
                                .unit_type(e.unit_type)
                                .is_some_and(|u| u.trains.contains(&unit.id))
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
    }
}

pub(super) fn put_ai_definition(bytes: &mut Vec<u8>, controllers: &[AiController]) {
    bytes.extend((controllers.len() as u32).to_le_bytes());
    for controller in controllers {
        bytes.extend(controller.player.0.to_le_bytes());
        put_position(bytes, controller.home);
        bytes.extend(controller.radius.to_le_bytes());
        bytes.push(u8::from(controller.active));
        bytes.extend((controller.program.len() as u32).to_le_bytes());
        for instruction in &controller.program {
            match *instruction {
                AiInstruction::Wait(ticks) => {
                    bytes.push(0);
                    bytes.extend(ticks.to_le_bytes());
                }
                AiInstruction::Request {
                    unit_type,
                    count,
                    priority,
                } => {
                    bytes.push(1);
                    bytes.extend(unit_type.0.to_le_bytes());
                    bytes.extend(count.to_le_bytes());
                    bytes.push(priority);
                }
                AiInstruction::AttackClear => bytes.push(2),
                AiInstruction::AttackAdd { unit_type, count } => {
                    bytes.push(3);
                    bytes.extend(unit_type.0.to_le_bytes());
                    bytes.extend(count.to_le_bytes());
                }
                AiInstruction::AttackPrepare => bytes.push(4),
                AiInstruction::Attack => bytes.push(5),
                AiInstruction::Jump(target) => {
                    bytes.push(6);
                    bytes.extend(target.to_le_bytes());
                }
                AiInstruction::Stop => bytes.push(7),
                AiInstruction::Defense { unit_type, count } => {
                    bytes.push(8);
                    bytes.extend(unit_type.0.to_le_bytes());
                    bytes.extend(count.to_le_bytes());
                }
            }
        }
    }
}
pub(super) fn put_ai_state(bytes: &mut Vec<u8>, states: &[AiState]) {
    bytes.extend((states.len() as u32).to_le_bytes());
    for state in states {
        bytes.push(u8::from(state.active));
        bytes.extend(state.instruction.to_le_bytes());
        bytes.extend(state.wake.0.to_le_bytes());
        bytes.push(u8::from(state.prepared));
        bytes.extend(state.accepted_orders.to_le_bytes());
        bytes.push(u8::from(state.transport_needed));
        bytes.extend((state.defense.len() as u32).to_le_bytes());
        for (id, count) in &state.defense {
            bytes.extend(id.0.to_le_bytes());
            bytes.extend(count.to_le_bytes());
        }
        bytes.extend((state.transports.len() as u32).to_le_bytes());
        for (id, transport) in &state.transports {
            bytes.extend(id.0.to_le_bytes());
            put_position(bytes, transport.target);
            bytes.push(u8::from(transport.departed));
            bytes.extend((transport.passengers.len() as u32).to_le_bytes());
            for passenger in &transport.passengers {
                bytes.extend(passenger.0.to_le_bytes());
            }
        }
        bytes.extend((state.requests.len() as u32).to_le_bytes());
        for r in &state.requests {
            bytes.extend(r.unit_type.0.to_le_bytes());
            bytes.extend(r.count.to_le_bytes());
            bytes.push(r.priority);
        }
        bytes.extend((state.attack.len() as u32).to_le_bytes());
        for (id, count) in &state.attack {
            bytes.extend(id.0.to_le_bytes());
            bytes.extend(count.to_le_bytes());
        }
        bytes.extend((state.members.len() as u32).to_le_bytes());
        for id in &state.members {
            bytes.extend(id.0.to_le_bytes());
        }
        bytes.extend((state.deployed.len() as u32).to_le_bytes());
        for id in &state.deployed {
            bytes.extend(id.0.to_le_bytes());
        }
        bytes.extend((state.guards.len() as u32).to_le_bytes());
        for (id, position) in &state.guards {
            bytes.extend(id.0.to_le_bytes());
            put_position(bytes, *position);
        }
    }
}

#[cfg(test)]
mod tests;
