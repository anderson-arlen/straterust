//! Scripted full waves and independently ready skirmish groups.
use super::*;

impl World {
    pub(super) fn ai_attack(
        &mut self,
        controller: &AiController,
        state: &mut AiState,
        available: bool,
    ) -> bool {
        if !available
            && state.attack.iter().any(|(id, count)| {
                self.ai_count(controller, state, *id)
                    < count.saturating_add(state.defense.get(id).copied().unwrap_or(0))
            })
        {
            return false;
        }
        let Some(target) = self.ai_target(controller) else {
            return false;
        };
        let mut wave = Vec::new();
        for (&unit_type, &count) in &state.attack {
            let members: Vec<_> = self
                .state
                .entities
                .iter()
                .filter(|e| {
                    e.owner == controller.player
                        && e.unit_type == self.researched_unit_type(controller.player, unit_type)
                        && e.construction.is_none()
                        && e.garrisoned_in.is_none()
                        && self.ai_in_town(controller, state, e)
                        && !state.deployed.contains(&e.id)
                        && !self.state.ai.iter().any(|s| s.deployed.contains(&e.id))
                })
                .skip(usize::from(
                    state.defense.get(&unit_type).copied().unwrap_or(0),
                ))
                .take(usize::from(count))
                .map(|e| e.id)
                .collect();
            if !available && members.len() < usize::from(count) {
                return false;
            }
            wave.extend(members);
        }
        if (available && wave.is_empty())
            || (state.transport_needed
                && !self.ai_load_wave(controller, state, &wave, target, available))
        {
            return false;
        }
        for entity in wave {
            if state.transport_needed
                && self
                    .unit_type(self.state.entities[self.index(entity).unwrap()].unit_type)
                    .is_some_and(|u| u.movement_class == MovementClass::Ground)
            {
                continue;
            }
            if self.ai_order(controller, state, Order::AttackMove { entity, target }) {
                state.deployed.insert(entity);
            }
        }
        state.prepared = false;
        true
    }
}
