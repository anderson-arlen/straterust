//! Liftable structures share ordinary movement after leaving ground occupancy.
use super::*;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Flight {
    pub speed: i32,
    pub lift_ticks: u32,
    pub land_ticks: u32,
}

impl World {
    pub(super) fn initialize_addons(&mut self) {
        for index in 0..self.state.entities.len() {
            let addon = &self.state.entities[index];
            let Some(parent_type) = self.unit_at(index).addon_parent else {
                continue;
            };
            let parent = self
                .state
                .entities
                .iter()
                .find(|parent| {
                    parent.owner == addon.owner
                        && parent.unit_type == parent_type
                        && !parent.airborne
                        && parent.construction.is_none()
                        && self.addon_position(parent.id) == Some(addon.position)
                        && !self
                            .state
                            .entities
                            .iter()
                            .any(|other| other.parent == Some(parent.id))
                })
                .map(|parent| parent.id);
            self.state.entities[index].parent = parent;
        }
    }
    pub fn movement_class(&self, entity: &Entity) -> MovementClass {
        if entity.airborne {
            MovementClass::Air
        } else {
            self.unit_type(entity.unit_type)
                .expect("validated type")
                .movement_class
        }
    }
    /// The weapon and target permissions shared by simulation and presentation.
    pub fn weapon_for(&self, actor: &Entity, target: &Entity) -> Option<&Weapon> {
        let unit = self.unit_type(actor.unit_type)?;
        let class = self.movement_class(target);
        let weapon = if class == MovementClass::Air {
            unit.air_weapon
                .as_ref()
                .or_else(|| unit.weapon.as_ref().filter(|w| w.targets_air))
        } else {
            unit.weapon.as_ref().filter(|_| unit.attacks_ground)
        };
        weapon.filter(|w| w.target_classes.is_empty() || w.target_classes.contains(&class))
    }
    /// Explicit attack eligibility; automatic/contextual targeting also checks hostility.
    pub fn can_target_entity(&self, actor: &Entity, target: &Entity) -> bool {
        self.is_enemy(actor.owner, target.owner)
            && target.hp > 0
            && !target.invincible
            && !self.inside_structure(target)
            && target.garrisoned_in.is_none()
            && !self.unit_type(target.unit_type).expect("type").revealer
            && self.weapon_for(actor, target).is_some()
    }
    /// Return fire only toward the remembered damage origin, never track an unseen mover.
    pub fn can_return_stationary_fire(&self, actor: &Entity, target: &Entity) -> bool {
        self.unit_type(actor.unit_type)
            .is_some_and(|unit| unit.speed == 0 && unit.weapon.is_some())
            && actor.auto_attack_target == Some(target.id)
            && actor.retaliation_position == Some(target.position)
            && (!target.cloaked || self.detected(actor.owner, target.position))
            && !self.movement_locked(target)
            && self.can_target_entity(actor, target)
    }
    pub(super) fn can_attack_entity(&self, actor: &Entity, target: &Entity) -> bool {
        self.can_target_entity(actor, target) && self.entity_visible(actor.owner, target.id)
    }
    pub fn lift_rejection(&self, id: EntityId) -> Option<Rejection> {
        let Some(index) = self.index(id) else {
            return Some(Rejection::UnknownEntity);
        };
        let actor = &self.state.entities[index];
        if self.unit_at(index).flight.is_none() || actor.airborne {
            return Some(Rejection::UnsupportedOrder);
        }
        if actor.construction.is_some() {
            return Some(Rejection::Unfinished);
        }
        if !actor.production.is_empty()
            || actor.research.is_some()
            || actor.flight_transition != 0
            || self
                .state
                .entities
                .iter()
                .any(|entity| entity.parent == Some(id) && entity.construction.is_some())
        {
            return Some(Rejection::QueueFull);
        }
        None
    }
    pub(super) fn start_lift(&mut self, index: usize) -> Option<Rejection> {
        let id = self.state.entities[index].id;
        if let Some(reason) = self.lift_rejection(id) {
            return Some(reason);
        }
        self.assign(index, UnitOrder::Idle, true);
        self.state.entities[index].airborne = true;
        self.state.entities[index].flight_transition =
            self.unit_at(index).flight.as_ref()?.lift_ticks;
        for entity in &mut self.state.entities {
            if entity.parent == Some(id) {
                entity.parent = None;
            }
        }
        None
    }
    pub fn land_rejection(&self, id: EntityId, target: Position) -> Option<Rejection> {
        let Some(index) = self.index(id) else {
            return Some(Rejection::UnknownEntity);
        };
        let actor = &self.state.entities[index];
        let unit = self.unit_at(index);
        if unit.flight.is_none() || !actor.airborne {
            return Some(Rejection::UnsupportedOrder);
        }
        if actor.flight_transition != 0 {
            return Some(Rejection::Unfinished);
        }
        // An addon's collision bounds may extend into the parent's reserved
        // placement rectangle. Exempt the compatible addon being reattached.
        let addon = self.landing_addon(actor, target);
        if !self.resource_clearance_allowed(unit, target)
            || !self.creep_placement_allowed(unit, target)
            || !self.map.can_build(target, unit.placement)
            || self.occupied_except(
                target,
                unit.placement,
                MovementClass::Ground,
                Some(id),
                addon,
            )
            || self.occupied_except(
                target,
                unit.footprint,
                MovementClass::Ground,
                Some(id),
                addon,
            )
        {
            return Some(Rejection::InvalidPlacement);
        }
        None
    }
    fn landing_addon(&self, actor: &Entity, target: Position) -> Option<EntityId> {
        self.state
            .entities
            .iter()
            .find(|entity| {
                // Detached addons can be claimed by landing, including neutral
                // map fixtures and addons abandoned by another player.
                entity.hp > 0
                    && !entity.airborne
                    && entity.parent.is_none()
                    && entity.construction.is_none()
                    && self
                        .unit_type(entity.unit_type)
                        .is_some_and(|addon| addon.addon_parent == Some(actor.unit_type))
                    && self.addon_parent_position(entity.unit_type, entity.position) == Some(target)
            })
            .map(|entity| entity.id)
    }
    pub(super) fn advance_land(&mut self, index: usize, target: Position) {
        let id = self.state.entities[index].id;
        if self.land_rejection(id, target).is_some() {
            self.finish(index);
            return;
        }
        if self.navigate(index, target, false) {
            self.state.entities[index].flight_transition = self
                .unit_at(index)
                .flight
                .as_ref()
                .expect("flight")
                .land_ticks;
        }
    }
    pub(super) fn advance_flight(&mut self, index: usize) -> bool {
        if self.state.entities[index].flight_transition == 0 {
            return false;
        }
        self.state.entities[index].flight_transition -= 1;
        if self.state.entities[index].flight_transition == 0
            && let UnitOrder::Land { target } = self.state.entities[index].order
        {
            let id = self.state.entities[index].id;
            if self.land_rejection(id, target).is_none() {
                let addon = self.landing_addon(&self.state.entities[index], target);
                self.state.entities[index].airborne = false;
                let owner = self.state.entities[index].owner;
                if let Some(addon) = addon.and_then(|id| self.index(id)) {
                    if self.state.entities[addon].owner != owner {
                        self.cancel_research(addon);
                        self.assign(addon, UnitOrder::Idle, true);
                        self.state.entities[addon].owner = owner;
                        let addon_id = self.state.entities[addon].id;
                        for child in &mut self.state.entities {
                            if child.parent == Some(addon_id) {
                                child.owner = owner;
                            }
                        }
                    }
                    self.state.entities[addon].parent = Some(id);
                }
            }
            self.finish(index);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lifted_building_changes_target_layer_moves_and_lands_without_ground_overlap() {
        for targets_air in [false, true] {
            let rules = Rules {
                id: "flight".into(),
                units: vec![
                    UnitType {
                        id: UnitTypeId(1),
                        speed: 0,
                        max_hp: 100,
                        structure: true,
                        footprint: Footprint {
                            width: 16,
                            height: 16,
                        },
                        placement: Footprint {
                            width: 16,
                            height: 16,
                        },
                        flight: Some(Flight {
                            speed: 8,
                            lift_ticks: 2,
                            land_ticks: 2,
                        }),
                        ..UnitType::default()
                    },
                    UnitType {
                        id: UnitTypeId(2),
                        speed: 0,
                        weapon: Some(Weapon {
                            friendly_splash: false,
                            projectile_speed: 0,
                            cooldown_jitter: None,
                            targets_air,
                            target_classes: Vec::new(),
                            damage: 10,
                            range: 128,
                            cooldown: 20,
                            damage_kind: DamageKind::Normal,
                            splash: None,
                            strikes: Vec::new(),
                        }),
                        ..UnitType::default()
                    },
                ],
                ..Rules::default()
            };
            let map = Map {
                id: "flight".into(),
                width: 256,
                height: 256,
                players: 2,
                spawns: vec![
                    Spawn {
                        unit_type: UnitTypeId(1),
                        position: Position { x: 64, y: 64 },
                        ..Spawn::default()
                    },
                    Spawn {
                        owner: PlayerId(1),
                        unit_type: UnitTypeId(2),
                        position: Position { x: 160, y: 64 },
                        ..Spawn::default()
                    },
                ],
                resources: Vec::new(),
                start_locations: Vec::new(),
                terrain: None,
                initial_explored: Default::default(),
                creation: Default::default(),
                ai: Vec::new(),
                mission: None,
                fog_of_war: false,
            };
            let mut world = World::new(rules, map, 0).unwrap();
            assert!(
                world
                    .step(&[Command {
                        tick: Tick(0),
                        player: PlayerId(0),
                        sequence: 1,
                        order: Order::Lift {
                            entity: EntityId(1)
                        }
                    }])
                    .unwrap()[0]
                    .rejection
                    .is_none()
            );
            assert!(world.state.entities[0].airborne);
            assert_eq!(
                world.state.entities[0].hp,
                if targets_air { 90 } else { 100 }
            );
            assert!(!world.is_occupied(
                Position { x: 64, y: 64 },
                Footprint::default(),
                MovementClass::Ground,
                None
            ));
            world.step(&[]).unwrap();
            assert_eq!(
                world.land_rejection(EntityId(1), Position { x: 160, y: 64 }),
                Some(Rejection::InvalidPlacement)
            );
            assert!(
                world
                    .step(&[Command {
                        tick: world.tick(),
                        player: PlayerId(0),
                        sequence: 2,
                        order: Order::Land {
                            entity: EntityId(1),
                            target: Position { x: 96, y: 128 }
                        }
                    }])
                    .unwrap()[0]
                    .rejection
                    .is_none()
            );
            for _ in 0..20 {
                world.step(&[]).unwrap();
            }
            assert!(!world.state.entities[0].airborne);
            assert_eq!(world.state.entities[0].position, Position { x: 96, y: 128 });
            assert!(world.is_occupied(
                Position { x: 96, y: 128 },
                Footprint::default(),
                MovementClass::Ground,
                None
            ));
        }
    }
}
