use super::*;

pub(super) fn entity_at(
    world: &World,
    presentation: &view::Presentation,
    position: Position,
    zoom: f64,
) -> Option<EntityId> {
    world
        .state()
        .entities
        .iter()
        .filter(|entity| world.entity_visible(world.view_player(), entity.id))
        .filter_map(|entity| {
            let half = view::unit_half_size(world, entity.unit_type, presentation);
            let dx = i64::from(entity.position.x) - i64::from(position.x);
            let dy = i64::from(entity.position.y) - i64::from(position.y);
            ((dx.abs() as f64 <= half[0] + 5.0 / zoom) && (dy.abs() as f64 <= half[1] + 5.0 / zoom))
                .then_some((dx * dx + dy * dy, entity.id))
        })
        .min()
        .map(|(_, id)| id)
}

pub(super) fn resource_at(
    world: &World,
    assets: Option<&AssetPack>,
    position: Position,
) -> Option<ResourceId> {
    world
        .state()
        .resources
        .iter()
        .filter(|resource| {
            (resource.amount > 0 || resource.requires_extractor)
                && world.visibility(world.view_player(), resource.position)
                    != straterust_engine::sim::Visibility::Unexplored
        })
        .filter_map(|resource| {
            let dx = i64::from(position.x) - i64::from(resource.position.x);
            let dy = i64::from(position.y) - i64::from(resource.position.y);
            let footprint_hit = dx.abs() <= i64::from(resource.footprint.width / 2).max(12)
                && dy.abs() <= i64::from(resource.footprint.height / 2).max(12);
            let image_hit = resource.amount > 0
                && assets.is_some_and(|assets| {
                    assets
                        .resources
                        .iter()
                        .find(|art| art.manifest.kind == resource.kind)
                        .is_some_and(|art| {
                            let x = dx + i64::from(art.manifest.anchor[0]);
                            let y = dy + i64::from(art.manifest.anchor[1]);
                            x >= 0
                                && y >= 0
                                && x < i64::from(art.image.width)
                                && y < i64::from(art.image.height)
                                && art.image.rgba
                                    [((y as usize * art.image.width as usize + x as usize) * 4) + 3]
                                    != 0
                        })
                });
            (footprint_hit || image_hit).then_some((dx * dx + dy * dy, resource.id))
        })
        .min()
        .map(|(_, id)| id)
}

pub(super) fn panel_members<'a>(
    world: &'a World,
    selected: &BTreeSet<EntityId>,
) -> (Option<EntityId>, Vec<&'a straterust_engine::sim::Entity>) {
    let container = if selected.len() == 1 {
        world
            .state()
            .entities
            .iter()
            .find(|entity| {
                selected.contains(&entity.id)
                    && entity.owner == world.view_player()
                    && world
                        .unit_type(entity.unit_type)
                        .unwrap()
                        .garrison
                        .is_some()
            })
            .map(|entity| entity.id)
    } else {
        None
    };
    let members = world
        .state()
        .entities
        .iter()
        .filter(|entity| {
            if let Some(container) = container {
                entity.garrisoned_in == Some(container)
            } else {
                selected.contains(&entity.id)
            }
        })
        .collect();
    (container, members)
}

pub(super) fn is_selection_drag(start: [f64; 2], end: [f64; 2]) -> bool {
    (end[0] - start[0]).abs().max((end[1] - start[1]).abs()) >= 8.0
}

impl App {
    pub(super) fn selected_unit_type(&self) -> Option<UnitTypeId> {
        self.selected
            .first()
            .and_then(|id| {
                self.world
                    .state()
                    .entities
                    .iter()
                    .find(|entity| entity.id == *id)
            })
            .map(|entity| entity.unit_type)
    }

    pub(super) fn selection_sound(&mut self) {
        if self.world.state().entities.iter().any(|entity| {
            self.selected.contains(&entity.id) && entity.owner == self.world.view_player()
        }) && let Some(unit_type) = self.selected_unit_type()
        {
            self.audio.event(Cue::Select, Some(unit_type));
        }
    }

