//! The finite research and temporary combat boost needed by the campaign.
//! Definitions contain gameplay effects; names and icons remain presentation data.
use super::*;
use anyhow::Context;
mod upgrades;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ResearchId(pub u16);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Research {
    /// Whether this content package permits starting this research. Completed
    /// progress remains meaningful when an updated package disables the button.
    #[serde(
        default = "available_by_default",
        skip_serializing_if = "available_by_default_value"
    )]
    pub available: bool,
    pub id: ResearchId,
    pub facility: UnitTypeId,
    /// Earlier level of the same upgrade, if any. Completed levels remain saved IDs.
    #[serde(default)]
    pub previous: Option<ResearchId>,
    #[serde(default)]
    pub prerequisites: Vec<UnitTypeId>,
    pub cost: Vec<ResourceAmount>,
    pub ticks: u32,
    pub effect: ResearchEffect,
}

fn available_by_default() -> bool {
    true
}
fn available_by_default_value(value: &bool) -> bool {
    *value
}

/// One technology or one level of an upgrade.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum ResearchEffect {
    /// Replace existing and future recruits while preserving their damage and orders.
    UnitUpgrade {
        units: Vec<UnitTypeId>,
        to: UnitTypeId,
    },
    Regeneration {
        units: Vec<UnitTypeId>,
        amount: u32,
    },
    WeaponUpgrade {
        units: Vec<UnitTypeId>,
        /// Ground and air damage increments, in the same order as `units`.
        bonuses: Vec<[u32; 2]>,
    },
    VisionRange {
        units: Vec<UnitTypeId>,
        amount: u32,
    },
    ShieldArmor {
        units: Vec<UnitTypeId>,
        amount: u32,
    },
    AttackRate {
        units: Vec<UnitTypeId>,
        percent: u16,
    },
    ProductionCapacity {
        units: Vec<UnitTypeId>,
        amount: u8,
    },
    Mode {
        units: Vec<UnitTypeId>,
    },
    Ability {
        units: Vec<UnitTypeId>,
        ability: AbilityId,
    },
    Transport {
        units: Vec<UnitTypeId>,
    },
    Cloak {
        units: Vec<UnitTypeId>,
    },
    Mines {
        units: Vec<UnitTypeId>,
    },
    EnergyCapacity {
        units: Vec<UnitTypeId>,
        amount: u32,
    },
    MovementSpeed {
        units: Vec<UnitTypeId>,
        percent: u16,
        acceleration_percent: u16,
    },
    WeaponDamage {
        units: Vec<UnitTypeId>,
        amount: u32,
    },
    Armor {
        units: Vec<UnitTypeId>,
        amount: u32,
    },
    WeaponRange {
        units: Vec<UnitTypeId>,
        amount: u32,
        /// Some range upgrades also extend sight to cover the new firing range.
        #[serde(default, skip_serializing_if = "zero_sight")]
        sight: u32,
    },
    Stim {
        units: Vec<UnitTypeId>,
        hp_cost: u32,
        duration_ticks: u32,
    },
}
fn zero_sight(value: &u32) -> bool {
    *value == 0
}

