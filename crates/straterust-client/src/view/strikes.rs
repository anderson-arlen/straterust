use super::*;
use straterust_engine::{
    assets::{Effect, ProjectileTrailManifest},
    sim::{StrikeAppearance, StrikeDelivery, StrikeStage},
};

impl<'a> View<'a> {
    pub(super) fn paint_strikes(&self, canvas: &mut Canvas<'_, 'a>, size: [f64; 2]) {
        let Some(assets) = self.assets else { return };
        for strike in self.world.strike_appearances(self.world.view_player()) {
            let Some(projectile) = assets
                .projectiles
                .iter()
                .find(|p| p.manifest.ability == Some(strike.ability))
            else {
                continue;
            };
            let facing = crate::visual::facing_between(
                Position { x: 0, y: 0 },
                Position {
                    x: i32::from(strike.heading[0]),
                    y: i32::from(strike.heading[1]),
                },
            );
            if let Some(charge) = &projectile.charge
                && let Some(entity) = strike
                    .caster
                    .and_then(|id| self.world.state().entities.iter().find(|e| e.id == id))
            {
                let heading = self.visuals.get(entity.id).map_or(facing, |v| v.facing);
                let offset = projectile
                    .manifest
                    .launch_offsets
                    .get(usize::from(heading))
                    .copied()
                    .unwrap_or([0, 0]);
                self.strike_sprite(
                    canvas,
                    charge,
                    self.world.tick().0.saturating_sub(strike.started.0)
                        * u64::from(self.world.rules().tick_ms),
                    Position {
                        x: entity.position.x + i32::from(offset[0]),
                        y: entity.position.y + i32::from(offset[1]),
                    },
                    size,
                    0,
                    false,
                );
            }
            let elapsed = u64::from(strike.elapsed) * u64::from(self.world.rules().tick_ms);
            if let Some(point) = strike.marker
                && let Some(marker) = &projectile.marker
            {
                self.strike_sprite(
                    canvas,
                    marker,
                    self.world.tick().0 * u64::from(self.world.rules().tick_ms),
                    point,
                    size,
                    0,
                    true,
                );
            }
            let (Some(mut point), Some(stage)) = (strike.position, strike.stage) else {
                continue;
            };
            if stage == StrikeStage::Flight
                && let Some(offset) = projectile.manifest.launch_offsets.get(usize::from(facing))
            {
                point = launch_position(
                    point,
                    strike.heading,
                    *offset,
                    strike.elapsed,
                    projectile.manifest.speed_fp8,
                );
            }
            let (sprite, repeat, heading) = match stage {
                StrikeStage::Charge => (None, false, 0),
                StrikeStage::Ascent => (Some(&projectile.flight), true, 0),
                StrikeStage::Flight => (
                    Some(&projectile.flight),
                    true,
                    if projectile.manifest.directional {
                        facing
                    } else {
                        0
                    },
                ),
                StrikeStage::Impact => (Some(&projectile.impact), false, 0),
                StrikeStage::Transit => (None, false, 0),
            };
            if matches!(stage, StrikeStage::Ascent | StrikeStage::Flight)
                && let (Some(trail), Some(animation)) =
                    (&projectile.manifest.trail, &projectile.trail)
                && let Some(straterust_engine::sim::AbilityEffect::Strike {
                    delivery: Some(delivery),
                    ..
                }) = self
                    .world
                    .rules()
                    .units
                    .iter()
                    .flat_map(|u| &u.abilities)
                    .find(|a| a.id == strike.ability)
                    .map(|a| &a.effect)
            {
                let lifetime = animation.sequence.len() as u64 * u64::from(animation.frame_ms);
                for (age, behind) in exhaust_samples(
                    &strike,
                    trail,
                    lifetime,
                    self.world.rules().tick_ms,
                    delivery,
                ) {
                    if self
                        .world
                        .terrain_visibility(self.world.view_player(), behind)
                        != Visibility::Visible
                    {
                        continue;
                    }
                    self.strike_sprite(
                        canvas,
                        animation,
                        age,
                        behind,
                        size,
                        if trail.directional { facing } else { 0 },
                        false,
                    );
                }
            }
            // Source sprul emissions are underlays: the missile body covers
            // exhaust that overlaps its engine, rather than being painted over.
            if let Some(sprite) = sprite {
                self.strike_sprite(canvas, sprite, elapsed, point, size, heading, repeat);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn strike_sprite(
        &self,
        canvas: &mut Canvas<'_, 'a>,
        effect: &'a Effect,
        elapsed: u64,
        point: Position,
        size: [f64; 2],
        heading: u8,
        repeat: bool,
    ) {
        let mut index = (elapsed / u64::from(effect.frame_ms)) as usize;
        if repeat {
            index %= effect.sequence.len();
        }
        if let Some(frame) = effect.sequence.get(index)
            && let Some(image) = effect.frames.get(
                usize::from(*frame)
                    + usize::from(if heading > 16 { 32 - heading } else { heading }),
            )
        {
            let p = self
                .camera
                .world_to_screen(f64::from(point.x), f64::from(point.y), size);
            let anchor_x = if heading > 16 {
                image.width as i32 - effect.anchor[0]
            } else {
                effect.anchor[0]
            };
            canvas.image_mirrored(
                image,
                [
                    p[0] - f64::from(anchor_x) * self.camera.zoom,
                    p[1] - f64::from(effect.anchor[1]) * self.camera.zoom,
                ],
                [image.width, image.height],
                self.camera.zoom,
                heading > 16,
            );
        }
    }
}

/// Keep the first visible projectile at the muzzle until its flight has cleared
/// the hull. A one-frame offset would jump backward toward the center next tick.
fn launch_position(
    point: Position,
    heading: [i16; 2],
    offset: [i16; 2],
    elapsed: u32,
    speed: u32,
) -> Position {
    let length = f64::from(heading[0]).hypot(f64::from(heading[1])).max(1.0);
    let forward = (f64::from(offset[0]) * f64::from(heading[0])
        + f64::from(offset[1]) * f64::from(heading[1]))
        / length;
    let travelled = f64::from(elapsed) * f64::from(speed) / 256.0;
    let remaining = (1.0 - travelled / forward.max(1.0)).clamp(0.0, 1.0);
    Position {
        x: point.x + (f64::from(offset[0]) * remaining).round() as i32,
        y: point.y + (f64::from(offset[1]) * remaining).round() as i32,
    }
}

/// Emit at fixed times and integrate the original acceleration between each
/// emission and the visible missile. Puffs stay at their emission positions;
/// their animation advances on every tick, rather than restarting every redraw.
fn exhaust_samples(
    strike: &StrikeAppearance,
    trail: &ProjectileTrailManifest,
    lifetime: u64,
    tick_ms: u32,
    delivery: &StrikeDelivery,
) -> Vec<(u64, Position)> {
    let Some(point) = strike.position else {
        return Vec::new();
    };
    let elapsed = u64::from(strike.elapsed) * u64::from(tick_ms);
    let start = u64::from(trail.start_ms);
    if elapsed < start || lifetime == 0 {
        return Vec::new();
    }
    let interval = u64::from(trail.interval_ms);
    let earliest = elapsed.saturating_sub(lifetime - 1);
    let first = earliest.saturating_sub(start).div_ceil(interval);
    let last = (elapsed - start) / interval;
    let length = f64::from(strike.heading[0])
        .hypot(f64::from(strike.heading[1]))
        .max(1.0);
    let distance_at = |tick: u64| {
        let acceleration = u64::from(delivery.acceleration_fp8);
        let speed = u64::from(delivery.speed_fp8);
        let accelerating = tick.min(speed.checked_div(acceleration).unwrap_or(tick));
        acceleration * accelerating * (accelerating + 1) / 2 + (tick - accelerating) * speed
    };
    (first..=last)
        .take(64)
        .map(|i| {
            let emitted = start + i * interval;
            let distance = (distance_at(u64::from(strike.elapsed))
                - distance_at(emitted / u64::from(tick_ms))) as f64
                / 256.0
                + f64::from(trail.rear_offset);
            (
                elapsed - emitted,
                Position {
                    x: point.x - (distance * f64::from(strike.heading[0]) / length).round() as i32,
                    y: point.y - (distance * f64::from(strike.heading[1]) / length).round() as i32,
                },
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn muzzle_launch_never_jumps_backward_into_the_hull() {
        let x: Vec<_> = (0..4)
            .map(|tick| {
                launch_position(
                    Position {
                        x: 100 + (tick * 8533 / 256) as i32,
                        y: 100,
                    },
                    [128, 0],
                    [42, 0],
                    tick,
                    8533,
                )
                .x
            })
            .collect();
        assert_eq!(x[0], 142);
        assert!(x.windows(2).all(|p| p[1] >= p[0]));
        assert_eq!(
            x[3], 199,
            "ordinary flight resumes after clearing the muzzle"
        );
    }

    #[test]
    #[ignore = "requires STRATERUST_CAMPAIGNS pointing to a refreshed retail import"]
    fn retail_yamato_charge_and_fireball_follow_the_ship_heading() {
        use straterust_engine::sim::{AbilityId, CastAppearance, Spawn, Tick, ViewedEntity};
        let directory = std::path::PathBuf::from(std::env::var_os("STRATERUST_CAMPAIGNS").unwrap())
            .join("terran09");
        let package = straterust_engine::content::Package::load(&directory)
            .unwrap()
            .world(42)
            .unwrap();
        let mut map = package.map().clone();
        map.fog_of_war = false;
        map.mission = None;
        map.ai.clear();
        map.terrain = None;
        map.resources.clear();
        map.spawns = vec![Spawn {
            unit_type: UnitTypeId(109),
            owner: PlayerId(0),
            position: Position { x: 512, y: 512 },
            ..Default::default()
        }];
        let base = World::new(package.rules().clone(), map, 42).unwrap();
        let assets = AssetPack::load(&directory).unwrap().unwrap();
        let presentation = Presentation::default();
        let selected = BTreeSet::new();
        let camera = Camera {
            x: 512.0,
            y: 512.0,
            zoom: 1.5,
        };
        let mut sheet = vec![0x203020; 1024 * 864];
        for (cell, heading) in [0, 4, 8, 12, 16, 20, 24, 28].into_iter().enumerate() {
            let angle = f64::from(heading) * std::f64::consts::PI / 16.0;
            let direction = [
                (angle.sin() * 128.0).round() as i16,
                (-angle.cos() * 128.0).round() as i16,
            ];
            let mut public = base.player_view(PlayerId(0)).unwrap();
            public.tick = Tick(20);
            if let ViewedEntity::Owned(actor) = &mut public.entities[0] {
                actor.last_cast = Some(CastAppearance {
                    ability: AbilityId(4),
                    tick: Tick(0),
                    origin: actor.position,
                    position: Position {
                        x: 512 + i32::from(direction[0]),
                        y: 512 + i32::from(direction[1]),
                    },
                });
            }
            public.strikes = vec![StrikeAppearance {
                ability: AbilityId(4),
                started: Tick(0),
                stage: Some(StrikeStage::Charge),
                elapsed: 20,
                position: Some(Position { x: 512, y: 512 }),
                marker: None,
                caster: Some(EntityId(1)),
                heading: direction,
                velocity_fp8: 0,
                warning: false,
            }];
            let world = public.into_world(&base).unwrap();
            let mut visuals = Visuals::new(&base);
            visuals.update(&world);
            assert_eq!(visuals.get(EntityId(1)).unwrap().facing, heading);
            let view = View {
                world: &world,
                visuals: &visuals,
                presentation: &presentation,
                cursor: [-1.0, -1.0],
                targeting: false,
                assets: Some(&assets),
                map_art: None,
                media: None,
                speaking: None,
                mission: None,
                animation_ms: 0,
                portrait_ms: 0,
                camera,
                selected: &selected,
                selected_resource: None,
                drag_box: None,
                paused: false,
                playback: false,
                status: "",
                buttons: &[],
                help: "",
                placement: None,
                placement_type: None,
                ending_hint: "",
            };
            let mut pixels = vec![0x203020; 256 * 432];
            let mut canvas = Canvas {
                scene: None,
                pixels: &mut pixels,
                width: 256,
                height: 432,
                scale: 1.0,
            };
            let actor = &world.state().entities[0];
            let body = visual::unit_image(&assets, actor, visuals.get(actor.id), &world).unwrap();
            let center = camera.world_to_screen(512.0, 512.0, [256.0, 432.0]);
            canvas.image_mirrored(
                body.image,
                [
                    center[0] - f64::from(body.anchor[0]) * 1.5,
                    center[1] - f64::from(body.anchor[1]) * 1.5,
                ],
                [body.image.width, body.image.height],
                1.5,
                body.flip_x,
            );
            view.paint_strikes(&mut canvas, [256.0, 432.0]);
            let projectile = assets
                .projectiles
                .iter()
                .find(|p| p.manifest.ability == Some(AbilityId(4)))
                .unwrap();
            // Show the moving fireball and its original directional trail below
            // the charge capture, at the same heading as the ship.
            let point = Position { x: 512, y: 650 };
            view.strike_sprite(
                &mut canvas,
                projectile.trail.as_ref().unwrap(),
                42,
                point,
                [256.0, 432.0],
                heading,
                false,
            );
            view.strike_sprite(
                &mut canvas,
                &projectile.flight,
                0,
                point,
                [256.0, 432.0],
                heading,
                true,
            );
            for row in 0..432 {
                let at = ((cell / 4) * 432 + row) * 1024 + (cell % 4) * 256;
                sheet[at..at + 256].copy_from_slice(&pixels[row * 256..(row + 1) * 256]);
            }
        }
        let mut ppm = b"P6\n1024 864\n255\n".to_vec();
        for pixel in sheet {
            ppm.extend([(pixel >> 16) as u8, (pixel >> 8) as u8, pixel as u8]);
        }
        std::fs::write("/tmp/stratarust-effects.ppm", ppm).unwrap();
    }

    #[test]
    #[ignore = "requires STRATERUST_CAMPAIGNS pointing to a refreshed retail import"]
    fn retail_nuclear_exhaust_renders_behind_both_missile_headings() {
        let directory = std::path::PathBuf::from(std::env::var_os("STRATERUST_CAMPAIGNS").unwrap())
            .join("terran09");
        let base = straterust_engine::content::Package::load(&directory)
            .unwrap()
            .world(42)
            .unwrap();
        let mut map = base.map().clone();
        map.fog_of_war = false;
        let base = World::new(base.rules().clone(), map, 42).unwrap();
        let mut public = base.player_view(PlayerId(0)).unwrap();
        public.strikes = [
            (176, StrikeStage::Ascent, -1),
            (336, StrikeStage::Flight, 1),
        ]
        .map(|(x, stage, direction)| StrikeAppearance {
            ability: straterust_engine::sim::AbilityId(6),
            started: straterust_engine::sim::Tick(0),
            stage: Some(stage),
            elapsed: 50,
            position: Some(Position { x, y: 256 }),
            marker: None,
            caster: None,
            heading: [0, direction],
            velocity_fp8: 1650,
            warning: false,
        })
        .into();
        let world = public.into_world(&base).unwrap();
        let visuals = Visuals::new(&world);
        let presentation = Presentation::default();
        let selected = BTreeSet::new();
        let mut assets = AssetPack::load(&directory).unwrap().unwrap();
        let camera = Camera {
            x: 256.0,
            y: 256.0,
            zoom: 3.0,
        };
        let render = |assets: &AssetPack| {
            let view = View {
                world: &world,
                visuals: &visuals,
                cursor: [-1.0, -1.0],
                targeting: false,
                presentation: &presentation,
                assets: Some(assets),
                map_art: None,
                media: None,
                speaking: None,
                mission: None,
                animation_ms: 0,
                portrait_ms: 0,
                camera,
                selected: &selected,
                selected_resource: None,
                drag_box: None,
                paused: false,
                playback: false,
                status: "",
                buttons: &[],
                help: "",
                placement: None,
                placement_type: None,
                ending_hint: "",
            };
            let mut pixels = vec![0x203020; 800 * 600];
            view.paint_strikes(
                &mut Canvas {
                    scene: None,
                    pixels: &mut pixels,
                    width: 800,
                    height: 600,
                    scale: 1.0,
                },
                [800.0, 600.0],
            );
            pixels
        };
        let exhaust = render(&assets);
        let projectile = assets
            .projectiles
            .iter_mut()
            .find(|p| p.manifest.ability == Some(straterust_engine::sim::AbilityId(6)))
            .unwrap();
        projectile.trail = None;
        let body = render(&assets);
        for (x, direction) in [(176, -1), (336, 1)] {
            let center = camera.world_to_screen(f64::from(x), 256.0, [800.0, 600.0]);
            let changed: Vec<_> = exhaust
                .iter()
                .zip(&body)
                .enumerate()
                .filter(|(i, (a, b))| a != b && ((*i % 800) as f64 - center[0]).abs() < 45.0)
                .map(|(i, _)| i / 800)
                .collect();
            assert!(changed.len() > 50, "missing visible smoke trail");
            assert!(
                changed
                    .iter()
                    .all(|y| (*y as f64 - center[1]) * f64::from(direction) < -15.0),
                "exhaust must emerge behind the engine, never at the missile center"
            );
        }
        let mut ppm = b"P6\n800 600\n255\n".to_vec();
        for pixel in exhaust {
            ppm.extend([(pixel >> 16) as u8, (pixel >> 8) as u8, pixel as u8]);
        }
        std::fs::write("/tmp/stratarust-effects.ppm", ppm).unwrap();
    }

    #[test]
    fn accelerating_exhaust_stays_at_the_engine_emission_point_and_ages() {
        let delivery = StrikeDelivery {
            charge_ticks: 0,
            ascent_ticks: 90,
            warning_ticks: 45,
            transit_ticks: 250,
            descent_height: 320,
            speed_fp8: 8533,
            acceleration_fp8: 256,
            impact_ticks: 52,
            reveal_radius: 96,
        };
        let trail = ProjectileTrailManifest {
            directional: false,
            start_ms: 0,
            interval_ms: 126,
            rear_offset: 10,
            effect: straterust_engine::assets::EffectManifest {
                frame_ms: 42,
                anchor: [0, 0],
                frames: Vec::new(),
                sequence: Vec::new(),
            },
        };
        for (stage, direction) in [(StrikeStage::Ascent, -1), (StrikeStage::Flight, 1)] {
            let mut strike = StrikeAppearance {
                ability: straterust_engine::sim::AbilityId(6),
                started: straterust_engine::sim::Tick(0),
                stage: Some(stage),
                elapsed: 5,
                position: Some(Position {
                    x: 100,
                    y: 100 + direction * 15,
                }),
                marker: None,
                caster: None,
                heading: [0, direction as i16],
                velocity_fp8: 5 * 256,
                warning: false,
            };
            let before = exhaust_samples(&strike, &trail, 462, 42, &delivery);
            // First puff remains ten pixels behind the initial missile position.
            assert_eq!(
                before[0],
                (
                    210,
                    Position {
                        x: 100,
                        y: 100 - direction * 10
                    }
                )
            );
            strike.elapsed = 6;
            strike.position.as_mut().unwrap().y += direction * 6;
            let after = exhaust_samples(&strike, &trail, 462, 42, &delivery);
            assert_eq!(
                after[0].1, before[0].1,
                "puffs must not drift with acceleration"
            );
            assert_eq!(
                after[0].0,
                before[0].0 + 42,
                "puffs must advance their animation"
            );
            assert_eq!(after[1].1, before[1].1);
        }
    }
}
