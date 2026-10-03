use super::*;

#[test]
fn mutual_lethal_attacks_apply_simultaneously_and_armor_has_a_damage_floor() {
    let (mut rules, mut map) = definitions();
    rules.units[0].max_hp = 1;
    rules.units[0].armor = 999;
    map.spawns = vec![spawn(0, 1, 100, 100), spawn(1, 1, 110, 100)];
    map.resources.clear();
    let mut world = World::new(rules, map, 0).unwrap();
    world.step(&[]).unwrap();
    assert_eq!(world.state().entities.len(), 2);
    assert!(
        world
            .state()
            .entities
            .iter()
            .all(|entity| entity.hp == 1 && entity.damage_fraction == 128)
    );
    let cooldown = world.rules().units[0].weapon.as_ref().unwrap().cooldown;
    for _ in 0..cooldown {
        world.step(&[]).unwrap();
    }
    assert!(world.state().entities.is_empty());
}

#[test]
fn queued_moves_patrol_and_hold_share_movement_and_stable_cooldowns() {
    let (rules, mut map) = definitions();
    map.spawns = vec![spawn(0, 1, 40, 40), spawn(1, 1, 220, 220)];
    map.resources.clear();
    let mut world = World::new(rules, map, 0).unwrap();
    send(
        &mut world,
        Order::Move {
            entity: EntityId(1),
            target: point(60, 40),
        },
    );
    send(
        &mut world,
        Order::Queue {
            entity: EntityId(1),
            order: UnitOrder::Move {
                target: point(60, 80),
            },
        },
    );
    run(&mut world, 15);
    assert_eq!(entity(&world, 1).position, point(60, 80));
    assert!(entity(&world, 1).queued_orders.is_empty());
    send(
        &mut world,
        Order::Patrol {
            entity: EntityId(1),
            target: point(100, 80),
        },
    );
    run(&mut world, 6);
    assert!(entity(&world, 1).patrol_returning);
    send(
        &mut world,
        Order::Hold {
            entity: EntityId(1),
        },
    );
    let position = entity(&world, 1).position;
    run(&mut world, 10);
    assert_eq!(entity(&world, 1).position, position);
}

#[test]
fn invalid_orders_preserve_economy_and_queues_are_bounded() {
    let mut world = world();
    assert_eq!(
        send(
            &mut world,
            Order::Train {
                entity: EntityId(2),
                unit_type: UnitTypeId(1)
            }
        ),
        Some(Rejection::UnsupportedOrder)
    );
    assert_eq!(
        send(
            &mut world,
            Order::Gather {
                entity: EntityId(2),
                resource: ResourceId(900)
            }
        ),
        Some(Rejection::InvalidTarget)
    );
    assert_eq!(
        send(
            &mut world,
            Order::Attack {
                entity: EntityId(2),
                target: EntityId(1)
            }
        ),
        Some(Rejection::UnsupportedOrder)
    );
    for _ in 0..5 {
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
    }
    // One front job has already completed, so refill to the explicit bound.
    while entity(&world, 1).production.len() < 5 {
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
    }
    let before = world.resource_balance(PlayerId(0), "ore");
    assert_eq!(
        send(
            &mut world,
            Order::Train {
                entity: EntityId(1),
                unit_type: UnitTypeId(2)
            }
        ),
        Some(Rejection::QueueFull)
    );
    assert_eq!(world.resource_balance(PlayerId(0), "ore"), before);
}

#[test]
fn weapon_cooldown_and_hold_range_apply_without_chasing() {
    let (mut rules, mut map) = definitions();
    rules.units[1].weapon = None;
    map.spawns = vec![spawn(0, 1, 100, 100), spawn(1, 2, 110, 100)];
    map.resources.clear();
    let mut world = World::new(rules.clone(), map.clone(), 0).unwrap();
    send(
        &mut world,
        Order::Hold {
            entity: EntityId(1),
        },
    );
    assert_eq!(entity(&world, 2).hp, 16);
    run(&mut world, 2);
    assert_eq!(entity(&world, 2).hp, 16);
    run(&mut world, 1);
    assert_eq!(entity(&world, 2).hp, 12);
    map.spawns[1].position = point(150, 100);
    let mut world = World::new(rules, map, 0).unwrap();
    send(
        &mut world,
        Order::Hold {
            entity: EntityId(1),
        },
    );
    run(&mut world, 10);
    assert_eq!(entity(&world, 1).position, point(100, 100));
    assert_eq!(entity(&world, 2).hp, 20);
}

#[test]
fn malformed_rules_and_resource_footprints_are_rejected_before_state_creation() {
    let (rules, map) = definitions();
    let mut invalid = rules.clone();
    invalid.units[0].build_ticks = 0;
    assert!(World::new(invalid, map.clone(), 0).is_err());
    let mut invalid = rules.clone();
    invalid.units[1].worker.as_mut().unwrap().capacity = 0;
    assert!(World::new(invalid, map.clone(), 0).is_err());
    let mut invalid = rules.clone();
    invalid.units[0].weapon.as_mut().unwrap().cooldown = 0;
    assert!(World::new(invalid, map.clone(), 0).is_err());
    let mut invalid = rules.clone();
    invalid.units[2].placement = fp(0, 1);
    assert!(World::new(invalid, map.clone(), 0).is_err());
    let mut invalid = rules.clone();
    invalid.units[0].cost = vec![amount(1), amount(1)];
    assert!(World::new(invalid, map.clone(), 0).is_err());
    let mut invalid = rules.clone();
    invalid.units[1].builds.push(UnitTypeId(900));
    assert!(World::new(invalid, map.clone(), 0).is_err());
    let mut invalid = rules.clone();
    invalid.units[0].supply_used = u32::MAX;
    assert!(World::new(invalid, map.clone(), 0).is_err());
    let mut invalid = rules.clone();
    invalid.starting_resources.push(ResourceAmount {
        kind: "bad kind".into(),
        amount: u32::MAX,
    });
    assert!(World::new(invalid, map.clone(), 0).is_err());
    let mut map = map;
    map.resources[0].footprint = fp(0, 0);
    assert!(World::new(rules, map, 0).is_err());
}

