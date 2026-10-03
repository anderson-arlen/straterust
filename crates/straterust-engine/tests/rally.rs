use straterust_engine::sim::*;

fn point(x: i32, y: i32) -> Position {
    Position { x, y }
}

fn mobile(position: Position) -> Spawn {
    Spawn {
        owner: PlayerId(0),
        unit_type: UnitTypeId(1),
        position,
        ..Spawn::default()
    }
}

fn world(spawns: Vec<Spawn>) -> World {
    World::new(
        Rules {
            id: "original-rally-rules".into(),
            units: vec![
                UnitType {
                    id: UnitTypeId(1),
                    speed: 8,
                    footprint: Footprint {
                        width: 8,
                        height: 8,
                    },
                    max_hp: 20,
                    build_ticks: 2,
                    ..UnitType::default()
                },
                UnitType {
                    id: UnitTypeId(2),
                    speed: 0,
                    footprint: Footprint {
                        width: 24,
                        height: 24,
                    },
                    placement: Footprint {
                        width: 24,
                        height: 24,
                    },
                    structure: true,
                    max_hp: 100,
                    trains: vec![UnitTypeId(1)],
                    ..UnitType::default()
                },
            ],
            ..Rules::default()
        },
        Map {
            initial_explored: Default::default(),
            creation: Default::default(),
            ai: Vec::new(),
            mission: None,
            fog_of_war: false,
            id: "original-rally-map".into(),
            width: 320,
            height: 192,
            players: 1,
            spawns,
            start_locations: vec![],
            resources: vec![],
            terrain: None,
        },
        7,
    )
    .unwrap()
}

fn factory() -> Spawn {
    Spawn {
        owner: PlayerId(0),
        unit_type: UnitTypeId(2),
        position: point(40, 100),
        ..Spawn::default()
    }
}

fn commands(orders: Vec<Order>) -> Vec<Command> {
    orders
        .into_iter()
        .enumerate()
        .map(|(index, order)| Command {
            tick: Tick(0),
            player: PlayerId(0),
            sequence: index as u64 + 1,
            order,
        })
        .collect()
}

fn entity(world: &World, id: u32) -> &Entity {
    world
        .state()
        .entities
        .iter()
        .find(|entity| entity.id == EntityId(id))
        .unwrap()
}

fn assert_clear(world: &World) {
    for entity in &world.state().entities {
        let unit = world.unit_type(entity.unit_type).unwrap();
        assert!(
            world.can_place(
                entity.position,
                unit.footprint,
                unit.movement_class,
                Some(entity.id)
            ),
            "entity {} overlaps terrain or another entity at {:?}",
            entity.id.0,
            entity.position
        );
    }
}

#[test]
fn surrounded_unit_keeps_its_order_until_the_group_opens_a_route() {
    let mut spawns = vec![mobile(point(100, 100))];
    for y in [92, 100, 108] {
        for x in [92, 100, 108] {
            if (x, y) != (100, 100) {
                spawns.push(mobile(point(x, y)));
            }
        }
    }
    let mut a = world(spawns);
    let mut b = a.clone();
    let target = point(228, 100);
    let orders = commands(vec![Order::Move {
        entity: EntityId(1),
        target,
    }]);
    a.step(&orders).unwrap();
    b.step(&orders).unwrap();
    for _ in 0..16 {
        assert_eq!(entity(&a, 1).position, point(100, 100));
        assert_eq!(entity(&a, 1).order, UnitOrder::Move { target });
        a.step(&[]).unwrap();
        b.step(&[]).unwrap();
    }
    let orders: Vec<_> = (2..=9)
        .map(|id| Command {
            tick: a.tick(),
            player: PlayerId(0),
            sequence: u64::from(id),
            order: Order::Move {
                entity: EntityId(id),
                target: point(
                    228 + ((id - 2) % 4) as i32 * 16,
                    40 + ((id - 2) / 4) as i32 * 16,
                ),
            },
        })
        .collect();
    a.step(&orders).unwrap();
    b.step(&orders.into_iter().rev().collect::<Vec<_>>())
        .unwrap();
    for _ in 0..160 {
        a.step(&[]).unwrap();
        b.step(&[]).unwrap();
        assert_clear(&a);
        assert_eq!(a.state_hash(), b.state_hash());
    }
    assert_eq!(entity(&a, 1).position, target);
    assert_eq!(entity(&a, 1).order, UnitOrder::Idle);
}

