use super::*;

#[test]
fn completed_owned_harvest_facilities_apply_the_strongest_bonus_once() {
    let (mut rules, mut map) = definitions();
    rules.units[3].harvest_bonus_percent = vec![amount(25)];
    rules.units[4].harvest_bonus_percent = vec![amount(100)];
    map.resources[0].amount = 8;
    map.spawns.push(spawn(1, 5, 200, 40));
    for improved in [false, true] {
        let mut map = map.clone();
        if improved {
            map.spawns
                .extend([spawn(0, 4, 160, 80), spawn(0, 4, 180, 80)]);
        }
        let mut w = World::new(rules.clone(), map, 42).unwrap();
        assert_eq!(
            send(
                &mut w,
                Order::Gather {
                    entity: EntityId(2),
                    resource: ResourceId(1),
                }
            ),
            None
        );
        run(&mut w, 100);
        let gained = if improved { 10 } else { 8 };
        assert_eq!(w.resource_balance(PlayerId(0), "ore"), 50 + gained);
        assert_eq!(w.state().statistics[0].resources_collected["ore"], gained);
        assert_eq!(w.state().resources[0].amount, 0);
    }
}

#[test]
fn resource_phasing_delivers_through_mobile_traffic_but_keeps_static_collision() {
    use straterust_engine::map::{Terrain, WALKABLE};
    let (mut rules, mut map) = definitions();
    rules.victory = false;
    rules.units[0].footprint = fp(8, 16);
    rules.units[2].footprint = fp(8, 8);
    rules.units[3].footprint = fp(8, 16);
    map.height = 64;
    map.spawns = vec![
        spawn(0, 3, 40, 32),
        spawn(0, 2, 70, 32),
        spawn(0, 1, 88, 32),
    ];
    map.resources[0].position = point(120, 32);
    map.terrain = Some(Terrain {
        cell_size: 8,
        columns: 32,
        rows: 8,
        flags: (0..256)
            .map(|n| if matches!(n / 32, 3 | 4) { WALKABLE } else { 0 })
            .collect(),
    });
    for phasing in [false, true] {
        rules.units[1].phases_while_gathering = phasing;
        let mut world = World::new(rules.clone(), map.clone(), 42).unwrap();
        assert_eq!(
            send(
                &mut world,
                Order::Gather {
                    entity: EntityId(2),
                    resource: ResourceId(1)
                }
            ),
            None
        );
        run(&mut world, 40);
        assert_eq!(
            world.resource_balance(PlayerId(0), "ore"),
            if phasing { 63 } else { 50 }
        );
        if phasing {
            assert_eq!(
                send(
                    &mut world,
                    Order::Stop {
                        entity: EntityId(2)
                    }
                ),
                None
            );
            assert_eq!(
                send(
                    &mut world,
                    Order::Move {
                        entity: EntityId(2),
                        target: point(140, 32)
                    }
                ),
                None
            );
            run(&mut world, 20);
            assert!(
                entity(&world, 2).position.x < 84,
                "ordinary movement restores collision"
            );
        }
    }
    let mut escaping = World::new(rules.clone(), map.clone(), 42).unwrap();
    assert_eq!(
        send(
            &mut escaping,
            Order::Gather {
                entity: EntityId(2),
                resource: ResourceId(1)
            }
        ),
        None
    );
    run(&mut escaping, 1);
    assert!((84..=92).contains(&entity(&escaping, 2).position.x));
    assert_eq!(
        send(
            &mut escaping,
            Order::Stop {
                entity: EntityId(2)
            }
        ),
        None
    );
    assert_eq!(
        send(
            &mut escaping,
            Order::Move {
                entity: EntityId(2),
                target: point(100, 32)
            }
        ),
        None
    );
    run(&mut escaping, 3);
    assert_eq!(
        entity(&escaping, 2).position,
        point(100, 32),
        "a new order can leave an existing harvest overlap"
    );
    let mut packed_map = map.clone();
    packed_map.spawns.push(spawn(0, 1, 96, 32));
    let mut packed = World::new(rules.clone(), packed_map, 42).unwrap();
    assert_eq!(
        send(
            &mut packed,
            Order::Gather {
                entity: EntityId(2),
                resource: ResourceId(1)
            }
        ),
        None
    );
    run(&mut packed, 1);
    assert_eq!(
        send(
            &mut packed,
            Order::Move {
                entity: EntityId(2),
                target: point(110, 32)
            }
        ),
        None
    );
    run(&mut packed, 8);
    assert_eq!(entity(&packed, 2).position, point(110, 32));
    map.spawns[2].unit_type = UnitTypeId(4);
    let mut world = World::new(rules, map, 42).unwrap();
    assert_eq!(
        send(
            &mut world,
            Order::Gather {
                entity: EntityId(2),
                resource: ResourceId(1)
            }
        ),
        None
    );
    run(&mut world, 40);
    assert_eq!(
        world.resource_balance(PlayerId(0), "ore"),
        50,
        "structures still block harvesting routes"
    );
}