    pub(super) fn logical_cursor(&self) -> [f64; 2] {
        let scale = self
            .window
            .as_ref()
            .map_or(1.0, |window| window.scale_factor());
        [self.cursor.x / scale, self.cursor.y / scale]
    }

    pub(super) fn logical_size(&self) -> [f64; 2] {
        let Some(window) = self.window.as_ref() else {
            return [f64::from(self.config.width), f64::from(self.config.height)];
        };
        let scale = window.scale_factor();
        let size = window.inner_size();
        [
            f64::from(size.width) / scale,
            f64::from(size.height) / scale,
        ]
    }

    pub(super) fn select_screen(
        &mut self,
        start: [f64; 2],
        end: [f64; 2],
        size: [f64; 2],
        now: Instant,
    ) {
        self.build_menu = false;
        if size[0] <= 1.0 || size[1] <= view::HEADER + view::FOOTER + 1.0 {
            self.last_selection_click = None;
            return;
        }
        let Some(start_world) = self.camera.screen_to_world(start, size) else {
            self.last_selection_click = None;
            return;
        };
        let previous = self.selected.clone();
        let end = [
            end[0].clamp(0.0, size[0] - 1.0),
            end[1].clamp(view::HEADER, size[1] - view::FOOTER - 1.0),
        ];
        let end_world = self.camera.screen_to_world(end, size).unwrap();
        let mut candidates = Vec::new();
        if !is_selection_drag(start, end) {
            self.select_at(end_world);
            if self.selected_resource.is_some() {
                self.last_selection_click = None;
                return;
            }
            let picked = self.selected.first().copied();
            if picked.is_some_and(|id| {
                self.world.state().entities.iter().any(|entity| {
                    entity.id == id
                        && (entity.owner != self.world.view_player()
                            || self.world.unit_type(entity.unit_type).unwrap().structure)
                })
            }) {
                self.last_selection_click = None;
                self.selection_sound();
                return;
            }
            let double_click = picked.is_some_and(|id| {
                self.last_selection_click
                    .is_some_and(|(time, previous, point)| {
                        id == previous
                            && now.saturating_duration_since(time) <= Duration::from_millis(350)
                            && (end[0] - point[0]).abs().max((end[1] - point[1]).abs()) <= 6.0
                    })
            });
            self.last_selection_click = if double_click {
                None
            } else {
                picked.map(|id| (now, id, end))
            };
            if double_click {
                let source = self
                    .world
                    .state()
                    .entities
                    .iter()
                    .find(|e| Some(e.id) == picked)
                    .unwrap();
                candidates = self
                    .world
                    .state()
                    .entities
                    .iter()
                    .filter(|entity| {
                        let screen = self.camera.world_to_screen(
                            f64::from(entity.position.x),
                            f64::from(entity.position.y),
                            size,
                        );
                        let half =
                            unit_half_size(&self.world, entity.unit_type, &self.presentation);
                        entity.owner == source.owner
                            && entity.unit_type == source.unit_type
                            && self
                                .world
                                .entity_visible(self.world.view_player(), entity.id)
                            && (entity.id == source.id
                                || (screen[0] + half[0] * self.camera.zoom >= 0.0
                                    && screen[0] - half[0] * self.camera.zoom < size[0]
                                    && screen[1] + half[1] * self.camera.zoom >= view::HEADER
                                    && screen[1] - half[1] * self.camera.zoom
                                        < size[1] - view::FOOTER))
                    })
                    .map(|entity| {
                        let dx = i64::from(entity.position.x) - i64::from(source.position.x);
                        let dy = i64::from(entity.position.y) - i64::from(source.position.y);
                        (dx * dx + dy * dy, entity.id)
                    })
                    .collect();
            } else {
                candidates.extend(picked.map(|id| (0, id)));
            }
        } else {
            self.selected_resource = None;
            self.last_selection_click = None;
            let center_x = (i64::from(start_world.x) + i64::from(end_world.x)) / 2;
            let center_y = (i64::from(start_world.y) + i64::from(end_world.y)) / 2;
            candidates = self
                .world
                .state()
                .entities
                .iter()
                .filter(|entity| {
                    entity.owner == self.world.view_player()
                        && self
                            .world
                            .entity_visible(self.world.view_player(), entity.id)
                        && !self.world.unit_type(entity.unit_type).unwrap().structure
                })
                .filter(|entity| {
                    entity.position.x >= start_world.x.min(end_world.x)
                        && entity.position.x <= start_world.x.max(end_world.x)
                        && entity.position.y >= start_world.y.min(end_world.y)
                        && entity.position.y <= start_world.y.max(end_world.y)
                })
                .map(|entity| {
                    let dx = i64::from(entity.position.x) - center_x;
                    let dy = i64::from(entity.position.y) - center_y;
                    (dx * dx + dy * dy, entity.id)
                })
                .collect();
        }
        candidates.sort_unstable();
        self.selected = if self.shifted() {
            previous
        } else {
            BTreeSet::new()
        };
        self.selected.retain(|id| {
            self.world.state().entities.iter().any(|entity| {
                entity.id == *id
                    && entity.owner == self.world.view_player()
                    && !self.world.unit_type(entity.unit_type).unwrap().structure
            })
        });
        for (_, id) in candidates {
            if self.selected.len() >= SELECTION_LIMIT {
                break;
            }
            self.selected.insert(id);
        }
        self.status = format!(
            "{} selected. Right-click: gather, resume, attack, move or set rally.",
            self.selected.len()
        );
        self.selection_sound();
    }

