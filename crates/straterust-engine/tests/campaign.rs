//! Original synthetic combat/objective fixtures; no campaign assets are needed.
use straterust_engine::sim::*;

fn definitions() -> (Rules, Map) {
    let rules = Rules {
        id: "original-campaign-mechanics".into(),
        units: vec![
            UnitType {
                id: UnitTypeId(1),
                speed: 4,
                max_hp: 100,
                weapon: Some(Weapon {
                    cooldown_jitter: None,
                    targets_air: false,
                    damage: 10,
                    range: 8,
                    cooldown: 3,
                    damage_kind: DamageKind::Explosive,
                    splash: None,
                    strikes: Vec::new(),
                }),
                ..UnitType::default()
            },
            UnitType {
                id: UnitTypeId(2),
                speed: 0,
                max_hp: 100,
                armor: 2,
                ..UnitType::default()
            },
        ],
        ..Rules::default()
    };
    let map = Map {
        id: "original-objective-field".into(),
        width: 256,
        height: 128,
        players: 2,
        spawns: vec![spawn(0, 1, 32), spawn(1, 2, 40)],
        start_locations: vec![],
        resources: vec![],
        initial_explored: Default::default(),
        creation: Default::default(),
        ai: Vec::new(),
        mission: None,
        fog_of_war: false,
        terrain: None,
    };
    (rules, map)
}

fn spawn(owner: u16, unit: u16, x: i32) -> Spawn {
    Spawn {
        owner: PlayerId(owner),
        unit_type: UnitTypeId(unit),
        position: Position { x, y: 32 },
        ..Spawn::default()
    }
}

fn guard_mission(map: &Map) -> Mission {
    Mission {
        schema_version: 1,
        player: PlayerId(0),
        rescuable_players: vec![],
        rescuers: vec![PlayerId(0)],
        alliances: vec![],
        poll_ticks: 1,
        wait_step_ms: 1,
        locations: vec![MissionLocation {
            excluded_elevations: 0,
            left: 0,
            top: 0,
            right: map.width,
            bottom: map.height,
        }],
        triggers: vec![MissionTrigger {
            conditions: vec![MissionCondition::Switch {
                index: 0,
                set: true,
            }],
            actions: vec![MissionAction::Victory],
        }],
    }
}

#[test]
fn explosive_damage_applies_armor_before_size_and_retains_half_hp_minimum() {
    for (size, armor, expected) in [
        (UnitSize::Small, 2, 4),
        (UnitSize::Medium, 2, 6),
        (UnitSize::Large, 2, 8),
        (UnitSize::Small, 99, 0),
    ] {
        let (mut rules, map) = definitions();
        rules.units[1].size = size;
        rules.units[1].armor = armor;
        let mut world = World::new(rules, map, 0).unwrap();
        world.step(&[]).unwrap();
        assert_eq!(world.state().entities[1].hp, 100 - expected);
        assert_eq!(
            world.state().entities[1].damage_fraction,
            if armor == 99 { 128 } else { 0 }
        );
    }
    let (mut rules, map) = definitions();
    rules.units[1].size = UnitSize::Small;
    rules.units[0].weapon.as_mut().unwrap().damage_kind = DamageKind::Normal;
    let mut world = World::new(rules, map, 0).unwrap();
    world.step(&[]).unwrap();
    assert_eq!(world.state().entities[1].hp, 92);
}

