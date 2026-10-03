use super::*;

#[test]
fn resource_sprite_tips_target_gather_and_rally_at_all_camera_scales() {
    for zoom in [0.5, 1.0, 2.0] {
        for dpi in [1.0, 2.0] {
            let mut app = demo();
            add_resource_art(&mut app);
            let node = app.world.state().resources[0].clone();
            let click = Position {
                x: node.position.x,
                y: node.position.y - 28,
            };
            app.camera.x = f64::from(node.position.x);
            app.camera.y = f64::from(node.position.y);
            app.camera.zoom = zoom;
            let size = [1280.0, 800.0];
            let logical = app
                .camera
                .world_to_screen(f64::from(click.x), f64::from(click.y), size);
            let physical = [logical[0] * dpi, logical[1] * dpi];
            let picked = app
                .camera
                .screen_to_world([physical[0] / dpi, physical[1] / dpi], size)
                .unwrap();
            assert_eq!(picked, click);
            assert_eq!(app.resource_at(picked), Some(node.id));
            assert_eq!(
                app.resource_at(Position {
                    x: click.x + 25,
                    ..click
                }),
                None,
                "transparent canvas outside the footprint must not intercept ground clicks"
            );
            app.selected = BTreeSet::from([EntityId(1)]);
            app.contextual_order(picked).unwrap();
            assert!(
                matches!(app.recorded.last().unwrap().order, Order::RallyResource { resource, .. } if resource == node.id)
            );
            app.activate(Action::Rally).unwrap();
            app.targeting_click(picked).unwrap();
            assert!(
                matches!(app.recorded.last().unwrap().order, Order::RallyResource { resource, .. } if resource == node.id)
            );
            app.selected = BTreeSet::from([EntityId(2)]);
            app.contextual_order(picked).unwrap();
            assert!(
                matches!(app.recorded.last().unwrap().order, Order::Gather { resource, .. } if resource == node.id)
            );
            app.activate(Action::Gather).unwrap();
            app.targeting_click(picked).unwrap();
            assert!(
                matches!(app.recorded.last().unwrap().order, Order::Gather { resource, .. } if resource == node.id)
            );
        }
    }
}

#[test]
fn resource_rally_clicks_start_new_workers_gathering_and_can_be_cleared() {
    let mut app = demo();
    let mut rules = app.world.rules().clone();
    rules
        .units
        .iter_mut()
        .find(|unit| unit.id == UnitTypeId(2))
        .unwrap()
        .build_ticks = 1;
    app.world = World::new(rules, app.world.map().clone(), 42).unwrap();
    add_resource_art(&mut app);
    let resource = app.world.state().resources[0].clone();
    let click = Position {
        y: resource.position.y - 28,
        ..resource.position
    };
    app.selected = BTreeSet::from([EntityId(1)]);
    app.contextual_order(click).unwrap();
    assert!(
        matches!(app.recorded[0].order, Order::RallyResource { entity: EntityId(1), resource: id } if id == resource.id)
    );
    step(&mut app);
    assert_eq!(
        app.world.state().entities[0].rally_resource,
        Some(resource.id)
    );
    app.activate(Action::Train(UnitTypeId(2))).unwrap();
    for _ in 0..10 {
        step(&mut app);
        if app.world.state().entities.len() > 3 {
            break;
        }
    }
    let child = app
        .world
        .state()
        .entities
        .iter()
        .find(|entity| entity.id.0 > 3)
        .unwrap();
    assert_eq!(
        child.order,
        UnitOrder::Gather {
            resource: resource.id
        }
    );

    let after_training = app.world.resource_balance(PlayerId(0), "minerals");
    for _ in 0..400 {
        step(&mut app);
        if app.world.resource_balance(PlayerId(0), "minerals") > after_training {
            break;
        }
    }
    assert!(
        app.world.resource_balance(PlayerId(0), "minerals") > after_training,
        "the trained worker must approach, harvest, return and deposit"
    );

    // Explicit targeting uses the same resource intent, even in a mixed
    // box selection; the selected worker is not issued a facility order.
    app.selected.insert(EntityId(2));
    let count = app.recorded.len();
    app.activate(Action::Rally).unwrap();
    app.targeting_click(click).unwrap();
    assert_eq!(app.recorded.len(), count + 1);
    assert!(matches!(
        app.recorded.last().unwrap().order,
        Order::RallyResource { .. }
    ));
    step(&mut app);
    app.selected = BTreeSet::from([EntityId(1)]);
    let position = Position { x: 700, y: 700 };
    app.contextual_order(position).unwrap();
    step(&mut app);
    assert_eq!(app.world.state().entities[0].rally, Some(position));
    assert_eq!(app.world.state().entities[0].rally_resource, None);
}

