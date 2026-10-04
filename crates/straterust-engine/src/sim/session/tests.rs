use super::*;
use crate::session::*;
use crate::{
    content::Package,
    sim::{EntityId, Order, Position, ProductionJob, UnitOrder, UnitTypeId},
};

fn world() -> World {
    Package::load(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/terran-demo"),
    )
    .unwrap()
    .world(42)
    .unwrap()
}

#[test]
fn player_wire_views_omit_hidden_state_and_cannot_simulate() {
    let initial = world();
    let mut rules = initial.rules().clone();
    rules.units.iter_mut().for_each(|u| u.vision_range = 96);
    let mut map = initial.map().clone();
    map.fog_of_war = true;
    let mut world = World::new(rules, map, 42).unwrap();
    let enemy = world
        .state
        .entities
        .iter()
        .find(|e| e.owner == PlayerId(1))
        .unwrap()
        .id;
    let owned = world
        .state
        .entities
        .iter()
        .find(|e| e.owner == PlayerId(0))
        .unwrap()
        .id;
    let enemy_index = world
        .state
        .entities
        .iter()
        .position(|e| e.id == enemy)
        .unwrap();
    world.state.entities[enemy_index]
        .production
        .push_back(ProductionJob {
            unit_type: UnitTypeId(1),
            remaining: 40,
            total: 80,
            started: true,
        });
    world.state.entities[enemy_index]
        .queued_orders
        .push_back(UnitOrder::Hold);
    let hidden = world.player_view(PlayerId(0)).unwrap();
    assert!(
        !hidden
            .entities
            .iter()
            .any(|e| matches!(e, ViewedEntity::Visible(e) if e.id == enemy))
    );
    // Bring an observer close enough to disclose the enemy's appearance.
    world
        .state
        .entities
        .iter_mut()
        .find(|e| e.id == owned)
        .unwrap()
        .position = world.state.entities[enemy_index].position;
    world.update_vision();
    let public = world.player_view(PlayerId(0)).unwrap();
    let enemy_view = public
        .entities
        .iter()
        .find(|e| matches!(e, ViewedEntity::Visible(e) if e.id == enemy))
        .unwrap();
    let wire = ron::ser::to_string(enemy_view).unwrap();
    for secret in [
        "production",
        "research",
        "queued_orders",
        "path",
        "cargo",
        "Hold",
        "rng_state",
        "last_sequences",
    ] {
        assert!(!wire.contains(secret), "leaked {secret}: {wire}");
    }
    assert!(wire.contains("working:true"));
    let roundtrip: PlayerView = ron::from_str(&ron::ser::to_string(&public).unwrap()).unwrap();
    let mut client = roundtrip.into_world(&world).unwrap();
    assert!(client.is_player_view());
    assert!(client.map().spawns.is_empty() && client.map().initial_explored.is_empty());
    assert!(client.state().ai.is_empty() && client.state().last_sequences.is_empty());
    assert!(client.state().players[1].resources.is_empty());
    assert!(client.entity_working(enemy));
    assert!(client.step(&[]).is_err());
    assert!(client.save_snapshot().is_err());
}

