//! Client menu state and actions; authored layouts live in native package data.
use std::path::PathBuf;

use anyhow::{Context, Result};
use straterust_engine::{
    content::Campaign,
    menus::{MenuAction, MenuButton, MenuPack},
};

use crate::Config;
pub mod catalog;
pub mod settings;
use catalog::{GameEntry, campaign_directory};
use settings::Setting;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Page {
    Closed,
    Packages,
    Authored(String),
    Campaigns,
    Missions,
    Settings,
    Multiplayer,
    LanMaps,
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
    Host,
    Join,
    DiscoverLan,
    ChooseLanMap,
    LanMap(usize),
    JoinLan(usize),
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
    pub address_selected: bool,
    pub network_map: Option<PathBuf>,
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
            address_selected: true,
            network_map: None,
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
        if in_game && self.history.is_empty() {
            self.page = Page::Closed;
        } else if let Some(page) = self.history.pop() {
            self.page = page;
        } else if self.page != Page::Packages {
            self.page = Page::Packages;
            self.game = None;
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
            Page::LanMaps => "Choose Multiplayer Map".into(),
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
        let mut choices = Vec::new();
        fn add(choices: &mut Vec<Choice>, label: String, y: u16, pick: Pick) {
            choices.push(Choice {
                button: straterust_engine::menus::button(&label, y, MenuAction::Resume),
                pick,
            });
        }
        match &self.page {
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
            Page::Packages => {
                for (i, game) in self.games.iter().enumerate().skip(self.offset).take(7) {
                    let mut label = game.title.clone();
                    label.truncate(40);
                    add(
                        &mut choices,
                        label,
                        112 + (i - self.offset) as u16 * 36,
                        Pick::Game(i),
                    );
                    choices.last_mut().unwrap().button.rect[0] = 96;
                    choices.last_mut().unwrap().button.rect[2] = 448;
                }
                add(&mut choices, "Rescan packages".into(), 404, Pick::Refresh);
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
            Page::Multiplayer => {
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
                add(&mut choices, "Join address above".into(), 220, Pick::Join);
                add(
                    &mut choices,
                    "Find LAN matches".into(),
                    260,
                    Pick::DiscoverLan,
                );
                for (i, game) in self.lan_games.iter().enumerate().take(3) {
                    add(
                        &mut choices,
                        format!(
                            "{} {}{}",
                            game.name,
                            game.address,
                            if game.compatible {
                                ""
                            } else {
                                " (rules unavailable)"
                            }
                        ),
                        302 + i as u16 * 34,
                        Pick::JoinLan(i),
                    );
                    choices.last_mut().unwrap().button.rect = [48, 302 + i as u16 * 34, 544, 28];
                }
            }
            Page::LanMaps => {
                for (i, game) in self
                    .games
                    .iter()
                    .enumerate()
                    .filter(|(_, g)| !g.campaign)
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
            Page::Packages | Page::Campaigns | Page::Missions | Page::LanMaps
        ) {
            if self.offset >= 7 {
                add(&mut choices, "Previous".into(), 374, Pick::Previous);
                choices.last_mut().unwrap().button.rect = [30, 374, 180, 26];
            }
            let len = match self.page {
                Page::Packages => self.games.len(),
                Page::LanMaps => self.games.iter().filter(|g| !g.campaign).count(),
                Page::Campaigns => self.pack.manifest.campaigns.len(),
                _ => self.campaign.as_ref().map_or(0, |(_, c)| c.missions.len()),
            };
            if self.offset + 7 < len {
                add(&mut choices, "Next".into(), 374, Pick::Next);
                choices.last_mut().unwrap().button.rect = [430, 374, 180, 26];
            }
        }
        if !matches!(self.page, Page::Packages | Page::Closed | Page::Results) {
            add(
                &mut choices,
                if matches!(self.page, Page::Confirm(_)) {
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
