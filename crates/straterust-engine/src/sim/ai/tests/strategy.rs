use super::*;

#[test]
fn unavailable_oil_does_not_prevent_rebalancing_woodcutters_to_gold() {
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
    let mut ship = rules
        .units
        .iter()
        .find(|u| u.id == UnitTypeId(1))
        .unwrap()
        .clone();
    ship.id = UnitTypeId(101);
    ship.cost = [("minerals", 100), ("wood", 100), ("oil", 1000)]
        .into_iter()
        .map(|(kind, amount)| ResourceAmount {
            kind: kind.into(),
            amount,
        })
        .collect();
    rules.units.push(ship);
    let mut hauler = rules
        .units
        .iter()
        .find(|u| u.id == UnitTypeId(2))
        .unwrap()
        .clone();
    hauler.id = UnitTypeId(102);
    hauler.builds.clear();
    hauler.repairs.clear();
    hauler.worker.as_mut().unwrap().resource_kinds = vec!["oil".into()];
    rules.units.push(hauler);
    rules.units.push(UnitType {
        id: UnitTypeId(103),
        speed: 0,
        structure: true,
        max_hp: 200,
        extracts: Some(Extraction {
            resource: "oil".into(),
            harvest_ticks: 10,
            depleted_amount: 0,
        }),
        ..Default::default()
    });
    let mut map = base.map().clone();
    map.ai[0].takeover = true;
    map.resources.extend(
        [("wood", false), ("oil", true)]
            .into_iter()
            .enumerate()
            .map(|(i, (kind, requires_extractor))| ResourceSpawn {
                kind: kind.into(),
                position: Position {
                    x: 1280 + i as i32 * 128,
                    y: 900,
                },
                amount: 10000,
                footprint: Footprint {
                    width: 32,
                    height: 32,
                },
                requires_extractor,
                terrain_corners: None,
            }),
    );
    map.spawns.extend((0..5).map(|n| Spawn {
        owner: PlayerId(1),
        unit_type: UnitTypeId(2),
        position: Position {
            x: 1184 + n * 32,
            y: 720,
        },
        ..Default::default()
    }));
    let mut world = World::new(rules, map, 42).unwrap();
    world.state.players[1].resources.clear();
    let gold = world
        .state
        .resources
        .iter()
        .find(|r| r.kind == "minerals")
        .unwrap()
        .id;
    let wood = world
        .state
        .resources
        .iter()
        .find(|r| r.kind == "wood")
        .unwrap()
        .id;
    for (count, e) in world
        .state
        .entities
        .iter_mut()
        .filter(|e| e.owner == PlayerId(1) && e.unit_type == UnitTypeId(2))
        .enumerate()
    {
        e.order = UnitOrder::Gather {
            resource: if count == 0 { gold } else { wood },
        };
    }
    let controller = world.map.ai[0].clone();
    let mut state = world.state.ai[0].clone();
    let requests = vec![AiRequest {
        unit_type: UnitTypeId(101),
        count: 1,
        priority: 100,
        include_deployed: false,
    }];
    for _ in 0..6 {
        world.ai_balance_gathering(&controller, &mut state, &requests);
    }
    let miners = world
        .state
        .entities
        .iter()
        .filter(|e| {
            e.owner == PlayerId(1)
                && matches!(e.order,
            UnitOrder::Gather {resource} if world.state.resources.iter()
                .any(|r| r.id == resource && r.kind == "minerals"))
        })
        .count();
    assert_eq!(
        miners, 3,
        "unavailable oil monopolized the allocation decision"
    );
    let oil = world
        .state
        .resources
        .iter()
        .find(|r| r.kind == "oil")
        .unwrap()
        .clone();
    world
        .spawn_offspring(PlayerId(1), UnitTypeId(103), oil.position, None)
        .unwrap();
    let tanker = world
        .spawn_offspring(
            PlayerId(1),
            UnitTypeId(102),
            Position { x: 1536, y: 900 },
            None,
        )
        .unwrap();
    world
        .state
        .entities
        .iter_mut()
        .find(|e| e.id == tanker)
        .unwrap()
        .order = UnitOrder::Gather { resource: oil.id };
    for (count, e) in world
        .state
        .entities
        .iter_mut()
        .filter(|e| e.owner == PlayerId(1) && e.unit_type == UnitTypeId(2))
        .enumerate()
    {
        e.order = UnitOrder::Gather {
            resource: if count == 0 { gold } else { wood },
        };
    }
    for _ in 0..6 {
        world.ai_balance_gathering(&controller, &mut state, &requests);
    }
    let miners = world
        .state
        .entities
        .iter()
        .filter(|e| {
            e.owner == PlayerId(1)
                && matches!(e.order,UnitOrder::Gather {resource} if world.state.resources.iter()
            .any(|r|r.id==resource && r.kind=="minerals"))
        })
        .count();
    assert_eq!(
        miners, 3,
        "oil-only haulers distorted the land workers' allocation"
    );
    world.state.players[1]
        .resources
        .insert("minerals".into(), 10000);
    for _ in 0..6 {
        world.ai_balance_gathering(&controller, &mut state, &requests);
    }
    assert_eq!(
        world
            .state
            .entities
            .iter()
            .filter(|e| e.owner == PlayerId(1)
                && e.unit_type == UnitTypeId(2)
                && matches!(e.order, UnitOrder::Gather { resource }
                    if world.state.resources.iter().any(|r|
                        r.id == resource && r.kind == "minerals")))
            .count(),
        1,
        "stockpiled gold must not keep workers from the unfunded wood demand"
    );
}

