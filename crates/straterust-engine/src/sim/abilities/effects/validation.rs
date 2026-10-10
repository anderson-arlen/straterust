use super::*;

pub(in crate::sim::abilities) fn validate_effect(
    rules: &Rules,
    effect: &AbilityEffect,
) -> Result<()> {
    let known = |id: &UnitTypeId| rules.units.iter().any(|u| u.id == *id);
    let targets = |ids: &[UnitTypeId]| {
        !ids.is_empty()
            && ids.len() <= rules.units.len()
            && ids.iter().all(known)
            && ids.iter().collect::<BTreeSet<_>>().len() == ids.len()
    };
    let valid = match effect {
        AbilityEffect::GroundEffect {
            radius,
            damage_fp8,
            period,
            duration,
            drift,
            travel_speed,
            offsets,
            ..
        } => {
            *radius <= 2048
                && (1..=256000).contains(damage_fp8)
                && (1..=10000).contains(period)
                && (1..=100000).contains(duration)
                && *drift <= 32
                && *travel_speed <= 256
                && (1..=16).contains(&offsets.len())
                && offsets.iter().flatten().all(|n| n.unsigned_abs() <= 2048)
        }
        AbilityEffect::DrainLife {
            affected,
            damage,
            healing,
            delivery,
        } => {
            delivery.as_ref().is_none_or(valid_delivery)
                && targets(affected)
                && (1..=100000).contains(damage)
                && *healing <= *damage
        }
        AbilityEffect::RaiseDead {
            affected,
            unit,
            radius,
            lifetime,
            corpse_ticks,
        } => {
            targets(affected)
                && known(unit)
                && (1..=2048).contains(radius)
                && (1..=100000).contains(lifetime)
                && (1..=100000).contains(corpse_ticks)
        }
        AbilityEffect::Heal { affected, amount } => {
            targets(affected) && (1..=10000).contains(amount)
        }
        AbilityEffect::Buff {
            affected,
            duration,
            speed_percent,
            attack_percent,
            damage_percent,
            health_cost_percent,
            ..
        } => {
            targets(affected)
                && (1..=100000).contains(duration)
                && (1..=400).contains(speed_percent)
                && (1..=400).contains(attack_percent)
                && (1..=400).contains(damage_percent)
                && *health_cost_percent < 100
        }
        AbilityEffect::Transform {
            affected,
            to,
            neutral,
        } => targets(affected) && known(to) && neutral.is_none_or(|p| p.0 < 16),
        AbilityEffect::Summon {
            unit,
            count,
            lifetime,
        } => known(unit) && (1..=8).contains(count) && (1..=100000).contains(lifetime),
        AbilityEffect::Reveal { radius, duration } => {
            (1..=2048).contains(radius) && (1..=10000).contains(duration)
        }
        AbilityEffect::LinkedTransport { exit, passengers } => {
            rules
                .units
                .iter()
                .any(|u| u.id == *exit && u.structure && u.autonomous_construction)
                && targets(passengers)
        }
        AbilityEffect::Disable {
            radius,
            duration,
            affected,
            ..
        } => *radius <= 2048 && (1..=100000).contains(duration) && targets(affected),
        AbilityEffect::Barrier { duration, amount } => {
            (1..=100000).contains(duration) && (1..=10000).contains(amount)
        }
        AbilityEffect::Strike {
            damage,
            kind,
            radii,
            delay,
            channel,
            ammunition,
            max_health_fraction,
            delivery,
            ..
        } => {
            delivery.as_ref().is_none_or(valid_delivery)
                && *damage <= 100000
                && match kind {
                    DamageKind::Split {
                        piercing,
                        minimum_percent,
                    } => *piercing <= *damage && (1..=100).contains(minimum_percent),
                    _ => true,
                }
                && *delay <= 100000
                && *channel <= *delay
                && max_health_fraction.is_none_or(|r| r[1] > 0 && r[0] <= r[1])
                && ammunition.as_ref().is_none_or(known)
                && radii.is_none_or(|r| r[0] <= r[1] && r[1] <= r[2] && r[2] <= 2048)
        }
        AbilityEffect::AreaDamage {
            radius,
            damage_fp8,
            period,
            duration,
            ..
        } => {
            *radius <= 2048
                && (1..=256000).contains(damage_fp8)
                && (1..=10000).contains(period)
                && (1..=100000).contains(duration)
        }
        AbilityEffect::SlowArea {
            radius,
            duration,
            percent,
        } => *radius <= 2048 && (1..=100000).contains(duration) && (1..=100).contains(percent),
        AbilityEffect::Consume { affected, energy } => targets(affected) && *energy <= 10000,
        AbilityEffect::KillSpawn {
            affected,
            unit,
            count,
            lifetime,
        } => {
            targets(affected)
                && known(unit)
                && (1..=8).contains(count)
                && (1..=100000).contains(lifetime)
        }
        AbilityEffect::Infest {
            from,
            to,
            max_hp_percent,
        } => targets(from) && known(to) && (1..=100).contains(max_hp_percent),
        AbilityEffect::Parasite => true,
        AbilityEffect::Protection { radius, duration } => {
            (1..=2048).contains(radius) && (1..=100000).contains(duration)
        }
        AbilityEffect::Illusions { count, lifetime } => {
            (1..=8).contains(count) && (1..=100000).contains(lifetime)
        }
        AbilityEffect::Recall { radius, delay } => {
            (1..=2048).contains(radius) && (1..=100000).contains(delay)
        }
        AbilityEffect::Merge {
            partner,
            result,
            delay,
        } => known(partner) && known(result) && (1..=100000).contains(delay),
        AbilityEffect::Recharge {
            rate,
            shield_per_energy,
        } => (1..=25600).contains(rate) && (1..=100).contains(shield_per_energy),
        _ => false,
    };
    ensure!(valid, "invalid targeted effect");
    Ok(())
}
pub(in crate::sim::abilities) fn validate_state(world: &World, state: &State) -> Result<()> {
    ensure!(
        state.remains.len() <= 4096
            && state
                .remains
                .iter()
                .all(|r| world.unit_type(r.unit_type).is_some()
                    && world.map.contains(r.position)
                    && (1..=100000).contains(&r.remaining)),
        "invalid saved remains"
    );
    for entity in &state.entities {
        if let Some(peer) = entity.linked_to {
            ensure!(
                peer != entity.id
                    && state.entities.iter().any(|e| e.id == peer
                        && e.owner == entity.owner
                        && e.linked_to == Some(entity.id)),
                "invalid linked transport endpoints"
            );
        }
    }
    ensure!(
        state.pending_effects.len() <= rts::MAX_ENTITIES
            && state.ability_fields.len() <= rts::MAX_ENTITIES,
        "too many ongoing effects"
    );
    for effect in &state.pending_effects {
        ensure!(
            world.effect_definition(effect.ability).is_some()
                && effect.owner.0 < world.map.players
                && effect.source.0 > 0
                && effect.source.0 < state.next_entity_id
                && world.map.contains(effect.origin)
                && effect.remaining > 0
                && effect.remaining <= 100000
                && effect.channel <= effect.remaining,
            "invalid pending effect"
        );
        ensure!(
            match effect.target {
                AbilityTarget::Point(p) => world.map.contains(p),
                AbilityTarget::Unit(id) => id.0 > 0 && id.0 < state.next_entity_id,
            },
            "invalid pending target"
        );
    }
    for effect in &state.pending_effects {
        if let Some(f) = &effect.flight {
            ensure!(
                f.started <= state.tick
                    && f.elapsed <= 100000
                    && world.map.contains(f.position)
                    && world.map.contains(f.leg_origin)
                    && world.map.contains(f.destination)
                    && f.velocity_fp8 <= 262144
                    && matches!(
                        world.effect_definition(effect.ability),
                        Some(
                            AbilityEffect::Strike {
                                delivery: Some(_),
                                ..
                            } | AbilityEffect::DrainLife {
                                delivery: Some(_),
                                ..
                            }
                        )
                    ),
                "invalid ongoing strike"
            );
        }
    }
    for field in &state.ability_fields {
        ensure!(
            world
                .effect_definition(field.ability)
                .is_some_and(|e| field.remaining > 0 && field.remaining <= e.duration())
                && world.map.contains(field.position)
                && field.velocity.iter().all(|v| v.unsigned_abs() <= 256)
                && field.owner.0 < world.map.players
                && field
                    .source
                    .is_none_or(|id| id.0 > 0 && id.0 < state.next_entity_id),
            "invalid ability field"
        );
    }
    Ok(())
}
pub(in crate::sim::abilities) fn put_state(bytes: &mut Vec<u8>, state: &State) {
    if !state.remains.is_empty() {
        bytes.extend(b"remains-v1");
        bytes.extend((state.remains.len() as u32).to_le_bytes());
        for remains in &state.remains {
            bytes.extend(remains.unit_type.0.to_le_bytes());
            bytes.extend(remains.position.x.to_le_bytes());
            bytes.extend(remains.position.y.to_le_bytes());
            bytes.extend(remains.remaining.to_le_bytes());
        }
    }
    if !state.pending_effects.is_empty() || !state.ability_fields.is_empty() {
        bytes.extend(b"ongoing-effects-v1");
        put_string(
            bytes,
            &ron::ser::to_string(&(&state.pending_effects, &state.ability_fields))
                .expect("serializable effects"),
        );
    }
    for entity in &state.entities {
        if let Some(remaining) = entity.lifetime_remaining {
            bytes.extend(b"unit-lifetime-v1");
            bytes.extend(entity.id.0.to_le_bytes());
            bytes.extend(remaining.to_le_bytes());
        }
    }
}

fn valid_delivery(d: &StrikeDelivery) -> bool {
    d.reveal_radius <= 2048
        && d.charge_ticks <= 10000
        && d.ascent_ticks <= 10000
        && d.warning_ticks <= d.ascent_ticks
        && d.transit_ticks <= 10000
        && d.descent_height <= 2048
        && (1..=262144).contains(&d.speed_fp8)
        && (1..=262144).contains(&d.acceleration_fp8)
        && (1..=10000).contains(&d.impact_ticks)
}
