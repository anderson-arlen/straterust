use super::*;

#[test]
#[ignore = "writes original/private visual review artifacts to fixed /tmp paths"]
fn playable_presentation_review() {
    let directory = std::env::var_os("STRATERUST_ASSET_PACKAGE").map_or_else(
        || PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../content/terran-demo"),
        PathBuf::from,
    );
    let package = Package::load(&directory).unwrap();
    let art = read_ron(&directory.join("presentation.ron")).unwrap();
    let assets = AssetPack::load(&directory).unwrap();
    let mut app = App::new(
        &package,
        Config {
            width: 1280,
            height: 800,
            audio: false,
            ..Config::default()
        },
        art,
        assets,
        None,
    )
    .unwrap();
    app.load_media(&directory).unwrap();
    app.selected.insert(EntityId(2));
    review_portrait(&app, "scv");
    app.activate(Action::BuildMenu).unwrap();
    app.activate(Action::Build(UnitTypeId(5))).unwrap();
    assert!(
        app.target_mode.is_none(),
        "unavailable Barracks entered placement"
    );
    assert!(app.status.contains("Requires completed"));
    capture(
        &app,
        "disabled",
        [1280, 800],
        Some(Action::Build(UnitTypeId(5))),
    );
    capture(
        &app,
        "disabled-small",
        [640, 480],
        Some(Action::Build(UnitTypeId(5))),
    );

    app.activate(Action::Back).unwrap();
    app.contextual_order(app.world.state().resources[0].position)
        .unwrap();
    until(
        &mut app,
        |app| app.visuals.get(EntityId(2)).unwrap().action == VisualAction::Work,
        200,
    );
    assert_work_effect_visible(&app, EntityId(2));
    capture(&app, "mining", [1280, 800], None);
    until(
        &mut app,
        |app| app.world.resource_balance(PlayerId(0), "minerals") >= 100,
        2000,
    );
    app.activate(Action::Build(UnitTypeId(4))).unwrap();
    let requested = Position { x: 352, y: 416 };
    let cursor = app.camera.world_to_screen(
        f64::from(requested.x),
        f64::from(requested.y),
        app.logical_size(),
    );
    app.cursor = winit::dpi::PhysicalPosition::new(cursor[0], cursor[1]);
    let (_, snapped, valid) = app.placement().expect("active build preview");
    assert!(valid);
    assert_ne!(snapped, requested);
    let [left, top, _, _] = app
        .world
        .unit_type(UnitTypeId(4))
        .unwrap()
        .placement
        .bounds(snapped);
    assert_eq!(left.rem_euclid(i64::from(BUILD_GRID)), 0);
    assert_eq!(top.rem_euclid(i64::from(BUILD_GRID)), 0);
    capture(&app, "placement-snapped", [1280, 800], None);
    app.targeting_click(requested).unwrap();
    assert!(
        matches!(app.recorded.last().unwrap().order,Order::Build{position,..} if position==snapped)
    );
    advance(&mut app);
    let depot = app
        .world
        .state()
        .entities
        .iter()
        .find(|unit| unit.unit_type == UnitTypeId(4))
        .unwrap()
        .id;
    app.selected = BTreeSet::from([depot]);
    review_portrait(&app, "advisor");
    capture(&app, "construction-early", [1280, 800], None);
    let stages = [
        (450, "construction-quarter"),
        (300, "construction-half"),
        (150, "construction-late"),
    ];
    let mut next_stage = 0;
    let mut first_work: Option<(Position, u32, Option<Vec<u8>>)> = None;
    let mut repositioned = false;
    let mut resumed_work = false;
    for _ in 0..1800 {
        let Some(remaining) = entity(&app, depot)
            .construction
            .as_ref()
            .map(|work| work.remaining)
        else {
            break;
        };
        let position = entity(&app, EntityId(2)).position;
        let action = app.visuals.get(EntityId(2)).unwrap().action;
        if first_work.is_none() && action == VisualAction::Work {
            assert_work_effect_visible(&app, EntityId(2));
            first_work = Some((position, remaining, pose(&app, EntityId(2))));
            capture(&app, "construction-worker-work", [1280, 800], None);
        }
        if let Some((work_position, work_remaining, work_pose)) = &first_work {
            if !repositioned && action == VisualAction::Move && position != *work_position {
                assert!(
                    remaining < *work_remaining,
                    "construction advances while worker changes position"
                );
                if let Some(before) = work_pose {
                    assert!(
                        pose(&app, EntityId(2)).as_ref() != Some(before),
                        "walking must not keep the drilling pose/effect"
                    );
                }
                repositioned = true;
                capture(&app, "construction-worker-moving", [1280, 800], None);
            } else if repositioned && !resumed_work && action == VisualAction::Work {
                assert!(
                    (position.x - work_position.x).abs() + (position.y - work_position.y).abs()
                        >= 32,
                    "construction worker reaches a visibly different work location"
                );
                assert!(remaining < *work_remaining);
                assert_work_effect_visible(&app, EntityId(2));
                resumed_work = true;
                capture(&app, "construction-worker-resumed", [1280, 800], None);
            }
        }
        while next_stage < stages.len() && remaining <= stages[next_stage].0 {
            capture(&app, stages[next_stage].1, [1280, 800], None);
            next_stage += 1;
        }
        advance(&mut app);
    }
    assert!(first_work.is_some() && repositioned && resumed_work);
    assert_eq!(next_stage, stages.len());
    assert_eq!(completed(&app, 4), Some(depot));
    capture(&app, "construction-complete", [1280, 800], None);
    assert_eq!(app.visuals.get(depot).unwrap().action, VisualAction::Idle);
    let depot_idle = pose(&app, depot);
    let idle_tick = app.world.tick().0;
    capture(&app, "depot-idle-a", [1280, 800], None);
    until(
        &mut app,
        |app| {
            app.world.tick().0 >= idle_tick + 3
                && depot_idle
                    .as_ref()
                    .is_none_or(|before| pose(app, depot).as_ref() != Some(before))
        },
        120,
    );
    assert_eq!(app.visuals.get(depot).unwrap().action, VisualAction::Idle);
    capture(&app, "depot-idle-b", [1280, 800], None);

    app.selected = BTreeSet::from([EntityId(2)]);
    app.contextual_order(app.world.state().resources[0].position)
        .unwrap();
    until(
        &mut app,
        |app| app.world.resource_balance(PlayerId(0), "minerals") >= 150,
        3000,
    );
    app.activate(Action::BuildMenu).unwrap();
    let button = app
        .buttons()
        .into_iter()
        .find(|button| button.action == Action::Build(UnitTypeId(5)))
        .unwrap();
    assert!(button.disabled.is_none());
    capture(
        &app,
        "enabled",
        [1280, 800],
        Some(Action::Build(UnitTypeId(5))),
    );
    app.activate(Action::Build(UnitTypeId(5))).unwrap();
    app.targeting_click(Position { x: 576, y: 416 }).unwrap();
    until(&mut app, |app| completed(app, 5).is_some(), 2000);
    app.contextual_order(app.world.state().resources[0].position)
        .unwrap();
    until(
        &mut app,
        |app| app.world.resource_balance(PlayerId(0), "minerals") >= 50,
        1000,
    );
    let barracks = completed(&app, 5).unwrap();
    app.selected = BTreeSet::from([barracks]);
    assert_eq!(
        app.visuals.get(barracks).unwrap().action,
        VisualAction::Idle
    );
    let barracks_idle = pose(&app, barracks);
    capture(&app, "barracks-before-production", [1280, 800], None);
    app.activate(Action::Train(UnitTypeId(1))).unwrap();
    until(
        &mut app,
        |app| app.visuals.get(barracks).unwrap().action == VisualAction::Production,
        10,
    );
    let production_a = pose(&app, barracks);
    if barracks_idle.is_some() {
        assert!(
            production_a != barracks_idle,
            "native production must change building pixels"
        );
    }
    let fan_during_production = pose(&app, depot);
    let production_tick = app.world.tick().0;
    capture(&app, "barracks-production-a", [1280, 800], None);
    until(
        &mut app,
        |app| {
            app.world.tick().0 >= production_tick + 3
                && production_a
                    .as_ref()
                    .is_none_or(|before| pose(app, barracks).as_ref() != Some(before))
                && fan_during_production
                    .as_ref()
                    .is_none_or(|before| pose(app, depot).as_ref() != Some(before))
        },
        120,
    );
    assert_eq!(
        app.visuals.get(barracks).unwrap().action,
        VisualAction::Production
    );
    assert_eq!(
        app.visuals.get(depot).unwrap().action,
        VisualAction::Idle,
        "depot fan is independent of production orders"
    );
    capture(&app, "barracks-production-b", [1280, 800], None);
    until(&mut app, |app| completed(app, 1).is_some(), 1000);
    let marine = completed(&app, 1).unwrap();
    assert!(entity(&app, barracks).production.is_empty());
    assert_eq!(
        app.visuals.get(barracks).unwrap().action,
        VisualAction::Idle
    );
    assert!(
        pose(&app, barracks) == barracks_idle,
        "production art stops when training completes"
    );
    capture(&app, "barracks-after-production", [1280, 800], None);
    app.selected = BTreeSet::from([depot]);
    let fan_after = pose(&app, depot);
    let stopped_tick = app.world.tick().0;
    capture(&app, "depot-after-production-a", [1280, 800], None);
    until(
        &mut app,
        |app| {
            app.world.tick().0 >= stopped_tick + 3
                && fan_after
                    .as_ref()
                    .is_none_or(|before| pose(app, depot).as_ref() != Some(before))
        },
        120,
    );
    assert_eq!(app.visuals.get(depot).unwrap().action, VisualAction::Idle);
    assert_eq!(
        app.visuals.get(barracks).unwrap().action,
        VisualAction::Idle
    );
    capture(&app, "depot-after-production-b", [1280, 800], None);
    app.selected = BTreeSet::from([marine]);
    review_portrait(&app, "marine");
    capture(&app, "marine-idle", [1280, 800], None);
    let from = app
        .world
        .state()
        .entities
        .iter()
        .find(|unit| unit.id == marine)
        .unwrap()
        .position;
    app.contextual_order(Position {
        x: from.x - 80,
        y: from.y,
    })
    .unwrap();
    advance(&mut app);
    assert_eq!(app.visuals.get(marine).unwrap().action, VisualAction::Move);
    assert_eq!(app.visuals.get(marine).unwrap().facing, 24);
    capture(&app, "marine-west", [1280, 800], None);
    until(
        &mut app,
        |app| app.visuals.get(marine).unwrap().action == VisualAction::Idle,
        100,
    );
    capture(&app, "marine-stopped", [1280, 800], None);
    app.contextual_order(Position { x: 1280, y: 256 }).unwrap();
    until(
        &mut app,
        |app| {
            app.visuals
                .get(marine)
                .is_some_and(|unit| unit.shot_tick == Some(app.world.tick().0))
        },
        1000,
    );
    app.camera.x = 1200.0;
    app.camera.y = 280.0;
    capture(&app, "marine-attack", [1280, 800], None);
    review_repair_and_deaths(&mut app);
}

