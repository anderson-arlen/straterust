use straterust_engine::sim::*;

fn spawn(owner: u16, kind: u16, x: i32) -> Spawn {
    Spawn {
        owner: PlayerId(owner),
        unit_type: UnitTypeId(kind),
        position: Position { x, y: 128 },
        ..Spawn::default()
    }
}
fn world(enemies: Vec<Spawn>, prioritize: bool) -> World {
    let weapon = |range| Weapon {
        damage: 1,
        range,
        cooldown: 10,
        cooldown_jitter: None,
        targets_air: false,
        damage_kind: Default::default(),
        splash: None,
        strikes: vec![],
    };
    let mut units = (1..=6)
        .map(|id| UnitType {
            id: UnitTypeId(id),
            speed: 8,
            max_hp: 1000,
            footprint: Footprint {
                width: 8,
                height: 8,
            },
            ..UnitType::default()
        })
        .collect::<Vec<_>>();
    units[0].weapon = Some(weapon(64));
    units[0].acquisition_range = Some(120);
    units[1].weapon = Some(weapon(1));
    units[1].worker = Some(WorkerStats {
        capacity: 8,
        harvest_amount: 8,
        harvest_ticks: 2,
        build_rate: 1,
        resource_kinds: vec!["ore".into()],
    });
    units[2].weapon = Some(weapon(1));
    units[3].weapon = Some(Weapon {
        targets_air: true,
        ..weapon(1)
    });
    units[3].attacks_ground = false;
    units[5].speed = 0;
    units[5].structure = true;
    let mut spawns = vec![spawn(0, 1, 128)];
    spawns.extend(enemies);
    World::new(
        Rules {
            id: "threat-priority".into(),
            prioritize_threats: prioritize,
            units,
            ..Rules::default()
        },
        Map {
            id: "threat-priority".into(),
            width: 512,
            height: 256,
            players: 2,
            spawns,
            resources: vec![],
            start_locations: vec![],
            terrain: None,
            fog_of_war: false,
            initial_explored: Default::default(),
            creation: Default::default(),
            ai: vec![],
            mission: None,
        },
        7,
    )
    .unwrap()
}
fn command(world: &mut World, player: u16, order: Order) {
    let outcomes = world
        .step(&[Command {
            tick: world.tick(),
            player: PlayerId(player),
            sequence: world.state().last_sequences[usize::from(player)] + 1,
            order,
        }])
        .unwrap();
    assert!(outcomes[0].rejection.is_none(), "{:?}", outcomes[0]);
}
fn attack_move(world: &mut World) {
    command(
        world,
        0,
        Order::AttackMove {
            entity: EntityId(1),
            target: Position { x: 448, y: 128 },
        },
    );
}
fn target(world: &World) -> Option<EntityId> {
    world.state().entities[0].auto_attack_target
}

#[test]
fn attack_move_ranks_combatants_before_closer_workers_and_buildings() {
    for closer in [2, 5, 6] {
        let enemies = vec![spawn(1, closer, 160), spawn(1, 3, 224)];
        let mut original = world(enemies.clone(), true);
        attack_move(&mut original);
        assert_eq!(target(&original), Some(EntityId(3)));
        let mut nearest = world(enemies, false);
        attack_move(&mut nearest);
        assert_eq!(
            target(&nearest),
            Some(EntityId(2)),
            "other rulesets retain nearest-first behavior"
        );
    }
}

#[test]
fn workers_and_incompatible_weapons_rank_before_unarmed_units() {
    for preferred in [2, 4] {
        for closer in [5, 6] {
            let mut world = world(vec![spawn(1, closer, 160), spawn(1, preferred, 208)], true);
            attack_move(&mut world);
            assert_eq!(target(&world), Some(EntityId(3)));
        }
    }
}

#[test]
fn newly_arriving_threat_replaces_worker_during_attack_move() {
    let mut world = world(vec![spawn(1, 2, 160), spawn(1, 3, 320)], true);
    attack_move(&mut world);
    assert_eq!(target(&world), Some(EntityId(2)));
    command(
        &mut world,
        1,
        Order::Move {
            entity: EntityId(3),
            target: Position { x: 224, y: 128 },
        },
    );
    for _ in 0..12 {
        world.step(&[]).unwrap();
    }
    assert_eq!(target(&world), Some(EntityId(3)));
}

#[test]
fn engaged_combatant_is_retained_when_equal_priority_enemy_arrives() {
    let mut world = world(vec![spawn(1, 3, 176), spawn(1, 3, 320)], true);
    attack_move(&mut world);
    command(
        &mut world,
        1,
        Order::Move {
            entity: EntityId(3),
            target: Position { x: 144, y: 128 },
        },
    );
    for _ in 0..24 {
        world.step(&[]).unwrap();
    }
    assert_eq!(target(&world), Some(EntityId(2)));
}

#[test]
fn explicit_attack_is_preserved_and_burrowed_threats_are_not_acquired() {
    let mut explicit = world(vec![spawn(1, 2, 160), spawn(1, 3, 224)], true);
    command(
        &mut explicit,
        0,
        Order::Attack {
            entity: EntityId(1),
            target: EntityId(2),
        },
    );
    assert_eq!(
        explicit.state().entities[0].order,
        UnitOrder::Attack {
            target: EntityId(2)
        }
    );
    assert!(explicit.state().entities[1].hp < 1000);
    assert_eq!(explicit.state().entities[2].hp, 1000);
    let mut hidden = world(
        vec![
            spawn(1, 2, 160),
            Spawn {
                cloaked: true,
                ..spawn(1, 3, 176)
            },
        ],
        true,
    );
    attack_move(&mut hidden);
    assert_eq!(target(&hidden), Some(EntityId(2)));
}
