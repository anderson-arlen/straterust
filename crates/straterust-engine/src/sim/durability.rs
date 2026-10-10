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
        self.record_weapon_hit(damage, (source, weapon_type), target, weapon, divisor, None);
    }

    pub(in crate::sim) fn record_weapon_hit(
        &self,
        damage: &mut rts::Damage,
        (source, weapon_type): (EntityId, UnitTypeId),
        target: &Entity,
        weapon: &Weapon,
        divisor: u64,
        shot_bonuses: Option<(u32, u32)>,
    ) {
        if damage.weapon_feedback.len() + self.weapon_feedback.len() < 4096
            && let Some(feedback) = self.weapon_feedback(source, weapon_type, target, true)
        {
            damage.weapon_feedback.push(feedback);
        }
        if target.invincible
            || self.effect_invulnerable(target)
            || self
                .state
                .entities
                .iter()
                .any(|e| e.id == source && e.illusion_remaining.is_some())
        {
            return;
        }
        if weapon.range > 32 && weapon.splash.is_none() && self.movement_class(target) == MovementClass::Ground
            && self.state.ability_fields.iter().any(|f| matches!(self.effect_definition(f.ability), Some(AbilityEffect::Protection { radius, .. }) if rts::distance(f.position, target.position) <= i64::from(*radius).pow(2))) {
            return;
        }
        let multiplier = shot_bonuses.map_or_else(
            || {
                self.state
                    .entities
                    .iter()
                    .find(|e| e.id == source)
                    .map_or(100, |e| self.buff_percent(e, false))
            },
            |(percent, _)| percent,
        );
        let mut raw = u64::from(weapon.damage) * u64::from(multiplier) / 100
            * 256
            * if target.illusion_remaining.is_some() {
                2
            } else {
                1
            }
            / divisor;
        let mut kind = weapon.damage_kind;
        if let DamageKind::Split {
            piercing,
            minimum_percent,
        } = kind
        {
            let armor = self.unit_type(target.unit_type).unwrap().armor
                + self.research_armor_bonus(target.owner, target.unit_type);
            let bonus = shot_bonuses.map_or_else(
                || {
                    self.state
                        .entities
                        .iter()
                        .find(|e| e.id == source)
                        .map_or(0, |e| {
                            self.research_weapon_bonus(
                                e.owner,
                                weapon_type,
                                self.movement_class(target) == MovementClass::Air,
                            )
                        })
                },
                |(_, bonus)| bonus,
            );
            let piercing =
                u64::from(piercing + bonus) * u64::from(multiplier) / 100 * 256 / divisor;
            raw = raw
                .saturating_sub(piercing)
                .saturating_sub(u64::from(armor) * 256)
                + piercing;
            // A stable per-shot sample avoids changing the random stream for
            // other weapons. Tick/source/target distinguish simultaneous hits.
            let mut seed = self.tick().0 ^ (source.0 as u64 * 0x9e3779b9) ^ target.id.0 as u64;
            let percent = u64::from(minimum_percent)
                + splitmix64(&mut seed) % (101 - u64::from(minimum_percent));
            raw = (raw * percent / 100).max(256);
            kind = DamageKind::Normal;
        }
        raw = self.absorb_barriers(damage, target, raw);
        let spent = damage.shields.entry(target.id).or_default();
        let remaining = u64::from(target.shields).saturating_sub(*spent);
        *damage
            .incoming
            .entry(target.id)
            .or_default()
            .entry(source)
            .or_default() += raw;
        let raw = if remaining > 0 && raw > 0 {
            raw.saturating_sub(
                u64::from(self.research_bonus(target.owner, target.unit_type, 12)) * 256,
            )
            .max(128)
        } else {
            raw
        };
        let absorbed = remaining.min(raw);
        *spent += absorbed;
        let hp = if raw > absorbed {
            if matches!(weapon.damage_kind, DamageKind::Split { .. }) {
                (raw - absorbed).max(256)
            } else {
                rts::scaled_damage(
                    raw - absorbed,
                    kind,
                    self.unit_type(target.unit_type).unwrap(),
                    self.research_armor_bonus(target.owner, target.unit_type),
                )
            }
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

    pub(in crate::sim) fn absorb_barriers(
        &self,
        damage: &mut rts::Damage,
        target: &Entity,
        mut raw: u64,
    ) -> u64 {
        for aura in &target.ability_auras {
            if matches!(
                self.effect_definition(aura.ability),
                Some(AbilityEffect::Barrier { .. })
            ) {
                let spent = damage
                    .barriers
                    .entry((target.id, aura.ability))
                    .or_default();
                let absorbed = u64::from(aura.strength).saturating_sub(*spent).min(raw);
                *spent += absorbed;
                raw -= absorbed;
            }
        }
        raw
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
                let visibility = if self.movement_class(target) == MovementClass::Air {
                    self.terrain_visibility(player, target.position)
                } else {
                    self.visibility(player, target.position)
                };
                !self.entity_visible(player, source) && visibility == Visibility::Visible
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
