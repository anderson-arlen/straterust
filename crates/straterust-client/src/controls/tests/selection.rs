use super::*;
use std::time::Instant;

#[test]
fn drag_ignores_buildings_and_small_mouse_jitter_still_clicks_them() {
    let mut app = demo();
    let size = [1280.0, 800.0];
    app.camera.x = 256.0;
    app.camera.y = 304.0;
    app.camera.zoom = 1.0;
    let screen = |app: &App, x, y| app.camera.world_to_screen(x, y, size);
    let start = screen(&app, 160.0, 180.0);
    let end = screen(&app, 360.0, 410.0);
    app.select_screen(start, end, size, Instant::now());
    assert_eq!(app.selected, BTreeSet::from([EntityId(2)]));
    let start = screen(&app, 224.0, 256.0);
    app.select_screen(
        start,
        [start[0] + 2.0, start[1] + 2.0],
        size,
        Instant::now(),
    );
    assert_eq!(app.selected, BTreeSet::from([EntityId(1)]));
    app.keys.insert(KeyCode::ShiftLeft);
    let start = screen(&app, 320.0, 256.0);
    app.select_screen(start, start, size, Instant::now());
    assert_eq!(app.selected, BTreeSet::from([EntityId(2)]));
}

#[test]
fn passenger_panel_click_unloads_one_and_preserves_container_selection() {
    use straterust_engine::sim::{GarrisonStats, Spawn};
    let mut app = demo();
    let mut rules = app.world.rules().clone();
    rules.victory = false;
    rules
        .units
        .iter_mut()
        .find(|unit| unit.id == UnitTypeId(3))
        .unwrap()
        .garrison = Some(GarrisonStats {
        capacity: 4,
        passengers: vec![UnitTypeId(1), UnitTypeId(2)],
        attackers: vec![],
        range_bonus: 0,
    });
    let mut map = app.world.map().clone();
    map.spawns.retain(|spawn| spawn.owner == PlayerId(0));
    map.spawns.push(Spawn {
        unit_type: UnitTypeId(1),
        position: Position { x: 320, y: 300 },
        ..Default::default()
    });
    app.world = World::new(rules, map, 42).unwrap();
    for passenger in [EntityId(2), EntityId(3)] {
        app.issue(Order::Load {
            entity: passenger,
            target: EntityId(1),
        })
        .unwrap();
    }
    for _ in 0..100 {
        step(&mut app);
    }
    app.selected = BTreeSet::from([EntityId(1)]);
    assert_eq!(
        crate::selection::panel_members(&app.world, &app.selected)
            .1
            .len(),
        2
    );
    let size = [1280.0, 800.0];
    for passenger in [EntityId(2), EntityId(3)] {
        let [x, y, w, h] = selection_rect(0, size, false);
        assert!(app.select_panel([x + w / 2.0, y + h / 2.0], size));
        step(&mut app);
        assert_eq!(app.selected, BTreeSet::from([EntityId(1)]));
        assert_eq!(
            app.world
                .state()
                .entities
                .iter()
                .find(|entity| entity.id == passenger)
                .unwrap()
                .garrisoned_in,
            None
        );
    }
    assert!(
        crate::selection::panel_members(&app.world, &app.selected)
            .1
            .is_empty()
    );
}

#[test]
fn fog_hides_enemy_picking_and_contextual_attacks() {
    let mut app = demo();
    let mut map = app.world.map().clone();
    map.fog_of_war = true;
    app.world = World::new(app.world.rules().clone(), map, 42).unwrap();
    assert_eq!(app.entity_at(Position { x: 1280, y: 256 }), None);
    app.selected = BTreeSet::from([EntityId(2)]);
    app.contextual_order(Position { x: 1280, y: 256 }).unwrap();
    assert!(
        matches!(app.recorded.last().unwrap().order, Order::Move { .. }),
        "unseen enemy must not become an attack target"
    );
}

#[test]
fn native_console_apertures_and_input_match_at_narrow_wide_and_hidpi_sizes() {
    let buttons: Vec<_> = (0..9)
        .map(|slot| Button {
            action: Action::Move,
            slot,
            label: "Move".into(),
            key: "M".into(),
            tooltip: vec![],
            disabled: None,
        })
        .collect();
    let map = [1536, 960];
    for size in [[640.0, 480.0], [1280.0, 800.0], [1920.0, 1080.0]] {
        let scale = native_ui_scale(size);
        assert!(
            size[1] - 186.0 * scale >= size[1] - FOOTER - 0.001,
            "source trim never enters interactive terrain"
        );
        for button in &buttons {
            let [x, y, w, h] = button_rect(button.slot, size, true);
            assert!(x >= 0.0 && y >= size[1] - FOOTER && x + w <= size[0] && y + h <= size[1]);
            for dpi in [1.0, 1.5, 2.0] {
                let physical = [(x + w / 2.0) * dpi, (y + h / 2.0) * dpi];
                assert_eq!(
                    button_at(&buttons, physical.map(|point| point / dpi), size, true),
                    Some(Action::Move)
                );
            }
        }
        let [x, y, w, h] = minimap_rect(size, map, true);
        assert!(x >= 6.0 * scale && y >= size[1] - 132.0 * scale);
        assert!(x + w <= 134.0 * scale + 0.001 && y + h <= size[1] - 4.0 * scale + 0.001);
        let center = minimap_position([x + w / 2.0, y + h / 2.0], size, map, true).unwrap();
        assert!((center.x - 768).abs() <= 1 && (center.y - 480).abs() <= 1);
    }
}

