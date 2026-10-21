//! A one-shot suicide blast. Terrain resources retain their IDs and depletion
//! follows the ordinary saved/replicated resource state.
use super::*;

impl World {
    pub(super) fn start_demolition(
        &mut self,
        index: usize,
        id: AbilityId,
        radius: u32,
        resources: &[String],
    ) {
        let actor = self.state.entities[index].clone();
        for node in &mut self.state.resources {
            if resources.contains(&node.kind)
                && rts::distance(node.position, actor.position) <= i64::from(radius).pow(2)
            {
                node.amount = 0;
            }
        }
        self.state.ability_fields.push(AbilityField {
            velocity: [0, 0],
            ability: id,
            position: actor.position,
            remaining: 1,
            owner: actor.owner,
            source: Some(actor.id),
        });
        self.state.entities[index].hp = 0;
    }

    pub(super) fn advance_demolitions(&self, damage: &mut rts::Damage) {
        for field in &self.state.ability_fields {
            let Some(AbilityEffect::Demolish {
                radius,
                damage: amount,
                ..
            }) = self.effect_definition(field.ability)
            else {
                continue;
            };
            for target in &self.state.entities {
                let unit = self.unit_type(target.unit_type).unwrap();
                let [left, top, right, bottom] = unit.footprint.bounds(target.position);
                let nearest = Position {
                    x: i64::from(field.position.x).clamp(left, right - 1) as i32,
                    y: i64::from(field.position.y).clamp(top, bottom - 1) as i32,
                };
                if target.hp > 0
                    && target.garrisoned_in.is_none()
                    && unit.movement_class != crate::map::MovementClass::Air
                    && rts::distance(nearest, field.position) <= i64::from(*radius).pow(2)
                {
                    self.record_effect_damage(
                        damage,
                        target,
                        (field.source.unwrap_or(target.id), field.owner),
                        amount * 256,
                        true,
                        true,
                    );
                }
            }
        }
    }
}
