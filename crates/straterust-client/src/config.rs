use super::*;

#[derive(Clone, serde::Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct Config {
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) zoom: f64,
    pub(super) seed: u64,
    pub(super) frames_per_second: u32,
    pub(super) movement_heading_debounce_ms: u32,
    pub(super) audio: bool,
    pub(super) fullscreen: bool,
    pub(super) music_volume: u8,
    pub(super) sound_volume: u8,
    pub(super) speech_volume: u8,
    pub(super) scroll_speed: u32,
    pub(super) game_speed: f64,
    pub(super) bindings: Bindings,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            width: 1100,
            height: 760,
            zoom: 1.0,
            seed: 42,
            frames_per_second: 60,
            movement_heading_debounce_ms: 500,
            audio: true,
            fullscreen: false,
            music_volume: 100,
            sound_volume: 100,
            speech_volume: 100,
            scroll_speed: 450,
            game_speed: 1.0,
            bindings: Bindings::default(),
        }
    }
}

impl Config {
    pub(super) fn validate(&self) -> Result<()> {
        self.bindings.validate()?;
        ensure!(
            [self.music_volume, self.sound_volume, self.speech_volume]
                .iter()
                .all(|v| *v <= 100),
            "audio levels must be 0..=100"
        );
        ensure!(
            (100..=1500).contains(&self.scroll_speed),
            "scroll_speed must be 100..=1500"
        );
        ensure!(
            self.game_speed.is_finite() && (0.5..=2.0).contains(&self.game_speed),
            "game_speed must be 0.5..=2.0"
        );
        ensure!(
            self.movement_heading_debounce_ms <= 5000,
            "movement_heading_debounce_ms must be 0..=5000"
        );
        ensure!(
            (640..=7680).contains(&self.width) && (480..=4320).contains(&self.height),
            "window size must be 640x480 through 7680x4320"
        );
        ensure!(
            self.zoom.is_finite() && (0.25..=4.0).contains(&self.zoom),
            "zoom must be 0.25..=4"
        );
        ensure!(
            (1..=240).contains(&self.frames_per_second),
            "frames_per_second must be 1..=240"
        );
        Ok(())
    }
}

impl Config {
    pub(super) fn save(&self, path: &Path) -> Result<()> {
        self.validate()?;
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        let temporary = path.with_extension("ron.tmp");
        std::fs::write(
            &temporary,
            ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())?,
        )?;
        std::fs::rename(temporary, path).context("cannot save client settings")
    }
}
