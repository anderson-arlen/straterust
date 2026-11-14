use super::*;

fn wait_for_status(guest: &mut RemoteClient, paused: bool, deadline: Instant) {
    loop {
        match guest.poll().unwrap() {
            Some(ServerMessage::Status {
                started: true,
                paused: value,
            }) if value == paused => return,
            Some(ServerMessage::Update(update)) => guest.acknowledge(update.sequence).unwrap(),
            Some(ServerMessage::End(reason)) => panic!("match ended: {reason}"),
            _ => {}
        }
        assert!(
            Instant::now() < deadline,
            "host stopped processing pause requests"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn background_pause_statuses_cannot_block_heartbeats_or_resuming() {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/lan-demo");
    let reservation = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = reservation.local_addr().unwrap();
    drop(reservation);
    let mut app = App::load_network(
        &directory,
        Config {
            audio: false,
            ..Config::default()
        },
    )
    .unwrap();
    let definitions = app.initial_world.clone();
    app.start_network(&directory, address, true, None).unwrap();
    let (mut guest, _) = RemoteClient::connect(address, &definitions, PlayerId(1)).unwrap();
    let deadline = Instant::now() + Duration::from_secs(8);

    // Leave the host window completely unpolled. Accelerate more control state
    // deliveries than the old 64-event channel could hold. Alternating values
    // proves each request reached the server, rather than observing old keepalives.
    for i in 0..160 {
        let paused = i % 2 == 0;
        app.network.as_ref().unwrap().pause(paused).unwrap();
        wait_for_status(&mut guest, paused, deadline);
    }
    app.network.as_ref().unwrap().pause(true).unwrap();
    wait_for_status(&mut guest, true, deadline);
    while !app.network.as_ref().unwrap().started || !app.paused {
        app.advance_network().unwrap();
        assert!(
            Instant::now() < deadline,
            "window did not receive the current pause state"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    let paused_tick = app.world.tick();
    app.bound_key(winit::keyboard::KeyCode::Space).unwrap();
    loop {
        app.advance_network().unwrap();
        if let Some(ServerMessage::Update(update)) = guest.poll().unwrap() {
            guest.acknowledge(update.sequence).unwrap();
        }
        if !app.paused && app.world.tick() > paused_tick {
            break;
        }
        assert!(!app.network.as_ref().unwrap().ended, "{}", app.status);
        assert!(
            Instant::now() < deadline,
            "unpause did not resume gameplay: {}",
            app.status
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    guest.leave();
    app.network.take().unwrap().finish().unwrap();
}

#[test]
fn closed_input_channel_is_reported_as_disconnected_instead_of_full() {
    let (sender, receiver) = mpsc::sync_channel(1);
    send_request(&sender, Request::Pause(true), "network input queue full").unwrap();
    assert_eq!(
        send_request(&sender, Request::Pause(false), "network input queue full")
            .unwrap_err()
            .to_string(),
        "network input queue full"
    );
    drop(receiver);
    assert_eq!(
        send_request(&sender, Request::Pause(false), "network input queue full")
            .unwrap_err()
            .to_string(),
        "network worker disconnected"
    );
}
