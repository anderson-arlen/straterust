use super::*;
use straterust_engine::content::Package;

#[test]
#[ignore = "requires STRATERUST_CAMPAIGNS pointing to a refreshed retail import"]
fn published_zerg_morphs_use_eggs_and_native_mutation_clips() -> Result<()> {
    let root = std::env::var_os("STRATERUST_CAMPAIGNS").context("set STRATERUST_CAMPAIGNS")?;
    let directory = std::path::Path::new(&root).join("zerg/zerg01");
    let package = Package::load(&directory)?;
    let definitions = package.world(42)?;
    let mut map = definitions.map().clone();
    map.mission = None;
    map.ai.clear();
    let mut rules = definitions.rules().clone();
    rules.victory = false;
    rules.starting_resources = vec![
        ResourceAmount {
            kind: "minerals".into(),
            amount: 10000,
        },
        ResourceAmount {
            kind: "gas".into(),
            amount: 10000,
        },
    ];
    let mut world = World::new(rules, map, 42)?;
    let larva = world
        .state()
        .entities
        .iter()
        .find(|e| e.owner == PlayerId(0) && e.unit_type == native_id(35).unwrap())
        .context("no starting larva")?
        .id;
    let target = native_id(41).unwrap();
    let outcomes = world.step(&[Command {
        tick: world.tick(),
        player: PlayerId(0),
        sequence: 1,
        order: Order::Train {
            entity: larva,
            unit_type: target,
        },
    }])?;
    assert_eq!(outcomes[0].rejection, None);
    assert_eq!(
        world
            .state()
            .entities
            .iter()
            .find(|e| e.id == larva)
            .unwrap()
            .unit_type,
        native_id(36).unwrap()
    );
    let filtered = world.player_view(PlayerId(0))?.into_world(&definitions)?;
    assert_eq!(
        filtered
            .state()
            .entities
            .iter()
            .find(|e| e.id == larva)
            .unwrap()
            .unit_type,
        native_id(36).unwrap()
    );
    for _ in 0..world.unit_type(target).unwrap().build_ticks + 2 {
        world.step(&[])?;
    }
    let hatched = world
        .state()
        .entities
        .iter()
        .find(|e| e.id == larva)
        .unwrap();
    assert_eq!(hatched.unit_type, target);
    assert!(hatched.production.is_empty());
    let assets: AssetManifest = ron::de::from_bytes(&std::fs::read(directory.join("assets.ron"))?)?;
    assets.validate()?;
    for (source, kinds) in [
        (36, vec![ClipKind::Birth, ClipKind::Transform]),
        (41, vec![ClipKind::Birth]),
        (42, vec![ClipKind::Birth]),
        (
            135,
            vec![
                ClipKind::Construction,
                ClipKind::ConstructionStart,
                ClipKind::ConstructionEnd,
            ],
        ),
        (132, vec![ClipKind::Construction, ClipKind::Birth]),
        (146, vec![ClipKind::Construction, ClipKind::Birth]),
    ] {
        let sprite = assets
            .extra_units
            .iter()
            .find(|s| Some(s.unit_type) == native_id(source))
            .unwrap();
        for kind in kinds {
            let clip = sprite
                .clips
                .iter()
                .find(|c| c.kind == kind)
                .context("missing native mutation clip")?;
            assert!(clip.frames.len() > 1, "{source}: {kind:?} is static");
            if kind == ClipKind::Construction {
                assert_eq!(
                    clip.progress_starts.iter().map(|v| v.0).collect::<Vec<_>>(),
                    vec![0, 25, 50]
                );
            }
        }
    }
    Ok(())
}

