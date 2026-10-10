//! Original repair fixtures. Damage is applied by normal enemy attacks; tests
//! never mutate authoritative entities, health, balances, or repair credit.
use straterust_engine::sim::*;

fn point(x: i32, y: i32) -> Position {
    Position { x, y }
}
fn footprint(size: u16) -> Footprint {
    Footprint {
        width: size,
        height: size,
    }
}
fn amount(kind: &str, amount: u32) -> ResourceAmount {
    ResourceAmount {
        kind: kind.into(),
        amount,
    }
}
fn spawn(player: u16, unit: u16, x: i32, y: i32) -> Spawn {
    Spawn {
        owner: PlayerId(player),
        unit_type: UnitTypeId(unit),
        position: point(x, y),
        ..Spawn::default()
    }
}

fn definitions(ore: u32, gas: u32) -> (Rules, Map) {
    let rules = Rules {
        id: "original-repair-rules".into(),
        starting_resources: vec![amount("ore", ore), amount("gas", gas)],
        repair: Some(RepairRules {
            rate_numerator: 9,
            rate_denominator: 10,
            cost_divisor: 3,
            range: 5,
        }),
        units: vec![
            UnitType {
                id: UnitTypeId(1),
                speed: 8,
                footprint: footprint(4),
                max_hp: 40,
                cost: vec![amount("ore", 30)],
                build_ticks: 100,
                repairs: vec![UnitTypeId(1), UnitTypeId(2), UnitTypeId(3)],
                builds: vec![UnitTypeId(3)],
                worker: Some(WorkerStats {
                    capacity: 6,
                    harvest_amount: 6,
                    harvest_ticks: 2,
                    build_rate: 1,
                    resource_kinds: vec!["ore".into()],
                    idle_resource_radius: 256,
                }),
                ..UnitType::default()
            },
            UnitType {
                id: UnitTypeId(2),
                speed: 4,
                footprint: footprint(8),
                max_hp: 100,
                cost: vec![amount("ore", 100), amount("gas", 20)],
                build_ticks: 200,
                ..UnitType::default()
            },
            UnitType {
                id: UnitTypeId(3),
                speed: 0,
                footprint: footprint(16),
                placement: footprint(16),
                structure: true,
                max_hp: 100,
                cost: vec![amount("ore", 100), amount("gas", 20)],
                build_ticks: 200,
                dropoff: vec!["ore".into()],
                ..UnitType::default()
            },
            UnitType {
                id: UnitTypeId(4),
                speed: 4,
                footprint: footprint(8),
                max_hp: 100,
                ..UnitType::default()
            },
            UnitType {
                id: UnitTypeId(5),
                speed: 0,
                max_hp: 100,
                weapon: Some(Weapon {
                    friendly_splash: false,
                    projectile_speed: 0,
                    cooldown_jitter: None,
                    targets_air: false,
                    target_classes: Vec::new(),
                    damage_kind: Default::default(),
                    splash: None,
                    strikes: Vec::new(),
                    damage: 13,
                    range: 1024,
                    cooldown: 1_000_000,
                }),
                ..UnitType::default()
            },
            UnitType {
                id: UnitTypeId(6),
                speed: 16,
                max_hp: 100,
                weapon: Some(Weapon {
                    friendly_splash: false,
                    projectile_speed: 0,
                    cooldown_jitter: None,
                    targets_air: false,
                    target_classes: Vec::new(),
                    damage_kind: Default::default(),
                    splash: None,
                    strikes: Vec::new(),
                    damage: 1000,
                    range: 1,
                    cooldown: 1_000_000,
                }),
                ..UnitType::default()
            },
        ],
        ..Rules::default()
    };
    let map = Map {
        initial_explored: Default::default(),
        creation: Default::default(),
        ai: Vec::new(),
        mission: None,
        fog_of_war: false,
        id: "original-repair-map".into(),
        width: 320,
        height: 224,
        players: 2,
        spawns: vec![
            spawn(0, 1, 112, 100),
            spawn(0, 2, 120, 100),
            spawn(0, 1, 128, 100),
            spawn(1, 5, 280, 100),
            spawn(0, 3, 40, 40),
            spawn(0, 4, 120, 160),
        ],
        resources: vec![ResourceSpawn {
            terrain_corners: None,
            requires_extractor: false,
            kind: "ore".into(),
            position: point(152, 40),
            amount: 6,
            footprint: footprint(8),
        }],
        terrain: None,
        start_locations: vec![],
    };
    (rules, map)
}

