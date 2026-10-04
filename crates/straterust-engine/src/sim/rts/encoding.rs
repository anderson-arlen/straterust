use super::*;

pub(in crate::sim) fn put_footprint(bytes: &mut Vec<u8>, footprint: Footprint) {
    bytes.extend(footprint.width.to_le_bytes());
    bytes.extend(footprint.height.to_le_bytes());
}
pub(in crate::sim) fn put_amounts(bytes: &mut Vec<u8>, amounts: &[ResourceAmount]) {
    bytes.extend((amounts.len() as u32).to_le_bytes());
    for amount in amounts {
        put_string(bytes, &amount.kind);
        bytes.extend(amount.amount.to_le_bytes());
    }
}
pub(in crate::sim) fn put_optional_position(bytes: &mut Vec<u8>, position: Option<Position>) {
    bytes.push(u8::from(position.is_some()));
    if let Some(position) = position {
        put_position(bytes, position);
    }
}
pub(in crate::sim) fn put_order(bytes: &mut Vec<u8>, order: &UnitOrder) {
    match *order {
        UnitOrder::UnloadAt { target } => {
            bytes.push(14);
            put_position(bytes, target);
        }
        UnitOrder::Pickup { target } => {
            bytes.push(13);
            bytes.extend(target.0.to_le_bytes());
        }
        UnitOrder::PlaceAddon { unit_type, target } => {
            bytes.push(12);
            bytes.extend(unit_type.0.to_le_bytes());
            put_position(bytes, target);
        }
        UnitOrder::PlaceMine { target } => {
            bytes.push(11);
            put_position(bytes, target);
        }
        UnitOrder::Land { target } => {
            bytes.push(10);
            put_position(bytes, target);
        }
        UnitOrder::Load { target } => {
            bytes.push(9);
            bytes.extend(target.0.to_le_bytes());
        }
        UnitOrder::Idle => bytes.push(0),
        UnitOrder::Move { target } => {
            bytes.push(1);
            put_position(bytes, target);
        }
        UnitOrder::Attack { target } => {
            bytes.push(2);
            bytes.extend(target.0.to_le_bytes());
        }
        UnitOrder::AttackMove { target } => {
            bytes.push(3);
            put_position(bytes, target);
        }
        UnitOrder::Hold => bytes.push(4),
        UnitOrder::Patrol { target } => {
            bytes.push(5);
            put_position(bytes, target);
        }
        UnitOrder::Gather { resource } => {
            bytes.push(6);
            bytes.extend(resource.0.to_le_bytes());
        }
        UnitOrder::Build { building } => {
            bytes.push(7);
            bytes.extend(building.0.to_le_bytes());
        }
        UnitOrder::Repair { target } => {
            bytes.push(8);
            bytes.extend(target.0.to_le_bytes());
        }
    }
}
pub(in crate::sim) fn put_rts_state(bytes: &mut Vec<u8>, state: &State) {
    bytes.extend((state.players.len() as u32).to_le_bytes());
    for player in &state.players {
        bytes.extend((player.resources.len() as u32).to_le_bytes());
        for (kind, amount) in &player.resources {
            put_string(bytes, kind);
            bytes.extend(amount.to_le_bytes());
        }
    }
    bytes.extend((state.resources.len() as u32).to_le_bytes());
    for resource in &state.resources {
        bytes.extend(resource.id.0.to_le_bytes());
        put_string(bytes, &resource.kind);
        put_position(bytes, resource.position);
        put_footprint(bytes, resource.footprint);
        bytes.extend(resource.amount.to_le_bytes());
        bytes.push(u8::from(resource.requires_extractor));
    }
    bytes.push(u8::from(state.winner.is_some()));
    if let Some(player) = state.winner {
        bytes.extend(player.0.to_le_bytes());
    }
    bytes.extend((state.defeated.len() as u32).to_le_bytes());
    for player in &state.defeated {
        bytes.extend(player.0.to_le_bytes());
    }
    for entity in &state.entities {
        bytes.extend(entity.hp.to_le_bytes());
        bytes.extend(entity.unload_remaining.to_le_bytes());
        bytes.push(entity.damage_fraction);
        bytes.push(u8::from(entity.auto_attack_target.is_some()));
        if let Some(target) = entity.auto_attack_target {
            bytes.extend(target.0.to_le_bytes());
        }
        bytes.push(u8::from(entity.retaliation_position.is_some()));
        if let Some(position) = entity.retaliation_position {
            put_position(bytes, position);
        }
        bytes.push(u8::from(entity.invincible));
        bytes.push(match entity.doodad_enabled {
            None => 0,
            Some(false) => 1,
            Some(true) => 2,
        });
        bytes.push(u8::from(entity.gathering_inside));
        bytes.extend(entity.energy.to_le_bytes());
        bytes.push(u8::from(entity.cloaked));
        bytes.push(u8::from(entity.last_attack_air));
        put_optional_position(bytes, entity.last_attack_position);
        bytes.push(u8::from(entity.last_attack_target.is_some()));
        if let Some(target) = entity.last_attack_target {
            bytes.extend(target.0.to_le_bytes());
        }
        bytes.extend(entity.cloak_transition.to_le_bytes());
        bytes.push(u8::from(entity.airborne));
        bytes.extend(entity.flight_transition.to_le_bytes());
        bytes.push(entity.mine_count);
        bytes.push(u8::from(entity.mine_state.is_some()));
        if let Some(mine) = &entity.mine_state {
            bytes.push(match mine.phase {
                MinePhase::Arming => 0,
                MinePhase::Concealing => 1,
                MinePhase::Armed => 2,
                MinePhase::Emerging => 3,
                MinePhase::Chasing => 4,
            });
            bytes.extend(mine.remaining.to_le_bytes());
            bytes.push(u8::from(mine.target.is_some()));
            if let Some(target) = mine.target {
                bytes.extend(target.0.to_le_bytes());
            }
        }
        bytes.push(u8::from(entity.garrisoned_in.is_some()));
        if let Some(parent) = entity.garrisoned_in {
            bytes.extend(parent.0.to_le_bytes());
        }
        bytes.push(u8::from(entity.parent.is_some()));
        if let Some(parent) = entity.parent {
            bytes.extend(parent.0.to_le_bytes());
        }
        bytes.extend((entity.strikes.len() as u32).to_le_bytes());
        for strike in &entity.strikes {
            bytes.extend(strike.remaining.to_le_bytes());
            bytes.extend(strike.target.0.to_le_bytes());
            put_position(bytes, strike.aim);
            bytes.extend(strike.forward.to_le_bytes());
            bytes.push(u8::from(strike.air));
        }
        bytes.push(u8::from(entity.construction.is_some()));
        if let Some(progress) = &entity.construction {
            bytes.push(u8::from(progress.worker.is_some()));
            if let Some(worker) = progress.worker {
                bytes.extend(worker.0.to_le_bytes());
            }
            bytes.extend(progress.remaining.to_le_bytes());
            bytes.extend(progress.total.to_le_bytes());
            put_optional_position(bytes, progress.work_position);
            bytes.extend(progress.work_ticks.to_le_bytes());
        }
        bytes.extend((entity.production.len() as u32).to_le_bytes());
        for job in &entity.production {
            bytes.extend(job.unit_type.0.to_le_bytes());
            bytes.extend(job.remaining.to_le_bytes());
            bytes.extend(job.total.to_le_bytes());
            bytes.push(u8::from(job.started));
        }
        bytes.push(u8::from(entity.cargo.is_some()));
        if let Some(cargo) = &entity.cargo {
            put_string(bytes, &cargo.kind);
            bytes.extend(cargo.amount.to_le_bytes());
        }
        bytes.push(u8::from(entity.dropoff_target.is_some()));
        if let Some(id) = entity.dropoff_target {
            bytes.extend(id.0.to_le_bytes());
        }
        put_optional_position(bytes, entity.rally);
        bytes.push(u8::from(entity.rally_resource.is_some()));
        if let Some(resource) = entity.rally_resource {
            bytes.extend(resource.0.to_le_bytes());
        }
        put_order(bytes, &entity.order);
        bytes.extend((entity.queued_orders.len() as u32).to_le_bytes());
        for order in &entity.queued_orders {
            put_order(bytes, order);
        }
        bytes.extend((entity.path.len() as u32).to_le_bytes());
        for point in &entity.path {
            put_position(bytes, *point);
        }
        bytes.extend(entity.path_retry.0.to_le_bytes());
        bytes.extend(entity.cooldown.to_le_bytes());
        bytes.extend(entity.harvest_progress.to_le_bytes());
        bytes.push(u8::from(entity.harvest_waiting_since.is_some()));
        if let Some(tick) = entity.harvest_waiting_since {
            bytes.extend(tick.0.to_le_bytes());
        }
        for fraction in entity.motion_fraction {
            bytes.extend(fraction.to_le_bytes());
        }
        bytes.extend(entity.motion_speed.to_le_bytes());
        bytes.extend(entity.motion_phase.to_le_bytes());
        bytes.extend(entity.repair_progress.to_le_bytes());
        bytes.extend((entity.repair_credit.len() as u32).to_le_bytes());
        for credit in &entity.repair_credit {
            bytes.extend(credit.to_le_bytes());
        }
        put_optional_position(bytes, entity.patrol_origin);
        bytes.push(u8::from(entity.patrol_returning));
    }
}
pub(in crate::sim) fn put_rts_rules(bytes: &mut Vec<u8>, rules: &Rules) {
    bytes.push(u8::from(rules.prioritize_threats));
    put_amounts(bytes, &rules.starting_resources);
    bytes.extend(rules.supply_limit.to_le_bytes());
    bytes.push(u8::from(rules.victory));
    bytes.push(u8::from(rules.repair.is_some()));
    if let Some(repair) = &rules.repair {
        bytes.extend(repair.rate_numerator.to_le_bytes());
        bytes.extend(repair.rate_denominator.to_le_bytes());
        bytes.extend(repair.cost_divisor.to_le_bytes());
        bytes.extend(repair.range.to_le_bytes());
    }
    for unit in &rules.units {
        bytes.push(unit.cargo_size);
        bytes.extend(unit.max_hp.to_le_bytes());
        bytes.push(u8::from(unit.motion.is_some()));
        if let Some(motion) = &unit.motion {
            bytes.extend(motion.speed.to_le_bytes());
            bytes.extend(motion.acceleration.to_le_bytes());
            bytes.extend((motion.steps.len() as u32).to_le_bytes());
            for step in &motion.steps {
                bytes.extend(step.to_le_bytes());
            }
        }
        bytes.extend(unit.armor.to_le_bytes());
        bytes.push(match unit.size {
            UnitSize::Small => 0,
            UnitSize::Medium => 1,
            UnitSize::Large => 2,
        });
        bytes.push(u8::from(unit.acquisition_range.is_some()));
        if let Some(range) = unit.acquisition_range {
            bytes.extend(range.to_le_bytes());
        }
        bytes.extend(unit.vision_range.to_le_bytes());
        bytes.extend(unit.detector_range.to_le_bytes());
        bytes.push(u8::from(unit.cloak.is_some()));
        if let Some(cloak) = &unit.cloak {
            cloak.put(bytes);
        }
        bytes.push(u8::from(unit.revealer));
        bytes.push(u8::from(unit.blocks_movement));
        bytes.push(u8::from(unit.phases_while_gathering));
        bytes.push(u8::from(unit.consumes_builder));
        bytes.push(u8::from(unit.attacks_ground));
        bytes.extend(unit.regeneration.to_le_bytes());
        bytes.push(u8::from(unit.extracts.is_some()));
        if let Some(extraction) = &unit.extracts {
            put_string(bytes, &extraction.resource);
            bytes.extend(extraction.harvest_ticks.to_le_bytes());
            bytes.extend(extraction.depleted_amount.to_le_bytes());
        }
        bytes.push(u8::from(unit.requires_creep));
        bytes.push(u8::from(unit.creep_radius.is_some()));
        if let Some(radius) = unit.creep_radius {
            for value in radius {
                bytes.extend(value.to_le_bytes());
            }
        }
        bytes.push(u8::from(unit.addon_parent.is_some()));
        if let Some(parent) = unit.addon_parent {
            bytes.extend(parent.0.to_le_bytes());
        }
        bytes.push(u8::from(unit.scanner.is_some()));
        if let Some(scanner) = &unit.scanner {
            scanner.put(bytes);
        }
        bytes.push(u8::from(unit.flight.is_some()));
        if let Some(flight) = &unit.flight {
            bytes.extend(flight.speed.to_le_bytes());
            bytes.extend(flight.lift_ticks.to_le_bytes());
            bytes.extend(flight.land_ticks.to_le_bytes());
        }
        bytes.push(u8::from(unit.triggers_mines));
        bytes.push(u8::from(unit.mine_layer.is_some()));
        if let Some(layer) = &unit.mine_layer {
            bytes.extend(layer.unit_type.0.to_le_bytes());
            bytes.push(layer.initial_count);
            bytes.extend(layer.deploy_range.to_le_bytes());
        }
        bytes.push(u8::from(unit.mine.is_some()));
        if let Some(mine) = &unit.mine {
            for value in [
                mine.arm_ticks,
                mine.conceal_ticks,
                mine.reveal_ticks,
                mine.trigger_range,
                mine.chase_range,
                mine.detonation_range,
            ] {
                bytes.extend(value.to_le_bytes());
            }
        }
        bytes.push(u8::from(unit.garrison.is_some()));
        if let Some(garrison) = &unit.garrison {
            bytes.push(garrison.capacity);
            bytes.extend(garrison.range_bonus.to_le_bytes());
            bytes.extend(garrison.unload_ticks.to_le_bytes());
            for units in [&garrison.passengers, &garrison.attackers] {
                bytes.extend((units.len() as u32).to_le_bytes());
                for id in units {
                    bytes.extend(id.0.to_le_bytes());
                }
            }
        }
        bytes.push(u8::from(unit.structure));
        put_footprint(bytes, unit.placement);
        bytes.extend(unit.resource_clearance.to_le_bytes());
        put_amounts(bytes, &unit.cost);
        bytes.extend(unit.build_ticks.to_le_bytes());
        bytes.extend(unit.supply_used.to_le_bytes());
        bytes.extend(unit.supply_provided.to_le_bytes());
        for list in [
            &unit.prerequisites,
            &unit.builds,
            &unit.trains,
            &unit.repairs,
        ] {
            bytes.extend((list.len() as u32).to_le_bytes());
            for id in list {
                bytes.extend(id.0.to_le_bytes());
            }
        }
        bytes.extend((unit.dropoff.len() as u32).to_le_bytes());
        for kind in &unit.dropoff {
            put_string(bytes, kind);
        }
        bytes.push(u8::from(unit.worker.is_some()));
        if let Some(worker) = &unit.worker {
            bytes.extend(worker.capacity.to_le_bytes());
            bytes.extend(worker.harvest_amount.to_le_bytes());
            bytes.extend(worker.harvest_ticks.to_le_bytes());
            bytes.extend(worker.build_rate.to_le_bytes());
            bytes.extend((worker.resource_kinds.len() as u32).to_le_bytes());
            for kind in &worker.resource_kinds {
                put_string(bytes, kind);
            }
        }
        put_weapon(bytes, &unit.weapon);
        put_weapon(bytes, &unit.air_weapon);
    }
}

