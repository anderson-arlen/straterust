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
        terrain_corners: None,
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
        takeover: false,
        research: Vec::new(),
        abilities: Vec::new(),
        harvest_weights: Vec::new(),
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
fn takeover_controller_yields_commands_to_humans_and_reserves_room_for_workers() {
    let original = economy();
    let mut rules = original.rules().clone();
    rules.starting_resources[0].amount = 10_000;
    let mut map = original.map().clone();
    map.ai[0].takeover = true;
    map.ai[0].active = false;
    let mut world = World::new(rules, map, 42).unwrap();
    let worker = world
        .state
        .entities
        .iter()
        .find(|e| e.owner == PlayerId(1) && world.unit_type(e.unit_type).unwrap().worker.is_some())
        .unwrap()
        .id;
    let command = Command {
        player: PlayerId(1),
        sequence: 1,
        tick: world.tick(),
        order: Order::Stop { entity: worker },
    };
    assert!(
        world.step(std::slice::from_ref(&command)).unwrap()[0]
            .rejection
            .is_none()
    );
    assert_eq!(world.state.ai[0].accepted_orders, 0);
    world.set_ai_control(PlayerId(1), true).unwrap();
    let command = Command {
        sequence: 2,
        tick: world.tick(),
        ..command
    };
    assert_eq!(
        world.step(&[command]).unwrap()[0].rejection,
        Some(Rejection::ComputerControlled)
    );
    world.set_ai_control(PlayerId(1), false).unwrap();
    let farm = world.unit_type(UnitTypeId(4)).unwrap().clone();
    let tight = Position { x: 1424, y: 640 };
    assert!(
        world
            .build_rejection(PlayerId(1), worker, farm.id, tight)
            .is_none()
    );
    assert!(!world.ai_building_space(PlayerId(1), &farm, tight, false));
    let roomy = Position { x: 1456, y: 640 };
    assert!(world.ai_building_space(PlayerId(1), &farm, roomy, false));
    let index = world.index(worker).unwrap();
    world.state.entities[index].order = UnitOrder::PlaceBuilding {
        unit_type: farm.id,
        target: roomy,
    };
    assert!(!world.ai_building_space(PlayerId(1), &farm, Position { x: 1472, y: 640 }, false));
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
mod strategy;
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

#[test]
fn harvest_allocation_balances_resource_kinds_instead_of_counting_every_tree() {
    let base = economy();
    let mut rules = base.rules().clone();
    rules
        .units
        .iter_mut()
        .find(|u| u.id == UnitTypeId(2))
        .unwrap()
        .worker
        .as_mut()
        .unwrap()
        .resource_kinds
        .push("wood".into());
    let mut map = base.map().clone();
    map.ai[0].program = vec![AiInstruction::Wait(1000)];
    map.ai[0].harvest_weights = [("minerals", 5), ("wood", 4)]
        .into_iter()
        .map(|(kind, amount)| ResourceAmount {
            kind: kind.into(),
            amount,
        })
        .collect();
    map.spawns.extend((0..8).map(|n| Spawn {
        owner: PlayerId(1),
        unit_type: UnitTypeId(2),
        position: Position {
            x: 1184 + n % 4 * 28,
            y: 700 + n / 4 * 28,
        },
        ..Spawn::default()
    }));
    map.resources.retain(|r| r.position.x == 1056);
    map.resources.truncate(1);
    map.resources.extend((0..32).map(|n| ResourceSpawn {
        terrain_corners: None,
        kind: "wood".into(),
        position: Position {
            x: 1280 + n % 8 * 32,
            y: 800 + n / 8 * 32,
        },
        amount: 1000,
        footprint: Footprint {
            width: 32,
            height: 32,
        },
        requires_extractor: false,
    }));
    let mut world = World::new(rules, map, 42).unwrap();
    world.step(&[]).unwrap();
    let mut allocations = BTreeMap::new();
    for e in &world.state.entities {
        if e.owner == PlayerId(1)
            && let UnitOrder::Gather { resource } = e.order
        {
            let kind = &world
                .state
                .resources
                .iter()
                .find(|r| r.id == resource)
                .unwrap()
                .kind;
            *allocations.entry(kind.as_str()).or_insert(0) += 1;
        }
    }
    assert_eq!(allocations.get("minerals"), Some(&5));
    assert_eq!(allocations.get("wood"), Some(&4));
}

#[test]
fn worker_only_start_funds_its_town_hall_before_emergency_supply() {
    let base = economy();
    let mut rules = base.rules().clone();
    rules.starting_resources = vec![ResourceAmount {
        kind: "minerals".into(),
        amount: 450,
    }];
    rules
        .units
        .iter_mut()
        .find(|u| u.id == UnitTypeId(4))
        .unwrap()
        .supply_provided = 4;
    let mut map = base.map().clone();
    map.spawns
        .retain(|s| s.owner != PlayerId(1) || s.unit_type == UnitTypeId(2));
    map.ai[0].program.insert(
        0,
        AiInstruction::Request {
            unit_type: UnitTypeId(3),
            count: 1,
            priority: 250,
        },
    );
    let mut world = World::new(rules, map, 42).unwrap();
    let mut hall_complete = false;
    for _ in 0..600 {
        world.step(&[]).unwrap();
        let center = world
            .state
            .entities
            .iter()
            .find(|e| e.owner == PlayerId(1) && e.unit_type == UnitTypeId(3));
        hall_complete |= center.is_some_and(|e| e.construction.is_none());
        if !hall_complete {
            assert!(
                world
                    .state
                    .entities
                    .iter()
                    .filter(|e| e.owner == PlayerId(1))
                    .all(|e| e.unit_type == UnitTypeId(2) || e.unit_type == UnitTypeId(3))
            );
            assert!(
                world
                    .state
                    .entities
                    .iter()
                    .filter(|e| e.owner == PlayerId(1))
                    .all(|e| !matches!(e.order,
                UnitOrder::PlaceBuilding { unit_type, .. } if unit_type != UnitTypeId(3)))
            );
        }
    }
    assert!(hall_complete);
    assert!(
        world
            .state
            .entities
            .iter()
            .filter(|e| e.owner == PlayerId(1) && e.unit_type == UnitTypeId(2))
            .count()
            > 1
    );
    assert!(
        world.state.statistics[1]
            .resources_collected
            .get("minerals")
            .copied()
            .unwrap_or(0)
            > 0
    );
}

#[test]
fn a_single_miner_switches_to_wood_when_supply_blocks_worker_production() {
    let base = economy();
    let mut rules = base.rules().clone();
    rules.starting_resources = vec![ResourceAmount {
        kind: "minerals".into(),
        amount: 450,
    }];
    for unit in &mut rules.units {
        if unit.id == UnitTypeId(2) {
            unit.worker
                .as_mut()
                .unwrap()
                .resource_kinds
                .push("wood".into());
            unit.harvest_profiles = vec![HarvestProfile {
                kind: "minerals".into(),
                capacity: 5,
                inside: true,
                amount: 8,
                ticks: 30,
                entry_range: 1,
                depot_ticks: 8,
                depot_inside: true,
            }];
        }
        if unit.id == UnitTypeId(3) {
            unit.supply_provided = 1;
            unit.dropoff.push("wood".into());
        }
        if unit.id == UnitTypeId(4) {
            unit.supply_provided = 4;
            unit.cost.push(ResourceAmount {
                kind: "wood".into(),
                amount: 16,
            });
        }
    }
    let mut map = base.map().clone();
    map.spawns
        .retain(|s| s.owner != PlayerId(1) || s.unit_type == UnitTypeId(2));
    map.resources.push(ResourceSpawn {
        kind: "wood".into(),
        position: Position { x: 1408, y: 768 },
        amount: 1000,
        footprint: Footprint {
            width: 32,
            height: 32,
        },
        terrain_corners: None,
        requires_extractor: false,
    });
    map.ai[0].program.insert(
        0,
        AiInstruction::Request {
            unit_type: UnitTypeId(3),
            count: 1,
            priority: 250,
        },
    );
    let mut world = World::new(rules, map, 42).unwrap();
    for _ in 0..1800 {
        world.step(&[]).unwrap();
    }
    assert!(
        world.state.statistics[1]
            .resources_collected
            .get("wood")
            .copied()
            .unwrap_or(0)
            >= 16
    );
    assert!(world.state.entities.iter().any(|e| e.owner == PlayerId(1)
        && e.unit_type == UnitTypeId(4)
        && e.construction.is_none()));
    assert!(
        world
            .state
            .entities
            .iter()
            .filter(|e| e.owner == PlayerId(1) && e.unit_type == UnitTypeId(2))
            .count()
            > 1
    );
}
