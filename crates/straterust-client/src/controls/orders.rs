use super::*;

impl App {
    pub fn is_rts(&self) -> bool {
        self.world
            .rules()
            .units
            .iter()
            .any(|unit| unit.structure || unit.worker.is_some() || unit.weapon.is_some())
    }

    pub fn shifted(&self) -> bool {
        self.keys.contains(&KeyCode::ShiftLeft) || self.keys.contains(&KeyCode::ShiftRight)
    }

    pub fn pan_minimap(&mut self, point: [f64; 2]) -> bool {
        let map = [self.world.map().width, self.world.map().height];
        if let Some(position) = minimap_position(
            point,
            self.logical_size(),
            map,
            native_ui(self.assets.as_ref()),
        ) {
            self.camera.x = f64::from(position.x);
            self.camera.y = f64::from(position.y);
            self.camera.clamp_to_map(map, self.logical_size());
            true
        } else {
            false
        }
    }

    // Lifted structures also accept movement orders when selected individually.
    pub(super) fn issue_mobile(&mut self, order: impl Fn(EntityId) -> Order) -> Result<()> {
        let ids: Vec<_> = self
            .selected
            .iter()
            .copied()
            .filter(|id| {
                self.world
                    .state()
                    .entities
                    .iter()
                    .find(|entity| entity.id == *id)
                    .is_some_and(|entity| {
                        !self.world.unit_type(entity.unit_type).unwrap().structure
                            || entity.airborne
                    })
            })
            .collect();
        for entity in ids {
            self.issue(order(entity))?;
        }
        Ok(())
    }

