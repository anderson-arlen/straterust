use super::*;
use straterust_engine::{
    map::{Terrain, WATER},
    session::{SavedGame, ServerSession},
};

fn definitions(auto: bool, inside: bool) -> (Rules, Map) {
    let (mut rules, mut map) = super::definitions();
    for unit in &mut rules.units {
        unit.movement_class = MovementClass::Water;
    }
    rules.units[3].builder_inside = inside;
    rules.units[3].builder_gathers_resource = auto;
    rules.units[3].build_ticks = 20;
    rules.units[3].extracts = Some(Extraction {
        resource: "ore".into(),
        harvest_ticks: 2,
        depleted_amount: 0,
    });
    map.spawns[1].position = point(100, 40);
    map.resources[0].requires_extractor = true;
    map.resources[0].amount = 1000;
    map.terrain = Some(Terrain {
        cell_size: 8,
        columns: 32,
        rows: 32,
        flags: vec![WATER; 1024],
    });
    (rules, map)
}

fn start(world: &mut World) {
    assert_eq!(
        send(
            world,
            Order::Build {
                entity: EntityId(2),
                unit_type: UnitTypeId(4),
                position: point(120, 40),
            }
        ),
        None
    );
    while !world.state().entities.iter().any(|e| e.id == EntityId(4)) {
        assert!(world.tick().0 < 20);
        run(world, 1);
    }
}

fn complete(world: &mut World) {
    for _ in 0..200 {
        if entity(world, 4).construction.is_none() {
            return;
        }
        run(world, 1);
    }
    panic!("extractor construction did not finish");
}

#[test]
fn completed_extractors_optionally_start_their_primary_builder_harvesting() {
    for inside in [false, true] {
        for auto in [false, true] {
            let (rules, map) = definitions(auto, inside);
            let mut world = World::new(rules, map, 42).unwrap();
            start(&mut world);
            complete(&mut world);
            assert_eq!(
                entity(&world, 2).order,
                if auto {
                    UnitOrder::Gather {
                        resource: ResourceId(1),
                    }
                } else {
                    UnitOrder::Idle
                }
            );
            if auto {
                for _ in 0..100 {
                    if world.state().statistics[0]
                        .resources_collected
                        .get("ore")
                        .copied()
                        .unwrap_or(0)
                        > 0
                    {
                        break;
                    }
                    run(&mut world, 1);
                }
                assert!(world.state().statistics[0].resources_collected["ore"] >= 8);
            } else {
                run(&mut world, 10);
                assert!(entity(&world, 2).cargo.is_none());
            }
        }
    }
}

#[test]
fn extractor_completion_preserves_queued_player_orders() {
    let (rules, map) = definitions(true, true);
    let mut world = World::new(rules, map, 42).unwrap();
    start(&mut world);
    let order = UnitOrder::Move {
        target: point(180, 80),
    };
    assert_eq!(
        send(
            &mut world,
            Order::Queue {
                entity: EntityId(2),
                order: order.clone()
            }
        ),
        None
    );
    complete(&mut world);
    assert_eq!(entity(&world, 2).order, order);
    assert!(entity(&world, 2).cargo.is_none());
}

#[test]
fn a_saved_incomplete_extractor_uses_the_updated_completion_option() {
    let (rules, map) = definitions(false, true);
    let mut old = World::new(rules, map, 42).unwrap();
    start(&mut old);
    let save = SavedGame::capture(&ServerSession::new(old.clone(), 42, vec![PlayerId(0)]).unwrap())
        .unwrap();
    let mut rules = old.rules().clone();
    rules.units[3].builder_gathers_resource = true;
    let current = World::new(rules, old.map().clone(), 42).unwrap();
    let restored = ServerSession::restore_saved(&current, save).unwrap();
    let mut world = restored.world().clone();
    assert_eq!(world.tick(), old.tick());
    assert_eq!(entity(&world, 4).construction, entity(&old, 4).construction);
    complete(&mut world);
    assert_eq!(
        entity(&world, 2).order,
        UnitOrder::Gather {
            resource: ResourceId(1)
        }
    );
    // Older definitions omit the new option and keep the previous default.
    let old_ron = ron::ser::to_string(old.rules()).unwrap();
    assert!(!old_ron.contains("builder_gathers_resource"));
    let decoded: Rules = ron::from_str(&old_ron).unwrap();
    assert!(!decoded.units[3].builder_gathers_resource);
}

