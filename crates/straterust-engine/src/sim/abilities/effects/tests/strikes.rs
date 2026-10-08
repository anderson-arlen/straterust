use super::*;

fn delivery(vertical: bool) -> StrikeDelivery {
    StrikeDelivery {
        charge_ticks: if vertical { 0 } else { 44 },
        ascent_ticks: if vertical { 90 } else { 0 },
        warning_ticks: if vertical { 45 } else { 0 },
        transit_ticks: if vertical { 250 } else { 0 },
        descent_height: if vertical { 320 } else { 0 },
        speed_fp8: 8533,
        acceleration_fp8: if vertical { 33 } else { 8533 },
        impact_ticks: 52,
        reveal_radius: if vertical { 96 } else { 0 },
    }
}

fn staged(vertical: bool) -> World {
    let mut w = world(AbilityEffect::Strike {
        damage: 250,
        kind: DamageKind::Normal,
        radii: vertical.then_some([16, 32, 64]),
        delay: 420,
        channel: if vertical { 330 } else { 49 },
        ammunition: vertical.then_some(UnitTypeId(5)),
        max_health_fraction: None,
        delivery: Some(delivery(vertical)),
    });
    if vertical {
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
        ticks(&mut w, 1);
    }
    w
}

fn phase(w: &World) -> StrikeStage {
    w.state.pending_effects[0].flight.as_ref().unwrap().stage
}

#[test]
fn remote_strike_approaches_casting_range_without_routing_around_the_target_wall() {
    let mut w = staged(true);
    let wall = &mut std::sync::Arc::make_mut(&mut w.rules).units[1];
    wall.structure = true;
    wall.speed = 0;
    wall.footprint = Footprint {
        width: 32,
        height: 448,
    };
    w.state.entities[1].position = Position { x: 352, y: 224 };
    let target = Position { x: 448, y: 64 };
    cast(&mut w, AbilityTarget::Point(target));
    for _ in 0..30 {
        if !w.state.pending_effects.is_empty() {
            break;
        }
        ticks(&mut w, 1);
    }
    assert!(
        !w.state.pending_effects.is_empty(),
        "cast from this side of the wall"
    );
    assert_eq!(w.state.entities[0].position, Position { x: 256, y: 64 });
    assert_eq!(phase(&w), StrikeStage::Ascent);
}

#[test]
fn remote_strike_approaches_an_occupied_target_before_launching() {
    let mut w = staged(true);
    let target = Position { x: 448, y: 64 };
    // A strike targets a location, including the center of an enemy building.
    // The caster only needs to reach casting range, never the occupied center.
    let victim = &mut std::sync::Arc::make_mut(&mut w.rules).units[1];
    victim.speed = 0;
    victim.structure = true;
    victim.footprint = Footprint {
        width: 96,
        height: 96,
    };
    w.state.entities[1].position = target;
    cast(&mut w, AbilityTarget::Point(target));
    assert!(w.state.pending_effects.is_empty());
    assert_eq!(
        w.supply(PlayerId(0)).0,
        2,
        "approaching does not consume the missile"
    );
    for _ in 0..100 {
        if !w.state.pending_effects.is_empty() {
            break;
        }
        ticks(&mut w, 1);
    }
    assert!(
        !w.state.pending_effects.is_empty(),
        "an occupied target must not prevent approach"
    );
    let actor = &w.state.entities[0];
    assert!(actor.position.x > 64);
    assert!(rts::distance(actor.position, target) <= i64::from(w.vision_range(actor)).pow(2));
    assert!(
        actor.target.is_none() && actor.path.is_empty(),
        "tracking stops movement"
    );
    assert_eq!(phase(&w), StrikeStage::Ascent);
    assert_eq!(actor.last_cast.as_ref().unwrap().position, target);
}

#[test]
fn charged_projectile_fires_after_charge_and_survives_caster_death() {
    let mut w = staged(false);
    cast(&mut w, AbilityTarget::Unit(EntityId(2)));
    ticks(&mut w, 43);
    assert_eq!(phase(&w), StrikeStage::Charge);
    assert!(
        w.player_view(PlayerId(0))
            .unwrap()
            .into_world(&w)
            .unwrap()
            .entity_casting(EntityId(1)),
        "filtered single-player clients retain casting state"
    );
    assert_eq!(w.state.entities[1].shields, 100 * 256);
    ticks(&mut w, 1);
    assert_eq!(phase(&w), StrikeStage::Flight);
    w.state.entities[0].hp = 0;
    ticks(&mut w, 3);
    assert_eq!(phase(&w), StrikeStage::Impact);
    let target = w
        .state
        .entities
        .iter()
        .find(|e| e.id == EntityId(2))
        .unwrap();
    assert_eq!(target.hp, 850);
    assert_eq!(target.shields, 0);
    ticks(&mut w, 60);
    assert!(w.state.pending_effects.is_empty());
    assert_eq!(
        w.state
            .entities
            .iter()
            .find(|e| e.id == EntityId(2))
            .unwrap()
            .hp,
        850,
        "impact damage occurs only once"
    );
}

