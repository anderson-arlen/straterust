//! Native rules from Warcraft II's unit-data record and original command graph.
use super::pud::word;
use anyhow::{Context, Result, ensure};
use straterust_engine::{
    map::{Footprint, MovementClass},
    sim::*,
};

pub fn id(source: usize) -> UnitTypeId {
    UnitTypeId(source as u16 + 1)
}

pub fn names(bytes: &[u8]) -> Result<Vec<String>> {
    let count = usize::from(word(bytes, 0)?);
    ensure!(
        count <= 8192 && bytes.len() <= 1024 * 1024,
        "invalid string table"
    );
    let mut names = Vec::with_capacity(count);
    for index in 0..count {
        let start = usize::from(word(bytes, 2 + index * 2)?);
        let tail = bytes.get(start..).context("string offset outside table")?;
        let end = tail
            .iter()
            .position(|v| *v == 0)
            .context("unterminated source string")?;
        names.push(
            String::from_utf8_lossy(&tail[..end])
                .chars()
                .filter(|c| !c.is_control() || *c == '\n')
                .collect(),
        );
    }
    Ok(names)
}

pub fn unit_name(source: usize, names: &[String]) -> String {
    match source {
        999..=1087 => "Wall".into(),
        36 => "Human Scout Ship".into(),
        37 => "Orc Scout Ship".into(),
        _ => names[source + 1]
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" "),
    }
}

