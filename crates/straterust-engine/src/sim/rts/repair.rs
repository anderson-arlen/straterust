//! Repair assignments stay at their target between bouts of damage.
use super::*;

impl World {
    pub(in crate::sim) fn repair(&mut self, index: usize, target: EntityId) {
        let actor = &self.state.entities[index];
        if self.repair_rejection(actor.id, target).is_some() {
            self.finish(index);
            return;
        }
        let owner = actor.owner;
        let other = self.index(target).expect("validated repair target");
        let position = self.state.entities[other].position;
        let footprint = self.unit_at(other).footprint;
        let repair = self
            .rules
            .repair
            .as_ref()
            .expect("validated repair rules")
            .clone();
        if !self.approach(index, position, footprint, repair.range) {
            return;
        }
        if self.state.entities[other].construction.is_some() {
            let rate = self
                .unit_at(index)
                .worker
                .as_ref()
                .map_or(1, |w| w.build_rate);
            self.state.entities[index].repair_progress += u64::from(rate);
            if self.progress_construction(other, rate) {
                self.repair_complete(index);
            }
            return;
        }
        if self.state.entities[other].hp == self.unit_at(other).max_hp
            && self.state.entities[other].damage_fraction == 0
        {
            self.repair_complete(index);
            return;
        }
        let unit = self.unit_at(other).clone();
        let time = u64::from(unit.build_ticks) * u64::from(repair.rate_denominator);
        let progress = self.state.entities[index].repair_progress
            + u64::from(unit.max_hp) * u64::from(repair.rate_numerator);
        let missing = unit.max_hp - self.state.entities[other].hp
            + u32::from(self.state.entities[other].damage_fraction != 0);
        let healed = (progress / time).min(u64::from(missing));
        let denominator = u64::from(unit.max_hp) * u64::from(repair.cost_divisor);
        // Check the next HP's price even during fractional work. An unfunded
        // order waits without accumulating work or emitting working effects.
        let prices: Vec<_> = unit
            .cost
            .iter()
            .enumerate()
            .map(|(i, cost)| {
                let credit = self.state.entities[other]
                    .repair_credit
                    .get(i)
                    .copied()
                    .unwrap_or(0);
                (healed.max(1) * u64::from(cost.amount))
                    .saturating_sub(credit)
                    .div_ceil(denominator)
            })
            .collect();
        if unit
            .cost
            .iter()
            .zip(&prices)
            .any(|(cost, price)| self.resource_balance(owner, &cost.kind) < *price)
        {
            if self.state.entities[index]
                .queued_orders
                .iter()
                .any(|order| matches!(order, UnitOrder::Gather { .. }))
            {
                self.maintain(index);
            }
            return;
        }
        self.state.entities[index].repair_progress = progress % time;
        if healed == 0 {
            return;
        }
        self.state.entities[other]
            .repair_credit
            .resize(unit.cost.len(), 0);
        for (i, (cost, price)) in unit.cost.iter().zip(prices).enumerate() {
            if price != 0 {
                *self.state.players[usize::from(owner.0)]
                    .resources
                    .entry(cost.kind.clone())
                    .or_default() -= price;
            }
            self.state.entities[other].repair_credit[i] += price * denominator;
            self.state.entities[other].repair_credit[i] -= healed * u64::from(cost.amount);
        }
        self.state.entities[other].hp =
            (self.state.entities[other].hp + healed as u32).min(unit.max_hp);
        if self.state.entities[other].hp == unit.max_hp {
            self.state.entities[other].damage_fraction = 0;
            self.repair_complete(index);
        }
    }

    fn repair_complete(&mut self, index: usize) {
        self.state.entities[index].repair_progress = 0;
        if !self.maintain(index) {
            self.finish(index);
        }
    }

    /// Repair targets can share one background gather order. Other orders keep
    /// ordinary finite queue semantics. Switch only outside resource interiors.
    pub(in crate::sim) fn maintain(&mut self, index: usize) -> bool {
        let actor = &self.state.entities[index];
        let gathering = matches!(actor.order, UnitOrder::Gather { .. });
        if gathering && (actor.gathering_inside || actor.queued_orders.is_empty()) {
            return false;
        }
        let mut gathers = usize::from(gathering);
        if self.state.entities[index]
            .queued_orders
            .iter()
            .any(|order| match order {
                UnitOrder::Repair { .. } => false,
                UnitOrder::Gather { .. } => {
                    gathers += 1;
                    gathers > 1
                }
                _ => true,
            })
        {
            return false;
        }
        let worker = self.state.entities[index].id;
        let mut next = None;
        let mut background = None;
        let mut offset = 0;
        while offset < self.state.entities[index].queued_orders.len() {
            let UnitOrder::Repair { target } = self.state.entities[index].queued_orders[offset]
            else {
                background = Some(offset);
                offset += 1;
                continue;
            };
            if self.repair_rejection(worker, target).is_some() {
                self.state.entities[index].queued_orders.remove(offset);
                continue;
            }
            let other = self.index(target).expect("validated maintenance target");
            let entity = &self.state.entities[other];
            if next.is_none()
                && (entity.construction.is_some()
                    || entity.hp < self.unit_at(other).max_hp
                    || entity.damage_fraction != 0)
                && (gathers == 0 || self.maintenance_funded(index, other))
            {
                next = Some(offset);
            }
            offset += 1;
        }
        if let Some(next) = next.or(background) {
            let actor = &mut self.state.entities[index];
            let order = actor.queued_orders.remove(next).unwrap();
            if !actor.queued_orders.contains(&actor.order) {
                actor.queued_orders.push_back(actor.order.clone());
            }
            let origin = actor.gather_origin;
            self.assign(index, order, false);
            if origin.is_some() {
                self.state.entities[index].gather_origin = origin;
            }
        }
        true
    }

    fn maintenance_funded(&self, index: usize, other: usize) -> bool {
        let target = &self.state.entities[other];
        if target.construction.is_some() {
            return true;
        }
        let actor = &self.state.entities[index];
        let unit = self.unit_at(other);
        let repair = self.rules.repair.as_ref().expect("repair rules");
        let progress = if actor.order == (UnitOrder::Repair { target: target.id }) {
            actor.repair_progress
        } else {
            0
        };
        let healed = ((progress + u64::from(unit.max_hp) * u64::from(repair.rate_numerator))
            / (u64::from(unit.build_ticks) * u64::from(repair.rate_denominator)))
        .min(u64::from(unit.max_hp - target.hp) + u64::from(target.damage_fraction != 0))
        .max(1);
        let denominator = u64::from(unit.max_hp) * u64::from(repair.cost_divisor);
        unit.cost.iter().enumerate().all(|(i, cost)| {
            let credit = target.repair_credit.get(i).copied().unwrap_or(0);
            let price = (healed * u64::from(cost.amount))
                .saturating_sub(credit)
                .div_ceil(denominator);
            self.resource_balance(actor.owner, &cost.kind) >= price
        })
    }
}