#[test]
fn firebat_burst_hits_on_distinct_ticks_and_splash_applies_before_armor() {
    let (mut rules, mut map) = definitions();
    rules.units[0].weapon = Some(Weapon {
        cooldown_jitter: None,
        targets_air: false,
        damage: 8,
        range: 64,
        cooldown: 22,
        damage_kind: DamageKind::Concussive,
        splash: Some([1, 8, 16]),
        strikes: vec![
            WeaponStrike {
                delay: 0,
                forward: 24,
            },
            WeaponStrike {
                delay: 3,
                forward: 52,
            },
            WeaponStrike {
                delay: 5,
                forward: 80,
            },
        ],
    });
    rules.units[1].size = UnitSize::Small;
    map.spawns = vec![
        spawn(0, 1, 32),
        spawn(1, 2, 56),
        spawn(1, 2, 88),
        spawn(1, 2, 112),
        spawn(0, 2, 60),
    ];
    let mut world = World::new(rules, map, 0).unwrap();
    world.step(&[]).unwrap();
    assert_eq!(world.state().entities[1].hp, 94);
    assert_eq!(world.state().entities[2].hp, 100);
    assert_eq!(
        world.state().entities[4].hp,
        100,
        "friendly units are outside enemy-only splash"
    );
    world.step(&[]).unwrap();
    world.step(&[]).unwrap();
    assert_eq!(world.state().entities[2].hp, 100);
    world.step(&[]).unwrap();
    assert_eq!(
        world.state().entities[2].hp,
        98,
        "half splash8/2 minus armor2 =2"
    );
    world.step(&[]).unwrap();
    world.step(&[]).unwrap();
    assert_eq!(world.state().entities[3].hp, 94);
}

#[test]
fn gas_requires_completed_owned_extractor_and_depleted_geyser_keeps_producing() {
    let mut worker = UnitType {
        id: UnitTypeId(1),
        speed: 8,
        builds: vec![UnitTypeId(3)],
        worker: Some(WorkerStats {
            capacity: 8,
            harvest_amount: 8,
            harvest_ticks: 1,
            build_rate: 1,
            resource_kinds: vec!["gas".into()],
            idle_resource_radius: 256,
        }),
        ..UnitType::default()
    };
    worker.weapon = None;
    let depot = UnitType {
        id: UnitTypeId(2),
        speed: 0,
        structure: true,
        dropoff: vec!["gas".into()],
        ..UnitType::default()
    };
    let refinery = UnitType {
        id: UnitTypeId(3),
        speed: 0,
        structure: true,
        build_ticks: 3,
        extracts: Some(Extraction {
            resource: "gas".into(),
            harvest_ticks: 2,
            depleted_amount: 2,
        }),
        footprint: Footprint {
            width: 16,
            height: 16,
        },
        placement: Footprint {
            width: 16,
            height: 16,
        },
        ..UnitType::default()
    };
    let rules = Rules {
        id: "gas-cycle".into(),
        units: vec![worker, depot, refinery],
        ..Rules::default()
    };
    let map = Map {
        id: "gas-cycle".into(),
        width: 256,
        height: 128,
        players: 1,
        spawns: vec![spawn(0, 1, 32), spawn(0, 2, 16)],
        resources: vec![ResourceSpawn {
            position: Position { x: 96, y: 32 },
            footprint: Footprint {
                width: 16,
                height: 16,
            },
            kind: "gas".into(),
            amount: 5,
            requires_extractor: true,
        }],
        start_locations: vec![],
        terrain: None,
        initial_explored: Default::default(),
        creation: Default::default(),
        ai: Vec::new(),
        mission: None,
        fog_of_war: false,
    };
    let mut world = World::new(rules, map, 0).unwrap();
    assert_eq!(
        world.gather_rejection(EntityId(1), ResourceId(1)),
        Some(Rejection::InvalidTarget)
    );
    assert!(
        world
            .step(&[Command {
                tick: world.tick(),
                player: PlayerId(0),
                sequence: 1,
                order: Order::Build {
                    entity: EntityId(1),
                    unit_type: UnitTypeId(3),
                    position: Position { x: 96, y: 32 }
                }
            }])
            .unwrap()[0]
            .rejection
            .is_none()
    );
    for _ in 0..100 {
        world.step(&[]).unwrap();
    }
    assert!(world.state().entities[2].construction.is_none());
    assert!(
        world
            .step(&[Command {
                tick: world.tick(),
                player: PlayerId(0),
                sequence: 2,
                order: Order::Gather {
                    entity: EntityId(1),
                    resource: ResourceId(1)
                }
            }])
            .unwrap()[0]
            .rejection
            .is_none()
    );
    for _ in 0..300 {
        world.step(&[]).unwrap();
    }
    assert_eq!(world.state().resources[0].amount, 0);
    assert!(world.resource_balance(PlayerId(0), "gas") >= 4);
    assert_eq!(world.resource_balance(PlayerId(0), "gas") % 2, 0);
}