pub fn rules(data: &[u8]) -> Result<Rules> {
    // Archive default records omit the two-byte PUD use-default flag.
    ensure!(
        data.len() >= 5694 && data.len() <= 8192,
        "invalid default unit data"
    );
    let mut rules = Rules {
        id: "warcraft2-bne".into(),
        tick_ms: 33,
        supply_limit: 200,
        repair: Some(RepairRules {
            rate_numerator: 4,
            rate_denominator: 1,
            cost_divisor: 2,
            range: 32,
        }),
        ..Rules::default()
    };
    let dword = |offset: usize| -> Result<u32> {
        Ok(u32::from_le_bytes(
            data.get(offset..offset + 4)
                .context("truncated unit stats")?
                .try_into()?,
        ))
    };
    for source in 0..110 {
        let hp =
            u32::from(word(data, 1676 + source * 2)?).max(u32::from(matches!(source, 100 | 102)));
        if hp == 0 || matches!(source, 34 | 48 | 54) || (92..=95).contains(&source) || source >= 103
        {
            continue;
        }
        let flags = dword(5254 + source * 4)?;
        let structure = flags & 32 != 0;
        let size = dword(2446 + source * 4)?;
        let placement = Footprint {
            width: ((size & 0xffff) * 32) as u16,
            height: ((size >> 16) * 32) as u16,
        };
        let class = if flags & 2 != 0 {
            MovementClass::Air
        } else if flags & 8 != 0 {
            MovementClass::Water
        } else {
            MovementClass::Ground
        };
        let basic = u32::from(data[3986 + source]);
        let pierce = u32::from(data[4096 + source]);
        let range = u32::from(data[3326 + source]) * 32;
        let speed = if structure {
            0
        } else {
            match source {
                4 | 5 => 1,
                6 | 7 | 12 | 13 | 44 | 50 | 52 => 3,
                _ => 2,
            }
        };
        let mut unit = UnitType {
            id: id(source),
            max_hp: hp,
            structure,
            placement,
            footprint: if structure {
                placement
            } else if class == MovementClass::Water {
                Footprint {
                    width: placement.width.saturating_sub(8).max(24),
                    height: placement.height.saturating_sub(8).max(24),
                }
            } else {
                Footprint {
                    width: 24,
                    height: 24,
                }
            },
            speed,
            motion: (speed > 0).then_some(Motion {
                eight_directions: true,
                speed: speed as u32 * 256,
                acceleration: 0,
                steps: Vec::new(),
            }),
            movement_class: class,
            armor: u32::from(data[3656 + source]),
            vision_range: dword(1236 + source * 4)? * 32,
            acquisition_range: (data[3546 + source] > 0)
                .then_some(u32::from(data[3546 + source]) * 32),
            build_ticks: u32::from(data[2006 + source]).max(1) * 6,
            supply_used: u32::from(!structure && !matches!(source,26..=41|45|55..=57)),
            supply_provided: if matches!(source, 58 | 59) {
                4
            } else if matches!(source, 74 | 75 | 88..=91) {
                1
            } else {
                0
            },
            cost: [("gold", 2116), ("wood", 2226), ("oil", 2336)]
                .into_iter()
                .map(|(kind, p)| ResourceAmount {
                    kind: kind.into(),
                    amount: u32::from(data[p + source]) * 10,
                })
                .filter(|a| a.amount != 0)
                .collect(),
            ..UnitType::default()
        };
        if matches!(source, 38 | 39) {
            unit.cloak = Some(Cloak {
                permanent: true,
                ..Default::default()
            });
        }
        if matches!(source,22|35|38..=43|45|64|65|96..=99) {
            unit.detector_range = unit.vision_range;
        }
        if basic + pierce > 0 && data[5144 + source] != 0 {
            unit.weapon = Some(Weapon {
                friendly_splash: matches!(source, 4 | 5 | 32 | 33 | 98 | 99),
                projectile_speed: missile_speed(source),
                damage: basic + pierce,
                range: range.max(24),
                cooldown: match source {
                    8 | 18 | 20 => 65,
                    9 | 19 | 53 => 74,
                    10 | 11 | 21 | 24 | 51 => 40,
                    4 | 5 => 200,
                    30 | 31 => 120,
                    32 | 33 => 230,
                    38 | 39 => 115,
                    22 | 35 | 42 | 43 => 200,
                    96 | 97 => 60,
                    98 | 99 => 151,
                    _ => 25,
                },
                cooldown_jitter: None,
                targets_air: data[5144 + source] & 4 != 0,
                target_classes: [
                    (1, MovementClass::Ground),
                    (2, MovementClass::Water),
                    (4, MovementClass::Air),
                ]
                .into_iter()
                .filter(|(mask, _)| data[5144 + source] & mask != 0)
                .map(|(_, class)| class)
                .collect(),
                damage_kind: DamageKind::Split {
                    piercing: pierce,
                    minimum_percent: 50,
                },
                splash: if matches!(source, 4 | 5 | 32 | 33 | 98 | 99) {
                    Some([16, 32, 48])
                } else {
                    None
                },
                strikes: Vec::new(),
            });
        }
        if unit.weapon.is_none() {
            unit.acquisition_range = None;
        }
        unit.builder_inside = structure;
        unit.repair_construction = structure;
        if matches!(source, 2 | 3 | 16 | 17) {
            unit.worker = Some(WorkerStats {
                capacity: 100,
                harvest_amount: 100,
                harvest_ticks: 150,
                build_rate: 1,
                resource_kinds: vec!["gold".into(), "wood".into()],
                idle_resource_radius: 256,
            });
            unit.harvest_profiles = vec![
                // Native adjacent tiles leave four pixels around each smaller
                // collision box. Eight admits side and diagonal entry alike.
                HarvestProfile {
                    kind: "gold".into(),
                    capacity: 5,
                    inside: true,
                    amount: 100,
                    ticks: 150,
                    entry_range: 8,
                    depot_ticks: 150,
                    depot_inside: true,
                },
                HarvestProfile {
                    kind: "wood".into(),
                    capacity: 1,
                    inside: false,
                    amount: 2,
                    ticks: 24,
                    entry_range: 8,
                    depot_ticks: 150,
                    depot_inside: true,
                },
            ];
        }
        if matches!(source, 26 | 27) {
            unit.worker = Some(WorkerStats {
                capacity: 100,
                harvest_amount: 100,
                harvest_ticks: 150,
                build_rate: 1,
                resource_kinds: vec!["oil".into()],
                idle_resource_radius: 256,
            });
            unit.harvest_profiles = vec![HarvestProfile {
                kind: "oil".into(),
                capacity: 1,
                inside: true,
                amount: 100,
                ticks: 150,
                entry_range: 8,
                depot_ticks: 0,
                depot_inside: false,
            }];
        }
        if matches!(source, 74 | 75 | 88..=91) {
            unit.dropoff = vec!["gold".into(), "wood".into()];
        }
        let bonus = match source {
            76 | 77 => Some(("wood", 25)),
            84 | 85 => Some(("oil", 25)),
            88 | 89 => Some(("gold", 10)),
            90 | 91 => Some(("gold", 20)),
            _ => None,
        };
        if let Some((kind, amount)) = bonus {
            unit.harvest_bonus_percent = vec![ResourceAmount {
                kind: kind.into(),
                amount,
            }];
        }
        if matches!(source, 88 | 89) {
            unit.provides_types = vec![id(source - 14)];
        }
        if matches!(source, 90 | 91) {
            unit.provides_types = vec![id(source - 16), id(source - 2)];
        }
        if matches!(source, 76 | 77) {
            unit.dropoff = vec!["wood".into()];
        }
        if matches!(source, 72 | 73 | 84 | 85) {
            unit.dropoff = vec!["oil".into()];
        }
        if source == 57 {
            unit.neutral = true;
            unit.idle_wander = Some(IdleWander {
                distance: 64,
                pause_ticks: [30, 120],
                move_ticks: 90,
            });
        }
        if source == 100 {
            unit.blocks_movement = false;
        }
        if matches!(source, 72 | 73 | 78 | 79 | 84 | 85) {
            unit.placement_surface = straterust_engine::map::PlacementSurface::Shore;
        }
        if matches!(source, 86 | 87) {
            unit.builder_gathers_resource = true;
            unit.placement_surface = straterust_engine::map::PlacementSurface::Water;
            unit.movement_class = MovementClass::Water;
            unit.extracts = Some(Extraction {
                resource: "oil".into(),
                harvest_ticks: 150,
                depleted_amount: 0,
            });
        }
        rules.units.push(unit);
    }
    let passengers = rules
        .units
        .iter()
        .filter(|u| !u.structure && u.movement_class == MovementClass::Ground)
        .map(|u| u.id)
        .collect::<Vec<_>>();
    for source in [28, 29] {
        rules
            .units
            .iter_mut()
            .find(|u| u.id == id(source))
            .unwrap()
            .garrison = Some(GarrisonStats {
            boarding_range: 16,
            capacity: 6,
            passengers: passengers.clone(),
            attackers: Vec::new(),
            range_bonus: 0,
            unload_ticks: 30,
        });
    }
    command_graph(&mut rules);
    rules.research = super::technology::definitions()
        .into_iter()
        .map(|t| t.rule)
        .collect();
    super::spells::add(&mut rules);
    Ok(rules)
}

