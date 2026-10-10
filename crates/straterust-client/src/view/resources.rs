//! Resource inspection shares world picking and the existing selection console.
use super::*;
use straterust_engine::sim::ResourceNode;

impl<'a> View<'a> {
    pub(super) fn paint_terrain_resources(&self, canvas: &mut Canvas<'_, 'a>, size: [f64; 2]) {
        let Some(assets) = self.assets else {
            return;
        };
        if let Some((atlas, Some(grid))) = self.terrain_texture() {
            for art in &assets.resources {
                let Some(edges) = &art.manifest.terrain_edges else {
                    continue;
                };
                let cleared = self
                    .world
                    .state()
                    .resources
                    .iter()
                    .filter(|node| {
                        node.amount == 0
                            && node.kind == art.manifest.kind
                            && (art.manifest.positions.is_empty()
                                || art.manifest.positions.contains(&node.position))
                    })
                    .map(|node| {
                        (
                            node.position.x / grid.tile_size as i32,
                            node.position.y / grid.tile_size as i32,
                        )
                    })
                    .collect();
                let columns = atlas.width / grid.tile_size;
                let count = columns * (atlas.height / grid.tile_size);
                for ((x, y), tile) in super::resource_edges::replacements(grid, edges, &cleared) {
                    let position = self.camera.world_to_screen(
                        f64::from(x) * f64::from(grid.tile_size),
                        f64::from(y) * f64::from(grid.tile_size),
                        size,
                    );
                    let Some(tile) = tile else {
                        // Unsupported fragments disappear with their remaining
                        // resource. Reconnect remembered edges the same way.
                        if let Some(image) = &art.depleted_image {
                            canvas.image(image, position, [grid.tile_size; 2], self.camera.zoom);
                        }
                        continue;
                    };
                    if tile >= count {
                        continue;
                    }
                    canvas.image_region(
                        atlas,
                        position,
                        [grid.tile_size; 2],
                        self.camera.zoom,
                        [
                            tile % columns * grid.tile_size,
                            tile / columns * grid.tile_size,
                            grid.tile_size,
                            grid.tile_size,
                        ],
                    );
                }
            }
        }
        // Initial harvestable terrain is part of map art, including downloaded
        // maps. Only disclosed/remembered depletion changes it. Never clear an
        // unknown tree before alpha fog is composited over its original artwork.
        for node in self
            .world
            .state()
            .resources
            .iter()
            .filter(|node| node.amount == 0)
        {
            let Some(art) = assets.resources.iter().find(|art| {
                art.manifest.terrain
                    && art.manifest.kind == node.kind
                    && (art.manifest.positions.is_empty()
                        || art.manifest.positions.contains(&node.position))
            }) else {
                continue;
            };
            let Some(image) = &art.depleted_image else {
                continue;
            };
            canvas.image(
                image,
                self.camera.world_to_screen(
                    f64::from(node.position.x - art.manifest.anchor[0]),
                    f64::from(node.position.y - art.manifest.anchor[1]),
                    size,
                ),
                [image.width, image.height],
                self.camera.zoom,
            );
        }
    }