#[test]
fn partial_route_waits_at_mobile_barrier_before_advancing_queued_move() {
    let original = world(vec![mobile(point(28, 100))]);
    let mut rules = original.rules().clone();
    let mut blocker = rules.units[0].clone();
    blocker.id = UnitTypeId(3);
    blocker.footprint.height = 64;
    rules.units.push(blocker);
    let mut map = original.map().clone();
    for y in [32, 96, 160] {
        let mut spawn = mobile(point(160, y));
        spawn.unit_type = UnitTypeId(3);
        map.spawns.push(spawn);
    }
    let mut world = World::new(rules, map, 7).unwrap();
    let target = point(228, 100);
    world
        .step(&commands(vec![
            Order::Move {
                entity: EntityId(1),
                target,
            },
            Order::Queue {
                entity: EntityId(1),
                order: UnitOrder::Move {
                    target: point(200, 120),
                },
            },
        ]))
        .unwrap();
    for _ in 0..40 {
        world.step(&[]).unwrap();
    }
    assert!(
        entity(&world, 1).position.x > 28,
        "unit never approached the barrier"
    );
    assert!(entity(&world, 1).position.x < 160);
    assert_eq!(entity(&world, 1).order, UnitOrder::Move { target });
    assert_eq!(entity(&world, 1).queued_orders.len(), 1);
    let orders: Vec<_> = (2..=4)
        .map(|id| Command {
            tick: world.tick(),
            player: PlayerId(0),
            sequence: u64::from(id) + 1,
            order: Order::Move {
                entity: EntityId(id),
                target: point(280, entity(&world, id).position.y),
            },
        })
        .collect();
    world.step(&orders).unwrap();
    for _ in 0..80 {
        world.step(&[]).unwrap();
        assert_clear(&world);
    }
    assert_eq!(entity(&world, 1).position, point(200, 120));
    assert_eq!(entity(&world, 1).order, UnitOrder::Idle);
}

#[test]
fn multiple_trained_units_approach_occupied_rally_and_settle_deterministically() {
    let rally = point(228, 100);
    let mut a = world(vec![factory(), mobile(rally)]);
    let mut b = a.clone();
    let mut orders = vec![Order::Rally {
        entity: EntityId(1),
        target: rally,
    }];
    orders.extend((0..4).map(|_| Order::Train {
        entity: EntityId(1),
        unit_type: UnitTypeId(1),
    }));
    let commands = commands(orders);
    assert!(
        a.step(&commands)
            .unwrap()
            .iter()
            .all(|outcome| outcome.rejection.is_none())
    );
    assert!(
        b.step(&commands.into_iter().rev().collect::<Vec<_>>())
            .unwrap()
            .iter()
            .all(|outcome| outcome.rejection.is_none())
    );
    for _ in 0..150 {
        a.step(&[]).unwrap();
        b.step(&[]).unwrap();
        assert_eq!(a.state_hash(), b.state_hash());
        assert_clear(&a);
    }
    assert_eq!(a.state().entities.len(), 6);
    assert!(entity(&a, 1).production.is_empty());
    assert_eq!(entity(&a, 2).position, rally);
    for id in 3..=6 {
        let unit = entity(&a, id);
        assert_eq!(unit.order, UnitOrder::Idle, "unit {id} did not settle");
        assert!(unit.path.is_empty());
        assert!(
            (unit.position.x - rally.x)
                .abs()
                .max((unit.position.y - rally.y).abs())
                <= 24,
            "unit {id} stayed near its producer: {:?}",
            unit.position
        );
        assert_ne!(unit.position, rally);
    }
}

#[test]
fn occupant_arriving_during_move_repaths_the_existing_exact_route() {
    let rally = point(228, 100);
    let mut world = world(vec![mobile(point(28, 100)), mobile(point(228, 28))]);
    let commands = commands(vec![
        Order::Move {
            entity: EntityId(1),
            target: rally,
        },
        Order::Move {
            entity: EntityId(2),
            target: rally,
        },
    ]);
    assert!(
        world
            .step(&commands)
            .unwrap()
            .iter()
            .all(|outcome| outcome.rejection.is_none())
    );
    assert_eq!(entity(&world, 1).path.back(), Some(&rally));
    for _ in 0..20 {
        if entity(&world, 2).position == rally {
            break;
        }
        world.step(&[]).unwrap();
        assert_clear(&world);
    }
    assert_eq!(entity(&world, 2).position, rally);
    assert!(entity(&world, 1).position.x < rally.x - 32);
    for _ in 0..80 {
        world.step(&[]).unwrap();
        assert_clear(&world);
    }
    assert_eq!(entity(&world, 1).position, point(220, 100));
    assert_eq!(entity(&world, 1).order, UnitOrder::Idle);
    assert_eq!(entity(&world, 2).position, rally);
}