#[test]
fn addon_constructs_from_parent_and_initializes_scanner_energy() {
    let parent = UnitType {
        id: UnitTypeId(1),
        speed: 0,
        structure: true,
        builds: vec![UnitTypeId(2)],
        footprint: Footprint {
            width: 120,
            height: 80,
        },
        placement: Footprint {
            width: 128,
            height: 96,
        },
        ..UnitType::default()
    };
    let addon = UnitType {
        id: UnitTypeId(2),
        speed: 0,
        structure: true,
        build_ticks: 3,
        addon_parent: Some(UnitTypeId(1)),
        footprint: Footprint {
            width: 69,
            height: 42,
        },
        placement: Footprint {
            width: 64,
            height: 64,
        },
        scanner: Some(Scanner {
            energy_max: 200,
            energy_initial: 50,
            energy_regeneration: 8,
            cost: 75,
            radius: 320,
            duration: 166,
        }),
        ..UnitType::default()
    };
    let map = Map {
        id: "addon".into(),
        width: 512,
        height: 256,
        players: 1,
        spawns: vec![Spawn {
            position: Position { x: 96, y: 96 },
            ..spawn(0, 1, 96)
        }],
        resources: vec![],
        start_locations: vec![],
        terrain: None,
        initial_explored: Default::default(),
        creation: Default::default(),
        ai: Vec::new(),
        mission: None,
        fog_of_war: false,
    };
    let mut map = map;
    map.spawns.push(Spawn {
        unit_type: UnitTypeId(3),
        position: Position { x: 192, y: 112 },
        ..Spawn::default()
    });
    let rules = Rules {
        id: "addon".into(),
        units: vec![
            parent,
            addon,
            UnitType {
                id: UnitTypeId(3),
                speed: 4,
                max_hp: 10,
                ..UnitType::default()
            },
        ],
        ..Rules::default()
    };
    let mut blocked_rules = rules.clone();
    blocked_rules.units[2].speed = 0;
    let blocked = World::new(blocked_rules, map.clone(), 0).unwrap();
    assert_eq!(
        blocked.build_rejection(
            PlayerId(0),
            EntityId(1),
            UnitTypeId(2),
            Position { x: 192, y: 112 }
        ),
        Some(Rejection::InvalidPlacement)
    );
    let mut world = World::new(rules, map, 0).unwrap();
    let target = world.addon_position(EntityId(1)).unwrap();
    assert_eq!(target, Position { x: 192, y: 112 });
    assert!(
        world
            .step(&[Command {
                tick: Tick(0),
                player: PlayerId(0),
                sequence: 1,
                order: Order::Build {
                    entity: EntityId(1),
                    unit_type: UnitTypeId(2),
                    position: target
                }
            }])
            .unwrap()[0]
            .rejection
            .is_none()
    );
    for _ in 0..2 {
        world.step(&[]).unwrap();
    }
    let mobile = &world.state().entities[1];
    assert_ne!(
        mobile.position, target,
        "the addon must not trap overlapping mobile traffic"
    );
    assert_eq!(mobile.order, UnitOrder::Idle);
    assert!(world.can_place(
        mobile.position,
        world.unit_type(mobile.unit_type).unwrap().footprint,
        MovementClass::Ground,
        Some(mobile.id)
    ));
    let addon = &world.state().entities[2];
    assert!(addon.construction.is_none());
    assert_eq!(addon.parent, Some(EntityId(1)));
    assert_eq!(addon.energy, 50 * 256);
    assert_eq!(
        world.build_rejection(PlayerId(0), EntityId(1), UnitTypeId(2), target),
        Some(Rejection::InvalidPlacement)
    );
}

