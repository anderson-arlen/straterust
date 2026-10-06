use straterust_engine::sim::*;

fn amount(value: u32) -> ResourceAmount {
    ResourceAmount {
        kind: "ore".into(),
        amount: value,
    }
}
fn fp(width: u16, height: u16) -> Footprint {
    Footprint { width, height }
}
fn point(x: i32, y: i32) -> Position {
    Position { x, y }
}
fn spawn(owner: u16, unit_type: u16, x: i32, y: i32) -> Spawn {
    Spawn {
        owner: PlayerId(owner),
        unit_type: UnitTypeId(unit_type),
        position: point(x, y),
        ..Spawn::default()
    }
}
fn definitions() -> (Rules, Map) {
    let rules = Rules {
        id: "original-rts-tests".into(),
        starting_resources: vec![amount(50)],
        supply_limit: 20,
        units: vec![
            UnitType {
                id: UnitTypeId(1),
                speed: 8,
                footprint: fp(2, 2),
                max_hp: 20,
                cost: vec![amount(5)],
                build_ticks: 3,
                supply_used: 1,
                weapon: Some(Weapon {
                    cooldown_jitter: None,
                    targets_air: false,
                    damage_kind: Default::default(),
                    splash: None,
                    strikes: Vec::new(),
                    damage: 4,
                    range: 12,
                    cooldown: 3,
                }),
                prerequisites: vec![UnitTypeId(5)],
                ..UnitType::default()
            },
            UnitType {
                id: UnitTypeId(2),
                speed: 8,
                footprint: fp(2, 2),
                max_hp: 20,
                cost: vec![amount(5)],
                build_ticks: 3,
                supply_used: 1,
                worker: Some(WorkerStats {
                    capacity: 8,
                    harvest_amount: 8,
                    harvest_ticks: 2,
                    build_rate: 1,
                    resource_kinds: vec!["ore".into()],
                    idle_resource_radius: 256,
                }),
                builds: vec![UnitTypeId(4), UnitTypeId(5)],
                ..UnitType::default()
            },
            UnitType {
                id: UnitTypeId(3),
                speed: 0,
                footprint: fp(20, 20),
                placement: fp(24, 24),
                structure: true,
                max_hp: 100,
                supply_provided: 2,
                dropoff: vec!["ore".into()],
                trains: vec![UnitTypeId(2)],
                ..UnitType::default()
            },
            UnitType {
                id: UnitTypeId(4),
                speed: 0,
                footprint: fp(12, 12),
                placement: fp(16, 16),
                structure: true,
                max_hp: 30,
                cost: vec![amount(10)],
                build_ticks: 6,
                supply_provided: 4,
                ..UnitType::default()
            },
            UnitType {
                id: UnitTypeId(5),
                speed: 0,
                footprint: fp(16, 16),
                placement: fp(20, 20),
                structure: true,
                max_hp: 50,
                cost: vec![amount(20)],
                build_ticks: 6,
                prerequisites: vec![UnitTypeId(4)],
                trains: vec![UnitTypeId(1)],
                ..UnitType::default()
            },
        ],
        ..Rules::default()
    };
    let map = Map {
        initial_explored: Default::default(),
        creation: Default::default(),
        ai: Vec::new(),
        mission: None,
        fog_of_war: false,
        id: "original-rts-map".into(),
        width: 256,
        height: 256,
        players: 2,
        spawns: vec![
            spawn(0, 3, 40, 40),
            spawn(0, 2, 70, 40),
            spawn(1, 1, 220, 220),
        ],
        start_locations: vec![],
        resources: vec![ResourceSpawn {
            requires_extractor: false,
            kind: "ore".into(),
            position: point(120, 40),
            amount: 13,
            footprint: fp(8, 8),
        }],
        terrain: None,
    };
    (rules, map)
}
fn world() -> World {
    let (rules, map) = definitions();
    World::new(rules, map, 42).unwrap()
}
fn send(world: &mut World, order: Order) -> Option<Rejection> {
    let sequence = world.state().last_sequences[0] + 1;
    world
        .step(&[Command {
            tick: world.tick(),
            player: PlayerId(0),
            sequence,
            order,
        }])
        .unwrap()
        .remove(0)
        .rejection
}
fn run(world: &mut World, ticks: usize) {
    for _ in 0..ticks {
        world.step(&[]).unwrap();
    }
}
fn entity(world: &World, id: u32) -> &Entity {
    world
        .state()
        .entities
        .iter()
        .find(|entity| entity.id == EntityId(id))
        .unwrap()
}
fn build(world: &mut World, kind: u16, position: Position) -> EntityId {
    let id = EntityId(world.state().next_entity_id);
    assert_eq!(
        send(
            world,
            Order::Build {
                entity: EntityId(2),
                unit_type: UnitTypeId(kind),
                position
            }
        ),
        None
    );
    for _ in 0..100 {
        if entity(world, id.0).construction.is_none() {
            return id;
        }
        world.step(&[]).unwrap();
    }
    panic!("construction did not complete: {:?}", world.state());
}

fn construction_world() -> World {
    let (mut rules, mut map) = definitions();
    rules.units[1].speed = 3;
    rules.units[1].footprint = fp(4, 4);
    rules.units[3].footprint = fp(40, 32);
    rules.units[3].placement = fp(40, 32);
    rules.units[3].build_ticks = 360;
    rules.units[3].max_hp = 500;
    map.spawns = vec![spawn(0, 2, 70, 100)];
    map.resources.clear();
    World::new(rules, map, 42).unwrap()
}

fn begin_construction(world: &mut World, position: Position) {
    assert_eq!(
        send(
            world,
            Order::Build {
                entity: EntityId(1),
                unit_type: UnitTypeId(4),
                position,
            }
        ),
        None
    );
}

fn wait_for_construction_travel(world: &mut World) {
    for _ in 0..200 {
        let worker = entity(world, 1);
        let progress = entity(world, 2).construction.as_ref().unwrap();
        if progress.work_position.is_some() && worker.target.is_some() {
            return;
        }
        world.step(&[]).unwrap();
    }
    panic!("worker never repositioned");
}

#[path = "rts/economy.rs"]
mod economy;

#[path = "rts/construction.rs"]
mod construction;

#[path = "rts/combat.rs"]
mod combat;
