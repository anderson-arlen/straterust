//! Game-authored spells and their technology gates; the client only sees native controls.
use super::stats::id;
use straterust_engine::sim::*;

pub struct Spell {
    pub id: AbilityId,
    pub casters: Vec<UnitTypeId>,
    pub name: &'static str,
    pub key: &'static str,
    pub slot: u8,
    pub energy: u32,
    pub range: u32,
    pub effect: AbilityEffect,
    pub research: Option<(ResearchId, usize, u32, u32)>,
}

pub fn definitions() -> Vec<Spell> {
    let units = |ids: &[usize]| ids.iter().copied().map(id).collect::<Vec<_>>();
    let organic = units(&[
        0, 1, 2, 3, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 35,
        42, 43, 44, 46, 47, 49, 50, 51, 52, 53, 55, 56, 57,
    ]);
    let undead = units(&[11, 21, 51, 55]);
    let all_mobile = units(
        &(0..58)
            .filter(|n| !matches!(n, 34 | 48 | 54))
            .collect::<Vec<_>>(),
    );
    let mut result = Vec::new();
    let mut add = |number, casters: &[usize], name, key, slot, energy, range, effect, research| {
        result.push(Spell {
            id: AbilityId(number),
            casters: units(casters),
            name,
            key,
            slot,
            energy,
            range,
            effect,
            research,
        });
    };
    let buff = |speed, attack, damage, invisible, invulnerable, hp, duration| AbilityEffect::Buff {
        affected: all_mobile.clone(),
        duration,
        speed_percent: speed,
        attack_percent: attack,
        damage_percent: damage,
        invisible,
        invulnerable,
        health_cost_percent: hp,
    };
    let area = |radius, damage: u32, period, duration| AbilityEffect::AreaDamage {
        radius,
        damage_fp8: damage * 256,
        period,
        duration,
        lethal: true,
        shields: false,
    };
    // IDs 1..19 describe spells, independently of research IDs and source formats.
    add(
        1,
        &[12, 44, 50, 52],
        "Holy Vision",
        "V",
        6,
        70,
        32768,
        AbilityEffect::Reveal {
            radius: 192,
            duration: 25,
        },
        None,
    );
    add(
        2,
        &[12, 44, 50, 52],
        "Healing",
        "H",
        7,
        6,
        192,
        AbilityEffect::Heal {
            affected: organic.clone(),
            amount: 1,
        },
        Some((ResearchId(18), 62, 1000, 200)),
    );
    add(
        3,
        &[12, 44, 50, 52],
        "Exorcism",
        "E",
        8,
        4,
        320,
        AbilityEffect::DrainLife {
            delivery: None,
            affected: undead,
            damage: 1,
            healing: 0,
        },
        Some((ResearchId(19), 62, 2000, 200)),
    );
    add(
        4,
        &[13, 23, 49],
        "Eye of Kilrogg",
        "K",
        6,
        70,
        32,
        AbilityEffect::Summon {
            unit: id(45),
            count: 1,
            lifetime: 765,
        },
        None,
    );
    add(
        5,
        &[13, 23, 49],
        "Bloodlust",
        "B",
        7,
        50,
        192,
        buff(100, 100, 300, false, false, 0, 1000),
        Some((ResearchId(82), 63, 1000, 100)),
    );
    add(
        6,
        &[13, 23, 49],
        "Runes",
        "R",
        8,
        200,
        320,
        AbilityEffect::GroundEffect {
            radius: 12,
            damage_fp8: 50 * 256,
            period: 1,
            duration: 2000,
            drift: 0,
            travel_speed: 0,
            trigger_on_contact: true,
            repeat: false,
            offsets: vec![[0, 0], [-32, 0], [32, 0], [0, -32], [0, 32]],
        },
        Some((ResearchId(83), 63, 1000, 150)),
    );
    add(
        7,
        &[10, 24],
        "Fireball",
        "F",
        3,
        100,
        256,
        AbilityEffect::GroundEffect {
            radius: 12,
            damage_fp8: 20 * 256,
            period: 2,
            duration: 20,
            drift: 0,
            travel_speed: 16,
            trigger_on_contact: false,
            repeat: false,
            offsets: vec![[0, 0]],
        },
        None,
    );
    add(
        8,
        &[10, 24],
        "Slow",
        "O",
        4,
        50,
        320,
        buff(50, 50, 100, false, false, 0, 1000),
        Some((ResearchId(20), 80, 500, 100)),
    );
    add(
        9,
        &[10, 24],
        "Flame Shield",
        "L",
        5,
        50,
        192,
        AbilityEffect::DamageAura {
            radius: 48,
            damage_fp8: 256,
            period: 8,
            duration: 600,
            affected: organic.clone(),
        },
        Some((ResearchId(21), 80, 1000, 100)),
    );
    add(
        10,
        &[10, 24],
        "Invisibility",
        "I",
        6,
        200,
        192,
        buff(100, 100, 100, true, false, 0, 2000),
        Some((ResearchId(22), 80, 2500, 200)),
    );
    add(
        11,
        &[10, 24],
        "Polymorph",
        "P",
        7,
        200,
        320,
        AbilityEffect::Transform {
            affected: organic.clone(),
            to: id(57),
            neutral: Some(PlayerId(15)),
        },
        Some((ResearchId(23), 80, 2000, 200)),
    );
    add(
        12,
        &[10, 24],
        "Blizzard",
        "B",
        8,
        25,
        384,
        AbilityEffect::GroundEffect {
            radius: 64,
            damage_fp8: 10 * 256,
            period: 15,
            duration: 15,
            drift: 0,
            travel_speed: 0,
            trigger_on_contact: false,
            repeat: true,
            offsets: vec![[0, 0]],
        },
        Some((ResearchId(24), 80, 2000, 200)),
    );
    add(
        13,
        &[11, 21, 51],
        "Death Coil",
        "C",
        3,
        100,
        320,
        AbilityEffect::DrainLife {
            delivery: Some(StrikeDelivery {
                charge_ticks: 1,
                ascent_ticks: 0,
                warning_ticks: 0,
                transit_ticks: 0,
                descent_height: 0,
                speed_fp8: 4096,
                acceleration_fp8: 4096,
                impact_ticks: 10,
                reveal_radius: 0,
            }),
            affected: organic.clone(),
            damage: 50,
            healing: 50,
        },
        None,
    );
    add(
        14,
        &[11, 21, 51],
        "Raise Dead",
        "R",
        5,
        50,
        192,
        AbilityEffect::RaiseDead {
            affected: organic,
            unit: id(55),
            radius: 64,
            lifetime: 3600,
            corpse_ticks: 400,
        },
        Some((ResearchId(84), 81, 1500, 100)),
    );
    add(
        15,
        &[11, 21, 51],
        "Haste",
        "H",
        4,
        50,
        192,
        buff(200, 200, 100, false, false, 0, 1000),
        Some((ResearchId(85), 81, 500, 100)),
    );
    add(
        16,
        &[11, 21, 51],
        "Unholy Armor",
        "U",
        7,
        100,
        192,
        buff(100, 100, 100, false, true, 50, 500),
        Some((ResearchId(86), 81, 2500, 200)),
    );
    add(
        17,
        &[11, 21, 51],
        "Whirlwind",
        "W",
        6,
        100,
        384,
        AbilityEffect::GroundEffect {
            radius: 32,
            damage_fp8: 3 * 256,
            period: 15,
            duration: 800,
            drift: 2,
            travel_speed: 0,
            trigger_on_contact: false,
            repeat: false,
            offsets: vec![[0, 0]],
        },
        Some((ResearchId(87), 81, 1500, 150)),
    );
    add(
        18,
        &[11, 21, 51],
        "Death and Decay",
        "D",
        8,
        25,
        384,
        AbilityEffect::GroundEffect {
            radius: 64,
            damage_fp8: 10 * 256,
            period: 15,
            duration: 15,
            drift: 0,
            travel_speed: 0,
            trigger_on_contact: false,
            repeat: true,
            offsets: vec![[0, 0]],
        },
        Some((ResearchId(88), 81, 2000, 200)),
    );
    add(
        19,
        &[14, 15],
        "Demolish",
        "D",
        6,
        0,
        32,
        area(96, 400, 1, 1),
        None,
    );
    result
}

