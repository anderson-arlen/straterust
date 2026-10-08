//! Content-defined targeted effects. Commands, energy and ongoing damage are
//! authoritative; the client receives only observable cast/overlay information.
use super::*;
mod links;
pub(in crate::sim) use links::initialize_spawn_links;
mod effects;
pub use effects::{AbilityField, PendingEffect};
mod strikes;
pub use strikes::{StrikeAppearance, StrikeDelivery, StrikeFlight, StrikeStage};
#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AbilityId(pub u16);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnergyPool {
    pub maximum: u32,
    pub initial: u32,
    /// 1/256 energy per tick.
    pub regeneration: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetedAbility {
    pub id: AbilityId,
    pub research: Option<ResearchId>,
    pub energy: u32,
    pub range: u32,
    pub effect: AbilityEffect,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum AbilityEffect {
    Disable {
        radius: u32,
        duration: u32,
        invulnerable: bool,
        affected: Vec<UnitTypeId>,
    },
    Barrier {
        duration: u32,
        amount: u32,
    },
    Strike {
        damage: u32,
        kind: DamageKind,
        radii: Option<[u32; 3]>,
        delay: u32,
        channel: u32,
        ammunition: Option<UnitTypeId>,
        max_health_fraction: Option<[u16; 2]>,
        #[serde(default)]
        delivery: Option<StrikeDelivery>,
    },
    AreaDamage {
        radius: u32,
        damage_fp8: u32,
        period: u32,
        duration: u32,
        lethal: bool,
        shields: bool,
    },
    SlowArea {
        radius: u32,
        duration: u32,
        percent: u16,
    },
    Consume {
        affected: Vec<UnitTypeId>,
        energy: u32,
    },
    KillSpawn {
        affected: Vec<UnitTypeId>,
        unit: UnitTypeId,
        count: u8,
        lifetime: u32,
    },
    Infest {
        from: Vec<UnitTypeId>,
        to: UnitTypeId,
        max_hp_percent: u8,
    },
    Parasite,
    Protection {
        radius: u32,
        duration: u32,
    },
    Illusions {
        count: u8,
        lifetime: u32,
    },
    Recall {
        radius: u32,
        delay: u32,
    },
    Merge {
        partner: UnitTypeId,
        result: UnitTypeId,
        delay: u32,
    },
    Recharge {
        rate: u32,
        shield_per_energy: u32,
    },
    /// Create one linked exit; eligible recipients walk in and emerge there.
    LinkedTransport {
        exit: UnitTypeId,
        passengers: Vec<UnitTypeId>,
    },
    DrainArea {
        radius: u32,
    },
    DamageAura {
        radius: u32,
        damage_fp8: u32,
        period: u32,
        duration: u32,
        affected: Vec<UnitTypeId>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum AbilityTarget {
    Unit(EntityId),
    Point(Position),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AbilityAura {
    pub ability: AbilityId,
    pub remaining: u32,
    #[serde(default)]
    pub strength: u32,
    /// Attribution is server-only and stripped from every player projection.
    pub source: Option<EntityId>,
    pub owner: PlayerId,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CastAppearance {
    pub ability: AbilityId,
    pub tick: Tick,
    pub position: Position,
    pub origin: Position,
}

pub(super) fn validate(rules: &Rules) -> Result<()> {
    let mut definitions = BTreeMap::new();
    for unit in &rules.units {
        if let Some(pool) = &unit.energy_pool {
            ensure!(
                unit.cloak.as_ref().is_none_or(|c| c.energy_max == 0) && unit.scanner.is_none(),
                "duplicate energy pool"
            );
            ensure!(
                pool.maximum <= 10000 && pool.initial <= pool.maximum && pool.regeneration <= 256,
                "invalid energy pool"
            );
        }
        ensure!(unit.abilities.len() <= 16, "too many unit abilities");
        let mut ids = BTreeSet::new();
        for ability in &unit.abilities {
            ensure!(ids.insert(ability.id), "duplicate unit ability");
            ensure!(
                ability.energy <= unit.energy_max() && ability.range <= 32768,
                "invalid ability energy/range"
            );
            if let Some(previous) = definitions.insert(ability.id, &ability.effect) {
                ensure!(previous == &ability.effect, "inconsistent ability effect");
            }
            match &ability.effect {
                AbilityEffect::DrainArea { radius } => {
                    ensure!((1..=2048).contains(radius), "invalid drain radius")
                }
                AbilityEffect::DamageAura {
                    radius,
                    damage_fp8,
                    period,
                    duration,
                    affected,
                } => {
                    ensure!(
                        *radius <= 2048
                            && (1..=256000).contains(damage_fp8)
                            && (1..=10000).contains(period)
                            && (1..=100000).contains(duration),
                        "invalid damage aura"
                    );
                    let unique: BTreeSet<_> = affected.iter().collect();
                    ensure!(
                        unique.len() == affected.len()
                            && !affected.is_empty()
                            && affected
                                .iter()
                                .all(|id| rules.units.iter().any(|u| u.id == *id)),
                        "invalid aura targets"
                    );
                }
                _ => effects::validate_effect(rules, &ability.effect)?,
            }
        }
    }
    Ok(())
}

impl World {
    pub fn targeted_ability(&self, unit: UnitTypeId, id: AbilityId) -> Option<&TargetedAbility> {
        self.unit_type(unit)?.abilities.iter().find(|a| a.id == id)
    }
    pub fn ability_ready(&self, actor: &Entity, id: AbilityId) -> bool {
        !self.attacks_locked(actor) && self.targeted_ability(actor.unit_type, id).is_some()
            && !self.targeted_ability(actor.unit_type, id).is_some_and(|a| matches!(a.effect, AbilityEffect::Strike { ammunition: Some(unit), .. } if self.ready_ammunition(actor.owner, unit).is_none()))
            && self.targeted_ability(actor.unit_type, id).unwrap().research.is_none_or(|r| self.has_research(actor.owner, r))
            && self.rules.research.iter().filter(|r| matches!(&r.effect, ResearchEffect::Ability { units, ability } if *ability == id && units.contains(&actor.unit_type)))
                .all(|r| self.has_research(actor.owner, r.id))
    }
    pub fn cast_rejection(
        &self,
        entity: EntityId,
        id: AbilityId,
        target: AbilityTarget,
    ) -> Option<Rejection> {
        let Some(index) = self.index(entity) else {
            return Some(Rejection::UnknownEntity);
        };
        let actor = &self.state.entities[index];
        let Some(ability) = self.targeted_ability(actor.unit_type, id) else {
            return Some(Rejection::UnsupportedOrder);
        };
        if actor.construction.is_some() || actor.garrisoned_in.is_some() {
            return Some(Rejection::Unfinished);
        }
        if self.attacks_locked(actor) {
            return Some(Rejection::UnsupportedOrder);
        }
        if let AbilityEffect::Strike {
            ammunition: Some(unit),
            ..
        } = ability.effect
            && self.ready_ammunition(actor.owner, unit).is_none()
        {
            return Some(Rejection::InsufficientResources);
        }
        if !self.ability_ready(actor, id) {
            return Some(Rejection::MissingPrerequisite);
        }
        if actor.energy < ability.energy * 256 {
            return Some(Rejection::InsufficientResources);
        }
        match (&ability.effect, target) {
            (AbilityEffect::DrainArea { .. }, AbilityTarget::Point(p)) if self.map.contains(p) => {
                None
            }
            (AbilityEffect::DamageAura { .. }, AbilityTarget::Unit(id))
                if self.entity_visible(actor.owner, id)
                    && self.index(id).is_some_and(|i| {
                        let target = &self.state.entities[i];
                        target.hp > 0
                            && !target.invincible
                            && target.garrisoned_in.is_none()
                            && !self.unit_at(i).structure
                            && !self.unit_at(i).revealer
                    }) =>
            {
                None
            }
            _ => self.effect_target_rejection(actor, &ability.effect, target),
        }
    }
    pub(super) fn advance_energy(&mut self) {
        if self.state.winner.is_some() || self.state.mission.as_ref().is_some_and(|m| m.paused) {
            return;
        }
        for index in 0..self.state.entities.len() {
            let Some(pool) = self.unit_at(index).energy_pool.as_ref() else {
                continue;
            };
            if self.state.entities[index].construction.is_none() {
                let energy = self.state.entities[index]
                    .energy
                    .saturating_add(pool.regeneration);
                self.state.entities[index].energy =
                    energy.min(self.energy_max(&self.state.entities[index]) * 256);
            }
        }
    }
    pub(super) fn advance_cast(&mut self, index: usize, id: AbilityId, target: AbilityTarget) {
        if self
            .state
            .pending_effects
            .iter()
            .any(|e| e.source == self.state.entities[index].id && e.channel > 0)
        {
            return;
        }
        if self
            .cast_rejection(self.state.entities[index].id, id, target)
            .is_some()
        {
            self.finish(index);
            return;
        }
        let ability = self
            .targeted_ability(self.state.entities[index].unit_type, id)
            .unwrap()
            .clone();
        let position = match target {
            AbilityTarget::Point(p) => p,
            AbilityTarget::Unit(id) => self.state.entities[self.index(id).unwrap()].position,
        };
        let actor = &self.state.entities[index];
        let range = if matches!(
            ability.effect,
            AbilityEffect::Strike {
                ammunition: Some(_),
                ..
            }
        ) {
            self.vision_range(actor)
        } else {
            ability.range
        };
        if rts::distance(actor.position, position) > i64::from(range).pow(2) {
            self.approach_cast(index, position, range);
            return;
        }
        let source = actor.id;
        let owner = actor.owner;
        let actor = &mut self.state.entities[index];
        actor.target = None;
        actor.path.clear();
        actor.route_wait = None;
        actor.motion_speed = 0;
        actor.motion_phase = 0;
        self.state.entities[index].energy -= ability.energy * 256;
        self.state.entities[index].last_cast = Some(CastAppearance {
            ability: id,
            tick: self.tick(),
            position,
            origin: self.state.entities[index].position,
        });
        if matches!(ability.effect, AbilityEffect::LinkedTransport { .. }) {
            self.state.entities[index].last_cast = None;
        }
        match ability.effect {
            AbilityEffect::DrainArea { radius } => {
                for target in &mut self.state.entities {
                    // Source EMP uses an axis-aligned area and affects either side.
                    if target.id != source
                        && (target.position.x - position.x).abs() <= radius as i32
                        && (target.position.y - position.y).abs() <= radius as i32
                        && target.garrisoned_in.is_none()
                    {
                        target.energy = 0;
                        target.shields = 0;
                        if target.illusion_remaining.is_some() {
                            target.hp = 0;
                        }
                    }
                }
            }
            AbilityEffect::DamageAura { duration, .. } => {
                let AbilityTarget::Unit(target) = target else {
                    unreachable!()
                };
                let target_index = self.index(target).unwrap();
                let target = &mut self.state.entities[target_index];
                target.ability_auras.retain(|a| a.ability != id);
                target.ability_auras.push(AbilityAura {
                    ability: id,
                    remaining: duration,
                    strength: 0,
                    source: Some(source),
                    owner,
                });
            }
            _ => {
                if self.start_effect(index, id, target) {
                    return;
                }
            }
        }
        self.finish(index);
    }

    fn approach_cast(&mut self, index: usize, position: Position, range: u32) {
        let actor = &self.state.entities[index];
        let current_geometry = actor.path_geometry == self.navigation_geometry_hash;
        if let Some(endpoint) = actor.target.filter(|p| {
            let square = rts::distance(*p, position);
            square <= i64::from(range).pow(2) && square >= i64::from(range.saturating_sub(2)).pow(2)
        }) && current_geometry
            && (!actor.path.is_empty() || self.tick() < actor.path_retry)
        {
            self.navigate(index, endpoint, false);
            return;
        }
        if current_geometry && actor.path.is_empty() && self.tick() < actor.path_retry {
            return;
        }
        let unit = self.unit_at(index);
        let route = crate::path::find_path_in_range(
            &self.map,
            unit.footprint,
            self.movement_class(actor),
            actor.position,
            position,
            range,
            &self.navigation_geometry,
        );
        let actor = &mut self.state.entities[index];
        actor.path = route.unwrap_or_default().into();
        actor.target = actor.path.back().copied();
        actor.path_geometry = self.navigation_geometry_hash;
        actor.route_wait = None;
        actor.path_retry = Tick(self.state.tick.0.saturating_add(rts::PATH_RETRY_TICKS));
        if let Some(endpoint) = actor.target {
            self.navigate(index, endpoint, false);
        }
    }
    pub(super) fn advance_auras(&mut self, damage: &mut rts::Damage) {
        let hosts: Vec<_> = self
            .state
            .entities
            .iter()
            .filter(|e| !e.ability_auras.is_empty())
            .cloned()
            .collect();
        for host in hosts {
            for aura in &host.ability_auras {
                let Some(AbilityEffect::DamageAura {
                    radius,
                    damage_fp8,
                    period,
                    duration,
                    affected,
                }) = self
                    .rules
                    .units
                    .iter()
                    .flat_map(|u| &u.abilities)
                    .find(|a| a.id == aura.ability)
                    .map(|a| &a.effect)
                else {
                    continue;
                };
                if (duration - aura.remaining + 1) % period != 0 {
                    continue;
                }
                for target in &self.state.entities {
                    if target.invincible
                        || self.effect_invulnerable(target)
                        || !affected.contains(&target.unit_type)
                    {
                        continue;
                    }
                    let burrowed = |e: &Entity| {
                        e.cloaked
                            && self
                                .unit_type(e.unit_type)
                                .and_then(|u| u.cloak.as_ref())
                                .is_some_and(|c| !c.can_move)
                    };
                    if burrowed(&host) && target.id != host.id
                        || burrowed(target) && target.id != host.id
                    {
                        continue;
                    }
                    if let Some(container) = host.garrisoned_in {
                        if target.id != container {
                            continue;
                        }
                    } else if target.garrisoned_in.is_some() {
                        continue;
                    }
                    let a = self.unit_type(host.unit_type).unwrap().footprint;
                    let b = self.unit_type(target.unit_type).unwrap().footprint;
                    let dx = ((host.position.x - target.position.x).abs()
                        - i32::from(a.width + b.width) / 2)
                        .max(0) as u64;
                    let dy = ((host.position.y - target.position.y).abs()
                        - i32::from(a.height + b.height) / 2)
                        .max(0) as u64;
                    if dx * dx + dy * dy > u64::from(*radius).pow(2) {
                        continue;
                    }
                    let amount = u64::from(*damage_fp8);
                    let shield_left = u64::from(target.shields)
                        .saturating_sub(*damage.shields.get(&target.id).unwrap_or(&0));
                    let shields = shield_left.min(amount);
                    *damage.shields.entry(target.id).or_default() += shields;
                    let source = aura.source.unwrap_or(host.id);
                    damage.source_owners.insert(source, aura.owner);
                    *damage
                        .hits
                        .entry(target.id)
                        .or_default()
                        .entry(source)
                        .or_default() += amount - shields;
                    // This effect deliberately does not recruit retaliation/AI defenders.
                }
            }
        }
        for entity in &mut self.state.entities {
            for aura in &mut entity.ability_auras {
                aura.remaining = aura.remaining.saturating_sub(1);
            }
            entity.ability_auras.retain(|a| a.remaining > 0);
        }
    }
}

pub(super) fn put_target(bytes: &mut Vec<u8>, target: AbilityTarget) {
    match target {
        AbilityTarget::Unit(id) => {
            bytes.push(0);
            bytes.extend(id.0.to_le_bytes());
        }
        AbilityTarget::Point(p) => {
            bytes.push(1);
            put_position(bytes, p);
        }
    }
}

pub(super) fn validate_state(world: &World, state: &State) -> Result<()> {
    for entity in &state.entities {
        ensure!(entity.ability_auras.len() <= 16, "too many active auras");
        let mut seen = BTreeSet::new();
        for aura in &entity.ability_auras {
            let definition = world
                .rules
                .units
                .iter()
                .flat_map(|u| &u.abilities)
                .find(|a| a.id == aura.ability);
            ensure!(
                seen.insert(aura.ability)
                    && aura.owner.0 < world.map.players
                    && aura
                        .source
                        .is_none_or(|id| id.0 > 0 && id.0 < state.next_entity_id)
                    && definition.is_some_and(
                        |a| aura.remaining > 0 && aura.remaining <= a.effect.duration()
                    ),
                "invalid active aura"
            );
        }
        ensure!(
            entity
                .last_cast
                .as_ref()
                .is_none_or(|c| c.tick <= state.tick
                    && world.map.contains(c.position)
                    && world.map.contains(c.origin)
                    && world
                        .targeted_ability(entity.unit_type, c.ability)
                        .is_some()),
            "invalid cast appearance"
        );
        ensure!(
            entity.mode_transition.as_ref().is_none_or(|m| world
                .unit_type(entity.unit_type)
                .and_then(|u| u.mode.as_ref())
                .is_some_and(|mode| m.total == mode.ticks && (1..=m.total).contains(&m.remaining))),
            "invalid mode transition"
        );
    }
    effects::validate_state(world, state)?;
    Ok(())
}

// Empty definitions/state retain existing package hashes. New content opts in.
pub(super) fn put_rules(bytes: &mut Vec<u8>, rules: &Rules) {
    for unit in &rules.units {
        if unit.energy_pool.is_none() && unit.abilities.is_empty() {
            continue;
        }
        bytes.extend(b"targeted-abilities-v1");
        bytes.extend(unit.id.0.to_le_bytes());
        bytes.push(u8::from(unit.energy_pool.is_some()));
        if let Some(pool) = &unit.energy_pool {
            for n in [pool.maximum, pool.initial, pool.regeneration] {
                bytes.extend(n.to_le_bytes());
            }
        }
        bytes.extend((unit.abilities.len() as u32).to_le_bytes());
        for a in &unit.abilities {
            bytes.extend(a.id.0.to_le_bytes());
            bytes.extend(a.energy.to_le_bytes());
            bytes.extend(a.range.to_le_bytes());
            bytes.push(u8::from(a.research.is_some()));
            if let Some(id) = a.research {
                bytes.extend(id.0.to_le_bytes());
            }
            match &a.effect {
                AbilityEffect::DrainArea { radius } => {
                    bytes.push(0);
                    bytes.extend(radius.to_le_bytes());
                }
                AbilityEffect::DamageAura {
                    radius,
                    damage_fp8,
                    period,
                    duration,
                    affected,
                } => {
                    bytes.push(1);
                    for n in [radius, damage_fp8, period, duration] {
                        bytes.extend(n.to_le_bytes());
                    }
                    bytes.extend((affected.len() as u32).to_le_bytes());
                    for id in affected {
                        bytes.extend(id.0.to_le_bytes());
                    }
                }
                other => {
                    bytes.push(2);
                    put_string(
                        bytes,
                        &ron::ser::to_string(other).expect("serializable effect"),
                    );
                }
            }
        }
    }
}
pub(super) fn put_state(bytes: &mut Vec<u8>, state: &State) {
    for e in &state.entities {
        if e.ability_auras.is_empty() && e.last_cast.is_none() {
            continue;
        }
        bytes.extend(b"ability-state-v1");
        bytes.extend(e.id.0.to_le_bytes());
        bytes.extend((e.ability_auras.len() as u32).to_le_bytes());
        for a in &e.ability_auras {
            bytes.extend(a.ability.0.to_le_bytes());
            bytes.extend(a.remaining.to_le_bytes());
            if a.strength > 0 {
                bytes.extend(b"aura-strength-v1");
                bytes.extend(a.strength.to_le_bytes());
            }
            bytes.push(u8::from(a.source.is_some()));
            if let Some(id) = a.source {
                bytes.extend(id.0.to_le_bytes());
            }
            bytes.extend(a.owner.0.to_le_bytes());
        }
        bytes.push(u8::from(e.last_cast.is_some()));
        if let Some(c) = &e.last_cast {
            bytes.extend(c.ability.0.to_le_bytes());
            bytes.extend(c.tick.0.to_le_bytes());
            put_position(bytes, c.position);
            put_position(bytes, c.origin);
        }
    }
    effects::put_state(bytes, state);
}
