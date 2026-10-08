use super::*;

#[test]
fn range_goal_stops_on_this_side_of_a_wall_even_when_the_center_is_reachable() {
    let map = map(64, 64);
    let start = Position { x: 64, y: 64 };
    let target = Position { x: 448, y: 64 };
    let footprint = Footprint {
        width: 8,
        height: 8,
    };
    let obstacles = [Obstacle {
        position: Position { x: 352, y: 224 },
        footprint: Footprint {
            width: 32,
            height: 448,
        },
        movement_class: MovementClass::Ground,
    }];
    assert!(
        find_path(
            &map,
            footprint,
            MovementClass::Ground,
            start,
            target,
            &obstacles
        )
        .unwrap()
        .iter()
        .any(|p| p.y > 448)
    );
    let path = find_path_in_range(
        &map,
        footprint,
        MovementClass::Ground,
        start,
        target,
        192,
        &obstacles,
    )
    .unwrap();
    assert_eq!(path, [Position { x: 256, y: 64 }]);
    assert_clear(&map, footprint, start, &path, &obstacles);
}

#[test]
fn blocked_ideal_range_point_uses_another_nearby_firing_position() {
    let map = map(64, 32);
    let start = Position { x: 64, y: 64 };
    let target = Position { x: 320, y: 64 };
    let footprint = Footprint {
        width: 8,
        height: 8,
    };
    let obstacles = [Obstacle {
        position: Position { x: 192, y: 64 },
        footprint: Footprint {
            width: 32,
            height: 64,
        },
        movement_class: MovementClass::Ground,
    }];
    let path = find_path_in_range(
        &map,
        footprint,
        MovementClass::Ground,
        start,
        target,
        128,
        &obstacles,
    )
    .unwrap();
    assert!(path.len() > 1, "the closest boundary point is blocked");
    assert_clear(&map, footprint, start, &path, &obstacles);
    let endpoint = path.last().unwrap();
    let square =
        (i64::from(endpoint.x - target.x).pow(2) + i64::from(endpoint.y - target.y).pow(2)) as u64;
    assert!((126u64.pow(2)..=128u64.pow(2)).contains(&square));
    assert!(path.iter().all(|p| p.x < target.x));
}

#[test]
fn inaccessible_range_is_a_failed_route_and_air_can_cross_the_ground_barrier() {
    let mut map = map(16, 16);
    for y in 0..16 {
        block(&mut map, 8, y);
    }
    let start = Position { x: 12, y: 64 };
    let target = Position { x: 100, y: 64 };
    let footprint = Footprint {
        width: 4,
        height: 4,
    };
    assert!(
        find_path_in_range(
            &map,
            footprint,
            MovementClass::Ground,
            start,
            target,
            20,
            &[]
        )
        .is_none()
    );
    assert_eq!(
        find_path_in_range(&map, footprint, MovementClass::Air, start, target, 20, &[]).unwrap(),
        [Position { x: 80, y: 64 }]
    );
    assert_eq!(
        find_path_in_range(
            &map,
            footprint,
            MovementClass::Ground,
            start,
            start,
            20,
            &[]
        )
        .unwrap(),
        []
    );
}
