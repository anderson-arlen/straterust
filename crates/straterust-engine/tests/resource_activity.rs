//! Resource activity is visible artwork, not disclosure of interior workers.
use straterust_engine::{content::Package, sim::*};

#[test]
fn visible_extractors_show_activity_in_both_player_views_without_disclosing_interior_workers() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/terran-demo");
    let original = Package::load(&path).unwrap().world(7).unwrap();
    let mut rules = original.rules().clone();
    rules.victory = false;
    let refinery = rules
        .units
        .iter_mut()
        .find(|u| !u.dropoff.is_empty())
        .unwrap();
    let refinery_type = refinery.id;
    let kind = refinery.dropoff[0].clone();
    refinery.extracts = Some(Extraction {
        resource: kind.clone(),
        harvest_ticks: 1000,
        depleted_amount: 0,
    });
    let worker = rules
        .units
        .iter_mut()
        .find(|u| {
            u.worker
                .as_ref()
                .is_some_and(|w| w.resource_kinds.contains(&kind))
        })
        .unwrap();
    let worker_type = worker.id;
    worker.speed = 16;
    let mut map = original.map().clone();
    map.terrain = None;
    map.ai.clear();
    map.mission = None;
    map.fog_of_war = false;
    let position = Position { x: 320, y: 160 };
    map.spawns = vec![
        Spawn {
            owner: PlayerId(1),
            unit_type: refinery_type,
            position,
            ..Default::default()
        },
        Spawn {
            owner: PlayerId(1),
            unit_type: worker_type,
            position: Position { x: 220, y: 160 },
            ..Default::default()
        },
    ];
    map.resources = vec![ResourceSpawn {
        kind,
        position,
        amount: 1000,
        footprint: Footprint {
            width: 64,
            height: 64,
        },
        requires_extractor: true,
        terrain_corners: None,
    }];
    let mut w = World::new(rules, map, 7).unwrap();
    let resource = w.state().resources[0].id;
    assert!(!w.entity_working(EntityId(1)));
    assert!(
        w.step(&[Command {
            tick: w.tick(),
            player: PlayerId(1),
            sequence: 1,
            order: Order::Gather {
                entity: EntityId(2),
                resource
            }
        }])
        .unwrap()[0]
            .rejection
            .is_none()
    );
    for _ in 0..30 {
        if w.resource_working(resource) {
            break;
        }
        w.step(&[]).unwrap();
    }
    assert!(w.entity_working(EntityId(1)));
    for player in [PlayerId(0), PlayerId(1)] {
        let packet = w.player_view(player).unwrap();
        assert!(packet.entities.iter().all(|e| match e {
            ViewedEntity::Owned(_) => true,
            ViewedEntity::Visible(e) => e.id != EntityId(2),
        }));
        let client = packet.into_world(&w).unwrap();
        assert!(
            client.entity_working(EntityId(1)),
            "activity must survive the owned and enemy view paths"
        );
    }
    w.step(&[Command {
        tick: w.tick(),
        player: PlayerId(1),
        sequence: 2,
        order: Order::Stop {
            entity: EntityId(2),
        },
    }])
    .unwrap();
    assert!(!w.entity_working(EntityId(1)));
}

