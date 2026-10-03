//! A secondary energy ability: toggling concealment preserves ordinary orders.
use super::*;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cloak {
    pub energy_max: u32,
    pub activation_cost: u32,
    /// Energy in 1/256 units per simulation tick.
    pub regeneration: u32,
    pub drain: u32,
}
impl Cloak {
    pub(super) fn validate(&self) -> Result<()> {
        ensure!(
            (1..=10000).contains(&self.energy_max)
                && self.activation_cost <= self.energy_max
                && self.regeneration <= 256
                && (1..=256).contains(&self.drain),
            "invalid cloak rules"
        );
        Ok(())
    }
    pub(super) fn put(&self, bytes: &mut Vec<u8>) {
        for value in [
            self.energy_max,
            self.activation_cost,
            self.regeneration,
            self.drain,
        ] {
            bytes.extend(value.to_le_bytes());
        }
    }
}
impl UnitType {
    pub fn energy_max(&self) -> u32 {
        self.cloak.as_ref().map_or_else(
            || self.scanner.as_ref().map_or(0, |s| s.energy_max),
            |c| c.energy_max,
        )
    }
    pub fn initial_energy(&self) -> u32 {
        self.cloak.as_ref().map_or_else(
            || self.scanner.as_ref().map_or(0, |s| s.energy_initial * 256),
            |c| c.energy_max * 64,
        )
    }
}
impl World {
    pub fn cloak_rejection(&self, id: EntityId, enabled: bool) -> Option<Rejection> {
        let Some(actor) = self.state.entities.iter().find(|e| e.id == id) else {
            return Some(Rejection::UnknownEntity);
        };
        let Some(cloak) = &self.unit_type(actor.unit_type)?.cloak else {
            return Some(Rejection::UnsupportedOrder);
        };
        if actor.construction.is_some() || actor.garrisoned_in.is_some() {
            return Some(Rejection::Unfinished);
        }
        if enabled && !self.ability_research_ready(actor, true) {
            return Some(Rejection::MissingPrerequisite);
        }
        if enabled && !actor.cloaked && actor.energy < cloak.activation_cost * 256 {
            return Some(Rejection::InsufficientResources);
        }
        None
    }
    pub(super) fn toggle_cloak(&mut self, index: usize, enabled: bool) -> Option<Rejection> {
        let actor = &self.state.entities[index];
        if let Some(reason) = self.cloak_rejection(actor.id, enabled) {
            return Some(reason);
        }
        if enabled && !actor.cloaked {
            let cost = self.unit_at(index).cloak.as_ref()?.activation_cost * 256;
            self.state.entities[index].energy -= cost;
        }
        self.state.entities[index].cloaked = enabled;
        None
    }
    pub(super) fn advance_cloaks(&mut self) {
        if self.state.winner.is_some() || self.state.mission.as_ref().is_some_and(|m| m.paused) {
            return;
        }
        for index in 0..self.state.entities.len() {
            let maximum = self.energy_max(&self.state.entities[index]);
            let entity = &mut self.state.entities[index];
            if entity.construction.is_some() {
                continue;
            }
            let unit = &self.rules.units[self
                .rules
                .units
                .binary_search_by_key(&entity.unit_type, |u| u.id)
                .expect("type")];
            let Some(cloak) = &unit.cloak else { continue };
            if entity.cloaked {
                entity.energy = entity.energy.saturating_sub(cloak.drain);
                if entity.energy == 0 {
                    entity.cloaked = false;
                }
            } else {
                entity.energy = (entity.energy + cloak.regeneration).min(maximum * 256);
            }
        }
    }
}
