//! Session transport. Local play uses `ServerSession` directly; TCP changes only
//! delivery. The server serializes PlayerUpdate, never World or State.
use crate::{
    session::{Handshake, PROTOCOL_VERSION, PlayerUpdate, Replay, ServerSession},
    sim::{Command, GameplayIdentity, PlayerId, World},
};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    net::{SocketAddr, TcpListener, TcpStream},
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
mod framing;
mod map_transfer;
pub mod replication;
mod snapshots;
pub use framing::{Connection, Traffic};
pub fn encoded_size(message: &impl Serialize) -> Result<usize> {
    framing::encode_frame(message).map(|frame| frame.len())
}
use map_transfer::{Download, MAP_CHUNK_BYTES};
pub use map_transfer::{MAX_MAP_BYTES, MapTransfer};
pub use snapshots::retain_latest;
pub mod chat;
pub mod clock;
pub mod commands;
pub mod datagram;
pub mod lan;
pub mod lobby;
mod reconnect;

pub const DEFAULT_PORT: u16 = 6112;
pub const PEER_TIMEOUT: Duration = Duration::from_secs(30);

pub use crate::session::Hello;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ClientMessage {
    Hello(Hello),
    Join {
        hello: Hello,
        slot: usize,
        ticket: [u8; 16],
    },
    Resume([u8; 16]),
    Lobby(lobby::Request),
    Command(commands::Input),
    Acknowledge {
        snapshot: u64,
        commands_retired: u64,
    },
    Probe(u64),
    Resync,
    MapReady(String),
    Pause(bool),
    Name(String),
    Chat {
        channel: chat::Channel,
        text: String,
    },
    Leave,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ServerMessage {
    MapBegin {
        length: usize,
        hash: String,
    },
    MapChunk {
        offset: usize,
        data: Vec<u8>,
    },
    Welcome {
        handshake: Handshake,
        initial: PlayerUpdate,
        udp: datagram::Offer,
    },
    Status {
        started: bool,
        paused: bool,
    },
    Update(PlayerUpdate),
    Delta(replication::Delta),
    Clock {
        pong: clock::Pong,
        tick_ms: u32,
    },
    Chat(chat::Message),
    Lobby(lobby::Response),
    Notice(String),
    Resumed,
    Rejected(String),
    End(String),
}

pub struct RemoteClient {
    connection: Connection,
    pub handshake: Handshake,
    pub map: MapTransfer,
    sequence: u64,
    applied: u64,
    acknowledged: u64,
    inputs: commands::Outbox,
    udp: datagram::Endpoint,
    pub datagram_offer: datagram::Offer,
    replica: replication::Receiver,
    last_heartbeat: Instant,
    reconnect: reconnect::Reconnect,
    pub clock: Arc<std::sync::Mutex<clock::ClockSync>>,
}

impl RemoteClient {
    pub fn connect(
        address: SocketAddr,
        definitions: &World,
        player: PlayerId,
    ) -> Result<(Self, PlayerUpdate)> {
        Self::connect_tracked(address, definitions, player, None)
    }

    pub fn connect_tracked(
        address: SocketAddr,
        definitions: &World,
        player: PlayerId,
        traffic: Option<Arc<Traffic>>,
    ) -> Result<(Self, PlayerUpdate)> {
        Self::connect_reserved(address, definitions, player, traffic, None)
    }

    pub fn connect_reserved(
        address: SocketAddr,
        definitions: &World,
        player: PlayerId,
        traffic: Option<Arc<Traffic>>,
        reservation: Option<(usize, [u8; 16])>,
    ) -> Result<(Self, PlayerUpdate)> {
        let deadline = Instant::now() + Duration::from_secs(3);
        let stream = loop {
            match TcpStream::connect_timeout(&address, Duration::from_millis(300)) {
                Ok(stream) => break stream,
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::ConnectionRefused
                            | std::io::ErrorKind::TimedOut
                            | std::io::ErrorKind::Interrupted
                    ) && Instant::now() < deadline =>
                {
                    std::thread::sleep(Duration::from_millis(25))
                }
                Err(error) => return Err(error).context("cannot connect to host"),
            }
        };
        let mut connection = Connection::new(stream)?;
        let traffic = traffic.unwrap_or_default();
        connection.track(Arc::clone(&traffic));
        let hello = Hello {
            protocol: PROTOCOL_VERSION,
            identity: GameplayIdentity::of(definitions),
            player,
            expected_map: None,
        };
        connection.send(&match reservation {
            Some((slot, ticket)) => ClientMessage::Join {
                hello,
                slot,
                ticket,
            },
            None => ClientMessage::Hello(hello),
        })?;
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut download: Option<Download> = None;
        let mut received_map: Option<MapTransfer> = None;
        loop {
            connection.flush()?;
            match connection.receive::<ServerMessage>()? {
                Some(ServerMessage::MapBegin { length, hash }) => {
                    ensure!(
                        download.is_none() && received_map.is_none(),
                        "duplicate map transfer"
                    );
                    download = Some(Download::new(length, hash)?);
                }
                Some(ServerMessage::MapChunk { offset, data }) => {
                    let transfer = download.as_mut().context("map chunk before header")?;
                    if let Some(map) = transfer.push(offset, &data)? {
                        map.definitions(definitions, player)?;
                        connection.send(&ClientMessage::MapReady(transfer.hash().into()))?;
                        received_map = Some(map);
                        download = None;
                    }
                }
                Some(ServerMessage::Welcome {
                    handshake,
                    initial,
                    udp,
                }) => {
                    let map = received_map.take().context("welcome before map download")?;
                    let public = map.definitions(definitions, player)?;
                    handshake.validate(&public)?;
                    ensure!(
                        handshake.player == player && initial.view.player == player,
                        "host assigned an unexpected player"
                    );
                    log::info!(
                        "joined host={address} map={} player={} tick={} protocol={PROTOCOL_VERSION}",
                        handshake.identity.map,
                        player.0,
                        initial.view.tick.0
                    );
                    let sequence = initial.sequence;
                    let control_address = connection.peer_addr()?;
                    let mut udp_address = control_address;
                    udp_address.set_port(udp.port);
                    let datagram = datagram::Endpoint::client(udp_address, udp.clone(), traffic)?;
                    return Ok((
                        Self {
                            connection,
                            handshake,
                            map,
                            sequence,
                            applied: sequence,
                            acknowledged: sequence,
                            inputs: Default::default(),
                            udp: datagram,
                            datagram_offer: udp,
                            replica: replication::Receiver::new(&initial),
                            last_heartbeat: Instant::now(),
                            reconnect: reconnect::Reconnect::new(control_address),
                            clock: Arc::new(std::sync::Mutex::new(clock::ClockSync::default())),
                        },
                        initial,
                    ));
                }
                Some(ServerMessage::Rejected(reason) | ServerMessage::End(reason)) => {
                    bail!("host rejected connection: {reason}")
                }
                Some(_) => bail!("host sent an update before handshake"),
                None => {}
            }
            ensure!(Instant::now() < deadline, "host handshake timed out");
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    pub fn command(&mut self, command: Command) -> Result<()> {
        ensure!(
            command.player == self.handshake.player,
            "command belongs to another player"
        );
        self.inputs.push(command)?;
        self.flush_commands()
    }

    pub fn acknowledge_commands(&mut self, acks: &commands::Acknowledgements) -> Result<()> {
        self.inputs.accept(acks)
    }

    fn flush_commands(&mut self) -> Result<()> {
        for _ in 0..64 {
            let Some(input) = self.inputs.due() else {
                break;
            };
            self.udp.send(&ClientMessage::Command(input), false)?;
        }
        Ok(())
    }

    pub fn pause(&mut self, paused: bool) -> Result<()> {
        ensure!(
            self.handshake.host,
            "only the host player can pause the match"
        );
        self.control(ClientMessage::Pause(paused))
    }

    pub fn set_name(&mut self, name: &str) -> Result<()> {
        self.control(ClientMessage::Name(chat::name(name)?))
    }

    pub fn chat(&mut self, channel: chat::Channel, text: &str) -> Result<()> {
        self.control(ClientMessage::Chat {
            channel,
            text: chat::text(text)?,
        })
    }

    pub fn poll(&mut self) -> Result<Option<ServerMessage>> {
        if let Some(sent) = self
            .clock
            .lock()
            .map_err(|_| anyhow::anyhow!("clock lock poisoned"))?
            .probe()
        {
            self.udp.send(&ClientMessage::Probe(sent), false)?;
        }
        self.reconnect_control()?;
        self.flush_commands()?;
        self.flush_acknowledgement()?;
        // Drain ready bytes before declaring silence: a temporarily delayed
        // polling thread may already have a fresh complete frame in its socket.
        let mut message = self.udp.poll::<ServerMessage>();
        if message.is_none() {
            message = self.receive_control()?;
        }
        ensure!(
            self.udp.last_received.elapsed() < PEER_TIMEOUT,
            "host timed out"
        );
        if let Some(ServerMessage::Clock { pong, tick_ms }) = message {
            ensure!(
                (1..=1000).contains(&tick_ms),
                "invalid network tick duration"
            );
            let mut clock = self
                .clock
                .lock()
                .map_err(|_| anyhow::anyhow!("clock lock poisoned"))?;
            let now = clock.now_ms();
            clock.observe(pong, now);
            clock.tick_ms = tick_ms;
            return Ok(None);
        }
        if let Some(ServerMessage::Delta(mut delta)) = message {
            ensure!(
                delta.time.is_some_and(|t| (1..=1000).contains(&t.tick_ms)),
                "invalid network state timestamp"
            );
            let mut clock = self
                .clock
                .lock()
                .map_err(|_| anyhow::anyhow!("clock lock poisoned"))?;
            // The authoritative host never predicts ahead of these updates.
            // Its newest state is useful regardless of clock/probe delay.
            if !self.handshake.host
                && delta.result.is_none()
                && !delta.time.is_some_and(|stamp| {
                    let now = clock.now_ms();
                    let limit = clock.max_frame_age_ms;
                    clock.accepts_frame(stamp, now, limit)
                })
            {
                // This frame is rejected, not accepted into the reconstruction
                // baseline. Its sequence cannot become eligible for an ACK.
                return Ok(None);
            }
            delta.expire_cosmetics(clock.server_ms(clock.now_ms()), clock.max_frame_age_ms);
            message = self
                .replica
                .apply_for(delta, self.applied)?
                .map(ServerMessage::Update);
        }
        if let Some(ServerMessage::Update(update)) = &message {
            ensure!(
                update.view.player == self.handshake.player,
                "player update ownership mismatch"
            );
            if update.sequence <= self.sequence {
                return Ok(None);
            }
            self.sequence = update.sequence;
        }
        Ok(message)
    }

    pub fn acknowledge(&mut self, sequence: u64) -> Result<()> {
        ensure!(
            sequence <= self.sequence,
            "acknowledgement exceeds received snapshot"
        );
        if sequence > self.applied {
            self.applied = sequence;
        }
        self.flush_acknowledgement()
    }
    fn flush_acknowledgement(&mut self) -> Result<()> {
        let interval = if self.applied > self.acknowledged {
            Duration::from_millis(clock::UPDATE_MS)
        } else {
            Duration::from_secs(1)
        };
        if self.last_heartbeat.elapsed() >= interval {
            self.udp.send(
                &ClientMessage::Acknowledge {
                    snapshot: self.applied,
                    commands_retired: self.inputs.retired(),
                },
                false,
            )?;
            self.acknowledged = self.applied;
            self.last_heartbeat = Instant::now();
        }
        Ok(())
    }
    pub fn resync(&mut self) -> Result<()> {
        self.udp.send(&ClientMessage::Resync, false)
    }
    pub fn leave(&mut self) {
        let _ = self.udp.send(
            &ClientMessage::Acknowledge {
                snapshot: self.applied,
                commands_retired: self.inputs.retired(),
            },
            false,
        );
        let _ = self.connection.send(&ClientMessage::Leave);
    }
}

struct Peer {
    connection: Connection,
    player: Option<PlayerId>,
    hello: Option<Hello>,
    map_offset: Option<usize>,
    name: String,
    chat_gate: chat::Gate,
    replication: Option<replication::Sender>,
    commands: commands::Inbox,
    udp: datagram::Endpoint,
    lost: Option<Instant>,
    ended: Option<String>,
    host: bool,
    slot: Option<usize>,
}

impl Peer {
    fn flush_update(&mut self, publish: bool) -> Result<()> {
        if self.lost.is_none()
            && let Err(error) = self.connection.flush()
        {
            self.control_lost(error);
        }
        self.udp.flush();
        // Finish this bounded socket-send batch before replacing it. Restarting
        // a snapshot larger than two flush budgets every tick means its tail
        // never gets sent, so the receiver can never reconstruct or ACK it.
        // Replication still retains only the newest pending state.
        if self.udp.has_pending_snapshot() {
            return Ok(());
        }
        let repeat = publish && self.udp.repeat_due();
        // Player zero is the host's own view. Its local display follows every
        // simulation tick; only remote players use the reduced publication rate.
        if (publish || self.host)
            && self.udp.remote.is_some()
            && let Some(update) = self
                .replication
                .as_mut()
                .and_then(|s| s.packet().or_else(|| repeat.then(|| s.repeat()).flatten()))
        {
            self.udp.send(&ServerMessage::Delta(update), true)?;
        }
        Ok(())
    }
}

mod host;
pub use host::{run_host, run_host_configured, run_host_with_map};

pub fn save_replay(path: &Path, replay: &Replay) -> Result<()> {
    let bytes = ron::ser::to_string(replay)?.into_bytes();
    ensure!(
        bytes.len() <= 128 * 1024 * 1024,
        "replay file exceeds size limit"
    );
    use std::io::Write;
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension("ron.tmp");
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .context("cannot create replay temporary file")?;
    let result = (|| -> Result<()> {
        file.write_all(&bytes)?;
        file.sync_all()?;
        std::fs::rename(&temporary, path).context("cannot publish replay")
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

pub fn load_replay(path: &Path) -> Result<Replay> {
    use std::io::Read;
    let file = std::fs::File::open(path)?;
    ensure!(
        file.metadata()?.len() <= 128 * 1024 * 1024,
        "replay file exceeds size limit"
    );
    let mut bytes = Vec::new();
    file.take(128 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= 128 * 1024 * 1024,
        "replay file exceeds size limit"
    );
    ron::de::from_bytes(&bytes).context("invalid replay file")
}

#[cfg(test)]
mod tests;
