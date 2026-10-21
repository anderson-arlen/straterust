use super::*;

#[test]
fn obstacle_broad_phase_matches_exact_sweeps_for_partial_cells_and_classes() {
    let map = map(32, 32);
    let grid = Grid::new(&map).unwrap();
    let mut seed = 7_u64;
    let mut random = || {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        (seed >> 32) as u32
    };
    let mut obstacles: Vec<_> = (0..80)
        .map(|index| Obstacle {
            position: Position {
                x: (random() % 288) as i32 - 16,
                y: (random() % 288) as i32 - 16,
            },
            footprint: Footprint {
                width: (random() % 24) as u16,
                height: (random() % 24) as u16,
            },
            movement_class: if index % 2 == 0 {
                MovementClass::Ground
            } else {
                MovementClass::Air
            },
        })
        .collect();
    obstacles.push(Obstacle {
        position: Position { x: 16, y: 90 },
        footprint: Footprint {
            width: 320,
            height: 5,
        },
        movement_class: MovementClass::Ground,
    });
    obstacles.push(Obstacle {
        position: Position {
            x: i32::MIN,
            y: i32::MAX,
        },
        footprint: Footprint {
            width: u16::MAX,
            height: u16::MAX,
        },
        movement_class: MovementClass::Ground,
    });
    for class in [MovementClass::Ground, MovementClass::Air] {
        for footprint in [
            Footprint::default(),
            Footprint {
                width: 7,
                height: 9,
            },
            Footprint {
                width: 32,
                height: 48,
            },
        ] {
            let data = ClearanceData::new(&map, grid, class, &obstacles);
            let clearance = Clearance {
                map: &map,
                grid,
                footprint,
                data: &data,
            };
            for _ in 0..2000 {
                let start = Position {
                    x: (random() % 288) as i32 - 16,
                    y: (random() % 288) as i32 - 16,
                };
                let end = Position {
                    x: start.x + (random() % 33) as i32 - 16,
                    y: start.y + (random() % 33) as i32 - 16,
                };
                assert_eq!(
                    clearance.clear(start, end),
                    segment_clear(&map, footprint, class, start, end, &obstacles),
                    "{class:?} {footprint:?} {start:?}->{end:?}"
                );
            }
        }
    }
    let touching = [Obstacle {
        position: Position { x: 12, y: 12 },
        footprint: Footprint {
            width: 2,
            height: 2,
        },
        movement_class: MovementClass::Ground,
    }];
    let data = ClearanceData::new(&map, grid, MovementClass::Ground, &touching);
    let clearance = Clearance {
        map: &map,
        grid,
        footprint: Footprint::default(),
        data: &data,
    };
    assert!(clearance.clear(Position { x: 10, y: 12 }, Position { x: 10, y: 12 }));
    assert!(!clearance.clear(Position { x: 11, y: 12 }, Position { x: 11, y: 12 }));
}

#[test]
fn exact_obstacle_checks_stay_local_on_a_dense_map() {
    let map = map(512, 32);
    let obstacles: Vec<_> = (0..512)
        .flat_map(|x| {
            (0..32).map(move |y| Obstacle {
                position: Position {
                    x: x * 8 + 2,
                    y: y * 8 + 2,
                },
                footprint: Footprint {
                    width: 2,
                    height: 2,
                },
                movement_class: MovementClass::Ground,
            })
        })
        .collect();
    let grid = Grid::new(&map).unwrap();
    let data = ClearanceData::new(&map, grid, MovementClass::Ground, &obstacles);
    let clearance = Clearance {
        map: &map,
        grid,
        footprint: Footprint::default(),
        data: &data,
    };
    let point = Position { x: 2004, y: 122 };
    let bounds = swept_bounds(Footprint::default(), point, point);
    // The coarse terrain cell is occupied, but touching an obstacle's edge is
    // legal. Exact checks must visit one column, not all 16,384 rectangles.
    assert!(clearance.count(&clearance.data.obstacle_cells, clearance.grid.cells(bounds)) > 0);
    assert!(clearance.horizontal_candidates(bounds).len() <= 32);
    assert_eq!(
        clearance.clear(point, point),
        segment_clear(
            &map,
            Footprint::default(),
            MovementClass::Ground,
            point,
            point,
            &obstacles
        )
    );
}

#[test]
fn blocked_terrain_elides_only_fully_redundant_rectangles() {
    let mut map = map(8, 8);
    block(&mut map, 2, 2);
    let obstacles = [
        Obstacle {
            position: Position { x: 20, y: 20 },
            footprint: Footprint {
                width: 8,
                height: 8,
            },
            movement_class: MovementClass::Ground,
        },
        // This one extends into walkable terrain and must remain an obstacle.
        Obstacle {
            position: Position { x: 24, y: 20 },
            footprint: Footprint {
                width: 8,
                height: 4,
            },
            movement_class: MovementClass::Ground,
        },
    ];
    for class in [
        MovementClass::Ground,
        MovementClass::Water,
        MovementClass::Air,
    ] {
        let grid = Grid::new(&map).unwrap();
        let data = ClearanceData::new(&map, grid, class, &obstacles);
        let clearance = Clearance {
            map: &map,
            grid,
            footprint: Footprint::default(),
            data: &data,
        };
        if class == MovementClass::Ground {
            assert_eq!(clearance.data.obstacles.len(), 1);
        }
        for y in 1..63 {
            for x in 1..63 {
                let p = Position { x, y };
                assert_eq!(
                    clearance.clear(p, p),
                    segment_clear(&map, Footprint::default(), class, p, p, &obstacles),
                    "{class:?} {p:?}"
                );
            }
        }
    }
}

