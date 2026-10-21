use super::*;
use crate::controls::{Action, TargetMode};

#[test]
fn guest_can_place_and_finish_a_building_after_prediction_has_advanced() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/lan-demo");
    let directory = tempfile::tempdir().unwrap();
    for name in [
        "manifest.ron",
        "rules.ron",
        "map.ron",
        "presentation.ron",
        "client.ron",
        "terrain.srtm",
    ] {
        std::fs::copy(source.join(name), directory.path().join(name)).unwrap();
    }
    let rules = std::fs::read_to_string(directory.path().join("rules.ron"))
        .unwrap()
        .replace("amount: 50)", "amount: 1000)")
        .replace("build_ticks: 600", "build_ticks: 10");
    std::fs::write(directory.path().join("rules.ron"), rules).unwrap();
    let reservation = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = reservation.local_addr().unwrap();
    drop(reservation);
    let mut apps = [true, false].map(|hosting| {
        let mut app = App::load_network(
            directory.path(),
            Config {
                audio: false,
                ..Config::default()
            },
        )
        .unwrap();
        app.start_network(directory.path(), address, hosting, None)
            .unwrap();
        app
    });
    let deadline = Instant::now() + Duration::from_secs(8);
    while apps[1].network.as_ref().unwrap().prediction.is_none()
        || apps[1].paused
        || apps[1].world.tick().0 < 5
    {
        for app in &mut apps {
            app.advance_network().unwrap();
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    let guest = &mut apps[1];
    guest.selected = BTreeSet::from([EntityId(5)]);
    guest.activate(Action::BuildMenu).unwrap();
    let button = guest
        .buttons()
        .into_iter()
        .find(|b| b.action == Action::Build(UnitTypeId(4)))
        .unwrap();
    assert!(button.disabled.is_none(), "{:?}", button.disabled);
    guest.activate(Action::Build(UnitTypeId(4))).unwrap();
    assert_eq!(guest.target_mode, Some(TargetMode::Build(UnitTypeId(4))));
    guest.targeting_click(Position { x: 1168, y: 832 }).unwrap();
    assert!(
        matches!(
            guest.recorded.last().map(|c| &c.order),
            Some(Order::Build {
                unit_type: UnitTypeId(4),
                ..
            })
        ),
        "{}",
        guest.status
    );
    loop {
        for app in &mut apps {
            app.advance_network().unwrap();
        }
        let confirmed = &apps[1]
            .network
            .as_ref()
            .unwrap()
            .prediction
            .as_ref()
            .unwrap()
            .confirmed;
        let buildings: Vec<_> = confirmed
            .state()
            .entities
            .iter()
            .filter(|e| e.owner == PlayerId(1) && e.unit_type == UnitTypeId(4))
            .collect();
        if buildings.iter().any(|e| e.construction.is_none()) {
            assert_eq!(
                buildings.len(),
                1,
                "prediction must not allocate duplicate foundations"
            );
            assert_eq!(confirmed.state().players[1].resources["minerals"], 900);
            break;
        }
        assert!(Instant::now() < deadline, "{}", apps[1].status);
        assert!(!apps[1].network.as_ref().unwrap().ended);
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn stalled_redraw_uses_latest_view_and_ended_session_commands_keep_ui_open() {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/lan-demo");
    let reservation = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = reservation.local_addr().unwrap();
    drop(reservation);
    let mut apps = [true, false].map(|hosting| {
        let mut app = App::load_network(
            &directory,
            Config {
                audio: false,
                ..Config::default()
            },
        )
        .unwrap();
        app.start_network(&directory, address, hosting, None)
            .unwrap();
        app
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    while apps
        .iter()
        .any(|app| !app.network.as_ref().unwrap().started)
    {
        for app in &mut apps {
            app.advance_network().unwrap();
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    let before = apps[1].world.tick();
    let until = Instant::now() + Duration::from_millis(500);
    while Instant::now() < until {
        apps[0].advance_network().unwrap();
        std::thread::sleep(Duration::from_millis(5));
    }
    let newest = apps[1]
        .network
        .as_ref()
        .unwrap()
        .latest_update
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .view
        .tick;
    assert!(newest.0 > before.0 + straterust_engine::session::INPUT_LEAD);
    apps[1].advance_network().unwrap();
    assert!(apps[1].world.tick().0 >= newest.0);
    assert!(!apps[1].network.as_ref().unwrap().ended);
    apps[0]
        .network
        .as_ref()
        .unwrap()
        .cancelled
        .store(true, Ordering::Release);
    while !apps[1].network.as_ref().unwrap().ended {
        apps[1].advance_network().unwrap();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    let message = apps[1].status.clone();
    let recorded = apps[1].recorded.len();
    let entity = apps[1]
        .world
        .state()
        .entities
        .iter()
        .find(|entity| entity.owner == apps[1].world.view_player())
        .unwrap()
        .id;
    apps[1].issue(Order::Stop { entity }).unwrap();
    assert_eq!(apps[1].status, message);
    assert_eq!(apps[1].recorded.len(), recorded);
    assert!(apps[1].failure.is_none());
}
