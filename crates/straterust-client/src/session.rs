use super::*;

impl App {
    pub(super) fn new(
        package: &Package,
        config: Config,
        presentation: Presentation,
        assets: Option<AssetPack>,
        scenario: Option<Scenario>,
    ) -> Result<Self> {
        let world = package.world(scenario.as_ref().map_or(config.seed, |s| s.seed))?;
        if let Some(assets) = &assets {
            assets.validate_for_world(&world)?;
        }
        let queue = scenario
            .as_ref()
            .map(CommandQueue::from_scenario)
            .transpose()?
            .unwrap_or_default();
        let camera_anchor = home_position(&world);
        let mut camera = Camera {
            x: f64::from(camera_anchor.x),
            y: f64::from(camera_anchor.y),
            zoom: config.zoom,
        };
        camera.clamp_to_map(
            [world.map().width, world.map().height],
            [f64::from(config.width), f64::from(config.height)],
        );
        let clock = TickClock::new(world.rules().tick_ms);
        let status = if let Some(objective) = &presentation.objective {
            objective.clone()
        } else if world.map().mission.is_some() {
            "Select a unit to issue orders. Mission objectives appear below.".into()
        } else if world.rules().victory {
            "Fully revealed demo. Select a worker and right-click minerals; buttons build/train."
                .into()
        } else {
            assets.as_ref().map_or_else(
                || "Select a friendly unit, then right-click to move. Original fixture.".into(),
                |assets| {
                    if assets.manifest.terrain_grid.is_some() {
                        "Terrain and placement preview. Unit mechanics are placeholders.".into()
                    } else {
                        format!(
                            "{} art preview. Fixture movement and rules.",
                            assets.manifest.unit_name
                        )
                    }
                },
            )
        };
        let mut audio = Audio::new(config.audio);
        audio.reset(&world);
        let mut visuals = Visuals::new(&world);
        visuals.movement_heading_debounce_ms = config.movement_heading_debounce_ms;
        Ok(Self {
            campaign: None,
            initial_world: world.clone(),
            initial_scenario: scenario.clone(),
            visuals,
            world,
            simulation: None,
            presentation,
            assets,
            media: None,
            mission_ui: None,
            animation_elapsed: Duration::ZERO,
            portrait_elapsed: Duration::ZERO,
            audio,
            config,
            queue,
            recorded: Vec::new(),
            playback_end: scenario.map(|s| s.ticks),
            selected: BTreeSet::new(),
            selected_resource: None,
            drag_start: None,
            last_selection_click: None,
            target_mode: None,
            build_menu: false,
            advanced_build_menu: false,
            minimap_drag: false,
            groups: std::array::from_fn(|_| BTreeSet::new()),
            sequence: 0,
            camera,
            cursor: PhysicalPosition::new(0.0, 0.0),
            keys: BTreeSet::new(),
            paused: false,
            status,
            clock,
            last_frame: Instant::now(),
            next_frame: Instant::now(),
            window: None,
            surface: None,
            smoke: false,
            benchmark_frames: None,
            frames: 0,
            frame_times: Vec::new(),
            frame_intervals: Vec::new(),
            resize_events: 0,
            observed_sizes: BTreeSet::new(),
            screenshot: None,
            failure: None,
        })
    }

    pub(super) fn load_media(&mut self, directory: &Path) -> Result<()> {
        let media = MediaPack::load(directory)?;
        if let Some(media) = &media {
            media.validate_world(&self.world)?;
            log::info!(
                "native media loaded: {} audio mappings, {} music tracks, {} portraits",
                media.audio.len(),
                media.music.len(),
                media.portraits.len()
            );
        }
        self.audio.set_media(media.as_ref());
        self.mission_ui = media
            .as_ref()
            .filter(|media| !media.mission_texts.is_empty() || !media.briefing.is_empty())
            .map(|media| mission::MissionUi::new(media, self.playback_end.is_none()));
        self.media = media;
        Ok(())
    }

    pub(super) fn issue(&mut self, order: Order) -> Result<()> {
        if self
            .world
            .state()
            .entities
            .iter()
            .any(|entity| entity.id == order.entity() && entity.owner != PlayerId(0))
        {
            self.status = "Only your units accept commands.".into();
            return Ok(());
        }
        if self
            .mission_ui
            .as_ref()
            .is_some_and(|mission| mission.briefing)
            || self
                .world
                .state()
                .mission
                .as_ref()
                .is_some_and(|mission| mission.paused)
        {
            self.status = "Mission transmission in progress.".into();
            return Ok(());
        }
        if self.playback_end.is_some() {
            self.status = "Playback is read-only. Camera controls remain available.".into();
            return Ok(());
        }
        if self.world.state().winner.is_some() || self.world.state().defeated.contains(&PlayerId(0))
        {
            self.status = format!(
                "Session finished. Press {} to restart.",
                self.config.bindings.restart
            );
            return Ok(());
        }
        let feedback = match &order {
            Order::Move { target, .. }
            | Order::AttackMove { target, .. }
            | Order::Patrol { target, .. }
            | Order::Rally { target, .. } => Some(crate::visual::CommandTarget::Ground(*target)),
            Order::Attack { target, .. }
            | Order::Load { target, .. }
            | Order::Repair { target, .. } => Some(crate::visual::CommandTarget::Entity(*target)),
            Order::Resume { building, .. } => Some(crate::visual::CommandTarget::Entity(*building)),
            Order::Gather { resource, .. } | Order::RallyResource { resource, .. } => Some(
                crate::visual::CommandTarget::resource(&self.world, *resource),
            ),
            _ => None,
        };
        let order = if self.shifted() {
            queued_order(order)
        } else {
            order
        };
        self.sequence = self
            .sequence
            .checked_add(1)
            .context("command sequence exhausted")?;
        let command = Command {
            tick: self.simulation.as_ref().map_or_else(
                || self.world.tick(),
                simulation::SimulationWorker::command_tick,
            ),
            player: PlayerId(0),
            sequence: self.sequence,
            order,
        };
        self.queue.push(command.clone())?;
        log::debug!("queued {command:?}");
        self.recorded.push(command);
        if let Some(target) = feedback {
            self.visuals.show_command_feedback(target);
        }
        self.status = "Order queued.".into();
        Ok(())
    }

