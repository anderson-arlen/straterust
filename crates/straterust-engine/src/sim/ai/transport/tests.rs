use super::*;
use crate::{
    content::Package,
    map::{Terrain, WALKABLE, WATER},
};

fn water_transport() -> (World, EntityId, EntityId) {
    let base = Package::load(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/terran-demo"),
    )
    .unwrap()
    .world(42)
    .unwrap();
    let mut rules = base.rules().clone();
    rules.victory = false;
    rules
        .units
        .iter_mut()
        .find(|u| u.id == UnitTypeId(2))
        .unwrap()
        .footprint = Footprint {
        width: 512,
        height: 512,
    };
    let mut ship = rules
        .units
        .iter()
        .find(|u| u.id == UnitTypeId(1))
        .unwrap()
        .clone();
    ship.id = UnitTypeId(101);
    ship.weapon = None;
    ship.movement_class = MovementClass::Water;
    ship.garrison = Some(GarrisonStats {
        boarding_range: 32,
        capacity: 8,
        passengers: vec![UnitTypeId(1), UnitTypeId(2)],
        attackers: vec![],
        range_bonus: 0,
        unload_ticks: 0,
    });
    rules.units.push(ship);
    let mut map = base.map().clone();
    map.width = 512;
    map.height = 256;
    map.ai.clear();
    map.resources.clear();
    map.start_locations.clear();
    map.mission = None;
    map.terrain = Some(Terrain {
        cell_size: 32,
        columns: 16,
        rows: 8,
        flags: (0..128)
            .map(|i| {
                if i % 16 >= 8 || i % 16 == 3 {
                    WALKABLE
                } else {
                    WATER
                }
            })
            .collect(),
    });
    map.spawns = vec![
        Spawn {
            unit_type: UnitTypeId(101),
            position: Position { x: 176, y: 128 },
            ..Default::default()
        },
        Spawn {
            unit_type: UnitTypeId(1),
            position: Position { x: 400, y: 128 },
            ..Default::default()
        },
    ];
    let mut world = World::new(rules, map, 42).unwrap();
    let ship = world.state.entities[0].id;
    let passenger = world.state.entities[1].id;
    world.state.entities[1].garrisoned_in = Some(ship);
    (world, ship, passenger)
}

#[test]
fn repeated_landings_preserve_state_and_recheck_changed_trip_inputs() {
    let (mut world, ship, passenger) = water_transport();
    let target = Position { x: 400, y: 128 };
    let before = world.state_hash();
    let expected = world.ai_water_drop_point(ship, target).unwrap();
    for _ in 0..8 {
        assert_eq!(world.ai_drop_point(ship, target), Some(expected));
    }
    assert_eq!(world.ai_drop_points.len(), 1);
    assert_eq!(world.state_hash(), before);
    // Ground troops cannot reach a destination in the sea.
    assert_eq!(world.ai_drop_point(ship, Position { x: 176, y: 128 }), None);
    // This separate pool has no landing connected to the enemy's landmass.
    let ship_index = world.index(ship).unwrap();
    world.state.entities[ship_index].position = Position { x: 48, y: 128 };
    assert_eq!(world.ai_drop_point(ship, target), None);
    world.state.entities[ship_index].position = Position { x: 176, y: 128 };
    assert_eq!(world.ai_drop_point(ship, target), Some(expected));
    // A newly boarded type can require an entirely different shore clearance.
    let passenger_index = world.index(passenger).unwrap();
    world.state.entities[passenger_index].unit_type = UnitTypeId(2);
    assert_eq!(world.ai_drop_point(ship, target), None);
    assert_eq!(world.ai_drop_points.len(), 4);
    assert!(world.snapshot().ai_drop_points.is_empty());
}

#[test]
fn failed_landings_are_bounded_and_survive_only_in_the_derived_cache() {
    let (mut world, ship, _) = water_transport();
    for x in 0..=LANDING_CACHE_LIMIT {
        assert_eq!(
            world.ai_drop_point(ship, Position { x: x as i32, y: 0 }),
            None
        );
        assert!(world.ai_drop_points.len() <= LANDING_CACHE_LIMIT);
    }
    let saved = world.save_snapshot().unwrap();
    let mut restored = world.restore_snapshot(saved).unwrap();
    assert!(restored.ai_drop_points.is_empty());
    assert_eq!(restored.state_hash(), world.state_hash());
    assert_eq!(
        restored.ai_drop_point(ship, Position { x: 400, y: 128 }),
        world.ai_drop_point(ship, Position { x: 400, y: 128 })
    );
}
