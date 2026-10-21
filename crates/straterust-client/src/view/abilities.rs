use super::*;

impl<'a> View<'a> {
    pub(super) fn paint_abilities(&self, canvas: &mut Canvas<'_, 'a>, size: [f64; 2]) {
        self.paint_strikes(canvas, size);
        let Some(assets) = self.assets else { return };
        for field in &self.world.state().ability_fields {
            if let Some(effect) = assets
                .projectiles
                .iter()
                .find(|p| p.manifest.ability == Some(field.ability))
            {
                let duration = self
                    .world
                    .rules()
                    .units
                    .iter()
                    .flat_map(|u| &u.abilities)
                    .find(|a| a.id == field.ability)
                    .map_or(field.remaining, |a| a.effect.duration());
                let heading = if effect.manifest.directional {
                    crate::visual::facing_between(
                        Position { x: 0, y: 0 },
                        Position {
                            x: i32::from(field.velocity[0]),
                            y: i32::from(field.velocity[1]),
                        },
                    )
                } else {
                    0
                };
                self.ability_effect(
                    canvas,
                    effect,
                    &effect.flight,
                    u64::from(duration.saturating_sub(field.remaining))
                        * u64::from(self.world.rules().tick_ms),
                    field.position,
                    size,
                    heading,
                    true,
                );
            }
        }
        for entity in &self.world.state().entities {
            if !self
                .world
                .entity_visible(self.world.view_player(), entity.id)
                || entity.garrisoned_in.is_some()
            {
                continue;
            }
            if let Some(cast) = &entity.last_cast
                && let Some(effect) = assets
                    .projectiles
                    .iter()
                    .find(|p| p.manifest.ability == Some(cast.ability))
            {
                (|| {
                    if self
                        .world
                        .targeted_ability(entity.unit_type, cast.ability)
                        .is_some_and(|a| {
                            matches!(
                                a.effect,
                                straterust_engine::sim::AbilityEffect::GroundEffect { .. }
                                    | straterust_engine::sim::AbilityEffect::Strike {
                                        delivery: Some(_),
                                        ..
                                    }
                                    | straterust_engine::sim::AbilityEffect::DrainLife {
                                        delivery: Some(_),
                                        ..
                                    }
                            )
                        })
                    {
                        return;
                    }
                    let mut elapsed = self.visuals.get(entity.id).map_or_else(
                        || {
                            self.world.tick().0.saturating_sub(cast.tick.0)
                                * u64::from(self.world.rules().tick_ms)
                        },
                        |v| v.phase_since(self.world, cast.tick.0) as u64,
                    );
                    if effect.manifest.on_target {
                        self.ability_effect(
                            canvas,
                            effect,
                            &effect.impact,
                            elapsed,
                            cast.position,
                            size,
                            0,
                            false,
                        );
                        return;
                    }
                    let dx = f64::from(cast.position.x - cast.origin.x);
                    let dy = f64::from(cast.position.y - cast.origin.y);
                    let flight_ms = ((dx * dx + dy * dy).sqrt() * 256.0
                        / f64::from(effect.manifest.speed_fp8)
                        * f64::from(self.world.rules().tick_ms))
                    .max(1.0) as u64;
                    if let Some(straterust_engine::sim::AbilityEffect::Strike { delay, .. }) = self
                        .world
                        .targeted_ability(entity.unit_type, cast.ability)
                        .map(|a| &a.effect)
                    {
                        let start = (u64::from(*delay) * u64::from(self.world.rules().tick_ms))
                            .saturating_sub(flight_ms);
                        if elapsed < start {
                            return;
                        }
                        elapsed -= start;
                    }
                    if elapsed < flight_ms {
                        let fraction = elapsed as f64 / flight_ms as f64;
                        self.ability_effect(
                            canvas,
                            effect,
                            &effect.flight,
                            elapsed,
                            Position {
                                x: (f64::from(cast.origin.x) + dx * fraction).round() as i32,
                                y: (f64::from(cast.origin.y) + dy * fraction).round() as i32,
                            },
                            size,
                            if effect.manifest.directional {
                                crate::visual::facing_between(cast.origin, cast.position)
                            } else {
                                0
                            },
                            true,
                        );
                    } else if self
                        .world
                        .terrain_visibility(self.world.view_player(), cast.position)
                        == Visibility::Visible
                    {
                        self.ability_effect(
                            canvas,
                            effect,
                            &effect.impact,
                            elapsed - flight_ms,
                            cast.position,
                            size,
                            0,
                            false,
                        );
                    }
                })();
            }
            let mut status_index = 0;
            for aura in &entity.ability_auras {
                if self
                    .world
                    .rules()
                    .units
                    .iter()
                    .flat_map(|u| &u.abilities)
                    .any(|a| {
                        a.id == aura.ability
                            && matches!(a.effect, straterust_engine::sim::AbilityEffect::Parasite)
                    })
                {
                    continue;
                }
                let Some(effect) = assets
                    .projectiles
                    .iter()
                    .find(|p| p.manifest.ability == Some(aura.ability))
                else {
                    continue;
                };
                if let Some(marker) = &effect.marker {
                    let unit = self.world.unit_type(entity.unit_type).unwrap();
                    self.strike_sprite(
                        canvas,
                        marker,
                        0,
                        Position {
                            x: entity.position.x - i32::from(unit.footprint.width / 2)
                                + 8
                                + status_index * 16,
                            y: entity.position.y - i32::from(unit.footprint.height / 2) + 8,
                        },
                        size,
                        0,
                        true,
                    );
                    status_index += 1;
                }
                let duration = self
                    .world
                    .rules()
                    .units
                    .iter()
                    .flat_map(|u| &u.abilities)
                    .find(|a| a.id == aura.ability)
                    .map(|a| a.effect.duration())
                    .unwrap_or(aura.remaining);
                let elapsed = u64::from(duration.saturating_sub(aura.remaining))
                    * u64::from(self.world.rules().tick_ms);
                self.ability_effect(
                    canvas,
                    effect,
                    if effect.manifest.on_target {
                        &effect.flight
                    } else {
                        &effect.impact
                    },
                    elapsed,
                    entity.position,
                    size,
                    0,
                    true,
                );
            }
        }
    }
}

