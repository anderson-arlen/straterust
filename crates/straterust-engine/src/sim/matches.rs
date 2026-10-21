//! Match objectives are separate from campaign trigger execution.
use super::*;
use crate::multiplayer::{GameType, MatchRules};
impl MatchRules {
    pub(crate) fn encode(&self, bytes: &mut Vec<u8>) {
        bytes.extend((self.participants.len() as u32).to_le_bytes());
        for p in &self.participants {
            bytes.extend(p.0.to_le_bytes());
        }
        bytes.extend((self.teams.len() as u32).to_le_bytes());
        for team in &self.teams {
            bytes.extend((team.len() as u32).to_le_bytes());
            for p in team {
                bytes.extend(p.0.to_le_bytes());
            }
        }
        bytes.push(self.mode as u8);
        bytes.extend(self.parameter.to_le_bytes());
        bytes.push(u8::from(self.cheats));
        bytes.push(u8::from(self.defeat_without_structures));
        bytes.extend((self.resources.len() as u32).to_le_bytes());
        for (p, amounts) in &self.resources {
            bytes.extend(p.0.to_le_bytes());
            bytes.extend((amounts.len() as u32).to_le_bytes());
            for amount in amounts {
                put_string(bytes, &amount.kind);
                bytes.extend(amount.amount.to_le_bytes());
            }
        }
        bytes.extend((self.bases.len() as u32).to_le_bytes());
        for id in &self.bases {
            bytes.extend(id.0.to_le_bytes());
        }
        bytes.extend((self.flags.len() as u32).to_le_bytes());
        for flag in &self.flags {
            bytes.extend(flag.item.0.to_le_bytes());
            bytes.extend(flag.owner.0.to_le_bytes());
            put_position(bytes, flag.position);
        }
    }
}
impl World {
    pub(in crate::sim) fn update_multiplayer_victory(&mut self) {
        let setup = self.map.multiplayer.as_ref().expect("match rules");
        if self.state.winner.is_some() {
            return;
        }
        let alive: BTreeSet<_> = self
            .state
            .entities
            .iter()
            .filter(|e| {
                setup.participants.contains(&e.owner)
                    && !self.unit_type(e.unit_type).unwrap().neutral
                    && (!setup.defeat_without_structures
                        || self.unit_type(e.unit_type).unwrap().structure)
                    && (setup.mode != GameType::SuddenDeath || setup.bases.contains(&e.unit_type))
            })
            .map(|e| e.owner)
            .collect();
        self.state.defeated = setup
            .participants
            .iter()
            .filter(|p| !alive.contains(p))
            .copied()
            .collect();
        let mut winner = None;
        if matches!(
            setup.mode,
            GameType::CaptureTheFlag | GameType::TeamCaptureTheFlag
        ) {
            winner = setup.flags.iter().find_map(|flag| {
                let item = self.state.entities.iter().find(|e| e.id == flag.item)?;
                let carrier = self
                    .state
                    .entities
                    .iter()
                    .find(|e| Some(e.id) == item.carried_by)?;
                let home = setup.flags.iter().find(|f| f.owner == carrier.owner)?;
                (flag.owner != carrier.owner
                    && !setup.allied(flag.owner, carrier.owner)
                    && rts::distance(item.position, home.position) <= 32 * 32)
                    .then_some(carrier.owner)
            });
        } else if setup.mode == GameType::Greed {
            winner = setup
                .participants
                .iter()
                .find(|p| {
                    self.state.players[usize::from(p.0)]
                        .resources
                        .values()
                        .sum::<u64>()
                        >= u64::from(setup.parameter)
                })
                .copied();
        } else if setup.mode == GameType::Slaughter
            && self.tick().0.saturating_mul(u64::from(self.rules.tick_ms))
                >= u64::from(setup.parameter) * 60000
        {
            let kills = |p: &PlayerId| {
                self.state.kills.get(p).map_or(0, |units| {
                    units.values().map(|count| u64::from(*count)).sum::<u64>()
                })
            };
            let most = setup.participants.iter().map(kills).max().unwrap_or(0);
            let leaders: Vec<_> = setup
                .participants
                .iter()
                .filter(|p| kills(p) == most)
                .copied()
                .collect();
            if leaders.len() == 1 {
                winner = Some(leaders[0]);
            } else {
                self.state.defeated = setup.participants.clone();
            }
        }
        if winner.is_none()
            && (alive.len() == 1
                || (!alive.is_empty()
                    && alive
                        .iter()
                        .all(|p| setup.allied(*alive.first().unwrap(), *p))))
        {
            winner = alive.first().copied();
        }
        self.state.winner = winner;
    }
}
