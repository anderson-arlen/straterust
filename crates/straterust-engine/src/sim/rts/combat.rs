use super::*;

impl World {
    pub(in crate::sim) fn target_priority(&self, actor: &Entity, target: &Entity) -> u8 {
        if !self.rules.prioritize_threats {
            return 0;
        }
        let loaded = self
            .state
            .entities
            .iter()
            .find(|entity| entity.garrisoned_in == Some(target.id));
        let candidate = loaded.unwrap_or(target);
        let unit = self.unit_type(candidate.unit_type).expect("validated type");
        let mut priority = if unit.worker.is_some() {
            2
        } else if unit.weapon.is_some() && self.can_target_entity(candidate, actor) {
            0
        } else if unit.weapon.is_some() {
            2
        } else if unit.speed > 0 || candidate.airborne {
            3
        } else {
            4
        };
        if loaded.is_some() || target.construction.is_some() {
            priority += 1;
        }
        if priority == 0
            && (candidate.burrowed
                || candidate.unburrow_remaining > 0
                || candidate.doodad_enabled == Some(false))
        {
            priority = 1;
        }
        priority
    }

    pub(in crate::sim) fn acquire(&self, index: usize) -> Option<usize> {
        let actor = &self.state.entities[index];
        let unit = self.unit_at(index);
        let weapon = unit.weapon.as_ref()?;
        let weapon_range = weapon.range + self.research_range_bonus(actor.owner, actor.unit_type);
        let range = if matches!(actor.order, UnitOrder::Hold) {
            weapon_range
        } else {
            unit.acquisition_range
                .unwrap_or(weapon_range)
                .max(weapon_range)
        };
        self.state
            .entities
            .iter()
            .enumerate()
            .filter(|(_, other)| self.can_attack_entity(actor, other))
            .filter(|(other, entity)| {
                let target_range = self
                    .weapon_for(actor, entity)
                    .expect("eligible weapon")
                    .range
                    + self.research_range_bonus(actor.owner, actor.unit_type);
                let range =
                    if matches!(actor.order, UnitOrder::Hold) || unit.acquisition_range.is_none() {
                        target_range
                    } else {
                        range.max(target_range)
                    };
                in_range(
                    actor.position,
                    unit.footprint,
                    entity.position,
                    self.unit_at(*other).footprint,
                    range,
                )
            })
            .min_by_key(|(_, other)| {
                (
                    self.target_priority(actor, other),
                    distance(actor.position, other.position),
                    other.id,
                )
            })
            .map(|(index, _)| index)
    }
    pub(in crate::sim) fn automatic_target(&mut self, index: usize) -> Option<usize> {
        let actor = self.state.entities[index].clone();
        let remembered = actor
            .auto_attack_target
            .and_then(|id| self.index(id))
            .filter(|&other| {
                self.can_target_entity(&actor, &self.state.entities[other])
                    && !self.undetected(actor.owner, &self.state.entities[other])
                    && (actor.retaliation_position.is_some()
                        || self.entity_visible(actor.owner, self.state.entities[other].id))
            });
        if let Some(other) = remembered {
            if actor.retaliation_position.is_some()
                && self.entity_visible(actor.owner, self.state.entities[other].id)
            {
                self.state.entities[index].retaliation_position =
                    Some(self.state.entities[other].position);
            }
        } else {
            self.state.entities[index].retaliation_position = None;
        }
        let target = if self.rules.prioritize_threats {
            let acquired = self.acquire(index);
            match (remembered, acquired) {
                (Some(old), Some(new))
                    if self.entity_visible(actor.owner, self.state.entities[old].id) =>
                {
                    let mut priority = self.target_priority(&actor, &self.state.entities[old]);
                    if priority == 0 {
                        let unit = self.unit_at(index);
                        let range = self
                            .weapon_for(&actor, &self.state.entities[old])
                            .expect("armed actor")
                            .range
                            + self.research_range_bonus(actor.owner, actor.unit_type);
                        if !in_range(
                            actor.position,
                            unit.footprint,
                            self.state.entities[old].position,
                            self.unit_at(old).footprint,
                            range,
                        ) {
                            priority = 1;
                        }
                    }
                    Some(
                        if self.target_priority(&actor, &self.state.entities[new]) < priority {
                            new
                        } else {
                            old
                        },
                    )
                }
                _ => remembered.or(acquired),
            }
        } else {
            remembered.or_else(|| self.acquire(index))
        };
        if target != remembered {
            self.state.entities[index].retaliation_position = None;
        }
        self.state.entities[index].auto_attack_target =
            target.map(|other| self.state.entities[other].id);
        target
    }
    pub(in crate::sim) fn attack(
        &mut self,
        index: usize,
        target: usize,
        damage: &mut Damage,
    ) -> bool {
        let actor = self.state.entities[index].clone();
        let unit = self.unit_at(index).clone();
        let Some(mut weapon) = self
            .weapon_for(&actor, &self.state.entities[target])
            .cloned()
        else {
            return false;
        };
        weapon.damage += self.research_damage_bonus(actor.owner, actor.unit_type);
        weapon.range += self.research_range_bonus(actor.owner, actor.unit_type);
        if actor.stim_remaining > 0 {
            weapon.cooldown = (weapon.cooldown / 2).max(5);
        }
        let enemy = self.state.entities[target].clone();
        if !self.can_attack_entity(&actor, &enemy)
            && !self.can_return_stationary_fire(&actor, &enemy)
        {
            // Incoming fire supplies an origin even outside sight. Investigate
            // that point; never follow the live position of an unseen enemy.
            if actor.auto_attack_target == Some(enemy.id)
                && self.can_target_entity(&actor, &enemy)
                && !self.undetected(actor.owner, &enemy)
                && let Some(position) = actor.retaliation_position
            {
                if self.navigate(index, position, true) {
                    self.state.entities[index].auto_attack_target = None;
                    self.state.entities[index].retaliation_position = None;
                }
                return true;
            }
            return false;
        }
        let target_type = self.unit_at(target).clone();
        if in_range(
            actor.position,
            unit.footprint,
            enemy.position,
            target_type.footprint,
            weapon.range,
        ) {
            if actor.cooldown == 0 {
                self.state.entities[index].last_attack_air =
                    self.movement_class(&enemy) == MovementClass::Air;
                self.state.entities[index].last_attack_target = Some(enemy.id);
                self.state.entities[index].last_attack_position = Some(enemy.position);
                if weapon.strikes.is_empty() {
                    *damage
                        .entry(enemy.id)
                        .or_default()
                        .entry(actor.id)
                        .or_default() += weapon_damage(
                        &weapon,
                        &target_type,
                        self.research_armor_bonus(enemy.owner, enemy.unit_type),
                        1,
                    );
                } else {
                    self.state.entities[index].strikes = weapon
                        .strikes
                        .iter()
                        .map(|strike| PendingStrike {
                            remaining: strike.delay,
                            target: enemy.id,
                            aim: enemy.position,
                            forward: strike.forward,
                            air: self.state.entities[index].last_attack_air,
                        })
                        .collect();
                    self.advance_strikes(index, damage);
                }
                self.state.entities[index].cooldown =
                    attack_cooldown(&weapon, &mut self.state.rng_state);
            }
            self.state.entities[index].target = None;
            self.state.entities[index].path.clear();
            self.state.entities[index].motion_speed = 0;
            self.state.entities[index].motion_phase = 0;
            return true;
        }
        if matches!(actor.order, UnitOrder::Attack { .. })
            || actor.auto_attack_target == Some(enemy.id)
            || (unit.acquisition_range.is_some()
                && matches!(
                    actor.order,
                    UnitOrder::Idle | UnitOrder::AttackMove { .. } | UnitOrder::Patrol { .. }
                ))
        {
            self.approach(index, enemy.position, target_type.footprint, weapon.range);
            return true;
        }
        false
    }

