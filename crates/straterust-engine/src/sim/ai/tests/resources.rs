use super::*;

mod growth;

fn town(fog: bool, gold: u32) -> World {
    let original = economy();
    let mut rules = original.rules().clone();
    rules.starting_resources[0].amount = 2500;
    let worker = rules
        .units
        .iter_mut()
        .find(|u| u.id == UnitTypeId(2))
        .unwrap();
    worker.worker.as_mut().unwrap().capacity = 20;
    worker.worker.as_mut().unwrap().harvest_amount = 20;
    worker.worker.as_mut().unwrap().build_rate = 4;
    worker.harvest_profiles = vec![HarvestProfile {
        kind: "minerals".into(),
        capacity: 5,
        inside: true,
        amount: 20,
        ticks: 4,
        entry_range: 8,
        depot_ticks: 4,
        depot_inside: true,
    }];
    rules.units[2].builder_inside = true;
    rules.units[2].resource_clearance = 32;
    let mut map = original.map().clone();
    map.width = 2048;
    map.height = 1024;
    map.fog_of_war = fog;
    map.spawns = vec![Spawn {
        owner: PlayerId(1),
        unit_type: UnitTypeId(3),
        position: Position { x: 256, y: 512 },
        ..Default::default()
    }];
    map.spawns.extend([448, 480, 544, 576].map(|y| Spawn {
        owner: PlayerId(1),
        unit_type: UnitTypeId(2),
        position: Position { x: 352, y },
        ..Default::default()
    }));
    map.spawns.push(Spawn {
        owner: PlayerId(0),
        unit_type: UnitTypeId(3),
        position: Position { x: 1792, y: 256 },
        ..Default::default()
    });
    map.resources = [(448, 512, gold), (1216, 512, 20_000)]
        .into_iter()
        .map(|(x, y, amount)| ResourceSpawn {
            kind: "minerals".into(),
            position: Position { x, y },
            amount,
            footprint: Footprint {
                width: 48,
                height: 48,
            },
            requires_extractor: false,
            terrain_corners: None,
        })
        .collect();
    map.ai[0].home = Position { x: 256, y: 512 };
    map.ai[0].radius = 512;
    map.ai[0].takeover = true;
    map.ai[0].program = vec![AiInstruction::Request {
        unit_type: UnitTypeId(2),
        count: 4,
        priority: 130,
    }];
    map.start_locations.clear();
    World::new(rules, map, 42).unwrap()
}

fn bases(world: &World) -> usize {
    world
        .state
        .entities
        .iter()
        .filter(|e| {
            e.owner == PlayerId(1) && e.unit_type == UnitTypeId(3) && e.construction.is_none()
        })
        .count()
}

#[test]
fn town_prepares_a_paid_replacement_before_its_mine_is_empty_then_keeps_mining() {
    let mut world = town(false, 200);
    world.step(&[]).unwrap();
    assert!(world.state.resources[0].amount > 0);
    assert_eq!(
        world.state.ai[0].expansion.as_ref().unwrap().resource,
        Some(ResourceId(2))
    );
    assert!(world.state.entities.iter().any(|e| matches!(e.order,
        UnitOrder::PlaceBuilding { unit_type: UnitTypeId(3), target } if target.x > 700)));
    let starting = world.resource_balance(PlayerId(1), "minerals");
    for _ in 0..800 {
        world.step(&[]).unwrap();
    }
    assert_eq!(world.state.resources[0].amount, 0);
    assert_eq!(bases(&world), 2);
    assert!(world.state.resources[1].amount < 20_000);
    assert!(
        world.state.statistics[1]
            .resources_collected
            .get("minerals")
            .copied()
            .unwrap_or(0)
            > 200
    );
    assert!(world.resource_balance(PlayerId(1), "minerals") < starting + 20_000 - 400);
    assert!(world.state.ai[0].expansion.is_none());
}

