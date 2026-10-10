//! Package-authored console geometry; the map aperture is also used by the camera.
use super::*;
use straterust_engine::assets::ConsoleLayout;

impl<'a> View<'a> {
    pub(super) fn draw_authored_console(
        &self,
        canvas: &mut Canvas<'_, 'a>,
        size: [f64; 2],
        layout: ConsoleLayout,
    ) {
        let assets = self.assets.unwrap();
        let scale = layout.viewport.scale(size);
        let [cw, ch] = layout.viewport.canvas.map(u32::from);
        let [left, top, right, bottom] = layout.viewport.margins.map(u32::from);
        if let Some(image) = assets.ui_image("console") {
            // The outer strips stretch independently of the map. The sidebar
            // retains uniform pixel scale and its authored control coordinates.
            canvas.rect(0.0, 0.0, f64::from(left) * scale, size[1], 0x101010);
            canvas.image_region(image, [0.0, 0.0], [left, ch], scale, [0, 0, left, ch]);
            let width = ((size[0] / scale).ceil() as u32).saturating_sub(left + right);
            let height = (size[1] / scale).ceil() as u32;
            canvas.image_region(
                image,
                [f64::from(left) * scale, 0.0],
                [width, top],
                scale,
                [left, 0, cw - left - right, top],
            );
            canvas.image_region(
                image,
                [size[0] - f64::from(right) * scale, 0.0],
                [right, height],
                scale,
                [cw - right, 0, right, ch],
            );
            canvas.image_region(
                image,
                [f64::from(left) * scale, size[1] - f64::from(bottom) * scale],
                [width, bottom],
                scale,
                [left, ch - bottom, cw - left - right, bottom],
            );
        }
        self.draw_minimap(canvas, size);
        let [x, y, w, h] = layout.rect(layout.selection, size);
        if !self.draw_resource_details(canvas, [x, y, w, h], scale) {
            let (container, members) = crate::selection::panel_members(self.world, self.selected);
            if members.len() > 1 || container.is_some() && !members.is_empty() {
                self.draw_selection_group(canvas, size);
            } else if let Some(entity) = self
                .world
                .state()
                .entities
                .iter()
                .find(|e| self.selected.contains(&e.id))
            {
                self.draw_console_entity(canvas, entity, [x, y, w, h], scale);
            }
        }
        self.draw_command_buttons(canvas, size, true);
        let player = self.world.view_player();
        let mut cursor = f64::from(left) * scale + 10.0 * scale;
        for kind in ["gold", "wood", "oil"] {
            let value = self.world.resource_balance(player, kind);
            canvas.text(
                &format!("{} {value}", kind.to_uppercase()),
                cursor,
                4.0 * scale,
                scale,
                0xf2e4b1,
            );
            cursor += 106.0 * scale;
        }
        let (used, provided) = self.world.supply(player);
        canvas.text(
            &format!("{used}/{provided}"),
            cursor,
            4.0 * scale,
            scale,
            0xf2e4b1,
        );
        canvas.text("Menu (Esc)", 38.0 * scale, 8.0 * scale, scale, 0xf2e4b1);
        let status_x = f64::from(left) * scale + 8.0 * scale;
        canvas.text(
            &shorten(
                self.status,
                ((size[0] - status_x - 20.0) / (8.0 * scale)) as usize,
            ),
            status_x,
            size[1] - 12.0 * scale,
            scale,
            0xf2e4b1,
        );
        self.draw_command_tooltips(canvas, size, size[1]);
    }

    fn draw_console_entity(
        &self,
        canvas: &mut Canvas<'_, 'a>,
        entity: &straterust_engine::sim::Entity,
        rect: [f64; 4],
        s: f64,
    ) {
        let [x, y, w, _] = rect;
        let definition = self.world.unit_type(entity.unit_type).unwrap();
        let name = self.presentation.unit_name(self.assets, entity.unit_type);
        for (i, line) in wrapped_lines(&name, 19).iter().take(2).enumerate() {
            canvas.text(line, x, y + i as f64 * 10.0 * s, s, 0xf2e4b1);
        }
        draw_unit_icon(
            canvas,
            self.assets,
            entity.unit_type,
            [x, y + 24.0 * s, 46.0 * s, 38.0 * s],
            0xffffff,
        );
        canvas.text(
            &format!("{}/{}", entity.hp, definition.max_hp),
            x + 52.0 * s,
            y + 28.0 * s,
            s,
            0x76bd67,
        );
        progress_bar(
            canvas,
            [x, y + 65.0 * s, w, 4.0 * s],
            f64::from(entity.hp) / f64::from(definition.max_hp.max(1)),
            0x76bd67,
        );
        if let Some(owned) = self
            .world
            .inspect_entity(self.world.view_player(), entity.id)
            .and_then(|i| i.owned)
        {
            let job = owned
                .construction
                .as_ref()
                .map(|j| ("Building", j.remaining, j.total))
                .or_else(|| {
                    owned
                        .research
                        .as_ref()
                        .map(|j| ("Research", j.remaining, j.total))
                })
                .or_else(|| {
                    owned
                        .production
                        .front()
                        .map(|j| ("Training", j.remaining, j.total))
                });
            if let Some((label, remaining, total)) = job {
                let progress = 1.0 - f64::from(remaining) / f64::from(total.max(1));
                canvas.text(
                    &format!("{label} {:.0}%", progress * 100.0),
                    x,
                    y + 82.0 * s,
                    s,
                    0xf2e4b1,
                );
                progress_bar(canvas, [x, y + 96.0 * s, w, 5.0 * s], progress, 0x76bd67);
                for (i, job) in owned.production.iter().take(4).enumerate() {
                    draw_unit_icon(
                        canvas,
                        self.assets,
                        job.unit_type,
                        [x + i as f64 * 38.0 * s, y + 110.0 * s, 34.0 * s, 28.0 * s],
                        0xffffff,
                    );
                }
            } else if self.world.energy_max(owned) > 0 {
                canvas.text(
                    &format!(
                        "Mana {}/{}",
                        owned.energy / 256,
                        self.world.energy_max(owned)
                    ),
                    x,
                    y + 82.0 * s,
                    s,
                    0x688bcc,
                );
                progress_bar(
                    canvas,
                    [x, y + 96.0 * s, w, 5.0 * s],
                    f64::from(owned.energy) / f64::from(self.world.energy_max(owned) * 256),
                    0x688bcc,
                );
            } else {
                if let Some(weapon) = &definition.weapon {
                    canvas.text(
                        &format!("Damage {}", weapon.damage),
                        x,
                        y + 82.0 * s,
                        s,
                        0xf2e4b1,
                    );
                }
                canvas.text(
                    &format!("Armor {}", definition.armor),
                    x,
                    y + 98.0 * s,
                    s,
                    0xf2e4b1,
                );
            }
        }
    }
}
