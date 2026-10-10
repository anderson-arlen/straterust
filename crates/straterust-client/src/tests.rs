use super::*;

fn app(scenario: Option<Scenario>) -> App {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../content/fixtures");
    let package = Package::load(&path).unwrap();
    let seed = scenario.as_ref().map_or(42, |s| s.seed);
    let queue = scenario
        .as_ref()
        .map(CommandQueue::from_scenario)
        .transpose()
        .unwrap()
        .unwrap_or_default();
    // These component fixtures exercise controls against an explicit headless
    // model; production App/worker privacy has its own session integration test.
    let mut app = App::new(
        &package,
        Config::default(),
        Presentation::default(),
        None,
        scenario,
    )
    .unwrap();
    app.world = package.world(seed).unwrap();
    app.initial_world = app.world.clone();
    app.simulation = None;
    app.queue = queue;
    app
}

#[test]
fn campaign_advances_only_after_victory_and_load_failure_retains_the_session() {
    use straterust_engine::content::CampaignMission;
    use straterust_engine::sim::{Mission, MissionAction, MissionTrigger};
    let root = std::env::temp_dir().join(format!(
        "straterust-campaign-session-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(root.join("second")).unwrap();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/fixtures");
    for name in ["manifest.ron", "rules.ron", "presentation.ron"] {
        std::fs::copy(fixtures.join(name), root.join("second").join(name)).unwrap();
    }
    let mut app = app(None);
    let mut map = app.world.map().clone();
    map.mission = Some(Mission {
        schema_version: 1,
        player: PlayerId(0),
        rescuable_players: Vec::new(),
        rescuers: Vec::new(),
        alliances: Vec::new(),
        poll_ticks: 1,
        wait_step_ms: 50,
        locations: vec![straterust_engine::sim::MissionLocation {
            excluded_elevations: 0,
            left: 0,
            top: 0,
            right: map.width,
            bottom: map.height,
        }],
        triggers: vec![MissionTrigger {
            conditions: vec![straterust_engine::sim::MissionCondition::Elapsed {
                comparison: straterust_engine::sim::MissionComparison::AtLeast,
                milliseconds: 0,
            }],
            actions: vec![MissionAction::Victory],
        }],
    });
    app.world = World::new(app.world.rules().clone(), map, 42).unwrap();
    app.campaign = Some(CampaignSession {
        root: root.clone(),
        index: 0,
        manifest: Campaign {
            schema_version: 1,
            id: "test".into(),
            missions: vec![
                CampaignMission {
                    title: "First".into(),
                    package: "first".into(),
                },
                CampaignMission {
                    title: "Second".into(),
                    package: "second".into(),
                },
            ],
        },
    });
    assert!(!app.advance_campaign().unwrap());
    app.world.step(&[]).unwrap();
    app.world.step(&[]).unwrap();
    assert_eq!(app.world.state().winner, Some(PlayerId(0)));
    let hash = app.world.state_hash();
    assert!(app.advance_campaign().is_err());
    assert_eq!(app.world.state_hash(), hash);
    assert_eq!(app.campaign.as_ref().unwrap().index, 0);
    std::fs::copy(fixtures.join("map.ron"), root.join("second/map.ron")).unwrap();
    app.selected.insert(EntityId(1));
    app.groups[0].insert(EntityId(1));
    app.build_menu = true;
    assert!(app.advance_campaign().unwrap());
    assert_eq!(app.campaign.as_ref().unwrap().index, 1);
    assert_eq!(app.world.tick().0, 0);
    assert!(app.selected.is_empty() && app.groups[0].is_empty() && !app.build_menu);
    assert!(app.initial_scenario.is_none());
    app.restart().unwrap();
    assert_eq!(app.campaign.as_ref().unwrap().index, 1);
    assert!(!app.advance_campaign().unwrap());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn group_panel_clicks_select_and_remove_members_without_issuing_orders() {
    let mut app = app(None);
    let mut map = app.world.map().clone();
    map.spawns = (0..12)
        .map(|slot| straterust_engine::sim::Spawn {
            owner: PlayerId(0),
            unit_type: UnitTypeId(1),
            position: Position {
                x: 100 + slot * 40,
                y: 100,
            },
            ..Default::default()
        })
        .collect();
    app.world = World::new(app.world.rules().clone(), map, 0).unwrap();
    let members: BTreeSet<_> = app
        .world
        .state()
        .entities
        .iter()
        .map(|entity| entity.id)
        .collect();
    let hash = app.world.state_hash();
    for size in [[640.0, 480.0], [800.0, 600.0], [1280.0, 800.0]] {
        for slot in 0..12 {
            app.selected = members.clone();
            app.selected.insert(EntityId(999)); // Dead IDs occupy no panel slot.
            let [x, y, w, h] = controls::selection_rect(slot, size, false);
            assert!(app.select_panel([x + w / 2.0, y + h / 2.0], size));
            assert_eq!(app.selected, BTreeSet::from([EntityId(slot as u32 + 1)]));
        }
        app.selected = members.clone();
        app.keys.insert(KeyCode::ShiftLeft);
        let [x, y, w, h] = controls::selection_rect(3, size, false);
        assert!(app.select_panel([x + w / 2.0, y + h / 2.0], size));
        assert_eq!(app.selected.len(), 11);
        assert!(!app.selected.contains(&EntityId(4)));
        let [x, y, w, h] = controls::selection_rect(11, size, false);
        assert!(
            !app.select_panel([x + w / 2.0, y + h / 2.0], size),
            "empty slot must not select another member"
        );
        app.keys.clear();
    }
    assert_eq!(app.world.state_hash(), hash);
    assert!(app.recorded.is_empty());
}

#[test]
fn selection_queues_commands_and_in_memory_recording_replays() {
    let mut app = app(None);
    app.click_at(MouseButton::Left, Position { x: 1080, y: 420 })
        .unwrap();
    assert_eq!(
        app.selected,
        BTreeSet::from([EntityId(4)]),
        "visible opponents can be inspected"
    );
    app.click_at(MouseButton::Right, Position { x: 700, y: 600 })
        .unwrap();
    assert!(
        app.recorded.is_empty(),
        "inspecting opponents grants no commands"
    );
    app.click_at(MouseButton::Left, Position { x: 480, y: 470 })
        .unwrap();
    assert_eq!(app.selected, BTreeSet::from([EntityId(2)]));
    let before = app.world.state_hash();
    app.click_at(MouseButton::Right, Position { x: 700, y: 600 })
        .unwrap();
    assert_eq!(
        app.world.state_hash(),
        before,
        "input must not mutate the world"
    );
    app.world.step(&app.queue.take(app.world.tick())).unwrap();
    assert_eq!(
        app.world.state().entities[1].position,
        Position { x: 483, y: 472 }
    );
    app.issue(Order::Stop {
        entity: EntityId(2),
    })
    .unwrap();
    for _ in 1..10 {
        app.world.step(&app.queue.take(app.world.tick())).unwrap();
    }
    let scenario = Scenario {
        schema_version: 1,
        seed: 42,
        ticks: 10,
        commands: app.recorded.clone(),
    };
    let mut playback = self::app(Some(scenario));
    for _ in 0..10 {
        playback
            .world
            .step(&playback.queue.take(playback.world.tick()))
            .unwrap();
    }
    assert_eq!(app.world.state_hash(), playback.world.state_hash());
    playback
        .issue(Order::Wander {
            entity: EntityId(1),
        })
        .unwrap();
    assert!(
        playback.recorded.is_empty(),
        "playback must refuse live commands"
    );
}

#[test]
fn bad_client_configuration_is_rejected() {
    assert!(
        Config {
            zoom: f64::NAN,
            ..Config::default()
        }
        .validate()
        .is_err()
    );
    assert!(
        Config {
            frames_per_second: 0,
            ..Config::default()
        }
        .validate()
        .is_err()
    );
    assert!(
        Config {
            width: 0,
            ..Config::default()
        }
        .validate()
        .is_err()
    );
}

#[test]
fn click_and_reverse_drag_selection_respect_camera_zoom_dpi_and_owner() {
    for dpi in [1.0, 1.5, 2.0] {
        for zoom in [0.5, 1.0, 3.0] {
            let mut app = app(None);
            app.camera = Camera {
                x: 450.0,
                y: 455.0,
                viewport: None,
                zoom,
            };
            let size = [1100.0, 760.0];
            let to_logical = |world: [f64; 2]| {
                let screen = app.camera.world_to_screen(world[0], world[1], size);
                let physical = [screen[0] * dpi, screen[1] * dpi];
                [physical[0] / dpi, physical[1] / dpi]
            };
            let start = to_logical([390.0, 390.0]);
            let end = to_logical([500.0, 490.0]);
            let click = to_logical([480.0, 470.0]);
            let before = app.world.state_hash();
            app.select_screen(start, end, size, Instant::now());
            assert_eq!(app.selected, BTreeSet::from([EntityId(1), EntityId(2)]));
            app.select_screen(end, start, size, Instant::now());
            assert_eq!(app.selected, BTreeSet::from([EntityId(1), EntityId(2)]));
            app.select_screen(click, click, size, Instant::now());
            assert_eq!(app.selected, BTreeSet::from([EntityId(2)]));
            assert_eq!(app.world.state_hash(), before);
        }
    }
    let mut app = app(None);
    app.camera = Camera {
        x: 800.0,
        y: 500.0,
        viewport: None,
        zoom: 0.4,
    };
    let size = [1100.0, 760.0];
    app.select_screen(
        app.camera.world_to_screen(0.0, 0.0, size),
        app.camera.world_to_screen(1600.0, 1000.0, size),
        size,
        Instant::now(),
    );
    assert_eq!(
        app.selected,
        BTreeSet::from([EntityId(1), EntityId(2), EntityId(3)])
    );
    app.select_screen([0.0, 0.0], [200.0, 200.0], size, Instant::now());
    assert_eq!(
        app.selected.len(),
        3,
        "HUD presses must not begin map selection"
    );
}

#[test]
fn double_click_selects_nearest_visible_matching_units_up_to_twelve() {
    use straterust_engine::sim::Spawn;
    for zoom in [0.75, 1.0, 1.25] {
        let mut app = app(None);
        let mut map = app.world.map().clone();
        let template = map.spawns[0].clone();
        map.spawns = (0..16)
            .map(|index| Spawn {
                position: Position {
                    x: 760 - index * 24,
                    y: 500,
                },
                ..template.clone()
            })
            .collect();
        map.spawns.extend([
            Spawn {
                position: Position { x: 1500, y: 500 },
                ..template.clone()
            },
            Spawn {
                owner: PlayerId(1),
                position: Position { x: 400, y: 530 },
                ..template.clone()
            },
            Spawn {
                unit_type: UnitTypeId(2),
                position: Position { x: 400, y: 560 },
                ..template
            },
        ]);
        app.world = World::new(app.world.rules().clone(), map, 42).unwrap();
        app.camera = Camera {
            x: 600.0,
            y: 500.0,
            viewport: None,
            zoom,
        };
        let size = [1100.0, 760.0];
        let click = app.camera.world_to_screen(400.0, 500.0, size);
        let now = Instant::now();
        let hash = app.world.state_hash();
        app.select_screen(click, click, size, now);
        assert_eq!(app.selected, BTreeSet::from([EntityId(16)]));
        app.select_screen(click, click, size, now + Duration::from_millis(100));
        assert_eq!(app.selected, (5..=16).map(EntityId).collect());
        assert_eq!(app.world.state_hash(), hash);
        assert!(app.recorded.is_empty(), "selection is client state only");

        app.selected = BTreeSet::from([EntityId(19)]);
        app.keys.insert(KeyCode::ShiftLeft);
        app.select_screen(click, click, size, now + Duration::from_secs(1));
        app.select_screen(click, click, size, now + Duration::from_millis(1100));
        let expected: BTreeSet<_> = (6..=16).chain([19]).map(EntityId).collect();
        assert_eq!(
            app.selected, expected,
            "Shift retains earlier picks within the limit"
        );
    }
}

#[test]
fn double_click_requires_a_quick_second_click_on_the_same_unit() {
    let mut app = app(None);
    let size = [1100.0, 760.0];
    app.camera = Camera {
        x: 450.0,
        y: 455.0,
        viewport: None,
        zoom: 1.0,
    };
    let first = app.camera.world_to_screen(420.0, 420.0, size);
    let second = app.camera.world_to_screen(480.0, 470.0, size);
    let now = Instant::now();
    app.select_screen(first, first, size, now);
    app.select_screen(second, second, size, now + Duration::from_millis(100));
    assert_eq!(app.selected, BTreeSet::from([EntityId(2)]));
    app.select_screen(second, second, size, now + Duration::from_secs(1));
    assert_eq!(app.selected, BTreeSet::from([EntityId(2)]));
    app.select_screen(second, second, size, now + Duration::from_millis(1100));
    assert_eq!(app.selected, BTreeSet::from([EntityId(1), EntityId(2)]));
    app.select_screen(first, first, size, now + Duration::from_secs(2));
    app.select_screen(first, second, size, now + Duration::from_millis(2050));
    assert!(
        app.last_selection_click.is_none(),
        "drag interrupts double click"
    );
    app.select_screen(first, first, size, now + Duration::from_millis(2100));
    assert_eq!(app.selected, BTreeSet::from([EntityId(1)]));
    app.select_screen(
        [0.0, 0.0],
        [0.0, 0.0],
        size,
        now + Duration::from_millis(2150),
    );
    app.select_screen(first, first, size, now + Duration::from_millis(2200));
    assert_eq!(
        app.selected,
        BTreeSet::from([EntityId(1)]),
        "HUD interrupts double click"
    );
}

#[test]
fn double_click_keeps_a_partly_visible_clicked_unit() {
    let mut app = app(None);
    let size = [1100.0, 760.0];
    // Unit 1's center is five pixels left of the viewport, but its right
    // edge is pickable. Unit 2 is fully on screen and has the same type.
    app.camera = Camera {
        x: 975.0,
        y: 455.0,
        viewport: None,
        zoom: 1.0,
    };
    let click = [1.0, app.camera.world_to_screen(420.0, 420.0, size)[1]];
    let now = Instant::now();
    app.select_screen(click, click, size, now);
    assert_eq!(app.selected, BTreeSet::from([EntityId(1)]));
    app.select_screen(click, click, size, now + Duration::from_millis(100));
    assert_eq!(app.selected, BTreeSet::from([EntityId(1), EntityId(2)]));
}

#[test]
fn group_orders_emit_one_canonical_command_per_entity_in_stable_order() {
    let mut app = app(None);
    app.selected = BTreeSet::from([EntityId(3), EntityId(1), EntityId(2)]);
    let before = app.world.state_hash();
    app.click_at(MouseButton::Right, Position { x: 700, y: 600 })
        .unwrap();
    assert_eq!(
        before,
        app.world.state_hash(),
        "group input must not mutate simulation"
    );
    assert_eq!(app.recorded.len(), 3);
    for (index, command) in app.recorded.iter().enumerate() {
        assert_eq!(command.sequence, index as u64 + 1);
        assert!(
            matches!(command.order, Order::Move { entity, .. } if entity == EntityId(index as u32 + 1))
        );
    }
    app.world.step(&app.queue.take(app.world.tick())).unwrap();
    assert!(
        app.world.state().entities[..3]
            .iter()
            .all(|entity| entity.target == Some(Position { x: 700, y: 600 }))
    );
}