#[test]
#[ignore = "requires a private campaign import and writes fixed burrow concealment review screenshots"]
fn burrow_concealment_presentation_review() {
    use straterust_engine::sim::{MissionAction, MissionCondition, MissionTrigger, Spawn};
    let directory = PathBuf::from(
        std::env::var_os("STRATERUST_ASSET_PACKAGE").expect("set STRATERUST_ASSET_PACKAGE"),
    );
    let package = Package::load(&directory).unwrap();
    let original = package.world(42).unwrap();
    let mut map = original.map().clone();
    map.fog_of_war = false;
    map.terrain = None;
    map.resources.clear();
    map.spawns = vec![
        Spawn {
            owner: PlayerId(0),
            unit_type: UnitTypeId(1),
            position: Position { x: 900, y: 1100 },
            ..Default::default()
        },
        Spawn {
            owner: PlayerId(1),
            unit_type: UnitTypeId(7),
            position: Position { x: 980, y: 1100 },
            cloaked: true,
            ..Default::default()
        },
    ];
    map.mission.as_mut().unwrap().triggers = vec![MissionTrigger {
        conditions: vec![MissionCondition::Switch {
            index: 0,
            set: true,
        }],
        actions: vec![MissionAction::Victory],
    }];
    let mut app = App::new(
        &package,
        Config {
            width: 1280,
            height: 800,
            audio: false,
            ..Default::default()
        },
        read_ron(&directory.join("presentation.ron")).unwrap(),
        AssetPack::load(&directory).unwrap(),
        None,
    )
    .unwrap();
    app.load_media(&directory).unwrap();
    app.mission_ui = None;
    app.world = World::new(original.rules().clone(), map, 42).unwrap();
    app.visuals = Visuals::new(&app.world);
    app.selected = BTreeSet::from([EntityId(1)]);
    app.camera = Camera {
        x: 940.0,
        y: 1100.0,
        zoom: 1.5,
    };
    let hp = entity(&app, EntityId(2)).hp;
    for (tick, name) in [
        (0, "buried"),
        (1, "startup"),
        (4, "midpoint"),
        (8, "emerged"),
        (9, "combat"),
    ] {
        while app.world.tick().0 < tick {
            advance(&mut app);
        }
        if tick < 8 {
            assert!(!app.world.entity_visible(PlayerId(0), EntityId(2)));
            assert_eq!(entity(&app, EntityId(2)).hp, hp);
            assert_eq!(
                entity(&app, EntityId(1)).cooldown,
                0,
                "Marine cannot fire underground"
            );
        } else {
            assert!(app.world.entity_visible(PlayerId(0), EntityId(2)));
        }
        capture(&app, &format!("burrow-{name}"), [1280, 800], None);
    }
    assert!(
        entity(&app, EntityId(1)).cooldown > 0,
        "Marine fires after emergence"
    );
    until(&mut app, |app| entity(app, EntityId(2)).hp < hp, 20);
}

