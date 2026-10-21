use super::*;

#[test]
fn three_players_keep_playing_after_control_loss_and_resume_pending_commands_once() {
    let world = definitions();
    let mut rules = world.rules().clone();
    rules.victory = false;
    let mut map = world.map().clone();
    map.players = 3;
    let mut spawn = map.spawns[2].clone();
    spawn.owner = PlayerId(2);
    spawn.position = crate::sim::Position { x: 768, y: 480 };
    map.spawns.push(spawn);
    let world = World::new(rules, map, 42).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let cancelled = stop.clone();
    let host_world = world.clone();
    let thread = std::thread::spawn(move || {
        run_host_configured(
            listener,
            host_world.clone(),
            42,
            cancelled,
            None,
            MapTransfer::of(&host_world).unwrap(),
            vec![PlayerId(0), PlayerId(1), PlayerId(2)],
            25,
            None,
        )
    });
    let mut clients: Vec<_> = (0..3)
        .map(|i| {
            RemoteClient::connect(address, &world, PlayerId(i))
                .unwrap()
                .0
        })
        .collect();
    let target = crate::sim::Position { x: 800, y: 480 };
    clients[2]
        .command(Command {
            tick: Tick(0),
            player: PlayerId(2),
            sequence: 73,
            order: Order::Move {
                entity: EntityId(7),
                target,
            },
        })
        .unwrap();
    let offer = clients[2].datagram_offer.token;
    clients[2].connection.interrupt();
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut ticks = [0; 3];
    let mut resumed = false;
    let mut acked = false;
    while !resumed || !acked || ticks.iter().any(|t| *t < 12) {
        for (i, client) in clients.iter_mut().enumerate() {
            for _ in 0..64 {
                match client.poll().unwrap() {
                    Some(ServerMessage::Update(update)) => {
                        ticks[i] = update.view.tick.0;
                        if i == 2 {
                            acked |= update.command_acks.contains(73);
                        }
                        client.acknowledge(update.sequence).unwrap();
                    }
                    Some(ServerMessage::Notice(notice)) if i == 2 => {
                        resumed |= notice == "Connection restored."
                    }
                    Some(ServerMessage::End(reason)) => {
                        panic!("one connection ended everyone: {reason}")
                    }
                    Some(_) => {}
                    None => break,
                }
            }
        }
        assert!(
            Instant::now() < deadline,
            "ticks={ticks:?}, resumed={resumed}, acked={acked}"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(clients[2].datagram_offer.token, offer);
    clients[2].leave();
    let before = ticks[0];
    let mut noticed = false;
    while ticks[0] < before + 8 || !noticed {
        for (i, client) in clients[..2].iter_mut().enumerate() {
            match client.poll().unwrap() {
                Some(ServerMessage::Update(update)) => ticks[i] = update.view.tick.0,
                Some(ServerMessage::Notice(reason)) => noticed |= reason.contains("disconnected"),
                Some(ServerMessage::End(reason)) => panic!("leaving ended everyone: {reason}"),
                _ => {}
            }
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    stop.store(true, Ordering::Release);
    assert!(thread.join().unwrap().is_err());
}
