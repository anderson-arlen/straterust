//! Network I/O stays off the window thread. Both connections deliver the engine
//! PlayerUpdate consumed by local sessions; this module owns no simulation.
use super::*;
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender, TryRecvError},
    },
};
use straterust_engine::{
    net::{self, RemoteClient, ServerMessage},
    session::PlayerUpdate,
};

enum Event {
    Connected {
        world: Box<World>,
        initial: PlayerUpdate,
        artwork: Option<straterust_engine::assets::MapArtwork>,
    },
    Message(ServerMessage),
}

enum Request {
    Command(Command),
    Pause(bool),
}

pub(super) struct NetworkWorker {
    requests: SyncSender<Request>,
    events: Receiver<Result<Event>>,
    cancelled: Arc<AtomicBool>,
    pub(super) started: bool,
    pub(super) ended: bool,
    pub(super) player: PlayerId,
    threads: Vec<std::thread::JoinHandle<()>>,
}

impl NetworkWorker {
    pub(super) fn start(
        address: SocketAddr,
        definitions: World,
        host: Option<PathBuf>,
        seed: u64,
        replay: Option<PathBuf>,
    ) -> Result<Self> {
        let hosting = host.is_some();
        let mut threads = Vec::new();
        let cancelled = Arc::new(AtomicBool::new(false));
        let mut connect_address = address;
        if let Some(directory) = host {
            let listener = TcpListener::bind(address).context("cannot listen for LAN match")?;
            connect_address = listener.local_addr()?;
            if connect_address.ip().is_unspecified() {
                connect_address.set_ip(IpAddr::V4(Ipv4Addr::LOCALHOST));
            }
            let stop = Arc::clone(&cancelled);
            threads.push(
                std::thread::Builder::new()
                    .name("straterust-network-host".into())
                    .spawn(move || {
                        let result = Package::load(&directory)
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
                                )
                            });
                        if let Err(error) = result {
                            log::info!("LAN host ended: {error:#}");
                        }
                    })?,
            );
        }
        let player = PlayerId(if hosting { 0 } else { 1 });
        let (requests, incoming) = mpsc::sync_channel(64);
        let (outgoing, events) = mpsc::sync_channel(64);
        let stop = Arc::clone(&cancelled);
        threads.push(
            std::thread::Builder::new()
                .name("straterust-network-client".into())
                .spawn(move || {
                    let run = || -> Result<()> {
                        let (mut client, initial) =
                            RemoteClient::connect(connect_address, &definitions, player)?;
                        let public = client.map.definitions(&definitions, player)?;
                        outgoing.send(Ok(Event::Connected {
                            world: Box::new(public),
                            initial,
                            artwork: client.map.artwork.take(),
                        }))?;
                        while !stop.load(Ordering::Acquire) {
                            for _ in 0..64 {
                                match incoming.try_recv() {
                                    Ok(Request::Command(command)) => client.command(command)?,
                                    Ok(Request::Pause(paused)) => client.pause(paused)?,
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
                                    if outgoing.send(Ok(Event::Message(message))).is_err() {
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
            cancelled,
            started: false,
            ended: false,
            player,
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

    pub(super) fn pause(&self, paused: bool) -> Result<()> {
        ensure!(
            self.player == PlayerId(0),
            "only the host player can pause the match"
        );
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

impl Drop for NetworkWorker {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
    }
}

impl App {
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
        )?;
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
        loop {
            let event = self.network.as_mut().context("no network session")?.poll();
            let message = match event {
                Ok(Some(message)) => message,
                Ok(None) => break,
                Err(error) => {
                    self.status = format!("Match ended: {error:#}");
                    self.network.as_mut().unwrap().ended = true;
                    self.paused = true;
                    break;
                }
            };
            match message {
                Event::Connected {
                    world,
                    initial,
                    artwork,
                } => {
                    self.world = initial.view.into_world(&world)?;
                    if let Some(report) = &initial.result {
                        report.validate(&self.world)?;
                    }
                    self.match_result = initial.result;
                    self.map_art = artwork
                        .as_ref()
                        .map(|art| art.decode(&self.world))
                        .transpose()?;
                    if let Some(surface) = &mut self.surface {
                        surface.reset_assets();
                    }
                    self.initial_world = self.world.clone();
                    self.visuals = Visuals::new(&self.world);
                    self.audio.reset(&self.world);
                    self.selected.clear();
                    let position = home_position(&self.world);
                    self.camera.x = f64::from(position.x);
                    self.camera.y = f64::from(position.y);
                    self.status = "Connected. Waiting for both players.".into();
                }
                Event::Message(ServerMessage::Update(PlayerUpdate {
                    view,
                    outcomes,
                    result,
                    ..
                })) => {
                    ensure!(
                        view.tick.0 == self.world.tick().0 + 1,
                        "network tick sequence mismatch"
                    );
                    self.world = view.into_world(&self.world)?;
                    if let Some(report) = &result {
                        report.validate(&self.world)?;
                    }
                    self.match_result = result;
                    self.observe_tick(outcomes)?;
                }
                Event::Message(ServerMessage::Status { started, paused }) => {
                    self.network.as_mut().unwrap().started = started;
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
                Event::Message(ServerMessage::Rejected(reason)) => {
                    self.status = format!("Order rejected: {reason}");
                }
                Event::Message(ServerMessage::End(reason)) => {
                    if self.match_result.is_none() {
                        self.status = format!("Match ended: {reason}");
                    }
                    self.network.as_mut().unwrap().ended = true;
                    self.paused = true;
                }
                Event::Message(_) => bail!("unexpected session message after connection"),
            }
        }
        Ok(())
    }
}
