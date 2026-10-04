use super::*;
use straterust_engine::content::Package;

fn package() -> Package {
    Package::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/fixtures")).unwrap()
}

fn app() -> App {
    App::new(
        &package(),
        Config {
            audio: false,
            ..Default::default()
        },
        Presentation::default(),
        None,
        None,
    )
    .unwrap()
}

fn receive(worker: &mut SimulationWorker) -> CompletedTick {
    loop {
        let completed = worker
            .results
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .unwrap();
        if completed.generation == worker.generation {
            return worker.accept(Ok(completed)).unwrap();
        }
    }
}

#[test]
fn local_server_updates_match_the_shared_player_protocol_in_tick_order() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/fixtures/scenario.ron");
    let scenario: Scenario = read_ron(&path).unwrap();
    for batch_size in [1, 8] {
        let mut expected = ServerSession::new(
            package().world(scenario.seed).unwrap(),
            scenario.seed,
            vec![PlayerId(0)],
        )
        .unwrap();
        expected.update(PlayerId(0), &[]).unwrap();
        let mut commands = CommandQueue::from_scenario(&scenario).unwrap();
        let (mut worker, initial) = SimulationWorker::local(
            package().world(scenario.seed).unwrap(),
            scenario.seed,
            Some(scenario.clone()),
        )
        .unwrap();
        assert!(initial.is_player_view());
        while expected.world().tick().0 < scenario.ticks {
            let count = batch_size.min(scenario.ticks - expected.world().tick().0);
            worker
                .submit(vec![Vec::new(); count as usize], false)
                .unwrap();
            for _ in 0..count {
                let batch = commands.take(expected.world().tick());
                let outcomes = expected.advance(&batch).unwrap();
                let view = expected.update(PlayerId(0), &outcomes).unwrap();
                let result = receive(&mut worker);
                assert_eq!(result.update.sequence, view.sequence);
                assert_eq!(result.update.view, view.view);
                assert_eq!(result.update.outcomes, view.outcomes);
            }
            assert!(!worker.pending);
            assert!(worker.poll().unwrap().is_none());
        }
    }
}

#[test]
fn client_never_retains_an_authoritative_world_even_at_start_or_restart() {
    let mut app = app();
    assert!(app.world.is_player_view() && app.initial_world.is_player_view());
    assert!(app.world.state().last_sequences.is_empty());
    assert_eq!(app.world.state().rng_state, 0);
    assert!(app.world.step(&[]).is_err());
    app.restart().unwrap();
    let worker = app.simulation.as_mut().unwrap();
    let reset = receive(worker);
    assert_eq!(reset.update.view.tick, Tick(0));
    assert!(
        reset
            .update
            .view
            .into_world(&app.initial_world)
            .unwrap()
            .is_player_view()
    );
}

#[test]
fn stalled_server_keeps_client_input_available_and_restart_discards_old_updates() {
    let mut app = app();
    let (started, entered) = mpsc::sync_channel(1);
    let (release, blocked) = mpsc::sync_channel(1);
    let mut first = true;
    let server = ServerSession::new(package().world(42).unwrap(), 42, vec![PlayerId(0)]).unwrap();
    app.simulation = Some(
        SimulationWorker::spawn_with_hook(server, None, move || {
            if first {
                first = false;
                started.send(()).unwrap();
                blocked.recv().unwrap();
            }
            Ok(())
        })
        .unwrap(),
    );
    app.advance_simulation(Duration::from_millis(400)).unwrap();
    entered.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(app.simulation.as_mut().unwrap().poll().unwrap().is_none());
    app.selected.insert(EntityId(1));
    app.issue(Order::Move {
        entity: EntityId(1),
        target: Position { x: 700, y: 420 },
    })
    .unwrap();
    assert_eq!(app.recorded[0].tick, Tick(8));
    assert!(!app.buttons().is_empty());
    app.restart().unwrap();
    assert!(app.recorded.is_empty());
    release.send(()).unwrap();
    let reset = receive(app.simulation.as_mut().unwrap());
    assert_eq!(reset.update.view.tick, Tick(0));
    assert!(app.simulation.as_mut().unwrap().poll().unwrap().is_none());
}

#[test]
fn failed_server_tick_reports_an_error_without_advancing_the_view() {
    let mut app = app();
    let initial = app.world.state_hash();
    app.simulation = Some(
        SimulationWorker::spawn_with_hook(
            ServerSession::new(package().world(42).unwrap(), 42, vec![PlayerId(0)]).unwrap(),
            None,
            || bail!("injected tick failure"),
        )
        .unwrap(),
    );
    app.advance_simulation(Duration::from_millis(400)).unwrap();
    let failure = app
        .simulation
        .as_mut()
        .unwrap()
        .results
        .recv_timeout(Duration::from_secs(2))
        .unwrap()
        .unwrap_err();
    assert!(failure.to_string().contains("injected tick failure"));
    assert_eq!(app.world.state_hash(), initial);
}
