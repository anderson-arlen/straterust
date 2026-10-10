//! Bounded bunkers and mobile transports. Occupants remain authoritative entities, while
//! their container determines visibility and origin; no duplicate passenger list.
use super::rts::{Damage, MAX_QUEUED_ORDERS, attack_cooldown, distance, in_range, perimeter};
use super::*;
use anyhow::Context;

pub(super) fn default_cargo_size() -> u8 {
    1
}
fn default_boarding_range() -> u32 {
    1
}
fn default_boarding(value: &u32) -> bool {
    *value == 1
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GarrisonStats {
    /// Reach across terrain boundaries when boarding or finding passenger exits.
    #[serde(
        default = "default_boarding_range",
        skip_serializing_if = "default_boarding"
    )]
    pub boarding_range: u32,
    pub capacity: u8,
    pub passengers: Vec<UnitTypeId>,
    pub attackers: Vec<UnitTypeId>,
    pub range_bonus: u32,
    /// Simulation ticks between successful passenger exits; zero unloads immediately.
    #[serde(default)]
    pub unload_ticks: u32,
}

pub(super) fn validate_garrison_rules(rules: &Rules) -> Result<()> {
    for unit in &rules.units {
        ensure!(
            (1..=8).contains(&unit.cargo_size),
            "invalid transport cargo size"
        );
        let Some(garrison) = &unit.garrison else {
            continue;
        };
        ensure!(
            (unit.structure || (unit.speed > 0 && unit.movement_class != MovementClass::Ground))
                && (1..=16).contains(&garrison.capacity)
                && garrison.range_bonus <= 32768,
            "invalid garrison capacity or range"
        );
        ensure!(garrison.unload_ticks <= 10000, "invalid unload interval");
        ensure!(
            (1..=128).contains(&garrison.boarding_range),
            "invalid boarding range"
        );
        let mut allowed = BTreeSet::new();
        ensure!(
            !garrison.passengers.is_empty() && garrison.passengers.len() <= rules.units.len(),
            "invalid garrison passenger count"
        );
        for id in &garrison.passengers {
            let passenger = rules
                .units
                .iter()
                .find(|u| u.id == *id)
                .context("unknown garrison passenger")?;
            ensure!(
                allowed.insert(*id)
                    && !passenger.structure
                    && passenger.speed > 0
                    && passenger.movement_class == MovementClass::Ground
                    && !passenger.revealer,
                "invalid garrison passenger"
            );
        }
        let mut attackers = BTreeSet::new();
        for id in &garrison.attackers {
            ensure!(
                attackers.insert(*id)
                    && allowed.contains(id)
                    && rules
                        .units
                        .iter()
                        .any(|u| u.id == *id && u.weapon.is_some()),
                "invalid garrison attacker"
            );
        }
    }
    Ok(())
}

impl World {
    pub(super) fn request_pickup(&mut self, index: usize, passenger: EntityId) {
        if self.unit_at(index).structure
            || self.unit_at(index).garrison.is_none()
            || matches!(self.state.entities[index].order, UnitOrder::Pickup { .. })
        {
            return;
        }
        let previous = self.state.entities[index].order.clone();
        if previous != UnitOrder::Idle {
            self.state.entities[index]
                .queued_orders
                .push_front(previous);
        }
        self.assign(index, UnitOrder::Pickup { target: passenger }, false);
    }