#[test]
fn configurable_bindings_reject_collisions_and_reserved_input() {
    let mut bindings = Bindings::default();
    bindings.validate().unwrap();
    bindings.build_1 = "S".into();
    assert!(bindings.validate().is_err());
    bindings.build_1 = "Escape".into();
    assert!(bindings.validate().is_err());
    bindings.build_1 = "Tab".into();
    assert!(
        bindings.validate().is_err(),
        "Tab is reserved for cycling owned units"
    );
    bindings.build_1 = "F6".into();
    bindings.validate().unwrap();
}

#[test]
fn tab_cycles_current_owned_units_wraps_and_ignores_stale_selection() {
    let mut app = demo();
    let before = app.world.state_hash();
    for expected in [EntityId(1), EntityId(2), EntityId(1)] {
        app.bound_key(KeyCode::Tab).unwrap();
        assert_eq!(app.selected, BTreeSet::from([expected]));
        let entity = app
            .world
            .state()
            .entities
            .iter()
            .find(|entity| entity.id == expected)
            .unwrap();
        let screen = app.camera.world_to_screen(
            f64::from(entity.position.x),
            f64::from(entity.position.y),
            app.logical_size(),
        );
        assert!(
            app.camera
                .screen_to_world(screen, app.logical_size())
                .is_some()
        );
    }
    app.selected = BTreeSet::from([EntityId(9999)]);
    app.bound_key(KeyCode::Tab).unwrap();
    assert_eq!(app.selected, BTreeSet::from([EntityId(1)]));
    assert!(app.recorded.is_empty());
    assert_eq!(before, app.world.state_hash());
}

#[test]
fn mixed_selection_only_sends_work_and_move_orders_to_eligible_units() {
    let mut app = demo();
    app.selected = BTreeSet::from([EntityId(1), EntityId(2)]);
    app.activate(Action::Move).unwrap();
    app.targeting_click(Position { x: 600, y: 500 }).unwrap();
    assert_eq!(app.recorded.len(), 1);
    assert!(matches!(
        app.recorded[0].order,
        Order::Move {
            entity: EntityId(2),
            ..
        }
    ));
    app.activate(Action::Gather).unwrap();
    app.targeting_click(Position { x: 480, y: 192 }).unwrap();
    assert_eq!(app.recorded.len(), 2);
    assert!(matches!(
        app.recorded[1].order,
        Order::Gather {
            entity: EntityId(2),
            ..
        }
    ));
}

#[test]
fn hidden_terrain_does_not_change_move_click_feedback() {
    let target = Position { x: 800, y: 700 };
    for blocked in [false, true] {
        for action in [
            None,
            Some(Action::Move),
            Some(Action::AttackMove),
            Some(Action::Patrol),
        ] {
            let mut app = demo();
            let mut map = app.world.map().clone();
            map.fog_of_war = true;
            let terrain = map.terrain.as_mut().unwrap();
            let index = (target.y as u32 / terrain.cell_size * terrain.columns
                + target.x as u32 / terrain.cell_size) as usize;
            terrain.flags[index] = if blocked {
                0
            } else {
                straterust_engine::map::WALKABLE
            };
            let mut rules = app.world.rules().clone();
            for unit in &mut rules.units {
                unit.vision_range = 32;
            }
            app.world = World::new(rules, map, 42).unwrap();
            app.selected = BTreeSet::from([EntityId(2)]);
            assert_eq!(
                app.world.visibility(PlayerId(0), target),
                straterust_engine::sim::Visibility::Unexplored
            );
            let before = app.world.state_hash();
            if let Some(action) = action {
                app.activate(action).unwrap();
                app.targeting_click(target).unwrap();
            } else {
                app.contextual_order(target).unwrap();
            }
            assert_eq!(app.status, "Order queued.");
            assert_eq!(app.recorded.len(), 1);
            assert_eq!(app.world.state_hash(), before);
            step(&mut app);
            let actor = &app.world.state().entities[1];
            assert!(matches!(actor.order,
                UnitOrder::Move { target: position }
                | UnitOrder::AttackMove { target: position }
                | UnitOrder::Patrol { target: position } if position == target));
        }
    }
}