#[test]
fn secondary_hauler_depot_does_not_block_farms_or_worker_training() {
    let base = economy();
    let mut rules = base.rules().clone();
    rules.starting_resources[0].amount = 500;
    let mut hauler = rules
        .units
        .iter()
        .find(|u| u.id == UnitTypeId(2))
        .unwrap()
        .clone();
    hauler.id = UnitTypeId(102);
    hauler.builds.clear();
    hauler.movement_class = MovementClass::Water;
    let mut dock = rules
        .units
        .iter()
        .find(|u| u.id == UnitTypeId(5))
        .unwrap()
        .clone();
    dock.id = UnitTypeId(101);
    dock.cost[0].amount = 1000;
    dock.prerequisites.clear();
    dock.trains = vec![hauler.id];
    dock.dropoff = vec!["minerals".into()];
    rules
        .units
        .iter_mut()
        .find(|u| u.id == UnitTypeId(2))
        .unwrap()
        .builds
        .push(dock.id);
    rules
        .units
        .iter_mut()
        .find(|u| u.id == UnitTypeId(3))
        .unwrap()
        .supply_provided = 1;
    rules.units.extend([dock, hauler]);
    let mut map = base.map().clone();
    map.resources.clear();
    map.ai[0].takeover = true;
    map.ai[0].program = vec![
        AiInstruction::Request {
            unit_type: UnitTypeId(3),
            count: 1,
            priority: 110,
        },
        AiInstruction::Request {
            unit_type: UnitTypeId(2),
            count: 3,
            priority: 100,
        },
        AiInstruction::Request {
            unit_type: UnitTypeId(101),
            count: 1,
            priority: 90,
        },
        AiInstruction::Wait(1000),
    ];
    let mut world = World::new(rules, map, 42).unwrap();
    for _ in 0..512 {
        world.step(&[]).unwrap();
    }
    assert!(
        world.supply(PlayerId(1)).1 > 1,
        "secondary depot suppressed farms"
    );
    assert_eq!(
        world
            .state
            .entities
            .iter()
            .filter(|e| e.owner == PlayerId(1) && e.unit_type == UnitTypeId(2))
            .count(),
        3,
        "secondary depot suppressed worker training"
    );
}

#[test]
fn ai_reassigned_builders_exit_the_mine_or_depot_before_routing() {
    for depot in [false, true] {
        let mut world = economy();
        world.state.players[1]
            .resources
            .insert("minerals".into(), 500);
        let controller = world.map.ai[0].clone();
        let mut state = world.state.ai[0].clone();
        let worker = world
            .state
            .entities
            .iter()
            .position(|e| e.owner == PlayerId(1) && e.unit_type == UnitTypeId(2))
            .unwrap();
        let resource = world
            .state
            .resources
            .iter()
            .min_by_key(|r| rts::distance(r.position, controller.home))
            .unwrap()
            .clone();
        let hall = world
            .state
            .entities
            .iter()
            .find(|e| e.owner == PlayerId(1) && e.unit_type == UnitTypeId(3))
            .unwrap()
            .clone();
        let (position, footprint) = if depot {
            (
                hall.position,
                world.unit_type(hall.unit_type).unwrap().footprint,
            )
        } else {
            (resource.position, resource.footprint)
        };
        let actor = &mut world.state.entities[worker];
        actor.position = position;
        actor.order = UnitOrder::Gather {
            resource: resource.id,
        };
        actor.gathering_inside = true;
        actor.dropoff_target = depot.then_some(hall.id);
        let entity = actor.id;
        assert!(world.ai_order(
            &controller,
            &mut state,
            Order::Build {
                entity,
                unit_type: UnitTypeId(4),
                position: Position { x: 1440, y: 480 }
            }
        ));
        let actor = &world.state.entities[worker];
        assert!(!actor.gathering_inside);
        assert!(
            !rts::overlaps(
                actor.position,
                world.unit_at(worker).footprint,
                position,
                footprint
            ),
            "builder starts inside its previous work site"
        );
        Arc::make_mut(&mut world.map).ai.clear();
        world.state.ai.clear();
        for _ in 0..256 {
            world.step(&[]).unwrap();
        }
        assert!(
            world.state.entities.iter().any(|e| e.owner == PlayerId(1)
                && e.unit_type == UnitTypeId(4)
                && e.construction.is_none()),
            "reassigned builder failed to finish construction (depot={depot})"
        );
    }
}

