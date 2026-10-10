use super::*;
use straterust_engine::{
    content::Package,
    session::{SavedGame, ServerSession},
    sim::Tick,
};
mod modes;

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../content/fixtures")
        .canonicalize()
        .unwrap()
}
fn app() -> App {
    App::load(
        &fixture(),
        Config {
            audio: false,
            ..Default::default()
        },
        None,
    )
    .unwrap()
}
fn path(name: &str) -> PathBuf {
    std::env::temp_dir()
        .join("stratarust-save-tests")
        .join(name)
}
fn wait_saved(app: &App) {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if app
            .simulation
            .as_ref()
            .unwrap()
            .poll_save()
            .unwrap()
            .is_some()
        {
            return;
        }
        assert!(Instant::now() < deadline, "save did not finish");
        std::thread::yield_now();
    }
}

#[test]
fn checkpoint_keeps_pending_orders_and_restores_only_a_filtered_view() {
    let mut app = app();
    app.selected.insert(EntityId(1));
    app.groups[2].insert(EntityId(1));
    app.camera.x = 440.0;
    app.paused = true;
    app.issue(Order::Move {
        entity: EntityId(1),
        target: Position { x: 700, y: 420 },
    })
    .unwrap();
    let header = SaveHeader::capture(&app, &fixture()).unwrap();
    let file = path("continuation.srsave");
    app.simulation
        .as_ref()
        .unwrap()
        .save(
            file.clone(),
            header.clone(),
            app.queue.commands().cloned().collect(),
        )
        .unwrap();
    wait_saved(&app);
    let (_, saved) = read(&file).unwrap();
    assert_eq!(saved.checkpoint.pending.len(), 1);
    let definitions = Package::load(&fixture()).unwrap().world(42).unwrap();
    let mut expected = ServerSession::restore_saved(&definitions, saved).unwrap();
    let mut loaded = App::load_saved(
        &file,
        Config {
            audio: false,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(loaded.world.is_player_view() && loaded.initial_world.is_player_view());
    assert_eq!(loaded.world.state().rng_state, 0);
    assert!(loaded.world.state().last_sequences.is_empty());
    assert_eq!(loaded.selected, app.selected);
    assert_eq!(loaded.groups[2], app.groups[2]);
    assert_eq!(loaded.sequence, app.sequence);
    assert!(loaded.paused);
    loaded.paused = false;
    for _ in 0..12 {
        let outcomes = expected.advance(&[]).unwrap();
        let view = expected.update(PlayerId(0), &outcomes).unwrap().view;
        loaded
            .advance_simulation(Duration::from_millis(u64::from(
                loaded.world.rules().tick_ms,
            )))
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        while loaded.world.tick() < view.tick {
            loaded.poll_simulation().unwrap();
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        assert_eq!(loaded.world.tick(), view.tick);
        assert_eq!(
            loaded.world.state().entities,
            view.into_world(&definitions).unwrap().state().entities
        );
    }
    // New sequences remain accepted after accepted-but-not-yet-executed input.
    loaded
        .issue(Order::Move {
            entity: EntityId(1),
            target: Position { x: 500, y: 400 },
        })
        .unwrap();
    assert_eq!(loaded.sequence, 2);
    std::fs::remove_file(file).unwrap();
}

#[test]
fn invalid_checkpoint_is_rejected_and_failed_overwrite_preserves_the_old_save() {
    let app = app();
    let header = SaveHeader::capture(&app, &fixture()).unwrap();
    let server = ServerSession::new(
        Package::load(&fixture()).unwrap().world(42).unwrap(),
        42,
        vec![PlayerId(0)],
    )
    .unwrap();
    let saved = SavedGame::capture(&server).unwrap();
    let file = path("validation.srsave");
    write(&file, &header, &saved).unwrap();
    let before = std::fs::read(&file).unwrap();
    let mut invalid_header = header.clone();
    invalid_header.camera.zoom = f64::NAN;
    assert!(write(&file, &invalid_header, &saved).is_err());
    assert_eq!(std::fs::read(&file).unwrap(), before);
    let mut corrupt = saved;
    corrupt.checkpoint.world.state.entities[0].hp -= 1;
    assert!(write(&file, &header, &corrupt).is_err());
    let mut damaged = before;
    *damaged.last_mut().unwrap() = b'!';
    std::fs::write(&file, damaged).unwrap();
    assert!(
        App::load_saved(
            &file,
            Config {
                audio: false,
                ..Default::default()
            }
        )
        .is_err()
    );
    std::fs::write(&file, b"bad").unwrap();
    assert!(read(&file).is_err());
    assert!(slot_path(Path::new("local/saves"), SLOT_COUNT).is_err());
    std::fs::remove_file(file).unwrap();
}

#[test]
fn restored_game_restart_uses_mission_start_not_the_checkpoint() {
    let definitions = Package::load(&fixture()).unwrap().world(42).unwrap();
    let mut server = ServerSession::new(definitions.clone(), 42, vec![PlayerId(0)]).unwrap();
    for _ in 0..10 {
        server.advance(&[]).unwrap();
    }
    let (mut worker, view, restart) =
        simulation::SimulationWorker::restored(definitions, SavedGame::capture(&server).unwrap())
            .unwrap();
    assert_eq!(view.tick(), Tick(10));
    assert_eq!(restart.tick(), Tick(0));
    worker.restart().unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(update) = worker.poll_for_test().unwrap() {
            assert_eq!(update, Tick(0));
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
}

#[test]
#[ignore = "run optimized against existing local saves and installed packages; never writes them"]
fn existing_local_saves_resume_after_updates() -> Result<()> {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/saves");
    for slot in 0..4 {
        let file = slot_path(&directory, slot)?;
        let before = std::fs::read(&file)?;
        let (mut header, saved) = read(&file)?;
        if let Some(game) = std::env::var_os("STRATERUST_STARCRAFT") {
            assert!(
                header.relocate(Path::new(&game))?,
                "old save campaign was not found in the new installation"
            );
        }
        let progress = saved.checkpoint.world.state.clone();
        let definitions = Package::load(&header.package)?.world(saved.checkpoint.seed)?;
        let mut server = ServerSession::restore_saved(&definitions, saved)?;
        let loaded = server.world().state();
        assert_eq!(loaded.tick, progress.tick);
        assert_eq!(loaded.rng_state, progress.rng_state);
        assert_eq!(loaded.entities.len(), progress.entities.len());
        assert_eq!(loaded.resources, progress.resources);
        assert_eq!(loaded.statistics, progress.statistics);
        assert_eq!(loaded.terrain_fog, progress.terrain_fog);
        for (old, new) in progress.entities.iter().zip(&loaded.entities) {
            assert_eq!(
                (
                    old.id,
                    old.unit_type,
                    old.position,
                    &old.cargo,
                    old.carried_by,
                    &old.mode_transition
                ),
                (
                    new.id,
                    new.unit_type,
                    new.position,
                    &new.cargo,
                    new.carried_by,
                    &new.mode_transition
                )
            );
        }
        let encoded = SavedGame::capture(&server)?.encode()?;
        let mut continued =
            ServerSession::restore_saved(&definitions, SavedGame::decode(&encoded)?)?;
        for _ in 0..1 {
            server.advance(&[])?;
            continued.advance(&[])?;
            assert_eq!(server.world().state_hash(), continued.world().state_hash());
        }
        assert_eq!(std::fs::read(&file)?, before);
        println!(
            "Slot {}: {} resumed at tick {}, {} entities; continuation and originals preserved",
            slot + 1,
            header.title,
            header.tick,
            progress.entities.len()
        );
    }
    Ok(())
}

#[test]
fn saved_campaign_advances_across_the_old_five_mission_boundary() {
    use straterust_engine::content::CampaignMission;
    use straterust_engine::sim::{
        Mission, MissionAction, MissionComparison, MissionCondition, MissionLocation,
        MissionTrigger,
    };
    let root = path("campaign");
    let manifest = Campaign {
        schema_version: 1,
        id: "save-test-campaign".into(),
        missions: (1..=10)
            .map(|n| CampaignMission {
                title: format!("Mission {n}"),
                package: format!("mission-{n}"),
            })
            .collect(),
    };
    for mission in &manifest.missions {
        let directory = root.join(&mission.package);
        std::fs::create_dir_all(&directory).unwrap();
        for name in ["manifest.ron", "rules.ron", "map.ron"] {
            std::fs::copy(fixture().join(name), directory.join(name)).unwrap();
        }
        let ending = Mission {
            schema_version: 1,
            player: PlayerId(0),
            rescuable_players: vec![],
            rescuers: vec![],
            alliances: vec![],
            poll_ticks: 1,
            wait_step_ms: 42,
            locations: vec![MissionLocation {
                excluded_elevations: 0,
                left: 0,
                top: 0,
                right: 1600,
                bottom: 1000,
            }],
            triggers: vec![MissionTrigger {
                conditions: vec![MissionCondition::Elapsed {
                    comparison: MissionComparison::AtLeast,
                    milliseconds: 0,
                }],
                actions: vec![MissionAction::Victory],
            }],
        };
        std::fs::write(
            directory.join("mission.ron"),
            ron::ser::to_string(&ending).unwrap(),
        )
        .unwrap();
    }
    std::fs::write(
        root.join("campaign.ron"),
        ron::ser::to_string(&manifest).unwrap(),
    )
    .unwrap();
    let root = root.canonicalize().unwrap();
    let mut original = App::load(
        &root.join("mission-5"),
        Config {
            audio: false,
            ..Default::default()
        },
        None,
    )
    .unwrap();
    original.campaign = Some(CampaignSession {
        root: root.clone(),
        manifest,
        index: 4,
    });
    let header = SaveHeader::capture(&original, &root).unwrap();
    let server = ServerSession::new(
        Package::load(&root.join("mission-5"))
            .unwrap()
            .world(42)
            .unwrap(),
        42,
        vec![PlayerId(0)],
    )
    .unwrap();
    let file = path("campaign.srsave");
    write(&file, &header, &SavedGame::capture(&server).unwrap()).unwrap();
    let mut loaded = App::load_saved(
        &file,
        Config {
            audio: false,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(loaded.campaign.as_ref().unwrap().index, 4);
    assert!(!loaded.advance_campaign().unwrap());
    loaded
        .advance_simulation(Duration::from_millis(
            2 * u64::from(loaded.world.rules().tick_ms),
        ))
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    while loaded.world.tick() < Tick(2) {
        loaded.poll_simulation().unwrap();
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    let header = SaveHeader::capture(&loaded, &root).unwrap();
    loaded
        .simulation
        .as_ref()
        .unwrap()
        .save(file.clone(), header, vec![])
        .unwrap();
    wait_saved(&loaded);
    let mut loaded = App::load_saved(
        &file,
        Config {
            audio: false,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(loaded.match_result.is_some());
    assert!(loaded.advance_campaign().unwrap());
    assert_eq!(loaded.campaign.as_ref().unwrap().index, 5);
    assert_eq!(loaded.package_directory, Some(root.join("mission-6")));
    std::fs::remove_file(file).unwrap();
    std::fs::remove_dir_all(root).unwrap();
}
