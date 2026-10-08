use super::*;
use crate::controls::Action;
use straterust_engine::sim::ModeChange;

fn advance(app: &mut App, count: u32) {
    for _ in 0..count {
        let tick = app.world.tick().0 + 1;
        app.advance_simulation(Duration::from_millis(u64::from(app.world.rules().tick_ms)))
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        while app.world.tick().0 < tick {
            app.poll_simulation().unwrap();
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
    }
}

fn checkpoint(app: &App, root: &Path, file: &Path) {
    app.simulation
        .as_ref()
        .unwrap()
        .save(
            file.to_owned(),
            SaveHeader::capture(app, root).unwrap(),
            app.queue.commands().cloned().collect(),
        )
        .unwrap();
    wait_saved(app);
}

#[test]
fn local_save_keeps_deployed_modes_and_both_transition_directions() {
    let root = path("modes-package");
    std::fs::create_dir_all(&root).unwrap();
    for name in ["manifest.ron", "map.ron"] {
        std::fs::copy(fixture().join(name), root.join(name)).unwrap();
    }
    let mut rules = Package::load(&fixture())
        .unwrap()
        .world(42)
        .unwrap()
        .rules()
        .clone();
    rules.units[0].mode = Some(ModeChange {
        target: UnitTypeId(99),
        ticks: 4,
        research: None,
    });
    let mut deployed = rules.units[0].clone();
    deployed.id = UnitTypeId(99);
    deployed.speed = 0;
    deployed.mode = Some(ModeChange {
        target: UnitTypeId(1),
        ticks: 3,
        research: None,
    });
    rules.units.push(deployed);
    std::fs::write(root.join("rules.ron"), ron::ser::to_string(&rules).unwrap()).unwrap();
    let root = root.canonicalize().unwrap();
    let config = Config {
        audio: false,
        ..Default::default()
    };
    let mut app = App::load(&root, config.clone(), None).unwrap();
    app.selected.insert(EntityId(1));
    app.activate(Action::ChangeMode).unwrap();
    advance(&mut app, 1);
    let file = path("modes.srsave");
    checkpoint(&app, &root, &file);
    let transition = app.world.state().entities[0].mode_transition.clone();
    assert!(transition.is_some());
    let mut app = App::load_saved(&file, config.clone()).unwrap();
    assert_eq!(app.world.state().entities[0].mode_transition, transition);
    advance(&mut app, 4);
    assert_eq!(app.world.state().entities[0].unit_type, UnitTypeId(99));
    checkpoint(&app, &root, &file);
    // Ordinary content changes exercise the compatible-save loader too.
    rules.units[0].speed += 1;
    std::fs::write(root.join("rules.ron"), ron::ser::to_string(&rules).unwrap()).unwrap();
    let mut app = App::load_saved(&file, config.clone()).unwrap();
    assert!(app.world.is_player_view());
    assert_eq!(app.world.state().entities[0].unit_type, UnitTypeId(99));
    let position = app.world.state().entities[0].position;
    app.issue(Order::Move {
        entity: EntityId(1),
        target: Position { x: 700, y: 400 },
    })
    .unwrap();
    advance(&mut app, 1);
    assert_eq!(app.world.state().entities[0].position, position);
    assert!(app.status.contains("UnsupportedOrder"));
    app.activate(Action::ChangeMode).unwrap();
    advance(&mut app, 1);
    checkpoint(&app, &root, &file);
    let transition = app.world.state().entities[0].mode_transition.clone();
    let mut app = App::load_saved(&file, config).unwrap();
    assert_eq!(app.world.state().entities[0].unit_type, UnitTypeId(99));
    assert_eq!(app.world.state().entities[0].mode_transition, transition);
    advance(&mut app, 3);
    assert_eq!(app.world.state().entities[0].unit_type, UnitTypeId(1));
    std::fs::remove_file(file).unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
#[ignore = "requires the existing Big Push save in local slot 4; never writes it"]
fn retail_tank_mode_roundtrips_from_the_latest_older_save() -> Result<()> {
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/saves/slot-4.srsave");
    let original = std::fs::read(&file)?;
    let (header, saved) = read(&file)?;
    let definitions = Package::load(&header.package)?.world(saved.checkpoint.seed)?;
    let mut server = ServerSession::restore_saved(&definitions, saved)?;
    let tank = server
        .world()
        .state()
        .entities
        .iter()
        .find(|e| {
            e.owner == PlayerId(0)
                && e.unit_type == UnitTypeId(22)
                && server.world().mode_rejection(e.id).is_none()
        })
        .context("save has no available tank with Siege Tech")?
        .id;
    let command = Command {
        tick: server.world().tick(),
        player: PlayerId(0),
        sequence: server.world().state().last_sequences[0] + 1,
        order: Order::ChangeMode { entity: tank },
    };
    assert_eq!(server.advance(&[command])?[0].rejection, None);
    let entity = |server: &ServerSession| {
        server
            .world()
            .state()
            .entities
            .iter()
            .find(|e| e.id == tank)
            .unwrap()
            .clone()
    };
    assert!(entity(&server).mode_transition.is_some());
    let roundtrip = |server: &ServerSession| -> Result<ServerSession> {
        ServerSession::restore_saved(
            &definitions,
            SavedGame::decode(&SavedGame::capture(server)?.encode()?)?,
        )
    };
    let progress = entity(&server);
    server = roundtrip(&server)?;
    assert_eq!(entity(&server), progress);
    let remaining = progress.mode_transition.unwrap().remaining;
    for _ in 0..remaining {
        server.advance(&[])?;
    }
    assert_eq!(entity(&server).unit_type, UnitTypeId(56));
    let deployed = entity(&server);
    server = roundtrip(&server)?;
    assert_eq!(entity(&server), deployed);
    let view = server
        .update(PlayerId(0), &[])?
        .view
        .into_world(&definitions)?;
    assert_eq!(
        view.state()
            .entities
            .iter()
            .find(|e| e.id == tank)
            .unwrap()
            .unit_type,
        UnitTypeId(56)
    );
    let command = Command {
        tick: server.world().tick(),
        player: PlayerId(0),
        sequence: server.world().state().last_sequences[0] + 1,
        order: Order::Move {
            entity: tank,
            target: Position { x: 1600, y: 2400 },
        },
    };
    assert_eq!(
        server.advance(&[command])?[0].rejection,
        Some(straterust_engine::sim::Rejection::UnsupportedOrder)
    );
    assert_eq!(entity(&server).position, deployed.position);
    assert_eq!(std::fs::read(file)?, original);
    Ok(())
}