impl<'a> View<'a> {
    #[allow(clippy::too_many_arguments)]
    fn ability_effect(
        &self,
        canvas: &mut Canvas<'_, 'a>,
        projectile: &'a straterust_engine::assets::Projectile,
        sprite: &'a straterust_engine::assets::Effect,
        elapsed: u64,
        point: Position,
        size: [f64; 2],
        heading: u8,
        repeat: bool,
    ) {
        for (position, phase, impact) in placements(
            if repeat {
                projectile.manifest.pattern.as_ref()
            } else {
                None
            },
            elapsed,
            point,
        ) {
            self.strike_sprite(
                canvas,
                if impact { &projectile.impact } else { sprite },
                phase,
                position,
                size,
                if impact { 0 } else { heading },
                repeat && !impact,
            );
        }
    }
}

fn placements(
    pattern: Option<&straterust_engine::assets::EffectPattern>,
    elapsed: u64,
    point: Position,
) -> Vec<(Position, u64, bool)> {
    use straterust_engine::assets::EffectPattern;
    match pattern {
        None => vec![(point, elapsed, false)],
        Some(EffectPattern::Orbit {
            count,
            radius,
            period_ms,
            phase_degrees,
            spacing_degrees,
        }) => (0..*count)
            .map(|i| {
                let spacing = if *spacing_degrees == 0 {
                    360.0 / f64::from(*count)
                } else {
                    f64::from(*spacing_degrees)
                };
                let angle = std::f64::consts::TAU
                    * (elapsed as f64 / f64::from(*period_ms)
                        + (f64::from(*phase_degrees) + f64::from(i) * spacing) / 360.0);
                (
                    Position {
                        x: point.x + (angle.cos() * f64::from(*radius)).round() as i32,
                        y: point.y + (angle.sin() * f64::from(*radius)).round() as i32,
                    },
                    elapsed
                        + (f64::from(i) * spacing.abs() * f64::from(*period_ms) / 360.0).round()
                            as u64,
                    false,
                )
            })
            .collect(),
        Some(EffectPattern::Bombardment {
            count,
            spread,
            fall_height,
            fall_ms,
            cycle_ms,
        }) => (0..*count)
            .filter_map(|i| {
                let delay = u64::from(*cycle_ms) * u64::from(i) / u64::from(*count);
                let time = elapsed.checked_sub(delay)?;
                let wave = time / u64::from(*cycle_ms);
                let phase = time % u64::from(*cycle_ms);
                let seed = (point.x as u32).wrapping_mul(2654435761)
                    ^ point.y as u32
                    ^ u32::from(i).wrapping_mul(2246822519)
                    ^ (wave as u32).wrapping_mul(3266489917);
                let span = u32::from(*spread) * 2 + 1;
                let impact = *fall_height > 0 && phase >= u64::from(*fall_ms);
                let remaining = if *fall_ms == 0 || impact {
                    0.0
                } else {
                    f64::from(*fall_height) * (1.0 - phase as f64 / f64::from(*fall_ms))
                };
                Some((
                    Position {
                        x: point.x + (seed % span) as i32
                            - i32::from(*spread)
                            - remaining.round() as i32,
                        y: point.y + (seed.rotate_left(13) % span) as i32
                            - i32::from(*spread)
                            - remaining.round() as i32,
                    },
                    if impact {
                        phase - u64::from(*fall_ms)
                    } else {
                        phase
                    },
                    impact,
                ))
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use straterust_engine::assets::EffectPattern;
    #[test]
    fn flame_shield_orbits_and_bombardment_falls_in_staggered_scattered_waves() {
        let point = Position { x: 512, y: 512 };
        let orbit = EffectPattern::Orbit {
            count: 3,
            radius: 32,
            period_ms: 1600,
            phase_degrees: 0,
            spacing_degrees: 0,
        };
        let first = placements(Some(&orbit), 0, point);
        assert_eq!(first.len(), 3);
        assert_eq!(first[0].0, Position { x: 544, y: 512 });
        assert_ne!(first[0].0, placements(Some(&orbit), 400, point)[0].0);
        let rain = EffectPattern::Bombardment {
            count: 6,
            spread: 64,
            fall_height: 128,
            fall_ms: 330,
            cycle_ms: 495,
        };
        assert_eq!(placements(Some(&rain), 0, point).len(), 1);
        let early = placements(Some(&rain), 100, point)[0].0;
        let late = placements(Some(&rain), 450, point)[0].0;
        assert!(late.y > early.y && late.x > early.x);
        assert_eq!(placements(Some(&rain), 450, point).len(), 6);
        assert_ne!(
            placements(Some(&rain), 450, point),
            placements(Some(&rain), 945, point)
        );
    }
}
