//! Native skirmish plans: paid economy, early pressure, then repeated mixed waves.
use super::*;

fn infantry_program(race: usize, difficulty: AiDifficulty) -> Vec<AiInstruction> {
    use AiInstruction::*;
    let (workers, first, melee, ranged, rest) = match difficulty {
        AiDifficulty::Easy => (8, 3, 4, 2, 1600),
        AiDifficulty::Normal => (12, 2, 6, 3, 600),
        AiDifficulty::Hard => (16, 2, 8, 4, 200),
    };
    let request = |n, count, priority| Request {
        unit_type: stats::id(n + race),
        count,
        priority,
    };
    vec![
        request(74, 1, 250), // hall
        request(2, 5, 245),  // expand income without starving military production
        request(60, 1, 240), // barracks
        request(76, 1, 210), // lumber mill
        Defense {
            unit_type: stats::id(race),
            count: 1,
        },
        AttackClear,
        AttackAdd {
            unit_type: stats::id(race),
            count: first,
        },
        AttackPrepare,
        Attack,
        request(2, workers, 230),
        request(60, 2, 220),
        request(82, 1, 200), // blacksmith, enabling weapon/armor research
        AttackClear,
        AttackAdd {
            unit_type: stats::id(race),
            count: melee,
        },
        AttackAdd {
            unit_type: stats::id(8 + race),
            count: ranged,
        },
        AttackPrepare,
        Attack,
        Wait(rest),
        Jump(12),
    ]
}

pub fn program(race: usize, difficulty: AiDifficulty) -> Vec<AiInstruction> {
    use AiInstruction::*;
    let (workers, first, melee, ranged, ships, flyers, rest) = match difficulty {
        AiDifficulty::Easy => (8, 3, 4, 2, 2, 1, 1600),
        AiDifficulty::Normal => (12, 2, 6, 3, 3, 2, 600),
        AiDifficulty::Hard => (16, 2, 8, 4, 4, 3, 200),
    };
    let request = |n, count, priority| Request {
        unit_type: stats::id(n + race),
        count,
        priority,
    };
    let maintain = |n, count, priority| Maintain {
        unit_type: stats::id(n + race),
        count,
        priority,
    };
    let attack = |n, count| AttackAdd {
        unit_type: stats::id(n + race),
        count,
    };
    let mut plan = vec![
        request(74, 1, 250),
        request(2, 5, 245),
        request(60, 1, 240),
        request(76, 1, 210),
        Defense {
            unit_type: stats::id(race),
            count: 1,
        },
        AttackClear,
        attack(0, first),
        AttackPrepare,
        JumpIfTransportNeeded(10),
        Attack,
        request(2, workers, 230),
        request(60, 2, 220),
        request(82, 1, 200),
        maintain(0, melee * 2 + 1, 160),
        maintain(8, ranged * 2, 150),
        // A standing air force is funded once; it cannot consume every future
        // purchase merely because its first riders have left the town.
        maintain(42, flyers, 145),
    ];
    let repeat = plan.len() as u16;
    plan.extend([
        AttackClear,
        attack(0, melee),
        attack(8, ranged),
        AttackPrepare,
    ]);
    let branch = plan.len();
    plan.extend([JumpIfTransportNeeded(0), AttackAvailable, Jump(0)]);
    let naval = plan.len() as u16;
    plan.extend([
        request(72, 1, 225),
        request(26, 2, 215),
        request(86, 1, 210),
        request(78, 1, 208),
        maintain(28, (melee + ranged).div_ceil(6), 207),
        maintain(30, ships, 206),
        AttackAvailable,
        AttackClear,
        attack(30, ships),
        AttackPrepare,
        AttackAvailable,
    ]);
    let air = plan.len() as u16;
    plan[branch] = JumpIfTransportNeeded(naval);
    plan[branch + 2] = Jump(air);
    plan.extend([
        AttackClear,
        attack(42, flyers),
        AttackPrepare,
        AttackAvailable,
        Wait(rest),
        Jump(repeat),
    ]);
    plan
}

