//! Save menu orchestration stays outside the authoritative server.
use super::*;

impl Client {
    fn save_directory(&self) -> PathBuf {
        self.settings_path
            .parent()
            .unwrap_or(Path::new("local"))
            .join("saves")
    }

    pub(super) fn open_saves(&mut self, saving: bool) -> Result<()> {
        ensure!(
            !self.menus.multiplayer,
            "save/load is available for local games only"
        );
        if saving {
            let app = self
                .session
                .as_ref()
                .context("start a local game before saving")?;
            ensure!(
                app.playback_end.is_none(),
                "cannot save a scenario playback"
            );
        }
        let game = &self
            .menus
            .game
            .as_ref()
            .context("choose a game first")?
            .directory;
        self.menus.save_labels = crate::saves::labels(&self.save_directory(), game);
        self.menus.navigate(Page::Saves(saving));
        Ok(())
    }

    pub(super) fn save_slot(&mut self, slot: usize) -> Result<()> {
        let path = crate::saves::slot_path(&self.save_directory(), slot)?;
        if path.exists() && self.menus.page != Page::Overwrite(slot) {
            self.menus.navigate(Page::Overwrite(slot));
            return Ok(());
        }
        ensure!(
            self.session
                .as_ref()
                .is_some_and(|app| app.network.is_none()),
            "no local session to save"
        );
        self.save_pending = Some((slot, false));
        self.menus.message = "Saving game...".into();
        self.poll_saves();
        Ok(())
    }

    pub(super) fn poll_saves(&mut self) {
        let Some((slot, started)) = self.save_pending else {
            return;
        };
        let result = (|| -> Result<Option<PathBuf>> {
            let path = crate::saves::slot_path(&self.save_directory(), slot)?;
            let game = self
                .menus
                .game
                .as_ref()
                .context("no selected game")?
                .directory
                .clone();
            let app = self
                .session
                .as_mut()
                .context("local session ended while saving")?;
            app.poll_simulation()?;
            let server = app.simulation.as_ref().context("no local server")?;
            if !started {
                if server.is_pending() {
                    return Ok(None);
                }
                let header = crate::saves::SaveHeader::capture(app, &game)?;
                server.save(path, header, app.queue.commands().cloned().collect())?;
                app.queue = CommandQueue::default();
                self.save_pending = Some((slot, true));
            }
            server.poll_save()
        })();
        match result {
            Ok(Some(_)) => {
                self.save_pending = None;
                self.menus.open_pause();
                self.menus.message = format!("Game saved in slot {}.", slot + 1);
                self.sync_menu();
            }
            Ok(None) => {}
            Err(error) => {
                self.save_pending = None;
                self.menus.message = format!("Could not save: {error:#}");
            }
        }
    }

    pub(super) fn load_slot(&mut self, slot: usize) -> Result<()> {
        ensure!(
            !self.menus.multiplayer,
            "cannot load into a multiplayer session"
        );
        let path = crate::saves::slot_path(&self.save_directory(), slot)?;
        let header = crate::saves::read_header(&path)?;
        ensure!(
            self.menus
                .game
                .as_ref()
                .is_some_and(|g| header.package.starts_with(&g.directory)),
            "select the saved game's package first"
        );
        let mut config = self.config.clone();
        config.audio = false;
        // Validate and fully construct the replacement before touching the live game.
        let app = App::load_saved(&path, config)?;
        self.install_session(app)
    }
}

#[cfg(test)]
mod tests;
