//! Crowded mine/depot traffic with eight-direction movement and real collision.
use straterust_engine::{content::Package, sim::*};

fn traffic_world() -> World {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/terran-demo");
    let original = Package::load(&path).unwrap().world(7).unwrap();
    let mut rules = original.rules().clone();
    rules.victory = false;
    let worker = rules.units.iter_mut().find(|u| u.worker.is_some()).unwrap();
    let worker_type = worker.id;
    worker.speed = 2;
    worker.motion = Some(Motion {
        eight_directions: true,
        speed: 512,
        acceleration: 0,
        steps: Vec::new(),
    });
    worker.footprint = Footprint {
        width: 24,
        height: 24,
    };
    worker.phases_while_gathering = false;
    worker.worker.as_mut().unwrap().capacity = 8;
    worker.harvest_profiles = vec![HarvestProfile {
        kind: "minerals".into(),
        capacity: 5,
        inside: true,
        amount: 8,
        ticks: 40,
        entry_range: 8,
        depot_ticks: 20,
        depot_inside: true,
    }];
    let depot = rules
        .units
        .iter_mut()
        .find(|u| u.dropoff.contains(&"minerals".into()))
        .unwrap();
    let depot_type = depot.id;
    depot.footprint = Footprint {
        width: 128,
        height: 128,
    };
    depot.placement = depot.footprint;
    let mut map = original.map().clone();
    map.terrain = None;
    map.ai.clear();
    map.mission = None;
    map.fog_of_war = false;
    map.spawns = (0..12)
        .map(|i| Spawn {
            unit_type: worker_type,
            position: Position {
                x: 368 + (i % 4) * 32,
                y: 256 + (i / 4) * 32,
            },
            ..Default::default()
        })
        .chain([Spawn {
            unit_type: depot_type,
            position: Position { x: 160, y: 320 },
            ..Default::default()
        }])
        .collect();
    map.resources = vec![ResourceSpawn {
        kind: "minerals".into(),
        position: Position { x: 640, y: 320 },
        footprint: Footprint {
            width: 96,
            height: 96,
        },
        amount: 10000,
        requires_extractor: false,
        terrain_corners: None,
    }];
    World::new(rules, map, 7).unwrap()
}

#[test]
fn crowded_interior_entrances_keep_every_worker_delivering() {
    let mut world = traffic_world();
    let resource = world.state().resources[0].id;
    let commands: Vec<_> = (1..=12)
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
    let mut deliveries = [0; 12];
    for tick in 0..3000 {
        let loaded = std::array::from_fn::<_, 12, _>(|i| world.state().entities[i].cargo.is_some());
        world.step(&[]).unwrap();
        for i in 0..12 {
            if loaded[i] && world.state().entities[i].cargo.is_none() {
                deliveries[i] += 1;
            }
        }
        if tick == 1000 {
            world = world
                .restore_snapshot(world.save_snapshot().unwrap())
                .unwrap();
        }
    }
    assert!(
        deliveries.iter().all(|n| *n >= 3),
        "each worker must keep cycling: {deliveries:?}; {:?}",
        world
            .state()
            .entities
            .iter()
            .take(12)
            .map(|e| (
                e.id,
                e.position,
                e.harvest_spot,
                e.dropoff_target,
                e.gathering_inside,
                &e.path
            ))
            .collect::<Vec<_>>()
    );
}

#[test]
fn interior_entry_accepts_each_adjacent_side_and_corner_without_exact_contact() {
    let original = traffic_world();
    let worker_type = original.state().entities[0].unit_type;
    let mut rules = original.rules().clone();
    rules
        .units
        .iter_mut()
        .find(|u| u.id == worker_type)
        .unwrap()
        .harvest_profiles[0]
        .capacity = 8;
    let mut map = original.map().clone();
    map.spawns = [
        (-64, -64),
        (0, -64),
        (64, -64),
        (-64, 0),
        (64, 0),
        (-64, 64),
        (0, 64),
        (64, 64),
    ]
    .map(|(x, y)| Spawn {
        unit_type: worker_type,
        position: Position {
            x: 640 + x,
            y: 320 + y,
        },
        ..Default::default()
    })
    .to_vec();
    let mut w = World::new(rules, map, 7).unwrap();
    let resource = w.state().resources[0].id;
    let commands: Vec<_> = (1..=8)
        .map(|id| Command {
            tick: w.tick(),
            player: PlayerId(0),
            sequence: u64::from(id),
            order: Order::Gather {
                entity: EntityId(id),
                resource,
            },
        })
        .collect();
    assert!(
        w.step(&commands)
            .unwrap()
            .iter()
            .all(|o| o.rejection.is_none())
    );
    for entity in &w.state().entities {
        assert!(
            entity.gathering_inside,
            "adjacent entry must not navigate to a reserved point: {:?}",
            entity.position
        );
        assert!(entity.harvest_spot.is_none());
    }
    // The new field is optional for existing packages.
    let legacy: HarvestProfile =
        ron::from_str("(kind:\"gold\",capacity:5,inside:true,amount:100,ticks:150)").unwrap();
    assert_eq!(legacy.entry_range, 1);
}

