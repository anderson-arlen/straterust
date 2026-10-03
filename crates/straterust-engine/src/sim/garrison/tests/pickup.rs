use super::*;

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
