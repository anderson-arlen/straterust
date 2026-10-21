//! Bounded, local chat history and composer; messages never issue unit orders.
use crate::*;
use std::collections::VecDeque;
use straterust_engine::net::chat::{self, Channel, Message};

#[derive(Default)]
pub(crate) struct Chat {
    pub open: bool,
    pub channel: Channel,
    pub draft: String,
    pub history: VecDeque<(Instant, Message)>,
    pub scroll: usize,
}
impl Chat {
    pub fn receive(&mut self, message: Message) -> Result<()> {
        chat::name(&message.name)?;
        chat::text(&message.text)?;
        if self.history.len() == 64 {
            self.history.pop_front();
        }
        self.history.push_back((Instant::now(), message));
        self.scroll = 0;
        Ok(())
    }
    pub fn begin(&mut self) {
        self.open = true;
        self.scroll = 0;
    }
    pub fn close(&mut self) {
        self.open = false;
        self.draft.clear();
        self.scroll = 0;
    }
    /// Returns false only when gameplay/menu input should still handle the key.
    pub fn edit(&mut self, code: KeyCode, text: Option<&str>, repeat: bool, teams: bool) -> bool {
        if !self.open {
            if code == KeyCode::Enter && !repeat {
                self.begin();
                return true;
            }
            return false;
        }
        match code {
            KeyCode::Escape => self.close(),
            KeyCode::Tab if !repeat => {
                self.channel = if teams && self.channel == Channel::All {
                    Channel::Team
                } else {
                    Channel::All
                };
            }
            KeyCode::Backspace => {
                self.draft.pop();
            }
            KeyCode::PageUp => {
                self.scroll = (self.scroll + 4).min(self.history.len().saturating_sub(1))
            }
            KeyCode::PageDown => self.scroll = self.scroll.saturating_sub(4),
            _ => {
                if let Some(text) = text {
                    append(&mut self.draft, text, chat::TEXT_LIMIT);
                }
            }
        }
        true
    }
}
pub(crate) fn append(value: &mut String, text: &str, limit: usize) {
    for ch in text.chars().filter(|ch| !ch.is_control()) {
        if value.chars().count() >= limit {
            break;
        }
        value.push(ch);
    }
}
impl App {
    pub(crate) fn chat_teams(&self) -> bool {
        self.world
            .map()
            .multiplayer
            .as_ref()
            .is_some_and(|m| !m.teams.is_empty())
            || self
                .world
                .map()
                .mission
                .as_ref()
                .is_some_and(|m| !m.alliances.is_empty())
    }
    pub(crate) fn chat_key(&mut self, event: &winit::event::KeyEvent, teams: bool) -> Result<bool> {
        let Some(network) = &self.network else {
            return Ok(false);
        };
        if network.ended || event.state != ElementState::Pressed {
            return Ok(false);
        }
        let PhysicalKey::Code(code) = event.physical_key else {
            return Ok(self.chat.open);
        };
        if !teams {
            self.chat.channel = Channel::All;
        }
        if self.chat.open && code == KeyCode::Enter && !event.repeat {
            if self.chat.draft.trim().is_empty() {
                self.chat.close();
            } else {
                network.chat(self.chat.channel, &self.chat.draft)?;
                self.chat.close();
            }
            return Ok(true);
        }
        let handled = self
            .chat
            .edit(code, event.text.as_deref(), event.repeat, teams);
        if handled {
            self.keys.clear();
            self.drag_start = None;
            self.minimap_drag = false;
        }
        Ok(handled)
    }
}

#[cfg(test)]
mod tests;
