use super::*;

#[test]
fn match_training_keys_follow_authored_unit_controls_instead_of_button_position() {
    let mut app = demo();
    let mut map = app.world.map().clone();
    map.spawns.push(straterust_engine::sim::Spawn {
        owner: PlayerId(0),
        unit_type: UnitTypeId(5),
        position: Position { x: 640, y: 512 },
        ..Default::default()
    });
    app.world = World::new(app.world.rules().clone(), map, 42).unwrap();
    app.presentation
        .train_keys
        .insert(UnitTypeId(1), "M".into());
    app.presentation
        .train_keys
        .insert(UnitTypeId(2), "S".into());
    assert_eq!(app.config.bindings.train_1, "V");
    for (entity, unit, key) in [
        (EntityId(1), UnitTypeId(2), KeyCode::KeyS),
        (EntityId(4), UnitTypeId(1), KeyCode::KeyM),
    ] {
        app.selected = BTreeSet::from([entity]);
        let button = app
            .buttons()
            .into_iter()
            .find(|b| b.action == Action::Train(unit))
            .unwrap();
        assert_eq!(parse_key(&button.key), Some(key));
        app.bound_key(key).unwrap();
        assert_eq!(
            app.recorded.last().unwrap().order,
            Order::Train {
                entity,
                unit_type: unit
            }
        );
    }
}

#[test]
fn repair_controls_validate_targets_and_shift_queue_the_same_order() {
    let mut app = damaged_base(3000);
    assert!(
        app.buttons()
            .iter()
            .any(|button| button.action == Action::Repair && button.key == "R")
    );
    app.bound_key(KeyCode::KeyR).unwrap();
    assert_eq!(app.target_mode, Some(TargetMode::Repair));
    app.targeting_click(Position { x: 1280, y: 256 }).unwrap();
    assert!(app.recorded.is_empty(), "enemy cannot be repaired");
    assert_eq!(app.target_mode, Some(TargetMode::Repair));
    app.targeting_click(Position { x: 224, y: 256 }).unwrap();
    assert!(matches!(
        app.recorded.last().unwrap().order,
        Order::Repair {
            entity: EntityId(2),
            target: EntityId(1)
        }
    ));
    step(&mut app);
    assert!(matches!(
        app.world.state().entities[1].order,
        UnitOrder::Repair {
            target: EntityId(1)
        }
    ));
    app.issue(Order::Move {
        entity: EntityId(2),
        target: Position { x: 320, y: 500 },
    })
    .unwrap();
    app.keys.insert(KeyCode::ShiftLeft);
    app.contextual_order(Position { x: 224, y: 256 }).unwrap();
    assert!(matches!(
        app.recorded.last().unwrap().order,
        Order::Queue {
            entity: EntityId(2),
            order: UnitOrder::Repair {
                target: EntityId(1)
            }
        }
    ));
    step(&mut app);
    assert!(
        app.world.state().entities[1]
            .queued_orders
            .contains(&UnitOrder::Repair {
                target: EntityId(1)
            })
    );
}

