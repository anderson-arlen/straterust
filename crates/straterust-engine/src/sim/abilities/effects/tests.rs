use super::*;
mod strikes;
mod support;

fn world(effect: AbilityEffect) -> World {
    let units = vec![
        UnitType {
            id: UnitTypeId(1),
            max_hp: 200,
            speed: 8,
            vision_range: 192,
            energy_pool: Some(EnergyPool {
                maximum: 200,
                initial: 200,
                regeneration: 0,
            }),
            abilities: vec![TargetedAbility {
                id: AbilityId(1),
                research: None,
                energy: 0,
                range: 512,
                effect,
            }],
            ..Default::default()
        },
        UnitType {
            id: UnitTypeId(2),
            max_hp: 1000,
            max_shields: 100,
            speed: 4,
            ..Default::default()
        },
        UnitType {
            id: UnitTypeId(3),
            max_hp: 100,
            max_shields: 100,
            speed: 4,
            ..Default::default()
        },
        UnitType {
            id: UnitTypeId(4),
            structure: true,
            speed: 0,
            max_hp: 500,
            production_capacity: 1,
            trains: vec![UnitTypeId(5)],
            supply_provided: 20,
            ..Default::default()
        },
        UnitType {
            id: UnitTypeId(5),
            speed: 0,
            cost: vec![ResourceAmount {
                kind: "minerals".into(),
                amount: 10,
            }],
            build_ticks: 2,
            supply_used: 2,
            ..Default::default()
        },
        UnitType {
            id: UnitTypeId(6),
            speed: 0,
            structure: true,
            max_hp: 100,
            ..Default::default()
        },
    ];
    let spawns = [
        (0, 1, 64, 64),
        (1, 2, 160, 64),
        (0, 3, 96, 96),
        (0, 4, 64, 160),
    ]
    .into_iter()
    .map(|(o, u, x, y)| Spawn {
        owner: PlayerId(o),
        unit_type: UnitTypeId(u),
        position: Position { x, y },
        ..Default::default()
    })
    .collect();
    World::new(
        Rules {
            id: "effect-policies".into(),
            units,
            starting_resources: vec![ResourceAmount {
                kind: "minerals".into(),
                amount: 1000,
            }],
            ..Default::default()
        },
        Map {
            id: "effect-policies".into(),
            width: 512,
            height: 512,
            players: 2,
            spawns,
            creation: BTreeMap::new(),
            initial_explored: BTreeMap::new(),
            start_locations: vec![],
            resources: vec![],
            ai: vec![],
            fog_of_war: false,
            mission: None,
            terrain: None,
        },
        1,
    )
    .unwrap()
}
fn issue(w: &mut World, player: u16, order: Order) -> Option<Rejection> {
    w.step(&[Command {
        tick: w.tick(),
        player: PlayerId(player),
        sequence: w.state.last_sequences[usize::from(player)] + 1,
        order,
    }])
    .unwrap()[0]
        .rejection
        .clone()
}
fn cast(w: &mut World, target: AbilityTarget) {
    assert_eq!(
        issue(
            w,
            0,
            Order::Cast {
                entity: EntityId(1),
                ability: AbilityId(1),
                target
            }
        ),
        None
    );
}
fn ticks(w: &mut World, n: usize) {
    for _ in 0..n {
        w.step(&[]).unwrap();
    }
}

#[test]
fn free_stationary_concealment_shares_a_casters_energy_without_allowing_casts() {
    let base = world(AbilityEffect::Protection {
        radius: 64,
        duration: 30,
    });
    let mut rules = (*base.rules).clone();
    rules.units[0].cloak = Some(Cloak {
        can_move: false,
        can_attack: false,
        ..Default::default()
    });
    rules.units[0].energy_pool.as_mut().unwrap().regeneration = 8;
    let mut w = World::new(rules, (*base.map).clone(), 1).unwrap();
    w.state.entities[0].energy = 100 * 256;
    assert_eq!(
        issue(
            &mut w,
            0,
            Order::Cloak {
                entity: EntityId(1),
                enabled: true
            }
        ),
        None
    );
    assert!(w.state.entities[0].cloaked);
    assert_eq!(w.state.entities[0].energy, 100 * 256 + 8);
    assert_eq!(
        w.cast_rejection(
            EntityId(1),
            AbilityId(1),
            AbilityTarget::Point(Position { x: 64, y: 64 })
        ),
        Some(Rejection::UnsupportedOrder)
    );
    assert_eq!(
        issue(
            &mut w,
            0,
            Order::Cloak {
                entity: EntityId(1),
                enabled: false
            }
        ),
        None
    );
    assert_eq!(
        w.cast_rejection(
            EntityId(1),
            AbilityId(1),
            AbilityTarget::Point(Position { x: 64, y: 64 })
        ),
        None
    );
}