    pub(in crate::sim) fn react_to_damage(
        &mut self,
        index: usize,
        sources: &BTreeMap<EntityId, u64>,
    ) {
        let actor = &self.state.entities[index];
        if actor.hp == 0
            || actor.invincible
            || actor.construction.is_some()
            || actor.garrisoned_in.is_some()
            || !matches!(
                actor.order,
                UnitOrder::Idle
                    | UnitOrder::Attack { .. }
                    | UnitOrder::AttackMove { .. }
                    | UnitOrder::Patrol { .. }
            )
        {
            return;
        }
        let incoming = sources
            .iter()
            .filter(|(_, amount)| **amount > 0)
            .filter_map(|(id, amount)| self.index(*id).map(|other| (other, amount)))
            .filter(|(other, _)| {
                let source = &self.state.entities[*other];
                source.hp > 0 && self.is_enemy(actor.owner, source.owner)
            })
            .min_by_key(|(other, amount)| {
                (
                    std::cmp::Reverse(**amount),
                    distance(actor.position, self.state.entities[*other].position),
                    self.state.entities[*other].id,
                )
            })
            .map(|(other, _)| other);
        if let Some(source) = incoming
            && self.undetected(actor.owner, &self.state.entities[source])
        {
            let origin = self.state.entities[source].position;
            if let Some(target) = self.retreat_position(index, origin) {
                self.assign(index, UnitOrder::Move { target }, false);
            } else {
                self.state.entities[index].auto_attack_target = None;
                self.state.entities[index].retaliation_position = None;
            }
            return;
        }
        if matches!(actor.order, UnitOrder::Attack { .. }) {
            return;
        }
        let Some(target) = sources
            .iter()
            .filter(|(_, amount)| **amount > 0)
            .filter_map(|(id, amount)| self.index(*id).map(|other| (other, amount)))
            .filter(|(other, _)| {
                let source = &self.state.entities[*other];
                self.can_target_entity(actor, source) && !self.undetected(actor.owner, source)
            })
            .min_by_key(|(other, amount)| {
                (
                    std::cmp::Reverse(**amount),
                    distance(actor.position, self.state.entities[*other].position),
                    self.state.entities[*other].id,
                )
            })
            .map(|(other, _)| other)
        else {
            return;
        };
        if self.rules.prioritize_threats
            && let Some(old) = actor.auto_attack_target.and_then(|id| self.index(id))
            && self.can_attack_entity(actor, &self.state.entities[old])
            && self.target_priority(actor, &self.state.entities[old]) == 0
        {
            let range = self
                .weapon_for(actor, &self.state.entities[old])
                .unwrap()
                .range
                + self.research_range_bonus(actor.owner, actor.unit_type);
            if in_range(
                actor.position,
                self.unit_at(index).footprint,
                self.state.entities[old].position,
                self.unit_at(old).footprint,
                range,
            ) {
                return;
            }
        }
        let source = &self.state.entities[target];
        let (id, position) = (source.id, source.position);
        self.state.entities[index].auto_attack_target = Some(id);
        self.state.entities[index].retaliation_position = Some(position);
    }

