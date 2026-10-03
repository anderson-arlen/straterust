use super::*;

impl App {
    pub(super) fn builder(&self, unit_type: UnitTypeId) -> Option<EntityId> {
        self.world
            .state()
            .entities
            .iter()
            .find(|entity| {
                self.selected.contains(&entity.id)
                    && entity.owner == PlayerId(0)
                    && self
                        .world
                        .unit_type(entity.unit_type)
                        .is_some_and(|unit| unit.builds.contains(&unit_type))
            })
            .map(|entity| entity.id)
    }

    pub fn buttons(&self) -> Vec<Button> {
        let bindings = &self.config.bindings;
        let mut builds = BTreeSet::new();
        let mut trains = BTreeSet::new();
        let mut mobile = false;
        let mut ground_mobile = false;
        let mut liftable = false;
        let mut airborne = false;
        let mut worker = false;
        let mut repairer = false;
        let mut structure = false;
        let mut cancel = false;
        let mut facilities = BTreeSet::new();
        let mut stim = false;
        let mut scanner = false;
        let mut cloak = false;
        let mut cloaked = false;
        let mut garrison = false;
        let mut mine_layer = false;
        for entity in self
            .world
            .state()
            .entities
            .iter()
            .filter(|entity| self.selected.contains(&entity.id) && entity.owner == PlayerId(0))
        {
            let unit = self.world.unit_type(entity.unit_type).unwrap();
            if !entity.airborne {
                facilities.insert(entity.unit_type);
            }
            liftable |= unit.flight.is_some() && !entity.airborne;
            airborne |= entity.airborne;
            ground_mobile |= !unit.structure && unit.mine.is_none();
            scanner |= unit.scanner.is_some();
            cloak |= unit.cloak.is_some();
            cloaked |= entity.cloaked;
            garrison |= unit.garrison.is_some();
            mine_layer |= unit.mine_layer.is_some();
            stim |= self.world.rules().research.iter().any(|research| matches!(&research.effect, ResearchEffect::Stim { units, .. } if units.contains(&entity.unit_type)));
            mobile |= (!unit.structure && unit.mine.is_none()) || entity.airborne;
            worker |= unit.worker.is_some();
            repairer |= !unit.repairs.is_empty();
            structure |= unit.structure && !unit.trains.is_empty();
            if !entity.airborne {
                builds.extend(
                    unit.builds
                        .iter()
                        .copied()
                        .filter(|id| self.world.creation_allowed(entity.owner, *id)),
                );
                trains.extend(
                    unit.trains
                        .iter()
                        .copied()
                        .filter(|id| self.world.creation_allowed(entity.owner, *id)),
                );
            }
            cancel |= entity.construction.is_some()
                || !entity.production.is_empty()
                || entity.research.is_some()
                || self.world.addon_pending(entity.id);
        }
        let plain = |slot, action, name: &str, key: &str, tip: &str| Button {
            action,
            slot,
            label: name.into(),
            key: key.into(),
            tooltip: vec![tip.into()],
            disabled: None,
        };
        if self.target_mode.is_some() {
            return vec![plain(
                8,
                Action::Cancel,
                "Cancel",
                &bindings.cancel,
                "Cancel targeting (also Escape or right-click).",
            )];
        }
        if self.build_menu {
            let keys = [
                &bindings.build_1,
                &bindings.build_2,
                &bindings.build_3,
                &bindings.build_4,
                &bindings.build_5,
                &bindings.build_6,
                &bindings.build_7,
                &bindings.build_8,
            ];
            let mut buttons = Vec::new();
            for (index, id) in builds.into_iter().enumerate() {
                if let Some(entry) = self.presentation.build_buttons.get(&id) {
                    if entry.advanced == self.advanced_build_menu
                        && self.world.creation_allowed(PlayerId(0), id)
                    {
                        buttons.push(self.unit_button(
                            usize::from(entry.slot),
                            Action::Build(id),
                            id,
                            &entry.key,
                        ));
                    }
                } else if self.presentation.build_buttons.is_empty()
                    && !self.advanced_build_menu
                    && index < 8
                {
                    buttons.push(self.unit_button(index, Action::Build(id), id, keys[index]));
                }
            }
            buttons.push(plain(
                8,
                Action::Back,
                "Back",
                &bindings.cancel,
                "Return to unit commands (also Escape).",
            ));
            return buttons;
        }
        let mut buttons = Vec::new();
        let source_worker = worker && !self.presentation.build_buttons.is_empty();
        if mobile {
            for (slot, action, name, key, tip) in [
                (
                    0,
                    Action::Move,
                    "Move",
                    &bindings.move_unit,
                    "Click a destination. Shift adds an order to the queue.",
                ),
                (
                    1,
                    Action::Stop,
                    "Stop",
                    &bindings.stop,
                    "Stop moving or working and clear queued orders.",
                ),
                (
                    2,
                    Action::Hold,
                    "Hold",
                    &bindings.hold,
                    "Hold this position and attack nearby enemies.",
                ),
                (
                    3,
                    Action::AttackMove,
                    "Attack",
                    &bindings.attack,
                    "Click an enemy to attack, or terrain to move and engage enemies.",
                ),
                (
                    4,
                    Action::Patrol,
                    "Patrol",
                    &bindings.patrol,
                    "Click a destination to patrol between it and this position.",
                ),
            ] {
                if source_worker && matches!(action, Action::Hold | Action::Patrol) {
                    continue;
                }
                let slot = if source_worker && action == Action::AttackMove {
                    2
                } else {
                    slot
                };
                if (self.is_rts() && ground_mobile) || matches!(action, Action::Move | Action::Stop)
                {
                    buttons.push(plain(slot, action, name, key, tip));
                }
            }
            if worker {
                buttons.push(plain(
                    if source_worker { 4 } else { 5 },
                    Action::Gather,
                    "Gather",
                    &bindings.gather,
                    "Click minerals or an owned extractor to gather. Right-click also works.",
                ));
            }
            if !builds.is_empty() {
                buttons.push(plain(
                    6,
                    Action::BuildMenu,
                    "Build",
                    &bindings.build_menu,
                    "Choose a structure. Costs and completed prerequisites appear in this menu.",
                ));
            }
            if builds.iter().any(|id| {
                self.presentation
                    .build_buttons
                    .get(id)
                    .is_some_and(|entry| entry.advanced)
            }) {
                buttons.push(plain(
                    7,
                    Action::AdvancedBuildMenu,
                    "Advanced structures",
                    &bindings.advanced_build_menu,
                    "Choose an advanced structure.",
                ));
            }
            if cloak {
                let enabled = !cloaked;
                let mut button = plain(
                    7,
                    Action::Cloak(enabled),
                    if enabled { "Cloak" } else { "Decloak" },
                    if enabled { "C" } else { "D" },
                    "Personal cloaking conceals this unit until detected. Costs 25 energy and drains energy while active.",
                );
                if self
                    .selected
                    .iter()
                    .all(|id| self.world.cloak_rejection(*id, enabled).is_some())
                {
                    button.disabled =
                        Some("Requires cloak research, 25 energy and an available unit.".into());
                }
                buttons.push(button);
            }
            if stim && !worker {
                let mut button = plain(
                    5,
                    Action::Stim,
                    "Stim",
                    "T",
                    "Trade health for a temporary increase in movement and attack speed.",
                );
                if !self
                    .selected
                    .iter()
                    .any(|id| self.world.stim_rejection(*id).is_none())
                {
                    button.disabled =
                        Some("Requires completed research and sufficient health.".into());
                }
                buttons.push(button);
            }
            if mine_layer && !worker {
                let mut button = plain(
                    6,
                    Action::PlaceMine,
                    "Spider Mine",
                    "I",
                    "Place a mine on clear ground. Shift queues deployments; each vehicle has a limited supply.",
                );
                if self.selected.iter().all(|id| {
                    self.world
                        .state()
                        .entities
                        .iter()
                        .find(|entity| entity.id == *id)
                        .is_none_or(|entity| {
                            entity.mine_count == 0
                                || self.world.mine_rejection(entity.id, entity.position)
                                    == Some(Rejection::MissingPrerequisite)
                        })
                }) {
                    button.disabled =
                        Some("Requires Spider Mines research and remaining mines.".into());
                }
                buttons.push(button);
            }
            if repairer {
                buttons.push(plain(
                    if source_worker {3} else {7},
                    Action::Repair,
                    "Repair",
                    &bindings.repair,
                    "Restore a damaged friendly mechanical unit or structure. Costs resources. Shift queues.",
                ));
            }
        } else {
            for (slot, (id, key)) in trains
                .into_iter()
                .filter(|id| {
                    self.presentation.build_buttons.is_empty()
                        || self.world.creation_allowed(PlayerId(0), *id)
                })
                .take(3)
                .zip([&bindings.train_1, &bindings.train_2, &bindings.train_2])
                .enumerate()
            {
                let key = self.presentation.train_keys.get(&id).unwrap_or(key);
                buttons.push(self.unit_button(slot, Action::Train(id), id, key));
            }
            for (slot, id) in builds.into_iter().take(3).enumerate() {
                buttons.push(self.unit_button(slot + 2, Action::Build(id), id, &bindings.build_1));
            }
            for (slot, research) in self
                .world
                .rules()
                .research
                .iter()
                .filter(|research| facilities.contains(&research.facility))
                .take(5)
                .enumerate()
            {
                let name = self
                    .presentation
                    .research_names
                    .get(&research.id)
                    .cloned()
                    .unwrap_or_else(|| format!("Research {}", research.id.0));
                let key = self
                    .presentation
                    .research_keys
                    .get(&research.id)
                    .cloned()
                    .unwrap_or_else(|| ["W", "A", "U", "T", "E"][slot].into());
                let disabled = if self.world.has_research(PlayerId(0), research.id) {
                    Some("Already researched.".into())
                } else if self.selected.iter().all(|id| {
                    self.world
                        .research_rejection(PlayerId(0), *id, research.id)
                        .is_some()
                }) {
                    Some("Requires an idle completed facility and sufficient resources.".into())
                } else {
                    None
                };
                buttons.push(Button {
                    action: Action::Research(research.id),
                    slot,
                    label: name.clone(),
                    key,
                    tooltip: vec![
                        name,
                        research
                            .cost
                            .iter()
                            .map(|cost| format!("{} {}", cost.amount, cost.kind))
                            .collect::<Vec<_>>()
                            .join("  "),
                    ],
                    disabled,
                });
            }
            if garrison && !mobile {
                let mut button = plain(
                    0,
                    Action::Unload,
                    "Unload",
                    "U",
                    "Unload passengers into nearby clear ground. Click a passenger in the panel to unload only that unit.",
                );
                if self
                    .selected
                    .iter()
                    .all(|id| self.world.unload_rejection(*id).is_some())
                {
                    button.disabled = Some("No passengers to unload.".into());
                }
                buttons.push(button);
            }
            if scanner {
                let mut button = plain(
                    0,
                    Action::Scan,
                    "Scanner Sweep",
                    "S",
                    "Reveal a target area temporarily. Costs energy.",
                );
                button.disabled = self.scanner_rejection();
                buttons.push(button);
            }
        }
        if garrison && mobile {
            let mut button = plain(
                5,
                Action::Unload,
                "Unload All",
                "U",
                "Choose a destination to fly to and unload every passenger. Click a passenger icon to unload just that unit here.",
            );
            if self.selected.iter().all(|id| {
                !self
                    .world
                    .state()
                    .entities
                    .iter()
                    .any(|passenger| passenger.garrisoned_in == Some(*id))
            }) {
                button.disabled = Some("No passengers aboard.".into());
            }
            buttons.push(button);
        }
        if structure && !mobile {
            buttons.push(plain(
                    5,
                    Action::Rally,
                    "Rally",
                    &bindings.rally,
                    "Click a destination for newly trained units, or a resource for new workers to gather. Right-click also works.",
                ));
        }
        if liftable {
            let mut button = plain(
                7,
                Action::Lift,
                "Lift Off",
                "L",
                "Lift this structure to move it. Training and research must finish first.",
            );
            if self
                .selected
                .iter()
                .all(|id| self.world.lift_rejection(*id).is_some())
            {
                button.disabled = Some(
                    "Finish construction, training, research, or add-on construction first.".into(),
                );
            }
            buttons.push(button);
        }
        if airborne {
            buttons.push(plain(
                6,
                Action::Land,
                "Land",
                "L",
                "Choose clear, buildable ground to land this structure.",
            ));
        }
        if cancel {
            buttons.push(plain(
                8,
                Action::Cancel,
                "Cancel",
                &bindings.cancel,
                "Cancel construction or the last queued unit; its cost is refunded.",
            ));
        }
        buttons
    }

