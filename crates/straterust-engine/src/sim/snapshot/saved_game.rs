//! Rebase local progress onto installed definitions. Metadata describes stable
//! content and indexed mission/AI programs; it is never executed as old rules.
use super::*;
#[cfg(test)]
mod tests;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SaveDefinitions {
    pub rules_id: String,
    pub map_id: String,
    pub width: i32,
    pub height: i32,
    pub mission: Option<Mission>,
    pub ai: Vec<AiController>,
    pub energy_max: BTreeMap<UnitTypeId, u32>,
}

impl SaveDefinitions {
    pub fn of(world: &World) -> Self {
        Self {
            rules_id: world.rules.id.clone(),
            map_id: world.map.id.clone(),
            width: world.map.width,
            height: world.map.height,
            mission: world.map.mission.clone(),
            ai: world.map.ai.clone(),
            energy_max: world
                .rules
                .units
                .iter()
                .map(|u| (u.id, u.energy_max()))
                .collect(),
        }
    }
}

impl World {
    pub(crate) fn restore_saved_snapshot(
        &self,
        snapshot: WorldSnapshot,
        old: Option<SaveDefinitions>,
    ) -> Result<Self> {
        ensure!(
            !self.is_player_view(),
            "saved games require server definitions"
        );
        ensure!(
            snapshot.version == SNAPSHOT_VERSION,
            "unsupported saved state version {}",
            snapshot.version
        );
        snapshot.verify_checksum()?;
        if snapshot.identity == GameplayIdentity::of(self) {
            return self.restore_snapshot(snapshot);
        }
        ensure!(
            snapshot.identity.players == self.map.players,
            "saved map player count changed"
        );
        ensure!(
            snapshot.identity.tick_ms == self.rules.tick_ms,
            "saved game tick duration changed; a timing migration is required"
        );
        if let Some(old) = &old {
            ensure!(
                old.rules_id == self.rules.id && old.map_id == self.map.id,
                "save belongs to a different ruleset or map"
            );
            ensure!(
                old.width == self.map.width && old.height == self.map.height,
                "saved map dimensions changed"
            );
        }
        let mut state = snapshot.state;
        self.migrate_saved_mission(&mut state, old.as_ref())?;
        if let Some(old) = &old {
            ensure!(
                state.ai.len() == old.ai.len(),
                "invalid saved AI state count"
            );
            state.ai =
                self.map
                    .ai
                    .iter()
                    .map(|controller| {
                        let previous = old.ai.iter().position(|a| {
                            a.player == controller.player && a.home == controller.home
                        });
                        if let Some(index) = previous {
                            let mut ai = state.ai[index].clone();
                            if old.ai[index].program != controller.program {
                                ai.instruction = 0;
                                ai.wake = state.tick;
                                ai.requests.clear();
                                ai.attack.clear();
                                ai.prepared = false;
                            }
                            ai
                        } else {
                            AiState::new(controller)
                        }
                    })
                    .collect();
        }
        for entity in &mut state.entities {
            let unit = self.unit_type(entity.unit_type).ok_or_else(|| {
                anyhow::anyhow!(
                    "updated package is missing saved unit type {}",
                    entity.unit_type.0
                )
            })?;
            // Balance changes may lower maxima. Preserve absolute damage and
            // energy up to the new limits, rather than rejecting the save.
            entity.hp = entity.hp.min(unit.max_hp);
            entity.shields = entity.shields.min(unit.max_shields * 256);
            let bonus: u32 = self
                .rules
                .research
                .iter()
                .filter(|r| {
                    state
                        .players
                        .get(usize::from(entity.owner.0))
                        .is_some_and(|p| p.completed_research.contains(&r.id))
                })
                .filter_map(|r| match &r.effect {
                    ResearchEffect::EnergyCapacity { units, amount }
                        if units.contains(&entity.unit_type) =>
                    {
                        Some(*amount)
                    }
                    _ => None,
                })
                .sum();
            if entity.energy == 0
                && old
                    .as_ref()
                    .is_some_and(|o| o.energy_max.get(&entity.unit_type) == Some(&0))
            {
                entity.energy = unit
                    .energy_pool
                    .as_ref()
                    .map_or(0, |pool| pool.initial * 256);
            }
            entity.energy = entity.energy.min((unit.energy_max() + bonus) * 256);
            if let Some(job) = &entity.research {
                ensure!(
                    self.research(job.id).is_some(),
                    "updated package is missing saved research {}",
                    job.id.0
                );
            }
            // Revalidate saved routes against current terrain and footprints.
            // Keep valid routes; discarding every path makes a busy save run
            // hundreds of unnecessary searches on its first resumed tick.
            entity.path_geometry = [0; 32];
            entity.route_wait = None;
        }
        abilities::initialize_spawn_links(&self.map, &mut state);
        self.validate_snapshot_state(&state)?;
        let mut restored = self.snapshot();
        restored.state = state;
        restored.weapon_feedback.clear();
        Ok(restored)
    }