#[test]
fn linked_buildings_construct_one_free_exit_transport_and_die_as_a_pair() {
    let base = world(AbilityEffect::Parasite);
    let mut rules = (*base.rules).clone();
    rules.units[0].abilities[0].effect = AbilityEffect::LinkedTransport {
        exit: UnitTypeId(1),
        passengers: vec![UnitTypeId(3)],
    };
    rules.units[0].speed = 0;
    rules.units[0].structure = true;
    rules.units[0].autonomous_construction = true;
    rules.units[0].build_ticks = 4;
    rules.units[0].footprint = Footprint {
        width: 32,
        height: 32,
    };
    rules.units[0].placement = rules.units[0].footprint;
    rules.units[2].footprint = Footprint {
        width: 8,
        height: 8,
    };
    let mut w = World::new(rules, (*base.map).clone(), 1).unwrap();
    let balance = w.resource_balance(PlayerId(0), "minerals");
    let point = AbilityTarget::Point(Position { x: 384, y: 384 });
    cast(&mut w, point);
    let exit = w.state.entities[0].linked_to.unwrap();
    assert_eq!(
        w.cast_rejection(EntityId(1), AbilityId(1), point),
        Some(Rejection::QueueFull)
    );
    assert_eq!(w.resource_balance(PlayerId(0), "minerals"), balance);
    assert!(
        w.receive_ability_rejection(EntityId(3), EntityId(1), AbilityId(1))
            .is_some()
    );
    let mut resumed = w.restore_snapshot(w.save_snapshot().unwrap()).unwrap();
    ticks(&mut w, 4);
    ticks(&mut resumed, 4);
    assert_eq!(w.state_hash(), resumed.state_hash());
    let blockers: Vec<_> = rts::perimeter(
        Position { x: 384, y: 384 },
        w.rules.units[0].footprint,
        w.rules.units[2].footprint,
        Position { x: 64, y: 64 },
    )
    .into_iter()
    .filter_map(|p| w.spawn_offspring(PlayerId(0), UnitTypeId(3), p, None))
    .collect();
    assert_eq!(
        issue(
            &mut w,
            0,
            Order::ReceiveAbility {
                entity: EntityId(3),
                provider: EntityId(1),
                ability: AbilityId(1)
            }
        ),
        None
    );
    ticks(&mut w, 30);
    assert!(
        w.state.entities[w.index(EntityId(3)).unwrap()].position.x < 200,
        "an occupied exit must retain the passenger at the entrance"
    );
    for id in blockers {
        let i = w.index(id).unwrap();
        w.state.entities[i].hp = 0;
    }
    ticks(&mut w, 2);
    assert!(w.state.entities[w.index(EntityId(3)).unwrap()].position.x > 300);
    assert_eq!(
        issue(
            &mut w,
            0,
            Order::ReceiveAbility {
                entity: EntityId(3),
                provider: exit,
                ability: AbilityId(1)
            }
        ),
        None
    );
    ticks(&mut w, 30);
    assert!(
        w.state.entities[w.index(EntityId(3)).unwrap()].position.x < 160,
        "either endpoint accepts passengers"
    );
    w.state.entities[0].hp = 0;
    ticks(&mut w, 1);
    assert!(w.index(exit).is_none());
}

fn fighter_world(expendable: Option<u32>) -> World {
    let base = world(AbilityEffect::Parasite);
    let mut rules = (*base.rules).clone();
    let producer = &mut rules.units[3];
    producer.weapon = Some(Weapon {
        friendly_splash: false,
        projectile_speed: 0,
        damage: 0,
        range: 256,
        cooldown: 8,
        targets_air: true,
        target_classes: Vec::new(),
        cooldown_jitter: None,
        damage_kind: DamageKind::Normal,
        splash: None,
        strikes: vec![],
    });
    producer.stored_weapon = Some(StoredWeapon::Fighters {
        unit: UnitTypeId(5),
        launch_ticks: 8,
        leash: 256,
        repair: 5 * 256,
        expendable,
    });
    producer.speed = 0;
    producer.footprint = Footprint {
        width: 32,
        height: 32,
    };
    producer.placement = producer.footprint;
    let fighter = &mut rules.units[4];
    fighter.speed = 8;
    fighter.max_shields = 40;
    fighter.footprint = Footprint {
        width: 8,
        height: 8,
    };
    fighter.movement_class = if expendable.is_some() {
        MovementClass::Ground
    } else {
        MovementClass::Air
    };
    fighter.weapon = Some(Weapon {
        friendly_splash: false,
        projectile_speed: 0,
        damage: 10,
        range: 32,
        cooldown: 2,
        targets_air: true,
        target_classes: Vec::new(),
        cooldown_jitter: None,
        damage_kind: DamageKind::Normal,
        splash: None,
        strikes: vec![],
    });
    World::new(rules, (*base.map).clone(), 1).unwrap()
}

