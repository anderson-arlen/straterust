use super::*;
use straterust_engine::{
    multiplayer::{GameType, MatchRules},
    session::{SavedGame, ServerSession},
};

fn definitions() -> (Rules, Map) {
    let (mut rules, mut map) = super::definitions(500, 100);
    for unit in &mut rules.units {
        unit.manual_target_only = true;
    }
    map.players = 3;
    map.spawns[3].owner = PlayerId(2);
    map.multiplayer = Some(MatchRules {
        participants: vec![PlayerId(0), PlayerId(1), PlayerId(2)],
        teams: vec![vec![PlayerId(0), PlayerId(1)], vec![PlayerId(2)]],
        mode: GameType::Teams,
        parameter: 1,
        resources: Default::default(),
        bases: vec![],
        cheats: false,
        ai_difficulty: None,
        flags: vec![],
        defeat_without_structures: false,
    });
    (rules, map)
}

fn damage(world: &mut World, target: u32) {
    assert_eq!(
        send(
            world,
            2,
            Order::Attack {
                entity: EntityId(4),
                target: EntityId(target),
            },
        ),
        None
    );
    assert_eq!(entity(world, target).hp, 87);
}

#[test]
fn allied_repairs_charge_the_worker_preserve_ownership_and_survive_saves() {
    let (rules, mut map) = definitions();
    map.spawns[0].position = point(52, 40);
    map.spawns[4].owner = PlayerId(1);
    map.spawns[5].owner = PlayerId(1);
    map.spawns.push(spawn(2, 3, 250, 180));
    let mut world = World::new(rules, map, 17).unwrap();
    for target in [1, 6, 7] {
        assert_eq!(
            world.repair_rejection(EntityId(1), EntityId(target)),
            Some(Rejection::InvalidTarget),
            "self, unsupported allied types and enemy buildings remain invalid"
        );
    }
    damage(&mut world, 5);
    assert_eq!(send(&mut world, 0, repair(1, 5)), None);
    run(&mut world, 10);
    let view = world
        .player_view(PlayerId(0))
        .unwrap()
        .into_world(&world)
        .unwrap();
    assert_eq!(view.repair_rejection(EntityId(1), EntityId(5)), None);
    let session = ServerSession::new(world.clone(), 17, vec![PlayerId(0)]).unwrap();
    let saved = SavedGame::capture(&session).unwrap().encode().unwrap();
    let mut restored = ServerSession::restore_saved(&world, SavedGame::decode(&saved).unwrap())
        .unwrap()
        .world()
        .clone();
    run(&mut restored, 100);
    assert_eq!(entity(&restored, 5).hp, 100);
    assert_eq!(entity(&restored, 5).owner, PlayerId(1));
    assert_eq!(
        entity(&restored, 1).order,
        UnitOrder::Repair {
            target: EntityId(5)
        }
    );
    assert_eq!(restored.resource_balance(PlayerId(0), "ore"), 495);
    assert_eq!(restored.resource_balance(PlayerId(0), "gas"), 99);
    assert_eq!(restored.resource_balance(PlayerId(1), "ore"), 500);
    assert_eq!(restored.resource_balance(PlayerId(1), "gas"), 100);
}

#[test]
fn background_gathering_maintains_an_allied_target_and_returns_to_work() {
    let (rules, mut map) = definitions();
    map.spawns[1].owner = PlayerId(1);
    map.resources[0].amount = 600;
    let mut world = World::new(rules, map, 17).unwrap();
    assert_eq!(
        send(
            &mut world,
            0,
            Order::Gather {
                entity: EntityId(1),
                resource: ResourceId(1),
            }
        ),
        None
    );
    assert_eq!(
        send(
            &mut world,
            0,
            Order::Queue {
                entity: EntityId(1),
                order: UnitOrder::Repair {
                    target: EntityId(2)
                },
            }
        ),
        None
    );
    damage(&mut world, 2);
    run(&mut world, 200);
    assert_eq!(entity(&world, 2).hp, 100);
    assert_eq!(entity(&world, 2).owner, PlayerId(1));
    assert_eq!(
        entity(&world, 1).order,
        UnitOrder::Gather {
            resource: ResourceId(1)
        }
    );
    assert!(
        entity(&world, 1)
            .queued_orders
            .contains(&UnitOrder::Repair {
                target: EntityId(2)
            })
    );
    assert!(world.state().resources[0].amount < 600);
    assert_eq!(world.resource_balance(PlayerId(1), "ore"), 500);
}

#[test]
fn allied_construction_assistance_keeps_the_primary_builder_and_job_owner() {
    let (mut rules, mut map) = definitions();
    rules.units[2].builder_inside = true;
    rules.units[2].repair_construction = true;
    map.spawns[0].position = point(180, 86);
    map.spawns[2].owner = PlayerId(1);
    map.spawns[2].position = point(192, 100);
    let mut world = World::new(rules, map, 17).unwrap();
    let building = world.state().next_entity_id;
    assert_eq!(
        send(
            &mut world,
            1,
            Order::Build {
                entity: EntityId(3),
                unit_type: UnitTypeId(3),
                position: point(180, 100),
            }
        ),
        None
    );
    for _ in 0..20 {
        if world
            .state()
            .entities
            .iter()
            .any(|e| e.id == EntityId(building))
        {
            break;
        }
        run(&mut world, 1);
    }
    let job = entity(&world, building).construction.clone().unwrap();
    assert_eq!(job.worker, Some(EntityId(3)));
    assert!(!world.entity_visible(PlayerId(0), EntityId(3)));
    assert_eq!(
        world.repair_rejection(EntityId(1), EntityId(3)),
        Some(Rejection::InvalidTarget)
    );
    assert_eq!(send(&mut world, 0, repair(1, building)), None);
    let assisted = entity(&world, building).construction.as_ref().unwrap();
    assert_eq!(assisted.worker, job.worker);
    assert_eq!(assisted.remaining, job.remaining - 2);
    run(&mut world, 110);
    assert!(entity(&world, building).construction.is_none());
    assert_eq!(entity(&world, building).owner, PlayerId(1));
    assert_eq!(entity(&world, 3).owner, PlayerId(1));
    assert!(world.entity_visible(PlayerId(0), EntityId(3)));
    assert_eq!(
        entity(&world, 1).order,
        UnitOrder::Repair {
            target: EntityId(building)
        }
    );
    assert_eq!(world.resource_balance(PlayerId(0), "ore"), 500);
    assert_eq!(world.resource_balance(PlayerId(1), "ore"), 400);
}
