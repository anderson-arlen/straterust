//! Campaign opponents use the native town AI and ordinary production/orders.
//! AIPL remains importer metadata; no source bytecode is executed at runtime.
use super::{pud::Pud, stats::id};
use straterust_engine::sim::*;

pub fn configure(pud: &Pud, number: usize, expansion: bool, rules: &Rules, map: &mut Map) {
    for source in 0..16 {
        let player = PlayerId(u16::from(pud.player(source)));
        let units: Vec<_> = map.spawns.iter().filter(|s| s.owner == player).collect();
        let home = units
            .iter()
            .min_by_key(|s| {
                let definition = rules.units.iter().find(|u| u.id == s.unit_type).unwrap();
                (
                    !definition.dropoff.iter().any(|k| k == "gold"),
                    !definition.structure,
                    s.position.x,
                    s.position.y,
                )
            })
            .map(|s| s.position);
        let Some(home) = home else { continue };
        if (source == pud.local || matches!(pud.owners[source], 4..=7))
            && !map.start_locations.iter().any(|s| s.player == player)
        {
            map.start_locations.push(StartLocation {
                player,
                position: home,
            });
        }
        if source == pud.local || pud.owners[source] != 4 {
            continue;
        }
        let race = usize::from(pud.sides[source]);
        if race > 1 {
            continue;
        }
        let allowed = |n| {
            map.creation
                .get(&player)
                .is_none_or(|u| u.contains(&id(n + race)))
        };
        let has = |n| units.iter().any(|u| u.unit_type == id(n + race));
        let profile = pud
            .chunks
            .get(b"AIPL")
            .and_then(|a| a.get(source))
            .copied()
            .unwrap_or(0);
        let passive = profile == 1;
        let mut program = Vec::new();
        let economy = units.iter().any(|s| {
            rules.units.iter().any(|u| {
                u.id == s.unit_type && (u.worker.is_some() || u.trains.contains(&id(2 + race)))
            })
        });
        if economy && !passive {
            let mut request = |n, count, priority| {
                if allowed(n) {
                    program.push(AiInstruction::Request {
                        unit_type: id(n + race),
                        count,
                        priority,
                    });
                }
            };
            request(74, 1, 250);
            request(
                2,
                if expansion {
                    12
                } else {
                    (5 + number).min(12) as u16
                },
                240,
            );
            request(60, 1, 220);
            request(76, 1, 210);
            if number >= 5 || expansion {
                request(82, 1, 200);
                request(88, 1, 180);
            }
            if number >= 6 || expansion {
                request(66, 1, 160);
                request(68, 1, 140);
            }
            if number >= 8 || expansion {
                request(62, 1, 130);
                request(90, 1, 120);
            }
            if number >= 11 || expansion {
                request(80, 1, 100);
            }
            if number >= 13 || expansion {
                request(70, 1, 90);
            }
            let navy = has(72) || has(26) || has(30) || matches!(profile, 25 | 26);
            if navy {
                request(72, 1, 200);
                request(26, 2, 190);
                request(86, 1, 180);
                request(78, 1, 170);
            }
            for (n, count) in [(0, 2), (8, 2), (30, 1)] {
                if allowed(n) && (n != 30 || navy) {
                    program.push(AiInstruction::Defense {
                        unit_type: id(n + race),
                        count,
                    });
                }
            }
        }
        let wave = program.len() as u16;
        program.push(AiInstruction::Wait(if expansion {
            1800
        } else {
            ((number + 2) * 600) as u32
        }));
        program.push(AiInstruction::AttackClear);
        let naval = has(72) || has(30) || has(32) || profile == 25;
        let mut attack_count = 0;
        for (n, count) in [
            (0, 4),
            (8, 3),
            (6, 2),
            (4, 1),
            (10, 1),
            (30, 2),
            (32, 1),
            (42, 2),
        ] {
            let can_produce = economy
                && allowed(n)
                && match n {
                    6 => number >= 6 || expansion,
                    4 => number >= 5 || expansion,
                    10 => number >= 11 || expansion,
                    30 | 32 => naval,
                    42 => profile == 26 || has(70),
                    _ => true,
                };
            let present = units.iter().filter(|u| u.unit_type == id(n + race)).count() as u16;
            let count = if can_produce {
                count
            } else {
                present.min(count)
            };
            if count > 0 {
                program.push(AiInstruction::AttackAdd {
                    unit_type: id(n + race),
                    count,
                });
                attack_count += count;
            }
        }
        if attack_count > 0 {
            program.extend([AiInstruction::AttackPrepare, AiInstruction::Attack]);
            if economy {
                program.push(AiInstruction::Jump(wave));
            } else {
                program.push(AiInstruction::Stop);
            }
        } else {
            program.push(AiInstruction::Stop);
        }
        // Passive AIPL towns retain local defense without initiating assaults.
        map.ai.push(AiController {
            harvest_weights: [("gold", 5), ("wood", 4), ("oil", 2)]
                .into_iter()
                .map(|(kind, amount)| ResourceAmount {
                    kind: kind.into(),
                    amount,
                })
                .collect(),
            research: rules
                .research
                .iter()
                .filter(|r| r.available && usize::from(r.id.0 / 64) == race)
                .map(|r| r.id)
                .collect(),
            abilities: rules
                .units
                .iter()
                .filter(|u| usize::from((u.id.0 - 1) % 2) == race)
                .flat_map(|u| u.abilities.iter().map(|a| a.id))
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect(),
            player,
            home,
            radius: 1536,
            // Placed forces without a town are guards, not an attacking economy.
            // Local combat/defense still runs for inactive controllers.
            active: !passive && economy,
            program,
        });
    }
}
