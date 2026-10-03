//! Per-unit presentation observations. Facing and animation never change the world.
use std::collections::BTreeMap;
use std::time::Duration;

use straterust_engine::assets::{AssetPack, ClipKind, Image, Projectile, SpriteRef};

use straterust_engine::sim::{
    Entity, EntityId, Footprint, MinePhase, PlayerId, Position, UnitOrder, UnitTypeId, World,
};

mod commands;
mod garrison;
pub use commands::{CommandFeedback, CommandTarget};
pub use garrison::garrison_frames;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum VisualAction {
    #[default]
    Idle,
    Move,
    Attack,
    Work,
    Production,
}

#[derive(Clone, Copy, Debug)]
pub struct UnitVisual {
    /// 32 clockwise directions: north=0, east=8, south=16, west=24.
    pub facing: u8,
    pub moving: bool,
    pub action: VisualAction,
    pub since_tick: u64,
    pub shot_tick: Option<u64>,
    pub hit_tick: Option<u64>,
    pub effect_target: Option<Position>,
    pub captured_tick: Option<u64>,
    position: Position,
    owner: PlayerId,
    unit_type: UnitTypeId,
    hp: u32,
    cooldown: u32,
    burrowed: bool,
    facing_since_tick: u64,
    burrow_changed: Option<u64>,
    cargo_amount: u32,
    construction_remaining: Option<u32>,
    repair_progress: u64,
    repair_target: Option<EntityId>,
}

impl UnitVisual {
    fn initial(entity: &Entity, tick: u64) -> Self {
        Self {
            facing: 8,
            moving: false,
            action: VisualAction::Idle,
            since_tick: tick,
            shot_tick: None,
            hit_tick: None,
            effect_target: None,
            captured_tick: None,
            position: entity.position,
            owner: entity.owner,
            unit_type: entity.unit_type,
            hp: entity.hp,
            cooldown: entity.cooldown,
            burrowed: entity.burrowed,
            facing_since_tick: tick,
            burrow_changed: None,
            cargo_amount: entity.cargo.as_ref().map_or(0, |cargo| cargo.amount),
            construction_remaining: entity.construction.as_ref().map(|work| work.remaining),
            repair_progress: entity.repair_progress,
            repair_target: if let UnitOrder::Repair { target } = entity.order {
                Some(target)
            } else {
                None
            },
        }
    }

    pub fn phase_ms(&self, world: &World) -> u128 {
        u128::from(world.tick().0.saturating_sub(self.since_tick))
            * u128::from(world.rules().tick_ms)
    }
}

pub struct Visuals {
    units: BTreeMap<EntityId, UnitVisual>,
    deaths: Vec<DeathVisual>,
    projectiles: Vec<ProjectileVisual>,
    tick: u64,
    finished: bool,
    command_feedback: Option<CommandFeedback>,
    pub movement_heading_debounce_ms: u32,
}

/// A finite presentation effect; the authoritative entity is already gone.
#[derive(Clone, Copy, Debug)]
pub struct DeathVisual {
    pub position: Position,
    pub unit_type: UnitTypeId,
    pub facing: u8,
    pub elapsed: Duration,
}

/// A shot retains its launch and target positions if either unit moves or dies.
pub struct ProjectileVisual {
    pub targets_air: bool,
    pub unit_type: UnitTypeId,
    pub owner: PlayerId,
    pub from: Position,
    pub to: Position,
    pub elapsed: Duration,
    tick_ms: u32,
}

impl ProjectileVisual {
    fn flight_ms(&self, effect: &Projectile) -> f64 {
        if effect.manifest.on_target {
            return 0.0;
        }
        let distance =
            (f64::from(self.to.x - self.from.x)).hypot(f64::from(self.to.y - self.from.y));
        (distance - f64::from(effect.manifest.forward_offset)).max(0.0)
            * 256.0
            * f64::from(self.tick_ms)
            / f64::from(effect.manifest.speed_fp8)
    }

