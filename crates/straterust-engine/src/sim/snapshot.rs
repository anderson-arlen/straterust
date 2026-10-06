//! Versioned authoritative snapshots. These never belong in a player update.
use super::*;

pub const SNAPSHOT_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GameplayIdentity {
    pub simulation: String,
    pub rules: String,
    pub map: String,
    pub tick_ms: u32,
    pub players: u16,
}

impl GameplayIdentity {
    pub fn same_rules(&self, other: &Self) -> bool {
        self.simulation == other.simulation
            && self.rules == other.rules
            && self.tick_ms == other.tick_ms
    }

    pub fn of(world: &World) -> Self {
        Self {
            simulation: SIMULATION_REVISION.into(),
            rules: world.rules_hash().to_hex().to_string(),
            map: world.map_hash().to_hex().to_string(),
            tick_ms: world.rules().tick_ms,
            players: world.map().players,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorldSnapshot {
    pub version: u32,
    pub identity: GameplayIdentity,
    pub state: State,
    pub state_hash: String,
    /// Includes cosmetic mission events, which the deterministic hash excludes.
    pub checksum: String,
}

impl World {
    pub fn save_snapshot(&self) -> Result<WorldSnapshot> {
        ensure!(
            !self.is_player_view(),
            "cannot save an authoritative snapshot from a player view"
        );
        let state = self.state.clone();
        Ok(WorldSnapshot {
            version: SNAPSHOT_VERSION,
            identity: GameplayIdentity::of(self),
            state_hash: self.state_hash().to_hex().to_string(),
            checksum: blake3::hash(ron::ser::to_string(&state)?.as_bytes())
                .to_hex()
                .to_string(),
            state,
        })
    }

    pub fn restore_snapshot(&self, snapshot: WorldSnapshot) -> Result<World> {
        ensure!(
            !self.is_player_view(),
            "snapshot restore requires server definitions"
        );
        ensure!(
            snapshot.version == SNAPSHOT_VERSION,
            "unsupported snapshot version {}",
            snapshot.version
        );
        ensure!(
            snapshot.identity == GameplayIdentity::of(self),
            "snapshot gameplay identity mismatch"
        );
        ensure!(
            snapshot.checksum
                == blake3::hash(ron::ser::to_string(&snapshot.state)?.as_bytes())
                    .to_hex()
                    .as_str(),
            "snapshot checksum mismatch"
        );
        self.validate_snapshot_state(&snapshot.state)?;
        let mut restored = self.snapshot();
        restored.state = snapshot.state;
        restored.weapon_feedback.clear();
        ensure!(
            restored.state_hash().to_hex().as_str() == snapshot.state_hash,
            "snapshot state hash mismatch"
        );
        Ok(restored)
    }

    fn validate_snapshot_state(&self, state: &State) -> Result<()> {
        let players = usize::from(self.map.players);
        let cells = ((self.map.width + 31) / 32 * ((self.map.height + 31) / 32)) as usize;
        ensure!(
            state.tick.0 < u64::MAX
                && state.players.len() == players
                && state.statistics.len() == players
                && state.last_sequences.len() == players,
            "invalid snapshot player/tick state"
        );
        ensure!(
            state.entities.len() <= 4096
                && state.resources.len() <= 4096
                && state.scans.len() <= 4096,
            "snapshot entity limit exceeded"
        );
        ensure!(
            state
                .entities
                .windows(2)
                .all(|pair| pair[0].id < pair[1].id)
                && state
                    .entities
                    .iter()
                    .all(|entity| entity.id.0 > 0 && entity.id.0 < state.next_entity_id),
            "invalid snapshot entity IDs"
        );
        let grid = |layers: &[Vec<u8>]| {
            layers.len() == players && layers.iter().all(|layer| layer.len() == cells)
        };
        ensure!(
            (self.map.fog_of_war
                && grid(&state.fog)
                && grid(&state.terrain_fog)
                && state.terrain_fog.iter().flatten().all(|value| *value <= 2))
                || (!self.map.fog_of_war && state.fog.is_empty() && state.terrain_fog.is_empty()),
            "invalid snapshot fog dimensions"
        );
        ensure!(
            (state.creep.is_empty() && state.creep_seen.is_empty())
                || (state.creep.len() == cells
                    && grid(&state.creep_seen)
                    && state.creep_seen.iter().flatten().all(|v| *v <= 1)),
            "invalid snapshot creep dimensions"
        );
        for entity in &state.entities {
            let unit = self
                .unit_type(entity.unit_type)
                .ok_or_else(|| anyhow::anyhow!("unknown snapshot unit type"))?;
            ensure!(
                entity.owner.0 < self.map.players
                    && self.map.contains(entity.position)
                    && entity.hp > 0
                    && entity.shields <= unit.max_shields * 256
                    && entity.hp <= unit.max_hp
                    && entity.energy <= unit.energy_max() * 256
                    && entity.path.len() <= 65536
                    && entity.path.iter().all(|p| self.map.contains(*p))
                    && entity.harvest_spot.is_none_or(|point| {
                        self.map.contains_footprint(point, unit.footprint)
                            && matches!(entity.order, UnitOrder::Gather { .. })
                    })
                    && entity.gather_origin.is_none_or(|point| {
                        self.map.contains(point) && matches!(entity.order, UnitOrder::Gather { .. })
                    })
                    && entity.route_wait.as_ref().is_none_or(|wait| {
                        wait.since <= state.tick
                            && self.map.contains(wait.origin)
                            && wait.ready_at >= wait.since
                            && wait.alternate.len() <= 65536
                            && wait.alternate.iter().all(|p| self.map.contains(*p))
                    })
                    && entity.queued_orders.len() <= 32
                    && entity.production.len() <= 32
                    && entity.strikes.len() <= 4096
                    && entity.repair_credit.len() <= 4096,
                "invalid snapshot entity state"
            );
            ensure!(
                entity
                    .construction
                    .as_ref()
                    .is_none_or(|job| job.total > 0 && job.remaining <= job.total)
                    && entity
                        .production
                        .iter()
                        .all(|job| self.unit_type(job.unit_type).is_some()
                            && job.total > 0
                            && job.remaining <= job.total),
                "invalid snapshot job"
            );
        }
        ensure!(
            state
                .players
                .iter()
                .all(|p| p.resources.len() <= 4096 && p.completed_research.len() <= 4096)
                && state
                    .resources
                    .iter()
                    .all(|r| self.map.contains(r.position) && r.kind.len() <= 64)
                && state.winner.is_none_or(|p| p.0 < self.map.players)
                && state.defeated.len() <= players
                && state.defeated.iter().all(|p| p.0 < self.map.players),
            "invalid snapshot economy/outcome"
        );
        ensure!(
            state.ai.len() == self.map.ai.len(),
            "invalid snapshot AI state count"
        );
        if let Some(mission) = &state.mission {
            let definition = self
                .map
                .mission
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("unexpected snapshot mission"))?;
            ensure!(
                mission.triggers.len() == definition.triggers.len()
                    && mission.switches.len() == 256
                    && mission.locations.len() == definition.locations.len()
                    && mission.events.len() <= 4096,
                "invalid snapshot mission state"
            );
        } else {
            ensure!(self.map.mission.is_none(), "missing snapshot mission state");
        }
        Ok(())
    }
}
