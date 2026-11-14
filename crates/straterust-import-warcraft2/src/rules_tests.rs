use super::{
    presentation,
    stats::{self, id},
    technology,
};
use straterust_engine::sim::*;

#[test]
fn both_building_trees_and_caster_buttons_match_native_dependencies() {
    let mut rules = Rules {
        units: (0..103)
            .map(|n| UnitType {
                id: id(n),
                structure: n >= 58,
                ..Default::default()
            })
            .collect(),
        ..Default::default()
    };
    stats::upgrade(&mut rules);
    let first = ron::ser::to_string(&rules).unwrap();
    stats::upgrade(&mut rules);
    assert_eq!(
        ron::ser::to_string(&rules).unwrap(),
        first,
        "upgrades must be idempotent"
    );
    // Every worker-built structure, including the structures with no dependencies.
    for race in 0..2 {
        for (source, kind) in [
            (74, "gold"),
            (88, "gold"),
            (90, "gold"),
            (72, "oil"),
            (84, "oil"),
        ] {
            let unit = rules
                .units
                .iter()
                .find(|unit| unit.id == id(source + race))
                .unwrap();
            assert_eq!(unit.resource_clearance, 96);
            assert_eq!(unit.resource_clearance_tile_size, 32);
            assert_eq!(unit.resource_clearance_kinds, [kind]);
        }
        for (source, needs) in [
            (58, vec![]),
            (60, vec![]),
            (62, vec![90]),
            (64, vec![]),
            (66, vec![88]),
            (68, vec![88]),
            (70, vec![90]),
            (72, vec![76]),
            (74, vec![]),
            (76, vec![]),
            (78, vec![72]),
            (80, vec![90]),
            (82, vec![]),
            (84, vec![72]),
            (86, vec![]),
            (88, vec![60]),
            (90, vec![66, 82, 76]),
            (96, vec![76]),
            (98, vec![82]),
        ] {
            let unit = rules
                .units
                .iter()
                .find(|u| u.id == id(source + race))
                .unwrap();
            assert_eq!(
                unit.prerequisites,
                needs.into_iter().map(|n| id(n + race)).collect::<Vec<_>>(),
                "building {source}, race {race}"
            );
        }
        let upgrade = technology::definitions()
            .into_iter()
            .find(|t| t.rule.id == ResearchId(17 + race as u16 * 64))
            .unwrap()
            .rule;
        assert_eq!(upgrade.facility, id(62 + race));
        assert_eq!(upgrade.prerequisites, vec![id(90 + race)]);
        assert_eq!(
            upgrade.effect,
            ResearchEffect::UnitUpgrade {
                units: vec![id(6 + race)],
                to: id(12 + race)
            }
        );
    }
    let names = vec!["Test".to_string(); 2048];
    let p = presentation::build(&rules, &names, "Test".into());
    assert!(super::descriptions::unit(2).unwrap().contains("constructs"));
    assert!(super::descriptions::unit(62).unwrap().contains("Paladins"));
    assert!(
        super::descriptions::unit(63)
            .unwrap()
            .contains("Ogre-Mages")
    );
    let value = ron::ser::to_string(&p).unwrap();
    let mut old: presentation::Presentation = ron::from_str(&value).unwrap();
    presentation::upgrade(&mut old);
    // Fresh imports and upgraded installations must produce identical commands.
    assert_eq!(ron::ser::to_string(&old).unwrap(), value);
}

fn supply_rules() -> Rules {
    let mut data = vec![0_u8; 5694];
    for source in 0..110 {
        data[1676 + source * 2..1678 + source * 2].copy_from_slice(&100_u16.to_le_bytes());
        data[2446 + source * 4..2450 + source * 4].copy_from_slice(&0x0001_0001_u32.to_le_bytes());
        data[3326 + source] = 4;
        data[3986 + source] = 10;
        data[5144 + source] = 7;
        let flags = if source >= 58 {
            32_u32
        } else if matches!(source, 22 | 35 | 40..=43 | 56) {
            2
        } else if matches!(source, 26..=33 | 36..=39) {
            8
        } else {
            0
        };
        data[5254 + source * 4..5258 + source * 4].copy_from_slice(&flags.to_le_bytes());
    }
    stats::rules(&data).unwrap()
}

#[test]
fn fresh_and_upgraded_rosters_charge_original_food_for_all_units() {
    let fresh = supply_rules();
    let mut older = fresh.clone();
    for unit in &mut older.units {
        unit.supply_used = 0;
        unit.supply_provided = 0;
    }
    stats::upgrade(&mut older);
    for rules in [&fresh, &older] {
        for unit in &rules.units {
            let source = usize::from(unit.id.0 - 1);
            // One food for land units, heroes, ships and aircraft. The Eye,
            // Skeleton, Daemon, critter and all buildings do not consume food.
            let used = match source {
                0..=44 | 46..=53 => 1,
                _ => 0,
            };
            let provided = match source {
                58 | 59 => 4,
                74 | 75 | 88..=91 => 1,
                _ => 0,
            };
            assert_eq!(unit.supply_used, used, "source {source}");
            assert_eq!(unit.supply_provided, provided, "source {source}");
            for child in unit
                .trains
                .iter()
                .filter_map(|id| rules.units.iter().find(|u| u.id == *id))
                .filter(|u| !u.structure)
            {
                assert_eq!(
                    child.supply_used, 1,
                    "every trained unit, including {:?}",
                    child.id
                );
            }
        }
    }
    let once = ron::to_string(&older).unwrap();
    stats::upgrade(&mut older);
    assert_eq!(ron::to_string(&older).unwrap(), once);
}

