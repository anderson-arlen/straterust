//! Setup controls read the selected game's native catalog.
use super::*;
use straterust_engine::multiplayer::GameType;

fn add(choices: &mut Vec<Choice>, label: String, rect: [u16; 4], pick: Pick) {
    let mut button = straterust_engine::menus::button(&label, rect[1], MenuAction::Resume);
    button.rect = rect;
    choices.push(Choice { button, pick });
}
fn cycle(value: &mut Option<usize>, length: usize) {
    *value = match *value {
        None if length > 0 => Some(0),
        Some(i) if i + 1 < length => Some(i + 1),
        _ => None,
    };
}
impl MenuUi {
    pub(super) fn join_choices(&self, config: &Config) -> Vec<Choice> {
        let mut choices = Vec::new();
        add(
            &mut choices,
            format!("Name: {}", config.player_name),
            [48, 90, 544, 28],
            Pick::PlayerName,
        );
        add(
            &mut choices,
            format!("Address: {}", self.address),
            [48, 136, 400, 28],
            Pick::Address,
        );
        add(&mut choices, "Join".into(), [464, 136, 128, 28], Pick::Join);
        add(
            &mut choices,
            "Find LAN games".into(),
            [48, 184, 544, 28],
            Pick::DiscoverLan,
        );
        for (i, game) in self.lan_games.iter().take(3).enumerate() {
            add(
                &mut choices,
                format!(
                    "{} — {} ({} open){}",
                    game.name,
                    if game.started { "Running" } else { "Lobby" },
                    game.open_slots,
                    if game.compatible {
                        ""
                    } else {
                        " (rules unavailable)"
                    }
                ),
                [48, 226 + i as u16 * 36, 544, 28],
                Pick::JoinLan(i),
            );
        }
        add(
            &mut choices,
            "Create game".into(),
            [176, 370, 288, 28],
            Pick::CreateGame,
        );
        add(
            &mut choices,
            "Back (Esc)".into(),
            [176, 438, 288, 28],
            Pick::Back,
        );
        choices
    }
    pub(super) fn identity_choices(&self, choices: &mut Vec<Choice>, config: &Config) {
        if self.page == Page::HostSetup {
            add(
                choices,
                format!("Name: {}", config.player_name),
                [48, 90, 240, 28],
                Pick::PlayerName,
            );
            add(
                choices,
                format!("Port: {}", self.host_port),
                [312, 90, 280, 28],
                Pick::HostPort,
            );
        } else if self.page == Page::Lobby {
            add(
                choices,
                "Change name".into(),
                [424, 66, 192, 22],
                Pick::PlayerName,
            );
        }
    }
    pub fn map_indices(&self) -> Vec<usize> {
        self.match_catalog.as_ref().map_or_else(Vec::new, |c| {
            if self.map_browser.indexed {
                self.map_browser
                    .groups
                    .iter()
                    .filter_map(|group| {
                        self.map_browser
                            .selected
                            .filter(|i| group.contains(i) && self.map_matches(*i))
                            .or_else(|| group.iter().copied().find(|i| self.map_matches(*i)))
                    })
                    .collect()
            } else {
                (0..c.maps.len()).filter(|i| self.map_matches(*i)).collect()
            }
        })
    }
    pub fn select_map(&mut self, index: usize) -> Result<()> {
        let catalog = self
            .match_catalog
            .as_ref()
            .context("no multiplayer catalog")?;
        let entry = catalog.maps.get(index).context("unknown multiplayer map")?;
        self.network_map = Some(straterust_engine::multiplayer::package_path(
            &self.game.as_ref().context("no game selected")?.directory,
            &entry.package,
        )?);
        self.match_map = index;
        let mut settings = catalog.defaults(index);
        settings.mode = catalog
            .modes
            .iter()
            .position(|m| *m == GameType::FreeForAll)
            .unwrap_or(0);
        if let Some((_, view)) = &self.lobby {
            for (i, slot) in settings.slots.iter_mut().enumerate() {
                slot.color = view
                    .players
                    .get(i)
                    .and_then(Option::as_ref)
                    .and_then(|_| view.settings.slots[i].color);
            }
            catalog.assign_colors(&mut settings)?;
        }
        self.match_settings = Some(settings);
        self.selected_slot = None;
        Ok(())
    }
    pub fn match_choices(&self) -> Vec<Choice> {
        let catalog = self.match_catalog.as_ref().unwrap();
        let mut choices = Vec::new();
        let settings = self.match_settings.as_ref();
        match self.page {
            Page::HostSetup => {
                add(
                    &mut choices,
                    format!("Map: {}", catalog.maps[self.match_map].title),
                    [48, 136, 544, 28],
                    Pick::ChooseLanMap,
                );
                add(
                    &mut choices,
                    "Match settings".into(),
                    [80, 176, 224, 28],
                    Pick::MatchOptions,
                );
                add(
                    &mut choices,
                    "Player slots".into(),
                    [336, 176, 224, 28],
                    Pick::MatchSlots,
                );
                add(
                    &mut choices,
                    "Host match".into(),
                    [176, 240, 288, 28],
                    Pick::Host,
                );
            }
            Page::LanMaps => choices.extend(self.map_browser_choices()),
            Page::Lobby => choices.extend(self.slot_map_choices()),
            Page::MatchOptions => {
                if let Some(settings) = settings {
                    let mut labels = vec![
                        format!("Game type: {}", catalog.modes[settings.mode].name()),
                        format!(
                            "Resources: {}",
                            settings
                                .resources
                                .map_or("Map default", |r| catalog.resources[r].name.as_str())
                        ),
                        format!(
                            "Starting units: {}",
                            if catalog.standard_start
                                && catalog.modes[settings.mode] != GameType::UseMapSettings
                            {
                                "Standard base and workers"
                            } else if settings.map_units {
                                "Map default"
                            } else {
                                "One worker"
                            }
                        ),
                        format!(
                            "Starting locations: {}",
                            if settings.random_locations {
                                "Random"
                            } else {
                                "Fixed"
                            }
                        ),
                        format!("Fog of war: {}", if settings.fog { "On" } else { "Off" }),
                        format!("Game speed: {}", catalog.speeds[settings.speed].name),
                    ];
                    if !catalog.tilesets.is_empty() {
                        labels.push(format!(
                            "Terrain: {}",
                            settings
                                .tileset
                                .map_or("Map default", |t| catalog.tilesets[t].as_str())
                        ));
                    }
                    if catalog.allow_cheats {
                        labels.push(format!(
                            "Cheat codes: {}",
                            if settings.cheats {
                                "Allowed"
                            } else {
                                "Unavailable"
                            }
                        ));
                    }
                    if matches!(
                        catalog.modes[settings.mode],
                        GameType::Greed | GameType::Slaughter
                    ) {
                        labels.push(format!(
                            "Goal: {}{}",
                            settings.parameter,
                            if catalog.modes[settings.mode] == GameType::Slaughter {
                                " minutes"
                            } else {
                                ""
                            }
                        ));
                    }
                    if catalog
                        .factions
                        .iter()
                        .any(|f| !f.ai_difficulties.is_empty())
                    {
                        add(
                            &mut choices,
                            format!("Computer difficulty: {}", settings.ai_difficulty.name()),
                            [48, 370, 544, 28],
                            Pick::AiDifficulty,
                        );
                    }
                    for (i, label) in labels.iter().enumerate().skip(self.offset).take(7) {
                        add(
                            &mut choices,
                            label.clone(),
                            [48, 112 + (i - self.offset) as u16 * 34, 544, 28],
                            if i == 2 && catalog.standard_start {
                                Pick::Action(MenuAction::Unavailable(
                                    "Starting armies are fixed for this game type.".into(),
                                ))
                            } else if catalog.allow_cheats
                                && i == 6 + usize::from(!catalog.tilesets.is_empty())
                            {
                                Pick::Action(MenuAction::Unavailable(
                                    "Cheat-code entry is not supported yet.".into(),
                                ))
                            } else {
                                Pick::MatchOption(i)
                            },
                        );
                    }
                    paging(&mut choices, self.offset, labels.len());
                }
            }
            Page::MatchSlots => choices.extend(self.slot_map_choices()),
            _ => {}
        }
        if !matches!(self.page, Page::Lobby | Page::MatchSlots) {
            add(
                &mut choices,
                if self.page == Page::Lobby {
                    "Leave lobby (Esc)"
                } else {
                    "Back (Esc)"
                }
                .into(),
                [176, 438, 288, 28],
                Pick::Back,
            );
        }
        if self.page == Page::LanMaps {
            choices.extend(self.map_dropdown_choices());
        }
        choices
    }
    pub fn change_match_option(&mut self, index: usize) -> Result<()> {
        let catalog = self.match_catalog.as_ref().context("no match catalog")?;
        let settings = self.match_settings.as_mut().context("no match settings")?;
        let terrain = usize::from(!catalog.tilesets.is_empty());
        match index {
            0 => {
                let next = if catalog.modes[settings.mode] == GameType::Teams {
                    GameType::FreeForAll
                } else {
                    GameType::Teams
                };
                settings.mode = catalog
                    .modes
                    .iter()
                    .position(|m| *m == next)
                    .context("match format unavailable")?;
                settings.parameter = match catalog.modes[settings.mode] {
                    GameType::Slaughter => 10,
                    GameType::CaptureTheFlag | GameType::TeamCaptureTheFlag => 1,
                    _ => 5000,
                };
            }
            1 => cycle(&mut settings.resources, catalog.resources.len()),
            2 => {
                if !catalog.standard_start {
                    settings.map_units = !settings.map_units;
                }
            }
            3 => settings.random_locations = !settings.random_locations,
            4 => settings.fog = !settings.fog,
            5 => settings.speed = (settings.speed + 1) % catalog.speeds.len(),
            6 if terrain == 1 => cycle(&mut settings.tileset, catalog.tilesets.len()),
            i if i == 6 + terrain && catalog.allow_cheats => {
                anyhow::bail!("Cheat-code entry is not supported yet.")
            }
            _ => {
                settings.parameter = match catalog.modes[settings.mode] {
                    GameType::Slaughter => {
                        if settings.parameter >= 60 {
                            10
                        } else {
                            settings.parameter + 10
                        }
                    }
                    GameType::Greed => {
                        if settings.parameter >= 10000 {
                            2500
                        } else {
                            settings.parameter + 2500
                        }
                    }
                    _ => {
                        if settings.parameter >= 10 {
                            1
                        } else {
                            settings.parameter + 1
                        }
                    }
                }
            }
        }
        catalog.validate_settings(self.match_map, settings)
    }
}
fn paging(choices: &mut Vec<Choice>, offset: usize, len: usize) {
    if offset >= 7 {
        add(
            choices,
            "Previous".into(),
            [48, 396, 180, 26],
            Pick::Previous,
        );
    }
    if offset + 7 < len {
        add(choices, "Next".into(), [412, 396, 180, 26], Pick::Next);
    }
}
