use super::*;
use straterust_engine::{
    session::ServerSession,
    sim::{Command, Order, UnitType},
};

fn encounter() -> (World, ServerSession, World, Command) {
    let original = super::super::tests::world();
    let mut rules = original.rules().clone();
    rules.victory = false;
    rules.units[0].weapon.as_mut().unwrap().damage = 100;
    rules.units[0].weapon.as_mut().unwrap().range = 128;
    rules.units.push(UnitType {
        id: UnitTypeId(2),
        speed: 0,
        neutral: true,
        max_hp: 1,
        weapon: None,
        ..Default::default()
    });
    let mut map = original.map().clone();
    map.spawns[0].owner = PlayerId(1);
    map.spawns[1].owner = PlayerId(0);
    map.spawns[1].unit_type = UnitTypeId(2);
    let definitions = World::new(rules, map, 42).unwrap();
    let mut server =
        ServerSession::new(definitions.clone(), 42, vec![PlayerId(0), PlayerId(1)]).unwrap();
    let initial = server
        .update(PlayerId(1), &[])
        .unwrap()
        .view
        .into_world(&definitions)
        .unwrap();
    let command = Command {
        tick: initial.tick(),
        player: PlayerId(1),
        sequence: 1,
        order: Order::Attack {
            entity: EntityId(1),
            target: EntityId(2),
        },
    };
    (definitions, server, initial, command)
}

#[test]
fn confirmed_death_survives_a_predicted_removal_and_older_reconciliation_tick() {
    let (definitions, mut server, mut predicted, command) = encounter();
    let mut visuals = Visuals::new(&predicted);
    visuals.confirm(&predicted);
    predicted.predict_command(&command).unwrap();
    for _ in 0..4 {
        predicted.predict_step().unwrap();
        visuals.update(&predicted);
    }
    assert!(
        !predicted
            .state()
            .entities
            .iter()
            .any(|e| e.id == EntityId(2))
    );
    assert!(
        visuals.deaths().is_empty(),
        "speculation cannot confirm a death"
    );
    server.submit(PlayerId(1), command).unwrap();
    let outcomes = server.advance(&[]).unwrap();
    let confirmed = server
        .update(PlayerId(1), &outcomes)
        .unwrap()
        .view
        .into_world(&definitions)
        .unwrap();
    assert!(confirmed.tick().0 < visuals.tick);
    visuals.confirm(&confirmed);
    visuals.reconcile(&confirmed);
    assert_eq!(visuals.tick, 4);
    assert_eq!(visuals.deaths().len(), 1);
    assert_eq!(visuals.deaths()[0].unit_type, UnitTypeId(2));
    assert_eq!(visuals.deaths()[0].position, Position { x: 100, y: 40 });
    visuals.advance_effects(Duration::from_millis(200), None);
    server.advance(&[]).unwrap();
    let confirmed = server
        .update(PlayerId(1), &[])
        .unwrap()
        .view
        .into_world(&confirmed)
        .unwrap();
    visuals.confirm(&confirmed);
    visuals.reconcile(&confirmed);
    assert_eq!(visuals.deaths().len(), 1);
    assert_eq!(visuals.deaths()[0].elapsed, Duration::from_millis(200));
}

#[test]
fn corrected_same_tick_can_restore_a_speculative_victim_without_a_corpse_or_clock_reset() {
    let (_, _, initial, command) = encounter();
    let mut visuals = Visuals::new(&initial);
    visuals.confirm(&initial);
    let mut predicted = initial.clone();
    predicted.predict_command(&command).unwrap();
    predicted.predict_step().unwrap();
    visuals.update(&predicted);
    visuals.advance_animations(Duration::from_millis(100), 40);
    let phase = visuals.get(EntityId(1)).unwrap().local_phase;
    visuals.confirm(&initial);
    visuals.reconcile(&initial);
    assert!(visuals.get(EntityId(2)).is_some());
    assert!(visuals.deaths().is_empty());
    assert_eq!(visuals.get(EntityId(1)).unwrap().local_phase, phase);
}

