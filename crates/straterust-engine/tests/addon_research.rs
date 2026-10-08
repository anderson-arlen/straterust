use straterust_engine::sim::*;

fn issue(world: &mut World, order: Order) -> Option<Rejection> {
    world
        .step(&[Command {
            tick: world.tick(),
            player: PlayerId(0),
            sequence: world.state().last_sequences[0] + 1,
            order,
        }])
        .unwrap()[0]
        .rejection
        .clone()
}

#[test]
fn connected_addon_research_unlocks_cloak_and_increases_the_regeneration_cap() {
    let rules = Rules {
        id: "addon-cloak".into(),
        units: vec![
            UnitType {
                id: UnitTypeId(1),
                speed: 0,
                structure: true,
                placement: Footprint {
                    width: 128,
                    height: 96,
                },
                builds: vec![UnitTypeId(2)],
                flight: Some(Flight {
                    speed: 8,
                    lift_ticks: 1,
                    land_ticks: 1,
                }),
                ..Default::default()
            },
            UnitType {
                id: UnitTypeId(2),
                speed: 0,
                structure: true,
                addon_parent: Some(UnitTypeId(1)),
                placement: Footprint {
                    width: 64,
                    height: 64,
                },
                build_ticks: 1,
                ..Default::default()
            },
            UnitType {
                id: UnitTypeId(3),
                speed: 4,
                movement_class: MovementClass::Air,
                cloak: Some(Cloak {
                    energy_max: 200,
                    activation_cost: 25,
                    regeneration: 8,
                    drain: 10,
                    ..Cloak::default()
                }),
                ..Default::default()
            },
        ],
        research: vec![
            Research {
                id: ResearchId(5),
                facility: UnitTypeId(2),
                previous: None,
                prerequisites: Vec::new(),
                cost: vec![],
                ticks: 2,
                effect: ResearchEffect::Cloak {
                    units: vec![UnitTypeId(3)],
                },
            },
            Research {
                id: ResearchId(6),
                facility: UnitTypeId(2),
                previous: None,
                prerequisites: Vec::new(),
                cost: vec![],
                ticks: 2,
                effect: ResearchEffect::EnergyCapacity {
                    units: vec![UnitTypeId(3)],
                    amount: 50,
                },
            },
        ],
        ..Default::default()
    };
    let map = Map {
        id: "addon-cloak".into(),
        width: 512,
        height: 512,
        players: 1,
        spawns: vec![
            Spawn {
                unit_type: UnitTypeId(1),
                position: Position { x: 128, y: 128 },
                ..Default::default()
            },
            Spawn {
                unit_type: UnitTypeId(3),
                position: Position { x: 128, y: 400 },
                energy_percent: Some(100),
                ..Default::default()
            },
        ],
        resources: vec![],
        start_locations: vec![],
        terrain: None,
        fog_of_war: false,
        initial_explored: Default::default(),
        creation: Default::default(),
        ai: vec![],
        mission: None,
    };
    let mut placed_map = map.clone();
    placed_map.spawns.push(Spawn {
        unit_type: UnitTypeId(2),
        position: Position { x: 224, y: 144 },
        ..Default::default()
    });
    let placed = World::new(rules.clone(), placed_map, 0).unwrap();
    assert_eq!(placed.state().entities[2].parent, Some(EntityId(1)));
    assert_eq!(
        placed.research_rejection(PlayerId(0), EntityId(3), ResearchId(5)),
        None
    );
    let mut world = World::new(rules, map, 0).unwrap();
    assert_eq!(
        world.cloak_rejection(EntityId(2), true),
        Some(Rejection::MissingPrerequisite)
    );
    let position = world.addon_position(EntityId(1)).unwrap();
    assert_eq!(
        issue(
            &mut world,
            Order::Build {
                entity: EntityId(1),
                unit_type: UnitTypeId(2),
                position
            }
        ),
        None
    );
    world.step(&[]).unwrap();
    assert_eq!(world.state().entities[2].parent, Some(EntityId(1)));
    assert_eq!(
        issue(
            &mut world,
            Order::Research {
                entity: EntityId(3),
                research: ResearchId(5)
            }
        ),
        None
    );
    world.step(&[]).unwrap();
    assert_eq!(world.cloak_rejection(EntityId(2), true), None);
    assert_eq!(
        issue(
            &mut world,
            Order::Research {
                entity: EntityId(3),
                research: ResearchId(6)
            }
        ),
        None
    );
    world.step(&[]).unwrap();
    assert_eq!(world.energy_max(&world.state().entities[1]), 250);
    world.step(&[]).unwrap();
    assert!(world.state().entities[1].energy > 200 * 256);
    assert_eq!(
        issue(
            &mut world,
            Order::Cloak {
                entity: EntityId(2),
                enabled: true
            }
        ),
        None
    );
    assert!(world.state().entities[1].cloaked);
    assert_eq!(
        issue(
            &mut world,
            Order::Lift {
                entity: EntityId(1)
            }
        ),
        None
    );
    world.step(&[]).unwrap();
    // A lifted parent detaches the addon and leaves its research unavailable.
    assert_eq!(
        world.research_rejection(PlayerId(0), EntityId(3), ResearchId(5)),
        Some(Rejection::MissingPrerequisite)
    );
}
