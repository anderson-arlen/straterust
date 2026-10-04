use super::*;
use std::{net::TcpListener, sync::atomic::AtomicBool};
use straterust_engine::{
    net::{RemoteClient, run_host},
    sim::GameplayIdentity,
};

#[test]
fn discovered_host_selects_installed_rules_and_the_menu_join_starts() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content");
    let world = Package::load(&root.join("lan-demo"))
        .unwrap()
        .world(42)
        .unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server_world = world.snapshot();
    let server = std::thread::spawn(move || {
        run_host(
            listener,
            server_world,
            42,
            Arc::new(AtomicBool::new(false)),
            None,
        )
    });
    let (mut host, _) = RemoteClient::connect(address, &world, PlayerId(0)).unwrap();
    let mut client = Client::new(
        Config {
            audio: false,
            ..Config::default()
        },
        PathBuf::from("/tmp/stratarust-client-settings.ron"),
        vec![root.clone()],
    );
    client.menus.page = Page::Multiplayer;
    client.menus.network_map = Some(root.join("terran-demo").canonicalize().unwrap());
    let mut identity = GameplayIdentity::of(&world);
    identity.map = "0".repeat(64); // Installed map identity is not a join requirement.
    let discovered = Discovered::new(
        vec![LanGame {
            name: "custom-map".into(),
            address,
            compatible: false,
            identity: identity.clone(),
        }],
        catalog::installed_rules(&client.menus.games),
    );
    assert!(
        discovered.games[0].compatible,
        "the selected demo's mismatched rules do not block another installed game"
    );
    let (send, receive) = std::sync::mpsc::channel();
    client.discovery = Some(receive);
    send.send(Ok(discovered)).unwrap();
    client.poll_lan();
    let (directory, _) = client.lan_join(0).unwrap();
    assert!(
        GameplayIdentity::of(&Package::client_definitions(&directory).unwrap())
            .same_rules(&identity)
    );
    assert!(
        !client
            .menus
            .choices(&client.config)
            .iter()
            .any(|c| c.button.label.contains("different map"))
    );
    client.pick(Pick::JoinLan(0)).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while client.session.as_ref().unwrap().world.tick().0 < 3 {
        host.poll().unwrap();
        let app = client.session.as_mut().unwrap();
        app.advance_network().unwrap();
        assert!(!app.network.as_ref().unwrap().ended, "{}", app.status);
        assert!(
            Instant::now() < deadline,
            "menu join did not start: {}",
            app.status
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    let app = client.session.as_ref().unwrap();
    assert!(app.world.is_player_view());
    assert_eq!(app.world.view_player(), PlayerId(1));
    assert!(app.network.as_ref().unwrap().started);
    client.finish().unwrap();
    drop(host);
    assert!(server.join().unwrap().is_err());
}

#[test]
fn campaign_rules_are_found_without_loading_its_scenarios_and_missing_rules_are_clear() {
    let root = PathBuf::from("/tmp/stratarust-lan-campaign");
    let mission = root.join("mission");
    std::fs::create_dir_all(&mission).unwrap();
    std::fs::write(
        root.join("campaign.ron"),
        "(schema_version:1,id:\"test\",missions:[(title:\"Mission\",package:\"mission\")])",
    )
    .unwrap();
    let content = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/lan-demo");
    std::fs::copy(content.join("rules.ron"), mission.join("rules.ron")).unwrap();
    for name in ["map.ron", "mission.ron"] {
        std::fs::write(mission.join(name), "deliberately invalid scenario").unwrap();
    }
    let mut client = Client::new(
        Config::default(),
        PathBuf::from("/tmp/stratarust-client-settings.ron"),
        vec![root.clone()],
    );
    let rules = catalog::installed_rules(&client.menus.games);
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].directory, mission.canonicalize().unwrap());
    let identity = rules[0].identity.clone();
    client.lan_rules = rules;
    client.menus.lan_games = vec![LanGame {
        name: "custom-map".into(),
        address: "127.0.0.1:6112".parse().unwrap(),
        compatible: true,
        identity,
    }];
    assert_eq!(
        client.lan_join(0).unwrap().0,
        mission.canonicalize().unwrap()
    );
    client.menus.lan_games[0].identity.rules = "0".repeat(64);
    assert!(
        client
            .lan_join(0)
            .unwrap_err()
            .to_string()
            .contains("No installed package matches")
    );
    std::fs::remove_dir_all(root).unwrap();
}
