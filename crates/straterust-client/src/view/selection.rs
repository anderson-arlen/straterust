use super::*;

impl<'a> View<'a> {
    pub(super) fn draw_portrait(
        &self,
        canvas: &mut Canvas<'_, 'a>,
        unit_type: UnitTypeId,
        rect: [f64; 4],
    ) {
        if let Some(portrait) = self.media.and_then(|media| media.portrait(unit_type)) {
            let mission_speaking = self.mission.is_some_and(|mission| {
                mission.talking()
                    && mission.active_slot.and_then(|slot| mission.portraits[slot])
                        == Some(unit_type)
            });
            let frames = if (self.speaking == Some(unit_type) || mission_speaking)
                && !portrait.talk.is_empty()
            {
                &portrait.talk
            } else {
                &portrait.idle
            };
            let index =
                (self.portrait_ms / u128::from(portrait.frame_ms) % frames.len() as u128) as usize;
            let image = &frames[index];
            let scale = (rect[2] / f64::from(image.width)).min(rect[3] / f64::from(image.height));
            canvas.image(
                image,
                [
                    rect[0] + (rect[2] - f64::from(image.width) * scale) / 2.0,
                    rect[1] + (rect[3] - f64::from(image.height) * scale) / 2.0,
                ],
                [image.width, image.height],
                scale,
            );
        } else {
            draw_unit_icon(
                canvas,
                self.assets,
                unit_type,
                rect,
                self.presentation.friendly,
            );
        }
    }

