use super::*;

#[test]
fn off_grid_corner_contacts_connect_through_clear_cardinal_bends() {
    let mut map = map(8, 8);
    map.width = 256;
    map.height = 256;
    map.terrain.as_mut().unwrap().cell_size = 32;
    let footprint = Footprint {
        width: 24,
        height: 24,
    };
    let start = Position { x: 124, y: 124 };
    let target = Position { x: 48, y: 48 };
    let mut obstacles: Vec<_> = [(100, 100), (148, 100), (100, 148), (148, 148)]
        .into_iter()
        .map(|(x, y)| Obstacle {
            position: Position { x, y },
            footprint: Footprint {
                width: 16,
                height: 16,
            },
            movement_class: MovementClass::Ground,
        })
        .collect();
    let grid = Grid::new(&map).unwrap();
    assert!(grid.near(start).all(|node| !segment_clear(
        &map,
        footprint,
        MovementClass::Ground,
        start,
        grid.position(node),
        &obstacles
    )));
    let path = find_path(
        &map,
        footprint,
        MovementClass::Ground,
        start,
        target,
        &obstacles,
    )
    .expect("a free cardinal lane must connect the off-grid worker to the search grid");
    assert!(path[0].x == start.x || path[0].y == start.y);
    assert_eq!(path.last(), Some(&target));
    assert_clear(&map, footprint, start, &path, &obstacles);
    obstacles.reverse();
    assert_eq!(
        find_path(
            &map,
            footprint,
            MovementClass::Ground,
            start,
            target,
            &obstacles
        ),
        Some(path)
    );
}

#[test]
fn wall_gap_exact_endpoints_and_stable_ties() {
    let mut map = map(12, 12);
    for y in 0..12 {
        if y != 7 {
            block(&mut map, 6, y);
        }
    }
    let start = Position { x: 11, y: 13 };
    let target = Position { x: 83, y: 15 };
    let path = find_path(
        &map,
        Footprint::default(),
        MovementClass::Ground,
        start,
        target,
        &[],
    )
    .unwrap();
    assert_eq!(path.last(), Some(&target));
    assert!(path.iter().any(|point| point.y >= 56));
    assert_clear(&map, Footprint::default(), start, &path, &[]);
    for _ in 0..10 {
        assert_eq!(
            Some(&path),
            find_path(
                &map,
                Footprint::default(),
                MovementClass::Ground,
                start,
                target,
                &[]
            )
            .as_ref()
        );
    }
    block(&mut map, 6, 7);
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
}

#[test]
fn obstacle_order_and_off_map_rectangles_do_not_change_route() {
    let map = map(16, 16);
    let start = Position { x: 12, y: 60 };
    let target = Position { x: 116, y: 60 };
    let mut obstacles = vec![
        Obstacle {
            position: Position { x: 60, y: 60 },
            footprint: Footprint {
                width: 20,
                height: 36,
            },
            movement_class: MovementClass::Ground,
        },
        Obstacle {
            position: Position { x: 70, y: 70 },
            footprint: Footprint {
                width: 12,
                height: 20,
            },
            movement_class: MovementClass::Ground,
        },
        Obstacle {
            position: Position {
                x: i32::MIN,
                y: i32::MIN,
            },
            footprint: Footprint {
                width: 10,
                height: 10,
            },
            movement_class: MovementClass::Ground,
        },
    ];
    let path = find_path(
        &map,
        Footprint {
            width: 7,
            height: 7,
        },
        MovementClass::Ground,
        start,
        target,
        &obstacles,
    )
    .unwrap();
    assert_clear(
        &map,
        Footprint {
            width: 7,
            height: 7,
        },
        start,
        &path,
        &obstacles,
    );
    obstacles.reverse();
    assert_eq!(
        Some(path),
        find_path(
            &map,
            Footprint {
                width: 7,
                height: 7
            },
            MovementClass::Ground,
            start,
            target,
            &obstacles
        )
    );
}

#[test]
fn single_occupant_on_maximum_grid_keeps_direct_approach() {
    let map = map(1024, 1024);
    let footprint = Footprint {
        width: 7,
        height: 7,
    };
    let start = Position { x: 12, y: 4100 };
    let target = Position { x: 8100, y: 4100 };
    let obstacles = [Obstacle {
        position: target,
        footprint,
        movement_class: MovementClass::Ground,
    }];
    // One direct waypoint: no full-grid A* or per-candidate route search.
    assert_eq!(
        find_path_near(
            &map,
            footprint,
            MovementClass::Ground,
            start,
            target,
            &obstacles
        ),
        Some(vec![Position { x: 8092, y: 4100 }])
    );
}

#[test]
#[ignore = "repeatable release-mode pathfinding measurement"]
fn maximum_grid_measurement() {
    let mut map = map(1024, 1024);
    for y in 0..1024 {
        if y != 900 {
            block(&mut map, 512, y);
        }
    }
    let start = Position { x: 80, y: 80 };
    let target = Position { x: 8100, y: 80 };
    let began = std::time::Instant::now();
    let path = find_path(
        &map,
        Footprint::default(),
        MovementClass::Ground,
        start,
        target,
        &[],
    )
    .unwrap();
    println!(
        "1024x1024 wall/gap: {:?}; {} waypoints",
        began.elapsed(),
        path.len()
    );
    assert_clear(&map, Footprint::default(), start, &path, &[]);
}
