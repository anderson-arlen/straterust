use super::*;
use straterust_engine::content::Package;

fn app(scenario: Option<Scenario>) -> App {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/fixtures");
    App::new(
        &Package::load(&path).unwrap(),
        Config {
            audio: false,
            ..Default::default()
        },
        Presentation::default(),
        None,
        scenario,
    )
    .unwrap()
}

fn receive(worker: &mut SimulationWorker) -> CompletedTick {
    let result = worker.results.recv_timeout(Duration::from_secs(2)).unwrap();
    worker.accept(result).unwrap()
}

#[test]
fn threaded_ticks_match_headless_playback_with_every_result_in_order() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/fixtures");
    let package = Package::load(&path).unwrap();
    let scenario: Scenario = read_ron(&path.join("scenario.ron")).unwrap();
    for batch_size in [1, 8] {
        let mut headless = package.world(scenario.seed).unwrap();
        let mut commands = CommandQueue::from_scenario(&scenario).unwrap();
        let mut worker = SimulationWorker::new(headless.snapshot()).unwrap();
        while headless.tick().0 < scenario.ticks {
            let count = batch_size.min(scenario.ticks - headless.tick().0);
            let mut expected = Vec::new();
            let mut batch = Vec::new();
            for _ in 0..count {
                let tick = commands.take(headless.tick());
                let outcomes = headless.step(&tick).unwrap();
                expected.push((headless.tick(), headless.state_hash(), outcomes));
                batch.push(tick);
            }
            worker.submit(batch, false).unwrap();
            assert_eq!(worker.command_tick(), headless.tick());
            for (tick, hash, outcomes) in expected {
                let completed = receive(&mut worker);
                assert_eq!(completed.world.tick(), tick);
                assert_eq!(completed.world.state_hash(), hash);
                assert_eq!(completed.outcomes, outcomes);
            }
            assert!(!worker.pending);
            assert!(worker.poll().unwrap().is_none());
        }
    }
}

#[test]
fn stalled_tick_keeps_input_and_rendering_available_and_restart_discards_it() {
    let mut app = app(None);
    let (started, entered) = mpsc::sync_channel(1);
    let (release, blocked) = mpsc::sync_channel(1);
    let (finished, exited) = mpsc::sync_channel(1);
    app.simulation = Some(
        SimulationWorker::spawn(app.world.snapshot(), move |world, commands| {
            started.send(()).unwrap();
            blocked.recv().unwrap();
            let result = world.step(commands);
            finished.send(()).unwrap();
            result
        })
        .unwrap(),
    );
    app.advance_simulation(Duration::from_millis(400)).unwrap();
    entered.recv_timeout(Duration::from_secs(2)).unwrap();
    let (responsive, response) = mpsc::sync_channel(1);
    let ui = std::thread::spawn(move || {
        assert!(app.simulation.as_mut().unwrap().poll().unwrap().is_none());
        let position = app.world.state().entities[0].position;
        app.select_at(position);
        app.issue(Order::Move {
            entity: EntityId(1),
            target: Position { x: 700, y: 420 },
        })
        .unwrap();
        assert_eq!(app.recorded[0].tick, Tick(8));
        let buttons = app.buttons();
        let scene = View {
            world: &app.world,
            visuals: &app.visuals,
            presentation: &app.presentation,
            assets: None,
            media: None,
            mission: None,
            speaking: None,
            animation_ms: 0,
            portrait_ms: 0,
            camera: app.camera,
            selected: &app.selected,
            selected_resource: None,
            cursor: [100.0, 100.0],
            targeting: false,
            drag_box: None,
            paused: false,
            playback: false,
            status: &app.status,
            buttons: &buttons,
            help: "",
            placement: None,
            ending_hint: "",
        }
        .scene(1100, 760, 1.0);
        assert!(!scene.commands.is_empty());
        app.restart().unwrap();
        assert_eq!(app.world.tick(), Tick(0));
        assert!(app.simulation.is_none() && app.recorded.is_empty());
        responsive.send(()).unwrap();
        app
    });
    // Release the artificial stall even on failure, so the regression fails
    // promptly instead of leaving a blocked test/thread behind.
    let progress = response.recv_timeout(Duration::from_secs(2));
    release.send(()).unwrap();
    exited.recv_timeout(Duration::from_secs(2)).unwrap();
    let mut app = ui.join().unwrap();
    assert!(
        progress.is_ok(),
        "input/render/restart waited on the simulation worker"
    );
    app.advance_simulation(Duration::ZERO).unwrap();
    assert_eq!(
        app.world.tick(),
        Tick(0),
        "old result entered restarted session"
    );
}

