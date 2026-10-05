use super::*;

impl App {
    pub(super) fn redraw(
        &mut self,
        event_loop: &ActiveEventLoop,
        menu: Option<&menus::MenuUi>,
    ) -> Result<()> {
        let now = Instant::now();
        let elapsed = now.saturating_duration_since(self.last_frame);
        self.last_frame = now;
        // Selection voices keep playing while simulation/world animation is paused.
        self.portrait_elapsed = self.portrait_elapsed.saturating_add(elapsed);
        if !self.menu_open {
            self.visuals.advance_effects(elapsed, self.assets.as_ref());
            if let (Some(mission), Some(media)) = (&mut self.mission_ui, &self.media) {
                mission.advance(elapsed, media, &mut self.audio);
            }
        }
        let briefing = self
            .mission_ui
            .as_ref()
            .is_some_and(|mission| mission.briefing);
        if !self.paused && !briefing && !self.menu_open {
            self.animation_elapsed = self.animation_elapsed.saturating_add(elapsed);
        }
        self.next_frame =
            now + Duration::from_secs_f64(1.0 / f64::from(self.config.frames_per_second));
        if self.network.is_some() {
            self.advance_network()?;
        } else if !self.paused && !briefing && !self.menu_open {
            let elapsed = if self.smoke {
                Duration::from_millis(200)
            } else {
                elapsed
            };
            self.advance_simulation(elapsed.mul_f64(self.config.game_speed))?;
        }
        self.audio.update();
        let pan =
            f64::from(self.config.scroll_speed) * elapsed.as_secs_f64().min(0.1) / self.camera.zoom;
        if self.keys.contains(&KeyCode::ArrowLeft) {
            self.camera.x -= pan;
        }
        if self.keys.contains(&KeyCode::ArrowRight) {
            self.camera.x += pan;
        }
        if self.keys.contains(&KeyCode::ArrowUp) {
            self.camera.y -= pan;
        }
        if self.keys.contains(&KeyCode::ArrowDown) {
            self.camera.y += pan;
        }
        let logical_size = self.logical_size();
        self.camera.clamp_to_map(
            [self.world.map().width, self.world.map().height],
            logical_size,
        );
        let drag_box = self
            .drag_start
            .filter(|start| crate::selection::is_selection_drag(*start, self.logical_cursor()))
            .map(|start| [start, self.logical_cursor()]);
        self.selected.retain(|id| {
            self.world.state().entities.iter().any(|entity| {
                entity.id == *id
                    && self
                        .world
                        .entity_visible(self.world.view_player(), entity.id)
            })
        });
        let buttons = self.buttons();
        if self.selected_resource.is_some_and(|id| {
            !self
                .world
                .state()
                .resources
                .iter()
                .any(|node| node.id == id && (node.amount > 0 || node.requires_extractor))
        }) {
            self.selected_resource = None;
        }
        let placement = self.placement();
        let cursor = self.logical_cursor();
        let restart = if self.network.is_some() {
            String::new()
        } else {
            format!("{} RESTART  ", self.config.bindings.restart)
        };
        let help = format!(
            "SHIFT QUEUE  CTRL+0-9 GROUP  {restart}{} PAUSE  {} HOME",
            self.config.bindings.pause, self.config.bindings.home
        );
        let window = self.window.as_ref().context("window not ready")?;
        let size = window.inner_size();
        window.set_cursor_visible(if let Some(menu) = menu {
            menu.pack.manifest.cursor.is_none()
        } else {
            self.assets
                .as_ref()
                .and_then(|assets| assets.indicators.as_ref())
                .is_none_or(|pack| {
                    !pack
                        .manifest
                        .cursors
                        .iter()
                        .any(|cursor| cursor.key == "arrow")
                })
        });
        if size.width == 0 || size.height == 0 {
            return Ok(());
        }
        ensure!(
            u64::from(size.width) * u64::from(size.height) <= 64 * 1024 * 1024,
            "window exceeds framebuffer size limit"
        );
        let speaking = self
            .mission_ui
            .as_ref()
            .and_then(|mission| mission.active_slot.and_then(|slot| mission.portraits[slot]))
            .filter(|unit_type| self.audio.is_speaking(*unit_type))
            .or_else(|| {
                self.selected_unit_type()
                    .filter(|unit_type| self.audio.is_speaking(*unit_type))
            });
        let ending_hint = self.ending_hint();
        let surface = self.surface.as_mut().context("GPU renderer not ready")?;
        surface.resize(size.width, size.height);
        let view = View {
            world: &self.world,
            visuals: &self.visuals,
            cursor: if menu.is_some() { [-1000.0; 2] } else { cursor },
            targeting: self.target_mode.is_some(),
            presentation: &self.presentation,
            assets: self.assets.as_ref(),
            map_art: self.map_art.as_ref(),
            media: self.media.as_ref(),
            mission: self.mission_ui.as_ref(),
            speaking,
            animation_ms: self.animation_elapsed.as_millis(),
            portrait_ms: self.portrait_elapsed.as_millis(),
            camera: self.camera,
            selected: &self.selected,
            selected_resource: self.selected_resource,
            drag_box,
            paused: self.paused
                || self
                    .world
                    .state()
                    .mission
                    .as_ref()
                    .is_some_and(|mission| mission.paused),
            playback: self.playback_end.is_some(),
            status: &self.status,
            buttons: &buttons,
            help: &help,
            placement,
            placement_type: match self.target_mode {
                Some(TargetMode::Build(kind)) => Some(kind),
                _ => None,
            },
            ending_hint: &ending_hint,
        };
        let mut scene = view.scene(size.width, size.height, window.scale_factor());
        if let Some(menu) = menu {
            view::draw_menu(
                &mut scene,
                menu,
                &self.config,
                cursor,
                self.portrait_elapsed.as_millis(),
                window.scale_factor(),
                menu.page != menus::Page::Results,
            );
        }
        view::draw_frame_stats(&mut scene, self.frame_stats, window.scale_factor(), true);
        surface.render(&scene)?;
        self.visuals.mark_rendered();
        if self.frames >= 5 {
            // Keep a bounded rolling window, excluding startup and screenshot readback.
            if self.frame_times.len() == 300 {
                self.frame_times.remove(0);
                self.frame_intervals.remove(0);
            }
            self.frame_times.push(now.elapsed().as_secs_f64() * 1000.0);
            self.frame_intervals.push(elapsed.as_secs_f64() * 1000.0);
        }
        let finished = (self.smoke && self.playback_end == Some(self.world.tick().0))
            || self
                .benchmark_frames
                .is_some_and(|limit| self.frames + 1 >= limit);
        if finished && let Some(path) = &self.screenshot {
            let mut output = BufWriter::new(File::create(path)?);
            writeln!(output, "P6\n{} {}\n255", size.width, size.height)?;
            let pixels = surface.readback()?;
            for pixel in &pixels {
                output.write_all(&[(pixel >> 16) as u8, (pixel >> 8) as u8, *pixel as u8])?;
            }
            output.flush()?;
        }
        self.frames += 1;
        if self.smoke && [10, 30].contains(&self.frames) {
            let size = if self.frames == 10 {
                LogicalSize::new(800, 600)
            } else {
                LogicalSize::new(1280, 800)
            };
            // Fixed bounds make the smoke window float under tiling compositors too.
            window.set_max_inner_size(Some(size));
            window.set_min_inner_size(Some(size));
            let _ = window.request_inner_size(size);
        }
        if (finished || (self.benchmark_frames.is_some() && self.frames.is_multiple_of(300)))
            && !self.frame_times.is_empty()
        {
            // Report active gameplay as well as the final scene. Keep the
            // rolling samples in chronological order for the next window.
            let mut times = self.frame_times.clone();
            let mut intervals = self.frame_intervals.clone();
            times.sort_by(f64::total_cmp);
            intervals.sort_by(f64::total_cmp);
            let n = times.len();
            log::info!(
                "native frame timing: tick={} frames={} n={n} CPU/submit median={:.2}ms p95={:.2}ms; redraw interval median={:.2}ms p95={:.2}ms",
                self.world.tick().0,
                self.frames,
                times[n / 2],
                times[n * 95 / 100],
                intervals[n / 2],
                intervals[n * 95 / 100]
            );
        }
        if finished {
            ensure!(
                !self.smoke
                    || (self.observed_sizes.contains(&(800, 600))
                        && self.observed_sizes.contains(&(1280, 800))),
                "smoke test did not observe both requested sizes: {:?}",
                self.observed_sizes
            );
            if !self.smoke {
                println!(
                    "view_tick={} view_hash={}",
                    self.world.tick().0,
                    self.world.state_hash()
                );
            }
            log::info!(
                "client run passed: {} frames, {} resize events, scale={}, sizes={:?}",
                self.frames,
                self.resize_events,
                window.scale_factor(),
                self.observed_sizes
            );
            event_loop.exit();
        }
        Ok(())
    }

    pub(super) fn fail(&mut self, event_loop: &ActiveEventLoop, error: anyhow::Error) {
        log::error!("{error:#}");
        self.failure = Some(error);
        event_loop.exit();
    }
}
