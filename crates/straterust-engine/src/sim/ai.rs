//! Deterministic native town AI. Importers translate source scripts into this
//! bounded instruction set; decisions use ordinary validated unit orders.
use super::*;
use anyhow::Context;

const MAX_STEPS: usize = 64;
const THINK_TICKS: u64 = 8;

mod allocation;
mod attack;
mod clearing;
mod defense;
mod dependencies;
mod economy;
mod technology;
mod transport;
pub use transport::AiTransport;
pub(super) use transport::LandingKey;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AiController {
    pub player: PlayerId,
    pub home: Position,
    /// Town membership/resource search radius, in world coordinates.
    pub radius: u32,
    #[serde(default)]
    pub active: bool,
    /// Multiplayer may suspend this controller while a human holds the slot.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub takeover: bool,
    #[serde(default)]
    pub program: Vec<AiInstruction>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub research: Vec<ResearchId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub abilities: Vec<AbilityId>,
    /// Relative worker allocation by resource kind; empty retains per-node balancing.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub harvest_weights: Vec<ResourceAmount>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AiInstruction {
    Wait(u32),
    Request {
        unit_type: UnitTypeId,
        count: u16,
        priority: u8,
    },
    /// Keep this many units, including those already fighting away from town.
    Maintain {
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
    /// Dispatch the ready members and continue, without waiting for missing ones.
    AttackAvailable,
    /// Branch using the terrain probe performed by AttackPrepare.
    JumpIfTransportNeeded(u16),
    Jump(u16),
    Stop,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiRequest {
    pub unit_type: UnitTypeId,
    pub count: u16,
    pub priority: u8,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub include_deployed: bool,
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
    fn request(&mut self, unit_type: UnitTypeId, count: u16, priority: u8, include_deployed: bool) {
        if let Some(request) = self.requests.iter_mut().find(|r| r.unit_type == unit_type) {
            request.count = request.count.max(count);
            request.priority = request.priority.max(priority);
            request.include_deployed |= include_deployed;
        } else {
            self.requests.push(AiRequest {
                unit_type,
                count,
                priority,
                include_deployed,
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
        ensure!(
            controller.research.len() <= rules.research.len()
                && controller
                    .research
                    .iter()
                    .all(|id| rules.research.iter().any(|r| r.id == *id)),
            "unknown AI research"
        );
        ensure!(
            controller.abilities.len() <= 256
                && controller.abilities.iter().all(|id| rules
                    .units
                    .iter()
                    .any(|u| u.abilities.iter().any(|a| a.id == *id))),
            "unknown AI ability"
        );
        let mut kinds = BTreeSet::new();
        ensure!(
            controller.harvest_weights.len() <= 16
                && controller.harvest_weights.iter().all(|a| {
                    !a.kind.is_empty()
                        && a.kind.len() <= 64
                        && kinds.insert(&a.kind)
                        && a.kind
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
                        && (1..=4096).contains(&a.amount)
                }),
            "invalid AI harvest allocation"
        );
        for instruction in &controller.program {
            match instruction {
                AiInstruction::Wait(ticks) => ensure!(*ticks > 0, "zero AI wait"),
                AiInstruction::Jump(target) | AiInstruction::JumpIfTransportNeeded(target) => {
                    ensure!(
                        usize::from(*target) < controller.program.len(),
                        "invalid AI jump"
                    )
                }
                AiInstruction::Request {
                    unit_type, count, ..
                }
                | AiInstruction::Maintain {
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
                            (!unit.structure
                                || matches!(instruction, AiInstruction::Defense { .. }))
                                && unit.worker.is_none()
                                && !unit.portable
                                && !unit.revealer
                                && (unit.speed > 0 || unit.weapon.is_some()),
                            "AI wave requires a mobile combat/support unit: {:?}",
                            unit.id
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
    pub(crate) fn set_ai_control(&mut self, player: PlayerId, active: bool) -> Result<()> {
        let mut found = false;
        for (controller, state) in self.map.ai.iter().zip(&mut self.state.ai) {
            if controller.player == player && controller.takeover {
                state.active = active;
                found = true;
            }
        }
        ensure!(found, "player has no takeover controller");
        Ok(())
    }

    /// AI is part of the simulation, so replays contain human inputs only. Its
    /// decisions are regenerated on every peer and its full state is hashed.
    pub(super) fn advance_ai(&mut self) {
        if self.state.winner.is_some() || self.state.mission.as_ref().is_some_and(|m| m.paused) {
            return;
        }
        let controllers = self.map.ai.clone();
        for (index, controller) in controllers.iter().enumerate() {
            let mut state = self.state.ai[index].clone();
            if controller.takeover && !state.active {
                continue;
            }
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
                    self.ai_replenish_fighters(controller, &mut state);
                    self.ai_technology(controller, &mut state);
                }
            }
            self.state.ai[index] = state;
        }
    }

    fn ai_replenish_fighters(&mut self, controller: &AiController, state: &mut AiState) {
        let producers: Vec<_> = self
            .state
            .entities
            .iter()
            .filter_map(|entity| {
                let unit = self.unit_type(entity.unit_type)?;
                let count = self.stored_production_count(entity);
                (entity.owner == controller.player
                    && self.ai_in_town(controller, state, entity)
                    && entity.construction.is_none()
                    && entity.production.is_empty()
                    && unit.stored_weapon.is_some()
                    && count < self.production_capacity(entity) as usize)
                    .then(|| (entity.id, unit.trains[0]))
            })
            .collect();
        for (entity, unit_type) in producers {
            self.ai_order(controller, state, Order::Train { entity, unit_type });
        }
    }

    fn ai_order(&mut self, controller: &AiController, state: &mut AiState, order: Order) -> bool {
        // An interior gatherer's stored position can be inside its mine/depot.
        // Replacing that order must first find a free exit, or the new route
        // starts embedded in an obstacle. If crowded, retain work and retry.
        if let Some(index) = self.index(order.entity())
            && self.state.entities[index].gathering_inside
        {
            let actor = &self.state.entities[index];
            let source = if let Some(depot) = actor.dropoff_target.and_then(|id| self.index(id)) {
                Some((
                    self.state.entities[depot].position,
                    self.unit_at(depot).footprint,
                ))
            } else if let UnitOrder::Gather { resource } = actor.order {
                self.state
                    .resources
                    .iter()
                    .find(|r| r.id == resource)
                    .map(|r| (r.position, r.footprint))
            } else {
                None
            };
            let Some((position, footprint)) = source else {
                return false;
            };
            if !self.leave_interior(index, position, footprint) {
                return false;
            }
        }
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
        self.ai_unit_count(controller, state, unit_type, false)
    }

    fn ai_unit_count(
        &self,
        controller: &AiController,
        state: &AiState,
        unit_type: UnitTypeId,
        include_deployed: bool,
    ) -> u16 {
        let wanted = self.researched_unit_type(controller.player, unit_type);
        self.state
            .entities
            .iter()
            .filter(|e| {
                e.owner == controller.player
                    && (include_deployed
                        || (!state.deployed.contains(&e.id)
                            && !self.state.ai.iter().any(|s| s.deployed.contains(&e.id))))
            })
            .filter(|e| self.ai_in_town(controller, state, e))
            .map(|e| {
                usize::from(e.unit_type == wanted || self.unit_type(e.unit_type).is_some_and(|u| u.provides_types.contains(&unit_type)))
                    + std::iter::once(&e.order).chain(e.queued_orders.iter())
                        .filter(|order| matches!(order, UnitOrder::PlaceBuilding { unit_type: planned, .. } if *planned == wanted)).count()
                    + e.production
                        .iter()
                        .filter(|j| self.researched_unit_type(controller.player, j.unit_type) == wanted)
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
                } => state.request(unit_type, count, priority, false),
                AiInstruction::Maintain {
                    unit_type,
                    count,
                    priority,
                } => state.request(unit_type, count, priority, true),
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
                    if !self.ai_attack(controller, state, false) {
                        return;
                    }
                }
                AiInstruction::AttackAvailable => {
                    self.ai_attack(controller, state, true);
                    state.prepared = false;
                }
                AiInstruction::JumpIfTransportNeeded(target) if state.transport_needed => {
                    state.instruction = target;
                    continue;
                }
                AiInstruction::JumpIfTransportNeeded(_) => {}
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
                self.is_automatic_target(controller.player, e)
                    && self.entity_visible(controller.player, e.id)
                    && !self.unit_type(e.unit_type).is_some_and(|u| u.revealer)
            })
            .min_by_key(|e| (rts::distance(controller.home, e.position), e.id))
            .map(|e| e.position)
            .or_else(|| {
                self.map
                    .start_locations
                    .iter()
                    .filter(|s| {
                        self.is_enemy(controller.player, s.player)
                            && !self.state.defeated.contains(&s.player)
                    })
                    .min_by_key(|s| (rts::distance(controller.home, s.position), s.player))
                    .map(|s| s.position)
            })
    }
}

pub(super) fn put_ai_definition(bytes: &mut Vec<u8>, controllers: &[AiController]) {
    bytes.extend((controllers.len() as u32).to_le_bytes());
    for controller in controllers {
        bytes.extend(controller.player.0.to_le_bytes());
        put_position(bytes, controller.home);
        bytes.extend(controller.radius.to_le_bytes());
        bytes.push(u8::from(controller.active));
        if controller.takeover {
            bytes.extend(b"human-ai-control-v1");
        }
        if !controller.research.is_empty() || !controller.abilities.is_empty() {
            bytes.extend(b"ai-technology-v1");
            put_string(
                bytes,
                &ron::ser::to_string(&(&controller.research, &controller.abilities))
                    .expect("AI technology"),
            );
        }
        if !controller.harvest_weights.is_empty() {
            bytes.extend(b"ai-harvest-allocation-v1");
            rts::put_amounts(bytes, &controller.harvest_weights);
        }
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
                AiInstruction::AttackAvailable => bytes.push(9),
                AiInstruction::JumpIfTransportNeeded(target) => {
                    bytes.push(10);
                    bytes.extend(target.to_le_bytes());
                }
                AiInstruction::Maintain {
                    unit_type,
                    count,
                    priority,
                } => {
                    bytes.push(11);
                    bytes.extend(unit_type.0.to_le_bytes());
                    bytes.extend(count.to_le_bytes());
                    bytes.push(priority);
                }
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
            if r.include_deployed {
                bytes.extend(b"standing-army-v1");
            }
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
