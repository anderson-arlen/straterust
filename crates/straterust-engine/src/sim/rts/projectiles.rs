//! Committed weapon missiles outlive their shooter and survive checkpoints.
use super::*;

pub(super) fn is_zero(value: &u32) -> bool {
    *value == 0
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PendingProjectile {
    pub remaining: u32,
    pub source: EntityId,
    pub unit_type: UnitTypeId,
    pub owner: PlayerId,
    pub target: EntityId,
    pub aim: Position,
    pub weapon: Weapon,
    pub damage_percent: u32,
    pub piercing_bonus: u32,
}

impl World {
    pub(in crate::sim) fn launch_weapon_projectile(
        &mut self,
        actor: &Entity,
        target: &Entity,
        weapon: &Weapon,
    ) {
        if actor.illusion_remaining.is_some() || self.state.projectiles.len() >= 16384 {
            return;
        }
        let length = (distance(actor.position, target.position) as u64).isqrt();
        // Presentation emits from twelve pixels ahead of the body anchor.
        let ticks = (length.saturating_sub(12) * 256)
            .div_ceil(u64::from(weapon.projectile_speed))
            .max(1);
        self.state.projectiles.push(PendingProjectile {
            remaining: ticks as u32,
            source: actor.garrisoned_in.unwrap_or(actor.id),
            unit_type: actor.unit_type,
            owner: actor.owner,
            target: target.id,
            aim: target.position,
            weapon: weapon.clone(),
            damage_percent: self.buff_percent(actor, false),
            piercing_bonus: self.research_weapon_bonus(
                actor.owner,
                actor.unit_type,
                self.movement_class(target) == MovementClass::Air,
            ),
        });
    }

    pub(in crate::sim) fn advance_weapon_projectiles(&mut self, damage: &mut Damage) {
        for mut shot in std::mem::take(&mut self.state.projectiles) {
            shot.remaining = shot.remaining.saturating_sub(1);
            if shot.remaining != 0 {
                self.state.projectiles.push(shot);
                continue;
            }
            damage.source_owners.insert(shot.source, shot.owner);
            for target in &self.state.entities {
                if target.invincible
                    || target.garrisoned_in.is_some()
                    || self.inside_structure(target)
                    || (!self.is_enemy(shot.owner, target.owner) && !shot.weapon.friendly_splash)
                {
                    continue;
                }
                let definition = self.unit_type(target.unit_type).expect("validated type");
                if definition.revealer
                    || self.movement_class(target) == MovementClass::Air && !shot.weapon.targets_air
                {
                    continue;
                }
                if !shot.weapon.target_classes.is_empty()
                    && !shot
                        .weapon
                        .target_classes
                        .contains(&self.movement_class(target))
                {
                    continue;
                }
                let divisor = if let Some(radii) = shot.weapon.splash {
                    radii
                        .iter()
                        .position(|radius| {
                            in_range(
                                shot.aim,
                                Footprint::default(),
                                target.position,
                                definition.footprint,
                                *radius,
                            )
                        })
                        .map(|ring| 1 << ring)
                } else {
                    (target.id == shot.target).then_some(1)
                };
                if let Some(divisor) = divisor {
                    self.record_weapon_hit(
                        damage,
                        (shot.source, shot.unit_type),
                        target,
                        &shot.weapon,
                        divisor,
                        Some((shot.damage_percent, shot.piercing_bonus)),
                    );
                }
            }
        }
    }
}

pub(in crate::sim) fn validate_weapon_projectiles(world: &World, state: &State) -> Result<()> {
    ensure!(
        state.projectiles.len() <= 16384,
        "too many saved weapon missiles"
    );
    for shot in &state.projectiles {
        if let DamageKind::Split {
            piercing,
            minimum_percent,
        } = shot.weapon.damage_kind
        {
            ensure!(
                piercing <= shot.weapon.damage && (1..=100).contains(&minimum_percent),
                "invalid saved missile damage"
            );
        }
        ensure!(
            shot.owner.0 < world.map.players
                && world.map.contains(shot.aim)
                && world.unit_type(shot.unit_type).is_some()
                && shot.source.0 < state.next_entity_id
                && shot.target.0 < state.next_entity_id
                && (1..=16_777_216).contains(&shot.remaining)
                && (1..=1024 * 256).contains(&shot.weapon.projectile_speed)
                && shot.weapon.strikes.is_empty()
                && shot.weapon.damage <= 1_000_000
                && (!shot.weapon.friendly_splash || shot.weapon.splash.is_some())
                && shot.damage_percent <= 10_000
                && shot.piercing_bonus <= 1_000_000
                && shot
                    .weapon
                    .splash
                    .is_none_or(|r| r[0] <= r[1] && r[1] <= r[2] && r[2] <= 32768),
            "invalid saved weapon missile"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fired_missile_delays_damage_survives_shooter_death_and_checkpoint() {
        let rules = Rules {
            id: "missile-checkpoint".into(),
            units: vec![
                UnitType {
                    id: UnitTypeId(1),
                    max_hp: 100,
                    vision_range: 512,
                    weapon: Some(Weapon {
                        friendly_splash: false,
                        projectile_speed: 8 * 256,
                        damage: 10,
                        range: 256,
                        cooldown: 100,
                        targets_air: false,
                        target_classes: Vec::new(),
                        cooldown_jitter: None,
                        damage_kind: DamageKind::Normal,
                        splash: None,
                        strikes: Vec::new(),
                    }),
                    ..Default::default()
                },
                UnitType {
                    id: UnitTypeId(2),
                    max_hp: 100,
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let map = Map {
            width: 1024,
            height: 1024,
            players: 2,
            spawns: vec![
                Spawn {
                    owner: PlayerId(0),
                    unit_type: UnitTypeId(1),
                    position: Position { x: 100, y: 100 },
                    ..Default::default()
                },
                Spawn {
                    owner: PlayerId(1),
                    unit_type: UnitTypeId(2),
                    position: Position { x: 220, y: 100 },
                    ..Default::default()
                },
                Spawn {
                    owner: PlayerId(0),
                    unit_type: UnitTypeId(2),
                    position: Position { x: 600, y: 600 },
                    ..Default::default()
                },
            ],
            id: "missile-checkpoint".into(),
            initial_explored: BTreeMap::new(),
            creation: BTreeMap::new(),
            ai: Vec::new(),
            start_locations: Vec::new(),
            resources: Vec::new(),
            fog_of_war: false,
            mission: None,
            terrain: None,
        };
        let mut world = World::new(rules, map, 7).unwrap();
        world.step(&[]).unwrap();
        assert_eq!(world.state.projectiles.len(), 1);
        assert_eq!(world.state.entities[1].hp, 100);
        world.state.entities[0].hp = 0;
        world.step(&[]).unwrap();
        assert!(world.index(EntityId(1)).is_none());
        let mut restored = world
            .restore_snapshot(world.save_snapshot().unwrap())
            .unwrap();
        let view = world
            .player_view(PlayerId(0))
            .unwrap()
            .into_world(&world)
            .unwrap();
        assert!(
            view.state.projectiles.is_empty(),
            "server missile state stays private"
        );
        for _ in 0..14 {
            world.step(&[]).unwrap();
            restored.step(&[]).unwrap();
            assert_eq!(world.state_hash(), restored.state_hash());
        }
        assert!(world.state.projectiles.is_empty());
        assert_eq!(
            world
                .state
                .entities
                .iter()
                .find(|e| e.owner == PlayerId(1))
                .unwrap()
                .hp,
            90
        );
    }
}