#[test]
fn campaign_controls_expose_all_builds_research_scan_and_bunker_orders() {
    use straterust_engine::sim::{
        Flight, GarrisonStats, MineLayer, MineStats, Research, ResearchEffect, ResearchId, Scanner,
        Spawn,
    };
    let mut app = demo();
    let mut rules = app.world.rules().clone();
    rules.victory = false;
    rules
        .units
        .iter_mut()
        .find(|unit| unit.id == UnitTypeId(3))
        .unwrap()
        .flight = Some(Flight {
        speed: 6,
        lift_ticks: 2,
        land_ticks: 2,
    });
    let building = rules
        .units
        .iter()
        .find(|unit| unit.id == UnitTypeId(4))
        .unwrap()
        .clone();
    for id in 6..=10 {
        let mut unit = building.clone();
        unit.id = UnitTypeId(id);
        rules.units.push(unit);
    }
    rules
        .units
        .iter_mut()
        .find(|unit| unit.id == UnitTypeId(2))
        .unwrap()
        .builds = (3..=10).map(UnitTypeId).collect();
    rules
        .units
        .iter_mut()
        .find(|unit| unit.id == UnitTypeId(6))
        .unwrap()
        .scanner = Some(Scanner {
        energy_max: 100,
        energy_initial: 100,
        energy_regeneration: 0,
        cost: 50,
        radius: 100,
        duration: 20,
    });
    rules
        .units
        .iter_mut()
        .find(|unit| unit.id == UnitTypeId(7))
        .unwrap()
        .garrison = Some(GarrisonStats {
        capacity: 4,
        passengers: vec![UnitTypeId(1), UnitTypeId(2)],
        attackers: vec![UnitTypeId(1)],
        range_bonus: 64,
        unload_ticks: 0,
    });
    let mut mine = rules
        .units
        .iter()
        .find(|unit| unit.id == UnitTypeId(1))
        .unwrap()
        .clone();
    mine.id = UnitTypeId(11);
    mine.weapon.as_mut().unwrap().splash = Some([10, 20, 30]);
    mine.weapon.as_mut().unwrap().strikes.clear();
    mine.mine = Some(MineStats {
        arm_ticks: 60,
        conceal_ticks: 4,
        reveal_ticks: 3,
        trigger_range: 50,
        chase_range: 120,
        detonation_range: 4,
    });
    rules.units.push(mine);
    rules
        .units
        .iter_mut()
        .find(|unit| unit.id == UnitTypeId(1))
        .unwrap()
        .mine_layer = Some(MineLayer {
        unit_type: UnitTypeId(11),
        initial_count: 3,
        deploy_range: 20,
    });
    rules.research.push(Research {
        id: ResearchId(1),
        facility: UnitTypeId(8),
        previous: None,
        prerequisites: Vec::new(),
        cost: vec![],
        ticks: 2,
        effect: ResearchEffect::Armor {
            units: vec![UnitTypeId(1)],
            amount: 1,
        },
    });
    let mut map = app.world.map().clone();
    for (unit_type, x, y) in [(7, 600, 500), (8, 900, 700), (6, 224, 700), (1, 530, 500)] {
        map.spawns.push(Spawn {
            owner: PlayerId(0),
            unit_type: UnitTypeId(unit_type),
            position: Position { x, y },
            ..Spawn::default()
        });
    }
    app.world = World::new(rules, map, 42).unwrap();
    app.audio.reset(&app.world);
    app.initial_world = app.world.clone();
    app.selected = BTreeSet::from([EntityId(2)]);
    app.activate(Action::BuildMenu).unwrap();
    assert_eq!(
        app.buttons()
            .iter()
            .filter(|button| matches!(button.action, Action::Build(_)))
            .count(),
        8
    );
    app.activate(Action::Back).unwrap();
    app.presentation
        .train_keys
        .insert(UnitTypeId(2), "S".into());
    app.selected = BTreeSet::from([EntityId(1)]);
    assert!(
        app.buttons()
            .iter()
            .any(|button| button.action == Action::Train(UnitTypeId(2)) && button.key == "S")
    );
    app.bound_key(KeyCode::KeyS).unwrap();
    assert!(matches!(
        app.recorded.last().unwrap().order,
        Order::Train {
            entity: EntityId(1),
            unit_type: UnitTypeId(2)
        }
    ));
    step(&mut app);
    app.activate(Action::Cancel).unwrap();
    step(&mut app);
    app.selected = BTreeSet::from([EntityId(5)]);
    assert!(app.buttons().iter().any(
        |button| button.action == Action::Research(ResearchId(1)) && button.disabled.is_none()
    ));
    app.activate(Action::Research(ResearchId(1))).unwrap();
    step(&mut app);
    step(&mut app);
    assert!(app.world.has_research(PlayerId(0), ResearchId(1)));
    app.selected = BTreeSet::from([EntityId(6)]);
    app.activate(Action::Scan).unwrap();
    app.targeting_click(Position { x: 1100, y: 800 }).unwrap();
    assert!(matches!(
        app.recorded.last().unwrap().order,
        Order::Scan {
            entity: EntityId(6),
            ..
        }
    ));
    step(&mut app);
    app.audio.observe(&app.world);
    assert_eq!(app.world.state().scans.len(), 1);
    assert!(app.audio.events.contains(&(Cue::Scan, Some(UnitTypeId(6)))));
    app.activate(Action::Scan).unwrap();
    app.targeting_click(Position { x: 1100, y: 800 }).unwrap();
    step(&mut app);
    app.audio.observe(&app.world);
    assert_eq!(
        app.audio
            .events
            .iter()
            .filter(|(cue, _)| *cue == Cue::Scan)
            .count(),
        2,
        "same-position scans each create one cue"
    );
    assert!(app.buttons().iter().any(|button| {
        button.action == Action::Scan
            && button
                .disabled
                .as_ref()
                .is_some_and(|reason| reason.contains("energy"))
    }));
    app.activate(Action::Scan).unwrap();
    assert!(app.target_mode.is_none());
    assert!(app.status.contains("energy"));
    assert!(
        !app.buttons()
            .iter()
            .any(|button| button.action == Action::Rally),
        "non-producing scanner has no rally button"
    );

    app.selected = BTreeSet::from([EntityId(7)]);
    app.keys.insert(KeyCode::ShiftLeft);
    app.contextual_order(Position { x: 600, y: 500 }).unwrap();
    assert!(matches!(
        app.recorded.last().unwrap().order,
        Order::Queue {
            order: UnitOrder::Load {
                target: EntityId(4)
            },
            ..
        }
    ));
    app.keys.clear();
    for _ in 0..40 {
        step(&mut app);
    }
    assert_eq!(
        app.world
            .state()
            .entities
            .iter()
            .find(|entity| entity.id == EntityId(7))
            .unwrap()
            .garrisoned_in,
        Some(EntityId(4))
    );
    app.selected = BTreeSet::from([EntityId(4)]);
    assert!(
        app.buttons()
            .iter()
            .any(|button| button.action == Action::Unload && button.disabled.is_none())
    );
    app.activate(Action::Unload).unwrap();
    step(&mut app);
    assert!(
        app.world
            .state()
            .entities
            .iter()
            .find(|entity| entity.id == EntityId(7))
            .unwrap()
            .garrisoned_in
            .is_none()
    );
    app.selected = BTreeSet::from([EntityId(7)]);
    app.activate(Action::PlaceMine).unwrap();
    app.keys.insert(KeyCode::ShiftLeft);
    app.targeting_click(Position { x: 530, y: 600 }).unwrap();
    assert!(matches!(
        app.recorded.last().unwrap().order,
        Order::Queue {
            order: UnitOrder::PlaceMine { .. },
            ..
        }
    ));
    app.keys.clear();
    for _ in 0..50 {
        step(&mut app);
    }
    assert_eq!(
        app.world
            .state()
            .entities
            .iter()
            .find(|entity| entity.id == EntityId(7))
            .unwrap()
            .mine_count,
        2
    );
    assert!(
        app.world
            .state()
            .entities
            .iter()
            .any(|entity| entity.unit_type == UnitTypeId(11))
    );
    app.simulation = Some(
        crate::simulation::SimulationWorker::local(app.initial_world.snapshot(), 42, None)
            .unwrap()
            .0,
    );
    app.restart().unwrap();
    app.simulation = None; // This fixture advances its explicit headless model below.
    assert!(app.world.state().scans.is_empty());
    assert!(!app.world.has_research(PlayerId(0), ResearchId(1)));
    app.selected = BTreeSet::from([EntityId(1)]);
    app.activate(Action::Lift).unwrap();
    for _ in 0..3 {
        step(&mut app);
    }
    assert!(app.world.state().entities[0].airborne);
    app.contextual_order(Position { x: 320, y: 600 }).unwrap();
    assert!(matches!(
        app.recorded.last().unwrap().order,
        Order::Move {
            entity: EntityId(1),
            ..
        }
    ));
    app.activate(Action::Land).unwrap();
    app.targeting_click(Position { x: 500, y: 700 }).unwrap();
    assert!(matches!(
        app.recorded.last().unwrap().order,
        Order::Land {
            entity: EntityId(1),
            ..
        }
    ));
    for _ in 0..130 {
        step(&mut app);
    }
    assert!(!app.world.state().entities[0].airborne);
    assert_eq!(
        app.world.state().entities[0].position,
        Position { x: 512, y: 688 }
    );
}

