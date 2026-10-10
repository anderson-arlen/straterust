use super::*;

#[test]
fn eight_facing_units_move_on_supported_angles_and_keep_exact_goals_across_loads() {
    let mut world = moving(Some(Motion {
        eight_directions: true,
        speed: 8 * 256,
        acceleration: 0,
        steps: Vec::new(),
    }));
    let target = point(511, 277);
    send(
        &mut world,
        Order::Move {
            entity: EntityId(1),
            target,
        },
    );
    let mut restored = world
        .restore_snapshot(world.save_snapshot().unwrap())
        .unwrap();
    let mut turns = 0;
    let mut last_heading = [0, 0];
    for _ in 0..100 {
        let from = precise(entity(&world, 1));
        world.step(&[]).unwrap();
        restored.step(&[]).unwrap();
        let to = precise(entity(&world, 1));
        let dx = to[0] - from[0];
        let dy = to[1] - from[1];
        assert!(
            dx == 0 || dy == 0 || dx.abs() == dy.abs(),
            "unsupported travel vector {dx},{dy}"
        );
        let heading = [dx.signum(), dy.signum()];
        if heading != [0, 0] && heading != last_heading {
            turns += 1;
            last_heading = heading;
        }
        assert_eq!(world.state_hash(), restored.state_hash());
        if entity(&world, 1).position == target {
            break;
        }
    }
    assert_eq!(entity(&world, 1).position, target);
    assert!(turns <= 2, "a clear route should not zigzag");
}

#[test]
fn eight_direction_routes_keep_obstacle_clearance_and_do_not_change_continuous_movement() {
    let mut world = moving(Some(Motion {
        eight_directions: true,
        speed: 8 * 256,
        acceleration: 0,
        steps: Vec::new(),
    }));
    let mut map = world.map().clone();
    map.terrain = Some(Terrain {
        cell_size: 32,
        columns: 32,
        rows: 20,
        flags: (0..640)
            .map(|i| if i % 32 == 8 && i / 32 < 12 { 0 } else { 3 })
            .collect(),
    });
    world = World::new(world.rules().clone(), map, 7).unwrap();
    let target = point(511, 277);
    send(
        &mut world,
        Order::Move {
            entity: EntityId(1),
            target,
        },
    );
    for _ in 0..200 {
        let from = precise(entity(&world, 1));
        world.step(&[]).unwrap();
        let to = entity(&world, 1).position;
        let precise_to = precise(entity(&world, 1));
        let dx = precise_to[0] - from[0];
        let dy = precise_to[1] - from[1];
        assert!(
            dx == 0 || dy == 0 || dx.abs() == dy.abs(),
            "unsupported vector {dx},{dy} from {from:?} to {precise_to:?}"
        );
        assert!(
            world
                .map()
                .can_move(to, Footprint::default(), MovementClass::Ground)
        );
        if to == target {
            break;
        }
    }
    assert_eq!(entity(&world, 1).position, target);
    let mut continuous = moving(None);
    let from = precise(entity(&continuous, 1));
    send(
        &mut continuous,
        Order::Move {
            entity: EntityId(1),
            target,
        },
    );
    let to = precise(entity(&continuous, 1));
    assert_ne!((to[0] - from[0]).abs(), (to[1] - from[1]).abs());
    assert!(to[0] != from[0] && to[1] != from[1]);
}