    pub fn sample<'a>(&self, effect: &'a Projectile) -> Option<(SpriteFrame<'a>, [f64; 2])> {
        let elapsed = self.elapsed.as_secs_f64() * 1000.0;
        // Match the existing one-frame attack startup before showing the shot.
        let elapsed = elapsed - f64::from(self.tick_ms);
        if elapsed < 0.0 {
            return None;
        }
        let flight_ms = self.flight_ms(effect);
        let (animation, phase, position) = if elapsed < flight_ms {
            let dx = f64::from(self.to.x - self.from.x);
            let dy = f64::from(self.to.y - self.from.y);
            let distance = dx.hypot(dy);
            let offset = (f64::from(effect.manifest.forward_offset) / distance).min(1.0);
            let fraction = offset + (1.0 - offset) * elapsed / flight_ms;
            let phase = elapsed / flight_ms;
            let height = 4.0 * f64::from(effect.manifest.arc_height) * phase * (1.0 - phase);
            (
                &effect.flight,
                elapsed,
                [
                    f64::from(self.from.x) + dx * fraction,
                    f64::from(self.from.y) + dy * fraction - height,
                ],
            )
        } else {
            (
                &effect.impact,
                elapsed - flight_ms,
                [f64::from(self.to.x), f64::from(self.to.y)],
            )
        };
        let index = (phase / f64::from(animation.frame_ms)) as usize;
        let frame = if elapsed < flight_ms {
            animation.sequence[index % animation.sequence.len()]
        } else {
            *animation.sequence.get(index)?
        };
        let facing = facing_between(self.from, self.to);
        let directional = elapsed < flight_ms && effect.manifest.directional;
        let heading = if facing <= 16 { facing } else { 32 - facing };
        let frame = usize::from(frame) + if directional { usize::from(heading) } else { 0 };
        Some((
            SpriteFrame {
                image: animation.frames.get(frame)?,
                anchor: animation.anchor,
                flip_x: directional && facing > 16,
            },
            position,
        ))
    }
}

const MAX_DEATH_VISUALS: usize = 1024;
pub const FALLBACK_DEATH_MS: u128 = 600;

impl Visuals {
    pub fn new(world: &World) -> Self {
        Self {
            units: world
                .state()
                .entities
                .iter()
                .map(|entity| (entity.id, UnitVisual::initial(entity, world.tick().0)))
                .collect(),
            deaths: Vec::new(),
            projectiles: Vec::new(),
            tick: world.tick().0,
            finished: world.state().winner.is_some(),
            command_feedback: None,
            movement_heading_debounce_ms: 500,
        }
    }

    pub fn get(&self, id: EntityId) -> Option<&UnitVisual> {
        self.units.get(&id)
    }

    pub fn deaths(&self) -> &[DeathVisual] {
        &self.deaths
    }

    pub fn projectiles(&self) -> &[ProjectileVisual] {
        &self.projectiles
    }

    /// Deaths finish even when the last kill pauses the simulation. Ordinary
    /// unit actions still use simulation time. No wall clock enters the world.
    pub fn advance_effects(&mut self, elapsed: Duration, assets: Option<&AssetPack>) {
        self.advance_command_feedback(elapsed);
        self.projectiles.retain_mut(|shot| {
            shot.elapsed = shot.elapsed.saturating_add(elapsed);
            assets
                .and_then(|assets| assets.projectile_for(shot.unit_type, shot.targets_air))
                .is_some_and(|effect| {
                    shot.elapsed.as_secs_f64() * 1000.0
                        < f64::from(shot.tick_ms)
                            + shot.flight_ms(effect)
                            + effect.impact.sequence.len() as f64
                                * f64::from(effect.impact.frame_ms)
                })
        });
        self.deaths.retain_mut(|death| {
            death.elapsed = death.elapsed.saturating_add(elapsed);
            let duration = assets
                .and_then(|assets| assets.sprite(death.unit_type))
                .and_then(|sprite| {
                    sprite.clip(ClipKind::Death).map(|clip| {
                        (clip.frames.len() / usize::from(clip.directions)) as u128
                            * u128::from(clip.frame_ms)
                    })
                })
                .unwrap_or(FALLBACK_DEATH_MS);
            death.elapsed.as_millis() < duration
        });
    }

    /// Cancelling a foundation removes it without a combat death.
    pub fn forget_entity(&mut self, id: EntityId) {
        self.units.remove(&id);
    }

