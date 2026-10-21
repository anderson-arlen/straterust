//! Network I/O stays off the window thread. Both connections deliver the engine
//! PlayerUpdate consumed by local sessions; this module owns no simulation.
use super::*;
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender, TryRecvError},
    },
};
use straterust_engine::{
    net::{self, RemoteClient, ServerMessage},
    session::PlayerUpdate,
};

#[cfg(test)]
mod gap_tests;
mod prediction;

enum Event {
    Starting,
    Connected {
        world: Box<World>,
        initial: PlayerUpdate,
        artwork: Option<straterust_engine::assets::MapArtwork>,
        presentation: Option<PathBuf>,
        scenario: Option<PathBuf>,
    },
    Lobby(usize, net::lobby::View),
    Notice(String),
    InputRejected(u64, String),
    Message(ServerMessage),
}

enum Request {
    Command(Command),
    Pause(bool),
    Lobby(net::lobby::Request),
    Chat(net::chat::Channel, String),
}

#[derive(Clone)]
pub(super) struct LobbySetup {
    pub root: PathBuf,
    pub catalog: straterust_engine::multiplayer::Catalog,
    pub map: usize,
    pub settings: straterust_engine::multiplayer::Settings,
}

pub(super) struct NetworkWorker {
    requests: SyncSender<Request>,
    events: Receiver<Result<Event>>,
    latest_update: Arc<std::sync::Mutex<Option<straterust_engine::session::PlayerUpdate>>>,
    pub(super) traffic: Arc<net::Traffic>,
    pub(super) clock: Arc<std::sync::Mutex<net::clock::ClockSync>>,
    pub(super) prediction: Option<prediction::Prediction>,
    hosting: bool,
    applied: Arc<AtomicU64>,
    command_acks: Arc<std::sync::Mutex<net::commands::Acknowledgements>>,
    cancelled: Arc<AtomicBool>,
    pub(super) started: bool,
    pub(super) starting: bool,
    pub(super) ended: bool,
    pub(super) player: PlayerId,
    pub(super) lobby: Option<(usize, net::lobby::View)>,
    threads: Vec<std::thread::JoinHandle<()>>,
}

impl NetworkWorker {
    pub(super) fn start(
        address: SocketAddr,
        definitions: World,
        host: Option<PathBuf>,
        seed: u64,
        replay: Option<PathBuf>,
        name: String,
    ) -> Result<Self> {
        Self::start_with_lobby(address, definitions, host, seed, replay, None, name)
    }

