use super::*;
use crate::{
    content::Package,
    sim::{EntityId, Order, Tick, UnitTypeId, ViewedEntity},
};
use std::{io::Write, sync::atomic::Ordering};

static NETWORK_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

mod lobby;

#[test]
fn delayed_fragmented_handshake_rejects_bad_inputs_and_reports_disconnect() {
    let _guard = NETWORK_LOCK.lock().unwrap();
    let world = definitions();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let server_world = world.snapshot();
    let server = std::thread::spawn(move || run_host(listener, server_world, 42, stop, None));
    // A rejected outsider cannot terminate the host or occupy a human slot.
    let mut invalid = world.rules().clone();
    invalid.tick_ms += 1;
    let incompatible = World::new(invalid, world.map().clone(), 42).unwrap();
    assert!(
        RemoteClient::connect(address, &incompatible, PlayerId(1))
            .err()
            .unwrap()
            .to_string()
            .contains("mismatch")
    );
    let mut outsider = Connection::new(TcpStream::connect(address).unwrap()).unwrap();
    outsider.send(&ClientMessage::Pause(true)).unwrap();
    drop(outsider);
    let (mut first, _) = RemoteClient::connect(address, &world, PlayerId(0)).unwrap();
    let mut stream = TcpStream::connect(address).unwrap();
    let mut second = Connection::new(stream.try_clone().unwrap()).unwrap();
    let hello = ClientMessage::Hello(Hello {
        protocol: PROTOCOL_VERSION,
        identity: GameplayIdentity::of(&world),
        player: PlayerId(1),
        expected_map: None,
    });
    let payload = ron::ser::to_string(&hello).unwrap();
    let mut frame = (payload.len() as u32).to_le_bytes().to_vec();
    frame.extend_from_slice(payload.as_bytes());
    for (index, part) in frame.chunks(23).enumerate() {
        stream.write_all(part).unwrap();
        std::thread::sleep(Duration::from_millis([1, 4, 2, 0][index % 4]));
    }
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut download = None;
    let mut tick = 0;
    while tick < 3 {
        if let Some(message) = second.receive::<ServerMessage>().unwrap() {
            match message {
                ServerMessage::MapBegin { length, hash } => {
                    download = Some(Download::new(length, hash).unwrap())
                }
                ServerMessage::MapChunk { offset, data } => {
                    let map = download.as_mut().unwrap();
                    if map.push(offset, &data).unwrap().is_some() {
                        second
                            .send(&ClientMessage::MapReady(map.hash().into()))
                            .unwrap();
                    }
                }
                ServerMessage::Update(update) => tick = update.view.tick.0,
                _ => {}
            }
        }
        let _ = first.poll().unwrap();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    for command in [
        Command {
            tick: Tick(tick + 20),
            player: PlayerId(0),
            sequence: 99,
            order: Order::Stop {
                entity: EntityId(1),
            },
        },
        Command {
            tick: Tick(0),
            player: PlayerId(1),
            sequence: 100,
            order: Order::Stop {
                entity: EntityId(4),
            },
        },
    ] {
        second.send(&ClientMessage::Command(command)).unwrap();
    }
    let mut rejections = Vec::new();
    while rejections.len() < 2 {
        if let Some(ServerMessage::Rejected(reason)) = second.receive().unwrap() {
            rejections.push(reason);
        }
        let _ = first.poll().unwrap();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(rejections.iter().any(|r| r.contains("unauthorized")));
    assert!(rejections.iter().any(|r| r.contains("late")));
    drop(second);
    drop(stream);
    loop {
        match first.poll() {
            Ok(Some(ServerMessage::End(reason))) => {
                assert!(reason.contains("connection"));
                break;
            }
            Err(error) => panic!("missing graceful disconnect reason: {error}"),
            _ => {}
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(server.join().unwrap().is_err());
}

#[test]
fn lan_multicast_finds_custom_maps_with_matching_installed_rules() {
    let _guard = NETWORK_LOCK.lock().unwrap();
    let world = definitions();
    let advertiser =
        lan::Advertiser::new("custom-map", 6122, GameplayIdentity::of(&world)).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let cancelled = Arc::clone(&stop);
    let thread = std::thread::spawn(move || {
        while !cancelled.load(Ordering::Acquire) {
            advertiser.poll().unwrap();
            std::thread::sleep(Duration::from_millis(2));
        }
    });
    let mut installed = GameplayIdentity::of(&world);
    installed.map = "a different local map".into();
    let result = lan::discover(&installed, Duration::from_millis(400)).unwrap();
    stop.store(true, Ordering::Release);
    thread.join().unwrap();
    assert!(
        result.iter().any(|game| game.name == "custom-map"
            && game.address.port() == 6122
            && game.compatible)
    );
}

fn definitions() -> World {
    let package =
        Package::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/lan-demo"))
            .unwrap();
    let world = package.world(42).unwrap();
    let mut rules = world.rules().clone();
    rules.tick_ms = 25;
    World::new(rules, world.map().clone(), 42).unwrap()
}

#[test]
fn tcp_session_enforces_privacy_sequences_pause_and_replay() {
    let _guard = NETWORK_LOCK.lock().unwrap();
    let world = definitions();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let cancelled = Arc::clone(&stop);
    let server_world = world.snapshot();
    let replay_path = std::path::PathBuf::from("/tmp/straterust-network-replay.ron");
    let recording = replay_path.clone();
    let server = std::thread::spawn(move || {
        run_host(listener, server_world, 42, cancelled, Some(&recording))
    });
    let (mut first, initial0) = RemoteClient::connect(address, &world, PlayerId(0)).unwrap();
    let (mut second, initial1) = RemoteClient::connect(address, &world, PlayerId(1)).unwrap();
    assert!(
        initial0
            .view
            .entities
            .iter()
            .all(|e| matches!(e, ViewedEntity::Owned(e) if e.owner == PlayerId(0)))
    );
    assert!(
        initial1
            .view
            .entities
            .iter()
            .all(|e| matches!(e, ViewedEntity::Owned(e) if e.owner == PlayerId(1)))
    );
    assert_eq!(initial0.view.home.unwrap().x, 384);
    assert_eq!(initial1.view.home.unwrap().x, 1120);
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut last_tick = 0;
    let mut sent = false;
    let mut duplicates = 0;
    let mut paused = false;
    let mut pause_requested = false;
    while Instant::now() < deadline {
        for (player, client) in [(PlayerId(0), &mut first), (PlayerId(1), &mut second)] {
            for _ in 0..64 {
                let Some(message) = client.poll().unwrap() else {
                    break;
                };
                if let ServerMessage::Update(update) = message {
                    assert!(update.outcomes.iter().all(|o| o.command.player == player));
                    let wire = ron::ser::to_string(&update).unwrap();
                    for secret in [
                        "rng_state",
                        "last_sequences",
                        "triggers",
                        "switches",
                        "accepted_orders",
                    ] {
                        assert!(!wire.contains(secret), "private wire field {secret}");
                    }
                    if player == PlayerId(1) {
                        last_tick = update.view.tick.0;
                        duplicates += update
                            .outcomes
                            .iter()
                            .filter(|o| {
                                o.rejection == Some(crate::sim::Rejection::DuplicateSequence)
                            })
                            .count();
                    }
                } else if let ServerMessage::Status { paused: true, .. } = message {
                    paused = true;
                } else if let ServerMessage::Rejected(reason) = message {
                    panic!("unexpected envelope rejection: {reason}");
                }
            }
        }
        if !sent && last_tick > 0 {
            let command = Command {
                tick: Tick(last_tick + 20),
                player: PlayerId(1),
                sequence: 1,
                order: Order::Train {
                    entity: EntityId(4),
                    unit_type: UnitTypeId(2),
                },
            };
            second.command(command.clone()).unwrap();
            second.command(command).unwrap();
            sent = true;
        }
        if duplicates == 2 && !pause_requested {
            first.pause(true).unwrap();
            pause_requested = true;
        }
        if paused {
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(duplicates, 2);
    assert!(paused);
    assert!(second.pause(false).is_err());
    stop.store(true, Ordering::Release);
    assert!(server.join().unwrap().is_err());
    let replay = load_replay(&replay_path).unwrap();
    assert_eq!(
        replay.ticks.iter().map(|t| t.commands.len()).sum::<usize>(),
        2
    );
    let restored = replay.play(&world).unwrap();
    assert_eq!(
        restored.state_hash().to_hex().as_str(),
        replay.ticks.last().unwrap().hash
    );
}

#[test]
fn framing_handles_fragmented_messages_and_rejects_oversized_lengths() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let mut sender = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let mut receiver = Connection::new(listener.accept().unwrap().0).unwrap();
    let payload = ron::ser::to_string(&ClientMessage::Acknowledge(9)).unwrap();
    let mut bytes = (payload.len() as u32).to_le_bytes().to_vec();
    bytes.extend_from_slice(payload.as_bytes());
    for byte in &bytes[..bytes.len() - 1] {
        sender.write_all(&[*byte]).unwrap();
        assert!(receiver.receive::<ClientMessage>().unwrap().is_none());
    }
    sender.write_all(&bytes[bytes.len() - 1..]).unwrap();
    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        if let Some(ClientMessage::Acknowledge(9)) = receiver.receive().unwrap() {
            break;
        }
        assert!(Instant::now() < deadline);
    }
    sender
        .write_all(&(framing::MAX_FRAME_BYTES as u32 + 1).to_le_bytes())
        .unwrap();
    loop {
        match receiver.receive::<ClientMessage>() {
            Err(error) => {
                assert!(error.to_string().contains("length"));
                break;
            }
            Ok(None) => {}
            Ok(Some(_)) => panic!("accepted oversized frame"),
        }
        assert!(Instant::now() < deadline);
    }
}

#[test]
fn downloaded_maps_are_public_bounded_data_and_reject_code_fields() {
    use map_transfer::*;
    let world = definitions();
    let map = MapTransfer::of(&world).unwrap();
    let bytes = map.encode().unwrap();
    let text = std::str::from_utf8(&bytes).unwrap();
    for forbidden in [
        "spawns",
        "triggers",
        "locations",
        "initial_explored",
        "scripts",
        "executable",
    ] {
        assert!(!text.contains(forbidden));
    }
    let hash = blake3::hash(&bytes).to_hex().to_string();
    let mut download = Download::new(bytes.len(), hash.clone()).unwrap();
    assert!(download.push(1, &[0]).is_err());
    let mut received = None;
    for (i, chunk) in bytes.chunks(7).enumerate() {
        received = download.push(i * 7, chunk).unwrap();
    }
    let received = received.unwrap();
    let client = received.definitions(&world, PlayerId(1)).unwrap();
    assert!(
        client.is_player_view() && client.map().spawns.is_empty() && client.map().mission.is_none()
    );
    assert!(client.state().entities.is_empty());
    assert_eq!(GameplayIdentity::of(&client), GameplayIdentity::of(&world));
    let with_code = text.replacen("version:1", "version:1,script:\"evil\"", 1);
    assert!(ron::from_str::<MapTransfer>(&with_code).is_err());
    assert!(Download::new(MAX_MAP_BYTES + 1, hash.clone()).is_err());
    let mut corrupt = bytes.clone();
    corrupt[0] ^= 1;
    assert!(
        Download::new(bytes.len(), hash)
            .unwrap()
            .push(0, &corrupt)
            .is_err()
    );
    let mut invalid = received.clone();
    invalid.map.width = i32::MAX;
    assert!(invalid.definitions(&world, PlayerId(1)).is_err());
    invalid = received;
    invalid.identity.rules = "different".into();
    assert!(invalid.definitions(&world, PlayerId(1)).is_err());
    let image = crate::assets::Image {
        width: 1,
        height: 1,
        rgba: vec![12, 34, 56, 255],
    };
    let mut with_art = MapTransfer::of(&world).unwrap();
    with_art.artwork = Some(crate::assets::MapArtwork {
        terrain: crate::assets::encode_image(&image).unwrap(),
        grid: None,
        decorations: Vec::new(),
    });
    let decoded = with_art
        .artwork
        .as_ref()
        .unwrap()
        .decode(&with_art.definitions(&world, PlayerId(0)).unwrap())
        .unwrap();
    assert_eq!(decoded.terrain, image);
    with_art.artwork.as_mut().unwrap().terrain = b"MZ executable bytes".to_vec();
    assert!(with_art.definitions(&world, PlayerId(0)).is_err());
}