#[test]
fn idle_defender_pursues_a_visible_attacker_outside_acquisition_range() {
    let (mut rules, mut map) = definitions();
    rules.units[0].weapon.as_mut().unwrap().range = 96;
    rules.units[0].speed = 0;
    rules.units[1].weapon = Some(Weapon {
        range: 8,
        damage: 4,
        ..rules.units[0].weapon.clone().unwrap()
    });
    rules.units[1].speed = 4;
    rules.units[1].acquisition_range = Some(16);
    map.spawns[1].position.x = 112;
    let mut world = World::new(rules, map, 0).unwrap();
    world.step(&[]).unwrap();
    assert!(world.state().entities[1].hp < 100);
    for _ in 0..24 {
        world.step(&[]).unwrap();
    }
    assert!(
        world.state().entities[1].position.x < 112,
        "defender never reacted to incoming fire"
    );
    assert!(
        world.state().entities[0].hp < 100,
        "defender never returned fire"
    );
}

#[test]
fn retaliation_waits_for_delayed_damage_and_respects_explicit_orders_even_beyond_sight() {
    for case in 0..5 {
        let (mut rules, mut map) = definitions();
        rules.units[0].weapon.as_mut().unwrap().range = 96;
        rules.units[0].weapon.as_mut().unwrap().cooldown = 100;
        rules.units[0].weapon.as_mut().unwrap().strikes = vec![WeaponStrike {
            delay: 3,
            forward: 0,
        }];
        rules.units[0].speed = 0;
        rules.units[1].weapon = Some(Weapon {
            range: 8,
            ..rules.units[0].weapon.clone().unwrap()
        });
        rules.units[1].speed = 4;
        rules.units[1].acquisition_range = Some(16);
        map.spawns[1].position.x = 112;
        if case == 4 {
            map.fog_of_war = true;
            rules.units[1].vision_range = 16;
            rules.units[0].vision_range = 128;
        }
        let mut world = World::new(rules, map, 0).unwrap();
        let commands = match case {
            1 => vec![Order::Hold {
                entity: EntityId(2),
            }],
            2 => vec![Order::Move {
                entity: EntityId(2),
                target: Position { x: 200, y: 32 },
            }],
            3 => vec![Order::AttackMove {
                entity: EntityId(2),
                target: Position { x: 112, y: 96 },
            }],
            _ => vec![],
        }
        .into_iter()
        .enumerate()
        .map(|(index, order)| Command {
            tick: Tick(0),
            player: PlayerId(1),
            sequence: index as u64 + 1,
            order,
        })
        .collect::<Vec<_>>();
        world.step(&commands).unwrap();
        for _ in 0..2 {
            world.step(&[]).unwrap();
            assert_eq!(world.state().entities[1].auto_attack_target, None);
            assert_eq!(
                world.state().entities[1].hp,
                100,
                "retaliation started before the strike landed"
            );
        }
        world.step(&[]).unwrap();
        assert!(world.state().entities[1].hp < 100);
        assert_eq!(
            world.state().entities[1].auto_attack_target,
            if matches!(case, 0 | 3 | 4) {
                Some(EntityId(1))
            } else {
                None
            }
        );
        let order = world.state().entities[1].order.clone();
        let position = world.state().entities[1].position;
        world.step(&[]).unwrap();
        if case == 1 {
            assert_eq!(world.state().entities[1].position, position);
        }
        if case == 4 {
            assert!(!world.entity_visible(PlayerId(1), EntityId(1)));
            assert!(world.state().entities[1].position.x < position.x);
            for _ in 0..30 {
                world.step(&[]).unwrap();
            }
            assert!(
                world.state().entities[0].hp < 100,
                "defender must return fire"
            );
        }
        assert_eq!(
            world.state().entities[1].order,
            order,
            "automatic combat replaced the player order"
        );
        if case == 0 {
            assert_eq!(
                world.state().entities[1].auto_attack_target,
                Some(EntityId(1))
            );
            world
                .step(&[Command {
                    tick: world.tick(),
                    player: PlayerId(1),
                    sequence: 1,
                    order: Order::Stop {
                        entity: EntityId(2),
                    },
                }])
                .unwrap();
            assert_eq!(
                world.state().entities[1].auto_attack_target,
                None,
                "Stop must clear pursuit"
            );
        }
    }
}