#[test]
fn repair_assisted_completion_starts_the_primary_builder_harvesting() {
    let (mut rules, mut map) = definitions(true, true);
    rules.repair = Some(RepairRules {
        rate_numerator: 1,
        rate_denominator: 1,
        cost_divisor: 2,
        range: 8,
    });
    rules.units[3].repair_construction = true;
    let mut helper = rules.units[1].clone();
    helper.id = UnitTypeId(6);
    helper.repairs = vec![UnitTypeId(4)];
    helper.worker.as_mut().unwrap().build_rate = 100;
    rules.units.push(helper);
    map.spawns.push(spawn(0, 6, 135, 40));
    let mut world = World::new(rules, map, 42).unwrap();
    assert_eq!(
        send(
            &mut world,
            Order::Build {
                entity: EntityId(2),
                unit_type: UnitTypeId(4),
                position: point(120, 40)
            }
        ),
        None
    );
    while !world.state().entities.iter().any(|e| e.id == EntityId(5)) {
        run(&mut world, 1);
    }
    assert_eq!(
        send(
            &mut world,
            Order::Repair {
                entity: EntityId(4),
                target: EntityId(5)
            }
        ),
        None
    );
    assert!(entity(&world, 5).construction.is_none());
    assert_eq!(
        entity(&world, 2).order,
        UnitOrder::Gather {
            resource: ResourceId(1)
        }
    );
    assert_eq!(entity(&world, 4).order, UnitOrder::Idle);
}

#[test]
fn completed_and_incomplete_naval_extractors_block_both_owners_placement() {
    for owner in [0, 1] {
        let (rules, mut map) = definitions(false, true);
        map.spawns.push(spawn(owner, 2, 137, 40));
        let mut world = World::new(rules, map, 42).unwrap();
        let command = Command {
            tick: world.tick(),
            player: PlayerId(owner),
            sequence: 1,
            order: Order::Build {
                entity: EntityId(4),
                unit_type: UnitTypeId(4),
                position: point(120, 40),
            },
        };
        assert!(world.step(&[command]).unwrap()[0].rejection.is_none());
        for _ in 0..20 {
            if world.state().entities.iter().any(|e| e.id == EntityId(5)) {
                break;
            }
            run(&mut world, 1);
        }
        assert!(entity(&world, 5).construction.is_some());
        for complete in [false, true] {
            if complete {
                run(&mut world, 100);
            }
            let balance = world.resource_balance(PlayerId(0), "ore");
            assert_eq!(
                world.build_rejection(PlayerId(0), EntityId(2), UnitTypeId(4), point(120, 40)),
                Some(Rejection::InvalidPlacement)
            );
            assert_eq!(
                send(
                    &mut world,
                    Order::Build {
                        entity: EntityId(2),
                        unit_type: UnitTypeId(4),
                        position: point(120, 40)
                    }
                ),
                Some(Rejection::InvalidPlacement)
            );
            assert_eq!(world.resource_balance(PlayerId(0), "ore"), balance);
            assert_eq!(
                world
                    .state()
                    .entities
                    .iter()
                    .filter(|e| e.unit_type == UnitTypeId(4))
                    .count(),
                1
            );
        }
    }
}

#[test]
fn travelling_builder_rechecks_oil_field_occupancy_before_spending() {
    let (mut rules, mut map) = definitions(false, true);
    rules.units[1].speed = 2;
    map.spawns[1].position = point(70, 40);
    map.spawns.push(spawn(1, 2, 137, 40));
    let mut world = World::new(rules, map, 42).unwrap();
    let commands: Vec<_> = [(0, 2), (1, 4)]
        .map(|(player, id)| Command {
            tick: world.tick(),
            player: PlayerId(player),
            sequence: 1,
            order: Order::Build {
                entity: EntityId(id),
                unit_type: UnitTypeId(4),
                position: point(120, 40),
            },
        })
        .into();
    assert!(
        world
            .step(&commands)
            .unwrap()
            .iter()
            .all(|outcome| outcome.rejection.is_none())
    );
    run(&mut world, 100);
    assert_eq!(world.resource_balance(PlayerId(0), "ore"), 50);
    assert_eq!(world.resource_balance(PlayerId(1), "ore"), 40);
    assert_eq!(entity(&world, 2).order, UnitOrder::Idle);
    let platforms: Vec<_> = world
        .state()
        .entities
        .iter()
        .filter(|e| e.unit_type == UnitTypeId(4))
        .collect();
    assert_eq!(platforms.len(), 1);
    assert_eq!(platforms[0].owner, PlayerId(1));
}
