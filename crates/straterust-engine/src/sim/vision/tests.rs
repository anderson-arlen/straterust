use super::*;

fn elevation_map() -> Map {
    Map {
        initial_explored: Default::default(),
        creation: Default::default(),
        ai: Vec::new(),
        id: "elevation".into(),
        width: 256,
        height: 256,
        players: 2,
        spawns: vec![
            Spawn {
                position: Position { x: 48, y: 112 },
                ..Spawn::default()
            },
            Spawn {
                owner: PlayerId(1),
                position: Position { x: 176, y: 112 },
                ..Spawn::default()
            },
        ],
        terrain: Some(crate::map::Terrain {
            cell_size: 8,
            columns: 32,
            rows: 32,
            flags: (0..1024)
                .map(|index| {
                    crate::map::WALKABLE
                        | if index % 32 >= 12 {
                            1 << crate::map::HEIGHT_SHIFT
                        } else {
                            0
                        }
                })
                .collect(),
        }),
        fog_of_war: true,
        start_locations: Vec::new(),
        resources: Vec::new(),
        mission: None,
    }
}

#[test]
fn ground_sight_blocks_high_ground_but_air_and_scanners_reveal_it() {
    let rules = Rules {
        id: "elevation".into(),
        units: vec![UnitType {
            vision_range: 224,
            ..UnitType::default()
        }],
        ..Rules::default()
    };
    let mut world = World::new(rules.clone(), elevation_map(), 0).unwrap();
    let high = Position { x: 176, y: 112 };
    assert_eq!(
        world.terrain_visibility(PlayerId(0), Position { x: 112, y: 112 }),
        Visibility::Visible
    );
    assert_eq!(
        world.terrain_visibility(PlayerId(0), Position { x: 144, y: 112 }),
        Visibility::Unexplored
    );
    assert_eq!(
        world.visibility(PlayerId(0), Position { x: 112, y: 112 }),
        Visibility::Unexplored
    );
    assert_eq!(world.visibility(PlayerId(0), high), Visibility::Unexplored);
    assert!(!world.entity_visible(PlayerId(0), EntityId(2)));
    // Looking downhill is permitted, and reaching the high plateau reveals it.
    assert_eq!(
        world.visibility(PlayerId(1), Position { x: 48, y: 112 }),
        Visibility::Visible
    );
    world.state.entities[0].position = Position { x: 112, y: 112 };
    world.update_vision();
    assert!(world.entity_visible(PlayerId(0), EntityId(2)));
    world.state.entities[0].position = Position { x: 48, y: 112 };
    world.update_vision();
    assert_eq!(world.visibility(PlayerId(0), high), Visibility::Explored);
    assert!(!world.entity_visible(PlayerId(0), EntityId(2)));
    let cached = world.canonical_state();
    world.vision_cells.clear();
    world.update_vision();
    assert_eq!(cached, world.canonical_state());

    let mut flying = World::new(rules.clone(), elevation_map(), 0).unwrap();
    flying.state.entities[0].airborne = true;
    flying.update_vision();
    assert!(flying.entity_visible(PlayerId(0), EntityId(2)));
    let mut scanner = World::new(rules, elevation_map(), 0).unwrap();
    scanner.state.scans.push(Scan {
        owner: PlayerId(0),
        position: high,
        radius: 64,
        remaining: 2,
    });
    scanner.update_vision();
    assert!(scanner.entity_visible(PlayerId(0), EntityId(2)));
}

