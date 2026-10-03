use super::*;

impl<'a> View<'a> {
    pub(super) fn draw_minimap(&self, canvas: &mut Canvas<'_, 'a>, size: [f64; 2]) {
        let map = self.world.map();
        let [x, y, w, h] = minimap_rect(size, [map.width, map.height], native_ui(self.assets));
        if !native_ui(self.assets) {
            canvas.rect(x - 2.0, y - 2.0, w + 4.0, h + 4.0, 0x090f0d);
        }
        // Sample native terrain at a small fixed resolution instead of re-rendering the map.
        for row in 0..48 {
            for col in 0..64 {
                let world_x = (col * map.width / 64).min(map.width - 1);
                let world_y = (row * map.height / 48).min(map.height - 1);
                let mut color = self.presentation.ground;
                if let Some(terrain) = &map.terrain {
                    let cell = (world_y as u32 / terrain.cell_size * terrain.columns
                        + world_x as u32 / terrain.cell_size)
                        as usize;
                    color = if terrain.flags[cell] & 1 == 0 {
                        0x5c6053
                    } else {
                        0x303f33
                    };
                }
                if let Some(assets) = self.assets
                    && let Some(grid) = &assets.manifest.terrain_grid
                {
                    let tile = grid.tiles[(world_y as u32 / grid.tile_size * grid.columns
                        + world_x as u32 / grid.tile_size)
                        as usize];
                    let across = assets.terrain.width / grid.tile_size;
                    let px = tile % across * grid.tile_size + grid.tile_size / 2;
                    let py = tile / across * grid.tile_size + grid.tile_size / 2;
                    let index = ((py * assets.terrain.width + px) * 4) as usize;
                    color = (u32::from(assets.terrain.rgba[index]) << 16)
                        | (u32::from(assets.terrain.rgba[index + 1]) << 8)
                        | u32::from(assets.terrain.rgba[index + 2]);
                }
                if map.fog_of_war {
                    let masks = fog::masks(
                        &self.world.state().terrain_fog[0],
                        (map.width + fog::CELL - 1) / fog::CELL,
                        (map.height + fog::CELL - 1) / fog::CELL,
                        world_x / fog::CELL,
                        world_y / fog::CELL,
                    );
                    let light = 255
                        - fog::opacity(
                            masks,
                            (world_x % fog::CELL) as u32,
                            (world_y % fog::CELL) as u32,
                        );
                    color = [0, 8, 16].into_iter().fold(0, |result, shift| {
                        result | (((((color >> shift) & 255) * light + 127) / 255) << shift)
                    });
                }
                canvas.rect(
                    x + f64::from(col) * w / 64.0,
                    y + f64::from(row) * h / 48.0,
                    (w / 64.0).ceil(),
                    (h / 48.0).ceil(),
                    color,
                );
            }
        }
        for resource in self.world.state().resources.iter().filter(|r| {
            r.amount > 0 && self.world.visibility(PlayerId(0), r.position) != Visibility::Unexplored
        }) {
            canvas.rect(
                x + f64::from(resource.position.x) / f64::from(map.width) * w - 1.0,
                y + f64::from(resource.position.y) / f64::from(map.height) * h - 1.0,
                3.0,
                3.0,
                0x72c9e6,
            );
        }
        for entity in self
            .world
            .state()
            .entities
            .iter()
            .filter(|entity| self.world.entity_visible(PlayerId(0), entity.id))
        {
            let definition = self.world.unit_type(entity.unit_type).unwrap();
            let ew = (f64::from(definition.footprint.width) / f64::from(map.width) * w).max(3.0);
            let eh = (f64::from(definition.footprint.height) / f64::from(map.height) * h).max(3.0);
            canvas.rect(
                x + f64::from(entity.position.x) / f64::from(map.width) * w - ew / 2.0,
                y + f64::from(entity.position.y) / f64::from(map.height) * h - eh / 2.0,
                ew,
                eh,
                if entity.owner.0 == 0 {
                    0x8beb70
                } else if self.world.is_enemy(PlayerId(0), entity.owner) {
                    0xe06751
                } else {
                    0x7db7df
                },
            );
        }
        let [left, top, right, bottom] = self.camera.visible_world(size);
        let left = left.clamp(0.0, f64::from(map.width));
        let top = top.clamp(0.0, f64::from(map.height));
        let right = right.clamp(left, f64::from(map.width));
        let bottom = bottom.clamp(top, f64::from(map.height));
        canvas.outline(
            x + left / f64::from(map.width) * w,
            y + top / f64::from(map.height) * h,
            (right - left) / f64::from(map.width) * w,
            (bottom - top) / f64::from(map.height) * h,
            0xf0ebd6,
        );
    }
}