#[test]
fn repair_art_and_sound_follow_actual_health_gain_not_resource_stalls() {
    use crate::visual::VisualAction;
    for resources in [0, 3000] {
        let mut app = damaged_base(resources);
        app.contextual_order(Position { x: 224, y: 256 }).unwrap();
        let mut worked = false;
        for _ in 0..160 {
            let before_hp = app.world.state().entities[0].hp;
            let before_position = app.world.state().entities[1].position;
            let before_order = app.world.state().entities[1].order.clone();
            let before_progress = app.world.state().entities[1].repair_progress;
            step(&mut app);
            let hash = app.world.state_hash();
            app.visuals.update(&app.world);
            app.audio.events.clear();
            app.audio.observe(&app.world);
            assert_eq!(app.world.state_hash(), hash);
            let worker = &app.world.state().entities[1];
            let expected = matches!(worker.order, UnitOrder::Repair { .. })
                && worker.position == before_position
                && (app.world.state().entities[0].hp > before_hp
                    || (worker.order == before_order && worker.repair_progress != before_progress));
            let visual = app.visuals.get(EntityId(2)).unwrap();
            assert_eq!(visual.action == VisualAction::Work, expected);
            assert_eq!(
                app.audio.events.contains(&(Cue::Work, Some(UnitTypeId(2)))),
                expected
            );
            if expected {
                worked = true;
                assert_eq!(visual.effect_target, Some(Position { x: 224, y: 256 }));
                assert_eq!(
                    visual.facing,
                    crate::visual::facing_between(worker.position, Position { x: 224, y: 256 })
                );
            }
        }
        assert_eq!(worked, resources > 0);
        app.activate(Action::Stop).unwrap();
        step(&mut app);
        app.visuals.update(&app.world);
        app.audio.events.clear();
        app.audio.observe(&app.world);
        assert_eq!(
            app.visuals.get(EntityId(2)).unwrap().action,
            VisualAction::Idle
        );
        assert!(!app.audio.events.contains(&(Cue::Work, Some(UnitTypeId(2)))));
    }
}