/// Upgrade older imported metadata in memory; read-only packaged games benefit
/// immediately, without rebuilding terrain/art or invalidating campaign saves.
pub fn upgrade_catalog(catalog: &mut Catalog) {
    if catalog.game != "warcraft2" {
        return;
    }
    if catalog.colors.is_empty() {
        catalog.colors = super::super::colors::catalog();
    }
    for (race, faction) in catalog.factions.iter_mut().take(2).enumerate() {
        for source in super::equivalent_sources() {
            faction
                .equivalents
                .entry(stats::id(source))
                .or_insert(stats::id((source & !1) + race));
        }
        // Only migrate converter output. Authored replacement plans keep their
        // program and instruction indexes, including in read-only packages.
        let legacy = faction.ai.len() == 11
            && faction.ai.first()
                == Some(&AiInstruction::Request {
                    unit_type: stats::id(74 + race),
                    count: 1,
                    priority: 250,
                })
            && faction.ai.get(8) == Some(&AiInstruction::Wait(15000))
            && faction.ai.last() == Some(&AiInstruction::Jump(4));
        let old = faction.ai == infantry_program(race, AiDifficulty::Normal);
        let current = faction.ai == program(race, AiDifficulty::Normal);
        let known_difficulties = faction.ai_difficulties.iter().all(|(difficulty, plan)| {
            *plan == infantry_program(race, *difficulty) || *plan == program(race, *difficulty)
        });
        if !known_difficulties || (!legacy && !old && !current) {
            continue;
        }

        faction.ai = program(race, AiDifficulty::Normal);
        faction.ai_difficulties = [AiDifficulty::Easy, AiDifficulty::Normal, AiDifficulty::Hard]
            .into_iter()
            .map(|difficulty| (difficulty, program(race, difficulty)))
            .collect();
        faction.ai_research = super::super::technology::definitions()
            .into_iter()
            .filter(|t| {
                usize::from(t.rule.id.0 / 64) == race
                    && matches!(
                        t.rule.effect,
                        ResearchEffect::WeaponUpgrade { .. }
                            | ResearchEffect::WeaponDamage { .. }
                            | ResearchEffect::Armor { .. }
                            | ResearchEffect::WeaponRange { .. }
                            | ResearchEffect::UnitUpgrade { .. }
                    )
            })
            .map(|t| t.rule.id)
            .collect();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires STRATERUST_GAME_ROOT; verifies actual imported army production and attacks"]
    fn retail_multiplayer_ai_builds_an_army_and_launches_attacks() -> anyhow::Result<()> {
        let root = std::path::PathBuf::from(
            std::env::var_os("STRATERUST_GAME_ROOT").expect("native game directory"),
        );
        let mut catalog = Catalog::load(&root)?;
        upgrade_catalog(&mut catalog);
        let map = catalog
            .maps
            .iter()
            .position(|m| m.players == 2 && m.size[0] <= 96)
            .unwrap();
        for faction in 0..2 {
            for difficulty in [AiDifficulty::Easy, AiDifficulty::Normal, AiDifficulty::Hard] {
                let mut settings = catalog.defaults(map);
                settings.ai_difficulty = difficulty;
                settings.map_units = false;
                settings.random_locations = false;
                settings.slots[0].faction = Some(1 - faction);
                settings.slots[1].faction = Some(faction);
                settings.slots[1].kind = SlotKind::Computer;
                let mut world = catalog.prepare(&root, map, &settings, &[0], 42)?.world;
                let mut trained = 0;
                let mut deployed = 0;
                for _ in 0..36000 {
                    world.step(&[])?;
                    trained = world
                        .state()
                        .entities
                        .iter()
                        .filter(|e| {
                            e.owner == PlayerId(1)
                                && world.unit_type(e.unit_type).is_some_and(|u| {
                                    !u.structure && u.worker.is_none() && u.weapon.is_some()
                                })
                        })
                        .count();
                    deployed = world.state().ai.iter().map(|s| s.deployed.len()).sum();
                    if deployed > 0 {
                        break;
                    }
                }
                assert!(
                    trained >= 3 && deployed > 0,
                    "{} {difficulty:?} AI failed: trained={trained}, deployed={deployed}; AI={:?}, economy={:?}",
                    catalog.factions[faction].name,
                    world.state().ai,
                    world.state().statistics[1]
                );
                println!(
                    "{} {difficulty:?}: attack tick={} army={trained} deployed={deployed} resources_collected={:?}",
                    catalog.factions[faction].name,
                    world.tick().0,
                    world.state().statistics[1].resources_collected
                );
            }
        }
        Ok(())
    }
}
