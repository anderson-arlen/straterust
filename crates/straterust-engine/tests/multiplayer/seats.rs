use super::*;

#[test]
fn host_assigns_named_players_and_guests_choose_their_own_team_and_ready_state() {
    let (root, mut catalog) = fixture();
    catalog.modes.push(GameType::Teams);
    let definitions = Package::client_definitions(&root.path().join("map")).unwrap();
    let mut settings = catalog.defaults(0);
    settings.mode = catalog.modes.len() - 1;
    for (i, slot) in settings.slots.iter_mut().enumerate() {
        slot.kind = if i < 4 {
            SlotKind::Open
        } else {
            SlotKind::Closed
        };
        slot.team = if i < 2 { 4 } else { 6 };
    }
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
    let mut host = lobby::Client::connect_named(address, &definitions, true, "Parent").unwrap();
    poll_until(&mut host, |r| matches!(r, lobby::Response::State { .. }));
    let mut kid = lobby::Client::connect_named(address, &definitions, false, "Kid").unwrap();
    poll_until(&mut kid, |r| {
        matches!(r, lobby::Response::State { slot: 1, .. })
    });
    host.send(lobby::Request::Ready(true)).unwrap();
    kid.send(lobby::Request::Ready(true)).unwrap();
    poll_until(
        &mut host,
        |r| matches!(r, lobby::Response::State { view, .. } if view.players.iter().flatten().all(|p| p.ready)),
    );
    // Moving someone via the map controls also updates that peer's own slot.
    host.send(lobby::Request::Assign { from: 1, to: 3 })
        .unwrap();
    poll_until(
        &mut kid,
        |r| matches!(r, lobby::Response::State { slot: 3, view } if view.players[1].is_none() && view.players[3].as_ref().unwrap().name == "Kid" && view.players.iter().flatten().all(|p| p.ready)),
    );
    kid.send(lobby::Request::Assign { from: 0, to: 1 }).unwrap();
    poll_until(&mut kid, |r| matches!(r, lobby::Response::Rejected(_)));
    kid.send(lobby::Request::Team(4)).unwrap();
    poll_until(
        &mut host,
        |r| matches!(r, lobby::Response::State { view, .. } if view.settings.slots[3].team == 4),
    );
    // Swapping occupied positions moves host authority with the person.
    host.send(lobby::Request::Assign { from: 0, to: 3 })
        .unwrap();
    poll_until(
        &mut host,
        |r| matches!(r, lobby::Response::State { slot: 3, view } if view.host == 3 && view.players[0].as_ref().unwrap().name == "Kid"),
    );
    poll_until(
        &mut kid,
        |r| matches!(r, lobby::Response::State { slot: 0, view } if view.host == 3),
    );
    kid.send(lobby::Request::Ready(true)).unwrap();
    poll_until(
        &mut host,
        |r| matches!(r, lobby::Response::State { view, .. } if view.players[0].as_ref().unwrap().ready),
    );
    kid.send(lobby::Request::Ready(false)).unwrap();
    poll_until(
        &mut host,
        |r| matches!(r, lobby::Response::State { view, .. } if !view.players[0].as_ref().unwrap().ready),
    );
    host.send(lobby::Request::Ready(true)).unwrap();
    kid.send(lobby::Request::Ready(true)).unwrap();
    poll_until(
        &mut host,
        |r| matches!(r, lobby::Response::State { view, .. } if view.players.iter().flatten().all(|p| p.ready)),
    );
    host.send(lobby::Request::Start).unwrap();
    assert!(matches!(
        poll_until(&mut host, |r| matches!(r, lobby::Response::Launch { .. })),
        lobby::Response::Launch {
            player: PlayerId(3),
            ..
        }
    ));
    assert!(matches!(
        poll_until(&mut kid, |r| matches!(r, lobby::Response::Launch { .. })),
        lobby::Response::Launch {
            player: PlayerId(0),
            ..
        }
    ));
    stop.store(true, Ordering::Release);
    assert!(thread.join().unwrap().is_err());
}