#[test]
fn retaliation_investigates_the_shot_origin_without_tracking_hidden_movement() {
    let (mut rules, mut map) = definitions();
    rules.units[0].weapon.as_mut().unwrap().range = 96;
    rules.units[0].weapon.as_mut().unwrap().cooldown = 100;
    rules.units[0].vision_range = 128;
    rules.units[0].speed = 8;
    rules.units[1].weapon = Some(Weapon {
        range: 8,
        ..rules.units[0].weapon.clone().unwrap()
    });
    rules.units[1].speed = 4;
    rules.units[1].vision_range = 16;
    rules.units[1].acquisition_range = Some(16);
    map.fog_of_war = true;
    map.spawns[0].position.x = 112;
    map.spawns[1].position.x = 32;
    let mut world = World::new(rules, map, 0).unwrap();
    assert!(!world.entity_visible(PlayerId(1), EntityId(1)));
    world.step(&[]).unwrap();
    let origin = Position { x: 112, y: 32 };
    assert_eq!(world.state().entities[1].retaliation_position, Some(origin));
    world
        .step(&[Command {
            tick: world.tick(),
            player: PlayerId(0),
            sequence: 1,
            order: Order::Move {
                entity: EntityId(1),
                target: Position { x: 240, y: 96 },
            },
        }])
        .unwrap();
    assert_eq!(world.state().entities[1].target, Some(origin));
    for _ in 0..45 {
        world.step(&[]).unwrap();
        assert!(!world.entity_visible(PlayerId(1), EntityId(1)));
    }
    let defender = &world.state().entities[1];
    assert_eq!(defender.position, origin);
    assert_eq!(defender.auto_attack_target, None);
    assert_eq!(defender.retaliation_position, None);
    assert_eq!(world.state().entities[0].hp, 100);
}

