use super::*;

#[test]
fn presentation_snapshot_shares_definitions_but_not_state_or_derived_caches() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/fixtures");
    let mut world = crate::content::Package::load(&path)
        .unwrap()
        .world(42)
        .unwrap();
    let mut copy = world.snapshot();
    assert!(Arc::ptr_eq(&world.map, &copy.map));
    assert!(Arc::ptr_eq(&world.rules, &copy.rules));
    assert!(copy.vision_cells.is_empty());
    assert_eq!(world.state_hash(), copy.state_hash());
    let before = copy.state_hash();
    world.step(&[]).unwrap();
    assert_eq!(copy.state_hash(), before);
    copy.step(&[]).unwrap();
    assert_eq!(world.state_hash(), copy.state_hash());
}

#[test]
#[ignore = "requires locally imported campaign data; short startup workload"]
fn mission_five_startup_profile() {
    let started = std::time::Instant::now();
    let package = crate::content::Package::load(std::path::Path::new(
        "../../local/packages/terran-campaign-v4/terran05",
    ))
    .unwrap();
    let mut world = package.world(0).unwrap();
    for _ in 0..20 {
        world.step(&[]).unwrap();
    }
    assert_eq!(world.tick(), Tick(20));
    assert!(!world.state.mission.as_ref().unwrap().paused);
    eprintln!(
        "mission 5 first 20 ticks: {:?}; hash: {}",
        started.elapsed(),
        world.state_hash()
    );
}

#[test]
fn rng_has_stable_zero_seed_sequence() {
    let mut state = 0;
    assert_eq!(splitmix64(&mut state), 0xe220a8397b1dcdaf);
    assert_eq!(splitmix64(&mut state), 0x6e789e6aa1b965f4);
    assert_eq!(splitmix64(&mut state), 0x06c45d188009454f);
}

#[test]
fn construction_work_state_changes_canonical_hash() {
    let rules = Rules {
        id: "construction-hash".into(),
        units: vec![UnitType::default()],
        ..Rules::default()
    };
    let map = Map {
        initial_explored: Default::default(),
        creation: Default::default(),
        ai: Vec::new(),
        mission: None,
        fog_of_war: false,
        id: "construction-hash".into(),
        width: 64,
        height: 64,
        players: 1,
        spawns: vec![Spawn {
            owner: PlayerId(0),
            unit_type: UnitTypeId(0),
            position: Position { x: 20, y: 20 },
            ..Spawn::default()
        }],
        start_locations: vec![],
        resources: vec![],
        terrain: None,
    };
    let mut world = World::new(rules, map, 0).unwrap();
    world.state.entities[0].construction = Some(Construction {
        worker: Some(EntityId(1)),
        remaining: 10,
        total: 20,
        work_position: None,
        work_ticks: 0,
    });
    let before = world.state_hash();
    world.state.entities[0]
        .construction
        .as_mut()
        .unwrap()
        .work_position = Some(Position { x: 25, y: 20 });
    assert_ne!(world.state_hash(), before);
    let before = world.state_hash();
    world.state.entities[0]
        .construction
        .as_mut()
        .unwrap()
        .work_ticks = 30;
    assert_ne!(world.state_hash(), before);
}