    pub fn activate(&mut self, action: Action) -> Result<()> {
        if let Action::Build(id) | Action::Train(id) = action
            && let Some(reason) = self.unit_action_rejection(action, id)
        {
            self.status = reason;
            self.audio.event(Cue::Error, None);
            return Ok(());
        }
        match action {
            Action::BuildMenu => {
                self.advanced_build_menu = false;
                self.build_menu = true;
                self.status = "Choose a structure. Hover for cost and requirements.".into();
            }
            Action::Back => {
                self.build_menu = false;
            }
            Action::AdvancedBuildMenu => {
                self.advanced_build_menu = true;
                self.build_menu = true;
                self.status = "Choose an advanced structure.".into();
            }
            Action::Move => {
                self.target_mode = Some(TargetMode::Move);
                self.status = "Click a destination. Shift queues movement.".into();
            }
            Action::Gather => {
                self.target_mode = Some(TargetMode::Gather);
                self.status = "Click minerals or an owned extractor to gather.".into();
            }
            Action::Repair => {
                self.target_mode = Some(TargetMode::Repair);
                self.status = "Click a damaged friendly mechanical unit or structure to repair. Shift queues.".into();
            }
            Action::Rally => {
                self.target_mode = Some(TargetMode::Rally);
                self.status = "Click a rally point for newly trained units.".into();
            }
            Action::Build(id) => {
                if self.builder(id).is_some() {
                    self.target_mode = Some(TargetMode::Build(id));
                    self.status = if self
                        .world
                        .unit_type(id)
                        .is_some_and(|unit| unit.addon_parent.is_some())
                    {
                        "Place the building and its addon on clear ground. It will lift and relocate if needed.".into()
                    } else {
                        format!(
                            "Place {} on a green footprint. Right-click/Escape cancels.",
                            self.presentation.unit_name(self.assets.as_ref(), id)
                        )
                    };
                }
            }
            Action::Train(id) => {
                if let Some(producer) = self
                    .world
                    .state()
                    .entities
                    .iter()
                    .find(|entity| {
                        self.selected.contains(&entity.id)
                            && entity.construction.is_none()
                            && entity.production.len() < 5
                            && self
                                .world
                                .unit_type(entity.unit_type)
                                .unwrap()
                                .trains
                                .contains(&id)
                    })
                    .map(|entity| entity.id)
                {
                    self.issue(Order::Train {
                        entity: producer,
                        unit_type: id,
                    })?;
                }
            }
            Action::Research(research) => {
                if let Some(entity) = self.selected.iter().copied().find(|id| {
                    self.world
                        .research_rejection(PlayerId(0), *id, research)
                        .is_none()
                }) {
                    self.issue(Order::Research { entity, research })?;
                } else {
                    self.status =
                        "Research requires an idle completed facility and sufficient resources."
                            .into();
                    self.audio.event(Cue::Error, None);
                }
            }
            Action::Cloak(enabled) => {
                let ids = self.selected.clone();
                for entity in ids {
                    if self.world.cloak_rejection(entity, enabled).is_none() {
                        self.issue(Order::Cloak { entity, enabled })?;
                    }
                }
            }
            Action::Stim => {
                let units: Vec<_> = self
                    .selected
                    .iter()
                    .copied()
                    .filter(|id| self.world.stim_rejection(*id).is_none())
                    .collect();
                for entity in units {
                    self.issue(Order::Stim { entity })?;
                }
            }
            Action::PlaceMine => {
                self.target_mode = Some(TargetMode::PlaceMine);
                self.status =
                    "Click clear ground to place a mine. Shift queues deployments.".into();
            }
            Action::Lift => {
                let units: Vec<_> = self
                    .selected
                    .iter()
                    .copied()
                    .filter(|id| self.world.lift_rejection(*id).is_none())
                    .collect();
                for entity in units {
                    self.issue(Order::Lift { entity })?;
                }
            }
            Action::Land => {
                self.target_mode = Some(TargetMode::Land);
                self.status =
                    "Choose clear, buildable ground. Shift queues the landing order.".into();
            }
            Action::Unload => {
                let bunkers: Vec<_> = self
                    .selected
                    .iter()
                    .copied()
                    .filter(|id| self.world.unload_rejection(*id).is_none())
                    .collect();
                for entity in bunkers {
                    self.issue(Order::Unload { entity })?;
                }
            }
            Action::Scan => {
                if let Some(reason) = self.scanner_rejection() {
                    self.status = reason;
                    self.audio.event(Cue::Error, None);
                } else {
                    self.target_mode = Some(TargetMode::Scan);
                    self.status = "Click an area to reveal with Scanner Sweep.".into();
                }
            }
            Action::AttackMove => {
                self.target_mode = Some(TargetMode::AttackMove);
                self.status =
                    "Click an enemy to attack, or terrain to attack-move. Shift queues.".into();
            }
            Action::Patrol => {
                self.target_mode = Some(TargetMode::Patrol);
                self.status =
                    "Click a patrol destination. Shift queues; right-click cancels.".into();
            }
            Action::Hold => self.issue_mobile(|entity| Order::Hold { entity })?,
            Action::Stop => self.issue_mobile(|entity| Order::Stop { entity })?,
            Action::Cancel => {
                if self.target_mode.take().is_some() {
                    self.status = "Targeting cancelled.".into();
                } else {
                    let entities: Vec<_> = self
                        .world
                        .state()
                        .entities
                        .iter()
                        .filter(|entity| {
                            self.selected.contains(&entity.id)
                                && (entity.construction.is_some()
                                    || !entity.production.is_empty()
                                    || entity.research.is_some())
                        })
                        .map(|entity| entity.id)
                        .collect();
                    for entity in entities {
                        self.issue(Order::Cancel { entity })?;
                    }
                }
            }
        }
        Ok(())
    }

