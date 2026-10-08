//! Shared spell policies; game content supplies target classes and parameters.
use super::*;
mod runtime;
#[cfg(test)]
mod tests;
mod validation;
pub(super) use validation::{put_state, validate_effect, validate_state};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AbilityField {
    pub ability: AbilityId,
    pub position: Position,
    pub remaining: u32,
    pub owner: PlayerId,
    pub source: Option<EntityId>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingEffect {
    pub ability: AbilityId,
    pub source: EntityId,
    pub owner: PlayerId,
    pub origin: Position,
    pub target: AbilityTarget,
    pub remaining: u32,
    pub channel: u32,
    #[serde(default)]
    pub flight: Option<StrikeFlight>,
}

impl AbilityEffect {
    pub fn point_target(&self) -> bool {
        matches!(
            self,
            Self::DrainArea { .. }
                | Self::LinkedTransport { .. }
                | Self::AreaDamage { .. }
                | Self::SlowArea { .. }
                | Self::Protection { .. }
                | Self::Recall { .. }
                | Self::Disable { radius: 1.., .. }
                | Self::Strike {
                    ammunition: Some(_),
                    ..
                }
        )
    }
    pub fn duration(&self) -> u32 {
        match self {
            Self::Disable { duration, .. }
            | Self::Barrier { duration, .. }
            | Self::DamageAura { duration, .. }
            | Self::SlowArea { duration, .. }
            | Self::AreaDamage { duration, .. }
            | Self::Protection { duration, .. } => *duration,
            Self::Parasite => u32::MAX,
            _ => 0,
        }
    }
}

impl World {
    pub fn receive_ability_rejection(
        &self,
        entity: EntityId,
        provider: EntityId,
        ability: AbilityId,
    ) -> Option<Rejection> {
        let Some(i) = self.index(entity) else {
            return Some(Rejection::UnknownEntity);
        };
        let Some(p) = self.index(provider) else {
            return Some(Rejection::UnknownEntity);
        };
        if self.state.entities[i].owner != self.state.entities[p].owner
            || self.unit_at(i).speed == 0
            || !self
                .targeted_ability(self.state.entities[p].unit_type, ability)
                .is_some_and(|a| {
                    matches!(
                        a.effect,
                        AbilityEffect::Recharge { .. } | AbilityEffect::LinkedTransport { .. }
                    )
                })
        {
            return Some(Rejection::InvalidTarget);
        }
        let definition = self
            .targeted_ability(self.state.entities[p].unit_type, ability)
            .unwrap();
        if let AbilityEffect::LinkedTransport { passengers, .. } = &definition.effect {
            return (!passengers.contains(&self.state.entities[i].unit_type)
                || self.state.entities[i].garrisoned_in.is_some()
                || self.state.entities[i].construction.is_some()
                || self.state.entities[p].hp == 0
                || self.state.entities[p].construction.is_some()
                || self.disabled(&self.state.entities[p])
                || self.linked_exit(&self.state.entities[p]).is_none())
            .then_some(Rejection::InvalidTarget);
        }
        self.cast_rejection(provider, ability, AbilityTarget::Unit(entity))
    }
    pub(in crate::sim) fn advance_receive_ability(
        &mut self,
        index: usize,
        provider: EntityId,
        ability: AbilityId,
    ) {
        let actor = self.state.entities[index].clone();
        if self
            .receive_ability_rejection(actor.id, provider, ability)
            .is_some()
        {
            self.finish(index);
            return;
        }
        let p = self.index(provider).unwrap();
        let source = self.state.entities[p].clone();
        let definition = self.targeted_ability(source.unit_type, ability).unwrap();
        if matches!(definition.effect, AbilityEffect::LinkedTransport { .. }) {
            self.advance_linked_transport(index, p, ability);
            return;
        }
        if rts::distance(actor.position, source.position) > i64::from(definition.range).pow(2) {
            self.navigate(index, source.position, false);
            return;
        }
        if self.tick().0.is_multiple_of(8) {
            self.state.entities[p].last_cast = Some(CastAppearance {
                ability,
                tick: self.tick(),
                position: actor.position,
                origin: source.position,
            });
        }
        if !self.start_effect(p, ability, AbilityTarget::Unit(actor.id)) {
            self.finish(index);
        }
    }
    pub(in crate::sim) fn effect_definition(&self, id: AbilityId) -> Option<&AbilityEffect> {
        self.rules
            .units
            .iter()
            .flat_map(|u| &u.abilities)
            .find(|a| a.id == id)
            .map(|a| &a.effect)
    }
    pub fn disabled(&self, actor: &Entity) -> bool {
        actor.ability_auras.iter().any(|a| {
            matches!(
                self.effect_definition(a.ability),
                Some(AbilityEffect::Disable { .. })
            )
        })
    }
    pub(in crate::sim) fn effect_invulnerable(&self, actor: &Entity) -> bool {
        actor.ability_auras.iter().any(|a| {
            matches!(
                self.effect_definition(a.ability),
                Some(AbilityEffect::Disable {
                    invulnerable: true,
                    ..
                })
            )
        })
    }
    pub(in crate::sim) fn effect_speed_percent(&self, actor: &Entity) -> u32 {
        actor
            .ability_auras
            .iter()
            .filter_map(|a| match self.effect_definition(a.ability) {
                Some(AbilityEffect::SlowArea { percent, .. }) => Some(u32::from(*percent)),
                _ => None,
            })
            .min()
            .unwrap_or(100)
    }
    pub(in crate::sim) fn ready_ammunition(
        &self,
        owner: PlayerId,
        unit: UnitTypeId,
    ) -> Option<EntityId> {
        self.state
            .entities
            .iter()
            .filter(|e| e.owner == owner && e.hp > 0 && e.unit_type == unit)
            .find(|e| {
                e.garrisoned_in.is_some_and(|id| {
                    self.index(id).is_some_and(|i| {
                        let storage = &self.state.entities[i];
                        storage.owner == owner
                            && storage.hp > 0
                            && storage.construction.is_none()
                            && !storage.airborne
                            && self.unit_at(i).addon_parent.is_none_or(|_| {
                                storage.parent.is_some_and(|p| {
                                    self.index(p).is_some_and(|i| {
                                        let parent = &self.state.entities[i];
                                        parent.owner == owner && parent.hp > 0 && !parent.airborne
                                    })
                                })
                            })
                    })
                })
            })
            .map(|e| e.id)
    }
    pub(super) fn effect_target_rejection(
        &self,
        actor: &Entity,
        effect: &AbilityEffect,
        target: AbilityTarget,
    ) -> Option<Rejection> {
        if let AbilityEffect::LinkedTransport { exit, .. } = effect {
            return match target {
                AbilityTarget::Point(point) => self.link_placement_rejection(actor, *exit, point),
                _ => Some(Rejection::InvalidTarget),
            };
        }
        if effect.point_target() {
            return match target {
                AbilityTarget::Point(p) if self.map.contains(p) => None,
                _ => Some(Rejection::InvalidTarget),
            };
        }
        let AbilityTarget::Unit(id) = target else {
            return Some(Rejection::InvalidTarget);
        };
        let Some(i) = self.index(id) else {
            return Some(Rejection::UnknownEntity);
        };
        let victim = &self.state.entities[i];
        if victim.hp == 0
            || victim.invincible
            || self.effect_invulnerable(victim)
            || victim.garrisoned_in.is_some()
            || self.unit_at(i).revealer
            || !self.entity_visible(actor.owner, id)
        {
            return Some(Rejection::InvalidTarget);
        }
        let valid = match effect {
            AbilityEffect::Disable { affected, .. } => affected.contains(&victim.unit_type),
            AbilityEffect::Consume { affected, .. } => {
                victim.owner == actor.owner
                    && victim.id != actor.id
                    && affected.contains(&victim.unit_type)
            }
            AbilityEffect::KillSpawn { affected, .. } => affected.contains(&victim.unit_type),
            AbilityEffect::Infest {
                from,
                max_hp_percent,
                ..
            } => {
                victim.owner != actor.owner
                    && from.contains(&victim.unit_type)
                    && u64::from(victim.hp) * 100
                        < u64::from(self.unit_at(i).max_hp) * u64::from(*max_hp_percent)
            }
            AbilityEffect::Merge { partner, .. } => {
                victim.owner == actor.owner
                    && victim.id != actor.id
                    && victim.unit_type == *partner
                    && !self.disabled(victim)
            }
            AbilityEffect::Recharge { .. } => {
                victim.owner == actor.owner
                    && victim.shields < self.unit_at(i).max_shields * 256
                    && self.unit_at(i).max_shields > 0
            }
            AbilityEffect::Barrier { .. }
            | AbilityEffect::Parasite
            | AbilityEffect::Illusions { .. } => !self.unit_at(i).structure,
            AbilityEffect::DamageAura { .. } => !self.unit_at(i).structure,
            AbilityEffect::Strike { .. } => true,
            _ => false,
        };
        (!valid).then_some(Rejection::InvalidTarget)
    }
    fn apply_aura(
        &mut self,
        index: usize,
        id: AbilityId,
        owner: PlayerId,
        source: EntityId,
        strength: u32,
    ) {
        let remaining = self.effect_definition(id).unwrap().duration();
        let target = &mut self.state.entities[index];
        target.ability_auras.retain(|a| a.ability != id);
        target.ability_auras.push(AbilityAura {
            ability: id,
            remaining,
            strength,
            source: Some(source),
            owner,
        });
    }
    /// True retains the cast order while channeling or continuously restoring shields.
    pub(super) fn start_effect(
        &mut self,
        index: usize,
        id: AbilityId,
        target: AbilityTarget,
    ) -> bool {
        let actor = self.state.entities[index].clone();
        let effect = self.effect_definition(id).unwrap().clone();
        let victim = match target {
            AbilityTarget::Unit(id) => self.index(id),
            _ => None,
        };
        let point = victim.map_or_else(
            || match target {
                AbilityTarget::Point(p) => p,
                _ => unreachable!(),
            },
            |i| self.state.entities[i].position,
        );
        match effect {
            AbilityEffect::LinkedTransport { exit, .. } => {
                self.create_linked_exit(index, exit, point);
            }
            AbilityEffect::Disable {
                radius, affected, ..
            } => {
                let targets: Vec<_> = self
                    .state
                    .entities
                    .iter()
                    .enumerate()
                    .filter(|(_, e)| {
                        e.id != actor.id
                            && e.hp > 0
                            && !e.invincible
                            && e.garrisoned_in.is_none()
                            && affected.contains(&e.unit_type)
                            && (radius == 0
                                && Some(e.id) == victim.map(|i| self.state.entities[i].id)
                                || radius > 0
                                    && rts::distance(e.position, point) <= i64::from(radius).pow(2))
                    })
                    .map(|(i, _)| i)
                    .collect();
                for i in targets {
                    self.apply_aura(i, id, actor.owner, actor.id, 0);
                }
            }
            AbilityEffect::Barrier { amount, .. } => {
                self.apply_aura(victim.unwrap(), id, actor.owner, actor.id, amount * 256)
            }
            AbilityEffect::Parasite => {
                self.apply_aura(victim.unwrap(), id, actor.owner, actor.id, 0)
            }
            AbilityEffect::SlowArea { radius, .. } | AbilityEffect::AreaDamage { radius, .. } => {
                // Ensnare/plague follow their victims after the initial area cast.
                let targets: Vec<_> = self
                    .state
                    .entities
                    .iter()
                    .enumerate()
                    .filter(|(_, e)| {
                        (e.id != actor.id
                            || matches!(effect, AbilityEffect::AreaDamage { lethal: true, .. }))
                            && e.hp > 0
                            && !e.invincible
                            && !self.effect_invulnerable(e)
                            && e.garrisoned_in.is_none()
                            && rts::distance(e.position, point) <= i64::from(radius).pow(2)
                            && (!matches!(effect, AbilityEffect::SlowArea { .. })
                                || !self.unit_type(e.unit_type).unwrap().structure)
                    })
                    .map(|(i, _)| i)
                    .collect();
                if matches!(effect, AbilityEffect::AreaDamage { lethal: true, .. }) {
                    self.state.ability_fields.push(AbilityField {
                        ability: id,
                        position: point,
                        remaining: effect.duration(),
                        owner: actor.owner,
                        source: Some(actor.id),
                    });
                } else {
                    for i in targets {
                        self.apply_aura(i, id, actor.owner, actor.id, 0);
                    }
                }
            }
            AbilityEffect::Protection { duration, .. } => {
                self.state.ability_fields.push(AbilityField {
                    ability: id,
                    position: point,
                    remaining: duration,
                    owner: actor.owner,
                    source: Some(actor.id),
                })
            }
            AbilityEffect::Strike {
                delay,
                channel,
                ammunition,
                delivery,
                ..
            } => {
                let mut origin = actor.position;
                if let Some(ammo) =
                    ammunition.and_then(|unit| self.ready_ammunition(actor.owner, unit))
                {
                    let i = self.index(ammo).unwrap();
                    origin = self.state.entities[i]
                        .garrisoned_in
                        .and_then(|id| self.index(id))
                        .map_or(self.state.entities[i].position, |i| {
                            self.state.entities[i].position
                        });
                    self.state.entities[i].hp = 0;
                }
                let flight = delivery
                    .as_ref()
                    .map(|d| StrikeFlight::new(self.tick(), origin, point, d));
                self.state.pending_effects.push(PendingEffect {
                    ability: id,
                    source: actor.id,
                    owner: actor.owner,
                    origin,
                    target,
                    remaining: if flight.is_some() {
                        100000
                    } else {
                        delay.max(1)
                    },
                    channel: if delivery.as_ref().is_some_and(|d| d.ascent_ticks > 0) {
                        100000
                    } else {
                        channel
                    },
                    flight,
                });
                return channel > 0;
            }
            AbilityEffect::Recall { delay, .. } | AbilityEffect::Merge { delay, .. } => {
                self.state.pending_effects.push(PendingEffect {
                    ability: id,
                    source: actor.id,
                    owner: actor.owner,
                    origin: actor.position,
                    target,
                    remaining: delay.max(1),
                    channel: delay.max(1),
                    flight: None,
                });
                return true;
            }
            AbilityEffect::Consume { energy, .. } => {
                self.state.entities[victim.unwrap()].hp = 0;
                self.state.entities[index].energy =
                    (actor.energy + energy * 256).min(self.energy_max(&actor) * 256);
            }
            AbilityEffect::KillSpawn {
                unit,
                count,
                lifetime,
                ..
            } => {
                self.state.entities[victim.unwrap()].hp = 0;
                self.spawn_effect_units(actor.owner, unit, point, count, Some(lifetime), false);
            }
            AbilityEffect::Infest { to, .. } => {
                let i = victim.unwrap();
                self.cancel_research(i);
                self.assign(i, UnitOrder::Idle, false);
                let max_hp = self.unit_type(to).unwrap().max_hp;
                let victim = &mut self.state.entities[i];
                victim.unit_type = to;
                victim.owner = actor.owner;
                victim.hp = max_hp;
                victim.production.clear();
                victim.last_cast = None;
                victim.parent = None;
            }
            AbilityEffect::Illusions { count, lifetime } => self.spawn_effect_units(
                actor.owner,
                self.state.entities[victim.unwrap()].unit_type,
                point,
                count,
                Some(lifetime),
                true,
            ),
            AbilityEffect::Recharge {
                rate,
                shield_per_energy,
            } => {
                let i = victim.unwrap();
                let amount = rate
                    .min(
                        self.state.entities[index]
                            .energy
                            .saturating_mul(shield_per_energy),
                    )
                    .min(self.unit_at(i).max_shields * 256 - self.state.entities[i].shields);
                self.state.entities[index].energy -= amount.div_ceil(shield_per_energy);
                self.state.entities[i].shields += amount;
                return self.state.entities[index].energy > 0
                    && self.state.entities[i].shields < self.unit_at(i).max_shields * 256;
            }
            _ => unreachable!("base effects handled by advance_cast"),
        }
        false
    }
    fn spawn_effect_units(
        &mut self,
        owner: PlayerId,
        unit: UnitTypeId,
        point: Position,
        count: u8,
        lifetime: Option<u32>,
        illusion: bool,
    ) {
        for _ in 0..count {
            let definition = self.unit_type(unit).unwrap();
            let spot = rts::perimeter(point, definition.footprint, definition.footprint, point)
                .into_iter()
                .find(|p| {
                    self.can_place(*p, definition.footprint, definition.movement_class, None)
                });
            if let Some(spot) = spot
                && let Some(id) = self.spawn_offspring(owner, unit, spot, None)
            {
                let i = self.index(id).unwrap();
                if illusion {
                    self.state.entities[i].illusion_remaining = lifetime;
                } else {
                    self.state.entities[i].lifetime_remaining = lifetime;
                }
            }
        }
    }
}
