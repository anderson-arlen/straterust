use super::*;

impl World {
    pub(in crate::sim) fn advance_effects(&mut self, damage: &mut rts::Damage) {
        let pending = std::mem::take(&mut self.state.pending_effects);
        for mut effect in pending {
            if effect.flight.is_some() {
                let Some(definition) = self.effect_definition(effect.ability).cloned() else {
                    continue;
                };
                let delivery = match &definition {
                    AbilityEffect::Strike {
                        delivery: Some(d), ..
                    }
                    | AbilityEffect::DrainLife {
                        delivery: Some(d), ..
                    } => d,
                    _ => continue,
                };
                let (retain, hit) = self.advance_strike(&mut effect, delivery);
                if hit {
                    match definition {
                        AbilityEffect::Strike {
                            damage: amount,
                            kind,
                            radii,
                            max_health_fraction,
                            ..
                        } => self.hit_strike(
                            &effect,
                            damage,
                            amount,
                            kind,
                            radii,
                            max_health_fraction,
                        ),
                        AbilityEffect::DrainLife {
                            damage: amount,
                            healing,
                            ..
                        } => self.hit_drain(&effect, damage, amount, healing),
                        _ => unreachable!(),
                    }
                }
                if retain {
                    self.state.pending_effects.push(effect);
                }
                continue;
            }
            if effect.channel > 0 {
                let source = self.index(effect.source);
                if source.is_none_or(|i| {
                    let e = &self.state.entities[i];
                    e.hp == 0
                        || self.disabled(e)
                        || e.owner != effect.owner
                        || e.order
                            != (UnitOrder::Cast {
                                ability: effect.ability,
                                target: effect.target,
                            })
                }) {
                    continue;
                }
                effect.channel -= 1;
                if effect.channel == 0 {
                    self.finish(source.unwrap());
                }
            }
            effect.remaining = effect.remaining.saturating_sub(1);
            if effect.remaining > 0 {
                self.state.pending_effects.push(effect);
                continue;
            }
            let Some(definition) = self.effect_definition(effect.ability).cloned() else {
                continue;
            };
            match definition {
                AbilityEffect::DrainLife {
                    damage: amount,
                    healing,
                    ..
                } => self.hit_drain(&effect, damage, amount, healing),
                AbilityEffect::Strike {
                    damage: amount,
                    kind,
                    radii,
                    max_health_fraction,
                    ..
                } => {
                    self.hit_strike(&effect, damage, amount, kind, radii, max_health_fraction);
                }
                AbilityEffect::Recall { radius, .. } => {
                    let AbilityTarget::Point(point) = effect.target else {
                        continue;
                    };
                    let ids: Vec<_> = self
                        .state
                        .entities
                        .iter()
                        .filter(|e| {
                            e.owner == effect.owner
                                && e.id != effect.source
                                && e.hp > 0
                                && !self.disabled(e)
                                && !self.unit_type(e.unit_type).unwrap().structure
                                && e.garrisoned_in.is_none()
                                && rts::distance(e.position, point) <= i64::from(radius).pow(2)
                        })
                        .map(|e| e.id)
                        .collect();
                    for id in ids {
                        let i = self.index(id).unwrap();
                        self.relocate_effect_unit(i, effect.origin);
                    }
                }
                AbilityEffect::Merge {
                    partner, result, ..
                } => {
                    let AbilityTarget::Unit(id) = effect.target else {
                        continue;
                    };
                    if let (Some(a), Some(b)) = (self.index(effect.source), self.index(id))
                        && self.state.entities[b].hp > 0
                        && self.state.entities[b].owner == effect.owner
                        && self.state.entities[b].unit_type == partner
                    {
                        let unit = self.unit_type(result).unwrap().clone();
                        self.state.entities[b].hp = 0;
                        self.assign(a, UnitOrder::Idle, false);
                        let e = &mut self.state.entities[a];
                        e.unit_type = result;
                        e.hp = unit.max_hp;
                        e.shields = unit.max_shields * 256;
                        e.energy = unit.initial_energy();
                        e.last_cast = None;
                        e.ability_auras.clear();
                        self.record_created(effect.owner, result);
                    }
                }
                _ => {}
            }
        }
        self.advance_ground_effects(damage);
        let fields = self.state.ability_fields.clone();
        let mut field_hits = BTreeSet::new();
        for field in fields {
            if let Some(AbilityEffect::AreaDamage {
                radius,
                damage_fp8,
                period,
                duration,
                lethal,
                shields,
            }) = self.effect_definition(field.ability)
                && (duration - field.remaining) % period == 0
            {
                for target in &self.state.entities {
                    if target.hp > 0
                        && target.garrisoned_in.is_none()
                        && rts::distance(target.position, field.position)
                            <= i64::from(*radius).pow(2)
                        && field_hits.insert((target.id, field.ability))
                    {
                        self.record_effect_damage(
                            damage,
                            target,
                            (field.source.unwrap_or(target.id), field.owner),
                            *damage_fp8,
                            *lethal,
                            *shields,
                        );
                    }
                }
            }
        }
        for target in &self.state.entities {
            for aura in &target.ability_auras {
                if let Some(AbilityEffect::AreaDamage {
                    damage_fp8,
                    period,
                    duration,
                    lethal,
                    shields,
                    ..
                }) = self.effect_definition(aura.ability)
                    && (duration - aura.remaining + 1) % period == 0
                {
                    self.record_effect_damage(
                        damage,
                        target,
                        (aura.source.unwrap_or(target.id), aura.owner),
                        *damage_fp8,
                        *lethal,
                        *shields,
                    );
                }
            }
        }
        for field in &mut self.state.ability_fields {
            field.remaining = field.remaining.saturating_sub(1);
        }
        self.state.ability_fields.retain(|f| f.remaining > 0);
    }
    fn hit_drain(
        &mut self,
        effect: &PendingEffect,
        damage: &mut rts::Damage,
        amount: u32,
        healing: u32,
    ) {
        if let AbilityTarget::Unit(target) = effect.target
            && let Some(i) = self.index(target)
        {
            let target = self.state.entities[i].clone();
            if target.hp == 0 || target.invincible || self.effect_invulnerable(&target) {
                return;
            }
            self.record_effect_damage(
                damage,
                &target,
                (effect.source, effect.owner),
                amount * 256,
                true,
                false,
            );
            if let Some(i) = self.index(effect.source)
                && self.state.entities[i].hp > 0
            {
                let maximum = self.unit_at(i).max_hp;
                self.state.entities[i].hp =
                    (self.state.entities[i].hp + healing.min(target.hp)).min(maximum);
            }
        }
    }
    pub(super) fn record_effect_damage(
        &self,
        damage: &mut rts::Damage,
        target: &Entity,
        (source, owner): (EntityId, PlayerId),
        amount: u32,
        lethal: bool,
        shields: bool,
    ) {
        if target.invincible || self.effect_invulnerable(target) {
            return;
        }
        damage.source_owners.insert(source, owner);
        let mut amount = u64::from(amount);
        if shields {
            amount = self.absorb_barriers(damage, target, amount);
        }
        if shields {
            let spent = damage.shields.entry(target.id).or_default();
            let absorbed = u64::from(target.shields).saturating_sub(*spent).min(amount);
            *spent += absorbed;
            amount -= absorbed;
        }
        if !lethal {
            let prior: u64 = damage.hits.get(&target.id).map_or(0, |s| s.values().sum());
            amount = amount.min(
                (u64::from(target.hp) * 256 - u64::from(target.damage_fraction))
                    .saturating_sub(prior + 256),
            );
        }
        *damage
            .hits
            .entry(target.id)
            .or_default()
            .entry(source)
            .or_default() += amount;
        *damage
            .incoming
            .entry(target.id)
            .or_default()
            .entry(source)
            .or_default() += amount;
    }
    fn relocate_effect_unit(&mut self, index: usize, point: Position) {
        let unit = self.unit_at(index);
        let spot = (1..=8)
            .flat_map(|ring| {
                rts::perimeter(
                    point,
                    Footprint {
                        width: ring * 32,
                        height: ring * 32,
                    },
                    unit.footprint,
                    point,
                )
            })
            .find(|p| {
                self.can_place(
                    *p,
                    unit.footprint,
                    unit.movement_class,
                    Some(self.state.entities[index].id),
                )
            });
        if let Some(spot) = spot {
            self.assign(index, UnitOrder::Idle, false);
            self.state.entities[index].position = spot;
            self.state.entities[index].motion_fraction = [0; 2];
        }
    }
}