fn entrance_pocket_world(blocked: bool) -> World {
    use straterust_engine::map::{Terrain, WALKABLE};
    let original = traffic_world();
    let mut map = original.map().clone();
    let mine = Position { x: 656, y: 336 };
    map.resources[0].position = mine;
    map.spawns = vec![
        Spawn {
            unit_type: original.state().entities[0].unit_type,
            position: Position { x: 720, y: 336 },
            ..Default::default()
        },
        original.map().spawns[12].clone(),
    ];
    let columns = map.width as u32 / 32;
    let rows = map.height as u32 / 32;
    let mut flags = vec![WALKABLE; (columns * rows) as usize];
    // A one-tile pocket is open against the mine's eastern entrance, but
    // surrounded by forest on its other three sides, as in Orc mission 2.
    for (x, y) in [(22, 9), (23, 10), (22, 11)] {
        flags[y * columns as usize + x] = 0;
    }
    for x in 19..=22 {
        flags[8 * columns as usize + x] = 0;
        flags[12 * columns as usize + x] = 0;
    }
    if blocked {
        map.spawns.extend((0..5).map(|i| Spawn {
            unit_type: original.state().entities[0].unit_type,
            position: Position {
                x: 596,
                y: 288 + i * 24,
            },
            ..Default::default()
        }));
    }
    map.terrain = Some(Terrain {
        cell_size: 32,
        columns,
        rows,
        flags,
    });
    World::new(original.rules().clone(), map, 7).unwrap()
}

#[test]
fn miner_exits_toward_depot_instead_of_an_isolated_entrance_pocket() {
    let mut w = entrance_pocket_world(false);
    let mine = w.state().resources[0].position;
    let resource = w.state().resources[0].id;
    let outcome = w
        .step(&[Command {
            tick: w.tick(),
            player: PlayerId(0),
            sequence: 1,
            order: Order::Gather {
                entity: EntityId(1),
                resource,
            },
        }])
        .unwrap();
    assert!(outcome[0].rejection.is_none());
    assert!(w.state().entities[0].gathering_inside);
    while w.state().entities[0].cargo.is_none() {
        assert!(
            w.tick().0 < 100,
            "the worker must leave when mining finishes"
        );
        w.step(&[]).unwrap();
    }
    assert!(
        w.state().entities[0].position.x < mine.x,
        "exit on the depot side, rather than reappearing in the enclosed entrance"
    );
    let mut deliveries = 0;
    for tick in 0..1500 {
        let loaded = w.state().entities[0].cargo.is_some();
        w.step(&[]).unwrap();
        if loaded && w.state().entities[0].cargo.is_none() {
            deliveries += 1;
        }
        if tick == 100 {
            w = w.restore_snapshot(w.save_snapshot().unwrap()).unwrap();
        }
    }
    assert!(
        deliveries >= 3,
        "the miner must keep delivering: {deliveries}"
    );
}

#[test]
fn crowded_exit_waits_inside_until_a_reachable_place_opens() {
    let mut w = entrance_pocket_world(true);
    let resource = w.state().resources[0].id;
    w.step(&[Command {
        tick: w.tick(),
        player: PlayerId(0),
        sequence: 1,
        order: Order::Gather {
            entity: EntityId(1),
            resource,
        },
    }])
    .unwrap();
    for _ in 0..60 {
        w.step(&[]).unwrap();
    }
    let miner = &w.state().entities[0];
    assert!(
        miner.gathering_inside,
        "stay inside instead of emerging into the enclosed pocket"
    );
    assert!(miner.cargo.is_none());
    assert_eq!(w.state().resources[0].amount, 10000);
    w = w.restore_snapshot(w.save_snapshot().unwrap()).unwrap();
    let commands: Vec<_> = (3..=7)
        .map(|id| Command {
            tick: w.tick(),
            player: PlayerId(0),
            sequence: u64::from(id),
            order: Order::Move {
                entity: EntityId(id),
                target: Position {
                    x: 400,
                    y: 288 + (id - 3) as i32 * 24,
                },
            },
        })
        .collect();
    assert!(
        w.step(&commands)
            .unwrap()
            .iter()
            .all(|outcome| outcome.rejection.is_none())
    );
    let mut deliveries = 0;
    for _ in 0..1500 {
        let loaded = w.state().entities[0].cargo.is_some();
        w.step(&[]).unwrap();
        if loaded && w.state().entities[0].cargo.is_none() {
            deliveries += 1;
        }
    }
    assert!(
        deliveries >= 3,
        "resume repeated deliveries after the entrance clears: {deliveries}"
    );
}

