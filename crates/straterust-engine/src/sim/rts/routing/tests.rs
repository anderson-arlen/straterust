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
fn expensive_backtrack_waits_but_the_preferred_path_resumes_immediately() {
    let mut world = traffic_world(true);
    advance(&mut world, 3);
    let actor = &world.state.entities[0];
    let waiting_at = actor.position;
    let wait = actor.route_wait.as_ref().unwrap();
    assert!(
        wait.ready_at.0 - wait.since.0 > 40,
        "long detour must wait over a second"
    );
    let deadline = wait.ready_at;
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
    assert!(world.tick() < deadline);
    assert!(world.state.entities[0].position.x > waiting_at.x);
    assert_eq!(world.state.entities[0].position.y, 4);
}

#[test]
fn the_longer_route_becomes_eligible_at_its_deadline() {
    let mut world = traffic_world(true);
    advance(&mut world, 3);
    let actor = &world.state.entities[0];
    let waiting_at = actor.position;
    let deadline = actor.route_wait.as_ref().unwrap().ready_at;
    while world.tick() < deadline {
        advance(&mut world, 1);
    }
    assert_eq!(world.state.entities[0].position, waiting_at);
    advance(&mut world, 1);
    assert!(world.state.entities[0].position.x < waiting_at.x);
    assert!(world.state.entities[0].route_wait.is_none());
    advance(&mut world, 300);
    assert_eq!(world.state.entities[0].position, point(76, 4));
    assert!(matches!(world.state.entities[0].order, UnitOrder::Idle));
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
fn mid_wait_snapshots_resume_identically_and_clients_do_not_get_detour_state() {
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
    assert!(own.route_wait.is_none());
    assert_eq!(own.path_geometry, [0; 32]);
    for _ in 0..200 {
        world.step(&[]).unwrap();
        restored.step(&[]).unwrap();
        assert_eq!(world.state_hash(), restored.state_hash());
    }
}