#[test]
fn obstacle_prefix_handles_maximum_overlapping_large_rectangles() {
    let map = map(16, 16);
    let obstacles = vec![
        Obstacle {
            position: Position { x: 64, y: 64 },
            footprint: Footprint {
                width: u16::MAX,
                height: u16::MAX
            },
            movement_class: MovementClass::Ground
        };
        MAX_OBSTACLES
    ];
    let grid = Grid::new(&map).unwrap();
    let data = ClearanceData::new(&map, grid, MovementClass::Ground, &obstacles);
    let clearance = Clearance {
        map: &map,
        grid,
        footprint: Footprint::default(),
        data: &data,
    };
    assert_eq!(
        clearance.count(&clearance.data.obstacle_cells, [0, 0, 16, 16]),
        256
    );
    assert!(!clearance.clear(Position { x: 64, y: 64 }, Position { x: 64, y: 64 }));
}

#[test]
fn footprint_must_fit_gap_and_entire_map() {
    let mut map = map(10, 10);
    for y in 0..10 {
        if y != 4 {
            block(&mut map, 5, y);
        }
    }
    let start = Position { x: 20, y: 36 };
    let target = Position { x: 60, y: 36 };
    assert!(
        find_path(
            &map,
            Footprint {
                width: 7,
                height: 7
            },
            MovementClass::Ground,
            start,
            target,
            &[]
        )
        .is_some()
    );
    assert!(
        find_path(
            &map,
            Footprint {
                width: 9,
                height: 9
            },
            MovementClass::Ground,
            start,
            target,
            &[]
        )
        .is_none()
    );
    assert!(
        find_path(
            &map,
            Footprint {
                width: 7,
                height: 7
            },
            MovementClass::Air,
            start,
            Position { x: 1, y: 1 },
            &[]
        )
        .is_none()
    );
}

#[test]
fn diagonal_cannot_squeeze_through_touching_corners() {
    let mut map = map(2, 2);
    block(&mut map, 1, 0);
    block(&mut map, 0, 1);
    let start = Position { x: 4, y: 4 };
    let target = Position { x: 12, y: 12 };
    assert!(!segment_clear(
        &map,
        Footprint::default(),
        MovementClass::Ground,
        start,
        target,
        &[]
    ));
    assert!(
        find_path(
            &map,
            Footprint::default(),
            MovementClass::Ground,
            start,
            target,
            &[]
        )
        .is_none()
    );
    assert_eq!(
        find_path(
            &map,
            Footprint::default(),
            MovementClass::Air,
            start,
            target,
            &[]
        ),
        Some(vec![target])
    );
}

#[test]
fn rectangular_obstacles_and_fast_steps_use_full_sweep() {
    let map = map(16, 10);
    let start = Position { x: 12, y: 36 };
    let target = Position { x: 115, y: 36 };
    let obstacles = [Obstacle {
        position: Position { x: 51, y: 36 },
        footprint: Footprint {
            width: 1,
            height: 31,
        },
        movement_class: MovementClass::Ground,
    }];
    assert!(!segment_clear(
        &map,
        Footprint::default(),
        MovementClass::Ground,
        start,
        target,
        &obstacles
    ));
    let path = find_path(
        &map,
        Footprint::default(),
        MovementClass::Ground,
        start,
        target,
        &obstacles,
    )
    .unwrap();
    assert!(path.len() > 1);
    assert_clear(&map, Footprint::default(), start, &path, &obstacles);
    assert_eq!(
        find_path(
            &map,
            Footprint::default(),
            MovementClass::Air,
            start,
            target,
            &obstacles
        ),
        Some(vec![target])
    );
    let target = obstacles[0].position;
    assert!(
        find_path(
            &map,
            Footprint::default(),
            MovementClass::Ground,
            start,
            target,
            &obstacles
        )
        .is_none()
    );
}

#[test]
fn invalid_blocked_and_reached_endpoints() {
    let mut map = map(8, 8);
    block(&mut map, 3, 3);
    let point = Position { x: 4, y: 4 };
    assert_eq!(
        find_path(
            &map,
            Footprint::default(),
            MovementClass::Ground,
            point,
            point,
            &[]
        ),
        Some(vec![])
    );
    for target in [
        Position { x: -1, y: 0 },
        Position { x: i32::MAX, y: 2 },
        Position { x: 64, y: 2 },
        Position { x: 28, y: 28 },
    ] {
        assert!(
            find_path(
                &map,
                Footprint::default(),
                MovementClass::Ground,
                point,
                target,
                &[]
            )
            .is_none()
        );
    }
    assert!(
        find_path(
            &map,
            Footprint {
                width: 0,
                height: 1
            },
            MovementClass::Ground,
            point,
            point,
            &[]
        )
        .is_none()
    );
    map.terrain.as_mut().unwrap().flags.clear();
    assert!(
        find_path(
            &map,
            Footprint::default(),
            MovementClass::Ground,
            point,
            point,
            &[]
        )
        .is_none()
    );
}
