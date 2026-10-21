//! Running-game slot reservations and human/computer handoff.
use super::*;
use crate::multiplayer::{Prepared, SlotKind};

pub(in crate::net) struct RunningLobby {
    pub view: lobby::View,
    owners: Vec<Option<PlayerId>>,
    factions: Vec<usize>,
    tileset: usize,
    initial: Vec<usize>,
    tickets: BTreeMap<usize, ([u8; 16], Instant)>,
    pub names: BTreeMap<PlayerId, Vec<String>>,
    slot_names: Vec<String>,
}
impl RunningLobby {
    pub fn new(
        mut view: lobby::View,
        occupied: &[usize],
        prepared: &Prepared,
        tileset: usize,
    ) -> Self {
        view.running = true;
        for (slot, faction) in view.settings.slots.iter_mut().zip(&prepared.factions) {
            slot.faction = Some(*faction);
        }
        // Empty Open seats start under computer control, just like seats whose
        // human leaves later. Inactive seats must never be offered for takeover.
        for (index, slot) in view.settings.slots.iter_mut().enumerate() {
            if prepared.owners[index].is_none() {
                slot.kind = SlotKind::Closed;
            } else if view.players[index].is_none() {
                slot.kind = SlotKind::Computer;
            }
        }
        let slot_names = view
            .players
            .iter()
            .map(|p| p.as_ref().map_or_else(String::new, |p| p.name.clone()))
            .collect();
        let mut result = Self {
            view,
            owners: prepared.owners.clone(),
            factions: prepared.factions.clone(),
            tileset,
            initial: occupied.to_vec(),
            tickets: BTreeMap::new(),
            names: BTreeMap::new(),
            slot_names,
        };
        result.refresh_names();
        result
    }
    fn refresh_names(&mut self) {
        self.names.clear();
        for (slot, owner) in self.owners.iter().enumerate() {
            if let Some(owner) = owner
                && !self.slot_names[slot].is_empty()
            {
                self.names
                    .entry(*owner)
                    .or_default()
                    .push(self.slot_names[slot].clone());
            }
        }
    }
    pub fn available(&self, peers: &[Peer], world: &World) -> usize {
        (0..self.owners.len())
            .filter(|&slot| self.open(slot, peers, world))
            .count()
    }
    fn open(&self, slot: usize, peers: &[Peer], world: &World) -> bool {
        self.owners.get(slot).is_some_and(|owner| {
            owner.is_some_and(|player| !world.state().defeated.contains(&player))
        }) && !self.tickets.contains_key(&slot)
            && !peers.iter().any(|p| p.slot == Some(slot))
            && self.view.players[slot].is_none()
    }
    pub fn expire(&mut self, peers: &[Peer]) {
        let expired: Vec<_> = self
            .tickets
            .iter()
            .filter(|(slot, (_, until))| {
                Instant::now() >= *until && !peers.iter().any(|p| p.slot == Some(**slot))
            })
            .map(|(slot, _)| *slot)
            .collect();
        for slot in expired {
            self.tickets.remove(&slot);
            self.view.players[slot] = None;
        }
    }
    pub fn claim(&mut self, slot: usize, ticket: [u8; 16], player: PlayerId) -> Result<()> {
        ensure!(
            self.owners.get(slot) == Some(&Some(player)),
            "invalid slot assignment"
        );
        ensure!(
            self.tickets
                .get(&slot)
                .is_some_and(|(expected, until)| *expected == ticket && Instant::now() < *until),
            "invalid or expired join reservation"
        );
        self.tickets.remove(&slot);
        Ok(())
    }
    pub fn request(
        &mut self,
        index: usize,
        request: lobby::Request,
        peers: &mut [Peer],
        world: &World,
    ) -> Result<()> {
        use lobby::{Request, Response};
        ensure!(
            peers[index].player.is_none() && peers[index].hello.is_none(),
            "already joined"
        );
        match request {
            Request::Hello {
                protocol,
                identity,
                name,
                ..
            } => {
                ensure!(
                    protocol == PROTOCOL_VERSION
                        && identity.same_rules(&GameplayIdentity::of(world)),
                    "incompatible game or protocol"
                );
                peers[index].name = chat::name(&name)?;
            }
            Request::Slot(slot) => {
                if peers[index].name.is_empty() || !self.open(slot, peers, world) {
                    let reason = match self.owners.get(slot) {
                        None | Some(None) => "This seat is closed in this match.",
                        Some(Some(player)) if world.state().defeated.contains(player) => {
                            "This player has been defeated. Choose another seat."
                        }
                        _ => {
                            "This seat is occupied or reserved for reconnect. Choose another seat."
                        }
                    };
                    peers[index].send(&ServerMessage::Lobby(Response::Rejected(reason.into())))?;
                    return Ok(());
                }
                let token = datagram::Offer::new(1)?.token;
                self.tickets
                    .insert(slot, (token, Instant::now() + PEER_TIMEOUT));
                let name = peers[index].name.clone();
                self.slot_names[slot] = name.clone();
                self.view.players[slot] = Some(lobby::Player { name, ready: true });
                self.refresh_names();
                peers[index].send(&ServerMessage::Lobby(Response::Launch {
                    player: self.owners[slot].unwrap(),
                    faction: self.factions[slot],
                    tileset: self.tileset,
                    reservation: Some((slot, token)),
                }))?;
                return Ok(());
            }
            Request::Name(name) => {
                peers[index].name = chat::name(&name)?;
            }
            Request::Ping => {}
            Request::Leave => {
                peers[index].ended = Some("left slot browser".into());
                return Ok(());
            }
            _ => {
                peers[index].send(&ServerMessage::Lobby(Response::Rejected(
                    "Select an available computer-controlled slot to join.".into(),
                )))?;
            }
        }
        let mut view = self.view.clone();
        for slot in 0..view.players.len() {
            if self.owners[slot].is_some_and(|player| world.state().defeated.contains(&player)) {
                view.settings.slots[slot].kind = SlotKind::Closed;
            }
            if let Some(peer) = peers
                .iter()
                .find(|p| p.slot == Some(slot) && p.player.is_some())
            {
                view.players[slot] = Some(lobby::Player {
                    name: if peer.lost.is_some() {
                        format!("{} (reconnecting)", peer.name)
                    } else {
                        peer.name.clone()
                    },
                    ready: true,
                });
            }
        }
        peers[index].send(&ServerMessage::Lobby(Response::State {
            slot: view.players.len(),
            view,
        }))
    }
    pub fn initial_slot(&self, player: PlayerId, peers: &[Peer]) -> Option<usize> {
        self.initial.iter().copied().find(|slot| {
            self.owners[*slot] == Some(player) && !peers.iter().any(|p| p.slot == Some(*slot))
        })
    }
    pub fn named(&mut self, slot: usize, name: &str) {
        self.slot_names[slot] = name.into();
        if let Some(player) = &mut self.view.players[slot] {
            player.name = name.into();
        }
        self.refresh_names();
    }
    pub fn release(&mut self, slot: usize) {
        self.tickets.remove(&slot);
        self.view.players[slot] = None;
        self.view.settings.slots[slot].kind = SlotKind::Computer;
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn accept_hello(
    index: usize,
    hello: Hello,
    peers: &mut [Peer],
    humans: &[PlayerId],
    server: &ServerSession,
    started: bool,
    lobby: &mut Option<RunningLobby>,
    map: &[u8],
    hash: &str,
) -> Result<()> {
    ensure!(
        hello.protocol == PROTOCOL_VERSION
            && hello
                .identity
                .same_rules(&GameplayIdentity::of(server.world()))
            && hello
                .expected_map
                .as_ref()
                .is_none_or(|h| *h == GameplayIdentity::of(server.world()).map),
        "protocol/gameplay mismatch"
    );
    if !started {
        ensure!(
            humans.iter().filter(|p| **p == hello.player).count()
                > peers
                    .iter()
                    .filter(|p| p.hello.as_ref().is_some_and(|h| h.player == hello.player))
                    .count(),
            "player slot unavailable"
        );
        if let Some(context) = lobby {
            let slot = context
                .initial_slot(hello.player, peers)
                .context("player slot unavailable")?;
            peers[index].slot = Some(slot);
            peers[index].host = slot == context.view.host;
            peers[index].name = context.slot_names[slot].clone();
        } else {
            peers[index].host = hello.player == PlayerId(0);
        }
    }
    peers[index].hello = Some(hello);
    peers[index].map_offset = Some(0);
    peers[index].send(&ServerMessage::MapBegin {
        length: map.len(),
        hash: hash.into(),
    })
}
pub(super) fn retire(
    peer: Peer,
    peers: &mut [Peer],
    server: &mut ServerSession,
    lobby: &mut Option<RunningLobby>,
    started: bool,
) -> Result<()> {
    let reason = peer.ended.as_deref().unwrap_or("connection closed");
    log::info!(
        "removing peer {:?} ({}) slot {:?}: {reason}",
        peer.player,
        peer.name,
        peer.slot
    );
    if let Some(slot) = peer.slot
        && let Some(context) = lobby
    {
        context.release(slot);
    }
    if let Some(player) = peer.player {
        let takeover = started
            && !peers.iter().any(|p| p.player == Some(player))
            && server
                .world()
                .map()
                .ai
                .iter()
                .any(|a| a.player == player && a.takeover);
        if takeover {
            server.control_ai(player, true)?;
        }
        let notice = format!(
            "{} disconnected.{}",
            peer.name,
            if takeover {
                " Computer control enabled."
            } else {
                ""
            }
        );
        for other in peers.iter_mut().filter(|p| p.player.is_some()) {
            other.send(&ServerMessage::Notice(notice.clone()))?;
        }
    }
    Ok(())
}
