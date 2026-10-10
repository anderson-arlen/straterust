//! Bounded authoritative remains for content that can raise fallen units.
use super::*;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Remains {
    pub unit_type: UnitTypeId,
    pub position: Position,
    pub remaining: u32,
}
impl World {
    pub(in crate::sim) fn advance_remains(&mut self) {
        for remains in &mut self.state.remains {
            remains.remaining = remains.remaining.saturating_sub(1);
        }
        self.state.remains.retain(|r| r.remaining > 0);
        let policies: Vec<_> = self
            .rules
            .units
            .iter()
            .flat_map(|u| &u.abilities)
            .filter_map(|a| {
                if let AbilityEffect::RaiseDead {
                    affected,
                    corpse_ticks,
                    ..
                } = &a.effect
                {
                    Some((affected, *corpse_ticks))
                } else {
                    None
                }
            })
            .collect();
        if policies.is_empty() {
            return;
        }
        for entity in &self.state.entities {
            if entity.hp != 0 || entity.garrisoned_in.is_some() {
                continue;
            }
            if let Some(remaining) = policies
                .iter()
                .filter(|(units, _)| units.contains(&entity.unit_type))
                .map(|(_, ticks)| *ticks)
                .max()
            {
                self.state.remains.push(Remains {
                    unit_type: entity.unit_type,
                    position: entity.position,
                    remaining,
                });
            }
        }
        if self.state.remains.len() > 4096 {
            self.state.remains.drain(..self.state.remains.len() - 4096);
        }
    }
}
