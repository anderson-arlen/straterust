//! Retail spell orders expressed as reusable, content-defined effects.
use super::*;

pub(in crate::campaign_units::combat) fn apply(
    tables: &Tables,
    tech: &[u8],
    rules: &mut Rules,
) -> Result<()> {
    let ids = |predicate: &dyn Fn(u16, u32) -> bool| {
        MAPPING
            .iter()
            .filter_map(|&(source, native)| {
                let flags = dword(&tables.units, 0x19b0 + usize::from(source) * 4);
                (rules.units.iter().any(|u| u.id == UnitTypeId(native)) && predicate(source, flags))
                    .then_some(UnitTypeId(native))
            })
            .collect::<Vec<_>>()
    };
    let organic = ids(&|s, f| f & 0x10000 != 0 && f & 1 == 0 && !matches!(s, 35 | 36));
    let mechanical = ids(&|_, f| f & 0x40000000 != 0 && f & 1 == 0);
    let mobile = ids(&|s, f| f & 1 == 0 && s < 106 && !matches!(s, 35 | 36 | 59 | 73 | 85));
    let concealed = ids(&|s, f| f & 1 == 0 && s < 106 && !matches!(s, 35 | 36 | 59 | 71));
    let passengers =
        ids(&|s, f| (35..=59).contains(&s) && f & 4 == 0 && !matches!(s, 35 | 36 | 59));
    // Broodlings require a biological ground body; robotic units and workers
    // without an organic body (Probe) are not valid victims.
    let broodling_targets = ids(&|s, f| {
        f & 4 == 0
            && f & 1 == 0
            && f & 0x4000 == 0
            && s < 106
            && !matches!(s, 35 | 36 | 59 | 64 | 68 | 84 | 85)
    });
    let consume = ids(&|s, f| f & 1 == 0 && (35..=59).contains(&s) && !matches!(s, 35 | 36 | 59));
    let word_energy = |id: usize| u32::from(word(tech, 24 * 6 + id * 2));
    let make = |id, technology: Option<(usize, u16)>, range, effect| TargetedAbility {
        id: AbilityId(id),
        research: technology
            .filter(|(_, r)| *r != 0)
            .map(|(_, r)| ResearchId(r)),
        energy: technology.map_or(0, |(t, _)| word_energy(t)),
        range,
        effect,
    };
    let native = |s| native_id(s).expect("mapped spell unit");
    let definitions = vec![
        make(
            20,
            None,
            32768,
            AbilityEffect::LinkedTransport {
                exit: native(134),
                passengers,
            },
        ),
        make(
            1,
            Some((2, 18)),
            dword(&tables.weapons, 0x514 + 33 * 4),
            AbilityEffect::DrainArea {
                radius: u32::from(word(&tables.weapons, 0x898 + 33 * 2)),
            },
        ),
        make(
            2,
            Some((7, 19)),
            dword(&tables.weapons, 0x514 + 34 * 4),
            AbilityEffect::DamageAura {
                radius: 32,
                damage_fp8: 250 * 256 / 75,
                period: 8,
                duration: 600,
                affected: organic,
            },
        ),
        make(
            3,
            Some((1, 22)),
            256,
            AbilityEffect::Disable {
                radius: 0,
                duration: 1048,
                invulnerable: false,
                affected: mechanical,
            },
        ),
        make(
            4,
            Some((8, 26)),
            320,
            AbilityEffect::Strike {
                damage: u32::from(word(&tables.weapons, 0xaf0 + 30 * 2)),
                kind: DamageKind::Explosive,
                radii: None,
                delay: 60,
                channel: 49,
                ammunition: None,
                max_health_fraction: None,
                delivery: Some(straterust_engine::sim::StrikeDelivery {
                    charge_ticks: 44,
                    ascent_ticks: 0,
                    warning_ticks: 0,
                    transit_ticks: 0,
                    descent_height: 0,
                    speed_fp8: dword(
                        &tables.flingy,
                        368 + dword(&tables.weapons, 200 + 30 * 4) as usize * 4,
                    ),
                    acceleration_fp8: u32::from(word(
                        &tables.flingy,
                        1104 + dword(&tables.weapons, 200 + 30 * 4) as usize * 2,
                    )),
                    impact_ticks: 28,
                    reveal_radius: 0,
                }),
            },
        ),
        make(
            5,
            Some((6, 0)),
            320,
            AbilityEffect::Barrier {
                duration: 1344,
                amount: 250,
            },
        ),
        make(
            6,
            None,
            320,
            AbilityEffect::Strike {
                damage: 500,
                kind: DamageKind::Explosive,
                radii: Some(
                    [0x898, 0x960, 0xa28].map(|o| u32::from(word(&tables.weapons, o + 31 * 2))),
                ),
                delay: 420,
                channel: 330,
                ammunition: Some(native(14)),
                max_health_fraction: Some([2, 3]),
                delivery: Some(straterust_engine::sim::StrikeDelivery {
                    charge_ticks: 0,
                    ascent_ticks: 90,
                    warning_ticks: 45,
                    transit_ticks: 250,
                    descent_height: 320,
                    speed_fp8: dword(&tables.flingy, 368 + usize::from(tables.units[14]) * 4),
                    acceleration_fp8: u32::from(word(
                        &tables.flingy,
                        1104 + usize::from(tables.units[14]) * 2,
                    )),
                    impact_ticks: 52,
                    reveal_radius: u32::from(tables.units[0x1e24 + 14]) * 32,
                }),
            },
        ),
        make(
            7,
            Some((13, 41)),
            288,
            AbilityEffect::KillSpawn {
                affected: broodling_targets,
                unit: native(40),
                count: 2,
                lifetime: 1800,
            },
        ),
        make(
            8,
            Some((17, 42)),
            288,
            AbilityEffect::SlowArea {
                radius: 64,
                duration: 600,
                percent: 50,
            },
        ),
        make(9, Some((18, 0)), 384, AbilityEffect::Parasite),
        make(
            10,
            Some((12, 0)),
            32,
            AbilityEffect::Infest {
                from: vec![native(106)],
                to: native(130),
                max_hp_percent: 50,
            },
        ),
        make(
            11,
            Some((14, 0)),
            288,
            AbilityEffect::Protection {
                radius: 96,
                duration: 900,
            },
        ),
        make(
            12,
            Some((15, 43)),
            288,
            AbilityEffect::AreaDamage {
                radius: 64,
                damage_fp8: 300 * 256 / 76,
                period: 8,
                duration: 600,
                lethal: false,
                shields: false,
            },
        ),
        make(
            13,
            Some((16, 44)),
            32,
            AbilityEffect::Consume {
                affected: consume,
                energy: 50,
            },
        ),
        make(
            14,
            Some((19, 45)),
            288,
            AbilityEffect::AreaDamage {
                radius: 48,
                damage_fp8: u32::from(word(&tables.weapons, 0xaf0 + 84 * 2)) * 256,
                period: 8,
                duration: 64,
                lethal: true,
                shields: true,
            },
        ),
        make(
            15,
            Some((20, 46)),
            224,
            AbilityEffect::Illusions {
                count: 2,
                lifetime: 1800,
            },
        ),
        make(
            16,
            Some((21, 47)),
            32768,
            AbilityEffect::Recall {
                radius: 64,
                delay: 22,
            },
        ),
        make(
            17,
            Some((22, 48)),
            288,
            AbilityEffect::Disable {
                radius: 48,
                duration: 1048,
                invulnerable: true,
                affected: mobile,
            },
        ),
        make(
            18,
            Some((23, 0)),
            32,
            AbilityEffect::Merge {
                partner: native(67),
                result: native(68),
                delay: 300,
            },
        ),
        make(
            19,
            None,
            128,
            AbilityEffect::Recharge {
                rate: 1280,
                shield_per_energy: 2,
            },
        ),
    ];
    let known: BTreeSet<_> = rules.units.iter().map(|u| u.id).collect();
    for &(source, id) in MAPPING {
        let Some(unit) = rules.units.iter_mut().find(|u| u.id == UnitTypeId(id)) else {
            continue;
        };
        if source == 71 {
            unit.concealment_field = Some(ConcealmentField {
                radius: 160,
                affected: concealed.clone(),
            });
        }
        if matches!(source, 72 | 82) && known.contains(&native(73)) {
            unit.weapon = Some(Weapon {
                damage: 0,
                range: 256,
                cooldown: 8,
                cooldown_jitter: None,
                targets_air: true,
                damage_kind: DamageKind::Normal,
                splash: None,
                strikes: Vec::new(),
            });
            unit.attacks_ground = true;
            unit.stored_weapon = Some(StoredWeapon::Fighters {
                unit: native(73),
                launch_ticks: 8,
                leash: 384,
                repair: 1280,
                expendable: None,
            });
            unit.trains = vec![native(73)];
            unit.production_capacity = if source == 82 { 8 } else { 4 };
        }
        if source == 83 && known.contains(&native(85)) {
            unit.weapon = Some(Weapon {
                damage: 0,
                range: 256,
                cooldown: 60,
                targets_air: false,
                cooldown_jitter: None,
                damage_kind: DamageKind::Normal,
                splash: None,
                strikes: vec![],
            });
            unit.attacks_ground = true;
            unit.stored_weapon = Some(StoredWeapon::Fighters {
                unit: native(85),
                launch_ticks: 60,
                leash: 512,
                repair: 0,
                expendable: Some(90),
            });
            unit.trains = vec![native(85)];
            unit.production_capacity = 5;
        }
        if source == 85 {
            unit.portable = false;
            unit.blocks_movement = false;
            unit.triggers_mines = false;
            if let Some(weapon) = &mut unit.weapon {
                weapon.range = 16;
                weapon.strikes.clear();
            }
        }
        let abilities: &[u16] = match source {
            1 | 16 => &[3, 6],
            9 => &[1, 2, 5],
            12 | 28 | 29 => &[4],
            45 | 51 => &[7, 8, 9, 10],
            46 => &[11, 12, 13],
            67 | 79 => &[14, 15, 18],
            71 => &[16, 17],
            172 => &[19],
            134 => &[20],
            _ => continue,
        };
        let hero = dword(&tables.units, 0x19b0 + usize::from(source) * 4) & 0x40 != 0;
        if source == 1 || source == 16 {
            unit.cloak = Some(Cloak {
                energy_max: if hero { 250 } else { 200 },
                activation_cost: word_energy(10),
                regeneration: 8,
                drain: 10,
                ..Cloak::default()
            });
        } else if source != 134 {
            unit.energy_pool = Some(EnergyPool {
                maximum: if hero { 250 } else { 200 },
                initial: 50,
                regeneration: 8,
            });
        }
        unit.abilities = definitions
            .iter()
            .filter(|a| abilities.contains(&a.id.0))
            .filter(|a| match &a.effect {
                AbilityEffect::Strike {
                    ammunition: Some(id),
                    ..
                } => known.contains(id),
                AbilityEffect::KillSpawn { unit, .. }
                | AbilityEffect::Merge { result: unit, .. }
                | AbilityEffect::Infest { to: unit, .. } => known.contains(unit),
                _ => true,
            })
            .cloned()
            .map(|mut a| {
                if hero {
                    a.research = None;
                }
                a
            })
            .collect();
    }
    Ok(())
}
