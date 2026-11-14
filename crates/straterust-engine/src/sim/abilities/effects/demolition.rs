//! A one-shot suicide blast. Terrain resources retain their IDs and depletion
//! follows the ordinary saved/replicated resource state.
use super::*;

impl World {
    pub(in crate::sim) fn approach_demolition(
        &mut self,
        index: usize,
        target: AbilityTarget,
        position: Position,
        range: u32,
        radius: u32,
    ) -> bool {
        let footprint = match target {
            AbilityTarget::Unit(id) => self.unit_at(self.index(id).unwrap()).footprint,
            AbilityTarget::Point(_) => Footprint::default(),
        };
        let reached = |world: &Self, reach| {
            let actor = &world.state.entities[index];
            if matches!(target, AbilityTarget::Unit(_)) {
                rts::in_range(
                    actor.position,
                    world.unit_at(index).footprint,
                    position,
                    footprint,
                    reach,
                )
            } else {
                rts::distance(actor.position, position) <= i64::from(reach).pow(2)
            }
        };
        if reached(self, range) {
            return true;
        }
        // The click can be inside an impassable footprint or a forest cluster.
        // Reuse ordinary nearest-reachable movement, then detonate at that edge
        // if the requested target is within the blast. An unrelated island on
        // the other side of the map must not cause an explosion in place.
        self.navigate(index, position, true) && reached(self, radius.max(range))
    }

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
                self.collision.resource_changed(node.id);
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
