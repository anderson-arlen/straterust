use super::*;

impl<'a> View<'a> {
    pub(super) fn paint(&self, mut canvas: Canvas<'_, 'a>, width: u32, height: u32, scale: f64) {
        let size = [f64::from(width) / scale, f64::from(height) / scale];
        let art = self.presentation;
        let map = self.world.map();
        self.paint_terrain(&mut canvas, size);
        // Deaths and persistent remains lie on the terrain, beneath living units
        // and world objects regardless of their ground position.
        if let Some(creep) = self.assets.and_then(|assets| assets.creep.as_ref()) {
            let columns = (map.width as u32).div_ceil(32);
            let rows = (map.height as u32).div_ceil(32);
            const NEIGHBORS: [(i32, i32); 8] = [
                (1, 1),
                (0, 1),
                (-1, 1),
                (1, 0),
                (-1, 0),
                (1, -1),
                (0, -1),
                (-1, -1),
            ];
            for y in 0..rows {
                for x in 0..columns {
                    let center = Position {
                        x: x as i32 * 32 + 16,
                        y: y as i32 * 32 + 16,
                    };
                    if self.world.visibility(self.world.view_player(), center)
                        == Visibility::Unexplored
                    {
                        continue;
                    }
                    let image = if self.world.known_creep(self.world.view_player(), x, y) {
                        let seed = (x.wrapping_mul(374761393) ^ y.wrapping_mul(668265263))
                            .wrapping_mul(1274126177);
                        let variant = if seed % 100 < 4 {
                            6 + seed / 100 % 7
                        } else {
                            seed / 100 % 6
                        };
                        Some(&creep.tiles[variant as usize])
                    } else {
                        let mut mask = 0_usize;
                        for (bit, (dx, dy)) in NEIGHBORS.into_iter().enumerate() {
                            let nx = x as i32 + dx;
                            let ny = y as i32 + dy;
                            if nx >= 0
                                && ny >= 0
                                && self.world.known_creep(
                                    self.world.view_player(),
                                    nx as u32,
                                    ny as u32,
                                )
                            {
                                mask |= 1 << bit;
                            }
                        }
                        let frame = creep.mask_frames[mask];
                        frame
                            .checked_sub(1)
                            .map(|frame| &creep.edges[usize::from(frame)])
                    };
                    if let Some(image) = image {
                        let point =
                            self.camera
                                .world_to_screen(f64::from(x * 32), f64::from(y * 32), size);
                        canvas.image(image, point, [32, 32], self.camera.zoom);
                    }
                }
            }
        }
        self.draw_coverage(&mut canvas, size);
        for death in self.visuals.deaths().iter().filter(|death| {
            self.world
                .visibility(self.world.view_player(), death.position)
                == Visibility::Visible
        }) {
            let p = self.camera.world_to_screen(
                f64::from(death.position.x),
                f64::from(death.position.y),
                size,
            );
            if let Some(frame) = self
                .assets
                .and_then(|assets| visual::death_image(assets, death))
            {
                canvas.image_mirrored(
                    frame.image,
                    [
                        p[0] - f64::from(frame.anchor[0]) * self.camera.zoom,
                        p[1] - f64::from(frame.anchor[1]) * self.camera.zoom,
                    ],
                    [frame.image.width, frame.image.height],
                    self.camera.zoom,
                    frame.flip_x,
                );
            } else {
                // Original geometric feedback also works without proprietary art.
                let phase = (death.elapsed.as_secs_f64() * 1000.0
                    / visual::FALLBACK_DEATH_MS as f64)
                    .min(1.0);
                let radius = (4.0 + phase * 15.0) * self.camera.zoom;
                let fragment = (5.0 * (1.0 - phase)).max(1.0) * self.camera.zoom;
                let color = if phase < 0.35 {
                    0xffefac
                } else if phase < 0.7 {
                    0xe49a50
                } else {
                    0x765e48
                };
                for [dx, dy] in [[-1.0, -0.5], [1.0, -0.5], [-0.5, 1.0], [0.5, 1.0]] {
                    canvas.rect(
                        p[0] + dx * radius - fragment / 2.0,
                        p[1] + dy * radius - fragment / 2.0,
                        fragment,
                        fragment,
                        color,
                    );
                }
            }
        }
        for resource in self.world.state().resources.iter().filter(|resource| {
            (resource.amount > 0 || resource.requires_extractor)
                && self
                    .world
                    .visibility(self.world.view_player(), resource.position)
                    != Visibility::Unexplored
                && !self.world.state().entities.iter().any(|entity| {
                    entity.position == resource.position
                        && self.world.unit_type(entity.unit_type).is_some_and(|unit| {
                            unit.extracts
                                .as_ref()
                                .is_some_and(|extractor| extractor.resource == resource.kind)
                        })
                })
        }) {
            let p = self.camera.world_to_screen(
                f64::from(resource.position.x),
                f64::from(resource.position.y),
                size,
            );
            let r = 10.0 * self.camera.zoom;
            if self.selected_resource == Some(resource.id)
                || self.visuals.command_feedback().is_some_and(|feedback| {
                    feedback.target == visual::CommandTarget::Resource(resource.id)
                        && feedback.visible()
                })
            {
                self.draw_resource_circle(&mut canvas, resource, p);
            }
            if let Some(art) = self.assets.and_then(|assets| {
                assets
                    .resources
                    .iter()
                    .find(|art| art.manifest.kind == resource.kind)
            }) {
                canvas.image(
                    &art.image,
                    [
                        p[0] - f64::from(art.manifest.anchor[0]) * self.camera.zoom,
                        p[1] - f64::from(art.manifest.anchor[1]) * self.camera.zoom,
                    ],
                    [art.image.width, art.image.height],
                    self.camera.zoom,
                );
                if resource.kind == "gas"
                    && self
                        .world
                        .visibility(self.world.view_player(), resource.position)
                        == Visibility::Visible
                {
                    for (frame, offset) in visual::gas_frames(
                        self.assets.unwrap(),
                        None,
                        self.animation_ms,
                        resource.id.0,
                        resource.amount == 0,
                    ) {
                        canvas.image(
                            frame.image,
                            [
                                p[0] + f64::from(offset[0] - frame.anchor[0]) * self.camera.zoom,
                                p[1] + f64::from(offset[1] - frame.anchor[1]) * self.camera.zoom,
                            ],
                            [frame.image.width, frame.image.height],
                            self.camera.zoom,
                        );
                    }
                }
            } else if resource.kind == "gas" {
                canvas.outline(p[0] - r, p[1] - r, 2.0 * r, 2.0 * r, 0xa3d17a);
                canvas.rect(p[0] - r * 0.6, p[1] - r * 0.6, r * 1.2, r * 1.2, 0x567a4b);
            } else {
                for (offset, height) in [(-0.9, 1.0), (-0.2, 1.7), (0.5, 1.2)] {
                    canvas.rect(
                        p[0] + r * offset,
                        p[1] + r * (0.8 - height),
                        r * 0.55,
                        r * height,
                        0x70bddb,
                    );
                }
            }
            if map.mission.is_none()
                && self
                    .world
                    .visibility(self.world.view_player(), resource.position)
                    == Visibility::Visible
            {
                canvas.text(
                    &resource.amount.to_string(),
                    p[0] - r,
                    p[1] + r + 3.0,
                    1.0,
                    0xb4c9cb,
                );
            }
        }
        for start in map
            .start_locations
            .iter()
            .filter(|_| !self.world.rules().victory && map.mission.is_none())
        {
            let p = self.camera.world_to_screen(
                f64::from(start.position.x),
                f64::from(start.position.y),
                size,
            );
            let r = 24.0 * self.camera.zoom;
            let color = if start.player.0 == 0 {
                art.friendly
            } else {
                art.opposing
            };
            canvas.outline(p[0] - r, p[1] - r, r * 2.0, r * 2.0, color);
            canvas.text(
                &format!("START {}", start.player.0 + 1),
                p[0] - r,
                p[1] - r - 12.0,
                1.0,
                color,
            );
        }
        let mut map_images: Vec<_> = self
            .map_art
            .map(|art| &art.decorations)
            .or_else(|| self.assets.map(|a| &a.map_images))
            .into_iter()
            .flatten()
            .collect();
        map_images.sort_by_key(|image| (image.position.y, image.position.x));
        let mut map_images = map_images.into_iter().peekable();
        let mut entities: Vec<_> = self
            .world
            .state()
            .entities
            .iter()
            .filter(|entity| {
                self.world
                    .entity_visible(self.world.view_player(), entity.id)
            })
            .collect();
        entities.sort_by_key(|entity| {
            (
                self.world.movement_class(entity) == straterust_engine::sim::MovementClass::Air,
                entity.position.y,
                entity.position.x,
                entity.id,
            )
        });
        // All shadows precede all bodies, so a later flying entity cannot shade
        // a ground unit or another aircraft. Visibility uses the same filter.
        for entity in &entities {
            if self.world.movement_class(entity) != straterust_engine::sim::MovementClass::Air {
                continue;
            }
            let p = self.camera.world_to_screen(
                f64::from(entity.position.x),
                f64::from(entity.position.y),
                size,
            );
            if let Some(frame) = self.assets.and_then(|assets| {
                visual::shadow_image(assets, entity, self.visuals.get(entity.id), self.world)
            }) {
                let draw = if entity.cloaked
                    && self
                        .assets
                        .and_then(|a| a.sprite(entity.unit_type))
                        .is_none_or(|sprite| {
                            sprite
                                .clip(straterust_engine::assets::ClipKind::Conceal)
                                .is_none()
                        }) {
                    Canvas::image_cloaked
                } else {
                    Canvas::image_mirrored
                };
                draw(
                    &mut canvas,
                    frame.image,
                    [
                        p[0] - f64::from(frame.anchor[0]) * self.camera.zoom,
                        p[1] - f64::from(frame.anchor[1]) * self.camera.zoom,
                    ],
                    [frame.image.width, frame.image.height],
                    self.camera.zoom,
                    frame.flip_x,
                );
            } else {
                let half = unit_half_size(self.world, entity.unit_type, art)
                    .map(|value| value * self.camera.zoom);
                let ground_y = p[1]
                    + if !entity.airborne
                        && self
                            .assets
                            .and_then(|assets| assets.indicators.as_ref())
                            .is_some()
                    {
                        42.0 * self.camera.zoom
                    } else {
                        0.0
                    };
                canvas.image_stretched(
                    &FLYING_SHADOW,
                    [
                        p[0] - half[0],
                        ground_y - half[1] / 2.0,
                        half[0] * 2.0,
                        half[1],
                    ],
                    0xffffff,
                );
            }
        }
        for entity in entities {
            while map_images.peek().is_some_and(|image| {
                (image.position.y, image.position.x) <= (entity.position.y, entity.position.x)
            }) {
                self.draw_map_image(&mut canvas, size, map_images.next().unwrap());
            }
            let p = self.camera.world_to_screen(
                f64::from(entity.position.x),
                f64::from(entity.position.y),
                size,
            );
            let p = if entity.carried_by.is_some() {
                [p[0], p[1] - 18.0 * self.camera.zoom]
            } else {
                p
            };
            let half_size = unit_half_size(self.world, entity.unit_type, art);
            let [half_width, half_height] = half_size.map(|half| half * self.camera.zoom);
            let color = if entity.owner.0 == 0 {
                art.friendly
            } else if self.world.is_enemy(self.world.view_player(), entity.owner) {
                art.opposing
            } else {
                0x7db7df
            };
            let observed = self.visuals.get(entity.id);
            if self.selected.contains(&entity.id)
                || self.visuals.command_feedback().is_some_and(|feedback| {
                    feedback.target == visual::CommandTarget::Entity(entity.id)
                        && feedback.visible()
                })
                || observed.is_some_and(|visual| {
                    visual.captured_tick.is_some_and(|tick| {
                        let elapsed = self.world.tick().0.saturating_sub(tick);
                        elapsed < 72 && (elapsed / 6).is_multiple_of(2)
                    })
                })
            {
                self.draw_selection_circle(&mut canvas, entity, p, [half_width, half_height]);
                if let Some(target) = entity.target.filter(|_| {
                    self.assets
                        .and_then(|assets| assets.indicators.as_ref())
                        .is_none()
                }) {
                    let target =
                        self.camera
                            .world_to_screen(f64::from(target.x), f64::from(target.y), size);
                    canvas.outline(target[0] - 6.0, target[1] - 6.0, 12.0, 12.0, color);
                    canvas.rect(target[0] - 2.0, target[1] - 2.0, 4.0, 4.0, color);
                }
            }
            if let Some(frame) = self
                .assets
                .and_then(|assets| visual::unit_image(assets, entity, observed, self.world))
            {
                let bob = if self.world.movement_class(entity)
                    == straterust_engine::sim::MovementClass::Air
                    && entity.flight_transition == 0
                    && observed.is_some_and(|visual| visual.action == VisualAction::Idle)
                {
                    // Retail common aircraft idle loop: setvertpos 1,2,1,0,-1,-2,-1,0
                    // with waitrand 8..10. Stable per-entity phase avoids lockstep motion.
                    [1.0, 2.0, 1.0, 0.0, -1.0, -2.0, -1.0, 0.0][(((u128::from(
                        self.world.tick().0,
                    ) * u128::from(
                        self.world.rules().tick_ms,
                    )) / 378
                        + u128::from(entity.id.0) * 3)
                        % 8)
                        as usize]
                        * self.camera.zoom
                } else {
                    0.0
                };
                let draw = if entity.cloaked
                    && self
                        .assets
                        .and_then(|a| a.sprite(entity.unit_type))
                        .is_none_or(|sprite| {
                            sprite
                                .clip(straterust_engine::assets::ClipKind::Conceal)
                                .is_none()
                        }) {
                    Canvas::image_cloaked
                } else {
                    Canvas::image_mirrored
                };
                draw(
                    &mut canvas,
                    frame.image,
                    [
                        p[0] - f64::from(frame.anchor[0]) * self.camera.zoom,
                        p[1] - f64::from(frame.anchor[1]) * self.camera.zoom + bob,
                    ],
                    [frame.image.width, frame.image.height],
                    self.camera.zoom,
                    frame.flip_x,
                );
            } else if let Some(stage) = visual::construction_stage(entity) {
                // A foundation and growing scaffold never imply a finished building.
                canvas.rect(
                    p[0] - half_width,
                    p[1] - half_height,
                    half_width * 2.0,
                    half_height * 2.0,
                    0x403b2d,
                );
                canvas.outline(
                    p[0] - half_width,
                    p[1] - half_height,
                    half_width * 2.0,
                    half_height * 2.0,
                    0xb69956,
                );
                for post in 0..=stage + 1 {
                    let x = p[0] - half_width
                        + 4.0
                        + post as f64 * (half_width * 2.0 - 8.0) / (stage + 1) as f64;
                    canvas.rect(x, p[1] - half_height, 3.0, half_height * 2.0, 0xd0b977);
                }
                for beam in 0..=stage {
                    let y = p[1] + half_height - beam as f64 * (half_height * 2.0) / 3.0;
                    canvas.rect(p[0] - half_width, y, half_width * 2.0, 3.0, 0xd0b977);
                }
            } else {
                canvas.rect(
                    p[0] - half_width + 3.0,
                    p[1] - half_height + 4.0,
                    half_width * 2.0,
                    half_height * 2.0,
                    0x101f21,
                );
                canvas.rect(
                    p[0] - half_width,
                    p[1] - half_height,
                    half_width * 2.0,
                    half_height * 2.0,
                    color,
                );
                canvas.rect(
                    p[0] - half_width + 3.0,
                    p[1] - half_height + 3.0,
                    half_width * 2.0 - 6.0,
                    3.0,
                    0xe2eee4,
                );
                canvas.rect(p[0] - 3.0, p[1] - 1.0, 6.0, 5.0, 0x213b3a);

                if observed.is_some_and(|visual| visual.action == VisualAction::Production) {
                    canvas.rect(
                        p[0] + half_width - 9.0,
                        p[1] - half_height + 8.0,
                        5.0,
                        5.0,
                        if (self.animation_ms / 200).is_multiple_of(2) {
                            0xffdc79
                        } else {
                            0x776437
                        },
                    );
                }

                if let Some(visual) = observed {
                    let angle = f64::from(visual.facing) * std::f64::consts::TAU / 32.0;
                    canvas.rect(
                        p[0] + angle.sin() * half_width * 0.8 - 2.0,
                        p[1] - angle.cos() * half_height * 0.8 - 2.0,
                        4.0,
                        4.0,
                        0xf2e9bd,
                    );
                }
            }
            if let Some(assets) = self.assets {
                if let Some(frame) =
                    visual::carried_resource_frame(assets, entity, observed, self.world)
                {
                    canvas.image_mirrored(
                        frame.image,
                        [
                            p[0] - f64::from(frame.anchor[0]) * self.camera.zoom,
                            p[1] - f64::from(frame.anchor[1]) * self.camera.zoom,
                        ],
                        [frame.image.width, frame.image.height],
                        self.camera.zoom,
                        frame.flip_x,
                    );
                }
                if let Some(frame) = visual::addon_connector(assets, entity) {
                    canvas.image(
                        frame.image,
                        [
                            p[0] - f64::from(frame.anchor[0]) * self.camera.zoom,
                            p[1] - f64::from(frame.anchor[1]) * self.camera.zoom,
                        ],
                        [frame.image.width, frame.image.height],
                        self.camera.zoom,
                    );
                }
                if entity.construction.is_none() {
                    for (frame, offset) in visual::gas_frames(
                        assets,
                        Some(entity.unit_type),
                        self.animation_ms,
                        entity.id.0,
                        self.world.state().resources.iter().any(|resource| {
                            resource.position == entity.position && resource.amount == 0
                        }),
                    ) {
                        canvas.image(
                            frame.image,
                            [
                                p[0] + f64::from(offset[0] - frame.anchor[0]) * self.camera.zoom,
                                p[1] + f64::from(offset[1] - frame.anchor[1]) * self.camera.zoom,
                            ],
                            [frame.image.width, frame.image.height],
                            self.camera.zoom,
                        );
                    }
                }
                for (frame, offset) in
                    visual::damage_frames(assets, entity, self.world, self.animation_ms)
                {
                    canvas.image(
                        frame.image,
                        [
                            p[0] + f64::from(offset[0] - frame.anchor[0]) * self.camera.zoom,
                            p[1] + f64::from(offset[1] - frame.anchor[1]) * self.camera.zoom,
                        ],
                        [frame.image.width, frame.image.height],
                        self.camera.zoom,
                    );
                }
                for frame in visual::garrison_frames(assets, self.world, self.visuals, entity) {
                    canvas.image_mirrored(
                        frame.image,
                        [
                            p[0] - f64::from(frame.anchor[0]) * self.camera.zoom,
                            p[1] - f64::from(frame.anchor[1]) * self.camera.zoom,
                        ],
                        [frame.image.width, frame.image.height],
                        self.camera.zoom,
                        frame.flip_x,
                    );
                }
            }
            let work_image = self
                .assets
                .and_then(|assets| visual::work_effect(assets, entity, observed, self.world));
            if let Some(frame) = work_image {
                let draw = if entity.cloaked
                    && self
                        .assets
                        .and_then(|a| a.sprite(entity.unit_type))
                        .is_none_or(|sprite| {
                            sprite
                                .clip(straterust_engine::assets::ClipKind::Conceal)
                                .is_none()
                        }) {
                    Canvas::image_cloaked
                } else {
                    Canvas::image_mirrored
                };
                draw(
                    &mut canvas,
                    frame.image,
                    [
                        p[0] - f64::from(frame.anchor[0]) * self.camera.zoom,
                        p[1] - f64::from(frame.anchor[1]) * self.camera.zoom,
                    ],
                    [frame.image.width, frame.image.height],
                    self.camera.zoom,
                    frame.flip_x,
                );
            } else if let Some(visual) = observed.filter(|v| {
                v.action == VisualAction::Work
                    && self
                        .assets
                        .and_then(|a| a.sprite(entity.unit_type))
                        .is_none_or(|s| s.clip(straterust_engine::assets::ClipKind::Work).is_none())
            }) {
                let angle = f64::from(visual.facing) * std::f64::consts::TAU / 32.0;
                let tip = [
                    p[0] + angle.sin() * (half_width + 6.0),
                    p[1] - angle.cos() * (half_height + 6.0),
                ];
                let bright = (self.animation_ms / 90).is_multiple_of(2);
                canvas.rect(
                    tip[0] - 2.0,
                    tip[1] - 2.0,
                    5.0,
                    5.0,
                    if bright { 0xc6ecff } else { 0x729dde },
                );
                if bright {
                    canvas.rect(tip[0] - 6.0, tip[1], 13.0, 1.0, 0xe5f5ff);
                    canvas.rect(tip[0], tip[1] - 6.0, 1.0, 13.0, 0xe5f5ff);
                }
            }
            if let Some(visual) = observed {
                if visual
                    .shot_tick
                    .is_some_and(|tick| self.world.tick().0.saturating_sub(tick) < 3)
                    && self
                        .assets
                        .and_then(|assets| {
                            assets.projectile_for(entity.unit_type, entity.last_attack_air)
                        })
                        .is_none()
                    && !self
                        .assets
                        .and_then(|assets| assets.sprite(entity.unit_type))
                        .is_some_and(|sprite| {
                            sprite
                                .clips
                                .iter()
                                .any(|clip| clip.kind == ClipKind::Attack)
                        })
                {
                    let angle = f64::from(visual.facing) * std::f64::consts::TAU / 32.0;
                    let tip = [
                        p[0] + angle.sin() * (half_width + 4.0),
                        p[1] - angle.cos() * (half_height + 4.0),
                    ];
                    canvas.rect(tip[0] - 3.0, tip[1] - 3.0, 7.0, 7.0, 0xffdc79);
                    canvas.rect(tip[0] - 1.0, tip[1] - 1.0, 3.0, 3.0, 0xffffe3);
                }
                if visual
                    .hit_tick
                    .is_some_and(|tick| self.world.tick().0.saturating_sub(tick) < 3)
                    && self
                        .assets
                        .and_then(|assets| assets.indicators.as_ref())
                        .is_none()
                {
                    canvas.outline(
                        p[0] - half_width,
                        p[1] - half_height,
                        half_width * 2.0,
                        half_height * 2.0,
                        0xffdfc5,
                    );
                }
            }
            if !self.world.rules().victory && map.mission.is_none() {
                canvas.text(
                    &format!("{:02}", entity.id.0),
                    p[0] - 8.0,
                    p[1] + half_height + 10.0,
                    1.0,
                    color,
                );
            }
            self.draw_unit_bars(&mut canvas, entity, p, [half_width, half_height]);
            if let Some(construction) = &entity.construction {
                let width = (2.0 * half_width).clamp(20.0, 100.0);
                let progress =
                    1.0 - f64::from(construction.remaining) / f64::from(construction.total.max(1));
                canvas.rect(
                    p[0] - width / 2.0,
                    p[1] + half_height + 4.0,
                    width,
                    3.0,
                    0x213238,
                );
                canvas.rect(
                    p[0] - width / 2.0,
                    p[1] + half_height + 4.0,
                    width * progress,
                    3.0,
                    0x80c9ef,
                );
            }
            if let Some(rally) = entity.rally.filter(|_| self.selected.contains(&entity.id)) {
                let mark =
                    self.camera
                        .world_to_screen(f64::from(rally.x), f64::from(rally.y), size);
                canvas.rect(mark[0], mark[1] - 12.0, 1.0, 16.0, color);
                canvas.rect(mark[0] + 1.0, mark[1] - 12.0, 8.0, 5.0, color);
            }
        }
        for image in map_images {
            self.draw_map_image(&mut canvas, size, image);
        }
        if let Some(assets) = self.assets {
            for shot in self.visuals.projectiles() {
                let effect = assets.projectile_for(shot.unit_type, shot.targets_air);
                let hit = effect.and_then(|effect| shot.sample(effect));
                let trail = effect.map_or_else(Vec::new, |effect| shot.trail_samples(effect));
                for (frame, position) in trail
                    .into_iter()
                    .chain(shot.launch_frame(assets))
                    .chain(hit)
                {
                    if shot.owner != self.world.view_player() {
                        let position = Position {
                            x: position[0] as i32,
                            y: position[1] as i32,
                        };
                        let visibility = if shot.targets_air {
                            self.world
                                .terrain_visibility(self.world.view_player(), position)
                        } else {
                            self.world.visibility(self.world.view_player(), position)
                        };
                        if visibility != Visibility::Visible {
                            continue;
                        }
                    }
                    let p = self.camera.world_to_screen(position[0], position[1], size);
                    canvas.image_mirrored(
                        frame.image,
                        [
                            p[0] - f64::from(frame.anchor[0]) * self.camera.zoom,
                            p[1] - f64::from(frame.anchor[1]) * self.camera.zoom,
                        ],
                        [frame.image.width, frame.image.height],
                        self.camera.zoom,
                        frame.flip_x,
                    );
                }
            }
        }
        self.paint_abilities(&mut canvas, size);
        if let Some(effect) = self.assets.and_then(|assets| assets.scan_effect.as_ref()) {
            let duration = effect.sequence.len() as u64 * u64::from(effect.frame_ms);
            for scan in self
                .world
                .state()
                .scans
                .iter()
                .filter(|scan| scan.owner == self.world.view_player())
            {
                let elapsed = duration.saturating_sub(
                    u64::from(scan.remaining) * u64::from(self.world.rules().tick_ms),
                );
                if let Some(index) = effect
                    .sequence
                    .get((elapsed / u64::from(effect.frame_ms)) as usize)
                {
                    let image = &effect.frames[usize::from(*index)];
                    let p = self.camera.world_to_screen(
                        f64::from(scan.position.x),
                        f64::from(scan.position.y),
                        size,
                    );
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
            }
        }
        self.draw_fog(&mut canvas, size);
        self.draw_command_feedback(&mut canvas, size);
        if let Some((unit_type, position, valid)) = self.placement {
            let unit = self
                .world
                .rules()
                .units
                .iter()
                .find(|unit| unit.id == unit_type)
                .unwrap();
            let p = self
                .camera
                .world_to_screen(f64::from(position.x), f64::from(position.y), size);
            let width = f64::from(unit.placement.width) * self.camera.zoom;
            let height = f64::from(unit.placement.height) * self.camera.zoom;
            let color = if valid { 0x9ee878 } else { 0xff7777 };
            if let Some(parent) = unit.addon_parent.and_then(|id| self.world.unit_type(id))
                && let Some(origin) = self.world.addon_parent_position(unit_type, position)
            {
                let p = self
                    .camera
                    .world_to_screen(f64::from(origin.x), f64::from(origin.y), size);
                let width = f64::from(parent.placement.width) * self.camera.zoom;
                let height = f64::from(parent.placement.height) * self.camera.zoom;
                canvas.outline(
                    p[0] - width / 2.0,
                    p[1] - height / 2.0,
                    width,
                    height,
                    color,
                );
            }
            canvas.outline(
                p[0] - width / 2.0,
                p[1] - height / 2.0,
                width,
                height,
                color,
            );
            for col in 1..(unit.placement.width / BUILD_GRID as u16) {
                canvas.rect(
                    p[0] - width / 2.0 + f64::from(col) * f64::from(BUILD_GRID) * self.camera.zoom,
                    p[1] - height / 2.0,
                    1.0,
                    height,
                    color,
                );
            }
            for row in 1..(unit.placement.height / BUILD_GRID as u16) {
                canvas.rect(
                    p[0] - width / 2.0,
                    p[1] - height / 2.0 + f64::from(row) * f64::from(BUILD_GRID) * self.camera.zoom,
                    width,
                    1.0,
                    color,
                );
            }
            canvas.text(
                if valid {
                    "CLICK TO BUILD"
                } else {
                    "CANNOT BUILD HERE"
                },
                p[0] - width / 2.0,
                p[1] + height / 2.0 + 5.0,
                1.0,
                color,
            );
        }
        if let Some([start, end]) = self.drag_box {
            canvas.outline(
                start[0].min(end[0]),
                start[1].min(end[1]),
                (end[0] - start[0]).abs().max(1.0),
                (end[1] - start[1]).abs().max(1.0),
                0xd6f7a4,
            );
        }
        self.draw_hud(&mut canvas, size);
        self.draw_mission(&mut canvas, size);
        let ended = if self
            .world
            .state()
            .defeated
            .contains(&self.world.view_player())
        {
            Some("DEFEAT")
        } else {
            self.world.state().winner.map(|winner| {
                if winner == self.world.view_player() {
                    "VICTORY"
                } else {
                    "DEFEAT"
                }
            })
        };
        if let Some(outcome) = ended {
            let [x, y, _, _] = ending_rect(size);
            canvas.rect(x, y, 360.0, 70.0, 0x12272a);
            canvas.outline(x, y, 360.0, 70.0, art.friendly);
            canvas.text(outcome, x + 24.0, y + 12.0, 2.0, 0xe2eee4);
            canvas.text(self.ending_hint, x + 24.0, y + 46.0, 1.0, art.friendly);
        }
        self.draw_cursor(&mut canvas, size);
    }
}

impl<'a> View<'a> {
    pub(super) fn draw_map_image(
        &self,
        canvas: &mut Canvas<'_, 'a>,
        size: [f64; 2],
        image: &'a straterust_engine::assets::MapImage,
    ) {
        let p = self.camera.world_to_screen(
            f64::from(image.position.x),
            f64::from(image.position.y),
            size,
        );
        canvas.image(
            &image.image,
            [
                p[0] - f64::from(image.anchor[0]) * self.camera.zoom,
                p[1] - f64::from(image.anchor[1]) * self.camera.zoom,
            ],
            [image.image.width, image.image.height],
            self.camera.zoom,
        );
    }
}

pub(super) fn selection_color(world: &World, owner: PlayerId) -> u32 {
    if owner == world.view_player() {
        0x00ff00
    } else if world.is_enemy(world.view_player(), owner) {
        0xff0000
    } else {
        0xffff00
    }
}
