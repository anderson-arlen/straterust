use super::*;
use straterust_engine::{
    content::Package,
    map::{BUILDABLE, Terrain, WALKABLE, WATER},
};

#[test]
#[ignore = "requires STRATERUST_GAME_ROOT; actual five-player map, normal resources and timing"]
fn adaptive_ai_native_five_player_opening_keeps_growing() -> anyhow::Result<()> {
    let root = std::path::PathBuf::from(
        std::env::var_os("STRATERUST_GAME_ROOT").expect("native game directory"),
    );
    let mut catalog = Catalog::load(&root)?;
    upgrade_catalog(&mut catalog);
    let map = catalog
        .maps
        .iter()
        .position(|m| m.package.contains("89e860b69c296e0a"))
        .expect("Crush all who oppose you!");
    for difficulty in [AiDifficulty::Normal, AiDifficulty::Hard] {
        let mut settings = catalog.defaults(map);
        settings.ai_difficulty = difficulty;
        for (slot, entry) in settings.slots.iter_mut().enumerate() {
            entry.faction = Some(slot % 2);
        }
        // Vacant Open seats must run AI, including the seat a guest joins later.
        let mut world = catalog.prepare(&root, map, &settings, &[3], 42)?.world;
        let mut workers = [false; 5];
        let mut military = [false; 5];
        let mut attacked = [false; 5];
        for _ in 0..72000 {
            world.step(&[])?;
            if world.tick().0.is_multiple_of(3000) {
                println!(
                    "{difficulty:?} tick={} entities={}",
                    world.tick().0,
                    world.state().entities.len()
                );
            }
            for (controller, state) in world.map().ai.iter().zip(&world.state().ai) {
                let player = usize::from(controller.player.0);
                if player == 3 {
                    continue;
                }
                let owned: Vec<_> = world
                    .state()
                    .entities
                    .iter()
                    .filter(|e| e.owner == controller.player)
                    .collect();
                workers[player] |= owned
                    .iter()
                    .filter(|e| {
                        world.unit_type(e.unit_type).is_some_and(|u| {
                            u.worker.is_some() && u.movement_class == MovementClass::Ground
                        })
                    })
                    .count()
                    >= 5;
                military[player] |= owned.iter().any(|e| {
                    world
                        .unit_type(e.unit_type)
                        .is_some_and(|u| !u.structure && u.worker.is_none() && u.weapon.is_some())
                });
                attacked[player] |= !state.deployed.is_empty();
            }
            if (0..5)
                .filter(|p| *p != 3)
                .all(|p| workers[p] && military[p] && attacked[p])
            {
                break;
            }
        }
        println!(
            "{difficulty:?} tick={} workers={workers:?} military={military:?} attacks={attacked:?}",
            world.tick().0
        );
        std::fs::write(
            "/tmp/stratarust-ai-native-world.ron",
            ron::to_string(&world.save_snapshot()?)?,
        )?;
        for controller in &world.map().ai {
            if controller.player.0 == 3 {
                continue;
            }
            let owned: Vec<_> = world
                .state()
                .entities
                .iter()
                .filter(|e| e.owner == controller.player)
                .map(|e| (e.unit_type, e.position, &e.order))
                .collect();
            println!("{:?}: {owned:?}", controller.player);
        }
        assert!(
            (0..5)
                .filter(|p| *p != 3)
                .all(|p| workers[p] && military[p] && attacked[p]),
            "{difficulty:?} stalled: AI={:?}; economy={:?}",
            world.state().ai,
            world.state().players
        );
    }
    Ok(())
}

