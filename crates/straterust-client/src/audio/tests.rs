use super::*;
use std::{collections::BTreeSet, path::Path};
use straterust_engine::{
    content::{Package, read_ron},
    scenario::{CommandQueue, Scenario},
};

fn offline() -> (Audio, rodio::mixer::MixerSource) {
    let (input, output) = rodio::mixer::mixer(1, 12000);
    let mut audio = Audio::new(false);
    audio.mixer = Some(Mixer {
        music: Sink::connect_new(&input),
        voice: Sink::connect_new(&input),
        mission: Sink::connect_new(&input),
        effects: Vec::new(),
        input,
        music_gain: 1.0,
        sound_gain: 1.0,
        speech_gain: 1.0,
        _stream: None,
    });
    (audio, output)
}
fn clip(value: i16, count: usize) -> Arc<PcmClip> {
    Arc::new(PcmClip {
        channels: 1,
        sample_rate: 12000,
        samples: vec![value; count].into(),
    })
}

#[test]
fn pcm_source_shares_data_and_real_mixer_overlaps_sources_without_a_device() {
    let pcm = clip(8192, 2000);
    let mut a = PcmSource {
        clip: Arc::clone(&pcm),
        cursor: 0,
    };
    assert_eq!(a.next(), Some(0.25));
    assert_eq!(a.current_span_len(), Some(1999));
    let (input, mut output) = rodio::mixer::mixer(1, 12000);
    input.add(a);
    input.add(PcmSource {
        clip: pcm,
        cursor: 0,
    });
    assert!((output.next().unwrap() - 0.5).abs() < 0.0001);
    assert!(
        Audio::new(true).mixer.is_none(),
        "ordinary tests must not open a device"
    );
}

#[test]
fn three_music_tracks_advance_and_survive_mission_media_reload() {
    let (mut audio, mut output) = offline();
    let mut media = MediaPack {
        mission_audio: vec![],
        mission_texts: vec![],
        briefing: vec![],
        portraits: vec![],
        music: vec![clip(1000, 12000), clip(2000, 12000), clip(3000, 12000)],
        audio: vec![],
    };
    audio.set_media(Some(&media));
    for track in 0..6 {
        let index = track % 3;
        let expected = (index + 1) as f32 * 1000.0 / 32768.0 * 0.28;
        let samples: Vec<_> = output.by_ref().take(4096).collect();
        assert!(
            samples
                .iter()
                .any(|sample| (*sample - expected).abs() < 0.00001)
        );
        assert!(
            samples
                .iter()
                .all(|sample| *sample == 0.0 || (*sample - expected).abs() < 0.00001)
        );
        let position = audio.mixer.as_ref().unwrap().music.get_pos();
        assert!(position > Duration::ZERO);
        // New missions load new PCM allocations with the same soundtrack.
        media.music = vec![clip(1000, 12000), clip(2000, 12000), clip(3000, 12000)];
        audio.set_media(Some(&media));
        assert_eq!(audio.mixer.as_ref().unwrap().music.get_pos(), position);
        assert_eq!(audio.music_index, (index + 1) % 3);
        for _ in 0..20000 {
            if audio.mixer.as_ref().unwrap().music.empty() {
                break;
            }
            output.next();
        }
        assert!(audio.mixer.as_ref().unwrap().music.empty());
        audio.update();
    }
    let world = Package::load(Path::new("../../content/fixtures"))
        .unwrap()
        .world(7)
        .unwrap();
    let position = audio.mixer.as_ref().unwrap().music.get_pos();
    audio.reset(&world);
    assert_eq!(audio.mixer.as_ref().unwrap().music.get_pos(), position);
    assert_eq!(audio.music_index, 1);
    let mut replacement = media;
    replacement.music = vec![clip(4000, 12000)];
    audio.set_media(Some(&replacement));
    assert_eq!(
        audio.mixer.as_ref().unwrap().music.get_pos(),
        Duration::ZERO
    );
    let expected = 4000.0 / 32768.0 * 0.28;
    assert!(
        output
            .by_ref()
            .take(4096)
            .any(|sample| (sample - expected).abs() < 0.00001)
    );
    audio.set_media(None);
    assert!(audio.mixer.as_ref().unwrap().music.empty());
}