#[test]
fn build_menu_explains_missing_completed_prerequisite_before_targeting() {
    let mut app = demo();
    app.selected = BTreeSet::from([EntityId(2)]);
    app.bound_key(KeyCode::KeyB).unwrap();
    assert!(app.build_menu);
    let buttons = app.buttons();
    let barracks = buttons
        .iter()
        .find(|b| b.action == Action::Build(UnitTypeId(5)))
        .unwrap();
    assert_eq!(barracks.slot, 2);
    assert_eq!(
        barracks.disabled.as_deref(),
        Some("Requires completed Supply depot")
    );
    assert!(
        barracks
            .tooltip
            .iter()
            .any(|text| text.contains("150 minerals"))
    );
    app.bound_key(KeyCode::KeyB).unwrap();
    assert!(app.target_mode.is_none());
    assert!(app.recorded.is_empty());
    assert!(app.status.contains("Requires completed Supply depot"));

    app.bound_key(KeyCode::KeyS).unwrap();
    assert_eq!(app.target_mode, Some(TargetMode::Build(UnitTypeId(4))));
    app.targeting_click(Position { x: 608, y: 320 }).unwrap();
    step(&mut app);
    app.activate(Action::BuildMenu).unwrap();
    assert!(
        app.buttons()
            .iter()
            .find(|b| b.action == Action::Build(UnitTypeId(5)))
            .unwrap()
            .disabled
            .is_some(),
        "unfinished prerequisite does not unlock the action"
    );
    for _ in 0..900 {
        if app
            .world
            .state()
            .entities
            .iter()
            .any(|e| e.unit_type == UnitTypeId(4) && e.construction.is_none())
        {
            break;
        }
        step(&mut app);
    }
    let barracks = app
        .buttons()
        .into_iter()
        .find(|b| b.action == Action::Build(UnitTypeId(5)))
        .unwrap();
    assert_eq!(
        barracks.slot, 2,
        "disabled and enabled buttons keep their locations"
    );
    assert!(barracks.disabled.is_none());
    app.bound_key(KeyCode::KeyB).unwrap();
    assert_eq!(app.target_mode, Some(TargetMode::Build(UnitTypeId(5))));
}