    pub(super) fn select_panel(&mut self, cursor: [f64; 2], size: [f64; 2]) -> bool {
        let (container, members) = panel_members(&self.world, &self.selected);
        if container.is_none() && members.len() < 2 {
            return false;
        }
        let Some(id) = members
            .into_iter()
            .take(SELECTION_LIMIT)
            .enumerate()
            .find(|(slot, _)| {
                controls::contains(
                    controls::selection_rect(
                        *slot,
                        size,
                        controls::native_ui(self.assets.as_ref()),
                    ),
                    cursor,
                )
            })
            .map(|(_, entity)| entity.id)
        else {
            return false;
        };
        if let Some(container) = container {
            self.status = match self.issue(Order::UnloadPassenger {
                entity: container,
                passenger: id,
            }) {
                Ok(()) => "Unloading selected passenger.".into(),
                Err(error) => error.to_string(),
            };
            self.drag_start = None;
            return true;
        }
        if self.shifted() {
            self.selected.remove(&id);
        } else {
            self.selected = BTreeSet::from([id]);
        }
        self.build_menu = false;
        self.target_mode = None;
        self.drag_start = None;
        self.last_selection_click = None;
        self.status = format!(
            "{} selected. Shift-click a panel unit to remove it.",
            self.selected.len()
        );
        self.selection_sound();
        true
    }

    pub(super) fn select_at(&mut self, position: Position) {
        self.build_menu = false;
        self.target_mode = None;
        let picked = self
            .world
            .state()
            .entities
            .iter()
            .filter(|entity| {
                self.world
                    .entity_visible(self.world.view_player(), entity.id)
            })
            .filter_map(|entity| {
                let half = unit_half_size(&self.world, entity.unit_type, &self.presentation);
                let dx = i64::from(entity.position.x) - i64::from(position.x);
                let dy = i64::from(entity.position.y) - i64::from(position.y);
                let margin = 5.0 / self.camera.zoom;
                ((dx.abs() as f64 <= half[0] + margin) && (dy.abs() as f64 <= half[1] + margin))
                    .then_some((dx * dx + dy * dy, entity.id))
            })
            .min()
            .map(|(_, id)| id);
        self.selected = picked.into_iter().collect();
        self.selected_resource = if picked.is_none() {
            self.resource_at(position)
        } else {
            None
        };
    }

    pub(super) fn click_at(&mut self, button: MouseButton, position: Position) -> Result<()> {
        match button {
            MouseButton::Left => {
                self.select_at(position);
            }
            MouseButton::Right => {
                self.contextual_order(position)?;
            }
            _ => {}
        }
        Ok(())
    }
}
