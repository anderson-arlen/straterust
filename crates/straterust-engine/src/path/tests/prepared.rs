use super::*;

#[test]
fn prepared_collision_data_preserves_paths_for_each_class_and_footprint() {
    let mut map = map(20, 16);
    map.terrain
        .as_mut()
        .unwrap()
        .flags
        .fill(WALKABLE | crate::map::WATER);
    for y in 0..12 {
        block(&mut map, 9, y);
    }
    let obstacles = [Obstacle {
        position: Position { x: 44, y: 72 },
        footprint: Footprint {
            width: 13,
            height: 31,
        },
        movement_class: MovementClass::Ground,
    }];
    let data = NavigationData::default();
    let prepared = Pathfinder::new(&map, &obstacles, &data);
    for class in [
        MovementClass::Ground,
        MovementClass::Water,
        MovementClass::Air,
    ] {
        for footprint in [
            Footprint::default(),
            Footprint {
                width: 7,
                height: 5,
            },
        ] {
            for target in [
                Position { x: 132, y: 20 },
                Position { x: 44, y: 72 },
                Position { x: 36, y: 92 },
            ] {
                let start = Position { x: 20, y: 20 };
                assert_eq!(
                    prepared.find_path(footprint, class, start, target),
                    find_path(&map, footprint, class, start, target, &obstacles)
                );
                assert_eq!(
                    prepared.find_path_near(footprint, class, start, target),
                    find_path_near(&map, footprint, class, start, target, &obstacles)
                );
                for near in [false, true] {
                    let path = if near {
                        find_path_near(&map, footprint, class, start, target, &obstacles)
                    } else {
                        find_path(&map, footprint, class, start, target, &obstacles)
                    };
                    let expected = path.map(|path| {
                        let mut from = start;
                        let mut cost = 0;
                        for next in path {
                            cost += distance(from, next);
                            from = next;
                        }
                        (cost, from)
                    });
                    for _ in 0..2 {
                        assert_eq!(
                            prepared.route_estimate(footprint, class, start, target, near),
                            expected
                        );
                    }
                }
                assert_eq!(
                    prepared.find_path_in_range(footprint, class, start, target, 20),
                    find_path_in_range(&map, footprint, class, start, target, 20, &obstacles)
                );
                let targets = [target, Position { x: 52, y: 100 }];
                assert_eq!(
                    prepared.find_path_to_any(footprint, class, start, &targets),
                    find_path_to_any(&map, footprint, class, start, &targets, &obstacles)
                );
            }
        }
    }
}

#[test]
fn bounded_detours_reject_long_routes_without_claiming_arrival() {
    let map = map(32, 32);
    let obstacles = [Obstacle {
        position: Position { x: 128, y: 112 },
        footprint: Footprint {
            width: 8,
            height: 224,
        },
        movement_class: MovementClass::Ground,
    }];
    let start = Position { x: 116, y: 20 };
    let target = Position { x: 140, y: 20 };
    let data = NavigationData::default();
    let prepared = Pathfinder::new(&map, &obstacles, &data);
    let route = prepared
        .find_path(Footprint::default(), MovementClass::Ground, start, target)
        .unwrap();
    assert!(
        route.iter().any(|p| p.y >= 224),
        "permanent terrain can require a long route"
    );
    let bounded = prepared.with_cost_limit(24 * 10 + 64 * 10);
    assert_eq!(
        bounded.find_path(Footprint::default(), MovementClass::Ground, start, target),
        None
    );
    assert_eq!(
        bounded.find_path_near(Footprint::default(), MovementClass::Ground, start, target),
        None
    );
    // A later reachable entrance stays eligible if an earlier one is beyond budget.
    let near = Position { x: 100, y: 20 };
    assert_eq!(
        bounded.find_path_to_any(
            Footprint::default(),
            MovementClass::Ground,
            start,
            &[Position { x: 252, y: 252 }, near]
        ),
        Some((near, vec![near]))
    );
}

#[test]
fn a_distant_unreachable_goal_does_not_inflate_the_local_travel_budget() {
    let mut map = map(128, 16);
    for y in 0..16 {
        block(&mut map, 64, y);
    }
    let start = Position { x: 20, y: 60 };
    let target = Position { x: 900, y: 60 };
    let data = NavigationData::default();
    let budget = 64 * 10;
    let route = Pathfinder::new(&map, &[], &data)
        .with_cost_limit(budget)
        .find_path_near(Footprint::default(), MovementClass::Ground, start, target)
        .expect("a limited search can make closer progress on this side");
    assert!(
        !route.is_empty(),
        "a partial search must never claim arrival"
    );
    let end = *route.last().unwrap();
    assert!(distance(end, target) < distance(start, target));
    let mut from = start;
    let mut cost = 0;
    for next in &route {
        cost += distance(from, *next);
        from = *next;
    }
    assert!(cost <= budget);
    assert_clear(&map, Footprint::default(), start, &route, &[]);
}
