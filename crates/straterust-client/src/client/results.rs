use super::*;

impl Client {
    pub(super) fn show_match_results(&mut self) {
        if self.menus.page == Page::Results {
            return;
        }
        let Some(app) = &mut self.session else { return };
        // Explicit scenario playback retains its developer playback controls.
        if app.playback_end.is_some() || app.smoke {
            return;
        }
        let Some(result) = &app.match_result else {
            return;
        };
        self.menus.result = Some(result.clone());
        self.menus.result_player = app.world.view_player();
        app.status = result
            .players
            .iter()
            .find(|p| p.player == app.world.view_player())
            .map_or("Match finished.", |p| match p.outcome {
                straterust_engine::session::MatchOutcome::Victory => "Victory.",
                straterust_engine::session::MatchOutcome::Defeat => "Defeat.",
                straterust_engine::session::MatchOutcome::Draw => "Draw.",
            })
            .into();
        self.menus.multiplayer = app.network.is_some();
        self.menus.continue_campaign = app.world.state().winner == Some(app.world.view_player())
            && app
                .campaign
                .as_ref()
                .is_some_and(|c| c.index + 1 < c.manifest.missions.len());
        app.paused = true;
        self.menus.page = Page::Results;
        self.menus.history.clear();
        self.menus.details.clear();
        self.menus.reset();
        self.menus.focus = Some(0);
        self.sync_menu();
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    pub(super) fn dismiss_results(&mut self) -> Result<()> {
        if self.menus.continue_campaign
            && let Some(app) = &mut self.session
            && app.advance_campaign()?
        {
            self.menus.result = None;
            self.menus.page = Page::Closed;
            self.menus.history.clear();
            self.menus.reset();
            self.sync_menu();
            return Ok(());
        }
        let multiplayer = self.menus.multiplayer;
        // A completed match has already stopped advancing. Finish its transport
        // before reopening Host so the old listener cannot keep the port bound.
        if multiplayer && let Some(network) = self.session.as_mut().and_then(|a| a.network.take()) {
            network.finish()?;
        }
        self.end_session();
        self.menus.result = None;
        self.menus.multiplayer = false;
        self.menus.details.clear();
        if multiplayer {
            self.menus.page = Page::Multiplayer;
            self.menus.history = vec![Page::Authored(self.menus.pack.manifest.home.clone())];
            self.menus.lan_games.clear();
            self.lan_rules.clear();
            self.menus.address_selected = true;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
