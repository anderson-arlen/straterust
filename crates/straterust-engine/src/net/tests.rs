use super::*;
use crate::{
    content::Package,
    sim::{EntityId, Order, Tick, UnitTypeId, ViewedEntity},
};
use std::{io::Write, sync::atomic::Ordering};

static NETWORK_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

mod growth;
mod lobby;
mod pacing;
#[path = "tests/reconnect.rs"]
mod recovery;
mod snapshots;

fn udp_pair() -> (datagram::Endpoint, datagram::Endpoint) {
    let a = Arc::new(std::net::UdpSocket::bind("127.0.0.1:0").unwrap());
    let b = Arc::new(std::net::UdpSocket::bind("127.0.0.1:0").unwrap());
    let (aa, bb) = (a.local_addr().unwrap(), b.local_addr().unwrap());
    let offer = datagram::Offer {
        port: aa.port(),
        token: [7; 16],
    };
    (
        datagram::Endpoint::new(a, offer.clone(), bb.ip(), Some(bb), Arc::default()).unwrap(),
        datagram::Endpoint::new(b, offer, aa.ip(), Some(aa), Arc::default()).unwrap(),
    )
}

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
    let mut replica = None;
    let mut udp: Option<datagram::Endpoint> = None;
    let mut tick = 0;
    while tick < 3 {
        if let Some(message) = udp
            .as_mut()
            .and_then(|u| u.poll::<ServerMessage>())
            .or_else(|| second.receive::<ServerMessage>().unwrap())
        {
            match message {
                ServerMessage::Welcome {
                    initial,
                    udp: offer,
                    ..
                } => {
                    replica = Some(replication::Receiver::new(&initial));
                    let mut remote = address;
                    remote.set_port(offer.port);
                    let mut endpoint =
                        datagram::Endpoint::client(remote, offer, Arc::default()).unwrap();
                    endpoint.send(&ClientMessage::Probe(0), false).unwrap();
                    udp = Some(endpoint);
                }
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
                ServerMessage::Delta(delta) => {
                    tick = replica
                        .as_mut()
                        .unwrap()
                        .apply(delta)
                        .unwrap()
                        .unwrap()
                        .view
                        .tick
                        .0
                }
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
        Command {
            tick: Tick(u64::MAX),
            player: PlayerId(1),
            sequence: 101,
            order: Order::Stop {
                entity: EntityId(4),
            },
        },
    ] {
        udp.as_mut()
            .unwrap()
            .send(
                &ClientMessage::Command(commands::Input {
                    command,
                    replace_orders: true,
                    after: None,
                }),
                false,
            )
            .unwrap();
    }
    let mut rejections = Vec::new();
    let mut accepted_stale_input = false;
    let mut accepted_future_input = false;
    while rejections.is_empty() || !accepted_stale_input || !accepted_future_input {
        let message = udp
            .as_mut()
            .and_then(|u| u.poll::<ServerMessage>())
            .or_else(|| second.receive().unwrap())
            .map(|message| match message {
                ServerMessage::Delta(delta) => {
                    ServerMessage::Update(replica.as_mut().unwrap().apply(delta).unwrap().unwrap())
                }
                other => other,
            });
        match message {
            Some(ServerMessage::Rejected(reason)) => rejections.push(reason),
            Some(ServerMessage::Update(update)) => {
                accepted_stale_input |= update.command_acks.contains(100);
                accepted_future_input |= update.outcomes.iter().any(|outcome| {
                    outcome.command.sequence == 101
                        && outcome.rejection.is_none()
                        && outcome.command.tick.0 >= tick
                        && outcome.command.tick.0 < u64::MAX
                });
            }
            _ => {}
        }
        let _ = first.poll().unwrap();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(rejections.iter().any(|r| r.contains("unauthorized")));
    assert_eq!(rejections.len(), 1);
    second.send(&ClientMessage::Leave).unwrap();
    drop(second);
    drop(stream);
    let until = Instant::now() + Duration::from_secs(2);
    loop {
        match first.poll().unwrap() {
            Some(ServerMessage::Notice(reason)) if reason.contains("disconnected") => break,
            Some(ServerMessage::End(reason)) => {
                panic!("a departing peer ended the match: {reason}")
            }
            _ => {}
        }
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(1));
    }
    first.leave();
    // The remaining raw UDP session expires after the normal reconnect grace.
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
fn udp_session_enforces_privacy_deduplicates_production_and_preserves_pause_and_replay() {
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
    let mut acknowledged = false;
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
                        acknowledged |= update.command_acks.contains(1);
                        if acknowledged {
                            let hall = update
                                .view
                                .entities
                                .iter()
                                .find_map(|e| match e {
                                    ViewedEntity::Owned(e) if e.id == EntityId(4) => Some(e),
                                    _ => None,
                                })
                                .unwrap();
                            assert_eq!(hall.production.len(), 1);
                            assert_eq!(update.view.economy.resources["minerals"], 0);
                        }
                    }
                    client.acknowledge(update.sequence).unwrap();
                    client.acknowledge_commands(&update.command_acks).unwrap();
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
            second
                .udp
                .send(
                    &ClientMessage::Command(commands::Input {
                        command,
                        replace_orders: true,
                        after: None,
                    }),
                    false,
                )
                .unwrap();
            sent = true;
        }
        if acknowledged && !pause_requested {
            first.pause(true).unwrap();
            pause_requested = true;
        }
        if paused {
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(acknowledged);
    assert!(paused);
    assert!(second.pause(false).is_err());
    stop.store(true, Ordering::Release);
    assert!(server.join().unwrap().is_err());
    let replay = load_replay(&replay_path).unwrap();
    assert_eq!(
        replay.ticks.iter().map(|t| t.commands.len()).sum::<usize>(),
        1
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
    let payload = ron::ser::to_string(&ClientMessage::Acknowledge {
        snapshot: 9,
        commands_retired: 0,
    })
    .unwrap();
    let mut bytes = (payload.len() as u32).to_le_bytes().to_vec();
    bytes.extend_from_slice(payload.as_bytes());
    for byte in &bytes[..bytes.len() - 1] {
        sender.write_all(&[*byte]).unwrap();
        assert!(receiver.receive::<ClientMessage>().unwrap().is_none());
    }
    sender.write_all(&bytes[bytes.len() - 1..]).unwrap();
    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        if let Some(ClientMessage::Acknowledge {
            snapshot: 9,
            commands_retired: 0,
        }) = receiver.receive().unwrap()
        {
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
        player_colors: Default::default(),
        terrain: crate::assets::encode_image(&image).unwrap(),
        grid: None,
        images: Vec::new(),
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
