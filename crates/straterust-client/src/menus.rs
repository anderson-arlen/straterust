//! Client menu state and actions; authored layouts live in native package data.
use std::path::PathBuf;

use anyhow::{Context, Result};
use straterust_engine::{
    content::Campaign,
    menus::{MenuAction, MenuButton, MenuPack},
};

use crate::Config;
pub mod catalog;
pub mod lobby_map;
pub mod map_browser;
pub mod multiplayer;
pub mod settings;
use catalog::{GameEntry, campaign_directory};
use settings::Setting;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Page {
    Closed,
    Packages,
    ExportGames,
    Exporting,
    Importers,
    ImportSource(straterust_importers::Importer),
    Importing(straterust_importers::Importer),
    Authored(String),
    Campaigns,
    Missions,
    Settings,
    Multiplayer,
    HostSetup,
    PlayerName,
    LanMaps,
    MapDetails,
    Lobby,
    MatchOptions,
    MatchSlots,
    Results,
    Objectives,
    Help,
    Saves(bool),
    Overwrite(usize),
    Confirm(MenuAction),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pick {
    Action(MenuAction),
    Game(usize),
    Mission(usize),
    Setting(Setting),
    Previous,
    Next,
    Back,
    Confirm,
    Refresh,
    Import,
    Export,
    ExportGame(usize),
    Importer(straterust_importers::Importer),
    ImportFile(bool),
    Host,
    CreateGame,
    Join,
    PlayerName,
    SaveName,
    Address,
    HostPort,
    DiscoverLan,
    ChooseLanMap,
    LanMap(usize),
    JoinLan(usize),
    MatchOption(usize),
    TakeSlot(usize),
    SelectSlot(usize),
    SlotField(u8),
    SetSlot(usize, u8, usize),
    AiDifficulty,
    MatchOptions,
    MatchSlots,
    Ready,
    StartMatch,
    MapFilter(bool),
    MapPlayers(Option<u8>),
    MapSize(Option<[u16; 2]>),
    MapVersions,
    MapVersion(usize),
    UseLanMap,
    MapDetails,
    DropdownPage(bool),
    DismissResults,
    SaveSlot(usize),
    LoadSlot(usize),
}

pub struct Choice {
    pub button: MenuButton,
    pub pick: Pick,
}

pub struct MenuUi {
    pub games: Vec<GameEntry>,
    pub game: Option<GameEntry>,
    pub pack: MenuPack,
    pub page: Page,
    pub history: Vec<Page>,
    pub campaign: Option<(PathBuf, Campaign)>,
    pub offset: usize,
    pub focus: Option<usize>,
    pub message: String,
    pub details: Vec<String>,
    pub address: String,
    pub host_port: String,
    pub address_selected: bool,
    pub name_input: String,
    pub name_selected: bool,
    pub network_map: Option<PathBuf>,
    pub match_catalog: Option<straterust_engine::multiplayer::Catalog>,
    pub match_settings: Option<straterust_engine::multiplayer::Settings>,
    pub match_map: usize,
    pub map_players: Option<u8>,
    pub map_size: Option<[u16; 2]>,
    pub map_browser: map_browser::MapBrowser,
    pub lobby: Option<(usize, straterust_engine::net::lobby::View)>,
    pub selected_slot: Option<usize>,
    pub slot_field: Option<u8>,
    pub lan_games: Vec<straterust_engine::net::lan::LanGame>,
    pub multiplayer: bool,
    pub result: Option<straterust_engine::session::MatchResult>,
    pub result_player: straterust_engine::sim::PlayerId,
    pub continue_campaign: bool,
    pub save_labels: Vec<String>,
}

impl MenuUi {
    pub fn new(games: Vec<GameEntry>) -> Self {
        Self {
            games,
            game: None,
            pack: MenuPack::plain("StrateRust", false),
            page: Page::Packages,
            history: Vec::new(),
            campaign: None,
            offset: 0,
            focus: None,
            message: String::new(),
            details: Vec::new(),
            address: "127.0.0.1:6112".into(),
            host_port: "6112".into(),
            address_selected: true,
            name_input: String::new(),
            name_selected: true,
            network_map: None,
            match_catalog: None,
            match_settings: None,
            match_map: 0,
            map_players: None,
            map_size: None,
            map_browser: Default::default(),
            lobby: None,
            selected_slot: None,
            slot_field: None,
            lan_games: Vec::new(),
            multiplayer: false,
            result: None,
            result_player: straterust_engine::sim::PlayerId(0),
            continue_campaign: false,
            save_labels: Vec::new(),
        }
    }
    pub fn choose(&mut self, game: GameEntry) -> Result<()> {
        let mut pack = game.menus()?;
        // Upgrade disabled save actions in already imported menu documents.
        for screen in &mut pack.manifest.screens {
            for button in &mut screen.buttons {
                if let MenuAction::Unavailable(reason) = &button.action {
                    if reason == "Saving games is not implemented yet." {
                        button.action = MenuAction::SaveGame;
                    } else if reason == "Loading saved games is not implemented yet." {
                        button.action = MenuAction::LoadGame;
                    }
                }
            }
            if screen.id == pack.manifest.home
                && !screen
                    .buttons
                    .iter()
                    .any(|b| b.action == MenuAction::LoadGame)
            {
                let mut button =
                    straterust_engine::menus::button("Load Game", 370, MenuAction::LoadGame);
                button.rect = [20, 370, 184, 28];
                screen.buttons.push(button);
            }
        }
        self.page = Page::Authored(pack.manifest.home.clone());
        self.pack = pack;
        self.network_map = None;
        self.match_catalog = if game.directory.join("multiplayer.ron").is_file() {
            Some(straterust_importers::multiplayer_catalog(&game.directory)?)
        } else {
            None
        };
        self.match_settings = None;
        self.match_map = 0;
        self.selected_slot = None;
        self.slot_field = None;
        self.map_players = None;
        self.map_size = None;
        self.map_browser = Default::default();
        self.lobby = None;
        self.game = Some(game);
        self.history.clear();
        self.campaign = None;
        self.result = None;
        self.multiplayer = false;
        self.reset();
        Ok(())
    }
    pub fn reset(&mut self) {
        self.offset = 0;
        self.focus = None;
        self.message.clear();
        self.map_browser.dropdown = None;
    }
    pub fn navigate(&mut self, page: Page) {
        self.history.push(self.page.clone());
        self.page = page;
        self.reset();
    }
    pub fn open_pause(&mut self) {
        self.page = Page::Authored(self.pack.manifest.pause.clone());
        self.history.clear();
        self.reset();
    }
    pub fn escape(&mut self, in_game: bool) {
        if self.map_browser.dropdown.take().is_some() {
            self.focus = None;
            return;
        }
        if in_game && self.history.is_empty() {
            self.page = Page::Closed;
        } else if let Some(page) = self.history.pop() {
            self.page = page;
        } else if self.page != Page::Packages {
            self.page = Page::Packages;
            self.game = None;
            self.match_catalog = None;
            self.match_settings = None;
            self.network_map = None;
            self.lobby = None;
            self.pack = MenuPack::plain("StrateRust", false);
        }
        self.reset();
    }
    pub fn select_campaign(&mut self, directory: &str) -> Result<()> {
        let game = self.game.as_ref().context("no game selected")?;
        let root = campaign_directory(game, directory)?;
        let manifest = Campaign::load(&root)?;
        self.campaign = Some((root, manifest));
        self.navigate(Page::Missions);
        Ok(())
    }
    pub fn title(&self) -> String {
        match &self.page {
            Page::Packages => "Choose a game".into(),
            Page::ExportGames => "Package a game".into(),
            Page::Exporting => "Creating AppImage".into(),
            Page::Importers => "Import a game".into(),
            Page::ImportSource(importer) => format!("Import {}", importer.name()),
            Page::Importing(importer) => format!("Importing {}", importer.name()),
            Page::Authored(id) => self
                .pack
                .manifest
                .screen(id)
                .map_or("Menu", |s| s.title.as_str())
                .into(),
            Page::Campaigns => "Choose a campaign".into(),
            Page::Missions => "Select Mission".into(),
            Page::Settings => "Options".into(),
            Page::Multiplayer => "LAN Multiplayer".into(),
            Page::HostSetup => "Create Game".into(),
            Page::PlayerName => "Player name".into(),
            Page::LanMaps => "Choose Multiplayer Map".into(),
            Page::MapDetails => "Map version settings".into(),
            Page::Lobby => if self.lobby.as_ref().is_some_and(|(_, view)| view.running) {
                "Join Running Game"
            } else {
                "Multiplayer Lobby"
            }
            .into(),
            Page::MatchOptions => "Match Settings".into(),
            Page::MatchSlots => "Player Slots".into(),
            Page::Results => self
                .result
                .as_ref()
                .and_then(|r| r.players.iter().find(|p| p.player == self.result_player))
                .map_or("Match Results", |p| match p.outcome {
                    straterust_engine::session::MatchOutcome::Victory => "Victory",
                    straterust_engine::session::MatchOutcome::Defeat => "Defeat",
                    straterust_engine::session::MatchOutcome::Draw => "Draw",
                })
                .into(),
            Page::Objectives => "Mission Objectives".into(),
            Page::Help => "Controls".into(),
            Page::Saves(true) => "Save Game".into(),
            Page::Saves(false) => "Load Game".into(),
            Page::Overwrite(_) => "Overwrite saved game?".into(),
            Page::Confirm(action) => match action {
                MenuAction::Restart => "Restart Mission?",
                MenuAction::EndMission => "End Mission?",
                _ => "Quit StrateRust?",
            }
            .into(),
            Page::Closed => String::new(),
        }
    }
    pub fn choices(&self, config: &Config) -> Vec<Choice> {
        if self.page == Page::Multiplayer {
            return self.join_choices(config);
        }
        if self.match_catalog.is_some()
            && matches!(
                self.page,
                Page::HostSetup
                    | Page::LanMaps
                    | Page::Lobby
                    | Page::MatchOptions
                    | Page::MatchSlots
            )
        {
            let mut choices = self.match_choices();
            self.identity_choices(&mut choices, config);
            return choices;
        }
        let mut choices = Vec::new();
        fn add(choices: &mut Vec<Choice>, label: String, y: u16, pick: Pick) {
            choices.push(Choice {
                button: straterust_engine::menus::button(&label, y, MenuAction::Resume),
                pick,
            });
        }
        match &self.page {
            Page::PlayerName => {
                add(
                    &mut choices,
                    "Save name (Enter)".into(),
                    240,
                    Pick::SaveName,
                );
            }
            Page::MapDetails => {
                let lines = self
                    .details
                    .iter()
                    .flat_map(|s| crate::view::wrapped_lines(s, 56))
                    .count();
                if self.offset > 0 {
                    add(&mut choices, "Previous".into(), 396, Pick::Previous);
                    choices.last_mut().unwrap().button.rect = [48, 396, 180, 26];
                }
                if self.offset + 22 < lines {
                    add(&mut choices, "Next".into(), 396, Pick::Next);
                    choices.last_mut().unwrap().button.rect = [412, 396, 180, 26];
                }
            }
            Page::Authored(id) => {
                if let Some(screen) = self.pack.manifest.screen(id) {
                    return screen
                        .buttons
                        .iter()
                        .filter(|b| !self.multiplayer || b.action != MenuAction::Restart)
                        .filter(|b| {
                            !self.multiplayer
                                || !matches!(b.action, MenuAction::SaveGame | MenuAction::LoadGame)
                        })
                        .map(|b| Choice {
                            button: b.clone(),
                            pick: Pick::Action(b.action.clone()),
                        })
                        .collect();
                }
            }
            Page::Packages | Page::ExportGames => {
                for (i, game) in self.games.iter().enumerate().skip(self.offset).take(7) {
                    let mut label = game.title.clone();
                    label.truncate(40);
                    if let Some(info) = &game.info {
                        label.push('\n');
                        label.push_str(&info.package_version());
                    }
                    add(
                        &mut choices,
                        label,
                        112 + (i - self.offset) as u16 * 36,
                        if self.page == Page::ExportGames {
                            Pick::ExportGame(i)
                        } else {
                            Pick::Game(i)
                        },
                    );
                    choices.last_mut().unwrap().button.rect[0] = 96;
                    choices.last_mut().unwrap().button.rect[2] = 448;
                }
                if self.page == Page::Packages {
                    add(&mut choices, "Import a game".into(), 404, Pick::Import);
                    choices.last_mut().unwrap().button.rect = [30, 404, 180, 28];
                    add(&mut choices, "Package a game".into(), 404, Pick::Export);
                    choices.last_mut().unwrap().button.rect = [230, 404, 180, 28];
                    add(&mut choices, "Rescan".into(), 404, Pick::Refresh);
                    choices.last_mut().unwrap().button.rect = [430, 404, 180, 28];
                    add(
                        &mut choices,
                        "Settings".into(),
                        444,
                        Pick::Action(MenuAction::Settings),
                    );
                    choices.last_mut().unwrap().button.rect = [30, 444, 200, 26];
                    add(
                        &mut choices,
                        "Quit".into(),
                        444,
                        Pick::Action(MenuAction::Quit),
                    );
                    choices.last_mut().unwrap().button.rect = [410, 444, 200, 26];
                }
            }
            Page::Importers => {
                for (index, importer) in straterust_importers::IMPORTERS.into_iter().enumerate() {
                    add(
                        &mut choices,
                        importer.name().into(),
                        156 + index as u16 * 44,
                        Pick::Importer(importer),
                    );
                }
            }
            Page::ImportSource(_) => {
                add(
                    &mut choices,
                    "Choose ISO or EXE".into(),
                    180,
                    Pick::ImportFile(false),
                );
                add(
                    &mut choices,
                    "Choose directory".into(),
                    224,
                    Pick::ImportFile(true),
                );
            }
            Page::Campaigns => {
                for (i, campaign) in self
                    .pack
                    .manifest
                    .campaigns
                    .iter()
                    .enumerate()
                    .skip(self.offset)
                    .take(7)
                {
                    add(
                        &mut choices,
                        campaign.title.clone(),
                        112 + (i - self.offset) as u16 * 36,
                        Pick::Action(MenuAction::Campaign(campaign.directory.clone())),
                    );
                }
            }
            Page::Missions => {
                if let Some((_, campaign)) = &self.campaign {
                    for (i, mission) in campaign
                        .missions
                        .iter()
                        .enumerate()
                        .skip(self.offset)
                        .take(7)
                    {
                        add(
                            &mut choices,
                            format!("{}. {}", i + 1, mission.title),
                            112 + (i - self.offset) as u16 * 36,
                            Pick::Mission(i),
                        );
                        choices.last_mut().unwrap().button.rect[0] = 96;
                        choices.last_mut().unwrap().button.rect[2] = 448;
                    }
                }
            }
            Page::HostSetup => {
                self.identity_choices(&mut choices, config);
                let title = self
                    .network_map
                    .as_ref()
                    .and_then(|path| self.games.iter().find(|g| g.directory == *path))
                    .map_or("Choose a map", |g| g.title.as_str());
                add(
                    &mut choices,
                    format!("Map: {title}"),
                    136,
                    Pick::ChooseLanMap,
                );
                add(&mut choices, "Host match".into(), 180, Pick::Host);
            }
            Page::LanMaps => {
                for (i, game) in self
                    .games
                    .iter()
                    .enumerate()
                    .filter(|(_, g)| {
                        !g.campaign
                            && self
                                .game
                                .as_ref()
                                .is_none_or(|selected| g.directory == selected.directory)
                    })
                    .skip(self.offset)
                    .take(7)
                {
                    let y = 112 + choices.len() as u16 * 36;
                    add(&mut choices, game.title.clone(), y, Pick::LanMap(i));
                }
            }
            Page::Settings => {
                for (i, setting) in settings::ALL.iter().enumerate() {
                    add(
                        &mut choices,
                        setting.label(config),
                        98 + i as u16 * 32,
                        Pick::Setting(*setting),
                    );
                    choices.last_mut().unwrap().button.rect = [80, 98 + i as u16 * 32, 480, 28];
                }
            }
            Page::Results => {
                add(
                    &mut choices,
                    if self.multiplayer {
                        "Return to Multiplayer Lobby"
                    } else if self.continue_campaign {
                        "Continue Campaign"
                    } else {
                        "Return to Game Menu"
                    }
                    .into(),
                    430,
                    Pick::DismissResults,
                );
                choices.last_mut().unwrap().button.rect = [128, 430, 384, 28];
            }
            Page::Confirm(_) => {
                add(&mut choices, "Confirm".into(), 242, Pick::Confirm);
            }
            Page::Overwrite(slot) => {
                add(&mut choices, "Overwrite".into(), 242, Pick::SaveSlot(*slot))
            }
            Page::Saves(saving) => {
                for (slot, label) in self.save_labels.iter().enumerate() {
                    add(
                        &mut choices,
                        label.clone(),
                        112 + slot as u16 * 36,
                        if *saving {
                            Pick::SaveSlot(slot)
                        } else {
                            Pick::LoadSlot(slot)
                        },
                    );
                    choices.last_mut().unwrap().button.rect = [80, 112 + slot as u16 * 36, 480, 30];
                }
            }
            _ => {}
        }
        if matches!(
            self.page,
            Page::Packages | Page::ExportGames | Page::Campaigns | Page::Missions | Page::LanMaps
        ) {
            if self.offset >= 7 {
                add(&mut choices, "Previous".into(), 374, Pick::Previous);
                choices.last_mut().unwrap().button.rect = [30, 374, 180, 26];
            }
            let len = match self.page {
                Page::Packages | Page::ExportGames => self.games.len(),
                Page::LanMaps => self
                    .games
                    .iter()
                    .filter(|g| {
                        !g.campaign
                            && self
                                .game
                                .as_ref()
                                .is_none_or(|selected| g.directory == selected.directory)
                    })
                    .count(),
                Page::Campaigns => self.pack.manifest.campaigns.len(),
                _ => self.campaign.as_ref().map_or(0, |(_, c)| c.missions.len()),
            };
            if self.offset + 7 < len {
                add(&mut choices, "Next".into(), 374, Pick::Next);
                choices.last_mut().unwrap().button.rect = [430, 374, 180, 26];
            }
        }
        if !matches!(
            self.page,
            Page::Packages | Page::Closed | Page::Results | Page::Importing(_) | Page::Exporting
        ) {
            add(
                &mut choices,
                if matches!(self.page, Page::Confirm(_) | Page::PlayerName) {
                    "Cancel (Esc)"
                } else {
                    "Back (Esc)"
                }
                .into(),
                438,
                Pick::Back,
            );
        }
        choices
    }
}
