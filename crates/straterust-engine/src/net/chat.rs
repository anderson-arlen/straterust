//! Social messages belong to connections, not deterministic simulation state.
use crate::sim::PlayerId;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

pub const NAME_LIMIT: usize = 24;
pub const TEXT_LIMIT: usize = 256;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Channel {
    #[default]
    All,
    Team,
}
impl Channel {
    pub fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Team => "Team",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub player: PlayerId,
    pub name: String,
    pub channel: Channel,
    pub text: String,
}

pub fn name(value: &str) -> Result<String> {
    bounded(value, NAME_LIMIT, "Player name")
}
pub fn text(value: &str) -> Result<String> {
    bounded(value, TEXT_LIMIT, "Chat message")
}
fn bounded(value: &str, limit: usize, label: &str) -> Result<String> {
    ensure!(
        value.len() <= limit * 4 && !value.chars().any(char::is_control),
        "{label} contains control characters or is too long"
    );
    let value = value.trim();
    ensure!(
        !value.is_empty() && value.chars().count() <= limit,
        "{label} must contain 1..={limit} characters"
    );
    Ok(value.into())
}

/// Limit social traffic independently of commands and simulation ticks.
pub(super) struct Gate {
    window: Instant,
    count: u8,
}
impl Default for Gate {
    fn default() -> Self {
        Self {
            window: Instant::now(),
            count: 0,
        }
    }
}
impl Gate {
    pub fn accept(&mut self) -> Result<()> {
        if self.window.elapsed() >= Duration::from_secs(1) {
            self.window = Instant::now();
            self.count = 0;
        }
        ensure!(
            self.count < 4,
            "Chat is being sent too quickly; try again shortly"
        );
        self.count += 1;
        Ok(())
    }
}