#[test]
fn expendable_fighters_travel_to_the_victim_hit_once_and_free_the_storage_slot() {
    let mut w = fighter_world(Some(90));
    let train = Order::Train {
        entity: EntityId(4),
        unit_type: UnitTypeId(5),
    };
    assert_eq!(issue(&mut w, 0, train.clone()), None);
    ticks(&mut w, 2);
    assert_eq!(
        w.state.entities[1].shields,
        100 * 256,
        "the producer must not inflict instant damage"
    );
    assert!(
        w.state
            .entities
            .iter()
            .any(|e| e.parent == Some(EntityId(4)) && e.garrisoned_in.is_none())
    );
    let mut refill = w.clone();
    assert_eq!(issue(&mut refill, 0, train.clone()), None);
    ticks(&mut w, 40);
    assert_eq!(w.state.entities[1].shields, 90 * 256);
    assert!(
        !w.state
            .entities
            .iter()
            .any(|e| e.parent == Some(EntityId(4)))
    );
    assert_eq!(issue(&mut w, 0, train), None);
}

#[test]
fn initial_hangar_stock_is_stored_without_payment_or_using_a_production_exit() {
    let base = fighter_world(Some(90));
    let mut map = (*base.map).clone();
    map.spawns[3].stored_units = 1;
    let w = World::new((*base.rules).clone(), map, 1).unwrap();
    let child = w
        .state
        .entities
        .iter()
        .find(|e| e.parent == Some(EntityId(4)))
        .unwrap();
    assert_eq!(child.garrisoned_in, Some(EntityId(4)));
    assert_eq!(w.supply(PlayerId(0)).0, 2);
    assert_eq!(w.resource_balance(PlayerId(0), "minerals"), 1000);
    w.restore_snapshot(w.save_snapshot().unwrap()).unwrap();
}

#[test]
fn active_ai_pays_for_and_replenishes_its_launched_fighters() {
    let base = fighter_world(Some(90));
    let mut map = (*base.map).clone();
    map.ai = vec![AiController {
        research: Vec::new(),
        abilities: Vec::new(),
        harvest_weights: Vec::new(),
        player: PlayerId(0),
        home: Position { x: 64, y: 160 },
        radius: 512,
        active: true,
        program: vec![AiInstruction::Wait(1000)],
    }];
    let mut w = World::new((*base.rules).clone(), map, 1).unwrap();
    ticks(&mut w, 60);
    assert!(w.resource_balance(PlayerId(0), "minerals") < 1000);
    assert!(w.state.entities[1].shields < 100 * 256);
    assert!(w.state.ai[0].accepted_orders > 0);
}

#[test]
fn fighters_launch_deal_damage_and_return_to_their_parent() {
    let mut w = fighter_world(None);
    assert_eq!(
        issue(
            &mut w,
            0,
            Order::Train {
                entity: EntityId(4),
                unit_type: UnitTypeId(5)
            }
        ),
        None
    );
    ticks(&mut w, 60);
    assert!(w.state.entities[1].shields < 100 * 256);
    let fighter = w
        .state
        .entities
        .iter()
        .find(|e| e.parent == Some(EntityId(4)))
        .unwrap();
    assert!(fighter.garrisoned_in.is_none());
    assert_ne!(fighter.position, w.state.entities[3].position);
    w.state.entities[1].hp = 0;
    ticks(&mut w, 60);
    let fighter = w
        .state
        .entities
        .iter()
        .find(|e| e.parent == Some(EntityId(4)))
        .unwrap();
    assert_eq!(fighter.garrisoned_in, Some(EntityId(4)));
    assert_eq!(fighter.shields, 40 * 256);
    w.restore_snapshot(w.save_snapshot().unwrap()).unwrap();
}

