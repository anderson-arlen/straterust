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
    Objectives,
    Help,
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
        }
    }
    pub fn choose(&mut self, game: GameEntry) -> Result<()> {
        let pack = game.menus()?;
        self.page = Page::Authored(pack.manifest.home.clone());
        self.pack = pack;
        self.game = Some(game);
        self.history.clear();
        self.campaign = None;
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
            Page::Objectives => "Mission Objectives".into(),
            Page::Help => "Controls".into(),
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
            Page::Confirm(_) => {
                add(&mut choices, "Confirm".into(), 242, Pick::Confirm);
            }
            _ => {}
        }
        if matches!(self.page, Page::Packages | Page::Campaigns | Page::Missions) {
            if self.offset >= 7 {
                add(&mut choices, "Previous".into(), 374, Pick::Previous);
                choices.last_mut().unwrap().button.rect = [30, 374, 180, 26];
            }
            let len = match self.page {
                Page::Packages => self.games.len(),
                Page::Campaigns => self.pack.manifest.campaigns.len(),
                _ => self.campaign.as_ref().map_or(0, |(_, c)| c.missions.len()),
            };
            if self.offset + 7 < len {
                add(&mut choices, "Next".into(), 374, Pick::Next);
                choices.last_mut().unwrap().button.rect = [430, 374, 180, 26];
            }
        }
        if self.page != Page::Packages && self.page != Page::Closed {
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
