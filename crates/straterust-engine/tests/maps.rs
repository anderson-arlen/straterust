use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use straterust_engine::{
    content::Package,
    map::{
        BLOCKS_SIGHT, BUILDABLE, Footprint, HEIGHT_SHIFT, MovementClass, RAMP, Terrain, WALKABLE,
        encode_terrain,
    },
    sim::{
        Command, EntityId, Map, Order, PlayerId, Position, ResourceSpawn, StartLocation, Tick,
        World,
    },
};

fn fixture() -> World {
    Package::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/fixtures"))
        .unwrap()
        .world(42)
        .unwrap()
}

fn terrain() -> Terrain {
    Terrain {
        cell_size: 8,
        columns: 200,
        rows: 125,
        flags: vec![WALKABLE | BUILDABLE; 200 * 125],
    }
}

struct NativePackage(PathBuf);

impl NativePackage {
    fn new(map: &Map) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "straterust-map-tests-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        for (name, content) in [
            (
                "manifest.ron",
                include_str!("../../../content/fixtures/manifest.ron"),
            ),
            (
                "rules.ron",
                include_str!("../../../content/fixtures/rules.ron"),
            ),
        ] {
            fs::write(path.join(name), content).unwrap();
        }
        fs::write(path.join("map.ron"), ron::to_string(map).unwrap()).unwrap();
        Self(path)
    }
}

