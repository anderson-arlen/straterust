use super::*;

impl<'a> View<'a> {
    pub(super) fn draw_hud(&self, canvas: &mut Canvas<'_, 'a>, size: [f64; 2]) {
        let art = self.presentation;
        let native = native_ui(self.assets);
        let bottom = size[1] - FOOTER;
        let left_width = (size[0] * 0.23).clamp(150.0, 210.0);
        let right = size[0] - command_width(size);
        let panel = 0x202323;
        let edge = 0x657064;
        let text = 0xd3d8bf;
        let muted = 0x9aab98;
        canvas.rect(0.0, 0.0, size[0], HEADER, 0x181d1d);
        canvas.rect(0.0, HEADER - 2.0, size[0], 2.0, edge);
        canvas.text("STRATERUST", 12.0, 10.0, 1.0, text);
        canvas.text(
            if self.world.map().mission.is_some() {
                "CAMPAIGN"
            } else if self.world.rules().victory {
                "PRACTICE / MAP REVEALED"
            } else {
                "MAP PREVIEW"
            },
            128.0,
            10.0,
            1.0,
            muted,
        );
        let (used, provided) = self.world.supply(straterust_engine::sim::PlayerId(0));
        let mut kinds: Vec<_> = self
            .world
            .state()
            .players
            .first()
            .map(|player| player.resources.keys().map(String::as_str).collect())
            .unwrap_or_default();
        // Native packs may expose counters whose starting balance is zero.
        if let Some(assets) = self.assets {
            for entry in &assets.ui {
                if let Some(kind) = entry.key.strip_prefix("resource.")
                    && kind != "supply"
                    && !kinds.contains(&kind)
                {
                    kinds.push(kind);
                }
            }
        }
        let mut counters: Vec<_> = kinds
            .into_iter()
            .map(|kind| {
                let value = self
                    .world
                    .resource_balance(straterust_engine::sim::PlayerId(0), kind);
                let image = self
                    .assets
                    .and_then(|assets| assets.ui_image(&format!("resource.{kind}")));
                (
                    image,
                    if image.is_some() {
                        value.to_string()
                    } else {
                        format!("{} {value}", kind.to_uppercase())
                    },
                )
            })
            .collect();
        let supply_icon = self
            .assets
            .and_then(|assets| assets.ui_image("resource.supply"));
        counters.push((
            supply_icon,
            if supply_icon.is_some() {
                format!("{used}/{provided}")
            } else {
                format!("SUPPLY {used}/{provided}")
            },
        ));
        let counter_width = |image: Option<&Image>, text: &str| {
            text.len() as f64 * 8.0 + if image.is_some() { 48.0 } else { 20.0 }
        };
        let mut x = (size[0]
            - counters
                .iter()
                .map(|(image, text)| counter_width(*image, text))
                .sum::<f64>()
            - 4.0)
            .max(130.0);
        for (image, value) in counters {
            if let Some(image) = image {
                canvas.image_stretched(image, [x, 5.0, 22.0, 22.0], 0xffffff);
            }
            canvas.text(
                &value,
                x + if image.is_some() { 28.0 } else { 0.0 },
                10.0,
                1.0,
                0x9bd6db,
            );
            x += counter_width(image, &value);
        }
        let mode = if self.paused {
            "PAUSED"
        } else if self.playback {
            "REPLAY"
        } else {
            "LIVE"
        };
        canvas.text(
            &format!("{mode}  {:.0}%", self.camera.zoom * 100.0),
            size[0] - 124.0,
            30.0,
            1.0,
            text,
        );

        if native {
            let image = self.assets.unwrap().ui_image("console").unwrap();
            let scale = native_ui_scale(size);
            let top = size[1] - 480.0 * scale;
            canvas.rect(0.0, bottom, size[0], FOOTER, 0x0b0d10);
            canvas.image_region(image, [0.0, top], [275, 480], scale, [0, 0, 275, 480]);
            let middle = (size[0] / scale - 619.0).ceil().max(0.0) as u32;
            canvas.image_region(
                image,
                [275.0 * scale, top],
                [middle, 480],
                scale,
                [275, 0, 21, 480],
            );
            canvas.image_region(
                image,
                [size[0] - 344.0 * scale, top],
                [344, 480],
                scale,
                [296, 0, 344, 480],
            );
            // The minimap letterboxes within its actual source aperture.
            canvas.rect(
                6.0 * scale,
                size[1] - 132.0 * scale,
                128.0 * scale,
                128.0 * scale,
                0x050707,
            );
            self.draw_minimap(canvas, size);
            self.draw_native_selection(canvas, size);
        } else {
            // Original console for content without imported UI art.
            canvas.rect(0.0, bottom, size[0], FOOTER, panel);
            if let Some(image) = self.assets.and_then(|assets| assets.ui_image("console")) {
                canvas.image_stretched(image, [0.0, bottom, size[0], FOOTER], 0xffffff);
            }
            canvas.rect(0.0, bottom, size[0], 3.0, edge);
            canvas.rect(left_width, bottom + 4.0, 2.0, 196.0, edge);
            canvas.rect(right, bottom + 4.0, 2.0, 196.0, edge);
            canvas.text("MAP", 12.0, bottom + 12.0, 1.0, text);
            canvas.text("COMMANDS", right + 12.0, bottom + 12.0, 1.0, text);
            self.draw_minimap(canvas, size);
            canvas.text("CLICK MAP TO PAN", 12.0, bottom + 196.0, 1.0, muted);

            let card_x = left_width + 14.0;
            let card_width = right - card_x - 12.0;
            let (container, members) = crate::selection::panel_members(self.world, self.selected);
            if self.draw_resource_details(canvas, [card_x, bottom + 12.0, card_width, 150.0], 1.0) {
                // A resource is a single neutral inspection selection.
            } else if members.len() > 1 || (container.is_some() && !members.is_empty()) {
                canvas.text(
                    if container.is_some() {
                        "PASSENGERS"
                    } else {
                        "SELECTED UNITS"
                    },
                    card_x,
                    bottom + 12.0,
                    1.0,
                    text,
                );
                self.draw_selection_group(canvas, size);
                canvas.text(
                    &shorten(
                        if container.is_some() {
                            "CLICK PASSENGER TO UNLOAD"
                        } else {
                            "CLICK: SELECT  SHIFT: REMOVE"
                        },
                        (card_width / 8.0) as usize,
                    ),
                    card_x,
                    bottom + 194.0,
                    1.0,
                    muted,
                );
            } else if let Some(entity) = self
                .world
                .state()
                .entities
                .iter()
                .find(|e| self.selected.contains(&e.id))
            {
                let definition = self.world.unit_type(entity.unit_type).unwrap();
                let name = art.unit_name(self.assets, entity.unit_type);
                canvas.text(
                    &shorten(&name, (card_width / 8.0) as usize),
                    card_x,
                    bottom + 12.0,
                    1.0,
                    text,
                );
                canvas.rect(card_x, bottom + 30.0, 84.0, 78.0, 0x101818);
                canvas.outline(card_x, bottom + 30.0, 84.0, 78.0, 0x4f6556);
                self.draw_portrait(
                    canvas,
                    entity.unit_type,
                    [card_x + 4.0, bottom + 34.0, 76.0, 70.0],
                );
                let detail_x = card_x + 94.0;
                let detail_width = (card_width - 94.0).max(50.0);
                canvas.text(
                    &format!("HP {}/{}", entity.hp, definition.max_hp),
                    detail_x,
                    bottom + 33.0,
                    1.0,
                    0x9de088,
                );
                progress_bar(
                    canvas,
                    [detail_x, bottom + 47.0, detail_width, 6.0],
                    f64::from(entity.hp) / f64::from(definition.max_hp.max(1)),
                    0x76bd67,
                );
                if let Some(entity) = self
                    .world
                    .inspect_entity(PlayerId(0), entity.id)
                    .and_then(|inspection| inspection.owned)
                {
                    let mut details = Vec::new();
                    if let Some(extraction) = &definition.extracts
                        && let Some(node) = self.world.state().resources.iter().find(|node| {
                            node.position == entity.position && node.kind == extraction.resource
                        })
                    {
                        details.push(format!("{} remaining", node.amount));
                    }
                    if self.selected.len() > 1 {
                        details.push(format!("{} units selected", self.selected.len()));
                    }
                    if let Some(cargo) = &entity.cargo {
                        details.push(format!("Carrying {} {}", cargo.amount, cargo.kind));
                    }
                    details.push(if entity.construction.is_some() {
                        "Under construction".into()
                    } else if definition.worker.is_some() {
                        if definition.repairs.is_empty() {
                            "Worker: gather / build"
                        } else {
                            "Worker: gather / build / repair"
                        }
                        .into()
                    } else if definition.structure && !definition.trains.is_empty() {
                        "Production structure".into()
                    } else if definition.structure && definition.supply_provided > 0 {
                        format!("Provides {} supply", definition.supply_provided)
                    } else if definition.structure {
                        "Structure".into()
                    } else if definition.weapon.is_some() {
                        "Combat unit".into()
                    } else {
                        "Mobile unit".into()
                    });
                    if !entity.queued_orders.is_empty() {
                        details.push(format!("{} orders queued", entity.queued_orders.len()));
                    }
                    let mut y = bottom + 62.0;
                    for line in wrapped_lines(&details.join(". "), (detail_width / 8.0) as usize)
                        .into_iter()
                        .take(4)
                    {
                        canvas.text(&line, detail_x, y, 1.0, muted);
                        y += 12.0;
                    }
                    if let Some(work) = &entity.construction {
                        let progress =
                            1.0 - f64::from(work.remaining) / f64::from(work.total.max(1));
                        canvas.text(
                            &format!("BUILDING {:.0}%", progress * 100.0),
                            card_x,
                            bottom + 119.0,
                            1.0,
                            0xe2c179,
                        );
                        progress_bar(
                            canvas,
                            [card_x, bottom + 133.0, card_width, 8.0],
                            progress,
                            0xd4ac63,
                        );
                        let hint = if work.worker.is_none() {
                            "PAUSED: right-click with a worker to resume."
                        } else if work
                            .worker
                            .and_then(|id| self.visuals.get(id))
                            .is_some_and(|visual| visual.action == VisualAction::Work)
                        {
                            "Worker is constructing this structure."
                        } else if work.work_position.is_some() {
                            "Construction continues while the worker repositions."
                        } else {
                            "Worker assigned; waiting to reach the site."
                        };
                        for (index, line) in wrapped_lines(hint, (card_width / 8.0) as usize)
                            .iter()
                            .take(3)
                            .enumerate()
                        {
                            canvas.text(
                                line,
                                card_x,
                                bottom + 151.0 + index as f64 * 12.0,
                                1.0,
                                muted,
                            );
                        }
                    } else if let Some(job) = &entity.research {
                        let name = art
                            .research_names
                            .get(&job.id)
                            .cloned()
                            .unwrap_or_else(|| format!("Research {}", job.id.0));
                        let progress = 1.0 - f64::from(job.remaining) / f64::from(job.total.max(1));
                        canvas.text(
                            &shorten(
                                &format!("{name} {:.0}%", progress * 100.0),
                                (card_width / 8.0) as usize,
                            ),
                            card_x,
                            bottom + 119.0,
                            1.0,
                            text,
                        );
                        progress_bar(
                            canvas,
                            [card_x, bottom + 133.0, card_width, 8.0],
                            progress,
                            0x6fc3b1,
                        );
                    } else if let Some(job) = entity.production.front() {
                        let name = art.unit_name(self.assets, job.unit_type);
                        let progress = 1.0 - f64::from(job.remaining) / f64::from(job.total.max(1));
                        canvas.text(
                            &shorten(
                                &format!("{} {:.0}%", name, progress * 100.0),
                                (card_width / 8.0) as usize,
                            ),
                            card_x,
                            bottom + 119.0,
                            1.0,
                            text,
                        );
                        progress_bar(
                            canvas,
                            [card_x, bottom + 133.0, card_width, 8.0],
                            progress,
                            0x6fc3b1,
                        );
                        for (index, queued) in entity.production.iter().take(5).enumerate() {
                            let x = card_x + index as f64 * 38.0;
                            canvas.rect(x, bottom + 149.0, 32.0, 32.0, 0x101818);
                            canvas.outline(
                                x,
                                bottom + 149.0,
                                32.0,
                                32.0,
                                if index == 0 { 0x8eca91 } else { 0x566452 },
                            );
                            draw_unit_icon(
                                canvas,
                                self.assets,
                                queued.unit_type,
                                [x + 2.0, bottom + 151.0, 28.0, 28.0],
                                art.friendly,
                            );
                        }
                        if !job.started {
                            canvas.text(
                                &shorten("WAITING FOR SUPPLY / SPACE", (card_width / 8.0) as usize),
                                card_x,
                                bottom + 188.0,
                                1.0,
                                0xe2c179,
                            );
                        }
                    } else {
                        let hint = if definition.worker.is_some() && !definition.repairs.is_empty()
                        {
                            "Right-click minerals to gather, unfinished buildings to resume, or damaged friendly machines to repair. Build opens the structure menu."
                        } else if definition.worker.is_some() {
                            "Right-click minerals to gather. Build opens the structure menu. Right-click unfinished buildings to resume."
                        } else if definition.structure && !definition.trains.is_empty() {
                            "Choose a unit to train. Right-click the map to set a rally point."
                        } else if definition.structure && definition.supply_provided > 0 {
                            "This completed structure increases available supply. Select a worker to build more structures."
                        } else if definition.structure {
                            "This structure is complete. Select a worker or a mobile unit for orders."
                        } else {
                            "Right-click to move or attack. Shift queues orders. Ctrl + number stores a group."
                        };
                        for (index, line) in wrapped_lines(hint, (card_width / 8.0) as usize)
                            .iter()
                            .take(6)
                            .enumerate()
                        {
                            canvas.text(
                                line,
                                card_x,
                                bottom + 121.0 + index as f64 * 12.0,
                                1.0,
                                muted,
                            );
                        }
                    }
                }
            } else {
                canvas.text("NO UNIT SELECTED", card_x, bottom + 12.0, 1.0, text);
                for (index, line) in wrapped_lines("Left-click a friendly unit or drag a selection box. Tab finds the next owned unit. Select a worker to gather and build.", (card_width / 8.0) as usize).iter().enumerate() {
                canvas.text(line, card_x, bottom + 44.0 + index as f64 * 14.0, 1.0, muted);
            }
            }
        }
        if !native {
            for slot in 0..9 {
                let [x, y, w, h] = button_rect(slot, size, false);
                canvas.rect(x, y, w, h, 0x141a1a);
                canvas.outline(x, y, w, h, 0x3d4943);
            }
        }
        for button in self.buttons {
            let [x, y, w, h] = button_rect(button.slot, size, native_ui(self.assets));
            let hover = contains([x, y, w, h], self.cursor);
            let disabled = button.disabled.is_some();
            let color = if disabled { 0x818781 } else { 0xc1d69b };
            if !native {
                canvas.rect(
                    x + 1.0,
                    y + 1.0,
                    w - 2.0,
                    h - 2.0,
                    if disabled { 0x292d2c } else { 0x344039 },
                );
            }
            canvas.outline(
                x,
                y,
                w,
                h,
                if hover {
                    0xd7cc85
                } else if disabled {
                    0x515854
                } else {
                    0x78856a
                },
            );
            if !native {
                canvas.text(&button.key.to_uppercase(), x + 4.0, y + 4.0, 1.0, color);
            }
            if let Some(image) = self
                .assets
                .and_then(|assets| command_icon(assets, button.action))
            {
                let height = if native { h } else { 26.0 };
                let width = height * f64::from(image.width) / f64::from(image.height);
                canvas.image_stretched(
                    image,
                    [
                        x + (w - width) / 2.0,
                        y + if native { 0.0 } else { 2.0 },
                        width,
                        height,
                    ],
                    if disabled { 0x707070 } else { 0xffffff },
                );
            } else {
                match button.action {
                    Action::Build(id) | Action::Train(id) if !disabled => draw_unit_icon(
                        canvas,
                        self.assets,
                        id,
                        [x + w / 2.0 - 13.0, y + 2.0, 26.0, 25.0],
                        color,
                    ),
                    _ => {
                        let symbol = match button.action {
                            Action::AdvancedBuildMenu => "ADV",
                            Action::Move => ">",
                            Action::Stop => "[]",
                            Action::Hold => "H",
                            Action::AttackMove => "+",
                            Action::Patrol => "<>",
                            Action::Gather => "*",
                            Action::Repair => "R",
                            Action::Research(_) => "+",
                            Action::Cloak(true) => "C",
                            Action::Cloak(false) => "D",
                            Action::Stim => "T",
                            Action::Scan => "S",
                            Action::Unload => "U",
                            Action::Lift | Action::Land => "L",
                            Action::PlaceMine => "I",
                            Action::BuildMenu | Action::Build(_) => "#",
                            Action::Train(_) => "+",
                            Action::Rally => "!",
                            Action::Back => "<",
                            Action::Cancel => "X",
                        };
                        canvas.text(
                            symbol,
                            x + w / 2.0 - symbol.len() as f64 * 4.0,
                            y + 13.0,
                            1.0,
                            color,
                        );
                    }
                }
            }
            if native {
                canvas.rect(x + w - 10.0, y + h - 10.0, 10.0, 10.0, 0x090c0b);
                canvas.text(
                    &button.key.to_uppercase(),
                    x + w - 9.0,
                    y + h - 9.0,
                    1.0,
                    color,
                );
            } else {
                for (line, text) in wrapped_lines(&button.label, ((w - 6.0) / 8.0) as usize)
                    .iter()
                    .take(2)
                    .enumerate()
                {
                    canvas.text(text, x + 4.0, y + 30.0 + line as f64 * 10.0, 1.0, color);
                }
            }
        }
        if let Some(objective) = &art.objective {
            canvas.text(
                &shorten(objective, ((size[0] - 160.0) / 8.0) as usize),
                12.0,
                30.0,
                1.0,
                text,
            );
        }
        if native && art.objective.is_none() {
            canvas.text(
                &shorten(self.status, ((size[0] - 160.0) / 8.0) as usize),
                12.0,
                30.0,
                1.0,
                text,
            );
        } else {
            canvas.rect(0.0, bottom - 23.0, size[0], 23.0, 0x151c1b);
            canvas.text(
                &shorten(self.status, ((size[0] - 24.0) / 8.0) as usize),
                12.0,
                bottom - 15.0,
                1.0,
                text,
            );
            if !native {
                canvas.text(
                    &shorten(self.help, ((size[0] - 24.0) / 8.0) as usize),
                    12.0,
                    size[1] - 13.0,
                    1.0,
                    muted,
                );
            }
        }
        if let Some(button) = self.buttons.iter().find(|b| {
            contains(
                button_rect(b.slot, size, native_ui(self.assets)),
                self.cursor,
            )
        }) {
            let tooltip_width = 360.0_f64.min(size[0] - 24.0);
            let mut lines: Vec<(String, u32)> = button
                .tooltip
                .iter()
                .flat_map(|line| {
                    wrapped_lines(line, ((tooltip_width - 24.0) / 8.0) as usize)
                        .into_iter()
                        .map(|line| (line, text))
                })
                .collect();
            if let Some(reason) = &button.disabled {
                lines.extend(
                    wrapped_lines(reason, ((tooltip_width - 24.0) / 8.0) as usize)
                        .into_iter()
                        .map(|line| (line, 0xf0b18a)),
                );
            } else {
                lines.push((format!("{} or click", button.key), 0x9bdd97));
            }
            let height = 20.0 + lines.len() as f64 * 14.0;
            let x = size[0] - tooltip_width - 12.0;
            let y = (bottom - height - 28.0).max(HEADER);
            canvas.rect(x, y, tooltip_width, height, 0x19221d);
            canvas.outline(x, y, tooltip_width, height, 0x9a9a68);
            for (index, (line, color)) in lines.iter().enumerate() {
                canvas.text(line, x + 12.0, y + 10.0 + index as f64 * 14.0, 1.0, *color);
            }
        } else if let (container, members) =
            crate::selection::panel_members(self.world, self.selected)
            && (container.is_some() || members.len() > 1)
            && let Some((slot, entity)) = members
                .into_iter()
                .take(crate::SELECTION_LIMIT)
                .enumerate()
                .find(|(slot, _)| contains(selection_rect(*slot, size, native), self.cursor))
        {
            let [left, _, _, _] = selection_rect(slot, size, native);
            let width = 320.0_f64.min(size[0] - 24.0);
            let x = left.min(size[0] - width - 12.0);
            let y = bottom - 68.0;
            canvas.rect(x, y, width, 48.0, 0x19221d);
            canvas.outline(x, y, width, 48.0, 0x9a9a68);
            let name = self.presentation.unit_name(self.assets, entity.unit_type);
            let hp = self.world.unit_type(entity.unit_type).unwrap().max_hp;
            canvas.text(
                &shorten(
                    &format!("{name}  HP {}/{hp}", entity.hp),
                    ((width - 20.0) / 8.0) as usize,
                ),
                x + 10.0,
                y + 10.0,
                1.0,
                text,
            );
            canvas.text(
                if container.is_some() {
                    "CLICK UNLOADS THIS PASSENGER"
                } else {
                    "CLICK SELECTS / SHIFT REMOVES"
                },
                x + 10.0,
                y + 28.0,
                1.0,
                0x9bdd97,
            );
        }
    }
}
