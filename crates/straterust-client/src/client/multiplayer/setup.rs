use super::*;
use crate::menus::map_browser::Dropdown;
use straterust_engine::net::lobby::Request;
impl Client {
    pub(in crate::client) fn pick_match(&mut self, pick: &Pick) -> Result<bool> {
        if self.menus.match_catalog.is_none() {
            return Ok(false);
        }
        let local = self.menus.lobby.as_ref().map(|l| l.0);
        let is_host = self
            .menus
            .lobby
            .as_ref()
            .is_none_or(|(slot, view)| *slot == view.host && !view.running);
        match pick {
            Pick::Action(MenuAction::Multiplayer) => {
                self.menus.address_selected = true;
                self.menus.navigate(Page::Multiplayer);
            }
            Pick::CreateGame => {
                if self.menus.match_settings.is_none() || self.menus.network_map.is_none() {
                    self.menus.select_map(self.menus.match_map)?;
                }
                self.menus.address_selected = true;
                self.menus.navigate(Page::HostSetup);
            }
            Pick::AiDifficulty => {
                ensure!(is_host, "only the host can change AI difficulty");
                let settings = self
                    .menus
                    .match_settings
                    .as_mut()
                    .context("no match settings")?;
                settings.ai_difficulty = settings.ai_difficulty.next();
                self.configure_lobby()?;
            }
            Pick::TakeSlot(slot) => self.lobby_request(Request::Slot(*slot))?,
            Pick::SelectSlot(slot) => {
                self.menus.selected_slot = Some(*slot);
                self.menus.slot_field = None;
                self.menus.focus = None;
            }
            Pick::SlotField(field) => self.menus.slot_field = (*field < 4).then_some(*field),
            Pick::SetSlot(index, field, value) => {
                ensure!(
                    is_host || local == Some(*index) && matches!(*field, 1..=3),
                    "only the host or this player can change the position"
                );
                if *field == 0 && *value >= 3 {
                    self.lobby_request(Request::Assign {
                        from: value - 3,
                        to: *index,
                    })?;
                } else if *field == 3 {
                    let catalog = self.menus.match_catalog.as_ref().unwrap();
                    let mut settings = self
                        .menus
                        .match_settings
                        .clone()
                        .context("no match settings")?;
                    catalog.choose_color(&mut settings, *index, *value)?;
                    if local == Some(*index) {
                        self.lobby_request(Request::Color(*value))?;
                    } else {
                        self.menus.match_settings = Some(settings);
                        self.configure_lobby()?;
                    }
                } else {
                    let slots = &mut self
                        .menus
                        .match_settings
                        .as_mut()
                        .context("no match settings")?
                        .slots;
                    let slot = slots.get_mut(*index).context("unknown player position")?;
                    match field {
                        0 => {
                            ensure!(
                                !self
                                    .menus
                                    .lobby
                                    .as_ref()
                                    .is_some_and(|(_, v)| v.players[*index].is_some()),
                                "move the connected player before closing this position"
                            );
                            slot.kind = match value {
                                0 => straterust_engine::multiplayer::SlotKind::Open,
                                1 => straterust_engine::multiplayer::SlotKind::Computer,
                                _ => straterust_engine::multiplayer::SlotKind::Closed,
                            };
                            slot.color = None;
                        }
                        1 => slot.faction = value.checked_sub(1),
                        _ => slot.team = *value as u8,
                    }
                    let faction = slot.faction;
                    let team = slot.team;
                    if local == Some(*index) && *field == 1 {
                        self.lobby_request(Request::Faction(faction))?;
                    } else if local == Some(*index) && *field == 2 {
                        self.lobby_request(Request::Team(team))?;
                    } else {
                        self.configure_lobby()?;
                    }
                }
                self.menus.slot_field = None;
                self.menus.focus = None;
            }
            Pick::MatchOptions => self.menus.navigate(Page::MatchOptions),
            Pick::MatchSlots => self.menus.navigate(Page::MatchSlots),
            Pick::ChooseLanMap => {
                self.menus.navigate(Page::LanMaps);
                self.menus.browse_map(self.menus.match_map);
                self.poll_map_browser();
            }
            Pick::MapFilter(players) => {
                self.menus.toggle_map_dropdown(if *players {
                    Dropdown::Players
                } else {
                    Dropdown::Size
                });
            }
            Pick::MapPlayers(players) => {
                self.menus.map_players = *players;
                self.menus.map_filter_changed();
            }
            Pick::MapSize(size) => {
                self.menus.map_size = *size;
                self.menus.map_filter_changed();
            }
            Pick::MapVersions => self.menus.toggle_map_dropdown(Dropdown::Versions),
            Pick::DropdownPage(next) => {
                self.menus.map_browser.dropdown_offset = if *next {
                    self.menus.map_browser.dropdown_offset + 12
                } else {
                    self.menus.map_browser.dropdown_offset.saturating_sub(12)
                };
                self.menus.focus = None;
            }
            Pick::LanMap(index) | Pick::MapVersion(index) => {
                self.menus.browse_map(*index);
                self.poll_map_browser();
            }
            Pick::UseLanMap => {
                let index = self
                    .menus
                    .map_browser
                    .selected
                    .context("select a map to preview")?;
                self.menus.select_map(index)?;
                self.configure_lobby()?;
                self.menus.escape(false);
            }
            Pick::MapDetails => {
                let index = self.menus.map_browser.selected.context("select a map")?;
                let entry = &self.menus.match_catalog.as_ref().unwrap().maps[index];
                self.menus.details = vec![
                    entry.title.clone(),
                    self.menus.version_label(index),
                    format!(
                        "{} players, {}x{}",
                        entry.players, entry.size[0], entry.size[1]
                    ),
                ];
                if let Some(overview) = self
                    .menus
                    .map_browser
                    .overviews
                    .get(index)
                    .and_then(Option::as_ref)
                {
                    self.menus
                        .details
                        .extend(overview.fields.iter().map(|(k, v)| format!("{k}: {v}")));
                }
                self.menus.details.push("These are this version's map defaults. Match options can override resources, races and starting units.".into());
                self.menus.navigate(Page::MapDetails);
            }
            Pick::MatchOption(index) => {
                ensure!(is_host, "only the host can change match settings");
                self.menus.change_match_option(*index)?;
                self.configure_lobby()?;
            }
            Pick::Ready => {
                let (slot, view) = self.menus.lobby.as_ref().context("still connecting")?;
                self.lobby_request(Request::Ready(
                    !view.players[*slot].as_ref().context("not joined")?.ready,
                ))?;
            }
            Pick::StartMatch => {
                self.lobby_request(Request::Start)?;
                if let Some(network) = self.session.as_mut().and_then(|a| a.network.as_mut()) {
                    network.starting = true;
                }
            }
            Pick::Back if self.menus.page == Page::Lobby => {
                if let Some(network) = self.session.as_mut().and_then(|app| app.network.take()) {
                    network.finish()?;
                }
                self.end_session();
                self.menus.lobby = None;
                self.menus.page = Page::Multiplayer;
            }
            Pick::Host | Pick::Join | Pick::JoinLan(_) => {
                let hosting = *pick == Pick::Host;
                let (directory, address) = if let Pick::JoinLan(index) = pick {
                    self.lan_join(*index)?
                } else if hosting {
                    if self.menus.match_settings.is_none() || self.menus.network_map.is_none() {
                        self.menus.select_map(self.menus.match_map)?;
                    }
                    (
                        self.menus.network_map.clone().unwrap(),
                        self.host_address()?,
                    )
                } else {
                    (
                        self.join_directory()?,
                        self.menus
                            .address
                            .parse()
                            .context("enter an IP address and port")?,
                    )
                };
                let (map, settings) = if hosting {
                    (
                        self.menus.match_map,
                        self.menus.match_settings.clone().unwrap(),
                    )
                } else {
                    (0, self.menus.match_catalog.as_ref().unwrap().defaults(0))
                };
                let setup = crate::network::LobbySetup {
                    root: self
                        .menus
                        .game
                        .as_ref()
                        .context("no game selected")?
                        .directory
                        .clone(),
                    catalog: self.menus.match_catalog.clone().unwrap(),
                    map,
                    settings,
                };
                self.play_mode(&directory, None, true)?;
                self.session
                    .as_mut()
                    .unwrap()
                    .start_lobby(setup, address, hosting)?;
                self.menus.lobby = None;
                self.menus.page = Page::Lobby;
                self.menus.history.clear();
            }
            _ => return Ok(false),
        }
        Ok(true)
    }
    fn lobby_request(&self, request: Request) -> Result<()> {
        if let Some(network) = self.session.as_ref().and_then(|a| a.network.as_ref()) {
            network.lobby_request(request)?;
        }
        Ok(())
    }
    fn configure_lobby(&self) -> Result<()> {
        self.lobby_request(Request::Configure {
            map: self.menus.match_map,
            settings: self
                .menus
                .match_settings
                .clone()
                .context("no match settings")?,
        })
    }
    pub(in crate::client) fn poll_lobby(&mut self) {
        if self.menus.match_catalog.is_none() {
            return;
        }
        if let Some(app) = &mut self.session
            && let Some(network) = &mut app.network
        {
            if let Some((slot, view)) = network.lobby.take() {
                self.menus.match_map = view.map;
                self.menus.match_settings = Some(view.settings.clone());
                self.menus.lobby = Some((slot, view));
            }
            if network.started && self.menus.lobby.is_some() {
                self.menus.lobby = None;
                self.menus.page = Page::Closed;
                app.menu_open = false;
            } else if self.menus.lobby.is_some() || self.menus.page == Page::Lobby {
                self.menus.message = app.status.clone();
            }
        }
    }
}
