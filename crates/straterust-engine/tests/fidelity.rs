use straterust_engine::{map::Terrain, sim::*};
#[path = "fidelity/blocked_minerals.rs"]
mod blocked_minerals;
#[path = "fidelity/harvest_spots.rs"]
mod harvest_spots;
#[path = "fidelity/redistribution.rs"]
mod redistribution;

fn point(x: i32, y: i32) -> Position {
    Position { x, y }
}
fn spawn(kind: u16, x: i32, y: i32) -> Spawn {
    Spawn {
        unit_type: UnitTypeId(kind),
        position: point(x, y),
        ..Spawn::default()
    }
}
fn entity(world: &World, id: u32) -> &Entity {
    world
        .state()
        .entities
        .iter()
        .find(|entity| entity.id == EntityId(id))
        .unwrap()
}
fn send(world: &mut World, order: Order) {
    let result = world
        .step(&[Command {
            tick: world.tick(),
            player: PlayerId(0),
            sequence: world.state().last_sequences[0] + 1,
            order,
        }])
        .unwrap();
    assert!(result[0].rejection.is_none(), "{:?}", result[0].rejection);
}
fn run(world: &mut World, ticks: usize) {
    for _ in 0..ticks {
        world.step(&[]).unwrap();
    }
}
fn map(spawns: Vec<Spawn>) -> Map {
    Map {
        id: "fidelity-map".into(),
        width: 1024,
        height: 640,
        players: 1,
        spawns,
        resources: vec![],
        start_locations: vec![],
        terrain: None,
        initial_explored: Default::default(),
        creation: Default::default(),
        ai: Vec::new(),
        mission: None,
        fog_of_war: false,
    }
}
fn moving(motion: Option<Motion>) -> World {
    World::new(
        Rules {
            id: "motion-tests".into(),
            units: vec![UnitType {
                id: UnitTypeId(1),
                speed: 8,
                motion,
                ..UnitType::default()
            }],
            ..Rules::default()
        },
        map(vec![spawn(1, 64, 64)]),
        7,
    )
    .unwrap()
}
fn precise(entity: &Entity) -> [i64; 2] {
    [
        i64::from(entity.position.x) * 256 + i64::from(entity.motion_fraction[0]),
        i64::from(entity.position.y) * 256 + i64::from(entity.motion_fraction[1]),
    ]
}

#[test]
fn diagonal_and_cardinal_moves_use_the_same_scalar_speed() {
    let mut cardinal = moving(None);
    let mut diagonal = cardinal.clone();
    send(
        &mut cardinal,
        Order::Move {
            entity: EntityId(1),
            target: point(600, 64),
        },
    );
    send(
        &mut diagonal,
        Order::Move {
            entity: EntityId(1),
            target: point(600, 600),
        },
    );
    run(&mut cardinal, 11);
    run(&mut diagonal, 11);
    let a = precise(entity(&cardinal, 1));
    let b = precise(entity(&diagonal, 1));
    let cardinal_distance = a[0] - 64 * 256;
    let dx = b[0] - 64 * 256;
    let dy = b[1] - 64 * 256;
    let diagonal_distance = ((dx * dx + dy * dy) as u64).isqrt() as i64;
    assert_eq!(cardinal_distance, 12 * 8 * 256);
    assert_eq!(dx, dy);
    assert!(
        (diagonal_distance - cardinal_distance).abs() <= 24,
        "subpixel rounding must not make diagonals faster: {cardinal_distance} vs {diagonal_distance}"
    );
}