#[test]
fn a_low_fog_cell_center_does_not_disclose_its_higher_corner() {
    let mut map = elevation_map();
    map.terrain
        .as_mut()
        .unwrap()
        .flags
        .fill(crate::map::WALKABLE);
    let high = Position { x: 128, y: 96 };
    // Only one corner minitile is high; center (144,112) stays low.
    map.terrain.as_mut().unwrap().flags[12 * 32 + 16] |= 1 << crate::map::HEIGHT_SHIFT;
    map.spawns[1].position = high;
    let rules = Rules {
        id: "mixed-elevation".into(),
        units: vec![UnitType {
            vision_range: 224,
            ..UnitType::default()
        }],
        ..Rules::default()
    };
    let mut world = World::new(rules, map, 0).unwrap();
    assert_eq!(world.map.height_at(high), Some(1));
    assert_eq!(world.map.height_at(Position { x: 144, y: 112 }), Some(0));
    assert_eq!(world.visibility(PlayerId(0), high), Visibility::Unexplored);
    assert_eq!(
        world.visibility(PlayerId(0), Position { x: 144, y: 112 }),
        Visibility::Visible
    );
    assert!(!world.entity_visible(PlayerId(0), EntityId(2)));
    assert_eq!(
        world.terrain_visibility(PlayerId(0), high),
        Visibility::Visible
    );
    // Even the observer's own fog cell may have a higher corner. It must
    // not cast a black pocket over low ground or hide low occupants.
    world.state.entities[0].position = Position { x: 144, y: 112 };
    world.update_vision();
    assert_eq!(
        world.visibility(PlayerId(0), Position { x: 144, y: 112 }),
        Visibility::Visible
    );
    assert_eq!(world.visibility(PlayerId(0), high), Visibility::Unexplored);
    let terrain_before = world.state_hash();
    let mut altered = world.clone();
    altered.state.terrain_fog[0][0] ^= 1;
    assert_ne!(
        terrain_before,
        altered.state_hash(),
        "terrain discovery must be canonical"
    );
    // A scanner legitimately explores every part of the cell. When it
    // expires, the lower observer cannot maintain its current visibility.
    world.state.scans.push(Scan {
        owner: PlayerId(0),
        position: high,
        radius: 64,
        remaining: 2,
    });
    world.update_vision();
    assert!(world.entity_visible(PlayerId(0), EntityId(2)));
    world.state.scans.clear();
    world.update_vision();
    assert_eq!(world.visibility(PlayerId(0), high), Visibility::Explored);
    assert!(!world.entity_visible(PlayerId(0), EntityId(2)));
}

#[test]
fn first_blocking_tile_is_visible_in_every_direction_but_stops_its_successor() {
    let mut map = elevation_map();
    map.terrain
        .as_mut()
        .unwrap()
        .flags
        .fill(crate::map::WALKABLE);
    let origin = Position { x: 112, y: 112 };
    for dy in -1_i32..=1 {
        for dx in -1_i32..=1 {
            if dx == 0 && dy == 0 {
                continue;
            }
            let boundary = ((3 + dy) * 8 + 3 + dx) as usize;
            let behind = ((3 + dy * 2) * 8 + 3 + dx * 2) as usize;
            for opaque in [false, true] {
                let mut tiles = vec![SightTile::default(); 64];
                tiles[boundary] = SightTile {
                    height: u8::from(!opaque),
                    opaque,
                };
                let visible = propagated_cells(&map, origin, 160, Some(&tiles));
                assert!(
                    visible.contains(&boundary),
                    "boundary {dx},{dy} opaque={opaque}"
                );
                assert!(
                    !visible.contains(&behind),
                    "shadow {dx},{dy} opaque={opaque}"
                );
            }
        }
    }
}

#[test]
fn tile_propagation_uses_either_inward_predecessor_without_turning_around_corners() {
    let mut map = elevation_map();
    map.terrain
        .as_mut()
        .unwrap()
        .flags
        .fill(crate::map::WALKABLE);
    let origin = Position { x: 112, y: 112 };
    let mut tiles = vec![SightTile::default(); 64];
    tiles[4 * 8 + 4].opaque = true;
    let visible = propagated_cells(&map, origin, 160, Some(&tiles));
    assert!(
        visible.contains(&(5 * 8 + 4)),
        "second inward predecessor remains open"
    );
    assert!(
        !visible.contains(&(5 * 8 + 5)),
        "diagonal has one inward predecessor"
    );
    tiles[4 * 8 + 3].opaque = true;
    assert!(
        !propagated_cells(&map, origin, 160, Some(&tiles)).contains(&(5 * 8 + 4)),
        "two blocked predecessors cannot propagate around the corner"
    );
}

