//! Authoritative TCP setup/control and UDP gameplay delivery.
use super::*;

/// Runs off the window thread. All state/hashes/replay remain here. A peer loss
/// failures are isolated to their slots. Host migration is not implied.
pub fn run_host(
    listener: TcpListener,
    world: World,
    seed: u64,
    cancelled: Arc<AtomicBool>,
    replay_path: Option<&Path>,
) -> Result<()> {
    let map = MapTransfer::of(&world)?;
    run_host_with_map(listener, world, seed, cancelled, replay_path, map, None)
}

pub fn run_host_with_map(
    listener: TcpListener,
    world: World,
    seed: u64,
    cancelled: Arc<AtomicBool>,
    replay_path: Option<&Path>,
    map: MapTransfer,
    traffic: Option<Arc<Traffic>>,
) -> Result<()> {
    let tick_ms = world.rules().tick_ms;
    run_host_configured(
        listener,
        world,
        seed,
        cancelled,
        replay_path,
        map,
        vec![PlayerId(0), PlayerId(1)],
        tick_ms,
        traffic,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn run_host_configured(
    listener: TcpListener,
    world: World,
    seed: u64,
    cancelled: Arc<AtomicBool>,
    replay_path: Option<&Path>,
    map: MapTransfer,
    humans: Vec<PlayerId>,
    tick_ms: u32,
    traffic: Option<Arc<Traffic>>,
) -> Result<()> {
    run_configured(
        listener,
        world,
        seed,
        cancelled,
        replay_path,
        map,
        humans,
        tick_ms,
        traffic,
        None,
    )
}

mod slots;
pub(super) use slots::RunningLobby;
use slots::{accept_hello, retire};
#[allow(clippy::too_many_arguments)]
pub(super) fn run_configured(
    listener: TcpListener,
    world: World,
    seed: u64,
    cancelled: Arc<AtomicBool>,
    replay_path: Option<&Path>,
    map: MapTransfer,
    humans: Vec<PlayerId>,
    tick_ms: u32,
    traffic: Option<Arc<Traffic>>,
    mut lobby: Option<RunningLobby>,
) -> Result<()> {
    ensure!(
        !humans.is_empty() && humans.len() <= 8 && (1..=1000).contains(&tick_ms),
        "invalid session setup"
    );
    ensure!(
        map.identity == GameplayIdentity::of(&world),
        "server map identity mismatch"
    );
    let map_bytes = map.encode()?;
    let map_hash = blake3::hash(&map_bytes).to_hex().to_string();
    ensure!(
        world.map().players >= 2,
        "map has fewer than two player slots"
    );
    let participants = humans
        .iter()
        .copied()
        .chain(
            world
                .map()
                .ai
                .iter()
                .filter(|a| a.takeover)
                .map(|a| a.player),
        )
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    let mut command_sequences = world.state().last_sequences.clone();
    let mut server = ServerSession::new(world, seed, participants)?;
    let mut advertisement = lan::Advertiser::new(
        &server.world().map().id,
        listener.local_addr()?.port(),
        GameplayIdentity::of(server.world()),
    )
    .ok();
    listener.set_nonblocking(true)?;
    let udp_socket = Arc::new(
        std::net::UdpSocket::bind(listener.local_addr()?).context("cannot bind UDP game port")?,
    );
    udp_socket.set_nonblocking(true)?;
    let host_ip = listener.local_addr()?.ip();
    let mut peers: Vec<Peer> = Vec::new();
    let mut started = false;
    let mut paused = false;
    let mut last_status = Instant::now();
    let epoch = Instant::now();
    let mut next_publish = Instant::now();
    let mut next_tick = Instant::now();
    let tick_duration = Duration::from_millis(u64::from(tick_ms));
    let mut last_diagnostics = Instant::now();
    let mut max_step_time = Duration::ZERO;
    let mut max_view_time = Duration::ZERO;
    let mut last_diagnostic_tick = server.world().tick().0;
    log::info!(
        "hosting map={} address={} human_slots={humans:?} protocol={PROTOCOL_VERSION}",
        server.world().map().id,
        listener.local_addr()?
    );
    let result = (|| -> Result<()> {
        while !cancelled.load(Ordering::Acquire) {
            if let Some(advertisement) = &advertisement {
                let _ = advertisement.poll();
            }
            loop {
                match listener.accept() {
                    Ok((stream, address)) if peers.len() < 16 => {
                        let Ok(mut connection) = Connection::new(stream) else {
                            continue;
                        };
                        let mut counters = Arc::new(Traffic::default());
                        if !address.ip().is_loopback()
                            && address.ip() != host_ip
                            && let Some(traffic) = &traffic
                        {
                            connection.track(Arc::clone(traffic));
                            counters = Arc::clone(traffic);
                        }
                        let udp = datagram::Endpoint::new(
                            Arc::clone(&udp_socket),
                            datagram::Offer::new(udp_socket.local_addr()?.port())?,
                            address.ip(),
                            None,
                            counters,
                        )?;
                        peers.push(Peer {
                            connection,
                            player: None,
                            hello: None,
                            map_offset: None,
                            name: String::new(),
                            chat_gate: chat::Gate::default(),
                            replication: None,
                            commands: Default::default(),
                            udp,
                            lost: None,
                            ended: None,
                            host: false,
                            slot: None,
                        });
                    }
                    Ok((stream, _)) => {
                        let Ok(mut connection) = Connection::new(stream) else {
                            continue;
                        };
                        let _ = connection.send(&ServerMessage::End("too many connections".into()));
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(error)
                        if matches!(
                            error.kind(),
                            std::io::ErrorKind::Interrupted
                                | std::io::ErrorKind::ConnectionAborted
                                | std::io::ErrorKind::ConnectionReset
                        ) =>
                    {
                        continue;
                    }
                    Err(error) => return Err(error).context("host accept failed"),
                }
            }
            let mut status_changed = false;
            let mut drop_peers = Vec::new();
            let mut datagrams: Vec<std::collections::VecDeque<ClientMessage>> =
                (0..peers.len()).map(|_| Default::default()).collect();
            let mut packet = [0; datagram::PACKET_BYTES + 1];
            for _ in 0..512 {
                let Ok((length, source)) = udp_socket.recv_from(&mut packet) else {
                    break;
                };
                if length < 20 {
                    continue;
                }
                if let Some(index) = peers
                    .iter()
                    .position(|p| p.player.is_some() && packet[4..20] == p.udp.offer.token)
                    && let Some(message) = peers[index]
                        .udp
                        .receive::<ClientMessage>(&packet[..length], source)
                    && matches!(
                        message,
                        ClientMessage::Command(_)
                            | ClientMessage::Acknowledge { .. }
                            | ClientMessage::Probe(_)
                            | ClientMessage::Resync
                    )
                {
                    datagrams[index].push_back(message);
                }
            }
            for index in 0..peers.len() {
                for _ in 0..256 {
                    let message = match datagrams[index]
                        .pop_front()
                        .map(Some)
                        .map(Ok)
                        .unwrap_or_else(|| peers[index].receive())
                    {
                        Ok(Some(message)) => message,
                        Ok(None) => break,
                        Err(error) => {
                            peers[index].ended = Some(format!("invalid peer data: {error:#}"));
                            drop_peers.push(index);
                            break;
                        }
                    };
                    if peers[index].player.is_none()
                        && !matches!(
                            &message,
                            ClientMessage::Hello(_)
                                | ClientMessage::MapReady(_)
                                | ClientMessage::Join { .. }
                                | ClientMessage::Resume(_)
                                | ClientMessage::Lobby(_)
                        )
                    {
                        drop_peers.push(index);
                        break;
                    }
                    let handled = (|| -> Result<()> {
                        match message {
                            ClientMessage::Resume(token) => {
                                let old = peers
                                    .iter()
                                    .position(|p| {
                                        p.player.is_some()
                                            && p.udp.offer.token == token
                                            && p.udp.expected_ip == peers[index].udp.expected_ip
                                    })
                                    .context("unknown reconnect session")?;
                                ensure!(
                                    old != index && peers[index].player.is_none(),
                                    "invalid reconnect request"
                                );
                                let (old_peer, new_peer) = if old < index {
                                    let (a, b) = peers.split_at_mut(index);
                                    (&mut a[old], &mut b[0])
                                } else {
                                    let (a, b) = peers.split_at_mut(old);
                                    (&mut b[0], &mut a[index])
                                };
                                std::mem::swap(&mut old_peer.connection, &mut new_peer.connection);
                                old_peer.lost = None;
                                old_peer.send(&ServerMessage::Resumed)?;
                                old_peer.send(&ServerMessage::Status { started, paused })?;
                                log::info!(
                                    "peer {:?} ({}) reconnected with existing UDP and command state",
                                    old_peer.player,
                                    old_peer.name
                                );
                                drop_peers.push(index);
                            }
                            ClientMessage::Lobby(request) => {
                                ensure!(started, "game has not started");
                                lobby
                                    .as_mut()
                                    .context("this match has no joinable lobby")?
                                    .request(index, request, &mut peers, server.world())?;
                            }
                            ClientMessage::Join {
                                hello,
                                slot,
                                ticket,
                            } => {
                                ensure!(
                                    started && peers[index].hello.is_none(),
                                    "invalid running-game join"
                                );
                                let context = lobby.as_mut().context("match is not joinable")?;
                                context.claim(slot, ticket, hello.player)?;
                                peers[index].slot = Some(slot);
                                if let Some(context) = &lobby {
                                    peers[index].name = context.view.players[slot]
                                        .as_ref()
                                        .map_or_else(String::new, |p| p.name.clone());
                                }
                                accept_hello(
                                    index, hello, &mut peers, &humans, &server, started,
                                    &mut lobby, &map_bytes, &map_hash,
                                )?;
                            }
                            ClientMessage::Hello(hello) if peers[index].hello.is_none() => {
                                ensure!(
                                    !started,
                                    "choose an available slot before joining a running game"
                                );
                                accept_hello(
                                    index, hello, &mut peers, &humans, &server, started,
                                    &mut lobby, &map_bytes, &map_hash,
                                )?;
                            }
                            ClientMessage::MapReady(hash) => {
                                if peers[index].player.is_some()
                                    || peers[index].hello.is_none()
                                    || peers[index].map_offset.is_some()
                                    || hash != map_hash
                                {
                                    if peers[index].player.is_some() {
                                        bail!("invalid map acknowledgement");
                                    }
                                    bail!("invalid map acknowledgement");
                                }
                                let hello = peers[index]
                                    .hello
                                    .as_ref()
                                    .context("map acknowledgement before handshake")?;
                                let (mut handshake, initial) = server.connect(hello)?;
                                handshake.input_lead = 0;
                                handshake.host = peers[index].host;
                                let player = hello.player;
                                peers[index].replication =
                                    Some(replication::Sender::new(initial.clone()));
                                peers[index].player = Some(player);
                                peers[index].udp.last_received = Instant::now();
                                if peers[index].name.is_empty() {
                                    peers[index].name = format!("Player {}", player.0 + 1);
                                }
                                if server
                                    .world()
                                    .map()
                                    .ai
                                    .iter()
                                    .any(|a| a.player == player && a.takeover)
                                {
                                    server.control_ai(player, false)?;
                                }
                                log::info!(
                                    "player {} joined slot {:?} as {} at tick {}",
                                    player.0,
                                    peers[index].slot,
                                    peers[index].name,
                                    server.world().tick().0
                                );
                                let udp = peers[index].udp.offer.clone();
                                peers[index].send(&ServerMessage::Welcome {
                                    handshake,
                                    initial,
                                    udp,
                                })?;
                                peers[index].send(&ServerMessage::Status { started, paused })?;
                            }
                            ClientMessage::Probe(echo_ms) => {
                                let received_ms = epoch.elapsed().as_millis() as u64;
                                peers[index].udp.send(
                                    &ServerMessage::Clock {
                                        pong: clock::Pong {
                                            echo_ms,
                                            received_ms,
                                            sent_ms: epoch.elapsed().as_millis() as u64,
                                        },
                                        tick_ms,
                                    },
                                    false,
                                )?;
                            }
                            ClientMessage::Resync => {
                                next_publish = Instant::now();
                            }
                            ClientMessage::Command(input) => {
                                let player =
                                    peers[index].player.context("command before handshake")?;
                                let result = if input.command.player != player {
                                    Err(anyhow::anyhow!("unauthorized player command"))
                                } else {
                                    peers[index].commands.receive(input)
                                };
                                if let Err(error) = result {
                                    peers[index]
                                        .send(&ServerMessage::Rejected(error.to_string()))?;
                                }
                            }
                            ClientMessage::Acknowledge {
                                snapshot,
                                commands_retired,
                            } => {
                                peers[index]
                                    .replication
                                    .as_mut()
                                    .context("acknowledgement before handshake")?
                                    .acknowledge(snapshot)?;
                                peers[index].commands.retire(commands_retired);
                            }
                            ClientMessage::Pause(value) if peers[index].host => {
                                paused = value;
                                status_changed = true;
                                next_tick = Instant::now() + tick_duration;
                            }
                            ClientMessage::Pause(_) => peers[index].send(
                                &ServerMessage::Rejected("only the host player may pause".into()),
                            )?,
                            ClientMessage::Name(name) => {
                                match chat::name(&name).and_then(|name| {
                                    peers[index].chat_gate.accept()?;
                                    if let (Some(context), Some(slot)) =
                                        (&mut lobby, peers[index].slot)
                                    {
                                        context.named(slot, &name);
                                    }
                                    peers[index].name = name;
                                    Ok(())
                                }) {
                                    Ok(()) => {}
                                    Err(error) => peers[index]
                                        .send(&ServerMessage::Rejected(error.to_string()))?,
                                }
                            }
                            ClientMessage::Chat { channel, text } => {
                                let author =
                                    peers[index].player.context("chat before handshake")?;
                                let result = chat::text(&text).and_then(|text| {
                                    peers[index].chat_gate.accept()?;
                                    Ok(chat::Message {
                                        player: author,
                                        name: peers[index].name.clone(),
                                        channel,
                                        text,
                                    })
                                });
                                match result {
                                    Ok(message) => {
                                        for peer in peers.iter_mut().filter(|p| {
                                            p.player.is_some_and(|player| {
                                                channel == chat::Channel::All
                                                    || !server.world().is_enemy(author, player)
                                            })
                                        }) {
                                            peer.send(&ServerMessage::Chat(message.clone()))?;
                                        }
                                    }
                                    Err(error) => peers[index]
                                        .send(&ServerMessage::Rejected(error.to_string()))?,
                                }
                            }
                            ClientMessage::Leave => {
                                peers[index].ended = Some("left the match".into());
                                drop_peers.push(index);
                            }
                            ClientMessage::Hello(_) if peers[index].player.is_none() => {
                                bail!("duplicate handshake");
                            }
                            ClientMessage::Hello(_) => bail!("duplicate handshake"),
                        }
                        Ok(())
                    })();
                    if let Err(error) = handled {
                        let _ = peers[index].send(&ServerMessage::End(format!("{error:#}")));
                        peers[index].ended = Some(format!("peer protocol error: {error:#}"));
                        drop_peers.push(index);
                    }
                    if drop_peers.contains(&index) || peers[index].ended.is_some() {
                        break;
                    }
                }
                if drop_peers.contains(&index) {
                    continue;
                }
                if let Some(offset) = peers[index].map_offset
                    && !peers[index].connection.has_pending_writes()
                {
                    let end = (offset + MAP_CHUNK_BYTES).min(map_bytes.len());
                    peers[index].send(&ServerMessage::MapChunk {
                        offset,
                        data: map_bytes[offset..end].to_vec(),
                    })?;
                    peers[index].map_offset = (end < map_bytes.len()).then_some(end);
                }
                let timeout = if peers[index].player.is_some() {
                    PEER_TIMEOUT
                } else {
                    Duration::from_secs(30)
                };
                let received = if peers[index].player.is_some() {
                    peers[index].udp.last_received
                } else {
                    peers[index].connection.last_received
                };
                if received.elapsed() >= timeout {
                    peers[index].ended = Some("no gameplay traffic for 30 seconds".into());
                    drop_peers.push(index);
                }
                if peers[index]
                    .lost
                    .is_some_and(|since| since.elapsed() >= PEER_TIMEOUT)
                {
                    peers[index].ended = Some("reconnect window expired".into());
                }
                if let Err(error) = peers[index].flush_update(Instant::now() >= next_publish) {
                    peers[index].ended = Some(format!("peer delivery failed: {error:#}"));
                }
                if peers[index].ended.is_some() {
                    drop_peers.push(index);
                }
            }
            if Instant::now() >= next_publish {
                next_publish = Instant::now() + Duration::from_millis(clock::UPDATE_MS);
            }
            drop_peers.sort_unstable();
            drop_peers.dedup();
            for index in drop_peers.into_iter().rev() {
                let peer = peers.remove(index);
                if peer.host && paused {
                    paused = false;
                    status_changed = true;
                    next_tick = Instant::now() + tick_duration;
                }
                retire(peer, &mut peers, &mut server, &mut lobby, started)?;
            }
            if !started && peers.iter().filter(|p| p.player.is_some()).count() == humans.len() {
                started = true;
                status_changed = true;
                next_tick = Instant::now() + tick_duration;
            }
            // Waiting and paused matches have no tick updates. Keep their healthy
            // connections alive without advancing simulation or view sequences.
            if status_changed || last_status.elapsed() >= Duration::from_secs(1) {
                for peer in peers.iter_mut().filter(|p| p.player.is_some()) {
                    if status_changed || !peer.connection.has_pending_writes() {
                        peer.send(&ServerMessage::Status { started, paused })?;
                    }
                }
                last_status = Instant::now();
            }
            if let Some(advertisement) = &mut advertisement {
                let open = lobby
                    .as_ref()
                    .map_or(0, |l| l.available(&peers, server.world()));
                advertisement.status(started, open)?;
            }
            if let Some(context) = &mut lobby {
                context.expire(&peers);
            }
            if started && lobby.is_none() && peers.iter().all(|p| p.player.is_none()) {
                bail!("all players disconnected");
            }
            if started && !paused {
                let batch_started = Instant::now();
                for _ in 0..8 {
                    if Instant::now() < next_tick {
                        break;
                    }
                    let mut available = crate::sim::MAX_COMMANDS_PER_TICK;
                    for peer in peers.iter_mut().filter(|p| p.player.is_some()) {
                        let player = peer.player.unwrap();
                        for input in peer.commands.ready(available) {
                            available -= 1;
                            let sequence = &mut command_sequences[usize::from(player.0)];
                            *sequence = (*sequence)
                                .max(server.world().state().last_sequences[usize::from(player.0)]);
                            *sequence = sequence
                                .checked_add(1)
                                .context("command sequence exhausted")?;
                            let command =
                                peer.commands
                                    .schedule(input, *sequence, server.world().tick())?;
                            server.submit(player, command)?;
                        }
                    }
                    let step_started = Instant::now();
                    let outcomes = server.advance(&[])?;
                    max_step_time = max_step_time.max(step_started.elapsed());
                    let view_started = Instant::now();
                    let mut updates = BTreeMap::new();
                    let mut names: BTreeMap<PlayerId, Vec<String>> = lobby
                        .as_ref()
                        .map_or_else(BTreeMap::new, |l| l.names.clone());
                    for peer in &peers {
                        if let Some(player) = peer.player {
                            let list = names.entry(player).or_default();
                            if !list.contains(&peer.name) {
                                list.push(peer.name.clone());
                            }
                        }
                    }
                    for peer in peers.iter_mut().filter(|p| p.player.is_some()) {
                        let player = peer.player.unwrap();
                        if let std::collections::btree_map::Entry::Vacant(entry) =
                            updates.entry(player)
                        {
                            let mut update = server.update(player, &outcomes)?;
                            if let Some(result) = &mut update.result {
                                for row in &mut result.players {
                                    row.name = names
                                        .get(&row.player)
                                        .map(|n| n.join(" / "))
                                        .unwrap_or_default();
                                }
                            }
                            entry.insert(update);
                        }
                        let mut update = updates[&player].clone();
                        update.outcomes = peer.commands.finish(&outcomes);
                        update.command_acks = peer.commands.acks.clone();
                        update.time = Some(clock::FrameTime {
                            captured_ms: epoch.elapsed().as_millis() as u64,
                            tick_ms,
                        });
                        peer.replication.as_mut().unwrap().observe(update)?;
                    }
                    max_view_time = max_view_time.max(view_started.elapsed());
                    next_tick += tick_duration;
                    if server.match_result().is_some() {
                        return Ok(());
                    }
                    // Publish and service ACKs/commands before doing more
                    // catch-up work. Eight expensive ticks in one batch can
                    // otherwise freeze delivery for hundreds of milliseconds.
                    if batch_started.elapsed() >= tick_duration {
                        break;
                    }
                }
            }
            if started && last_diagnostics.elapsed() >= Duration::from_secs(10) {
                let tick = server.world().tick().0;
                let rate =
                    (tick - last_diagnostic_tick) as f64 / last_diagnostics.elapsed().as_secs_f64();
                for peer in peers.iter().filter(|p| p.player.is_some()) {
                    if let Some(sender) = &peer.replication {
                        let (observed, sent, acknowledged) = sender.progress();
                        log::info!(
                            "match progress: tick={tick}, ticks_per_second={rate:.1}/{:.1}, entities={}, player={:?}, host={}, snapshot={observed}, sent={sent}, acknowledged={acknowledged}, frame_bytes={}, paused={paused}, max_step_ms={:.1}, max_view_ms={:.1}",
                            1000. / f64::from(tick_ms),
                            server.world().state().entities.len(),
                            peer.player,
                            peer.host,
                            peer.udp.last_snapshot_bytes,
                            max_step_time.as_secs_f64() * 1000.,
                            max_view_time.as_secs_f64() * 1000.
                        );
                    }
                }
                last_diagnostics = Instant::now();
                last_diagnostic_tick = tick;
                max_step_time = Duration::ZERO;
                max_view_time = Duration::ZERO;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        bail!("host closed the match")
    })();
    let reason = result
        .as_ref()
        .err()
        .map_or_else(|| "match finished".into(), |e| format!("{e:#}"));
    let mut ended = vec![false; peers.len()];
    let final_result = server.match_result().is_some();
    let deadline = Instant::now()
        + if final_result {
            PEER_TIMEOUT
        } else {
            Duration::from_secs(2)
        };
    while (ended.iter().any(|done| !done)
        || peers.iter().any(|p| p.connection.has_pending_writes()))
        && Instant::now() < deadline
    {
        let mut packet = [0; datagram::PACKET_BYTES + 1];
        for _ in 0..512 {
            let Ok((length, source)) = udp_socket.recv_from(&mut packet) else {
                break;
            };
            if length < 20 {
                continue;
            }
            if let Some(peer) = peers
                .iter_mut()
                .find(|p| packet[4..20] == p.udp.offer.token)
                && let Some(ClientMessage::Acknowledge {
                    snapshot,
                    commands_retired,
                }) = peer.udp.receive::<ClientMessage>(&packet[..length], source)
                && let Some(sender) = &mut peer.replication
                && sender.acknowledge(snapshot).is_ok()
            {
                peer.commands.retire(commands_retired);
            }
        }
        for (index, peer) in peers.iter_mut().enumerate() {
            if peer.flush_update(true).is_err() {
                peer.connection.discard_writes();
                peer.replication = None;
                ended[index] = true;
            } else if !ended[index]
                && peer.replication.as_ref().is_none_or(|s| {
                    if final_result {
                        !s.unacknowledged()
                    } else {
                        !s.pending()
                    }
                })
            {
                let _ = peer.connection.send(&ServerMessage::End(reason.clone()));
                ended[index] = true;
            }
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    if let Some(path) = replay_path {
        save_replay(path, server.replay())?;
    }
    result
}
