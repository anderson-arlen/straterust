//! Authored research priorities and observable spell decisions use ordinary orders.
use super::*;

impl World {
    pub(super) fn ai_technology(&mut self, controller: &AiController, state: &mut AiState) {
        for &research in &controller.research {
            if self.has_research(controller.player, research) {
                continue;
            }
            let facility = self
                .state
                .entities
                .iter()
                .find(|e| {
                    e.owner == controller.player
                        && self.ai_in_town(controller, state, e)
                        && self
                            .research_rejection(controller.player, e.id, research)
                            .is_none()
                })
                .map(|e| e.id);
            if let Some(entity) = facility {
                self.ai_order(controller, state, Order::Research { entity, research });
                break;
            }
        }
        let casters: Vec<_> = self
            .state
            .entities
            .iter()
            .filter(|e| {
                e.owner == controller.player
                    && e.hp > 0
                    && e.construction.is_none()
                    && e.garrisoned_in.is_none()
                    && matches!(
                        e.order,
                        UnitOrder::Idle
                            | UnitOrder::AttackMove { .. }
                            | UnitOrder::Attack { .. }
                            | UnitOrder::Patrol { .. }
                    )
            })
            .map(|e| e.id)
            .collect();
        for entity in casters {
            let actor = self.state.entities[self.index(entity).unwrap()].clone();
            for &id in &controller.abilities {
                let Some(ability) = self.targeted_ability(actor.unit_type, id) else {
                    continue;
                };
                let candidate = self.ai_spell_target(&actor, ability);
                if let Some(target) = candidate
                    && self.cast_rejection(entity, id, target).is_none()
                    && self.ai_order(
                        controller,
                        state,
                        Order::Cast {
                            entity,
                            ability: id,
                            target,
                        },
                    )
                {
                    break;
                }
            }
        }
    }

    fn ai_spell_target(&self, actor: &Entity, ability: &TargetedAbility) -> Option<AbilityTarget> {
        let nearby = |e: &&Entity| {
            e.hp > 0
                && e.garrisoned_in.is_none()
                && rts::distance(actor.position, e.position)
                    <= i64::from(ability.range.min(768).saturating_add(64)).pow(2)
                && self.entity_visible(actor.owner, e.id)
        };
        let enemy = self
            .state
            .entities
            .iter()
            .filter(nearby)
            .filter(|e| {
                self.is_enemy_entity(actor.owner, e)
                    && !e.invincible
                    && !self.undetected(actor.owner, e)
            })
            .min_by_key(|e| (rts::distance(actor.position, e.position), e.id));
        let friend = |e: &&Entity| {
            e.owner == actor.owner && e.ability_auras.iter().all(|a| a.ability != ability.id)
        };
        match &ability.effect {
            AbilityEffect::Heal { affected, .. } => self
                .state
                .entities
                .iter()
                .filter(nearby)
                .filter(friend)
                .filter(|e| {
                    affected.contains(&e.unit_type)
                        && e.hp + 3 < self.unit_type(e.unit_type).unwrap().max_hp
                })
                .min_by_key(|e| e.hp)
                .map(|e| AbilityTarget::Unit(e.id)),
            AbilityEffect::Buff {
                speed_percent,
                damage_percent,
                invulnerable,
                invisible,
                ..
            } => {
                if enemy.is_none() || *invisible {
                    return None;
                }
                if *speed_percent < 100 && *damage_percent == 100 && !*invulnerable {
                    enemy.map(|e| AbilityTarget::Unit(e.id))
                } else {
                    self.state
                        .entities
                        .iter()
                        .filter(nearby)
                        .filter(friend)
                        .filter(|e| {
                            self.unit_type(e.unit_type)
                                .is_some_and(|u| !u.structure && u.weapon.is_some())
                        })
                        .min_by_key(|e| rts::distance(e.position, enemy.unwrap().position))
                        .map(|e| AbilityTarget::Unit(e.id))
                }
            }
            AbilityEffect::DamageAura { .. } => enemy.and_then(|enemy| {
                self.state
                    .entities
                    .iter()
                    .filter(nearby)
                    .filter(friend)
                    .filter(|e| {
                        e.id != actor.id
                            && rts::distance(e.position, enemy.position) <= 96_i64.pow(2)
                    })
                    .min_by_key(|e| e.hp)
                    .map(|e| AbilityTarget::Unit(e.id))
            }),
            AbilityEffect::DrainLife { affected, .. }
            | AbilityEffect::Transform { affected, .. } => enemy
                .filter(|e| affected.contains(&e.unit_type))
                .map(|e| AbilityTarget::Unit(e.id)),
            AbilityEffect::GroundEffect { .. } | AbilityEffect::AreaDamage { .. } => enemy
                .filter(|e| {
                    ability.energy > 0 || rts::distance(actor.position, e.position) <= 32_i64.pow(2)
                })
                .map(|e| AbilityTarget::Point(e.position)),
            AbilityEffect::RaiseDead {
                radius, affected, ..
            } => self
                .state
                .remains
                .iter()
                .filter(|r| {
                    affected.contains(&r.unit_type)
                        && rts::distance(actor.position, r.position)
                            <= i64::from(ability.range + radius).pow(2)
                        && self.terrain_visibility(actor.owner, r.position) == Visibility::Visible
                })
                .min_by_key(|r| rts::distance(actor.position, r.position))
                .map(|r| AbilityTarget::Point(r.position)),
            AbilityEffect::Summon { .. } if enemy.is_some() => {
                Some(AbilityTarget::Point(actor.position))
            }
            _ => None,
        }
    }
}
