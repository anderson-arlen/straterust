//! Local debug controls and bounded histories; these never change simulation.
use crate::*;
use std::collections::VecDeque;

pub const COMMANDS: &[(&str, &str)] = &[
    ("help", "List commands"),
    ("debug on", "Show FPS and network graphs"),
    ("debug off", "Hide debug graphs"),
    ("debug toggle", "Toggle debug graphs"),
    ("fps on", "Show the FPS graph"),
    ("fps off", "Hide the FPS graph"),
    ("net on", "Show upload/download graphs"),
    ("net off", "Hide network graphs"),
    ("status", "Print current FPS, bandwidth and snapshot tick"),
    ("clear", "Clear console output"),
];
#[derive(Clone, Copy, Default)]
pub struct Sample {
    pub fps: f64,
    pub up: f64,
    pub down: f64,
}
pub struct Diagnostics {
    pub fps: bool,
    pub network: bool,
    pub samples: VecDeque<Sample>,
    pub current: Sample,
    pub open: bool,
    pub height: f64,
    pub input: String,
    pub caret: usize,
    pub output: VecDeque<String>,
    pub scroll: usize,
    pub completion: usize,
    completion_seed: Option<String>,
    history: VecDeque<String>,
    history_index: Option<usize>,
    elapsed: Duration,
    frames: u32,
    bytes: [u64; 2],
    tick: u64,
}
impl Default for Diagnostics {
    fn default() -> Self {
        Self {
            fps: true,
            network: true,
            samples: VecDeque::new(),
            current: Sample::default(),
            open: false,
            height: 0.,
            input: String::new(),
            caret: 0,
            output: VecDeque::from([
                "Console: Tab completes commands; help lists them. PgUp/PgDn scroll.".into(),
            ]),
            scroll: 0,
            completion: 0,
            completion_seed: None,
            history: VecDeque::new(),
            history_index: None,
            elapsed: Duration::ZERO,
            frames: 0,
            bytes: [0, 0],
            tick: 0,
        }
    }
}
impl Diagnostics {
    pub fn sample(&mut self, elapsed: Duration, bytes: [u64; 2], tick: u64) {
        let target = if self.open { 0.5 } else { 0. };
        let movement = elapsed.as_secs_f64() * 3.;
        self.height = if self.height < target {
            (self.height + movement).min(target)
        } else {
            (self.height - movement).max(target)
        };
        self.tick = tick;
        self.elapsed += elapsed;
        self.frames += 1;
        if self.elapsed >= Duration::from_millis(200) {
            let seconds = self.elapsed.as_secs_f64();
            self.current = Sample {
                fps: f64::from(self.frames) / seconds,
                up: bytes[0].saturating_sub(self.bytes[0]) as f64 / seconds,
                down: bytes[1].saturating_sub(self.bytes[1]) as f64 / seconds,
            };
            self.samples.push_back(self.current);
            if self.samples.len() > 120 {
                self.samples.pop_front();
            }
            self.elapsed = Duration::ZERO;
            self.frames = 0;
            self.bytes = bytes;
        }
    }
    pub fn suggestions(&self) -> Vec<&'static str> {
        let query = self
            .completion_seed
            .as_deref()
            .unwrap_or(&self.input)
            .trim()
            .to_ascii_lowercase();
        let mut matches: Vec<_> = COMMANDS
            .iter()
            .filter_map(|(command, _)| {
                let mut position = 0;
                let mut gaps = 0;
                for ch in query.chars().filter(|c| !c.is_whitespace()) {
                    let found = command[position..].find(ch)?;
                    gaps += found;
                    position += found + ch.len_utf8();
                }
                Some((
                    (!command.starts_with(&query)) as usize * 100 + gaps + command.len(),
                    *command,
                ))
            })
            .collect();
        matches.sort_unstable();
        matches.into_iter().map(|(_, c)| c).collect()
    }
    pub fn edit(&mut self, code: KeyCode, text: Option<&str>, repeat: bool) -> bool {
        if code == KeyCode::Backquote {
            if !repeat {
                self.open = !self.open;
            }
            return true;
        }
        if !self.open {
            return false;
        }
        match code {
            KeyCode::Escape => self.open = false,
            KeyCode::Enter if !repeat => {
                let command = self.input.trim().to_ascii_lowercase();
                if !command.is_empty() {
                    self.print(format!("> {command}"));
                    self.execute(&command);
                    self.history.push_back(command);
                    if self.history.len() > 32 {
                        self.history.pop_front();
                    }
                }
                self.input.clear();
                self.caret = 0;
                self.history_index = None;
                self.completion_seed = None;
                self.completion = 0;
            }
            KeyCode::Tab => {
                if self.completion_seed.is_some() {
                    self.completion += 1;
                } else {
                    self.completion_seed = Some(self.input.clone());
                    self.completion = 0;
                }
                let suggestions = self.suggestions();
                if !suggestions.is_empty() {
                    self.completion %= suggestions.len();
                    self.input = suggestions[self.completion].into();
                    self.caret = self.input.len();
                }
            }
            KeyCode::ArrowUp | KeyCode::ArrowDown => {
                if !self.history.is_empty() {
                    let index = if code == KeyCode::ArrowUp {
                        self.history_index
                            .unwrap_or(self.history.len())
                            .saturating_sub(1)
                    } else {
                        (self.history_index.unwrap_or(self.history.len()) + 1)
                            .min(self.history.len())
                    };
                    self.history_index = Some(index);
                    self.input = self.history.get(index).cloned().unwrap_or_default();
                    self.caret = self.input.len();
                    self.completion_seed = None;
                }
            }
            KeyCode::ArrowLeft => {
                self.caret = self.input[..self.caret]
                    .char_indices()
                    .last()
                    .map_or(0, |(i, _)| i);
            }
            KeyCode::ArrowRight => {
                if let Some(ch) = self.input[self.caret..].chars().next() {
                    self.caret += ch.len_utf8();
                }
            }
            KeyCode::Home => self.caret = 0,
            KeyCode::End => self.caret = self.input.len(),
            KeyCode::Backspace => {
                if let Some((index, _)) = self.input[..self.caret].char_indices().last() {
                    self.input.drain(index..self.caret);
                    self.caret = index;
                }
                self.completion_seed = None;
            }
            KeyCode::Delete => {
                if let Some(ch) = self.input[self.caret..].chars().next() {
                    self.input.drain(self.caret..self.caret + ch.len_utf8());
                }
                self.completion_seed = None;
            }
            KeyCode::PageUp => {
                self.scroll = (self.scroll + 4).min(self.output.len().saturating_sub(1))
            }
            KeyCode::PageDown => self.scroll = self.scroll.saturating_sub(4),
            _ => {
                if let Some(text) = text {
                    for ch in text.chars().filter(|c| !c.is_control()) {
                        if self.input.chars().count() >= 256 {
                            break;
                        }
                        self.input.insert(self.caret, ch);
                        self.caret += ch.len_utf8();
                    }
                    self.completion_seed = None;
                    self.completion = 0;
                }
            }
        }
        true
    }
    fn print(&mut self, text: String) {
        if self.output.len() == 128 {
            self.output.pop_front();
        }
        self.output.push_back(text);
        self.scroll = 0;
    }
    fn execute(&mut self, command: &str) {
        match command {
            "debug on" => {
                self.fps = true;
                self.network = true;
                self.print("Debug graphs on.".into());
            }
            "debug off" => {
                self.fps = false;
                self.network = false;
                self.print("Debug graphs off.".into());
            }
            "debug toggle" => {
                let enabled = !(self.fps || self.network);
                self.fps = enabled;
                self.network = enabled;
                self.print(format!(
                    "Debug graphs {}.",
                    if enabled { "on" } else { "off" }
                ));
            }
            "fps on" => self.fps = true,
            "fps off" => self.fps = false,
            "net on" => self.network = true,
            "net off" => self.network = false,
            "clear" => self.output.clear(),
            "help" => {
                for (name, description) in COMMANDS {
                    self.print(format!("{name}: {description}"));
                }
            }
            "status" => self.print(format!(
                "FPS {:.1}; up {}; down {}; tick {}",
                self.current.fps,
                rate(self.current.up),
                rate(self.current.down),
                self.tick
            )),
            _ => self.print("Unknown command. Use Tab for fuzzy completion or type help.".into()),
        }
    }
}
pub fn rate(bytes: f64) -> String {
    if bytes >= 1_000_000. {
        format!("{:.2} MB/s", bytes / 1_000_000.)
    } else if bytes >= 1000. {
        format!("{:.1} kB/s", bytes / 1000.)
    } else {
        format!("{bytes:.0} B/s")
    }
}
impl App {
    pub(crate) fn console_key(&mut self, event: &winit::event::KeyEvent) -> bool {
        if event.state != ElementState::Pressed {
            return self.diagnostics.open;
        }
        let PhysicalKey::Code(code) = event.physical_key else {
            return self.diagnostics.open;
        };
        let handled = self
            .diagnostics
            .edit(code, event.text.as_deref(), event.repeat);
        if handled {
            self.keys.clear();
            self.drag_start = None;
            self.minimap_drag = false;
            self.chat.close();
        }
        handled
    }
}

#[cfg(test)]
mod tests;