#[test]
#[ignore = "requires STRATERUST_GAME_ROOT; paid native land/island production and attacks"]
fn adaptive_ai_native_land_and_island_forces_use_air_and_sea() -> anyhow::Result<()> {
    let root = std::path::PathBuf::from(
        std::env::var_os("STRATERUST_GAME_ROOT").expect("native game directory"),
    );
    let mut catalog = Catalog::load(&root)?;
    upgrade_catalog(&mut catalog);
    let migrated = ron::to_string(&catalog)?;
    upgrade_catalog(&mut catalog);
    assert_eq!(ron::to_string(&catalog)?, migrated);
    let mut custom = catalog.clone();
    custom.factions[0].ai = vec![AiInstruction::Stop];
    custom.factions[0].ai_difficulties.clear();
    upgrade_catalog(&mut custom);
    assert_eq!(custom.factions[0].ai, vec![AiInstruction::Stop]);
    let first = Package::load(&root.join(&catalog.maps[0].package))?.world(42)?;
    for (race, island, difficulty) in [
        (0, false, AiDifficulty::Normal),
        (1, true, AiDifficulty::Hard),
    ] {
        let mut rules = first.rules().clone();
        rules.victory = false;
        rules.starting_resources = [
            ("gold", 20000),
            ("wood", 20000),
            ("oil", if island { 0 } else { 1000 }),
        ]
        .into_iter()
        .map(|(kind, amount)| ResourceAmount {
            kind: kind.into(),
            amount,
        })
        .collect();
        for unit in &mut rules.units {
            unit.build_ticks = (unit.build_ticks / 20).max(1);
            if let Some(worker) = &mut unit.worker {
                worker.harvest_ticks = (worker.harvest_ticks / 20).max(1);
            }
            for harvest in &mut unit.harvest_profiles {
                harvest.ticks = (harvest.ticks / 20).max(1);
                harvest.depot_ticks = (harvest.depot_ticks / 20).max(1);
            }
        }
        rules
            .units
            .iter_mut()
            .find(|u| u.id == stats::id(74))
            .unwrap()
            .max_hp = 500000;
        let home = Position { x: 1696, y: 640 };
        let mut map = first.map().clone();
        map.width = 2048;
        map.height = 1280;
        map.players = 2;
        map.mission = None;
        map.multiplayer = None;
        map.creation.clear();
        map.initial_explored.clear();
        map.fog_of_war = false;
        map.spawns = vec![
            Spawn {
                owner: PlayerId(0),
                unit_type: stats::id(74),
                position: Position { x: 352, y: 640 },
                ..Default::default()
            },
            Spawn {
                owner: PlayerId(1),
                unit_type: stats::id(2 + race),
                position: home,
                ..Default::default()
            },
        ];
        map.start_locations = vec![
            StartLocation {
                player: PlayerId(0),
                position: Position { x: 352, y: 640 },
            },
            StartLocation {
                player: PlayerId(1),
                position: home,
            },
        ];
        let mut flags = vec![WALKABLE | BUILDABLE; 64 * 40];
        if island {
            for y in 0..40 {
                for x in 24..40 {
                    flags[y * 64 + x] = WATER;
                }
            }
        }
        map.terrain = Some(Terrain {
            cell_size: 32,
            columns: 64,
            rows: 40,
            flags,
        });
        map.resources = vec![ResourceSpawn {
            kind: "gold".into(),
            position: Position { x: 1856, y: 800 },
            amount: 1000000,
            footprint: Footprint {
                width: 96,
                height: 96,
            },
            requires_extractor: false,
            terrain_corners: None,
        }];
        map.resources.extend((0..8).map(|n| ResourceSpawn {
            kind: "wood".into(),
            position: Position {
                x: 1856 + (n % 2) * 32,
                y: 352 + (n / 2) * 32,
            },
            amount: 100000,
            footprint: Footprint {
                width: 32,
                height: 32,
            },
            requires_extractor: false,
            terrain_corners: None,
        }));
        if island {
            map.resources.push(ResourceSpawn {
                kind: "oil".into(),
                position: Position { x: 1088, y: 288 },
                amount: 100000,
                footprint: Footprint {
                    width: 96,
                    height: 96,
                },
                requires_extractor: true,
                terrain_corners: None,
            });
        }
        map.ai = vec![AiController {
            player: PlayerId(1),
            home,
            radius: 8192,
            active: true,
            takeover: true,
            program: catalog.factions[race].ai_difficulties[&difficulty].clone(),
            research: catalog.factions[race].ai_research.clone(),
            abilities: Vec::new(),
            harvest_weights: Vec::new(),
        }];
        let mut world = World::new(rules, map, 42)?;
        let mut air = false;
        let mut sea = false;
        let mut landed = false;
        for _ in 0..24000 {
            world.step(&[])?;
            let ai = &world.state().ai[0];
            for entity in world
                .state()
                .entities
                .iter()
                .filter(|e| e.owner == PlayerId(1))
            {
                air |= entity.unit_type == stats::id(42 + race) && ai.deployed.contains(&entity.id);
                sea |= entity.unit_type == stats::id(30 + race) && ai.deployed.contains(&entity.id);
                landed |= island && entity.unit_type == stats::id(race) && entity.position.x < 768;
            }
            if air && (!island || (sea && landed)) {
                break;
            }
        }
        println!(
            "race={race} island={island} tick={} air={air} sea={sea} landed={landed}",
            world.tick().0
        );
        assert!(
            air && (!island || (sea && landed)),
            "AI stalled: {:?}; units={:?}; resources={:?}",
            world.state().ai,
            world
                .state()
                .entities
                .iter()
                .filter(|e| e.owner == PlayerId(1))
                .map(|e| (e.unit_type, e.position, &e.order, e.production.len()))
                .collect::<Vec<_>>(),
            world.state().players[1]
        );
        if island {
            assert!(
                world.state().statistics[1]
                    .resources_collected
                    .get("oil")
                    .copied()
                    .unwrap_or(0)
                    > 0
            );
        } else {
            assert!(
                !world
                    .state()
                    .entities
                    .iter()
                    .any(|e| e.owner == PlayerId(1) && e.unit_type == stats::id(72 + race)),
                "land opening needlessly built a shipyard"
            );
        }
    }
    Ok(())
}