impl ResearchEffect {
    fn units(&self) -> &[UnitTypeId] {
        match self {
            Self::UnitUpgrade { units, .. }
            | Self::Regeneration { units, .. }
            | Self::WeaponUpgrade { units, .. }
            | Self::VisionRange { units, .. }
            | Self::ShieldArmor { units, .. }
            | Self::AttackRate { units, .. }
            | Self::ProductionCapacity { units, .. }
            | Self::Transport { units }
            | Self::Mode { units }
            | Self::Ability { units, .. }
            | Self::WeaponDamage { units, .. }
            | Self::Armor { units, .. }
            | Self::WeaponRange { units, .. }
            | Self::Stim { units, .. }
            | Self::Cloak { units }
            | Self::Mines { units }
            | Self::EnergyCapacity { units, .. }
            | Self::MovementSpeed { units, .. } => units,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResearchJob {
    pub id: ResearchId,
    pub remaining: u32,
    pub total: u32,
}

pub(super) fn validate_research_rules(rules: &Rules) -> Result<()> {
    ensure!(rules.research.len() <= 128, "too many research definitions");
    let mut ids = BTreeSet::new();
    let mut effects = BTreeSet::new();
    for research in &rules.research {
        ensure!(ids.insert(research.id), "duplicate research ID");
        ensure!(
            research.previous.is_none_or(|id| id < research.id
                && rules
                    .research
                    .iter()
                    .any(|r| r.id == id && r.facility == research.facility)),
            "invalid previous upgrade level"
        );
        ensure!(
            research
                .prerequisites
                .iter()
                .all(|id| rules.units.iter().any(|u| u.id == *id)),
            "unknown research prerequisite"
        );
        ensure!(
            (1..=1_000_000).contains(&research.ticks),
            "invalid research duration"
        );
        rts::validate_amounts(&research.cost)?;
        ensure!(
            rules
                .units
                .iter()
                .any(|unit| unit.id == research.facility && unit.structure),
            "research requires a structure facility"
        );
        let units = research.effect.units();
        ensure!(
            !units.is_empty() && units.len() <= 4096,
            "invalid research target count"
        );
        let mut targets = BTreeSet::new();
        for &id in units {
            ensure!(targets.insert(id), "duplicate research effect target");
            let unit = rules
                .units
                .iter()
                .find(|unit| unit.id == id)
                .context("unknown research target type")?;
            let tag = match research.effect {
                ResearchEffect::UnitUpgrade { .. } => 15,
                ResearchEffect::Regeneration { .. } => 16,
                ResearchEffect::WeaponUpgrade { .. } => 0,
                ResearchEffect::VisionRange { .. } => 11,
                ResearchEffect::ShieldArmor { .. } => 12,
                ResearchEffect::AttackRate { .. } => 13,
                ResearchEffect::ProductionCapacity { .. } => 14,
                ResearchEffect::Mode { .. } => 10,
                ResearchEffect::Ability { ability, .. } => 256 + u32::from(ability.0),
                ResearchEffect::Transport { .. } => 8,
                ResearchEffect::WeaponDamage { .. } => 0,
                ResearchEffect::Armor { .. } => 1,
                ResearchEffect::WeaponRange { .. } => 2,
                ResearchEffect::Stim { .. } => 3,
                ResearchEffect::Cloak { .. } => 4,
                ResearchEffect::Mines { .. } => 5,
                ResearchEffect::EnergyCapacity { .. } => 6,
                ResearchEffect::MovementSpeed { .. } => 7,
            };
            ensure!(
                effects.insert((id, tag)) || research.previous.is_some(),
                "overlapping single-level research effect"
            );
            match research.effect {
                ResearchEffect::UnitUpgrade { to, .. } => ensure!(
                    rules.units.iter().any(|u| u.id == to
                        && !u.structure
                        && u.movement_class == unit.movement_class
                        && u.footprint == unit.footprint)
                        && to != id
                        && !unit.structure,
                    "invalid researched unit replacement"
                ),
                ResearchEffect::Regeneration { amount, .. } => ensure!(
                    (1..=25600).contains(&amount),
                    "invalid researched regeneration"
                ),
                ResearchEffect::WeaponUpgrade { ref bonuses, .. } => ensure!(
                    bonuses.len() == units.len()
                        && bonuses.iter().all(|b| b.iter().all(|n| *n <= 10000)),
                    "invalid weapon upgrade bonuses"
                ),
                ResearchEffect::VisionRange { amount, .. } => {
                    ensure!(amount <= 32768, "invalid researched vision")
                }
                ResearchEffect::ShieldArmor { amount, .. } => ensure!(
                    unit.max_shields > 0 && amount <= 1000,
                    "invalid shield armor"
                ),
                ResearchEffect::AttackRate { percent, .. } => ensure!(
                    unit.weapon.is_some() && (101..=400).contains(&percent),
                    "invalid researched attack rate"
                ),
                ResearchEffect::ProductionCapacity { amount, .. } => ensure!(
                    unit.production_capacity > 0 && amount <= 64,
                    "invalid researched production capacity"
                ),
                ResearchEffect::Mode { .. } => ensure!(
                    unit.mode.is_some(),
                    "mode research targets a unit without modes"
                ),
                ResearchEffect::Ability { ability, .. } => ensure!(
                    unit.abilities.iter().any(|a| a.id == ability),
                    "research targets an unavailable ability"
                ),
                ResearchEffect::Transport { .. } => ensure!(
                    unit.garrison.is_some(),
                    "transport research targets a unit without cargo capacity"
                ),
                ResearchEffect::Cloak { .. } => ensure!(
                    unit.cloak.is_some(),
                    "cloak research targets a unit without cloak"
                ),
                ResearchEffect::Mines { .. } => ensure!(
                    unit.mine_layer.is_some(),
                    "mine research targets a unit without mines"
                ),
                ResearchEffect::EnergyCapacity { amount, .. } => ensure!(
                    unit.energy_max() > 0 && (1..=1000).contains(&amount),
                    "invalid researched energy capacity"
                ),
                ResearchEffect::MovementSpeed {
                    percent,
                    acceleration_percent,
                    ..
                } => ensure!(
                    unit.speed > 0
                        && (101..=400).contains(&percent)
                        && (100..=400).contains(&acceleration_percent),
                    "invalid researched movement speed"
                ),
                ResearchEffect::WeaponRange { amount, sight, .. } => ensure!(
                    unit.weapon.is_some() && (1..=1_000_000).contains(&amount) && sight <= 32768,
                    "invalid researched weapon range"
                ),
                ResearchEffect::WeaponDamage { amount, .. } => ensure!(
                    unit.weapon.is_some() && (1..=1_000_000).contains(&amount),
                    "invalid researched weapon effect"
                ),
                ResearchEffect::Armor { amount, .. } => ensure!(
                    (1..=1_000_000).contains(&amount),
                    "invalid researched armor"
                ),
                ResearchEffect::Stim {
                    hp_cost,
                    duration_ticks,
                    ..
                } => ensure!(
                    unit.weapon.is_some()
                        && !unit.structure
                        && hp_cost > 0
                        && hp_cost < unit.max_hp
                        && (1..=1_000_000).contains(&duration_ticks),
                    "invalid temporary combat boost"
                ),
            }
        }
    }
    upgrades::validate(rules)?;
    Ok(())
}

impl World {
    pub fn energy_max(&self, entity: &Entity) -> u32 {
        self.unit_type(entity.unit_type).unwrap().energy_max()
            + self.research_bonus(entity.owner, entity.unit_type, 6)
    }
    pub fn transport_ready(&self, entity: &Entity) -> bool {
        self.rules.research.iter().filter(|research| matches!(&research.effect, ResearchEffect::Transport { units } if units.contains(&entity.unit_type))).all(|research| self.has_research(entity.owner, research.id))
    }
    pub(super) fn ability_research_ready(&self, entity: &Entity, cloak: bool) -> bool {
        self.rules
            .research
            .iter()
            .filter(|research| {
                matches!(
                    (&research.effect, cloak),
                    (ResearchEffect::Cloak { .. }, true) | (ResearchEffect::Mines { .. }, false)
                ) && research.effect.units().contains(&entity.unit_type)
            })
            .all(|research| self.has_research(entity.owner, research.id))
    }
    pub(super) fn researched_motion(&self, entity: &Entity) -> Option<(u16, u16)> {
        self.rules
            .research
            .iter()
            .filter(|research| self.has_research(entity.owner, research.id))
            .find_map(|research| match &research.effect {
                ResearchEffect::MovementSpeed {
                    units,
                    percent,
                    acceleration_percent,
                } if units.contains(&entity.unit_type) => Some((*percent, *acceleration_percent)),
                _ => None,
            })
    }
    pub fn research(&self, id: ResearchId) -> Option<&Research> {
        self.rules
            .research
            .iter()
            .find(|research| research.id == id)
    }
    pub fn has_research(&self, player: PlayerId, id: ResearchId) -> bool {
        self.state
            .players
            .get(usize::from(player.0))
            .is_some_and(|state| state.completed_research.contains(&id))
    }
    pub fn research_rejection(
        &self,
        player: PlayerId,
        entity: EntityId,
        id: ResearchId,
    ) -> Option<Rejection> {
        let Some(actor) = self.state.entities.iter().find(|actor| actor.id == entity) else {
            return Some(Rejection::UnknownEntity);
        };
        if actor.owner != player {
            return Some(Rejection::NotOwner);
        }
        let Some(research) = self.research(id) else {
            return Some(Rejection::UnsupportedOrder);
        };
        if !research.available {
            return Some(Rejection::UnsupportedOrder);
        }
        if actor.unit_type != research.facility
            && !self
                .unit_type(actor.unit_type)?
                .provides_types
                .contains(&research.facility)
        {
            return Some(Rejection::UnsupportedOrder);
        }
        if !self.powered(actor) {
            return Some(Rejection::MissingPrerequisite);
        }
        if research
            .previous
            .is_some_and(|previous| !self.has_research(player, previous))
            || !research.prerequisites.iter().all(|id| {
                self.state.entities.iter().any(|e| {
                    e.owner == player
                        && e.hp > 0
                        && e.construction.is_none()
                        && !e.airborne
                        && (e.unit_type == *id
                            || self
                                .unit_type(e.unit_type)
                                .unwrap()
                                .provides_types
                                .contains(id))
                })
            })
        {
            return Some(Rejection::MissingPrerequisite);
        }
        if actor.airborne {
            return Some(Rejection::UnsupportedOrder);
        }
        if let Some(parent_type) = self.unit_type(actor.unit_type)?.addon_parent
            && !actor
                .parent
                .and_then(|id| self.state.entities.iter().find(|parent| parent.id == id))
                .is_some_and(|parent| {
                    parent.unit_type == parent_type
                        && parent.owner == player
                        && parent.construction.is_none()
                        && !parent.airborne
                })
        {
            return Some(Rejection::MissingPrerequisite);
        }
        if actor.construction.is_some() {
            return Some(Rejection::Unfinished);
        }
        if actor.research.is_some() || !actor.production.is_empty() || self.addon_pending(actor.id)
        {
            return Some(Rejection::QueueFull);
        }
        if self.has_research(player, id)
            || self.state.entities.iter().any(|other| {
                other.owner == player && other.research.as_ref().is_some_and(|job| job.id == id)
            })
        {
            return Some(Rejection::InvalidTarget);
        }
        if !self.can_pay(player, &research.cost) {
            return Some(Rejection::InsufficientResources);
        }
        None
    }
    pub(super) fn start_research(&mut self, index: usize, id: ResearchId) -> Option<Rejection> {
        let actor = &self.state.entities[index];
        if let Some(rejection) = self.research_rejection(actor.owner, actor.id, id) {
            return Some(rejection);
        }
        let owner = actor.owner;
        let research = self.research(id).expect("validated research").clone();
        self.pay(owner, &research.cost, false);
        self.state.entities[index].research = Some(ResearchJob {
            id,
            remaining: research.ticks,
            total: research.ticks,
        });
        None
    }
    /// Returns true only if a research job was cancelled. Existing construction
    /// or production cancellation can continue when this returns false.
    pub(super) fn cancel_research(&mut self, index: usize) -> bool {
        let Some(job) = self.state.entities[index].research.take() else {
            return false;
        };
        let owner = self.state.entities[index].owner;
        let cost = self.research(job.id).expect("validated job").cost.clone();
        self.pay(owner, &cost, true);
        true
    }
    pub(super) fn advance_research(&mut self) {
        let powered: BTreeSet<_> = self
            .state
            .entities
            .iter()
            .filter(|e| self.powered(e))
            .map(|e| e.id)
            .collect();
        for entity in &mut self.state.entities {
            entity.stim_remaining = entity.stim_remaining.saturating_sub(1);
            let Some(job) = &mut entity.research else {
                continue;
            };
            if entity.construction.is_some() || !powered.contains(&entity.id) {
                continue;
            }
            job.remaining = job.remaining.saturating_sub(1);
            if job.remaining == 0 {
                self.state.players[usize::from(entity.owner.0)]
                    .completed_research
                    .insert(job.id);
                entity.research = None;
            }
        }
        self.apply_unit_upgrades();
    }
    fn stim_definition(&self, entity: &Entity) -> Option<(u32, u32)> {
        self.rules.research.iter().find_map(|research| {
            if !self.has_research(entity.owner, research.id) {
                return None;
            }
            match &research.effect {
                ResearchEffect::Stim {
                    units,
                    hp_cost,
                    duration_ticks,
                } if units.contains(&entity.unit_type) => Some((*hp_cost, *duration_ticks)),
                _ => None,
            }
        })
    }
    pub fn stim_rejection(&self, entity: EntityId) -> Option<Rejection> {
        let Some(actor) = self.state.entities.iter().find(|actor| actor.id == entity) else {
            return Some(Rejection::UnknownEntity);
        };
        if actor.construction.is_some() {
            return Some(Rejection::Unfinished);
        }
        let Some((cost, _)) = self.stim_definition(actor) else {
            return Some(Rejection::MissingPrerequisite);
        };
        if actor.hp <= cost {
            return Some(Rejection::InvalidTarget);
        }
        None
    }
    pub(super) fn use_stim(&mut self, index: usize) -> Option<Rejection> {
        let actor = &self.state.entities[index];
        if let Some(rejection) = self.stim_rejection(actor.id) {
            return Some(rejection);
        }
        let (cost, duration) = self.stim_definition(actor).expect("validated boost");
        self.state.entities[index].hp -= cost;
        self.state.entities[index].stim_remaining = duration;
        None
    }
    pub fn research_damage_bonus(&self, player: PlayerId, unit: UnitTypeId) -> u32 {
        self.research_weapon_bonus(player, unit, false)
    }
    pub fn research_weapon_bonus(&self, player: PlayerId, unit: UnitTypeId, air: bool) -> u32 {
        self.research_bonus(player, unit, 0)
            + self
                .rules
                .research
                .iter()
                .filter(|r| self.has_research(player, r.id))
                .filter_map(|r| match &r.effect {
                    ResearchEffect::WeaponUpgrade { units, bonuses } => units
                        .iter()
                        .position(|id| *id == unit)
                        .map(|i| bonuses[i][usize::from(air)]),
                    _ => None,
                })
                .sum::<u32>()
    }
    pub fn vision_range(&self, entity: &Entity) -> u32 {
        self.unit_type(entity.unit_type).unwrap().vision_range
            + self.research_bonus(entity.owner, entity.unit_type, 11)
    }
    pub fn production_capacity(&self, entity: &Entity) -> u32 {
        u32::from(
            self.unit_type(entity.unit_type)
                .unwrap()
                .production_capacity,
        ) + self.research_bonus(entity.owner, entity.unit_type, 14)
    }
    pub fn research_level_visible(&self, player: PlayerId, research: &Research) -> bool {
        research.available
            && research
                .previous
                .is_none_or(|previous| self.has_research(player, previous))
            && !self.rules.research.iter().any(|next| {
                next.previous == Some(research.id) && self.has_research(player, research.id)
            })
    }
    pub(in crate::sim) fn researched_cooldown(&self, entity: &Entity, cooldown: u32) -> u32 {
        let percent = self
            .research_bonus(entity.owner, entity.unit_type, 13)
            .max(100)
            * self.buff_percent(entity, true)
            / 100;
        let slow = if entity.ability_auras.iter().any(|a| {
            matches!(
                self.effect_definition(a.ability),
                Some(AbilityEffect::SlowArea { .. })
            )
        }) {
            125
        } else {
            100
        };
        (cooldown * slow / percent).max(if slow != 100 || percent != 100 { 5 } else { 1 })
    }
    pub fn research_armor_bonus(&self, player: PlayerId, unit: UnitTypeId) -> u32 {
        self.research_bonus(player, unit, 1)
    }
    pub fn research_range_bonus(&self, player: PlayerId, unit: UnitTypeId) -> u32 {
        self.research_bonus(player, unit, 2)
    }
    pub(in crate::sim) fn research_bonus(
        &self,
        player: PlayerId,
        unit: UnitTypeId,
        tag: u8,
    ) -> u32 {
        self.rules
            .research
            .iter()
            .filter(|research| {
                self.has_research(player, research.id) && research.effect.units().contains(&unit)
            })
            .filter_map(|research| match (&research.effect, tag) {
                (ResearchEffect::WeaponDamage { amount, .. }, 0)
                | (ResearchEffect::Armor { amount, .. }, 1)
                | (ResearchEffect::WeaponRange { amount, .. }, 2)
                | (ResearchEffect::EnergyCapacity { amount, .. }, 6) => Some(*amount),
                (ResearchEffect::Regeneration { amount, .. }, 16) => Some(*amount),
                (ResearchEffect::WeaponRange { sight, .. }, 11) => Some(*sight),
                (ResearchEffect::VisionRange { amount, .. }, 11)
                | (ResearchEffect::ShieldArmor { amount, .. }, 12) => Some(*amount),
                (ResearchEffect::AttackRate { percent, .. }, 13) => Some(u32::from(*percent)),
                (ResearchEffect::ProductionCapacity { amount, .. }, 14) => Some(u32::from(*amount)),
                _ => None,
            })
            .sum()
    }
}

pub(super) fn put_research_rules(bytes: &mut Vec<u8>, rules: &Rules) {
    bytes.extend((rules.research.len() as u32).to_le_bytes());
    for research in &rules.research {
        if !research.available {
            bytes.extend(b"research-unavailable-v1");
        }
        bytes.extend(research.id.0.to_le_bytes());
        bytes.extend(research.facility.0.to_le_bytes());
        bytes.extend(research.ticks.to_le_bytes());
        bytes.extend((research.cost.len() as u32).to_le_bytes());
        for cost in &research.cost {
            put_string(bytes, &cost.kind);
            bytes.extend(cost.amount.to_le_bytes());
        }
        match &research.effect {
            ResearchEffect::UnitUpgrade { to, .. } => {
                bytes.push(16);
                bytes.extend(to.0.to_le_bytes());
            }
            ResearchEffect::Regeneration { amount, .. } => {
                bytes.push(17);
                bytes.extend(amount.to_le_bytes());
            }
            ResearchEffect::WeaponUpgrade { bonuses, .. } => {
                bytes.push(11);
                for pair in bonuses {
                    for n in pair {
                        bytes.extend(n.to_le_bytes());
                    }
                }
            }
            ResearchEffect::VisionRange { amount, .. } => {
                bytes.push(12);
                bytes.extend(amount.to_le_bytes());
            }
            ResearchEffect::ShieldArmor { amount, .. } => {
                bytes.push(13);
                bytes.extend(amount.to_le_bytes());
            }
            ResearchEffect::AttackRate { percent, .. } => {
                bytes.push(14);
                bytes.extend(percent.to_le_bytes());
            }
            ResearchEffect::ProductionCapacity { amount, .. } => {
                bytes.push(15);
                bytes.push(*amount);
            }
            ResearchEffect::Mode { .. } => bytes.push(10),
            ResearchEffect::Ability { ability, .. } => {
                bytes.push(9);
                bytes.extend(ability.0.to_le_bytes());
            }
            ResearchEffect::Transport { .. } => bytes.push(8),
            ResearchEffect::Cloak { .. } => bytes.push(4),
            ResearchEffect::Mines { .. } => bytes.push(5),
            ResearchEffect::EnergyCapacity { amount, .. } => {
                bytes.push(6);
                bytes.extend(amount.to_le_bytes());
            }
            ResearchEffect::MovementSpeed {
                percent,
                acceleration_percent,
                ..
            } => {
                bytes.push(7);
                bytes.extend(percent.to_le_bytes());
                bytes.extend(acceleration_percent.to_le_bytes());
            }
            ResearchEffect::WeaponDamage { amount, .. } => {
                bytes.push(0);
                bytes.extend(amount.to_le_bytes());
            }
            ResearchEffect::Armor { amount, .. } => {
                bytes.push(1);
                bytes.extend(amount.to_le_bytes());
            }
            ResearchEffect::WeaponRange { amount, sight, .. } => {
                bytes.push(2);
                bytes.extend(amount.to_le_bytes());
                if *sight != 0 {
                    bytes.extend(b"weapon-range-sight-v1");
                    bytes.extend(sight.to_le_bytes());
                }
            }
            ResearchEffect::Stim {
                hp_cost,
                duration_ticks,
                ..
            } => {
                bytes.push(3);
                bytes.extend(hp_cost.to_le_bytes());
                bytes.extend(duration_ticks.to_le_bytes());
            }
        }
        let units = research.effect.units();
        bytes.extend((units.len() as u32).to_le_bytes());
        for id in units {
            bytes.extend(id.0.to_le_bytes());
        }
        if research.previous.is_some() || !research.prerequisites.is_empty() {
            bytes.extend(b"research-requirements-v1");
            bytes.extend(research.previous.map_or(0, |id| id.0).to_le_bytes());
            bytes.extend((research.prerequisites.len() as u32).to_le_bytes());
            for id in &research.prerequisites {
                bytes.extend(id.0.to_le_bytes());
            }
        }
    }
}
pub(super) fn put_research_state(bytes: &mut Vec<u8>, state: &State) {
    bytes.extend((state.players.len() as u32).to_le_bytes());
    for player in &state.players {
        bytes.extend((player.completed_research.len() as u32).to_le_bytes());
        for id in &player.completed_research {
            bytes.extend(id.0.to_le_bytes());
        }
    }
    bytes.extend((state.entities.len() as u32).to_le_bytes());
    for entity in &state.entities {
        bytes.extend(entity.id.0.to_le_bytes());
        bytes.extend(entity.stim_remaining.to_le_bytes());
        bytes.push(u8::from(entity.research.is_some()));
        if let Some(job) = &entity.research {
            bytes.extend(job.id.0.to_le_bytes());
            bytes.extend(job.remaining.to_le_bytes());
            bytes.extend(job.total.to_le_bytes());
        }
    }
}

#[cfg(test)]
mod tests;
