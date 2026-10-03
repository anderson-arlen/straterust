//! The finite research and temporary combat boost needed by the campaign.
//! Definitions contain gameplay effects; names and icons remain presentation data.
use super::*;
use anyhow::Context;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ResearchId(pub u16);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Research {
    pub id: ResearchId,
    pub facility: UnitTypeId,
    pub cost: Vec<ResourceAmount>,
    pub ticks: u32,
    pub effect: ResearchEffect,
}

/// Each imported upgrade in this mission has exactly one available level.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum ResearchEffect {
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
    },
    Stim {
        units: Vec<UnitTypeId>,
        hp_cost: u32,
        duration_ticks: u32,
    },
}
impl ResearchEffect {
    fn units(&self) -> &[UnitTypeId] {
        match self {
            Self::WeaponDamage { units, .. }
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
                effects.insert((id, tag)),
                "overlapping single-level research effect"
            );
            match research.effect {
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
                ResearchEffect::WeaponDamage { amount, .. }
                | ResearchEffect::WeaponRange { amount, .. } => ensure!(
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
    Ok(())
}

impl World {
    pub fn energy_max(&self, entity: &Entity) -> u32 {
        self.unit_type(entity.unit_type).unwrap().energy_max()
            + self.research_bonus(entity.owner, entity.unit_type, 6)
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
        if actor.unit_type != research.facility {
            return Some(Rejection::UnsupportedOrder);
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
        for entity in &mut self.state.entities {
            entity.stim_remaining = entity.stim_remaining.saturating_sub(1);
            let Some(job) = &mut entity.research else {
                continue;
            };
            if entity.construction.is_some() {
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
        self.research_bonus(player, unit, 0)
    }
    pub fn research_armor_bonus(&self, player: PlayerId, unit: UnitTypeId) -> u32 {
        self.research_bonus(player, unit, 1)
    }
    pub fn research_range_bonus(&self, player: PlayerId, unit: UnitTypeId) -> u32 {
        self.research_bonus(player, unit, 2)
    }
    fn research_bonus(&self, player: PlayerId, unit: UnitTypeId, tag: u8) -> u32 {
        self.rules
            .research
            .iter()
            .filter(|research| {
                self.has_research(player, research.id) && research.effect.units().contains(&unit)
            })
            .find_map(|research| match (&research.effect, tag) {
                (ResearchEffect::WeaponDamage { amount, .. }, 0)
                | (ResearchEffect::Armor { amount, .. }, 1)
                | (ResearchEffect::WeaponRange { amount, .. }, 2)
                | (ResearchEffect::EnergyCapacity { amount, .. }, 6) => Some(*amount),
                _ => None,
            })
            .unwrap_or(0)
    }
}

pub(super) fn put_research_rules(bytes: &mut Vec<u8>, rules: &Rules) {
    bytes.extend((rules.research.len() as u32).to_le_bytes());
    for research in &rules.research {
        bytes.extend(research.id.0.to_le_bytes());
        bytes.extend(research.facility.0.to_le_bytes());
        bytes.extend(research.ticks.to_le_bytes());
        bytes.extend((research.cost.len() as u32).to_le_bytes());
        for cost in &research.cost {
            put_string(bytes, &cost.kind);
            bytes.extend(cost.amount.to_le_bytes());
        }
        match &research.effect {
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
            ResearchEffect::WeaponRange { amount, .. } => {
                bytes.push(2);
                bytes.extend(amount.to_le_bytes());
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
mod tests {
    use super::*;
    fn world() -> World {
        let soldier = UnitType {
            id: UnitTypeId(1),
            max_hp: 40,
            speed: 3,
            weapon: Some(Weapon {
                cooldown_jitter: None,
                targets_air: false,
                damage: 6,
                range: 32,
                cooldown: 15,
                damage_kind: DamageKind::Normal,
                splash: None,
                strikes: Vec::new(),
            }),
            ..UnitType::default()
        };
        let lab = UnitType {
            id: UnitTypeId(2),
            max_hp: 100,
            structure: true,
            speed: 0,
            ..UnitType::default()
        };
        let mut research = Vec::new();
        for (id, effect) in [
            (
                1,
                ResearchEffect::WeaponDamage {
                    units: vec![UnitTypeId(1)],
                    amount: 1,
                },
            ),
            (
                2,
                ResearchEffect::Armor {
                    units: vec![UnitTypeId(1)],
                    amount: 1,
                },
            ),
            (
                3,
                ResearchEffect::WeaponRange {
                    units: vec![UnitTypeId(1)],
                    amount: 32,
                },
            ),
            (
                4,
                ResearchEffect::Stim {
                    units: vec![UnitTypeId(1)],
                    hp_cost: 10,
                    duration_ticks: 8,
                },
            ),
        ] {
            research.push(Research {
                id: ResearchId(id),
                facility: UnitTypeId(2),
                cost: vec![ResourceAmount {
                    kind: "minerals".into(),
                    amount: 100,
                }],
                ticks: 3,
                effect,
            });
        }
        let rules = Rules {
            id: "synthetic.research".into(),
            units: vec![soldier, lab],
            starting_resources: vec![ResourceAmount {
                kind: "minerals".into(),
                amount: 1000,
            }],
            research,
            ..Rules::default()
        };
        let mut spawns = Vec::new();
        for (owner, unit, x) in [(0, 1, 24), (0, 2, 80), (1, 2, 256), (0, 2, 112)] {
            spawns.push(Spawn {
                doodad_enabled: None,
                owner: PlayerId(owner),
                unit_type: UnitTypeId(unit),
                position: Position { x, y: 32 },
                hp_percent: None,
                energy_percent: None,
                invincible: false,
                burrowed: false,
            });
        }
        World::new(
            rules,
            Map {
                id: "synthetic.research".into(),
                width: 512,
                height: 128,
                players: 2,
                spawns,
                start_locations: Vec::new(),
                resources: Vec::new(),
                initial_explored: Default::default(),
                creation: Default::default(),
                ai: Vec::new(),
                mission: None,
                terrain: None,
                fog_of_war: false,
            },
            1,
        )
        .unwrap()
    }
    fn order(world: &mut World, order: Order) -> Option<Rejection> {
        let sequence = world.state.last_sequences[0] + 1;
        world
            .step(&[Command {
                tick: world.tick(),
                player: PlayerId(0),
                sequence,
                order,
            }])
            .unwrap()
            .remove(0)
            .rejection
    }
    fn finish(world: &mut World, id: u16) {
        assert_eq!(
            order(
                world,
                Order::Research {
                    entity: EntityId(2),
                    research: ResearchId(id)
                }
            ),
            None
        );
        world.step(&[]).unwrap();
        world.step(&[]).unwrap();
        assert!(world.has_research(PlayerId(0), ResearchId(id)));
    }
    #[test]
    fn research_checks_owner_facility_funds_and_duplicate_jobs_and_refunds_cancel() {
        let mut w = world();
        assert_eq!(
            w.research_rejection(PlayerId(0), EntityId(3), ResearchId(1)),
            Some(Rejection::NotOwner)
        );
        assert_eq!(
            w.research_rejection(PlayerId(0), EntityId(1), ResearchId(1)),
            Some(Rejection::UnsupportedOrder)
        );
        assert_eq!(
            order(
                &mut w,
                Order::Research {
                    entity: EntityId(2),
                    research: ResearchId(1)
                }
            ),
            None
        );
        assert_eq!(w.resource_balance(PlayerId(0), "minerals"), 900);
        assert_eq!(
            w.research_rejection(PlayerId(0), EntityId(4), ResearchId(1)),
            Some(Rejection::InvalidTarget)
        );
        assert_eq!(
            w.research_rejection(PlayerId(0), EntityId(2), ResearchId(2)),
            Some(Rejection::QueueFull)
        );
        assert_eq!(
            order(
                &mut w,
                Order::Cancel {
                    entity: EntityId(2)
                }
            ),
            None
        );
        assert_eq!(w.resource_balance(PlayerId(0), "minerals"), 1000);
        assert!(!w.has_research(PlayerId(0), ResearchId(1)));
        w.state.players[0].resources.insert("minerals".into(), 99);
        assert_eq!(
            w.research_rejection(PlayerId(0), EntityId(2), ResearchId(1)),
            Some(Rejection::InsufficientResources)
        );
    }
    #[test]
    fn completed_research_is_owner_scoped_persistent_and_not_repeatable() {
        let mut w = world();
        for id in 1..=3 {
            finish(&mut w, id);
        }
        assert_eq!(w.research_damage_bonus(PlayerId(0), UnitTypeId(1)), 1);
        assert_eq!(w.research_armor_bonus(PlayerId(0), UnitTypeId(1)), 1);
        assert_eq!(w.research_range_bonus(PlayerId(0), UnitTypeId(1)), 32);
        assert_eq!(w.research_damage_bonus(PlayerId(1), UnitTypeId(1)), 0);
        assert_eq!(
            w.research_rejection(PlayerId(0), EntityId(2), ResearchId(1)),
            Some(Rejection::InvalidTarget)
        );
        w.state.entities.retain(|e| e.id != EntityId(2));
        assert_eq!(w.research_damage_bonus(PlayerId(0), UnitTypeId(1)), 1);
    }
    #[test]
    fn stim_cost_refresh_and_expiry_preserve_hp_and_prevent_self_kill() {
        let mut w = world();
        assert_eq!(
            w.stim_rejection(EntityId(1)),
            Some(Rejection::MissingPrerequisite)
        );
        finish(&mut w, 4);
        assert_eq!(
            order(
                &mut w,
                Order::Stim {
                    entity: EntityId(1)
                }
            ),
            None
        );
        assert_eq!(w.state.entities[0].hp, 30);
        let first = w.state.entities[0].stim_remaining;
        assert!(first > 0 && first <= 8);
        w.step(&[]).unwrap();
        assert_eq!(
            order(
                &mut w,
                Order::Stim {
                    entity: EntityId(1)
                }
            ),
            None
        );
        assert_eq!(w.state.entities[0].hp, 20);
        assert_eq!(w.state.entities[0].stim_remaining, first);
        assert_eq!(
            order(
                &mut w,
                Order::Stim {
                    entity: EntityId(1)
                }
            ),
            None
        );
        assert_eq!(w.state.entities[0].hp, 10);
        assert_eq!(
            w.stim_rejection(EntityId(1)),
            Some(Rejection::InvalidTarget)
        );
        for _ in 0..8 {
            w.step(&[]).unwrap();
        }
        assert_eq!(w.state.entities[0].stim_remaining, 0);
        assert_eq!(w.state.entities[0].hp, 10);
    }
    #[test]
    fn research_job_completion_and_boost_are_canonical_and_deterministic() {
        let mut a = world();
        let mut b = world();
        finish(&mut a, 4);
        finish(&mut b, 4);
        assert_eq!(
            order(
                &mut a,
                Order::Stim {
                    entity: EntityId(1)
                }
            ),
            None
        );
        assert_eq!(
            order(
                &mut b,
                Order::Stim {
                    entity: EntityId(1)
                }
            ),
            None
        );
        assert_eq!(a.state_hash(), b.state_hash());
        b.state.entities[0].stim_remaining -= 1;
        assert_ne!(a.state_hash(), b.state_hash());
        b.state.entities[0].stim_remaining += 1;
        b.state.players[0].completed_research.clear();
        assert_ne!(a.state_hash(), b.state_hash());
        let mut invalid = a.rules().clone();
        invalid.research.push(invalid.research[0].clone());
        assert!(validate_research_rules(&invalid).is_err());
        invalid = a.rules().clone();
        invalid.research[0].effect = ResearchEffect::WeaponDamage {
            units: vec![UnitTypeId(600)],
            amount: 1,
        };
        assert!(validate_research_rules(&invalid).is_err());
    }
}