    /// Observe every completed tick, even when multiple ticks run in one frame.
    /// A redraw with no simulation progress cannot restart or advance an action.
    pub fn update(&mut self, world: &World) {
        let tick = world.tick().0;
        if tick <= self.tick {
            if tick < self.tick {
                *self = Self::new(world);
            }
            return;
        }
        let mut next = BTreeMap::new();
        for entity in &world.state().entities {
            let old = self
                .units
                .get(&entity.id)
                .copied()
                .unwrap_or_else(|| UnitVisual::initial(entity, tick));
            let mut visual = old;
            if entity.owner != old.owner {
                visual.captured_tick = Some(tick);
                visual.owner = entity.owner;
            }
            visual.position = entity.position;
            visual.hp = entity.hp;
            visual.cooldown = entity.cooldown;
            visual.burrowed = entity.burrowed;
            if old.burrowed != entity.burrowed {
                visual.burrow_changed = Some(tick);
            }
            visual.cargo_amount = entity.cargo.as_ref().map_or(0, |cargo| cargo.amount);
            visual.construction_remaining = entity.construction.as_ref().map(|work| work.remaining);
            visual.repair_progress = entity.repair_progress;
            visual.repair_target = if let UnitOrder::Repair { target } = entity.order {
                Some(target)
            } else {
                None
            };
            visual.moving = old.position != entity.position;
            visual.effect_target = None;
            visual.action = if visual.moving {
                VisualAction::Move
            } else {
                VisualAction::Idle
            };
            if visual.moving {
                let heading = facing_between(old.position, entity.position);
                let difference = heading.abs_diff(old.facing);
                let difference = difference.min(32 - difference);
                if !old.moving
                    || difference > 2
                    || tick.saturating_sub(old.facing_since_tick) * u64::from(world.rules().tick_ms)
                        >= u64::from(self.movement_heading_debounce_ms)
                {
                    visual.facing = heading;
                    if heading != old.facing || !old.moving {
                        visual.facing_since_tick = tick;
                    }
                }
            }
            if entity.hp < old.hp {
                visual.hit_tick = Some(tick);
            }
            let definition = world.unit_type(entity.unit_type).expect("validated unit");
            if entity.construction.is_none()
                && !self.finished
                && !world
                    .state()
                    .mission
                    .as_ref()
                    .is_some_and(|mission| mission.paused)
            {
                // Production art follows actual progress, including stopping when
                // a completed job is waiting for supply or an open exit.
                if world.entity_working(entity.id) {
                    visual.action = VisualAction::Production;
                }
                // Observe attack commitment rather than comparing with the base
                // duration: jitter and temporary boosts change the reset value.
                // Delayed strikes do not restart the animation when damage lands.
                if definition.weapon.is_some()
                    && entity.cooldown > 0
                    && (entity.cooldown > old.cooldown || old.cooldown <= 1)
                {
                    visual.shot_tick = Some(tick);
                    visual.effect_target = entity.last_attack_position;
                }
                if !visual.moving {
                    let work_target = match entity.order {
                        UnitOrder::Gather { resource }
                            if entity.harvest_progress > 0
                                || visual.cargo_amount > old.cargo_amount =>
                        {
                            world
                                .state()
                                .resources
                                .iter()
                                .find(|node| node.id == resource)
                                .map(|node| node.position)
                        }
                        UnitOrder::Build { building } if definition.worker.is_some() => world
                            .state()
                            .entities
                            .iter()
                            .find(|other| other.id == building)
                            .filter(|other| {
                                let before = self
                                    .units
                                    .get(&building)
                                    .and_then(|previous| previous.construction_remaining);
                                let after =
                                    other.construction.as_ref().map_or(0, |work| work.remaining);
                                before.is_some_and(|remaining| after < remaining)
                            })
                            .map(|building| building.position),
                        UnitOrder::Repair { target } => world
                            .state()
                            .entities
                            .iter()
                            .find(|other| other.id == target)
                            .filter(|other| {
                                self.units.get(&target).is_some_and(|old| other.hp > old.hp)
                                    || (old.repair_target == Some(target)
                                        && entity.repair_progress != old.repair_progress)
                            })
                            .map(|other| other.position),
                        _ => None,
                    };
                    if let Some(target) = work_target {
                        visual.action = VisualAction::Work;
                        visual.effect_target = Some(target);
                        visual.facing = facing_between(entity.position, target);
                    } else if let Some(target) = if visual.shot_tick == Some(tick) {
                        entity.last_attack_position
                    } else {
                        self.attack_target(world, entity)
                    } {
                        visual.action = VisualAction::Attack;
                        visual.effect_target = Some(target);
                        visual.facing = facing_between(entity.position, target);
                    }
                }
            }
            if visual.action != old.action
                || (old.construction_remaining.is_some() && visual.construction_remaining.is_none())
            {
                visual.since_tick = tick;
            }
            if visual.shot_tick == Some(tick)
                && let Some(to) = visual.effect_target
            {
                if self.projectiles.len() == MAX_DEATH_VISUALS {
                    self.projectiles.remove(0);
                }
                self.projectiles.push(ProjectileVisual {
                    targets_air: entity.last_attack_air,
                    unit_type: entity.unit_type,
                    owner: entity.owner,
                    from: entity.position,
                    to,
                    elapsed: Duration::ZERO,
                    tick_ms: world.rules().tick_ms,
                });
            }
            next.insert(entity.id, visual);
        }
        for (id, old) in &self.units {
            if !next.contains_key(id)
                && !world
                    .unit_type(old.unit_type)
                    .is_some_and(|unit| unit.revealer)
            {
                if self.deaths.len() == MAX_DEATH_VISUALS {
                    self.deaths.remove(0);
                }
                self.deaths.push(DeathVisual {
                    position: old.position,
                    unit_type: old.unit_type,
                    facing: old.facing,
                    elapsed: Duration::ZERO,
                });
            }
        }
        self.units = next;
        self.tick = tick;
        self.finished = world.state().winner.is_some();
    }