#[test]
fn fractional_acceleration_and_stride_cycle_survive_tick_boundaries() {
    let mut accelerated = moving(Some(Motion {
        speed: 384,
        acceleration: 64,
        steps: vec![],
    }));
    send(
        &mut accelerated,
        Order::Move {
            entity: EntityId(1),
            target: point(600, 64),
        },
    );
    assert_eq!(precise(entity(&accelerated, 1))[0] - 64 * 256, 64);
    run(&mut accelerated, 5);
    assert_eq!(
        precise(entity(&accelerated, 1))[0] - 64 * 256,
        64 + 128 + 192 + 256 + 320 + 384
    );
    assert_eq!(entity(&accelerated, 1).motion_speed, 384);
    run(&mut accelerated, 2);
    assert_eq!(precise(entity(&accelerated, 1))[0] - 64 * 256, 1344 + 768);

    let profile = Motion {
        speed: 3 * 256,
        acceleration: 0,
        steps: vec![0, 1, 0, 3],
    };
    let mut striding = moving(Some(profile.clone()));
    let mut replay = striding.clone();
    for world in [&mut striding, &mut replay] {
        send(
            world,
            Order::Move {
                entity: EntityId(1),
                target: point(600, 64),
            },
        );
    }
    assert_eq!(entity(&striding, 1).position.x, 64);
    for expected in [65, 65, 68, 68, 69, 69, 72] {
        striding.step(&[]).unwrap();
        replay.step(&[]).unwrap();
        assert_eq!(entity(&striding, 1).position.x, expected);
        assert_eq!(striding.state_hash(), replay.state_hash());
    }
    let mut other = profile;
    other.steps = vec![0, 0, 1, 3];
    assert_ne!(
        moving(Some(other)).state_hash(),
        moving(Some(Motion {
            speed: 3 * 256,
            acceleration: 0,
            steps: vec![0, 1, 0, 3],
        }))
        .state_hash(),
        "stride data belongs in canonical rules identity"
    );
}

#[test]
fn ordinary_move_finishes_at_nearest_reachable_edge_of_a_disconnected_target() {
    let rules = moving(None).rules().clone();
    let mut map = map(vec![spawn(1, 64, 64)]);
    let mut flags = vec![straterust_engine::map::WALKABLE; 128 * 80];
    for y in 0..80 {
        flags[y * 128 + 20] = 0;
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
        Order::Move {
            entity: EntityId(1),
            target: point(240, 64),
        },
    );
    run(&mut world, 30);
    assert_eq!(entity(&world, 1).position, point(156, 60));
    assert_eq!(entity(&world, 1).order, UnitOrder::Idle);
    assert!(entity(&world, 1).path.is_empty());
}

fn economic_rules() -> Rules {
    Rules {
        id: "resource-fidelity".into(),
        units: vec![
            UnitType {
                id: UnitTypeId(1),
                speed: 8,
                footprint: Footprint {
                    width: 2,
                    height: 2,
                },
                max_hp: 20,
                vision_range: 96,
                build_ticks: 2,
                builds: vec![UnitTypeId(4)],
                worker: Some(WorkerStats {
                    capacity: 8,
                    harvest_amount: 8,
                    harvest_ticks: 4,
                    build_rate: 1,
                    resource_kinds: vec!["ore".into(), "gas".into()],
                    idle_resource_radius: 256,
                }),
                ..UnitType::default()
            },
            UnitType {
                id: UnitTypeId(2),
                speed: 0,
                structure: true,
                max_hp: 100,
                footprint: Footprint {
                    width: 24,
                    height: 24,
                },
                placement: Footprint {
                    width: 24,
                    height: 24,
                },
                vision_range: 96,
                trains: vec![UnitTypeId(1), UnitTypeId(3)],
                dropoff: vec!["ore".into(), "gas".into()],
                ..UnitType::default()
            },
            UnitType {
                id: UnitTypeId(3),
                speed: 8,
                max_hp: 20,
                vision_range: 96,
                build_ticks: 2,
                footprint: Footprint {
                    width: 2,
                    height: 2,
                },
                ..UnitType::default()
            },
            UnitType {
                id: UnitTypeId(4),
                speed: 0,
                structure: true,
                max_hp: 100,
                footprint: Footprint {
                    width: 16,
                    height: 16,
                },
                placement: Footprint {
                    width: 16,
                    height: 16,
                },
                extracts: Some(Extraction {
                    resource: "gas".into(),
                    harvest_ticks: 2,
                    depleted_amount: 2,
                }),
                ..UnitType::default()
            },
        ],
        ..Rules::default()
    }
}
fn node(kind: &str, x: i32, y: i32, amount: u32) -> ResourceSpawn {
    ResourceSpawn {
        kind: kind.into(),
        position: point(x, y),
        amount,
        footprint: Footprint {
            width: 8,
            height: 8,
        },
        requires_extractor: false,
    }
}
fn economy() -> World {
    let mut map = map(vec![spawn(2, 40, 64), spawn(1, 78, 64)]);
    map.resources = vec![node("ore", 120, 64, 100)];
    World::new(economic_rules(), map, 7).unwrap()
}
fn train(world: &mut World, kind: u16) -> u32 {
    let id = world.state().next_entity_id;
    send(
        world,
        Order::Train {
            entity: EntityId(1),
            unit_type: UnitTypeId(kind),
        },
    );
    for _ in 0..20 {
        if world
            .state()
            .entities
            .iter()
            .any(|entity| entity.id.0 == id)
        {
            return id;
        }
        world.step(&[]).unwrap();
    }
    panic!("training failed to produce unit");
}

