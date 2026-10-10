use super::*;

#[test]
fn travelling_effect_crosses_footprints_between_damage_samples_without_hitting_its_caster() {
    let mut w = world(AbilityEffect::GroundEffect {
        radius: 8,
        damage_fp8: 20 * 256,
        period: 2,
        duration: 20,
        drift: 0,
        travel_speed: 64,
        trigger_on_contact: false,
        repeat: false,
        offsets: vec![[0, 0]],
    });
    w.state.entities[1].position = Position { x: 160, y: 64 };
    let hp = w.state.entities[0].hp;
    cast(&mut w, AbilityTarget::Point(Position { x: 400, y: 64 }));
    ticks(&mut w, 5);
    assert_eq!(w.state.entities[0].hp, hp);
    assert_eq!(w.state.entities[1].hp, 980);
}

#[test]
fn ground_traps_wait_for_contact_and_moving_hazards_resume_identically() {
    let mut w = world(AbilityEffect::GroundEffect {
        radius: 12,
        damage_fp8: 50 * 256,
        period: 1,
        duration: 40,
        drift: 0,
        travel_speed: 0,
        trigger_on_contact: true,
        repeat: false,
        offsets: vec![[0, 0]],
    });
    cast(&mut w, AbilityTarget::Point(Position { x: 220, y: 64 }));
    ticks(&mut w, 4);
    assert_eq!(w.state.entities[1].hp, 1000);
    assert_eq!(w.state.ability_fields.len(), 1);
    assert_eq!(
        issue(
            &mut w,
            1,
            Order::Move {
                entity: EntityId(2),
                target: Position { x: 220, y: 64 }
            }
        ),
        None
    );
    ticks(&mut w, 20);
    assert_eq!(w.state.entities[1].hp, 950);
    assert!(w.state.ability_fields.is_empty());

    let mut w = world(AbilityEffect::GroundEffect {
        radius: 12,
        damage_fp8: 256,
        period: 3,
        duration: 80,
        drift: 2,
        travel_speed: 0,
        trigger_on_contact: false,
        repeat: false,
        offsets: vec![[0, 0]],
    });
    cast(&mut w, AbilityTarget::Point(Position { x: 200, y: 200 }));
    ticks(&mut w, 10);
    let mut restored = w.restore_snapshot(w.save_snapshot().unwrap()).unwrap();
    ticks(&mut w, 40);
    ticks(&mut restored, 40);
    assert_eq!(w.state_hash(), restored.state_hash());
}

#[test]
fn draining_projectile_heals_only_on_impact_and_restores_midflight() {
    let mut w = world(AbilityEffect::DrainLife {
        affected: vec![UnitTypeId(2)],
        damage: 50,
        healing: 50,
        delivery: Some(StrikeDelivery {
            charge_ticks: 1,
            ascent_ticks: 0,
            warning_ticks: 0,
            transit_ticks: 0,
            descent_height: 0,
            speed_fp8: 8 * 256,
            acceleration_fp8: 8 * 256,
            impact_ticks: 5,
            reveal_radius: 0,
        }),
    });
    w.state.entities[0].hp = 80;
    cast(&mut w, AbilityTarget::Unit(EntityId(2)));
    assert_eq!(w.state.entities[1].hp, 1000);
    assert_eq!(w.state.entities[0].hp, 80);
    ticks(&mut w, 4);
    let mut restored = w.restore_snapshot(w.save_snapshot().unwrap()).unwrap();
    ticks(&mut w, 30);
    ticks(&mut restored, 30);
    assert_eq!(w.state_hash(), restored.state_hash());
    assert_eq!(w.state.entities[1].hp, 950);
    assert_eq!(w.state.entities[0].hp, 130);
    assert!(w.state.pending_effects.is_empty());
}

#[test]
fn healing_pays_for_each_health_point_without_restarting_feedback_each_tick() {
    let base = world(AbilityEffect::Heal {
        affected: vec![UnitTypeId(3)],
        amount: 1,
    });
    let mut rules = base.rules().clone();
    rules.units[0].abilities[0].energy = 6;
    let mut w = World::new(rules, base.map().clone(), 1).unwrap();
    w.state.entities[2].hp = 90;
    w.state.entities[0].energy = 30 * 256;
    cast(&mut w, AbilityTarget::Unit(EntityId(3)));
    let first = w.state.entities[0].last_cast.clone();
    ticks(&mut w, 3);
    assert_eq!(w.state.entities[0].last_cast, first);
    ticks(&mut w, 10);
    assert_eq!(w.state.entities[2].hp, 95);
    assert_eq!(w.state.entities[0].energy, 0);
    assert_eq!(w.state.entities[0].order, UnitOrder::Idle);
}

#[test]
fn research_promotes_existing_and_future_units_without_losing_damage_or_orders() {
    let base = world(AbilityEffect::Reveal {
        radius: 32,
        duration: 4,
    });
    let mut rules = base.rules().clone();
    rules.units[2].max_hp = 1100;
    rules.units[3].trains = vec![UnitTypeId(2)];
    rules.research = vec![Research {
        available: true,
        id: ResearchId(1),
        facility: UnitTypeId(4),
        previous: None,
        prerequisites: vec![],
        cost: vec![],
        ticks: 2,
        effect: ResearchEffect::UnitUpgrade {
            units: vec![UnitTypeId(2)],
            to: UnitTypeId(3),
        },
    }];
    let mut map = base.map().clone();
    map.spawns[1].owner = PlayerId(0);
    let mut w = World::new(rules, map, 1).unwrap();
    w.state.entities[1].hp = 900;
    let position = w.state.entities[1].position;
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
    ticks(&mut w, 3);
    assert_eq!(w.state.entities[1].unit_type, UnitTypeId(3));
    assert_eq!(w.state.entities[1].hp, 1000);
    assert_eq!(w.state.entities[1].position, position);
    assert!(w.can_train_type(&w.state.entities[3], UnitTypeId(3)));
    assert_eq!(
        issue(
            &mut w,
            0,
            Order::Train {
                entity: EntityId(4),
                unit_type: UnitTypeId(3)
            }
        ),
        None
    );
    ticks(&mut w, 10);
    assert_eq!(
        w.state
            .entities
            .iter()
            .filter(|e| e.unit_type == UnitTypeId(3))
            .count(),
        3
    );
}
