//! Session transport. Local play uses `ServerSession` directly; TCP changes only
//! delivery. The server serializes PlayerUpdate, never World or State.
use crate::{
    session::{Handshake, PROTOCOL_VERSION, PlayerUpdate, Replay, ServerSession},
    sim::{Command, GameplayIdentity, PlayerId, World},
};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::{
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
pub use framing::Connection;
pub use map_transfer::MapTransfer;
use map_transfer::{Download, MAP_CHUNK_BYTES};
pub mod lan;

pub const DEFAULT_PORT: u16 = 6112;
pub const PEER_TIMEOUT: Duration = Duration::from_secs(10);

pub use crate::session::Hello;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ClientMessage {
    Hello(Hello),
    Command(Command),
    Acknowledge(u64),
    MapReady(String),
    Pause(bool),
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
    },
    Status {
        started: bool,
        paused: bool,
    },
    Update(PlayerUpdate),
    Rejected(String),
    End(String),
}

pub struct RemoteClient {
    connection: Connection,
    pub handshake: Handshake,
    pub map: MapTransfer,
    sequence: u64,
    last_heartbeat: Instant,
}

impl RemoteClient {
    pub fn connect(
        address: SocketAddr,
        definitions: &World,
        player: PlayerId,
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
        connection.send(&ClientMessage::Hello(Hello {
            protocol: PROTOCOL_VERSION,
            identity: GameplayIdentity::of(definitions),
            player,
            expected_map: None,
        }))?;
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
                Some(ServerMessage::Welcome { handshake, initial }) => {
                    let map = received_map.take().context("welcome before map download")?;
                    let public = map.definitions(definitions, player)?;
                    handshake.validate(&public)?;
                    ensure!(
                        handshake.player == player && initial.view.player == player,
                        "host assigned an unexpected player"
                    );
                    let sequence = initial.sequence;
                    return Ok((
                        Self {
                            connection,
                            handshake,
                            map,
                            sequence,
                            last_heartbeat: Instant::now(),
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
        self.connection.send(&ClientMessage::Command(command))
    }

    pub fn pause(&mut self, paused: bool) -> Result<()> {
        ensure!(
            self.handshake.player == PlayerId(0),
            "only the host player can pause the match"
        );
        self.connection.send(&ClientMessage::Pause(paused))
    }

    pub fn poll(&mut self) -> Result<Option<ServerMessage>> {
        self.connection.flush()?;
        if self.last_heartbeat.elapsed() >= Duration::from_secs(1) {
            self.connection
                .send(&ClientMessage::Acknowledge(self.sequence))?;
            self.last_heartbeat = Instant::now();
        }
        ensure!(
            self.connection.last_received.elapsed() < PEER_TIMEOUT,
            "host timed out"
        );
        let message = self.connection.receive::<ServerMessage>()?;
        if let Some(ServerMessage::Update(update)) = &message {
            ensure!(
                update.sequence
                    == self
                        .sequence
                        .checked_add(1)
                        .context("view sequence exhausted")?
                    && update.view.player == self.handshake.player,
                "player update sequence or ownership mismatch"
            );
            self.sequence = update.sequence;
        }
        Ok(message)
    }

    pub fn leave(&mut self) {
        let _ = self.connection.send(&ClientMessage::Leave);
    }
}

struct Peer {
    connection: Connection,
    player: Option<PlayerId>,
    last_ack: u64,
    hello: Option<Hello>,
    map_offset: Option<usize>,
}

/// Runs off the window thread. All state/hashes/replay remain here. A peer loss
/// ends the two-player session explicitly; reconnect/migration are not implied.
pub fn run_host(
    listener: TcpListener,
    world: World,
    seed: u64,
    cancelled: Arc<AtomicBool>,
    replay_path: Option<&Path>,
) -> Result<()> {
    let map = MapTransfer::of(&world)?;
    run_host_with_map(listener, world, seed, cancelled, replay_path, map)
}

pub fn run_host_with_map(
    listener: TcpListener,
    world: World,
    seed: u64,
    cancelled: Arc<AtomicBool>,
    replay_path: Option<&Path>,
    map: MapTransfer,
) -> Result<()> {
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
    let mut server = ServerSession::new(world, seed, vec![PlayerId(0), PlayerId(1)])?;
    let advertisement = lan::Advertiser::new(
        &server.world().map().id,
        listener.local_addr()?.port(),
        GameplayIdentity::of(server.world()),
    )
    .ok();
    listener.set_nonblocking(true)?;
    let mut peers: Vec<Peer> = Vec::new();
    let mut started = false;
    let mut paused = false;
    let mut last_status = Instant::now();
    let mut next_tick = Instant::now();
    let tick_duration = Duration::from_millis(u64::from(server.world().rules().tick_ms));
    let result = (|| -> Result<()> {
        while !cancelled.load(Ordering::Acquire) {
            if let Some(advertisement) = &advertisement {
                let _ = advertisement.poll();
            }
            loop {
                match listener.accept() {
                    Ok((stream, _)) if !started && peers.len() < 8 => peers.push(Peer {
                        connection: Connection::new(stream)?,
                        player: None,
                        last_ack: 0,
                        hello: None,
                        map_offset: None,
                    }),
                    Ok((stream, _)) => {
                        let mut connection = Connection::new(stream)?;
                        connection.send(&ServerMessage::End(
                            "session full or already started".into(),
                        ))?;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(error) => return Err(error).context("host accept failed"),
                }
            }
            let mut status_changed = false;
            let mut drop_peers = Vec::new();
            for index in 0..peers.len() {
                for _ in 0..256 {
                    let message = match peers[index].connection.receive::<ClientMessage>() {
                        Ok(Some(message)) => message,
                        Ok(None) => break,
                        Err(error) if peers[index].player.is_none() => {
                            drop_peers.push(index);
                            let _ = error;
                            break;
                        }
                        Err(error) => return Err(error).context("player connection failed"),
                    };
                    if peers[index].player.is_none()
                        && !matches!(
                            &message,
                            ClientMessage::Hello(_) | ClientMessage::MapReady(_)
                        )
                    {
                        drop_peers.push(index);
                        break;
                    }
                    match message {
                        ClientMessage::Hello(hello) if peers[index].hello.is_none() => {
                            let available = hello.player.0 < 2
                                && !peers.iter().any(|p| {
                                    p.hello.as_ref().is_some_and(|h| h.player == hello.player)
                                });
                            let compatible = hello.protocol == PROTOCOL_VERSION
                                && hello
                                    .identity
                                    .same_rules(&GameplayIdentity::of(server.world()))
                                && hello
                                    .expected_map
                                    .as_ref()
                                    .is_none_or(|h| *h == GameplayIdentity::of(server.world()).map);
                            if !available || !compatible {
                                peers[index].connection.send(&ServerMessage::End(
                                    "protocol/gameplay mismatch or player slot unavailable".into(),
                                ))?;
                                drop_peers.push(index);
                                break;
                            }
                            peers[index].hello = Some(hello);
                            peers[index].map_offset = Some(0);
                            peers[index].connection.send(&ServerMessage::MapBegin {
                                length: map_bytes.len(),
                                hash: map_hash.clone(),
                            })?;
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
                                drop_peers.push(index);
                                break;
                            }
                            let hello = peers[index]
                                .hello
                                .as_ref()
                                .context("map acknowledgement before handshake")?;
                            let (handshake, initial) = server.connect(hello)?;
                            peers[index].player = Some(hello.player);
                            peers[index]
                                .connection
                                .send(&ServerMessage::Welcome { handshake, initial })?;
                            peers[index]
                                .connection
                                .send(&ServerMessage::Status { started, paused })?;
                        }
                        ClientMessage::Command(command) => {
                            let player = peers[index].player.context("command before handshake")?;
                            if let Err(error) = server.submit(player, command) {
                                peers[index]
                                    .connection
                                    .send(&ServerMessage::Rejected(error.to_string()))?;
                            }
                        }
                        ClientMessage::Acknowledge(sequence) => {
                            ensure!(
                                peers[index].player.is_some() && sequence >= peers[index].last_ack,
                                "invalid client acknowledgement sequence"
                            );
                            peers[index].last_ack = sequence;
                        }
                        ClientMessage::Pause(value) if peers[index].player == Some(PlayerId(0)) => {
                            paused = value;
                            status_changed = true;
                            next_tick = Instant::now() + tick_duration;
                        }
                        ClientMessage::Pause(_) => peers[index].connection.send(
                            &ServerMessage::Rejected("only the host player may pause".into()),
                        )?,
                        ClientMessage::Leave => bail!("player left the match"),
                        ClientMessage::Hello(_) if peers[index].player.is_none() => {
                            drop_peers.push(index);
                            break;
                        }
                        ClientMessage::Hello(_) => bail!("duplicate handshake"),
                    }
                }
                if drop_peers.contains(&index) {
                    continue;
                }
                if let Some(offset) = peers[index].map_offset
                    && !peers[index].connection.has_pending_writes()
                {
                    let end = (offset + MAP_CHUNK_BYTES).min(map_bytes.len());
                    peers[index].connection.send(&ServerMessage::MapChunk {
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
                if peers[index].connection.last_received.elapsed() >= timeout {
                    if peers[index].player.is_some() {
                        bail!("player {} timed out", peers[index].player.unwrap().0);
                    }
                    drop_peers.push(index);
                }
                peers[index].connection.flush()?;
            }
            drop_peers.sort_unstable();
            drop_peers.dedup();
            for index in drop_peers.into_iter().rev() {
                peers.remove(index);
            }
            if !started && peers.iter().filter(|p| p.player.is_some()).count() == 2 {
                started = true;
                status_changed = true;
                next_tick = Instant::now() + tick_duration;
            }
            // Waiting and paused matches have no tick updates. Keep their healthy
            // connections alive without advancing simulation or view sequences.
            if status_changed || last_status.elapsed() >= Duration::from_secs(1) {
                for peer in peers.iter_mut().filter(|p| p.player.is_some()) {
                    peer.connection
                        .send(&ServerMessage::Status { started, paused })?;
                }
                last_status = Instant::now();
            }
            if started && !paused {
                for _ in 0..8 {
                    if Instant::now() < next_tick {
                        break;
                    }
                    let outcomes = server.advance(&[])?;
                    for peer in peers.iter_mut().filter(|p| p.player.is_some()) {
                        let update = server.update(peer.player.unwrap(), &outcomes)?;
                        peer.connection.send(&ServerMessage::Update(update))?;
                    }
                    next_tick += tick_duration;
                    if server.match_result().is_some() {
                        return Ok(());
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        bail!("host closed the match")
    })();
    let reason = result
        .as_ref()
        .err()
        .map_or_else(|| "match finished".into(), |e| format!("{e:#}"));
    for peer in &mut peers {
        let _ = peer.connection.send(&ServerMessage::End(reason.clone()));
    }
    let deadline = Instant::now() + Duration::from_secs(2);
    while peers.iter().any(|p| p.connection.has_pending_writes()) && Instant::now() < deadline {
        for peer in &mut peers {
            if peer.connection.flush().is_err() {
                peer.connection.discard_writes();
            }
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    if let Some(path) = replay_path {
        save_replay(path, server.replay())?;
    }
    result
}

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