#[test]
#[ignore = "repeatable release-mode 3/128 mover tick measurements"]
fn movement_scale_measurement() {
    use std::time::Instant;
    for count in [3, 128] {
        let (mut rules, mut map) = definitions();
        rules.units[0].footprint = fp(8, 8);
        map.width = 2048;
        map.height = 2048;
        map.resources.clear();
        map.spawns = (0..count)
            .map(|index| spawn(0, 1, 40, 16 + index * 15))
            .collect();
        let mut world = World::new(rules, map, 0).unwrap();
        let commands: Vec<_> = (0..count)
            .map(|index| Command {
                tick: Tick(0),
                player: PlayerId(0),
                sequence: index as u64 + 1,
                order: Order::Move {
                    entity: EntityId(index as u32 + 1),
                    target: point(1800, 16 + index * 15),
                },
            })
            .collect();
        let mut times = Vec::new();
        for tick in 0..200 {
            let start = Instant::now();
            world.step(if tick == 0 { &commands } else { &[] }).unwrap();
            times.push(start.elapsed());
        }
        assert!(
            world
                .state()
                .entities
                .iter()
                .all(|entity| entity.position.x >= 1600)
        );
        times.sort();
        eprintln!(
            "{count} movers / 2048x2048 open map / 200 moving ticks: median={}us p95={}us max={}us",
            times[100].as_micros(),
            times[190].as_micros(),
            times[199].as_micros()
        );
    }
}

#[test]
fn overlapping_spawns_are_rejected_but_rallies_accept_in_bounds_edge_intents() {
    let (rules, mut map) = definitions();
    map.spawns[1].position = map.spawns[0].position;
    assert!(World::new(rules.clone(), map.clone(), 0).is_err());
    map.spawns[1].position = map.resources[0].position;
    assert!(World::new(rules, map, 0).is_err());
    let mut world = world();
    assert_eq!(
        send(
            &mut world,
            Order::Rally {
                entity: EntityId(1),
                target: point(0, 0)
            }
        ),
        None
    );
    assert_eq!(entity(&world, 1).rally, Some(point(0, 0)));
    assert_eq!(
        send(
            &mut world,
            Order::Rally {
                entity: EntityId(1),
                target: point(-1, 0)
            }
        ),
        Some(Rejection::OutOfBounds)
    );
    assert_eq!(entity(&world, 1).rally, Some(point(0, 0)));
}

#[test]
fn delayed_strikes_and_source_cooldown_jitter_control_damage_not_animation_frames() {
    let (mut rules, mut map) = definitions();
    let weapon = rules.units[0].weapon.as_mut().unwrap();
    weapon.cooldown = 8;
    weapon.cooldown_jitter = Some([-1, 2]);
    weapon.strikes = vec![WeaponStrike {
        delay: 2,
        forward: 0,
    }];
    rules.units[1].max_hp = 1000;
    map.spawns = vec![spawn(0, 1, 40, 40), spawn(1, 2, 48, 40)];
    let mut a = World::new(rules, map, 17).unwrap();
    let mut b = a.clone();
    let mut prior_hp = 1000;
    let mut hits = Vec::new();
    for tick in 0..200 {
        a.step(&[]).unwrap();
        b.step(&[]).unwrap();
        assert_eq!(a.state_hash(), b.state_hash());
        let hp = entity(&a, 2).hp;
        if hp != prior_hp {
            assert_eq!(prior_hp - hp, 4);
            hits.push(tick);
        }
        prior_hp = hp;
    }
    assert_eq!(hits[0], 2, "damage waits for the strike frame");
    assert!(
        hits.windows(2)
            .all(|pair| (7..=10).contains(&(pair[1] - pair[0])))
    );
    assert!(hits.windows(2).any(|pair| pair[1] - pair[0] == 7));
    assert!(hits.windows(2).any(|pair| pair[1] - pair[0] == 10));
}

#[test]
fn delayed_single_target_strikes_preserve_air_target_permissions() {
    let (mut rules, mut map) = definitions();
    let weapon = rules.units[0].weapon.as_mut().unwrap();
    weapon.targets_air = true;
    weapon.strikes = vec![WeaponStrike {
        delay: 1,
        forward: 0,
    }];
    rules.units[1].movement_class = MovementClass::Air;
    map.spawns = vec![spawn(0, 1, 40, 40), spawn(1, 2, 48, 40)];
    let mut world = World::new(rules, map, 0).unwrap();
    run(&mut world, 1);
    assert_eq!(entity(&world, 2).hp, 20);
    run(&mut world, 1);
    assert_eq!(entity(&world, 2).hp, 16);
}