impl Drop for NativePackage {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn headless_package_loads_native_terrain_without_presentation() {
    let original = fixture();
    let mut map = original.map().clone();
    map.start_locations = vec![StartLocation {
        player: PlayerId(0),
        position: Position { x: 80, y: 80 },
    }];
    map.resources = vec![ResourceSpawn {
        requires_extractor: false,
        footprint: Footprint::default(),
        kind: "minerals".into(),
        position: Position { x: 40, y: 40 },
        amount: 1500,
    }];
    let package = NativePackage::new(&map);
    fs::write(
        package.0.join("terrain.srtm"),
        encode_terrain(&terrain()).unwrap(),
    )
    .unwrap();
    let world = Package::load(&package.0).unwrap().world(42).unwrap();
    assert_eq!(world.map().terrain.as_ref(), Some(&terrain()));
    assert_eq!(
        world.map().start_locations[0].position,
        map.start_locations[0].position
    );
    assert_eq!(world.map().resources[0].amount, 1500);
    assert!(!package.0.join("assets.ron").exists());
    assert_ne!(world.map_hash(), original.map_hash());
    fs::write(package.0.join("terrain.srtm"), b"SRTM").unwrap();
    assert!(Package::load(&package.0).is_err());
    let mut wrong_coverage = terrain();
    wrong_coverage.cell_size = 4;
    fs::write(
        package.0.join("terrain.srtm"),
        encode_terrain(&wrong_coverage).unwrap(),
    )
    .unwrap();
    assert!(Package::load(&package.0).is_err());
    fs::write(
        package.0.join("terrain.srtm"),
        vec![0; straterust_engine::map::MAX_TERRAIN_BYTES + 1],
    )
    .unwrap();
    assert!(Package::load(&package.0).is_err());
}

#[cfg(unix)]
#[test]
fn terrain_package_member_cannot_be_a_symlink() {
    let map = fixture().map().clone();
    let package = NativePackage::new(&map);
    let outside = NativePackage::new(&map);
    fs::write(
        outside.0.join("terrain.srtm"),
        encode_terrain(&terrain()).unwrap(),
    )
    .unwrap();
    std::os::unix::fs::symlink(
        outside.0.join("terrain.srtm"),
        package.0.join("terrain.srtm"),
    )
    .unwrap();
    assert!(Package::load(&package.0).is_err());
}

#[test]
fn gameplay_hash_covers_terrain_starts_resources_footprints_and_movement() {
    let world = fixture();
    for change in [
        Footprint {
            width: 2,
            height: 1,
        },
        Footprint {
            width: 1,
            height: 2,
        },
    ] {
        let mut rules = world.rules().clone();
        rules.units[0].footprint = change;
        let changed = World::new(rules, world.map().clone(), 42).unwrap();
        assert_ne!(world.rules_hash(), changed.rules_hash());
        assert_ne!(world.state_hash(), changed.state_hash());
    }
    let mut rules = world.rules().clone();
    rules.units[0].movement_class = MovementClass::Air;
    assert_ne!(
        world.state_hash(),
        World::new(rules, world.map().clone(), 42)
            .unwrap()
            .state_hash()
    );
    let mut map = world.map().clone();
    map.start_locations = vec![StartLocation {
        player: PlayerId(0),
        position: Position { x: 10, y: 10 },
    }];
    map.resources = vec![ResourceSpawn {
        requires_extractor: false,
        footprint: Footprint::default(),
        kind: "minerals".into(),
        position: Position { x: 20, y: 20 },
        amount: 1500,
    }];
    map.terrain = Some(terrain());
    let baseline = World::new(world.rules().clone(), map.clone(), 42).unwrap();
    assert_ne!(world.map_hash(), baseline.map_hash());
    for index in 0..8 {
        let mut changed = map.clone();
        match index {
            0 => changed.start_locations[0].player = PlayerId(1),
            1 => changed.start_locations[0].position.x += 1,
            2 => changed.resources[0].kind = "gas".into(),
            3 => changed.resources[0].position.y += 1,
            4 => changed.resources[0].amount += 1,
            5 => changed.resources.clear(),
            6 => changed.start_locations.clear(),
            _ => changed.terrain = None,
        }
        let changed = World::new(world.rules().clone(), changed, 42).unwrap();
        assert_ne!(baseline.map_hash(), changed.map_hash(), "case {index}");
        assert_ne!(baseline.state_hash(), changed.state_hash(), "case {index}");
    }
    for bit in [
        WALKABLE,
        BUILDABLE,
        1 << HEIGHT_SHIFT,
        2 << HEIGHT_SHIFT,
        BLOCKS_SIGHT,
        RAMP,
    ] {
        let mut changed = map.clone();
        changed.terrain.as_mut().unwrap().flags[0] ^= bit;
        assert_ne!(
            baseline.map_hash(),
            World::new(world.rules().clone(), changed, 42)
                .unwrap()
                .map_hash()
        );
    }
    let mut changed = map;
    changed.terrain = Some(Terrain {
        cell_size: 4,
        columns: 400,
        rows: 250,
        flags: vec![WALKABLE | BUILDABLE; 400 * 250],
    });
    assert_ne!(
        baseline.map_hash(),
        World::new(world.rules().clone(), changed, 42)
            .unwrap()
            .map_hash()
    );
}

#[test]
fn map_rejects_invalid_placements_and_terrain_coverage() {
    let world = fixture();
    let mut rules = world.rules().clone();
    rules.units[0].footprint.width = 0;
    assert!(World::new(rules, world.map().clone(), 42).is_err());
    let mut rules = world.rules().clone();
    rules.units[0].footprint = Footprint {
        width: 1000,
        height: 1000,
    };
    assert!(World::new(rules, world.map().clone(), 42).is_err());
    for start in [
        StartLocation {
            player: PlayerId(2),
            position: Position { x: 10, y: 10 },
        },
        StartLocation {
            player: PlayerId(0),
            position: Position { x: -1, y: 10 },
        },
    ] {
        let mut map = world.map().clone();
        map.start_locations = vec![start];
        assert!(World::new(world.rules().clone(), map, 42).is_err());
    }
    let mut map = world.map().clone();
    map.start_locations = vec![
        StartLocation {
            player: PlayerId(0),
            position: Position { x: 10, y: 10 }
        };
        2
    ];
    assert!(World::new(world.rules().clone(), map, 42).is_err());
    for resource in [
        ResourceSpawn {
            requires_extractor: false,
            footprint: Footprint::default(),
            kind: String::new(),
            position: Position { x: 10, y: 10 },
            amount: 1500,
        },
        ResourceSpawn {
            requires_extractor: false,
            footprint: Footprint::default(),
            kind: "minerals".into(),
            position: Position { x: 1600, y: 10 },
            amount: 1500,
        },
        ResourceSpawn {
            requires_extractor: false,
            footprint: Footprint::default(),
            kind: "minerals".into(),
            position: Position { x: 10, y: 10 },
            amount: 0,
        },
    ] {
        let mut map = world.map().clone();
        map.resources = vec![resource];
        assert!(World::new(world.rules().clone(), map, 42).is_err());
    }
    let mut map = world.map().clone();
    map.terrain = Some(terrain());
    map.width += 1;
    assert!(World::new(world.rules().clone(), map, 42).is_err());
}

#[test]
fn occupancy_uses_current_footprints_separate_movement_layers_and_self_exclusion() {
    let original = fixture();
    let mut rules = original.rules().clone();
    rules.units[0].footprint = Footprint {
        width: 8,
        height: 8,
    };
    rules.units[1].footprint = Footprint {
        width: 16,
        height: 16,
    };
    rules.units[1].movement_class = MovementClass::Air;
    let mut world = World::new(rules.clone(), original.map().clone(), 42).unwrap();
    let ground = MovementClass::Ground;
    let center = world.state().entities[0].position;
    let footprint = rules.units[0].footprint;
    assert!(world.is_occupied(center, footprint, ground, None));
    assert!(!world.is_occupied(center, footprint, ground, Some(EntityId(1))));
    assert!(!world.is_occupied(center, footprint, MovementClass::Air, None));
    assert!(!world.can_place(center, footprint, ground, None));
    assert!(world.can_place(center, footprint, ground, Some(EntityId(1))));
    assert!(!world.is_occupied(Position { x: 428, y: 420 }, footprint, ground, None));
    assert!(world.is_occupied(Position { x: 427, y: 420 }, footprint, ground, None));
    assert!(world.is_occupied(
        Position { x: 392, y: 520 },
        Footprint::default(),
        MovementClass::Air,
        None
    ));
    assert!(!world.is_occupied(
        Position { x: 391, y: 520 },
        Footprint::default(),
        MovementClass::Air,
        None
    ));
    assert!(!world.can_place(Position { x: 0, y: 0 }, footprint, ground, None));
    assert!(world.is_occupied(
        Position { x: 416, y: 420 },
        Footprint::default(),
        ground,
        None
    ));
    world
        .step(&[Command {
            tick: Tick(0),
            player: PlayerId(0),
            sequence: 1,
            order: Order::Move {
                entity: EntityId(1),
                target: Position { x: 600, y: 420 },
            },
        }])
        .unwrap();
    assert!(!world.is_occupied(
        Position { x: 416, y: 420 },
        Footprint::default(),
        ground,
        None
    ));
    assert!(world.is_occupied(
        Position { x: 424, y: 420 },
        Footprint::default(),
        ground,
        None
    ));
    let mut map = original.map().clone();
    let mut blocked = terrain();
    blocked.flags[52 * 200 + 52] = 0;
    map.terrain = Some(blocked);
    assert!(World::new(rules.clone(), map.clone(), 42).is_err());
    map.spawns[0].position.x = 600;
    let world = World::new(rules, map, 42).unwrap();
    assert!(!world.can_place(center, footprint, ground, Some(EntityId(1))));
    assert!(world.can_place(center, footprint, MovementClass::Air, None));
}