    fn migrate_saved_mission(
        &self,
        state: &mut State,
        old: Option<&SaveDefinitions>,
    ) -> Result<()> {
        let (Some(progress), Some(current)) = (&mut state.mission, &self.map.mission) else {
            ensure!(
                state.mission.is_none() && self.map.mission.is_none(),
                "saved mission definition is missing"
            );
            return Ok(());
        };
        let previous = old.and_then(|o| o.mission.as_ref());
        let mapping = if let Some(previous) = previous {
            ensure!(
                progress.triggers.len() == previous.triggers.len(),
                "invalid saved trigger count"
            );
            match_triggers(previous, current)?
        } else {
            // v1 saves predate definition metadata. Their known importer update
            // prepended only elapsed-zero research grants; old trigger indices
            // otherwise stayed unchanged. Never guess at arbitrary insertions.
            let added = current
                .triggers
                .len()
                .checked_sub(progress.triggers.len())
                .ok_or_else(|| anyhow::anyhow!("legacy save mission triggers were removed"))?;
            ensure!(
                current.triggers[..added].iter().all(initial_research),
                "legacy save needs a mission trigger migration"
            );
            (0..progress.triggers.len())
                .map(|i| Some(i + added))
                .collect()
        };
        let mut triggers = vec![MissionTriggerState::default(); current.triggers.len()];
        let mut matched = BTreeSet::new();
        for (index, saved) in progress.triggers.iter().enumerate() {
            let Some(next) = mapping[index] else {
                ensure!(
                    !saved.started || saved.complete,
                    "active saved mission trigger {} was removed",
                    index + 1
                );
                continue;
            };
            let mut saved = saved.clone();
            if saved.complete {
                saved.action = current.triggers[next].actions.len() as u16;
            } else if saved.started
                && let Some(previous) = previous
            {
                let before = &previous.triggers[index].actions;
                let after = &current.triggers[next].actions;
                if before != after && usize::from(saved.action) < before.len() {
                    let action = &before[usize::from(saved.action)];
                    let positions: Vec<_> = after
                        .iter()
                        .enumerate()
                        .filter(|(_, a)| *a == action)
                        .map(|(i, _)| i)
                        .collect();
                    ensure!(
                        positions.len() == 1,
                        "active saved mission action {} needs migration",
                        index + 1
                    );
                    saved.action = positions[0] as u16;
                }
            }
            ensure!(
                usize::from(saved.action) <= current.triggers[next].actions.len(),
                "invalid saved mission action"
            );
            triggers[next] = saved;
            matched.insert(next);
        }
        if let Some(wait) = &mut progress.wait {
            wait.trigger = mapping
                .get(usize::from(wait.trigger))
                .and_then(|i| *i)
                .ok_or_else(|| anyhow::anyhow!("waiting saved mission trigger was removed"))?
                as u16;
        }
        // Newly defined starting research must be available immediately, without
        // replaying any old initialization actions (resources, spawns, etc.).
        for (index, trigger) in current.triggers.iter().enumerate() {
            if initial_research(trigger) {
                for action in &trigger.actions {
                    let already_defined = previous.is_some_and(|m| {
                        m.triggers
                            .iter()
                            .filter(|t| initial_research(t))
                            .any(|t| t.actions.contains(action))
                    });
                    if matched.contains(&index) && (previous.is_none() || already_defined) {
                        continue;
                    }
                    if let MissionAction::GrantResearch { player, research } = action {
                        let player = state
                            .players
                            .get_mut(usize::from(player.0))
                            .ok_or_else(|| anyhow::anyhow!("invalid saved research player"))?;
                        player.completed_research.insert(*research);
                    }
                }
                if !matched.contains(&index) {
                    triggers[index] = MissionTriggerState {
                        action: trigger.actions.len() as u16,
                        started: true,
                        complete: true,
                        preserve: false,
                    };
                }
            }
        }
        progress.triggers = triggers;
        ensure!(
            progress.locations.len() <= current.locations.len(),
            "saved mission locations were removed"
        );
        for (index, location) in progress.locations.iter_mut().enumerate() {
            if previous.is_some_and(|p| p.locations.get(index) == Some(location)) {
                *location = current.locations[index];
            } else if previous.is_none() {
                // Legacy saves may have dynamically moved locations. Preserve
                // those bounds while updating the source elevation-mask fix.
                location.excluded_elevations = current.locations[index].excluded_elevations;
            }
        }
        progress
            .locations
            .extend_from_slice(&current.locations[progress.locations.len()..]);
        Ok(())
    }
}

fn initial_research(trigger: &MissionTrigger) -> bool {
    trigger.conditions
        == [MissionCondition::Elapsed {
            comparison: MissionComparison::AtLeast,
            milliseconds: 0,
        }]
        && !trigger.actions.is_empty()
        && trigger
            .actions
            .iter()
            .all(|a| matches!(a, MissionAction::GrantResearch { .. }))
}

fn match_triggers(previous: &Mission, current: &Mission) -> Result<Vec<Option<usize>>> {
    let mut used = BTreeSet::new();
    let mut mapping = vec![None; previous.triggers.len()];
    // Exact matches first, including duplicate triggers in their original order.
    for (index, trigger) in previous.triggers.iter().enumerate() {
        if let Some(next) = current
            .triggers
            .iter()
            .enumerate()
            .find(|(i, t)| !used.contains(i) && *t == trigger)
            .map(|(i, _)| i)
        {
            used.insert(next);
            mapping[index] = Some(next);
        }
    }
    // A corrected action may keep its condition. Only migrate unambiguous
    // matches; an unknown active trigger must not silently restart or vanish.
    for (index, trigger) in previous.triggers.iter().enumerate() {
        if mapping[index].is_some() {
            continue;
        }
        let candidates: Vec<_> = current
            .triggers
            .iter()
            .enumerate()
            .filter(|(i, t)| {
                !used.contains(i)
                    && (t.conditions == trigger.conditions || t.actions == trigger.actions)
                    && initial_research(t) == initial_research(trigger)
            })
            .map(|(i, _)| i)
            .collect();
        ensure!(
            candidates.len() <= 1,
            "saved mission trigger {} has ambiguous updated matches",
            index + 1
        );
        if candidates.len() == 1 {
            used.insert(candidates[0]);
            mapping[index] = Some(candidates[0]);
        }
    }
    Ok(mapping)
}
