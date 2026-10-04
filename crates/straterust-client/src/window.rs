use super::*;

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let result = (|| -> Result<()> {
            let mut attributes = Window::default_attributes()
                .with_title(if self.world.map().mission.is_some() {
                    "StrateRust - Campaign"
                } else if self.world.rules().victory {
                    "StrateRust - Playable RTS fixture (fully revealed)"
                } else if self
                    .assets
                    .as_ref()
                    .is_some_and(|assets| assets.manifest.terrain_grid.is_some())
                {
                    "StrateRust - Imported map inspection preview"
                } else if self.assets.is_some() {
                    "StrateRust - Imported art / fixture simulation"
                } else {
                    "StrateRust - Original fixture"
                })
                .with_inner_size(LogicalSize::new(self.config.width, self.config.height))
                .with_min_inner_size(LogicalSize::new(640, 480))
                .with_fullscreen(
                    self.config
                        .fullscreen
                        .then_some(Fullscreen::Borderless(None)),
                );
            if self.smoke || self.benchmark_frames.is_some() {
                let size = LogicalSize::new(self.config.width, self.config.height);
                attributes = attributes
                    .with_min_inner_size(size)
                    .with_max_inner_size(size);
            }
            let window = Arc::new(event_loop.create_window(attributes)?);
            let renderer = gpu::Renderer::new(window.clone())?;
            log::info!("GPU renderer: {}", renderer.adapter_name());
            self.surface = Some(renderer);
            self.last_frame = Instant::now();
            self.next_frame = self.last_frame;
            log::info!(
                "window created: {:?}, scale={}",
                window.inner_size(),
                window.scale_factor()
            );
            window.request_redraw();
            self.window = Some(window);
            Ok(())
        })();
        if let Err(error) = result {
            self.fail(event_loop, error);
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        if self
            .window
            .as_ref()
            .is_none_or(|window| window.id() != window_id)
        {
            return;
        }
        let result = (|| -> Result<()> {
            match event {
                WindowEvent::CloseRequested => event_loop.exit(),
                WindowEvent::RedrawRequested => self.redraw(event_loop, None)?,
                WindowEvent::CursorMoved { position, .. } => {
                    self.cursor = position;
                    if self.minimap_drag {
                        self.pan_minimap(self.logical_cursor());
                    }
                }
                WindowEvent::CursorLeft { .. } => {
                    self.cursor = PhysicalPosition::new(-1000.0, -1000.0);
                }
                WindowEvent::MouseInput { state, button, .. } => {
                    let cursor = self.logical_cursor();
                    let size = self.logical_size();
                    if self
                        .mission_ui
                        .as_ref()
                        .is_some_and(|mission| mission.briefing)
                    {
                        if button == MouseButton::Left
                            && state == ElementState::Pressed
                            && controls::contains(mission::start_button(size), cursor)
                        {
                            self.mission_ui.as_mut().unwrap().start(&mut self.audio);
                            self.last_frame = Instant::now();
                        }
                        return Ok(());
                    }
                    match (button, state) {
                        (MouseButton::Left, ElementState::Pressed) => {
                            if self.world.state().winner == Some(self.world.view_player())
                                && controls::contains(view::ending_rect(size), cursor)
                                && self.advance_campaign()?
                            {
                                return Ok(());
                            }
                            if self.pan_minimap(cursor) {
                                self.last_selection_click = None;
                                self.minimap_drag = true;
                            } else if self.select_panel(cursor, size) {
                                // Selection-only interaction; no world order is issued.
                            } else if let Some(action) = button_at(
                                &self.buttons(),
                                cursor,
                                size,
                                controls::native_ui(self.assets.as_ref()),
                            ) {
                                self.last_selection_click = None;
                                self.activate(action)?;
                            } else if let Some(position) = self.camera.screen_to_world(cursor, size)
                            {
                                if self.target_mode.is_some() {
                                    self.last_selection_click = None;
                                    self.targeting_click(position)?;
                                } else {
                                    self.drag_start = Some(cursor);
                                }
                            } else {
                                self.last_selection_click = None;
                            }
                        }
                        (MouseButton::Left, ElementState::Released) => {
                            self.minimap_drag = false;
                            if let Some(start) = self.drag_start.take() {
                                self.select_screen(start, cursor, size, Instant::now());
                            }
                        }
                        (MouseButton::Right, ElementState::Pressed) => {
                            self.last_selection_click = None;
                            if let Some(position) = self.camera.screen_to_world(cursor, size) {
                                self.click_at(button, position)?;
                            }
                        }
                        _ => {}
                    }
                }
                WindowEvent::MouseWheel { delta, .. } => {
                    self.last_selection_click = None;
                    let amount = match delta {
                        MouseScrollDelta::LineDelta(_, y) => f64::from(y),
                        MouseScrollDelta::PixelDelta(p) => p.y / 80.0,
                    };
                    self.camera.zoom = (self.camera.zoom * 1.15_f64.powf(amount)).clamp(0.25, 4.0);
                    self.camera.clamp_to_map(
                        [self.world.map().width, self.world.map().height],
                        self.logical_size(),
                    );
                }
                WindowEvent::KeyboardInput { event, .. } => {
                    self.last_selection_click = None;
                    if let PhysicalKey::Code(code) = event.physical_key {
                        if self
                            .mission_ui
                            .as_ref()
                            .is_some_and(|mission| mission.briefing)
                        {
                            if event.state == ElementState::Pressed
                                && !event.repeat
                                && matches!(code, KeyCode::Enter | KeyCode::Space | KeyCode::Escape)
                            {
                                self.mission_ui.as_mut().unwrap().start(&mut self.audio);
                                self.last_frame = Instant::now();
                            }
                            return Ok(());
                        }
                        if event.state == ElementState::Released {
                            self.keys.remove(&code);
                        } else {
                            self.keys.insert(code);
                            if !event.repeat && !self.bound_key(code)? {
                                match code {
                                    KeyCode::Escape => {
                                        if self.target_mode.take().is_some() {
                                            self.build_menu = false;
                                            self.status = "Targeting cancelled.".into();
                                        } else if self.build_menu {
                                            self.build_menu = false;
                                        } else {
                                            // The outer client handles Escape menus.
                                            self.menu_open = true;
                                        }
                                    }
                                    KeyCode::KeyR if !self.is_rts() => {
                                        self.issue_selected(|entity| Order::Wander { entity })?;
                                    }
                                    KeyCode::F11 => {
                                        if let Some(window) = &self.window {
                                            window.set_fullscreen(
                                                if window.fullscreen().is_some() {
                                                    None
                                                } else {
                                                    Some(Fullscreen::Borderless(None))
                                                },
                                            );
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                }
                WindowEvent::Focused(false) => {
                    self.keys.clear();
                    self.drag_start = None;
                    self.last_selection_click = None;
                }
                WindowEvent::Resized(size) => {
                    self.resize_events += 1;
                    if let Some(window) = &self.window {
                        let logical = size.to_logical::<u32>(window.scale_factor());
                        self.observed_sizes.insert((logical.width, logical.height));
                    }
                    log::debug!("resized to {size:?}");
                }
                WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                    log::info!("scale factor changed to {scale_factor}")
                }
                _ => {}
            }
            Ok(())
        })();
        if let Err(error) = result {
            self.fail(event_loop, error);
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        if now >= self.next_frame
            && let Some(window) = &self.window
        {
            // A minimized window may not receive redraws. Keep its wake-up deadline future-facing.
            self.next_frame =
                now + Duration::from_secs_f64(1.0 / f64::from(self.config.frames_per_second));
            window.request_redraw();
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(self.next_frame));
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        log::info!(
            "exit at view_tick={}, view_hash={}, recorded_commands={}",
            self.world.tick().0,
            self.world.state_hash(),
            self.recorded.len()
        );
        // Vulkan surfaces must be destroyed while Winit's native display is alive.
        self.surface = None;
        self.window = None;
        self.audio.shutdown();
    }
}
