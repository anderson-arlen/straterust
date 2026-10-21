use super::*;
use net::chat::{self, Channel, Message};

fn lobby_messages(clients: &mut [lobby::Client], marker: &str) -> Vec<Vec<Message>> {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut messages = vec![Vec::new(); clients.len()];
    let mut done = vec![false; clients.len()];
    while !done.iter().all(|v| *v) {
        for (i, client) in clients.iter_mut().enumerate() {
            while let Some(response) = client.poll().unwrap() {
                if let lobby::Response::Chat(message) = response {
                    done[i] |= message.text == marker;
                    messages[i].push(message);
                }
            }
        }
        assert!(Instant::now() < deadline, "lobby chat timed out");
        std::thread::sleep(Duration::from_millis(2));
    }
    messages
}
fn match_messages(clients: &mut [net::RemoteClient], marker: &str) -> Vec<Vec<Message>> {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut messages = vec![Vec::new(); clients.len()];
    let mut done = vec![false; clients.len()];
    while !done.iter().all(|v| *v) {
        for (i, client) in clients.iter_mut().enumerate() {
            while let Some(response) = client.poll().unwrap() {
                match response {
                    net::ServerMessage::Chat(message) => {
                        done[i] |= message.text == marker;
                        messages[i].push(message);
                    }
                    net::ServerMessage::Rejected(reason) => panic!("chat rejected: {reason}"),
                    _ => {}
                }
            }
        }
        assert!(Instant::now() < deadline, "match chat timed out");
        std::thread::sleep(Duration::from_millis(2));
    }
    messages
}
#[test]
fn named_lobby_and_paused_shared_control_chat_reach_only_intended_peers() {
    named_roundtrip(true);
    named_roundtrip(false);
}
fn named_roundtrip(shared: bool) {
    let (root, catalog) = fixture();
    let definitions = Package::client_definitions(&root.path().join("map")).unwrap();
    let identity = GameplayIdentity::of(&definitions);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let stopped = stop.clone();
    let directory = root.path().to_owned();
    let mut settings = catalog.defaults(0);
    settings.mode = if shared { 3 } else { 2 };
    for (i, slot) in settings.slots.iter_mut().enumerate() {
        slot.kind = if (shared && i < 4) || (!shared && [0, 1, 4, 5].contains(&i)) {
            SlotKind::Open
        } else {
            SlotKind::Closed
        };
        slot.team = (i % 2) as u8;
    }
    let server = std::thread::spawn(move || {
        let result = lobby::host(
            listener, directory, catalog, 0, settings, 42, stopped, None, None,
        );
        if let Err(error) = &result {
            eprintln!("chat test host ended: {error:#}");
        }
        result
    });
    let names = ["Arlen", "Enemy", "Ally", "Other enemy"];
    let mut waiting: Vec<_> = names
        .iter()
        .enumerate()
        .map(|(i, name)| lobby::Client::connect_named(address, &definitions, i == 0, name).unwrap())
        .collect();
    let state = poll_until(
        &mut waiting[0],
        |r| matches!(r,lobby::Response::State{view,..} if view.players.iter().flatten().count()==4),
    );
    if let lobby::Response::State { view, .. } = state {
        assert_eq!(
            view.players
                .iter()
                .flatten()
                .map(|p| p.name.as_str())
                .collect::<Vec<_>>(),
            names
        );
    }
    waiting[0]
        .send(lobby::Request::Name("Commander".into()))
        .unwrap();
    waiting[0]
        .send(lobby::Request::Chat {
            channel: Channel::All,
            text: "Hello everyone".into(),
        })
        .unwrap();
    waiting[0]
        .send(lobby::Request::Chat {
            channel: Channel::Team,
            text: "Secret lobby plan".into(),
        })
        .unwrap();
    waiting[0]
        .send(lobby::Request::Chat {
            channel: Channel::All,
            text: "lobby barrier".into(),
        })
        .unwrap();
    let received = lobby_messages(&mut waiting, "lobby barrier");
    for (i, messages) in received.iter().enumerate() {
        assert_eq!(
            messages
                .iter()
                .filter(|m| m.channel == Channel::Team)
                .count(),
            usize::from(if shared { i % 2 == 0 } else { i < 2 })
        );
        assert!(
            messages
                .iter()
                .all(|m| m.name == "Commander" && m.player == PlayerId(0))
        );
        assert_eq!(
            messages
                .iter()
                .filter(|m| m.channel == Channel::All)
                .count(),
            2
        );
    }
    waiting[3]
        .send(lobby::Request::Name("bad\nname".into()))
        .unwrap();
    poll_until(
        &mut waiting[3],
        |r| matches!(r,lobby::Response::Rejected(reason) if reason.contains("control")),
    );
    for client in &mut waiting {
        client.send(lobby::Request::Ready(true)).unwrap();
    }
    poll_until(
        &mut waiting[0],
        |r| matches!(r,lobby::Response::State{view,..} if view.players.iter().flatten().all(|p|p.ready)),
    );
    waiting[0].send(lobby::Request::Start).unwrap();
    let mut assigned = Vec::new();
    for client in &mut waiting {
        if let lobby::Response::Launch { player, .. } =
            poll_until(client, |r| matches!(r, lobby::Response::Launch { .. }))
        {
            assigned.push(player);
        }
    }
    if shared {
        assert_eq!(assigned[0], assigned[2]);
    }
    assert_ne!(assigned[0], assigned[1]);
    drop(waiting);
    let mut clients: Vec<_> = assigned
        .into_iter()
        .enumerate()
        .map(|(i, player)| {
            let (mut client, _) =
                net::RemoteClient::connect(address, &definitions, player).unwrap();
            client
                .set_name(if i == 0 { "Commander" } else { names[i] })
                .unwrap();
            client
        })
        .collect();
    clients[0].pause(true).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut paused = [false; 4];
    while !paused.iter().all(|v| *v) {
        for (i, client) in clients.iter_mut().enumerate() {
            while let Some(message) = client.poll().unwrap() {
                if matches!(
                    message,
                    net::ServerMessage::Status {
                        started: true,
                        paused: true
                    }
                ) {
                    paused[i] = true;
                }
            }
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    let ally = if shared { 2 } else { 1 };
    clients[ally]
        .chat(Channel::Team, "Secret match plan")
        .unwrap();
    clients[ally].chat(Channel::All, "match barrier").unwrap();
    let received = match_messages(&mut clients, "match barrier");
    for (i, messages) in received.iter().enumerate() {
        assert_eq!(
            messages
                .iter()
                .filter(|m| m.channel == Channel::Team)
                .count(),
            usize::from(if shared { i % 2 == 0 } else { i < 2 })
        );
        assert!(
            messages
                .iter()
                .all(|m| m.name == names[ally] && m.player == clients[ally].handshake.player)
        );
        assert_eq!(
            messages
                .iter()
                .filter(|m| m.channel == Channel::All)
                .count(),
            1
        );
    }
    assert_eq!(GameplayIdentity::of(&definitions), identity);
    stop.store(true, Ordering::Release);
    drop(clients);
    let _ = server.join().unwrap();
}

#[test]
fn chat_validation_and_bounded_traffic_do_not_break_lobby_readiness() {
    for invalid in ["", "   ", "a\nb", "a\0b", &"a".repeat(25)] {
        assert!(chat::name(invalid).is_err());
    }
    assert_eq!(chat::name("  Pilot  ").unwrap(), "Pilot");
    assert!(chat::text(&"a".repeat(257)).is_err());
    assert!(chat::text(&"é".repeat(256)).is_ok());
    assert!(chat::text("evil\rline").is_err());
    // The actual socket host rejects invalid text and throttles social traffic,
    // while allowing readiness and later valid input to continue normally.
    let (root, catalog) = fixture();
    let definitions = Package::client_definitions(&root.path().join("map")).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let stopped = stop.clone();
    let directory = root.path().to_owned();
    let settings = catalog.defaults(0);
    let server = std::thread::spawn(move || {
        lobby::host(
            listener, directory, catalog, 0, settings, 42, stopped, None, None,
        )
    });
    let mut client = lobby::Client::connect_named(address, &definitions, true, "Pilot").unwrap();
    poll_until(&mut client, |r| matches!(r, lobby::Response::State { .. }));
    client
        .send(lobby::Request::Chat {
            channel: Channel::All,
            text: "bad\nmessage".into(),
        })
        .unwrap();
    poll_until(
        &mut client,
        |r| matches!(r,lobby::Response::Rejected(reason) if reason.contains("control")),
    );
    for i in 0..5 {
        client
            .send(lobby::Request::Chat {
                channel: Channel::All,
                text: format!("burst {i}"),
            })
            .unwrap();
    }
    poll_until(
        &mut client,
        |r| matches!(r,lobby::Response::Rejected(reason) if reason.contains("quickly")),
    );
    client.send(lobby::Request::Ready(true)).unwrap();
    poll_until(
        &mut client,
        |r| matches!(r,lobby::Response::State{view,..} if view.players[0].as_ref().unwrap().ready),
    );
    stop.store(true, Ordering::Release);
    drop(client);
    let _ = server.join().unwrap();
}
