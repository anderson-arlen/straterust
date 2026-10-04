use straterust_engine::sim::*;

fn weapon(damage: u32, air: bool) -> Weapon {
    Weapon {
        damage,
        range: 96,
        cooldown: 3,
        targets_air: air,
        cooldown_jitter: None,
        damage_kind: DamageKind::Normal,
        splash: None,
        strikes: vec![],
    }
}
fn world(detector: bool) -> World {
    World::new(
        Rules {
            id: "cloak".into(),
            units: vec![
                UnitType {
                    id: UnitTypeId(1),
                    max_hp: 100,
                    speed: 4,
                    weapon: Some(weapon(8, true)),
                    cloak: Some(Cloak {
                        energy_max: 250,
                        activation_cost: 25,
                        regeneration: 8,
                        drain: 10,
                        ..Cloak::default()
                    }),
                    ..UnitType::default()
                },
                UnitType {
                    id: UnitTypeId(2),
                    max_hp: 100,
                    speed: 0,
                    weapon: Some(weapon(1, true)),
                    detector_range: if detector { 96 } else { 0 },
                    ..UnitType::default()
                },
            ],
            ..Rules::default()
        },
        Map {
            id: "cloak".into(),
            width: 256,
            height: 256,
            players: 2,
            spawns: vec![
                Spawn {
                    unit_type: UnitTypeId(1),
                    position: Position { x: 64, y: 64 },
                    energy_percent: Some(100),
                    ..Spawn::default()
                },
                Spawn {
                    owner: PlayerId(1),
                    unit_type: UnitTypeId(2),
                    position: Position { x: 128, y: 64 },
                    ..Spawn::default()
                },
            ],
            resources: vec![],
            start_locations: vec![],
            terrain: None,
            initial_explored: Default::default(),
            creation: Default::default(),
            ai: vec![],
            mission: None,
            fog_of_war: false,
        },
        42,
    )
    .unwrap()
}
fn command(world: &World, sequence: u64, order: Order) -> Command {
    Command {
        tick: world.tick(),
        player: PlayerId(0),
        sequence,
        order,
    }
}