    pub(super) fn draw_native_selection(&self, canvas: &mut Canvas<'_, 'a>, size: [f64; 2]) {
        let scale = native_ui_scale(size);
        let x = 156.0 * scale;
        let y = size[1] - 91.0 * scale;
        let width = size[0] - (156.0 + 242.0) * scale;
        let portrait = [
            size[0] - 225.0 * scale,
            size[1] - 69.0 * scale,
            60.0 * scale,
            56.0 * scale,
        ];
        canvas.rect(x, y, width, 88.0 * scale, 0x050707);
        canvas.rect(portrait[0], portrait[1], portrait[2], portrait[3], 0x050707);
        if self.draw_resource_details(canvas, [x, y, width, 88.0 * scale], scale) {
            return;
        }
        let Some(entity) = self
            .world
            .state()
            .entities
            .iter()
            .find(|entity| self.selected.contains(&entity.id))
        else {
            canvas.text("SELECT A UNIT", x + 8.0, y + 12.0, 1.0, 0xd3d8bf);
            canvas.text("SHIFT QUEUES ORDERS", x + 8.0, y + 30.0, 1.0, 0x9aab98);
            return;
        };
        self.draw_portrait(canvas, entity.unit_type, portrait);
        let (container, members) = crate::selection::panel_members(self.world, self.selected);
        if members.len() > 1 || (container.is_some() && !members.is_empty()) {
            self.draw_selection_group(canvas, size);
            return;
        }
        let definition = self.world.unit_type(entity.unit_type).unwrap();
        let title = self.presentation.unit_name(self.assets, entity.unit_type);
        canvas.text(
            &shorten(&title, ((width - 16.0) / 8.0) as usize),
            x + 8.0,
            y + 5.0,
            1.0,
            0xd3d8bf,
        );
        let wire = [
            x + 4.0 * scale,
            y + 20.0 * scale,
            64.0 * scale,
            64.0 * scale,
        ];
        self.draw_wireframe(canvas, entity, wire, false);
        let dx = x + 76.0 * scale;
        let dw = width - 84.0 * scale;
        let dy = y + 24.0 * scale;
        canvas.text(
            &format!("HP {}/{}", entity.hp, definition.max_hp),
            dx,
            dy,
            1.0,
            0x9de088,
        );
        progress_bar(
            canvas,
            [dx, dy + 12.0, dw, 4.0],
            f64::from(entity.hp) / f64::from(definition.max_hp),
            0x76bd67,
        );
        if definition.max_shields > 0 {
            canvas.text(
                &format!(
                    "SHIELDS {}/{}",
                    entity.shields.div_ceil(256),
                    definition.max_shields
                ),
                dx,
                dy + 22.0 * scale,
                1.0,
                0x688bcc,
            );
        }
        let Some(entity) = self
            .world
            .inspect_entity(self.world.view_player(), entity.id)
            .and_then(|inspection| inspection.owned)
        else {
            return;
        };
        // Reserve a row for shields before placing job/energy status and bars.
        let dy = dy
            + if definition.max_shields > 0 {
                20.0 * scale
            } else {
                0.0
            };
        let label;
        if let Some(work) = &entity.construction {
            let progress = 1.0 - f64::from(work.remaining) / f64::from(work.total.max(1));
            label = if work.worker.is_none()
                && !definition.autonomous_construction
                && !definition.consumes_builder
            {
                "BUILD PAUSED".into()
            } else {
                format!("BUILDING {:.0}%", progress * 100.0)
            };
            progress_bar(canvas, [dx, dy + 38.0, dw, 5.0], progress, 0xd4ac63);
        } else if !self.world.powered(entity) {
            label = "UNPOWERED".into();
        } else if let Some(job) = &entity.research {
            let progress = 1.0 - f64::from(job.remaining) / f64::from(job.total.max(1));
            label = format!("RESEARCH {:.0}%", progress * 100.0);
            progress_bar(canvas, [dx, dy + 36.0, dw, 5.0], progress, 0x6fc3b1);
        } else if definition.mine_layer.is_some() {
            label = format!("MINES {}", entity.mine_count);
        } else if let Some(garrison) = &definition.garrison {
            let occupants = self
                .world
                .state()
                .entities
                .iter()
                .filter(|passenger| passenger.garrisoned_in == Some(entity.id))
                .count();
            label = format!("OCCUPANTS {}/{}", occupants, garrison.capacity);
        } else if self.world.energy_max(entity) > 0 {
            label = format!("ENERGY {}", entity.energy / 256);
            progress_bar(
                canvas,
                [dx, dy + 36.0, dw, 5.0],
                f64::from(entity.energy) / f64::from(self.world.energy_max(entity) * 256),
                0x688bcc,
            );
        } else if let Some(job) = entity.production.front() {
            let progress = 1.0 - f64::from(job.remaining) / f64::from(job.total.max(1));
            label = if !job.started || job.remaining == 0 {
                "WAITING: SUPPLY/EXIT".into()
            } else {
                format!("TRAINING {:.0}%", progress * 100.0)
            };
            progress_bar(canvas, [dx, dy + 38.0, dw, 5.0], progress, 0x6fc3b1);
            for (index, job) in entity.production.iter().take(5).enumerate() {
                let (queue_x, queue_y) = if definition.max_shields > 0 {
                    (dx + 144.0 * scale + index as f64 * 24.0 * scale, dy + 16.0)
                } else {
                    (dx + index as f64 * 24.0, dy + 47.0)
                };
                if queue_x + 22.0 * scale > dx + dw {
                    break;
                }
                draw_unit_icon(
                    canvas,
                    self.assets,
                    job.unit_type,
                    [queue_x, queue_y, 22.0 * scale, 21.0 * scale],
                    self.presentation.friendly,
                );
            }
        } else if definition.production_capacity > 0 {
            let stored = self.world.stored_production_count(entity);
            label = format!("READY {stored}/{}", self.world.production_capacity(entity));
        } else if let Some(cargo) = &entity.cargo {
            label = format!("CARRYING {}", cargo.amount);
        } else if let Some(extraction) = &definition.extracts {
            let remaining = self
                .world
                .state()
                .resources
                .iter()
                .find(|node| node.position == entity.position && node.kind == extraction.resource)
                .map_or(0, |node| node.amount);
            label = format!("GAS {remaining}");
        } else {
            label = match entity.order {
                UnitOrder::Repair { .. } => {
                    if self
                        .visuals
                        .get(entity.id)
                        .is_some_and(|visual| visual.action == VisualAction::Work)
                    {
                        "REPAIRING"
                    } else {
                        "REPAIR ORDER"
                    }
                }
                UnitOrder::Gather { .. } => "GATHERING",
                UnitOrder::Build { .. } => "CONSTRUCTING",
                UnitOrder::PlaceBuilding { .. } => "BUILD ORDER",
                UnitOrder::Attack { .. } | UnitOrder::AttackMove { .. } => "ATTACKING",
                UnitOrder::Move { .. } => "MOVING",
                UnitOrder::UnloadAt { .. } => "UNLOADING",
                UnitOrder::Hold => "HOLDING",
                UnitOrder::Patrol { .. } => "PATROLLING",
                _ => "READY",
            }
            .into();
        }
        canvas.text(
            &shorten(&label, (dw / 8.0) as usize),
            dx,
            dy + 24.0,
            1.0,
            0x9aab98,
        );
    }