#[test]
fn concealment_fields_exclude_provider_and_end_when_provider_is_disabled() {
    let base = world(AbilityEffect::Disable {
        radius: 0,
        duration: 20,
        invulnerable: false,
        affected: vec![UnitTypeId(4)],
    });
    let mut rules = (*base.rules).clone();
    rules.units[3].concealment_field = Some(ConcealmentField {
        radius: 128,
        affected: vec![UnitTypeId(3)],
    });
    let mut w = World::new(rules, (*base.map).clone(), 1).unwrap();
    assert!(w.concealed(&w.state.entities[2]));
    assert!(!w.concealed(&w.state.entities[3]));
    cast(&mut w, AbilityTarget::Unit(EntityId(4)));
    assert!(!w.concealed(&w.state.entities[2]));
    ticks(&mut w, 20);
    assert!(w.concealed(&w.state.entities[2]));
    w.state.entities[3].hp = 0;
    assert!(!w.concealed(&w.state.entities[2]));
}

#[test]
fn stored_missile_has_one_slot_costs_supply_and_can_be_launched_or_interrupted() {
    let mut w = world(AbilityEffect::Strike {
        damage: 500,
        kind: DamageKind::Normal,
        radii: Some([16, 32, 64]),
        delay: 4,
        channel: 2,
        ammunition: Some(UnitTypeId(5)),
        max_health_fraction: Some([2, 3]),
        delivery: None,
    });
    let target = AbilityTarget::Point(Position { x: 160, y: 64 });
    assert_eq!(
        w.cast_rejection(EntityId(1), AbilityId(1), target),
        Some(Rejection::InsufficientResources)
    );
    let train = Order::Train {
        entity: EntityId(4),
        unit_type: UnitTypeId(5),
    };
    assert_eq!(issue(&mut w, 0, train.clone()), None);
    ticks(&mut w, 1);
    assert_eq!(w.supply(PlayerId(0)).0, 2);
    let ammo = w.state.entities.last().unwrap();
    assert_eq!(ammo.garrisoned_in, Some(EntityId(4)));
    assert_eq!(issue(&mut w, 0, train.clone()), Some(Rejection::QueueFull));
    cast(&mut w, target);
    assert_eq!(w.supply(PlayerId(0)).0, 0);
    assert_eq!(
        issue(
            &mut w,
            0,
            Order::Stop {
                entity: EntityId(1)
            }
        ),
        None
    );
    ticks(&mut w, 5);
    assert_eq!(
        w.state.entities[1].hp, 1000,
        "interrupted designation must not strike"
    );
    assert_eq!(issue(&mut w, 0, train), None);
    ticks(&mut w, 1);
    cast(&mut w, target);
    ticks(&mut w, 1);
    let mut resumed = w.restore_snapshot(w.save_snapshot().unwrap()).unwrap();
    ticks(&mut w, 4);
    ticks(&mut resumed, 4);
    assert_eq!(w.state_hash(), resumed.state_hash());
    assert_eq!(w.state.entities[1].shields, 0);
    assert_eq!(
        w.state.entities[1].hp, 367,
        "two thirds of max combined health minus shields"
    );
}

#[test]
fn disable_expires_and_stasis_rejects_damage_and_other_spells() {
    let mut w = world(AbilityEffect::Disable {
        radius: 0,
        duration: 5,
        invulnerable: true,
        affected: vec![UnitTypeId(2)],
    });
    cast(&mut w, AbilityTarget::Unit(EntityId(2)));
    assert!(w.disabled(&w.state.entities[1]));
    let before = w.state.entities[1].position;
    assert_eq!(
        issue(
            &mut w,
            1,
            Order::Move {
                entity: EntityId(2),
                target: Position { x: 300, y: 64 }
            }
        ),
        None
    );
    assert_eq!(w.state.entities[1].position, before);
    let mut damage = rts::Damage::default();
    let weapon = Weapon {
        friendly_splash: false,
        projectile_speed: 0,
        damage: 500,
        range: 100,
        cooldown: 10,
        cooldown_jitter: None,
        targets_air: true,
        target_classes: Vec::new(),
        damage_kind: DamageKind::Normal,
        splash: None,
        strikes: vec![],
    };
    w.record_hit(
        &mut damage,
        (EntityId(1), UnitTypeId(1)),
        &w.state.entities[1],
        &weapon,
        1,
    );
    assert!(damage.hits.is_empty());
    assert_eq!(
        w.cast_rejection(EntityId(1), AbilityId(1), AbilityTarget::Unit(EntityId(2))),
        Some(Rejection::InvalidTarget)
    );
    ticks(&mut w, 6);
    assert!(w.state.entities[1].position.x > before.x);
}