#[test]
fn canonical_replay_keeps_rejected_envelopes_and_snapshot_continuation() {
    let mut session = ServerSession::new(world(), 42, vec![PlayerId(0), PlayerId(1)]).unwrap();
    let command = Command {
        tick: Tick(0),
        player: PlayerId(0),
        sequence: 7,
        order: Order::Move {
            entity: EntityId(999999),
            target: Position { x: 64, y: 64 },
        },
    };
    session.submit(PlayerId(0), command.clone()).unwrap();
    let future = Command {
        tick: Tick(10),
        player: PlayerId(0),
        sequence: 8,
        order: Order::Stop {
            entity: EntityId(1),
        },
    };
    session.submit(PlayerId(0), future).unwrap();
    let outcomes = session.advance(&[]).unwrap();
    assert!(outcomes[0].rejection.is_some());
    let snapshot = session.world().save_snapshot().unwrap();
    let encoded = ron::ser::to_string(&snapshot).unwrap();
    let checkpoint = ron::ser::to_string(&session.save_snapshot().unwrap()).unwrap();
    let mut continued =
        ServerSession::restore(&world(), ron::from_str(&checkpoint).unwrap()).unwrap();
    let mut resumed = world()
        .restore_snapshot(ron::from_str(&encoded).unwrap())
        .unwrap();
    for _ in 0..20 {
        session.advance(&[]).unwrap();
        continued.advance(&[]).unwrap();
        let batch = &session.replay().ticks.last().unwrap().commands;
        resumed.step(batch).unwrap();
    }
    assert_eq!(session.world().state_hash(), resumed.state_hash());
    assert_eq!(session.world().state_hash(), continued.world().state_hash());
    let replay: Replay = ron::from_str(&ron::ser::to_string(session.replay()).unwrap()).unwrap();
    assert_eq!(replay.ticks[0].commands, vec![command]);
    assert_eq!(
        replay.play(&world()).unwrap().state_hash(),
        session.world().state_hash()
    );
    let mut corrupted = snapshot.clone();
    corrupted.state.players.clear();
    assert!(world().restore_snapshot(corrupted).is_err());
    let mut wrong_version = snapshot;
    wrong_version.version += 1;
    assert!(world().restore_snapshot(wrong_version).is_err());
}

#[test]
fn assignments_identity_and_command_bounds_are_enforced() {
    let mut session = ServerSession::new(world(), 42, vec![PlayerId(0), PlayerId(1)]).unwrap();
    let handshake = session.handshake(PlayerId(1)).unwrap();
    handshake.validate(&world()).unwrap();
    let mut altered = handshake.clone();
    altered.identity.map = "different".into();
    assert!(altered.validate(&world()).is_err());
    let command = Command {
        tick: Tick(0),
        player: PlayerId(0),
        sequence: 1,
        order: Order::Stop {
            entity: EntityId(1),
        },
    };
    assert!(session.submit(PlayerId(1), command.clone()).is_err());
    let mut future = command.clone();
    future.tick = Tick(MAX_INPUT_AHEAD + 1);
    assert!(session.submit(PlayerId(0), future).is_err());
    session.submit(PlayerId(0), command.clone()).unwrap();
    session.submit(PlayerId(0), command).unwrap();
    let outcomes = session.advance(&[]).unwrap();
    assert!(
        outcomes
            .iter()
            .all(|o| o.rejection == Some(crate::sim::Rejection::DuplicateSequence))
    );
    assert!(
        session
            .update(PlayerId(1), &outcomes)
            .unwrap()
            .outcomes
            .is_empty()
    );
    assert_eq!(
        session
            .update(PlayerId(0), &outcomes)
            .unwrap()
            .outcomes
            .len(),
        2
    );
}

#[test]
fn match_results_resolve_campaign_defeat_without_a_winner_for_both_participants() {
    let mut initial = world();
    let mut server =
        ServerSession::new(initial.snapshot(), 42, vec![PlayerId(0), PlayerId(1)]).unwrap();
    for player in [PlayerId(0), PlayerId(1)] {
        assert!(server.update(player, &[]).unwrap().result.is_none());
    }
    initial.state.defeated = vec![PlayerId(0)];
    assert!(initial.state.winner.is_none());
    let mut server = ServerSession::new(initial, 42, vec![PlayerId(0), PlayerId(1)]).unwrap();
    let first = server.update(PlayerId(0), &[]).unwrap();
    let second = server.update(PlayerId(1), &[]).unwrap();
    assert_eq!(first.result, second.result);
    let report = first.result.unwrap();
    assert_eq!(report.players[0].outcome, MatchOutcome::Defeat);
    assert_eq!(report.players[1].outcome, MatchOutcome::Victory);
    let wire = ron::ser::to_string(&report).unwrap();
    for secret in [
        "entities",
        "production:",
        "orders",
        "position",
        "research",
        "fog:",
    ] {
        assert!(!wire.contains(secret), "report leaked {secret}");
    }
    let mut both_dead = world();
    both_dead.state.defeated = vec![PlayerId(0), PlayerId(1)];
    let drawn = ServerSession::new(both_dead, 42, vec![PlayerId(0), PlayerId(1)]).unwrap();
    assert!(
        drawn
            .match_result()
            .unwrap()
            .players
            .iter()
            .all(|p| p.outcome == MatchOutcome::Draw)
    );
}
