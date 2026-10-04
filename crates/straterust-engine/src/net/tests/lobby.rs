use super::*;

#[test]
fn lobby_keepalives_allow_a_late_guest_and_a_long_pause() {
    let _guard = NETWORK_LOCK.lock().unwrap();
    let world = definitions();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let cancelled = Arc::clone(&stop);
    let server_world = world.snapshot();
    let server = std::thread::spawn(move || run_host(listener, server_world, 42, cancelled, None));
    let (mut host, _) = RemoteClient::connect(address, &world, PlayerId(0)).unwrap();

    // A healthy waiting lobby must outlive the deadline for an unresponsive peer.
    let deadline = Instant::now() + PEER_TIMEOUT + Duration::from_millis(500);
    while Instant::now() < deadline {
        while let Some(message) = host.poll().expect("waiting host stays connected") {
            assert!(matches!(
                message,
                ServerMessage::Status { started: false, .. }
            ));
        }
        std::thread::sleep(Duration::from_millis(5));
    }

    let (mut guest, _) = RemoteClient::connect(address, &world, PlayerId(1)).unwrap();
    host.pause(true).unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut paused = [false; 2];
    while !paused.iter().all(|p| *p) {
        for (index, client) in [&mut host, &mut guest].into_iter().enumerate() {
            while let Some(message) = client.poll().unwrap() {
                match message {
                    ServerMessage::Status {
                        started: true,
                        paused: true,
                    } => paused[index] = true,
                    ServerMessage::Status { .. } | ServerMessage::Update(_) => {}
                    message => panic!("unexpected lobby message: {message:?}"),
                }
            }
        }
        assert!(Instant::now() < deadline, "both clients receive host pause");
        std::thread::sleep(Duration::from_millis(5));
    }

    let deadline = Instant::now() + PEER_TIMEOUT + Duration::from_millis(500);
    while Instant::now() < deadline {
        for client in [&mut host, &mut guest] {
            while let Some(message) = client.poll().expect("paused peers stay connected") {
                assert!(matches!(
                    message,
                    ServerMessage::Status {
                        started: true,
                        paused: true
                    }
                ));
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    host.pause(false).unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if matches!(host.poll().unwrap(), Some(ServerMessage::Update(_))) {
            break;
        }
        guest.poll().unwrap();
        assert!(
            Instant::now() < deadline,
            "simulation resumes after the long pause"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    stop.store(true, Ordering::Release);
    assert!(server.join().unwrap().is_err());
}