#[test]
fn music_effects_and_priority_voice_have_bounded_independent_playback() {
    let (mut audio, mut output) = offline();
    let media = MediaPack {
        mission_audio: Vec::new(),
        mission_texts: Vec::new(),
        briefing: Vec::new(),
        portraits: vec![],
        music: vec![clip(100, 12000)],
        audio: vec![
            AudioClips {
                cue: Cue::Select,
                unit_type: Some(UnitTypeId(1)),
                voice: true,
                variants: vec![clip(200, 12000), clip(300, 12000)],
            },
            AudioClips {
                cue: Cue::Ready,
                unit_type: Some(UnitTypeId(2)),
                voice: true,
                variants: vec![clip(400, 12000)],
            },
            AudioClips {
                cue: Cue::Attack,
                unit_type: None,
                voice: false,
                variants: vec![clip(500, 12000)],
            },
        ],
    };
    audio.set_media(Some(&media));
    audio.event(Cue::Select, Some(UnitTypeId(1)));
    assert!(audio.is_speaking(UnitTypeId(1)));
    audio.event(Cue::Ready, Some(UnitTypeId(2)));
    assert_eq!(
        audio
            .pending_voice
            .map(|(cue, unit_type, _)| (cue, unit_type)),
        Some((Cue::Ready, Some(UnitTypeId(2))))
    );
    for id in 0..40 {
        audio.event(Cue::Attack, Some(UnitTypeId(id)));
    }
    assert_eq!(audio.mixer.as_ref().unwrap().effects.len(), MAX_EFFECTS);
    assert_eq!(audio.mixer.as_ref().unwrap().music.len(), 1);
    assert_eq!(audio.mixer.as_ref().unwrap().voice.len(), 1);
    // Rodio's idle sink starts with a bounded silence span before its first
    // queued source. Consume that startup span without any wall-clock wait.
    assert!(output.by_ref().take(4096).any(|sample| sample != 0.0));
    audio.event(Cue::Select, Some(UnitTypeId(1)));
    assert_eq!(
        audio.variants[&(Cue::Select, Some(UnitTypeId(1)))],
        1,
        "rapid clicks must not queue voice variants"
    );
    for _ in 0..25000 {
        output.next();
    }
    audio.update();
    assert!(
        audio.is_speaking(UnitTypeId(2)),
        "one deferred readiness voice plays after user speech"
    );
    assert_eq!(
        audio.mixer.as_ref().unwrap().music.len(),
        1,
        "music playlist loops separately"
    );
    audio.shutdown();
    assert!(!audio.is_speaking(UnitTypeId(2)));
}

#[test]
fn mission_voice_blocks_acknowledgements_but_preserves_music_and_effects() {
    let (mut audio, mut output) = offline();
    let media = MediaPack {
        mission_audio: vec![clip(300, 12000)],
        mission_texts: vec![],
        briefing: vec![],
        portraits: vec![],
        music: vec![clip(100, 12000)],
        audio: vec![AudioClips {
            cue: Cue::Select,
            unit_type: Some(UnitTypeId(1)),
            voice: true,
            variants: vec![clip(200, 12000)],
        }],
    };
    audio.set_media(Some(&media));
    audio.play_mission(0, Some(UnitTypeId(1000)));
    audio.event(Cue::Select, Some(UnitTypeId(1)));
    audio.play_mission(0, None);
    let mixer = audio.mixer.as_ref().unwrap();
    assert_eq!(mixer.music.len(), 1);
    assert_eq!(mixer.mission.len(), 1);
    assert!(
        mixer.voice.empty(),
        "unit acknowledgement cannot preempt mission speech"
    );
    assert_eq!(
        mixer.effects.len(),
        1,
        "separate PlayWAV remains audible during speech"
    );
    assert!(audio.is_speaking(UnitTypeId(1000)));
    assert!(output.by_ref().take(4096).any(|sample| sample != 0.0));
    audio.stop_mission();
    assert!(!audio.is_speaking(UnitTypeId(1000)));
    assert_eq!(audio.mixer.as_ref().unwrap().music.len(), 1);
    audio.mute_unit_speech(true);
    audio.event(Cue::Select, Some(UnitTypeId(1)));
    assert!(audio.mixer.as_ref().unwrap().voice.empty());
    audio.mute_unit_speech(false);
    audio.event(Cue::Select, Some(UnitTypeId(1)));
    assert!(!audio.mixer.as_ref().unwrap().voice.empty());
}