    pub fn targeting_click(&mut self, position: Position) -> Result<()> {
        match self.target_mode {
            Some(TargetMode::PlaceMine) => {
                if let Some(entity) = self
                    .selected
                    .iter()
                    .copied()
                    .find(|id| self.world.mine_rejection(*id, position).is_none())
                {
                    self.issue(Order::PlaceMine {
                        entity,
                        target: position,
                    })?;
                } else {
                    self.status = "Choose clear ground and a vehicle with mines remaining.".into();
                    self.audio.event(Cue::Error, None);
                    return Ok(());
                }
            }
            Some(TargetMode::Land) => {
                let Some(entity) = self
                    .world
                    .state()
                    .entities
                    .iter()
                    .find(|entity| self.selected.contains(&entity.id) && entity.airborne)
                else {
                    return Ok(());
                };
                let target = snap_build_position(
                    position,
                    self.world.unit_type(entity.unit_type).unwrap().placement,
                );
                if let Some(reason) = self.world.land_rejection(entity.id, target) {
                    self.status = format!("Cannot land here: {reason:?}.");
                    self.audio.event(Cue::Error, None);
                    return Ok(());
                }
                self.issue(Order::Land {
                    entity: entity.id,
                    target,
                })?;
            }
            Some(TargetMode::Scan) => {
                if let Some(entity) = self
                    .selected
                    .iter()
                    .copied()
                    .find(|id| self.world.scan_rejection(*id, position).is_none())
                {
                    self.issue(Order::Scan {
                        entity,
                        target: position,
                    })?;
                } else {
                    self.status =
                        "Scanner Sweep needs a completed scanner with sufficient energy.".into();
                    self.audio.event(Cue::Error, None);
                    return Ok(());
                }
            }
            Some(TargetMode::Move) => self.issue_mobile(|entity| Order::Move {
                entity,
                target: position,
            })?,
            Some(TargetMode::Rally) => {
                for entity in self.selected.clone() {
                    if self.world.state().entities.iter().any(|actor| {
                        actor.id == entity
                            && !self
                                .world
                                .unit_type(actor.unit_type)
                                .unwrap()
                                .trains
                                .is_empty()
                    }) {
                        self.issue(self.rally_order(entity, position))?;
                    }
                }
            }
            Some(TargetMode::Gather) => {
                let resource = self.resource_at(position);
                if let Some(resource) = resource {
                    let workers: Vec<_> = self
                        .selected
                        .iter()
                        .copied()
                        .filter(|id| {
                            self.world
                                .state()
                                .entities
                                .iter()
                                .find(|entity| entity.id == *id)
                                .is_some_and(|entity| {
                                    self.world
                                        .unit_type(entity.unit_type)
                                        .unwrap()
                                        .worker
                                        .is_some()
                                })
                        })
                        .collect();
                    for entity in workers {
                        if self.world.gather_rejection(entity, resource).is_none() {
                            self.issue(Order::Gather { entity, resource })?;
                        } else {
                            self.status = "Gas requires a completed friendly extractor; minerals must have resources remaining.".into();
                            self.audio.event(Cue::Error, None);
                        }
                    }
                } else {
                    self.status = "Choose minerals or a completed friendly extractor.".into();
                    return Ok(());
                }
            }
            Some(TargetMode::Repair) => {
                let target = self.entity_at(position);
                let workers: Vec<_> = self
                    .selected
                    .iter()
                    .copied()
                    .filter_map(|entity| {
                        let target = target?;
                        self.world
                            .repair_rejection(entity, target)
                            .is_none()
                            .then_some((entity, target))
                    })
                    .collect();
                if workers.is_empty() {
                    self.status = "Choose a damaged, completed friendly unit that the selected worker can repair.".into();
                    self.audio.event(Cue::Error, None);
                    return Ok(());
                }
                for (entity, target) in workers {
                    self.issue(Order::Repair { entity, target })?;
                }
            }
            Some(TargetMode::Build(unit_type)) => {
                let position = self.build_position(unit_type, position);
                if let Some(entity) = self.builder(unit_type) {
                    if let Some(reason) =
                        self.world
                            .build_rejection(PlayerId(0), entity, unit_type, position)
                    {
                        let message = match reason {
                            Rejection::InvalidPlacement => {
                                "space is occupied or terrain is not buildable".to_string()
                            }
                            Rejection::OutOfBounds => {
                                "the entire footprint must be inside the map".to_string()
                            }
                            Rejection::InsufficientResources | Rejection::MissingPrerequisite => {
                                self.unit_action_rejection(Action::Build(unit_type), unit_type)
                                    .unwrap_or_else(|| {
                                        "requirements changed; choose the structure again".into()
                                    })
                            }
                            _ => "select an available worker and choose the structure again".into(),
                        };
                        self.status = format!("Cannot build here: {message}.");
                        self.audio.event(Cue::Error, None);
                        return Ok(());
                    }
                    self.issue(Order::Build {
                        entity,
                        unit_type,
                        position,
                    })?;
                }
            }
            Some(TargetMode::AttackMove) => {
                if let Some(target) = self.entity_at(position).filter(|id| {
                    self.world.state().entities.iter().any(|entity| {
                        entity.id == *id && self.world.is_enemy(PlayerId(0), entity.owner)
                    })
                }) {
                    self.issue_mobile(|entity| Order::Attack { entity, target })?;
                } else {
                    self.issue_mobile(|entity| Order::AttackMove {
                        entity,
                        target: position,
                    })?;
                }
            }
            Some(TargetMode::Patrol) => self.issue_mobile(|entity| Order::Patrol {
                entity,
                target: position,
            })?,
            None => {}
        }
        self.target_mode = None;
        self.build_menu = false;
        Ok(())
    }