    #[allow(clippy::too_many_arguments)]
    fn start_with_lobby(
        address: SocketAddr,
        definitions: World,
        host: Option<PathBuf>,
        seed: u64,
        replay: Option<PathBuf>,
        lobby: Option<LobbySetup>,
        name: String,
    ) -> Result<Self> {
        let hosting = host.is_some();
        let starting = lobby.is_none();
        let mut threads = Vec::new();
        let cancelled = Arc::new(AtomicBool::new(false));
        let mut connect_address = address;
        let (requests, incoming) = mpsc::sync_channel(64);
        let (outgoing, events) = mpsc::sync_channel(64);
        let latest_update = Arc::new(std::sync::Mutex::new(None));
        let traffic = Arc::new(net::Traffic::default());
        let clock = Arc::new(std::sync::Mutex::new(net::clock::ClockSync::default()));
        let applied = Arc::new(AtomicU64::new(0));
        let command_acks = Arc::new(std::sync::Mutex::new(
            net::commands::Acknowledgements::default(),
        ));
        if let Some(directory) = host {
            let listener = TcpListener::bind(address).context("cannot listen for LAN match")?;
            connect_address = listener.local_addr()?;
            if connect_address.ip().is_unspecified() {
                connect_address.set_ip(IpAddr::V4(Ipv4Addr::LOCALHOST));
            }
            let stop = Arc::clone(&cancelled);
            let setup = lobby.clone();
            let report = outgoing.clone();
            let host_traffic = Arc::clone(&traffic);
            threads.push(
                std::thread::Builder::new()
                    .name("straterust-network-host".into())
                    .spawn(move || {
                        let result = if let Some(setup) = setup {
                            net::lobby::host(
                                listener,
                                setup.root,
                                setup.catalog,
                                setup.map,
                                setup.settings,
                                seed,
                                stop,
                                replay,
                                Some(host_traffic),
                            )
                        } else {
                            Package::load(&directory)
                                .and_then(|package| package.world(seed))
                                .and_then(|world| {
                                    let map = net::MapTransfer::load(&directory, &world)?;
                                    net::run_host_with_map(
                                        listener,
                                        world,
                                        seed,
                                        stop,
                                        replay.as_deref(),
                                        map,
                                        Some(host_traffic),
                                    )
                                })
                        };
                        if let Err(error) = result {
                            log::info!("LAN host ended: {error:#}");
                            let _ = report.send(Err(error));
                        }
                    })?,
            );
        }
        let player = PlayerId(if hosting { 0 } else { 1 });
        let stop = Arc::clone(&cancelled);
        let snapshot_sink = Arc::clone(&latest_update);
        let client_traffic = Arc::clone(&traffic);
        let client_clock = Arc::clone(&clock);
        let client_applied = Arc::clone(&applied);
        let client_command_acks = Arc::clone(&command_acks);
        threads.push(
            std::thread::Builder::new()
                .name("straterust-network-client".into())
                .spawn(move || {
                    let mut name = name;
                    let run = || -> Result<()> {
                        let mut assigned = player;
                        let mut reservation = None;
                        let mut presentation = None;
                        let mut scenario = None;
                        if let Some(setup) = lobby {
                            let mut current_map = setup.map;
                            let mut current_mode = setup.catalog.modes[setup.settings.mode];
                            let mut client = net::lobby::Client::connect_named(
                                connect_address,
                                &definitions,
                                hosting,
                                &name,
                            )?;
                            'waiting: while !stop.load(Ordering::Acquire) {
                                for _ in 0..64 {
                                    match incoming.try_recv() {
                                        Ok(Request::Lobby(request)) => {
                                            if let net::lobby::Request::Name(value) = &request { name = value.clone(); }
                                            client.send(request)?;
                                        }
                                        Ok(Request::Chat(channel, text)) => client.send(net::lobby::Request::Chat { channel, text })?,
                                        Ok(_) => {}
                                        Err(TryRecvError::Empty) => break,
                                        Err(TryRecvError::Disconnected) => return Ok(()),
                                    }
                                }
                                for _ in 0..64 {
                                    match client.poll()? {
                                        Some(net::lobby::Response::State { slot, view }) => {
                                            setup
                                                .catalog
                                                .validate_settings(view.map, &view.settings)?;
                                            ensure!(
                                                view.players.len() == view.settings.slots.len()
                                                    && (slot < view.players.len() || view.running && slot == view.players.len()),
                                                "invalid lobby players"
                                            );
                                            current_map = view.map;
                                            current_mode = setup.catalog.modes[view.settings.mode];
                                            outgoing.send(Ok(Event::Lobby(slot, view)))?;
                                        }
                                        Some(net::lobby::Response::Starting) => {
                                            outgoing.send(Ok(Event::Starting))?;
                                        }
                                        Some(net::lobby::Response::Launch {
                                            player,
                                            faction,
                                            tileset, reservation: ticket,
                                        }) => {
                                            reservation = ticket;
                                            assigned = player;
                                            let faction = setup
                                                .catalog
                                                .factions
                                                .get(faction)
                                                .context("invalid assigned race")?;
                                            let path = if faction.tileset_presentations.is_empty() {
                                                &faction.presentation
                                            } else {
                                                faction
                                                    .tileset_presentations
                                                    .get(tileset)
                                                    .context("invalid assigned terrain")?
                                            };
                                            presentation =
                                                Some(straterust_engine::multiplayer::package_path(
                                                    &setup.root,
                                                    path,
                                                )?);
                                            if current_mode == straterust_engine::multiplayer::GameType::UseMapSettings {
                                                scenario = Some(straterust_engine::multiplayer::package_path(
                                                    &setup.root,
                                                    &setup.catalog.maps[current_map].package,
                                                )?);
                                            }
                                            break 'waiting;
                                        }
                                        Some(net::lobby::Response::Rejected(reason)) => {
                                            outgoing.send(Ok(Event::Notice(reason)))?;
                                        }
                                        Some(net::lobby::Response::Chat(message)) => {
                                            outgoing.send(Ok(Event::Message(ServerMessage::Chat(message))))?;
                                        }
                                        Some(net::lobby::Response::End(reason)) => {
                                            bail!("{reason}")
                                        }
                                        None => break,
                                    }
                                }
                                std::thread::sleep(Duration::from_millis(2));
                            }
                            if stop.load(Ordering::Acquire) {
                                return Ok(());
                            }
                        }
                        let (mut client, initial) =
                            RemoteClient::connect_reserved(connect_address, &definitions, assigned, (!hosting).then_some(client_traffic), reservation)?;
                        client.set_name(&name)?;
                        client.clock = client_clock;
                        let public = client.map.definitions(&definitions, assigned)?;
                        outgoing.send(Ok(Event::Connected {
                            world: Box::new(public),
                            initial,
                            artwork: client.map.artwork.take(),
                            presentation,
                            scenario,
                        }))?;
                        let mut last_resync = Instant::now();
                        while !stop.load(Ordering::Acquire) {
                            // One cumulative ACK for the newest view accepted by
                            // the window. Rejected/coalesced frames need no ACK.
                            client.acknowledge(client_applied.load(Ordering::Acquire))?;
                            client.acknowledge_commands(&client_command_acks.lock().unwrap())?;
                            for _ in 0..64 {
                                match incoming.try_recv() {
                                    Ok(Request::Command(command)) => {
                                        let sequence = command.sequence;
                                        if let Err(error) = client.command(command) {
                                            outgoing.send(Ok(Event::InputRejected(sequence, format!("Command not sent: {error:#}"))))?;
                                        }
                                    }
                                    Ok(Request::Pause(paused)) => { if let Err(error) = client.pause(paused) { outgoing.send(Ok(Event::Notice(format!("Pause not sent: {error:#}"))))?; } },
                                    Ok(Request::Lobby(_)) => {}
                                    Ok(Request::Chat(channel, text)) => { if let Err(error) = client.chat(channel, &text) { outgoing.send(Ok(Event::Notice(format!("Chat not sent: {error:#}"))))?; } },
                                    Err(TryRecvError::Empty) => break,
                                    Err(TryRecvError::Disconnected) => {
                                        client.leave();
                                        return Ok(());
                                    }
                                }
                            }
                            for _ in 0..64 {
                                if let Some(message) = client.poll()? {
                                    let ended = matches!(message, ServerMessage::End(_));
                                    if let ServerMessage::Update(mut update) = message {
                                        let fresh = hosting || update.time.is_some_and(|stamp| {
                                            let mut clock = client.clock.lock().unwrap();
                                            let now = clock.now_ms();
                                            let limit = clock.max_frame_age_ms;
                                            clock.accepts_frame(stamp, now, limit)
                                        });
                                        if !fresh && update.result.is_none() {
                                            if last_resync.elapsed() >= Duration::from_millis(500) {
                                                client.resync()?;
                                                last_resync = Instant::now();
                                            }
                                            continue;
                                        }
                                        if !fresh {
                                            update.view.shots.clear();
                                            update.view.weapon_feedback.clear();
                                        }
                                        net::retain_latest(
                                            &mut *snapshot_sink.lock().map_err(|_| {
                                                anyhow::anyhow!("network snapshot lock poisoned")
                                            })?,
                                            update,
                                        );
                                    } else if outgoing.send(Ok(Event::Message(message))).is_err() {
                                        client.leave();
                                        return Ok(());
                                    }
                                    if ended {
                                        return Ok(());
                                    }
                                } else {
                                    break;
                                }
                            }
                            std::thread::sleep(Duration::from_millis(2));
                        }
                        client.leave();
                        Ok(())
                    };
                    if let Err(error) = run() {
                        let _ = outgoing.send(Err(error));
                    }
                })?,
        );
        Ok(Self {
            requests,
            events,
            latest_update,
            traffic,
            clock,
            prediction: None,
            hosting,
            applied,
            command_acks,
            cancelled,
            started: false,
            starting,
            ended: false,
            player,
            lobby: None,
            threads,
        })
    }

    pub(super) fn finish(mut self) -> Result<()> {
        self.cancelled.store(true, Ordering::Release);
        let (sender, _) = mpsc::sync_channel(1);
        let (_, receiver) = mpsc::sync_channel(1);
        drop(std::mem::replace(&mut self.requests, sender));
        drop(std::mem::replace(&mut self.events, receiver));
        for thread in self.threads.drain(..) {
            thread
                .join()
                .map_err(|_| anyhow::anyhow!("network worker failed during shutdown"))?;
        }
        Ok(())
    }

    pub(super) fn command(&self, command: Command) -> Result<()> {
        ensure!(self.started && !self.ended, "network match has not started");
        self.requests
            .try_send(Request::Command(command))
            .context("network input queue full")
    }

    pub(super) fn lobby_request(&self, request: net::lobby::Request) -> Result<()> {
        ensure!(!self.started && !self.ended, "lobby is closed");
        self.requests
            .try_send(Request::Lobby(request))
            .context("lobby input queue full")
    }

    pub(super) fn chat(&self, channel: net::chat::Channel, text: &str) -> Result<()> {
        ensure!(!self.ended, "match has ended");
        self.requests
            .try_send(Request::Chat(channel, net::chat::text(text)?))
            .context("chat input queue full")
    }

    pub(super) fn pause(&self, paused: bool) -> Result<()> {
        ensure!(self.hosting, "only the host player can pause the match");
        self.requests
            .try_send(Request::Pause(paused))
            .context("network input queue full")
    }

    fn poll(&mut self) -> Result<Option<Event>> {
        if self.ended {
            return Ok(None);
        }
        match self.events.try_recv() {
            Ok(event) => event.map(Some),
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => bail!("network worker disconnected"),
        }
    }
}