#[test]
fn cloak_preserves_movement_costs_energy_conceals_return_fire_and_replays() {
    let mut a = world(false);
    let mut b = a.clone();
    assert_eq!(a.state().entities[0].energy, 250 * 256);
    assert!(a.entity_visible(PlayerId(1), EntityId(1)));
    for order in [
        Order::Move {
            entity: EntityId(1),
            target: Position { x: 80, y: 64 },
        },
        Order::Cloak {
            entity: EntityId(1),
            enabled: true,
        },
    ] {
        let cmd = command(&a, a.tick().0 + 1, order);
        assert_eq!(
            a.step(std::slice::from_ref(&cmd)).unwrap()[0].rejection,
            None
        );
        b.step(&[cmd]).unwrap();
    }
    let actor = &a.state().entities[0];
    assert!(actor.cloaked);
    assert!(matches!(actor.order, UnitOrder::Move { .. }));
    assert_eq!(actor.energy, 225 * 256 - 10);
    assert!(!a.entity_visible(PlayerId(1), actor.id));
    let hp = actor.hp;
    for _ in 0..10 {
        a.step(&[]).unwrap();
        b.step(&[]).unwrap();
    }
    assert_eq!(a.state_hash(), b.state_hash());
    assert_eq!(
        a.state().entities[0].hp,
        hp,
        "remembered attacker must not bypass cloak detection"
    );
    assert!(a.state().entities[1].hp < 100, "cloaked units may attack");
    let cmd = command(
        &a,
        20,
        Order::Cloak {
            entity: EntityId(1),
            enabled: false,
        },
    );
    let energy = a.state().entities[0].energy;
    a.step(&[cmd]).unwrap();
    assert!(a.entity_visible(PlayerId(1), EntityId(1)));
    assert_eq!(a.state().entities[0].energy, energy + 8);
}
#[test]
fn detector_exposes_cloak_and_empty_energy_decloaks() {
    let mut detected = world(true);
    let cmd = command(
        &detected,
        1,
        Order::Cloak {
            entity: EntityId(1),
            enabled: true,
        },
    );
    detected.step(&[cmd]).unwrap();
    assert!(detected.entity_visible(PlayerId(1), EntityId(1)));
    assert_eq!(detected.state().entities[0].hp, 99);
    let base = world(false);
    let mut map = base.map().clone();
    map.spawns[0].energy_percent = Some(10); // exactly the 25-energy activation cost
    let mut empty = World::new(base.rules().clone(), map, 42).unwrap();
    let cmd = command(
        &empty,
        1,
        Order::Cloak {
            entity: EntityId(1),
            enabled: true,
        },
    );
    empty.step(&[cmd]).unwrap();
    assert!(!empty.state().entities[0].cloaked);
    assert_eq!(empty.state().entities[0].energy, 0);
    assert_eq!(
        empty.cloak_rejection(EntityId(1), true),
        Some(Rejection::InsufficientResources)
    );
}
#[test]
fn ground_only_units_reject_air_and_distinct_air_weapon_uses_its_damage() {
    let base = world(false);
    for air_damage in [None, Some(15)] {
        let mut rules = base.rules().clone();
        rules.units[0].weapon = Some(weapon(8, false));
        rules.units[0].air_weapon = air_damage.map(|damage| weapon(damage, true));
        rules.units[1].weapon = None;
        rules.units[1].movement_class = MovementClass::Air;
        let mut world = World::new(rules, base.map().clone(), 42).unwrap();
        let cmd = command(
            &world,
            1,
            Order::Attack {
                entity: EntityId(1),
                target: EntityId(2),
            },
        );
        let result = world.step(&[cmd]).unwrap();
        if let Some(damage) = air_damage {
            assert_eq!(result[0].rejection, None);
            assert_eq!(world.state().entities[1].hp, 100 - damage);
            assert!(world.state().entities[0].last_attack_air);
        } else {
            assert_eq!(result[0].rejection, Some(Rejection::InvalidTarget));
            assert_eq!(world.state().entities[1].hp, 100);
        }
    }
}

fn mobile_defender(detector: bool) -> World {
    let base = world(detector);
    let mut rules = base.rules().clone();
    rules.units[1].speed = 4;
    rules.units[1].footprint = Footprint {
        width: 8,
        height: 8,
    };
    let mut map = base.map().clone();
    map.width = 512;
    World::new(rules, map, 42).unwrap()
}

#[test]
fn undetected_hits_make_idle_attack_move_and_patrol_units_retreat_and_replay() {
    for order in [
        None,
        Some(Order::AttackMove {
            entity: EntityId(2),
            target: Position { x: 32, y: 64 },
        }),
        Some(Order::Patrol {
            entity: EntityId(2),
            target: Position { x: 32, y: 64 },
        }),
    ] {
        let mut a = mobile_defender(false);
        let mut map = a.map().clone();
        if order.is_none() {
            map.ai.push(AiController {
                player: PlayerId(1),
                home: Position { x: 128, y: 64 },
                radius: 512,
                active: true,
                program: vec![AiInstruction::Wait(100), AiInstruction::Stop],
            });
        }
        a = World::new(a.rules().clone(), map, 42).unwrap();
        let mut b = a.clone();
        let mut commands = vec![command(
            &a,
            1,
            Order::Cloak {
                entity: EntityId(1),
                enabled: true,
            },
        )];
        if let Some(order) = order {
            commands.push(Command {
                tick: a.tick(),
                player: PlayerId(1),
                sequence: 1,
                order,
            });
        }
        for result in a.step(&commands).unwrap() {
            assert_eq!(result.rejection, None);
        }
        b.step(&commands).unwrap();
        let defender = &a.state().entities[1];
        let start = defender.position;
        assert!(defender.hp < 100);
        assert_eq!(defender.auto_attack_target, None);
        assert_eq!(defender.retaliation_position, None);
        assert!(matches!(defender.order, UnitOrder::Move { target } if target.x > start.x));
        for _ in 0..12 {
            a.step(&[]).unwrap();
            b.step(&[]).unwrap();
        }
        assert!(
            a.state().entities[1].position.x > start.x,
            "AI guards must keep retreating"
        );
        assert_eq!(
            a.state().entities[0].hp,
            100,
            "no futile shots at undetected attackers"
        );
        assert_eq!(a.state_hash(), b.state_hash());
    }
}