fn world(ore: u32, gas: u32) -> World {
    let (rules, map) = definitions(ore, gas);
    World::new(rules, map, 17).unwrap()
}
fn entity(world: &World, id: u32) -> &Entity {
    world
        .state()
        .entities
        .iter()
        .find(|entity| entity.id == EntityId(id))
        .unwrap()
}
fn command(world: &World, player: u16, offset: u64, order: Order) -> Command {
    Command {
        tick: world.tick(),
        player: PlayerId(player),
        sequence: world.state().last_sequences[usize::from(player)] + offset,
        order,
    }
}
fn send(world: &mut World, player: u16, order: Order) -> Option<Rejection> {
    let command = command(world, player, 1, order);
    world.step(&[command]).unwrap().remove(0).rejection
}
fn repair(worker: u32, target: u32) -> Order {
    Order::Repair {
        entity: EntityId(worker),
        target: EntityId(target),
    }
}
fn damage(world: &mut World, target: u32) {
    let before = entity(world, target).hp;
    assert_eq!(
        send(
            world,
            1,
            Order::Attack {
                entity: EntityId(4),
                target: EntityId(target)
            }
        ),
        None
    );
    assert_eq!(entity(world, target).hp, before - 13);
    assert_eq!(entity(world, 4).cooldown, 1_000_000);
}
fn run(world: &mut World, ticks: usize) {
    for _ in 0..ticks {
        world.step(&[]).unwrap();
    }
}
fn assert_clear(world: &World) {
    for entity in &world.state().entities {
        let unit = world.unit_type(entity.unit_type).unwrap();
        assert!(world.can_place(
            entity.position,
            unit.footprint,
            unit.movement_class,
            Some(entity.id)
        ));
    }
}

#[test]
fn fractional_repair_matches_rate_and_per_resource_ceiling_then_stops_at_full_hp() {
    let mut world = world(30, 10);
    damage(&mut world, 2);
    assert_eq!(send(&mut world, 0, repair(1, 2)), None);
    // 100 HP * 9 / (200 build ticks * 10) = 0.45 HP/tick.
    for ticks in 1..=29_u64 {
        if ticks != 1 {
            world.step(&[]).unwrap();
        }
        let restored = (ticks * 9 / 20).min(13);
        assert_eq!(u64::from(entity(&world, 2).hp), 87 + restored);
        assert_eq!(
            world.resource_balance(PlayerId(0), "ore"),
            30 - (restored * 100).div_ceil(300)
        );
        assert_eq!(
            world.resource_balance(PlayerId(0), "gas"),
            10 - (restored * 20).div_ceil(300)
        );
        if restored < 13 {
            assert_eq!(entity(&world, 1).repair_progress, ticks * 900 % 2000);
        }
    }
    assert_eq!(entity(&world, 1).order, UnitOrder::Idle);
    assert_eq!(entity(&world, 1).repair_progress, 0);
    let balances = world.state().players[0].resources.clone();
    run(&mut world, 15);
    assert_eq!(entity(&world, 2).hp, 100);
    assert_eq!(world.state().players[0].resources, balances);
}

#[test]
fn queued_repair_approaches_and_follows_a_moving_target_without_working_remotely() {
    let (rules, mut map) = definitions(30, 10);
    map.spawns[0].position = point(40, 100);
    let mut world = World::new(rules, map, 17).unwrap();
    damage(&mut world, 2);
    let commands = vec![
        command(
            &world,
            0,
            1,
            Order::Move {
                entity: EntityId(1),
                target: point(80, 100),
            },
        ),
        command(
            &world,
            0,
            2,
            Order::Queue {
                entity: EntityId(1),
                order: UnitOrder::Repair {
                    target: EntityId(2),
                },
            },
        ),
        command(
            &world,
            0,
            3,
            Order::Move {
                entity: EntityId(2),
                target: point(200, 100),
            },
        ),
    ];
    assert!(
        world
            .step(&commands)
            .unwrap()
            .iter()
            .all(|outcome| outcome.rejection.is_none())
    );
    for _ in 0..5 {
        world.step(&[]).unwrap();
        assert_eq!(entity(&world, 2).hp, 87);
        assert_eq!(entity(&world, 1).repair_progress, 0);
        assert_clear(&world);
    }
    for _ in 0..140 {
        world.step(&[]).unwrap();
        assert_clear(&world);
    }
    assert_eq!(entity(&world, 2).position, point(200, 100));
    assert_eq!(entity(&world, 2).hp, 100);
    assert_ne!(entity(&world, 1).position, point(40, 100));
    assert_eq!(entity(&world, 1).order, UnitOrder::Idle);
    assert!(entity(&world, 1).queued_orders.is_empty());
}

