//! Town production, attacks and replay regressions.
use super::*;
use crate::content::Package;
fn economy() -> World {
    let package = Package::load(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/terran-demo"),
    )
    .unwrap();
    let original = package.world(42).unwrap();
    let mut rules = original.rules().clone();
    rules.victory = false;
    for unit in &mut rules.units {
        unit.build_ticks = 16;
        unit.vision_range = 192;
    }
    let mut map = original.map().clone();
    map.terrain = None;
    map.spawns.retain(|s| s.owner == PlayerId(0));
    map.spawns.push(Spawn {
        owner: PlayerId(1),
        unit_type: UnitTypeId(3),
        position: Position { x: 1280, y: 640 },
        ..Spawn::default()
    });
    map.spawns.push(Spawn {
        owner: PlayerId(1),
        unit_type: UnitTypeId(2),
        position: Position { x: 1184, y: 640 },
        ..Spawn::default()
    });
    map.resources.extend((0..3).map(|n| ResourceSpawn {
        kind: "minerals".into(),
        position: Position {
            x: 1056,
            y: 576 + n * 48,
        },
        amount: 1500,
        footprint: Footprint {
            width: 64,
            height: 32,
        },
        requires_extractor: false,
    }));
    map.ai = vec![AiController {
        player: PlayerId(1),
        home: Position { x: 1280, y: 640 },
        radius: 512,
        active: true,
        program: vec![
            AiInstruction::Request {
                unit_type: UnitTypeId(2),
                count: 3,
                priority: 130,
            },
            AiInstruction::Request {
                unit_type: UnitTypeId(4),
                count: 1,
                priority: 100,
            },
            AiInstruction::Request {
                unit_type: UnitTypeId(5),
                count: 1,
                priority: 80,
            },
            AiInstruction::Wait(32),
            AiInstruction::AttackClear,
            AiInstruction::AttackAdd {
                unit_type: UnitTypeId(1),
                count: 3,
            },
            AiInstruction::AttackPrepare,
            AiInstruction::Attack,
            AiInstruction::Wait(64),
            AiInstruction::Jump(4),
        ],
    }];
    World::new(rules, map, 42).unwrap()
}
#[test]
fn scripted_waves_can_include_unarmed_mobile_support() {
    let base = economy();
    let mut rules = base.rules().clone();
    rules
        .units
        .iter_mut()
        .find(|u| u.id == UnitTypeId(1))
        .unwrap()
        .weapon = None;
    let mut map = base.map().clone();
    let home = map.ai[0].home;
    let spawn_at = Position {
        x: home.x + 160,
        y: home.y,
    };
    map.spawns.push(Spawn {
        owner: PlayerId(1),
        unit_type: UnitTypeId(1),
        position: spawn_at,
        ..Spawn::default()
    });
    map.ai[0].program = vec![
        AiInstruction::AttackAdd {
            unit_type: UnitTypeId(1),
            count: 1,
        },
        AiInstruction::AttackPrepare,
        AiInstruction::Attack,
        AiInstruction::Stop,
    ];
    let mut world = World::new(rules, map, 42).unwrap();
    for _ in 0..32 {
        world.step(&[]).unwrap();
    }
    assert_eq!(world.state.ai[0].deployed.len(), 1);
    let id = *world.state.ai[0].deployed.iter().next().unwrap();
    let escort = &world.state.entities[world.index(id).unwrap()];
    assert!(matches!(escort.order, UnitOrder::AttackMove { .. }));
    assert_ne!(escort.position, spawn_at);
}

#[test]
fn ai_gathers_builds_trains_supplies_and_launches_paid_groups() {
    let mut world = economy();
    for _ in 0..3200 {
        world.step(&[]).unwrap();
    }
    let state = &world.state.ai[0];
    assert!(state.accepted_orders > 12, "{state:?}");
    assert!(!state.deployed.is_empty(), "{state:?}");
    for id in [3, 4, 5] {
        assert!(world.state.entities.iter().any(|e| e.owner == PlayerId(1)
            && e.unit_type == UnitTypeId(id)
            && e.construction.is_none()));
    }
    assert!(
        world
            .state
            .resources
            .iter()
            .filter(|r| r.position.x == 1056)
            .any(|r| r.amount < 1500)
    );
    assert!(world.supply(PlayerId(1)).1 > 10);
    assert!(
        world
            .state
            .entities
            .iter()
            .filter(|e| e.owner == PlayerId(1) && e.unit_type == UnitTypeId(2))
            .count()
            >= 3
    );
}
#[test]
fn ai_replay_and_serialized_mid_attack_state_resume_identically() {
    let mut world = economy();
    let mut replay = economy();
    for _ in 0..800 {
        world.step(&[]).unwrap();
        replay.step(&[]).unwrap();
    }
    assert_eq!(world.state_hash(), replay.state_hash());
    replay.state = ron::from_str(&ron::to_string(&world.state).unwrap()).unwrap();
    for _ in 0..900 {
        world.step(&[]).unwrap();
        replay.step(&[]).unwrap();
    }
    assert_eq!(world.state_hash(), replay.state_hash());
    let hash = world.state_hash();
    replay.state.ai[0].wake.0 += 1;
    assert_ne!(hash, replay.state_hash());
    let mut map = world.map().clone();
    map.ai[0].program[3] = AiInstruction::Wait(33);
    assert_ne!(
        world.map_hash(),
        World::new(world.rules().clone(), map, 42)
            .unwrap()
            .map_hash()
    );
}
#[test]
fn ai_program_validation_and_wait_free_loop_are_bounded() {
    let world = economy();
    let mut map = world.map().clone();
    map.ai[0].program = vec![AiInstruction::Jump(0)];
    let mut looping = World::new(world.rules().clone(), map.clone(), 42).unwrap();
    looping.step(&[]).unwrap();
    assert_eq!(looping.tick(), Tick(1));
    map.ai[0].program = vec![AiInstruction::Jump(1)];
    assert!(World::new(world.rules().clone(), map.clone(), 42).is_err());
    map.ai[0].program = vec![AiInstruction::Wait(0)];
    assert!(World::new(world.rules().clone(), map.clone(), 42).is_err());
    map.ai[0].program = vec![AiInstruction::AttackAdd {
        unit_type: UnitTypeId(2),
        count: 1,
    }];
    assert!(World::new(world.rules().clone(), map, 42).is_err());
}
#[test]
fn player_inputs_cannot_override_computer_and_hidden_targets_do_not_leak() {
    let mut world = economy();
    let result = world
        .step(&[Command {
            player: PlayerId(1),
            tick: Tick(0),
            sequence: 100,
            order: Order::Move {
                entity: EntityId(4),
                target: Position { x: 10, y: 10 },
            },
        }])
        .unwrap();
    assert_eq!(result[0].rejection, Some(Rejection::ComputerControlled));
    let mut map = world.map().clone();
    map.fog_of_war = true;
    let mut hidden = World::new(world.rules().clone(), map, 42).unwrap();
    let controller = hidden.map.ai[0].clone();
    let target = hidden.ai_target(&controller).unwrap();
    hidden.state.entities[0].position.x += 100;
    assert_eq!(hidden.ai_target(&controller), Some(target));
}