#[test]
#[ignore = "requires a private campaign import and writes fixed group-selection review screenshots"]
fn group_selection_presentation_review() {
    use straterust_engine::sim::Spawn;
    let directory = PathBuf::from(
        std::env::var_os("STRATERUST_ASSET_PACKAGE").expect("set STRATERUST_ASSET_PACKAGE"),
    );
    let package = Package::load(&directory).unwrap();
    let original = package.world(42).unwrap();
    let mut rules = original.rules().clone();
    rules.victory = false;
    let mut map = original.map().clone();
    map.mission = None;
    map.fog_of_war = false;
    map.terrain = None;
    map.resources.clear();
    map.spawns = [1, 2, 10, 11, 18, 6, 7, 3, 4, 5, 15, 12]
        .into_iter()
        .enumerate()
        .map(|(slot, id)| Spawn {
            owner: PlayerId(0),
            unit_type: UnitTypeId(id),
            position: Position {
                x: 300 + (slot / 2) as i32 * 240,
                y: 800 + (slot % 2) as i32 * 240,
            },
            hp_percent: Some([100, 50, 20][slot % 3]),
            ..Default::default()
        })
        .collect();
    let mut app = App::new(
        &package,
        Config {
            width: 1280,
            height: 800,
            audio: false,
            ..Config::default()
        },
        read_ron(&directory.join("presentation.ron")).unwrap(),
        AssetPack::load(&directory).unwrap(),
        None,
    )
    .unwrap();
    app.load_media(&directory).unwrap();
    app.mission_ui = None;
    app.world = World::new(rules, map, 42).unwrap();
    app.visuals = Visuals::new(&app.world);
    app.camera = Camera {
        x: 1000.0,
        y: 1100.0,
        zoom: 1.0,
    };
    let members: BTreeSet<_> = app
        .world
        .state()
        .entities
        .iter()
        .map(|entity| entity.id)
        .collect();
    app.selected = members.clone();
    app.cursor = PhysicalPosition::new(-1.0, -1.0);
    for (name, size) in [
        ("640", [640, 480]),
        ("800", [800, 600]),
        ("1280", [1280, 800]),
    ] {
        capture(&app, &format!("selection-{name}"), size, None);
        let logical = size.map(f64::from);
        for slot in 0..12 {
            app.selected = members.clone();
            let [x, y, w, h] = controls::selection_rect(slot, logical, true);
            assert!(app.select_panel([x + w / 2.0, y + h / 2.0], logical));
            assert_eq!(app.selected, BTreeSet::from([EntityId(slot as u32 + 1)]));
        }
        app.selected = members.clone();
    }
    let [x, y, w, h] = controls::selection_rect(2, [1280.0, 800.0], true);
    app.cursor = PhysicalPosition::new(x + w / 2.0, y + h / 2.0);
    capture(&app, "selection-hover", [1280, 800], None);
    app.select_panel([x + w / 2.0, y + h / 2.0], [1280.0, 800.0]);
    capture(&app, "selection-single", [1280, 800], None);
}