#[test]
fn repair_rejects_self_enemy_biological_full_and_unfinished_targets() {
    for (target, damaged) in [(1, true), (6, true), (2, false)] {
        let mut world = world(200, 100);
        if damaged {
            damage(&mut world, target);
        }
        let balances = world.state().players[0].resources.clone();
        assert_eq!(
            world.repair_rejection(EntityId(1), EntityId(target)),
            Some(Rejection::InvalidTarget)
        );
        assert_eq!(
            send(&mut world, 0, repair(1, target)),
            Some(Rejection::InvalidTarget)
        );
        assert_eq!(world.state().players[0].resources, balances);
        assert_eq!(entity(&world, 1).order, UnitOrder::Idle);
    }
    // An otherwise eligible, damaged mechanical unit must still be rejected
    // solely because its owner is hostile.
    let (rules, mut map) = definitions(200, 100);
    map.spawns[3].unit_type = UnitTypeId(2);
    map.spawns.push(spawn(0, 5, 280, 180));
    let mut hostile = World::new(rules, map, 17).unwrap();
    assert_eq!(
        send(
            &mut hostile,
            0,
            Order::Attack {
                entity: EntityId(7),
                target: EntityId(4)
            }
        ),
        None
    );
    assert_eq!(entity(&hostile, 4).hp, 87);
    assert_eq!(
        send(&mut hostile, 0, repair(1, 4)),
        Some(Rejection::InvalidTarget)
    );
    let mut world = world(200, 100);
    damage(&mut world, 2);
    assert_eq!(
        send(&mut world, 0, repair(6, 2)),
        Some(Rejection::UnsupportedOrder)
    );
    let building = world.state().next_entity_id;
    assert_eq!(
        send(
            &mut world,
            0,
            Order::Build {
                entity: EntityId(1),
                unit_type: UnitTypeId(3),
                position: point(180, 100)
            }
        ),
        None
    );
    for _ in 0..100 {
        if world
            .state()
            .entities
            .iter()
            .any(|e| e.id == EntityId(building))
        {
            break;
        }
        run(&mut world, 1);
    }
    assert!(entity(&world, building).construction.is_some());
    assert_eq!(
        send(&mut world, 0, repair(3, building)),
        Some(Rejection::InvalidTarget)
    );
    assert!(entity(&world, building).hp < world.unit_type(UnitTypeId(3)).unwrap().max_hp);
    assert_eq!(
        send(&mut world, 0, repair(1, 9999)),
        Some(Rejection::InvalidTarget)
    );
}

#[test]
fn unfunded_repair_waits_without_fractional_work_then_resumes_after_gathering() {
    let mut world = world(0, 10);
    damage(&mut world, 2);
    assert_eq!(send(&mut world, 0, repair(1, 2)), None);
    run(&mut world, 12);
    assert_eq!(entity(&world, 2).hp, 87);
    assert_eq!(entity(&world, 1).repair_progress, 0);
    assert_eq!(
        entity(&world, 1).order,
        UnitOrder::Repair {
            target: EntityId(2)
        }
    );
    assert_eq!(
        send(
            &mut world,
            0,
            Order::Gather {
                entity: EntityId(3),
                resource: ResourceId(1)
            }
        ),
        None
    );
    run(&mut world, 160);
    assert_eq!(world.state().resources[0].amount, 0);
    assert!(entity(&world, 3).cargo.is_none());
    assert_eq!(entity(&world, 2).hp, 100);
    assert_eq!(entity(&world, 1).order, UnitOrder::Idle);
    assert_eq!(world.resource_balance(PlayerId(0), "ore"), 1);
    assert_eq!(world.resource_balance(PlayerId(0), "gas"), 9);
}

