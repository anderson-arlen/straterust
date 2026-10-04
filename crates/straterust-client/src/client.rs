//! The window host owns either frontend menus or a live session. No dummy world
//! or gameplay package is loaded just to display the package chooser.
use super::*;
use menus::{
    MenuUi, Page, Pick,
    catalog::{self, GameEntry},
};
use straterust_engine::menus::MenuAction;

mod multiplayer;
mod results;

pub(super) struct Client {
    session: Option<App>,
    menus: MenuUi,
    config: Config,
    settings_path: PathBuf,
    roots: Vec<PathBuf>,
    window: Option<Arc<Window>>,
    surface: Option<gpu::Renderer>,
    cursor: PhysicalPosition<f64>,
    started: Instant,
    next_frame: Instant,
    failure: Option<anyhow::Error>,
    frame_rate: timing::FrameRate,
    frame_stats: Option<timing::FrameStats>,
    discovery: Option<std::sync::mpsc::Receiver<Result<multiplayer::Discovered>>>,
    lan_rules: Vec<catalog::RulesPackage>,
}

impl Client {
    pub(super) fn new(config: Config, settings_path: PathBuf, roots: Vec<PathBuf>) -> Self {
        let games = catalog::discover(&roots);
        Self {
            session: None,
            menus: MenuUi::new(games),
            config,
            settings_path,
            roots,
            window: None,
            surface: None,
            cursor: PhysicalPosition::new(-1000.0, -1000.0),
            started: Instant::now(),
            next_frame: Instant::now(),
            failure: None,
            frame_rate: timing::FrameRate::default(),
            frame_stats: None,
            discovery: None,
            lan_rules: Vec::new(),
        }
    }
    pub(super) fn finish(&mut self) -> Result<()> {
        if let Some(app) = &mut self.session {
            if let Some(server) = app.simulation.take() {
                server.finish(app.smoke)?;
            }
            if let Some(network) = app.network.take() {
                network.finish()?;
            }
        }
        if let Some(error) = self
            .failure
            .take()
            .or_else(|| self.session.as_mut().and_then(|a| a.failure.take()))
        {
            return Err(error);
        }
        Ok(())
    }
    pub(super) fn direct(&mut self, app: App, game: GameEntry) -> Result<()> {
        self.menus.choose(game)?;
        self.menus.page = Page::Closed;
        self.session = Some(app);
        self.sync_menu();
        Ok(())
    }
    pub(super) fn remember_network(
        &mut self,
        directory: &Path,
        address: std::net::SocketAddr,
    ) -> Result<()> {
        self.menus.address = address.to_string();
        self.menus.network_map = Some(directory.canonicalize()?);
        Ok(())
    }
    fn cursor(&self) -> [f64; 2] {
        let dpi = self.window.as_ref().map_or(1.0, |w| w.scale_factor());
        [self.cursor.x / dpi, self.cursor.y / dpi]
    }
    fn size(&self) -> [f64; 2] {
        self.window.as_ref().map_or(
            [f64::from(self.config.width), f64::from(self.config.height)],
            |w| {
                let size = w.inner_size().to_logical::<f64>(w.scale_factor());
                [size.width, size.height]
            },
        )
    }
    fn sync_menu(&mut self) {
        if let Some(app) = &mut self.session {
            self.menus.multiplayer = app.network.is_some();
            app.menu_open = self.menus.page != Page::Closed;
            app.keys.clear();
            app.drag_start = None;
            app.minimap_drag = false;
            app.last_frame = Instant::now();
        }
    }
    fn open_pause(&mut self) {
        self.menus.open_pause();
        self.sync_menu();
        if let Some(app) = &mut self.session {
            app.target_mode = None;
            app.build_menu = false;
        }
    }
    fn apply_settings(&mut self) -> Result<()> {
        self.config.validate()?;
        if let Some(window) = &self.window {
            window.set_fullscreen(
                self.config
                    .fullscreen
                    .then_some(Fullscreen::Borderless(None)),
            );
            if !self.config.fullscreen {
                let _ = window
                    .request_inner_size(LogicalSize::new(self.config.width, self.config.height));
            }
        }
        if let Some(app) = &mut self.session {
            app.config = self.config.clone();
            app.audio.configure(
                self.config.audio,
                self.config.music_volume,
                self.config.sound_volume,
                self.config.speech_volume,
            );
        }
        self.config.save(&self.settings_path)?;
        Ok(())
    }
    fn play(&mut self, directory: &Path, campaign: Option<CampaignSession>) -> Result<()> {
        self.play_mode(directory, campaign, false)
    }
    fn play_mode(
        &mut self,
        directory: &Path,
        campaign: Option<CampaignSession>,
        network: bool,
    ) -> Result<()> {
        let mut config = self.config.clone();
        config.audio = false;
        let mut next = if network {
            App::load_network(directory, config)?
        } else {
            App::load(directory, config, None)?
        };
        next.config = self.config.clone();
        next.audio = if let Some(app) = &mut self.session {
            std::mem::replace(&mut app.audio, Audio::new(false))
        } else {
            Audio::new(self.config.audio)
        };
        next.audio.set_media(next.media.as_ref());
        next.audio.reset(&next.world);
        next.audio.configure(
            self.config.audio,
            self.config.music_volume,
            self.config.sound_volume,
            self.config.speech_volume,
        );
        next.campaign = campaign;
        if let Some(app) = &mut self.session {
            next.surface = app.surface.take();
        } else {
            next.surface = self.surface.take();
        }
        if let Some(surface) = &mut next.surface {
            surface.reset_assets();
        }
        next.window = self.window.clone();
        self.session = Some(next);
        self.menus.page = Page::Closed;
        self.menus.history.clear();
        self.menus.reset();
        if let Some(window) = &self.window {
            window.set_title(&format!("StrateRust - {}", self.menus.pack.manifest.title));
        }
        Ok(())
    }
    fn end_session(&mut self) {
        if let Some(mut app) = self.session.take() {
            self.surface = app.surface.take();
            app.audio.shutdown();
        }
        if let Some(surface) = &mut self.surface {
            surface.reset_assets();
        }
        self.menus.page = Page::Authored(self.menus.pack.manifest.home.clone());
        self.menus.history.clear();
        self.menus.reset();
    }
    fn pick(&mut self, pick: Pick) -> Result<bool> {
        match pick {
            Pick::DismissResults => self.dismiss_results()?,
            Pick::ChooseLanMap => self.menus.navigate(Page::LanMaps),
            Pick::LanMap(index) => {
                self.menus.network_map = Some(self.menus.games[index].directory.clone());
                self.menus.escape(false);
            }
            Pick::Host | Pick::Join | Pick::JoinLan(_) => {
                let (directory, address) = if let Pick::JoinLan(index) = pick {
                    self.lan_join(index)?
                } else {
                    let directory = self
                        .menus
                        .network_map
                        .clone()
                        .context("choose a multiplayer map first")?;
                    let address: std::net::SocketAddr = self
                        .menus
                        .address
                        .parse()
                        .context("enter an IP address and port, such as 192.168.1.10:6112")?;
                    (directory, address)
                };
                let hosting = pick == Pick::Host;
                self.menus.network_map = Some(directory.clone());
                if !hosting {
                    self.menus.address = address.to_string();
                }
                let address = if hosting {
                    std::net::SocketAddr::from(([0, 0, 0, 0], address.port()))
                } else {
                    address
                };
                self.play_mode(&directory, None, true)?;
                self.session
                    .as_mut()
                    .unwrap()
                    .start_network(&directory, address, hosting, None)?;
            }
            Pick::DiscoverLan => {
                self.discover_lan();
            }
            Pick::Game(index) => self.menus.choose(self.menus.games[index].clone())?,
            Pick::Mission(index) => {
                let (root, manifest) = self
                    .menus
                    .campaign
                    .clone()
                    .context("no campaign selected")?;
                let directory = root.join(&manifest.missions[index].package);
                self.play(
                    &directory,
                    Some(CampaignSession {
                        root,
                        manifest,
                        index,
                    }),
                )?;
            }
            Pick::Previous => {
                self.menus.offset = self.menus.offset.saturating_sub(7);
                self.menus.focus = None;
            }
            Pick::Next => {
                self.menus.offset += 7;
                self.menus.focus = None;
            }
            Pick::Refresh => {
                self.menus.games = catalog::discover(&self.roots);
                self.menus.reset();
            }
            Pick::Back => self.menus.escape(self.session.is_some()),
            Pick::Setting(setting) => {
                setting.change(&mut self.config, 1);
                self.apply_settings()?;
            }
            Pick::Confirm => match self.menus.page.clone() {
                Page::Confirm(MenuAction::Quit) => return Ok(true),
                Page::Confirm(MenuAction::Restart) => {
                    if let Some(app) = &mut self.session {
                        app.restart()?;
                    }
                    self.menus.page = Page::Closed;
                    self.menus.history.clear();
                }
                Page::Confirm(MenuAction::EndMission) => self.end_session(),
                _ => {}
            },
            Pick::Action(action) => match action {
                MenuAction::Screen(id) => {
                    if id == self.menus.pack.manifest.home {
                        self.menus.history.clear();
                        self.menus.page = Page::Authored(id);
                        self.menus.reset();
                    } else {
                        self.menus.navigate(Page::Authored(id));
                    }
                }
                MenuAction::Campaigns => self.menus.navigate(Page::Campaigns),
                MenuAction::Campaign(directory) => self.menus.select_campaign(&directory)?,
                MenuAction::Play => {
                    let directory = self
                        .menus
                        .game
                        .as_ref()
                        .context("no game selected")?
                        .directory
                        .clone();
                    self.play(&directory, None)?;
                }
                MenuAction::Settings => self.menus.navigate(Page::Settings),
                MenuAction::Multiplayer => {
                    self.menus.address_selected = true;
                    self.menus.network_map = self
                        .menus
                        .game
                        .as_ref()
                        .filter(|g| !g.campaign)
                        .map(|g| g.directory.clone())
                        .or_else(|| {
                            self.menus
                                .games
                                .iter()
                                .find(|g| g.directory.ends_with("lan-demo"))
                                .map(|g| g.directory.clone())
                        });
                    self.menus.navigate(Page::Multiplayer);
                }
                MenuAction::Resume => {
                    if self.session.is_some() {
                        self.menus.page = Page::Closed;
                        self.menus.history.clear();
                    }
                }
                MenuAction::Restart | MenuAction::EndMission | MenuAction::Quit => {
                    self.menus.navigate(Page::Confirm(action));
                    self.menus.details = vec![if self.session.is_some() {
                        "Current mission progress will be lost.".into()
                    } else {
                        "Exit StrateRust?".into()
                    }];
                }
                MenuAction::ChooseGame => {
                    self.end_session();
                    self.menus.page = Page::Packages;
                    self.menus.game = None;
                    self.menus.pack =
                        straterust_engine::menus::MenuPack::plain("StrateRust", false);
                }
                MenuAction::Objectives => {
                    self.menus.details = self
                        .session
                        .as_ref()
                        .and_then(|app| app.mission_ui.as_ref().zip(app.media.as_ref()))
                        .and_then(|(m, media)| m.objectives(media))
                        .map(|text| text.lines().map(str::to_owned).collect())
                        .unwrap_or_else(|| {
                            vec![
                                self.session
                                    .as_ref()
                                    .and_then(|a| a.presentation.objective.clone())
                                    .unwrap_or_else(|| "Complete the mission's objectives.".into()),
                            ]
                        });
                    self.menus.navigate(Page::Objectives);
                }
                MenuAction::Help => {
                    let restart = if self.menus.multiplayer {
                        String::new()
                    } else {
                        format!("{}: Restart. ", self.config.bindings.restart)
                    };
                    self.menus.details = vec![
                        "Left click selects. Drag selects a group of units.".into(),
                        "Right click moves, attacks, gathers or loads.".into(),
                        "Shift queues orders. Ctrl+0-9 saves a group; 0-9 recalls it.".into(),
                        "A: Attack-move. B/V: Basic/advanced building menus.".into(),
                        format!(
                            "{}: Pause/resume. Esc/F10: Game menu.",
                            self.config.bindings.pause
                        ),
                        format!(
                            "{restart}{}: Center camera. F11: Fullscreen.",
                            self.config.bindings.home
                        ),
                        "Arrow keys scroll; mouse wheel zooms.".into(),
                    ];
                    self.menus.navigate(Page::Help);
                }
                MenuAction::Unavailable(reason) => self.menus.message = reason,
            },
        }
        self.sync_menu();
        Ok(false)
    }
    fn key(&mut self, code: KeyCode) -> Result<bool> {
        if self.menus.page == Page::Closed && matches!(code, KeyCode::Escape | KeyCode::F10) {
            if code == KeyCode::Escape
                && let Some(app) = &mut self.session
                && app.bound_key(code)?
            {
                return Ok(false);
            }
            self.open_pause();
            return Ok(false);
        }
        if self.menus.page == Page::Results {
            if matches!(
                code,
                KeyCode::Escape | KeyCode::F10 | KeyCode::Enter | KeyCode::Space
            ) {
                return self.pick(Pick::DismissResults);
            }
            if !self.menus.multiplayer
                && controls::parse_key(&self.config.bindings.restart) == Some(code)
            {
                if let Some(app) = &mut self.session {
                    app.restart()?;
                }
                self.menus.result = None;
                self.menus.page = Page::Closed;
                self.sync_menu();
            }
            return Ok(false);
        }
        if matches!(code, KeyCode::Escape | KeyCode::F10) {
            self.menus.escape(self.session.is_some());
            self.sync_menu();
            return Ok(false);
        }
        let choices = self.menus.choices(&self.config);
        if choices.is_empty() {
            return Ok(false);
        }
        match code {
            KeyCode::ArrowUp => {
                self.menus.focus = Some(
                    self.menus
                        .focus
                        .unwrap_or(0)
                        .wrapping_add(choices.len() - 1)
                        % choices.len(),
                );
            }
            KeyCode::ArrowDown | KeyCode::Tab => {
                self.menus.focus = Some(self.menus.focus.map_or(0, |n| (n + 1) % choices.len()));
            }
            KeyCode::Enter | KeyCode::Space => {
                if let Some(index) = self.menus.focus {
                    return self.pick(choices[index.min(choices.len() - 1)].pick.clone());
                }
            }
            KeyCode::ArrowLeft | KeyCode::ArrowRight if self.menus.page == Page::Settings => {
                if let Some(index) = self.menus.focus
                    && let Pick::Setting(setting) = choices[index.min(choices.len() - 1)].pick
                {
                    setting.change(
                        &mut self.config,
                        if code == KeyCode::ArrowLeft { -1 } else { 1 },
                    );
                    self.apply_settings()?;
                }
            }
            _ => {
                if let Some(choice) = choices.iter().find(|c| {
                    c.button.key.as_ref().is_some_and(|key| {
                        controls::parse_key(&key.to_ascii_uppercase()) == Some(code)
                    })
                }) {
                    return self.pick(choice.pick.clone());
                }
            }
        }
        Ok(false)
    }
    fn draw(&mut self, event_loop: &ActiveEventLoop) -> Result<()> {
        self.next_frame = Instant::now()
            + Duration::from_secs_f64(1.0 / f64::from(self.config.frames_per_second));
        self.poll_lan();
        if let Some(app) = &mut self.session {
            app.cursor = self.cursor;
            app.redraw(event_loop, Some(&self.menus))?;
            self.show_match_results();
            return Ok(());
        }
        let window = self.window.as_ref().context("window not ready")?;
        let size = window.inner_size();
        if size.width == 0 || size.height == 0 {
            return Ok(());
        }
        ensure!(
            u64::from(size.width) * u64::from(size.height) <= 64 * 1024 * 1024,
            "window exceeds framebuffer limit"
        );
        window.set_cursor_visible(self.menus.pack.manifest.cursor.is_none());
        let mut scene = gpu::Scene {
            width: size.width,
            height: size.height,
            clear: 0,
            commands: Vec::new(),
        };
        view::draw_menu(
            &mut scene,
            &self.menus,
            &self.config,
            self.cursor(),
            self.started.elapsed().as_millis(),
            window.scale_factor(),
            false,
        );
        view::draw_frame_stats(&mut scene, self.frame_stats, window.scale_factor(), false);
        let surface = self.surface.as_mut().context("renderer not ready")?;
        surface.resize(size.width, size.height);
        surface.render(&scene)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
mod window;