#[cfg(test)]
mod tests;

impl Drop for NetworkWorker {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
    }
}

impl App {
    pub(super) fn start_lobby(
        &mut self,
        setup: LobbySetup,
        address: SocketAddr,
        hosting: bool,
    ) -> Result<()> {
        let worker = NetworkWorker::start_with_lobby(
            address,
            self.initial_world.snapshot(),
            hosting.then(|| setup.root.clone()),
            self.config.seed,
            None,
            Some(setup),
            self.config.player_name.clone(),
        )?;
        worker.clock.lock().unwrap().max_frame_age_ms = self.config.max_server_frame_age_ms;
        self.simulation = None;
        self.network = Some(worker);
        self.queue = CommandQueue::default();
        self.campaign = None;
        self.mission_ui = None;
        self.paused = true;
        self.status = "Connecting to multiplayer lobby...".into();
        Ok(())
    }
    pub(super) fn start_network(
        &mut self,
        directory: &Path,
        address: SocketAddr,
        hosting: bool,
        replay: Option<PathBuf>,
    ) -> Result<()> {
        ensure!(
            self.playback_end.is_none(),
            "scenario playback cannot join a LAN match"
        );
        let worker = NetworkWorker::start(
            address,
            self.initial_world.snapshot(),
            hosting.then(|| directory.to_path_buf()),
            self.config.seed,
            replay,
            self.config.player_name.clone(),
        )?;
        worker.clock.lock().unwrap().max_frame_age_ms = self.config.max_server_frame_age_ms;
        self.simulation = None;
        self.network = Some(worker);
        self.queue = CommandQueue::default();
        self.campaign = None;
        self.mission_ui = None;
        self.paused = true;
        self.status = if hosting {
            format!("Hosting {address}; waiting for another player.")
        } else {
            format!("Connecting to {address}...")
        };
        Ok(())
    }

