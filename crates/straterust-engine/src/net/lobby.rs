//! Ready-player lobby before the normal filtered simulation protocol begins.
use super::*;
use crate::multiplayer::{Catalog, Settings, SlotKind};
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Player {
    pub ready: bool,
    pub name: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct View {
    pub host: usize,
    pub running: bool,
    pub map: usize,
    pub settings: Settings,
    pub players: Vec<Option<Player>>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Request {
    Hello {
        protocol: u32,
        identity: GameplayIdentity,
        host: bool,
        name: String,
    },
    Configure {
        map: usize,
        settings: Settings,
    },
    Faction(Option<usize>),
    Color(usize),
    Team(u8),
    Slot(usize),
    Assign {
        from: usize,
        to: usize,
    },
    Name(String),
    Chat {
        channel: chat::Channel,
        text: String,
    },
    Ready(bool),
    Start,
    Ping,
    Leave,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Response {
    State {
        slot: usize,
        view: View,
    },
    Starting,
    Launch {
        player: PlayerId,
        faction: usize,
        tileset: usize,
        reservation: Option<(usize, [u8; 16])>,
    },
    Rejected(String),
    Chat(chat::Message),
    End(String),
}
pub struct Client {
    connection: Connection,
    pub slot: Option<usize>,
    heartbeat: Instant,
}
impl Client {
    pub fn connect(address: SocketAddr, definitions: &World, host: bool) -> Result<Self> {
        Self::connect_named(address, definitions, host, "Player")
    }
    pub fn connect_named(
        address: SocketAddr,
        definitions: &World,
        host: bool,
        name: &str,
    ) -> Result<Self> {
        let name = chat::name(name)?;
        let deadline = Instant::now() + Duration::from_secs(3);
        let stream = loop {
            match TcpStream::connect_timeout(&address, Duration::from_millis(300)) {
                Ok(stream) => break stream,
                Err(_) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(25))
                }
                Err(error) => return Err(error).context("cannot connect to lobby"),
            }
        };
        let mut connection = Connection::new(stream)?;
        connection.send(&ClientMessage::Lobby(Request::Hello {
            protocol: PROTOCOL_VERSION,
            identity: GameplayIdentity::of(definitions),
            host,
            name,
        }))?;
        Ok(Self {
            connection,
            slot: None,
            heartbeat: Instant::now(),
        })
    }
    pub fn send(&mut self, request: Request) -> Result<()> {
        self.connection.send(&ClientMessage::Lobby(request))
    }
    pub fn poll(&mut self) -> Result<Option<Response>> {
        self.connection.flush()?;
        if self.heartbeat.elapsed() >= Duration::from_secs(1) {
            self.send(Request::Ping)?;
            self.heartbeat = Instant::now();
        }
        ensure!(
            self.connection.last_received.elapsed() < PEER_TIMEOUT,
            "lobby host timed out"
        );
        let response = match self.connection.receive::<ServerMessage>()? {
            Some(ServerMessage::Lobby(response)) => Some(response),
            Some(ServerMessage::End(reason)) => Some(Response::End(reason)),
            Some(_) => bail!("unexpected lobby message"),
            None => None,
        };
        if let Some(Response::State { slot, .. }) = &response {
            ensure!(*slot <= 8, "invalid lobby slot");
            self.slot = Some(*slot);
        }
        Ok(response)
    }
}
struct Peer {
    connection: Connection,
    slot: Option<usize>,
    host: bool,
    chat_gate: chat::Gate,
}
impl Peer {
    fn send(&mut self, response: Response) -> Result<()> {
        if let Err(error) = self.connection.send(&ServerMessage::Lobby(response)) {
            log::warn!("lobby slot {:?} delivery failed: {error:#}", self.slot);
            self.connection.last_received = Instant::now() - PEER_TIMEOUT;
        }
        Ok(())
    }
}
#[allow(clippy::too_many_arguments)]
pub fn host(
    listener: TcpListener,
    root: PathBuf,
    catalog: Catalog,
    map: usize,
    mut settings: Settings,
    seed: u64,
    cancelled: Arc<AtomicBool>,
    replay: Option<PathBuf>,
    traffic: Option<Arc<Traffic>>,
) -> Result<()> {
    catalog.validate_settings(map, &settings)?;
    catalog.assign_colors(&mut settings)?;
    let definition = crate::content::Package::client_definitions(
        &crate::multiplayer::package_path(&root, &catalog.maps[map].package)?,
    )?;
    let identity = GameplayIdentity::of(&definition);
    let mut advertisement = lan::Advertiser::new(
        &catalog.maps[map].title,
        listener.local_addr()?.port(),
        identity.clone(),
    )
    .ok();
    listener.set_nonblocking(true)?;
    let mut peers: Vec<Peer> = Vec::new();
    let mut view = View {
        map,
        host: 0,
        running: false,
        players: vec![None; settings.slots.len()],
        settings,
    };
    let mut heartbeat = Instant::now();
    while !cancelled.load(Ordering::Acquire) {
        if let Some(advertisement) = &mut advertisement {
            advertisement.status(
                false,
                view.settings
                    .slots
                    .iter()
                    .enumerate()
                    .filter(|(i, s)| s.kind == SlotKind::Open && view.players[*i].is_none())
                    .count(),
            )?;
            let _ = advertisement.poll();
        }
        loop {
            match listener.accept() {
                Ok((stream, _)) if peers.len() < 8 => {
                    let Ok(connection) = Connection::new(stream) else {
                        continue;
                    };
                    peers.push(Peer {
                        connection,
                        slot: None,
                        host: false,
                        chat_gate: chat::Gate::default(),
                    });
                }
                Ok((stream, _)) => {
                    let Ok(mut c) = Connection::new(stream) else {
                        continue;
                    };
                    let _ = c.send(&ServerMessage::Lobby(Response::End("Lobby is full".into())));
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::Interrupted
                            | std::io::ErrorKind::ConnectionAborted
                            | std::io::ErrorKind::ConnectionReset
                    ) =>
                {
                    continue;
                }
                Err(e) => return Err(e).context("lobby accept failed"),
            }
        }
        let mut dirty = false;
        let mut start = false;
        let mut assignments = Vec::new();
        let mut drop_peers = Vec::new();
        let mut chats = Vec::new();
        for (index, peer) in peers.iter_mut().enumerate() {
            for _ in 0..32 {
                let request = match peer.connection.receive::<ClientMessage>() {
                    Ok(Some(ClientMessage::Lobby(request))) => request,
                    Ok(Some(_)) => {
                        drop_peers.push(index);
                        break;
                    }
                    Ok(None) => break,
                    Err(_) => {
                        drop_peers.push(index);
                        break;
                    }
                };
                let slot = peer.slot;
                let result = (|| -> Result<()> {
                    match request {
                        Request::Hello {
                            protocol,
                            identity: client,
                            host,
                            name,
                        } => {
                            ensure!(
                                slot.is_none()
                                    && protocol == PROTOCOL_VERSION
                                    && identity.same_rules(&client),
                                "incompatible game or protocol"
                            );
                            let slot = if host {
                                ensure!(view.players[view.host].is_none(), "host slot occupied");
                                view.host
                            } else {
                                (0..view.players.len())
                                    .find(|i| {
                                        view.settings.slots[*i].kind == SlotKind::Open
                                            && view.players[*i].is_none()
                                    })
                                    .context("no open player slots")?
                            };
                            let name = chat::name(&name)?;
                            peer.slot = Some(slot);
                            peer.host = host;
                            view.players[slot] = Some(Player { ready: false, name });
                            dirty = true;
                        }
                        Request::Configure { map, mut settings } => {
                            ensure!(peer.host, "only the host can change match settings");
                            ensure!(
                                view.players.iter().enumerate().all(|(i, p)| p.is_none()
                                    || settings
                                        .slots
                                        .get(i)
                                        .is_some_and(|s| s.kind == SlotKind::Open)),
                                "occupied slots cannot be closed"
                            );
                            if view.map != map {
                                for (i, slot) in settings.slots.iter_mut().enumerate() {
                                    slot.color = view
                                        .players
                                        .get(i)
                                        .and_then(Option::as_ref)
                                        .and_then(|_| view.settings.slots[i].color);
                                }
                            }
                            catalog.validate_settings(map, &settings)?;
                            catalog.assign_colors(&mut settings)?;
                            view.players.resize(settings.slots.len(), None);
                            if view.map != map {
                                advertisement = lan::Advertiser::new(
                                    &catalog.maps[map].title,
                                    listener.local_addr()?.port(),
                                    identity.clone(),
                                )
                                .ok();
                            }
                            view.map = map;
                            view.settings = settings;
                            dirty = true;
                        }
                        Request::Slot(target) => {
                            let previous = slot.context("not joined")?;
                            ensure!(
                                target < view.players.len()
                                    && view.settings.slots[target].kind != SlotKind::Closed,
                                "slot is not open"
                            );
                            ensure!(
                                target == previous || view.players[target].is_none(),
                                "slot is occupied"
                            );
                            if target != previous {
                                view.settings.slots[target].kind = SlotKind::Open;
                                let player = view.players[previous].take().unwrap();
                                view.players[target] = Some(player);
                                let color = view.settings.slots[previous].color;
                                view.settings.slots[previous].color =
                                    view.settings.slots[target].color;
                                view.settings.slots[target].color = color;
                                peer.slot = Some(target);
                                if peer.host {
                                    view.host = target;
                                }
                                dirty = true;
                            }
                        }
                        Request::Faction(faction) => {
                            let slot = slot.context("not joined")?;
                            ensure!(
                                faction.is_none_or(|f| f < catalog.factions.len()),
                                "unknown race"
                            );
                            view.settings.slots[slot].faction = faction;
                            dirty = true;
                        }
                        Request::Color(color) => {
                            let slot = slot.context("not joined")?;
                            let mut settings = view.settings.clone();
                            catalog.choose_color(&mut settings, slot, color)?;
                            catalog.validate_settings(view.map, &settings)?;
                            view.settings = settings;
                            dirty = true;
                        }
                        Request::Assign { from, to } => {
                            ensure!(peer.host, "only the host can assign players");
                            ensure!(
                                from < view.players.len()
                                    && to < view.players.len()
                                    && view.players[from].is_some(),
                                "unknown player position"
                            );
                            assignments.push((from, to));
                            dirty = true;
                        }
                        Request::Team(team) => {
                            let slot = slot.context("not joined")?;
                            ensure!(team < 8, "unknown team");
                            ensure!(
                                has_teams(catalog.modes[view.settings.mode]),
                                "this match is free for all"
                            );
                            view.settings.slots[slot].team = team;
                            dirty = true;
                        }
                        Request::Ready(ready) => {
                            view.players[slot.context("not joined")?]
                                .as_mut()
                                .unwrap()
                                .ready = ready;
                            dirty = true;
                        }
                        Request::Name(name) => {
                            let slot = slot.context("not joined")?;
                            let name = chat::name(&name)?;
                            peer.chat_gate.accept()?;
                            view.players[slot].as_mut().unwrap().name = name;
                            dirty = true;
                        }
                        Request::Chat { channel, text } => {
                            let slot = slot.context("not joined")?;
                            let text = chat::text(&text)?;
                            peer.chat_gate.accept()?;
                            let recipients: Vec<_> = (0..view.players.len())
                                .filter(|i| {
                                    view.players[*i].is_some()
                                        && (channel == chat::Channel::All
                                            || same_team(
                                                catalog.modes[view.settings.mode],
                                                &view.settings,
                                                slot,
                                                *i,
                                            ))
                                })
                                .collect();
                            chats.push((
                                recipients,
                                chat::Message {
                                    player: PlayerId(slot as u16),
                                    name: view.players[slot].as_ref().unwrap().name.clone(),
                                    channel,
                                    text,
                                },
                            ));
                        }
                        Request::Start => {
                            ensure!(peer.host, "only the host can start");
                            ensure!(
                                view.players.iter().flatten().all(|p| p.ready),
                                "all players must be ready"
                            );
                            start = true;
                        }
                        Request::Ping => {}
                        Request::Leave => drop_peers.push(index),
                    }
                    Ok(())
                })();
                if let Err(error) = result {
                    let _ = peer.send(Response::Rejected(error.to_string()));
                    dirty = true;
                }
            }
            if peer.connection.last_received.elapsed() > PEER_TIMEOUT {
                drop_peers.push(index);
            }
        }
        for (recipients, message) in chats {
            for peer in peers
                .iter_mut()
                .filter(|p| p.slot.is_some_and(|s| recipients.contains(&s)))
            {
                let _ = peer.send(Response::Chat(message.clone()));
            }
        }
        drop_peers.sort();
        drop_peers.dedup();
        for index in drop_peers.into_iter().rev() {
            if let Some(slot) = peers[index].slot {
                ensure!(!peers[index].host, "host left lobby");
                view.players[slot] = None;
                dirty = true;
            }
            peers.remove(index);
        }
        for (from, to) in assignments {
            if view.players[from].is_none() {
                continue;
            }
            view.players.swap(from, to);
            let color = view.settings.slots[from].color;
            view.settings.slots[from].color = view.settings.slots[to].color;
            view.settings.slots[to].color = color;
            for slot in [from, to] {
                if view.players[slot].is_some() {
                    view.settings.slots[slot].kind = SlotKind::Open;
                }
            }
            if view.host == from {
                view.host = to;
            } else if view.host == to {
                view.host = from;
            }
            for peer in &mut peers {
                if peer.slot == Some(from) {
                    peer.slot = Some(to);
                } else if peer.slot == Some(to) {
                    peer.slot = Some(from);
                }
            }
            catalog.assign_colors(&mut view.settings)?;
            start = false;
        }
        if start {
            peers.retain(|p| p.slot.is_some());
            // Report progress before preparing the map, which can take seconds.
            for peer in &mut peers {
                let _ = peer.send(Response::Starting);
            }
            let occupied: Vec<_> = view
                .players
                .iter()
                .enumerate()
                .filter(|(_, p)| p.is_some())
                .map(|(i, _)| i)
                .collect();
            match catalog.prepare(&root, view.map, &view.settings, &occupied, seed) {
                Ok(prepared) => {
                    let transfer = prepared.map_transfer()?;
                    for peer in &mut peers {
                        let slot = peer.slot.context("unassigned lobby peer")?;
                        let n = occupied.iter().position(|i| *i == slot).unwrap();
                        peer.send(Response::Launch {
                            player: prepared.humans[n],
                            faction: prepared.factions[slot],
                            reservation: None,
                            tileset: view
                                .settings
                                .tileset
                                .unwrap_or(usize::from(catalog.maps[view.map].tileset)),
                        })?;
                    }
                    let deadline = Instant::now() + Duration::from_secs(3);
                    while peers.iter().any(|p| p.connection.has_pending_writes())
                        && Instant::now() < deadline
                    {
                        for peer in &mut peers {
                            if peer.connection.flush().is_err() {
                                peer.connection.last_received = Instant::now() - PEER_TIMEOUT;
                            }
                        }
                        std::thread::sleep(Duration::from_millis(2));
                    }
                    drop(peers);
                    drop(advertisement);
                    let tileset = view
                        .settings
                        .tileset
                        .unwrap_or(usize::from(catalog.maps[view.map].tileset));
                    let context =
                        super::host::RunningLobby::new(view, &occupied, &prepared, tileset);
                    return super::host::run_configured(
                        listener,
                        prepared.world,
                        seed,
                        cancelled,
                        replay.as_deref(),
                        transfer,
                        prepared.humans,
                        prepared.tick_ms,
                        traffic,
                        Some(context),
                    );
                }
                Err(error) => {
                    for peer in &mut peers {
                        let _ = peer.send(Response::Rejected(error.to_string()));
                    }
                }
            }
        }
        if dirty || heartbeat.elapsed() >= Duration::from_secs(1) {
            for peer in &mut peers {
                if let Some(slot) = peer.slot {
                    peer.send(Response::State {
                        slot,
                        view: view.clone(),
                    })?;
                }
            }
            heartbeat = Instant::now();
        }
        for peer in &mut peers {
            if peer.connection.flush().is_err() {
                peer.connection.last_received = Instant::now() - PEER_TIMEOUT;
            }
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    Ok(())
}

pub fn has_teams(mode: crate::multiplayer::GameType) -> bool {
    use crate::multiplayer::GameType::*;
    matches!(
        mode,
        Teams | TopVsBottom | TeamMelee | TeamFreeForAll | TeamCaptureTheFlag
    )
}
fn same_team(mode: crate::multiplayer::GameType, settings: &Settings, a: usize, b: usize) -> bool {
    use crate::multiplayer::GameType::*;
    a == b
        || match mode {
            TopVsBottom => {
                (a < settings.slots.len().div_ceil(2)) == (b < settings.slots.len().div_ceil(2))
            }
            Teams | TeamMelee | TeamFreeForAll | TeamCaptureTheFlag => {
                settings.slots[a].team == settings.slots[b].team
            }
            _ => false,
        }
}