#[test]
fn interior_mine_handoffs_release_outdoor_reservations_and_continue_after_load() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/terran-demo");
    let original = Package::load(&path).unwrap().world(7).unwrap();
    let mut rules = original.rules().clone();
    rules.victory = false;
    let worker = rules.units.iter_mut().find(|u| u.worker.is_some()).unwrap();
    let worker_type = worker.id;
    worker.speed = 2;
    worker.motion = None;
    worker.phases_while_gathering = false;
    worker.worker.as_mut().unwrap().capacity = 8;
    worker.harvest_profiles = vec![HarvestProfile {
        kind: "minerals".into(),
        capacity: 5,
        inside: true,
        amount: 8,
        ticks: 12,
        entry_range: 1,
        depot_ticks: 6,
        depot_inside: true,
    }];
    let depot_type = rules
        .units
        .iter()
        .find(|u| u.dropoff.contains(&"minerals".into()))
        .unwrap()
        .id;
    let mut map = original.map().clone();
    map.terrain = None;
    map.ai.clear();
    map.mission = None;
    map.fog_of_war = false;
    map.spawns = vec![
        Spawn {
            unit_type: worker_type,
            position: Position { x: 255, y: 160 },
            ..Default::default()
        },
        Spawn {
            unit_type: worker_type,
            position: Position { x: 180, y: 160 },
            ..Default::default()
        },
        Spawn {
            unit_type: depot_type,
            position: Position { x: 128, y: 320 },
            ..Default::default()
        },
    ];
    map.resources = vec![ResourceSpawn {
        terrain_corners: None,
        kind: "minerals".into(),
        position: Position { x: 320, y: 160 },
        footprint: Footprint {
            width: 96,
            height: 96,
        },
        amount: 10000,
        requires_extractor: false,
    }];
    let mut world = World::new(rules, map, 7).unwrap();
    let resource = world.state().resources[0].id;
    let before = world.resource_balance(PlayerId(0), "minerals");
    for id in 1..=2 {
        let outcomes = world
            .step(&[Command {
                tick: world.tick(),
                player: PlayerId(0),
                sequence: id,
                order: Order::Gather {
                    entity: EntityId(id as u32),
                    resource,
                },
            }])
            .unwrap();
        assert!(outcomes.iter().all(|o| o.rejection.is_none()));
        if id == 1 {
            for _ in 0..20 {
                if world.state().entities[0].gathering_inside {
                    break;
                }
                world.step(&[]).unwrap();
            }
            assert!(world.state().entities[0].gathering_inside);
        }
    }
    for _ in 0..20 {
        if world.state().entities[0].cargo.is_some() {
            break;
        }
        world.step(&[]).unwrap();
    }
    let outgoing = &world.state().entities[0];
    let incoming = &world.state().entities[1];
    assert!(outgoing.cargo.is_some());
    assert!(
        incoming.harvest_spot.is_none(),
        "interior entrants must not reserve outdoor spots"
    );
    world = world
        .restore_snapshot(world.save_snapshot().unwrap())
        .unwrap();
    for _ in 0..800 {
        world.step(&[]).unwrap();
    }
    assert!(
        world.resource_balance(PlayerId(0), "minerals") >= before + 6 * 8,
        "entrance handoff must keep delivering: {:?}",
        world.state().entities
    );
}

