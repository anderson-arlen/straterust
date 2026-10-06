use straterust_engine::sim::*;

fn world() -> World {
    let units = vec![
        UnitType {
            id: UnitTypeId(1),
            speed: 0,
            structure: true,
            flight: Some(Flight {
                speed: 16,
                lift_ticks: 2,
                land_ticks: 2,
            }),
            builds: vec![UnitTypeId(2)],
            trains: vec![UnitTypeId(3)],
            footprint: Footprint {
                width: 112,
                height: 80,
            },
            placement: Footprint {
                width: 128,
                height: 96,
            },
            ..UnitType::default()
        },
        UnitType {
            id: UnitTypeId(2),
            speed: 0,
            structure: true,
            addon_parent: Some(UnitTypeId(1)),
            build_ticks: 5,
            cost: vec![ResourceAmount {
                kind: "minerals".into(),
                amount: 50,
            }],
            footprint: Footprint {
                width: 71,
                height: 49,
            },
            placement: Footprint {
                width: 64,
                height: 64,
            },
            ..UnitType::default()
        },
        UnitType {
            id: UnitTypeId(3),
            speed: 4,
            build_ticks: 5,
            ..UnitType::default()
        },
    ];
    World::new(
        Rules {
            id: "building-busy".into(),
            units,
            starting_resources: vec![ResourceAmount {
                kind: "minerals".into(),
                amount: 1000,
            }],
            research: vec![Research {
                id: ResearchId(1),
                facility: UnitTypeId(1),
                cost: vec![],
                ticks: 5,
                effect: ResearchEffect::Armor {
                    units: vec![UnitTypeId(3)],
                    amount: 1,
                },
            }],
            ..Rules::default()
        },
        Map {
            id: "building-busy".into(),
            width: 512,
            height: 512,
            players: 1,
            spawns: vec![Spawn {
                unit_type: UnitTypeId(1),
                position: Position { x: 128, y: 128 },
                ..Spawn::default()
            }],
            resources: vec![],
            start_locations: vec![],
            terrain: None,
            fog_of_war: false,
            initial_explored: Default::default(),
            creation: Default::default(),
            ai: vec![],
            mission: None,
        },
        42,
    )
    .unwrap()
}
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
fn addon_order(world: &World) -> Order {
    Order::Build {
        entity: EntityId(1),
        unit_type: UnitTypeId(2),
        position: world.addon_position(EntityId(1)).unwrap(),
    }
}
#[test]
fn addons_exclude_training_and_research_until_complete_or_cancelled() {
    for cancel in [false, true] {
        let mut world = world();
        let order = addon_order(&world);
        assert_eq!(
            world.addon_position(EntityId(1)),
            Some(Position { x: 224, y: 144 })
        );
        assert_eq!(issue(&mut world, order), None);
        assert!(world.constructing_addon(EntityId(1)));
        assert_eq!(
            issue(
                &mut world,
                Order::Train {
                    entity: EntityId(1),
                    unit_type: UnitTypeId(3)
                }
            ),
            Some(Rejection::QueueFull)
        );
        assert_eq!(
            world.research_rejection(PlayerId(0), EntityId(1), ResearchId(1)),
            Some(Rejection::QueueFull)
        );
        if cancel {
            assert_eq!(
                issue(
                    &mut world,
                    Order::Cancel {
                        entity: EntityId(1)
                    }
                ),
                None
            );
            assert_eq!(world.resource_balance(PlayerId(0), "minerals"), 1000);
        } else {
            for _ in 0..5 {
                world.step(&[]).unwrap();
            }
        }
        assert!(!world.constructing_addon(EntityId(1)));
        assert_eq!(
            issue(
                &mut world,
                Order::Train {
                    entity: EntityId(1),
                    unit_type: UnitTypeId(3)
                }
            ),
            None
        );
    }
}
#[test]
fn queued_training_or_research_prevents_starting_an_addon_without_charging() {
    for research in [false, true] {
        let mut world = world();
        let order = if research {
            Order::Research {
                entity: EntityId(1),
                research: ResearchId(1),
            }
        } else {
            Order::Train {
                entity: EntityId(1),
                unit_type: UnitTypeId(3),
            }
        };
        assert_eq!(issue(&mut world, order), None);
        let addon = addon_order(&world);
        assert_eq!(issue(&mut world, addon), Some(Rejection::QueueFull));
        assert_eq!(world.resource_balance(PlayerId(0), "minerals"), 1000);
        assert!(!world.constructing_addon(EntityId(1)));
    }
}

