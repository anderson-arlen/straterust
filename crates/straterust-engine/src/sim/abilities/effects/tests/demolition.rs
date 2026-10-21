use super::*;

fn demolition() -> World {
    let base = world(AbilityEffect::Demolish {
        radius: 96,
        damage: 400,
        resources: vec!["wood".into(), "rock".into()],
    });
    let mut rules = (*base.rules).clone();
    rules.units.push(UnitType {
        id: UnitTypeId(7),
        movement_class: crate::map::MovementClass::Air,
        max_hp: 1000,
        speed: 8,
        ..Default::default()
    });
    let mut map = (*base.map).clone();
    map.spawns.push(Spawn {
        owner: PlayerId(1),
        unit_type: UnitTypeId(7),
        position: Position { x: 128, y: 64 },
        ..Default::default()
    });
    map.resources = [
        ("wood", 64, 32),
        ("rock", 32, 64),
        ("wood", 240, 240),
        ("gold", 32, 32),
    ]
    .into_iter()
    .map(|(kind, x, y)| ResourceSpawn {
        kind: kind.into(),
        position: Position { x, y },
        amount: 100,
        footprint: Footprint {
            width: 32,
            height: 32,
        },
        requires_extractor: false,
        terrain_corners: None,
    })
    .collect();
    World::new(rules, map, 1).unwrap()
}

#[test]
fn suicide_blast_clears_only_authored_terrain_and_hits_ground_units_on_either_side_once() {
    let mut w = demolition();
    cast(&mut w, AbilityTarget::Point(Position { x: 64, y: 64 }));
    assert!(
        w.index(EntityId(1)).is_none(),
        "the demolition unit is consumed"
    );
    assert_eq!(
        w.state
            .resources
            .iter()
            .map(|r| r.amount)
            .collect::<Vec<_>>(),
        [0, 0, 100, 100]
    );
    ticks(&mut w, 2);
    assert_eq!(
        w.state
            .entities
            .iter()
            .find(|e| e.id == EntityId(2))
            .unwrap()
            .hp,
        700
    );
    assert!(
        w.index(EntityId(3)).is_none(),
        "friendly fire is intentional"
    );
    assert_eq!(
        w.state
            .entities
            .iter()
            .find(|e| e.id == EntityId(4))
            .unwrap()
            .hp,
        100
    );
    assert_eq!(
        w.state
            .entities
            .iter()
            .find(|e| e.unit_type == UnitTypeId(7))
            .unwrap()
            .hp,
        1000
    );
    assert!(w.state.ability_fields.is_empty());
    let mut restored = w.restore_snapshot(w.save_snapshot().unwrap()).unwrap();
    ticks(&mut w, 5);
    ticks(&mut restored, 5);
    assert_eq!(w.state_hash(), restored.state_hash());
}

#[test]
fn demolition_walks_to_blocked_terrain_before_consuming_itself_and_opens_the_route() {
    let base = demolition();
    let mut rules = (*base.rules).clone();
    rules.units[0].abilities[0].range = 32;
    let mut map = (*base.map).clone();
    map.spawns.truncate(1);
    map.spawns[0].position = Position { x: 64, y: 64 };
    map.resources.truncate(1);
    map.resources[0].position = Position { x: 240, y: 64 };
    let mut w = World::new(rules, map, 1).unwrap();
    cast(&mut w, AbilityTarget::Point(Position { x: 240, y: 64 }));
    assert!(w.index(EntityId(1)).is_some());
    assert_eq!(w.state.resources[0].amount, 100);
    for _ in 0..60 {
        if w.index(EntityId(1)).is_none() {
            break;
        }
        w.step(&[]).unwrap();
    }
    assert!(
        w.index(EntityId(1)).is_none(),
        "must approach the obstructed tile"
    );
    assert_eq!(w.state.resources[0].amount, 0);
    assert!(!w.resource_blocks_movement(&w.state.resources[0]));
}
