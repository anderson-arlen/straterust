use super::*;

impl World {
    pub(in crate::sim) fn advance_rts(&mut self) {
        if self.state.winner.is_some()
            || self
                .state
                .mission
                .as_ref()
                .is_some_and(|mission| mission.paused)
        {
            return;
        }
        self.advance_research();
        self.advance_offspring();
        self.refresh_navigation_geometry();
        let ids: Vec<_> = self.state.entities.iter().map(|entity| entity.id).collect();
        for entity in &mut self.state.entities {
            entity.cooldown = entity.cooldown.saturating_sub(1);
            entity.unload_remaining = entity.unload_remaining.saturating_sub(1);
            for strike in &mut entity.strikes {
                strike.remaining = strike.remaining.saturating_sub(1);
            }
        }
        let mut damage = Damage::default();
        for id in &ids {
            let Some(index) = self.index(*id) else {
                continue;
            };
            if self.state.entities[index].construction.is_some() {
                if (self.unit_at(index).consumes_builder
                    || self.unit_at(index).autonomous_construction)
                    && self.state.entities[index]
                        .construction
                        .as_ref()
                        .is_some_and(|c| c.worker.is_none() && c.work_position.is_some())
                {
                    self.progress_construction(index, 1);
                }
                continue;
            }
            if self.unit_at(index).revealer
                || self.state.entities[index].doodad_enabled == Some(false)
            {
                continue;
            }
            if self.advance_flight(index) {
                continue;
            }
            if self.unit_at(index).mine.is_some() {
                self.advance_mine(index, &mut damage);
                continue;
            }
            let shields = self.unit_at(index).max_shields * 256;
            self.state.entities[index].shields = (self.state.entities[index].shields
                + u32::from(self.unit_at(index).shield_regeneration))
            .min(shields);
            if !self.powered(&self.state.entities[index]) {
                continue;
            }
            let regeneration = u64::from(self.unit_at(index).regeneration);
            if regeneration != 0 {
                let max_hp = self.unit_at(index).max_hp;
                let entity = &mut self.state.entities[index];
                let health = (u64::from(entity.hp) * 256 - u64::from(entity.damage_fraction)
                    + regeneration)
                    .min(u64::from(max_hp) * 256);
                entity.hp = health.div_ceil(256) as u32;
                entity.damage_fraction = (u64::from(entity.hp) * 256 - health) as u8;
            }
            if self.state.entities[index].cloaked
                && self
                    .unit_at(index)
                    .cloak
                    .as_ref()
                    .is_some_and(|c| c.auto_reveal)
                && matches!(self.state.entities[index].order, UnitOrder::Idle)
                && self.automatic_target(index).is_some()
            {
                self.reveal(index);
                continue;
            }
            if self.state.entities[index].cloak_transition != 0 {
                self.state.entities[index].cloak_transition -= 1;
                continue;
            }
            if self.movement_locked(&self.state.entities[index])
                && self.attacks_locked(&self.state.entities[index])
            {
                continue;
            }
            if self.state.entities[index].garrisoned_in.is_some() {
                self.advance_garrison_attack(index, &mut damage);
                continue;
            }
            self.advance_strikes(index, &mut damage);
            let order = self.state.entities[index].order.clone();
            let combat_target = match order {
                UnitOrder::Attack { target } => self.index(target),
                UnitOrder::Hold => self.acquire(index),
                UnitOrder::Idle | UnitOrder::AttackMove { .. } | UnitOrder::Patrol { .. } => {
                    self.automatic_target(index)
                }
                _ => None,
            };
            if let Some(target) = combat_target {
                if self.attack(index, target, &mut damage) {
                    continue;
                }
            } else if matches!(order, UnitOrder::Attack { .. }) {
                self.finish(index);
                continue;
            }
            match order {
                UnitOrder::PlaceAddon { unit_type, target } => {
                    let actor = &self.state.entities[index];
                    let command = Command {
                        tick: self.state.tick,
                        player: actor.owner,
                        sequence: 0,
                        order: Order::Build {
                            entity: actor.id,
                            unit_type,
                            position: target,
                        },
                    };
                    if self.apply(&command).is_some() {
                        self.finish(index);
                    }
                }
                UnitOrder::PlaceMine { target } => self.advance_place_mine(index, target),
                UnitOrder::Land { target } => self.advance_land(index, target),
                UnitOrder::Load { target } => self.advance_load(index, target),
                UnitOrder::Pickup { target } => self.advance_pickup(index, target),
                UnitOrder::UnloadAt { target } => self.advance_unload(index, target),
                UnitOrder::Move { target } | UnitOrder::AttackMove { target } => {
                    if self.navigate(index, target, true) {
                        self.finish(index);
                    }
                }
                UnitOrder::Patrol { target } => {
                    let goal = if self.state.entities[index].patrol_returning {
                        self.state.entities[index].patrol_origin.unwrap_or(target)
                    } else {
                        target
                    };
                    if self.navigate(index, goal, true) {
                        self.state.entities[index].patrol_returning =
                            !self.state.entities[index].patrol_returning;
                    }
                }
                UnitOrder::Gather { resource } => self.gather(index, resource),
                UnitOrder::Build { building } => self.construct(index, building),
                UnitOrder::Repair { target } => self.repair(index, target),
                UnitOrder::Idle | UnitOrder::Hold | UnitOrder::Attack { .. } => {}
            }
        }
        self.sync_passenger_positions();
        for (id, amount) in &damage.shields {
            if let Some(index) = self.index(*id)
                && !self.state.entities[index].invincible
            {
                self.state.entities[index].shields = self.state.entities[index]
                    .shields
                    .saturating_sub(*amount as u32);
            }
        }
        // Every attack above observes pre-damage HP. Mutual lethal attacks land.
        for (id, sources) in &damage.hits {
            if let Some(index) = self.index(*id) {
                let entity = &mut self.state.entities[index];
                if entity.invincible {
                    continue;
                }
                let health = (u64::from(entity.hp) * 256 - u64::from(entity.damage_fraction))
                    .saturating_sub(sources.values().sum());
                entity.hp = health.div_ceil(256) as u32;
                entity.damage_fraction = (u64::from(entity.hp) * 256 - health) as u8;
            }
        }
        self.weapon_feedback
            .extend(std::mem::take(&mut damage.weapon_feedback));
        self.weapon_feedback.truncate(4096);
        // Attribute kills only after every hit has landed.
        for (victim, sources) in &damage.hits {
            if let Some(index) = self.index(*victim)
                && self.state.entities[index].hp == 0
            {
                let unit_type = self.state.entities[index].unit_type;
                let owner = sources
                    .iter()
                    .max_by_key(|(id, amount)| (**amount, std::cmp::Reverse(**id)))
                    .and_then(|(id, _)| self.index(*id))
                    .map(|source| self.state.entities[source].owner);
                if let Some(owner) = owner {
                    let structure = self.unit_type(unit_type).unwrap().structure;
                    let statistics = &mut self.state.statistics[usize::from(owner.0)];
                    let count = if structure {
                        &mut statistics.structures_razed
                    } else {
                        &mut statistics.units_killed
                    };
                    *count = count.saturating_add(1);
                    let count = self
                        .state
                        .kills
                        .entry(owner)
                        .or_default()
                        .entry(unit_type)
                        .or_default();
                    *count = count.saturating_add(1);
                }
            }
        }
        // Damage reactions cannot recruit dead attackers or override move/hold/work orders.
        for (id, sources) in &damage.incoming {
            if let Some(index) = self.index(*id) {
                self.ai_help_on_damage(index, sources);
                self.react_to_damage(index, sources);
            }
        }
        let dead_garrisons: Vec<_> = self
            .state
            .entities
            .iter()
            .filter(|entity| {
                entity.hp == 0
                    && self
                        .unit_type(entity.unit_type)
                        .expect("validated type")
                        .garrison
                        .is_some()
            })
            .map(|entity| entity.id)
            .collect();
        for id in dead_garrisons {
            if let Some(index) = self.index(id) {
                self.unload_garrison(index, true);
            }
        }
        self.advance_carried_items();
        self.record_deaths(|e| e.hp == 0);
        self.record_losses(|e| e.hp == 0);
        self.state.entities.retain(|entity| entity.hp > 0);
        self.clear_dead_references();
        for id in ids {
            if let Some(index) = self.index(id) {
                self.produce(index);
            }
        }
        self.update_victory();
    }
    pub(in crate::sim) fn clear_dead_references(&mut self) {
        let alive: BTreeSet<_> = self.state.entities.iter().map(|entity| entity.id).collect();
        for entity in &mut self.state.entities {
            if entity
                .auto_attack_target
                .is_some_and(|id| !alive.contains(&id))
            {
                entity.auto_attack_target = None;
                entity.retaliation_position = None;
            }
            if let Some(progress) = &mut entity.construction
                && progress.worker.is_some_and(|id| !alive.contains(&id))
            {
                progress.worker = None;
                progress.work_position = None;
                progress.work_ticks = 0;
            }
        }
        let invalid: Vec<_> = self.state.entities.iter().filter(|entity| matches!(entity.order, UnitOrder::Build { building } if !alive.contains(&building)) || matches!(entity.order, UnitOrder::Attack { target } | UnitOrder::Repair { target } | UnitOrder::Load { target } | UnitOrder::Pickup { target } if !alive.contains(&target))).map(|entity| entity.id).collect();
        for id in invalid {
            if let Some(index) = self.index(id) {
                self.finish(index);
            }
        }
    }
    pub(in crate::sim) fn update_victory(&mut self) {
        if !self.rules.victory || self.map.mission.is_some() {
            return;
        }
        let participants: BTreeSet<_> = self.map.spawns.iter().map(|spawn| spawn.owner).collect();
        if participants.len() < 2 {
            return;
        }
        let alive: BTreeSet<_> = self
            .state
            .entities
            .iter()
            .map(|entity| entity.owner)
            .collect();
        self.state.defeated = participants.difference(&alive).copied().collect();
        let survivors: Vec<_> = participants.intersection(&alive).copied().collect();
        if survivors.len() == 1 {
            self.state.winner = survivors.first().copied();
        }
    }
}