#[test]
fn exhausted_mine_releases_every_interior_worker_without_stacking_and_keeps_last_load() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/terran-demo");
    let original = Package::load(&path).unwrap().world(7).unwrap();
    let mut rules = original.rules().clone();
    rules.victory = false;
    let worker = rules.units.iter_mut().find(|u| u.worker.is_some()).unwrap();
    let worker_type = worker.id;
    worker.speed = 16;
    worker.phases_while_gathering = false;
    worker.worker.as_mut().unwrap().capacity = 8;
    worker.harvest_profiles = vec![HarvestProfile {
        kind: "minerals".into(),
        capacity: 5,
        inside: true,
        amount: 8,
        ticks: 40,
        entry_range: 1,
        depot_ticks: 6,
        depot_inside: true,
    }];
    let depot_type = rules
        .units
        .iter()
        .find(|u| u.dropoff.contains(&"minerals".into()))
        .unwrap()
        .id;
    let mut map = original.map().clone();
    map.terrain = None;
    map.ai.clear();
    map.mission = None;
    map.fog_of_war = false;
    map.spawns = (0..5)
        .map(|i| Spawn {
            unit_type: worker_type,
            position: Position {
                x: 240,
                y: 120 + i * 24,
            },
            ..Default::default()
        })
        .chain([Spawn {
            unit_type: depot_type,
            position: Position { x: 128, y: 256 },
            ..Default::default()
        }])
        .collect();
    map.resources = vec![ResourceSpawn {
        terrain_corners: None,
        kind: "minerals".into(),
        position: Position { x: 320, y: 160 },
        footprint: Footprint {
            width: 96,
            height: 96,
        },
        amount: 8,
        requires_extractor: false,
    }];
    let mut world = World::new(rules, map, 7).unwrap();
    let resource = world.state().resources[0].id;
    let before = world.resource_balance(PlayerId(0), "minerals");
    let commands: Vec<_> = (1..=5)
        .map(|id| Command {
            tick: world.tick(),
            player: PlayerId(0),
            sequence: u64::from(id),
            order: Order::Gather {
                entity: EntityId(id),
                resource,
            },
        })
        .collect();
    assert!(
        world
            .step(&commands)
            .unwrap()
            .iter()
            .all(|o| o.rejection.is_none())
    );
    for _ in 0..30 {
        if world
            .state()
            .entities
            .iter()
            .filter(|e| e.gathering_inside)
            .count()
            == 5
        {
            break;
        }
        world.step(&[]).unwrap();
    }
    assert_eq!(
        world
            .state()
            .entities
            .iter()
            .filter(|e| e.gathering_inside)
            .count(),
        5
    );
    // An existing save inside a mine must also emerge safely under new rules.
    world = world
        .restore_snapshot(world.save_snapshot().unwrap())
        .unwrap();
    for _ in 0..250 {
        world.step(&[]).unwrap();
    }
    assert_eq!(world.state().resources[0].amount, 0);
    assert_eq!(world.resource_balance(PlayerId(0), "minerals"), before + 8);
    assert!(world.state().entities.iter().all(|e| !e.gathering_inside));
    for id in 1..=5 {
        let actor = world
            .state()
            .entities
            .iter()
            .find(|e| e.id == EntityId(id))
            .unwrap();
        let footprint = world.unit_type(actor.unit_type).unwrap().footprint;
        assert!(world.can_place(
            actor.position,
            footprint,
            MovementClass::Ground,
            Some(actor.id)
        ));
    }
    let commands: Vec<_> = (1..=5)
        .map(|id| Command {
            tick: world.tick(),
            player: PlayerId(0),
            sequence: u64::from(5 + id),
            order: Order::Move {
                entity: EntityId(id),
                target: Position {
                    x: 600,
                    y: 100 + id as i32 * 32,
                },
            },
        })
        .collect();
    assert!(
        world
            .step(&commands)
            .unwrap()
            .iter()
            .all(|o| o.rejection.is_none())
    );
    for _ in 0..100 {
        world.step(&[]).unwrap();
    }
    for id in 1..=5 {
        assert_eq!(
            world
                .state()
                .entities
                .iter()
                .find(|e| e.id == EntityId(id))
                .unwrap()
                .position,
            Position {
                x: 600,
                y: 100 + id as i32 * 32
            }
        );
    }
}

#[test]
fn visible_mine_discloses_only_activity_and_hidden_mine_does_not() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/terran-demo");
    let original = Package::load(&path).unwrap().world(7).unwrap();
    let mut rules = original.rules().clone();
    rules.victory = false;
    rules.units[0].speed = 32;
    rules.units[0].vision_range = 128;
    rules.units[0].weapon = None;
    let worker = rules.units.iter_mut().find(|u| u.worker.is_some()).unwrap();
    let worker_type = worker.id;
    worker.harvest_profiles = vec![HarvestProfile {
        kind: "minerals".into(),
        capacity: 5,
        inside: true,
        amount: 8,
        ticks: 1000,
        entry_range: 1,
        depot_ticks: 0,
        depot_inside: false,
    }];
    let mut map = original.map().clone();
    map.terrain = None;
    map.ai.clear();
    map.mission = None;
    map.fog_of_war = true;
    map.spawns = vec![
        Spawn {
            owner: PlayerId(0),
            unit_type: UnitTypeId(1),
            position: Position { x: 128, y: 128 },
            ..Default::default()
        },
        Spawn {
            owner: PlayerId(1),
            unit_type: worker_type,
            position: Position { x: 168, y: 128 },
            ..Default::default()
        },
    ];
    map.resources = vec![ResourceSpawn {
        terrain_corners: None,
        kind: "minerals".into(),
        position: Position { x: 200, y: 128 },
        footprint: Footprint {
            width: 32,
            height: 32,
        },
        amount: 100,
        requires_extractor: false,
    }];
    let mut server = World::new(rules, map, 7).unwrap();
    let resource = server.state().resources[0].id;
    let outcome = server
        .step(&[Command {
            tick: server.tick(),
            player: PlayerId(1),
            sequence: 1,
            order: Order::Gather {
                entity: EntityId(2),
                resource,
            },
        }])
        .unwrap();
    assert!(outcome[0].rejection.is_none());
    for _ in 0..20 {
        if server.resource_working(resource) {
            break;
        }
        server.step(&[]).unwrap();
    }
    assert!(server.resource_working(resource));
    let packet = server.player_view(PlayerId(0)).unwrap();
    assert!(packet.active_resources.contains(&resource));
    assert!(packet.entities.iter().all(|e| match e {
        ViewedEntity::Owned(e) => e.id != EntityId(2),
        ViewedEntity::Visible(e) => e.id != EntityId(2),
    }));
    let client = packet.into_world(&server).unwrap();
    assert!(client.resource_working(resource));
    server
        .step(&[Command {
            tick: server.tick(),
            player: PlayerId(0),
            sequence: 1,
            order: Order::Move {
                entity: EntityId(1),
                target: Position { x: 800, y: 500 },
            },
        }])
        .unwrap();
    for _ in 0..30 {
        server.step(&[]).unwrap();
    }
    assert!(server.resource_working(resource));
    let packet = server.player_view(PlayerId(0)).unwrap();
    assert!(packet.active_resources.is_empty());
    let client = packet.into_world(&client).unwrap();
    assert!(!client.resource_working(resource));
}