    fn attack_target(&self, world: &World, entity: &Entity) -> Option<Position> {
        let definition = world.unit_type(entity.unit_type)?;
        definition.weapon.as_ref()?;
        if entity.cooldown == 0 {
            return None;
        }
        let eligible = |id, other: &UnitVisual| {
            if entity.last_attack_target.is_some_and(|target| target != id) {
                return false;
            }
            let Some(target) = world.state().entities.iter().find(|e| e.id == id) else {
                return false;
            };
            let Some(weapon) = world.weapon_for(entity, target) else {
                return false;
            };
            world.can_target_entity(entity, target)
                && world.is_enemy(entity.owner, other.owner)
                && world.unit_type(other.unit_type).is_some_and(|unit| {
                    edge_distance_squared(
                        entity.position,
                        definition.footprint,
                        other.position,
                        unit.footprint,
                    ) <= i64::from(weapon.range).pow(2)
                })
                && (world.entity_visible(entity.owner, id)
                    || world
                        .state()
                        .entities
                        .iter()
                        .find(|target| target.id == id)
                        .is_some_and(|target| world.can_return_stationary_fire(entity, target)))
        };
        if let Some(target) = entity.auto_attack_target
            && let Some(other) = self
                .units
                .get(&target)
                .filter(|other| eligible(target, other))
        {
            return Some(other.position);
        }
        match entity.order {
            UnitOrder::Attack { target } => self
                .units
                .get(&target)
                .filter(|other| eligible(target, other))
                .map(|other| other.position),
            UnitOrder::Idle
            | UnitOrder::Hold
            | UnitOrder::AttackMove { .. }
            | UnitOrder::Patrol { .. } => self
                .units
                .iter()
                .filter(|(id, other)| eligible(**id, other))
                .min_by_key(|(id, other)| {
                    let dx = i64::from(entity.position.x) - i64::from(other.position.x);
                    let dy = i64::from(entity.position.y) - i64::from(other.position.y);
                    (dx * dx + dy * dy, **id)
                })
                .map(|(_, other)| other.position),
            _ => None,
        }
    }
}

fn edge_distance_squared(a: Position, af: Footprint, b: Position, bf: Footprint) -> i64 {
    let [al, at, ar, ab] = af.bounds(a);
    let [bl, bt, br, bb] = bf.bounds(b);
    let dx = (al - br).max(bl - ar).max(0);
    let dy = (at - bb).max(bt - ab).max(0);
    dx * dx + dy * dy
}

pub fn facing_between(from: Position, to: Position) -> u8 {
    let dx = f64::from(to.x) - f64::from(from.x);
    let dy = f64::from(to.y) - f64::from(from.y);
    ((dx.atan2(-dy) * 16.0 / std::f64::consts::PI).round() as i32).rem_euclid(32) as u8
}

/// Four visibly distinct construction stages; a finished structure is never sampled here.
pub fn construction_stage(entity: &Entity) -> Option<usize> {
    entity.construction.as_ref().map(|work| {
        ((u64::from(work.total.saturating_sub(work.remaining)) * 5) / u64::from(work.total.max(1)))
            .min(3) as usize
    })
}

pub struct SpriteFrame<'a> {
    pub image: &'a Image,
    pub anchor: [i32; 2],
    pub flip_x: bool,
}

