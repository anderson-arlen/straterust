//! Mission text, briefing and sound are presentation; this clock never changes the world.
use std::time::Duration;

use crate::audio::Audio;
use straterust_engine::{
    media::{BriefingAction, MediaPack},
    sim::{MissionEvent, Position, UnitTypeId, World},
};

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MissionUi {
    pub briefing: bool,
    pub briefing_finished: bool,
    pub objectives: Option<u16>,
    pub text: Option<u16>,
    pub portraits: [Option<UnitTypeId>; 4],
    pub active_slot: Option<usize>,
    pub signal: Option<Position>,
    pub elapsed_ms: u64,
    next_briefing: usize,
    remaining_ms: u64,
    talk_until_ms: u64,
    caption_until_ms: Option<u64>,
    seen_events: usize,
}
impl MissionUi {
    pub fn new(media: &MediaPack, briefing: bool) -> Self {
        Self {
            briefing: briefing && !media.briefing.is_empty(),
            ..Self::default()
        }
    }
    pub fn talking(&self) -> bool {
        self.elapsed_ms < self.talk_until_ms
    }
    pub fn start(&mut self, audio: &mut Audio) {
        self.briefing = false;
        self.text = None;
        self.portraits = [None; 4];
        self.active_slot = None;
        self.remaining_ms = 0;
        self.talk_until_ms = 0;
        self.caption_until_ms = None;
        audio.stop_mission();
    }
    pub fn advance(&mut self, elapsed: Duration, media: &MediaPack, audio: &mut Audio) {
        let mut elapsed = elapsed.as_millis().min(u128::from(u64::MAX)) as u64;
        self.elapsed_ms = self.elapsed_ms.saturating_add(elapsed);
        self.expire_caption();
        if !self.briefing {
            return;
        }
        loop {
            if elapsed < self.remaining_ms {
                self.remaining_ms -= elapsed;
                break;
            }
            elapsed -= self.remaining_ms;
            self.remaining_ms = 0;
            let Some(action) = media.briefing.get(self.next_briefing) else {
                self.briefing_finished = true;
                break;
            };
            self.next_briefing += 1;
            match action {
                BriefingAction::Objectives { text } => self.objectives = Some(*text),
                BriefingAction::ShowPortrait { slot, portrait } => {
                    self.portraits[usize::from(*slot)] = Some(*portrait)
                }
                BriefingAction::HidePortrait { slot } => self.portraits[usize::from(*slot)] = None,
                BriefingAction::SpeakPortrait { slot, milliseconds } => {
                    self.active_slot = Some(usize::from(*slot));
                    self.talk_until_ms = self
                        .elapsed_ms
                        .saturating_sub(elapsed)
                        .saturating_add(u64::from(*milliseconds));
                }
                BriefingAction::Wait { milliseconds } => {
                    self.remaining_ms = u64::from(*milliseconds)
                }
                BriefingAction::Sound { sound } => audio.play_mission(*sound, None),
                BriefingAction::Transmission {
                    slot,
                    text,
                    sound,
                    milliseconds,
                } => {
                    self.text = Some(*text);
                    self.active_slot = Some(usize::from(*slot));
                    self.talk_until_ms = self
                        .elapsed_ms
                        .saturating_sub(elapsed)
                        .saturating_add(u64::from(*milliseconds));
                    if let Some(sound) = sound {
                        audio.play_mission(*sound, self.portraits[usize::from(*slot)]);
                    }
                    self.remaining_ms = u64::from(*milliseconds);
                    let caption_ms = sound
                        .and_then(|id| media.mission_audio.get(usize::from(id)))
                        .map_or(u64::from(*milliseconds), |clip| clip.duration_ms());
                    self.caption_until_ms = Some(
                        self.elapsed_ms
                            .saturating_sub(elapsed)
                            .saturating_add(caption_ms),
                    );
                }
                BriefingAction::Text { text, milliseconds } => {
                    self.text = Some(*text);
                    self.active_slot = None;
                    // Source MBRF action 3 sets caption lifetime and immediately continues.
                    // A zero lifetime is the persistent final briefing message.
                    self.caption_until_ms = (*milliseconds != 0).then_some(
                        self.elapsed_ms
                            .saturating_sub(elapsed)
                            .saturating_add(u64::from(*milliseconds)),
                    );
                }
            }
        }
        self.expire_caption();
    }
    fn expire_caption(&mut self) {
        if self
            .caption_until_ms
            .is_some_and(|until| self.elapsed_ms >= until)
        {
            self.text = None;
            self.active_slot = None;
            self.signal = None;
            self.caption_until_ms = None;
        }
    }
    pub fn observe(
        &mut self,
        world: &World,
        media: &MediaPack,
        audio: &mut Audio,
    ) -> Option<Position> {
        let state = world.state().mission.as_ref()?;
        let mut camera = None;
        for event in &state.events[self.seen_events.min(state.events.len())..] {
            match event {
                MissionEvent::Objectives { text } => self.objectives = Some(*text),
                MissionEvent::Text { text } => {
                    self.text = Some(*text);
                    self.active_slot = None;
                    self.caption_until_ms = Some(self.elapsed_ms.saturating_add(10_000));
                }
                MissionEvent::Sound { sound } => audio.play_mission(*sound, None),
                MissionEvent::CenterView { position } => camera = Some(*position),
                MissionEvent::Speech { muted } => audio.mute_unit_speech(*muted),
                MissionEvent::Transmission {
                    text,
                    sound,
                    portrait,
                    position,
                    milliseconds,
                } => {
                    self.text = Some(*text);
                    self.portraits[0] = Some(*portrait);
                    self.active_slot = Some(0);
                    self.talk_until_ms = self.elapsed_ms.saturating_add(u64::from(*milliseconds));
                    self.signal = Some(*position);
                    // The legacy SCV line has zero trigger wait followed by an explicit
                    // Wait action. Its audio and subtitle continue independently.
                    self.caption_until_ms = Some(
                        self.elapsed_ms.saturating_add(
                            sound
                                .and_then(|id| media.mission_audio.get(usize::from(id)))
                                .map_or(u64::from(*milliseconds), |clip| clip.duration_ms()),
                        ),
                    );
                    if let Some(sound) = sound {
                        audio.play_mission(*sound, Some(*portrait));
                    }
                }
            }
        }
        self.seen_events = state.events.len();
        camera
    }
    pub fn text<'a>(&self, media: &'a MediaPack) -> Option<&'a str> {
        self.text
            .and_then(|id| media.mission_texts.get(usize::from(id)))
            .map(String::as_str)
    }
    pub fn objectives<'a>(&self, media: &'a MediaPack) -> Option<&'a str> {
        self.objectives
            .and_then(|id| media.mission_texts.get(usize::from(id)))
            .map(String::as_str)
    }
}

