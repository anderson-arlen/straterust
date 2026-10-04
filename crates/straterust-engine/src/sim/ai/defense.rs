//! Damage calls for help independently of the victim's ability to retaliate.
use super::*;

impl World {
    pub(in crate::sim) fn ai_help_on_damage(
        &mut self,
        index: usize,
        sources: &BTreeMap<EntityId, u64>,
    ) {
        let victim = &self.state.entities[index];
        if victim.invincible {
            return;
        }
        let Some(town) = self.map.ai.iter().position(|c| c.player == victim.owner) else {
            return;
        };
        let Some((&attacker, &amount)) = sources
            .iter()
            .filter(|(_, amount)| **amount > 0)
            .filter(|(id, _)| {
                self.index(**id).is_some_and(|other| {
                    let enemy = &self.state.entities[other];
                    enemy.hp > 0 && self.is_enemy(victim.owner, enemy.owner)
                })
            })
            .max_by_key(|(id, amount)| (**amount, std::cmp::Reverse(**id)))
        else {
            return;
        };
        let enemy = self.state.entities[self.index(attacker).unwrap()].clone();
        let owner = victim.owner;
        let position = victim.position;
        // Buildings summon a wider defense, as in the source hit routine.
        let radius = if self.unit_at(index).structure {
            512_i64
        } else {
            256
        };
        let helpers: Vec<_> = self
            .state
            .entities
            .iter()
            .filter(|e| {
                e.owner == owner
                    && e.id != victim.id
                    && e.hp > 0
                    && e.construction.is_none()
                    && e.garrisoned_in.is_none()
                    && rts::distance(position, e.position) <= radius.pow(2)
                    && matches!(
                        e.order,
                        UnitOrder::Idle | UnitOrder::AttackMove { .. } | UnitOrder::Patrol { .. }
                    )
                    && self
                        .unit_type(e.unit_type)
                        .is_some_and(|u| u.worker.is_none() && u.weapon.is_some())
                    && self.can_target_entity(e, &enemy)
                    && !self.undetected(owner, &enemy)
            })
            .map(|e| e.id)
            .collect();
        let hit = BTreeMap::from([(attacker, amount)]);
        for id in helpers {
            let helper = self.index(id).unwrap();
            let deployed = self.state.ai.iter().any(|s| s.deployed.contains(&id));
            if !deployed && !self.unit_at(helper).structure {
                let home = *self.state.ai[town]
                    .guards
                    .entry(id)
                    .or_insert(self.state.entities[helper].position);
                let range = self
                    .weapon_for(&self.state.entities[helper], &enemy)
                    .unwrap()
                    .range;
                if rts::distance(home, enemy.position) > i64::from(range + 192).pow(2) {
                    continue;
                }
            }
            if self.movement_locked(&self.state.entities[helper]) {
                self.assign(
                    helper,
                    UnitOrder::AttackMove {
                        target: enemy.position,
                    },
                    false,
                );
            }
            self.react_to_damage(helper, &hit);
        }
    }

    pub(super) fn ai_defend(&mut self, controller: &AiController, state: &mut AiState) {
        let guards: Vec<_> = self
            .state
            .entities
            .iter()
            .filter(|e| {
                e.owner == controller.player
                    && e.hp > 0
                    && rts::distance(e.position, controller.home)
                        <= i64::from(controller.radius).pow(2)
                    && e.construction.is_none()
                    && !state.deployed.contains(&e.id)
                    && !self.state.ai.iter().any(|s| s.deployed.contains(&e.id))
                    && e.garrisoned_in.is_none()
                    && self
                        .unit_type(e.unit_type)
                        .is_some_and(|u| u.weapon.is_some() && u.worker.is_none() && !u.structure)
            })
            .map(|e| {
                (
                    e.id,
                    e.position,
                    e.order.clone(),
                    e.auto_attack_target,
                    self.movement_locked(e),
                )
            })
            .collect();
        for (entity, position, order, target, movement_locked) in guards {
            let home = *state.guards.entry(entity).or_insert(position);
            if target.is_none()
                && !movement_locked
                && matches!(order, UnitOrder::Idle)
                && rts::distance(position, home) > 32_i64.pow(2)
            {
                self.ai_order(
                    controller,
                    state,
                    Order::AttackMove {
                        entity,
                        target: home,
                    },
                );
            }
        }
    }
}