#[test]
fn build_cost_and_minimap_feedback_do_not_mutate_the_world() {
    let mut app = demo();
    let mut rules = app.world.rules().clone();
    rules.starting_resources[0].amount = 50;
    app.world = World::new(rules, app.world.map().clone(), 42).unwrap();
    let before = app.world.state_hash();
    app.selected = BTreeSet::from([EntityId(2)]);
    app.activate(Action::BuildMenu).unwrap();
    let buttons = app.buttons();
    let depot = buttons
        .iter()
        .find(|b| b.action == Action::Build(UnitTypeId(4)))
        .unwrap();
    assert_eq!(
        depot.disabled.as_deref(),
        Some("Need 50 more minerals (cost 100)")
    );
    for size in [[640.0, 480.0], [1100.0, 760.0], [1920.0, 1080.0]] {
        let rect = button_rect(depot.slot, size, false);
        let logical = [rect[0] + rect[2] / 2.0, rect[1] + rect[3] / 2.0];
        for scale in [1.0, 1.5, 2.0] {
            let physical = logical.map(|v| v * scale);
            assert_eq!(
                button_at(&buttons, physical.map(|v| v / scale), size, false),
                Some(depot.action)
            );
        }
        let map = [app.world.map().width, app.world.map().height];
        let rect = minimap_rect(size, map, false);
        let center = [rect[0] + rect[2] / 2.0, rect[1] + rect[3] / 2.0];
        assert_eq!(
            minimap_position(center, size, map, false),
            Some(Position {
                x: map[0] / 2,
                y: map[1] / 2
            })
        );
        assert!(minimap_position([rect[0] - 1.0, center[1]], size, map, false).is_none());
    }
    app.activate(depot.action).unwrap();
    assert!(app.target_mode.is_none() && app.recorded.is_empty());
    let rect = minimap_rect(
        app.logical_size(),
        [app.world.map().width, app.world.map().height],
        false,
    );
    assert!(app.pan_minimap([rect[0] + rect[2] * 0.75, rect[1] + rect[3] * 0.75]));
    assert_eq!(before, app.world.state_hash());
}

