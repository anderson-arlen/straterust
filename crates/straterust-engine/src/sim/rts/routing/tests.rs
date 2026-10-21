use super::*;
use crate::map::{Terrain, WALKABLE};

fn point(x: i32, y: i32) -> Position {
    Position { x, y }
}

fn traffic_world(ring: bool) -> World {
    let rules = Rules {
        id: "route-retention".into(),
        tick_ms: 25,
        units: vec![
            UnitType {
                id: UnitTypeId(1),
                speed: 2,
                footprint: Footprint {
                    width: 6,
                    height: 6,
                },
                max_hp: 100,
                ..UnitType::default()
            },
            UnitType {
                id: UnitTypeId(2),
                speed: 0,
                structure: true,
                footprint: Footprint {
                    width: 6,
                    height: 6,
                },
                max_hp: 100,
                ..UnitType::default()
            },
        ],
        ..Rules::default()
    };
    let terrain = ring.then(|| Terrain {
        cell_size: 8,
        columns: 16,
        rows: 16,
        flags: (0..256)
            .map(|cell| {
                let (x, y) = (cell % 16, cell / 16);
                if x == 0 || x == 15 || y == 0 || y == 15 {
                    WALKABLE
                } else {
                    0
                }
            })
            .collect(),
    });
    let y = if ring { 4 } else { 60 };
    let map = Map {
        multiplayer: None,
        id: "traffic-ring".into(),
        width: 128,
        height: 128,
        players: 1,
        spawns: vec![
            Spawn {
                owner: PlayerId(0),
                unit_type: UnitTypeId(1),
                position: point(60, y),
                ..Spawn::default()
            },
            Spawn {
                owner: PlayerId(0),
                unit_type: UnitTypeId(1),
                position: point(68, y),
                ..Spawn::default()
            },
        ],
        terrain,
        initial_explored: Default::default(),
        creation: Default::default(),
        ai: Vec::new(),
        mission: None,
        fog_of_war: false,
        start_locations: Vec::new(),
        resources: Vec::new(),
    };
    let mut world = World::new(rules, map, 1).unwrap();
    world.state.entities[0].order = UnitOrder::Move {
        target: point(if ring { 76 } else { 116 }, y),
    };
    world
}

fn advance(world: &mut World, count: usize) {
    for _ in 0..count {
        world.step(&[]).unwrap();
    }
}

#[test]
fn proportional_wait_matches_the_requested_distance_examples() {
    assert_eq!(detour_wait_ticks(100, 100, 25), 0);
    assert_eq!(detour_wait_ticks(100, 200, 25), 4);
    assert_eq!(detour_wait_ticks(100, 500, 25), 16);
    assert_eq!(detour_wait_ticks(100, 1100, 25), 40);
}

#[test]
fn excessive_traffic_detours_wait_but_the_preferred_path_resumes_immediately() {
    let mut world = traffic_world(true);
    advance(&mut world, 3);
    let actor = &world.state.entities[0];
    let waiting_at = actor.position;
    let wait = actor.route_wait.as_ref().unwrap();
    assert!(
        wait.alternate.is_empty(),
        "a nearby blocker must not route around the map"
    );
    let since = wait.since;
    advance(&mut world, 12);
    assert_eq!(world.state.entities[0].position, waiting_at);
    assert_eq!(
        world.state.entities[0].route_wait.as_ref().unwrap().since,
        since
    );
    world.state.entities[1].order = UnitOrder::Move {
        target: point(100, 4),
    };
    advance(&mut world, 4);
    assert!(world.state.entities[0].position.x > waiting_at.x);
    assert_eq!(world.state.entities[0].position.y, 4);
}

#[test]
fn a_short_move_never_tours_the_map_while_a_mobile_blocker_remains() {
    let mut world = traffic_world(true);
    advance(&mut world, 3);
    let waiting_at = world.state.entities[0].position;
    advance(&mut world, 200);
    let actor = &world.state.entities[0];
    assert_eq!(actor.position, waiting_at);
    assert!(actor.route_wait.as_ref().unwrap().alternate.is_empty());
    assert!(matches!(actor.order, UnitOrder::Move { .. }));
}

#[test]
fn a_short_detour_around_a_stopped_unit_is_taken_promptly() {
    let mut world = traffic_world(false);
    world.state.entities[1].order = UnitOrder::Hold;
    advance(&mut world, 2);
    let blocked_at = world.state.entities[0].position;
    advance(&mut world, 3);
    assert_ne!(world.state.entities[0].position, blocked_at);
    advance(&mut world, 60);
    assert_eq!(world.state.entities[0].position, point(116, 60));
}

#[test]
fn a_new_building_invalidates_the_remaining_route_before_the_unit_reaches_it() {
    let mut world = traffic_world(true);
    world.state.entities[1].position = point(100, 124);
    advance(&mut world, 1);
    let actor = &mut world.state.entities[0];
    let start = actor.position;
    actor.path = [point(68, 4), point(76, 4)].into();
    world.state.entities[1].unit_type = UnitTypeId(2);
    world.state.entities[1].position = point(72, 4);
    advance(&mut world, 1);
    assert!(
        world.state.entities[0].position.x < start.x,
        "must replan now, even though the next waypoint remains clear"
    );
    assert!(world.state.entities[0].path.iter().any(|point| point.y > 8));
    assert!(world.state.entities[0].route_wait.is_none());
}