    pub(super) fn advance_pickup(&mut self, index: usize, target: EntityId) {
        let container = self.state.entities[index].id;
        if self.load_rejection(target, container).is_some()
            || self.index(target).is_none_or(|passenger| {
                self.state.entities[passenger].order != (UnitOrder::Load { target: container })
            })
        {
            if let Some(passenger) = self
                .state
                .entities
                .iter()
                .filter(|passenger| {
                    passenger.order == (UnitOrder::Load { target: container })
                        && self.load_rejection(passenger.id, container).is_none()
                })
                .min_by_key(|passenger| {
                    (
                        distance(passenger.position, self.state.entities[index].position),
                        passenger.id,
                    )
                })
                .map(|passenger| passenger.id)
            {
                self.assign(index, UnitOrder::Pickup { target: passenger }, false);
                return;
            }
            self.finish(index);
            return;
        }
        let passenger = self.index(target).unwrap();
        if self.movement_class(&self.state.entities[index]) == MovementClass::Water {
            // A passenger inland has no water perimeter. Approach its nearest
            // reachable coastline; the passenger can then walk to boarding range.
            let position = self.state.entities[passenger].position;
            self.navigate(index, position, true);
            return;
        }
        self.approach(
            index,
            self.state.entities[passenger].position,
            self.unit_at(passenger).footprint,
            self.unit_at(index)
                .garrison
                .as_ref()
                .unwrap()
                .boarding_range,
        );
    }

