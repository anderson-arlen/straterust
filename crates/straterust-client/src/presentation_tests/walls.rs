use super::*;
use straterust_engine::sim::{Order, Spawn, Visibility};

#[test]
#[ignore = "requires STRATERUST_WARCRAFT2 pointing to the retail import"]
fn warcraft2_fog_keeps_original_walls_as_unselectable_remembered_bodies() {
    let root = PathBuf::from(std::env::var_os("STRATERUST_WARCRAFT2").unwrap());
    let mut app = App::load(
        &root.join("human/mission07"),
        Config {
            audio: false,
            zoom: 2.0,
            ..Default::default()
        },
        None,
    )
    .unwrap();
    app.mission_ui = None;
    app.simulation = None;
    let mut rules = app.world.rules().clone();
    rules.victory = false;
    let wall = rules.units.iter().find(|u| u.id.0 >= 1000).unwrap().id;
    let observer = UnitTypeId(41);
    let scout = rules.units.iter_mut().find(|u| u.id == observer).unwrap();
    scout.vision_range = 64;
    scout.speed = 16;
    let mut map = app.world.map().clone();
    map.width = 1024;
    map.height = 1024;
    map.terrain = None;
    map.mission = None;
    map.ai.clear();
    map.resources.clear();
    map.creation.clear();
    map.initial_explored.clear();
    map.start_locations.clear();
    map.fog_of_war = true;
    map.spawns = vec![
        Spawn {
            unit_type: observer,
            position: Position { x: 464, y: 512 },
            ..Default::default()
        },
        Spawn {
            owner: PlayerId(15),
            unit_type: wall,
            position: Position { x: 512, y: 512 },
            ..Default::default()
        },
    ];
    let mut server = World::new(rules, map, 7).unwrap();
    assert!(server.entity_visible(PlayerId(0), EntityId(2)));
    let outcome = server
        .step(&[straterust_engine::sim::Command {
            tick: server.tick(),
            player: PlayerId(0),
            sequence: 1,
            order: Order::Move {
                entity: EntityId(1),
                target: Position { x: 320, y: 512 },
            },
        }])
        .unwrap();
    assert!(outcome[0].rejection.is_none());
    for _ in 0..30 {
        server.step(&[]).unwrap();
    }
    assert_eq!(
        server.visibility(PlayerId(0), Position { x: 512, y: 512 }),
        Visibility::Explored
    );
    let packet = server.player_view(PlayerId(0)).unwrap();
    assert_eq!(packet.remembered_structures.len(), 1);
    app.world = packet.clone().into_world(&server).unwrap();
    assert!(!app.world.entity_visible(PlayerId(0), EntityId(2)));
    assert_eq!(app.world.state().entities.len(), 1);
    app.visuals = Visuals::new(&app.world);
    app.camera.x = 512.0;
    app.camera.y = 512.0;
    let size = [1280, 800];
    let remembered = capture_at(&app, "warcraft2-wall-remembered", size, None, None, 0);
    let later = capture_at(
        &app,
        "warcraft2-wall-remembered-later",
        size,
        None,
        None,
        2000,
    );
    let point = app
        .camera
        .world_to_screen(512.0, 512.0, size.map(f64::from));
    for y in (point[1] as usize - 32)..(point[1] as usize + 32) {
        for x in (point[0] as usize - 32)..(point[0] as usize + 32) {
            let pixel = y * size[0] as usize + x;
            assert_eq!(
                remembered[pixel], later[pixel],
                "remembered walls must remain frozen"
            );
        }
    }
    let mut empty = packet;
    empty.remembered_structures.clear();
    app.world = empty.into_world(&server).unwrap();
    let rubble = capture_at(&app, "warcraft2-wall-rubble-control", size, None, None, 0);
    let changed = remembered
        .iter()
        .zip(&rubble)
        .filter(|(a, b)| a != b)
        .count();
    assert!(
        changed > 100,
        "fog must show native wall pixels above its rubble terrain"
    );
}
