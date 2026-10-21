use std::{
    collections::BTreeMap,
    net::TcpListener,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use straterust_engine::{
    content::Package,
    multiplayer::*,
    net::{self, lobby},
    sim::*,
};
#[path = "multiplayer/ai.rs"]
mod ai;
#[path = "multiplayer/chat.rs"]
mod chat;
#[path = "multiplayer/colors.rs"]
mod colors;
#[path = "multiplayer/delivery.rs"]
mod delivery;
#[path = "multiplayer/recovery.rs"]
mod recovery;
#[path = "multiplayer/seats.rs"]
mod seats;
#[path = "multiplayer/teams.rs"]
mod teams;
#[path = "multiplayer/terrain.rs"]
mod terrain;

fn write<T: serde::Serialize>(root: &Path, file: &str, value: &T) {
    std::fs::write(root.join(file), ron::ser::to_string(value).unwrap()).unwrap();
}
#[test]
fn browser_separates_layout_changes_from_settings_and_ignores_package_names() {
    use straterust_engine::{content::read_ron, multiplayer::browser};
    let (root, catalog) = fixture();
    let directory = root.path().join("map");
    let entry = &catalog.maps[0];
    let original = browser::inspect(&directory, entry, &catalog).unwrap();
    let mut map: Map = read_ron(&directory.join("map.ron")).unwrap();
    let mut setup: MapSetup = read_ron(&directory.join("match.ron")).unwrap();
    map.id = "another.file.copy".into();
    setup.variants = vec!["another/terrain".into()];
    write(&directory, "map.ron", &map);
    write(&directory, "match.ron", &setup);
    let copy = browser::inspect(&directory, entry, &catalog).unwrap();
    assert_eq!(original.layout, copy.layout);
    assert_eq!(original.settings, copy.settings);
    setup.resources[0][0].amount = 999;
    write(&directory, "match.ron", &setup);
    let resources = browser::inspect(&directory, entry, &catalog).unwrap();
    assert_eq!(resources.layout, original.layout);
    assert_ne!(resources.settings, original.settings);
    assert!(
        resources
            .fields
            .iter()
            .any(|(k, v)| k == "Starting resources" && v.contains("999"))
    );
    map.spawns[0].unit_type = UnitTypeId(1);
    write(&directory, "map.ron", &map);
    let army = browser::inspect(&directory, entry, &catalog).unwrap();
    assert_eq!(army.layout, resources.layout);
    assert_ne!(army.settings, resources.settings);
    assert_ne!(army.army, resources.army);
    map.resources.push(ResourceSpawn {
        kind: "gold".into(),
        position: Position { x: 64, y: 64 },
        amount: 10000,
        footprint: Default::default(),
        terrain_corners: None,
        requires_extractor: false,
    });
    write(&directory, "map.ron", &map);
    let location = browser::inspect(&directory, entry, &catalog).unwrap();
    assert_eq!(location.layout, army.layout);
    assert_ne!(location.settings, army.settings);
    assert_ne!(location.resources, army.resources);
    map.resources[0].amount /= 2;
    write(&directory, "map.ron", &map);
    let deposit = browser::inspect(&directory, entry, &catalog).unwrap();
    assert_eq!(location.layout, deposit.layout);
    assert_ne!(location.settings, deposit.settings);
    // Without native geometry, art is the available layout comparison.
    let grid = straterust_engine::assets::TerrainGrid {
        tile_size: 32,
        columns: 36,
        rows: 32,
        tiles: vec![0; 36 * 32],
    };
    // The native manifest is a RON struct, not a dictionary.
    std::fs::write(
        directory.join("assets.ron"),
        format!(
            "(terrain:(file:\"terrain.srim\",blake3:\"unneeded\"),terrain_grid:Some({}))",
            ron::ser::to_string(&grid).unwrap()
        ),
    )
    .unwrap();
    let first = browser::inspect(&directory, entry, &catalog).unwrap();
    let mut grid = grid;
    grid.tiles[0] = 1;
    std::fs::write(
        directory.join("assets.ron"),
        format!(
            "(terrain:(file:\"terrain.srim\",blake3:\"unneeded\"),terrain_grid:Some({}))",
            ron::ser::to_string(&grid).unwrap()
        ),
    )
    .unwrap();
    let second = browser::inspect(&directory, entry, &catalog).unwrap();
    assert_ne!(first.layout, second.layout);
    // Once gameplay geometry exists, cosmetic edits are selectable versions.
    map.terrain = Some(straterust_engine::map::Terrain {
        cell_size: 32,
        columns: 36,
        rows: 32,
        flags: vec![straterust_engine::map::WALKABLE | straterust_engine::map::BUILDABLE; 36 * 32],
    });
    std::fs::write(
        directory.join("terrain.srtm"),
        straterust_engine::map::encode_terrain(map.terrain.as_ref().unwrap()).unwrap(),
    )
    .unwrap();
    let first = browser::inspect(&directory, entry, &catalog).unwrap();
    grid.tiles[0] = 2;
    std::fs::write(
        directory.join("assets.ron"),
        format!(
            "(terrain:(file:\"terrain.srim\",blake3:\"unneeded\"),terrain_grid:Some({}))",
            ron::ser::to_string(&grid).unwrap()
        ),
    )
    .unwrap();
    let second = browser::inspect(&directory, entry, &catalog).unwrap();
    assert_eq!(first.layout, second.layout);
    assert_ne!(first.settings, second.settings);
    assert_ne!(first.terrain, second.terrain);
    map.terrain.as_mut().unwrap().flags[0] = 0;
    std::fs::write(
        directory.join("terrain.srtm"),
        straterust_engine::map::encode_terrain(map.terrain.as_ref().unwrap()).unwrap(),
    )
    .unwrap();
    let blocked = browser::inspect(&directory, entry, &catalog).unwrap();
    assert_ne!(second.layout, blocked.layout);
    let mut mission = Mission {
        schema_version: 1,
        player: PlayerId(0),
        rescuable_players: Vec::new(),
        rescuers: Vec::new(),
        rescue_margin: None,
        alliances: Vec::new(),
        poll_ticks: 1,
        wait_step_ms: 42,
        locations: Vec::new(),
        triggers: vec![MissionTrigger {
            conditions: Vec::new(),
            actions: vec![MissionAction::Victory],
        }],
    };
    write(&directory, "mission.ron", &mission);
    let victory = browser::inspect(&directory, entry, &catalog).unwrap();
    mission.triggers[0].actions = vec![MissionAction::Defeat];
    write(&directory, "mission.ron", &mission);
    let defeat = browser::inspect(&directory, entry, &catalog).unwrap();
    assert_eq!(victory.layout, defeat.layout);
    assert_ne!(
        victory.settings, defeat.settings,
        "scenario scripts must not collapse"
    );
    setup.scenario_error = Some("unsupported statistics".into());
    write(&directory, "match.ron", &setup);
    let first = browser::inspect(&directory, entry, &catalog).unwrap();
    let mut entry = entry.clone();
    entry.package = "copy".into();
    let second = browser::inspect(&directory, &entry, &catalog).unwrap();
    assert_ne!(
        first.settings, second.settings,
        "retain unconverted source variants"
    );
}

#[test]
fn map_preview_preserves_rectangle_aspect_and_authored_tiles() {
    use straterust_engine::assets::*;
    let (root, _) = fixture();
    let mut map: Map =
        straterust_engine::content::read_ron(&root.path().join("map/map.ron")).unwrap();
    map.width = 64;
    map.height = 32;
    let image = Image {
        width: 64,
        height: 32,
        rgba: (0..64 * 32)
            .flat_map(|p| {
                if p % 64 < 32 {
                    [240, 0, 0, 255]
                } else {
                    [0, 0, 240, 255]
                }
            })
            .collect(),
    };
    let art = MapArtwork {
        terrain: encode_image(&image).unwrap(),
        grid: Some(TerrainGrid {
            tile_size: 32,
            columns: 2,
            rows: 1,
            tiles: vec![1, 0],
        }),
        images: Vec::new(),
        decorations: Vec::new(),
        player_colors: BTreeMap::new(),
    };
    let preview = art.thumbnail(&map).unwrap();
    assert_eq!([preview.width, preview.height], [256, 128]);
    assert_eq!(&preview.rgba[..4], &[0, 0, 240, 255]);
    assert_eq!(&preview.rgba[255 * 4..256 * 4], &[240, 0, 0, 255]);
    map.width = 0;
    assert!(art.thumbnail(&map).is_err());
}

fn fixture() -> (tempfile::TempDir, Catalog) {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("map");
    std::fs::create_dir(&directory).unwrap();
    let sample =
        Package::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/lan-demo"))
            .unwrap()
            .world(42)
            .unwrap();
    let mut rules = sample.rules().clone();
    rules
        .units
        .iter_mut()
        .find(|u| u.id == UnitTypeId(4))
        .unwrap()
        .blocks_movement = false;
    rules.units.push(UnitType {
        id: UnitTypeId(6),
        portable: true,
        neutral: true,
        blocks_movement: false,
        max_hp: 1,
        ..Default::default()
    });
    let locations: Vec<_> = (0..8)
        .map(|i| StartLocation {
            player: PlayerId(i),
            position: Position {
                x: 160 + i32::from(i % 4) * 256,
                y: 160 + i32::from(i / 4) * 512,
            },
        })
        .collect();
    let map = Map {
        multiplayer: None,
        id: "test.multiplayer".into(),
        width: 1152,
        height: 1024,
        players: 16,
        terrain: None,
        spawns: locations
            .iter()
            .map(|s| Spawn {
                owner: s.player,
                unit_type: UnitTypeId(2),
                position: s.position,
                ..Default::default()
            })
            .collect(),
        start_locations: locations,
        resources: Vec::new(),
        fog_of_war: true,
        mission: None,
        initial_explored: BTreeMap::new(),
        creation: BTreeMap::new(),
        ai: Vec::new(),
    };
    std::fs::write(
        directory.join("manifest.ron"),
        "(schema_version:1,id:\"test.multiplayer\")",
    )
    .unwrap();
    write(&directory, "rules.ron", &rules);
    write(&directory, "map.ron", &map);
    write(
        &directory,
        "match.ron",
        &MapSetup {
            terrain_grid: None,
            scenario_spawns: None,
            selectable_factions: Vec::new(),
            factions: vec![0; 8],
            resources: vec![
                vec![ResourceAmount {
                    kind: "minerals".into(),
                    amount: 50
                }];
                8
            ],
            variants: Vec::new(),
            scenario_error: None,
        },
    );
    let factions = (0..2)
        .map(|i| Faction {
            ai_difficulties: Default::default(),
            ai_research: Vec::new(),
            name: format!("Race {i}"),
            presentation: "map".into(),
            tileset_presentations: Vec::new(),
            worker: UnitTypeId(if i == 0 { 2 } else { 1 }),
            starting_units: vec![
                (UnitTypeId(3), Position { x: 0, y: 0 }),
                (
                    UnitTypeId(if i == 0 { 2 } else { 1 }),
                    Position { x: 0, y: 80 },
                ),
            ],
            equivalents: BTreeMap::from([(UnitTypeId(2), UnitTypeId(if i == 0 { 2 } else { 1 }))]),
            ai: vec![AiInstruction::Wait(1000), AiInstruction::Jump(0)],
        })
        .collect();
    let catalog = Catalog {
        colors: Vec::new(),
        schema_version: 1,
        game: "fixture".into(),
        maps: vec![MapEntry {
            title: "Eight players".into(),
            package: "map".into(),
            players: 8,
            size: [36, 32],
            tileset: 0,
        }],
        factions,
        resources: vec![ResourcePreset {
            name: "High".into(),
            amounts: vec![ResourceAmount {
                kind: "minerals".into(),
                amount: 10000,
            }],
        }],
        speeds: vec![Speed {
            name: "Fastest".into(),
            tick_ms: 50,
        }],
        modes: vec![
            GameType::Melee,
            GameType::OneOnOne,
            GameType::TopVsBottom,
            GameType::TeamMelee,
            GameType::Greed,
            GameType::Slaughter,
            GameType::SuddenDeath,
            GameType::CaptureTheFlag,
        ],
        tilesets: Vec::new(),
        terrain_mappings: Vec::new(),
        allow_cheats: false,
        random_faction: true,
        standard_start: false,
        defeat_without_structures: false,
        flag_art: Some(FlagArt {
            item: UnitTypeId(6),
            homes: vec![UnitTypeId(4); 2],
        }),
    };
    write(root.path(), "multiplayer.ron", &catalog);
    (root, catalog)
}

#[test]
fn setup_changes_race_resources_starts_fog_units_and_computer_slots_deterministically() {
    let (root, catalog) = fixture();
    let mut settings = catalog.defaults(0);
    settings.resources = Some(0);
    settings.random_locations = false;
    settings.fog = false;
    settings.map_units = false;
    settings.slots[0].faction = Some(1);
    settings.slots[1].kind = SlotKind::Computer;
    for s in &mut settings.slots[2..] {
        s.kind = SlotKind::Closed;
    }
    let first = catalog
        .prepare(root.path(), 0, &settings, &[0], 42)
        .unwrap();
    let second = catalog
        .prepare(root.path(), 0, &settings, &[0], 42)
        .unwrap();
    assert_eq!(first.world.state_hash(), second.world.state_hash());
    assert_eq!(first.world.state().entities.len(), 2);
    assert!(!first.world.map().fog_of_war);
    assert_eq!(first.world.state().entities[0].unit_type, UnitTypeId(1));
    assert_eq!(
        first.world.state().entities[0].position,
        Position { x: 160, y: 160 }
    );
    assert_eq!(first.world.state().players[0].resources["minerals"], 10000);
    assert!(
        first
            .world
            .map()
            .ai
            .iter()
            .any(|a| a.player == PlayerId(1) && a.active && a.takeover)
    );
    assert!(
        first
            .world
            .map()
            .ai
            .iter()
            .any(|a| a.player == PlayerId(0) && !a.active && a.takeover)
    );
    settings.random_locations = true;
    let random = catalog
        .prepare(root.path(), 0, &settings, &[0], 42)
        .unwrap();
    assert_ne!(
        random
            .world
            .map()
            .start_locations
            .iter()
            .map(|s| s.position)
            .collect::<Vec<_>>(),
        first
            .world
            .map()
            .start_locations
            .iter()
            .map(|s| s.position)
            .collect::<Vec<_>>()
    );
    settings.mode = 1;
    settings.slots[2].kind = SlotKind::Computer;
    assert!(
        catalog
            .prepare(root.path(), 0, &settings, &[0], 42)
            .is_err()
    );
}
#[test]
fn catalog_rejects_escaping_paths_invalid_settings_and_unsupported_data() {
    let (_, mut catalog) = fixture();
    catalog.validate().unwrap();
    let mut settings = catalog.defaults(0);
    settings.slots[0].faction = Some(8);
    assert!(catalog.validate_settings(0, &settings).is_err());
    catalog.maps[0].package = "../other-game".into();
    assert!(catalog.validate().is_err());
    catalog.maps[0].package = "map".into();
    catalog.maps[0].players = 9;
    assert!(catalog.validate().is_err());
}
#[test]
fn greed_ends_with_real_results_for_computer_players_too() {
    let (root, catalog) = fixture();
    let mut settings = catalog.defaults(0);
    for slot in &mut settings.slots[2..] {
        slot.kind = SlotKind::Closed;
    }
    settings.mode = 4;
    settings.parameter = 50;
    settings.slots[1].kind = SlotKind::Computer;
    let mut world = catalog
        .prepare(root.path(), 0, &settings, &[0], 42)
        .unwrap()
        .world;
    world.step(&[]).unwrap();
    assert!(world.state().winner.is_some());
    let server =
        straterust_engine::session::ServerSession::new(world, 42, vec![PlayerId(0)]).unwrap();
    assert_eq!(server.match_result().unwrap().players.len(), 2);
}

#[test]
fn scenario_armies_and_unsupported_settings_do_not_change_ordinary_starts() {
    let (root, mut catalog) = fixture();
    catalog.modes.push(GameType::UseMapSettings);
    let mut setup: MapSetup =
        straterust_engine::content::read_ron(&root.path().join("map/match.ron")).unwrap();
    let scenario = vec![
        Spawn {
            owner: PlayerId(0),
            unit_type: UnitTypeId(1),
            position: Position { x: 220, y: 220 },
            ..Default::default()
        },
        Spawn {
            owner: PlayerId(1),
            unit_type: UnitTypeId(1),
            position: Position { x: 480, y: 220 },
            ..Default::default()
        },
    ];
    setup.scenario_spawns = Some(scenario.clone());
    write(&root.path().join("map"), "match.ron", &setup);
    let mut settings = catalog.defaults(0);
    for slot in &mut settings.slots[2..] {
        slot.kind = SlotKind::Closed;
    }
    settings.mode = catalog.modes.len() - 1;
    for slot in &mut settings.slots {
        slot.faction = Some(0);
    }
    let world = catalog
        .prepare(root.path(), 0, &settings, &[0, 1], 42)
        .unwrap()
        .world;
    assert_eq!(world.state().entities.len(), 2);
    assert!(
        world
            .state()
            .entities
            .iter()
            .zip(&scenario)
            .all(|(e, s)| e.position == s.position && e.unit_type == s.unit_type)
    );
    setup.scenario_error = Some("unsupported custom statistics".into());
    write(&root.path().join("map"), "match.ron", &setup);
    assert!(
        catalog
            .prepare(root.path(), 0, &settings, &[0, 1], 42)
            .is_err()
    );
    settings.mode = 0;
    let world = catalog
        .prepare(root.path(), 0, &settings, &[0, 1], 42)
        .unwrap()
        .world;
    assert!(
        world
            .state()
            .entities
            .iter()
            .all(|e| e.unit_type == UnitTypeId(2))
    );
}

#[test]
fn generated_starting_workers_find_clear_positions_without_changing_resources() {
    let (root, mut catalog) = fixture();
    catalog.standard_start = true;
    let file = root.path().join("map/map.ron");
    let mut map: Map = straterust_engine::content::read_ron(&file).unwrap();
    let blocked = Position { x: 160, y: 240 };
    map.resources.push(ResourceSpawn {
        kind: "minerals".into(),
        position: blocked,
        amount: 500,
        footprint: Footprint {
            width: 32,
            height: 32,
        },
        requires_extractor: false,
        terrain_corners: None,
    });
    write(&root.path().join("map"), "map.ron", &map);
    let mut settings = catalog.defaults(0);
    settings.random_locations = false;
    for slot in &mut settings.slots {
        slot.faction = Some(0);
    }
    let world = catalog
        .prepare(root.path(), 0, &settings, &[0, 1], 42)
        .unwrap()
        .world;
    let worker = world
        .state()
        .entities
        .iter()
        .find(|e| e.owner == PlayerId(0) && e.unit_type == UnitTypeId(2))
        .unwrap();
    assert_ne!(worker.position, blocked);
    assert!(
        (worker.position.x - blocked.x).abs() <= 64 && (worker.position.y - blocked.y).abs() <= 64
    );
    assert_eq!(world.map().resources[0].position, blocked);
    assert_eq!(world.map().resources[0].amount, 500);
}

#[test]
fn elimination_policy_can_require_structures_while_preserving_armies() {
    let (root, mut catalog) = fixture();
    catalog.defeat_without_structures = true;
    let settings = catalog.defaults(0);
    let prepared = catalog
        .prepare(root.path(), 0, &settings, &[0, 1], 42)
        .unwrap();
    let mut map = prepared.world.map().clone();
    map.spawns[0].unit_type = UnitTypeId(3);
    let mut world = World::new(prepared.world.rules().clone(), map.clone(), 42).unwrap();
    world.step(&[]).unwrap();
    assert_eq!(world.state().winner, Some(PlayerId(0)));
    assert!(
        world
            .state()
            .entities
            .iter()
            .any(|e| e.owner == PlayerId(1))
    );
    map.multiplayer.as_mut().unwrap().defeat_without_structures = false;
    let mut world = World::new(prepared.world.rules().clone(), map, 42).unwrap();
    world.step(&[]).unwrap();
    assert_eq!(world.state().winner, None);
}

#[test]
fn slaughter_counts_authoritative_kills_and_sudden_death_requires_a_base() {
    let (root, catalog) = fixture();
    let mut settings = catalog.defaults(0);
    settings.mode = 5;
    settings.parameter = 1;
    let prepared = catalog
        .prepare(root.path(), 0, &settings, &[0, 1], 42)
        .unwrap();
    let mut rules = prepared.world.rules().clone();
    rules.tick_ms = 1000;
    let mut map = prepared.world.map().clone();
    map.spawns[0].unit_type = UnitTypeId(1);
    map.spawns[0].position = Position { x: 200, y: 200 };
    map.spawns[1].position = Position { x: 240, y: 200 };
    map.spawns[1].hp_percent = Some(1);
    map.spawns.push(Spawn {
        owner: PlayerId(1),
        unit_type: UnitTypeId(2),
        position: Position { x: 1000, y: 800 },
        ..Default::default()
    });
    let mut world = World::new(rules, map, 42).unwrap();
    world
        .step(&[Command {
            tick: Tick(0),
            player: PlayerId(0),
            sequence: 1,
            order: Order::Attack {
                entity: EntityId(1),
                target: EntityId(2),
            },
        }])
        .unwrap();
    for _ in 0..60 {
        world.step(&[]).unwrap();
    }
    assert_eq!(world.state().winner, Some(PlayerId(0)));
    assert!(
        world
            .state()
            .entities
            .iter()
            .any(|e| e.owner == PlayerId(1))
    );

    settings.mode = 6;
    let prepared = catalog
        .prepare(root.path(), 0, &settings, &[0, 1], 42)
        .unwrap();
    let mut map = prepared.world.map().clone();
    map.spawns[0].unit_type = UnitTypeId(3);
    let mut world = World::new(prepared.world.rules().clone(), map, 42).unwrap();
    world.step(&[]).unwrap();
    assert_eq!(world.state().winner, Some(PlayerId(0)));
}

#[test]
fn capture_the_flag_requires_an_enemy_flag_delivered_home() {
    let (root, catalog) = fixture();
    let mut settings = catalog.defaults(0);
    settings.mode = 7;
    settings.random_locations = false;
    for s in &mut settings.slots {
        s.faction = Some(0);
    }
    let prepared = catalog
        .prepare(root.path(), 0, &settings, &[0, 1], 42)
        .unwrap();
    let mut map = prepared.world.map().clone();
    let flags = map.multiplayer.as_ref().unwrap().flags.clone();
    map.spawns[0].position = flags[1].position;
    let mut world = World::new(prepared.world.rules().clone(), map, 42).unwrap();
    world.step(&[]).unwrap();
    assert_eq!(world.state().winner, None);
    assert_eq!(
        world
            .state()
            .entities
            .iter()
            .find(|e| e.id == flags[1].item)
            .unwrap()
            .carried_by,
        Some(EntityId(1))
    );
    world
        .step(&[Command {
            tick: world.tick(),
            player: PlayerId(0),
            sequence: 1,
            order: Order::Move {
                entity: EntityId(1),
                target: flags[0].position,
            },
        }])
        .unwrap();
    for _ in 0..100 {
        world.step(&[]).unwrap();
    }
    assert_eq!(world.state().winner, Some(PlayerId(0)));
}

fn poll_until(
    client: &mut lobby::Client,
    predicate: impl Fn(&lobby::Response) -> bool,
) -> lobby::Response {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(response) = client.poll().unwrap()
            && predicate(&response)
        {
            return response;
        }
        assert!(Instant::now() < deadline, "lobby response timed out");
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
#[ignore = "requires STRATERUST_GAMES containing refreshed original game imports"]
fn original_multiplayer_maps_prepare_each_race_and_terrain() -> anyhow::Result<()> {
    use anyhow::Context;
    let root = std::path::PathBuf::from(
        std::env::var_os("STRATERUST_GAMES").context("set games directory")?,
    );
    let selected = std::env::var("STRATERUST_GAME").ok();
    for name in ["StarCraft", "Warcraft II"] {
        if selected.as_deref().is_some_and(|game| game != name) {
            continue;
        }
        let directory = root.join(name);
        let catalog = Catalog::load(&directory)?;
        println!("{name}: {} maps", catalog.maps.len());
        for (i, entry) in catalog.maps.iter().enumerate() {
            println!("Checking {}", entry.title);
            for faction in 0..catalog.factions.len() {
                for era in 0..catalog.tilesets.len().max(1) {
                    let mut settings = catalog.defaults(i);
                    settings.random_locations = false;
                    settings.tileset = (!catalog.tilesets.is_empty()).then_some(era);
                    for slot in &mut settings.slots {
                        slot.faction = Some(faction);
                    }
                    let occupied = (0..usize::from(entry.players)).collect::<Vec<_>>();
                    let prepared = catalog
                        .prepare(&directory, i, &settings, &occupied, 42)
                        .with_context(|| format!("{} race {faction} terrain {era}", entry.title))?;
                    let transfer = prepared
                        .map_transfer()
                        .with_context(|| format!("{} race {faction}: transfer", entry.title))?;
                    if faction == 0 && era == 0 {
                        anyhow::ensure!(
                            ron::ser::to_string(&transfer)?.len() <= net::MAX_MAP_BYTES,
                            "map download exceeds limit"
                        );
                    }
                }
            }
            let setup: MapSetup = straterust_engine::content::read_ron(
                &directory.join(&entry.package).join("match.ron"),
            )?;
            if setup.scenario_error.is_none()
                && let Some(mode) = catalog
                    .modes
                    .iter()
                    .position(|m| *m == GameType::UseMapSettings)
            {
                let mut settings = catalog.defaults(i);
                settings.mode = mode;
                let occupied = (0..usize::from(entry.players)).collect::<Vec<_>>();
                let mut prepared = catalog
                    .prepare(&directory, i, &settings, &occupied, 42)
                    .with_context(|| format!("{}: Use Map Settings", entry.title))?;
                prepared.world.step(&[])?;
            }
        }
        for faction in &catalog.factions {
            let paths = if faction.tileset_presentations.is_empty() {
                vec![&faction.presentation]
            } else {
                faction.tileset_presentations.iter().collect()
            };
            for path in paths {
                straterust_engine::assets::AssetPack::load(&package_path(&directory, path)?)?;
                straterust_engine::media::MediaPack::load(&package_path(&directory, path)?)?;
            }
        }
    }
    Ok(())
}
#[test]
fn three_player_lobby_requires_readiness_and_transfers_configured_match() {
    lobby_roundtrip(3, false);
}
#[test]
fn eight_players_and_shared_team_connections_receive_matching_updates() {
    lobby_roundtrip(8, true);
}
fn lobby_roundtrip(count: usize, shared: bool) {
    let (root, catalog) = fixture();
    let definitions = Package::client_definitions(&root.path().join("map")).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let stopped = stop.clone();
    let directory = root.path().to_owned();
    let mut settings = catalog.defaults(0);
    for slot in &mut settings.slots[count..] {
        slot.kind = SlotKind::Closed;
    }
    if shared {
        settings.mode = 3;
    }
    let server = std::thread::spawn(move || {
        lobby::host(
            listener, directory, catalog, 0, settings, 42, stopped, None, None,
        )
    });
    let mut waiting: Vec<_> = (0..count)
        .map(|i| lobby::Client::connect(address, &definitions, i == 0).unwrap())
        .collect();
    poll_until(
        &mut waiting[0],
        |r| matches!(r,lobby::Response::State{view,..} if view.players.iter().flatten().count()==count),
    );
    waiting[0].send(lobby::Request::Start).unwrap();
    poll_until(&mut waiting[0], |r| {
        matches!(r, lobby::Response::Rejected(_))
    });
    waiting[1].send(lobby::Request::Faction(Some(1))).unwrap();
    for c in &mut waiting {
        c.send(lobby::Request::Ready(true)).unwrap();
    }
    poll_until(
        &mut waiting[0],
        |r| matches!(r,lobby::Response::State{view,..} if view.players.iter().flatten().all(|p|p.ready)),
    );
    waiting[0].send(lobby::Request::Start).unwrap();
    let mut assigned = Vec::new();
    for c in &mut waiting {
        poll_until(c, |r| matches!(r, lobby::Response::Starting));
        let response = poll_until(c, |r| matches!(r, lobby::Response::Launch { .. }));
        if let lobby::Response::Launch { player, .. } = response {
            assigned.push(player);
        }
    }
    drop(waiting);
    let mut clients = Vec::new();
    for (i, player) in assigned.into_iter().enumerate() {
        let (mut client, initial) =
            net::RemoteClient::connect(address, &definitions, player).unwrap();
        assert_eq!(initial.view.player, player);
        assert_eq!(
            client.handshake.participants.len(),
            if shared { 2 } else { count }
        );
        if shared {
            let unit = initial
                .view
                .entities
                .iter()
                .filter_map(|e| match e {
                    ViewedEntity::Owned(e) => Some(e),
                    _ => None,
                })
                .nth(i / 2)
                .unwrap();
            for sequence in 1..=2 {
                client
                    .command(Command {
                        tick: Tick(initial.view.tick.0 + 16 + sequence),
                        player,
                        sequence,
                        order: Order::Move {
                            entity: unit.id,
                            target: Position {
                                x: unit.position.x + 48,
                                y: unit.position.y + 48,
                            },
                        },
                    })
                    .unwrap();
            }
        }
        clients.push(client);
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut started = vec![false; count];
    while !started.iter().all(|p| *p) {
        for (i, c) in clients.iter_mut().enumerate() {
            if let Some(net::ServerMessage::Status { started: true, .. }) = c.poll().unwrap() {
                started[i] = true;
            }
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    if shared {
        let mut accepted = vec![0; count];
        let mut views = BTreeMap::new();
        while !accepted.iter().all(|n| *n >= 8) {
            for (i, client) in clients.iter_mut().enumerate() {
                if let Some(net::ServerMessage::Update(update)) = client.poll().unwrap() {
                    assert!(update.outcomes.iter().all(|o| o.rejection.is_none()));
                    accepted[i] += update.outcomes.len();
                    let key = (update.view.player, update.view.tick);
                    let value = ron::ser::to_string(&update.view).unwrap();
                    if let Some(previous) = views.insert(key, value.clone()) {
                        assert_eq!(previous, value);
                    }
                }
            }
            assert!(
                Instant::now() < deadline,
                "shared commands were not accepted"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    stop.store(true, Ordering::Release);
    drop(clients);
    let _ = server.join().unwrap();
}
