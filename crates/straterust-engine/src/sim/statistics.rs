//! Authoritative match counters. These describe play, never affect gameplay,
//! and are disclosed to clients only once the session has ended.
use super::*;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PlayerStatistics {
    pub units_produced: u32,
    pub units_killed: u32,
    pub units_lost: u32,
    pub structures_built: u32,
    pub structures_razed: u32,
    pub structures_lost: u32,
    pub resources_collected: BTreeMap<String, u64>,
}

impl PlayerStatistics {
    pub(super) fn created(&mut self, structure: bool) {
        let counter = if structure {
            &mut self.structures_built
        } else {
            &mut self.units_produced
        };
        *counter = counter.saturating_add(1);
    }

    pub(super) fn lost(&mut self, structure: bool) {
        let counter = if structure {
            &mut self.structures_lost
        } else {
            &mut self.units_lost
        };
        *counter = counter.saturating_add(1);
    }
}

impl World {
    pub(in crate::sim) fn record_created(&mut self, player: PlayerId, unit: UnitTypeId) {
        let structure = self.unit_type(unit).unwrap().structure;
        self.state.statistics[usize::from(player.0)].created(structure);
    }

    pub(in crate::sim) fn record_losses(&mut self, matches: impl Fn(&Entity) -> bool) {
        for entity in self.state.entities.iter().filter(|e| matches(e)) {
            let structure = self
                .rules
                .units
                .iter()
                .find(|u| u.id == entity.unit_type)
                .unwrap()
                .structure;
            self.state.statistics[usize::from(entity.owner.0)].lost(structure);
        }
    }
}

#[cfg(test)]
mod tests;