#[test]
fn build_place_stop_resume_and_cancel_use_authoritative_validation() {
    let mut app = demo();
    app.selected = BTreeSet::from([EntityId(2)]);
    app.activate(Action::BuildMenu).unwrap();
    assert!(
        app.buttons()
            .iter()
            .any(|button| button.action == Action::Build(UnitTypeId(4))
                && button.label.contains("Supply"))
    );
    app.config.bindings.build_2 = "F6".into();
    app.bound_key(KeyCode::F6).unwrap();
    assert_eq!(app.target_mode, Some(TargetMode::Build(UnitTypeId(4))));
    app.targeting_click(Position { x: 224, y: 256 }).unwrap();
    assert!(
        app.recorded.is_empty(),
        "occupied placement must not submit a build"
    );
    assert!(app.status.contains("Cannot build"));
    assert!(app.target_mode.is_some());
    app.targeting_click(Position { x: 608, y: 320 }).unwrap();
    assert!(app.target_mode.is_none());
    assert!(matches!(
        app.recorded.last().unwrap().order,
        Order::Build {
            unit_type: UnitTypeId(4),
            ..
        }
    ));
    step(&mut app);
    for _ in 0..500 {
        if app
            .world
            .state()
            .entities
            .iter()
            .any(|e| e.unit_type == UnitTypeId(4))
        {
            break;
        }
        step(&mut app);
    }
    let building = app
        .world
        .state()
        .entities
        .iter()
        .find(|entity| entity.unit_type == UnitTypeId(4))
        .unwrap()
        .id;
    app.activate(Action::Stop).unwrap();
    step(&mut app);
    app.contextual_order(Position { x: 608, y: 320 }).unwrap();
    assert!(
        matches!(app.recorded.last().unwrap().order,Order::Resume {building:id,..} if id==building)
    );
    app.selected = BTreeSet::from([building]);
    app.activate(Action::Cancel).unwrap();
    step(&mut app);
    assert!(
        !app.world
            .state()
            .entities
            .iter()
            .any(|entity| entity.id == building)
    );
}

#[test]
fn training_rally_combat_modes_and_restart_have_working_ui_paths() {
    let mut app = demo();
    app.selected = BTreeSet::from([EntityId(1)]);
    assert!(
        app.buttons()
            .iter()
            .any(|button| button.action == Action::Train(UnitTypeId(2))
                && button.label.contains("Worker"))
    );
    app.activate(Action::Train(UnitTypeId(2))).unwrap();
    step(&mut app);
    assert_eq!(app.world.state().entities[0].production.len(), 1);
    app.contextual_order(Position { x: 600, y: 500 }).unwrap();
    step(&mut app);
    assert_eq!(
        app.world.state().entities[0].rally,
        Some(Position { x: 600, y: 500 })
    );
    app.activate(Action::Cancel).unwrap();
    step(&mut app);
    assert!(app.world.state().entities[0].production.is_empty());
    app.selected = BTreeSet::from([EntityId(2)]);
    app.activate(Action::AttackMove).unwrap();
    app.targeting_click(Position { x: 1280, y: 256 }).unwrap();
    assert!(matches!(
        app.recorded.last().unwrap().order,
        Order::Attack {
            target: EntityId(3),
            ..
        }
    ));
    app.activate(Action::Patrol).unwrap();
    app.targeting_click(Position { x: 600, y: 500 }).unwrap();
    assert!(matches!(
        app.recorded.last().unwrap().order,
        Order::Patrol { .. }
    ));
    app.activate(Action::Hold).unwrap();
    assert!(matches!(
        app.recorded.last().unwrap().order,
        Order::Hold { .. }
    ));
    app.simulation = Some(
        crate::simulation::SimulationWorker::local(app.initial_world.snapshot(), 42, None)
            .unwrap()
            .0,
    );
    app.restart().unwrap();
    assert_eq!(app.world.state_hash(), app.initial_world.state_hash());
    assert!(app.recorded.is_empty() && app.selected.is_empty() && app.target_mode.is_none());
}

#[test]
fn build_snap_anchors_odd_and_even_footprints_to_tile_origins() {
    let cursor = Position { x: 611, y: 327 };
    for (width, height, expected) in [
        (32, 32, Position { x: 624, y: 336 }),
        (64, 64, Position { x: 608, y: 320 }),
        (96, 64, Position { x: 624, y: 320 }),
        (128, 96, Position { x: 608, y: 336 }),
    ] {
        let footprint = Footprint { width, height };
        let snapped = snap_build_position(cursor, footprint);
        assert_eq!(snapped, expected);
        let [left, top, _, _] = footprint.bounds(snapped);
        assert_eq!(left.rem_euclid(i64::from(BUILD_GRID)), 0);
        assert_eq!(top.rem_euclid(i64::from(BUILD_GRID)), 0);
        assert_eq!(snap_build_position(snapped, footprint), snapped);
    }
    let footprint = Footprint {
        width: 96,
        height: 64,
    };
    assert_eq!(
        snap_build_position(Position { x: 607, y: 320 }, footprint).x,
        592
    );
    assert_eq!(
        snap_build_position(Position { x: 608, y: 320 }, footprint).x,
        624,
        "a midpoint advances to the next tile instead of oscillating"
    );
    assert_eq!(
        snap_build_position(Position { x: -17, y: -17 }, footprint),
        Position { x: -16, y: -32 }
    );
    for coordinate in [i32::MIN, i32::MAX] {
        let _ = snap_build_position(
            Position {
                x: coordinate,
                y: coordinate,
            },
            footprint,
        );
    }
}

