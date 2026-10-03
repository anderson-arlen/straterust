use straterust_engine::sim::*;
fn point(x: i32, y: i32) -> Position {
    Position { x, y }
}
fn rules() -> Rules {
    Rules {
        id: "creep-tests".into(),
        units: vec![
            UnitType {
                id: UnitTypeId(1),
                speed: 0,
                structure: true,
                max_hp: 10,
                footprint: Footprint {
                    width: 32,
                    height: 32,
                },
                placement: Footprint {
                    width: 64,
                    height: 64,
                },
                creep_radius: Some([96, 64]),
                ..UnitType::default()
            },
            UnitType {
                id: UnitTypeId(2),
                speed: 0,
                structure: true,
                max_hp: 10,
                placement: Footprint {
                    width: 32,
                    height: 32,
                },
                requires_creep: true,
                creep_radius: Some([0, 0]),
                ..UnitType::default()
            },
            UnitType {
                id: UnitTypeId(3),
                speed: 8,
                max_hp: 40,
                vision_range: 96,
                builds: vec![UnitTypeId(2), UnitTypeId(4)],
                worker: Some(WorkerStats {
                    capacity: 8,
                    harvest_amount: 8,
                    harvest_ticks: 2,
                    build_rate: 1,
                    resource_kinds: vec!["ore".into()],
                }),
                ..UnitType::default()
            },
            UnitType {
                id: UnitTypeId(4),
                speed: 0,
                structure: true,
                max_hp: 10,
                placement: Footprint {
                    width: 32,
                    height: 32,
                },
                ..UnitType::default()
            },
            UnitType {
                id: UnitTypeId(5),
                speed: 0,
                max_hp: 100,
                weapon: Some(Weapon {
                    damage: 100,
                    range: 160,
                    cooldown: 10,
                    cooldown_jitter: None,
                    targets_air: false,
                    damage_kind: Default::default(),
                    splash: None,
                    strikes: vec![],
                }),
                ..UnitType::default()
            },
        ],
        ..Rules::default()
    }
}
fn map(fog: bool) -> Map {
    Map {
        id: "creep-tests".into(),
        width: 512,
        height: 512,
        players: 2,
        spawns: vec![
            Spawn {
                unit_type: UnitTypeId(1),
                position: point(128, 128),
                ..Spawn::default()
            },
            Spawn {
                unit_type: UnitTypeId(3),
                position: point(416, 416),
                ..Spawn::default()
            },
        ],
        resources: vec![],
        start_locations: vec![],
        terrain: None,
        fog_of_war: fog,
        initial_explored: Default::default(),
        creation: Default::default(),
        ai: vec![],
        mission: None,
    }
}
#[test]
fn initial_creep_blocks_terran_construction_and_requires_covered_zerg_footprints() {
    let world = World::new(rules(), map(false), 7).unwrap();
    assert!(world.creep_at(point(128, 128)));
    assert!(!world.creep_at(point(240, 240)));
    assert_eq!(
        world.build_rejection(PlayerId(0), EntityId(2), UnitTypeId(4), point(48, 128)),
        Some(Rejection::InvalidPlacement)
    );
    assert_eq!(
        world.build_rejection(PlayerId(0), EntityId(2), UnitTypeId(2), point(48, 128)),
        None
    );
    assert_eq!(
        world.build_rejection(PlayerId(0), EntityId(2), UnitTypeId(2), point(336, 336)),
        Some(Rejection::InvalidPlacement)
    );
    assert_eq!(
        world.build_rejection(PlayerId(0), EntityId(2), UnitTypeId(4), point(336, 336)),
        None
    );
}
#[test]
fn destroyed_providers_recede_without_revealing_changes_in_fog() {
    let mut map = map(true);
    map.spawns[0].owner = PlayerId(1);
    map.spawns[1].position = point(192, 128);
    map.spawns.push(Spawn {
        unit_type: UnitTypeId(5),
        position: point(272, 128),
        ..Spawn::default()
    });
    let mut world = World::new(rules(), map, 7).unwrap();
    let mut replay = world.clone();
    assert!(world.known_creep(PlayerId(0), 4, 4));
    let order = Command {
        tick: Tick(0),
        player: PlayerId(0),
        sequence: 1,
        order: Order::Move {
            entity: EntityId(2),
            target: point(416, 416),
        },
    };
    for instance in [&mut world, &mut replay] {
        instance.step(std::slice::from_ref(&order)).unwrap();
    }
    assert!(
        !world
            .state()
            .entities
            .iter()
            .any(|entity| entity.id == EntityId(1))
    );
    assert!(
        world.creep_at(point(128, 128)),
        "death does not instantly erase the terrain"
    );
    for _ in 0..130 {
        world.step(&[]).unwrap();
        replay.step(&[]).unwrap();
        assert_eq!(world.state_hash(), replay.state_hash());
    }
    assert!(!world.creep_at(point(128, 128)));
    assert!(
        world.known_creep(PlayerId(0), 4, 4),
        "hidden recession must not update explored terrain memory"
    );
}

#[test]
fn stationary_defender_returns_fire_at_known_origin_but_does_not_track_hidden_movement() {
    let weapon = |damage| Weapon {
        damage,
        range: 160,
        cooldown: 5,
        cooldown_jitter: None,
        targets_air: false,
        damage_kind: Default::default(),
        splash: None,
        strikes: vec![],
    };
    let rules = Rules {
        id: "stationary-defense".into(),
        units: vec![
            UnitType {
                id: UnitTypeId(1),
                speed: 0,
                max_hp: 100,
                vision_range: 0,
                weapon: Some(weapon(10)),
                ..UnitType::default()
            },
            UnitType {
                id: UnitTypeId(2),
                speed: 8,
                max_hp: 100,
                vision_range: 160,
                weapon: Some(weapon(1)),
                ..UnitType::default()
            },
        ],
        ..Rules::default()
    };
    let mut map = map(true);
    map.spawns = vec![
        Spawn {
            unit_type: UnitTypeId(1),
            position: point(128, 128),
            ..Spawn::default()
        },
        Spawn {
            owner: PlayerId(1),
            unit_type: UnitTypeId(2),
            position: point(208, 128),
            ..Spawn::default()
        },
    ];
    let mut world = World::new(rules, map, 7).unwrap();
    assert!(!world.entity_visible(PlayerId(0), EntityId(2)));
    world.step(&[]).unwrap();
    assert_eq!(world.state().entities[0].hp, 99);
    world.step(&[]).unwrap();
    assert_eq!(world.state().entities[1].hp, 90);
    world
        .step(&[Command {
            tick: world.tick(),
            player: PlayerId(1),
            sequence: 1,
            order: Order::Move {
                entity: EntityId(2),
                target: point(416, 128),
            },
        }])
        .unwrap();
    for _ in 0..10 {
        world.step(&[]).unwrap();
    }
    assert_eq!(
        world.state().entities[1].hp,
        90,
        "unseen movement cannot update the remembered firing point"
    );
}