#[test]
fn depleted_town_skips_an_unreachable_mine_and_uses_another_known_deposit() {
    use crate::map::{BUILDABLE, Terrain, WALKABLE, WATER};
    let original = town(false, 1);
    let mut map = original.map().clone();
    let mut remote = map.resources[1].clone();
    remote.position = Position { x: 512, y: 256 };
    map.resources.insert(1, remote);
    map.terrain = Some(Terrain {
        cell_size: 32,
        columns: 64,
        rows: 32,
        flags: (0..64 * 32)
            .map(|i| {
                let (x, y) = (i % 64, i / 64);
                if (14..19).contains(&x) && (6..11).contains(&y) && (x, y) != (16, 8) {
                    WATER
                } else {
                    WALKABLE | BUILDABLE
                }
            })
            .collect(),
    });
    let mut world = World::new(original.rules().clone(), map, 42).unwrap();
    world.state.resources[0].amount = 0;
    world.collision.resource_changed(ResourceId(1));
    for _ in 0..1000 {
        world.step(&[]).unwrap();
        assert!(!world.state.entities.iter().any(|e| e.owner == PlayerId(1)
            && e.order
                == (UnitOrder::Gather {
                    resource: ResourceId(2)
                })));
    }
    assert!(world.state.resources[2].amount < 20_000);
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
fn scouts_discover_replacement_mines_and_never_gather_undisclosed_nodes() {
    let mut world = town(true, 200);
    let target = world.state.resources[1].position;
    assert_eq!(
        world.visibility(PlayerId(1), target),
        Visibility::Unexplored
    );
    let mut scouted = false;
    for _ in 0..2400 {
        world.step(&[]).unwrap();
        scouted |= world.state.ai[0]
            .expansion
            .as_ref()
            .is_some_and(|p| p.scout.is_some());
        if world.visibility(PlayerId(1), target) == Visibility::Unexplored {
            assert!(!world.state.entities.iter().any(|e| e.owner == PlayerId(1)
                && e.order
                    == (UnitOrder::Gather {
                        resource: ResourceId(2)
                    })));
        }
        if bases(&world) == 2 {
            break;
        }
    }
    assert!(scouted);
    assert_ne!(
        world.visibility(PlayerId(1), target),
        Visibility::Unexplored
    );
    assert_eq!(bases(&world), 2, "{:?}", world.state.ai[0].expansion);
}

#[test]
fn a_healthy_mine_does_not_trigger_an_unnecessary_expansion() {
    let mut world = town(false, 100_000);
    for _ in 0..256 {
        world.step(&[]).unwrap();
    }
    assert_eq!(bases(&world), 1);
    assert!(world.state.ai[0].expansion.is_none());
}

#[test]
fn an_interior_miner_can_be_reassigned_through_a_free_connected_exit() {
    let mut world = town(false, 100_000);
    world.step(&[]).unwrap();
    let worker = world.index(EntityId(2)).unwrap();
    world.state.entities[worker].position = world.state.resources[0].position;
    world.state.entities[worker].gathering_inside = true;
    world.state.entities[worker].cargo = None;
    world.collision.changed(EntityId(2));
    let controller = world.map.ai[0].clone();
    let mut state = world.state.ai[0].clone();
    assert_eq!(
        world.ai_gather_choice(
            &controller,
            &state,
            &world.state.entities[worker],
            &[ResourceId(2)]
        ),
        Some(ResourceId(2))
    );
    assert!(world.ai_order(
        &controller,
        &mut state,
        Order::Gather {
            entity: EntityId(2),
            resource: ResourceId(2)
        }
    ));
    assert!(!world.state.entities[worker].gathering_inside);
    assert!(world.can_place(
        world.state.entities[worker].position,
        world.unit_at(worker).footprint,
        MovementClass::Ground,
        Some(EntityId(2))
    ));
}

#[test]
fn discovering_a_healthy_local_mine_cancels_the_expansion_reserve() {
    let original = town(true, 100_000);
    let mut map = original.map().clone();
    map.resources[0].position.x = 704;
    let mut world = World::new(original.rules().clone(), map, 42).unwrap();
    let mine = world.state.resources[0].position;
    assert_eq!(world.visibility(PlayerId(1), mine), Visibility::Unexplored);
    world.step(&[]).unwrap();
    assert!(world.state.ai[0].expansion.is_some());
    for _ in 0..1200 {
        world.step(&[]).unwrap();
        if world.visibility(PlayerId(1), mine) != Visibility::Unexplored
            && world.state.ai[0].expansion.is_none()
        {
            break;
        }
    }
    assert_ne!(world.visibility(PlayerId(1), mine), Visibility::Unexplored);
    assert!(world.state.ai[0].expansion.is_none());
    assert_eq!(bases(&world), 1);
}

#[test]
fn expansion_budget_collects_missing_materials_before_military_spending() {
    let original = town(false, 200);
    let mut rules = original.rules().clone();
    rules.starting_resources[0].amount = 500;
    rules.units[2].cost.push(ResourceAmount {
        kind: "wood".into(),
        amount: 200,
    });
    rules.units[2].dropoff.push("wood".into());
    rules.units[1]
        .worker
        .as_mut()
        .unwrap()
        .resource_kinds
        .push("wood".into());
    let mut wood = rules.units[1].harvest_profiles[0].clone();
    wood.kind = "wood".into();
    wood.inside = false;
    rules.units[1].harvest_profiles.push(wood);
    let mut map = original.map().clone();
    map.resources.push(ResourceSpawn {
        kind: "wood".into(),
        position: Position { x: 448, y: 768 },
        amount: 20_000,
        footprint: Footprint {
            width: 48,
            height: 48,
        },
        requires_extractor: false,
        terrain_corners: None,
    });
    map.ai[0].program.push(AiInstruction::Maintain {
        unit_type: UnitTypeId(1),
        count: 20,
        priority: 80,
    });
    let mut world = World::new(rules, map, 42).unwrap();
    for _ in 0..1600 {
        world.step(&[]).unwrap();
        if bases(&world) == 2 {
            break;
        }
    }
    assert_eq!(bases(&world), 2, "{:?}", world.state.ai[0].expansion);
    assert!(
        world.state.statistics[1]
            .resources_collected
            .get("wood")
            .copied()
            .unwrap_or(0)
            >= 200
    );
}

#[test]
fn expansion_intent_survives_snapshots_and_old_ai_state_defaults_to_no_plan() {
    let mut world = town(true, 200);
    world.step(&[]).unwrap();
    assert!(world.state.ai[0].expansion.is_some());
    let encoded = ron::to_string(&world.save_snapshot().unwrap()).unwrap();
    let mut restored = world
        .restore_snapshot(ron::from_str(&encoded).unwrap())
        .unwrap();
    for _ in 0..300 {
        world.step(&[]).unwrap();
        restored.step(&[]).unwrap();
        assert_eq!(world.state_hash(), restored.state_hash());
    }
    let mut old = world.state.ai[0].clone();
    old.expansion = None;
    let old_ron = ron::to_string(&old).unwrap();
    assert!(!old_ron.contains("expansion"));
    assert!(
        ron::from_str::<AiState>(&old_ron)
            .unwrap()
            .expansion
            .is_none()
    );
}
