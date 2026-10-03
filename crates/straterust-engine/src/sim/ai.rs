//! Deterministic native town AI. Importers translate source scripts into this
//! bounded instruction set; decisions use ordinary validated unit orders.
use super::*;
use anyhow::Context;

const MAX_STEPS: usize = 64;
const THINK_TICKS: u64 = 8;

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
    pub deployed: BTreeSet<EntityId>,
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
            deployed: BTreeSet::new(),
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
                | AiInstruction::AttackAdd { unit_type, count } => {
                    ensure!(*count > 0 && *count <= 4096, "invalid AI request count");
                    let unit = rules
                        .units
                        .iter()
                        .find(|u| u.id == *unit_type)
                        .context("unknown AI unit type")?;
                    if matches!(instruction, AiInstruction::AttackAdd { .. }) {
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
            state.deployed.retain(|id| living.contains(id));
            state.guards.retain(|id, _| living.contains(id));
            if self.tick().0.is_multiple_of(THINK_TICKS) {
                if !controllers[..index]
                    .iter()
                    .any(|c| c.player == controller.player)
                {
                    self.ai_defend(controller, &mut state);
                }
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

    fn ai_count(&self, controller: &AiController, state: &AiState, unit_type: UnitTypeId) -> u16 {
        self.state
            .entities
            .iter()
            .filter(|e| {
                e.owner == controller.player
                    && !state.deployed.contains(&e.id)
                    && !self.state.ai.iter().any(|s| s.deployed.contains(&e.id))
            })
            .filter(|e| {
                rts::distance(e.position, controller.home) <= i64::from(controller.radius).pow(2)
            })
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
                AiInstruction::AttackClear => {
                    state.attack.clear();
                    state.prepared = false;
                }
                AiInstruction::AttackAdd { unit_type, count } => {
                    let total = state.attack.entry(unit_type).or_default();
                    *total = total.saturating_add(count).min(4096);
                }
                AiInstruction::AttackPrepare => state.prepared = true,
                AiInstruction::Attack => {
                    // A depleted town waits for the requested real production.
                    // It never creates free attackers to satisfy a wave.
                    if state
                        .attack
                        .iter()
                        .any(|(id, count)| self.ai_count(controller, state, *id) < *count)
                    {
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
                                    && rts::distance(e.position, controller.home)
                                        <= i64::from(controller.radius).pow(2)
                                    && !state.deployed.contains(&e.id)
                                    && !self.state.ai.iter().any(|s| s.deployed.contains(&e.id))
                            })
                            .take(usize::from(count))
                            .map(|e| e.id)
                            .collect();
                        if members.len() < usize::from(count) {
                            return;
                        }
                        wave.extend(members);
                    }
                    for entity in wave {
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
                    && rts::distance(e.position, controller.home) <= radius
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
                    && rts::distance(e.position, controller.home) <= radius
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
        if state.prepared {
            for (&unit_type, &count) in &state.attack {
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
                            && rts::distance(e.position, controller.home) <= radius
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
                            && rts::distance(e.position, controller.home) <= radius
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

    fn ai_defend(&mut self, controller: &AiController, state: &mut AiState) {
        let trouble: Vec<_> = self
            .state
            .entities
            .iter()
            .filter(|e| e.owner == controller.player && e.auto_attack_target.is_some())
            .filter_map(|e| e.retaliation_position.map(|p| (e.position, p)))
            .collect();
        let guards: Vec<_> = self
            .state
            .entities
            .iter()
            .filter(|e| {
                e.owner == controller.player
                    && rts::distance(e.position, controller.home)
                        <= i64::from(controller.radius).pow(2)
                    && e.construction.is_none()
                    && !state.deployed.contains(&e.id)
                    && !self.state.ai.iter().any(|s| s.deployed.contains(&e.id))
                    && !e.burrowed
                    && e.unburrow_remaining == 0
                    && e.garrisoned_in.is_none()
                    && self
                        .unit_type(e.unit_type)
                        .is_some_and(|u| u.weapon.is_some() && u.worker.is_none() && !u.structure)
            })
            .map(|e| (e.id, e.position, e.order.clone(), e.auto_attack_target))
            .collect();
        for (entity, position, order, target) in guards {
            let home = *state.guards.entry(entity).or_insert(position);
            if !matches!(order, UnitOrder::Idle | UnitOrder::AttackMove { .. }) || target.is_some()
            {
                continue;
            }
            if let Some((_, target)) = trouble
                .iter()
                .filter(|(ally, _)| rts::distance(position, *ally) <= 256_i64.pow(2))
                .min_by_key(|(ally, _)| rts::distance(position, *ally))
            {
                if order != (UnitOrder::AttackMove { target: *target }) {
                    self.ai_order(
                        controller,
                        state,
                        Order::AttackMove {
                            entity,
                            target: *target,
                        },
                    );
                }
            } else if matches!(order, UnitOrder::Idle)
                && rts::distance(position, home) > 32_i64.pow(2)
            {
                self.ai_order(
                    controller,
                    state,
                    Order::AttackMove {
                        entity,
                        target: home,
                    },
                );
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
mod tests {
    use super::*;
    use crate::content::Package;
    fn economy() -> World {
        let package = Package::load(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/terran-demo"),
        )
        .unwrap();
        let original = package.world(42).unwrap();
        let mut rules = original.rules().clone();
        rules.victory = false;
        for unit in &mut rules.units {
            unit.build_ticks = 16;
            unit.vision_range = 192;
        }
        let mut map = original.map().clone();
        map.terrain = None;
        map.spawns.retain(|s| s.owner == PlayerId(0));
        map.spawns.push(Spawn {
            owner: PlayerId(1),
            unit_type: UnitTypeId(3),
            position: Position { x: 1280, y: 640 },
            ..Spawn::default()
        });
        map.spawns.push(Spawn {
            owner: PlayerId(1),
            unit_type: UnitTypeId(2),
            position: Position { x: 1184, y: 640 },
            ..Spawn::default()
        });
        map.resources.extend((0..3).map(|n| ResourceSpawn {
            kind: "minerals".into(),
            position: Position {
                x: 1056,
                y: 576 + n * 48,
            },
            amount: 1500,
            footprint: Footprint {
                width: 64,
                height: 32,
            },
            requires_extractor: false,
        }));
        map.ai = vec![AiController {
            player: PlayerId(1),
            home: Position { x: 1280, y: 640 },
            radius: 512,
            active: true,
            program: vec![
                AiInstruction::Request {
                    unit_type: UnitTypeId(2),
                    count: 3,
                    priority: 130,
                },
                AiInstruction::Request {
                    unit_type: UnitTypeId(4),
                    count: 1,
                    priority: 100,
                },
                AiInstruction::Request {
                    unit_type: UnitTypeId(5),
                    count: 1,
                    priority: 80,
                },
                AiInstruction::Wait(32),
                AiInstruction::AttackClear,
                AiInstruction::AttackAdd {
                    unit_type: UnitTypeId(1),
                    count: 3,
                },
                AiInstruction::AttackPrepare,
                AiInstruction::Attack,
                AiInstruction::Wait(64),
                AiInstruction::Jump(4),
            ],
        }];
        World::new(rules, map, 42).unwrap()
    }
    #[test]
    fn ai_gathers_builds_trains_supplies_and_launches_paid_groups() {
        let mut world = economy();
        for _ in 0..3200 {
            world.step(&[]).unwrap();
        }
        let state = &world.state.ai[0];
        assert!(state.accepted_orders > 12, "{state:?}");
        assert!(!state.deployed.is_empty(), "{state:?}");
        for id in [3, 4, 5] {
            assert!(world.state.entities.iter().any(|e| e.owner == PlayerId(1)
                && e.unit_type == UnitTypeId(id)
                && e.construction.is_none()));
        }
        assert!(
            world
                .state
                .resources
                .iter()
                .filter(|r| r.position.x == 1056)
                .any(|r| r.amount < 1500)
        );
        assert!(world.supply(PlayerId(1)).1 > 10);
        assert!(
            world
                .state
                .entities
                .iter()
                .filter(|e| e.owner == PlayerId(1) && e.unit_type == UnitTypeId(2))
                .count()
                >= 3
        );
    }
    #[test]
    fn ai_replay_and_serialized_mid_attack_state_resume_identically() {
        let mut world = economy();
        let mut replay = economy();
        for _ in 0..800 {
            world.step(&[]).unwrap();
            replay.step(&[]).unwrap();
        }
        assert_eq!(world.state_hash(), replay.state_hash());
        replay.state = ron::from_str(&ron::to_string(&world.state).unwrap()).unwrap();
        for _ in 0..900 {
            world.step(&[]).unwrap();
            replay.step(&[]).unwrap();
        }
        assert_eq!(world.state_hash(), replay.state_hash());
        let hash = world.state_hash();
        replay.state.ai[0].wake.0 += 1;
        assert_ne!(hash, replay.state_hash());
        let mut map = world.map().clone();
        map.ai[0].program[3] = AiInstruction::Wait(33);
        assert_ne!(
            world.map_hash(),
            World::new(world.rules().clone(), map, 42)
                .unwrap()
                .map_hash()
        );
    }
    #[test]
    fn ai_program_validation_and_wait_free_loop_are_bounded() {
        let world = economy();
        let mut map = world.map().clone();
        map.ai[0].program = vec![AiInstruction::Jump(0)];
        let mut looping = World::new(world.rules().clone(), map.clone(), 42).unwrap();
        looping.step(&[]).unwrap();
        assert_eq!(looping.tick(), Tick(1));
        map.ai[0].program = vec![AiInstruction::Jump(1)];
        assert!(World::new(world.rules().clone(), map.clone(), 42).is_err());
        map.ai[0].program = vec![AiInstruction::Wait(0)];
        assert!(World::new(world.rules().clone(), map.clone(), 42).is_err());
        map.ai[0].program = vec![AiInstruction::AttackAdd {
            unit_type: UnitTypeId(2),
            count: 1,
        }];
        assert!(World::new(world.rules().clone(), map, 42).is_err());
    }
    #[test]
    fn player_inputs_cannot_override_computer_and_hidden_targets_do_not_leak() {
        let mut world = economy();
        let result = world
            .step(&[Command {
                player: PlayerId(1),
                tick: Tick(0),
                sequence: 100,
                order: Order::Move {
                    entity: EntityId(4),
                    target: Position { x: 10, y: 10 },
                },
            }])
            .unwrap();
        assert_eq!(result[0].rejection, Some(Rejection::ComputerControlled));
        let mut map = world.map().clone();
        map.fog_of_war = true;
        let mut hidden = World::new(world.rules().clone(), map, 42).unwrap();
        let controller = hidden.map.ai[0].clone();
        let target = hidden.ai_target(&controller).unwrap();
        hidden.state.entities[0].position.x += 100;
        assert_eq!(hidden.ai_target(&controller), Some(target));
    }

    #[test]
    fn guard_controller_does_not_recall_another_towns_attack_group() {
        let base = economy();
        let mut map = base.map().clone();
        let mut guards = map.ai[0].clone();
        guards.active = false;
        guards.program.clear();
        map.ai.insert(0, guards);
        map.ai[1].program = vec![AiInstruction::Wait(10000)];
        map.spawns.push(Spawn {
            owner: PlayerId(1),
            unit_type: UnitTypeId(1),
            position: Position { x: 976, y: 416 },
            ..Spawn::default()
        });
        let mut world = World::new(base.rules().clone(), map, 42).unwrap();
        let id = world.state.entities.last().unwrap().id;
        world.state.ai[1].deployed.insert(id);
        for _ in 0..16 {
            world.step(&[]).unwrap();
        }
        assert!(!world.state.ai[0].guards.contains_key(&id));
        let entity = world.state.entities.iter().find(|e| e.id == id).unwrap();
        assert!(matches!(entity.order, UnitOrder::AttackMove { .. }));
        assert!(entity.position.x < 976);
    }

    #[test]
    fn consuming_construction_spends_resources_and_finishes_without_a_worker() {
        let base = economy();
        let mut rules = base.rules().clone();
        rules
            .units
            .iter_mut()
            .find(|u| u.id == UnitTypeId(4))
            .unwrap()
            .consumes_builder = true;
        let mut map = base.map().clone();
        map.ai.clear();
        rules.starting_resources = vec![ResourceAmount {
            kind: "minerals".into(),
            amount: 500,
        }];
        let mut world = World::new(rules, map, 42).unwrap();
        let worker = world
            .state
            .entities
            .iter()
            .find(|e| e.owner == PlayerId(0) && e.unit_type == UnitTypeId(2))
            .unwrap()
            .id;
        let before = world.resource_balance(PlayerId(0), "minerals");
        let position = Position { x: 384, y: 384 };
        assert!(
            world
                .step(&[Command {
                    tick: Tick(0),
                    player: PlayerId(0),
                    sequence: 1,
                    order: Order::Build {
                        entity: worker,
                        unit_type: UnitTypeId(4),
                        position
                    }
                }])
                .unwrap()[0]
                .rejection
                .is_none()
        );
        assert_eq!(
            world.resource_balance(PlayerId(0), "minerals"),
            before - 100
        );
        for _ in 0..512 {
            world.step(&[]).unwrap();
        }
        assert!(!world.state.entities.iter().any(|e| e.id == worker));
        assert!(world.state.entities.iter().any(|e| e.owner == PlayerId(0)
            && e.unit_type == UnitTypeId(4)
            && e.construction.is_none()));
    }
}
