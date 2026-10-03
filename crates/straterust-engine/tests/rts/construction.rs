use super::*;

#[test]
fn construction_charges_once_pauses_resumes_and_cancels_with_refund() {
    let mut world = world();
    assert_eq!(
        world.build_rejection(PlayerId(0), EntityId(2), UnitTypeId(4), point(40, 40)),
        Some(Rejection::InvalidPlacement)
    );
    assert_eq!(
        send(
            &mut world,
            Order::Build {
                entity: EntityId(2),
                unit_type: UnitTypeId(5),
                position: point(100, 100)
            }
        ),
        Some(Rejection::MissingPrerequisite)
    );
    assert_eq!(
        send(
            &mut world,
            Order::Build {
                entity: EntityId(2),
                unit_type: UnitTypeId(4),
                position: point(90, 40)
            }
        ),
        None
    );
    assert_eq!(world.resource_balance(PlayerId(0), "ore"), 40);
    run(&mut world, 2);
    send(
        &mut world,
        Order::Stop {
            entity: EntityId(2),
        },
    );
    let remaining = entity(&world, 4).construction.as_ref().unwrap().remaining;
    assert_eq!(
        entity(&world, 4).construction.as_ref().unwrap().worker,
        None
    );
    run(&mut world, 10);
    assert_eq!(
        entity(&world, 4).construction.as_ref().unwrap().remaining,
        remaining
    );
    assert_eq!(
        send(
            &mut world,
            Order::Resume {
                entity: EntityId(2),
                building: EntityId(4)
            }
        ),
        None
    );
    run(&mut world, 10);
    assert!(entity(&world, 4).construction.is_none());
    assert_eq!(entity(&world, 4).hp, 30);
    assert_eq!(
        send(
            &mut world,
            Order::Build {
                entity: EntityId(2),
                unit_type: UnitTypeId(5),
                position: point(140, 100)
            }
        ),
        None
    );
    assert_eq!(
        send(
            &mut world,
            Order::Cancel {
                entity: EntityId(5)
            }
        ),
        None
    );
    assert_eq!(world.resource_balance(PlayerId(0), "ore"), 40);
    assert_eq!(entity(&world, 2).order, UnitOrder::Idle);
    assert!(
        world
            .state()
            .entities
            .iter()
            .all(|entity| entity.id != EntityId(5))
    );
}

#[test]
fn construction_repositions_and_pauses_while_progress_and_hashes_remain_deterministic() {
    let mut a = construction_world();
    let mut b = a.clone();
    begin_construction(&mut a, point(100, 100));
    begin_construction(&mut b, point(100, 100));
    let mut positions = std::collections::BTreeSet::new();
    let mut moving_work_ticks = 0;
    let mut stationary_work_ticks = 0;
    let mut pauses = 0;
    for _ in 0..400 {
        let before = entity(&a, 1).position;
        let Some(progress) = entity(&a, 2).construction.clone() else {
            break;
        };
        a.step(&[]).unwrap();
        b.step(&[]).unwrap();
        assert_eq!(a.canonical_state(), b.canonical_state());
        let worker = entity(&a, 1);
        assert!(a.can_place(
            worker.position,
            fp(4, 4),
            MovementClass::Ground,
            Some(worker.id)
        ));
        if progress.work_position.is_some() {
            positions.insert(worker.position);
            if worker.position != before {
                moving_work_ticks += 1;
            } else {
                stationary_work_ticks += 1;
            }
            let remaining = entity(&a, 2)
                .construction
                .as_ref()
                .map_or(0, |progress| progress.remaining);
            assert_eq!(remaining, progress.remaining - 1);
        }
        if let Some(after) = &entity(&a, 2).construction
            && after.work_ticks > progress.work_ticks
        {
            assert!((30..=93).contains(&after.work_ticks));
            pauses += 1;
        }
    }
    assert!(entity(&a, 2).construction.is_none());
    assert_eq!(entity(&a, 2).hp, 500);
    assert!(positions.len() > 10 && moving_work_ticks > 20);
    assert!(stationary_work_ticks > 60 && pauses >= 3);
    assert_eq!(entity(&a, 1).order, UnitOrder::Idle);
    assert!(entity(&a, 1).path.is_empty());
}

