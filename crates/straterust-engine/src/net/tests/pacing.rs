//! Snapshot rejection, cumulative ACK pacing and disconnect grace are separate.
use super::*;
use crate::sim::GameplayIdentity;

fn pair() -> (RemoteClient, Connection, datagram::Endpoint, PlayerUpdate) {
    let world = definitions();
    let mut server = ServerSession::new(world.clone(), 42, vec![PlayerId(0), PlayerId(1)]).unwrap();
    let (handshake, initial) = server
        .connect(&Hello {
            protocol: PROTOCOL_VERSION,
            identity: GameplayIdentity::of(&world),
            player: PlayerId(1),
            expected_map: None,
        })
        .unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let outgoing = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (incoming, _) = listener.accept().unwrap();
    let mut clock = clock::ClockSync::default();
    clock.observe(
        clock::Pong {
            echo_ms: 0,
            received_ms: 10_000,
            sent_ms: 10_000,
        },
        0,
    );
    let (udp, remote) = udp_pair();
    let datagram_offer = udp.offer.clone();
    (
        RemoteClient {
            connection: Connection::new(incoming).unwrap(),
            handshake,
            map: MapTransfer::of(&world).unwrap(),
            sequence: initial.sequence,
            applied: initial.sequence,
            acknowledged: initial.sequence,
            inputs: Default::default(),
            udp,
            datagram_offer,
            replica: replication::Receiver::new(&initial),
            last_heartbeat: Instant::now(),
            reconnect: reconnect::Reconnect::new(listener.local_addr().unwrap()),
            clock: Arc::new(std::sync::Mutex::new(clock)),
        },
        Connection::new(outgoing).unwrap(),
        remote,
        initial,
    )
}

#[test]
fn rejected_old_snapshots_are_not_applied_or_acknowledged_and_changes_arrive_in_a_newer_one() {
    let (mut client, mut server, _, initial) = pair();
    let mut sender = replication::Sender::new(initial.clone());
    let mut old = initial.clone();
    old.sequence += 1;
    old.view.tick.0 += 1;
    old.view.resources[0].amount -= 20;
    old.time = Some(clock::FrameTime {
        captured_ms: 9000,
        tick_ms: 25,
    });
    sender.observe(old.clone()).unwrap();
    server
        .send(&ServerMessage::Delta(sender.packet().unwrap()))
        .unwrap();
    assert!(client.poll().unwrap().is_none());
    assert_eq!(client.sequence, initial.sequence);
    assert_eq!(client.applied, initial.sequence);
    let mut fresh = old;
    fresh.sequence += 1;
    fresh.view.tick.0 += 1;
    fresh.time.as_mut().unwrap().captured_ms = 10_000;
    sender.observe(fresh.clone()).unwrap();
    let packet = sender.packet().unwrap();
    assert!(
        !packet.resources.is_empty(),
        "unacknowledged changes must still be sent"
    );
    server.send(&ServerMessage::Delta(packet)).unwrap();
    let Some(ServerMessage::Update(update)) = client.poll().unwrap() else {
        panic!("fresh snapshot missing")
    };
    assert_eq!(update.view.resources, fresh.view.resources);
    assert_eq!(
        client.applied, initial.sequence,
        "decoding is not acceptance by the window"
    );
    client.acknowledge(update.sequence).unwrap();
    assert_eq!(client.applied, update.sequence);
}

#[test]
fn authoritative_host_accepts_new_state_despite_old_future_or_unsynchronized_clock() {
    for captured_ms in [9000, 11000] {
        let (mut client, mut server, mut remote, initial) = pair();
        client.handshake.host = true;
        let mut sender = replication::Sender::new(initial.clone());
        let mut update = initial.clone();
        update.sequence += 1;
        update.view.tick.0 += 1;
        update.view.resources[0].amount -= 20;
        update.time = Some(clock::FrameTime {
            captured_ms,
            tick_ms: 25,
        });
        assert!(
            !client
                .clock
                .lock()
                .unwrap()
                .fresh(update.time.unwrap(), 0, 250)
        );
        sender.observe(update.clone()).unwrap();
        server
            .send(&ServerMessage::Delta(sender.packet().unwrap()))
            .unwrap();
        let Some(ServerMessage::Update(received)) = client.poll().unwrap() else {
            panic!("authoritative host discarded its newest state");
        };
        assert_eq!(received.view.resources, update.view.resources);
        client.acknowledge(received.sequence).unwrap();
        assert_eq!(client.applied, update.sequence);
        client.last_heartbeat = Instant::now() - Duration::from_millis(clock::UPDATE_MS);
        client.flush_acknowledgement().unwrap();
        let ack = (0..64)
            .find_map(|_| match remote.poll::<ClientMessage>() {
                Some(ClientMessage::Acknowledge { snapshot, .. }) => Some(snapshot),
                _ => None,
            })
            .expect("host never sent its applied ACK");
        sender.acknowledge(ack).unwrap();

        *client.clock.lock().unwrap() = clock::ClockSync::default();
        update.sequence += 1;
        update.view.tick.0 += 1;
        sender.observe(update.clone()).unwrap();
        let next = sender.packet().unwrap();
        assert!(
            next.resources.is_empty(),
            "host ACK did not clear unchanged resources"
        );
        server.send(&ServerMessage::Delta(next)).unwrap();
        assert!(matches!(
            client.poll().unwrap(),
            Some(ServerMessage::Update(_))
        ));
    }
}

#[test]
fn a_burst_needs_only_one_cumulative_ack_and_no_additional_ack_while_unchanged() {
    let (mut client, _, mut server, initial) = pair();
    client.sequence = initial.sequence + 20;
    for sequence in initial.sequence + 1..=client.sequence {
        client.acknowledge(sequence).unwrap();
    }
    assert!(server.poll::<ClientMessage>().is_none());
    client.last_heartbeat = Instant::now() - Duration::from_millis(clock::UPDATE_MS);
    client.flush_acknowledgement().unwrap();
    assert!(matches!(
        server.poll::<ClientMessage>(),
        Some(ClientMessage::Acknowledge { snapshot: 20, .. })
    ));
    client.flush_acknowledgement().unwrap();
    assert!(server.poll::<ClientMessage>().is_none());
}

#[test]
fn silence_has_thirty_seconds_grace_and_buffered_traffic_is_drained_before_timeout() {
    let (mut client, _tcp, mut server, _) = pair();
    assert_eq!(PEER_TIMEOUT, Duration::from_secs(30));
    client.connection.last_received = Instant::now() - Duration::from_secs(29);
    client.udp.last_received = client.connection.last_received;
    assert!(client.poll().unwrap().is_none());
    client.connection.last_received = Instant::now() - Duration::from_secs(31);
    client.udp.last_received = client.connection.last_received;
    server
        .send(
            &ServerMessage::Status {
                started: true,
                paused: false,
            },
            false,
        )
        .unwrap();
    assert!(matches!(
        client.poll().unwrap(),
        Some(ServerMessage::Status { started: true, .. })
    ));
    client.connection.last_received = Instant::now() - Duration::from_secs(31);
    client.udp.last_received = client.connection.last_received;
    assert!(
        client
            .poll()
            .unwrap_err()
            .to_string()
            .contains("host timed out")
    );
}
