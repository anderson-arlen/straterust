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
        minimum_range: 0,
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
            multiplayer: None,
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
fn automatic_acquisition_respects_distinct_air_range_and_large_target_edges() {
    use straterust_engine::map::MovementClass;
    let base = world(vec![spawn(1, 5, 176), spawn(1, 4, 240)], true);
    let mut rules = base.rules().clone();
    rules.units[0].weapon.as_mut().unwrap().range = 16;
    rules.units[0].air_weapon = Some(Weapon {
        range: 128,
        targets_air: true,
        ..rules.units[0].weapon.as_ref().unwrap().clone()
    });
    rules.units[0].acquisition_range = None;
    rules.units[3].movement_class = MovementClass::Air;
    let mut air = World::new(rules.clone(), base.map().clone(), 7).unwrap();
    command(
        &mut air,
        0,
        Order::Hold {
            entity: EntityId(1),
        },
    );
    assert_eq!(
        air.state().entities[0].last_attack_target,
        Some(EntityId(3))
    );
    assert_eq!(
        air.state().entities[1].hp,
        1000,
        "ground target is outside the shorter ground range"
    );
    // Distance is measured from the target's edge, including large buildings.
    rules.units[4].footprint.width = 128;
    let mut map = base.map().clone();
    map.spawns.truncate(2);
    map.spawns[1].position.x = 208;
    let mut large = World::new(rules, map, 7).unwrap();
    command(
        &mut large,
        0,
        Order::Hold {
            entity: EntityId(1),
        },
    );
    assert_eq!(
        large.state().entities[0].last_attack_target,
        Some(EntityId(2))
    );
}

fn siege_world(enemies: Vec<Spawn>, minimum_range: u32) -> World {
    let original = world(enemies, true);
    let mut rules = original.rules().clone();
    let weapon = rules.units[0].weapon.as_mut().unwrap();
    weapon.range = 256;
    weapon.minimum_range = minimum_range;
    rules.units[0].acquisition_range = Some(256);
    // Stationary, unarmed targets isolate the firing-distance checks.
    for unit in &mut rules.units[1..] {
        unit.weapon = None;
    }
    World::new(rules, original.map().clone(), 7).unwrap()
}

#[test]
fn weapon_minimum_range_checks_footprint_edges_and_diagonal_boundaries() {
    for (dx, dy, fires) in [
        (71, 0, false),
        (72, 0, true),
        (73, 0, true),
        (53, 53, false),
        (54, 54, true),
    ] {
        let mut enemy = spawn(1, 5, 128 + dx);
        enemy.position.y += dy;
        let mut w = siege_world(vec![enemy], 64);
        command(
            &mut w,
            0,
            Order::Hold {
                entity: EntityId(1),
            },
        );
        assert_eq!(w.state().entities[1].hp < 1000, fires, "{dx},{dy}");
        assert_eq!(w.state().entities[0].last_attack_target.is_some(), fires);
        assert_eq!(w.state().entities[0].position, Position { x: 128, y: 128 });
    }
    // Default-zero weapons retain their existing adjacent-target behavior.
    let mut adjacent = siege_world(vec![spawn(1, 5, 136)], 0);
    adjacent.step(&[]).unwrap();
    assert_eq!(adjacent.state().entities[1].hp, 999);
}

#[test]
fn automatic_siege_orders_skip_close_targets_and_deliberate_orders_wait_without_firing() {
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
        let mut w = siege_world(vec![spawn(1, 5, 160), spawn(1, 5, 224)], 64);
        command(&mut w, 0, order);
        assert_eq!(w.state().entities[0].last_attack_target, Some(EntityId(3)));
        assert_eq!(w.state().entities[1].hp, 1000);
        assert_eq!(w.state().entities[2].hp, 999);
    }
    let mut w = siege_world(vec![spawn(1, 5, 160)], 64);
    command(
        &mut w,
        0,
        Order::Attack {
            entity: EntityId(1),
            target: EntityId(2),
        },
    );
    for _ in 0..20 {
        w.step(&[]).unwrap();
    }
    let actor = &w.state().entities[0];
    assert_eq!(
        actor.order,
        UnitOrder::Attack {
            target: EntityId(2)
        }
    );
    assert_eq!(actor.position, Position { x: 128, y: 128 });
    assert_eq!(actor.cooldown, 0);
    assert_eq!(actor.last_attack_target, None);
    assert_eq!(w.state().entities[1].hp, 1000);
}