#[test]
fn construction_travel_is_interrupted_resumed_and_cancelled_without_lingering_motion() {
    let mut world = construction_world();
    begin_construction(&mut world, point(100, 100));
    wait_for_construction_travel(&mut world);
    assert_eq!(
        send(
            &mut world,
            Order::Stop {
                entity: EntityId(1)
            }
        ),
        None
    );
    let position = entity(&world, 1).position;
    let paused = entity(&world, 2).construction.clone().unwrap();
    assert_eq!(paused.worker, None);
    assert_eq!(paused.work_position, None);
    assert_eq!(paused.work_ticks, 0);
    run(&mut world, 20);
    assert_eq!(entity(&world, 1).position, position);
    assert_eq!(entity(&world, 2).construction.as_ref(), Some(&paused));
    assert_eq!(
        send(
            &mut world,
            Order::Resume {
                entity: EntityId(1),
                building: EntityId(2)
            }
        ),
        None
    );
    wait_for_construction_travel(&mut world);
    assert_eq!(
        send(
            &mut world,
            Order::Move {
                entity: EntityId(1),
                target: point(180, 160)
            }
        ),
        None
    );
    let remaining = entity(&world, 2).construction.as_ref().unwrap().remaining;
    run(&mut world, 60);
    assert_eq!(entity(&world, 1).position, point(180, 160));
    assert_eq!(
        entity(&world, 2).construction.as_ref().unwrap().remaining,
        remaining
    );
    assert_eq!(
        send(
            &mut world,
            Order::Resume {
                entity: EntityId(1),
                building: EntityId(2)
            }
        ),
        None
    );
    wait_for_construction_travel(&mut world);
    assert_eq!(
        send(
            &mut world,
            Order::Cancel {
                entity: EntityId(2)
            }
        ),
        None
    );
    let worker = entity(&world, 1).clone();
    assert_eq!(worker.order, UnitOrder::Idle);
    assert!(worker.target.is_none() && worker.path.is_empty());
    run(&mut world, 20);
    assert_eq!(entity(&world, 1).position, worker.position);
    assert_eq!(world.resource_balance(PlayerId(0), "ore"), 50);
}

#[test]
fn construction_work_points_respect_map_edges_terrain_and_resources() {
    use straterust_engine::map::{BUILDABLE, Terrain, WALKABLE};
    let base = construction_world();
    let mut rules = base.rules().clone();
    rules.units[3].footprint = fp(32, 32);
    rules.units[3].placement = fp(32, 32);
    let mut map = base.map().clone();
    map.spawns = vec![spawn(0, 2, 40, 96)];
    map.resources.push(ResourceSpawn {
        requires_extractor: false,
        kind: "ore".into(),
        position: point(40, 72),
        amount: 100,
        footprint: fp(12, 12),
    });
    let mut flags = vec![WALKABLE | BUILDABLE; 32 * 32];
    flags[7 * 32..8 * 32].fill(0);
    map.terrain = Some(Terrain {
        cell_size: 8,
        columns: 32,
        rows: 32,
        flags,
    });
    let mut world = World::new(rules, map, 42).unwrap();
    begin_construction(&mut world, point(16, 80));
    let mut positions = std::collections::BTreeSet::new();
    for _ in 0..400 {
        world.step(&[]).unwrap();
        let worker = entity(&world, 1);
        assert!(world.can_place(
            worker.position,
            fp(4, 4),
            MovementClass::Ground,
            Some(worker.id)
        ));
        positions.insert(worker.position);
        if entity(&world, 2).construction.is_none() {
            break;
        }
    }
    assert!(entity(&world, 2).construction.is_none());
    assert!(positions.len() > 5);
}

