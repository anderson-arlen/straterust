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
        friendly_splash: false,
        projectile_speed: 0,
        damage: 1,
        range,
        cooldown: 10,
        cooldown_jitter: None,
        targets_air: false,
        target_classes: Vec::new(),
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
        idle_resource_radius: 256,
    });
    units[2].weapon = Some(weapon(1));
    units[3].weapon = Some(Weapon {
        friendly_splash: false,
        projectile_speed: 0,
        targets_air: true,
        target_classes: Vec::new(),
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
fn neutral_wildlife_is_ignored_by_automatic_combat_but_can_be_attacked_deliberately() {
    let original = world(
        vec![spawn(1, 5, 160), spawn(1, 3, 184), spawn(1, 6, 256)],
        false,
    );
    let mut rules = original.rules().clone();
    let wildlife = rules
        .units
        .iter_mut()
        .find(|u| u.id == UnitTypeId(5))
        .unwrap();
    let old_definition = ron::ser::to_string(wildlife).unwrap();
    assert!(!old_definition.contains("neutral"));
    assert!(!ron::from_str::<UnitType>(&old_definition).unwrap().neutral);
    wildlife.neutral = true;
    for order in [
        Order::Stop {
            entity: EntityId(1),
        },
        Order::Hold {
            entity: EntityId(1),
        },
        Order::AttackMove {
            entity: EntityId(1),
            target: Position { x: 448, y: 128 },
        },
        Order::Patrol {
            entity: EntityId(1),
            target: Position { x: 448, y: 128 },
        },
    ] {
        let mut world = World::new(rules.clone(), original.map().clone(), 7).unwrap();
        assert_ne!(world.rules_hash(), original.rules_hash());
        assert!(!world.is_enemy_entity(PlayerId(0), &world.state().entities[1]));
        assert!(world.is_enemy_entity(PlayerId(0), &world.state().entities[3]));
        let hold = matches!(order, Order::Hold { .. });
        command(&mut world, 0, order);
        assert_eq!(target(&world), (!hold).then_some(EntityId(3)));
        assert_eq!(world.state().entities[1].hp, 1000);
        assert!(world.state().entities[2].hp < 1000);
    }
    // Resume progress from before the definition fix and discard its old animal target.
    use straterust_engine::session::{SavedGame, ServerSession};
    let mut old = ServerSession::new(original.clone(), 7, vec![PlayerId(0)]).unwrap();
    old.advance(&[]).unwrap();
    assert_eq!(target(old.world()), Some(EntityId(2)));
    let old_hp = old.world().state().entities[1].hp;
    let saved = SavedGame::decode(&SavedGame::capture(&old).unwrap().encode().unwrap()).unwrap();
    let updated = World::new(rules.clone(), original.map().clone(), 7).unwrap();
    let mut resumed = ServerSession::restore_saved(&updated, saved).unwrap();
    resumed.advance(&[]).unwrap();
    assert_eq!(target(resumed.world()), Some(EntityId(3)));
    assert_eq!(resumed.world().state().entities[1].hp, old_hp);
    let mut world = World::new(rules, original.map().clone(), 7).unwrap();
    command(
        &mut world,
        0,
        Order::Attack {
            entity: EntityId(1),
            target: EntityId(2),
        },
    );
    assert!(world.state().entities[1].hp < 1000);
    command(
        &mut world,
        0,
        Order::Attack {
            entity: EntityId(1),
            target: EntityId(4),
        },
    );
    assert_eq!(
        world.state().entities[0].order,
        UnitOrder::Attack {
            target: EntityId(4)
        }
    );
}

#[test]
fn surviving_wildlife_does_not_prevent_elimination_victory() {
    let original = world(vec![spawn(1, 5, 160), spawn(1, 3, 184)], false);
    let mut rules = original.rules().clone();
    rules.victory = true;
    rules
        .units
        .iter_mut()
        .find(|u| u.id == UnitTypeId(5))
        .unwrap()
        .neutral = true;
    rules
        .units
        .iter_mut()
        .find(|u| u.id == UnitTypeId(3))
        .unwrap()
        .max_hp = 1;
    let mut world = World::new(rules, original.map().clone(), 7).unwrap();
    world.step(&[]).unwrap();
    assert_eq!(world.state().winner, Some(PlayerId(0)));
    assert!(
        world
            .state()
            .entities
            .iter()
            .any(|e| e.unit_type == UnitTypeId(5))
    );
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
