//! Construction sounds start on arrival, when a foundation first exists.
use super::*;
use straterust_engine::sim::*;

#[test]
#[ignore = "requires STRATERUST_WARCRAFT2 pointing to a refreshed retail import"]
fn warcraft2_native_workers_play_construction_and_jobs_done_through_the_mixer() {
    let root = std::path::PathBuf::from(std::env::var_os("STRATERUST_WARCRAFT2").unwrap());
    for (race, name) in [(0, "human"), (1, "orc")] {
        let directory = root.join(name).join("mission01");
        let package = Package::load(&directory).unwrap();
        let media = straterust_engine::media::MediaPack::load(&directory)
            .unwrap()
            .unwrap();
        let initial = package.world(7).unwrap();
        let mut rules = initial.rules().clone();
        rules.victory = false;
        let worker = UnitTypeId(3 + race);
        let farm = UnitTypeId(59 + race);
        rules
            .units
            .iter_mut()
            .find(|u| u.id == farm)
            .unwrap()
            .build_ticks = 20;
        rules.starting_resources = ["gold", "wood", "oil"]
            .into_iter()
            .map(|kind| straterust_engine::sim::ResourceAmount {
                kind: kind.into(),
                amount: 10000,
            })
            .collect();
        let mut map = initial.map().clone();
        map.terrain = None;
        map.fog_of_war = false;
        map.ai.clear();
        map.mission = None;
        map.creation.clear();
        map.spawns = vec![Spawn {
            unit_type: worker,
            position: Position { x: 96, y: 192 },
            ..Default::default()
        }];
        map.resources.clear();
        let mut world = World::new(rules, map, 7).unwrap();
        let (mut audio, mut output) = offline();
        audio.clips = media.audio.clone();
        for (cue, speaker) in [
            (Cue::Transform, farm),
            (Cue::SelectConstruction, farm),
            (Cue::Complete, worker),
        ] {
            let mapping = audio
                .clips
                .iter()
                .find(|c| c.cue == cue && c.unit_type == Some(speaker))
                .unwrap();
            assert!(!mapping.variants[0].samples.is_empty());
            audio.event(cue, Some(speaker));
            assert!(output.by_ref().take(12000).any(|s| s != 0.0));
        }
        // Clear playback/rate limits before the actual construction observer.
        let (mut audio, mut output) = offline();
        audio.clips = media.audio.clone();
        audio.reset(&world);
        world
            .step(&[Command {
                tick: world.tick(),
                player: PlayerId(0),
                sequence: 1,
                order: Order::Build {
                    entity: EntityId(1),
                    unit_type: farm,
                    position: Position { x: 320, y: 192 },
                },
            }])
            .unwrap();
        let mut started = 0;
        let mut completed = 0;
        for _ in 0..300 {
            audio.events.clear();
            audio.observe(&world);
            started += audio
                .events
                .iter()
                .filter(|e| **e == (Cue::Transform, Some(farm)))
                .count();
            if audio.events.contains(&(Cue::Complete, Some(worker))) {
                completed += 1;
                assert!(output.by_ref().take(12000).any(|s| s != 0.0));
            }
            if completed > 0 {
                break;
            }
            world.step(&[]).unwrap();
        }
        assert_eq!(started, 1);
        assert_eq!(
            completed, 1,
            "completion must use the worker's native recording"
        );
        audio.events.clear();
        audio.observe(&world);
        assert!(audio.events.is_empty());
    }
}

#[test]
fn independent_construction_plays_start_and_completion_once_after_arrival() {
    let rules = Rules {
        id: "original-construction-audio".into(),
        victory: false,
        units: vec![
            UnitType {
                id: UnitTypeId(1),
                speed: 8,
                footprint: Footprint {
                    width: 8,
                    height: 8,
                },
                builds: vec![UnitTypeId(2)],
                worker: Some(WorkerStats {
                    capacity: 8,
                    harvest_amount: 8,
                    harvest_ticks: 75,
                    build_rate: 1,
                    resource_kinds: vec!["ore".into()],
                    idle_resource_radius: 256,
                }),
                ..Default::default()
            },
            UnitType {
                id: UnitTypeId(2),
                speed: 0,
                structure: true,
                autonomous_construction: true,
                footprint: Footprint {
                    width: 32,
                    height: 32,
                },
                placement: Footprint {
                    width: 32,
                    height: 32,
                },
                build_ticks: 20,
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    let map = Map {
        id: "original-construction-audio".into(),
        width: 512,
        height: 256,
        players: 2,
        spawns: vec![Spawn {
            unit_type: UnitTypeId(1),
            position: Position { x: 32, y: 64 },
            ..Default::default()
        }],
        fog_of_war: false,
        resources: vec![],
        start_locations: vec![],
        initial_explored: Default::default(),
        creation: Default::default(),
        ai: vec![],
        mission: None,
        terrain: None,
    };
    let mut world = World::new(rules, map, 42).unwrap();
    let (mut audio, mut output) = offline();
    audio.clips = [Cue::Transform, Cue::Complete]
        .map(|cue| AudioClips {
            cue,
            unit_type: Some(UnitTypeId(2)),
            voice: false,
            variants: vec![clip(8192, 12000)],
        })
        .to_vec();
    audio.reset(&world);
    world
        .step(&[Command {
            tick: Tick(0),
            player: PlayerId(0),
            sequence: 1,
            order: Order::Build {
                entity: EntityId(1),
                unit_type: UnitTypeId(2),
                position: Position { x: 240, y: 64 },
            },
        }])
        .unwrap();
    let mut heard_start = false;
    for _ in 0..60 {
        let building = world
            .state()
            .entities
            .iter()
            .find(|e| e.unit_type == UnitTypeId(2));
        audio.events.clear();
        audio.observe(&world);
        let Some(building) = building else {
            assert!(
                !heard_start && audio.events.is_empty(),
                "silent while the worker is walking and no foundation exists"
            );
            world.step(&[]).unwrap();
            continue;
        };
        assert!(
            !world.construction_pending(building),
            "a new foundation only exists after the worker arrives"
        );
        if audio
            .events
            .contains(&(Cue::Transform, Some(UnitTypeId(2))))
        {
            assert!(!heard_start, "construction starts exactly once");
            heard_start = true;
            assert!(
                output.by_ref().take(4096).any(|s| s != 0.0),
                "native mapping reaches the mixer"
            );
        }
        if building.construction.is_none() {
            assert_eq!(
                audio
                    .events
                    .iter()
                    .filter(|e| **e == (Cue::Complete, Some(UnitTypeId(2))))
                    .count(),
                1
            );
            audio.observe(&world);
            assert_eq!(
                audio
                    .events
                    .iter()
                    .filter(|e| **e == (Cue::Complete, Some(UnitTypeId(2))))
                    .count(),
                1
            );
            assert!(heard_start);
            return;
        }
        world.step(&[]).unwrap();
    }
    panic!("construction did not finish");
}
