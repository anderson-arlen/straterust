//! Bounded, deterministic hazards configured by game content.
use super::*;

impl World {
    pub(super) fn start_ground_effect(
        &mut self,
        index: usize,
        id: AbilityId,
        point: Position,
    ) -> bool {
        let AbilityEffect::GroundEffect {
            duration,
            travel_speed,
            repeat,
            offsets,
            ..
        } = self.effect_definition(id).unwrap().clone()
        else {
            unreachable!()
        };
        let actor = &self.state.entities[index];
        let origin = if travel_speed > 0 {
            actor.position
        } else {
            point
        };
        let dx = i64::from(point.x - origin.x);
        let dy = i64::from(point.y - origin.y);
        let length = ((dx * dx + dy * dy) as u64).isqrt().max(1) as i64;
        let velocity = [
            (dx * i64::from(travel_speed) / length) as i16,
            (dy * i64::from(travel_speed) / length) as i16,
        ];
        for offset in offsets {
            let position = Position {
                x: origin.x + i32::from(offset[0]),
                y: origin.y + i32::from(offset[1]),
            };
            if self.map.contains(position) && self.state.ability_fields.len() < rts::MAX_ENTITIES {
                self.state.ability_fields.push(AbilityField {
                    ability: id,
                    position,
                    velocity,
                    remaining: duration,
                    owner: actor.owner,
                    source: Some(actor.id),
                });
            }
        }
        repeat
    }

    pub(super) fn advance_ground_effects(&mut self, damage: &mut rts::Damage) {
        for index in 0..self.state.ability_fields.len() {
            let mut field = self.state.ability_fields[index].clone();
            let Some(AbilityEffect::GroundEffect {
                radius,
                damage_fp8,
                period,
                duration,
                drift,
                travel_speed,
                trigger_on_contact,
                ..
            }) = self.effect_definition(field.ability).cloned()
            else {
                continue;
            };
            let elapsed = duration.saturating_sub(field.remaining);
            if drift > 0 && elapsed.is_multiple_of(32) {
                // Seeded by the field itself: no wall clock or cosmetic randomness.
                let seed = field.source.map_or(0, |s| s.0).wrapping_mul(2654435761)
                    ^ (self.tick().0 / 32) as u32;
                let directions = [
                    [1, 0],
                    [1, 1],
                    [0, 1],
                    [-1, 1],
                    [-1, 0],
                    [-1, -1],
                    [0, -1],
                    [1, -1],
                ];
                let d = directions[seed as usize % directions.len()];
                field.velocity = [d[0] * drift as i16, d[1] * drift as i16];
            }
            let next = Position {
                x: field.position.x + i32::from(field.velocity[0]),
                y: field.position.y + i32::from(field.velocity[1]),
            };
            if self.map.contains(next) {
                field.position = next;
            } else {
                field.remaining = 0;
            }
            if field.remaining > 0 && elapsed.is_multiple_of(period) {
                let steps = if elapsed == 0 { 1 } else { period };
                let start = Position {
                    x: field.position.x - i32::from(field.velocity[0]) * steps as i32,
                    y: field.position.y - i32::from(field.velocity[1]) * steps as i32,
                };
                let targets: Vec<_> = self
                    .state
                    .entities
                    .iter()
                    .filter(|e| {
                        e.hp > 0
                            && e.garrisoned_in.is_none()
                            && !e.invincible
                            && !self.effect_invulnerable(e)
                            && (travel_speed == 0 || Some(e.id) != field.source)
                            && (!trigger_on_contact
                                || self.unit_type(e.unit_type).is_some_and(|u| {
                                    !u.structure && u.movement_class == MovementClass::Ground
                                }))
                            && rts::in_range(
                                nearest_on_segment(start, field.position, e.position),
                                crate::map::Footprint::default(),
                                e.position,
                                self.unit_type(e.unit_type).unwrap().footprint,
                                radius,
                            )
                    })
                    .cloned()
                    .collect();
                for target in &targets {
                    self.record_effect_damage(
                        damage,
                        target,
                        (field.source.unwrap_or(target.id), field.owner),
                        damage_fp8,
                        true,
                        false,
                    );
                }
                if trigger_on_contact && !targets.is_empty() {
                    field.remaining = 0;
                }
            }
            self.state.ability_fields[index] = field;
        }
    }
}

fn nearest_on_segment(start: Position, end: Position, point: Position) -> Position {
    let dx = i64::from(end.x) - i64::from(start.x);
    let dy = i64::from(end.y) - i64::from(start.y);
    let length = dx * dx + dy * dy;
    if length == 0 {
        return end;
    }
    let along = ((i64::from(point.x) - i64::from(start.x)) * dx
        + (i64::from(point.y) - i64::from(start.y)) * dy)
        .clamp(0, length);
    Position {
        x: start.x + (i128::from(dx) * i128::from(along) / i128::from(length)) as i32,
        y: start.y + (i128::from(dy) * i128::from(along) / i128::from(length)) as i32,
    }
}
