//! Bounded UDP messages. Only snapshots fragment; a newer snapshot immediately
//! replaces incomplete older work. Control messages fit in one datagram.
use super::*;
use serde::de::DeserializeOwned;
use std::{
    collections::VecDeque,
    net::{IpAddr, UdpSocket},
};

const HEADER: usize = 37;
pub(super) const PACKET_BYTES: usize = 1200;
const PAYLOAD: usize = PACKET_BYTES - HEADER;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Offer {
    pub port: u16,
    pub token: [u8; 16],
}
impl Offer {
    pub(super) fn new(port: u16) -> Result<Self> {
        let mut token = [0; 16];
        getrandom::fill(&mut token)
            .map_err(|error| anyhow::anyhow!("cannot create UDP session token: {error}"))?;
        Ok(Self { port, token })
    }
}

struct Assembly {
    id: u64,
    length: usize,
    pieces: Vec<Option<Vec<u8>>>,
    count: usize,
    since: Instant,
}

pub(super) struct Endpoint {
    pub socket: Arc<UdpSocket>,
    pub offer: Offer,
    pub remote: Option<SocketAddr>,
    pub(super) expected_ip: IpAddr,
    next: u64,
    latest: u64,
    assembly: Option<Assembly>,
    outgoing: VecDeque<Vec<u8>>,
    last_snapshot: Instant,
    pub(super) last_snapshot_bytes: usize,
    pub last_received: Instant,
    pub(super) traffic: Arc<Traffic>,
}
impl Endpoint {
    pub fn new(
        socket: Arc<UdpSocket>,
        offer: Offer,
        expected_ip: IpAddr,
        remote: Option<SocketAddr>,
        traffic: Arc<Traffic>,
    ) -> Result<Self> {
        socket.set_nonblocking(true)?;
        Ok(Self {
            socket,
            offer,
            remote,
            expected_ip,
            next: 0,
            latest: 0,
            assembly: None,
            outgoing: VecDeque::new(),
            last_snapshot: Instant::now() - Duration::from_secs(1),
            last_snapshot_bytes: 0,
            last_received: Instant::now(),
            traffic,
        })
    }
    pub fn client(server: SocketAddr, offer: Offer, traffic: Arc<Traffic>) -> Result<Self> {
        let bind = if server.is_ipv4() {
            "0.0.0.0:0"
        } else {
            "[::]:0"
        };
        Self::new(
            Arc::new(UdpSocket::bind(bind)?),
            offer,
            server.ip(),
            Some(server),
            traffic,
        )
    }
    pub fn send(&mut self, message: &impl Serialize, snapshot: bool) -> Result<()> {
        let frame = framing::encode_frame(message)?;
        ensure!(
            snapshot || frame.len() <= PAYLOAD,
            "UDP control message exceeds datagram limit"
        );
        self.next = self
            .next
            .checked_add(1)
            .context("UDP message ID exhausted")?;
        let packets = packets(&self.offer.token, self.next, snapshot, &frame);
        if snapshot {
            self.last_snapshot = Instant::now();
            self.last_snapshot_bytes = frame.len();
            self.outgoing = packets.into();
            self.flush();
        } else if let Some(remote) = self.remote {
            // Failed or full socket sends are retried by the command/ACK/probe
            // protocol; a transient OS/network error does not end the match.
            if let Ok(count) = self.socket.send_to(&packets[0], remote) {
                self.traffic.sent(count);
            }
        }
        Ok(())
    }
    pub fn repeat_due(&self) -> bool {
        self.last_snapshot.elapsed() >= Duration::from_millis(clock::UPDATE_MS)
    }
    pub(super) fn has_pending_snapshot(&self) -> bool {
        !self.outgoing.is_empty()
    }
    pub fn flush(&mut self) {
        let Some(remote) = self.remote else {
            return;
        };
        for _ in 0..64 {
            let Some(packet) = self.outgoing.front() else {
                break;
            };
            match self.socket.send_to(packet, remote) {
                Ok(count) => {
                    self.traffic.sent(count);
                    self.outgoing.pop_front();
                }
                Err(_) => break,
            }
        }
    }
    pub fn poll<T: DeserializeOwned>(&mut self) -> Option<T> {
        let mut bytes = [0; PACKET_BYTES + 1];
        for _ in 0..512 {
            let Ok((length, source)) = self.socket.recv_from(&mut bytes) else {
                break;
            };
            if let Some(message) = self.receive(&bytes[..length], source) {
                return Some(message);
            }
        }
        None
    }
    pub fn receive<T: DeserializeOwned>(&mut self, packet: &[u8], source: SocketAddr) -> Option<T> {
        if source.ip() != self.expected_ip
            || packet.len() < HEADER
            || packet.len() > PACKET_BYTES
            || packet[..4] != *b"SRTU"
            || packet[4..20] != self.offer.token
        {
            return None;
        }
        let snapshot = match packet[20] {
            0 => false,
            1 => true,
            _ => return None,
        };
        let id = u64::from_le_bytes(packet[21..29].try_into().ok()?);
        let index = u16::from_le_bytes(packet[29..31].try_into().ok()?) as usize;
        let count = u16::from_le_bytes(packet[31..33].try_into().ok()?) as usize;
        let length = u32::from_le_bytes(packet[33..37].try_into().ok()?) as usize;
        if id == 0
            || !(4..=framing::MAX_FRAME_BYTES + 4).contains(&length)
            || count != length.div_ceil(PAYLOAD)
            || index >= count
            || packet.len() - HEADER != (length - index * PAYLOAD).min(PAYLOAD)
            || (!snapshot && count != 1)
        {
            return None;
        }
        self.traffic.received(packet.len());
        self.remote = Some(source);
        self.last_received = Instant::now();
        if !snapshot {
            return framing::decode_frame(&packet[HEADER..]).ok();
        }
        if id < self.latest || (id == self.latest && self.assembly.is_none()) {
            return None;
        }
        if id > self.latest {
            self.latest = id;
            self.assembly = Some(Assembly {
                id,
                length,
                pieces: vec![None; count],
                count: 0,
                since: Instant::now(),
            });
        }
        let assembly = self.assembly.as_mut()?;
        if assembly.id != id
            || assembly.length != length
            || assembly.pieces.len() != count
            || assembly.since.elapsed() > Duration::from_secs(2)
        {
            return None;
        }
        if assembly.pieces[index].is_none() {
            assembly.pieces[index] = Some(packet[HEADER..].to_vec());
            assembly.count += 1;
        }
        if assembly.count != count {
            return None;
        }
        let assembly = self.assembly.take()?;
        let mut frame = Vec::with_capacity(length);
        for piece in assembly.pieces {
            frame.extend(piece?);
        }
        framing::decode_frame(&frame).ok()
    }
}

fn packets(token: &[u8; 16], id: u64, snapshot: bool, frame: &[u8]) -> Vec<Vec<u8>> {
    let count = frame.len().div_ceil(PAYLOAD) as u16;
    frame
        .chunks(PAYLOAD)
        .enumerate()
        .map(|(index, payload)| {
            let mut packet = Vec::with_capacity(HEADER + payload.len());
            packet.extend(b"SRTU");
            packet.extend(token);
            packet.push(u8::from(snapshot));
            packet.extend(id.to_le_bytes());
            packet.extend((index as u16).to_le_bytes());
            packet.extend(count.to_le_bytes());
            packet.extend((frame.len() as u32).to_le_bytes());
            packet.extend(payload);
            packet
        })
        .collect()
}

#[cfg(test)]
mod tests;
