//! Confirmed effects survive speculative removals and corrections to older ticks.
use super::*;

impl Visuals {
    /// Observe each authoritative view once, before replaying prediction. Keep
    /// the last disclosed body independently of speculative living-unit visuals.
    pub fn confirm(&mut self, world: &World) {
        let previous = self.confirmed_bodies.take().unwrap_or_default();
        let mut current = BTreeMap::new();
        for entity in world
            .state()
            .entities
            .iter()
            .filter(|e| !world.construction_pending(e))
        {
            let facing = self
                .units
                .get(&entity.id)
                .map(|v| v.facing)
                .or_else(|| previous.get(&entity.id).map(|v: &DeathVisual| v.facing))
                .unwrap_or(0);
            current.insert(
                entity.id,
                DeathVisual {
                    owner: entity.owner,
                    position: entity.position,
                    unit_type: entity.unit_type,
                    facing,
                    elapsed: Duration::ZERO,
                },
            );
        }
        for (id, body) in previous {
            if !current.contains_key(&id)
                && world.disclosed_death(id)
                && !world
                    .unit_type(body.unit_type)
                    .is_some_and(|unit| unit.revealer)
            {
                if self.deaths.len() == MAX_DEATH_VISUALS {
                    self.deaths.remove(0);
                }
                self.deaths.push(body);
            }
        }
        self.confirmed_bodies = Some(current);
        self.observe_events(world, world.tick().0.max(self.tick));
    }

    /// Apply a reconciled body at the display tick, preserving local clip/effect
    /// clocks even when the authoritative baseline tick is older or unchanged.
    pub fn reconcile(&mut self, world: &World) {
        self.update_bodies(world, true);
    }

    pub(super) fn observe_events(&mut self, world: &World, tick: u64) {
        for shot in world.public_shots() {
            if self.container_shots.len() >= 4096 {
                self.container_shots.remove(0);
            }
            self.container_shots.push((shot.clone(), tick));
        }
        for impact in world.public_weapon_feedback().iter().filter(|e| e.impact) {
            if self.projectiles.len() == MAX_DEATH_VISUALS {
                self.projectiles.remove(0);
            }
            self.projectiles.push(ProjectileVisual {
                targets_air: impact.targets_air,
                unit_type: impact.weapon,
                owner: world.view_player(),
                from: impact.position,
                to: impact.position,
                elapsed: Duration::ZERO,
                tick_ms: world.rules().tick_ms,
                impact_only: true,
            });
        }
    }
}

#[cfg(test)]
mod tests;
