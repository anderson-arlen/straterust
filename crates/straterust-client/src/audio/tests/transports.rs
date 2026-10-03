use super::*;
use straterust_engine::sim::*;

#[test]
#[ignore = "requires private mission 5 media; staggered source unload PCM"]
fn native_dropship_unload_plays_source_sound_for_each_passenger() {
    let directory = std::env::var_os("STRATERUST_ASSET_PACKAGE").unwrap();
    let source = Package::load(Path::new(&directory))
        .unwrap()
        .world(42)
        .unwrap();
    let media = MediaPack::load(Path::new(&directory)).unwrap().unwrap();
    let mut rules = source.rules().clone();
    rules.victory = false;
    assert_eq!(
        rules
            .units
            .iter()
            .find(|u| u.id == UnitTypeId(53))
            .unwrap()
            .garrison
            .as_ref()
            .unwrap()
            .unload_ticks,
        15
    );
    let mut map = source.map().clone();
    map.mission = None;
    map.ai.clear();
    map.terrain = None;
    map.resources.clear();
    map.fog_of_war = false;
    map.initial_explored.clear();
    map.spawns = [(53, 384, 384), (1, 400, 400), (1, 448, 400)]
        .into_iter()
        .map(|(unit, x, y)| Spawn {
            unit_type: UnitTypeId(unit),
            position: Position { x, y },
            ..Default::default()
        })
        .collect();
    let mut world = World::new(rules, map, 42).unwrap();
    let (mut audio, mut output) = offline();
    audio.clips = media.audio.clone();
    audio.reset(&world);
    for (sequence, id) in [2, 3].into_iter().enumerate() {
        assert!(
            world
                .step(&[Command {
                    tick: world.tick(),
                    player: PlayerId(0),
                    sequence: sequence as u64 + 1,
                    order: Order::Load {
                        entity: EntityId(id),
                        target: EntityId(1)
                    }
                }])
                .unwrap()[0]
                .rejection
                .is_none()
        );
    }
    for _ in 0..80 {
        world.step(&[]).unwrap();
        audio.observe(&world);
    }
    assert!(
        world.state().entities[1..]
            .iter()
            .all(|e| e.garrisoned_in == Some(EntityId(1)))
    );
    output.by_ref().take(150000).for_each(drop);
    audio.events.clear();
    world
        .step(&[Command {
            tick: world.tick(),
            player: PlayerId(0),
            sequence: 3,
            order: Order::UnloadAt {
                entity: EntityId(1),
                target: Position { x: 448, y: 448 },
            },
        }])
        .unwrap();
    let mut exits = Vec::new();
    for _ in 0..100 {
        let before = audio.events.iter().filter(|e| e.0 == Cue::Unload).count();
        // Offline ticks run faster than real time. Each source interval represents
        // 630ms of actual playback, beyond the sound event's 120ms debounce.
        audio
            .last_event
            .remove(&(Cue::Unload, Some(UnitTypeId(53))));
        audio.observe(&world);
        if audio.events.iter().filter(|e| e.0 == Cue::Unload).count() > before {
            exits.push(world.tick().0);
            assert!(
                output
                    .by_ref()
                    .take(150000)
                    .filter(|sample| *sample != 0.0)
                    .count()
                    > 0
            );
        }
        world.step(&[]).unwrap();
    }
    assert_eq!(exits.len(), 2);
    assert_eq!(exits[1] - exits[0], 15);
    assert!(
        audio
            .events
            .iter()
            .filter(|e| e.0 == Cue::Unload)
            .all(|e| e.1 == Some(UnitTypeId(53)))
    );
}