#[test]
fn stop_reassign_and_multiple_workers_share_credit_without_extra_or_free_repair() {
    let mut world = world(30, 10);
    damage(&mut world, 2);
    // Repeatedly cancelling fractional work cannot create HP or consume money.
    for _ in 0..4 {
        assert_eq!(send(&mut world, 0, repair(1, 2)), None);
        assert_eq!(
            send(
                &mut world,
                0,
                Order::Stop {
                    entity: EntityId(1)
                }
            ),
            None
        );
    }
    assert_eq!(entity(&world, 2).hp, 87);
    assert_eq!(world.resource_balance(PlayerId(0), "ore"), 30);
    assert_eq!(send(&mut world, 0, repair(1, 2)), None);
    run(&mut world, 2);
    assert_eq!(entity(&world, 2).hp, 88);
    assert_eq!(world.resource_balance(PlayerId(0), "ore"), 29);
    let credit = entity(&world, 2).repair_credit.clone();
    assert_eq!(
        send(
            &mut world,
            0,
            Order::Stop {
                entity: EntityId(1)
            }
        ),
        None
    );
    run(&mut world, 5);
    assert_eq!(entity(&world, 2).hp, 88);
    assert_eq!(entity(&world, 2).repair_credit, credit);
    assert_eq!(send(&mut world, 0, repair(3, 2)), None);
    run(&mut world, 2);
    assert_eq!(entity(&world, 2).hp, 89);
    assert_eq!(
        world.resource_balance(PlayerId(0), "ore"),
        29,
        "new worker must reuse prepaid target credit"
    );
    let mut duplicate = world.clone();
    let commands = vec![
        command(&world, 0, 1, repair(1, 2)),
        command(&world, 0, 2, repair(3, 2)),
    ];
    assert!(
        world
            .step(&commands)
            .unwrap()
            .iter()
            .all(|outcome| outcome.rejection.is_none())
    );
    assert!(
        duplicate
            .step(&commands.into_iter().rev().collect::<Vec<_>>())
            .unwrap()
            .iter()
            .all(|outcome| outcome.rejection.is_none())
    );
    for _ in 0..40 {
        world.step(&[]).unwrap();
        duplicate.step(&[]).unwrap();
        assert_eq!(world.state_hash(), duplicate.state_hash());
        assert!(entity(&world, 2).hp <= 100);
    }
    assert_eq!(entity(&world, 2).hp, 100);
    assert_eq!(world.resource_balance(PlayerId(0), "ore"), 25);
    assert_eq!(world.resource_balance(PlayerId(0), "gas"), 9);
    assert_eq!(entity(&world, 1).order, UnitOrder::Idle);
    assert_eq!(entity(&world, 3).order, UnitOrder::Idle);
}

#[test]
fn destroyed_repair_target_cleans_up_work_and_advances_queued_order() {
    let (rules, mut map) = definitions(30, 10);
    map.spawns.push(spawn(1, 6, 280, 180));
    let mut world = World::new(rules, map, 17).unwrap();
    damage(&mut world, 2);
    let destination = point(80, 140);
    let commands = vec![
        command(&world, 0, 1, repair(1, 2)),
        command(
            &world,
            0,
            2,
            Order::Queue {
                entity: EntityId(1),
                order: UnitOrder::Move {
                    target: destination,
                },
            },
        ),
        command(
            &world,
            1,
            1,
            Order::Attack {
                entity: EntityId(7),
                target: EntityId(2),
            },
        ),
    ];
    assert!(
        world
            .step(&commands)
            .unwrap()
            .iter()
            .all(|outcome| outcome.rejection.is_none())
    );
    for _ in 0..80 {
        if world
            .state()
            .entities
            .iter()
            .all(|entity| entity.id != EntityId(2))
        {
            break;
        }
        assert_eq!(
            entity(&world, 1).order,
            UnitOrder::Repair {
                target: EntityId(2)
            },
            "target must die during active repair, not after normal completion"
        );
        world.step(&[]).unwrap();
    }
    assert!(
        world
            .state()
            .entities
            .iter()
            .all(|entity| entity.id != EntityId(2))
    );
    assert_eq!(entity(&world, 1).repair_progress, 0);
    assert_eq!(
        entity(&world, 1).order,
        UnitOrder::Move {
            target: destination
        }
    );
    run(&mut world, 40);
    assert_eq!(entity(&world, 1).position, destination);
    assert_eq!(entity(&world, 1).order, UnitOrder::Idle);
    assert!(entity(&world, 1).queued_orders.is_empty());
}

#[test]
fn repair_parameters_and_target_permissions_are_validated_and_hashed() {
    let (rules, map) = definitions(30, 10);
    let original = World::new(rules.clone(), map.clone(), 17).unwrap();
    for field in 0..5 {
        let mut changed = rules.clone();
        let repair = changed.repair.as_mut().unwrap();
        match field {
            0 => repair.rate_numerator += 1,
            1 => repair.rate_denominator += 1,
            2 => repair.cost_divisor += 1,
            3 => repair.range += 1,
            _ => {
                changed.units[0].repairs.pop();
            }
        }
        assert_ne!(
            World::new(changed, map.clone(), 17).unwrap().rules_hash(),
            original.rules_hash()
        );
    }
    let mut bad = rules.clone();
    bad.repair.as_mut().unwrap().rate_denominator = 0;
    assert!(World::new(bad, map.clone(), 17).is_err());
    let mut bad = rules.clone();
    bad.repair.as_mut().unwrap().cost_divisor = 0;
    assert!(World::new(bad, map.clone(), 17).is_err());
    let mut bad = rules;
    bad.repair = None;
    assert!(World::new(bad, map, 17).is_err());
}
