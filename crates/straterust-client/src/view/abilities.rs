use super::*;

impl<'a> View<'a> {
    pub(super) fn paint_abilities(&self, canvas: &mut Canvas<'_, 'a>, size: [f64; 2]) {
        self.paint_strikes(canvas, size);
        let Some(assets) = self.assets else { return };
        let draw = |canvas: &mut Canvas<'_, 'a>,
                    effect: &'a straterust_engine::assets::Effect,
                    elapsed: u64,
                    p: [f64; 2],
                    repeat: bool| {
            let mut index = (elapsed / u64::from(effect.frame_ms)) as usize;
            if repeat {
                index %= effect.sequence.len();
            }
            if let Some(frame) = effect.sequence.get(index) {
                let image = &effect.frames[usize::from(*frame)];
                let p = self.camera.world_to_screen(p[0], p[1], size);
                canvas.image(
                    image,
                    [
                        p[0] - f64::from(effect.anchor[0]) * self.camera.zoom,
                        p[1] - f64::from(effect.anchor[1]) * self.camera.zoom,
                    ],
                    [image.width, image.height],
                    self.camera.zoom,
                );
            }
        };
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
                draw(
                    canvas,
                    &effect.flight,
                    u64::from(duration.saturating_sub(field.remaining))
                        * u64::from(self.world.rules().tick_ms),
                    [f64::from(field.position.x), f64::from(field.position.y)],
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
                                straterust_engine::sim::AbilityEffect::Strike {
                                    delivery: Some(_),
                                    ..
                                }
                            )
                        })
                    {
                        return;
                    }
                    let mut elapsed = self.world.tick().0.saturating_sub(cast.tick.0)
                        * u64::from(self.world.rules().tick_ms);
                    if effect.manifest.on_target {
                        if !entity
                            .ability_auras
                            .iter()
                            .any(|a| a.ability == cast.ability)
                        {
                            draw(
                                canvas,
                                &effect.impact,
                                elapsed,
                                [f64::from(cast.position.x), f64::from(cast.position.y)],
                                false,
                            );
                        }
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
                        draw(
                            canvas,
                            &effect.flight,
                            elapsed,
                            [
                                f64::from(cast.origin.x) + dx * fraction,
                                f64::from(cast.origin.y) + dy * fraction,
                            ],
                            true,
                        );
                    } else if self
                        .world
                        .terrain_visibility(self.world.view_player(), cast.position)
                        == Visibility::Visible
                    {
                        draw(
                            canvas,
                            &effect.impact,
                            elapsed - flight_ms,
                            [f64::from(cast.position.x), f64::from(cast.position.y)],
                            false,
                        );
                    }
                })();
            }
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
                draw(
                    canvas,
                    if effect.manifest.on_target {
                        &effect.flight
                    } else {
                        &effect.impact
                    },
                    elapsed,
                    [f64::from(entity.position.x), f64::from(entity.position.y)],
                    true,
                );
            }
        }
    }
}