#[test]
fn bunker_audio_follows_successful_load_and_unload_once() {
    use straterust_engine::sim::*;
    let rules = Rules {
        id: "bunker-audio".into(),
        units: vec![
            UnitType {
                id: UnitTypeId(1),
                speed: 4,
                max_hp: 40,
                ..UnitType::default()
            },
            UnitType {
                id: UnitTypeId(2),
                speed: 0,
                structure: true,
                max_hp: 100,
                footprint: Footprint {
                    width: 32,
                    height: 32,
                },
                garrison: Some(GarrisonStats {
                    capacity: 1,
                    passengers: vec![UnitTypeId(1)],
                    attackers: vec![],
                    range_bonus: 0,
                    unload_ticks: 0,
                }),
                ..UnitType::default()
            },
        ],
        ..Rules::default()
    };
    let map = Map {
        id: "bunker-audio".into(),
        width: 256,
        height: 256,
        players: 1,
        spawns: vec![
            Spawn {
                unit_type: UnitTypeId(1),
                position: Position { x: 111, y: 128 },
                ..Spawn::default()
            },
            Spawn {
                unit_type: UnitTypeId(2),
                position: Position { x: 128, y: 128 },
                ..Spawn::default()
            },
        ],
        resources: vec![],
        start_locations: vec![],
        terrain: None,
        fog_of_war: false,
        initial_explored: Default::default(),
        creation: Default::default(),
        ai: vec![],
        mission: None,
    };
    let mut world = World::new(rules, map, 42).unwrap();
    let mut audio = Audio::new(false);
    audio.reset(&world);
    for (sequence, order, cue) in [
        (
            1,
            Order::Load {
                entity: EntityId(1),
                target: EntityId(2),
            },
            Cue::Load,
        ),
        (
            2,
            Order::Unload {
                entity: EntityId(2),
            },
            Cue::Unload,
        ),
    ] {
        let result = world
            .step(&[Command {
                tick: world.tick(),
                player: PlayerId(0),
                sequence,
                order,
            }])
            .unwrap();
        assert!(result[0].rejection.is_none());
        audio.events.clear();
        audio.observe(&world);
        assert_eq!(audio.events, vec![(cue, Some(UnitTypeId(2)))]);
        audio.observe(&world);
        assert_eq!(audio.events.len(), 1);
    }
    world
        .step(&[Command {
            tick: world.tick(),
            player: PlayerId(0),
            sequence: 3,
            order: Order::Unload {
                entity: EntityId(2),
            },
        }])
        .unwrap();
    audio.events.clear();
    audio.observe(&world);
    assert!(audio.events.is_empty(), "failed unload must remain silent");
}