#[test]
fn gather_contends_deterministically_returns_and_exhausts_without_losing_resources() {
    let (rules, mut map) = definitions();
    map.spawns.push(spawn(0, 2, 110, 50));
    let mut a = World::new(rules, map, 9).unwrap();
    let mut b = a.clone();
    let commands = vec![
        Command {
            tick: Tick(0),
            player: PlayerId(0),
            sequence: 1,
            order: Order::Gather {
                entity: EntityId(2),
                resource: ResourceId(1),
            },
        },
        Command {
            tick: Tick(0),
            player: PlayerId(0),
            sequence: 2,
            order: Order::Gather {
                entity: EntityId(4),
                resource: ResourceId(1),
            },
        },
    ];
    a.step(&commands).unwrap();
    b.step(&commands.into_iter().rev().collect::<Vec<_>>())
        .unwrap();
    for _ in 0..150 {
        a.step(&[]).unwrap();
        b.step(&[]).unwrap();
        assert_eq!(a.state_hash(), b.state_hash());
    }
    assert_eq!(a.state().resources[0].amount, 0);
    assert_eq!(a.resource_balance(PlayerId(0), "ore"), 63);
    assert!(
        a.state()
            .entities
            .iter()
            .all(|entity| entity.cargo.is_none())
    );
}

#[test]
fn missing_dropoff_keeps_cargo_and_stop_preserves_it() {
    let (mut rules, map) = definitions();
    rules.units[2].dropoff.clear();
    let mut world = World::new(rules, map, 0).unwrap();
    assert_eq!(
        send(
            &mut world,
            Order::Gather {
                entity: EntityId(2),
                resource: ResourceId(1)
            }
        ),
        None
    );
    run(&mut world, 50);
    assert_eq!(entity(&world, 2).cargo.as_ref().unwrap().amount, 8);
    assert!(entity(&world, 2).harvest_spot.is_none());
    assert_eq!(world.state().resources[0].amount, 5);
    assert_eq!(world.resource_balance(PlayerId(0), "ore"), 50);
    send(
        &mut world,
        Order::Stop {
            entity: EntityId(2),
        },
    );
    run(&mut world, 10);
    assert_eq!(entity(&world, 2).cargo.as_ref().unwrap().amount, 8);
}

#[test]
fn production_waits_for_supply_cancels_and_releases_reservations() {
    let (rules, mut map) = definitions();
    map.spawns.push(spawn(0, 2, 70, 70));
    let mut world = World::new(rules, map, 0).unwrap();
    assert_eq!(world.supply(PlayerId(0)), (2, 2));
    assert_eq!(
        send(
            &mut world,
            Order::Train {
                entity: EntityId(1),
                unit_type: UnitTypeId(2)
            }
        ),
        None
    );
    run(&mut world, 10);
    assert!(!entity(&world, 1).production[0].started);
    assert_eq!(world.supply(PlayerId(0)), (2, 2));
    assert_eq!(
        send(
            &mut world,
            Order::Cancel {
                entity: EntityId(1)
            }
        ),
        None
    );
    assert_eq!(world.resource_balance(PlayerId(0), "ore"), 50);
    build(&mut world, 4, point(100, 100));
    assert_eq!(world.supply(PlayerId(0)), (2, 6));
    assert_eq!(
        send(
            &mut world,
            Order::Train {
                entity: EntityId(1),
                unit_type: UnitTypeId(2)
            }
        ),
        None
    );
    assert!(entity(&world, 1).production[0].started);
    assert_eq!(world.supply(PlayerId(0)), (3, 6));
    assert_eq!(
        send(
            &mut world,
            Order::Cancel {
                entity: EntityId(1)
            }
        ),
        None
    );
    assert_eq!(world.supply(PlayerId(0)), (2, 6));
    assert_eq!(world.resource_balance(PlayerId(0), "ore"), 40);
}

#[test]
fn completed_production_waits_for_exit_and_keeps_supply_reserved() {
    let (mut rules, mut map) = definitions();
    rules.units[2].footprint = fp(252, 252);
    rules.units[2].placement = fp(252, 252);
    rules.units[1].footprint = fp(8, 8);
    map.spawns = vec![spawn(0, 3, 128, 128)];
    map.resources.clear();
    let mut world = World::new(rules, map, 0).unwrap();
    assert_eq!(
        send(
            &mut world,
            Order::Train {
                entity: EntityId(1),
                unit_type: UnitTypeId(2)
            }
        ),
        None
    );
    run(&mut world, 10);
    assert_eq!(entity(&world, 1).production[0].remaining, 0);
    assert_eq!(world.state().entities.len(), 1);
    assert_eq!(world.supply(PlayerId(0)), (1, 2));
    send(
        &mut world,
        Order::Cancel {
            entity: EntityId(1),
        },
    );
    assert_eq!(world.supply(PlayerId(0)), (0, 2));
}

