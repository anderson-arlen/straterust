use super::*;

#[test]
fn water_transport_boards_across_a_coast_and_resolves_a_land_unload_click() {
    use crate::map::{BUILDABLE, Terrain, WALKABLE, WATER};
    let old = world();
    let mut rules = old.rules().clone();
    rules.units[0].weapon = None;
    let ship = &mut rules.units[2];
    ship.structure = false;
    ship.speed = 8;
    ship.movement_class = MovementClass::Water;
    ship.garrison.as_mut().unwrap().attackers.clear();
    ship.garrison.as_mut().unwrap().boarding_range = 16;
    let mut map = old.map().clone();
    map.spawns.truncate(3);
    map.spawns[0].position = Position { x: 96, y: 80 };
    map.spawns[1].position = Position { x: 96, y: 96 };
    map.spawns[2].position = Position { x: 176, y: 80 };
    map.terrain = Some(Terrain {
        cell_size: 16,
        columns: 16,
        rows: 16,
        flags: (0..256)
            .map(|i| {
                if i % 16 < 8 {
                    WALKABLE | BUILDABLE
                } else {
                    WATER
                }
            })
            .collect(),
    });
    let mut w = World::new(rules, map, 42).unwrap();
    assert_eq!(
        issue(
            &mut w,
            Order::Load {
                entity: EntityId(1),
                target: EntityId(3)
            }
        ),
        None
    );
    for _ in 0..60 {
        w.step(&[]).unwrap();
        if w.state.entities[0].garrisoned_in.is_some() {
            break;
        }
    }
    assert_eq!(w.state.entities[0].garrisoned_in, Some(EntityId(3)));
    let target = Position { x: 112, y: 208 };
    assert_eq!(
        issue(
            &mut w,
            Order::UnloadAt {
                entity: EntityId(3),
                target
            }
        ),
        None
    );
    let mut resumed = w.clone();
    resumed
        .restore_snapshot(w.save_snapshot().unwrap())
        .unwrap();
    for _ in 0..80 {
        w.step(&[]).unwrap();
        resumed.step(&[]).unwrap();
        assert_eq!(w.state_hash(), resumed.state_hash());
        assert!(w.map.can_move(
            w.state.entities[2].position,
            w.unit_at(2).footprint,
            MovementClass::Water
        ));
        if w.state.entities[0].garrisoned_in.is_none() {
            break;
        }
    }
    let passenger = &w.state.entities[0];
    assert!(passenger.garrisoned_in.is_none());
    assert!(passenger.position.x < 128 && passenger.position.y > 180);
    assert!(w.map.can_move(
        passenger.position,
        w.unit_at(0).footprint,
        MovementClass::Ground
    ));
}

fn transport() -> World {
    let old = world();
    let mut rules = old.rules().clone();
    rules.units[0].weapon = None;
    rules.units[2].structure = false;
    rules.units[2].speed = 8;
    rules.units[2].movement_class = MovementClass::Air;
    rules.units[2].garrison.as_mut().unwrap().attackers.clear();
    rules.units[2].garrison.as_mut().unwrap().unload_ticks = 15;
    let mut map = old.map().clone();
    map.spawns.truncate(3);
    let mut world = World::new(rules, map, 42).unwrap();
    load(&mut world, 0);
    load(&mut world, 1);
    world
}

#[test]
fn instant_water_unloading_places_a_full_load_in_clear_nonoverlapping_rows() {
    use crate::map::{BUILDABLE, Terrain, WALKABLE, WATER};
    let old = world();
    let mut rules = old.rules().clone();
    rules.units[0].weapon = None;
    rules.units[0].footprint = Footprint {
        width: 24,
        height: 24,
    };
    let ship = &mut rules.units[2];
    ship.structure = false;
    ship.speed = 8;
    ship.footprint = Footprint {
        width: 56,
        height: 56,
    };
    ship.movement_class = MovementClass::Water;
    let garrison = ship.garrison.as_mut().unwrap();
    garrison.capacity = 6;
    garrison.boarding_range = 16;
    garrison.attackers.clear();
    let mut map = old.map().clone();
    map.width = 512;
    map.height = 512;
    map.spawns = vec![Spawn {
        unit_type: UnitTypeId(3),
        position: Position { x: 284, y: 256 },
        ..Default::default()
    }];
    map.spawns.extend((0..6).map(|n| Spawn {
        unit_type: UnitTypeId(1),
        position: Position {
            x: 224,
            y: 192 + 24 * n,
        },
        ..Default::default()
    }));
    map.terrain = Some(Terrain {
        cell_size: 32,
        columns: 16,
        rows: 16,
        flags: (0..256)
            .map(|i| {
                if i % 16 < 8 {
                    WALKABLE | BUILDABLE
                } else {
                    WATER
                }
            })
            .collect(),
    });
    let mut w = World::new(rules, map, 42).unwrap();
    // Boarding is covered separately; start this exit regression fully loaded.
    for passenger in &mut w.state.entities[1..] {
        passenger.garrisoned_in = Some(EntityId(1));
        passenger.position = Position { x: 284, y: 256 };
    }
    let mut restored = w.restore_snapshot(w.save_snapshot().unwrap()).unwrap();
    for world in [&mut w, &mut restored] {
        assert_eq!(
            issue(
                world,
                Order::UnloadAt {
                    entity: EntityId(1),
                    target: Position { x: 224, y: 352 }
                }
            ),
            None
        );
    }
    for _ in 0..100 {
        if w.state.entities[1..]
            .iter()
            .any(|e| e.garrisoned_in.is_none())
        {
            break;
        }
        w.step(&[]).unwrap();
        restored.step(&[]).unwrap();
        assert_eq!(w.state_hash(), restored.state_hash());
    }
    assert!(
        w.state.entities[1..]
            .iter()
            .all(|e| e.garrisoned_in.is_none())
    );
    for (i, passenger) in w.state.entities.iter().enumerate().skip(1) {
        assert!(w.map.can_move(
            passenger.position,
            w.unit_at(i).footprint,
            MovementClass::Ground
        ));
        for other in w.state.entities.iter().skip(i + 1) {
            assert!(!overlaps(
                passenger.position,
                w.unit_at(i).footprint,
                other.position,
                w.unit_type(other.unit_type).unwrap().footprint
            ));
        }
    }
}

