use super::*;
use crate::sim::{ContainerShot, WeaponFeedback};

#[test]
fn growing_snapshots_finish_fragmenting_while_new_ticks_keep_arriving() {
    for host in [true, false] {
        let mut server =
            ServerSession::new(definitions(), 42, vec![PlayerId(0), PlayerId(1)]).unwrap();
        let initial = server.update(PlayerId(0), &[]).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let _receiver = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (udp, mut reader) = udp_pair();
        let mut peer = Peer {
            connection: Connection::new(listener.accept().unwrap().0).unwrap(),
            player: Some(PlayerId(0)),
            hello: None,
            map_offset: None,
            name: "host".into(),
            chat_gate: chat::Gate::default(),
            replication: Some(replication::Sender::new(initial.clone())),
            commands: Default::default(),
            udp,
            lost: None,
            ended: None,
            host,
            slot: None,
        };
        let base = initial
            .view
            .entities
            .iter()
            .find_map(|e| match e {
                ViewedEntity::Owned(e) => Some(e.clone()),
                _ => None,
            })
            .unwrap();
        let mut large = initial.clone();
        let mut random = 42_u64;
        large.view.entities = (1..=512)
            .map(|id| {
                let mut entity = base.clone();
                entity.id = EntityId(id);
                entity.path = (0..128)
                    .map(|_| {
                        random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
                        crate::sim::Position {
                            x: ((random >> 32) % 1000) as i32,
                            y: ((random >> 48) % 1000) as i32,
                        }
                    })
                    .collect();
                ViewedEntity::Owned(entity)
            })
            .collect();
        let mut delivered = None;
        for sequence in 1..=32 {
            large.sequence = sequence;
            large.view.tick = Tick(sequence);
            peer.replication
                .as_mut()
                .unwrap()
                .observe(large.clone())
                .unwrap();
            peer.flush_update(!host).unwrap();
            assert!(
                peer.udp.last_snapshot_bytes > 128 * 1200,
                "fixture needs more fragments than two flush budgets"
            );
            if let Some(ServerMessage::Delta(update)) = reader.poll() {
                delivered = Some(update);
                break;
            }
        }
        let update = delivered
            .expect("every newer tick restarted the partial snapshot; no complete frame arrived");
        peer.replication
            .as_mut()
            .unwrap()
            .acknowledge(update.sequence)
            .unwrap();
        assert_eq!(update.entities.len(), 512);
        if let Some(next) = peer.replication.as_mut().unwrap().repeat() {
            assert!(
                next.entities.is_empty(),
                "ACK must retire the large unchanged unit paths"
            );
        }
    }
}

#[test]
fn snapshot_replacement_preserves_feedback_without_preserving_old_state() {
    let world = definitions();
    let mut session = ServerSession::new(world, 42, vec![PlayerId(0), PlayerId(1)]).unwrap();
    let mut old = session.update(PlayerId(0), &[]).unwrap();
    old.view.removed.push(EntityId(2));
    old.view.shots.push(ContainerShot {
        container: EntityId(1),
        weapon: UnitTypeId(1),
        heading: [0, 128],
        targets_air: false,
    });
    old.view.weapon_feedback.push(WeaponFeedback {
        weapon: UnitTypeId(1),
        position: crate::sim::Position { x: 32, y: 32 },
        targets_air: false,
        impact: true,
    });
    let mut pending = Some(old.clone());
    session.advance(&[]).unwrap();
    let mut current = session.update(PlayerId(0), &[]).unwrap();
    current.view.entities.clear();
    let current_tick = current.view.tick;
    retain_latest(&mut pending, current);
    let latest = pending.unwrap();
    assert_eq!(latest.view.tick, current_tick);
    assert!(latest.view.entities.is_empty());
    assert_eq!(latest.view.removed, old.view.removed);
    assert_eq!(latest.view.shots, old.view.shots);
    assert_eq!(latest.view.weapon_feedback, old.view.weapon_feedback);
}

#[test]
fn blocked_tcp_control_writer_does_not_block_new_udp_snapshots() {
    let mut server = ServerSession::new(definitions(), 42, vec![PlayerId(0), PlayerId(1)]).unwrap();
    let initial = server.update(PlayerId(0), &[]).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let _unread = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let mut connection = Connection::new(listener.accept().unwrap().0).unwrap();
    let text: String = (0..80_000_u64)
        .map(|i| blake3::hash(&i.to_le_bytes()).to_hex().to_string())
        .collect();
    for _ in 0..8 {
        connection
            .send(&ServerMessage::Rejected(text.clone()))
            .unwrap();
        if connection.has_pending_writes() {
            break;
        }
    }
    assert!(connection.has_pending_writes());
    let (udp, mut reader) = udp_pair();
    let mut peer = Peer {
        connection,
        player: Some(PlayerId(0)),
        hello: None,
        map_offset: None,
        name: "Alice".into(),
        lost: None,
        ended: None,
        host: true,
        slot: None,
        chat_gate: chat::Gate::default(),
        commands: Default::default(),
        udp,
        replication: Some(replication::Sender::new(initial)),
    };
    server.advance(&[]).unwrap();
    peer.replication
        .as_mut()
        .unwrap()
        .observe(server.update(PlayerId(0), &[]).unwrap())
        .unwrap();
    peer.flush_update(true).unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if let Some(ServerMessage::Delta(update)) = reader.poll() {
            assert_eq!(update.sequence, 1);
            break;
        }
        assert!(Instant::now() < deadline);
    }
    assert!(peer.connection.has_pending_writes());
}

#[test]
fn host_views_publish_each_tick_while_remote_views_wait_for_the_network_interval() {
    let mut server = ServerSession::new(definitions(), 42, vec![PlayerId(0), PlayerId(1)]).unwrap();
    for player in [PlayerId(0), PlayerId(1)] {
        let initial = server.update(player, &[]).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let receiving = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (sending, _) = listener.accept().unwrap();
        let _receiving = receiving;
        let (udp, mut reader) = udp_pair();
        let mut peer = Peer {
            connection: Connection::new(sending).unwrap(),
            player: Some(player),
            hello: None,
            map_offset: None,
            name: "test".into(),
            lost: None,
            ended: None,
            host: player == PlayerId(0),
            slot: None,
            chat_gate: chat::Gate::default(),
            commands: Default::default(),
            udp,

            replication: Some(replication::Sender::new(initial)),
        };
        server.advance(&[]).unwrap();
        peer.replication
            .as_mut()
            .unwrap()
            .observe(server.update(player, &[]).unwrap())
            .unwrap();
        peer.flush_update(false).unwrap();
        if player == PlayerId(1) {
            assert!(reader.poll::<ServerMessage>().is_none());
            assert!(peer.replication.as_ref().unwrap().pending());
            peer.flush_update(true).unwrap();
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if let Some(ServerMessage::Delta(update)) = reader.poll() {
                assert_eq!(update.tick, server.world().tick());
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
    }
}