#[test]
fn players_choose_unused_colors_and_keep_readiness_through_lobby_changes() {
    let (root, mut catalog) = fixture();
    catalog.colors = (0..8)
        .map(|i| PlayerColor {
            name: format!("Color {i}"),
            shades: vec![[i * 30, 40, 60]],
        })
        .collect();
    catalog.modes.push(GameType::Teams);
    let mut other = catalog.maps[0].clone();
    other.title = "Another map".into();
    other.package = "other".into();
    catalog.maps.push(other);
    std::fs::create_dir(root.path().join("other")).unwrap();
    for entry in std::fs::read_dir(root.path().join("map")).unwrap() {
        let entry = entry.unwrap();
        std::fs::copy(
            entry.path(),
            root.path().join("other").join(entry.file_name()),
        )
        .unwrap();
    }
    catalog.validate().unwrap();
    let definitions = Package::client_definitions(&root.path().join("map")).unwrap();
    let mut settings = catalog.defaults(0);
    settings.mode = catalog.modes.len() - 1;
    for slot in &mut settings.slots[3..] {
        slot.kind = SlotKind::Closed;
    }
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
    let mut host = lobby::Client::connect_named(address, &definitions, true, "Parent").unwrap();
    poll_until(&mut host, |r| matches!(r, lobby::Response::State { .. }));
    let mut guest = lobby::Client::connect_named(address, &definitions, false, "Kid").unwrap();
    poll_until(&mut guest, |r| {
        matches!(r, lobby::Response::State { slot: 1, .. })
    });
    host.send(lobby::Request::Ready(true)).unwrap();
    guest.send(lobby::Request::Ready(true)).unwrap();
    poll_until(
        &mut host,
        |r| matches!(r, lobby::Response::State { view, .. } if view.players.iter().flatten().all(|p|p.ready)),
    );
    host.send(lobby::Request::Color(6)).unwrap();
    poll_until(
        &mut guest,
        |r| matches!(r, lobby::Response::State { view, .. } if view.settings.slots[0].color == Some(6)),
    );
    // The authoritative lobby rejects a race for the same color without losing
    // either player's current palette or ready state.
    guest.send(lobby::Request::Color(6)).unwrap();
    let rejected = poll_until(&mut guest, |r| matches!(r, lobby::Response::Rejected(_)));
    assert!(
        matches!(rejected, lobby::Response::Rejected(reason) if reason.contains("already in use"))
    );
    guest.send(lobby::Request::Color(7)).unwrap();
    guest.send(lobby::Request::Faction(Some(1))).unwrap();
    guest.send(lobby::Request::Team(0)).unwrap();
    let response = poll_until(
        &mut host,
        |r| matches!(r, lobby::Response::State { view, .. } if view.settings.slots[1].color == Some(7) && view.settings.slots[1].faction == Some(1) && view.settings.slots[1].team == 0),
    );
    let lobby::Response::State { view, .. } = response else {
        unreachable!()
    };
    assert!(view.players.iter().flatten().all(|p| p.ready));
    guest.send(lobby::Request::Color(99)).unwrap();
    poll_until(&mut guest, |r| matches!(r, lobby::Response::Rejected(_)));
    let mut settings = view.settings.clone();
    settings.resources = Some(0);
    settings.fog = false;
    host.send(lobby::Request::Configure {
        map: 0,
        settings: settings.clone(),
    })
    .unwrap();
    poll_until(
        &mut guest,
        |r| matches!(r, lobby::Response::State { view, .. } if !view.settings.fog && view.players.iter().flatten().all(|p|p.ready)),
    );
    settings.slots[1].color = Some(6);
    host.send(lobby::Request::Configure { map: 0, settings })
        .unwrap();
    poll_until(&mut host, |r| matches!(r, lobby::Response::Rejected(_)));
    // Map defaults must preserve connected players' selected colors and ready
    // choices, assigning the remaining palettes around them.
    let mut defaults = view.settings.clone();
    for (i, slot) in defaults.slots.iter_mut().enumerate() {
        slot.color = Some(i);
    }
    host.send(lobby::Request::Configure {
        map: 1,
        settings: defaults,
    })
    .unwrap();
    poll_until(
        &mut guest,
        |r| matches!(r, lobby::Response::State { view, .. } if view.map == 1 && view.settings.slots[0].color == Some(6) && view.settings.slots[1].color == Some(7) && view.players.iter().flatten().all(|p| p.ready)),
    );
    guest.send(lobby::Request::Slot(2)).unwrap();
    poll_until(
        &mut guest,
        |r| matches!(r, lobby::Response::State { slot: 2, view } if view.settings.slots[2].color == Some(7) && view.players.iter().flatten().all(|p|p.ready)),
    );
    let mut newcomer =
        lobby::Client::connect_named(address, &definitions, false, "Newcomer").unwrap();
    poll_until(
        &mut newcomer,
        |r| matches!(r, lobby::Response::State { slot: 1, view } if !view.players[1].as_ref().unwrap().ready && view.players[0].as_ref().unwrap().ready && view.players[2].as_ref().unwrap().ready),
    );
    newcomer.send(lobby::Request::Leave).unwrap();
    poll_until(
        &mut host,
        |r| matches!(r, lobby::Response::State { view, .. } if view.players[1].is_none() && view.players.iter().flatten().all(|p|p.ready)),
    );
    guest.send(lobby::Request::Ready(false)).unwrap();
    poll_until(
        &mut host,
        |r| matches!(r, lobby::Response::State { view, .. } if !view.players[2].as_ref().unwrap().ready && view.players[0].as_ref().unwrap().ready),
    );
    host.send(lobby::Request::Start).unwrap();
    poll_until(&mut host, |r| matches!(r, lobby::Response::Rejected(_)));
    stop.store(true, Ordering::Release);
    thread.join().unwrap().unwrap();
}
