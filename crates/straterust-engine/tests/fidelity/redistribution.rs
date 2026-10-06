use super::*;

fn rule_set() -> Rules {
    let mut rules = economic_rules();
    rules.units[0].phases_while_gathering = true;
    let mut slow_worker = rules.units[0].clone();
    slow_worker.id = UnitTypeId(5);
    slow_worker.worker.as_mut().unwrap().harvest_ticks = 10_000;
    rules.units.push(slow_worker);
    rules
}

fn gathering(world: &World, worker: u32) -> ResourceId {
    let UnitOrder::Gather { resource } = entity(world, worker).order else {
        panic!("lost gather intent");
    };
    resource
}

fn until(world: &mut World, condition: impl Fn(&World) -> bool) {
    for _ in 0..400 {
        if condition(world) {
            return;
        }
        world.step(&[]).unwrap();
    }
    panic!(
        "condition not met; workers: {:?}",
        world
            .state()
            .entities
            .iter()
            .filter(|e| e.unit_type == UnitTypeId(1))
            .map(|e| (e.position, e.order.clone(), e.cargo.clone()))
            .collect::<Vec<_>>()
    );
}

#[test]
fn group_gather_claims_distinct_patches_before_arrival_and_restores_deterministically() {
    let mut map = map(vec![
        spawn(2, 64, 240),
        spawn(1, 160, 200),
        spawn(1, 160, 224),
        spawn(1, 160, 248),
        spawn(1, 160, 272),
    ]);
    map.resources = vec![
        node("ore", 320, 240, 1000),
        node("ore", 320, 304, 1000),
        node("ore", 384, 240, 1000),
        node("ore", 400, 304, 1000),
    ];
    let mut world = World::new(rule_set(), map, 7).unwrap();
    let mut reordered = world.clone();
    let commands: Vec<_> = (2..=5)
        .map(|worker| Command {
            tick: Tick(0),
            player: PlayerId(0),
            sequence: u64::from(worker - 1),
            order: Order::Gather {
                entity: EntityId(worker),
                resource: ResourceId(1),
            },
        })
        .collect();
    world.step(&commands).unwrap();
    reordered
        .step(&commands.into_iter().rev().collect::<Vec<_>>())
        .unwrap();
    assert_eq!(world.state_hash(), reordered.state_hash());
    let mut targets: Vec<_> = (2..=5).map(|worker| gathering(&world, worker)).collect();
    targets.sort();
    assert_eq!(
        targets,
        vec![ResourceId(1), ResourceId(2), ResourceId(3), ResourceId(4)]
    );
    assert!((2..=5).all(|worker| entity(&world, worker).gather_origin == Some(point(320, 240))));
    assert!((2..=5).all(|worker| entity(&world, worker).harvest_spot.is_some()));
    let snapshot = ron::ser::to_string(&world.save_snapshot().unwrap()).unwrap();
    let mut restored = world
        .restore_snapshot(ron::from_str(&snapshot).unwrap())
        .unwrap();
    for _ in 0..160 {
        world.step(&[]).unwrap();
        restored.step(&[]).unwrap();
        assert_eq!(world.state_hash(), restored.state_hash());
    }
    assert!(
        world
            .player_view(PlayerId(0))
            .unwrap()
            .entities
            .iter()
            .all(|e| { !matches!(e, ViewedEntity::Owned(own) if own.gather_origin.is_some()) })
    );
}

#[test]
fn every_deposit_rechecks_nearest_free_patch_and_preserves_the_ordered_anchor() {
    let mut map = map(vec![
        spawn(2, 64, 240),
        spawn(1, 304, 240),
        spawn(5, 224, 280),
    ]);
    map.resources = vec![
        node("ore", 320, 240, 1000),
        node("ore", 224, 240, 1000),
        node("ore", 224, 304, 1000),
    ];
    let mut world = World::new(rule_set(), map, 7).unwrap();
    send(
        &mut world,
        Order::Gather {
            entity: EntityId(2),
            resource: ResourceId(1),
        },
    );
    assert_eq!(
        gathering(&world, 2),
        ResourceId(1),
        "the first leg honors the explicit order when free"
    );
    until(&mut world, |w| w.resource_balance(PlayerId(0), "ore") >= 8);
    assert_eq!(
        gathering(&world, 2),
        ResourceId(2),
        "choose the nearest free patch on return even if the previous one is free"
    );
    assert_eq!(entity(&world, 2).gather_origin, Some(point(320, 240)));
    until(&mut world, |w| entity(w, 2).cargo.is_some());
    send(
        &mut world,
        Order::Gather {
            entity: EntityId(3),
            resource: ResourceId(2),
        },
    );
    until(&mut world, |w| w.resource_balance(PlayerId(0), "ore") >= 16);
    assert_eq!(
        gathering(&world, 2),
        ResourceId(3),
        "an inbound worker claims the closer patch"
    );
    assert_eq!(entity(&world, 2).gather_origin, Some(point(320, 240)));
    send(
        &mut world,
        Order::Gather {
            entity: EntityId(2),
            resource: ResourceId(3),
        },
    );
    assert_eq!(
        entity(&world, 2).gather_origin,
        Some(point(224, 304)),
        "a new user order replaces the anchor"
    );
}

