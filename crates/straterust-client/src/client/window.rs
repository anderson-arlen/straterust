use super::*;

impl ApplicationHandler for Client {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        if let Some(app) = &mut self.session {
            app.resumed(event_loop);
            self.window = app.window.clone();
            return;
        }
        let result = (|| -> Result<()> {
            let window = Arc::new(
                event_loop.create_window(
                    Window::default_attributes()
                        .with_title("StrateRust - Choose a game")
                        .with_inner_size(LogicalSize::new(self.config.width, self.config.height))
                        .with_min_inner_size(LogicalSize::new(640, 480))
                        .with_fullscreen(
                            self.config
                                .fullscreen
                                .then_some(Fullscreen::Borderless(None)),
                        ),
                )?,
            );
            self.surface = Some(gpu::Renderer::new(window.clone())?);
            window.request_redraw();
            self.window = Some(window);
            Ok(())
        })();
        if let Err(error) = result {
            self.failure = Some(error);
            event_loop.exit();
        }
    }
    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        if self.window.as_ref().is_none_or(|w| w.id() != id) {
            return;
        }
        if matches!(event, WindowEvent::RedrawRequested) {
            self.frame_stats = self.frame_rate.frame(Instant::now());
            if let Some(app) = &mut self.session {
                app.frame_stats = self.frame_stats;
            }
        }
        if let WindowEvent::CursorMoved { position, .. } = &event {
            self.cursor = *position;
        }
        if matches!(event, WindowEvent::CursorLeft { .. }) {
            self.cursor = PhysicalPosition::new(-1000.0, -1000.0);
        }
        if let WindowEvent::KeyboardInput { event: key, .. } = &event
            && key.state == ElementState::Pressed
            && !key.repeat
            && key.physical_key == PhysicalKey::Code(KeyCode::F11)
        {
            self.config.fullscreen = !self.config.fullscreen;
            if let Err(error) = self.apply_settings() {
                log::error!("fullscreen settings: {error:#}");
                self.menus.message = format!("{error:#}");
            }
            return;
        }
        if let WindowEvent::KeyboardInput { event: key, .. } = &event
            && key.state == ElementState::Pressed
            && !key.repeat
            && matches!(
                key.physical_key,
                PhysicalKey::Code(KeyCode::Escape | KeyCode::F10)
            )
            && self.menus.page == Page::Closed
        {
            self.open_pause();
            return;
        }
        if self.menus.page == Page::Closed {
            if let Some(app) = &mut self.session {
                app.window_event(event_loop, id, event);
            }
            return;
        }
        let result = (|| -> Result<bool> {
            match event {
                WindowEvent::CloseRequested => return Ok(true),
                WindowEvent::RedrawRequested => self.draw(event_loop)?,
                WindowEvent::KeyboardInput { event: key, .. }
                    if key.state == ElementState::Pressed && !key.repeat =>
                {
                    if let PhysicalKey::Code(code) = key.physical_key {
                        return self.key(code);
                    }
                }
                WindowEvent::MouseInput {
                    state: ElementState::Pressed,
                    button: MouseButton::Left,
                    ..
                } => {
                    if let Some(choice) = self.menus.choices(&self.config).iter().find(|c| {
                        controls::contains(
                            view::menu_rect(c.button.rect, self.size()),
                            self.cursor(),
                        )
                    }) {
                        return self.pick(choice.pick.clone());
                    }
                }
                WindowEvent::MouseInput {
                    state: ElementState::Pressed,
                    button: MouseButton::Right,
                    ..
                } => {
                    self.menus.escape(self.session.is_some());
                    self.sync_menu();
                }
                WindowEvent::MouseWheel { delta, .. }
                    if matches!(
                        self.menus.page,
                        Page::Packages | Page::Missions | Page::Campaigns
                    ) =>
                {
                    let amount = match delta {
                        MouseScrollDelta::LineDelta(_, y) => f64::from(y),
                        MouseScrollDelta::PixelDelta(p) => p.y,
                    };
                    let choices = self.menus.choices(&self.config);
                    let next = if amount < 0.0 {
                        Pick::Next
                    } else {
                        Pick::Previous
                    };
                    if choices.iter().any(|c| c.pick == next) {
                        return self.pick(next);
                    }
                }
                _ => {}
            }
            Ok(false)
        })();
        match result {
            Ok(true) => event_loop.exit(),
            Ok(false) => {}
            Err(error) => {
                log::error!("menu action: {error:#}");
                self.menus.message = format!("{error:#}");
            }
        }
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.menus.page == Page::Closed {
            if let Some(app) = &mut self.session {
                app.about_to_wait(event_loop);
            }
            return;
        }
        if Instant::now() >= self.next_frame
            && let Some(window) = &self.window
        {
            self.next_frame = Instant::now()
                + Duration::from_secs_f64(1.0 / f64::from(self.config.frames_per_second));
            window.request_redraw();
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(self.next_frame));
    }
    fn exiting(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(app) = &mut self.session {
            app.exiting(event_loop);
        }
        self.surface = None;
        self.window = None;
    }
}