    pub(super) fn paint_resources(&self, canvas: &mut Canvas<'_, 'a>, size: [f64; 2]) {
        let map = self.world.map();
        for resource in self.world.state().resources.iter().filter(|resource| {
            self.world
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
            let art = self.assets.and_then(|assets| {
                assets.resources.iter().find(|art| {
                    art.manifest.kind == resource.kind
                        && (art.manifest.positions.is_empty()
                            || art.manifest.positions.contains(&resource.position))
                })
            });
            if resource.amount == 0
                && !resource.requires_extractor
                && art.is_none_or(|art| art.depleted_image.is_none())
            {
                continue;
            }
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
                self.draw_resource_circle(canvas, resource, p);
            }
            if let Some(art) = art {
                // Terrain resources were already drawn beneath remains and units.
                if art.manifest.terrain {
                    continue;
                }
                let image = if resource.amount == 0 {
                    art.depleted_image.as_ref().unwrap_or(&art.image)
                } else if self.world.resource_working(resource.id)
                    && self
                        .world
                        .visibility(self.world.view_player(), resource.position)
                        == Visibility::Visible
                {
                    art.active_image.as_ref().unwrap_or(&art.image)
                } else {
                    &art.image
                };
                canvas.image(
                    image,
                    [
                        p[0] - f64::from(art.manifest.anchor[0]) * self.camera.zoom,
                        p[1] - f64::from(art.manifest.anchor[1]) * self.camera.zoom,
                    ],
                    [image.width, image.height],
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
    }

    pub(super) fn draw_resource_circle(
        &self,
        canvas: &mut Canvas<'_, 'a>,
        resource: &ResourceNode,
        p: [f64; 2],
    ) {
        let source = self.assets.and_then(|assets| {
            let pack = assets.indicators.as_ref()?;
            let art = assets
                .resources
                .iter()
                .find(|art| art.manifest.kind == resource.kind)?;
            let circle = art.manifest.selection_circle?;
            Some((
                pack.circles.get(usize::from(circle))?,
                art.manifest.selection_y,
            ))
        });
        if let Some((image, offset)) = source {
            let height = image.height / 3;
            canvas.image_region(
                image,
                [
                    p[0] - f64::from(image.width) * self.camera.zoom / 2.0,
                    p[1] + (f64::from(offset) - f64::from(height) / 2.0) * self.camera.zoom,
                ],
                [image.width, height],
                self.camera.zoom,
                [0, height, image.width, height],
            );
        } else {
            canvas.selection_circle(
                p,
                [
                    f64::from(resource.footprint.width.max(24)) * self.camera.zoom / 2.0 + 4.0,
                    f64::from(resource.footprint.height.max(12)) * self.camera.zoom / 2.0 + 2.0,
                ],
                0xffff00,
            );
        }
    }

    pub(super) fn draw_resource_details(
        &self,
        canvas: &mut Canvas<'_, 'a>,
        rect: [f64; 4],
        scale: f64,
    ) -> bool {
        let Some(resource) = self.world.state().resources.iter().find(|resource| {
            self.selected_resource == Some(resource.id)
                && (resource.amount > 0 || resource.requires_extractor)
                && self
                    .world
                    .visibility(self.world.view_player(), resource.position)
                    != Visibility::Unexplored
        }) else {
            return false;
        };
        let name = match resource.kind.as_str() {
            "minerals" => "MINERAL FIELD",
            "gas" => "VESPENE GEYSER",
            _ => resource.kind.as_str(),
        };
        let [x, y, width, _] = rect;
        canvas.text(
            &shorten(name, (width / 8.0) as usize),
            x + 8.0,
            y + 5.0,
            1.0,
            0xd3d8bf,
        );
        let mut detail_x = x + 8.0;
        if let Some(art) = self.assets.and_then(|assets| {
            assets
                .resources
                .iter()
                .find(|art| art.manifest.kind == resource.kind)
        }) {
            let zoom = (64.0 * scale / f64::from(art.image.width))
                .min(56.0 * scale / f64::from(art.image.height));
            canvas.image(
                &art.image,
                [x + 4.0 * scale, y + 23.0 * scale],
                [art.image.width, art.image.height],
                zoom,
            );
            detail_x = x + 76.0 * scale;
        }
        canvas.text("REMAINING", detail_x, y + 28.0 * scale, 1.0, 0x9aab98);
        canvas.text(
            &resource.amount.to_string(),
            detail_x,
            y + 46.0 * scale,
            1.0,
            0xffff00,
        );
        if resource.amount == 0 && resource.requires_extractor {
            canvas.text("DEPLETED", detail_x, y + 64.0 * scale, 1.0, 0x9aab98);
        }
        true
    }
}
