//! One concealment ability. Content chooses mobility, combat, collision,
//! transition timing, automatic activation policy and energy requirements.
use super::*;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConcealmentField {
    pub radius: u32,
    pub affected: Vec<UnitTypeId>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Cloak {
    pub energy_max: u32,
    pub activation_cost: u32,
    /// Energy in 1/256 units per simulation tick; zero means no upkeep.
    pub regeneration: u32,
    pub drain: u32,
    pub can_move: bool,
    pub can_attack: bool,
    pub blocks_movement: bool,
    pub reveal_ticks: u32,
    pub reveal_on_order: bool,
    pub auto_reveal: bool,
}
impl Default for Cloak {
    fn default() -> Self {
        Self {
            energy_max: 0,
            activation_cost: 0,
            regeneration: 0,
            drain: 0,
            can_move: true,
            can_attack: true,
            blocks_movement: true,
            reveal_ticks: 0,
            reveal_on_order: false,
            auto_reveal: false,
        }
    }
}
impl Cloak {
    pub(super) fn validate(&self) -> Result<()> {
        ensure!(
            self.energy_max <= 10000
                && self.activation_cost <= self.energy_max
                && self.regeneration <= 256
                && self.drain <= 256
                && self.reveal_ticks <= 10000
                && (self.energy_max != 0 || self.drain == 0 && self.regeneration == 0),
            "invalid concealment rules"
        );
        Ok(())
    }
    pub(super) fn put(&self, bytes: &mut Vec<u8>) {
        for value in [
            self.energy_max,
            self.activation_cost,
            self.regeneration,
            self.drain,
            self.reveal_ticks,
        ] {
            bytes.extend(value.to_le_bytes());
        }
        for flag in [
            self.can_move,
            self.can_attack,
            self.blocks_movement,
            self.reveal_on_order,
            self.auto_reveal,
        ] {
            bytes.push(u8::from(flag));
        }
    }
}
impl UnitType {
    pub fn energy_max(&self) -> u32 {
        if let Some(pool) = &self.energy_pool {
            return pool.maximum;
        }
        self.cloak.as_ref().map_or_else(
            || self.scanner.as_ref().map_or(0, |s| s.energy_max),
            |c| c.energy_max,
        )
    }
    pub fn initial_energy(&self) -> u32 {
        if let Some(pool) = &self.energy_pool {
            return pool.initial * 256;
        }
        self.cloak.as_ref().map_or_else(
            || self.scanner.as_ref().map_or(0, |s| s.energy_initial * 256),
            |c| c.energy_max * 64,
        )
    }
}
impl World {
    pub fn concealed(&self, entity: &Entity) -> bool {
        entity.cloaked
            || self.state.entities.iter().any(|source| {
                source.owner == entity.owner
                    && source.id != entity.id
                    && source.hp > 0
                    && source.construction.is_none()
                    && source.garrisoned_in.is_none()
                    && !self.disabled(source)
                    && self
                        .unit_type(source.unit_type)
                        .unwrap()
                        .concealment_field
                        .as_ref()
                        .is_some_and(|f| {
                            f.affected.contains(&entity.unit_type)
                                && rts::distance(source.position, entity.position)
                                    <= i64::from(f.radius).pow(2)
                        })
            })
    }
    pub fn movement_locked(&self, entity: &Entity) -> bool {
        self.disabled(entity)
            || entity.cloak_transition != 0
            || entity.cloaked
                && self
                    .unit_type(entity.unit_type)
                    .and_then(|u| u.cloak.as_ref())
                    .is_some_and(|c| !c.can_move)
    }
    pub fn attacks_locked(&self, entity: &Entity) -> bool {
        self.disabled(entity)
            || entity.cloak_transition != 0
            || entity.cloaked
                && self
                    .unit_type(entity.unit_type)
                    .and_then(|u| u.cloak.as_ref())
                    .is_some_and(|c| !c.can_attack)
    }
    pub(super) fn phases_collision(&self, entity: &Entity) -> bool {
        entity.cloaked
            && self.unit_type(entity.unit_type).is_some_and(|u| {
                u.mine.is_some() || u.cloak.as_ref().is_some_and(|c| !c.blocks_movement)
            })
    }
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
        if actor.cloak_transition != 0 {
            return Some(Rejection::Cooldown);
        }
        if enabled && !self.ability_research_ready(actor, true) {
            return Some(Rejection::MissingPrerequisite);
        }
        if enabled && !actor.cloaked && actor.energy < cloak.activation_cost * 256 {
            return Some(Rejection::InsufficientResources);
        }
        None
    }
    pub(super) fn reveal(&mut self, index: usize) {
        if self.state.entities[index].cloaked {
            self.state.entities[index].cloaked = false;
            self.state.entities[index].cloak_transition = self
                .unit_at(index)
                .cloak
                .as_ref()
                .map_or(0, |c| c.reveal_ticks);
        }
    }
    pub(super) fn toggle_cloak(&mut self, index: usize, enabled: bool) -> Option<Rejection> {
        if let Some(reason) = self.cloak_rejection(self.state.entities[index].id, enabled) {
            return Some(reason);
        }
        if enabled && !self.state.entities[index].cloaked {
            let cloak = self.unit_at(index).cloak.as_ref()?.clone();
            if !cloak.can_move || !cloak.can_attack {
                self.assign(index, UnitOrder::Hold, true);
            }
            self.state.entities[index].energy -= cloak.activation_cost * 256;
            self.state.entities[index].cloaked = true;
        } else if !enabled {
            let stationary = self
                .unit_at(index)
                .cloak
                .as_ref()
                .is_some_and(|c| !c.can_move);
            self.reveal(index);
            if stationary {
                self.assign(index, UnitOrder::Idle, true);
            }
        }
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
                if cloak.drain != 0 && entity.energy == 0 {
                    self.reveal(index);
                }
            } else {
                entity.energy = (entity.energy + cloak.regeneration).min(maximum * 256);
            }
        }
    }
}

#[cfg(test)]
mod tests;
