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