#[test]
fn repeated_moving_reconciliations_preserve_walk_animation_and_heading() {
    let definitions = super::super::tests::world();
    let mut world = definitions
        .player_view(PlayerId(0))
        .unwrap()
        .into_world(&definitions)
        .unwrap();
    let mut visuals = Visuals::new(&world);
    world
        .predict_command(&Command {
            player: PlayerId(0),
            sequence: 1,
            tick: world.tick(),
            order: Order::Move {
                entity: EntityId(1),
                target: Position { x: 40, y: 112 },
            },
        })
        .unwrap();
    world.predict_step().unwrap();
    visuals.reconcile(&world);
    assert_eq!(visuals.get(EntityId(1)).unwrap().action, VisualAction::Move);
    let heading = visuals.get(EntityId(1)).unwrap().facing;
    for _ in 0..10 {
        visuals.advance_animations(Duration::from_millis(100), world.rules().tick_ms);
        visuals.reconcile(&world);
        let unit = visuals.get(EntityId(1)).unwrap();
        assert_eq!(unit.action, VisualAction::Move);
        assert_eq!(unit.facing, heading);
    }
    assert_eq!(visuals.get(EntityId(1)).unwrap().phase_ms(&world), 1000);
    world
        .predict_command(&Command {
            player: PlayerId(0),
            sequence: 2,
            tick: world.tick(),
            order: Order::Stop {
                entity: EntityId(1),
            },
        })
        .unwrap();
    visuals.reconcile(&world);
    assert_eq!(visuals.get(EntityId(1)).unwrap().action, VisualAction::Idle);
}

#[test]
fn stopped_rollback_does_not_turn_or_run_in_place_between_updates() {
    let definitions = super::super::tests::world();
    let initial = definitions
        .player_view(PlayerId(0))
        .unwrap()
        .into_world(&definitions)
        .unwrap();
    let mut predicted = initial.clone();
    let mut visuals = Visuals::new(&initial);
    predicted
        .predict_command(&Command {
            player: PlayerId(0),
            sequence: 1,
            tick: predicted.tick(),
            order: Order::Move {
                entity: EntityId(1),
                target: Position { x: 20, y: 40 },
            },
        })
        .unwrap();
    predicted.predict_step().unwrap();
    visuals.reconcile(&predicted);
    assert!(visuals.get(EntityId(1)).unwrap().moving);
    let heading = visuals.get(EntityId(1)).unwrap().facing;
    visuals.reconcile(&initial);
    assert!(initial.tick().0 < visuals.tick);
    for _ in 0..20 {
        visuals.advance_animations(Duration::from_millis(100), initial.rules().tick_ms);
        visuals.reconcile(&initial);
        let unit = visuals.get(EntityId(1)).unwrap();
        assert!(!unit.moving);
        assert_eq!(unit.action, VisualAction::Idle);
        assert_eq!(unit.facing, heading);
    }
    assert_eq!(visuals.get(EntityId(1)).unwrap().phase_ms(&initial), 2000);
}

#[test]
fn reaching_attack_range_ends_walking_and_keeps_the_attack_animation() {
    let original = super::super::tests::world();
    let mut rules = original.rules().clone();
    rules.victory = false;
    rules.units[0].weapon.as_mut().unwrap().cooldown = 100;
    rules.units.push(UnitType {
        id: UnitTypeId(2),
        max_hp: 10_000,
        structure: true,
        speed: 0,
        ..Default::default()
    });
    let mut map = original.map().clone();
    map.spawns[1].unit_type = UnitTypeId(2);
    let mut world = World::new(rules, map, 42).unwrap();
    let mut visuals = Visuals::new(&world);
    let command = Command {
        player: PlayerId(0),
        sequence: 1,
        tick: world.tick(),
        order: Order::Attack {
            entity: EntityId(1),
            target: EntityId(2),
        },
    };
    world.step(&[command]).unwrap();
    visuals.reconcile(&world);
    assert!(visuals.get(EntityId(1)).unwrap().moving);
    let mut firing = false;
    for _ in 0..30 {
        let previous = world.state().entities[0].position;
        world.step(&[]).unwrap();
        visuals.reconcile(&world);
        let actor = &world.state().entities[0];
        if actor.cooldown > 0 && actor.position == previous {
            assert!(actor.path.is_empty());
            let visual = visuals.get(actor.id).unwrap();
            assert!(!visual.moving);
            assert_eq!(visual.action, VisualAction::Attack);
            firing = true;
        }
    }
    assert!(firing);
}