    pub fn load_rejection(&self, passenger: EntityId, bunker: EntityId) -> Option<Rejection> {
        let Some(index) = self.index(passenger) else {
            return Some(Rejection::UnknownEntity);
        };
        let Some(target) = self.index(bunker) else {
            return Some(Rejection::InvalidTarget);
        };
        let actor = &self.state.entities[index];
        let container = &self.state.entities[target];
        if !self.transport_ready(container) {
            return Some(Rejection::MissingPrerequisite);
        }
        let Some(garrison) = &self.unit_at(target).garrison else {
            return Some(Rejection::UnsupportedOrder);
        };
        if !self.unit_at(target).structure
            && !matches!(container.order, UnitOrder::Idle | UnitOrder::Pickup { .. })
            && container.queued_orders.len() == MAX_QUEUED_ORDERS
        {
            return Some(Rejection::QueueFull);
        }
        if actor.owner != container.owner
            || actor.id == container.id
            || actor.hp == 0
            || container.hp == 0
            || actor.garrisoned_in.is_some()
            || self.movement_locked(actor)
            || self.inside_structure(actor)
            || actor.cloak_transition != 0
        {
            return Some(Rejection::InvalidTarget);
        }
        if actor.construction.is_some() || container.construction.is_some() {
            return Some(Rejection::Unfinished);
        }
        if !garrison.passengers.contains(&actor.unit_type) {
            return Some(Rejection::UnsupportedOrder);
        }
        let occupied: u32 = self
            .state
            .entities
            .iter()
            .filter(|u| u.garrisoned_in == Some(bunker))
            .map(|u| u32::from(self.unit_type(u.unit_type).unwrap().cargo_size))
            .sum();
        if occupied + u32::from(self.unit_at(index).cargo_size) > u32::from(garrison.capacity) {
            return Some(Rejection::QueueFull);
        }
        None
    }
    pub fn unload_rejection(&self, bunker: EntityId) -> Option<Rejection> {
        let Some(index) = self.index(bunker) else {
            return Some(Rejection::UnknownEntity);
        };
        let container = &self.state.entities[index];
        if self.unit_at(index).garrison.is_none() {
            return Some(Rejection::UnsupportedOrder);
        }
        if container.construction.is_some() {
            return Some(Rejection::Unfinished);
        }
        if !self
            .state
            .entities
            .iter()
            .any(|u| u.garrisoned_in == Some(bunker))
        {
            return Some(Rejection::InvalidTarget);
        }
        None
    }
    pub(super) fn advance_load(&mut self, index: usize, target: EntityId) {
        let id = self.state.entities[index].id;
        if self.load_rejection(id, target).is_some() {
            self.finish(index);
            return;
        }
        let container_index = self.index(target).expect("validated container");
        let position = self.state.entities[container_index].position;
        let range = self
            .unit_at(container_index)
            .garrison
            .as_ref()
            .unwrap()
            .boarding_range;
        if !self.approach(
            index,
            position,
            self.unit_at(container_index).footprint,
            range,
        ) {
            return;
        }
        // Revalidate after approach because earlier entities may fill this container.
        if self.load_rejection(id, target).is_some() {
            self.finish(index);
            return;
        }
        self.assign(index, UnitOrder::Idle, true);
        let actor = &mut self.state.entities[index];
        actor.position = position;
        actor.garrisoned_in = Some(target);
        if self.state.entities[container_index].order == (UnitOrder::Pickup { target: id }) {
            self.advance_pickup(container_index, id);
        }
    }
    pub fn unload_at_rejection(&self, container: EntityId, target: Position) -> Option<Rejection> {
        if let Some(reason) = self.unload_rejection(container) {
            return Some(reason);
        }
        let unit = self.unit_at(self.index(container).unwrap());
        if unit.structure || unit.speed == 0 {
            return Some(Rejection::UnsupportedOrder);
        }
        if !self.map.contains(target) {
            return Some(Rejection::OutOfBounds);
        }
        if self.unload_destination(container, target).is_none() {
            return Some(Rejection::InvalidPlacement);
        }
        // Occupancy can change while travelling; keep a valid intent and retry exits.
        None
    }
    /// Water transports resolve a shore click to a nearby navigable landing.
    /// Aircraft retain their exact destination. Only terrain is checked here:
    /// passengers retry if another unit temporarily occupies their exit.
    pub(super) fn unload_destination(
        &self,
        container: EntityId,
        target: Position,
    ) -> Option<Position> {
        let ship = &self.state.entities[self.index(container)?];
        let unit = self.unit_type(ship.unit_type)?;
        let valid = |position| self.unload_position_valid(container, position);
        if valid(target) {
            return Some(target);
        }
        if unit.movement_class != MovementClass::Water {
            return None;
        }
        let mut candidates: Vec<_> = (-128..=128)
            .step_by(8)
            .flat_map(|y| {
                (-128..=128).step_by(8).map(move |x| Position {
                    x: target.x + x,
                    y: target.y + y,
                })
            })
            .filter(|p| distance(*p, target) <= 128_i64.pow(2))
            .collect();
        candidates.sort_by_key(|p| (distance(*p, target), distance(*p, ship.position), p.y, p.x));
        candidates.into_iter().find(|p| valid(*p))
    }
    pub(super) fn unload_position_valid(&self, container: EntityId, position: Position) -> bool {
        let unit = self.unit_at(self.index(container).unwrap());
        self.map
            .can_move(position, unit.footprint, unit.movement_class)
            && !self
                .state
                .entities
                .iter()
                .filter(|passenger| passenger.garrisoned_in == Some(container))
                .any(|passenger| {
                    let passenger = self.unit_type(passenger.unit_type).unwrap();
                    !perimeter(
                        position,
                        self.boarding_footprint(container),
                        passenger.footprint,
                        position,
                    )
                    .into_iter()
                    .any(|exit| {
                        self.map
                            .can_move(exit, passenger.footprint, passenger.movement_class)
                    })
                })
    }
    pub(super) fn boarding_footprint(&self, container: EntityId) -> Footprint {
        let unit = self.unit_at(self.index(container).unwrap());
        let padding = (unit.garrison.as_ref().unwrap().boarding_range - 1) as u16 * 2;
        Footprint {
            width: unit.footprint.width.saturating_add(padding),
            height: unit.footprint.height.saturating_add(padding),
        }
    }
    pub(super) fn advance_unload(&mut self, index: usize, target: Position) {
        if self
            .unload_rejection(self.state.entities[index].id)
            .is_some()
        {
            self.finish(index);
            return;
        }
        let Some(target) = self.unload_destination(self.state.entities[index].id, target) else {
            self.finish(index);
            return;
        };
        self.state.entities[index].order = UnitOrder::UnloadAt { target };
        if self.navigate(index, target, true) {
            self.unload_garrison(index, false);
            if self
                .unload_rejection(self.state.entities[index].id)
                .is_some()
            {
                self.finish(index);
            }
        }
    }
    pub fn unload_passenger_rejection(
        &self,
        container: EntityId,
        passenger: EntityId,
    ) -> Option<Rejection> {
        if let Some(reason) = self.unload_rejection(container) {
            return Some(reason);
        }
        if self.state.entities[self.index(container).unwrap()].unload_remaining > 0 {
            return Some(Rejection::Cooldown);
        }
        if !self
            .state
            .entities
            .iter()
            .any(|u| u.id == passenger && u.garrisoned_in == Some(container) && u.hp > 0)
        {
            return Some(Rejection::InvalidTarget);
        }
        None
    }
    pub(super) fn unload_garrison(&mut self, index: usize, destroyed: bool) {
        let container = self.state.entities[index].id;
        let passengers: Vec<_> = self
            .state
            .entities
            .iter()
            .filter(|u| u.garrisoned_in == Some(container))
            .map(|u| u.id)
            .collect();
        for id in passengers {
            self.unload_passenger(index, id, destroyed);
        }
    }
    pub(super) fn unload_passenger(&mut self, index: usize, id: EntityId, destroyed: bool) {
        if !destroyed && self.state.entities[index].unload_remaining > 0 {
            return;
        }
        let container = self.state.entities[index].clone();
        let passenger = self.index(id).expect("validated passenger");
        if destroyed && self.movement_class(&container) != MovementClass::Ground {
            self.state.entities[passenger].garrisoned_in = None;
            self.state.entities[passenger].hp = 0;
            return;
        }
        let unit = self.unit_at(passenger);
        let exit = perimeter(
            container.position,
            self.boarding_footprint(container.id),
            unit.footprint,
            container.position,
        )
        .into_iter()
        .find(|p| self.can_place(*p, unit.footprint, unit.movement_class, Some(id)));
        if let Some(exit) = exit {
            self.assign(passenger, UnitOrder::Idle, true);
            self.state.entities[passenger].garrisoned_in = None;
            self.state.entities[passenger].position = exit;
            if !destroyed {
                self.state.entities[index].unload_remaining =
                    self.unit_at(index).garrison.as_ref().unwrap().unload_ticks;
            }
        } else if destroyed {
            self.state.entities[passenger].garrisoned_in = None;
            self.state.entities[passenger].hp = 0;
        }
    }
    pub(super) fn sync_passenger_positions(&mut self) {
        let positions: BTreeMap<_, _> = self
            .state
            .entities
            .iter()
            .map(|u| (u.id, u.position))
            .collect();
        for passenger in &mut self.state.entities {
            if let Some(position) = passenger.garrisoned_in.and_then(|id| positions.get(&id)) {
                passenger.position = *position;
            }
        }
    }
    /// The ordinary RTS loop decrements cooldown/strike timers before this hook.
    pub(super) fn advance_garrison_attack(&mut self, index: usize, damage: &mut Damage) {
        let Some(container_id) = self.state.entities[index].garrisoned_in else {
            return;
        };
        let Some(container_index) = self.index(container_id) else {
            return;
        };
        let container = &self.state.entities[container_index];
        let Some(garrison) = self.unit_at(container_index).garrison.as_ref() else {
            return;
        };
        if container.hp == 0
            || container.construction.is_some()
            || !garrison
                .attackers
                .contains(&self.state.entities[index].unit_type)
        {
            return;
        }
        let range_bonus = garrison.range_bonus;
        self.state.entities[index].position = container.position;
        self.advance_strikes(index, damage);
        let actor = self.state.entities[index].clone();
        let unit = self.unit_at(index);
        let Some(mut weapon) = unit.weapon.clone() else {
            return;
        };
        weapon.damage += self.research_damage_bonus(actor.owner, actor.unit_type);
        weapon.range += range_bonus + self.research_range_bonus(actor.owner, actor.unit_type);
        if actor.stim_remaining > 0 {
            weapon.cooldown = (weapon.cooldown / 2).max(5);
        }
        let target = self
            .state
            .entities
            .iter()
            .enumerate()
            .filter(|(_, other)| {
                self.is_enemy_entity(actor.owner, other)
                    && self.can_attack_entity(&actor, other)
                    && other.hp > 0
                    && other.garrisoned_in.is_none()
                    && !self.inside_structure(other)
            })
            .filter(|(other, entity)| {
                in_range(
                    actor.position,
                    unit.footprint,
                    entity.position,
                    self.unit_at(*other).footprint,
                    weapon.range,
                )
            })
            .min_by_key(|(_, other)| (distance(actor.position, other.position), other.id))
            .map(|(index, _)| index);
        let Some(target) = target else {
            self.state.entities[index].order = UnitOrder::Idle;
            return;
        };
        let enemy = self.state.entities[target].clone();
        self.state.entities[index].order = UnitOrder::Attack { target: enemy.id };
        if actor.cooldown != 0 {
            return;
        }
        self.record_attack_feedback((container_id, actor.unit_type), &enemy);
        self.state.entities[index].last_attack_air =
            self.movement_class(&enemy) == MovementClass::Air;
        self.state.entities[index].last_attack_target = Some(enemy.id);
        self.state.entities[index].last_attack_position = Some(enemy.position);
        if weapon.projectile_speed != 0 {
            self.launch_weapon_projectile(&actor, &enemy, &weapon);
        } else if weapon.strikes.is_empty() {
            self.record_hit(damage, (container_id, actor.unit_type), &enemy, &weapon, 1);
        } else {
            self.state.entities[index].strikes = weapon
                .strikes
                .iter()
                .map(|strike| PendingStrike {
                    remaining: strike.delay,
                    target: enemy.id,
                    aim: enemy.position,
                    forward: strike.forward,
                    air: self.movement_class(&enemy) == MovementClass::Air,
                })
                .collect();
            self.advance_strikes(index, damage);
        }
        self.state.entities[index].cooldown = attack_cooldown(&weapon, &mut self.state.rng_state);
    }
}

