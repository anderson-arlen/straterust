//! Replace a broken TCP control socket without resetting the live UDP session.
use super::*;
use std::collections::VecDeque;

pub(super) struct Reconnect {
    address: SocketAddr,
    since: Option<Instant>,
    attempt: Instant,
    awaiting: bool,
    pending: VecDeque<ClientMessage>,
    notices: VecDeque<String>,
}
impl Reconnect {
    pub fn new(address: SocketAddr) -> Self {
        Self {
            address,
            since: None,
            attempt: Instant::now(),
            awaiting: false,
            pending: VecDeque::new(),
            notices: VecDeque::new(),
        }
    }
}
impl RemoteClient {
    fn control_failed(&mut self, error: anyhow::Error) -> Result<()> {
        // Bad protocol data is not a recoverable socket interruption.
        if !error.chain().any(|e| e.is::<std::io::Error>()) {
            return Err(error);
        }
        if self.reconnect.since.is_none() {
            self.reconnect.since = Some(Instant::now());
            self.reconnect
                .notices
                .push_back("Connection interrupted. Reconnecting...".into());
            log::warn!(
                "control connection to {} lost: {error:#}; reconnecting",
                self.reconnect.address
            );
        }
        self.reconnect.awaiting = false;
        self.connection.discard_writes();
        Ok(())
    }
    pub(super) fn control(&mut self, message: ClientMessage) -> Result<()> {
        ensure!(
            self.reconnect.pending.len() < 64,
            "too many pending control requests"
        );
        self.reconnect.pending.push_back(message);
        self.reconnect_control()
    }
    pub(super) fn reconnect_control(&mut self) -> Result<()> {
        if let Some(since) = self.reconnect.since {
            ensure!(
                since.elapsed() < PEER_TIMEOUT,
                "unable to reconnect to host after 30 seconds"
            );
            if self.reconnect.awaiting
                && Instant::now() >= self.reconnect.attempt + Duration::from_secs(1)
            {
                self.reconnect.awaiting = false;
                self.connection.discard_writes();
            }
            if !self.reconnect.awaiting && Instant::now() >= self.reconnect.attempt {
                self.reconnect.attempt = Instant::now() + Duration::from_millis(250);
                if let Ok(stream) =
                    TcpStream::connect_timeout(&self.reconnect.address, Duration::from_millis(30))
                {
                    let mut connection = Connection::new(stream)?;
                    connection.track(Arc::clone(&self.udp.traffic));
                    if connection
                        .send(&ClientMessage::Resume(self.datagram_offer.token))
                        .is_ok()
                    {
                        self.connection = connection;
                        self.reconnect.awaiting = true;
                    }
                }
            }
        }
        if (self.reconnect.since.is_none() || self.reconnect.awaiting)
            && let Err(error) = self.connection.flush()
        {
            self.control_failed(error)?;
        }
        if self.reconnect.since.is_none() {
            while let Some(message) = self.reconnect.pending.pop_front() {
                if let Err(error) = self.connection.send(&message) {
                    self.reconnect.pending.push_front(message);
                    self.control_failed(error)?;
                    break;
                }
            }
        }
        Ok(())
    }
    pub(super) fn receive_control(&mut self) -> Result<Option<ServerMessage>> {
        if let Some(notice) = self.reconnect.notices.pop_front() {
            return Ok(Some(ServerMessage::Notice(notice)));
        }
        if self.reconnect.since.is_some() && !self.reconnect.awaiting {
            return Ok(None);
        }
        match self.connection.receive() {
            Ok(Some(ServerMessage::Resumed)) => {
                ensure!(
                    self.reconnect.awaiting,
                    "unexpected reconnect acknowledgement"
                );
                self.reconnect.since = None;
                self.reconnect.awaiting = false;
                log::info!(
                    "control connection to {} restored; input IDs and snapshot ACKs retained",
                    self.reconnect.address
                );
                Ok(Some(ServerMessage::Notice("Connection restored.".into())))
            }
            Ok(message) => Ok(message),
            Err(error) => {
                self.control_failed(error)?;
                Ok(None)
            }
        }
    }
}
impl Peer {
    pub(super) fn control_lost(&mut self, error: anyhow::Error) {
        if self.lost.is_none() {
            log::warn!(
                "peer {:?} ({}) control connection lost: {error:#}; reserving slot for 30 seconds",
                self.player,
                self.name
            );
            self.lost = Some(Instant::now());
        }
        self.connection.discard_writes();
    }
    pub(super) fn send(&mut self, message: &ServerMessage) -> Result<()> {
        if self.lost.is_none()
            && let Err(error) = self.connection.send(message)
        {
            if error.chain().any(|e| e.is::<std::io::Error>()) {
                self.control_lost(error);
            } else {
                // A bounded queue belongs to this peer, not the entire match.
                self.ended = Some(format!("control delivery failed: {error:#}"));
            }
        }
        Ok(())
    }
    pub(super) fn receive(&mut self) -> Result<Option<ClientMessage>> {
        if self.lost.is_some() {
            return Ok(None);
        }
        match self.connection.receive() {
            Ok(message) => Ok(message),
            Err(error)
                if self.player.is_some() && error.chain().any(|e| e.is::<std::io::Error>()) =>
            {
                self.control_lost(error);
                Ok(None)
            }
            Err(error) => Err(error),
        }
    }
}
