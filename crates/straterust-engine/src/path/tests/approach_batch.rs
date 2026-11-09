use super::*;

#[test]
fn disconnected_workers_share_failed_searches_without_hiding_reachable_routes() {
    let map = map(64, 32);
    let data = NavigationData::default();
    let obstacles = [
        Obstacle {
            position: Position { x: 260, y: 128 },
            footprint: Footprint {
                width: 8,
                height: 256,
            },
            movement_class: MovementClass::Ground,
        },
        Obstacle {
            position: Position { x: 300, y: 100 },
            footprint: Footprint {
                width: 16,
                height: 32,
            },
            movement_class: MovementClass::Air,
        },
    ];
    let finder = Pathfinder::new(&map, &obstacles, &data);
    let footprint = Footprint {
        width: 4,
        height: 4,
    };
    SEARCH_COUNT.with(|count| count.set(0));
    for y in 0..24 {
        let start = Position {
            x: 100,
            y: 20 + y * 8,
        };
        let targets = [Position { x: 400, y: 20 }, Position { x: 420, y: 100 }];
        assert!(
            finder
                .find_path_to_any(footprint, MovementClass::Ground, start, &targets)
                .is_none()
        );
    }
    assert_eq!(
        SEARCH_COUNT.with(|count| count.get()),
        1,
        "one exhausted component must cover different workers and entrances"
    );
    let start = Position { x: 100, y: 100 };
    let target = Position { x: 200, y: 100 };
    assert!(
        finder
            .find_path_to_any(footprint, MovementClass::Ground, start, &[target])
            .is_some()
    );
    // Ground proofs do not prohibit aircraft crossing the separating water/wall.
    let other_side = Position { x: 400, y: 100 };
    assert!(
        finder
            .find_path_to_any(footprint, MovementClass::Air, start, &[other_side])
            .is_some()
    );
    // Changing geometry discards its derived data and permits the reopened path.
    let fresh = NavigationData::default();
    assert!(
        Pathfinder::new(&map, &[], &fresh)
            .find_path_to_any(footprint, MovementClass::Ground, start, &[other_side])
            .is_some()
    );
}

#[test]
fn terrain_islands_reject_impossible_destinations_without_a_route_search() {
    let mut map = map(64, 32);
    for y in 0..32 {
        block(&mut map, 32, y);
    }
    let data = NavigationData::default();
    let finder = Pathfinder::new(&map, &[], &data);
    SEARCH_COUNT.with(|count| count.set(0));
    for y in 0..24 {
        let start = Position {
            x: 100,
            y: 20 + y * 8,
        };
        let target = Position {
            x: 400,
            y: 20 + y * 8,
        };
        assert!(
            finder
                .find_path_to_any(
                    Footprint::default(),
                    MovementClass::Ground,
                    start,
                    &[target]
                )
                .is_none()
        );
        assert!(
            finder
                .find_path(Footprint::default(), MovementClass::Ground, start, target)
                .is_none()
        );
    }
    assert_eq!(SEARCH_COUNT.with(|count| count.get()), 0);
    // A resource/building collision update preserves immutable terrain labels.
    let fresh = data.with_unchanged_terrain();
    assert!(
        Pathfinder::new(&map, &[], &fresh)
            .find_path_to_any(
                Footprint::default(),
                MovementClass::Ground,
                Position { x: 100, y: 100 },
                &[Position { x: 400, y: 100 }]
            )
            .is_none()
    );
    assert_eq!(SEARCH_COUNT.with(|count| count.get()), 0);
}

#[test]
fn batched_entrances_preserve_first_reachable_path_after_disconnected_candidates() {
    let mut map = map(16, 12);
    for y in 0..12 {
        block(&mut map, 8, y);
    }
    let start = Position { x: 12, y: 12 };
    let targets = [
        Position { x: 100, y: 20 },
        Position { x: 108, y: 72 },
        Position { x: 44, y: 76 },
    ];
    let footprint = Footprint {
        width: 4,
        height: 4,
    };
    for class in [MovementClass::Ground, MovementClass::Air] {
        let expected = targets.iter().find_map(|&target| {
            find_path(&map, footprint, class, start, target, &[]).map(|path| (target, path))
        });
        assert_eq!(
            find_path_to_any(&map, footprint, class, start, &targets, &[]),
            expected
        );
    }
    assert!(
        find_path_to_any(
            &map,
            footprint,
            MovementClass::Ground,
            start,
            &targets[..2],
            &[]
        )
        .is_none()
    );
}
