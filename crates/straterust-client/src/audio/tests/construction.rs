//! Construction sounds start when work starts, rather than at reservation time.
use super::*;
use straterust_engine::sim::*;

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
            .find(|e| e.unit_type == UnitTypeId(2))
            .unwrap();
        let pending = world.construction_pending(building);
        audio.events.clear();
        audio.observe(&world);
        if audio
            .events
            .contains(&(Cue::Transform, Some(UnitTypeId(2))))
        {
            assert!(!pending && !heard_start);
            heard_start = true;
            assert!(
                output.by_ref().take(4096).any(|s| s != 0.0),
                "native mapping reaches the mixer"
            );
        }
        if pending {
            assert!(
                !heard_start && audio.events.is_empty(),
                "silent while the worker is walking"
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