#[test]
fn resource_rally_assigns_workers_to_gather_and_fighters_to_approach() {
    let mut world = economy();
    let mut position_only = world.clone();
    send(
        &mut world,
        Order::RallyResource {
            entity: EntityId(1),
            resource: ResourceId(1),
        },
    );
    send(
        &mut position_only,
        Order::Rally {
            entity: EntityId(1),
            target: point(120, 64),
        },
    );
    assert_eq!(entity(&world, 1).rally, entity(&position_only, 1).rally);
    assert_ne!(
        world.state_hash(),
        position_only.state_hash(),
        "resource identity must be canonical"
    );
    let worker = train(&mut world, 1);
    assert_eq!(
        entity(&world, worker).order,
        UnitOrder::Gather {
            resource: ResourceId(1)
        }
    );
    let fighter = train(&mut world, 3);
    assert_eq!(
        entity(&world, fighter).order,
        UnitOrder::Move {
            target: point(120, 64)
        }
    );
    send(
        &mut world,
        Order::Rally {
            entity: EntityId(1),
            target: point(40, 160),
        },
    );
    assert_eq!(entity(&world, 1).rally_resource, None);
    let positional_worker = train(&mut world, 1);
    assert_eq!(
        entity(&world, positional_worker).order,
        UnitOrder::Move {
            target: point(40, 160)
        }
    );
}

