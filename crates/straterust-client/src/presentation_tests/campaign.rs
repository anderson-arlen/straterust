use super::*;

#[test]
#[ignore = "requires a private original campaign import and writes fixed review screenshots"]
fn campaign_presentation_review() {
    let directory = PathBuf::from(
        std::env::var_os("STRATERUST_ASSET_PACKAGE")
            .expect("set STRATERUST_ASSET_PACKAGE to the private campaign package"),
    );
    let package = Package::load(&directory).unwrap();
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
    assert!(app.world.map().mission.is_some());
    let initial = app.world.state_hash();
    let mut speakers = BTreeSet::new();
    for _ in 0..12000 {
        let mission = app.mission_ui.as_mut().unwrap();
        mission.advance(
            Duration::from_millis(42),
            app.media.as_ref().unwrap(),
            &mut app.audio,
        );
        if let Some(slot) = mission.active_slot
            && let Some(portrait) = mission.portraits[slot]
            && speakers.insert(portrait)
        {
            capture(
                &app,
                &format!("campaign-briefing-{}", portrait.0),
                [1280, 800],
                None,
            );
            capture(
                &app,
                &format!("campaign-briefing-{}-narrow", portrait.0),
                [640, 480],
                None,
            );
        }
        if app.mission_ui.as_ref().unwrap().briefing_finished {
            break;
        }
    }
    assert!(app.mission_ui.as_ref().unwrap().briefing_finished);
    assert!(!speakers.is_empty());
    assert_eq!(
        app.world.state_hash(),
        initial,
        "briefing must not tick gameplay"
    );
    capture(&app, "campaign-briefing-end", [1280, 800], None);
    app.mission_ui.as_mut().unwrap().start(&mut app.audio);
    for _ in 0..600 {
        advance(&mut app);
    }
    for (producer, trainees) in [(3, vec![(2, "S")]), (5, vec![(1, "M"), (11, "F")])] {
        let id = completed(&app, producer).expect("original starting production facility");
        app.selected = BTreeSet::from([id]);
        let buttons = app.buttons();
        for (unit_type, key) in trainees {
            assert!(
                buttons.iter().any(
                    |button| button.action == Action::Train(UnitTypeId(unit_type))
                        && button.key == key
                ),
                "source training key {key} for {unit_type}"
            );
        }
    }
    let controlled = app
        .world
        .state()
        .entities
        .iter()
        .find(|entity| {
            entity.owner == PlayerId(0) && !app.world.unit_type(entity.unit_type).unwrap().structure
        })
        .unwrap()
        .id;
    app.selected = BTreeSet::from([controlled]);
    let home = entity(&app, controlled).position;
    app.camera.x = f64::from(home.x);
    app.camera.y = f64::from(home.y);
    capture(&app, "campaign-start", [1280, 800], None);
    capture(&app, "campaign-start-narrow", [640, 480], None);
    for (unit_type, name, limit) in [(10, "raynor", 2400), (3, "outpost", 4800)] {
        let target_entity = app
            .world
            .state()
            .entities
            .iter()
            .find(|entity| {
                entity.unit_type == UnitTypeId(unit_type)
                    && (unit_type == 10 || entity.owner == PlayerId(3))
            })
            .unwrap()
            .clone();
        let target = target_entity.position;
        let actor = if unit_type == 3 {
            completed(&app, 10).unwrap()
        } else {
            controlled
        };
        app.selected = BTreeSet::from([actor]);
        if target_entity.owner != PlayerId(0) {
            app.issue(Order::Move {
                entity: actor,
                target,
            })
            .unwrap();
            until(
                &mut app,
                |app| {
                    app.world
                        .state()
                        .entities
                        .iter()
                        .any(|entity| entity.id == target_entity.id && entity.owner == PlayerId(0))
                },
                limit,
            );
        }
        app.selected = BTreeSet::from([target_entity.id]);
        app.camera.x = f64::from(target.x);
        app.camera.y = f64::from(target.y);
        capture(&app, &format!("campaign-rescue-{name}"), [1280, 800], None);
        capture(
            &app,
            &format!("campaign-rescue-{name}-narrow"),
            [640, 480],
            None,
        );
    }
    for _ in 0..300 {
        advance(&mut app);
    }
    if let Some(worker) = completed(&app, 2) {
        app.selected = BTreeSet::from([worker]);
        app.activate(Action::BuildMenu).unwrap();
        capture(&app, "campaign-build-menu", [1280, 800], None);
        capture(&app, "campaign-build-menu-narrow", [640, 480], None);
        app.activate(Action::Back).unwrap();
    }
    let rescued = app
        .world
        .state()
        .entities
        .iter()
        .filter(|entity| entity.owner == PlayerId(0))
        .count();
    assert!(rescued > 5, "original outpost rescue transfers its owner");
    app.restart().unwrap();
    assert!(app.mission_ui.as_ref().unwrap().briefing);
    assert_eq!(app.world.state_hash(), initial);
}

