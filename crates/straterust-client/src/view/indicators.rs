use super::world::selection_color;
use super::*;
use straterust_engine::{assets::UnitIndicator, sim::Entity};

#[cfg(test)]
mod tests;

fn allegiance(world: &World, owner: PlayerId) -> usize {
    if owner == world.view_player() {
        0
    } else if world.is_enemy(world.view_player(), owner) {
        2
    } else {
        1
    }
}

impl<'a> View<'a> {
    pub(super) fn draw_command_feedback(&self, canvas: &mut Canvas<'_, 'a>, size: [f64; 2]) {
        let Some(feedback) = self.visuals.command_feedback() else {
            return;
        };
        let visual::CommandTarget::Ground(position) = feedback.target else {
            return;
        };
        let p = self
            .camera
            .world_to_screen(f64::from(position.x), f64::from(position.y), size);
        if let Some(pack) = self.assets.and_then(|assets| assets.indicators.as_ref())
            && let Some(index) = pack
                .manifest
                .cursors
                .iter()
                .position(|cursor| cursor.key == "target-green")
        {
            let cursor = &pack.manifest.cursors[index];
            let image = &pack.cursors[index];
            let height = image.height / u32::from(cursor.frames);
            let frame = (feedback.elapsed.as_millis() / u128::from(cursor.frame_ms)
                % u128::from(cursor.frames)) as u32;
            canvas.image_region(
                image,
                [
                    p[0] - f64::from(cursor.anchor[0]) * self.camera.zoom,
                    p[1] - f64::from(cursor.anchor[1]) * self.camera.zoom,
                ],
                [image.width, height],
                self.camera.zoom,
                [0, frame * height, image.width, height],
            );
        } else {
            canvas.selection_circle(
                p,
                [8.0 * self.camera.zoom, 5.0 * self.camera.zoom],
                self.presentation.friendly,
            );
        }
    }