#[test]
fn targeted_unload_travels_before_releasing_passengers_then_resumes_queued_movement() {
    let mut world = transport();
    let target = Position { x: 180, y: 180 };
    let next = Position { x: 220, y: 220 };
    assert_eq!(
        issue(
            &mut world,
            Order::UnloadAt {
                entity: EntityId(3),
                target
            }
        ),
        None
    );
    assert!(
        world.state.entities[..2]
            .iter()
            .all(|passenger| passenger.garrisoned_in == Some(EntityId(3)))
    );
    assert_eq!(
        issue(
            &mut world,
            Order::Queue {
                entity: EntityId(3),
                order: UnitOrder::Move { target: next }
            }
        ),
        None
    );
    let mut replay = world.clone();
    let mut unloaded = false;
    let mut exit_ticks = Vec::new();
    for _ in 0..60 {
        let before = world.state.entities[..2]
            .iter()
            .any(|passenger| passenger.garrisoned_in.is_some());
        let loaded_before = world.state.entities[..2]
            .iter()
            .filter(|passenger| passenger.garrisoned_in.is_some())
            .count();
        world.step(&[]).unwrap();
        let loaded_after = world.state.entities[..2]
            .iter()
            .filter(|passenger| passenger.garrisoned_in.is_some())
            .count();
        if loaded_after < loaded_before {
            assert_eq!(loaded_before - loaded_after, 1);
            exit_ticks.push(world.tick().0);
        }
        replay.step(&[]).unwrap();
        assert_eq!(world.state_hash(), replay.state_hash());
        if before
            && world.state.entities[..2]
                .iter()
                .all(|passenger| passenger.garrisoned_in.is_none())
        {
            assert_eq!(world.state.entities[2].position, target);
            for passenger in &world.state.entities[..2] {
                assert!(distance(passenger.position, target) <= 20_i64.pow(2));
            }
            unloaded = true;
        }
    }
    assert!(unloaded);
    assert_eq!(exit_ticks.len(), 2);
    assert_eq!(exit_ticks[1] - exit_ticks[0], 15);
    assert_eq!(world.state.entities[2].position, next);
}

#[test]
fn targeted_unload_retries_occupied_exits_without_dropping_its_destination() {
    let mut world = transport();
    let target = Position { x: 180, y: 180 };
    let mut blocker = world.state.entities[0].clone();
    blocker.id = EntityId(4);
    blocker.garrisoned_in = None;
    blocker.unit_type = UnitTypeId(4);
    blocker.position = target;
    let mut rules = world.rules().clone();
    rules.units.push(UnitType {
        id: UnitTypeId(4),
        speed: 8,
        footprint: Footprint {
            width: 64,
            height: 64,
        },
        ..Default::default()
    });
    world.rules = std::sync::Arc::new(rules);
    world.state.entities.push(blocker);
    assert_eq!(
        issue(
            &mut world,
            Order::UnloadAt {
                entity: EntityId(3),
                target
            }
        ),
        None
    );
    for _ in 0..30 {
        world.step(&[]).unwrap();
    }
    assert_eq!(world.state.entities[2].position, target);
    assert_eq!(
        world.state.entities[2].order,
        UnitOrder::UnloadAt { target }
    );
    assert!(
        world.state.entities[..2]
            .iter()
            .all(|passenger| passenger.garrisoned_in.is_some())
    );
    assert_eq!(
        issue(
            &mut world,
            Order::Move {
                entity: EntityId(4),
                target: Position { x: 48, y: 180 }
            }
        ),
        None
    );
    for _ in 0..30 {
        world.step(&[]).unwrap();
    }
    assert!(
        world.state.entities[..2]
            .iter()
            .all(|passenger| passenger.garrisoned_in.is_none())
    );
    assert_eq!(world.state.entities[2].order, UnitOrder::Idle);
}

#[test]
fn targeted_unload_rejects_stationary_empty_out_of_bounds_and_unwalkable_destinations() {
    let target = Position { x: 180, y: 180 };
    let mut bunker = world();
    load(&mut bunker, 0);
    assert_eq!(
        bunker.unload_at_rejection(EntityId(3), target),
        Some(Rejection::UnsupportedOrder)
    );
    let mut world = transport();
    assert_eq!(
        world.unload_at_rejection(EntityId(3), Position { x: -1, y: 180 }),
        Some(Rejection::OutOfBounds)
    );
    let mut map = world.map().clone();
    map.terrain = Some(crate::map::Terrain {
        cell_size: 32,
        columns: 8,
        rows: 8,
        flags: vec![0; 64],
    });
    world.map = std::sync::Arc::new(map);
    assert_eq!(
        world.unload_at_rejection(EntityId(3), target),
        Some(Rejection::InvalidPlacement)
    );
    world.unload_garrison(2, true);
    assert_eq!(
        world.unload_at_rejection(EntityId(3), target),
        Some(Rejection::InvalidTarget)
    );
}
