//! Finite deployable ground mines. The state is explicit because deployment,
//! hidden waiting, emergence and friendly splash are required campaign behavior.
use super::rts::{Damage, distance, in_range};
use super::*;
use anyhow::Context;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MineLayer {
    pub unit_type: UnitTypeId,
    pub initial_count: u8,
    pub deploy_range: u32,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MineStats {
    pub arm_ticks: u32,
    pub conceal_ticks: u32,
    pub reveal_ticks: u32,
    pub trigger_range: u32,
    pub chase_range: u32,
    pub detonation_range: u32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MinePhase {
    Arming,
    Concealing,
    Armed,
    Emerging,
    Chasing,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MineState {
    pub phase: MinePhase,
    pub remaining: u32,
    pub target: Option<EntityId>,
}
pub(super) fn triggers_mines_default() -> bool {
    true
}
pub(super) fn validate_mine_rules(rules: &Rules) -> Result<()> {
    for unit in &rules.units {
        if let Some(layer) = &unit.mine_layer {
            ensure!(
                !unit.structure
                    && unit.speed > 0
                    && (1..=16).contains(&layer.initial_count)
                    && layer.deploy_range <= 1024,
                "invalid mine layer"
            );
            ensure!(
                rules
                    .units
                    .iter()
                    .any(|mine| mine.id == layer.unit_type && mine.mine.is_some()),
                "unknown deployable mine type"
            );
        }
        if let Some(mine) = &unit.mine {
            let weapon = unit.weapon.as_ref().context("mine needs a weapon")?;
            ensure!(
                !unit.structure
                    && unit.mine_layer.is_none()
                    && unit.speed > 0
                    && unit.movement_class == MovementClass::Ground
                    && weapon.splash.is_some()
                    && !weapon.targets_air
                    && weapon.strikes.is_empty()
                    && [mine.arm_ticks, mine.conceal_ticks, mine.reveal_ticks]
                        .iter()
                        .all(|&v| (1..=10000).contains(&v))
                    && mine.trigger_range > 0
                    && mine.trigger_range <= mine.chase_range
                    && mine.chase_range <= 32768
                    && mine.detonation_range <= mine.trigger_range,
                "invalid deployable mine definition"
            );
        }
    }
    Ok(())
}
impl World {
    pub fn mine_rejection(&self, entity: EntityId, position: Position) -> Option<Rejection> {
        let Some(index) = self.index(entity) else {
            return Some(Rejection::UnknownEntity);
        };
        let actor = &self.state.entities[index];
        let Some(layer) = &self.unit_at(index).mine_layer else {
            return Some(Rejection::UnsupportedOrder);
        };
        if !self.ability_research_ready(actor, false) {
            return Some(Rejection::MissingPrerequisite);
        }
        if actor.mine_count == 0
            || actor.garrisoned_in.is_some()
            || actor.cloaked
            || self.inside_structure(actor)
        {
            return Some(Rejection::InvalidTarget);
        }
        if actor.construction.is_some() {
            return Some(Rejection::Unfinished);
        }
        if self.state.entities.len() >= 4096 || self.state.next_entity_id == u32::MAX {
            return Some(Rejection::EntityLimit);
        }
        let mine = self
            .unit_type(layer.unit_type)
            .expect("validated mine definition");
        if !self.can_place(
            position,
            mine.footprint,
            MovementClass::Ground,
            Some(entity),
        ) {
            return Some(Rejection::InvalidTarget);
        }
        None
    }
    pub(super) fn advance_place_mine(&mut self, index: usize, position: Position) {
        let id = self.state.entities[index].id;
        if self.mine_rejection(id, position).is_some() {
            self.finish(index);
            return;
        }
        let layer = self
            .unit_at(index)
            .mine_layer
            .clone()
            .expect("validated layer");
        if distance(self.state.entities[index].position, position)
            > i64::from(layer.deploy_range).pow(2)
        {
            self.navigate(index, position, false);
            return;
        }
        let mine = self
            .unit_type(layer.unit_type)
            .expect("validated mine")
            .clone();
        let stats = mine.mine.as_ref().expect("validated mine stats");
        let entity = Entity {
            id: EntityId(self.state.next_entity_id),
            owner: self.state.entities[index].owner,
            unit_type: mine.id,
            position,
            hp: mine.max_hp,
            mine_state: Some(MineState {
                phase: MinePhase::Arming,
                remaining: stats.arm_ticks,
                target: None,
            }),
            ..Entity::default()
        };
        self.state.next_entity_id += 1;
        self.state.entities[index].mine_count -= 1;
        self.record_created(entity.owner, entity.unit_type);
        self.state.entities.push(entity);
        self.finish(index);
    }
    pub(super) fn advance_mine(&mut self, index: usize, damage: &mut Damage) {
        let stats = self
            .unit_at(index)
            .mine
            .clone()
            .expect("mine hook only for mine types");
        let mut state = self.state.entities[index]
            .mine_state
            .clone()
            .unwrap_or(MineState {
                phase: MinePhase::Arming,
                remaining: stats.arm_ticks,
                target: None,
            });
        if state.remaining > 0 {
            state.remaining -= 1;
        }
        match state.phase {
            MinePhase::Arming if state.remaining == 0 => {
                state.phase = MinePhase::Concealing;
                state.remaining = stats.conceal_ticks;
            }
            MinePhase::Concealing if state.remaining == 0 => {
                self.state.entities[index].cloaked = true;
                state.phase = MinePhase::Armed;
            }
            MinePhase::Emerging if state.remaining == 0 => {
                state.phase = MinePhase::Chasing;
            }
            _ => {}
        }
        match state.phase {
            MinePhase::Armed => {
                let actor = &self.state.entities[index];
                let range = i64::from(stats.trigger_range);
                state.target = self
                    .state
                    .entities
                    .iter()
                    .filter(|other| {
                        let unit = self.unit_type(other.unit_type).expect("validated unit");
                        self.is_enemy_entity(actor.owner, other)
                            && other.hp > 0
                            && !other.invincible
                            && !unit.structure
                            && !unit.revealer
                            && unit.triggers_mines
                            && self.movement_class(other) == MovementClass::Ground
                            && other.garrisoned_in.is_none()
                            && !self.inside_structure(other)
                            && (i64::from(actor.position.x) - i64::from(other.position.x)).abs()
                                <= range
                            && (i64::from(actor.position.y) - i64::from(other.position.y)).abs()
                                <= range
                    })
                    .min_by_key(|other| (distance(actor.position, other.position), other.id))
                    .map(|other| other.id);
                if state.target.is_some() {
                    state.phase = MinePhase::Emerging;
                    state.remaining = stats.reveal_ticks;
                    self.state.entities[index].cloaked = false;
                }
            }
            MinePhase::Chasing => {
                let target = state.target.and_then(|id| self.index(id));
                let actor = self.state.entities[index].clone();
                if let Some(target) = target.filter(|&target| {
                    let enemy = &self.state.entities[target];
                    enemy.hp > 0
                        && !enemy.invincible
                        && enemy.garrisoned_in.is_none()
                        && !self.inside_structure(enemy)
                        && self.movement_class(enemy) == MovementClass::Ground
                        && self.is_enemy_entity(actor.owner, enemy)
                        && in_range(
                            actor.position,
                            self.unit_at(index).footprint,
                            enemy.position,
                            self.unit_at(target).footprint,
                            stats.chase_range,
                        )
                }) {
                    let enemy = &self.state.entities[target];
                    if in_range(
                        actor.position,
                        self.unit_at(index).footprint,
                        enemy.position,
                        self.unit_at(target).footprint,
                        stats.detonation_range,
                    ) {
                        self.explode_mine(index, damage);
                    } else {
                        let position = enemy.position;
                        let footprint = self.unit_at(target).footprint;
                        let retry_due = self.state.tick >= actor.path_retry;
                        self.approach(index, position, footprint, stats.detonation_range);
                        if retry_due
                            && self.state.entities[index].path.is_empty()
                            && !in_range(
                                self.state.entities[index].position,
                                self.unit_at(index).footprint,
                                position,
                                footprint,
                                stats.detonation_range,
                            )
                        {
                            self.assign(index, UnitOrder::Idle, true);
                            state.phase = MinePhase::Concealing;
                            state.remaining = stats.conceal_ticks;
                            state.target = None;
                        }
                    }
                } else {
                    self.assign(index, UnitOrder::Idle, true);
                    state.phase = MinePhase::Concealing;
                    state.remaining = stats.conceal_ticks;
                    state.target = None;
                }
            }
            _ => {}
        }
        self.state.entities[index].mine_state = Some(state);
    }
    fn explode_mine(&mut self, index: usize, damage: &mut Damage) {
        let actor = &self.state.entities[index];
        let weapon = self
            .unit_at(index)
            .weapon
            .as_ref()
            .expect("validated mine weapon");
        let radii = weapon.splash.expect("validated mine splash");
        for other in &self.state.entities {
            let unit = self.unit_type(other.unit_type).expect("validated unit");
            if other.id == actor.id
                || other.hp == 0
                || other.invincible
                || unit.revealer
                || other.garrisoned_in.is_some()
                || self.inside_structure(other)
                || self.movement_class(other) != MovementClass::Ground
            {
                continue;
            }
            if let Some(ring) = radii.iter().position(|&range| {
                in_range(
                    actor.position,
                    Footprint::default(),
                    other.position,
                    unit.footprint,
                    range,
                )
            }) {
                if other.cloaked && ring != 0 {
                    continue;
                }
                self.record_hit(
                    damage,
                    (actor.id, actor.unit_type),
                    other,
                    weapon,
                    1_u64 << ring,
                );
            }
        }
        self.state.entities[index].hp = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn world() -> World {
        let rules = Rules {
            id: "mines".into(),
            units: vec![
                UnitType {
                    id: UnitTypeId(1),
                    speed: 8,
                    max_hp: 500,
                    size: UnitSize::Large,
                    footprint: Footprint {
                        width: 8,
                        height: 8,
                    },
                    mine_layer: Some(MineLayer {
                        unit_type: UnitTypeId(2),
                        initial_count: 3,
                        deploy_range: 20,
                    }),
                    ..UnitType::default()
                },
                UnitType {
                    id: UnitTypeId(2),
                    speed: 16,
                    max_hp: 20,
                    footprint: Footprint {
                        width: 15,
                        height: 15,
                    },
                    triggers_mines: false,
                    mine: Some(MineStats {
                        arm_ticks: 60,
                        conceal_ticks: 4,
                        reveal_ticks: 3,
                        trigger_range: 96,
                        chase_range: 576,
                        detonation_range: 30,
                    }),
                    weapon: Some(Weapon {
                        friendly_splash: false,
                        projectile_speed: 0,
                        cooldown_jitter: None,
                        damage: 125,
                        range: 10,
                        cooldown: 22,
                        damage_kind: DamageKind::Explosive,
                        targets_air: false,
                        target_classes: Vec::new(),
                        splash: Some([40, 60, 80]),
                        strikes: vec![],
                    }),
                    ..UnitType::default()
                },
                UnitType {
                    id: UnitTypeId(3),
                    speed: 4,
                    max_hp: 500,
                    size: UnitSize::Large,
                    footprint: Footprint {
                        width: 4,
                        height: 4,
                    },
                    ..UnitType::default()
                },
                UnitType {
                    id: UnitTypeId(4),
                    speed: 4,
                    max_hp: 500,
                    size: UnitSize::Large,
                    footprint: Footprint {
                        width: 4,
                        height: 4,
                    },
                    triggers_mines: false,
                    ..UnitType::default()
                },
                UnitType {
                    id: UnitTypeId(5),
                    speed: 0,
                    max_hp: 500,
                    size: UnitSize::Large,
                    structure: true,
                    footprint: Footprint {
                        width: 4,
                        height: 4,
                    },
                    ..UnitType::default()
                },
            ],
            ..Rules::default()
        };
        let map = Map {
            initial_explored: Default::default(),
            creation: Default::default(),
            ai: Vec::new(),
            id: "mines".into(),
            width: 512,
            height: 512,
            players: 2,
            spawns: vec![
                Spawn {
                    unit_type: UnitTypeId(1),
                    position: Position { x: 20, y: 20 },
                    ..Spawn::default()
                },
                Spawn {
                    unit_type: UnitTypeId(3),
                    owner: PlayerId(1),
                    position: Position { x: 220, y: 128 },
                    ..Spawn::default()
                },
                Spawn {
                    unit_type: UnitTypeId(4),
                    owner: PlayerId(1),
                    position: Position { x: 180, y: 128 },
                    ..Spawn::default()
                },
                Spawn {
                    unit_type: UnitTypeId(5),
                    owner: PlayerId(1),
                    position: Position { x: 128, y: 210 },
                    ..Spawn::default()
                },
            ],
            resources: vec![],
            start_locations: vec![],
            terrain: None,
            mission: None,
            fog_of_war: false,
        };
        World::new(rules, map, 1).unwrap()
    }
    fn deploy(w: &mut World) -> usize {
        w.state.entities[0].position = Position { x: 120, y: 128 };
        w.state.entities[0].mine_count = 3;
        w.advance_place_mine(0, Position { x: 128, y: 128 });
        w.state.entities.len() - 1
    }
    #[test]
    fn deployment_uses_inventory_and_rejects_occupied_target_without_spending() {
        let mut w = world();
        let mine = deploy(&mut w);
        assert_eq!(w.state.entities[0].mine_count, 2);
        assert_eq!(w.state.entities[mine].position, Position { x: 128, y: 128 });
        assert_eq!(
            w.mine_rejection(EntityId(1), Position { x: 220, y: 128 }),
            Some(Rejection::InvalidTarget)
        );
        w.advance_place_mine(0, Position { x: 220, y: 128 });
        assert_eq!(w.state.entities[0].mine_count, 2);
        w.state.entities[0].mine_count = 0;
        assert_eq!(
            w.mine_rejection(EntityId(1), Position { x: 100, y: 100 }),
            Some(Rejection::InvalidTarget)
        );
    }
    #[test]
    fn mine_arms_then_burrows_and_uses_square_trigger_ignoring_hover_and_structures() {
        let mut w = world();
        let mine = deploy(&mut w);
        w.state.entities[0].position = Position { x: 20, y: 20 };
        w.state.entities[1].position = Position { x: 400, y: 400 };
        let mut damage = Damage::default();
        for _ in 0..59 {
            w.advance_mine(mine, &mut damage);
        }
        assert_eq!(
            w.state.entities[mine].mine_state.as_ref().unwrap().phase,
            MinePhase::Arming
        );
        w.advance_mine(mine, &mut damage);
        assert_eq!(
            w.state.entities[mine].mine_state.as_ref().unwrap().phase,
            MinePhase::Concealing
        );
        for _ in 0..4 {
            w.advance_mine(mine, &mut damage);
        }
        assert_eq!(
            w.state.entities[mine].mine_state.as_ref().unwrap().phase,
            MinePhase::Armed
        );
        assert!(w.state.entities[mine].cloaked);
        w.state.entities[1].position = Position { x: 220, y: 220 };
        w.advance_mine(mine, &mut damage);
        assert_eq!(
            w.state.entities[mine].mine_state.as_ref().unwrap().target,
            Some(EntityId(2))
        );
        assert_eq!(
            w.state.entities[mine].mine_state.as_ref().unwrap().phase,
            MinePhase::Emerging
        );
        assert!(!w.state.entities[mine].cloaked);
        for _ in 0..3 {
            w.advance_mine(mine, &mut damage);
        }
        assert_eq!(
            w.state.entities[mine].mine_state.as_ref().unwrap().phase,
            MinePhase::Chasing
        );
    }
    #[test]
    fn explosion_has_friendly_ground_splash_and_excludes_air_or_loaded_units() {
        let mut w = world();
        let mine = deploy(&mut w);
        w.state.entities[1].position = Position { x: 150, y: 128 };
        w.state.entities[2].airborne = true;
        w.state.entities[3].position = Position { x: 190, y: 128 };
        let mut damage = Damage::default();
        w.explode_mine(mine, &mut damage);
        assert_eq!(damage.hits[&EntityId(1)].values().sum::<u64>(), 125 * 256);
        assert_eq!(damage.hits[&EntityId(2)].values().sum::<u64>(), 125 * 256);
        assert!(!damage.hits.contains_key(&EntityId(3)));
        assert_eq!(
            damage.hits[&EntityId(4)].values().sum::<u64>(),
            125 * 256 / 2
        );
        assert_eq!(w.state.entities[mine].hp, 0);
    }
    #[test]
    fn lost_target_reburrows_without_spending_another_mine() {
        let mut w = world();
        let mine = deploy(&mut w);
        w.state.entities[mine].mine_state = Some(MineState {
            phase: MinePhase::Chasing,
            remaining: 0,
            target: Some(EntityId(99)),
        });
        w.advance_mine(mine, &mut Damage::default());
        let state = w.state.entities[mine].mine_state.as_ref().unwrap();
        assert_eq!(state.phase, MinePhase::Concealing);
        assert_eq!(state.remaining, 4);
        assert_eq!(w.state.entities[0].mine_count, 2);
    }
    #[test]
    fn unreachable_chase_reburrows_instead_of_staying_exposed_forever() {
        let mut w = world();
        let mine = deploy(&mut w);
        w.state.entities[0].position = Position { x: 20, y: 20 };
        let mut flags = vec![crate::map::WALKABLE; 64 * 64];
        for y in 0..64 {
            flags[y * 64 + 20] = 0;
        }
        Arc::make_mut(&mut w.map).terrain = Some(Terrain {
            cell_size: 8,
            columns: 64,
            rows: 64,
            flags,
        });
        w.state.entities[mine].mine_state = Some(MineState {
            phase: MinePhase::Chasing,
            remaining: 0,
            target: Some(EntityId(2)),
        });
        w.advance_mine(mine, &mut Damage::default());
        assert_eq!(
            w.state.entities[mine].mine_state.as_ref().unwrap().phase,
            MinePhase::Concealing
        );
    }
}