    pub(super) fn build_position(&self, unit_type: UnitTypeId, position: Position) -> Position {
        let unit = self.world.unit_type(unit_type).unwrap();
        if let Some(kind) = &unit.extracts
            && let Some(resource) = self
                .world
                .state()
                .resources
                .iter()
                .filter(|resource| {
                    resource.kind == kind.resource
                        && resource.requires_extractor
                        && self.world.visibility(PlayerId(0), resource.position)
                            != straterust_engine::sim::Visibility::Unexplored
                        && (resource.position.x - position.x).abs()
                            <= i32::from(unit.placement.width / 2)
                        && (resource.position.y - position.y).abs()
                            <= i32::from(unit.placement.height / 2)
                })
                .min_by_key(|resource| {
                    let dx = i64::from(resource.position.x) - i64::from(position.x);
                    let dy = i64::from(resource.position.y) - i64::from(position.y);
                    (dx * dx + dy * dy, resource.id)
                })
        {
            return resource.position;
        }
        snap_build_position(position, unit.placement)
    }

    pub fn placement(&self) -> Option<(UnitTypeId, Position, bool)> {
        let mode = self.target_mode?;
        let cursor = self
            .camera
            .screen_to_world(self.logical_cursor(), self.logical_size())?;
        match mode {
            TargetMode::Build(unit_type) => {
                let position = self.build_position(unit_type, cursor);
                let builder = self.builder(unit_type)?;
                Some((
                    unit_type,
                    position,
                    self.world
                        .build_rejection(PlayerId(0), builder, unit_type, position)
                        .is_none(),
                ))
            }
            TargetMode::Land => {
                let entity = self
                    .world
                    .state()
                    .entities
                    .iter()
                    .find(|entity| self.selected.contains(&entity.id) && entity.airborne)?;
                let position =
                    snap_build_position(cursor, self.world.unit_type(entity.unit_type)?.placement);
                Some((
                    entity.unit_type,
                    position,
                    self.world.land_rejection(entity.id, position).is_none(),
                ))
            }
            _ => None,
        }
    }

    pub(super) fn entity_at(&self, position: Position) -> Option<EntityId> {
        crate::selection::entity_at(&self.world, &self.presentation, position, self.camera.zoom)
    }

    // Resources are drawn above their ground footprint. Picking only collision
    // bounds misses visible crystal tips and silently turns gather rallies into
    // position rallies. Use the actual anchored image, retaining the footprint
    // for original geometry and exhausted extractor targets.
    pub(crate) fn resource_at(&self, position: Position) -> Option<ResourceId> {
        crate::selection::resource_at(&self.world, self.assets.as_ref(), position)
    }

    pub(super) fn rally_order(&self, entity: EntityId, position: Position) -> Order {
        let resource = self.resource_at(position);
        if let Some(resource) = resource {
            Order::RallyResource { entity, resource }
        } else {
            Order::Rally {
                entity,
                target: position,
            }
        }
    }

    pub fn contextual_order(&mut self, position: Position) -> Result<()> {
        self.build_menu = false;
        if self.target_mode.take().is_some() {
            self.status = "Targeting cancelled.".into();
            return Ok(());
        }
        let target = self
            .entity_at(position)
            .and_then(|id| {
                self.world
                    .state()
                    .entities
                    .iter()
                    .find(|entity| entity.id == id)
            })
            .cloned();
        let resource = self.resource_at(position);
        for entity in self.selected.clone() {
            let Some(selected) = self
                .world
                .state()
                .entities
                .iter()
                .find(|unit| unit.id == entity)
            else {
                continue;
            };
            let definition = self.world.unit_type(selected.unit_type).unwrap();
            let order = if selected.airborne {
                Order::Move {
                    entity,
                    target: position,
                }
            } else if definition.structure {
                if definition.trains.is_empty() {
                    continue;
                }
                self.rally_order(entity, position)
            } else if let Some(target) = target
                .as_ref()
                .filter(|target| self.world.is_enemy(PlayerId(0), target.owner))
            {
                Order::Attack {
                    entity,
                    target: target.id,
                }
            } else if let Some(target) = target
                .as_ref()
                .filter(|target| self.world.load_rejection(entity, target.id).is_none())
            {
                Order::Load {
                    entity,
                    target: target.id,
                }
            } else if definition.worker.is_some() || !definition.repairs.is_empty() {
                if let Some(target) = target
                    .as_ref()
                    .filter(|target| target.construction.is_some() && target.owner == PlayerId(0))
                {
                    Order::Resume {
                        entity,
                        building: target.id,
                    }
                } else if let Some(target) = target
                    .as_ref()
                    .filter(|target| self.world.repair_rejection(entity, target.id).is_none())
                {
                    Order::Repair {
                        entity,
                        target: target.id,
                    }
                } else if let Some(resource) = resource {
                    if self.world.gather_rejection(entity, resource).is_some() {
                        self.status = "Gas requires a completed friendly extractor; minerals must have resources remaining.".into();
                        self.audio.event(Cue::Error, None);
                        continue;
                    }
                    Order::Gather { entity, resource }
                } else {
                    Order::Move {
                        entity,
                        target: position,
                    }
                }
            } else {
                Order::Move {
                    entity,
                    target: position,
                }
            };
            let feedback = match &order {
                Order::Attack { target, .. }
                | Order::Load { target, .. }
                | Order::Repair { target, .. } => crate::visual::CommandTarget::Entity(*target),
                Order::Resume { building, .. } => crate::visual::CommandTarget::Entity(*building),
                Order::Gather { resource, .. } | Order::RallyResource { resource, .. } => {
                    crate::visual::CommandTarget::resource(&self.world, *resource)
                }
                _ => target
                    .as_ref()
                    .map_or(crate::visual::CommandTarget::Ground(position), |target| {
                        crate::visual::CommandTarget::Entity(target.id)
                    }),
            };
            let before = self.recorded.len();
            self.issue(order)?;
            if self.recorded.len() > before {
                self.visuals.show_command_feedback(feedback);
            }
        }
        Ok(())
    }