    fn indicator(&self, unit_type: UnitTypeId) -> Option<&'a UnitIndicator> {
        self.assets?
            .indicators
            .as_ref()?
            .manifest
            .units
            .iter()
            .find(|entry| entry.unit_type == unit_type)
    }

    pub(super) fn draw_selection_circle(
        &self,
        canvas: &mut Canvas<'_, 'a>,
        entity: &Entity,
        p: [f64; 2],
        half: [f64; 2],
    ) {
        if let Some(metrics) = self.indicator(entity.unit_type) {
            let pack = self.assets.unwrap().indicators.as_ref().unwrap();
            let image = &pack.circles[usize::from(metrics.circle)];
            let height = image.height / 3;
            canvas.image_region(
                image,
                [
                    p[0] - f64::from(image.width) * self.camera.zoom / 2.0,
                    p[1] + (f64::from(metrics.circle_y) - f64::from(height) / 2.0)
                        * self.camera.zoom,
                ],
                [image.width, height],
                self.camera.zoom,
                [
                    0,
                    allegiance(self.world, entity.owner) as u32 * height,
                    image.width,
                    height,
                ],
            );
        } else {
            canvas.selection_circle(
                [p[0], p[1] + half[1] * 0.35],
                [
                    half[0] + 4.0 * self.camera.zoom,
                    (half[1] * 0.65).max(5.0 * self.camera.zoom),
                ],
                selection_color(self.world, entity.owner),
            );
        }
    }

    pub(super) fn draw_unit_bars(
        &self,
        canvas: &mut Canvas<'_, 'a>,
        entity: &Entity,
        p: [f64; 2],
        half: [f64; 2],
    ) {
        let definition = self.world.unit_type(entity.unit_type).unwrap();
        if let Some(metrics) = self.indicator(entity.unit_type) {
            if entity.invincible || !self.selected.contains(&entity.id) {
                return;
            }
            let colors = &self
                .assets
                .unwrap()
                .indicators
                .as_ref()
                .unwrap()
                .manifest
                .health_colors;
            let fraction = f64::from(entity.hp) / f64::from(definition.max_hp.max(1));
            let first = if fraction >= 0.66 {
                0
            } else if fraction >= 0.33 {
                3
            } else {
                6
            };
            let origin = [
                p[0] - f64::from(metrics.bar_width) * self.camera.zoom / 2.0,
                p[1] + f64::from(metrics.bar_y) * self.camera.zoom,
            ];
            draw_bar(
                canvas,
                origin,
                metrics.bar_width,
                fraction,
                colors,
                first,
                self.camera.zoom,
            );
            if self.world.energy_max(entity) > 0 {
                draw_bar(
                    canvas,
                    [origin[0], origin[1] + 6.0 * self.camera.zoom],
                    metrics.bar_width,
                    f64::from(entity.energy) / f64::from(self.world.energy_max(entity) * 256),
                    colors,
                    12,
                    self.camera.zoom,
                );
            }
        } else if self.world.rules().victory
            || self.selected.contains(&entity.id)
            || entity.hp < definition.max_hp
        {
            let width = (2.0 * half[0]).clamp(20.0, 100.0);
            canvas.rect(
                p[0] - width / 2.0,
                p[1] - half[1] - 6.0,
                width,
                3.0,
                0x182022,
            );
            let color = if entity.owner == self.world.view_player() {
                self.presentation.friendly
            } else {
                self.presentation.opposing
            };
            canvas.rect(
                p[0] - width / 2.0,
                p[1] - half[1] - 6.0,
                width * f64::from(entity.hp) / f64::from(definition.max_hp.max(1)),
                3.0,
                color,
            );
        }
    }

    pub(super) fn draw_cursor(&self, canvas: &mut Canvas<'_, 'a>, size: [f64; 2]) {
        let Some(pack) = self.assets.and_then(|assets| assets.indicators.as_ref()) else {
            return;
        };
        if self.cursor[0] < 0.0
            || self.cursor[1] < 0.0
            || self.cursor[0] >= size[0]
            || self.cursor[1] >= size[1]
        {
            return;
        }
        let world_point = self.camera.screen_to_world(self.cursor, size);
        let hovered = world_point
            .and_then(|position| {
                crate::selection::entity_at(
                    self.world,
                    self.presentation,
                    position,
                    self.camera.zoom,
                )
            })
            .and_then(|id| {
                self.world
                    .state()
                    .entities
                    .iter()
                    .find(|entity| entity.id == id)
            });
        let key = if self.drag_box.is_some() {
            "drag"
        } else if world_point.is_some() && self.placement.is_some_and(|(_, _, valid)| !valid) {
            "illegal"
        } else if let Some(entity) = hovered {
            match (self.targeting, allegiance(self.world, entity.owner)) {
                (false, 0) => "hover-green",
                (false, 1) => "hover-yellow",
                (false, _) => "hover-red",
                (true, 0) => "target-green",
                (true, 1) => "target-yellow",
                (true, _) => "target-red",
            }
        } else if world_point.is_some_and(|position| {
            crate::selection::resource_at(self.world, self.assets, position).is_some()
        }) {
            if self.targeting {
                "target-yellow"
            } else {
                "hover-yellow"
            }
        } else if world_point.is_some() && self.targeting {
            "target"
        } else {
            "arrow"
        };
        let Some(index) = pack
            .manifest
            .cursors
            .iter()
            .position(|cursor| cursor.key == key)
        else {
            return;
        };
        let cursor = &pack.manifest.cursors[index];
        let image = &pack.cursors[index];
        let height = image.height / u32::from(cursor.frames);
        let frame =
            (self.animation_ms / u128::from(cursor.frame_ms) % u128::from(cursor.frames)) as u32;
        let zoom = native_ui_scale(size);
        canvas.image_region(
            image,
            [
                self.cursor[0] - f64::from(cursor.anchor[0]) * zoom,
                self.cursor[1] - f64::from(cursor.anchor[1]) * zoom,
            ],
            [image.width, height],
            zoom,
            [0, frame * height, image.width, height],
        );
    }
}

fn draw_bar(
    canvas: &mut Canvas<'_, '_>,
    p: [f64; 2],
    width: u16,
    fraction: f64,
    colors: &[u32; 19],
    first: usize,
    zoom: f64,
) {
    let filled = if fraction <= 0.0 {
        0
    } else {
        ((fraction.min(1.0) * f64::from(width) / 3.0).round() as u16 * 3)
            .max(3)
            .min(width)
    };
    canvas.rect_snapped(p[0], p[1], f64::from(width) * zoom, 5.0 * zoom, colors[18]);
    for x in (1..width).step_by(3) {
        let start = if x < filled { first } else { 15 };
        for row in 0..3 {
            canvas.rect_snapped(
                p[0] + f64::from(x) * zoom,
                p[1] + (row as f64 + 1.0) * zoom,
                f64::from(2.min(width - x)) * zoom,
                zoom,
                colors[start + row],
            );
        }
    }
}