pub fn start_button(size: [f64; 2]) -> [f64; 4] {
    [size[0] / 2.0 - 110.0, size[1] - 70.0, 220.0, 40.0]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{App, Config, view::Presentation};
    use std::path::Path;
    use straterust_engine::{
        content::Package,
        sim::{
            EntityId, Mission, MissionAction, MissionComparison, MissionCondition, MissionLocation,
            MissionTrigger, Order, PlayerId,
        },
    };

    fn media() -> MediaPack {
        MediaPack {
            audio: Vec::new(),
            music: Vec::new(),
            mission_audio: Vec::new(),
            mission_texts: vec![
                "Keep the leader alive.".into(),
                "Incoming report.".into(),
                "End of briefing.".into(),
            ],
            portraits: Vec::new(),
            briefing: vec![
                BriefingAction::Objectives { text: 0 },
                BriefingAction::ShowPortrait {
                    slot: 3,
                    portrait: UnitTypeId(1),
                },
                BriefingAction::Wait { milliseconds: 100 },
                BriefingAction::Transmission {
                    slot: 3,
                    text: 1,
                    sound: None,
                    milliseconds: 200,
                },
                BriefingAction::HidePortrait { slot: 3 },
                BriefingAction::Text {
                    text: 2,
                    milliseconds: 400,
                },
            ],
        }
    }
    fn app() -> App {
        let package =
            Package::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/fixtures"))
                .unwrap();
        App::new(
            &package,
            Config {
                audio: false,
                ..Config::default()
            },
            Presentation::default(),
            None,
            None,
        )
        .unwrap()
    }
    #[test]
    fn briefing_text_does_not_block_and_zero_duration_final_caption_persists() {
        let mut media = media();
        media.briefing = vec![
            BriefingAction::Text {
                text: 0,
                milliseconds: 1000,
            },
            BriefingAction::Wait { milliseconds: 100 },
            BriefingAction::Text {
                text: 2,
                milliseconds: 0,
            },
        ];
        let mut ui = MissionUi::new(&media, true);
        let mut audio = Audio::new(false);
        ui.advance(Duration::from_millis(100), &media, &mut audio);
        assert!(ui.briefing_finished);
        assert_eq!(ui.text(&media), Some("End of briefing."));
        ui.advance(Duration::from_secs(60), &media, &mut audio);
        assert_eq!(ui.text(&media), Some("End of briefing."));
    }

    #[test]
    fn original_briefing_sequence_waits_holds_gameplay_and_can_skip_or_start() {
        let media = media();
        let mut app = app();
        let hash = app.world.state_hash();
        app.mission_ui = Some(MissionUi::new(&media, true));
        app.issue(Order::Stop {
            entity: EntityId(1),
        })
        .unwrap();
        assert!(app.recorded.is_empty());
        let ui = app.mission_ui.as_mut().unwrap();
        ui.advance(Duration::ZERO, &media, &mut app.audio);
        assert_eq!(ui.objectives(&media), Some("Keep the leader alive."));
        assert!(ui.text.is_none());
        ui.advance(Duration::from_millis(100), &media, &mut app.audio);
        assert_eq!(ui.text(&media), Some("Incoming report."));
        assert!(ui.talking());
        ui.advance(Duration::from_millis(200), &media, &mut app.audio);
        assert_eq!(ui.text(&media), Some("End of briefing."));
        assert!(!ui.talking());
        ui.advance(Duration::from_secs(1), &media, &mut app.audio);
        assert!(
            ui.briefing && ui.briefing_finished,
            "the user starts the mission after the briefing"
        );
        assert_eq!(app.world.state_hash(), hash);
        ui.start(&mut app.audio);
        app.issue(Order::Stop {
            entity: EntityId(1),
        })
        .unwrap();
        assert_eq!(app.recorded.len(), 1);
        app.media = Some(media);
        app.restart().unwrap();
        assert!(app.mission_ui.as_ref().unwrap().briefing);
        assert_eq!(app.world.state_hash(), hash);
    }
    #[test]
    fn mission_events_show_once_and_cosmetic_clock_does_not_change_state() {
        let media = media();
        let mut app = app();
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/fixtures");
        app.world = Package::load(&fixture).unwrap().world(42).unwrap();
        let mut map = app.world.map().clone();
        map.mission = Some(Mission {
            schema_version: 1,
            player: PlayerId(0),
            rescuable_players: Vec::new(),
            rescuers: Vec::new(),
            alliances: Vec::new(),
            poll_ticks: 31,
            wait_step_ms: 42,
            locations: vec![MissionLocation {
                excluded_elevations: 0,
                left: 10,
                top: 20,
                right: 110,
                bottom: 120,
            }],
            triggers: vec![MissionTrigger {
                conditions: vec![MissionCondition::Countdown {
                    comparison: MissionComparison::AtLeast,
                    milliseconds: 0,
                }],
                actions: vec![
                    MissionAction::Objectives { text: 0 },
                    MissionAction::CenterView { location: 0 },
                    MissionAction::Speech { muted: true },
                    MissionAction::Transmission {
                        text: 1,
                        sound: None,
                        portrait: UnitTypeId(1),
                        location: 0,
                        milliseconds: 84,
                    },
                ],
            }],
        });
        app.world = World::new(app.world.rules().clone(), map, 42).unwrap();
        app.world.step(&[]).unwrap();
        app.world.step(&[]).unwrap();
        let mut ui = MissionUi::new(&media, false);
        let hash = app.world.state_hash();
        assert_eq!(
            ui.observe(&app.world, &media, &mut app.audio),
            Some(Position { x: 60, y: 70 })
        );
        assert_eq!(ui.text(&media), Some("Incoming report."));
        assert_eq!(ui.observe(&app.world, &media, &mut app.audio), None);
        ui.advance(Duration::from_secs(1), &media, &mut app.audio);
        assert!(ui.text.is_none());
        assert_eq!(app.world.state_hash(), hash);
    }
}
