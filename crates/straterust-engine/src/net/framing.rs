//! Length-prefixed, bounded reliable frames. Partial reads/writes never block
//! simulation ticks. TCP handles packet retransmission below this protocol.
use anyhow::{Context, Result, bail, ensure};
use serde::{Serialize, de::DeserializeOwned};
use std::{
    collections::VecDeque,
    io::{Read, Write},
    net::TcpStream,
    time::{Duration, Instant},
};

pub const MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;
const MAX_QUEUED_BYTES: usize = 32 * 1024 * 1024;
const FRAME_TIMEOUT: Duration = Duration::from_secs(5);

pub struct Connection {
    stream: TcpStream,
    incoming: Vec<u8>,
    outgoing: VecDeque<Vec<u8>>,
    written: usize,
    partial_since: Option<Instant>,
    pub last_received: Instant,
}

impl Connection {
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
        })
    }

    pub fn send(&mut self, message: &impl Serialize) -> Result<()> {
        let payload = ron::ser::to_string(message)?.into_bytes();
        ensure!(
            !payload.is_empty() && payload.len() <= MAX_FRAME_BYTES,
            "network frame exceeds size limit"
        );
        let queued: usize = self.outgoing.iter().map(Vec::len).sum();
        ensure!(
            self.outgoing.len() < 8 && queued + payload.len() + 4 <= MAX_QUEUED_BYTES,
            "client is not receiving updates: bounded network queue full"
        );
        let mut frame = Vec::with_capacity(payload.len() + 4);
        frame.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        frame.extend(payload);
        self.outgoing.push_back(frame);
        self.flush()
    }

    pub fn flush(&mut self) -> Result<()> {
        while let Some(frame) = self.outgoing.front() {
            match self.stream.write(&frame[self.written..]) {
                Ok(0) => bail!("peer closed during write"),
                Ok(count) => {
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
                let length = u32::from_le_bytes(self.incoming[..4].try_into().unwrap()) as usize;
                ensure!(
                    length > 0 && length <= MAX_FRAME_BYTES,
                    "invalid network frame length"
                );
                if self.incoming.len() >= length + 4 {
                    let message = ron::de::from_bytes(&self.incoming[4..length + 4])
                        .context("malformed network message")?;
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
                Ok(0) => bail!("peer disconnected"),
                Ok(count) => {
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
