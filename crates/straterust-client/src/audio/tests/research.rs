use super::*;
use straterust_engine::sim::*;

#[test]
fn completion_announces_own_finished_research_once_but_not_cancelled_enemy_or_loaded_jobs() {
    let facility = UnitType {
        id: UnitTypeId(1),
        structure: true,
        speed: 0,
        ..Default::default()
    };
    let facility_id = facility.id;
    let research = |id| Research {
        id: ResearchId(id),
        facility: facility_id,
        previous: None,
        prerequisites: vec![],
        cost: vec![],
        ticks: 3,
        effect: if id == 1 {
            ResearchEffect::Armor {
                units: vec![facility_id],
                amount: 1,
            }
        } else {
            ResearchEffect::VisionRange {
                units: vec![facility_id],
                amount: 32,
            }
        },
    };
    let mut world = World::new(
        Rules {
            id: "research-audio".into(),
            units: vec![facility],
            research: vec![research(1), research(2)],
            ..Default::default()
        },
        Map {
            id: "research-audio".into(),
            players: 2,
            width: 256,
            height: 256,
            spawns: vec![
                Spawn {
                    unit_type: UnitTypeId(1),
                    position: Position { x: 64, y: 64 },
                    ..Default::default()
                },
                Spawn {
                    unit_type: UnitTypeId(1),
                    owner: PlayerId(1),
                    position: Position { x: 192, y: 192 },
                    ..Default::default()
                },
            ],
            terrain: None,
            resources: vec![],
            creation: BTreeMap::new(),
            initial_explored: BTreeMap::new(),
            start_locations: vec![],
            ai: vec![],
            mission: None,
            fog_of_war: false,
        },
        42,
    )
    .unwrap();
    let mut audio = Audio::new(false);
    audio.reset(&world);
    let command = |world: &World, player, sequence, order| Command {
        tick: world.tick(),
        player: PlayerId(player),
        sequence,
        order,
    };
    world
        .step(&[
            command(
                &world,
                0,
                1,
                Order::Research {
                    entity: EntityId(1),
                    research: ResearchId(1),
                },
            ),
            command(
                &world,
                1,
                1,
                Order::Research {
                    entity: EntityId(2),
                    research: ResearchId(2),
                },
            ),
        ])
        .unwrap();
    audio.observe(
        &world
            .player_view(PlayerId(0))
            .unwrap()
            .into_world(&world)
            .unwrap(),
    );
    for _ in 0..4 {
        world.step(&[]).unwrap();
        let view = world
            .player_view(PlayerId(0))
            .unwrap()
            .into_world(&world)
            .unwrap();
        audio.observe(&view);
        audio.observe(&view);
    }
    assert_eq!(
        audio
            .events
            .iter()
            .filter(|e| matches!(e.0, Cue::ResearchComplete(_)))
            .copied()
            .collect::<Vec<_>>(),
        vec![(Cue::ResearchComplete(ResearchId(1)), None)]
    );
    audio.reset(&world);
    audio.events.clear();
    world
        .step(&[command(
            &world,
            0,
            2,
            Order::Research {
                entity: EntityId(1),
                research: ResearchId(2),
            },
        )])
        .unwrap();
    audio.observe(&world);
    world
        .step(&[command(
            &world,
            0,
            3,
            Order::Cancel {
                entity: EntityId(1),
            },
        )])
        .unwrap();
    audio.observe(&world);
    assert!(
        !audio
            .events
            .iter()
            .any(|e| matches!(e.0, Cue::ResearchComplete(_)))
    );
    audio.reset(&world);
    world.step(&[]).unwrap();
    audio.observe(&world);
    assert!(
        !audio
            .events
            .iter()
            .any(|e| matches!(e.0, Cue::ResearchComplete(_)))
    );
}

#[test]
fn research_announcement_waits_for_current_speech_and_reaches_the_mixer() {
    let (mut audio, mut output) = offline();
    audio.clips = vec![AudioClips {
        cue: Cue::ResearchComplete(ResearchId(1)),
        unit_type: None,
        voice: true,
        variants: vec![clip(8192, 12000)],
    }];
    // Even another notification started this frame must not swallow completion.
    audio.mixer.as_mut().unwrap().voice.append(PcmSource {
        clip: clip(1000, 12000),
        cursor: 0,
    });
    audio.voice_priority = 1;
    audio.voice_started = Some(Instant::now());
    audio.event(Cue::ResearchComplete(ResearchId(1)), None);
    assert!(audio.pending_voice.is_some());
    output.by_ref().take(20000).for_each(drop);
    audio.update();
    assert!(audio.pending_voice.is_none());
    assert!(!audio.mixer.as_ref().unwrap().voice.empty());
    assert!(
        output
            .take(4096)
            .any(|sample| (sample - 0.25 * 0.85).abs() < 0.00001)
    );
}
