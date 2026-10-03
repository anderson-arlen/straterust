use super::*;

impl<'a> View<'a> {
    pub(super) fn draw_fog(&self, canvas: &mut Canvas<'_, 'a>, size: [f64; 2]) {
        if !self.world.map().fog_of_war {
            return;
        }
        let bounds = self.camera.visible_world(size);
        let map = self.world.map();
        let columns = (map.width + fog::CELL - 1) / fog::CELL;
        let rows = (map.height + fog::CELL - 1) / fog::CELL;
        let grid = &self.world.state().terrain_fog[0];
        let left = (bounds[0].floor() as i32).max(0) / fog::CELL;
        let top = (bounds[1].floor() as i32).max(0) / fog::CELL;
        let right = ((bounds[2].ceil() as i32).min(map.width) + fog::CELL - 1) / fog::CELL;
        let bottom = ((bounds[3].ceil() as i32).min(map.height) + fog::CELL - 1) / fog::CELL;
        let atlas = fog::atlas();
        for y in top..bottom {
            for x in left..right {
                let masks = fog::masks(grid, columns, rows, x, y);
                let p = self.camera.world_to_screen(
                    f64::from(x * fog::CELL),
                    f64::from(y * fog::CELL),
                    size,
                );
                for (layer, mask) in masks.into_iter().enumerate() {
                    if mask == 0 || (layer == 0 && masks[1] == fog::FULL) {
                        continue;
                    }
                    canvas.image_region(
                        atlas,
                        p,
                        [
                            fog::CELL.min(map.width - x * fog::CELL) as u32,
                            fog::CELL.min(map.height - y * fog::CELL) as u32,
                        ],
                        self.camera.zoom,
                        fog::region(mask, layer),
                    );
                }
            }
        }
    }
}