#[test]
#[ignore = "requires STRATERUST_WARCRAFT2 pointing to the native installation"]
fn native_crowded_mine_and_town_hall_keep_every_worker_delivering() {
    let root = std::path::PathBuf::from(std::env::var_os("STRATERUST_WARCRAFT2").unwrap());
    for race in ["human", "orc"] {
        let original = Package::load(&root.join(race).join("mission02"))
            .unwrap()
            .world(7)
            .unwrap();
        let worker_type = UnitTypeId(if race == "human" { 3 } else { 4 });
        let town = original
            .state()
            .entities
            .iter()
            .find(|e| {
                e.owner == PlayerId(0)
                    && original
                        .unit_type(e.unit_type)
                        .unwrap()
                        .dropoff
                        .contains(&"gold".into())
            })
            .unwrap()
            .clone();
        let mine = original
            .state()
            .resources
            .iter()
            .filter(|r| r.kind == "gold")
            .min_by_key(|r| {
                (
                    i64::from(r.position.x - town.position.x).pow(2)
                        + i64::from(r.position.y - town.position.y).pow(2),
                    r.id,
                )
            })
            .unwrap()
            .clone();
        let mut rules = original.rules().clone();
        rules.victory = false;
        let mut map = original.map().clone();
        map.ai.clear();
        map.mission = None;
        map.fog_of_war = false;
        // Exercise sustained traffic instead of ending when the native mine's
        // finite starting supply runs out during the stress check.
        map.resources[(mine.id.0 - 1) as usize].amount = 100000;
        map.spawns = vec![Spawn {
            unit_type: town.unit_type,
            position: town.position,
            ..Default::default()
        }];
        let probe = World::new(rules.clone(), map.clone(), 7).unwrap();
        let worker = probe.unit_type(worker_type).unwrap();
        let mut places: Vec<_> = (-8..=8)
            .flat_map(|y| {
                (-8..=8).map(move |x| Position {
                    x: mine.position.x / 32 * 32 + 16 + x * 32,
                    y: mine.position.y / 32 * 32 + 16 + y * 32,
                })
            })
            .filter(|p| probe.can_place(*p, worker.footprint, MovementClass::Ground, None))
            .collect();
        places.sort_by_key(|p| {
            i64::from(p.x - mine.position.x).pow(2) + i64::from(p.y - mine.position.y).pow(2)
        });
        assert!(places.len() >= 12);
        map.spawns
            .extend(places.into_iter().take(12).map(|position| Spawn {
                unit_type: worker_type,
                position,
                ..Default::default()
            }));
        let mut w = World::new(rules, map, 7).unwrap();
        let commands: Vec<_> = (2..=13)
            .map(|id| Command {
                tick: w.tick(),
                player: PlayerId(0),
                sequence: u64::from(id),
                order: Order::Gather {
                    entity: EntityId(id),
                    resource: mine.id,
                },
            })
            .collect();
        assert!(
            w.step(&commands)
                .unwrap()
                .iter()
                .all(|o| o.rejection.is_none())
        );
        let mut deliveries = [0; 12];
        let mut last_delivery = [0; 12];
        for tick in 0..6000 {
            let loaded =
                std::array::from_fn::<_, 12, _>(|i| w.state().entities[i + 1].cargo.is_some());
            w.step(&[]).unwrap();
            for i in 0..12 {
                if loaded[i] && w.state().entities[i + 1].cargo.is_none() {
                    deliveries[i] += 1;
                    last_delivery[i] = tick;
                }
            }
            if tick == 2000 {
                w = w.restore_snapshot(w.save_snapshot().unwrap()).unwrap();
            }
        }
        assert!(
            deliveries.iter().all(|n| *n >= 3),
            "{race}: {deliveries:?}; worker 4 {:?}; mine {:?}; {:?}",
            w.state().entities[3],
            w.state().resources.iter().find(|r| r.id == mine.id),
            w.state()
                .entities
                .iter()
                .skip(1)
                .map(|e| (
                    e.id,
                    e.position,
                    e.harvest_spot,
                    e.dropoff_target,
                    e.gathering_inside,
                    &e.path
                ))
                .collect::<Vec<_>>()
        );
        assert!(
            last_delivery.iter().all(|tick| *tick >= 4000),
            "{race}: traffic must keep making progress in the final interval: {last_delivery:?}; deliveries {deliveries:?}; remaining {:?}; {:?}",
            w.state().resources.iter().find(|r| r.id == mine.id),
            w.state()
                .entities
                .iter()
                .skip(1)
                .map(|e| (
                    e.id,
                    e.position,
                    &e.cargo,
                    e.harvest_progress,
                    e.dropoff_target,
                    e.gathering_inside,
                    e.path_retry,
                    &e.route_wait,
                    &e.path
                ))
                .collect::<Vec<_>>()
        );
        eprintln!(
            "{race}: twelve workers delivered {deliveries:?}; final deliveries {last_delivery:?}"
        );
    }
}
