//! Global content-defined promotion of existing units and future recruits.
use super::*;

impl World {
    pub fn can_train_type(&self, entity: &Entity, unit: UnitTypeId) -> bool {
        self.unit_type(entity.unit_type).is_some_and(|producer| {
            producer.trains.iter().any(|original| {
                *original == unit || self.researched_unit_type(entity.owner, *original) == unit
            })
        })
    }

    pub fn researched_unit_type(&self, owner: PlayerId, mut unit: UnitTypeId) -> UnitTypeId {
        // IDs must form an acyclic graph (validated below). This bound also keeps
        // malformed externally supplied views from looping during presentation.
        for _ in 0..self.rules.research.len() {
            let next = self.rules.research.iter().find_map(|r| match &r.effect {
                ResearchEffect::UnitUpgrade { units, to }
                    if units.contains(&unit) && self.has_research(owner, r.id) =>
                {
                    Some(*to)
                }
                _ => None,
            });
            let Some(next) = next else { break };
            unit = next;
        }
        unit
    }

    pub(in crate::sim) fn apply_unit_upgrades(&mut self) {
        if !self
            .rules
            .research
            .iter()
            .any(|r| matches!(r.effect, ResearchEffect::UnitUpgrade { .. }))
        {
            return;
        }
        for i in 0..self.state.entities.len() {
            let actor = &self.state.entities[i];
            if actor.hp == 0 || actor.construction.is_some() {
                continue;
            }
            let upgraded = self.researched_unit_type(actor.owner, actor.unit_type);
            if upgraded == actor.unit_type {
                continue;
            }
            let missing = self
                .unit_type(actor.unit_type)
                .unwrap()
                .max_hp
                .saturating_sub(actor.hp);
            let definition = self.unit_type(upgraded).unwrap().clone();
            let actor = &mut self.state.entities[i];
            actor.unit_type = upgraded;
            actor.hp = definition.max_hp.saturating_sub(missing).max(1);
            actor.energy = definition.initial_energy();
            actor.strikes.clear();
            actor.last_cast = None;
        }
    }
}

pub(super) fn validate(rules: &Rules) -> Result<()> {
    let mut edges = BTreeMap::new();
    for research in &rules.research {
        if let ResearchEffect::UnitUpgrade { units, to } = &research.effect {
            for unit in units {
                ensure!(edges.insert(*unit, *to).is_none(), "ambiguous unit upgrade");
            }
        }
    }
    for start in edges.keys() {
        let mut seen = BTreeSet::new();
        let mut unit = start;
        while let Some(next) = edges.get(unit) {
            ensure!(seen.insert(unit), "cyclic unit upgrade");
            unit = next;
        }
    }
    Ok(())
}