/// The source splits each attachment into a small and large flame state.
/// Healthy buildings have no overlay; repair reverses the same HP thresholds.
pub fn damage_frames<'a>(
    assets: &'a AssetPack,
    entity: &Entity,
    world: &World,
    animation_ms: u128,
) -> Vec<(SpriteFrame<'a>, [i32; 2])> {
    if entity.construction.is_some() || entity.hp == 0 {
        return Vec::new();
    }
    let Some(damage) = &assets.damage_effects else {
        return Vec::new();
    };
    let Some(mapping) = damage
        .manifest
        .units
        .iter()
        .find(|mapping| mapping.unit_type == entity.unit_type)
    else {
        return Vec::new();
    };
    let maximum = u64::from(
        world
            .unit_type(entity.unit_type)
            .expect("validated unit")
            .max_hp,
    ) * 256;
    let health = u64::from(entity.hp) * 256 - u64::from(entity.damage_fraction);
    let count = mapping.spots.len();
    let states = (count * 2) as u64;
    let two_thirds = maximum - maximum / 3;
    let per_state = (two_thirds / (states + 1)).max(1);
    let state = (health as i64 - (two_thirds % per_state) as i64 - (maximum / 3) as i64 - 1).max(0)
        as u64
        / per_state;
    let lost_states = states.saturating_sub(state) as usize;
    let height = world
        .unit_type(entity.unit_type)
        .and_then(|unit| unit.flight.as_ref())
        .filter(|_| entity.airborne)
        .map_or(0, |flight| {
            if entity.flight_transition == 0 {
                42
            } else if matches!(entity.order, UnitOrder::Land { .. }) {
                (42 * entity.flight_transition / flight.land_ticks.max(1)) as i32
            } else {
                (42 * (flight.lift_ticks.saturating_sub(entity.flight_transition))
                    / flight.lift_ticks.max(1)) as i32
            }
        });
    mapping
        .spots
        .iter()
        .enumerate()
        .filter_map(|(index, spot)| {
            if lost_states <= index * 2 {
                return None;
            }
            let effect = if lost_states > index * 2 + 1 {
                &damage.large[usize::from(spot.variant)]
            } else {
                &damage.small[usize::from(spot.variant)]
            };
            let step = (animation_ms / u128::from(effect.frame_ms)
                + index as u128 * 5
                + u128::from(entity.id.0) * 3)
                % effect.sequence.len() as u128;
            Some((
                SpriteFrame {
                    image: &effect.frames[usize::from(effect.sequence[step as usize])],
                    anchor: effect.anchor,
                    flip_x: false,
                },
                [spot.offset[0], spot.offset[1] - height],
            ))
        })
        .collect()
}

pub fn gas_frames(
    assets: &AssetPack,
    unit_type: Option<UnitTypeId>,
    animation_ms: u128,
    seed: u32,
    depleted: bool,
) -> Vec<(SpriteFrame<'_>, [i32; 2])> {
    let Some(gas) = &assets.gas_effects else {
        return Vec::new();
    };
    let spots = if let Some(unit_type) = unit_type {
        let Some(unit) = gas
            .manifest
            .units
            .iter()
            .find(|unit| unit.unit_type == unit_type)
        else {
            return Vec::new();
        };
        &unit.spots
    } else {
        &gas.manifest.geyser_spots
    };
    spots
        .iter()
        .filter_map(|spot| {
            let effect = if depleted {
                &gas.depleted
            } else {
                &gas.plumes[usize::from(spot.variant)]
            };
            // Emit distinct finite puffs, with the source wait2 smoke poses.
            let step = ((animation_ms / u128::from(effect.frame_ms))
                + u128::from(seed)
                + u128::from(spot.variant) * 19)
                % 64;
            let frame = *effect.sequence.get(step as usize)?;
            Some((
                SpriteFrame {
                    image: &effect.frames[usize::from(frame)],
                    anchor: effect.anchor,
                    flip_x: false,
                },
                spot.offset,
            ))
        })
        .collect()
}