    pub(super) fn draw_selection_group(&self, canvas: &mut Canvas<'_, 'a>, size: [f64; 2]) {
        let native = native_ui(self.assets);
        let (container, members) = crate::selection::panel_members(self.world, self.selected);
        for (slot, entity) in members.into_iter().take(crate::SELECTION_LIMIT).enumerate() {
            let [x, y, w, h] = selection_rect(slot, size, native);
            let hover = contains([x, y, w, h], self.cursor);
            let scale = if native { native_ui_scale(size) } else { 1.0 };
            canvas.rect(x, y, w, h, 0x050707);
            let icon_size = (w - scale).min(h - 3.0 * scale);
            let rect = [x + (w - icon_size) / 2.0, y, icon_size, icon_size];
            self.draw_wireframe(canvas, entity, rect, true);
            let max_hp = self
                .world
                .unit_type(entity.unit_type)
                .unwrap()
                .max_hp
                .max(1);
            let health = f64::from(entity.hp) / f64::from(max_hp);
            let color = if health > 0.66 {
                0x76bd67
            } else if health > 0.33 {
                0xd4ac63
            } else {
                0xdb655b
            };
            progress_bar(
                canvas,
                [x + scale, y + h - 3.0 * scale, w - 2.0 * scale, 2.0 * scale],
                health,
                color,
            );
            if hover || !native {
                canvas.outline(x, y, w, h, if hover { 0xd7cc85 } else { 0x4f6556 });
            }
            if !native {
                let hp = if w >= 60.0 {
                    format!("{}/{}", entity.hp, max_hp)
                } else {
                    entity.hp.to_string()
                };
                canvas.text(
                    &shorten(&hp, ((w - 4.0) / 8.0) as usize),
                    x + 2.0,
                    y + h - 16.0,
                    1.0,
                    color,
                );
            }
        }
        if container.is_some() && native {
            let scale = native_ui_scale(size);
            canvas.text(
                "CLICK PASSENGER TO UNLOAD",
                168.0 * scale,
                size[1] - 11.0 * scale,
                scale.min(1.0),
                0x9aab98,
            );
        }
    }

    fn draw_wireframe(
        &self,
        canvas: &mut Canvas<'_, 'a>,
        entity: &straterust_engine::sim::Entity,
        rect: [f64; 4],
        group: bool,
    ) {
        let hp = self
            .world
            .unit_type(entity.unit_type)
            .unwrap()
            .max_hp
            .max(1);
        let kinds = if group {
            &["groupwire", "wireframe"][..]
        } else {
            &["wireframe"][..]
        };
        for kind in kinds {
            if let Some(image) = self.assets.and_then(|assets| {
                assets.ui_image(&format!("{kind}.damage.{}", entity.unit_type.0))
            }) && image.height == image.width * 20
            {
                let level = (u64::from(entity.hp) * 9 / u64::from(hp)).min(9) as u32;
                let row = (entity.id.0 % 2) * 10 + level;
                canvas.image_region(
                    image,
                    [rect[0], rect[1]],
                    [image.width, image.width],
                    rect[2] / f64::from(image.width),
                    [0, row * image.width, image.width, image.width],
                );
                return;
            }
            if let Some(image) = self
                .assets
                .and_then(|assets| assets.ui_image(&format!("{kind}.{}", entity.unit_type.0)))
            {
                canvas.image_stretched(image, rect, 0xffffff);
                return;
            }
        }
        draw_unit_icon(
            canvas,
            self.assets,
            entity.unit_type,
            rect,
            self.presentation.friendly,
        );
    }
}