#[test]
fn stationary_public_actor_stops_walking_without_disclosing_its_traffic_wait() {
    use straterust_engine::map::{Terrain, WALKABLE};
    use straterust_engine::sim::{Motion, Spawn};
    let original = super::super::tests::world();
    let mut rules = original.rules().clone();
    rules.victory = false;
    rules.units[0].weapon = None;
    rules.units[0].footprint = Footprint {
        width: 16,
        height: 16,
    };
    rules.units[0].motion = Some(Motion {
        eight_directions: true,
        speed: 4 * 256,
        acceleration: 0,
        steps: vec![],
    });
    rules.units.push(UnitType {
        id: UnitTypeId(2),
        speed: 1,
        max_hp: 100,
        footprint: Footprint {
            width: 24,
            height: 24,
        },
        ..Default::default()
    });
    let mut map = original.map().clone();
    map.width = 256;
    map.spawns = [(1, 40), (2, 80)]
        .map(|(unit, x)| Spawn {
            owner: PlayerId(1),
            unit_type: UnitTypeId(unit),
            position: Position { x, y: 48 },
            ..Default::default()
        })
        .to_vec();
    map.terrain = Some(Terrain {
        cell_size: 8,
        columns: 32,
        rows: 16,
        flags: (0..512)
            .map(|i| {
                if (4..8).contains(&(i / 32)) {
                    WALKABLE
                } else {
                    0
                }
            })
            .collect(),
    });
    let mut world = World::new(rules, map, 42).unwrap();
    let mut client = world
        .player_view(PlayerId(0))
        .unwrap()
        .into_world(&world)
        .unwrap();
    let mut visuals = Visuals::new(&client);
    let command = Command {
        tick: world.tick(),
        player: PlayerId(1),
        sequence: 1,
        order: Order::Move {
            entity: EntityId(1),
            target: Position { x: 224, y: 48 },
        },
    };
    world.step(&[command]).unwrap();
    let mut stationary = 0;
    let mut walked = false;
    for _ in 0..60 {
        let previous = world.state().entities[0].position;
        world.step(&[]).unwrap();
        client = world
            .player_view(PlayerId(0))
            .unwrap()
            .into_world(&client)
            .unwrap();
        visuals.reconcile(&client);
        walked |= visuals.get(EntityId(1)).unwrap().moving;
        let actor = &world.state().entities[0];
        if actor.position == previous && !actor.path.is_empty() && actor.route_wait.is_some() {
            assert!(
                client.state().entities[0].route_wait.is_none(),
                "private wait is not needed for animation"
            );
            assert!(!visuals.get(EntityId(1)).unwrap().moving);
            assert_eq!(visuals.get(EntityId(1)).unwrap().action, VisualAction::Idle);
            stationary += 1;
        }
    }
    assert!(walked);
    assert!(
        stationary > 10,
        "must exercise a persistent mobile blockage"
    );
}

#[test]
#[ignore = "requires STRATERUST_WARCRAFT2 pointing to the retail import"]
fn retail_seal_has_a_finite_death_and_a_held_native_corpse() {
    let root = std::path::PathBuf::from(std::env::var_os("STRATERUST_WARCRAFT2").unwrap());
    let assets = AssetPack::load(&root.join("human/mission02"))
        .unwrap()
        .unwrap();
    let sprite = assets.sprite(UnitTypeId(58)).unwrap();
    let clip = sprite.clip(ClipKind::Death).unwrap();
    assert_eq!(clip.loop_start, None);
    assert!(clip.frames.len() > 32);
    let duration =
        u128::from(clip.frame_ms) * (clip.frames.len() / usize::from(clip.directions)) as u128;
    let mut body = DeathVisual {
        owner: PlayerId(15),
        position: Position { x: 100, y: 100 },
        unit_type: UnitTypeId(58),
        facing: 0,
        elapsed: Duration::ZERO,
    };
    assert!(
        death_image(&assets, &body)
            .unwrap()
            .image
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .any(|p| p[3] > 0)
    );
    body.elapsed = Duration::from_millis((duration - 100) as u64);
    assert!(
        death_image(&assets, &body)
            .unwrap()
            .image
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .any(|p| p[3] > 0)
    );
    body.elapsed = Duration::from_millis(duration as u64);
    assert!(death_image(&assets, &body).is_none());
}