fn sample<'a>(
    sprite: &SpriteRef<'a>,
    kind: ClipKind,
    facing: u8,
    phase_ms: u128,
    construction: Option<usize>,
) -> Option<SpriteFrame<'a>> {
    let clip = sprite.clips.iter().find(|clip| clip.kind == kind)?;
    let directions = usize::from(clip.directions);
    let steps = clip.frames.len() / directions;
    let step = if let Some(stage) = construction {
        stage.min(steps - 1)
    } else {
        let elapsed = phase_ms / u128::from(clip.frame_ms);
        if matches!(
            kind,
            ClipKind::Death
                | ClipKind::Burrow
                | ClipKind::Unburrow
                | ClipKind::Lift
                | ClipKind::Land
                | ClipKind::LiftShadow
                | ClipKind::LandShadow
        ) && elapsed >= steps as u128
        {
            return None;
        }
        if kind == ClipKind::Attack && elapsed >= steps as u128 {
            return sample(sprite, ClipKind::Idle, facing, 0, None);
        }
        (elapsed % steps as u128) as usize
    };
    let direction = if directions == 1 {
        0
    } else {
        usize::from(facing % 32)
    };
    let frame = &clip.frames[step * directions + direction];
    let image = &sprite.frames[usize::from(frame.frame)];
    let mut anchor = sprite.anchor;
    if frame.flip_x {
        anchor[0] = image.width as i32 - anchor[0];
    }
    // Imported offsets already resolve the heading. Mirroring changes the
    // image anchor, not the effect's world-facing position.
    anchor[0] -= i32::from(frame.offset[0]);
    anchor[1] -= i32::from(frame.offset[1]);
    Some(SpriteFrame {
        image,
        anchor,
        flip_x: frame.flip_x,
    })
}

pub fn death_image<'a>(assets: &'a AssetPack, death: &DeathVisual) -> Option<SpriteFrame<'a>> {
    let sprite = assets.sprite(death.unit_type)?;
    sample(
        &sprite,
        ClipKind::Death,
        death.facing,
        death.elapsed.as_millis(),
        None,
    )
}

pub fn shadow_image<'a>(
    assets: &'a AssetPack,
    entity: &Entity,
    visual: Option<&UnitVisual>,
    world: &World,
) -> Option<SpriteFrame<'a>> {
    let sprite = assets.sprite(entity.unit_type)?;
    let unit = world.unit_type(entity.unit_type)?;
    if entity.airborne
        && let Some(flight) = &unit.flight
    {
        let (kind, phase) = flight_clip(entity, flight, world.rules().tick_ms);
        let kind = match kind {
            ClipKind::Lift => ClipKind::LiftShadow,
            ClipKind::Land => ClipKind::LandShadow,
            _ => ClipKind::Shadow,
        };
        return sample(
            &sprite,
            kind,
            visual.map_or(0, |visual| visual.facing),
            phase,
            None,
        )
        .or_else(|| sample(&sprite, ClipKind::Shadow, 0, 0, None));
    }
    sample(
        &sprite,
        ClipKind::Shadow,
        visual.map_or(0, |visual| visual.facing),
        0,
        None,
    )
}

fn flight_clip(
    entity: &Entity,
    flight: &straterust_engine::sim::Flight,
    tick_ms: u32,
) -> (ClipKind, u128) {
    if entity.flight_transition == 0 {
        return (ClipKind::Airborne, 0);
    }
    let landing = matches!(entity.order, UnitOrder::Land { .. });
    let duration = if landing {
        flight.land_ticks
    } else {
        flight.lift_ticks
    };
    (
        if landing {
            ClipKind::Land
        } else {
            ClipKind::Lift
        },
        u128::from(duration.saturating_sub(entity.flight_transition)) * u128::from(tick_ms),
    )
}

/// Clip selection depends on observed work/movement, not a perpetual global walk loop.
pub fn addon_connector<'a>(assets: &'a AssetPack, entity: &Entity) -> Option<SpriteFrame<'a>> {
    if entity.parent.is_none() || entity.construction.is_some() {
        return None;
    }
    sample(
        &assets.sprite(entity.unit_type)?,
        ClipKind::AddonConnector,
        0,
        0,
        None,
    )
}

