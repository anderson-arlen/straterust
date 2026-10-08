//! One authoritative session for local channels and network hosts. No transport,
//! wall clock, graphics or game-specific scripting belongs in this command path.
use crate::sim::{
    Command, CommandOutcome, EntityId, GameplayIdentity, MAX_COMMANDS_PER_TICK, PlayerId,
    PlayerView, Tick, ViewedEntity, World, WorldSnapshot,
};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

mod results;
pub use results::{MatchOutcome, MatchResult, PlayerResult};
mod saved_game;
pub use saved_game::SavedGame;

pub const PROTOCOL_VERSION: u32 = 14;
pub const INPUT_LEAD: u64 = 2;
pub const MAX_INPUT_AHEAD: u64 = 64;
pub const MAX_REPLAY_TICKS: usize = 1_000_000;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hello {
    pub protocol: u32,
    pub identity: GameplayIdentity,
    pub player: PlayerId,
    /// None asks the server to supply its public custom map. Some pins a known
    /// scenario, used by local sessions and explicit compatibility checks.
    pub expected_map: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Handshake {
    pub protocol: u32,
    pub identity: GameplayIdentity,
    pub seed: u64,
    pub player: PlayerId,
    pub participants: Vec<PlayerId>,
    pub input_lead: u64,
}

impl Handshake {
    pub fn validate(&self, definitions: &World) -> Result<()> {
        ensure!(
            self.protocol == PROTOCOL_VERSION,
            "protocol version mismatch"
        );
        ensure!(
            self.identity == GameplayIdentity::of(definitions),
            "gameplay identity mismatch (rules/map/scripts or simulation revision)"
        );
        ensure!(
            self.player.0 < self.identity.players
                && self.participants.contains(&self.player)
                && self.participants.len() <= 32
                && self.input_lead <= MAX_INPUT_AHEAD,
            "invalid session assignment"
        );
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlayerUpdate {
    pub sequence: u64,
    pub view: PlayerView,
    pub outcomes: Vec<CommandOutcome>,
    pub result: Option<MatchResult>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplayTick {
    pub commands: Vec<Command>,
    pub hash: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Replay {
    pub version: u32,
    pub seed: u64,
    pub initial: WorldSnapshot,
    pub ticks: Vec<ReplayTick>,
}

impl Replay {
    pub fn play(&self, definitions: &World) -> Result<World> {
        ensure!(
            self.version == 1,
            "unsupported replay version {}",
            self.version
        );
        ensure!(
            self.ticks.len() <= MAX_REPLAY_TICKS,
            "replay exceeds tick limit"
        );
        let mut world = definitions.restore_snapshot(self.initial.clone())?;
        for batch in &self.ticks {
            ensure!(
                batch.commands.len() <= MAX_COMMANDS_PER_TICK
                    && batch.commands.windows(2).all(|pair| pair[0] <= pair[1]),
                "noncanonical replay batch"
            );
            world.step(&batch.commands)?;
            ensure!(
                world.state_hash().to_hex().as_str() == batch.hash,
                "replay hash mismatch after tick {}",
                world.tick().0
            );
        }
        Ok(world)
    }
}

pub struct ServerSession {
    world: World,
    seed: u64,
    participants: Vec<PlayerId>,
    pending: BTreeMap<Tick, Vec<Command>>,
    disclosed: BTreeMap<PlayerId, BTreeSet<EntityId>>,
    sequence: u64,
    replay: Replay,
    shots: Vec<crate::sim::ContainerShot>,
}

/// A server checkpoint includes inputs already accepted for future ticks.
/// Connections re-handshake after restore; presentation delivery is ephemeral.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionSnapshot {
    pub version: u32,
    pub world: WorldSnapshot,
    pub seed: u64,
    pub participants: Vec<PlayerId>,
    pub pending: Vec<Command>,
}

impl ServerSession {
    pub fn save_snapshot(&self) -> Result<SessionSnapshot> {
        Ok(SessionSnapshot {
            version: 1,
            world: self.world.save_snapshot()?,
            seed: self.seed,
            participants: self.participants.clone(),
            pending: self.pending.values().flatten().cloned().collect(),
        })
    }

    pub fn restore(definitions: &World, snapshot: SessionSnapshot) -> Result<Self> {
        ensure!(
            snapshot.version == 1
                && snapshot.pending.len() <= MAX_COMMANDS_PER_TICK * (MAX_INPUT_AHEAD as usize + 1),
            "invalid session snapshot version or input count"
        );
        let world = definitions.restore_snapshot(snapshot.world)?;
        let mut server = Self::new(world, snapshot.seed, snapshot.participants)?;
        for command in snapshot.pending {
            server.submit(command.player, command)?;
        }
        Ok(server)
    }

    /// Local saves may migrate to installed fixes. Network/replay checkpoints
    /// continue to use `restore`, with exact gameplay identity checks.
    pub fn restore_saved(definitions: &World, saved: SavedGame) -> Result<Self> {
        let mut checkpoint = saved.checkpoint;
        let world = definitions.restore_saved_snapshot(checkpoint.world, saved.definitions)?;
        checkpoint.world = world.save_snapshot()?;
        Self::restore(&world, checkpoint)
    }

    pub fn new(world: World, seed: u64, participants: Vec<PlayerId>) -> Result<Self> {
        ensure!(
            !world.is_player_view(),
            "server requires authoritative state"
        );
        let unique: BTreeSet<_> = participants.iter().copied().collect();
        ensure!(
            !participants.is_empty()
                && unique.len() == participants.len()
                && participants.iter().all(|p| p.0 < world.map().players
                    && !world.map().ai.iter().any(|ai| ai.player == *p)),
            "invalid human player assignments"
        );
        let initial = world.save_snapshot()?;
        Ok(Self {
            world,
            seed,
            participants,
            pending: BTreeMap::new(),
            disclosed: BTreeMap::new(),
            sequence: 0,
            replay: Replay {
                version: 1,
                seed,
                initial,
                ticks: Vec::new(),
            },
            shots: Vec::new(),
        })
    }

    pub fn world(&self) -> &World {
        &self.world
    }
    pub fn replay(&self) -> &Replay {
        &self.replay
    }

    pub fn restart(&mut self) -> Result<()> {
        self.world = self.world.restore_snapshot(self.replay.initial.clone())?;
        self.pending.clear();
        self.disclosed.clear();
        self.sequence = 0;
        self.replay.ticks.clear();
        self.shots.clear();
        Ok(())
    }

    pub fn connect(&mut self, hello: &Hello) -> Result<(Handshake, PlayerUpdate)> {
        ensure!(
            hello.protocol == PROTOCOL_VERSION,
            "protocol version mismatch"
        );
        let identity = GameplayIdentity::of(&self.world);
        ensure!(
            hello.identity.same_rules(&identity)
                && hello
                    .expected_map
                    .as_ref()
                    .is_none_or(|map| *map == identity.map),
            "gameplay identity mismatch"
        );
        let handshake = self.handshake(hello.player)?;
        let initial = self.update(hello.player, &[])?;
        Ok((handshake, initial))
    }

    pub fn handshake(&self, player: PlayerId) -> Result<Handshake> {
        ensure!(
            self.participants.contains(&player),
            "player is not assigned to this session"
        );
        Ok(Handshake {
            protocol: PROTOCOL_VERSION,
            identity: GameplayIdentity::of(&self.world),
            seed: self.seed,
            player,
            participants: self.participants.clone(),
            input_lead: INPUT_LEAD,
        })
    }

    /// Authenticate before accepting a future command. Duplicated envelopes are
    /// retained in the canonical batch so sequence rejection is replayable.
    pub fn submit(&mut self, player: PlayerId, command: Command) -> Result<()> {
        ensure!(
            self.participants.contains(&player) && command.player == player,
            "unauthorized player command"
        );
        ensure!(
            command.tick.0 >= self.world.tick().0
                && command.tick.0 <= self.world.tick().0.saturating_add(MAX_INPUT_AHEAD),
            "late or excessively future command"
        );
        let batch = self.pending.entry(command.tick).or_default();
        ensure!(batch.len() < MAX_COMMANDS_PER_TICK, "command batch full");
        batch.push(command);
        Ok(())
    }

    /// Server-owned scenario playback can add envelopes without impersonating a
    /// connected client. Both modes then execute the exact same canonical step.
    pub fn advance(&mut self, scenario: &[Command]) -> Result<Vec<CommandOutcome>> {
        ensure!(
            self.replay.ticks.len() < MAX_REPLAY_TICKS,
            "session replay tick limit reached"
        );
        let mut commands = self.pending.remove(&self.world.tick()).unwrap_or_default();
        commands.extend_from_slice(scenario);
        ensure!(
            commands.len() <= MAX_COMMANDS_PER_TICK,
            "command batch full"
        );
        commands.sort();
        let before: BTreeMap<_, _> = self
            .world
            .state()
            .entities
            .iter()
            .map(|e| (e.id, e.cooldown))
            .collect();
        let outcomes = self.world.step(&commands)?;
        self.shots = self
            .world
            .state()
            .entities
            .iter()
            .filter_map(|e| {
                let container = e.garrisoned_in?;
                let old = before.get(&e.id).copied().unwrap_or(0);
                (e.cooldown > 0 && (e.cooldown > old || old <= 1))
                    .then(|| {
                        e.last_attack_position.map(|to| crate::sim::ContainerShot {
                            container,
                            weapon: e.unit_type,
                            heading: crate::sim::public_heading(e.position, to),
                            targets_air: e.last_attack_air,
                        })
                    })
                    .flatten()
            })
            .collect();
        self.shots.dedup();
        self.sequence = self
            .sequence
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("view sequence exhausted"))?;
        self.replay.ticks.push(ReplayTick {
            commands,
            hash: self.world.state_hash().to_hex().to_string(),
        });
        Ok(outcomes)
    }

    pub fn update(
        &mut self,
        player: PlayerId,
        outcomes: &[CommandOutcome],
    ) -> Result<PlayerUpdate> {
        ensure!(
            self.participants.contains(&player),
            "unassigned view player"
        );
        let mut view = self.world.player_view(player)?;
        view.shots = self
            .shots
            .iter()
            .filter(|shot| self.world.entity_visible(player, shot.container))
            .cloned()
            .collect();
        let current: BTreeSet<_> = view
            .entities
            .iter()
            .map(|e| match e {
                ViewedEntity::Owned(e) => e.id,
                ViewedEntity::Visible(e) => e.id,
            })
            .collect();
        let alive: BTreeSet<_> = self.world.state().entities.iter().map(|e| e.id).collect();
        if let Some(previous) = self.disclosed.insert(player, current) {
            view.removed = previous.difference(&alive).copied().collect();
        }
        Ok(PlayerUpdate {
            result: self.match_result(),
            sequence: self.sequence,
            view,
            outcomes: outcomes
                .iter()
                .filter(|o| o.command.player == player)
                .cloned()
                .collect(),
        })
    }
}
