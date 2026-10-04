//! Optional LAN discovery: multicast probes, small unicast replies. It carries
//! public session metadata only and is independent of the game connection.
use crate::{session::PROTOCOL_VERSION, sim::GameplayIdentity};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    net::{Ipv4Addr, SocketAddr, UdpSocket},
    time::{Duration, Instant},
};

const GROUP: Ipv4Addr = Ipv4Addr::new(239, 255, 83, 82);
const PORT: u16 = 6113;
const MAX_DATAGRAM: usize = 2048;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Probe {
    protocol: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Advertisement {
    protocol: u32,
    name: String,
    port: u16,
    identity: GameplayIdentity,
}

#[derive(Clone, Debug)]
pub struct LanGame {
    pub name: String,
    pub address: SocketAddr,
    pub compatible: bool,
    pub identity: GameplayIdentity,
}

pub(super) struct Advertiser {
    socket: UdpSocket,
    reply: Vec<u8>,
}

impl Advertiser {
    pub fn new(name: &str, port: u16, identity: GameplayIdentity) -> Result<Self> {
        ensure!(name.len() <= 128, "LAN session name too long");
        let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, PORT))?;
        socket.join_multicast_v4(&GROUP, &Ipv4Addr::UNSPECIFIED)?;
        socket.set_nonblocking(true)?;
        let reply = ron::ser::to_string(&Advertisement {
            protocol: PROTOCOL_VERSION,
            name: name.into(),
            port,
            identity,
        })?
        .into_bytes();
        ensure!(reply.len() <= MAX_DATAGRAM, "LAN advertisement too large");
        Ok(Self { socket, reply })
    }

    pub fn poll(&self) -> Result<()> {
        let mut bytes = [0; MAX_DATAGRAM + 1];
        for _ in 0..32 {
            match self.socket.recv_from(&mut bytes) {
                Ok((count, address)) if count <= MAX_DATAGRAM => {
                    if ron::de::from_bytes::<Probe>(&bytes[..count])
                        .is_ok_and(|p| p.protocol == PROTOCOL_VERSION)
                    {
                        let _ = self.socket.send_to(&self.reply, address);
                    }
                }
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(error) => return Err(error.into()),
            }
        }
        Ok(())
    }
}

pub fn discover(identity: &GameplayIdentity, wait: Duration) -> Result<Vec<LanGame>> {
    ensure!(
        wait <= Duration::from_secs(3),
        "LAN discovery wait exceeds limit"
    );
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))?;
    socket.set_nonblocking(true)?;
    socket.set_multicast_ttl_v4(1)?;
    socket.set_multicast_loop_v4(true)?;
    let probe = ron::ser::to_string(&Probe {
        protocol: PROTOCOL_VERSION,
    })?;
    let deadline = Instant::now() + wait;
    let mut next_probe = Instant::now();
    let mut games = BTreeMap::new();
    let mut bytes = [0; MAX_DATAGRAM + 1];
    while Instant::now() < deadline {
        if Instant::now() >= next_probe {
            socket.send_to(probe.as_bytes(), (GROUP, PORT))?;
            next_probe = Instant::now() + Duration::from_millis(250);
        }
        for _ in 0..32 {
            match socket.recv_from(&mut bytes) {
                Ok((count, source)) if count <= MAX_DATAGRAM => {
                    if let Ok(ad) = ron::de::from_bytes::<Advertisement>(&bytes[..count])
                        && ad.protocol == PROTOCOL_VERSION
                        && ad.port > 0
                        && ad.name.len() <= 128
                        && games.len() < 64
                    {
                        let address = SocketAddr::new(source.ip(), ad.port);
                        games.insert(
                            address,
                            LanGame {
                                name: ad.name,
                                address,
                                compatible: ad.identity.same_rules(identity),
                                identity: ad.identity,
                            },
                        );
                    }
                }
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(error) => return Err(error.into()),
            }
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    Ok(games.into_values().collect())
}
