//! Finite stored production and individually simulated launched fighters.
use super::*;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum StoredWeapon {
    Fighters {
        unit: UnitTypeId,
        launch_ticks: u32,
        leash: u32,
        repair: u32,
        /// Expire after this many ticks and consume the fighter on its first hit.
        #[serde(default)]
        expendable: Option<u32>,
    },
}

impl World {
    /// Expendable fighters free their ammunition slot when launched. Reusable
    /// fighters occupy a slot while deployed as well as while docked.
    pub fn stored_production_count(&self, producer: &Entity) -> usize {
        let expendable = self.unit_type(producer.unit_type).is_some_and(|unit| {
            matches!(
                unit.stored_weapon,
                Some(StoredWeapon::Fighters {
                    expendable: Some(_),
                    ..
                })
            )
        });
        self.state
            .entities
            .iter()
            .filter(|child| {
                child.hp > 0
                    && child.parent == Some(producer.id)
                    && (!expendable || child.garrisoned_in.is_some())
            })
            .count()
    }

    pub(in crate::sim) fn initialize_stored_production(&mut self) {
        let spawns: Vec<_> = self
            .map
            .spawns
            .iter()
            .enumerate()
            .filter(|(_, s)| s.stored_units > 0)
            .map(|(i, s)| (i, s.stored_units))
            .collect();
        for (index, count) in spawns {
            let parent = self.state.entities[index].clone();
            let unit = self.unit_at(index).trains[0];
            for _ in 0..count {
                if let Some(id) =
                    self.spawn_offspring(parent.owner, unit, parent.position, Some(parent.id))
                {
                    let child = self.index(id).unwrap();
                    self.state.entities[child].garrisoned_in = Some(parent.id);
                }
            }
        }
    }