#[test]
fn majority_height_and_any_opaque_cell_control_tile_propagation() {
    let mut map = elevation_map();
    map.terrain
        .as_mut()
        .unwrap()
        .flags
        .fill(crate::map::WALKABLE);
    let origin = Position { x: 48, y: 112 };
    let native: Vec<_> = (0..4)
        .flat_map(|y| (0..4).map(move |x| (12 + y) * 32 + 12 + x))
        .collect();
    for &cell in &native[..11] {
        map.terrain.as_mut().unwrap().flags[cell] |= 1 << crate::map::HEIGHT_SHIFT;
    }
    let tiles = fog_terrain(&map);
    assert_eq!(tiles[3 * 8 + 3].height, 0);
    assert!(propagated_cells(&map, origin, 160, Some(&tiles)).contains(&(3 * 8 + 4)));
    map.terrain.as_mut().unwrap().flags[native[11]] |= 1 << crate::map::HEIGHT_SHIFT;
    let tiles = fog_terrain(&map);
    assert_eq!(tiles[3 * 8 + 3].height, 1);
    assert!(!propagated_cells(&map, origin, 160, Some(&tiles)).contains(&(3 * 8 + 4)));
    map.terrain
        .as_mut()
        .unwrap()
        .flags
        .fill(crate::map::WALKABLE);
    map.terrain.as_mut().unwrap().flags[native[0]] |= crate::map::BLOCKS_SIGHT;
    let tiles = fog_terrain(&map);
    assert!(tiles[3 * 8 + 3].opaque);
    assert!(!propagated_cells(&map, origin, 160, Some(&tiles)).contains(&(3 * 8 + 4)));
}

#[test]
fn flat_ground_has_no_internal_shadow_and_radius_does_not_expand_via_neighbors() {
    let mut map = elevation_map();
    map.terrain
        .as_mut()
        .unwrap()
        .flags
        .fill(crate::map::WALKABLE);
    let tiles = fog_terrain(&map);
    let origin = Position { x: 112, y: 112 };
    let visible = propagated_cells(&map, origin, 64, Some(&tiles));
    assert_eq!(visible.len(), 37);
    for y in 0..8 {
        for x in 0..8 {
            let dx = x - 3_i32;
            let dy = y - 3_i32;
            let inside = 4 * (dx * dx + dy * dy) <= 49;
            assert_eq!(visible.contains(&((y * 8 + x) as usize)), inside, "{x},{y}");
        }
    }
    assert_eq!(propagated_cells(&map, origin, 0, Some(&tiles)).len(), 9);
    assert_eq!(
        propagated_cells(&map, origin, 64, None),
        visible,
        "air and ground cover the same unobstructed stencil"
    );
}

#[test]
fn flying_targets_use_terrain_visibility_instead_of_ground_height() {
    let rules = Rules {
        id: "flying-target".into(),
        units: vec![UnitType {
            vision_range: 224,
            ..UnitType::default()
        }],
        ..Rules::default()
    };
    let mut map = elevation_map();
    map.spawns[1].position = Position { x: 112, y: 112 };
    let mut world = World::new(rules, map, 0).unwrap();
    assert!(!world.entity_visible(PlayerId(0), EntityId(2)));
    world.state.entities[1].airborne = true;
    assert!(world.entity_visible(PlayerId(0), EntityId(2)));
}

#[test]
fn sight_follows_units_and_exploration_survives_their_departure() {
    let mut world = World::new(
        Rules {
            id: "sight".into(),
            units: vec![UnitType {
                vision_range: 64,
                ..UnitType::default()
            }],
            ..Rules::default()
        },
        Map {
            id: "sight".into(),
            width: 512,
            height: 512,
            players: 2,
            spawns: vec![
                Spawn {
                    position: Position { x: 64, y: 64 },
                    ..Spawn::default()
                },
                Spawn {
                    owner: PlayerId(1),
                    position: Position { x: 400, y: 400 },
                    ..Spawn::default()
                },
            ],
            start_locations: Vec::new(),
            resources: Vec::new(),
            initial_explored: Default::default(),
            creation: Default::default(),
            ai: Vec::new(),
            mission: None,
            terrain: None,
            fog_of_war: true,
        },
        0,
    )
    .unwrap();
    assert_eq!(
        world.visibility(PlayerId(0), Position { x: 64, y: 64 }),
        Visibility::Visible
    );
    assert_eq!(
        world.visibility(PlayerId(0), Position { x: 400, y: 400 }),
        Visibility::Unexplored
    );
    assert!(!world.entity_visible(PlayerId(0), EntityId(2)));
    world.state.entities[0].position = Position { x: 360, y: 400 };
    world.update_vision();
    assert_eq!(
        world.visibility(PlayerId(0), Position { x: 64, y: 64 }),
        Visibility::Explored
    );
    assert!(world.entity_visible(PlayerId(0), EntityId(2)));
    world.state.entities[1].cloaked = true;
    assert!(!world.entity_visible(PlayerId(0), EntityId(2)));
    let restored: State = ron::from_str(&ron::to_string(&world.state).unwrap()).unwrap();
    assert_eq!(restored, world.state);
    let cached = world.canonical_state();
    world.vision_cells.clear();
    world.update_vision();
    assert_eq!(
        cached,
        world.canonical_state(),
        "derived sight cache must not affect state"
    );
}