#[test]
fn fresh_and_upgraded_workers_repair_both_factions_without_building_foreign_structures() {
    let fresh = supply_rules();
    let mut older = fresh.clone();
    for worker in older.units.iter_mut().filter(|u| u.worker.is_some()) {
        worker
            .repairs
            .retain(|target| target.0 % 2 == worker.id.0 % 2);
    }
    stats::upgrade(&mut older);
    for rules in [&fresh, &older] {
        let buildings: Vec<_> = rules
            .units
            .iter()
            .filter(|u| u.structure)
            .map(|u| u.id)
            .collect();
        for source in [2, 3, 16, 17] {
            let worker = rules.units.iter().find(|u| u.id == id(source)).unwrap();
            assert_eq!(worker.repairs, buildings);
            assert!(
                worker
                    .builds
                    .iter()
                    .all(|target| usize::from(target.0 - 1) % 2 == source % 2)
            );
        }
    }
}

fn supply_arena(rules: Rules, farm: bool, aircraft: bool) -> World {
    let mut spawns = vec![
        Spawn {
            unit_type: id(74),
            position: Position { x: 96, y: 96 },
            ..Default::default()
        },
        Spawn {
            unit_type: id(0),
            position: Position { x: 200, y: 96 },
            ..Default::default()
        },
    ];
    if farm {
        spawns.push(Spawn {
            unit_type: id(58),
            position: Position { x: 400, y: 96 },
            ..Default::default()
        });
    }
    if aircraft {
        spawns.push(Spawn {
            unit_type: id(40),
            position: Position { x: 256, y: 192 },
            ..Default::default()
        });
    }
    World::new(
        rules,
        Map {
            id: "warcraft2.supply-review".into(),
            width: 512,
            height: 512,
            players: 2,
            spawns,
            multiplayer: None,
            initial_explored: Default::default(),
            creation: Default::default(),
            ai: vec![],
            start_locations: vec![],
            resources: vec![],
            fog_of_war: false,
            mission: None,
            terrain: None,
        },
        42,
    )
    .unwrap()
}

#[test]
fn every_trainable_unit_waits_for_food_and_reserves_one_when_it_starts() {
    let mut rules = supply_rules();
    let trainable: std::collections::BTreeSet<_> = rules
        .units
        .iter()
        .flat_map(|u| &u.trains)
        .filter(|id| rules.units.iter().any(|u| u.id == **id && !u.structure))
        .copied()
        .collect();
    assert!(trainable.contains(&id(40)) && trainable.contains(&id(41)));
    assert!(trainable.contains(&id(14)) && trainable.contains(&id(15)));
    for unit in &mut rules.units {
        unit.prerequisites.clear();
        unit.cost.clear();
        unit.build_ticks = 20;
    }
    rules
        .units
        .iter_mut()
        .find(|u| u.id == id(74))
        .unwrap()
        .trains = trainable.iter().copied().collect();
    for &unit_type in &trainable {
        for farm in [false, true] {
            let mut world = supply_arena(rules.clone(), farm, false);
            assert_eq!(world.supply(PlayerId(0)), (1, if farm { 5 } else { 1 }));
            let outcomes = world
                .step(&[Command {
                    player: PlayerId(0),
                    tick: world.tick(),
                    sequence: 1,
                    order: Order::Train {
                        entity: EntityId(1),
                        unit_type,
                    },
                }])
                .unwrap();
            assert!(
                outcomes[0].rejection.is_none(),
                "{unit_type:?}: {outcomes:?}"
            );
            for _ in 0..4 {
                world.step(&[]).unwrap();
            }
            let job = &world.state().entities[0].production[0];
            assert_eq!(job.started, farm, "{unit_type:?}");
            assert_eq!(
                world.supply(PlayerId(0)),
                if farm { (2, 5) } else { (1, 1) }
            );
            assert_eq!(job.remaining < job.total, farm, "{unit_type:?}");
        }
    }
}

#[test]
fn an_older_save_retains_its_aircraft_and_uses_correct_food_after_upgrade() {
    use straterust_engine::session::{SavedGame, ServerSession};
    let rules = supply_rules();
    let mut older_rules = rules.clone();
    older_rules
        .units
        .iter_mut()
        .find(|u| u.id == id(40))
        .unwrap()
        .supply_used = 0;
    let older = supply_arena(older_rules, false, true);
    assert_eq!(older.supply(PlayerId(0)), (1, 1));
    let server = ServerSession::new(older, 42, vec![PlayerId(0)]).unwrap();
    let bytes = SavedGame::capture(&server).unwrap().encode().unwrap();
    let definitions = supply_arena(rules, false, true);
    let restored =
        ServerSession::restore_saved(&definitions, SavedGame::decode(&bytes).unwrap()).unwrap();
    assert_eq!(
        restored.world().state().entities,
        server.world().state().entities
    );
    assert_eq!(restored.world().supply(PlayerId(0)), (2, 1));
    assert_eq!(
        SavedGame::capture(&server).unwrap().encode().unwrap(),
        bytes
    );
}