#[test]
fn marines_cannot_acquire_or_shoot_emerging_guards_without_detection() {
    for fog in [false, true] {
        for scanned in [false, true] {
            let (mut rules, mut map) = definitions();
            rules.units[0].weapon.as_mut().unwrap().range = 96;
            rules.units[0].weapon.as_mut().unwrap().cooldown = 1;
            rules.units[0].speed = 0;
            rules.units[0].vision_range = 128;
            rules.units[0].scanner = Some(Scanner {
                energy_max: 50,
                energy_initial: 50,
                energy_regeneration: 0,
                cost: 50,
                radius: 96,
                duration: 3,
            });
            rules.units[1].weapon = Some(Weapon {
                range: 8,
                ..rules.units[0].weapon.clone().unwrap()
            });
            rules.units[1].vision_range = 128;
            rules.units[1].acquisition_range = Some(96);
            rules.units[1].cloak = Some(straterust_engine::sim::Cloak {
                can_move: false,
                can_attack: false,
                blocks_movement: false,
                reveal_ticks: 3,
                reveal_on_order: true,
                auto_reveal: true,
                ..Default::default()
            });
            map.spawns[1].position.x = 112;
            map.spawns[1].cloaked = true;
            map.fog_of_war = fog;
            map.mission = Some(guard_mission(&map));
            let mut world = World::new(rules, map, 0).unwrap();
            assert!(!world.entity_visible(PlayerId(0), EntityId(2)));
            let commands = if scanned {
                vec![Command {
                    tick: Tick(0),
                    player: PlayerId(0),
                    sequence: 1,
                    order: Order::Scan {
                        entity: EntityId(1),
                        target: Position { x: 112, y: 32 },
                    },
                }]
            } else {
                vec![]
            };
            world.step(&commands).unwrap();
            assert_eq!(world.state().entities[1].cloak_transition, 3);
            assert_eq!(world.entity_visible(PlayerId(0), EntityId(2)), scanned);
            assert!(
                world.entity_visible(PlayerId(1), EntityId(2)),
                "owner can see its emerging unit"
            );
            if scanned {
                assert!(world.state().entities[1].hp < 100);
            } else {
                assert_eq!(world.state().entities[1].hp, 100);
                assert_eq!(world.state().entities[0].auto_attack_target, None);
            }
            world.step(&[]).unwrap();
            assert_eq!(world.entity_visible(PlayerId(0), EntityId(2)), scanned);
            let hp = world.state().entities[1].hp;
            world.step(&[]).unwrap(); // Scan expires while still underground.
            assert_eq!(world.state().entities[1].cloak_transition, 1);
            assert!(!world.entity_visible(PlayerId(0), EntityId(2)));
            assert_eq!(world.state().entities[0].auto_attack_target, None);
            assert_eq!(world.state().entities[1].hp, hp);
            world.step(&[]).unwrap();
            assert_eq!(world.state().entities[1].cloak_transition, 0);
            assert!(world.entity_visible(PlayerId(0), EntityId(2)));
            assert_eq!(world.state().entities[1].hp, hp);
            world.step(&[]).unwrap();
            assert!(
                world.state().entities[1].hp < hp,
                "Marine fires after emergence"
            );
        }
    }
}

#[test]
fn detected_burrowed_guard_remembers_distant_fire_and_unburrows_before_pursuing() {
    let (mut rules, mut map) = definitions();
    rules.units[0].weapon.as_mut().unwrap().range = 96;
    rules.units[0].weapon.as_mut().unwrap().cooldown = 100;
    rules.units[0].speed = 0;
    rules.units[0].vision_range = 128;
    rules.units[0].scanner = Some(Scanner {
        energy_max: 50,
        energy_initial: 50,
        energy_regeneration: 0,
        cost: 50,
        radius: 96,
        duration: 100,
    });
    rules.units[1].weapon = Some(Weapon {
        range: 8,
        ..rules.units[0].weapon.clone().unwrap()
    });
    rules.units[1].speed = 4;
    rules.units[1].acquisition_range = Some(16);
    rules.units[1].vision_range = 16;
    rules.units[1].cloak = Some(straterust_engine::sim::Cloak {
        can_move: false,
        can_attack: false,
        blocks_movement: false,
        reveal_ticks: 3,
        reveal_on_order: true,
        auto_reveal: true,
        ..Default::default()
    });
    map.spawns[1].position.x = 112;
    map.spawns[1].cloaked = true;
    map.fog_of_war = true;
    map.mission = Some(guard_mission(&map));
    let mut world = World::new(rules, map, 0).unwrap();
    world
        .step(&[Command {
            tick: Tick(0),
            player: PlayerId(0),
            sequence: 1,
            order: Order::Scan {
                entity: EntityId(1),
                target: Position { x: 112, y: 32 },
            },
        }])
        .unwrap();
    let defender = &world.state().entities[1];
    assert!(defender.hp < 100 && defender.cloaked);
    assert_eq!(defender.auto_attack_target, Some(EntityId(1)));
    assert!(!world.entity_visible(PlayerId(1), EntityId(1)));
    world.step(&[]).unwrap();
    assert!(!world.state().entities[1].cloaked);
    assert_eq!(world.state().entities[1].cloak_transition, 3);
    for _ in 0..3 {
        world.step(&[]).unwrap();
        assert_eq!(world.state().entities[1].position.x, 112);
    }
    for _ in 0..24 {
        world.step(&[]).unwrap();
    }
    assert!(world.state().entities[1].position.x < 112);
    assert!(world.state().entities[0].hp < 100);
}