impl World {
    fn hit_strike(
        &self,
        effect: &PendingEffect,
        damage: &mut rts::Damage,
        amount: u32,
        kind: DamageKind,
        radii: Option<[u32; 3]>,
        max_health_fraction: Option<[u16; 2]>,
    ) {
        let point = if let Some(flight) = &effect.flight {
            flight.destination
        } else {
            match effect.target {
                AbilityTarget::Point(p) => p,
                AbilityTarget::Unit(id) => match self.index(id) {
                    Some(i) => self.state.entities[i].position,
                    None => return,
                },
            }
        };
        let targets: Vec<_> = self
            .state
            .entities
            .iter()
            .filter(|e| {
                e.hp > 0
                    && e.garrisoned_in.is_none()
                    && (radii.is_some_and(|r| {
                        rts::distance(e.position, point) <= i64::from(r[2]).pow(2)
                    }) || effect.target == AbilityTarget::Unit(e.id))
            })
            .cloned()
            .collect();
        damage.source_owners.insert(effect.source, effect.owner);
        for target in targets {
            let definition = self.unit_type(target.unit_type).unwrap();
            let amount = amount.max(
                (u64::from(definition.max_hp + definition.max_shields)
                    * max_health_fraction.map_or(0, |r| u64::from(r[0]))
                    / max_health_fraction.map_or(1, |r| u64::from(r[1]))) as u32,
            );
            let divisor = radii.map_or(1, |r| {
                let d = rts::distance(target.position, point);
                if d <= i64::from(r[0]).pow(2) {
                    1
                } else if d <= i64::from(r[1]).pow(2) {
                    2
                } else {
                    4
                }
            });
            let weapon = Weapon {
                friendly_splash: false,
                projectile_speed: 0,
                damage: amount,
                range: 0,
                cooldown: 1,
                damage_kind: kind,
                cooldown_jitter: None,
                targets_air: true,
                target_classes: Vec::new(),
                splash: radii,
                strikes: Vec::new(),
            };
            self.record_hit(
                damage,
                (
                    effect.source,
                    self.index(effect.source)
                        .map_or(target.unit_type, |i| self.state.entities[i].unit_type),
                ),
                &target,
                &weapon,
                divisor,
            );
        }
    }
}