pub fn unit_image<'a>(
    assets: &'a AssetPack,
    entity: &Entity,
    visual: Option<&UnitVisual>,
    world: &World,
) -> Option<SpriteFrame<'a>> {
    let sprite = assets.sprite(entity.unit_type)?;
    let unit = world.unit_type(entity.unit_type)?;
    let facing = visual.map_or(0, |visual| visual.facing);
    if entity.airborne
        && let Some(flight) = &unit.flight
    {
        let (kind, phase) = flight_clip(entity, flight, world.rules().tick_ms);
        return sample(&sprite, kind, facing, phase, None)
            .or_else(|| sample(&sprite, ClipKind::Airborne, facing, 0, None));
    }
    if let (Some(state), Some(stats)) = (&entity.mine_state, &unit.mine) {
        let (kind, phase) = match state.phase {
            MinePhase::Burrowing => (
                ClipKind::Burrow,
                u128::from(stats.burrow_ticks.saturating_sub(state.remaining))
                    * u128::from(world.rules().tick_ms),
            ),
            MinePhase::Emerging => (
                ClipKind::Unburrow,
                u128::from(stats.unburrow_ticks.saturating_sub(state.remaining))
                    * u128::from(world.rules().tick_ms),
            ),
            MinePhase::Armed => {
                let clip = sprite.clip(ClipKind::Burrow)?;
                (
                    ClipKind::Burrow,
                    (clip.frames.len() / usize::from(clip.directions) - 1) as u128
                        * u128::from(clip.frame_ms),
                )
            }
            MinePhase::Chasing if visual.is_some_and(|visual| visual.moving) => (
                ClipKind::Walk,
                u128::from(world.tick().0) * u128::from(world.rules().tick_ms),
            ),
            _ => (ClipKind::Idle, 0),
        };
        return sample(&sprite, kind, facing, phase, None);
    }
    if let Some(visual) = visual
        && let Some(tick) = visual.burrow_changed
    {
        let phase =
            u128::from(world.tick().0.saturating_sub(tick)) * u128::from(world.rules().tick_ms);
        let kind = if entity.burrowed {
            ClipKind::Burrow
        } else {
            ClipKind::Unburrow
        };
        if let Some(frame) = sample(&sprite, kind, visual.facing, phase, None) {
            return Some(frame);
        }
    }
    if entity.doodad_enabled == Some(false) {
        return sample(&sprite, ClipKind::Disabled, 0, 0, Some(usize::MAX));
    }
    if entity.burrowed {
        let clip = sprite.clip(ClipKind::Burrow)?;
        let final_step = clip.frames.len() / usize::from(clip.directions) - 1;
        return sample(
            &sprite,
            ClipKind::Burrow,
            visual.map_or(0, |visual| visual.facing),
            final_step as u128 * u128::from(clip.frame_ms),
            None,
        );
    }
    let stage = construction_stage(entity);
    if stage.is_some() {
        // A missing construction clip must use the original scaffold fallback,
        // never the finished building image.
        return sample(&sprite, ClipKind::Construction, 0, 0, stage);
    }
    let action = visual.map_or(VisualAction::Idle, |visual| visual.action);
    let kind = match action {
        VisualAction::Idle => ClipKind::Idle,
        VisualAction::Move => ClipKind::Walk,
        VisualAction::Attack => ClipKind::Attack,
        VisualAction::Work => ClipKind::Work,
        VisualAction::Production => ClipKind::Production,
    };
    let facing = visual.map_or(8, |visual| visual.facing);
    let phase = visual.map_or(0, |visual| {
        if kind == ClipKind::Attack {
            u128::from(
                world
                    .tick()
                    .0
                    .saturating_sub(visual.shot_tick.unwrap_or(world.tick().0)),
            ) * u128::from(world.rules().tick_ms)
        } else {
            visual.phase_ms(world)
        }
    });
    if !sprite.clips.is_empty() {
        return sample(&sprite, kind, facing, phase, None)
            .or_else(|| sample(&sprite, ClipKind::Idle, facing, 0, None));
    }
    // Legacy preview packs only have a walking sequence. Hold frame zero at
    // rest; no fabricated rotations of a single east-facing image.
    let index = if action == VisualAction::Move {
        (phase / u128::from(sprite.frame_ms) % sprite.frames.len() as u128) as usize
    } else {
        0
    };
    Some(SpriteFrame {
        image: &sprite.frames[index],
        anchor: sprite.anchor,
        flip_x: false,
    })
}

pub fn work_effect<'a>(
    assets: &'a AssetPack,
    entity: &Entity,
    visual: Option<&UnitVisual>,
    world: &World,
) -> Option<SpriteFrame<'a>> {
    let visual = visual.filter(|visual| visual.action == VisualAction::Work)?;
    let sprite = assets.sprite(entity.unit_type)?;
    sample(
        &sprite,
        ClipKind::WorkEffect,
        visual.facing,
        visual.phase_ms(world),
        None,
    )
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod combat_tests;