#[test]
fn explored_resource_rally_preserves_identity_and_trained_worker_gathers() {
    let mut app = demo();
    let mut rules = app.world.rules().clone();
    for unit in &mut rules.units {
        unit.vision_range = 80;
    }
    let worker = rules
        .units
        .iter_mut()
        .find(|unit| unit.id == UnitTypeId(2))
        .unwrap();
    worker.build_ticks = 1;
    let mut map = app.world.map().clone();
    map.fog_of_war = true;
    let position = map.resources[0].position;
    map.spawns[1].position = Position {
        x: position.x - 64,
        y: position.y,
    };
    app.world = World::new(rules, map, 42).unwrap();
    assert_eq!(
        app.world.visibility(PlayerId(0), position),
        straterust_engine::sim::Visibility::Visible
    );
    app.selected = BTreeSet::from([EntityId(2)]);
    app.contextual_order(Position { x: 640, y: 192 }).unwrap();
    for _ in 0..100 {
        step(&mut app);
    }
    assert_eq!(
        app.world.visibility(PlayerId(0), position),
        straterust_engine::sim::Visibility::Explored
    );
    app.selected = BTreeSet::from([EntityId(1)]);
    app.contextual_order(position).unwrap();
    step(&mut app);
    assert_eq!(
        app.world.state().entities[0].rally_resource,
        Some(ResourceId(1))
    );
    app.activate(Action::Train(UnitTypeId(2))).unwrap();
    step(&mut app);
    let child = app.world.state().entities.last().unwrap();
    assert!(child.id.0 > 3);
    assert_eq!(
        child.order,
        UnitOrder::Gather {
            resource: ResourceId(1)
        }
    );
    let before = app.world.resource_balance(PlayerId(0), "minerals");
    for _ in 0..400 {
        step(&mut app);
        if app.world.resource_balance(PlayerId(0), "minerals") > before {
            break;
        }
    }
    assert!(app.world.resource_balance(PlayerId(0), "minerals") > before);
}

#[test]
fn hidden_resource_rally_is_only_a_position_and_nonproducers_ignore_it() {
    let mut app = demo();
    let mut map = app.world.map().clone();
    map.fog_of_war = true;
    let mut rules = app.world.rules().clone();
    for unit in &mut rules.units {
        unit.vision_range = 32;
    }
    app.world = World::new(rules.clone(), map.clone(), 42).unwrap();
    let position = app.world.state().resources[0].position;
    assert_eq!(
        app.world.visibility(PlayerId(0), position),
        straterust_engine::sim::Visibility::Unexplored
    );
    app.selected = BTreeSet::from([EntityId(1)]);
    app.contextual_order(position).unwrap();
    assert_eq!(
        app.recorded[0].order,
        Order::Rally {
            entity: EntityId(1),
            target: position
        }
    );
    app.activate(Action::Rally).unwrap();
    app.targeting_click(position).unwrap();
    assert_eq!(app.recorded[1].order, app.recorded[0].order);
    rules
        .units
        .iter_mut()
        .find(|unit| unit.id == UnitTypeId(3))
        .unwrap()
        .trains
        .clear();
    app.world = World::new(rules, map, 42).unwrap();
    app.recorded.clear();
    app.contextual_order(position).unwrap();
    assert!(
        app.recorded.is_empty(),
        "non-producing structures have no rally"
    );
}