#[test]
fn build_preview_and_submitted_command_share_snap_at_each_zoom_and_dpi() {
    for unit_type in [UnitTypeId(3), UnitTypeId(4)] {
        for zoom in [0.5, 1.0, 2.25] {
            for dpi in [1.0, 1.5, 2.0] {
                let mut app = demo();
                app.selected.insert(EntityId(2));
                app.activate(Action::Build(unit_type)).unwrap();
                app.camera = crate::view::Camera {
                    x: 608.0,
                    y: 320.0,
                    zoom,
                };
                let size = app.logical_size();
                let cursor = Position { x: 611, y: 327 };
                let screen =
                    app.camera
                        .world_to_screen(f64::from(cursor.x), f64::from(cursor.y), size);
                let physical = screen.map(|coordinate| coordinate * dpi);
                let logical = physical.map(|coordinate| coordinate / dpi);
                app.cursor = winit::dpi::PhysicalPosition::new(logical[0], logical[1]);
                let before = app.world.state_hash();
                let (preview_type, preview_position, valid) = app.placement().unwrap();
                assert_eq!(preview_type, unit_type);
                assert!(valid);
                assert_ne!(preview_position, cursor);
                assert_eq!(before, app.world.state_hash());
                app.targeting_click(app.camera.screen_to_world(logical, size).unwrap())
                    .unwrap();
                assert!(matches!(app.recorded.last().unwrap().order,
                    Order::Build { entity:EntityId(2),unit_type:id,position }
                    if id==unit_type && position==preview_position));
                assert_eq!(
                    before,
                    app.world.state_hash(),
                    "UI only queues the snapped command"
                );
                step(&mut app);
                assert!(
                    app.world
                        .state()
                        .entities
                        .iter()
                        .any(|entity| (entity.unit_type == unit_type
                            && entity.position == preview_position)
                            || (entity.id == EntityId(2)
                                && entity.order
                                    == UnitOrder::PlaceBuilding {
                                        unit_type,
                                        target: preview_position
                                    }))
                );
            }
        }
    }
}

#[test]
fn snapped_builds_at_map_edges_are_validated_without_relocation() {
    for (cursor, valid) in [
        (Position { x: 47, y: 31 }, true),
        (Position { x: 1490, y: 930 }, true),
        (Position { x: 0, y: 0 }, false),
        (Position { x: 1536, y: 960 }, false),
    ] {
        let mut app = demo();
        app.selected.insert(EntityId(2));
        app.activate(Action::Build(UnitTypeId(4))).unwrap();
        let footprint = app.world.unit_type(UnitTypeId(4)).unwrap().placement;
        let snapped = snap_build_position(cursor, footprint);
        assert_eq!(
            app.world.map().contains_footprint(snapped, footprint),
            valid
        );
        assert_eq!(
            app.world
                .build_rejection(PlayerId(0), EntityId(2), UnitTypeId(4), snapped)
                .is_none(),
            valid
        );
        app.targeting_click(cursor).unwrap();
        if valid {
            assert!(
                matches!(app.recorded.last().unwrap().order,Order::Build { position,.. } if position==snapped)
            );
        } else {
            assert!(app.recorded.is_empty());
            assert!(
                app.target_mode.is_some(),
                "invalid edge placement remains in targeting mode"
            );
        }
    }
}