#[test]
fn static_blockages_and_new_orders_do_not_inherit_a_traffic_delay() {
    let mut world = traffic_world(true);
    advance(&mut world, 3);
    let start = world.state.entities[0].position;
    world.state.entities[1].unit_type = UnitTypeId(2);
    advance(&mut world, 1);
    assert!(world.state.entities[0].position.x < start.x);
    assert!(world.state.entities[0].route_wait.is_none());
    world.state.entities.remove(1);
    advance(&mut world, 1);
    assert_eq!(
        world.state.entities[0].position, start,
        "removing the building should reopen the preferred route immediately"
    );

    let mut world = traffic_world(true);
    advance(&mut world, 3);
    let start = world.state.entities[0].position;
    let result = world
        .step(&[Command {
            tick: world.tick(),
            player: PlayerId(0),
            sequence: 1,
            order: Order::Move {
                entity: EntityId(1),
                target: point(20, 4),
            },
        }])
        .unwrap();
    assert!(result[0].rejection.is_none());
    assert!(world.state.entities[0].position.x < start.x);
    assert!(world.state.entities[0].route_wait.is_none());
}

#[test]
fn mid_wait_snapshots_resume_identically_and_owned_prediction_keeps_detour_state() {
    let mut world = traffic_world(true);
    advance(&mut world, 15);
    let snapshot = world.save_snapshot().unwrap();
    let encoded = ron::ser::to_string(&snapshot).unwrap();
    let mut restored = world
        .restore_snapshot(ron::from_str(&encoded).unwrap())
        .unwrap();
    let ViewedEntity::Owned(own) = &world.player_view(PlayerId(0)).unwrap().entities[0] else {
        panic!("owned unit")
    };
    assert_eq!(own.route_wait, world.state.entities[0].route_wait);
    assert_eq!(own.path_geometry, world.state.entities[0].path_geometry);
    for _ in 0..200 {
        world.step(&[]).unwrap();
        restored.step(&[]).unwrap();
        assert_eq!(world.state_hash(), restored.state_hash());
    }
}

#[test]
fn prepared_static_geometry_survives_ticks_but_refreshes_for_changed_obstacles() {
    let mut world = traffic_world(true);
    world.state.entities[1].unit_type = UnitTypeId(2);
    world.refresh_navigation_geometry();
    let start = world.state.entities[0].position;
    let target = point(76, 4);
    let footprint = world.unit_at(0).footprint;
    let route = world
        .navigator()
        .find_path(footprint, MovementClass::Ground, start, target)
        .unwrap();
    assert!(route.iter().any(|p| p.y > 8));
    let prepared = Arc::clone(&world.navigation_data);
    world.refresh_navigation_geometry();
    assert!(Arc::ptr_eq(&prepared, &world.navigation_data));
    // A cloned world has independent invalidation; another world may retain the grids.
    let unchanged = world.clone();
    world.state.entities[1].position = point(100, 124);
    world.refresh_navigation_geometry();
    assert!(!Arc::ptr_eq(&prepared, &world.navigation_data));
    assert_eq!(
        world
            .navigator()
            .find_path(footprint, MovementClass::Ground, start, target),
        Some(vec![target])
    );
    assert_eq!(
        unchanged
            .navigator()
            .find_path(footprint, MovementClass::Ground, start, target),
        Some(route)
    );
}

#[test]
fn nearest_stop_proof_reuses_static_reachability_but_checks_current_collision() {
    let mut world = traffic_world(true);
    // A straight corridor split by a building; no route around the map.
    Arc::make_mut(&mut world.map)
        .terrain
        .as_mut()
        .unwrap()
        .flags[16..]
        .fill(0);
    world.state.entities[1].unit_type = UnitTypeId(2);
    world.refresh_navigation_geometry();
    let start = world.state.entities[0].position;
    let target = point(72, 4);
    let footprint = world.unit_at(0).footprint;
    assert_eq!(
        world
            .navigator()
            .find_path_near(footprint, MovementClass::Ground, start, target),
        Some(vec![])
    );
    assert!(
        world
            .navigator()
            .known_nearest_stop(footprint, MovementClass::Ground, start, target)
    );
    let check = |world: &World| {
        let obstacles = world.navigation_obstacles(EntityId(1), Some((target, footprint)));
        assert_eq!(
            world.near_arrival(0, target),
            find_path_near(
                &world.map,
                footprint,
                MovementClass::Ground,
                start,
                target,
                &obstacles
            )
            .is_some_and(|p| p.is_empty())
        );
    };
    check(&world);
    assert!(world.near_arrival(0, target));
    let blocker = world.state.entities[0].clone();
    world.state.entities.push(Entity {
        id: EntityId(3),
        ..blocker
    });
    check(&world);
    assert!(
        !world.near_arrival(0, target),
        "a cached stop cannot stand inside a new mobile unit"
    );
    world.state.entities.pop();
    // Removing a blocker during the tick invalidates the subset proof even
    // before the next regular static-geometry refresh.
    world.state.entities.remove(1);
    check(&world);
    assert!(
        !world.near_arrival(0, target),
        "the destination is now reachable"
    );
    world.refresh_navigation_geometry();
    assert!(
        !world
            .navigator()
            .known_nearest_stop(footprint, MovementClass::Ground, start, target)
    );
}
