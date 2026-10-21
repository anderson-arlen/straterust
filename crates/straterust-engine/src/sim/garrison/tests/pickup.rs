use super::*;

#[test]
fn reachable_idle_transports_wait_for_passengers_instead_of_chasing_them() {
    use crate::map::{BUILDABLE, Terrain, WALKABLE, WATER};
    for class in [MovementClass::Air, MovementClass::Water] {
        let old = world();
        let mut rules = old.rules().clone();
        rules.units[0].weapon = None;
        let ship = &mut rules.units[2];
        ship.structure = false;
        ship.speed = 8;
        ship.movement_class = class;
        ship.garrison.as_mut().unwrap().attackers.clear();
        ship.garrison.as_mut().unwrap().boarding_range = 16;
        let mut map = old.map().clone();
        map.spawns.truncate(3);
        map.spawns[0].position = Position { x: 32, y: 32 };
        // Another mobile unit occupies the nearest shore entrance.
        map.spawns[1].position = Position { x: 124, y: 192 };
        let landing = Position { x: 144, y: 192 };
        map.spawns[2].position = landing;
        if class == MovementClass::Water {
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
        }
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
        for _ in 0..100 {
            assert_eq!(
                w.state.entities[2].position, landing,
                "{class:?} moved away from boarding shore"
            );
            if w.state.entities[0].garrisoned_in.is_some() {
                break;
            }
            w.step(&[]).unwrap();
        }
        assert_eq!(w.state.entities[0].garrisoned_in, Some(EntityId(3)));
    }
}

#[test]
fn moving_transport_and_passenger_rendezvous_then_resume_the_transport_order() {
    let old = world();
    let mut rules = old.rules().clone();
    rules.units[2].structure = false;
    rules.units[2].speed = 8;
    rules.units[2].movement_class = MovementClass::Air;
    rules.units[2].garrison.as_mut().unwrap().attackers.clear();
    let mut map = old.map().clone();
    map.spawns[0].position = Position { x: 32, y: 64 };
    map.spawns[2].position = Position { x: 208, y: 64 };
    map.spawns[3].position = Position { x: 240, y: 240 };
    let mut w = World::new(rules, map, 42).unwrap();
    let destination = Position { x: 208, y: 208 };
    assert_eq!(
        issue(
            &mut w,
            Order::Move {
                entity: EntityId(3),
                target: destination
            }
        ),
        None
    );
    let transport = w.state.entities[2].position;
    let passenger = w.state.entities[0].position;
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
    assert!(w.state.entities[0].position.x > passenger.x);
    assert!(w.state.entities[2].position.x < transport.x);
    assert_eq!(
        w.state.entities[2].order,
        UnitOrder::Pickup {
            target: EntityId(1)
        }
    );
    let mut replay = w.clone();
    for _ in 0..60 {
        w.step(&[]).unwrap();
        replay.step(&[]).unwrap();
    }
    assert_eq!(w.state_hash(), replay.state_hash());
    assert_eq!(w.state.entities[0].garrisoned_in, Some(EntityId(3)));
    assert_eq!(w.state.entities[2].position, destination);
    assert_eq!(w.state.entities[0].position, destination);
}