#[test]
fn adaptive_ai_dispatches_ready_units_and_counts_the_deployed_army() {
    let base = economy();
    let mut rules = base.rules().clone();
    let mut absent = rules
        .units
        .iter()
        .find(|u| u.id == UnitTypeId(1))
        .unwrap()
        .clone();
    absent.id = UnitTypeId(101);
    rules.units.push(absent);
    let mut map = base.map().clone();
    map.resources.clear();
    map.ai[0].takeover = true;
    map.ai[0].program = vec![
        AiInstruction::Maintain {
            unit_type: UnitTypeId(1),
            count: 2,
            priority: 100,
        },
        AiInstruction::AttackClear,
        AiInstruction::AttackAdd {
            unit_type: UnitTypeId(1),
            count: 2,
        },
        AiInstruction::AttackAdd {
            unit_type: UnitTypeId(101),
            count: 3,
        },
        AiInstruction::AttackPrepare,
        AiInstruction::AttackAvailable,
        AiInstruction::Wait(16),
        AiInstruction::Jump(1),
    ];
    map.spawns.extend((0..2).map(|n| Spawn {
        owner: PlayerId(1),
        unit_type: UnitTypeId(1),
        position: Position {
            x: 1120,
            y: 480 + n * 48,
        },
        ..Default::default()
    }));
    let mut world = World::new(rules, map, 42).unwrap();
    for _ in 0..64 {
        world.step(&[]).unwrap();
    }
    assert_eq!(
        world.state.ai[0].deployed.len(),
        2,
        "missing unit type held back ready attackers"
    );
    let state = world.state.ai[0].clone();
    let controller = world.map.ai[0].clone();
    assert_eq!(world.ai_count(&controller, &state, UnitTypeId(1)), 0);
    assert_eq!(
        world.ai_requested_count(&controller, &state, &state.requests[0]),
        2
    );
    assert!(state.requests[0].include_deployed);
    let restored: AiState = ron::from_str(&ron::to_string(&state).unwrap()).unwrap();
    assert_eq!(restored, state);
    let mut old_request = state.requests[0].clone();
    old_request.include_deployed = false;
    let serialized = ron::to_string(&old_request).unwrap();
    assert!(!serialized.contains("include_deployed"));
    assert!(
        !ron::from_str::<AiRequest>(&serialized)
            .unwrap()
            .include_deployed
    );
}

#[test]
fn adaptive_ai_pays_for_aircraft_and_their_missing_production_chain() {
    let base = economy();
    let mut rules = base.rules().clone();
    rules.starting_resources = vec![ResourceAmount {
        kind: "minerals".into(),
        amount: 2000,
    }];
    let mut flyer = rules
        .units
        .iter()
        .find(|u| u.id == UnitTypeId(1))
        .unwrap()
        .clone();
    flyer.id = UnitTypeId(101);
    flyer.movement_class = MovementClass::Air;
    flyer.cost[0].amount = 150;
    flyer.build_ticks = 8;
    flyer.supply_used = 0;
    flyer.prerequisites.clear();
    let mut lab = rules
        .units
        .iter()
        .find(|u| u.id == UnitTypeId(5))
        .unwrap()
        .clone();
    lab.id = UnitTypeId(102);
    lab.trains.clear();
    lab.cost[0].amount = 80;
    lab.build_ticks = 8;
    lab.prerequisites.clear();
    let mut airfield = lab.clone();
    airfield.id = UnitTypeId(103);
    airfield.prerequisites = vec![lab.id];
    airfield.trains = vec![flyer.id];
    airfield.cost[0].amount = 90;
    rules
        .units
        .iter_mut()
        .find(|u| u.id == UnitTypeId(2))
        .unwrap()
        .builds
        .extend([lab.id, airfield.id]);
    rules.units.extend([flyer, lab, airfield]);
    let mut map = base.map().clone();
    map.resources.clear();
    map.ai[0].takeover = true;
    map.ai[0].program = vec![
        AiInstruction::Maintain {
            unit_type: UnitTypeId(101),
            count: 2,
            priority: 100,
        },
        AiInstruction::AttackClear,
        AiInstruction::AttackAdd {
            unit_type: UnitTypeId(101),
            count: 2,
        },
        AiInstruction::AttackPrepare,
        AiInstruction::AttackAvailable,
        AiInstruction::Wait(16),
        AiInstruction::Jump(1),
    ];
    let mut world = World::new(rules, map, 42).unwrap();
    for _ in 0..512 {
        world.step(&[]).unwrap();
    }
    for unit_type in [101, 102, 103] {
        assert_eq!(
            world
                .state
                .entities
                .iter()
                .filter(|e| e.owner == PlayerId(1) && e.unit_type == UnitTypeId(unit_type))
                .count(),
            if unit_type == 101 { 2 } else { 1 },
            "missing or overproduced type {unit_type}: {:?}",
            world.state.ai
        );
    }
    assert_eq!(world.state.ai[0].deployed.len(), 2);
    assert_eq!(world.resource_balance(PlayerId(1), "minerals"), 1530);
}