    pub(super) fn advance_campaign(&mut self) -> Result<bool> {
        if self.world.state().winner != Some(PlayerId(0)) {
            return Ok(false);
        }
        let Some(mut campaign) = self.campaign.clone() else {
            return Ok(false);
        };
        if campaign.index + 1 >= campaign.manifest.missions.len() {
            return Ok(false);
        }
        campaign.index += 1;
        let entry = &campaign.manifest.missions[campaign.index];
        let directory = campaign.root.join(&entry.package);
        let package = Package::load(&directory)?;
        let presentation: Presentation = read_ron(&directory.join("presentation.ron"))?;
        presentation.validate()?;
        let assets = AssetPack::load(&directory)?;
        // Keep the current audio device and soundtrack until the new mission is
        // fully loaded. Its media is validated without opening a second device.
        let mut config = self.config.clone();
        config.audio = false;
        let mut next = Self::new(&package, config, presentation, assets, None)?;
        next.load_media(&directory)?;
        // Complete loading before replacing any live session state.
        next.config.audio = self.config.audio;
        next.audio = std::mem::replace(&mut self.audio, Audio::new(false));
        next.audio.set_media(next.media.as_ref());
        next.audio.reset(&next.world);
        next.status = format!("Mission {}: {}", campaign.index + 1, entry.title);
        if let Some(window) = &self.window {
            window.set_title(&format!("StrateRust - {}", entry.title));
        }
        next.campaign = Some(campaign);
        if let Some(renderer) = &mut self.surface {
            renderer.reset_assets();
        }
        next.window = self.window.take();
        next.surface = self.surface.take();
        *self = next;
        Ok(true)
    }

    pub(super) fn ending_hint(&self) -> String {
        let restart = format!("{} RESTART", self.config.bindings.restart);
        if self.world.state().winner == Some(PlayerId(0))
            && let Some(campaign) = &self.campaign
        {
            if campaign.index + 1 < campaign.manifest.missions.len() {
                return format!("ENTER NEXT MISSION | {restart}");
            }
            return format!("CAMPAIGN COMPLETE | {restart}");
        }
        restart
    }

    pub(super) fn restart(&mut self) -> Result<()> {
        self.simulation = None;
        self.world = self.initial_world.clone();
        self.visuals = Visuals::new(&self.world);
        self.visuals.movement_heading_debounce_ms = self.config.movement_heading_debounce_ms;
        self.audio.reset(&self.world);
        self.mission_ui = self
            .media
            .as_ref()
            .filter(|media| !media.mission_texts.is_empty() || !media.briefing.is_empty())
            .map(|media| mission::MissionUi::new(media, self.initial_scenario.is_none()));
        self.queue = self
            .initial_scenario
            .as_ref()
            .map(CommandQueue::from_scenario)
            .transpose()?
            .unwrap_or_default();
        self.playback_end = self
            .initial_scenario
            .as_ref()
            .map(|scenario| scenario.ticks);
        self.recorded.clear();
        self.selected.clear();
        self.selected_resource = None;
        self.groups.iter_mut().for_each(BTreeSet::clear);
        self.target_mode = None;
        self.build_menu = false;
        self.minimap_drag = false;
        self.drag_start = None;
        self.last_selection_click = None;
        self.sequence = 0;
        self.paused = false;
        self.clock = TickClock::new(self.world.rules().tick_ms);
        self.last_frame = Instant::now();
        self.animation_elapsed = Duration::ZERO;
        self.portrait_elapsed = Duration::ZERO;
        let anchor = home_position(&self.world);
        self.camera.x = f64::from(anchor.x);
        self.camera.y = f64::from(anchor.y);
        self.status = self.presentation.objective.clone().unwrap_or_else(|| {
            "Session restarted. Select a worker and right-click minerals.".into()
        });
        Ok(())
    }

    pub(super) fn issue_selected(&mut self, order: impl Fn(EntityId) -> Order) -> Result<()> {
        for entity in self.selected.clone() {
            self.issue(order(entity))?;
        }
        if self.playback_end.is_none() && !self.selected.is_empty() {
            self.status = format!("Orders queued for {} selected units.", self.selected.len());
        }
        Ok(())
    }
}