pub fn add(rules: &mut Rules) {
    for mut spell in definitions() {
        let known = |id: &UnitTypeId| rules.units.iter().any(|u| u.id == *id);
        spell.casters.retain(known);
        match &mut spell.effect {
            AbilityEffect::Heal { affected, .. }
            | AbilityEffect::Buff { affected, .. }
            | AbilityEffect::Transform { affected, .. }
            | AbilityEffect::DrainLife { affected, .. }
            | AbilityEffect::RaiseDead { affected, .. }
            | AbilityEffect::DamageAura { affected, .. } => affected.retain(known),
            _ => {}
        }
        for caster in &spell.casters {
            if let Some(unit) = rules.units.iter_mut().find(|u| u.id == *caster) {
                if spell.energy > 0 {
                    unit.energy_pool = Some(EnergyPool {
                        maximum: 255,
                        initial: 84,
                        regeneration: 8,
                    });
                }
                unit.abilities.push(TargetedAbility {
                    id: spell.id,
                    research: if *caster == id(49) {
                        None
                    } else {
                        spell.research.map(|r| r.0)
                    },
                    energy: spell.energy,
                    range: spell.range,
                    effect: spell.effect.clone(),
                });
            }
        }
        if let Some((research, facility, gold, time)) = spell.research {
            rules.research.push(Research {
                available: true,
                id: research,
                facility: id(facility),
                previous: None,
                prerequisites: Vec::new(),
                cost: vec![ResourceAmount {
                    kind: "gold".into(),
                    amount: gold,
                }],
                ticks: time * 6,
                effect: ResearchEffect::Ability {
                    units: spell.casters.into_iter().filter(|u| *u != id(49)).collect(),
                    ability: spell.id,
                },
            });
        }
    }
}