#[test]
fn detection_allows_return_fire_and_hold_or_move_orders_remain_explicit() {
    let mut detected = mobile_defender(true);
    let cmd = command(
        &detected,
        1,
        Order::Cloak {
            entity: EntityId(1),
            enabled: true,
        },
    );
    detected.step(&[cmd]).unwrap();
    assert_eq!(detected.state().entities[0].hp, 99);
    assert_eq!(detected.state().entities[1].order, UnitOrder::Idle);
    assert_eq!(
        detected.state().entities[1].auto_attack_target,
        Some(EntityId(1))
    );
    for order in [
        Order::Hold {
            entity: EntityId(2),
        },
        Order::Move {
            entity: EntityId(2),
            target: Position { x: 32, y: 64 },
        },
    ] {
        let mut world = mobile_defender(false);
        let commands = [
            command(
                &world,
                1,
                Order::Cloak {
                    entity: EntityId(1),
                    enabled: true,
                },
            ),
            Command {
                tick: world.tick(),
                player: PlayerId(1),
                sequence: 1,
                order,
            },
        ];
        world.step(&commands).unwrap();
        let before = world.state().entities[1].clone();
        world.step(&[]).unwrap();
        let after = &world.state().entities[1];
        assert_eq!(after.order, before.order);
        assert!(after.position.x <= before.position.x);
    }
}

#[test]
fn unarmed_units_retreat_and_shorter_reachable_routes_avoid_walls() {
    let base = mobile_defender(false);
    let mut rules = base.rules().clone();
    rules.units[1].weapon = None;
    rules.units.push(UnitType {
        id: UnitTypeId(3),
        speed: 0,
        structure: true,
        footprint: Footprint {
            width: 16,
            height: 256,
        },
        ..UnitType::default()
    });
    let mut map = base.map().clone();
    map.spawns.push(Spawn {
        unit_type: UnitTypeId(3),
        position: Position { x: 224, y: 128 },
        ..Spawn::default()
    });
    let mut world = World::new(rules, map, 42).unwrap();
    let cmd = command(
        &world,
        1,
        Order::Cloak {
            entity: EntityId(1),
            enabled: true,
        },
    );
    world.step(&[cmd]).unwrap();
    assert_eq!(
        world.state().entities[1].order,
        UnitOrder::Move {
            target: Position { x: 192, y: 64 }
        }
    );
    world.step(&[]).unwrap();
    assert!(world.state().entities[1].position.x > 128);
}

#[test]
fn retreat_at_map_edge_uses_a_reachable_side_direction() {
    let base = mobile_defender(false);
    let mut map = base.map().clone();
    map.spawns[0].position.x = 448;
    map.spawns[1].position.x = 508;
    let mut world = World::new(base.rules().clone(), map, 42).unwrap();
    let cmd = command(
        &world,
        1,
        Order::Cloak {
            entity: EntityId(1),
            enabled: true,
        },
    );
    world.step(&[cmd]).unwrap();
    assert!(matches!(world.state().entities[1].order,
        UnitOrder::Move { target } if target.y > 64 && target.x <= 508));
    world.step(&[]).unwrap();
    assert!(world.state().entities[1].position.y > 64);
}
