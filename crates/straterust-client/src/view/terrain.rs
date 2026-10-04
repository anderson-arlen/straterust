use super::*;

impl<'a> View<'a> {
    pub(super) fn terrain_texture(&self) -> Option<(&'a Image, Option<&'a TerrainGrid>)> {
        self.map_art
            .map(|art| (&art.terrain, art.grid.as_ref()))
            .or_else(|| {
                self.assets
                    .map(|a| (&a.terrain, a.manifest.terrain_grid.as_ref()))
            })
    }

    pub(super) fn paint_terrain(&self, canvas: &mut Canvas<'_, 'a>, size: [f64; 2]) {
        let art = self.presentation;
        canvas.clear(0x0e1b1e);
        let top = self.camera.world_to_screen(0.0, 0.0, size);
        let map = self.world.map();
        canvas.rect(
            top[0],
            top[1],
            f64::from(map.width) * self.camera.zoom,
            f64::from(map.height) * self.camera.zoom,
            art.ground,
        );
        let texture = self.terrain_texture();
        if let Some((image, grid)) = texture {
            if let Some(grid) = grid {
                let [x0, y0, x1, y1] = self.camera.visible_tiles(grid, size);
                let atlas_columns = image.width / grid.tile_size;
                for row in y0..y1 {
                    for column in x0..x1 {
                        let tile = grid.tiles[(row * grid.columns + column) as usize];
                        let origin = self.camera.world_to_screen(
                            f64::from(column * grid.tile_size),
                            f64::from(row * grid.tile_size),
                            size,
                        );
                        canvas.image_region(
                            image,
                            origin,
                            [grid.tile_size; 2],
                            self.camera.zoom,
                            [
                                tile % atlas_columns * grid.tile_size,
                                tile / atlas_columns * grid.tile_size,
                                grid.tile_size,
                                grid.tile_size,
                            ],
                        );
                    }
                }
            } else {
                canvas.image(
                    image,
                    top,
                    [map.width as u32, map.height as u32],
                    self.camera.zoom,
                );
            }
        } else {
            for x in (0..=map.width).step_by(64) {
                let p = self.camera.world_to_screen(f64::from(x), 0.0, size);
                canvas.rect(
                    p[0],
                    p[1],
                    1.0,
                    f64::from(map.height) * self.camera.zoom,
                    art.grid,
                );
            }
            for y in (0..=map.height).step_by(64) {
                let p = self.camera.world_to_screen(0.0, f64::from(y), size);
                canvas.rect(
                    p[0],
                    p[1],
                    f64::from(map.width) * self.camera.zoom,
                    1.0,
                    art.grid,
                );
            }
        }
        if texture.is_none_or(|(_, grid)| grid.is_none())
            && let Some(terrain) = &map.terrain
        {
            let bounds = self.camera.visible_world(size);
            let cell = f64::from(terrain.cell_size);
            let x0 = (bounds[0] / cell)
                .floor()
                .clamp(0.0, f64::from(terrain.columns)) as u32;
            let y0 = (bounds[1] / cell)
                .floor()
                .clamp(0.0, f64::from(terrain.rows)) as u32;
            let x1 = (bounds[2] / cell)
                .ceil()
                .clamp(0.0, f64::from(terrain.columns)) as u32;
            let y1 = (bounds[3] / cell)
                .ceil()
                .clamp(0.0, f64::from(terrain.rows)) as u32;
            for y in y0..y1 {
                for x in x0..x1 {
                    if terrain.flags[(y * terrain.columns + x) as usize]
                        & straterust_engine::map::WALKABLE
                        == 0
                    {
                        let p = self.camera.world_to_screen(
                            f64::from(x * terrain.cell_size),
                            f64::from(y * terrain.cell_size),
                            size,
                        );
                        let extent = cell * self.camera.zoom;
                        canvas.rect(p[0], p[1], extent, extent, 0x39464d);
                        canvas.outline(
                            p[0] + 1.0,
                            p[1] + 1.0,
                            (extent - 2.0).max(1.0),
                            (extent - 2.0).max(1.0),
                            0x617078,
                        );
                    }
                }
            }
        }
    }
}
