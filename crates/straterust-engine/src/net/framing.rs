//! Length-prefixed, bounded reliable frames. Partial reads/writes never block
//! simulation ticks. TCP handles packet retransmission below this protocol.
use anyhow::{Context, Result, ensure};
use flate2::{Compression, read::ZlibDecoder, write::ZlibEncoder};
use serde::{Serialize, de::DeserializeOwned};
use std::{
    collections::VecDeque,
    io::{Read, Write},
    net::TcpStream,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

pub const MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;
const MAX_QUEUED_BYTES: usize = 32 * 1024 * 1024;
const FRAME_TIMEOUT: Duration = super::PEER_TIMEOUT;
const COMPRESSED: u32 = 1 << 31;
#[cfg(test)]
mod tests;

/// Actual application bytes on sockets, including framing and compressed payloads.
#[derive(Default)]
pub struct Traffic {
    sent: AtomicU64,
    received: AtomicU64,
}
impl Traffic {
    pub(super) fn sent(&self, bytes: usize) {
        self.sent.fetch_add(bytes as u64, Ordering::Relaxed);
    }
    pub(super) fn received(&self, bytes: usize) {
        self.received.fetch_add(bytes as u64, Ordering::Relaxed);
    }
    pub fn totals(&self) -> [u64; 2] {
        [
            self.sent.load(Ordering::Relaxed),
            self.received.load(Ordering::Relaxed),
        ]
    }
}

pub(crate) fn encode_frame(message: &impl Serialize) -> Result<Vec<u8>> {
    let raw = ron::ser::to_string(message)?.into_bytes();
    ensure!(
        !raw.is_empty() && raw.len() <= MAX_FRAME_BYTES,
        "network frame exceeds size limit"
    );
    let mut payload = Vec::new();
    let mut compressed = false;
    if raw.len() >= 512 {
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
        encoder.write_all(&raw)?;
        let bytes = encoder.finish()?;
        if bytes.len() + 4 < raw.len() {
            payload.extend((raw.len() as u32).to_le_bytes());
            payload.extend(bytes);
            compressed = true;
        }
    }
    if !compressed {
        payload = raw;
    }
    let length = payload.len() as u32 | if compressed { COMPRESSED } else { 0 };
    let mut frame = Vec::with_capacity(payload.len() + 4);
    frame.extend(length.to_le_bytes());
    frame.extend(payload);
    Ok(frame)
}

fn decode<T: DeserializeOwned>(payload: &[u8], compressed: bool) -> Result<T> {
    if !compressed {
        return ron::de::from_bytes(payload).context("malformed network message");
    }
    ensure!(payload.len() > 4, "invalid compressed network frame");
    let length = u32::from_le_bytes(payload[..4].try_into().unwrap()) as usize;
    ensure!(
        length > 0 && length <= MAX_FRAME_BYTES,
        "invalid decompressed network length"
    );
    let mut raw = Vec::with_capacity(length.min(64 * 1024));
    ZlibDecoder::new(&payload[4..])
        .take(length as u64 + 1)
        .read_to_end(&mut raw)
        .context("invalid compressed network payload")?;
    ensure!(raw.len() == length, "decompressed network length mismatch");
    ron::de::from_bytes(&raw).context("malformed network message")
}

pub(super) fn decode_frame<T: DeserializeOwned>(frame: &[u8]) -> Result<T> {
    ensure!(frame.len() >= 4, "incomplete datagram frame");
    let word = u32::from_le_bytes(frame[..4].try_into().unwrap());
    let length = (word & !COMPRESSED) as usize;
    ensure!(
        length > 0 && length <= MAX_FRAME_BYTES && frame.len() == length + 4,
        "invalid datagram frame length"
    );
    decode(&frame[4..], word & COMPRESSED != 0)
}

pub struct Connection {
    stream: TcpStream,
    incoming: Vec<u8>,
    outgoing: VecDeque<Vec<u8>>,
    written: usize,
    partial_since: Option<Instant>,
    pub last_received: Instant,
    traffic: Arc<Traffic>,
}

impl Connection {
    #[cfg(test)]
    pub(super) fn interrupt(&self) {
        self.stream.shutdown(std::net::Shutdown::Both).unwrap();
    }
    pub(super) fn peer_addr(&self) -> Result<std::net::SocketAddr> {
        Ok(self.stream.peer_addr()?)
    }
    pub fn new(stream: TcpStream) -> Result<Self> {
        stream.set_nonblocking(true)?;
        stream.set_nodelay(true)?;
        Ok(Self {
            stream,
            incoming: Vec::new(),
            outgoing: VecDeque::new(),
            written: 0,
            partial_since: None,
            last_received: Instant::now(),
            traffic: Arc::default(),
        })
    }

    pub fn send(&mut self, message: &impl Serialize) -> Result<()> {
        let frame = encode_frame(message)?;
        let queued: usize = self.outgoing.iter().map(Vec::len).sum();
        ensure!(
            self.outgoing.len() < 8 && queued + frame.len() <= MAX_QUEUED_BYTES,
            "client is not receiving updates: bounded network queue full"
        );
        self.outgoing.push_back(frame);
        self.flush()
    }

    pub fn track(&mut self, traffic: Arc<Traffic>) {
        if Arc::ptr_eq(&traffic, &self.traffic) {
            return;
        }
        self.traffic = traffic;
    }

    pub fn flush(&mut self) -> Result<()> {
        while let Some(frame) = self.outgoing.front() {
            match self.stream.write(&frame[self.written..]) {
                Ok(0) => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::BrokenPipe,
                        "peer closed during write",
                    )
                    .into());
                }
                Ok(count) => {
                    self.traffic.sent.fetch_add(count as u64, Ordering::Relaxed);
                    self.written += count;
                    if self.written == frame.len() {
                        self.outgoing.pop_front();
                        self.written = 0;
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error).context("network write failed"),
            }
        }
        Ok(())
    }

    pub(super) fn discard_writes(&mut self) {
        self.outgoing.clear();
        self.written = 0;
    }

    pub fn has_pending_writes(&self) -> bool {
        !self.outgoing.is_empty()
    }

    pub fn receive<T: DeserializeOwned>(&mut self) -> Result<Option<T>> {
        loop {
            if self.incoming.len() >= 4 {
                let word = u32::from_le_bytes(self.incoming[..4].try_into().unwrap());
                let length = (word & !COMPRESSED) as usize;
                ensure!(
                    length > 0 && length <= MAX_FRAME_BYTES,
                    "invalid network frame length"
                );
                if self.incoming.len() >= length + 4 {
                    let message = decode(&self.incoming[4..length + 4], word & COMPRESSED != 0)?;
                    self.incoming.drain(..length + 4);
                    self.partial_since = if self.incoming.is_empty() {
                        None
                    } else {
                        Some(Instant::now())
                    };
                    self.last_received = Instant::now();
                    return Ok(Some(message));
                }
            }
            ensure!(
                self.partial_since
                    .is_none_or(|start| start.elapsed() < FRAME_TIMEOUT),
                "partial network frame timed out"
            );
            let mut bytes = [0; 8192];
            // Bound read-ahead even when a peer streams an incomplete frame.
            let available = (MAX_FRAME_BYTES + 4 - self.incoming.len()).min(bytes.len());
            ensure!(available > 0, "network receive buffer full");
            match self.stream.read(&mut bytes[..available]) {
                Ok(0) => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::UnexpectedEof,
                        "peer disconnected",
                    )
                    .into());
                }
                Ok(count) => {
                    self.traffic
                        .received
                        .fetch_add(count as u64, Ordering::Relaxed);
                    self.partial_since.get_or_insert_with(Instant::now);
                    self.incoming.extend_from_slice(&bytes[..count]);
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => return Ok(None),
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error).context("network read failed"),
            }
        }
    }
}