    pub(in crate::sim) fn use_stored_weapon(&mut self, index: usize, target: usize) -> bool {
        let Some(config) = self.unit_at(index).stored_weapon.clone() else {
            return true;
        };
        let actor = self.state.entities[index].clone();
        match config {
            StoredWeapon::Fighters {
                unit,
                launch_ticks,
                expendable,
                ..
            } => {
                let target = self.state.entities[target].id;
                let children: Vec<_> = self
                    .state
                    .entities
                    .iter()
                    .filter(|e| e.hp > 0 && e.parent == Some(actor.id) && e.unit_type == unit)
                    .map(|e| e.id)
                    .collect();
                if children.is_empty() {
                    self.state.entities[index].cooldown = 1;
                    return false;
                }
                if expendable.is_some()
                    && children.iter().any(|id| {
                        self.state.entities[self.index(*id).unwrap()]
                            .garrisoned_in
                            .is_none()
                    })
                {
                    self.state.entities[index].cooldown = 1;
                    return false;
                }
                let mut launched = false;
                for id in children {
                    let i = self.index(id).unwrap();
                    if self.state.entities[i].garrisoned_in.is_some() {
                        if launched
                            || self.state.entities[i].shields < self.unit_at(i).max_shields * 256
                        {
                            continue;
                        }
                        let position = if self.unit_at(i).movement_class == MovementClass::Ground {
                            let footprint = self.unit_at(i).footprint;
                            let reference =
                                self.state.entities[self.index(target).unwrap()].position;
                            let Some(point) = rts::perimeter(
                                actor.position,
                                self.unit_at(index).footprint,
                                footprint,
                                reference,
                            )
                            .into_iter()
                            .find(|p| {
                                self.can_place(*p, footprint, MovementClass::Ground, Some(id))
                            }) else {
                                continue;
                            };
                            point
                        } else {
                            actor.position
                        };
                        self.state.entities[i].garrisoned_in = None;
                        self.state.entities[i].position = position;
                        if let Some(lifetime) = expendable {
                            self.state.entities[i].lifetime_remaining = Some(lifetime);
                            self.state.entities[i].invincible = true;
                        }
                        launched = true;
                    }
                    if self.state.entities[i].order != (UnitOrder::Attack { target }) {
                        self.assign(i, UnitOrder::Attack { target }, false);
                    }
                }
                self.state.entities[index].cooldown = launch_ticks;
                self.state.entities[index].last_attack_target = Some(target);
                false // Fighters themselves inflict damage, never the carrier proxy.
            }
        }
    }
    pub(in crate::sim) fn advance_fighter(&mut self, index: usize) -> bool {
        let child = self.state.entities[index].clone();
        let Some(parent) = child.parent.and_then(|id| self.index(id)) else {
            return false;
        };
        let Some(StoredWeapon::Fighters {
            unit,
            leash,
            repair,
            expendable,
            ..
        }) = self.unit_at(parent).stored_weapon
        else {
            return false;
        };
        if child.unit_type != unit {
            return false;
        }
        if self.disabled(&child) {
            return true;
        }
        let provider = self.state.entities[parent].clone();
        if provider.hp == 0 {
            self.state.entities[index].hp = 0;
            return true;
        }
        if child.garrisoned_in.is_some() {
            let max_hp = self.unit_at(index).max_hp;
            let shields = self.unit_at(index).max_shields * 256;
            self.state.entities[index].hp =
                child.hp.saturating_add(repair.div_ceil(256)).min(max_hp);
            self.state.entities[index].shields = child.shields.saturating_add(repair).min(shields);
            return true;
        }
        if expendable.is_some() {
            if matches!(child.order, UnitOrder::Idle) {
                self.state.entities[index].hp = 0;
                return true;
            }
            return false;
        }
        let engaging = matches!(
            provider.order,
            UnitOrder::Attack { .. }
                | UnitOrder::AttackMove { .. }
                | UnitOrder::Idle
                | UnitOrder::Hold
                | UnitOrder::Patrol { .. }
        ) && provider.last_attack_target.is_some_and(|id| {
            self.index(id).is_some_and(|i| {
                let target = &self.state.entities[i];
                target.hp > 0
                    && self.can_attack_entity(&provider, target)
                    && rts::distance(target.position, provider.position) <= i64::from(leash).pow(2)
            })
        });
        if engaging && child.shields > 0 {
            return false;
        }
        if rts::distance(child.position, provider.position) <= 32 * 32 {
            self.assign(index, UnitOrder::Idle, false);
            self.state.entities[index].garrisoned_in = Some(provider.id);
            self.state.entities[index].position = provider.position;
        } else {
            self.navigate(index, provider.position, false);
        }
        true
    }
    pub(in crate::sim) fn finish_fighter_shot(&mut self, index: usize) {
        if self.state.entities[index]
            .parent
            .and_then(|p| self.index(p))
            .is_some_and(|p| {
                matches!(
                    self.unit_at(p).stored_weapon,
                    Some(StoredWeapon::Fighters {
                        expendable: Some(_),
                        ..
                    })
                )
            })
        {
            self.state.entities[index].hp = 0;
        }
    }
}

pub(super) fn validate(rules: &Rules) -> Result<()> {
    for unit in &rules.units {
        ensure!(
            unit.production_capacity <= 64,
            "invalid production capacity"
        );
        if let Some(config) = &unit.stored_weapon {
            let child = match config {
                StoredWeapon::Fighters {
                    unit,
                    launch_ticks,
                    leash,
                    repair,
                    expendable,
                } => {
                    ensure!(
                        (1..=10000).contains(launch_ticks)
                            && (1..=32768).contains(leash)
                            && *repair <= 25600
                            && expendable.is_none_or(|n| (1..=100000).contains(&n)),
                        "invalid fighter configuration"
                    );
                    *unit
                }
            };
            ensure!(
                unit.production_capacity > 0
                    && unit.trains.contains(&child)
                    && unit.weapon.is_some(),
                "invalid stored weapon"
            );
        }
    }
    Ok(())
}
pub(super) fn put_rules(bytes: &mut Vec<u8>, rules: &Rules) {
    for unit in &rules.units {
        if let Some(config) = &unit.stored_weapon {
            bytes.extend(b"stored-weapon-v1");
            bytes.extend(unit.id.0.to_le_bytes());
            put_string(
                bytes,
                &ron::ser::to_string(config).expect("serializable stored weapon"),
            );
        }
    }
}
