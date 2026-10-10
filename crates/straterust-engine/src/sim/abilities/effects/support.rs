//! Healing, temporary modifiers and permanent transformations shared by content packs.
use super::*;
impl World {
    pub(super) fn start_support_effect(
        &mut self,
        index: usize,
        id: AbilityId,
        effect: AbilityEffect,
        victim: Option<usize>,
        point: Position,
    ) -> bool {
        let actor = self.state.entities[index].clone();
        match effect {
            AbilityEffect::Heal { amount, .. } => {
                let i = victim.unwrap();
                let maximum = self.unit_at(i).max_hp;
                self.state.entities[i].hp = (self.state.entities[i].hp + amount).min(maximum);
                return self.state.entities[i].hp < maximum;
            }
            AbilityEffect::Buff {
                health_cost_percent,
                speed_percent,
                ..
            } => {
                let i = victim.unwrap();
                if speed_percent != 100 {
                    let replaced: BTreeSet<_> = self.state.entities[i].ability_auras.iter()
                        .filter(|a| matches!(self.effect_definition(a.ability), Some(AbilityEffect::Buff { speed_percent, .. }) if *speed_percent != 100))
                        .map(|a| a.ability).collect();
                    self.state.entities[i]
                        .ability_auras
                        .retain(|a| !replaced.contains(&a.ability));
                }
                let target = &mut self.state.entities[i];
                target.hp = (target.hp * (100 - u32::from(health_cost_percent)) / 100).max(1);
                self.apply_aura(i, id, actor.owner, actor.id, 0);
            }
            AbilityEffect::Transform { to, neutral, .. } => {
                let i = victim.unwrap();
                let definition = self.unit_type(to).unwrap().clone();
                self.assign(i, UnitOrder::Idle, false);
                let target = &mut self.state.entities[i];
                target.unit_type = to;
                target.owner = neutral.unwrap_or(target.owner);
                target.hp = definition.max_hp;
                target.shields = definition.max_shields * 256;
                target.energy = definition.initial_energy();
                target.production.clear();
                target.research = None;
                target.mode_transition = None;
                target.strikes.clear();
                target.last_cast = None;
                target.ability_auras.clear();
                target.cargo = None;
                target.dropoff_target = None;
                target.cloaked = false;
                target.cloak_transition = 0;
            }
            AbilityEffect::Summon {
                unit,
                count,
                lifetime,
            } => self.spawn_effect_units(actor.owner, unit, point, count, Some(lifetime), false),
            AbilityEffect::Reveal { radius, duration } => {
                if self.state.scans.len() < 4096 {
                    self.state.scans.push(Scan {
                        owner: actor.owner,
                        position: point,
                        radius,
                        remaining: duration,
                    });
                }
            }
            _ => unreachable!(),
        }
        false
    }
    pub(in crate::sim) fn buff_percent(&self, entity: &Entity, attack: bool) -> u32 {
        entity
            .ability_auras
            .iter()
            .filter_map(|a| match self.effect_definition(a.ability) {
                Some(AbilityEffect::Buff {
                    attack_percent,
                    damage_percent,
                    ..
                }) => Some(u32::from(if attack {
                    *attack_percent
                } else {
                    *damage_percent
                })),
                _ => None,
            })
            .fold(100, |total, p| total * p / 100)
            .clamp(1, 400)
    }
    pub(in crate::sim) fn magically_concealed(&self, entity: &Entity) -> bool {
        entity.ability_auras.iter().any(|a| {
            matches!(
                self.effect_definition(a.ability),
                Some(AbilityEffect::Buff {
                    invisible: true,
                    ..
                })
            )
        })
    }
    pub(in crate::sim) fn reveal_attacking_buff(&mut self, index: usize) {
        let ids: BTreeSet<_> = self.state.entities[index]
            .ability_auras
            .iter()
            .filter(|a| {
                matches!(
                    self.effect_definition(a.ability),
                    Some(AbilityEffect::Buff {
                        invisible: true,
                        ..
                    })
                )
            })
            .map(|a| a.ability)
            .collect();
        self.state.entities[index]
            .ability_auras
            .retain(|a| !ids.contains(&a.ability));
    }
}