#[test]
fn interrupting_charge_prevents_projectile_and_damage() {
    let mut w = staged(false);
    cast(&mut w, AbilityTarget::Unit(EntityId(2)));
    ticks(&mut w, 20);
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
    ticks(&mut w, 100);
    assert!(w.state.pending_effects.is_empty());
    assert_eq!(w.state.entities[1].hp, 1000);
    assert_eq!(w.state.entities[1].shields, 100 * 256);
}

#[test]
fn remote_strike_launches_from_storage_holds_target_and_descends_after_transit() {
    let mut w = staged(true);
    cast(&mut w, AbilityTarget::Point(Position { x: 160, y: 64 }));
    assert_eq!(phase(&w), StrikeStage::Ascent);
    let effect = &w.state.pending_effects[0];
    assert_eq!(effect.origin, Position { x: 64, y: 160 });
    assert_eq!(
        w.supply(PlayerId(0)).0,
        0,
        "launch frees the ammunition slot immediately"
    );
    for _ in 0..90 {
        if phase(&w) == StrikeStage::Transit {
            break;
        }
        ticks(&mut w, 1);
    }
    assert_eq!(phase(&w), StrikeStage::Transit);
    assert!(w.entity_casting(EntityId(1)));
    let visible = w.strike_appearances(PlayerId(0));
    assert_eq!(visible[0].marker, Some(Position { x: 160, y: 64 }));
    assert!(visible[0].warning);
    assert!(visible[0].position.is_none(), "transit hides the missile");
    let mut resumed = w.restore_snapshot(w.save_snapshot().unwrap()).unwrap();
    for _ in 0..250 {
        ticks(&mut w, 1);
        ticks(&mut resumed, 1);
        assert_eq!(w.state_hash(), resumed.state_hash());
    }
    assert_eq!(phase(&w), StrikeStage::Flight);
    assert!(!w.entity_casting(EntityId(1)));
    assert!(w.strike_appearances(PlayerId(0))[0].marker.is_none());
    w.state.entities[0].hp = 0;
    for _ in 0..100 {
        if phase(&w) == StrikeStage::Impact {
            break;
        }
        ticks(&mut w, 1);
    }
    assert_eq!(phase(&w), StrikeStage::Impact);
    assert_eq!(
        w.state
            .entities
            .iter()
            .find(|e| e.id == EntityId(2))
            .unwrap()
            .hp,
        850
    );
}

#[test]
fn remote_strike_is_cancelled_before_descent_without_refunding_ammunition() {
    let mut w = staged(true);
    cast(&mut w, AbilityTarget::Point(Position { x: 160, y: 64 }));
    for _ in 0..90 {
        if phase(&w) == StrikeStage::Transit {
            break;
        }
        ticks(&mut w, 1);
    }
    assert_eq!(
        issue(
            &mut w,
            0,
            Order::Move {
                entity: EntityId(1),
                target: Position { x: 32, y: 32 }
            }
        ),
        None
    );
    ticks(&mut w, 400);
    assert!(w.state.pending_effects.is_empty());
    assert_eq!(w.supply(PlayerId(0)).0, 0);
    assert_eq!(w.state.entities[1].hp, 1000);
}

#[test]
fn global_warning_discloses_no_hidden_launcher_target_or_projectile() {
    let base = staged(true);
    let mut rules = base.rules().clone();
    rules.units[1].vision_range = 16;
    let mut map = base.map().clone();
    map.fog_of_war = true;
    map.spawns[1].position = Position { x: 480, y: 480 };
    let mut w = World::new(rules, map, 1).unwrap();
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
    ticks(&mut w, 1);
    cast(&mut w, AbilityTarget::Point(Position { x: 160, y: 64 }));
    for _ in 0..90 {
        if phase(&w) == StrikeStage::Transit {
            break;
        }
        ticks(&mut w, 1);
    }
    let enemy = w.player_view(PlayerId(1)).unwrap();
    assert_eq!(enemy.strikes.len(), 1);
    let strike = &enemy.strikes[0];
    assert!(strike.warning);
    assert_eq!(strike.position, None);
    assert_eq!(strike.marker, None);
    assert_eq!(strike.caster, None);
    assert_eq!(strike.stage, None);
    assert_eq!(strike.elapsed, 0);
    assert_eq!(strike.heading, [0, 0]);
    assert_eq!(strike.velocity_fp8, 0);
}