    pub(super) fn unit_button(
        &self,
        slot: usize,
        action: Action,
        id: UnitTypeId,
        key: &str,
    ) -> Button {
        let unit = self.world.unit_type(id).unwrap();
        let name = self.presentation.unit_name(self.assets.as_ref(), id);
        let verb = if matches!(action, Action::Build(_)) {
            "Build"
        } else {
            "Train"
        };
        let mut tooltip = vec![
            format!("{verb} {name}"),
            unit.cost
                .iter()
                .map(|a| format!("{} {}", a.amount, a.kind))
                .collect::<Vec<_>>()
                .join("  "),
        ];
        if unit.supply_used > 0 {
            tooltip.push(format!("Supply: {}", unit.supply_used));
        }
        if unit.supply_provided > 0 {
            tooltip.push(format!("Provides {} supply", unit.supply_provided));
        }
        if !unit.prerequisites.is_empty() {
            tooltip.push(format!(
                "Requires completed: {}",
                unit.prerequisites
                    .iter()
                    .map(|id| self.presentation.unit_name(self.assets.as_ref(), *id))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        Button {
            action,
            slot,
            label: name,
            key: key.into(),
            tooltip,
            disabled: self.unit_action_rejection(action, id),
        }
    }

    pub(super) fn unit_action_rejection(&self, action: Action, id: UnitTypeId) -> Option<String> {
        let unit = self.world.unit_type(id)?;
        let missing: Vec<_> = unit
            .prerequisites
            .iter()
            .filter(|required| {
                !self.world.state().entities.iter().any(|entity| {
                    entity.owner == PlayerId(0)
                        && entity.unit_type == **required
                        && entity.construction.is_none()
                })
            })
            .map(|id| self.presentation.unit_name(self.assets.as_ref(), *id))
            .collect();
        if !missing.is_empty() {
            return Some(format!("Requires completed {}", missing.join(", ")));
        }
        if unit.addon_parent.is_some() && matches!(action, Action::Build(_)) {
            let builders: Vec<_> = self
                .world
                .state()
                .entities
                .iter()
                .filter(|entity| {
                    self.selected.contains(&entity.id)
                        && self
                            .world
                            .unit_type(entity.unit_type)
                            .unwrap()
                            .builds
                            .contains(&id)
                })
                .collect();
            if builders.iter().all(|entity| {
                !entity.production.is_empty()
                    || entity.research.is_some()
                    || self.world.addon_pending(entity.id)
            }) {
                return Some("Finish training, research, or addon construction first".into());
            }
        }
        if matches!(action, Action::Train(_)) {
            let producers: Vec<_> = self
                .world
                .state()
                .entities
                .iter()
                .filter(|entity| {
                    self.selected.contains(&entity.id)
                        && self
                            .world
                            .unit_type(entity.unit_type)
                            .unwrap()
                            .trains
                            .contains(&id)
                })
                .collect();
            if producers.iter().all(|e| e.construction.is_some()) {
                return Some("Producer is still under construction".into());
            }
            if producers
                .iter()
                .all(|entity| entity.research.is_some() || self.world.addon_pending(entity.id))
            {
                return Some("Finish research or addon construction first".into());
            }
            if producers.iter().all(|producer| {
                unit.prerequisites.iter().any(|id| {
                    self.world
                        .unit_type(*id)
                        .is_some_and(|addon| addon.addon_parent == Some(producer.unit_type))
                        && !self.world.state().entities.iter().any(|addon| {
                            addon.parent == Some(producer.id)
                                && addon.unit_type == *id
                                && addon.construction.is_none()
                        })
                })
            }) {
                return Some("Requires a completed addon attached to this producer".into());
            }
            if producers
                .iter()
                .filter(|e| e.construction.is_none())
                .all(|e| e.production.len() >= 5)
            {
                return Some("Training queue is full (5 units)".into());
            }
            let (used, provided) = self.world.supply(PlayerId(0));
            if used + unit.supply_used > provided {
                return Some("Not enough supply; complete a supply structure".into());
            }
        }
        for cost in &unit.cost {
            let balance = self.world.resource_balance(PlayerId(0), &cost.kind);
            if balance < u64::from(cost.amount) {
                return Some(format!(
                    "Need {} more {} (cost {})",
                    u64::from(cost.amount) - balance,
                    cost.kind,
                    cost.amount
                ));
            }
        }
        None
    }

    pub(super) fn scanner_rejection(&self) -> Option<String> {
        let mut blocked = None;
        for entity in self
            .world
            .state()
            .entities
            .iter()
            .filter(|entity| self.selected.contains(&entity.id))
        {
            let Some(scanner) = &self.world.unit_type(entity.unit_type).unwrap().scanner else {
                continue;
            };
            let reason = self.world.scan_rejection(entity.id, entity.position)?;
            blocked.get_or_insert_with(|| match reason {
                Rejection::InsufficientResources => format!(
                    "Needs {} more energy (cost {}).",
                    (scanner.cost * 256)
                        .saturating_sub(entity.energy)
                        .div_ceil(256),
                    scanner.cost
                ),
                Rejection::MissingPrerequisite => {
                    "Scanner must be connected to its completed parent structure.".into()
                }
                Rejection::Unfinished => "Finish scanner construction first.".into(),
                _ => "Scanner is currently unavailable.".into(),
            });
        }
        blocked.or_else(|| Some("Select a completed scanner.".into()))
    }
}
