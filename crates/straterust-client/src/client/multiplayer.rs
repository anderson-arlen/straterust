use super::*;
use straterust_engine::net::lan::{self, LanGame};

pub(super) struct Discovered {
    games: Vec<LanGame>,
    rules: Vec<catalog::RulesPackage>,
}

impl Discovered {
    fn new(mut games: Vec<LanGame>, rules: Vec<catalog::RulesPackage>) -> Self {
        for game in &mut games {
            game.compatible = rules.iter().any(|p| p.identity.same_rules(&game.identity));
        }
        Self { games, rules }
    }
}

impl Client {
    pub(super) fn discover_lan(&mut self) {
        let games = self.menus.games.clone();
        let preferred = self.menus.network_map.clone();
        let (send, receive) = std::sync::mpsc::channel();
        // Rules normalization can be expensive for imported games. Keep it out
        // of menu drawing/input, alongside the multicast discovery itself.
        std::thread::spawn(move || {
            let run = || -> Result<Discovered> {
                let rules = catalog::installed_rules(&games);
                let identity = &rules
                    .iter()
                    .find(|p| Some(&p.directory) == preferred.as_ref())
                    .or_else(|| rules.first())
                    .context("no installed game rules available")?
                    .identity;
                let games = lan::discover(identity, Duration::from_secs(1))?;
                Ok(Discovered::new(games, rules))
            };
            let _ = send.send(run());
        });
        self.discovery = Some(receive);
        self.menus.lan_games.clear();
        self.lan_rules.clear();
        self.menus.message = "Searching the LAN...".into();
    }

    pub(super) fn poll_lan(&mut self) {
        if let Some(result) = self
            .discovery
            .as_ref()
            .and_then(|receiver| receiver.try_recv().ok())
        {
            self.discovery = None;
            match result {
                Ok(Discovered { games, rules }) => {
                    self.menus.message = format!("{} LAN matches found.", games.len());
                    self.menus.lan_games = games;
                    self.lan_rules = rules;
                }
                Err(error) => self.menus.message = format!("LAN discovery failed: {error}"),
            }
        }
    }

    pub(super) fn lan_join(&self, index: usize) -> Result<(PathBuf, std::net::SocketAddr)> {
        let game = self
            .menus
            .lan_games
            .get(index)
            .context("LAN match no longer listed")?;
        let matching = |p: &&catalog::RulesPackage| p.identity.same_rules(&game.identity);
        let package = self
            .lan_rules
            .iter()
            .filter(matching)
            .find(|p| Some(&p.directory) == self.menus.network_map.as_ref())
            .or_else(|| self.lan_rules.iter().find(matching))
            .context("No installed package matches this host's game rules. Install the same game package/version.")?;
        Ok((package.directory.clone(), game.address))
    }
}

#[cfg(test)]
mod tests;
