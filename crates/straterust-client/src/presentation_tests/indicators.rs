//! One short native review of source indicators, pointer contexts and fog.
use super::*;
use straterust_engine::sim::{Rejection, ResearchId, ResourceAmount, Spawn};

#[test]
#[ignore = "requires private mission 5 assets; captures selection and pointer presentation only"]
fn native_selection_pointer_and_fog() {
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
    app.mission_ui = None;
    let pack = app.assets.as_ref().unwrap().indicators.as_ref().unwrap();
    let marine = pack
        .manifest
        .units
        .iter()
        .find(|entry| entry.unit_type == UnitTypeId(1))
        .unwrap();
    assert_eq!(
        (
            marine.circle,
            marine.circle_y,
            marine.bar_width,
            marine.bar_y
        ),
        (0, 9, 19, 23)
    );
    let cc = pack
        .manifest
        .units
        .iter()
        .find(|entry| entry.unit_type == UnitTypeId(3))
        .unwrap();
    assert_eq!((cc.circle, cc.circle_y, cc.bar_width), (8, 6, 109));
    assert!(
        pack.manifest
            .cursors
            .iter()
            .all(|cursor| cursor.frame_ms == 100 && cursor.anchor == [63, 63])
    );
    let mut map = app.world.map().clone();
    map.mission = None;
    map.ai.clear();
    map.terrain = None;
    map.resources.clear();
    map.initial_explored.clear();
    map.start_locations.clear();
    map.spawns = [
        (1, 512, 512, 100, 0),
        (1, 544, 512, 50, 0),
        (1, 576, 512, 20, 0),
        (10, 544, 576, 100, 0),
        (3, 768, 576, 100, 0),
        (1, 672, 480, 100, 1),
        (1, 640, 448, 100, 2),
    ]
    .into_iter()
    .map(|(unit, x, y, hp, player)| Spawn {
        unit_type: UnitTypeId(unit),
        position: Position { x, y },
        hp_percent: Some(hp),
        owner: PlayerId(player),
        ..Default::default()
    })
    .collect();
    let mut rules = app.world.rules().clone();
    rules.victory = false;
    app.world = World::new(rules, map, 0).unwrap();
    app.camera.x = 640.0;
    app.camera.y = 544.0;
    app.visuals = Visuals::new(&app.world);
    app.visuals.update(&app.world);
    app.selected = app
        .world
        .state()
        .entities
        .iter()
        .filter(|entity| entity.owner == PlayerId(0) && entity.unit_type != UnitTypeId(3))
        .map(|entity| entity.id)
        .collect();
    app.cursor = PhysicalPosition::new(1040.0, 360.0);
    capture_at(&app, "indicators-group", [1280, 800], None, None, 0);
    let id = app
        .world
        .state()
        .entities
        .iter()
        .find(|entity| entity.unit_type == UnitTypeId(3))
        .unwrap()
        .id;
    app.selected = BTreeSet::from([id]);
    let p = app.camera.world_to_screen(768.0, 576.0, [1280.0, 800.0]);
    app.cursor = PhysicalPosition::new(p[0], p[1]);
    let a = capture_at(&app, "indicators-building-a", [1280, 800], None, None, 0);
    let b = capture_at(&app, "indicators-building-b", [1280, 800], None, None, 300);
    assert_ne!(a, b, "hover cursor must animate while gameplay is frozen");
    app.cursor = PhysicalPosition::new(1040.0, 360.0);
    app.selected.clear();
    capture_at(&app, "indicators-unselected", [1280, 800], None, None, 0);

    // The same short review covers the newly imported mission 5 controls/art.
    assert!(
        !package
            .world(0)
            .unwrap()
            .creation_allowed(PlayerId(0), UnitTypeId(21))
    );
    assert_eq!(
        app.world
            .unit_type(UnitTypeId(3))
            .unwrap()
            .resource_clearance,
        96
    );
    assert_eq!(
        app.world
            .rules()
            .research
            .iter()
            .filter(|r| (5..=8).contains(&r.id.0))
            .count(),
        4
    );
    let mut rules = app.world.rules().clone();
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
    for research in &mut rules.research {
        research.ticks = 1;
    }
    let mut map = app.world.map().clone();
    map.fog_of_war = false;
    map.spawns = [
        (2, 512, 512),
        (32, 768, 544),
        (35, 864, 560),
        (33, 768, 736),
        (34, 864, 752),
        (23, 576, 640),
        (53, 512, 704),
        (1, 512, 736),
        (21, 576, 512),
        (20, 576, 736),
    ]
    .into_iter()
    .map(|(unit, x, y)| Spawn {
        unit_type: UnitTypeId(unit),
        position: Position { x, y },
        ..Default::default()
    })
    .collect();
    app.world = World::new(rules, map, 0).unwrap();
    app.visuals = Visuals::new(&app.world);
    app.selected = BTreeSet::from([EntityId(1)]);
    app.activate(Action::BuildMenu).unwrap();
    let basic = app.buttons();
    assert_eq!(
        basic
            .iter()
            .filter(|b| matches!(b.action, Action::Build(_)))
            .count(),
        8
    );
    capture(&app, "indicators-basic-menu", [1280, 800], None);
    app.activate(Action::Back).unwrap();
    app.activate(Action::AdvancedBuildMenu).unwrap();
    // Mission 5 permits Factory and Starport here. Engineering Bay belongs in
    // the basic menu, and Science Facility unlocks in later missions.
    assert_eq!(
        app.buttons()
            .iter()
            .filter_map(|b| match b.action {
                Action::Build(unit) => Some(unit),
                _ => None,
            })
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([UnitTypeId(32), UnitTypeId(33)])
    );
    capture(&app, "indicators-advanced-menu", [1280, 800], None);
    app.activate(Action::Back).unwrap();
    app.selected = BTreeSet::from([EntityId(5)]);
    assert!(
        app.buttons()
            .iter()
            .any(|b| b.action == Action::Research(ResearchId(5)) && b.disabled.is_none())
    );
    assert_eq!(
        app.world.cloak_rejection(EntityId(6), true),
        Some(Rejection::MissingPrerequisite)
    );
    capture(&app, "indicators-tower-research", [1280, 800], None);
    app.activate(Action::Research(ResearchId(5))).unwrap();
    advance(&mut app);
    assert_eq!(app.world.cloak_rejection(EntityId(6), true), None);
    app.selected = BTreeSet::from([EntityId(3)]);
    capture(&app, "indicators-shop-research", [1280, 800], None);
    let destination = Position { x: 656, y: 736 };
    assert_eq!(
        app.world
            .mine_rejection(EntityId(10), Position { x: 640, y: 800 }),
        Some(Rejection::MissingPrerequisite)
    );
    let mut baseline = app.world.clone();
    baseline
        .step(&[crate::Command {
            tick: baseline.tick(),
            player: PlayerId(0),
            sequence: baseline.state().last_sequences[0] + 1,
            order: Order::Move {
                entity: EntityId(10),
                target: destination,
            },
        }])
        .unwrap();
    for _ in 0..8 {
        baseline.step(&[]).unwrap();
    }
    assert_eq!(
        app.world
            .research_rejection(PlayerId(0), EntityId(3), ResearchId(7)),
        None
    );
    app.activate(Action::Research(ResearchId(7))).unwrap();
    advance(&mut app);
    assert!(app.world.has_research(PlayerId(0), ResearchId(7)));
    app.issue(Order::Move {
        entity: EntityId(10),
        target: destination,
    })
    .unwrap();
    for _ in 0..9 {
        advance(&mut app);
    }
    assert!(
        entity(&app, EntityId(10)).position.x > baseline.state().entities[9].position.x,
        "Ion Thrusters must increase actual movement: upgraded {:?}, baseline {:?}",
        entity(&app, EntityId(10)).position,
        baseline.state().entities[9].position
    );
    app.activate(Action::Research(ResearchId(8))).unwrap();
    advance(&mut app);
    assert_eq!(
        app.world
            .mine_rejection(EntityId(10), Position { x: 640, y: 800 }),
        None
    );
    app.selected = BTreeSet::from([EntityId(7)]);
    app.issue(Order::Load {
        entity: EntityId(8),
        target: EntityId(7),
    })
    .unwrap();
    until(
        &mut app,
        |a| entity(a, EntityId(8)).garrisoned_in.is_some(),
        100,
    );
    assert!(
        app.buttons()
            .iter()
            .any(|b| b.action == Action::Unload && b.disabled.is_none())
    );
    capture(&app, "indicators-dropship", [1280, 800], None);
    app.activate(Action::Unload).unwrap();
    assert_eq!(app.target_mode, Some(TargetMode::Unload));
    app.targeting_click(Position { x: 512, y: 736 }).unwrap();
    until(
        &mut app,
        |a| entity(a, EntityId(8)).garrisoned_in.is_none(),
        200,
    );
    assert!(entity(&app, EntityId(8)).garrisoned_in.is_none());
    let a = capture_at(&app, "indicators-aircraft-a", [1280, 800], None, None, 0);
    for _ in 0..9 {
        advance(&mut app);
    }
    let b = capture_at(&app, "indicators-aircraft-b", [1280, 800], None, None, 0);
    assert_ne!(
        a, b,
        "idle aircraft body must float independently of its ground shadow"
    );
    let effect = app
        .assets
        .as_ref()
        .unwrap()
        .projectiles
        .iter()
        .find(|p| p.manifest.unit_type == UnitTypeId(21) && p.manifest.targets_air)
        .expect("Goliath imports its original air missile");
    assert!(!effect.flight.frames.is_empty());
    let mut map = app.world.map().clone();
    map.spawns = vec![
        Spawn {
            unit_type: UnitTypeId(21),
            position: Position { x: 576, y: 512 },
            ..Default::default()
        },
        Spawn {
            unit_type: UnitTypeId(23),
            position: Position { x: 720, y: 512 },
            owner: PlayerId(1),
            ..Default::default()
        },
    ];
    app.world = World::new(app.world.rules().clone(), map, 0).unwrap();
    app.visuals = Visuals::new(&app.world);
    app.selected = BTreeSet::from([EntityId(1)]);
    app.camera.x = 640.0;
    app.camera.y = 512.0;
    app.issue(Order::Attack {
        entity: EntityId(1),
        target: EntityId(2),
    })
    .unwrap();
    until(
        &mut app,
        |a| {
            a.visuals
                .projectiles()
                .iter()
                .any(|p| p.unit_type == UnitTypeId(21) && p.targets_air)
        },
        20,
    );
    app.visuals
        .advance_effects(Duration::from_millis(100), app.assets.as_ref());
    capture(&app, "indicators-goliath-missile", [1280, 800], None);
    let mut map = app.world.map().clone();
    map.fog_of_war = true;
    map.spawns.truncate(1);
    map.spawns[0].unit_type = UnitTypeId(1);
    app.world = World::new(app.world.rules().clone(), map, 0).unwrap();
    app.visuals = Visuals::new(&app.world);
    app.selected.clear();
    app.camera.zoom = 1.0;
    capture(&app, "indicators-fog", [1280, 800], None);
}