#[test]
fn redistribution_radius_is_configurable_and_cannot_drift_between_patch_switches() {
    for (radius, expected) in [
        (256, ResourceId(2)),
        (64, ResourceId(1)),
        (0, ResourceId(1)),
    ] {
        let mut rules = rule_set();
        rules.units[0].worker.as_mut().unwrap().idle_resource_radius = radius;
        let mut map = map(vec![spawn(2, 64, 240), spawn(1, 384, 240)]);
        map.resources = vec![
            node("ore", 400, 240, 1000),
            node("ore", 320, 240, 1000),
            node("ore", 100, 240, 1000),
        ];
        let mut world = World::new(rules, map, 7).unwrap();
        send(
            &mut world,
            Order::Gather {
                entity: EntityId(2),
                resource: ResourceId(1),
            },
        );
        until(&mut world, |w| w.resource_balance(PlayerId(0), "ore") >= 24);
        assert_eq!(gathering(&world, 2), expected);
        assert_eq!(entity(&world, 2).gather_origin, Some(point(400, 240)));
        assert_eq!(
            world.state().resources[2].amount,
            1000,
            "outside the original radius, though close to the replacement patch"
        );
    }
    let mut map = map(vec![spawn(2, 64, 240), spawn(1, 384, 240)]);
    map.resources = vec![
        node("ore", 400, 240, 8),
        node("ore", 320, 240, 8),
        node("ore", 100, 240, 1000),
    ];
    let mut world = World::new(rule_set(), map, 7).unwrap();
    send(
        &mut world,
        Order::Gather {
            entity: EntityId(2),
            resource: ResourceId(1),
        },
    );
    until(&mut world, |w| entity(w, 2).order == UnitOrder::Idle);
    assert_eq!(world.resource_balance(PlayerId(0), "ore"), 16);
    assert_eq!(
        world.state().resources[2].amount,
        1000,
        "depletion fallback also respects the original area"
    );
}

#[test]
fn unavailable_nearest_patch_is_skipped_for_a_reachable_patch_of_the_same_kind() {
    let mut rules = rule_set();
    rules.units[0].worker.as_mut().unwrap().idle_resource_radius = 384;
    let mut map = map(vec![
        spawn(2, 64, 64),
        spawn(1, 128, 120),
        spawn(5, 394, 120),
    ]);
    map.resources = vec![
        node("ore", 400, 120, 1000),
        node("ore", 200, 120, 1000),
        node("ore", 144, 300, 1000),
        node("gas", 128, 160, 1000),
    ];
    let mut flags = vec![straterust_engine::map::WALKABLE; 128 * 80];
    for row in 0..80 {
        flags[row * 128 + 21] = 0;
    }
    map.terrain = Some(Terrain {
        cell_size: 8,
        columns: 128,
        rows: 80,
        flags,
    });
    let mut world = World::new(rules, map, 7).unwrap();
    send(
        &mut world,
        Order::Gather {
            entity: EntityId(3),
            resource: ResourceId(1),
        },
    );
    send(
        &mut world,
        Order::Gather {
            entity: EntityId(2),
            resource: ResourceId(1),
        },
    );
    assert_eq!(gathering(&world, 2), ResourceId(3));
    until(&mut world, |w| w.resource_balance(PlayerId(0), "ore") >= 8);
    assert_eq!(world.state().resources[1].amount, 1000);
    assert_eq!(world.state().resources[3].amount, 1000);
}

#[test]
fn redistribution_never_uses_an_unexplored_patch() {
    let mut rules = rule_set();
    for unit in &mut rules.units {
        unit.vision_range = 0;
    }
    rules.units[2].vision_range = 16;
    let mut map = map(vec![
        spawn(2, 32, 32),
        spawn(1, 120, 64),
        spawn(5, 314, 64),
        spawn(3, 112, 160),
    ]);
    map.fog_of_war = true;
    map.resources = vec![
        node("ore", 320, 64, 1000),
        node("ore", 192, 64, 1000),
        node("ore", 120, 160, 1000),
    ];
    let mut world = World::new(rules, map, 7).unwrap();
    assert_eq!(
        world.visibility(PlayerId(0), point(192, 64)),
        Visibility::Unexplored
    );
    assert_eq!(
        world.visibility(PlayerId(0), point(120, 160)),
        Visibility::Visible
    );
    send(
        &mut world,
        Order::Gather {
            entity: EntityId(3),
            resource: ResourceId(1),
        },
    );
    send(
        &mut world,
        Order::Gather {
            entity: EntityId(2),
            resource: ResourceId(1),
        },
    );
    assert_eq!(gathering(&world, 2), ResourceId(3));
}

#[test]
fn busy_patches_keep_the_queue_until_a_free_alternative_opens_without_losing_queued_orders() {
    let mut map = map(vec![
        spawn(2, 64, 240),
        spawn(1, 160, 240),
        spawn(5, 314, 240),
        spawn(5, 314, 304),
    ]);
    map.resources = vec![node("ore", 320, 240, 1000), node("ore", 320, 304, 1000)];
    let mut world = World::new(rule_set(), map, 7).unwrap();
    send(
        &mut world,
        Order::Gather {
            entity: EntityId(3),
            resource: ResourceId(1),
        },
    );
    send(
        &mut world,
        Order::Gather {
            entity: EntityId(4),
            resource: ResourceId(2),
        },
    );
    send(
        &mut world,
        Order::Gather {
            entity: EntityId(2),
            resource: ResourceId(1),
        },
    );
    send(
        &mut world,
        Order::Queue {
            entity: EntityId(2),
            order: UnitOrder::Move {
                target: point(64, 200),
            },
        },
    );
    until(&mut world, |w| entity(w, 2).harvest_waiting_since.is_some());
    assert_eq!(gathering(&world, 2), ResourceId(1));
    assert_eq!(entity(&world, 2).harvest_progress, 0);
    send(
        &mut world,
        Order::Stop {
            entity: EntityId(4),
        },
    );
    until(&mut world, |w| gathering(w, 2) == ResourceId(2));
    assert_eq!(entity(&world, 2).gather_origin, Some(point(320, 240)));
    assert_eq!(entity(&world, 2).queued_orders.len(), 1);
}