#[test]
fn combined_addon_placement_relocates_and_busy_orders_do_not_charge_early() {
    let mut world = world();
    let target = Position { x: 384, y: 304 };
    assert_eq!(
        issue(
            &mut world,
            Order::Build {
                entity: EntityId(1),
                unit_type: UnitTypeId(2),
                position: target,
            }
        ),
        None
    );
    assert!(world.addon_pending(EntityId(1)));
    assert_eq!(world.resource_balance(PlayerId(0), "minerals"), 1000);
    assert_eq!(
        issue(
            &mut world,
            Order::Train {
                entity: EntityId(1),
                unit_type: UnitTypeId(3),
            }
        ),
        Some(Rejection::UnsupportedOrder)
    ); // Still airborne.
    for _ in 0..40 {
        world.step(&[]).unwrap();
    }
    let parent = &world.state().entities[0];
    assert!(!parent.airborne);
    assert_eq!(parent.position, Position { x: 288, y: 288 });
    let addon = world
        .state()
        .entities
        .iter()
        .find(|u| u.parent == Some(parent.id))
        .unwrap();
    assert_eq!(addon.position, target);
    assert!(addon.construction.is_none());
    assert_eq!(world.resource_balance(PlayerId(0), "minerals"), 950);
}

#[test]
fn training_requires_own_attached_addon_and_landing_reconnects_it() {
    let old = world();
    let mut rules = old.rules().clone();
    rules.units[2].prerequisites = vec![UnitTypeId(2)];
    let mut map = old.map().clone();
    map.spawns.push(Spawn {
        unit_type: UnitTypeId(1),
        position: Position { x: 128, y: 384 },
        ..Default::default()
    });
    let mut world = World::new(rules, map, 42).unwrap();
    let addon = addon_order(&world);
    assert_eq!(issue(&mut world, addon), None);
    for _ in 0..5 {
        world.step(&[]).unwrap();
    }
    assert_eq!(
        issue(
            &mut world,
            Order::Train {
                entity: EntityId(2),
                unit_type: UnitTypeId(3),
            }
        ),
        Some(Rejection::MissingPrerequisite)
    );
    assert_eq!(
        issue(
            &mut world,
            Order::Lift {
                entity: EntityId(1)
            }
        ),
        None
    );
    for _ in 0..2 {
        world.step(&[]).unwrap();
    }
    let target = Position { x: 128, y: 128 };
    assert_eq!(world.land_rejection(EntityId(1), target), None);
    assert_eq!(
        issue(
            &mut world,
            Order::Land {
                entity: EntityId(1),
                target
            }
        ),
        None
    );
    for _ in 0..4 {
        world.step(&[]).unwrap();
    }
    assert_eq!(world.state().entities[2].parent, Some(EntityId(1)));
    assert_eq!(
        issue(
            &mut world,
            Order::Train {
                entity: EntityId(1),
                unit_type: UnitTypeId(3)
            }
        ),
        None
    );
}

#[test]
fn builder_does_not_block_its_foundation_and_can_continue_construction() {
    let old = world();
    let mut rules = old.rules().clone();
    rules.units[2].builds = vec![UnitTypeId(1)];
    rules.units[2].worker = Some(WorkerStats {
        capacity: 8,
        harvest_amount: 8,
        harvest_ticks: 8,
        build_rate: 1,
        resource_kinds: vec!["minerals".into()],
        idle_resource_radius: 256,
    });
    rules.units[0].build_ticks = 5;
    let mut map = old.map().clone();
    map.spawns = vec![Spawn {
        unit_type: UnitTypeId(3),
        position: Position { x: 128, y: 128 },
        ..Default::default()
    }];
    let mut world = World::new(rules, map, 42).unwrap();
    let position = Position { x: 128, y: 128 };
    assert_eq!(
        world.build_rejection(PlayerId(0), EntityId(1), UnitTypeId(1), position),
        None
    );
    assert_eq!(
        issue(
            &mut world,
            Order::Build {
                entity: EntityId(1),
                unit_type: UnitTypeId(1),
                position
            }
        ),
        None
    );
    assert_ne!(world.state().entities[0].position, position);
    for _ in 0..10 {
        world.step(&[]).unwrap();
    }
    assert!(world.state().entities[1].construction.is_none());
}
