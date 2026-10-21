//! Final, aggregate reports shared by local and remote sessions. Live views do
//! not carry opponents' counters, and reports never contain hidden entities.
use super::*;
use crate::sim::PlayerStatistics;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MatchOutcome {
    Victory,
    Defeat,
    Draw,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlayerResult {
    pub player: PlayerId,
    /// Connection display names are attached by the network host, outside simulation.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    pub outcome: MatchOutcome,
    pub statistics: PlayerStatistics,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MatchResult {
    pub tick: Tick,
    pub tick_ms: u32,
    pub players: Vec<PlayerResult>,
}

impl MatchResult {
    pub fn validate(&self, world: &World) -> Result<()> {
        let players: BTreeSet<_> = self.players.iter().map(|p| p.player).collect();
        ensure!(
            self.tick == world.tick()
                && self.tick_ms == world.rules().tick_ms
                && !players.is_empty()
                && players.len() == self.players.len()
                && players.contains(&world.view_player())
                && players.iter().all(|p| p.0 < world.map().players),
            "invalid match report"
        );
        ensure!(
            self.players.iter().all(|p| p.name.chars().count() <= 216
                && !p.name.chars().any(char::is_control)
                && p.statistics.resources_collected.len() <= 128
                && p.statistics
                    .resources_collected
                    .keys()
                    .all(|kind| !kind.is_empty() && kind.len() <= 64)),
            "invalid match resource counters"
        );
        Ok(())
    }
}

impl ServerSession {
    pub fn match_result(&self) -> Option<MatchResult> {
        let state = self.world.state();
        let participants = self
            .world
            .map()
            .multiplayer
            .as_ref()
            .map_or(&self.participants, |m| &m.participants);
        let survivors: Vec<_> = self
            .world
            .map()
            .multiplayer
            .as_ref()
            .map_or(&self.participants, |m| &m.participants)
            .iter()
            .filter(|p| !state.defeated.contains(p))
            .copied()
            .collect();
        let eliminated = survivors.len() < participants.len();
        let winner = state.winner.or_else(|| {
            (participants.len() > 1 && eliminated && survivors.len() == 1).then(|| survivors[0])
        });
        if winner.is_none() && !(eliminated && survivors.is_empty()) {
            return None;
        }
        let draw = winner.is_none() && participants.len() > 1;
        Some(MatchResult {
            tick: state.tick,
            tick_ms: self.world.rules().tick_ms,
            players: participants
                .iter()
                .map(|&player| PlayerResult {
                    player,
                    name: String::new(),
                    outcome: if draw {
                        MatchOutcome::Draw
                    } else if winner.is_some_and(|winner| {
                        winner == player
                            || self
                                .world
                                .map()
                                .multiplayer
                                .as_ref()
                                .is_some_and(|m| m.allied(winner, player))
                    }) {
                        MatchOutcome::Victory
                    } else {
                        MatchOutcome::Defeat
                    },
                    statistics: {
                        let mut statistics = state.statistics[usize::from(player.0)].clone();
                        for kind in self.world.map().resources.iter().map(|r| &r.kind).chain(
                            self.world
                                .rules()
                                .starting_resources
                                .iter()
                                .map(|r| &r.kind),
                        ) {
                            statistics
                                .resources_collected
                                .entry(kind.clone())
                                .or_default();
                        }
                        statistics
                    },
                })
                .collect(),
        })
    }
}
