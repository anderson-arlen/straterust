use super::*;

fn harvesting(workers: usize, resource: Footprint) -> World {
    let mut rules = economic_rules();
    rules.units[0].footprint = Footprint {
        width: 16,
        height: 16,
    };
    rules.units[0].phases_while_gathering = true;
    rules.units[0].worker.as_mut().unwrap().harvest_ticks = 10_000;
    let mut map = map(vec![spawn(2, 64, 128)]);
    map.spawns.extend(
        (0..workers).map(|n| spawn(1, 80 + (n % 3) as i32 * 24, 192 + (n / 3) as i32 * 24)),
    );
    map.resources = vec![node("ore", 256, 256, 1000)];
    map.resources[0].footprint = resource;
    World::new(rules, map, 7).unwrap()
}

fn order_all(world: &mut World) {
    let commands: Vec<_> = world
        .state()
        .entities
        .iter()
        .skip(1)
        .enumerate()
        .map(|(n, e)| Command {
            tick: world.tick(),
            player: PlayerId(0),
            sequence: n as u64 + 1,
            order: Order::Gather {
                entity: e.id,
                resource: ResourceId(1),
            },
        })
        .collect();
    assert!(
        world
            .step(&commands)
            .unwrap()
            .iter()
            .all(|o| o.rejection.is_none())
    );
}

fn assert_separate(world: &World) {
    let workers: Vec<_> = world
        .state()
        .entities
        .iter()
        .filter(|e| e.harvest_spot.is_some())
        .collect();
    for (n, a) in workers.iter().enumerate() {
        let af = world.unit_type(a.unit_type).unwrap().footprint;
        let [al, at, ar, ab] = af.bounds(a.harvest_spot.unwrap());
        for b in &workers[n + 1..] {
            let bf = world.unit_type(b.unit_type).unwrap().footprint;
            let [bl, bt, br, bb] = bf.bounds(b.harvest_spot.unwrap());
            assert!(
                al >= br || ar <= bl || at >= bb || ab <= bt,
                "overlapping reservations: {a:?} {b:?}"
            );
        }
        if a.harvest_waiting_since.is_some() {
            assert_eq!(a.position, a.harvest_spot.unwrap());
            assert!(
                world.can_place(a.position, af, MovementClass::Ground, Some(a.id)),
                "overlapping waiter: {a:?}"
            );
        }
    }
    assert!(
        world
            .state()
            .entities
            .iter()
            .filter(|e| e.harvest_progress > 0)
            .count()
            <= 1
    );
}

#[test]
fn phased_workers_use_all_resource_edges_without_overlapping_waiters() {
    let mut world = harvesting(
        10,
        Footprint {
            width: 32,
            height: 16,
        },
    );
    order_all(&mut world);
    for _ in 0..240 {
        world.step(&[]).unwrap();
        assert_separate(&world);
    }
    let waiters: Vec<_> = world
        .state()
        .entities
        .iter()
        .filter(|e| e.harvest_waiting_since.is_some())
        .collect();
    assert_eq!(
        waiters.len(),
        10,
        "{:?}",
        world
            .state()
            .entities
            .iter()
            .skip(1)
            .map(|e| (e.id, e.position, e.harvest_spot))
            .collect::<Vec<_>>()
    );
    for edge in [
        point(232, 256),
        point(280, 256),
        point(256, 240),
        point(256, 272),
    ] {
        assert!(waiters.iter().any(|e| if edge.x != 256 {
            e.position.x == edge.x
        } else {
            e.position.y == edge.y
        }));
    }
    let snapshot = ron::ser::to_string(&world.save_snapshot().unwrap()).unwrap();
    let mut restored = world
        .restore_snapshot(ron::from_str(&snapshot).unwrap())
        .unwrap();
    for _ in 0..40 {
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
            .all(|e| { !matches!(e, ViewedEntity::Owned(own) if own.harvest_spot.is_some()) })
    );
}

#[test]
fn full_resource_edges_keep_excess_workers_outside_and_reuse_released_spots() {
    let mut world = harvesting(
        20,
        Footprint {
            width: 8,
            height: 8,
        },
    );
    order_all(&mut world);
    run(&mut world, 180);
    assert_separate(&world);
    let waiting: Vec<_> = world
        .state()
        .entities
        .iter()
        .filter(|e| matches!(e.order, UnitOrder::Gather { .. }) && e.harvest_spot.is_none())
        .map(|e| e.id)
        .collect();
    assert!(!waiting.is_empty());
    let first = world
        .state()
        .entities
        .iter()
        .find(|e| e.harvest_waiting_since.is_some())
        .unwrap()
        .id;
    send(&mut world, Order::Stop { entity: first });
    assert!(entity(&world, first.0).harvest_spot.is_none());
    send(
        &mut world,
        Order::Move {
            entity: first,
            target: point(96, 96),
        },
    );
    for _ in 0..180 {
        world.step(&[]).unwrap();
        assert_separate(&world);
    }
    assert!(
        waiting
            .iter()
            .any(|id| entity(&world, id.0).harvest_waiting_since.is_some())
    );
}

#[test]
fn workers_reserve_reachable_edges_when_the_nearest_edge_is_blocked() {
    let world = harvesting(
        6,
        Footprint {
            width: 16,
            height: 32,
        },
    );
    let rules = world.rules().clone();
    let mut map = world.map().clone();
    for (n, spawn) in map.spawns.iter_mut().skip(1).enumerate() {
        spawn.position = point(352, 192 + n as i32 * 24);
    }
    let mut flags = vec![straterust_engine::map::WALKABLE; 128 * 80];
    for row in 0..80 {
        flags[row * 128 + 30] = 0;
    }
    map.terrain = Some(Terrain {
        cell_size: 8,
        columns: 128,
        rows: 80,
        flags,
    });
    let mut world = World::new(rules, map, 7).unwrap();
    order_all(&mut world);
    for _ in 0..180 {
        world.step(&[]).unwrap();
        assert_separate(&world);
    }
    assert_eq!(
        world
            .state()
            .entities
            .iter()
            .filter(|e| e.harvest_waiting_since.is_some())
            .count(),
        6
    );
    assert!(
        world
            .state()
            .entities
            .iter()
            .filter_map(|e| e.harvest_spot)
            .all(|p| p.x >= 256)
    );
}

#[test]
fn crowded_phased_workers_cycle_through_spots_and_deliver_every_resource() {
    let world = harvesting(
        10,
        Footprint {
            width: 32,
            height: 16,
        },
    );
    let mut rules = world.rules().clone();
    rules.units[0].worker.as_mut().unwrap().harvest_ticks = 4;
    let mut map = world.map().clone();
    map.resources[0].amount = 240;
    let mut world = World::new(rules, map, 7).unwrap();
    order_all(&mut world);
    for _ in 0..600 {
        world.step(&[]).unwrap();
        assert_separate(&world);
    }
    assert_eq!(world.state().resources[0].amount, 0);
    assert_eq!(world.resource_balance(PlayerId(0), "ore"), 240);
    assert!(
        world
            .state()
            .entities
            .iter()
            .all(|e| e.harvest_spot.is_none() && e.cargo.is_none())
    );
}