#[test]
fn pause_keeps_the_displayed_snapshot_and_playback_stops_at_its_exact_end() {
    let mut app = app(Some(Scenario {
        schema_version: 1,
        seed: 42,
        ticks: 3,
        commands: vec![],
    }));
    app.advance_simulation(Duration::from_millis(400)).unwrap();
    assert_eq!(app.simulation.as_ref().unwrap().command_tick(), Tick(3));
    app.paused = true;
    let hash = app.world.state_hash();
    app.advance_simulation(Duration::from_secs(10)).unwrap();
    assert_eq!(app.world.state_hash(), hash);
    // Wait only in this test; the window always uses nonblocking polling.
    let worker = app.simulation.as_mut().unwrap();
    let mut results = Vec::new();
    for _ in 0..3 {
        results.push(receive(worker));
    }
    for completed in results {
        app.world = completed.world;
        app.observe_tick(completed.outcomes).unwrap();
    }
    app.paused = false;
    app.advance_simulation(Duration::ZERO).unwrap();
    assert!(app.paused);
    assert_eq!(app.world.tick(), Tick(3));
    assert!(!app.simulation.as_ref().unwrap().pending);
}

#[test]
fn worker_errors_reach_the_ui_without_publishing_a_partial_tick() {
    let original = app(None).world;
    let mut worker =
        SimulationWorker::spawn(original, |_, _| bail!("injected tick failure")).unwrap();
    worker.submit(vec![vec![]], false).unwrap();
    let result = worker.results.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(
        worker
            .accept(result)
            .unwrap_err()
            .to_string()
            .contains("injected tick failure")
    );
}

#[test]
fn input_received_during_an_unfinished_tick_executes_on_the_next_tick() {
    let mut app = app(None);
    let (started, entered) = mpsc::sync_channel(1);
    let (release, blocked) = mpsc::sync_channel(1);
    let mut first = true;
    app.simulation = Some(
        SimulationWorker::spawn(app.world.snapshot(), move |world, commands| {
            if first {
                first = false;
                started.send(()).unwrap();
                blocked.recv().unwrap();
            }
            world.step(commands)
        })
        .unwrap(),
    );
    app.advance_simulation(Duration::from_millis(50)).unwrap();
    entered.recv_timeout(Duration::from_secs(2)).unwrap();
    app.issue(Order::Move {
        entity: EntityId(1),
        target: Position { x: 700, y: 420 },
    })
    .unwrap();
    assert_eq!(app.recorded[0].tick, Tick(1));
    release.send(()).unwrap();
    let completed = receive(app.simulation.as_mut().unwrap());
    app.world = completed.world;
    app.observe_tick(completed.outcomes).unwrap();
    app.advance_simulation(Duration::from_millis(50)).unwrap();
    let completed = receive(app.simulation.as_mut().unwrap());
    assert_eq!(completed.outcomes.len(), 1);
    assert_eq!(completed.outcomes[0].command, app.recorded[0]);
    assert_eq!(completed.outcomes[0].rejection, None);
    assert_eq!(completed.world.tick(), Tick(2));
    assert!(completed.world.state().entities[0].position.x > 420);
}

#[test]
fn live_victory_stops_the_batch_before_later_ticks() {
    use straterust_engine::sim::{Mission, MissionAction, MissionLocation, MissionTrigger};
    let initial = app(None).world;
    let mut map = initial.map().clone();
    map.mission = Some(Mission {
        schema_version: 1,
        player: PlayerId(0),
        rescuable_players: vec![],
        rescuers: vec![],
        alliances: vec![],
        poll_ticks: 1,
        wait_step_ms: 50,
        locations: vec![MissionLocation {
            excluded_elevations: 0,
            left: 0,
            top: 0,
            right: map.width,
            bottom: map.height,
        }],
        triggers: vec![MissionTrigger {
            conditions: vec![straterust_engine::sim::MissionCondition::Elapsed {
                comparison: straterust_engine::sim::MissionComparison::AtLeast,
                milliseconds: 0,
            }],
            actions: vec![MissionAction::Victory],
        }],
    });
    let world = World::new(initial.rules().clone(), map, 42).unwrap();
    let mut expected = world.snapshot();
    let mut worker = SimulationWorker::new(world).unwrap();
    worker.submit(vec![vec![]; 8], true).unwrap();
    loop {
        expected.step(&[]).unwrap();
        let result = receive(&mut worker);
        assert_eq!(result.world.state_hash(), expected.state_hash());
        if result.batch_done {
            assert_eq!(result.world.state().winner, Some(PlayerId(0)));
            assert!(result.world.tick().0 < 8);
            assert!(!worker.pending);
            assert!(worker.poll().unwrap().is_none());
            break;
        }
    }
}
