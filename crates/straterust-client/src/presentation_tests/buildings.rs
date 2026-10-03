//! A short source-asset scenario; it never plays through a campaign mission.
use super::*;
use straterust_engine::sim::{Order, ResourceAmount, Spawn};

#[test]
#[ignore = "requires private mission 5 assets; checks addons, cargo and native presentation only"]
fn campaign_buildings_transports_selection_and_scv_anchors() {
    let directory = PathBuf::from(std::env::var_os("STRATERUST_ASSET_PACKAGE").unwrap());
    let package = Package::load(&directory).unwrap();
    let mut app = App::new(
        &package,
        Config {
            audio: false,
            zoom: 2.0,
            ..Default::default()
        },
        read_ron(&directory.join("presentation.ron")).unwrap(),
        AssetPack::load(&directory).unwrap(),
        None,
    )
    .unwrap();
    app.load_media(&directory).unwrap();
    app.mission_ui = None;
    assert!(
        app.world.map().creation[&PlayerId(0)].contains(&UnitTypeId(53)),
        "source PUNI allows mission 5 Dropships"
    );
    let mut map = app.world.map().clone();
    map.mission = None;
    map.ai.clear();
    map.terrain = None;
    map.fog_of_war = false;
    map.initial_explored.clear();
    map.resources.clear();
    map.start_locations.clear();
    map.spawns = [
        (32, 512, 512, 100),
        (33, 768, 512, 100),
        (2, 640, 800, 100),
        (1, 512, 720, 25),
        (1, 544, 720, 70),
        (25, 600, 720, 100),
    ]
    .into_iter()
    .map(|(unit, x, y, hp)| Spawn {
        unit_type: UnitTypeId(unit),
        position: Position { x, y },
        hp_percent: Some(hp),
        ..Default::default()
    })
    .collect();
    let mut rules = app.world.rules().clone();
    rules.victory = false;
    rules.starting_resources = vec![
        ResourceAmount {
            kind: "minerals".into(),
            amount: 5000,
        },
        ResourceAmount {
            kind: "gas".into(),
            amount: 5000,
        },
    ];
    for unit in &mut rules.units {
        unit.build_ticks = 40;
        if unit.id == UnitTypeId(32) {
            unit.supply_provided = 50;
        }
    }
    app.world = World::new(rules, map.clone(), 42).unwrap();
    app.visuals = Visuals::new(&app.world);
    app.audio.reset(&app.world);
    app.camera.x = 700.0;
    app.camera.y = 600.0;
    for action in [VisualAction::Idle, VisualAction::Move, VisualAction::Work] {
        for facing in 0..32 {
            let mut observed = *app.visuals.get(EntityId(3)).unwrap();
            observed.facing = facing;
            observed.action = action;
            let frame = visual::unit_image(
                app.assets.as_ref().unwrap(),
                entity(&app, EntityId(3)),
                Some(&observed),
                &app.world,
            )
            .unwrap();
            assert_eq!(frame.anchor, [64, 64], "SCV {action:?} direction {facing}");
        }
    }
    app.selected = BTreeSet::from([EntityId(4), EntityId(5), EntityId(6)]);
    let assets = app.assets.as_ref().unwrap();
    let atlas = assets.ui_image("groupwire.damage.1").unwrap();
    let stride = (atlas.width * atlas.width * 4) as usize;
    assert_ne!(&atlas.rgba[..stride], &atlas.rgba[9 * stride..10 * stride]);
    capture(&app, "group-damage", [1280, 800], None);
    let position = app.world.addon_position(EntityId(1)).unwrap();
    app.issue(Order::Build {
        entity: EntityId(1),
        unit_type: UnitTypeId(35),
        position,
    })
    .unwrap();
    app.issue(Order::Build {
        entity: EntityId(2),
        unit_type: UnitTypeId(34),
        position: Position { x: 928, y: 528 },
    })
    .unwrap();
    advance(&mut app);
    let first = pose(&app, EntityId(1));
    for _ in 0..6 {
        advance(&mut app);
    }
    assert_eq!(
        app.visuals.get(EntityId(1)).unwrap().action,
        VisualAction::Production
    );
    assert_ne!(
        first,
        pose(&app, EntityId(1)),
        "Factory working child animates"
    );
    app.selected = BTreeSet::from([EntityId(1)]);
    capture(&app, "factory-working-starport-lifting", [1280, 800], None);
    until(
        &mut app,
        |app| app.world.constructing_addon(EntityId(2)),
        250,
    );
    let first = pose(&app, EntityId(2));
    for _ in 0..2 {
        advance(&mut app);
    }
    assert_eq!(
        app.visuals.get(EntityId(2)).unwrap().action,
        VisualAction::Production
    );
    assert_ne!(
        first,
        pose(&app, EntityId(2)),
        "Starport working child animates"
    );
    until(
        &mut app,
        |app| !app.world.addon_pending(EntityId(1)) && !app.world.addon_pending(EntityId(2)),
        60,
    );
    assert_eq!(
        entity(&app, EntityId(2)).position,
        Position { x: 832, y: 512 }
    );
    let assets = app.assets.as_ref().unwrap();
    for parent in [EntityId(1), EntityId(2)] {
        let addon = app
            .world
            .state()
            .entities
            .iter()
            .find(|unit| unit.parent == Some(parent))
            .unwrap();
        assert_eq!(Some(addon.position), app.world.addon_position(parent));
        let connector = visual::addon_connector(assets, addon).expect("source dock connector");
        assert!(
            connector
                .image
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel[3] != 0)
        );
    }
    capture(&app, "building-addons-attached", [1280, 800], None);
    app.selected = BTreeSet::from([EntityId(2)]);
    assert!(
        app.buttons()
            .iter()
            .any(|button| button.action == Action::Train(UnitTypeId(53))
                && button.disabled.is_none())
    );
    app.activate(Action::Train(UnitTypeId(53))).unwrap();
    until(
        &mut app,
        |app| {
            app.world
                .state()
                .entities
                .iter()
                .any(|unit| unit.unit_type == UnitTypeId(53))
        },
        80,
    );
    let dropship = app
        .world
        .state()
        .entities
        .iter()
        .find(|unit| unit.unit_type == UnitTypeId(53))
        .unwrap()
        .id;
    for passenger in [EntityId(4), EntityId(5), EntityId(6)] {
        app.issue(Order::Load {
            entity: passenger,
            target: dropship,
        })
        .unwrap();
    }
    until(
        &mut app,
        |app| {
            app.world
                .state()
                .entities
                .iter()
                .filter(|unit| unit.garrisoned_in == Some(dropship))
                .count()
                == 3
        },
        250,
    );
    app.selected = BTreeSet::from([dropship]);
    capture(&app, "dropship-passenger-damage", [1280, 800], None);
    let [x, y, w, h] = controls::selection_rect(0, [1280.0, 800.0], true);
    assert!(app.select_panel([x + w / 2.0, y + h / 2.0], [1280.0, 800.0]));
    advance(&mut app);
    assert_eq!(entity(&app, EntityId(4)).garrisoned_in, None);
    assert_eq!(entity(&app, EntityId(5)).garrisoned_in, Some(dropship));
    app.issue(Order::Move {
        entity: dropship,
        target: Position { x: 1100, y: 600 },
    })
    .unwrap();
    until(
        &mut app,
        |app| entity(app, dropship).position == Position { x: 1100, y: 600 },
        200,
    );
    assert_eq!(
        entity(&app, EntityId(5)).position,
        entity(&app, dropship).position
    );
    app.activate(Action::Unload).unwrap();
    advance(&mut app);
    assert_eq!(entity(&app, EntityId(5)).garrisoned_in, None);
    assert_eq!(entity(&app, EntityId(6)).garrisoned_in, None);
    // Ownership observers receive the same entity IDs with changed owners.
    map.spawns.truncate(1);
    map.spawns[0].owner = PlayerId(1);
    let before = World::new(app.world.rules().clone(), map.clone(), 42).unwrap();
    app.visuals = Visuals::new(&before);
    app.audio.reset(&before);
    map.spawns[0].owner = PlayerId(0);
    app.world = World::new(app.world.rules().clone(), map, 42).unwrap();
    advance(&mut app);
    app.audio.observe(&app.world);
    assert_eq!(app.visuals.get(EntityId(1)).unwrap().captured_tick, Some(1));
    assert!(
        app.audio
            .events
            .contains(&(straterust_engine::media::AudioCue::Capture, None))
    );
    assert!(
        app.media
            .as_ref()
            .unwrap()
            .audio
            .iter()
            .any(|mapping| mapping.cue == straterust_engine::media::AudioCue::Capture)
    );
    app.selected.clear();
    app.camera.x = 512.0;
    app.camera.y = 512.0;
    capture(&app, "capture-circle", [1280, 800], None);
}
