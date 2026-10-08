//! Finite changes between content-defined unit forms, independent of production.
use super::*;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModeChange {
    pub target: UnitTypeId,
    pub ticks: u32,
    pub research: Option<ResearchId>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModeTransition {
    pub remaining: u32,
    pub total: u32,
}

pub(super) fn validate(rules: &Rules) -> Result<()> {
    for unit in &rules.units {
        let Some(mode) = &unit.mode else { continue };
        let target = rules.units.iter().find(|u| u.id == mode.target);
        ensure!(
            mode.target != unit.id
                && (1..=10000).contains(&mode.ticks)
                && !unit.structure
                && !unit.transforms_on_production
                && target.is_some_and(|t| !t.structure
                    && t.max_hp == unit.max_hp
                    && t.max_shields == unit.max_shields
                    && t.supply_used == unit.supply_used
                    && t.footprint == unit.footprint
                    && t.movement_class == unit.movement_class),
            "invalid unit mode"
        );
    }
    Ok(())
}
impl World {
    pub fn mode_rejection(&self, entity: EntityId) -> Option<Rejection> {
        let Some(index) = self.index(entity) else {
            return Some(Rejection::UnknownEntity);
        };
        let actor = &self.state.entities[index];
        let Some(mode) = &self.unit_at(index).mode else {
            return Some(Rejection::UnsupportedOrder);
        };
        if actor.construction.is_some() || actor.garrisoned_in.is_some() {
            return Some(Rejection::Unfinished);
        }
        if actor.mode_transition.is_some() {
            return Some(Rejection::Cooldown);
        }
        if mode
            .research
            .is_some_and(|r| !self.has_research(actor.owner, r))
        {
            return Some(Rejection::MissingPrerequisite);
        }
        None
    }
    pub(super) fn start_mode_change(&mut self, index: usize) -> Option<Rejection> {
        if let Some(reason) = self.mode_rejection(self.state.entities[index].id) {
            return Some(reason);
        }
        let total = self.unit_at(index).mode.as_ref().unwrap().ticks;
        self.assign(index, UnitOrder::Idle, true);
        self.state.entities[index].mode_transition = Some(ModeTransition {
            remaining: total,
            total,
        });
        None
    }
    pub(super) fn advance_mode(&mut self, index: usize) -> bool {
        let Some(progress) = &mut self.state.entities[index].mode_transition else {
            return false;
        };
        progress.remaining = progress.remaining.saturating_sub(1);
        if progress.remaining == 0 {
            self.state.entities[index].unit_type =
                self.unit_at(index).mode.as_ref().unwrap().target;
            self.state.entities[index].mode_transition = None;
        }
        true
    }
}
pub(super) fn put_rules(bytes: &mut Vec<u8>, rules: &Rules) {
    for u in &rules.units {
        if let Some(m) = &u.mode {
            bytes.extend(b"unit-mode-v1");
            bytes.extend(u.id.0.to_le_bytes());
            bytes.extend(m.target.0.to_le_bytes());
            bytes.extend(m.ticks.to_le_bytes());
            bytes.push(u8::from(m.research.is_some()));
            if let Some(id) = m.research {
                bytes.extend(id.0.to_le_bytes());
            }
        }
    }
}
pub(super) fn put_state(bytes: &mut Vec<u8>, state: &State) {
    for e in &state.entities {
        if let Some(m) = &e.mode_transition {
            bytes.extend(b"mode-state-v1");
            bytes.extend(e.id.0.to_le_bytes());
            bytes.extend(m.remaining.to_le_bytes());
            bytes.extend(m.total.to_le_bytes());
        }
    }
}