    pub(super) fn advance_network(&mut self) -> Result<()> {
        let mut ended = None;
        loop {
            let event = self.network.as_mut().context("no network session")?.poll();
            let message = match event {
                Ok(Some(message)) => message,
                Ok(None) => break,
                Err(error) => {
                    log::warn!("LAN session ended: {error:#}");
                    ended = Some(format!("{error:#}"));
                    break;
                }
            };
            match message {
                Event::Starting => {
                    self.network.as_mut().unwrap().starting = true;
                    self.status = "Starting game...".into();
                    // Render the modal before handling a possibly already queued
                    // Connected event, which loads native presentation/media.
                    return Ok(());
                }
                Event::Connected {
                    world,
                    initial,
                    artwork,
                    presentation,
                    scenario,
                } => {
                    self.world = initial.view.into_world(&world)?;
                    self.network.as_mut().unwrap().player = self.world.view_player();
                    if let Some(directory) = presentation {
                        let colors = self
                            .assets
                            .as_ref()
                            .map(|a| a.manifest.player_colors.clone())
                            .unwrap_or_default();
                        self.assets = AssetPack::load(&directory)?;
                        if let Some(assets) = &mut self.assets {
                            assets.manifest.terrain_grid = None;
                            assets.map_images.clear();
                            assets.manifest.player_colors = colors;
                        }
                        self.presentation = read_ron(&directory.join("presentation.ron"))?;
                        self.presentation.validate()?;
                        self.load_media(&directory)?;
                    }
                    if let Some(directory) = scenario
                        && let Some(media) = &mut self.media
                    {
                        media.load_scenario(&directory)?;
                        self.audio.set_media(Some(media));
                        self.mission_ui = Some(mission::MissionUi::new(media, false));
                    }
                    if let Some(report) = &initial.result {
                        report.validate(&self.world)?;
                    }
                    self.match_result = initial.result;
                    if let Some(art) = &artwork
                        && let Some(assets) = &mut self.assets
                    {
                        assets.manifest.player_colors = art.player_colors.clone();
                    }
                    self.map_art = artwork
                        .as_ref()
                        .map(|art| art.decode(&self.world))
                        .transpose()?;
                    if let Some(surface) = &mut self.surface {
                        surface.reset_assets();
                    }
                    self.initial_world = self.world.clone();
                    self.network.as_mut().unwrap().prediction = None;
                    self.visuals = Visuals::new(&self.world);
                    self.visuals.confirm(&self.world);
                    self.audio.reset(&self.world);
                    self.selected.clear();
                    let position = home_position(&self.world);
                    self.camera.x = f64::from(position.x);
                    self.camera.y = f64::from(position.y);
                    self.status = "Connected. Waiting for players.".into();
                }
                Event::Lobby(slot, view) => {
                    self.network.as_mut().unwrap().lobby = Some((slot, view));
                }
                Event::Notice(reason) => {
                    self.network.as_mut().unwrap().starting = false;
                    self.status = reason;
                }
                Event::InputRejected(sequence, reason) => {
                    if let Some(prediction) = &mut self.network.as_mut().unwrap().prediction {
                        prediction.reject(sequence);
                    }
                    self.status = reason;
                }
                Event::Message(ServerMessage::Status { started, paused }) => {
                    self.network.as_mut().unwrap().started = started;
                    if started {
                        self.network.as_mut().unwrap().starting = false;
                    }
                    self.paused = !started || paused;
                    self.status = if !started {
                        "Waiting for another player."
                    } else if paused {
                        "Match paused by host. Space resumes."
                    } else {
                        "LAN match started."
                    }
                    .into();
                }
                Event::Message(ServerMessage::Notice(reason)) => {
                    log::info!("LAN notice: {reason}");
                    self.status = reason;
                }
                Event::Message(ServerMessage::Rejected(reason)) => {
                    self.status = format!("Request rejected: {reason}");
                }
                Event::Message(ServerMessage::Chat(message)) => self.chat.receive(message)?,
                Event::Message(ServerMessage::End(reason)) => {
                    log::info!("LAN session ended: {reason}");
                    ended = Some(reason);
                    break;
                }
                Event::Message(_) => bail!("unexpected session message after connection"),
            }
        }
        // Apply one newest full view per redraw, including the final view before
        // processing End. Chat/control messages never wait behind stale views.
        let latest = self
            .network
            .as_ref()
            .unwrap()
            .latest_update
            .lock()
            .map_err(|_| anyhow::anyhow!("network snapshot lock poisoned"))?
            .take();
        let mut changed = false;
        if let Some(mut update) = latest {
            let time = update.time.context("network update lacks timestamp")?;
            let fresh = self.network.as_ref().unwrap().hosting || {
                let mut clock = self.network.as_ref().unwrap().clock.lock().unwrap();
                let now = clock.now_ms();
                clock.accepts_frame(time, now, self.config.max_server_frame_age_ms)
            };
            if fresh || update.result.is_some() {
                update.retain_feedback_after(
                    self.network
                        .as_ref()
                        .unwrap()
                        .applied
                        .load(Ordering::Acquire),
                );
                let baseline = self
                    .network
                    .as_ref()
                    .unwrap()
                    .prediction
                    .as_ref()
                    .map_or(&self.initial_world, |prediction| &prediction.confirmed);
                let confirmed = update.view.into_world(baseline)?;
                if let Some(report) = &update.result {
                    report.validate(&confirmed)?;
                }
                self.match_result = update.result;
                let prediction = &mut self.network.as_mut().unwrap().prediction;
                self.world = if let Some(prediction) = prediction {
                    prediction.correct(confirmed, time, &update.command_acks)
                } else {
                    *prediction = Some(prediction::Prediction::new(confirmed.clone(), time));
                    confirmed
                };
                // Announcements/outcomes observe confirmed ticks only. Speculative
                // completions or hits must not play again after reconciliation.
                self.observe_feedback(update.outcomes)?;
                self.visuals.confirm(&self.world);
                *self.network.as_ref().unwrap().command_acks.lock().unwrap() = update.command_acks;
                self.network
                    .as_ref()
                    .unwrap()
                    .applied
                    .fetch_max(update.sequence, Ordering::Release);
                changed = true;
            }
        }
        if !self.paused && self.match_result.is_none() {
            let network = self.network.as_mut().unwrap();
            let server_ms = {
                let clock = network.clock.lock().unwrap();
                clock.server_ms(clock.now_ms())
            };
            if !network.hosting
                && let (Some(prediction), Some(server_ms)) = (&mut network.prediction, server_ms)
                && prediction.advance(&mut self.world, server_ms)?
            {
                changed = true;
            }
        }
        if changed {
            self.visuals.reconcile(&self.world);
        }
        if let Some(reason) = ended {
            if self.match_result.is_none() {
                self.status = format!("Match ended: {reason}");
            }
            self.network.as_mut().unwrap().ended = true;
            self.network.as_mut().unwrap().starting = false;
            self.paused = true;
        }
        Ok(())
    }
}