#[test]
fn older_saves_use_current_minimum_range_and_strict_snapshots_reject_changed_rules() {
    use straterust_engine::session::{SavedGame, ServerSession};
    let mut old = ServerSession::new(
        siege_world(vec![spawn(1, 5, 160), spawn(1, 5, 224)], 0),
        7,
        vec![PlayerId(0)],
    )
    .unwrap();
    old.advance(&[]).unwrap();
    assert_eq!(target(old.world()), Some(EntityId(2)));
    let save = SavedGame::decode(&SavedGame::capture(&old).unwrap().encode().unwrap()).unwrap();
    let updated = siege_world(vec![spawn(1, 5, 160), spawn(1, 5, 224)], 64);
    assert_ne!(old.world().rules_hash(), updated.rules_hash());
    assert!(
        updated
            .restore_snapshot(old.world().save_snapshot().unwrap())
            .is_err()
    );
    let mut resumed = ServerSession::restore_saved(&updated, save).unwrap();
    let near_hp = resumed.world().state().entities[1].hp;
    for _ in 0..10 {
        resumed.advance(&[]).unwrap();
    }
    assert_eq!(target(resumed.world()), Some(EntityId(3)));
    assert_eq!(resumed.world().state().entities[1].hp, near_hp);
    assert_eq!(
        resumed.world().state().entities[0].last_attack_target,
        Some(EntityId(3))
    );
    let snapshot = resumed.world().save_snapshot().unwrap();
    let restored = updated.restore_snapshot(snapshot).unwrap();
    assert_eq!(restored.state_hash(), resumed.world().state_hash());
    let mut invalid = updated.rules().clone();
    invalid.units[0].weapon.as_mut().unwrap().minimum_range = 257;
    assert!(World::new(invalid, updated.map().clone(), 7).is_err());
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
fn manual_only_targets_remain_hostile_but_require_deliberate_attacks_even_after_loading() {
    use straterust_engine::session::{SavedGame, ServerSession};
    let mut old = world(vec![spawn(1, 6, 160)], true);
    let mut rules = old.rules().clone();
    let wall = &mut rules.units[5];
    let original = ron::ser::to_string(wall).unwrap();
    assert!(!original.contains("manual_target_only"));
    assert!(
        !ron::from_str::<UnitType>(&original)
            .unwrap()
            .manual_target_only
    );
    wall.manual_target_only = true;
    let updated = World::new(rules, old.map().clone(), 7).unwrap();
    assert_ne!(old.rules_hash(), updated.rules_hash());
    assert!(
        updated
            .restore_snapshot(old.save_snapshot().unwrap())
            .is_err()
    );
    assert!(updated.is_enemy_entity(PlayerId(0), &updated.state().entities[1]));
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
        let mut w = updated.clone();
        command(&mut w, 0, order);
        for _ in 0..4 {
            w.step(&[]).unwrap();
        }
        assert_eq!(target(&w), None);
        assert_eq!(w.state().entities[1].hp, 1000);
    }
    old.step(&[]).unwrap();
    assert_eq!(target(&old), Some(EntityId(2)));
    let hp = old.state().entities[1].hp;
    let saved =
        SavedGame::capture(&ServerSession::new(old, 7, vec![PlayerId(0)]).unwrap()).unwrap();
    let mut restored = ServerSession::restore_saved(&updated, saved).unwrap();
    restored.advance(&[]).unwrap();
    assert_eq!(target(restored.world()), None);
    assert_eq!(restored.world().state().entities[1].hp, hp);
    let mut explicit = restored.world().clone();
    command(
        &mut explicit,
        0,
        Order::Attack {
            entity: EntityId(1),
            target: EntityId(2),
        },
    );
    for _ in 0..10 {
        explicit.step(&[]).unwrap();
    }
    assert!(explicit.state().entities[1].hp < hp);
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
fn automatic_combat_prioritizes_troops_over_an_armed_building_but_explicit_attacks_stay_fixed() {
    let original = world(vec![spawn(1, 6, 160), spawn(1, 3, 224)], true);
    let mut rules = original.rules().clone();
    rules.units[5].weapon = rules.units[0].weapon.clone();
    let mut automatic = World::new(rules.clone(), original.map().clone(), 7).unwrap();
    automatic.step(&[]).unwrap();
    assert_eq!(target(&automatic), Some(EntityId(3)));
    let mut explicit = World::new(rules, original.map().clone(), 7).unwrap();
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
fn idle_and_patrolling_units_switch_from_buildings_or_workers_to_combatants() {
    for kind in [2, 6] {
        for patrol in [false, true] {
            let mut world = world(vec![spawn(1, kind, 160), spawn(1, 3, 320)], true);
            if patrol {
                command(
                    &mut world,
                    0,
                    Order::Patrol {
                        entity: EntityId(1),
                        target: Position { x: 448, y: 128 },
                    },
                );
            } else {
                world.step(&[]).unwrap();
            }
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
            // A new movement order must still recall the unit from combat.
            command(
                &mut world,
                0,
                Order::Move {
                    entity: EntityId(1),
                    target: Position { x: 32, y: 128 },
                },
            );
            let x = world.state().entities[0].position.x;
            world.step(&[]).unwrap();
            assert_eq!(target(&world), None);
            assert!(world.state().entities[0].position.x < x);
        }
    }
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