#[test]
fn guards_pursue_within_acquisition_range_but_hold_and_legacy_idle_stay_put() {
    for (radius, hold, moves) in [
        (None, false, false),
        (Some(96), true, false),
        (Some(96), false, true),
    ] {
        let (mut rules, mut map) = definitions();
        rules.units[0].acquisition_range = radius;
        map.spawns[1].position.x = 96;
        let mut world = World::new(rules, map, 0).unwrap();
        if hold {
            assert!(
                world
                    .step(&[Command {
                        tick: Tick(0),
                        player: PlayerId(0),
                        sequence: 1,
                        order: Order::Hold {
                            entity: EntityId(1)
                        },
                    }])
                    .unwrap()[0]
                    .rejection
                    .is_none()
            );
        }
        for _ in 0..20 {
            world.step(&[]).unwrap();
        }
        assert_eq!(world.state().entities[0].position.x > 32, moves);
        assert_eq!(world.state().entities[1].hp < 100, moves);
    }
}

#[test]
fn new_combat_rules_are_hashed_and_invalid_acquisition_is_rejected() {
    let (rules, map) = definitions();
    let baseline = World::new(rules.clone(), map.clone(), 0).unwrap();
    for field in 0..3 {
        let mut changed = rules.clone();
        match field {
            0 => changed.units[0].size = UnitSize::Small,
            1 => changed.units[0].acquisition_range = Some(96),
            _ => changed.units[0].weapon.as_mut().unwrap().damage_kind = DamageKind::Normal,
        }
        assert_ne!(
            baseline.rules_hash(),
            World::new(changed, map.clone(), 0).unwrap().rules_hash()
        );
    }
    let mut changed = rules;
    changed.units[0].acquisition_range = Some(32769);
    assert!(World::new(changed, map, 0).is_err());
}

#[test]
#[ignore = "requires a private mission 3 package; checks the starting Command Center attachment"]
fn mission_three_starting_command_center_addon_placement() {
    let path = std::env::var_os("STRATERUST_ASSET_PACKAGE").unwrap();
    let package = straterust_engine::content::Package::load(std::path::Path::new(&path)).unwrap();
    let initial = package.world(7).unwrap();
    let mut rules = initial.rules().clone();
    rules.starting_resources = ["minerals", "gas"]
        .map(|kind| ResourceAmount {
            kind: kind.into(),
            amount: 1000,
        })
        .to_vec();
    let mut map = initial.map().clone();
    map.spawns.push(Spawn {
        unit_type: UnitTypeId(2),
        position: Position { x: 416, y: 2720 },
        ..Spawn::default()
    });
    let mut world = World::new(rules, map, 7).unwrap();
    let parent = world
        .state()
        .entities
        .iter()
        .find(|entity| entity.owner == PlayerId(0) && entity.unit_type == UnitTypeId(3))
        .unwrap()
        .id;
    let position = world.addon_position(parent).unwrap();
    assert_eq!(position, Position { x: 416, y: 2720 });
    assert_eq!(
        world.build_rejection(PlayerId(0), parent, UnitTypeId(16), position),
        None
    );
    let result = world
        .step(&[Command {
            tick: world.tick(),
            player: PlayerId(0),
            sequence: 1,
            order: Order::Build {
                entity: parent,
                unit_type: UnitTypeId(16),
                position,
            },
        }])
        .unwrap();
    assert!(result[0].rejection.is_none(), "{:?}", result[0]);
}