#[test]
#[ignore = "requires the private ordinary-command campaign victory recording"]
fn campaign_outcome_review() {
    let directory = PathBuf::from(
        std::env::var_os("STRATERUST_ASSET_PACKAGE").expect("set STRATERUST_ASSET_PACKAGE"),
    );
    let recording = std::env::var_os("STRATERUST_CAMPAIGN_RECORDING")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            directory
                .parent()
                .unwrap()
                .join("backwater-playthrough.ron")
        });
    let scenario: Scenario = read_ron(&recording).unwrap();
    let ticks = scenario.ticks;
    let package = Package::load(&directory).unwrap();
    let mut app = App::new(
        &package,
        Config {
            audio: false,
            ..Config::default()
        },
        read_ron(&directory.join("presentation.ron")).unwrap(),
        AssetPack::load(&directory).unwrap(),
        Some(scenario),
    )
    .unwrap();
    app.load_media(&directory).unwrap();
    let mut captured_pause = false;
    for _ in 0..ticks {
        advance(&mut app);
        if !captured_pause
            && app
                .world
                .state()
                .mission
                .as_ref()
                .is_some_and(|mission| mission.paused)
            && app
                .mission_ui
                .as_ref()
                .is_some_and(|mission| mission.text.is_some())
        {
            capture(&app, "campaign-finale", [1280, 800], None);
            capture(&app, "campaign-finale-narrow", [640, 480], None);
            captured_pause = true;
        }
        if app.world.tick().0.is_multiple_of(1000) {
            eprintln!("campaign outcome review tick={}", app.world.tick().0);
        }
    }
    assert!(
        captured_pause,
        "original finale pauses gameplay and presents dialogue"
    );
    assert_eq!(app.world.state().winner, Some(PlayerId(0)));
    app.paused = true;
    capture(&app, "campaign-victory", [1280, 800], None);
    capture(&app, "campaign-victory-narrow", [640, 480], None);
}