#[test]
fn interior_worker_keeps_mine_visible_and_waits_inside_depot_before_delivery() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/terran-demo");
    let original = Package::load(&path).unwrap().world(7).unwrap();
    let mut rules = original.rules().clone();
    rules.victory = false;
    let worker = rules.units.iter_mut().find(|u| u.worker.is_some()).unwrap();
    let worker_type = worker.id;
    worker.speed = 24;
    worker.vision_range = 96;
    worker.worker.as_mut().unwrap().capacity = 8;
    worker.harvest_profiles = vec![HarvestProfile {
        kind: "minerals".into(),
        capacity: 5,
        inside: true,
        amount: 8,
        ticks: 4,
        entry_range: 1,
        depot_ticks: 6,
        depot_inside: true,
    }];
    let depot = rules
        .units
        .iter_mut()
        .find(|u| u.dropoff.contains(&"minerals".into()))
        .unwrap();
    let depot_type = depot.id;
    depot.vision_range = 96;
    let mut map = original.map().clone();
    map.terrain = None;
    map.ai.clear();
    map.mission = None;
    map.fog_of_war = true;
    map.spawns = vec![
        Spawn {
            owner: PlayerId(0),
            unit_type: worker_type,
            position: Position { x: 600, y: 200 },
            ..Default::default()
        },
        Spawn {
            owner: PlayerId(0),
            unit_type: depot_type,
            position: Position { x: 128, y: 200 },
            ..Default::default()
        },
    ];
    map.resources = vec![ResourceSpawn {
        terrain_corners: None,
        kind: "minerals".into(),
        position: Position { x: 640, y: 200 },
        footprint: Footprint {
            width: 32,
            height: 32,
        },
        amount: 100,
        requires_extractor: false,
    }];
    let mut server = World::new(rules, map, 7).unwrap();
    let resource = server.state().resources[0].id;
    let before = server.resource_balance(PlayerId(0), "minerals");
    server
        .step(&[Command {
            tick: server.tick(),
            player: PlayerId(0),
            sequence: 1,
            order: Order::Gather {
                entity: EntityId(1),
                resource,
            },
        }])
        .unwrap();
    for _ in 0..100 {
        if server.resource_working(resource) {
            break;
        }
        server.step(&[]).unwrap();
    }
    assert!(server.resource_working(resource));
    assert_eq!(
        server.visibility(PlayerId(0), Position { x: 640, y: 200 }),
        Visibility::Visible
    );
    assert!(
        server
            .player_view(PlayerId(0))
            .unwrap()
            .active_resources
            .contains(&resource)
    );
    assert!(!server.entity_visible(PlayerId(0), EntityId(1)));
    for _ in 0..100 {
        let worker = &server.state().entities[0];
        if worker.gathering_inside && worker.dropoff_target.is_some() {
            break;
        }
        server.step(&[]).unwrap();
    }
    let worker = &server.state().entities[0];
    assert!(worker.gathering_inside && worker.dropoff_target.is_some());
    assert_eq!(worker.harvest_progress, 1);
    assert_eq!(server.resource_balance(PlayerId(0), "minerals"), before);
    assert!(
        !server.resource_working(resource),
        "delivery must not light the mine"
    );
    let mut restored = server
        .restore_snapshot(server.save_snapshot().unwrap())
        .unwrap();
    for _ in 0..4 {
        server.step(&[]).unwrap();
        restored.step(&[]).unwrap();
        assert_eq!(server.state_hash(), restored.state_hash());
        assert!(server.state().entities[0].gathering_inside);
        assert_eq!(server.resource_balance(PlayerId(0), "minerals"), before);
    }
    server.step(&[]).unwrap();
    restored.step(&[]).unwrap();
    assert_eq!(server.state_hash(), restored.state_hash());
    assert_eq!(server.resource_balance(PlayerId(0), "minerals"), before + 8);
    assert!(!server.state().entities[0].gathering_inside);
    assert!(server.state().entities[0].cargo.is_none());
}