#[test]
fn building_flight_audio_follows_visible_transitions_and_waits_for_arrival() {
    use straterust_engine::sim::*;
    for fog_of_war in [false, true] {
        let mut map: Map = read_ron(Path::new("../../content/fixtures/map.ron")).unwrap();
        map.fog_of_war = fog_of_war;
        map.spawns = [(0, 1, 64), (1, 1, 1080), (0, 2, 192)]
            .into_iter()
            .map(|(owner, unit_type, x)| Spawn {
                owner: PlayerId(owner),
                unit_type: UnitTypeId(unit_type),
                position: Position { x, y: 64 },
                ..Default::default()
            })
            .collect();
        let rules = Rules {
            id: "building-flight-audio".into(),
            victory: false,
            units: vec![
                UnitType {
                    id: UnitTypeId(1),
                    structure: true,
                    speed: 0,
                    max_hp: 100,
                    vision_range: 64,
                    flight: Some(Flight {
                        speed: 16,
                        lift_ticks: 3,
                        land_ticks: 3,
                    }),
                    ..Default::default()
                },
                UnitType {
                    id: UnitTypeId(2),
                    structure: true,
                    speed: 0,
                    max_hp: 100,
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let mut world = World::new(rules, map, 42).unwrap();
        let mut audio = Audio::new(false);
        audio.reset(&world);
        world
            .step(&[0, 1].map(|owner| Command {
                tick: Tick(0),
                player: PlayerId(owner),
                sequence: 1,
                order: Order::Lift {
                    entity: EntityId(u32::from(owner) + 1),
                },
            }))
            .unwrap();
        audio.observe(&world);
        let lifts = if fog_of_war { 1 } else { 2 };
        assert_eq!(audio.events, vec![(Cue::Lift, Some(UnitTypeId(1))); lifts]);
        audio.observe(&world);
        for _ in 0..3 {
            world.step(&[]).unwrap();
            audio.observe(&world);
        }
        assert_eq!(audio.events.len(), lifts, "no repeated transition cues");
        for (sequence, order) in [
            (
                2,
                Order::Lift {
                    entity: EntityId(1),
                },
            ),
            (
                3,
                Order::Land {
                    entity: EntityId(1),
                    target: Position { x: 192, y: 64 },
                },
            ),
        ] {
            assert!(
                world
                    .step(&[Command {
                        tick: world.tick(),
                        player: PlayerId(0),
                        sequence,
                        order,
                    }])
                    .unwrap()[0]
                    .rejection
                    .is_some()
            );
            audio.observe(&world);
        }
        assert_eq!(audio.events.len(), lifts, "rejected orders stay silent");
        let target = Position { x: 256, y: 64 };
        assert!(
            world
                .step(&[Command {
                    tick: world.tick(),
                    player: PlayerId(0),
                    sequence: 4,
                    order: Order::Land {
                        entity: EntityId(1),
                        target,
                    },
                }])
                .unwrap()[0]
                .rejection
                .is_none()
        );
        audio.observe(&world);
        assert_eq!(audio.events.len(), lifts, "travel is not landing");
        for _ in 0..30 {
            let before = audio.events.len();
            world.step(&[]).unwrap();
            audio.observe(&world);
            if audio.events.len() > before {
                let building = &world.state().entities[0];
                assert_eq!(building.position, target);
                assert!(building.flight_transition > 0);
                assert_eq!(audio.events.last(), Some(&(Cue::Land, Some(UnitTypeId(1)))));
            }
        }
        assert!(!world.state().entities[0].airborne);
        assert_eq!(
            audio.events.len(),
            lifts + 1,
            "one landing cue, no touchdown repeat"
        );
        audio.reset(&world);
        audio.events.clear();
        world.step(&[]).unwrap();
        audio.observe(&world);
        assert!(audio.events.is_empty(), "reset must not replay old sounds");
    }
}

#[test]
#[ignore = "requires private mission 5 assets refreshed with building flight audio"]
fn original_building_flight_audio_has_source_delays_and_plays_through_the_mixer() {
    use straterust_engine::sim::*;
    let directory = std::env::var_os("STRATERUST_ASSET_PACKAGE").unwrap();
    let package = Package::load(Path::new(&directory)).unwrap();
    let source = package.world(42).unwrap();
    let media = MediaPack::load(Path::new(&directory)).unwrap().unwrap();
    for (id, delay) in [(3, 18), (5, 15), (15, 25), (32, 15), (33, 20)] {
        for cue in [Cue::Lift, Cue::Land] {
            let mapping = media
                .audio
                .iter()
                .find(|mapping| mapping.cue == cue && mapping.unit_type == Some(UnitTypeId(id)))
                .expect("original liftable building audio");
            assert!(!mapping.voice);
            let clip = &mapping.variants[0];
            let wait = if cue == Cue::Land { delay * 42 } else { 0 };
            let silence = clip.sample_rate as usize * wait / 1000 * usize::from(clip.channels);
            assert!(clip.samples[..silence].iter().all(|sample| *sample == 0));
            assert!(clip.samples[silence..].iter().any(|sample| *sample != 0));
        }
        let mut rules = source.rules().clone();
        rules.victory = false;
        let mut map = source.map().clone();
        map.mission = None;
        map.ai.clear();
        map.terrain = None;
        map.resources.clear();
        map.fog_of_war = false;
        map.initial_explored.clear();
        map.spawns = vec![Spawn {
            unit_type: UnitTypeId(id),
            position: Position { x: 384, y: 384 },
            ..Default::default()
        }];
        let mut world = World::new(rules, map, 42).unwrap();
        let (mut audio, mut output) = offline();
        audio.clips = media.audio.clone();
        audio.reset(&world);
        for (sequence, order, cue) in [
            (
                1,
                Order::Lift {
                    entity: EntityId(1),
                },
                Cue::Lift,
            ),
            (
                2,
                Order::Land {
                    entity: EntityId(1),
                    target: Position { x: 384, y: 384 },
                },
                Cue::Land,
            ),
        ] {
            assert!(
                world
                    .step(&[Command {
                        tick: world.tick(),
                        player: PlayerId(0),
                        sequence,
                        order,
                    }])
                    .unwrap()[0]
                    .rejection
                    .is_none()
            );
            audio.observe(&world);
            for _ in 0..60 {
                world.step(&[]).unwrap();
                audio.observe(&world);
            }
            assert_eq!(
                audio.events.iter().filter(|event| event.0 == cue).count(),
                1
            );
            // Drain the whole effect before checking the next cue, so an old
            // lift sound cannot make the landing playback assertion pass.
            let audible_samples = output
                .by_ref()
                .take(150000)
                .filter(|sample| *sample != 0.0)
                .count();
            assert!(
                audible_samples > 0,
                "original {cue:?} PCM reaches the effects mixer"
            );
        }
    }
}

#[test]
fn simulation_events_match_work_training_completion_attacks_and_deaths_once() {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/terran-demo");
    let package = Package::load(&directory).unwrap();
    let scenario: Scenario = read_ron(&directory.join("scenario.ron")).unwrap();
    let mut queue = CommandQueue::from_scenario(&scenario).unwrap();
    let mut world = package.world(scenario.seed).unwrap();
    let mut audio = Audio::new(false);
    audio.reset(&world);
    let mut found = BTreeSet::new();
    let mut moving_build_ticks = 0;
    for _ in 0..scenario.ticks {
        let previous = world
            .state()
            .entities
            .iter()
            .find(|entity| entity.unit_type == UnitTypeId(2))
            .map(|entity| entity.position);
        world.step(&queue.take(world.tick())).unwrap();
        let before = world.state_hash();
        audio.events.clear();
        audio.observe(&world);
        assert_eq!(world.state_hash(), before);
        found.extend(audio.events.iter().copied());
        let count = audio.events.len();
        audio.observe(&world);
        assert_eq!(audio.events.len(), count);
        if world.state().entities.iter().any(|entity| {
            entity.unit_type == UnitTypeId(2)
                && matches!(entity.order, UnitOrder::Build { .. })
                && Some(entity.position) != previous
        }) {
            moving_build_ticks += 1;
            assert!(
                !audio.events.contains(&(Cue::Work, Some(UnitTypeId(2)))),
                "no drill sound while builder is walking"
            );
        }
    }
    for cue in [
        (Cue::Work, 2),
        (Cue::Complete, 2),
        (Cue::Ready, 1),
        (Cue::Attack, 1),
        (Cue::Death, 1),
    ] {
        assert!(
            found.contains(&(cue.0, Some(UnitTypeId(cue.1)))),
            "missing actual event {cue:?}"
        );
    }
    assert!(moving_build_ticks > 10);
}

#[test]
fn deferred_voice_expires_instead_of_interrupting_a_later_selection() {
    let (mut audio, _) = offline();
    audio.pending_voice = Some((
        Cue::Ready,
        Some(UnitTypeId(1)),
        Instant::now() - Duration::from_secs(4),
    ));
    audio.update();
    assert!(audio.pending_voice.is_none());
    assert!(audio.mixer.as_ref().unwrap().voice.empty());
    assert!(audio.events.is_empty());
}

#[test]
fn final_tick_sounds_play_once_and_frozen_victory_state_stays_silent() {
    use straterust_engine::sim::*;
    let rules = Rules {
        id: "audio-victory".into(),
        victory: true,
        units: vec![
            UnitType {
                id: UnitTypeId(1),
                max_hp: 10,
                weapon: Some(Weapon {
                    cooldown_jitter: None,
                    targets_air: false,
                    damage_kind: Default::default(),
                    splash: None,
                    strikes: Vec::new(),
                    damage: 20,
                    range: 1000,
                    cooldown: 1,
                }),
                ..UnitType::default()
            },
            UnitType {
                id: UnitTypeId(2),
                worker: Some(WorkerStats {
                    capacity: 8,
                    harvest_amount: 8,
                    harvest_ticks: 30,
                    build_rate: 1,
                    resource_kinds: vec!["ore".into()],
                }),
                ..UnitType::default()
            },
            UnitType {
                id: UnitTypeId(3),
                ..UnitType::default()
            },
        ],
        ..Rules::default()
    };
    let map = Map {
        initial_explored: Default::default(),
        creation: Default::default(),
        ai: Vec::new(),
        mission: None,
        fog_of_war: false,
        id: "audio-victory".into(),
        width: 128,
        height: 128,
        players: 2,
        terrain: None,
        start_locations: vec![],
        spawns: vec![
            Spawn {
                owner: PlayerId(0),
                unit_type: UnitTypeId(1),
                position: Position { x: 20, y: 20 },
                ..Spawn::default()
            },
            Spawn {
                owner: PlayerId(1),
                unit_type: UnitTypeId(3),
                position: Position { x: 80, y: 20 },
                ..Spawn::default()
            },
            Spawn {
                owner: PlayerId(0),
                unit_type: UnitTypeId(2),
                position: Position { x: 20, y: 80 },
                ..Spawn::default()
            },
        ],
        resources: vec![ResourceSpawn {
            requires_extractor: false,
            kind: "ore".into(),
            position: Position { x: 22, y: 80 },
            amount: 100,
            footprint: Footprint::default(),
        }],
    };
    let mut world = World::new(rules, map, 0).unwrap();
    let mut audio = Audio::new(false);
    audio.reset(&world);
    audio.pending_voice = Some((Cue::Ready, Some(UnitTypeId(3)), Instant::now()));
    world
        .step(&[Command {
            tick: Tick(0),
            player: PlayerId(0),
            sequence: 1,
            order: Order::Gather {
                entity: EntityId(3),
                resource: ResourceId(1),
            },
        }])
        .unwrap();
    assert_eq!(world.state().winner, Some(PlayerId(0)));
    audio.observe(&world);
    assert!(
        audio.pending_voice.is_none(),
        "death removes deferred readiness"
    );
    assert!(audio.events.contains(&(Cue::Attack, Some(UnitTypeId(1)))));
    assert!(audio.events.contains(&(Cue::Work, Some(UnitTypeId(2)))));
    for _ in 0..10 {
        audio.events.clear();
        world.step(&[]).unwrap();
        audio.observe(&world);
        assert!(audio.events.is_empty());
    }
}

#[test]
#[ignore = "requires STRATERUST_TEST_AUDIO=1 and a working output device"]
fn optional_mixer_backend_plays_music_voice_and_effect() {
    assert_eq!(std::env::var("STRATERUST_TEST_AUDIO").as_deref(), Ok("1"));
    let mut audio = Audio::new(false);
    audio.mixer = Some(Mixer::open().expect("output device unavailable"));
    if let Some(directory) = std::env::var_os("STRATERUST_ASSET_PACKAGE") {
        let media = MediaPack::load(Path::new(&directory))
            .unwrap()
            .expect("private package lacks media.ron");
        assert!(!media.music.is_empty());
        audio.set_media(Some(&media));
        audio.event(Cue::Select, Some(UnitTypeId(1)));
        audio.event(Cue::Attack, Some(UnitTypeId(1)));
        assert!(
            audio.is_speaking(UnitTypeId(1)),
            "Marine voice was not queued"
        );
        assert_eq!(audio.mixer.as_ref().unwrap().music.len(), 1);
        assert!(!audio.mixer.as_ref().unwrap().effects.is_empty());
        let deadline = Instant::now() + Duration::from_secs(8);
        while audio.mixer.as_ref().is_some_and(|mixer| {
            !mixer.voice.empty() || mixer.effects.iter().any(|sink| !sink.empty())
        }) {
            assert!(
                Instant::now() < deadline,
                "source voice or effect did not finish"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            audio.mixer.as_ref().unwrap().music.get_pos() > Duration::ZERO,
            "source music made no playback progress"
        );
        assert!(
            audio
                .mixer
                .as_ref()
                .unwrap()
                .effects
                .iter()
                .all(Sink::empty),
            "source effect did not finish"
        );
        audio.shutdown();
        return;
    }
    let media = MediaPack {
        mission_audio: Vec::new(),
        mission_texts: Vec::new(),
        briefing: Vec::new(),
        portraits: vec![],
        music: vec![Arc::new(tone(Cue::Select))],
        audio: vec![AudioClips {
            cue: Cue::Order,
            unit_type: Some(UnitTypeId(1)),
            voice: true,
            variants: vec![Arc::new(tone(Cue::Order))],
        }],
    };
    audio.set_media(Some(&media));
    audio.event(Cue::Order, Some(UnitTypeId(1)));
    audio.event(Cue::Complete, None);
    assert!(audio.is_speaking(UnitTypeId(1)));
    assert_eq!(audio.mixer.as_ref().unwrap().music.len(), 1);
    assert_eq!(audio.mixer.as_ref().unwrap().effects.len(), 1);
    let deadline = Instant::now() + Duration::from_secs(3);
    while audio.mixer.as_ref().is_some_and(|mixer| {
        !mixer.music.empty()
            || !mixer.voice.empty()
            || mixer.effects.iter().any(|sink| !sink.empty())
    }) {
        assert!(
            Instant::now() < deadline,
            "audio did not finish within three seconds"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(!audio.is_speaking(UnitTypeId(1)));
    audio.shutdown();
}

mod transports;