    pub fn bound_key(&mut self, key: KeyCode) -> Result<bool> {
        if key == KeyCode::Tab {
            self.selected_resource = None;
            let current = self.selected.last().copied();
            let owned = || {
                self.world.state().entities.iter().filter(|entity| {
                    entity.owner == PlayerId(0) && self.world.entity_visible(PlayerId(0), entity.id)
                })
            };
            if let Some(next) = owned()
                .find(|entity| current.is_none_or(|id| entity.id > id))
                .or_else(|| owned().next())
            {
                self.selected = BTreeSet::from([next.id]);
                self.camera.x = f64::from(next.position.x);
                self.camera.y = f64::from(next.position.y);
                self.camera.clamp_to_map(
                    [self.world.map().width, self.world.map().height],
                    self.logical_size(),
                );
                self.status = format!(
                    "Selected {}. Tab selects the next owned unit.",
                    self.presentation
                        .unit_name(self.assets.as_ref(), next.unit_type)
                );
                self.target_mode = None;
                self.build_menu = false;
                self.drag_start = None;
                self.selection_sound();
            } else {
                self.selected.clear();
            }
            return Ok(true);
        }
        if let Some(group) = group_index(key) {
            if self.keys.contains(&KeyCode::ControlLeft)
                || self.keys.contains(&KeyCode::ControlRight)
            {
                self.groups[group] = self.selected.clone();
                self.status = format!(
                    "Stored {} units in group {}.",
                    self.selected.len(),
                    (group + 1) % 10
                );
            } else {
                self.selected_resource = None;
                self.selected = self.groups[group]
                    .iter()
                    .copied()
                    .filter(|id| {
                        self.world.state().entities.iter().any(|entity| {
                            entity.id == *id
                                && entity.owner == PlayerId(0)
                                && self.world.entity_visible(PlayerId(0), entity.id)
                        })
                    })
                    .collect();
                self.status = format!(
                    "Recalled group {}: {} units.",
                    (group + 1) % 10,
                    self.selected.len()
                );
            }
            self.build_menu = false;
            self.target_mode = None;
            self.selection_sound();
            return Ok(true);
        }
        if key == KeyCode::Enter && self.advance_campaign()? {
            return Ok(true);
        }
        if parse_key(&self.config.bindings.restart) == Some(key) {
            self.restart()?;
            return Ok(true);
        }
        if parse_key(&self.config.bindings.pause) == Some(key) {
            self.paused = !self.paused;
            return Ok(true);
        }
        if parse_key(&self.config.bindings.home) == Some(key) {
            let anchor = home_position(&self.world);
            self.camera.x = f64::from(anchor.x);
            self.camera.y = f64::from(anchor.y);
            return Ok(true);
        }
        if let Some(action) = self
            .buttons()
            .iter()
            .find(|button| parse_key(&button.key) == Some(key))
            .map(|button| button.action)
        {
            self.activate(action)?;
            return Ok(true);
        }
        Ok(false)
    }
}