#[test]
fn clear_rally_still_reaches_the_exact_requested_point() {
    let rally = point(229, 101);
    let mut world = world(vec![factory()]);
    let commands = commands(vec![
        Order::Rally {
            entity: EntityId(1),
            target: rally,
        },
        Order::Train {
            entity: EntityId(1),
            unit_type: UnitTypeId(1),
        },
    ]);
    assert!(
        world
            .step(&commands)
            .unwrap()
            .iter()
            .all(|outcome| outcome.rejection.is_none())
    );
    for _ in 0..80 {
        world.step(&[]).unwrap();
        assert_clear(&world);
    }
    assert_eq!(entity(&world, 2).position, rally);
    assert_eq!(entity(&world, 2).order, UnitOrder::Idle);
}

#[test]
fn crowded_off_grid_production_exit_retries_until_newborns_can_approach() {
    let mut original = world(vec![factory(), mobile(point(155, 155))]);
    let mut rules = original.rules().clone();
    rules.units[0].footprint = Footprint {
        width: 23,
        height: 23,
    };
    rules.units[0].speed = 5;
    rules.units[0].motion = Some(Motion {
        speed: 1280,
        acceleration: 67,
        steps: vec![],
    });
    rules.units[0].build_ticks = 1;
    rules.units[1].footprint = Footprint {
        width: 117,
        height: 83,
    };
    rules.units[1].placement = rules.units[1].footprint;
    let mut map = original.map().clone();
    map.height = 384;
    map.spawns[0].position = point(192, 80);
    map.spawns[1].position = point(155, 255);
    original = World::new(rules, map, 7).unwrap();
    verify_crowded_rally(original, EntityId(1), UnitTypeId(1), point(155, 255), 48);
}

fn verify_crowded_rally(
    mut a: World,
    producer: EntityId,
    kind: UnitTypeId,
    target: Position,
    distance: i32,
) {
    let first = a.state().next_entity_id;
    let mut b = a.clone();
    let mut orders = vec![Order::Rally {
        entity: producer,
        target,
    }];
    orders.extend((0..5).map(|_| Order::Train {
        entity: producer,
        unit_type: kind,
    }));
    let orders = commands(orders);
    assert!(
        a.step(&orders)
            .unwrap()
            .iter()
            .all(|outcome| outcome.rejection.is_none())
    );
    b.step(&orders).unwrap();
    for _ in 0..400 {
        a.step(&[]).unwrap();
        b.step(&[]).unwrap();
        assert_eq!(a.state_hash(), b.state_hash());
        for actor in a
            .state()
            .entities
            .iter()
            .filter(|actor| actor.id.0 >= first)
        {
            let unit = a.unit_type(actor.unit_type).unwrap();
            assert!(a.can_place(
                actor.position,
                unit.footprint,
                unit.movement_class,
                Some(actor.id)
            ));
            if actor.order == UnitOrder::Idle {
                assert!(
                    (actor.position.x - target.x)
                        .abs()
                        .max((actor.position.y - target.y).abs())
                        <= distance,
                    "newborn {} stopped at {:?}, far from occupied rally {:?}",
                    actor.id.0,
                    actor.position,
                    target
                );
            }
        }
    }
    for id in first..first + 5 {
        let actor = entity(&a, id);
        assert_eq!(
            actor.order,
            UnitOrder::Idle,
            "newborn {id} never settled: {:?}",
            actor.position
        );
    }
}

#[test]
#[ignore = "requires an imported native package via STRATERUST_ASSET_PACKAGE"]
fn imported_production_rallies_survive_crowded_exits() {
    use straterust_engine::content::Package;
    let path = std::env::var_os("STRATERUST_ASSET_PACKAGE").expect("native package path");
    let original = Package::load(std::path::Path::new(&path))
        .unwrap()
        .world(42)
        .unwrap();
    let mut rules = original.rules().clone();
    // Compress production time to exercise simultaneous exit congestion. Keep
    // actual footprints, fixed-point movement, terrain and source placements.
    for unit in &mut rules.units {
        unit.build_ticks = 1;
        unit.cost.clear();
        unit.prerequisites.clear();
        unit.supply_used = 0;
    }
    let mut map = original.map().clone();
    map.mission = None;
    map.fog_of_war = false;
    for spawn in &mut map.spawns {
        spawn.owner = PlayerId(0);
    }
    rules.victory = false;
    let world = World::new(rules, map, 42).unwrap();
    let producer = world
        .state()
        .entities
        .iter()
        .find(|e| e.position == point(192, 1616))
        .unwrap()
        .id;
    let target = point(155, 1791);
    verify_crowded_rally(world, producer, UnitTypeId(2), target, 64);
}