#[test]
fn barriers_absorb_damage_and_are_removed_when_spent() {
    let mut w = world(AbilityEffect::Barrier {
        duration: 40,
        amount: 20,
    });
    cast(&mut w, AbilityTarget::Unit(EntityId(3)));
    let weak = Weapon {
        friendly_splash: false,
        projectile_speed: 0,
        damage: 5,
        range: 16,
        cooldown: 10,
        cooldown_jitter: None,
        targets_air: true,
        target_classes: Vec::new(),
        damage_kind: DamageKind::Normal,
        splash: None,
        strikes: vec![],
    };
    let mut absorbed = rts::Damage::default();
    w.record_hit(
        &mut absorbed,
        (EntityId(2), UnitTypeId(2)),
        &w.state.entities[2],
        &weak,
        1,
    );
    assert_eq!(
        absorbed.shields[&EntityId(3)],
        0,
        "a barrier must not leak minimum shield damage"
    );
    let mut damage = rts::Damage::default();
    let weapon = Weapon {
        friendly_splash: false,
        projectile_speed: 0,
        damage: 30,
        range: 16,
        cooldown: 10,
        cooldown_jitter: None,
        targets_air: true,
        target_classes: Vec::new(),
        damage_kind: DamageKind::Normal,
        splash: None,
        strikes: vec![],
    };
    w.record_hit(
        &mut damage,
        (EntityId(2), UnitTypeId(2)),
        &w.state.entities[2],
        &weapon,
        1,
    );
    assert_eq!(damage.barriers[&(EntityId(3), AbilityId(1))], 20 * 256);
    assert_eq!(damage.shields[&EntityId(3)], 10 * 256);
}

#[test]
fn plague_follows_targets_bypasses_shields_and_never_kills() {
    let mut w = world(AbilityEffect::AreaDamage {
        radius: 64,
        damage_fp8: 300 * 256,
        period: 1,
        duration: 8,
        lethal: false,
        shields: false,
    });
    cast(&mut w, AbilityTarget::Point(Position { x: 160, y: 64 }));
    assert_eq!(
        issue(
            &mut w,
            1,
            Order::Move {
                entity: EntityId(2),
                target: Position { x: 400, y: 64 }
            }
        ),
        None
    );
    ticks(&mut w, 10);
    assert_eq!(w.state.entities[1].hp, 1);
    assert_eq!(w.state.entities[1].shields, 100 * 256);
}

#[test]
fn storm_is_an_area_and_only_visible_fields_cross_the_disclosure_boundary() {
    let mut w = world(AbilityEffect::AreaDamage {
        radius: 64,
        damage_fp8: 14 * 256,
        period: 1,
        duration: 4,
        lethal: true,
        shields: true,
    });
    cast(&mut w, AbilityTarget::Point(Position { x: 160, y: 64 }));
    ticks(&mut w, 4);
    assert_eq!(w.state.entities[1].shields, 44 * 256);
    assert!(w.state.ability_fields.is_empty());
    let mut map = w.map().clone();
    map.fog_of_war = true;
    let mut w = World::new(w.rules().clone(), map, 1).unwrap();
    cast(&mut w, AbilityTarget::Point(Position { x: 400, y: 400 }));
    assert!(
        w.player_view(PlayerId(0))
            .unwrap()
            .ability_fields
            .is_empty()
    );
    assert_eq!(w.state.ability_fields.len(), 1);
}

#[test]
fn battery_service_routes_each_recipient_and_spends_provider_energy() {
    let mut w = world(AbilityEffect::Recharge {
        rate: 1280,
        shield_per_energy: 2,
    });
    w.state.entities[2].shields = 0;
    let rules = Arc::make_mut(&mut w.rules);
    rules.units[0].abilities[0].range = 32;
    assert_eq!(
        issue(
            &mut w,
            0,
            Order::ReceiveAbility {
                entity: EntityId(3),
                provider: EntityId(1),
                ability: AbilityId(1)
            }
        ),
        None
    );
    ticks(&mut w, 40);
    assert_eq!(w.state.entities[2].shields, 100 * 256);
    assert_eq!(w.state.entities[0].energy, 150 * 256);
    assert_eq!(w.state.entities[2].order, UnitOrder::Idle);
}