#[test]
fn full_gather_build_train_rally_attack_loop_and_repeated_hashes() {
    let mut world = world();
    send(
        &mut world,
        Order::Gather {
            entity: EntityId(2),
            resource: ResourceId(1),
        },
    );
    run(&mut world, 80);
    assert_eq!(world.resource_balance(PlayerId(0), "ore"), 63);
    build(&mut world, 4, point(90, 100));
    let barracks = build(&mut world, 5, point(130, 100));
    assert_eq!(
        send(
            &mut world,
            Order::Rally {
                entity: barracks,
                target: point(180, 180)
            }
        ),
        None
    );
    let id = world.state().next_entity_id;
    assert_eq!(
        send(
            &mut world,
            Order::Train {
                entity: barracks,
                unit_type: UnitTypeId(1)
            }
        ),
        None
    );
    run(&mut world, 30);
    assert_eq!(entity(&world, id).position, point(180, 180));
    assert_eq!(world.resource_balance(PlayerId(0), "ore"), 28);
    let mut replay = world.clone();
    let command = Command {
        tick: world.tick(),
        player: PlayerId(0),
        sequence: world.state().last_sequences[0] + 1,
        order: Order::Attack {
            entity: EntityId(id),
            target: EntityId(3),
        },
    };
    world.step(std::slice::from_ref(&command)).unwrap();
    replay.step(&[command]).unwrap();
    for _ in 0..60 {
        world.step(&[]).unwrap();
        replay.step(&[]).unwrap();
        assert_eq!(world.canonical_state(), replay.canonical_state());
    }
    assert!(
        world
            .state()
            .entities
            .iter()
            .all(|entity| entity.id != EntityId(3))
            || world
                .state()
                .entities
                .iter()
                .all(|entity| entity.id != EntityId(id))
    );
}

#[test]
fn workers_skip_unreachable_dropoffs_and_keep_a_reachable_return_route() {
    use straterust_engine::map::{BUILDABLE, Terrain, WALKABLE};
    let (rules, mut map) = definitions();
    map.spawns = vec![
        spawn(0, 3, 50, 120),
        spawn(0, 2, 100, 120),
        spawn(0, 3, 220, 220),
    ];
    map.resources[0].position = point(120, 120);
    map.resources[0].amount = 8;
    let mut flags = vec![WALKABLE | BUILDABLE; 32 * 32];
    for row in 0..32 {
        flags[row * 32 + 10] = 0;
    }
    map.terrain = Some(Terrain {
        cell_size: 8,
        columns: 32,
        rows: 32,
        flags,
    });
    let mut world = World::new(rules, map, 0).unwrap();
    send(
        &mut world,
        Order::Gather {
            entity: EntityId(2),
            resource: ResourceId(1),
        },
    );
    run(&mut world, 100);
    assert_eq!(world.resource_balance(PlayerId(0), "ore"), 58);
    assert_eq!(entity(&world, 2).cargo, None);
}

#[test]
fn each_mineral_patch_has_one_active_worker_and_waiters_get_the_next_turn() {
    let (mut rules, mut map) = definitions();
    rules.units[1].worker.as_mut().unwrap().harvest_ticks = 5;
    map.resources[0].amount = 80;
    map.spawns[1].position = point(114, 40);
    map.spawns.push(spawn(0, 2, 126, 40));
    let mut world = World::new(rules, map, 0).unwrap();
    let orders = [EntityId(2), EntityId(4)]
        .into_iter()
        .enumerate()
        .map(|(i, entity)| Command {
            tick: Tick(0),
            player: PlayerId(0),
            sequence: i as u64 + 1,
            order: Order::Gather {
                entity,
                resource: ResourceId(1),
            },
        })
        .collect::<Vec<_>>();
    world.step(&orders).unwrap();
    run(&mut world, 3);
    assert_eq!(entity(&world, 2).harvest_progress, 4);
    assert_eq!(entity(&world, 4).harvest_progress, 0);
    assert_eq!(world.state().resources[0].amount, 80);
    run(&mut world, 1);
    assert_eq!(entity(&world, 2).cargo.as_ref().unwrap().amount, 8);
    assert_eq!(entity(&world, 4).harvest_progress, 1);
    run(&mut world, 4);
    assert_eq!(entity(&world, 4).cargo.as_ref().unwrap().amount, 8);
    assert_eq!(world.state().resources[0].amount, 64);
}