fn command_graph(rules: &mut Rules) {
    for race in 0..2 {
        let get = |source: usize| id(source + race);
        let buildings: Vec<_> = (58..=86).step_by(2).filter(|s| *s != 86).map(get).collect();
        let repairs: Vec<_> = rules
            .units
            .iter()
            .filter(|u| u.structure && usize::from(u.id.0 - 1) % 2 == race)
            .map(|u| u.id)
            .collect();
        for worker in [2 + race, 16 + race] {
            if let Some(u) = rules.units.iter_mut().find(|u| u.id == id(worker)) {
                u.builds = buildings.clone();
                u.repairs = repairs.clone();
            }
        }
        if let Some(u) = rules.units.iter_mut().find(|u| u.id == id(26 + race)) {
            u.builds = vec![get(86)];
        }
        for (producer, children) in [
            (74, vec![2]),
            (88, vec![2]),
            (90, vec![2]),
            (60, vec![0, 8, 6, 4]),
            (80, vec![10]),
            (68, vec![14, 40]),
            (70, vec![42]),
            (72, vec![26, 28, 30, 32, 38]),
        ] {
            if let Some(u) = rules.units.iter_mut().find(|u| u.id == get(producer)) {
                u.trains = children.into_iter().map(get).collect();
            }
        }
        for (producer, target) in [(74, 88), (88, 90), (64, 96)] {
            if let Some(u) = rules.units.iter_mut().find(|u| u.id == get(producer)) {
                u.transforms_on_production = true;
                u.trains.push(get(target));
                if producer == 64 {
                    u.trains.push(get(98));
                }
            }
        }
        for (unit, requirements) in [
            (4, vec![82]),
            (88, vec![60]),
            (90, vec![62, 66]),
            (96, vec![76]),
            (98, vec![82]),
            (8, vec![76]),
            (6, vec![66]),
            (66, vec![82]),
            (68, vec![88]),
            (70, vec![90]),
            (80, vec![90]),
            (32, vec![78]),
            (38, vec![68]),
        ] {
            if let Some(u) = rules.units.iter_mut().find(|u| u.id == get(unit)) {
                u.prerequisites = requirements.into_iter().map(get).collect();
            }
        }
    }
}

/// Source weapon speeds are gameplay data; the native renderer uses these too.
pub fn missile_speed(source: usize) -> u32 {
    let speed = match source {
        4 | 5 => 8,
        8 | 9 | 18 | 19 | 20 | 53 | 96 | 97 => 32,
        30 | 31 => 22,
        32 | 33 => 44,
        10 | 11 | 21 | 22 | 24 | 35 | 38 | 39 | 42 | 43 | 51 | 56 | 98 | 99 => 16,
        _ => 0,
    };
    speed * 256
}
