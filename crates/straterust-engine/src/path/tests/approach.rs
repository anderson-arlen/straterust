use super::*;

#[test]
fn occupied_destination_uses_nearest_clear_stop_and_preserves_exact_search() {
    let map = map(16, 16);
    let footprint = Footprint {
        width: 7,
        height: 7,
    };
    let start = Position { x: 12, y: 60 };
    let target = Position { x: 100, y: 60 };
    let obstacles = [Obstacle {
        position: target,
        footprint,
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
        .is_none()
    );
    let expected = vec![Position { x: 92, y: 60 }];
    for _ in 0..10 {
        let path = find_path_near(
            &map,
            footprint,
            MovementClass::Ground,
            start,
            target,
            &obstacles,
        )
        .unwrap();
        assert_eq!(path, expected);
        assert_clear(&map, footprint, start, &path, &obstacles);
    }
    assert_eq!(
        find_path_near(
            &map,
            footprint,
            MovementClass::Ground,
            expected[0],
            target,
            &obstacles
        ),
        Some(vec![])
    );
    // Ground occupants do not displace an air unit's destination.
    assert_eq!(
        find_path_near(
            &map,
            footprint,
            MovementClass::Air,
            start,
            target,
            &obstacles
        ),
        Some(vec![target])
    );
}

#[test]
fn occupied_cluster_uses_outer_edge_independent_of_obstacle_order() {
    let map = map(20, 16);
    let footprint = Footprint {
        width: 7,
        height: 7,
    };
    let start = Position { x: 12, y: 60 };
    let target = Position { x: 100, y: 60 };
    let mut obstacles: Vec<_> = (52..=68)
        .step_by(8)
        .flat_map(|y| {
            (92..=108).step_by(8).map(move |x| Obstacle {
                position: Position { x, y },
                footprint,
                movement_class: MovementClass::Ground,
            })
        })
        .collect();
    let first = find_path_near(
        &map,
        footprint,
        MovementClass::Ground,
        start,
        target,
        &obstacles,
    )
    .unwrap();
    assert_eq!(first, vec![Position { x: 84, y: 60 }]);
    assert_clear(&map, footprint, start, &first, &obstacles);
    obstacles.reverse();
    assert_eq!(
        find_path_near(
            &map,
            footprint,
            MovementClass::Ground,
            start,
            target,
            &obstacles
        ),
        Some(first)
    );
}

#[test]
fn inaccessible_closer_stops_do_not_hide_nearest_reachable_stop() {
    let mut map = map(16, 12);
    for y in 0..12 {
        block(&mut map, 8, y);
    }
    let footprint = Footprint {
        width: 7,
        height: 7,
    };
    let start = Position { x: 12, y: 44 };
    let target = Position { x: 76, y: 44 };
    let obstacles = [Obstacle {
        position: target,
        footprint,
        movement_class: MovementClass::Ground,
    }];
    let path = find_path_near(
        &map,
        footprint,
        MovementClass::Ground,
        start,
        target,
        &obstacles,
    )
    .unwrap();
    assert_eq!(path.last(), Some(&Position { x: 60, y: 44 }));
    assert_clear(&map, footprint, start, &path, &obstacles);
    assert_eq!(
        find_path_near(
            &map,
            footprint,
            MovementClass::Ground,
            *path.last().unwrap(),
            target,
            &obstacles
        ),
        Some(vec![])
    );
    // Removing the occupant does not reconnect the island. Near movement
    // still stops at the reachable edge; the exact API still rejects it.
    assert_eq!(
        find_path_near(&map, footprint, MovementClass::Ground, start, target, &[]),
        Some(path)
    );
    assert!(find_path(&map, footprint, MovementClass::Ground, start, target, &[]).is_none());
}

#[test]
fn disconnected_clear_destination_uses_stable_nearest_edge_for_dynamic_walls() {
    let map = map(16, 12);
    let start = Position { x: 12, y: 48 };
    let target = Position { x: 100, y: 48 };
    let mut obstacles = [
        Obstacle {
            position: Position { x: 68, y: 48 },
            footprint: Footprint {
                width: 8,
                height: 96,
            },
            movement_class: MovementClass::Ground,
        },
        Obstacle {
            position: Position { x: 300, y: 300 },
            footprint: Footprint::default(),
            movement_class: MovementClass::Ground,
        },
    ];
    for (width, expected_x) in [(7, 60), (15, 52)] {
        let footprint = Footprint { width, height: 7 };
        let first = find_path_near(
            &map,
            footprint,
            MovementClass::Ground,
            start,
            target,
            &obstacles,
        )
        .unwrap();
        // Equidistant rows also have equal route cost: row-major wins.
        assert_eq!(
            first.last(),
            Some(&Position {
                x: expected_x,
                y: 44
            })
        );
        assert_clear(&map, footprint, start, &first, &obstacles);
        obstacles.reverse();
        assert_eq!(
            find_path_near(
                &map,
                footprint,
                MovementClass::Ground,
                start,
                target,
                &obstacles,
            ),
            Some(first.clone())
        );
        assert_eq!(
            find_path_near(
                &map,
                footprint,
                MovementClass::Ground,
                *first.last().unwrap(),
                target,
                &obstacles,
            ),
            Some(Vec::new())
        );
        assert_eq!(
            find_path_near(
                &map,
                footprint,
                MovementClass::Air,
                start,
                target,
                &obstacles,
            ),
            Some(vec![target])
        );
    }
}

#[test]
fn off_grid_unit_without_start_connector_retries_instead_of_arriving() {
    let map = map(32, 40);
    let start = Position { x: 58, y: 133 };
    let target = Position { x: 59, y: 255 };
    let footprint = Footprint {
        width: 23,
        height: 23,
    };
    let obstacle = |position, footprint| Obstacle {
        position,
        footprint,
        movement_class: MovementClass::Ground,
    };
    let obstacles = [
        obstacle(
            Position { x: 96, y: 80 },
            Footprint {
                width: 117,
                height: 83,
            },
        ),
        obstacle(Position { x: 34, y: 133 }, footprint),
        obstacle(Position { x: 82, y: 133 }, footprint),
        obstacle(target, footprint),
    ];
    assert!(segment_clear(
        &map,
        footprint,
        MovementClass::Ground,
        start,
        start,
        &obstacles
    ));
    assert_eq!(
        find_path_near(
            &map,
            footprint,
            MovementClass::Ground,
            start,
            target,
            &obstacles
        ),
        None
    );
    // Neighbors leave the exit; the same unchanged order can now approach.
    let route = find_path_near(
        &map,
        footprint,
        MovementClass::Ground,
        start,
        target,
        &[obstacles[0], obstacles[3]],
    )
    .unwrap();
    assert!(!route.is_empty());
    assert_clear(
        &map,
        footprint,
        start,
        &route,
        &[obstacles[0], obstacles[3]],
    );
    assert!(distance(*route.last().unwrap(), target) <= 300);
}

#[test]
fn disconnected_clear_destination_keeps_already_nearest_trapped_start() {
    let mut map = map(16, 12);
    map.terrain.as_mut().unwrap().flags.fill(0);
    map.terrain.as_mut().unwrap().flags[17] = WALKABLE;
    map.terrain.as_mut().unwrap().flags[26] = WALKABLE;
    let start = Position { x: 12, y: 12 };
    let target = Position { x: 84, y: 12 };
    let footprint = Footprint {
        width: 7,
        height: 7,
    };
    assert_eq!(
        find_path_near(&map, footprint, MovementClass::Ground, start, target, &[],),
        Some(Vec::new())
    );
    assert!(find_path(&map, footprint, MovementClass::Ground, start, target, &[]).is_none());
}

#[test]
fn blocked_rally_routes_through_gap_and_clear_targets_keep_exact_paths() {
    let mut map = map(16, 16);
    for y in 0..16 {
        if y != 10 {
            block(&mut map, 8, y);
        }
    }
    let footprint = Footprint {
        width: 7,
        height: 7,
    };
    let start = Position { x: 12, y: 20 };
    let target = Position { x: 100, y: 20 };
    let obstacles = [Obstacle {
        position: target,
        footprint,
        movement_class: MovementClass::Ground,
    }];
    let path = find_path_near(
        &map,
        footprint,
        MovementClass::Ground,
        start,
        target,
        &obstacles,
    )
    .unwrap();
    assert_eq!(distance(*path.last().unwrap(), target), 80);
    assert!(path.iter().any(|point| point.y == 84));
    assert_clear(&map, footprint, start, &path, &obstacles);
    assert_eq!(
        find_path_near(&map, footprint, MovementClass::Ground, start, target, &[]),
        find_path(&map, footprint, MovementClass::Ground, start, target, &[])
    );
    assert_eq!(
        find_path_near(&map, footprint, MovementClass::Ground, start, start, &[]),
        Some(vec![])
    );
}

#[test]
fn near_destinations_respect_edges_and_reject_invalid_inputs() {
    let map = map(16, 16);
    let footprint = Footprint {
        width: 7,
        height: 7,
    };
    let start = Position { x: 60, y: 60 };
    for (target, endpoint) in [
        (Position { x: 0, y: 0 }, Position { x: 4, y: 4 }),
        (Position { x: 127, y: 127 }, Position { x: 124, y: 124 }),
    ] {
        let path =
            find_path_near(&map, footprint, MovementClass::Ground, start, target, &[]).unwrap();
        assert_eq!(path, vec![endpoint]);
        assert_clear(&map, footprint, start, &path, &[]);
        assert_eq!(
            find_path_near(
                &map,
                footprint,
                MovementClass::Ground,
                endpoint,
                target,
                &[]
            ),
            Some(vec![])
        );
    }
    for target in [
        Position { x: -1, y: 0 },
        Position { x: 128, y: 60 },
        Position {
            x: i32::MAX,
            y: i32::MIN,
        },
    ] {
        assert!(
            find_path_near(&map, footprint, MovementClass::Ground, start, target, &[]).is_none()
        );
    }
    assert!(
        find_path_near(
            &map,
            footprint,
            MovementClass::Ground,
            Position { x: 0, y: 0 },
            start,
            &[]
        )
        .is_none()
    );
    assert!(
        find_path_near(
            &map,
            Footprint {
                width: 0,
                height: 1
            },
            MovementClass::Ground,
            start,
            start,
            &[]
        )
        .is_none()
    );
}
