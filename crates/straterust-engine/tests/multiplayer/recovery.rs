use super::*;

#[test]
fn empty_open_teammate_runs_ai_and_both_seats_can_be_rejoined_after_leaving() {
    let (root, mut catalog) = fixture();
    catalog.maps[0].players = 5;
    let mut setup: MapSetup =
        straterust_engine::content::read_ron(&root.path().join("map/match.ron")).unwrap();
    setup.factions.truncate(5);
    setup.resources.truncate(5);
    setup.selectable_factions.truncate(5);
    write(&root.path().join("map"), "match.ron", &setup);
    let definitions = Package::client_definitions(&root.path().join("map")).unwrap();
    let mut settings = catalog.defaults(0);
    settings.mode = 2; // Top vs Bottom: three allied seats, two opponents.
    settings.random_locations = false;
    for (i, slot) in settings.slots.iter_mut().enumerate() {
        slot.kind = match i {
            0..=2 => SlotKind::Open,
            3..=4 => SlotKind::Computer,
            _ => SlotKind::Closed,
        };
    }
    let prepared = catalog
        .prepare(root.path(), 0, &settings, &[0, 1], 42)
        .unwrap();
    assert_eq!(prepared.owners[2], Some(PlayerId(2)));
    assert!(
        prepared
            .world
            .map()
            .ai
            .iter()
            .any(|a| a.player == PlayerId(2) && a.active)
    );
    assert!(
        !prepared
            .world
            .map()
            .ai
            .iter()
            .any(|a| a.player == PlayerId(1) && a.active)
    );
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let stopped = stop.clone();
    let directory = root.path().to_owned();
    let thread = std::thread::spawn(move || {
        lobby::host(
            listener, directory, catalog, 0, settings, 42, stopped, None, None,
        )
    });
    let mut host = lobby::Client::connect_named(address, &definitions, true, "Host").unwrap();
    poll_until(&mut host, |r| matches!(r, lobby::Response::State { .. }));
    let mut guest = lobby::Client::connect_named(address, &definitions, false, "Kid").unwrap();
    poll_until(&mut guest, |r| {
        matches!(r, lobby::Response::State { slot: 1, .. })
    });
    host.send(lobby::Request::Ready(true)).unwrap();
    guest.send(lobby::Request::Ready(true)).unwrap();
    poll_until(
        &mut host,
        |r| matches!(r, lobby::Response::State { view, .. } if view.players.iter().flatten().all(|p| p.ready)),
    );
    host.send(lobby::Request::Start).unwrap();
    for client in [&mut host, &mut guest] {
        poll_until(client, |r| matches!(r, lobby::Response::Launch { .. }));
    }
    drop((host, guest));
    let (mut host, _) = net::RemoteClient::connect(address, &definitions, PlayerId(0)).unwrap();
    let (mut guest, _) = net::RemoteClient::connect(address, &definitions, PlayerId(1)).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    // Let the match actually start before leaving.
    loop {
        host.poll().unwrap();
        if matches!(guest.poll().unwrap(), Some(net::ServerMessage::Update(ref u)) if u.view.tick.0 > 2)
        {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    guest.leave();
    loop {
        if matches!(host.poll().unwrap(), Some(net::ServerMessage::Notice(ref s)) if s.contains("Computer control enabled"))
        {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    // Rejoin the initially empty seat, leave it, then reclaim the original seat.
    for slot in [2, 1] {
        let mut browser =
            lobby::Client::connect_named(address, &definitions, false, "Kid").unwrap();
        let state = poll_until(
            &mut browser,
            |r| matches!(r, lobby::Response::State { view, .. } if view.running),
        );
        let lobby::Response::State { view, .. } = state else {
            unreachable!()
        };
        assert!(view.players[slot].is_none());
        assert_eq!(view.settings.slots[slot].kind, SlotKind::Computer);
        // A stale or occupied choice must not end the browser or the match.
        browser.send(lobby::Request::Slot(0)).unwrap();
        poll_until(&mut browser, |r| matches!(r, lobby::Response::Rejected(_)));
        browser.send(lobby::Request::Slot(99)).unwrap();
        poll_until(&mut browser, |r| matches!(r, lobby::Response::Rejected(_)));
        browser.send(lobby::Request::Slot(slot)).unwrap();
        let launch = poll_until(&mut browser, |r| {
            matches!(r, lobby::Response::Launch { .. })
        });
        let lobby::Response::Launch {
            player,
            reservation,
            ..
        } = launch
        else {
            unreachable!()
        };
        drop(browser);
        let (mut joined, initial) =
            net::RemoteClient::connect_reserved(address, &definitions, player, None, reservation)
                .unwrap();
        assert_eq!(player, PlayerId(slot as u16));
        let entity = initial
            .view
            .entities
            .iter()
            .find_map(|e| match e {
                ViewedEntity::Owned(e) => Some(e.id),
                _ => None,
            })
            .unwrap();
        joined
            .command(Command {
                player,
                sequence: 1,
                tick: initial.view.tick,
                order: Order::Hold { entity },
            })
            .unwrap();
        loop {
            if let Some(net::ServerMessage::Update(u)) = host.poll().unwrap() {
                host.acknowledge(u.sequence).unwrap();
            }
            if let Some(net::ServerMessage::Update(u)) = joined.poll().unwrap() {
                joined.acknowledge(u.sequence).unwrap();
                if u.outcomes
                    .iter()
                    .any(|o| o.command.sequence == 1 && o.rejection.is_none())
                {
                    break;
                }
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(2));
        }
        joined.leave();
        loop {
            if matches!(host.poll().unwrap(), Some(net::ServerMessage::Notice(ref s)) if s.contains("Computer control enabled"))
            {
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    stop.store(true, Ordering::Release);
    assert!(thread.join().unwrap().is_err());
}

#[test]
fn host_can_choose_slot_and_departed_players_are_replaced_without_restarting_the_match() {
    let (root, catalog) = fixture();
    let definitions = Package::client_definitions(&root.path().join("map")).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let stopped = stop.clone();
    let directory = root.path().to_owned();
    let mut settings = catalog.defaults(0);
    settings.random_locations = false;
    for (i, slot) in settings.slots.iter_mut().enumerate() {
        slot.kind = if i < 3 {
            SlotKind::Open
        } else if i == 3 {
            SlotKind::Computer
        } else {
            SlotKind::Closed
        };
    }
    let replay_path = root.path().join("handoff.ron");
    let replay_output = replay_path.clone();
    let thread = std::thread::spawn(move || {
        lobby::host(
            listener,
            directory,
            catalog,
            0,
            settings,
            42,
            stopped,
            Some(replay_output),
            None,
        )
    });
    let mut host = lobby::Client::connect_named(address, &definitions, true, "Host").unwrap();
    poll_until(&mut host, |r| matches!(r, lobby::Response::State { .. }));
    host.send(lobby::Request::Slot(2)).unwrap();
    poll_until(
        &mut host,
        |r| matches!(r, lobby::Response::State { slot: 2, view } if view.host == 2),
    );
    let mut first = lobby::Client::connect_named(address, &definitions, false, "First").unwrap();
    poll_until(&mut first, |r| {
        matches!(r, lobby::Response::State { slot: 0, .. })
    });
    let mut second = lobby::Client::connect_named(address, &definitions, false, "Second").unwrap();
    poll_until(&mut second, |r| {
        matches!(r, lobby::Response::State { slot: 1, .. })
    });
    second.send(lobby::Request::Slot(2)).unwrap();
    poll_until(&mut second, |r| matches!(r, lobby::Response::Rejected(_)));
    let mut waiting = [host, first, second];
    for client in &mut waiting {
        client.send(lobby::Request::Ready(true)).unwrap();
    }
    poll_until(
        &mut waiting[0],
        |r| matches!(r, lobby::Response::State { view, .. } if view.players.iter().flatten().all(|p| p.ready)),
    );
    waiting[0].send(lobby::Request::Start).unwrap();
    let assignments: Vec<_> = waiting
        .iter_mut()
        .map(
            |client| match poll_until(client, |r| matches!(r, lobby::Response::Launch { .. })) {
                lobby::Response::Launch { player, .. } => player,
                _ => unreachable!(),
            },
        )
        .collect();
    assert_eq!(assignments, vec![PlayerId(2), PlayerId(0), PlayerId(1)]);
    drop(waiting);
    let mut clients: Vec<_> = assignments
        .into_iter()
        .map(|p| {
            net::RemoteClient::connect(address, &definitions, p)
                .unwrap()
                .0
        })
        .collect();
    assert!(clients[0].handshake.host);
    assert!(!clients[1].handshake.host);
    let deadline = Instant::now() + Duration::from_secs(8);
    let mut tick = 0;
    while tick < 4 {
        for client in &mut clients {
            if let Some(net::ServerMessage::Update(update)) = client.poll().unwrap() {
                tick = tick.max(update.view.tick.0);
            }
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    clients[0].pause(true).unwrap();
    let mut paused = false;
    while !paused {
        for client in &mut clients {
            if let Some(net::ServerMessage::Status { paused: true, .. }) = client.poll().unwrap() {
                paused = true;
            }
        }
        assert!(Instant::now() < deadline);
    }
    clients[0].pause(false).unwrap();
    clients[2].leave();
    let mut noticed = false;
    let departed_at = tick;
    while !noticed || tick < departed_at + 4 {
        for client in &mut clients[..2] {
            match client.poll().unwrap() {
                Some(net::ServerMessage::Update(update)) => tick = tick.max(update.view.tick.0),
                Some(net::ServerMessage::Notice(reason)) => {
                    noticed |= reason.contains("Computer control enabled")
                }
                Some(net::ServerMessage::End(reason)) => panic!("departure ended match: {reason}"),
                _ => {}
            }
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    let mut browser =
        lobby::Client::connect_named(address, &definitions, false, "Replacement").unwrap();
    let state = poll_until(
        &mut browser,
        |r| matches!(r, lobby::Response::State { view, .. } if view.running),
    );
    if let lobby::Response::State { view, .. } = state {
        assert!(view.players[1].is_none());
        assert!(view.players[3].is_none()); // Original computer slot also joinable.
        assert!(view.players[0].is_some() && view.players[2].is_some());
    }
    browser.send(lobby::Request::Slot(1)).unwrap();
    let launch = poll_until(&mut browser, |r| {
        matches!(r, lobby::Response::Launch { .. })
    });
    let (player, ticket) = match launch {
        lobby::Response::Launch {
            player,
            reservation: Some(ticket),
            ..
        } => (player, ticket),
        _ => panic!("missing slot reservation"),
    };
    drop(browser);
    let (mut replacement, initial) =
        net::RemoteClient::connect_reserved(address, &definitions, player, None, Some(ticket))
            .unwrap();
    assert_eq!(player, PlayerId(1));
    assert!(initial.view.tick.0 >= tick);
    let actor = initial
        .view
        .entities
        .iter()
        .find_map(|e| match e {
            ViewedEntity::Owned(e) => Some(e.id),
            _ => None,
        })
        .unwrap();
    replacement
        .command(Command {
            player,
            sequence: 1,
            tick: initial.view.tick,
            order: Order::Hold { entity: actor },
        })
        .unwrap();
    let mut accepted = false;
    while !accepted {
        for client in &mut clients[..2] {
            if let Some(net::ServerMessage::End(reason)) = client.poll().unwrap() {
                panic!("joining ended match: {reason}");
            }
        }
        if let Some(net::ServerMessage::Update(update)) = replacement.poll().unwrap() {
            accepted |= update
                .outcomes
                .iter()
                .any(|o| o.command.sequence == 1 && o.rejection.is_none());
            replacement.acknowledge(update.sequence).unwrap();
        }
        assert!(
            Instant::now() < deadline,
            "replacement could not issue a command"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    stop.store(true, Ordering::Release);
    assert!(thread.join().unwrap().is_err());
    let replay = net::load_replay(&replay_path).unwrap();
    assert!(
        replay
            .ticks
            .iter()
            .any(|t| t.control.contains(&(PlayerId(1), true)))
    );
    assert!(
        replay
            .ticks
            .iter()
            .any(|t| t.control.contains(&(PlayerId(1), false)))
    );
    // Replaying includes handoffs as well as ordinary inputs.
    let (root, catalog) = fixture();
    let mut settings = catalog.defaults(0);
    settings.random_locations = false;
    for (i, slot) in settings.slots.iter_mut().enumerate() {
        slot.kind = if i < 3 {
            SlotKind::Open
        } else if i == 3 {
            SlotKind::Computer
        } else {
            SlotKind::Closed
        };
    }
    let world = catalog
        .prepare(root.path(), 0, &settings, &[0, 1, 2], 42)
        .unwrap()
        .world;
    replay.play(&world).unwrap();
}