#[test]
#[ignore = "requires STRATERUST_CAMPAIGNS pointing to a refreshed retail import"]
fn published_protoss_power_training_mining_and_warp_art_are_connected() -> Result<()> {
    let root = std::env::var_os("STRATERUST_CAMPAIGNS").context("set STRATERUST_CAMPAIGNS")?;
    let directory = std::path::Path::new(&root).join("protoss/protoss01");
    let definitions = Package::load(&directory)?.world(42)?;
    let mut rules = definitions.rules().clone();
    rules.victory = false;
    rules.starting_resources = vec![ResourceAmount {
        kind: "minerals".into(),
        amount: 1000,
    }];
    let spawn = |source, x| Spawn {
        owner: PlayerId(0),
        unit_type: native_id(source).unwrap(),
        position: Position { x, y: 384 },
        ..Spawn::default()
    };
    let mut map = Map {
        id: "private-source-probe".into(),
        width: 1024,
        height: 768,
        players: 2,
        spawns: vec![spawn(154, 768), spawn(160, 384)],
        start_locations: vec![],
        resources: vec![],
        initial_explored: Default::default(),
        creation: Default::default(),
        ai: vec![],
        mission: None,
        fog_of_war: false,
        terrain: None,
    };
    let command = Command {
        tick: Tick(0),
        player: PlayerId(0),
        sequence: 1,
        order: Order::Train {
            entity: EntityId(2),
            unit_type: native_id(65).unwrap(),
        },
    };
    let mut unpowered = World::new(rules.clone(), map.clone(), 42)?;
    assert_eq!(
        unpowered.step(std::slice::from_ref(&command))?[0].rejection,
        Some(Rejection::NotPowered)
    );
    map.spawns.push(spawn(156, 160));
    let mut powered = World::new(rules.clone(), map.clone(), 42)?;
    assert_eq!(powered.step(&[command])?[0].rejection, None);
    assert!(
        powered
            .unit_type(native_id(64).unwrap())
            .unwrap()
            .phases_while_gathering
    );
    // A returning worker shares the extractor approach with an incoming one.
    // Both must repeatedly deposit cargo instead of blocking the other's exit.
    map.spawns = vec![
        spawn(154, 768),
        spawn(157, 384),
        spawn(64, 470),
        spawn(64, 498),
    ];
    map.resources = vec![ResourceSpawn {
        footprint: Footprint {
            width: 128,
            height: 64,
        },
        kind: "gas".into(),
        position: Position { x: 384, y: 384 },
        amount: 5000,
        requires_extractor: true,
    }];
    let mut gas = World::new(rules, map, 42)?;
    let commands = [3_u32, 4].map(|id| Command {
        tick: Tick(0),
        player: PlayerId(0),
        sequence: u64::from(id),
        order: Order::Gather {
            entity: EntityId(id),
            resource: ResourceId(1),
        },
    });
    assert!(
        gas.step(&commands)?
            .iter()
            .all(|outcome| outcome.rejection.is_none())
    );
    let mut deposits = BTreeSet::new();
    let mut carrying = BTreeSet::new();
    for _ in 0..500 {
        gas.step(&[])?;
        for entity in gas.state().entities.iter().filter(|e| e.id.0 >= 3) {
            if entity.cargo.is_some() {
                carrying.insert(entity.id);
            } else if carrying.remove(&entity.id) {
                deposits.insert(entity.id);
            }
        }
    }
    assert_eq!(deposits, BTreeSet::from([EntityId(3), EntityId(4)]));
    assert!(gas.state().players[0].resources["gas"] >= 32);
    let assets = straterust_engine::assets::AssetPack::load(&directory)?.unwrap();
    // Melee graphics zero means no bullet, rather than Scourge (flingy zero).
    assert!(
        assets
            .projectile_for(native_id(65).unwrap(), false)
            .is_none()
    );
    assert!(
        assets
            .projectile_for(native_id(37).unwrap(), false)
            .is_none()
    );
    assert!(
        assets
            .projectile_for(native_id(66).unwrap(), false)
            .is_some()
    );
    let pylon = assets.sprite(native_id(156).unwrap()).unwrap();
    assert!(pylon.clip(ClipKind::Coverage).is_some());
    let gateway = assets.sprite(native_id(160).unwrap()).unwrap();
    let growing = gateway.clip(ClipKind::Construction).unwrap();
    assert_eq!(growing.progress_starts, vec![(0, 0)]);
    assert!(
        growing
            .frames
            .iter()
            .map(|f| f.frame)
            .collect::<BTreeSet<_>>()
            .len()
            > 1
    );
    assert!(
        gateway
            .clip(ClipKind::ConstructionEnd)
            .unwrap()
            .frames
            .len()
            > 10
    );
    let probe = assets.sprite(native_id(64).unwrap()).unwrap();
    assert_eq!(probe.clip(ClipKind::WorkEffect).unwrap().directions, 32);
    Ok(())
}
