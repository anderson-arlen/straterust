use super::*;
use straterust_engine::net::{chat, lobby};

impl Client {
    pub(super) fn save_name(&mut self) -> Result<()> {
        let name = chat::name(&self.menus.name_input)?;
        let mut config = self.config.clone();
        config.player_name = name.clone();
        config.save(&self.settings_path)?;
        self.config = config;
        if let Some(app) = &mut self.session {
            app.config.player_name = name.clone();
            if let Some(network) = &app.network
                && !network.started
                && !network.ended
            {
                network.lobby_request(lobby::Request::Name(name))?;
            }
        }
        self.menus.escape(self.session.is_some());
        Ok(())
    }

    pub(super) fn social_key(&mut self, event: &winit::event::KeyEvent) -> Result<bool> {
        if self.menus.page == Page::PlayerName && event.state == ElementState::Pressed {
            match event.physical_key {
                PhysicalKey::Code(KeyCode::Enter) if !event.repeat => self.save_name()?,
                PhysicalKey::Code(KeyCode::Escape) if !event.repeat => {
                    self.menus.escape(self.session.is_some());
                }
                PhysicalKey::Code(KeyCode::Backspace) => {
                    if self.menus.name_selected {
                        self.menus.name_input.clear();
                    } else {
                        self.menus.name_input.pop();
                    }
                    self.menus.name_selected = false;
                }
                _ => {
                    if let Some(text) = &event.text
                        && text.chars().any(|c| !c.is_control())
                    {
                        if self.menus.name_selected {
                            self.menus.name_input.clear();
                        }
                        self.menus.name_selected = false;
                        crate::chat::append(&mut self.menus.name_input, text, chat::NAME_LIMIT);
                    }
                }
            }
            self.sync_menu();
            return Ok(true);
        }
        if !matches!(self.menus.page, Page::Lobby | Page::Closed) {
            return Ok(false);
        }
        let teams = if self.menus.page == Page::Lobby {
            self.menus
                .match_catalog
                .as_ref()
                .zip(self.menus.match_settings.as_ref())
                .is_some_and(|(c, s)| lobby::has_teams(c.modes[s.mode]))
        } else {
            self.session.as_ref().is_some_and(App::chat_teams)
        };
        if let Some(app) = &mut self.session {
            app.chat_key(event, teams)
        } else {
            Ok(false)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn player_name_entry_persists_and_old_configs_keep_a_default() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("client-settings.ron");
        let legacy: Config = ron::from_str("(width:800,height:600)").unwrap();
        assert_eq!(legacy.player_name, "Player");
        legacy.validate().unwrap();
        let mut client = Client::new(legacy, path.clone(), Vec::new());
        client.menus.page = Page::Multiplayer;
        let choices = client.menus.choices(&client.config);
        assert!(choices.iter().any(|c| c.pick == Pick::PlayerName));
        client.pick(Pick::PlayerName).unwrap();
        assert_eq!(client.menus.page, Page::PlayerName);
        client.menus.name_input = "  Commander  ".into();
        client.pick(Pick::SaveName).unwrap();
        assert_eq!(client.menus.page, Page::Multiplayer);
        let saved: Config = read_ron(&path).unwrap();
        assert_eq!(saved.player_name, "Commander");
        client.pick(Pick::PlayerName).unwrap();
        client.menus.name_input = "bad\nname".into();
        assert!(client.pick(Pick::SaveName).is_err());
        assert_eq!(client.config.player_name, "Commander");
        client.pick(Pick::Back).unwrap();
        assert_eq!(read_ron::<Config>(&path).unwrap().player_name, "Commander");
    }
}