#[cfg(test)]
mod tests {
    mod pickup;
    mod unload;
    use super::*;
    fn world() -> World {
        let foot = Footprint {
            width: 4,
            height: 4,
        };
        let rules = Rules {
            id: "garrison".into(),
            units: vec![
                UnitType {
                    id: UnitTypeId(1),
                    speed: 8,
                    footprint: foot,
                    max_hp: 40,
                    weapon: Some(Weapon {
                        friendly_splash: false,
                        projectile_speed: 0,
                        cooldown_jitter: None,
                        targets_air: false,
                        target_classes: Vec::new(),
                        damage: 6,
                        range: 32,
                        cooldown: 3,
                        damage_kind: DamageKind::Normal,
                        splash: None,
                        strikes: Vec::new(),
                    }),
                    ..UnitType::default()
                },
                UnitType {
                    id: UnitTypeId(2),
                    speed: 8,
                    footprint: foot,
                    max_hp: 60,
                    ..UnitType::default()
                },
                UnitType {
                    id: UnitTypeId(3),
                    speed: 0,
                    structure: true,
                    footprint: Footprint {
                        width: 20,
                        height: 20,
                    },
                    placement: Footprint {
                        width: 20,
                        height: 20,
                    },
                    max_hp: 100,
                    garrison: Some(GarrisonStats {
                        boarding_range: 1,
                        capacity: 2,
                        passengers: vec![UnitTypeId(1), UnitTypeId(2)],
                        attackers: vec![UnitTypeId(1)],
                        range_bonus: 64,
                        unload_ticks: 0,
                    }),
                    ..UnitType::default()
                },
            ],
            ..Rules::default()
        };
        let map = Map {
            initial_explored: Default::default(),
            creation: Default::default(),
            ai: Vec::new(),
            id: "garrison".into(),
            width: 256,
            height: 256,
            players: 2,
            spawns: vec![
                Spawn {
                    unit_type: UnitTypeId(1),
                    position: Position { x: 50, y: 60 },
                    ..Spawn::default()
                },
                Spawn {
                    unit_type: UnitTypeId(2),
                    position: Position { x: 50, y: 70 },
                    ..Spawn::default()
                },
                Spawn {
                    unit_type: UnitTypeId(3),
                    position: Position { x: 64, y: 64 },
                    ..Spawn::default()
                },
                Spawn {
                    owner: PlayerId(1),
                    unit_type: UnitTypeId(1),
                    position: Position { x: 140, y: 64 },
                    ..Spawn::default()
                },
            ],
            start_locations: vec![],
            resources: vec![],
            mission: None,
            terrain: None,
            fog_of_war: false,
        };
        World::new(rules, map, 1).unwrap()
    }
    fn load(world: &mut World, index: usize) {
        for _ in 0..12 {
            world.advance_load(index, EntityId(3));
            if world.state.entities[index].garrisoned_in.is_some() {
                return;
            }
        }
        panic!("passenger did not reach container");
    }
    fn issue(world: &mut World, order: Order) -> Option<Rejection> {
        world
            .step(&[Command {
                tick: world.tick(),
                player: PlayerId(0),
                sequence: world.state().last_sequences[0] + 1,
                order,
            }])
            .unwrap()[0]
            .rejection
            .clone()
    }
    #[test]
    fn individual_unload_changes_only_the_named_passenger() {
        let mut w = world();
        load(&mut w, 0);
        load(&mut w, 1);
        assert_eq!(
            w.unload_passenger_rejection(EntityId(3), EntityId(4)),
            Some(Rejection::InvalidTarget)
        );
        assert_eq!(
            issue(
                &mut w,
                Order::UnloadPassenger {
                    entity: EntityId(3),
                    passenger: EntityId(1),
                }
            ),
            None
        );
        assert_eq!(w.state.entities[0].garrisoned_in, None);
        assert_eq!(w.state.entities[1].garrisoned_in, Some(EntityId(3)));
        assert_eq!(
            w.unload_passenger_rejection(EntityId(3), EntityId(1)),
            Some(Rejection::InvalidTarget)
        );
    }
    #[test]
    fn air_transport_counts_cargo_slots_and_keeps_passengers_at_its_position() {
        let old = world();
        let mut rules = old.rules().clone();
        rules.units[0].cargo_size = 2;
        rules.units[2].structure = false;
        rules.units[2].speed = 8;
        rules.units[2].movement_class = MovementClass::Air;
        rules.units[2].garrison.as_mut().unwrap().attackers.clear();
        let mut w = World::new(rules, old.map().clone(), 42).unwrap();
        assert_eq!(
            issue(
                &mut w,
                Order::Load {
                    entity: EntityId(1),
                    target: EntityId(3)
                }
            ),
            None
        );
        for _ in 0..12 {
            w.step(&[]).unwrap();
        }
        assert_eq!(w.state.entities[0].garrisoned_in, Some(EntityId(3)));
        assert_eq!(
            w.load_rejection(EntityId(2), EntityId(3)),
            Some(Rejection::QueueFull)
        );
        let destination = Position { x: 200, y: 180 };
        assert_eq!(
            issue(
                &mut w,
                Order::Move {
                    entity: EntityId(3),
                    target: destination
                }
            ),
            None
        );
        for _ in 0..35 {
            w.step(&[]).unwrap();
        }
        assert_eq!(w.state.entities[2].position, destination);
        assert_eq!(w.state.entities[0].position, destination);
        assert!(!w.entity_visible(PlayerId(0), EntityId(1)));
        w.unload_garrison(2, true);
        assert_eq!(w.state.entities[0].hp, 0);
    }
    #[test]
    fn loading_checks_ownership_capacity_and_real_passenger_eligibility() {
        let mut w = world();
        assert_eq!(
            w.load_rejection(EntityId(4), EntityId(3)),
            Some(Rejection::InvalidTarget)
        );
        assert_eq!(
            w.load_rejection(EntityId(3), EntityId(3)),
            Some(Rejection::InvalidTarget)
        );
        load(&mut w, 0);
        load(&mut w, 1);
        assert_eq!(w.state.entities[0].position, w.state.entities[2].position);
        assert_eq!(
            w.state
                .entities
                .iter()
                .filter(|u| u.garrisoned_in == Some(EntityId(3)))
                .count(),
            2
        );
        w.state.entities[3].owner = PlayerId(0);
        assert_eq!(
            w.load_rejection(EntityId(4), EntityId(3)),
            Some(Rejection::QueueFull)
        );
    }
    #[test]
    fn passengers_fire_independent_weapons_with_range_bonus_and_workers_do_not_fire() {
        let mut w = world();
        load(&mut w, 0);
        load(&mut w, 1);
        let mut damage = Damage::default();
        w.advance_garrison_attack(0, &mut damage);
        w.advance_garrison_attack(1, &mut damage);
        assert_eq!(damage.hits[&EntityId(4)].values().sum::<u64>(), 6 * 256);
        assert_eq!(
            damage.hits[&EntityId(4)][&EntityId(3)],
            6 * 256,
            "defenders must see the bunker as the attacker"
        );
        assert_eq!(w.state.entities[0].cooldown, 3);
        w.advance_garrison_attack(0, &mut damage);
        assert_eq!(damage.hits[&EntityId(4)].values().sum::<u64>(), 6 * 256);
        assert_eq!(w.state.entities[1].cooldown, 0);
    }
    #[test]
    fn loaded_attacks_apply_research_armor_range_and_stim_cooldown() {
        let mut w = world();
        load(&mut w, 0);
        Arc::make_mut(&mut w.rules).units[0]
            .weapon
            .as_mut()
            .unwrap()
            .cooldown = 22;
        for (id, effect) in [
            (
                1,
                ResearchEffect::WeaponDamage {
                    units: vec![UnitTypeId(1)],
                    amount: 3,
                },
            ),
            (
                2,
                ResearchEffect::WeaponRange {
                    units: vec![UnitTypeId(1)],
                    amount: 32,
                    sight: 0,
                },
            ),
            (
                3,
                ResearchEffect::Armor {
                    units: vec![UnitTypeId(1)],
                    amount: 1,
                },
            ),
        ] {
            Arc::make_mut(&mut w.rules).research.push(Research {
                available: true,
                id: ResearchId(id),
                facility: UnitTypeId(3),
                previous: None,
                prerequisites: Vec::new(),
                cost: vec![],
                ticks: 1,
                effect,
            });
        }
        w.state.players[0]
            .completed_research
            .extend([ResearchId(1), ResearchId(2)]);
        w.state.players[1].completed_research.insert(ResearchId(3));
        w.state.entities[3].position = Position { x: 185, y: 64 };
        w.state.entities[0].stim_remaining = 100;
        let mut damage = Damage::default();
        w.advance_garrison_attack(0, &mut damage);
        assert_eq!(damage.hits[&EntityId(4)].values().sum::<u64>(), 8 * 256);
        assert_eq!(w.state.entities[0].cooldown, 11);
        Arc::make_mut(&mut w.rules).units[0]
            .weapon
            .as_mut()
            .unwrap()
            .cooldown = 8;
        w.state.entities[0].cooldown = 0;
        w.advance_garrison_attack(0, &mut damage);
        assert_eq!(w.state.entities[0].cooldown, 5);
    }
    #[test]
    fn unloading_places_each_passenger_without_overlap_and_blocked_death_kills_only_trapped() {
        let mut w = world();
        load(&mut w, 0);
        load(&mut w, 1);
        w.unload_garrison(2, false);
        assert!(
            w.state.entities[..2]
                .iter()
                .all(|u| u.garrisoned_in.is_none())
        );
        assert_ne!(w.state.entities[0].position, w.state.entities[1].position);
        let mut w = world();
        load(&mut w, 0);
        load(&mut w, 1);
        Arc::make_mut(&mut w.map).terrain = Some(crate::map::Terrain {
            cell_size: 8,
            columns: 32,
            rows: 32,
            flags: vec![0; 1024],
        });
        w.unload_garrison(2, false);
        assert!(
            w.state.entities[..2]
                .iter()
                .all(|u| u.garrisoned_in.is_some() && u.hp > 0)
        );
        w.unload_garrison(2, true);
        assert!(
            w.state.entities[..2]
                .iter()
                .all(|u| u.garrisoned_in.is_none() && u.hp == 0)
        );
    }
}