#[test]
fn consume_broodlings_illusions_infestation_and_merge_change_actual_units() {
    let mut w = world(AbilityEffect::Consume {
        affected: vec![UnitTypeId(3)],
        energy: 50,
    });
    w.state.entities[0].energy = 100 * 256;
    cast(&mut w, AbilityTarget::Unit(EntityId(3)));
    assert!(w.index(EntityId(3)).is_none());
    assert_eq!(w.state.entities[0].energy, 150 * 256);
    let mut w = world(AbilityEffect::KillSpawn {
        affected: vec![UnitTypeId(2)],
        unit: UnitTypeId(3),
        count: 2,
        lifetime: 4,
    });
    cast(&mut w, AbilityTarget::Unit(EntityId(2)));
    assert!(w.index(EntityId(2)).is_none());
    assert_eq!(
        w.state
            .entities
            .iter()
            .filter(|e| e.lifetime_remaining.is_some())
            .count(),
        2
    );
    ticks(&mut w, 5);
    assert_eq!(
        w.state
            .entities
            .iter()
            .filter(|e| e.unit_type == UnitTypeId(3))
            .count(),
        1
    );
    let mut w = world(AbilityEffect::Illusions {
        count: 2,
        lifetime: 4,
    });
    cast(&mut w, AbilityTarget::Unit(EntityId(3)));
    assert_eq!(
        w.state
            .entities
            .iter()
            .filter(|e| e.illusion_remaining.is_some())
            .count(),
        2
    );
    let mut w = world(AbilityEffect::Infest {
        from: vec![UnitTypeId(2)],
        to: UnitTypeId(6),
        max_hp_percent: 50,
    });
    w.state.entities[1].hp = 400;
    cast(&mut w, AbilityTarget::Unit(EntityId(2)));
    assert_eq!(
        (w.state.entities[1].unit_type, w.state.entities[1].owner),
        (UnitTypeId(6), PlayerId(0))
    );
    let mut w = world(AbilityEffect::Merge {
        partner: UnitTypeId(1),
        result: UnitTypeId(3),
        delay: 3,
    });
    let partner = w
        .spawn_offspring(PlayerId(0), UnitTypeId(1), Position { x: 80, y: 64 }, None)
        .unwrap();
    cast(&mut w, AbilityTarget::Unit(partner));
    ticks(&mut w, 4);
    assert_eq!(w.state.entities[0].unit_type, UnitTypeId(3));
    assert!(w.index(partner).is_none());
}

#[test]
fn upgrade_levels_require_previous_level_and_tech_and_accumulate_weapon_bonuses() {
    let w = world(AbilityEffect::Protection {
        radius: 64,
        duration: 8,
    });
    let mut rules = w.rules().clone();
    for (id, previous, prerequisites, cost) in [
        (1, None, vec![], 10),
        (201, Some(ResearchId(1)), vec![UnitTypeId(6)], 20),
    ] {
        rules.research.push(Research {
            available: true,
            id: ResearchId(id),
            facility: UnitTypeId(4),
            previous,
            prerequisites,
            cost: vec![ResourceAmount {
                kind: "minerals".into(),
                amount: cost,
            }],
            ticks: 2,
            effect: ResearchEffect::WeaponUpgrade {
                units: vec![UnitTypeId(2)],
                bonuses: vec![[2, 3]],
            },
        });
    }
    let mut w = World::new(rules, w.map().clone(), 1).unwrap();
    assert_eq!(
        w.research_rejection(PlayerId(0), EntityId(4), ResearchId(201)),
        Some(Rejection::MissingPrerequisite)
    );
    assert_eq!(
        issue(
            &mut w,
            0,
            Order::Research {
                entity: EntityId(4),
                research: ResearchId(1)
            }
        ),
        None
    );
    ticks(&mut w, 1);
    assert_eq!(
        w.research_rejection(PlayerId(0), EntityId(4), ResearchId(201)),
        Some(Rejection::MissingPrerequisite)
    );
    w.spawn_offspring(
        PlayerId(0),
        UnitTypeId(6),
        Position { x: 300, y: 300 },
        None,
    );
    assert_eq!(
        issue(
            &mut w,
            0,
            Order::Research {
                entity: EntityId(4),
                research: ResearchId(201)
            }
        ),
        None
    );
    ticks(&mut w, 1);
    assert_eq!(
        w.research_weapon_bonus(PlayerId(0), UnitTypeId(2), false),
        4
    );
    assert_eq!(w.research_weapon_bonus(PlayerId(0), UnitTypeId(2), true), 6);
    assert_eq!(
        w.restore_snapshot(w.save_snapshot().unwrap())
            .unwrap()
            .state_hash(),
        w.state_hash()
    );
}