#[test]
fn oversized_sight_is_evaluated_without_retaining_an_unbounded_cache() {
    let mut world = World::new(
        Rules {
            id: "large-sight".into(),
            units: vec![UnitType {
                vision_range: 32768,
                movement_class: MovementClass::Air,
                ..UnitType::default()
            }],
            ..Rules::default()
        },
        Map {
            id: "large-sight".into(),
            width: 32768,
            height: 32768,
            players: 1,
            fog_of_war: true,
            spawns: vec![Spawn {
                position: Position { x: 16384, y: 16384 },
                ..Spawn::default()
            }],
            start_locations: Vec::new(),
            resources: Vec::new(),
            initial_explored: Default::default(),
            creation: Default::default(),
            ai: Vec::new(),
            mission: None,
            terrain: None,
        },
        0,
    )
    .unwrap();
    assert!(world.vision_cells.is_empty());
    assert_eq!(
        world.visibility(PlayerId(0), Position { x: 0, y: 0 }),
        Visibility::Visible
    );
    assert_eq!(
        world.visibility(PlayerId(0), Position { x: 32767, y: 32767 }),
        Visibility::Visible
    );
    let before = world.state_hash();
    world.update_vision();
    assert_eq!(world.state_hash(), before);
    assert!(world.vision_cells.is_empty());
}

#[test]
fn scanner_spends_energy_reveals_concealed_targets_and_expires() {
    let mut world = World::new(
        Rules {
            id: "scan".into(),
            units: vec![UnitType {
                scanner: Some(Scanner {
                    energy_max: 200,
                    energy_initial: 75,
                    energy_regeneration: 8,
                    cost: 75,
                    radius: 64,
                    duration: 4,
                }),
                ..UnitType::default()
            }],
            ..Rules::default()
        },
        Map {
            id: "scan".into(),
            width: 512,
            height: 512,
            players: 2,
            spawns: vec![
                Spawn {
                    position: Position { x: 32, y: 32 },
                    ..Spawn::default()
                },
                Spawn {
                    owner: PlayerId(1),
                    position: Position { x: 400, y: 400 },
                    cloaked: true,
                    ..Spawn::default()
                },
            ],
            start_locations: Vec::new(),
            resources: Vec::new(),
            initial_explored: Default::default(),
            creation: Default::default(),
            ai: Vec::new(),
            mission: None,
            terrain: None,
            fog_of_war: true,
        },
        0,
    )
    .unwrap();
    assert!(!world.entity_visible(PlayerId(0), EntityId(2)));
    let outcome = world
        .step(&[Command {
            tick: Tick(0),
            player: PlayerId(0),
            sequence: 1,
            order: Order::Scan {
                entity: EntityId(1),
                target: Position { x: 400, y: 400 },
            },
        }])
        .unwrap();
    assert!(outcome[0].rejection.is_none());
    assert_eq!(world.state.entities[0].energy, 8);
    assert!(world.entity_visible(PlayerId(0), EntityId(2)));
    assert_eq!(
        world.scan_rejection(EntityId(1), Position { x: 400, y: 400 }),
        Some(Rejection::InsufficientResources)
    );
    for _ in 0..3 {
        world.step(&[]).unwrap();
    }
    assert!(!world.entity_visible(PlayerId(0), EntityId(2)));
    assert_eq!(
        world.visibility(PlayerId(0), Position { x: 400, y: 400 }),
        Visibility::Explored
    );
}