#[test]
fn stopped_workers_can_resume_depleted_gas_through_both_targeting_paths() {
    use straterust_engine::sim::Extraction;
    let mut app = demo();
    let mut rules = app.world.rules().clone();
    rules
        .units
        .iter_mut()
        .find(|unit| unit.id == UnitTypeId(2))
        .unwrap()
        .worker
        .as_mut()
        .unwrap()
        .resource_kinds
        .push("gas".into());
    rules
        .units
        .iter_mut()
        .find(|unit| unit.id == UnitTypeId(3))
        .unwrap()
        .dropoff
        .push("gas".into());
    let mut refinery = rules
        .units
        .iter()
        .find(|unit| unit.id == UnitTypeId(4))
        .unwrap()
        .clone();
    refinery.id = UnitTypeId(6);
    refinery.build_ticks = 1;
    refinery.extracts = Some(Extraction {
        resource: "gas".into(),
        harvest_ticks: 1,
        depleted_amount: 2,
    });
    rules
        .units
        .iter_mut()
        .find(|unit| unit.id == UnitTypeId(2))
        .unwrap()
        .builds
        .push(UnitTypeId(6));
    rules.units.push(refinery);
    let mut map = app.world.map().clone();
    map.terrain = None;
    map.resources.truncate(1);
    let node = &mut map.resources[0];
    node.kind = "gas".into();
    node.amount = 1;
    node.requires_extractor = true;
    let position = node.position;
    app.world = World::new(rules, map, 42).unwrap();
    let resource = app.world.state().resources[0].id;
    app.selected = BTreeSet::from([EntityId(2)]);
    app.issue(Order::Build {
        entity: EntityId(2),
        unit_type: UnitTypeId(6),
        position,
    })
    .unwrap();
    for _ in 0..100 {
        step(&mut app);
        if app
            .world
            .state()
            .entities
            .iter()
            .any(|entity| entity.unit_type == UnitTypeId(6) && entity.construction.is_none())
        {
            break;
        }
    }
    assert!(
        app.world
            .state()
            .entities
            .iter()
            .any(|entity| entity.unit_type == UnitTypeId(6) && entity.construction.is_none())
    );
    app.contextual_order(position).unwrap();
    for _ in 0..300 {
        step(&mut app);
        if app.world.resource_balance(PlayerId(0), "gas") >= 2 {
            break;
        }
    }
    assert_eq!(app.world.state().resources[0].amount, 0);
    assert!(app.world.resource_balance(PlayerId(0), "gas") >= 2);
    for explicit in [false, true] {
        app.activate(Action::Stop).unwrap();
        step(&mut app);
        assert_eq!(app.world.state().entities[1].order, UnitOrder::Idle);
        let previous = app.world.resource_balance(PlayerId(0), "gas");
        if explicit {
            app.activate(Action::Gather).unwrap();
            app.targeting_click(position).unwrap();
        } else {
            app.contextual_order(position).unwrap();
        }
        assert_eq!(
            app.recorded.last().unwrap().order,
            Order::Gather {
                entity: EntityId(2),
                resource
            }
        );
        for _ in 0..300 {
            step(&mut app);
            if app.world.resource_balance(PlayerId(0), "gas") > previous {
                break;
            }
        }
        assert_eq!(app.world.resource_balance(PlayerId(0), "gas"), previous + 2);
    }
    app.selected = BTreeSet::from([EntityId(1)]);
    app.contextual_order(position).unwrap();
    assert_eq!(
        app.recorded.last().unwrap().order,
        Order::RallyResource {
            entity: EntityId(1),
            resource
        }
    );
    step(&mut app);
    assert_eq!(app.world.state().entities[0].rally_resource, Some(resource));
}

#[test]
fn contextual_gather_shift_queue_and_control_groups_remain_commands() {
    let mut app = demo();
    app.selected = BTreeSet::from([EntityId(2)]);
    let before = app.world.state_hash();
    app.contextual_order(Position { x: 480, y: 192 }).unwrap();
    assert!(matches!(
        app.recorded[0].order,
        Order::Gather {
            entity: EntityId(2),
            ..
        }
    ));
    app.keys.insert(KeyCode::ShiftLeft);
    app.contextual_order(Position { x: 600, y: 500 }).unwrap();
    assert!(matches!(
        app.recorded[1].order,
        Order::Queue {
            entity: EntityId(2),
            order: UnitOrder::Move { .. }
        }
    ));
    app.keys.clear();
    app.keys.insert(KeyCode::ControlLeft);
    app.bound_key(KeyCode::Digit1).unwrap();
    app.keys.clear();
    app.selected.clear();
    app.bound_key(KeyCode::Digit1).unwrap();
    assert_eq!(app.selected, BTreeSet::from([EntityId(2)]));
    assert_eq!(app.world.state_hash(), before);
    step(&mut app);
    assert!(matches!(
        app.world.state().entities[1].order,
        UnitOrder::Gather { .. }
    ));
    assert_eq!(app.world.state().entities[1].queued_orders.len(), 1);
}