#[test]
fn construction_completes_when_no_other_work_point_is_reachable() {
    use straterust_engine::map::{BUILDABLE, Terrain, WALKABLE};
    let base = construction_world();
    let mut rules = base.rules().clone();
    rules.units[3].footprint = fp(16, 16);
    rules.units[3].placement = fp(16, 16);
    rules.units[3].build_ticks = 100;
    let mut map = base.map().clone();
    map.width = 64;
    map.height = 64;
    map.spawns = vec![spawn(0, 2, 34, 24)];
    let mut flags = vec![0; 32 * 32];
    for y in 8..16 {
        for x in 8..16 {
            flags[y * 32 + x] = BUILDABLE;
        }
    }
    // The worker fits in this single pocket beside the foundation. Every
    // other work point is blocked, so it must keep working safely in place.
    for y in 11..13 {
        for x in 16..18 {
            flags[y * 32 + x] = WALKABLE;
        }
    }
    map.terrain = Some(Terrain {
        cell_size: 2,
        columns: 32,
        rows: 32,
        flags,
    });
    let mut world = World::new(rules, map, 42).unwrap();
    begin_construction(&mut world, point(24, 24));
    for _ in 0..100 {
        world.step(&[]).unwrap();
        assert_eq!(entity(&world, 1).position, point(34, 24));
        assert!(entity(&world, 1).path.is_empty());
    }
    assert!(entity(&world, 2).construction.is_none());
    assert_eq!(entity(&world, 2).hp, 500);
}

#[test]
fn terrain_blocks_fast_moves_and_construction_without_tunneling() {
    use straterust_engine::map::{BUILDABLE, Terrain, WALKABLE};
    let (mut rules, mut map) = definitions();
    rules.units[1].speed = 32;
    map.spawns = vec![spawn(0, 2, 40, 40), spawn(1, 1, 220, 220)];
    map.resources.clear();
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
    assert_eq!(
        send(
            &mut world,
            Order::Move {
                entity: EntityId(1),
                target: point(160, 40)
            }
        ),
        None
    );
    run(&mut world, 20);
    assert!(entity(&world, 1).position.x < 80);
    assert_eq!(
        send(
            &mut world,
            Order::Build {
                entity: EntityId(1),
                unit_type: UnitTypeId(4),
                position: point(84, 100)
            }
        ),
        Some(Rejection::InvalidPlacement)
    );
    assert_eq!(world.resource_balance(PlayerId(0), "ore"), 50);
}

#[test]
fn builder_death_leaves_resumable_foundation_and_victory_is_authoritative() {
    let (mut rules, mut map) = definitions();
    rules.victory = true;
    rules.units[0].weapon = Some(Weapon {
        cooldown_jitter: None,
        targets_air: false,
        damage_kind: Default::default(),
        splash: None,
        strikes: Vec::new(),
        damage: 100,
        range: 15,
        cooldown: 1,
    });
    map.spawns = vec![spawn(0, 2, 70, 40), spawn(1, 1, 70, 55)];
    map.resources.clear();
    let mut world = World::new(rules, map, 0).unwrap();
    assert_eq!(
        send(
            &mut world,
            Order::Build {
                entity: EntityId(1),
                unit_type: UnitTypeId(4),
                position: point(100, 40)
            }
        ),
        None
    );
    assert!(
        world
            .state()
            .entities
            .iter()
            .all(|entity| entity.id != EntityId(1))
    );
    assert_eq!(
        entity(&world, 3).construction.as_ref().unwrap().worker,
        None
    );
    assert_eq!(world.state().winner, None); // An unfinished building still belongs to its owner.
    assert_eq!(
        send(
            &mut world,
            Order::Cancel {
                entity: EntityId(3)
            }
        ),
        None
    );
    assert_eq!(world.state().winner, Some(PlayerId(1)));
    assert_eq!(
        send(
            &mut world,
            Order::Stop {
                entity: EntityId(1)
            }
        ),
        Some(Rejection::GameOver)
    );
}