#[test]
fn guard_controller_does_not_recall_another_towns_attack_group() {
    let base = economy();
    let mut map = base.map().clone();
    let mut guards = map.ai[0].clone();
    guards.active = false;
    guards.program.clear();
    map.ai.insert(0, guards);
    map.ai[1].program = vec![AiInstruction::Wait(10000)];
    map.spawns.push(Spawn {
        owner: PlayerId(1),
        unit_type: UnitTypeId(1),
        position: Position { x: 976, y: 416 },
        ..Spawn::default()
    });
    let mut world = World::new(base.rules().clone(), map, 42).unwrap();
    let id = world.state.entities.last().unwrap().id;
    world.state.ai[1].deployed.insert(id);
    for _ in 0..16 {
        world.step(&[]).unwrap();
    }
    assert!(!world.state.ai[0].guards.contains_key(&id));
    let entity = world.state.entities.iter().find(|e| e.id == id).unwrap();
    assert!(matches!(entity.order, UnitOrder::AttackMove { .. }));
    assert!(entity.position.x < 976);
}

#[test]
fn consuming_construction_spends_resources_and_finishes_without_a_worker() {
    let base = economy();
    let mut rules = base.rules().clone();
    rules
        .units
        .iter_mut()
        .find(|u| u.id == UnitTypeId(4))
        .unwrap()
        .consumes_builder = true;
    let mut map = base.map().clone();
    map.ai.clear();
    rules.starting_resources = vec![ResourceAmount {
        kind: "minerals".into(),
        amount: 500,
    }];
    let mut world = World::new(rules, map, 42).unwrap();
    let worker = world
        .state
        .entities
        .iter()
        .find(|e| e.owner == PlayerId(0) && e.unit_type == UnitTypeId(2))
        .unwrap()
        .id;
    let before = world.resource_balance(PlayerId(0), "minerals");
    let position = Position { x: 384, y: 384 };
    assert!(
        world
            .step(&[Command {
                tick: Tick(0),
                player: PlayerId(0),
                sequence: 1,
                order: Order::Build {
                    entity: worker,
                    unit_type: UnitTypeId(4),
                    position
                }
            }])
            .unwrap()[0]
            .rejection
            .is_none()
    );
    assert_eq!(world.resource_balance(PlayerId(0), "minerals"), before);
    for _ in 0..512 {
        world.step(&[]).unwrap();
    }
    assert_eq!(
        world.resource_balance(PlayerId(0), "minerals"),
        before - 100
    );
    assert!(
        world
            .state
            .entities
            .iter()
            .any(|e| e.id == worker && e.unit_type == UnitTypeId(4)),
        "consuming construction preserves the builder identity as the building"
    );
    assert!(world.state.entities.iter().any(|e| e.owner == PlayerId(0)
        && e.unit_type == UnitTypeId(4)
        && e.construction.is_none()));
}

mod campaign;
mod defense;
mod transport;

#[test]
fn town_tracks_newborns_outside_its_radius_and_attached_addons() {
    let mut world = economy();
    let controller = world.map.ai[0].clone();
    let producer = world
        .state
        .entities
        .iter()
        .find(|e| e.owner == PlayerId(1) && e.unit_type == UnitTypeId(3))
        .unwrap()
        .id;
    let index = world.index(producer).unwrap();
    let mut newborn = world.state.entities[index].clone();
    newborn.id = EntityId(50);
    newborn.unit_type = UnitTypeId(1);
    newborn.position.x += 1024;
    world.state.entities.push(newborn);
    world.ai_produced(producer, EntityId(50));
    assert_eq!(
        world.ai_count(&controller, &world.state.ai[0], UnitTypeId(1)),
        1
    );
    world.state.entities.last_mut().unwrap().parent = Some(producer);
    world.state.ai[0].members.clear();
    assert_eq!(
        world.ai_count(&controller, &world.state.ai[0], UnitTypeId(1)),
        1
    );
}
