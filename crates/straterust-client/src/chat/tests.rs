use super::*;

#[test]
fn window_workers_deliver_named_chat_and_preserve_paused_gameplay() {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/lan-demo");
    let reservation = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = reservation.local_addr().unwrap();
    drop(reservation);
    let mut apps = [("Alice", true), ("Bob", false)].map(|(name, hosting)| {
        let mut app = App::load_network(
            &directory,
            Config {
                player_name: name.into(),
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
    while apps.iter().any(|a| !a.network.as_ref().unwrap().started) {
        for app in &mut apps {
            app.advance_network().unwrap();
        }
        assert!(Instant::now() < deadline, "clients did not start");
        std::thread::sleep(Duration::from_millis(2));
    }
    apps[0].network.as_ref().unwrap().pause(true).unwrap();
    while apps.iter().any(|a| !a.paused) {
        for app in &mut apps {
            app.advance_network().unwrap();
        }
        assert!(Instant::now() < deadline, "clients did not pause");
        std::thread::sleep(Duration::from_millis(2));
    }
    let hashes = apps.each_ref().map(|a| a.world.state_hash());
    apps[1]
        .network
        .as_ref()
        .unwrap()
        .chat(Channel::All, "Hello Alice")
        .unwrap();
    apps[1]
        .network
        .as_ref()
        .unwrap()
        .chat(Channel::Team, "private echo with no allies")
        .unwrap();
    apps[1]
        .network
        .as_ref()
        .unwrap()
        .chat(Channel::All, "barrier")
        .unwrap();
    while apps
        .iter()
        .any(|a| !a.chat.history.iter().any(|(_, m)| m.text == "barrier"))
    {
        for app in &mut apps {
            app.advance_network().unwrap();
        }
        assert!(Instant::now() < deadline, "chat did not reach the windows");
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(apps[0].chat.history.len(), 2);
    assert_eq!(apps[1].chat.history.len(), 3);
    for (i, app) in apps.iter().enumerate() {
        assert!(
            app.chat
                .history
                .iter()
                .all(|(_, m)| m.name == "Bob" && m.player == PlayerId(1))
        );
        assert_eq!(app.world.state_hash(), hashes[i]);
        assert!(app.recorded.is_empty());
    }
    for app in &mut apps {
        app.network.take().unwrap().finish().unwrap();
    }
}

#[test]
fn composer_captures_orders_channels_repeats_and_cancellation() {
    let mut chat = Chat::default();
    assert!(!chat.edit(KeyCode::KeyA, Some("a"), false, true));
    assert!(chat.edit(KeyCode::Enter, None, false, true));
    for code in [KeyCode::KeyA, KeyCode::Space, KeyCode::F5, KeyCode::ArrowUp] {
        assert!(chat.edit(code, Some("x"), false, true));
    }
    assert!(chat.open);
    chat.edit(KeyCode::Tab, None, false, true);
    assert_eq!(chat.channel, Channel::Team);
    chat.edit(KeyCode::Tab, None, true, true);
    assert_eq!(chat.channel, Channel::Team);
    chat.edit(KeyCode::Tab, None, false, false);
    assert_eq!(chat.channel, Channel::All);
    chat.edit(KeyCode::Backspace, None, true, true);
    assert_eq!(chat.draft, "xxx");
    chat.edit(KeyCode::Escape, None, false, true);
    assert!(!chat.open);
    assert!(chat.draft.is_empty());
    assert!(!chat.edit(KeyCode::Enter, None, true, true));
    append(&mut chat.draft, &"é".repeat(300), chat::TEXT_LIMIT);
    assert_eq!(chat.draft.chars().count(), 256);
    assert!(chat::text(&chat.draft).is_ok());
    assert!(!chat.edit(KeyCode::Backspace, None, false, true));
}

#[test]
fn history_is_bounded_and_rejects_invalid_wire_text() {
    let mut chat = Chat::default();
    for i in 0..70 {
        chat.receive(Message {
            player: PlayerId(0),
            name: "Arlen".into(),
            channel: Channel::All,
            text: format!("message {i}"),
        })
        .unwrap();
    }
    assert_eq!(chat.history.len(), 64);
    assert_eq!(chat.history.front().unwrap().1.text, "message 6");
    assert!(
        chat.receive(Message {
            player: PlayerId(0),
            name: "Arlen".into(),
            channel: Channel::All,
            text: "forged\nsecond author".into()
        })
        .is_err()
    );
    chat.begin();
    chat.edit(KeyCode::PageUp, None, false, true);
    assert_eq!(chat.scroll, 4);
    chat.edit(KeyCode::PageDown, None, false, true);
    assert_eq!(chat.scroll, 0);
}