#[test]
#[ignore = "requires STRATERUST_WARCRAFT2 pointing to the native installation"]
fn native_mine_handoffs_and_depot_delivery_continue_for_both_worker_races() {
    let root = std::path::PathBuf::from(std::env::var_os("STRATERUST_WARCRAFT2").unwrap());
    for folder in ["human", "orc"] {
        let original = Package::load(&root.join(folder).join("mission02"))
            .unwrap()
            .world(7)
            .unwrap();
        let mut rules = original.rules().clone();
        rules.victory = false;
        let worker = rules
            .units
            .iter()
            .find(|u| u.id == UnitTypeId(if folder == "human" { 3 } else { 4 }))
            .unwrap();
        let worker_type = worker.id;
        let entrance = 320 - 48 - i32::from(worker.footprint.width.div_ceil(2)) - 1;
        let depot_type = rules
            .units
            .iter()
            .find(|u| u.id == UnitTypeId(if folder == "human" { 75 } else { 76 }))
            .unwrap()
            .id;
        let mut map = original.map().clone();
        map.terrain = None;
        map.ai.clear();
        map.mission = None;
        map.fog_of_war = false;
        map.spawns = vec![
            Spawn {
                unit_type: worker_type,
                position: Position {
                    x: entrance,
                    y: 160,
                },
                ..Default::default()
            },
            Spawn {
                unit_type: worker_type,
                position: Position {
                    x: entrance - 80,
                    y: 160,
                },
                ..Default::default()
            },
            Spawn {
                unit_type: depot_type,
                position: Position { x: 128, y: 320 },
                ..Default::default()
            },
        ];
        map.resources = vec![ResourceSpawn {
            terrain_corners: None,
            kind: "gold".into(),
            position: Position { x: 320, y: 160 },
            footprint: Footprint {
                width: 96,
                height: 96,
            },
            amount: 10000,
            requires_extractor: false,
        }];
        let mut w = World::new(rules, map, 7).unwrap();
        let node = w.state().resources[0].id;
        for id in 1..=2 {
            assert!(
                w.step(&[Command {
                    tick: w.tick(),
                    player: PlayerId(0),
                    sequence: id,
                    order: Order::Gather {
                        entity: EntityId(id as u32),
                        resource: node
                    }
                }])
                .unwrap()
                .iter()
                .all(|o| o.rejection.is_none())
            );
            if id == 1 {
                for _ in 0..50 {
                    if w.state().entities[0].gathering_inside {
                        break;
                    }
                    w.step(&[]).unwrap();
                }
                assert!(w.state().entities[0].gathering_inside);
            }
        }
        let mut deliveries = [0; 2];
        for tick in 0..2000 {
            let loaded = std::array::from_fn::<_, 2, _>(|i| w.state().entities[i].cargo.is_some());
            w.step(&[]).unwrap();
            for i in 0..2 {
                if loaded[i] && w.state().entities[i].cargo.is_none() {
                    deliveries[i] += 1;
                }
            }
            if tick == 500 {
                w = w.restore_snapshot(w.save_snapshot().unwrap()).unwrap();
            }
        }
        assert!(
            deliveries.iter().all(|count| *count >= 3),
            "{folder} must keep cycling: {deliveries:?}"
        );
    }
}
