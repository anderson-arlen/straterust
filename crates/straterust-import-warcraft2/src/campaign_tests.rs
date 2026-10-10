//! End states from the campaign objectives, exercised against the actual imports.
//! These are outcome checks, not substitutes for ordinary-command gameplay checks.
use super::gameplay_tests::{load, root};
use super::stats::id;
use straterust_engine::sim::*;

#[test]
#[ignore = "requires STRATERUST_WARCRAFT2 pointing to a retail import"]
fn all_campaign_objectives_accept_completed_goals_and_reject_player_elimination() {
    let root = root();
    let mut checked = 0;
    for (campaign, race, expansion, count) in [
        ("human", 0, false, 14),
        ("orc", 1, false, 14),
        ("human-expansion", 0, true, 12),
        ("orc-expansion", 1, true, 12),
    ] {
        for number in 1..=count {
            let original = load(&root, campaign, number);
            let mut map = original.map().clone();
            map.spawns.clear();
            map.resources.clear();
            map.terrain = None;
            map.ai.clear();
            map.creation.clear();
            let mut spawns = Vec::new();
            // All required construction totals and all surviving campaign heroes.
            // Actual definitions remain intact; only the scenario is advanced to
            // the declared completed objectives.
            for (source, count) in [
                (2 + race, 1),
                (58 + race, 4),
                (60 + race, 1),
                (72 + race, 5),
                (86 + race, 4),
                (90 + race, 1),
                (70 + race, 1),
            ] {
                if original.unit_type(id(source)).is_some() {
                    for _ in 0..count {
                        spawns.push(source);
                    }
                }
            }
            for source in [20, 21, 22, 23, 24, 25, 44, 46, 47, 49, 52, 53] {
                if original
                    .map()
                    .spawns
                    .iter()
                    .any(|s| s.unit_type == id(source))
                {
                    spawns.push(source);
                }
            }
            for (index, source) in spawns.into_iter().enumerate() {
                let columns = (map.width / 160 - 1) as usize;
                map.spawns.push(Spawn {
                    owner: PlayerId(0),
                    unit_type: id(source),
                    position: Position {
                        x: 80 + (index % columns) as i32 * 160,
                        y: 80 + (index / columns) as i32 * 160,
                    },
                    ..Default::default()
                });
            }
            let escorts: &[usize] = match (expansion, race, number) {
                (false, 0, 2) => &[8],
                (false, 1, 2) => &[53],
                (false, 0, 9) => &[52],
                (false, 0, 10) => &[16, 16, 16, 16],
                (false, 1, 6) => &[49],
                (true, 0, 1) => &[20, 46, 44],
                (true, 0, 3) => &[44],
                (true, 0, 6) => &[46, 44],
                (true, 1, 10) => &[10],
                _ => &[],
            };
            if !escorts.is_empty() {
                let circle = original
                    .map()
                    .spawns
                    .iter()
                    .find(|s| s.unit_type == id(100))
                    .expect("escort destination")
                    .position;
                map.spawns.retain(|s| {
                    !escorts.contains(&usize::from(s.unit_type.0 - 1))
                        && ((s.position.x - circle.x).abs() > 160
                            || (s.position.y - circle.y).abs() > 160)
                });
                for (i, source) in escorts.iter().enumerate() {
                    map.spawns.push(Spawn {
                        owner: PlayerId(0),
                        unit_type: id(*source),
                        position: Position {
                            x: circle.x - 24 + (i % 2) as i32 * 48,
                            y: circle.y - 24 + (i / 2) as i32 * 48,
                        },
                        ..Default::default()
                    });
                }
            }
            if (expansion, race, number) == (false, 1, 9) {
                map.spawns
                    .retain(|s| ![id(91), id(73)].contains(&s.unit_type));
                for (source, x, y) in [(91, 60, 25), (73, 64, 35)] {
                    map.spawns.push(Spawn {
                        owner: PlayerId(0),
                        unit_type: id(source),
                        position: Position {
                            x: x * 32,
                            y: y * 32,
                        },
                        ..Default::default()
                    });
                }
            }
            let mut completed = World::new(original.rules().clone(), map, 7)
                .unwrap_or_else(|e| panic!("{campaign} {number}: {e}"));
            for _ in 0..3 {
                completed.step(&[]).unwrap();
            }
            assert_eq!(
                completed.state().winner,
                Some(PlayerId(0)),
                "{campaign} {number} completed objectives"
            );
            let mut map = original.map().clone();
            map.spawns.retain(|s| s.owner != PlayerId(0));
            map.ai.clear();
            let mut defeated = World::new(original.rules().clone(), map, 7).unwrap();
            for _ in 0..3 {
                defeated.step(&[]).unwrap();
            }
            assert!(
                defeated.state().defeated.contains(&PlayerId(0)),
                "{campaign} {number} player elimination"
            );
            checked += 1;
        }
    }
    assert_eq!(checked, 52);
}
