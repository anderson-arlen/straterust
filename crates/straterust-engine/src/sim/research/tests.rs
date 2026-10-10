use super::*;
fn world() -> World {
    let soldier = UnitType {
        id: UnitTypeId(1),
        max_hp: 40,
        speed: 3,
        weapon: Some(Weapon {
            friendly_splash: false,
            projectile_speed: 0,
            cooldown_jitter: None,
            targets_air: false,
            target_classes: Vec::new(),
            damage: 6,
            range: 32,
            cooldown: 15,
            damage_kind: DamageKind::Normal,
            splash: None,
            strikes: Vec::new(),
        }),
        ..UnitType::default()
    };
    let lab = UnitType {
        id: UnitTypeId(2),
        max_hp: 100,
        structure: true,
        speed: 0,
        ..UnitType::default()
    };
    let mut research = Vec::new();
    for (id, effect) in [
        (
            1,
            ResearchEffect::WeaponDamage {
                units: vec![UnitTypeId(1)],
                amount: 1,
            },
        ),
        (
            2,
            ResearchEffect::Armor {
                units: vec![UnitTypeId(1)],
                amount: 1,
            },
        ),
        (
            3,
            ResearchEffect::WeaponRange {
                units: vec![UnitTypeId(1)],
                amount: 32,
                sight: 0,
            },
        ),
        (
            4,
            ResearchEffect::Stim {
                units: vec![UnitTypeId(1)],
                hp_cost: 10,
                duration_ticks: 8,
            },
        ),
    ] {
        research.push(Research {
            available: true,
            id: ResearchId(id),
            facility: UnitTypeId(2),
            previous: None,
            prerequisites: Vec::new(),
            cost: vec![ResourceAmount {
                kind: "minerals".into(),
                amount: 100,
            }],
            ticks: 3,
            effect,
        });
    }
    let rules = Rules {
        id: "synthetic.research".into(),
        units: vec![soldier, lab],
        starting_resources: vec![ResourceAmount {
            kind: "minerals".into(),
            amount: 1000,
        }],
        research,
        ..Rules::default()
    };
    let mut spawns = Vec::new();
    for (owner, unit, x) in [(0, 1, 24), (0, 2, 80), (1, 2, 256), (0, 2, 112)] {
        spawns.push(Spawn {
            stored_units: 0,
            linked_to: None,
            doodad_enabled: None,
            owner: PlayerId(owner),
            unit_type: UnitTypeId(unit),
            position: Position { x, y: 32 },
            hp_percent: None,
            shield_percent: None,
            energy_percent: None,
            invincible: false,
            cloaked: false,
        });
    }
    World::new(
        rules,
        Map {
            id: "synthetic.research".into(),
            width: 512,
            height: 128,
            players: 2,
            spawns,
            start_locations: Vec::new(),
            resources: Vec::new(),
            initial_explored: Default::default(),
            creation: Default::default(),
            ai: Vec::new(),
            mission: None,
            terrain: None,
            fog_of_war: false,
        },
        1,
    )
    .unwrap()
}
fn order(world: &mut World, order: Order) -> Option<Rejection> {
    let sequence = world.state.last_sequences[0] + 1;
    world
        .step(&[Command {
            tick: world.tick(),
            player: PlayerId(0),
            sequence,
            order,
        }])
        .unwrap()
        .remove(0)
        .rejection
}
fn finish(world: &mut World, id: u16) {
    assert_eq!(
        order(
            world,
            Order::Research {
                entity: EntityId(2),
                research: ResearchId(id)
            }
        ),
        None
    );
    world.step(&[]).unwrap();
    world.step(&[]).unwrap();
    assert!(world.has_research(PlayerId(0), ResearchId(id)));
}
#[test]
fn research_checks_owner_facility_funds_and_duplicate_jobs_and_refunds_cancel() {
    let mut w = world();
    assert_eq!(
        w.research_rejection(PlayerId(0), EntityId(3), ResearchId(1)),
        Some(Rejection::NotOwner)
    );
    assert_eq!(
        w.research_rejection(PlayerId(0), EntityId(1), ResearchId(1)),
        Some(Rejection::UnsupportedOrder)
    );
    assert_eq!(
        order(
            &mut w,
            Order::Research {
                entity: EntityId(2),
                research: ResearchId(1)
            }
        ),
        None
    );
    assert_eq!(w.resource_balance(PlayerId(0), "minerals"), 900);
    assert_eq!(
        w.research_rejection(PlayerId(0), EntityId(4), ResearchId(1)),
        Some(Rejection::InvalidTarget)
    );
    assert_eq!(
        w.research_rejection(PlayerId(0), EntityId(2), ResearchId(2)),
        Some(Rejection::QueueFull)
    );
    assert_eq!(
        order(
            &mut w,
            Order::Cancel {
                entity: EntityId(2)
            }
        ),
        None
    );
    assert_eq!(w.resource_balance(PlayerId(0), "minerals"), 1000);
    assert!(!w.has_research(PlayerId(0), ResearchId(1)));
    w.state.players[0].resources.insert("minerals".into(), 99);
    assert_eq!(
        w.research_rejection(PlayerId(0), EntityId(2), ResearchId(1)),
        Some(Rejection::InsufficientResources)
    );
}
#[test]
fn completed_research_is_owner_scoped_persistent_and_not_repeatable() {
    let mut w = world();
    for id in 1..=3 {
        finish(&mut w, id);
    }
    assert_eq!(w.research_damage_bonus(PlayerId(0), UnitTypeId(1)), 1);
    assert_eq!(w.research_armor_bonus(PlayerId(0), UnitTypeId(1)), 1);
    assert_eq!(w.research_range_bonus(PlayerId(0), UnitTypeId(1)), 32);
    assert_eq!(w.research_damage_bonus(PlayerId(1), UnitTypeId(1)), 0);
    assert_eq!(
        w.research_rejection(PlayerId(0), EntityId(2), ResearchId(1)),
        Some(Rejection::InvalidTarget)
    );
    w.state.entities.retain(|e| e.id != EntityId(2));
    assert_eq!(w.research_damage_bonus(PlayerId(0), UnitTypeId(1)), 1);
}
#[test]
fn stim_cost_refresh_and_expiry_preserve_hp_and_prevent_self_kill() {
    let mut w = world();
    assert_eq!(
        w.stim_rejection(EntityId(1)),
        Some(Rejection::MissingPrerequisite)
    );
    finish(&mut w, 4);
    assert_eq!(
        order(
            &mut w,
            Order::Stim {
                entity: EntityId(1)
            }
        ),
        None
    );
    assert_eq!(w.state.entities[0].hp, 30);
    let first = w.state.entities[0].stim_remaining;
    assert!(first > 0 && first <= 8);
    w.step(&[]).unwrap();
    assert_eq!(
        order(
            &mut w,
            Order::Stim {
                entity: EntityId(1)
            }
        ),
        None
    );
    assert_eq!(w.state.entities[0].hp, 20);
    assert_eq!(w.state.entities[0].stim_remaining, first);
    assert_eq!(
        order(
            &mut w,
            Order::Stim {
                entity: EntityId(1)
            }
        ),
        None
    );
    assert_eq!(w.state.entities[0].hp, 10);
    assert_eq!(
        w.stim_rejection(EntityId(1)),
        Some(Rejection::InvalidTarget)
    );
    for _ in 0..8 {
        w.step(&[]).unwrap();
    }
    assert_eq!(w.state.entities[0].stim_remaining, 0);
    assert_eq!(w.state.entities[0].hp, 10);
}
#[test]
fn research_job_completion_and_boost_are_canonical_and_deterministic() {
    let mut a = world();
    let mut b = world();
    finish(&mut a, 4);
    finish(&mut b, 4);
    assert_eq!(
        order(
            &mut a,
            Order::Stim {
                entity: EntityId(1)
            }
        ),
        None
    );
    assert_eq!(
        order(
            &mut b,
            Order::Stim {
                entity: EntityId(1)
            }
        ),
        None
    );
    assert_eq!(a.state_hash(), b.state_hash());
    b.state.entities[0].stim_remaining -= 1;
    assert_ne!(a.state_hash(), b.state_hash());
    b.state.entities[0].stim_remaining += 1;
    b.state.players[0].completed_research.clear();
    assert_ne!(a.state_hash(), b.state_hash());
    let mut invalid = a.rules().clone();
    invalid.research.push(invalid.research[0].clone());
    assert!(validate_research_rules(&invalid).is_err());
    invalid = a.rules().clone();
    invalid.research[0].effect = ResearchEffect::WeaponDamage {
        units: vec![UnitTypeId(600)],
        amount: 1,
    };
    assert!(validate_research_rules(&invalid).is_err());
}

#[test]
fn range_research_can_extend_sight_without_changing_older_definitions() {
    let base = world();
    let mut rules = base.rules().clone();
    let ResearchEffect::WeaponRange { sight, .. } = &mut rules.research[2].effect else {
        unreachable!()
    };
    *sight = 32;
    let mut w = World::new(rules, base.map().clone(), 42).unwrap();
    let unit = &w.state.entities[0];
    let original = w.vision_range(unit);
    finish(&mut w, 3);
    assert_eq!(w.vision_range(&w.state.entities[0]), original + 32);
    let legacy: ResearchEffect = ron::from_str("WeaponRange(units:[1],amount:32)").unwrap();
    assert!(matches!(
        legacy,
        ResearchEffect::WeaponRange { sight: 0, .. }
    ));
    assert!(!ron::to_string(&legacy).unwrap().contains("sight"));
}