#[test]
fn depletion_returns_cargo_then_selects_only_known_reachable_nearby_same_kind() {
    let mut map = map(vec![spawn(2, 40, 64), spawn(1, 78, 64), spawn(3, 40, 560)]);
    map.fog_of_war = true;
    map.resources = vec![
        node("ore", 120, 64, 1),
        node("gas", 128, 80, 20),
        node("ore", 160, 64, 20),
        node("ore", 128, 304, 20),
        node("ore", 80, 560, 20),
        node("ore", 96, 144, 1),
    ];
    let mut flags = vec![straterust_engine::map::WALKABLE; 128 * 80];
    for y in 0..80 {
        flags[y * 128 + 18] = 0;
    }
    map.terrain = Some(Terrain {
        cell_size: 8,
        columns: 128,
        rows: 80,
        flags,
    });
    let mut world = World::new(economic_rules(), map, 7).unwrap();
    assert_eq!(
        world.visibility(PlayerId(0), point(128, 304)),
        Visibility::Unexplored
    );
    assert_eq!(
        world.visibility(PlayerId(0), point(80, 560)),
        Visibility::Visible
    );
    assert_eq!(
        world.visibility(PlayerId(0), point(160, 64)),
        Visibility::Visible
    );
    send(
        &mut world,
        Order::Gather {
            entity: EntityId(2),
            resource: ResourceId(1),
        },
    );
    for _ in 0..80 {
        if world.state().resources[0].amount == 0 {
            break;
        }
        world.step(&[]).unwrap();
    }
    assert_eq!(entity(&world, 2).cargo.as_ref().unwrap().amount, 1);
    assert_eq!(
        entity(&world, 2).order,
        UnitOrder::Gather {
            resource: ResourceId(1)
        }
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
    let mut stopped = world.clone();
    send(
        &mut stopped,
        Order::Stop {
            entity: EntityId(2),
        },
    );
    run(&mut stopped, 10);
    assert_eq!(entity(&stopped, 2).order, UnitOrder::Idle);
    assert_eq!(entity(&stopped, 2).cargo.as_ref().unwrap().amount, 1);
    assert!(entity(&stopped, 2).queued_orders.is_empty());
    let mut selected_next = false;
    for _ in 0..300 {
        world.step(&[]).unwrap();
        if entity(&world, 2).order
            == (UnitOrder::Gather {
                resource: ResourceId(6),
            })
        {
            selected_next = true;
            assert!(world.resource_balance(PlayerId(0), "ore") >= 1);
            assert_eq!(entity(&world, 2).queued_orders.len(), 1);
        }
    }
    assert!(selected_next);
    assert_eq!(world.resource_balance(PlayerId(0), "ore"), 2);
    assert_eq!(
        world
            .state()
            .resources
            .iter()
            .map(|node| node.amount)
            .collect::<Vec<_>>(),
        vec![0, 20, 20, 20, 20, 0]
    );
    assert_eq!(entity(&world, 2).position, point(64, 200));
    assert_eq!(entity(&world, 2).order, UnitOrder::Idle);
    assert!(entity(&world, 2).cargo.is_none());
}

#[test]
fn mineral_access_is_fifo_and_only_one_worker_harvests_at_once() {
    let mut map = map(vec![spawn(2, 40, 64), spawn(1, 114, 64), spawn(1, 120, 58)]);
    map.resources = vec![node("ore", 120, 64, 32)];
    let mut world = World::new(economic_rules(), map, 7).unwrap();
    // Higher ID arrives first. Lower ID must wait instead of stealing the lock.
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
    assert_eq!(entity(&world, 2).harvest_progress, 0);
    assert_eq!(entity(&world, 3).harvest_progress, 2);
    let mut first_cargo = None;
    for _ in 0..120 {
        assert!(
            world
                .state()
                .entities
                .iter()
                .filter(|entity| entity.harvest_progress > 0)
                .count()
                <= 1
        );
        if first_cargo.is_none() {
            first_cargo = world
                .state()
                .entities
                .iter()
                .find(|entity| entity.cargo.is_some())
                .map(|entity| entity.id);
        }
        world.step(&[]).unwrap();
    }
    assert_eq!(first_cargo, Some(EntityId(3)));
    assert_eq!(world.resource_balance(PlayerId(0), "ore"), 32);
    assert_eq!(world.state().resources[0].amount, 0);
}

#[test]
fn depleted_gas_still_yields_two_and_does_not_retarget() {
    let mut map = map(vec![spawn(2, 40, 64), spawn(1, 110, 64)]);
    map.resources = vec![
        ResourceSpawn {
            requires_extractor: true,
            ..node("gas", 120, 64, 1)
        },
        node("gas", 180, 64, 100),
    ];
    let mut world = World::new(economic_rules(), map, 7).unwrap();
    send(
        &mut world,
        Order::Build {
            entity: EntityId(2),
            unit_type: UnitTypeId(4),
            position: point(120, 64),
        },
    );
    run(&mut world, 4);
    assert!(entity(&world, 3).construction.is_none());
    send(
        &mut world,
        Order::Gather {
            entity: EntityId(2),
            resource: ResourceId(1),
        },
    );
    for _ in 0..40 {
        if entity(&world, 2).cargo.is_some() {
            break;
        }
        world.step(&[]).unwrap();
    }
    assert_eq!(entity(&world, 2).cargo.as_ref().unwrap().amount, 2);
    run(&mut world, 80);
    assert!(world.resource_balance(PlayerId(0), "gas") >= 2);
    assert_eq!(world.resource_balance(PlayerId(0), "gas") % 2, 0);
    assert_eq!(
        entity(&world, 2).order,
        UnitOrder::Gather {
            resource: ResourceId(1)
        }
    );
    assert_eq!(world.state().resources[0].amount, 0);
    assert_eq!(world.state().resources[1].amount, 100);
}
