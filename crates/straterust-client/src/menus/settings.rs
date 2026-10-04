use crate::Config;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Setting {
    Audio,
    Music,
    Sound,
    Speech,
    Fullscreen,
    Resolution,
    Speed,
    Scroll,
    FrameRate,
}
pub const ALL: [Setting; 9] = [
    Setting::Audio,
    Setting::Music,
    Setting::Sound,
    Setting::Speech,
    Setting::Fullscreen,
    Setting::Resolution,
    Setting::Speed,
    Setting::Scroll,
    Setting::FrameRate,
];
impl Setting {
    pub fn label(self, config: &Config) -> String {
        match self {
            Self::Audio => format!("Audio: {}", if config.audio { "On" } else { "Off" }),
            Self::Music => format!("Music volume: {}%", config.music_volume),
            Self::Sound => format!("Sound effects: {}%", config.sound_volume),
            Self::Speech => format!("Speech volume: {}%", config.speech_volume),
            Self::Fullscreen => format!(
                "Fullscreen: {}",
                if config.fullscreen { "On" } else { "Off" }
            ),
            Self::Resolution => format!("Window size: {} x {}", config.width, config.height),
            Self::Speed => format!("Game speed: {:.2}x", config.game_speed),
            Self::Scroll => format!("Camera scroll speed: {}", config.scroll_speed),
            Self::FrameRate => format!("Frame rate limit: {}", config.frames_per_second),
        }
    }
    /// Left/Right keys go both ways; clicking advances a bounded choice.
    pub fn change(self, config: &mut Config, direction: i32) {
        fn cycle<T: Copy + PartialEq>(value: &mut T, choices: &[T], direction: i32) {
            let current = choices.iter().position(|v| v == value).unwrap_or(0) as i32;
            *value = choices[(current + direction).rem_euclid(choices.len() as i32) as usize];
        }
        match self {
            Self::Audio => config.audio = !config.audio,
            Self::Music => cycle(&mut config.music_volume, &[0, 25, 50, 75, 100], direction),
            Self::Sound => cycle(&mut config.sound_volume, &[0, 25, 50, 75, 100], direction),
            Self::Speech => cycle(&mut config.speech_volume, &[0, 25, 50, 75, 100], direction),
            Self::Fullscreen => config.fullscreen = !config.fullscreen,
            Self::Resolution => {
                let mut size = (config.width, config.height);
                cycle(
                    &mut size,
                    &[
                        (640, 480),
                        (800, 600),
                        (1100, 760),
                        (1280, 800),
                        (1920, 1080),
                    ],
                    direction,
                );
                (config.width, config.height) = size;
            }
            Self::Speed => cycle(
                &mut config.game_speed,
                &[0.5, 0.75, 1.0, 1.25, 1.5, 2.0],
                direction,
            ),
            Self::Scroll => cycle(
                &mut config.scroll_speed,
                &[300, 450, 600, 900, 1200],
                direction,
            ),
            Self::FrameRate => cycle(
                &mut config.frames_per_second,
                &[30, 60, 120, 144, 240],
                direction,
            ),
        }
    }
}
