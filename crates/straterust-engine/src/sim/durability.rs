//! Configurable shield health and completed-building power coverage.
use super::*;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PowerField {
    pub cell_size: u16,
    /// Symmetric quadrants, nearest row first; each bit enables one column.
    pub rows: Vec<u16>,
}
impl PowerField {
    fn contains(&self, source: Position, target: Position) -> bool {
        let coordinate =
            |value: i64| (value.abs() - i64::from(value < 0)) / i64::from(self.cell_size);
        let x = coordinate(i64::from(target.x) - i64::from(source.x));
        let y = coordinate(i64::from(target.y) - i64::from(source.y));
        x < 16
            && self
                .rows
                .get(y as usize)
                .is_some_and(|row| row & (1 << x) != 0)
    }
}
impl World {
    pub fn powered(&self, entity: &Entity) -> bool {
        if let Some(powered) = self.appearance(entity.id).and_then(|a| a.powered) {
            return powered;
        }
        self.powered_position(
            entity.owner,
            self.unit_type(entity.unit_type).unwrap(),
            entity.position,
        )
    }
    pub(in crate::sim) fn powered_position(
        &self,
        player: PlayerId,
        unit: &UnitType,
        position: Position,
    ) -> bool {
        !unit.requires_power
            || self.state.entities.iter().any(|source| {
                source.owner == player
                    && source.hp > 0
                    && source.construction.is_none()
                    && self
                        .unit_type(source.unit_type)
                        .unwrap()
                        .power_field
                        .as_ref()
                        .is_some_and(|field| field.contains(source.position, position))
            })
    }
    pub(in crate::sim) fn record_hit(
        &self,
        damage: &mut rts::Damage,
        (source, weapon_type): (EntityId, UnitTypeId),
        target: &Entity,
        weapon: &Weapon,
        divisor: u64,
    ) {
        if damage.weapon_feedback.len() + self.weapon_feedback.len() < 4096
            && let Some(feedback) = self.weapon_feedback(source, weapon_type, target, true)
        {
            damage.weapon_feedback.push(feedback);
        }
        if target.invincible {
            return;
        }
        let spent = damage.shields.entry(target.id).or_default();
        let remaining = u64::from(target.shields).saturating_sub(*spent);
        let raw = u64::from(weapon.damage) * 256 / divisor;
        *damage
            .incoming
            .entry(target.id)
            .or_default()
            .entry(source)
            .or_default() += raw;
        let absorbed = remaining.min(raw);
        *spent += absorbed;
        let hp = if raw > absorbed {
            rts::scaled_damage(
                raw - absorbed,
                weapon.damage_kind,
                self.unit_type(target.unit_type).unwrap(),
                self.research_armor_bonus(target.owner, target.unit_type),
            )
        } else {
            0
        };
        *damage
            .hits
            .entry(target.id)
            .or_default()
            .entry(source)
            .or_default() += hp;
    }

    fn weapon_feedback(
        &self,
        source: EntityId,
        weapon: UnitTypeId,
        target: &Entity,
        impact: bool,
    ) -> Option<(Vec<PlayerId>, WeaponFeedback)> {
        // Decide disclosure before deaths change visibility. The client never
        // receives an unseen source ID, origin or direction.
        let observers: Vec<_> = (0..self.map.players)
            .map(PlayerId)
            .filter(|&player| {
                !self.entity_visible(player, source)
                    && self.visibility(player, target.position) == Visibility::Visible
            })
            .collect();
        (!observers.is_empty()).then(|| {
            (
                observers,
                WeaponFeedback {
                    weapon,
                    position: target.position,
                    targets_air: self.movement_class(target) == MovementClass::Air,
                    impact,
                },
            )
        })
    }

    pub(in crate::sim) fn record_attack_feedback(
        &mut self,
        (source, weapon): (EntityId, UnitTypeId),
        target: &Entity,
    ) {
        if self.weapon_feedback.len() < 4096
            && let Some(feedback) = self.weapon_feedback(source, weapon, target, false)
        {
            self.weapon_feedback.push(feedback);
        }
    }
}

impl World {
    pub(in crate::sim) fn advance_carried_items(&mut self) {
        let items: Vec<_> = self
            .state
            .entities
            .iter()
            .filter(|e| self.unit_type(e.unit_type).unwrap().portable)
            .map(|e| e.id)
            .collect();
        for id in items {
            let Some(index) = self.index(id) else {
                continue;
            };
            let item = self.state.entities[index].clone();
            let carrier = item
                .carried_by
                .and_then(|id| self.index(id))
                .filter(|index| self.state.entities[*index].hp > 0)
                .or_else(|| {
                    self.state
                        .entities
                        .iter()
                        .enumerate()
                        .find(|(_, e)| {
                            e.hp > 0
                                && e.garrisoned_in.is_none()
                                && self.unit_type(e.unit_type).unwrap().worker.is_some()
                                && rts::distance(item.position, e.position) <= 32 * 32
                                && !self
                                    .state
                                    .entities
                                    .iter()
                                    .any(|other| other.carried_by == Some(e.id))
                        })
                        .map(|(index, _)| index)
                });
            if let Some(carrier) = carrier {
                let actor = self.state.entities[carrier].clone();
                let position = actor
                    .garrisoned_in
                    .and_then(|id| self.index(id))
                    .map_or(actor.position, |index| self.state.entities[index].position);
                let item = &mut self.state.entities[index];
                item.carried_by = Some(actor.id);
                item.owner = actor.owner;
                item.position = position;
            } else {
                self.state.entities[index].carried_by = None;
            }
        }
    }
}