fn put_weapon(bytes: &mut Vec<u8>, weapon: &Option<Weapon>) {
    bytes.push(u8::from(weapon.is_some()));
    if let Some(weapon) = weapon {
        bytes.push(u8::from(weapon.cooldown_jitter.is_some()));
        if let Some([low, high]) = weapon.cooldown_jitter {
            bytes.extend(low.to_le_bytes());
            bytes.extend(high.to_le_bytes());
        }
        bytes.push(u8::from(weapon.targets_air));
        bytes.push(match weapon.damage_kind {
            DamageKind::Normal => 0,
            DamageKind::Explosive => 1,
            DamageKind::Concussive => 2,
        });
        bytes.extend(weapon.damage.to_le_bytes());
        bytes.extend(weapon.range.to_le_bytes());
        bytes.extend(weapon.cooldown.to_le_bytes());
        bytes.push(u8::from(weapon.splash.is_some()));
        if let Some(radii) = weapon.splash {
            for radius in radii {
                bytes.extend(radius.to_le_bytes());
            }
        }
        bytes.extend((weapon.strikes.len() as u32).to_le_bytes());
        for strike in &weapon.strikes {
            bytes.extend(strike.delay.to_le_bytes());
            bytes.extend(strike.forward.to_le_bytes());
        }
    }
}
