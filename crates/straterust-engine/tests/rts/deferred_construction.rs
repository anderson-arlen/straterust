use super::*;

fn ordered_world() -> World {
    let mut world = construction_world();
    assert_eq!(
        send(
            &mut world,
            Order::Build {
                entity: EntityId(1),
                unit_type: UnitTypeId(4),
                position: point(200, 100),
            }
        ),
        None
    );
    world
}

#[test]
fn resumed_foundation_stays_visible_while_the_worker_returns() {
    let base = construction_world();
    let mut rules = base.rules().clone();
    rules
        .units
        .iter_mut()
        .find(|u| u.id == UnitTypeId(4))
        .unwrap()
        .build_ticks = 1000;
    let mut world = World::new(rules, base.map().clone(), 42).unwrap();
    assert_eq!(
        send(
            &mut world,
            Order::Build {
                entity: EntityId(1),
                unit_type: UnitTypeId(4),
                position: point(200, 100)
            }
        ),
        None
    );
    for _ in 0..200 {
        run(&mut world, 1);
        if world.state().entities.len() == 2 {
            break;
        }
    }
    assert!(!world.construction_pending(entity(&world, 2)));
    assert_eq!(
        send(
            &mut world,
            Order::Move {
                entity: EntityId(1),
                target: point(30, 200)
            }
        ),
        None
    );
    run(&mut world, 25);
    let remaining = entity(&world, 2).construction.as_ref().unwrap().remaining;
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
    assert!(
        entity(&world, 2)
            .construction
            .as_ref()
            .unwrap()
            .work_position
            .is_none()
    );
    assert!(!world.construction_pending(entity(&world, 2)));
    let projected = world.player_view(PlayerId(0)).unwrap();
    assert!(projected.entities.iter().any(|e| match e {
        straterust_engine::sim::ViewedEntity::Owned(e) => e.id == EntityId(2),
        straterust_engine::sim::ViewedEntity::Visible(e) => e.id == EntityId(2),
    }));
    let mut restored = world
        .restore_snapshot(world.save_snapshot().unwrap())
        .unwrap();
    run(&mut world, 1);
    run(&mut restored, 1);
    assert_eq!(world.state_hash(), restored.state_hash());
    assert_eq!(
        entity(&world, 2).construction.as_ref().unwrap().remaining,
        remaining
    );
    assert!(!world.construction_pending(entity(&world, 2)));
    run(&mut world, 100);
    assert!(entity(&world, 2).construction.as_ref().unwrap().remaining < remaining);
}

#[test]
fn construction_travel_is_unpaid_and_saved_until_the_builder_arrives() {
    let mut world = ordered_world();
    let balance = world.resource_balance(PlayerId(0), "ore");
    assert_eq!(balance, 50);
    assert_eq!(world.state().entities.len(), 1);
    assert!(matches!(
        entity(&world, 1).order,
        UnitOrder::PlaceBuilding { .. }
    ));
    let snapshot = world.save_snapshot().unwrap();
    let mut restored = world.restore_snapshot(snapshot).unwrap();
    for _ in 0..8 {
        world.step(&[]).unwrap();
        restored.step(&[]).unwrap();
        assert_eq!(world.state_hash(), restored.state_hash());
        assert_eq!(world.resource_balance(PlayerId(0), "ore"), balance);
        assert_eq!(world.state().entities.len(), 1);
    }
    for _ in 0..100 {
        world.step(&[]).unwrap();
        restored.step(&[]).unwrap();
        assert_eq!(world.state_hash(), restored.state_hash());
        if world.state().entities.len() == 2 {
            break;
        }
    }
    let building = entity(&world, 2);
    assert_eq!(building.position, point(200, 100));
    assert!(
        building
            .construction
            .as_ref()
            .unwrap()
            .work_position
            .is_some()
    );
    assert_eq!(world.resource_balance(PlayerId(0), "ore"), balance - 10);
    run(&mut world, 10);
    assert_eq!(world.resource_balance(PlayerId(0), "ore"), balance - 10);
}

#[test]
fn stopping_or_cancelling_construction_travel_spends_nothing() {
    for cancel in [false, true] {
        let mut world = ordered_world();
        let order = if cancel {
            Order::Cancel {
                entity: EntityId(1),
            }
        } else {
            Order::Stop {
                entity: EntityId(1),
            }
        };
        assert_eq!(send(&mut world, order), None);
        run(&mut world, 100);
        assert_eq!(world.state().entities.len(), 1);
        assert_eq!(world.resource_balance(PlayerId(0), "ore"), 50);
        assert_eq!(entity(&world, 1).order, UnitOrder::Idle);
    }
}

#[test]
fn construction_rechecks_funds_and_placement_at_arrival() {
    for same_site in [false, true] {
        let base = construction_world();
        let mut rules = base.rules().clone();
        rules.starting_resources = vec![amount(if same_site { 50 } else { 10 })];
        let mut map = base.map().clone();
        map.spawns.push(spawn(0, 2, 223, 100));
        let mut world = World::new(rules, map, 42).unwrap();
        let destination = if same_site {
            point(200, 100)
        } else {
            point(70, 200)
        };
        assert_eq!(
            send(
                &mut world,
                Order::Build {
                    entity: EntityId(1),
                    unit_type: UnitTypeId(4),
                    position: destination
                }
            ),
            None
        );
        // The other worker uses the remaining money or occupies the site.
        assert_eq!(
            send(
                &mut world,
                Order::Build {
                    entity: EntityId(2),
                    unit_type: UnitTypeId(4),
                    position: point(200, 100)
                }
            ),
            None
        );
        run(&mut world, 100);
        assert_eq!(world.state().entities.len(), 3);
        assert_eq!(
            world.resource_balance(PlayerId(0), "ore"),
            if same_site { 40 } else { 0 }
        );
        assert!(if same_site {
            entity(&world, 1).order == UnitOrder::Idle
        } else {
            matches!(entity(&world, 1).order, UnitOrder::PlaceBuilding { .. })
        });
    }
}

#[test]
fn unreachable_construction_stays_unpaid_without_a_foundation() {
    use straterust_engine::map::{BUILDABLE, Terrain, WALKABLE};
    let base = construction_world();
    let mut map = base.map().clone();
    let mut flags = vec![WALKABLE | BUILDABLE; 32 * 32];
    for row in 0..32 {
        flags[row * 32 + 16] = 0;
    }
    map.terrain = Some(Terrain {
        cell_size: 8,
        columns: 32,
        rows: 32,
        flags,
    });
    let mut world = World::new(base.rules().clone(), map, 42).unwrap();
    assert_eq!(
        send(
            &mut world,
            Order::Build {
                entity: EntityId(1),
                unit_type: UnitTypeId(4),
                position: point(200, 100)
            }
        ),
        None
    );
    run(&mut world, 100);
    assert_eq!(world.state().entities.len(), 1);
    assert_eq!(world.resource_balance(PlayerId(0), "ore"), 50);
    assert!(matches!(
        entity(&world, 1).order,
        UnitOrder::PlaceBuilding { .. }
    ));
}