    pub(in crate::sim) fn advance_strikes(&mut self, index: usize, damage: &mut Damage) {
        let strikes = std::mem::take(&mut self.state.entities[index].strikes);
        for strike in strikes {
            if strike.remaining != 0 {
                self.state.entities[index].strikes.push(strike);
                continue;
            }
            let actor = &self.state.entities[index];
            let unit = self.unit_at(index);
            let mut weapon = if strike.air {
                unit.air_weapon.as_ref().or(unit.weapon.as_ref())
            } else {
                unit.weapon.as_ref()
            }
            .cloned()
            .expect("validated strike weapon");
            weapon.damage += self.research_damage_bonus(actor.owner, actor.unit_type);
            let dx = i64::from(strike.aim.x) - i64::from(actor.position.x);
            let dy = i64::from(strike.aim.y) - i64::from(actor.position.y);
            let length = ((dx * dx + dy * dy) as u64).isqrt().max(1) as i64;
            let center = Position {
                x: actor.position.x + (dx * i64::from(strike.forward) / length) as i32,
                y: actor.position.y + (dy * i64::from(strike.forward) / length) as i32,
            };
            for other in &self.state.entities {
                if !self.is_enemy(actor.owner, other.owner)
                    || ((other.airborne
                        || self
                            .unit_type(other.unit_type)
                            .expect("validated type")
                            .movement_class
                            == MovementClass::Air)
                        && !weapon.targets_air)
                    || other.invincible
                    || other.gathering_inside
                    || other.garrisoned_in.is_some()
                    || self
                        .unit_type(other.unit_type)
                        .expect("validated type")
                        .revealer
                {
                    continue;
                }
                let target_type = self.unit_type(other.unit_type).expect("validated type");
                let divisor = if let Some(radii) = weapon.splash {
                    radii
                        .iter()
                        .position(|radius| {
                            in_range(
                                center,
                                Footprint::default(),
                                other.position,
                                target_type.footprint,
                                *radius,
                            )
                        })
                        .map(|ring| 1_u64 << ring)
                } else {
                    (other.id == strike.target).then_some(1)
                };
                if let Some(divisor) = divisor {
                    if other.burrowed && divisor != 1 {
                        continue;
                    }
                    *damage
                        .entry(other.id)
                        .or_default()
                        .entry(actor.garrisoned_in.unwrap_or(actor.id))
                        .or_default() += weapon_damage(
                        &weapon,
                        target_type,
                        self.research_armor_bonus(other.owner, other.unit_type),
                        divisor,
                    );
                }
            }
        }
    }
}
