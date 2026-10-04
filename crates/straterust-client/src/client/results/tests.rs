use super::*;
use std::net::TcpListener;
use straterust_engine::{session::MatchOutcome, sim::Map};

#[test]
fn match_results_show_player_one_victory_and_both_clients_return_to_the_lobby() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/lan-duel");
    let temporary = PathBuf::from("/tmp/straterust-match-results-package");
    std::fs::create_dir_all(&temporary).unwrap();
    std::fs::copy(root.join("manifest.ron"), temporary.join("manifest.ron")).unwrap();
    std::fs::copy(root.join("rules.ron"), temporary.join("rules.ron")).unwrap();
    let mut map: Map = read_ron(&root.join("map.ron")).unwrap();
    map.spawns[0].hp_percent = Some(1);
    map.spawns[1].hp_percent = None;
    map.spawns[1].position.x = 224;
    std::fs::write(
        temporary.join("map.ron"),
        ron::ser::to_string(&map).unwrap(),
    )
    .unwrap();
    let reservation = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = reservation.local_addr().unwrap();
    drop(reservation);
    let mut clients = [true, false].map(|hosting| {
        let mut client = Client::new(
            Config {
                audio: false,
                ..Config::default()
            },
            PathBuf::from("/tmp/straterust-client-settings.ron"),
            vec![temporary.clone()],
        );
        client
            .menus
            .choose(GameEntry::read(&root).unwrap())
            .unwrap();
        client.remember_network(&temporary, address).unwrap();
        client.play_mode(&temporary, None, true).unwrap();
        client
            .session
            .as_mut()
            .unwrap()
            .start_network(&temporary, address, hosting, None)
            .unwrap();
        client
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    while clients.iter().any(|c| c.menus.page != Page::Results) {
        for client in &mut clients {
            client.session.as_mut().unwrap().advance_network().unwrap();
            client.show_match_results();
        }
        assert!(
            Instant::now() < deadline,
            "missing results: {:?}",
            clients
                .iter()
                .map(|c| &c.session.as_ref().unwrap().status)
                .collect::<Vec<_>>()
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(clients[0].menus.title(), "Defeat");
    assert_eq!(clients[1].menus.title(), "Victory");
    assert_eq!(clients[0].menus.result, clients[1].menus.result);
    let report = clients[1].menus.result.as_ref().unwrap();
    assert_eq!(report.players[1].outcome, MatchOutcome::Victory);
    assert_eq!(report.players[1].statistics.units_killed, 1);
    assert_eq!(report.players[0].statistics.units_lost, 1);
    for client in &mut clients {
        assert!(client.session.as_ref().unwrap().menu_open);
        assert_eq!(
            client.menus.choices(&client.config)[0].button.label,
            "Return to Multiplayer Lobby"
        );
        client.key(KeyCode::F5).unwrap();
        assert_eq!(client.menus.page, Page::Results);
    }
    for (index, client) in clients.iter_mut().enumerate() {
        client
            .key(if index == 0 {
                KeyCode::Enter
            } else {
                KeyCode::Escape
            })
            .unwrap();
        assert!(client.session.is_none());
        assert_eq!(client.menus.page, Page::Multiplayer);
        assert_eq!(
            client.menus.network_map,
            Some(temporary.canonicalize().unwrap())
        );
        assert_eq!(client.menus.address, address.to_string());
        assert!(client.menus.result.is_none());
    }
    // The completed host has released its listener, so hosting again is possible.
    let _listener = TcpListener::bind(address).unwrap();
    std::fs::remove_dir_all(temporary).unwrap();
}