#[test]
#[ignore = "requires the private Backwater terrain/art and writes moving cliff-visibility captures"]
fn campaign_cliff_visibility_review() {
    use straterust_engine::sim::{Spawn, Visibility};
    let directory = PathBuf::from(
        std::env::var_os("STRATERUST_ASSET_PACKAGE").expect("set STRATERUST_ASSET_PACKAGE"),
    );
    let package = Package::load(&directory).unwrap();
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
    assert_eq!(app.world.map().id, "straterust.backwater-station");
    let low = Position { x: 256, y: 1744 };
    let cliff = Position { x: 424, y: 1600 };
    let high_enemy = Position { x: 584, y: 1632 };
    let plateau = Position { x: 648, y: 1632 };
    let mut rules = app.world.rules().clone();
    rules.victory = false;
    let mut map = app.world.map().clone();
    map.mission = None;
    map.start_locations.clear();
    // Isolate one source-sized moving scout. The stationary invincible enemy
    // checks the terrain/unit distinction without combat altering the route.
    map.spawns = vec![
        Spawn {
            owner: PlayerId(0),
            unit_type: UnitTypeId(1),
            position: low,
            invincible: true,
            ..Spawn::default()
        },
        Spawn {
            owner: PlayerId(1),
            unit_type: UnitTypeId(1),
            position: high_enemy,
            invincible: true,
            ..Spawn::default()
        },
    ];
    app.world = World::new(rules, map, 42).unwrap();
    app.visuals = Visuals::new(&app.world);
    app.selected = BTreeSet::from([EntityId(1)]);
    app.camera = Camera {
        x: 680.0,
        y: 1520.0,
        viewport: None,
        zoom: 1.0,
    };
    let clear_flat_ground = |app: &App| {
        for dy in (-96..=96).step_by(32) {
            for dx in (-96..=96).step_by(32) {
                let point = Position {
                    x: low.x + dx,
                    y: low.y + dy,
                };
                assert_eq!(
                    app.world.map().height_at(point),
                    Some(0),
                    "source low-ground fixture changed"
                );
                assert_eq!(
                    app.world.terrain_visibility(PlayerId(0), point),
                    Visibility::Visible,
                    "spurious flat-ground terrain shadow at {point:?}"
                );
                assert_eq!(
                    app.world.visibility(PlayerId(0), point),
                    Visibility::Visible,
                    "spurious flat-ground object shadow at {point:?}"
                );
            }
        }
    };
    let stage = |app: &mut App, name: &str| {
        let scout = entity(app, EntityId(1));
        app.status = format!(
            "Visibility review: {name}; scout {},{}",
            scout.position.x, scout.position.y
        );
        capture(app, &format!("cliff-{name}"), [1280, 800], None);
        capture(app, &format!("cliff-{name}-tall"), [1400, 1334], None);
    };
    clear_flat_ground(&app);
    stage(&mut app, "flat");
    for (name, target) in [
        ("approach", Position { x: 352, y: 1616 }),
        ("edge", Position { x: 388, y: 1612 }),
        ("lateral", Position { x: 360, y: 1456 }),
        ("ramp-foot", Position { x: 552, y: 1424 }),
        ("ramp", Position { x: 624, y: 1428 }),
        ("ramp-top", Position { x: 688, y: 1408 }),
        ("plateau", plateau),
        ("return", low),
    ] {
        assert!(
            app.world.map().can_move(
                target,
                app.world.unit_type(UnitTypeId(1)).unwrap().footprint,
                app.world.unit_type(UnitTypeId(1)).unwrap().movement_class,
            ),
            "source review waypoint does not fit its Marine: {target:?}"
        );
        app.issue(Order::Move {
            entity: EntityId(1),
            target,
        })
        .unwrap();
        until(
            &mut app,
            |app| entity(app, EntityId(1)).position == target,
            2000,
        );
        if name == "edge" {
            assert_eq!(app.world.map().height_at(target), Some(0));
            assert_eq!(app.world.map().height_at(cliff), Some(1));
            assert_eq!(
                app.world.terrain_visibility(PlayerId(0), cliff),
                Visibility::Visible,
                "the first raised cliff face must be drawn from below"
            );
            assert!(
                !app.world.entity_visible(PlayerId(0), EntityId(2)),
                "drawing the cliff must not expose a higher-ground enemy"
            );
        }
        if name == "plateau" {
            assert_eq!(app.world.map().height_at(target), Some(1));
            assert!(
                app.world.entity_visible(PlayerId(0), EntityId(2)),
                "ascending reveals the higher-ground enemy"
            );
        }
        if name == "return" {
            clear_flat_ground(&app);
            assert_eq!(
                app.world.terrain_visibility(PlayerId(0), plateau),
                Visibility::Explored
            );
            assert_eq!(
                app.world.visibility(PlayerId(0), high_enemy),
                Visibility::Explored
            );
            assert!(!app.world.entity_visible(PlayerId(0), EntityId(2)));
        }
        if !matches!(name, "approach" | "ramp-top") {
            stage(&mut app, name);
        }
    }
}
