use super::*;
use crate::{content::Package, session::ServerSession};
use std::path::Path;

fn package(name: &str) -> World {
    Package::load(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../content")
            .join(name),
    )
    .unwrap()
    .world(42)
    .unwrap()
}

#[test]
fn match_statistics_count_completed_production_and_delivered_resources_and_resume() {
    let initial = package("terran-demo");
    let mut rules = initial.rules().clone();
    for unit in &mut rules.units {
        unit.cost.clear();
        unit.prerequisites.clear();
        unit.build_ticks = 1;
        if let Some(worker) = &mut unit.worker {
            worker.harvest_ticks = 1;
            unit.speed = 64;
        }
    }
    let world = World::new(rules, initial.map().clone(), 42).unwrap();
    let baseline = world.state.statistics[0].clone();
    assert_eq!((baseline.units_produced, baseline.structures_built), (1, 1));
    let mut server = ServerSession::new(world, 42, vec![PlayerId(0)]).unwrap();
    let command = |tick, sequence, order| Command {
        tick: Tick(tick),
        player: PlayerId(0),
        sequence,
        order,
    };
    server
        .advance(&[command(
            0,
            1,
            Order::Gather {
                entity: EntityId(2),
                resource: ResourceId(1),
            },
        )])
        .unwrap();
    while server.world().state.statistics[0]
        .resources_collected
        .is_empty()
    {
        assert!(server.world().tick().0 < 100);
        server.advance(&[]).unwrap();
    }
    let gathered = server.world().state.statistics[0].resources_collected["minerals"];
    assert!(gathered >= 8);
    assert_eq!(
        server.world().resource_balance(PlayerId(0), "minerals"),
        50 + gathered
    );
    let checkpoint = server.save_snapshot().unwrap();
    let mut resumed = ServerSession::restore(server.world(), checkpoint).unwrap();
    let tick = server.world().tick().0;
    let commands = [
        command(
            tick,
            2,
            Order::Train {
                entity: EntityId(1),
                unit_type: UnitTypeId(2),
            },
        ),
        command(
            tick,
            3,
            Order::Build {
                entity: EntityId(2),
                unit_type: UnitTypeId(4),
                position: Position { x: 640, y: 512 },
            },
        ),
    ];
    server.advance(&commands).unwrap();
    resumed.advance(&commands).unwrap();
    for _ in 0..100 {
        server.advance(&[]).unwrap();
        resumed.advance(&[]).unwrap();
    }
    let statistics = &server.world().state.statistics[0];
    assert_eq!(
        (statistics.units_produced, statistics.structures_built),
        (2, 2)
    );
    assert_eq!(statistics.resources_collected["minerals"], gathered);
    assert_eq!(
        server.world().state.statistics,
        resumed.world().state.statistics
    );
    assert_eq!(server.world().state_hash(), resumed.world().state_hash());
    assert_eq!(
        server
            .replay()
            .play(server.world())
            .unwrap()
            .state
            .statistics,
        server.world().state.statistics
    );
    let view = server.update(PlayerId(0), &[]).unwrap();
    assert!(view.result.is_none());
    assert!(
        view.view
            .into_world(server.world())
            .unwrap()
            .state
            .statistics
            .is_empty()
    );
    server.restart().unwrap();
    assert_eq!(server.world().state.statistics[0], baseline);
}

#[test]
fn match_statistics_attribute_unit_and_structure_deaths_to_the_right_players() {
    for structure in [false, true] {
        let initial = package("lan-duel");
        let mut rules = initial.rules().clone();
        rules.units[0].weapon.as_mut().unwrap().cooldown = 1;
        let mut map = initial.map().clone();
        map.spawns[0].unit_type = UnitTypeId(if structure { 3 } else { 1 });
        map.spawns[0].hp_percent = Some(1);
        map.spawns[1].hp_percent = None;
        map.spawns[1].position = Position { x: 224, y: 240 };
        let mut world = World::new(rules, map, 42).unwrap();
        for _ in 0..10 {
            world.step(&[]).unwrap();
        }
        assert_eq!(world.state.winner, Some(PlayerId(1)));
        let loser = &world.state.statistics[0];
        let winner = &world.state.statistics[1];
        if structure {
            assert_eq!((loser.structures_lost, winner.structures_razed), (1, 1));
            assert_eq!((loser.units_lost, winner.units_killed), (0, 0));
        } else {
            assert_eq!((loser.units_lost, winner.units_killed), (1, 1));
            assert_eq!((loser.structures_lost, winner.structures_razed), (0, 0));
        }
    }
}