#[test]
fn placement_ghost_and_original_obstacles_render_without_changing_the_world() {
    let mut app = demo();
    app.selected = BTreeSet::from([EntityId(2)]);
    app.activate(Action::Build(UnitTypeId(4))).unwrap();
    let size = app.logical_size();
    for (position, valid, color) in [
        (Position { x: 608, y: 320 }, true, 0x9ee878),
        (Position { x: 224, y: 256 }, false, 0xff7777),
    ] {
        let screen = app
            .camera
            .world_to_screen(f64::from(position.x), f64::from(position.y), size);
        app.cursor = winit::dpi::PhysicalPosition::new(screen[0], screen[1]);
        let (_, position, preview_valid) = app.placement().unwrap();
        assert_eq!(preview_valid, valid);
        let before = app.world.state_hash();
        let buttons = app.buttons();
        let view = crate::view::View {
            world: &app.world,
            visuals: &app.visuals,
            cursor: [-1.0, -1.0],
            targeting: false,
            presentation: &app.presentation,
            assets: None,
            map_art: None,
            media: None,
            speaking: None,
            mission: None,
            animation_ms: 0,
            portrait_ms: 0,
            camera: app.camera,
            selected: &app.selected,
            selected_resource: app.selected_resource,
            drag_box: None,
            paused: false,
            playback: false,
            status: &app.status,
            buttons: &buttons,
            help: "Synthetic rendering check",
            placement: app.placement(),
            placement_type: None,
            ending_hint: "F5 RESTART",
        };
        let mut pixels = vec![0; 1100 * 760];
        view.draw(&mut pixels, 1100, 760, 1.0);
        let corner = app.camera.world_to_screen(
            f64::from(position.x) - 48.0,
            f64::from(position.y) - 32.0,
            size,
        );
        assert_eq!(
            pixels[corner[1] as usize * 1100 + corner[0] as usize],
            color
        );
        let rock = app.camera.world_to_screen(760.0, 200.0, size);
        assert_eq!(
            pixels[rock[1] as usize * 1100 + rock[0] as usize],
            0x39464d,
            "the authored impassable ridge must be visible"
        );
        assert_eq!(app.world.state_hash(), before);
    }
}

#[test]
fn cloak_button_toggles_secondary_order_and_requires_energy() {
    let mut app = demo();
    let mut rules = app.world.rules().clone();
    let id = app.world.state().entities[1].unit_type;
    rules.units.iter_mut().find(|u| u.id == id).unwrap().cloak =
        Some(straterust_engine::sim::Cloak {
            energy_max: 250,
            activation_cost: 25,
            regeneration: 8,
            drain: 10,
            ..Default::default()
        });
    app.world = World::new(rules, app.world.map().clone(), 42).unwrap();
    app.selected = BTreeSet::from([EntityId(2)]);
    app.presentation.command_buttons = ron::from_str(&format!(
        r#"{{
        "cloak.{}.on":(slot:7,key:"C",label:"Cloak",tip:"Conceal",icon:"cloak"),
        "cloak.{}.off":(slot:7,key:"D",label:"Decloak",tip:"Reveal",icon:"decloak"),
    }}"#,
        id.0, id.0
    ))
    .unwrap();
    assert!(
        app.buttons()
            .iter()
            .any(|b| b.action == Action::Cloak(true) && b.disabled.is_none())
    );
    app.activate(Action::Cloak(true)).unwrap();
    assert!(matches!(
        app.recorded.last().unwrap().order,
        Order::Cloak { enabled: true, .. }
    ));
    step(&mut app);
    assert!(app.world.state().entities[1].cloaked);
    assert!(
        app.buttons()
            .iter()
            .any(|b| b.action == Action::Cloak(false))
    );
    app.activate(Action::Cloak(false)).unwrap();
    step(&mut app);
    assert!(!app.world.state().entities[1].cloaked);
}
