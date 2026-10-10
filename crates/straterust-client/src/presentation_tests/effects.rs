use super::*;

#[test]
#[ignore = "requires a private campaign package; captures one Raynor shot in ten ticks"]
fn raynor_grenade_flight_and_impact_use_source_art() {
    let directory = PathBuf::from(
        std::env::var_os("STRATERUST_ASSET_PACKAGE").expect("set STRATERUST_ASSET_PACKAGE"),
    );
    let package = Package::load(&directory).unwrap();
    let mut map = package.world(42).unwrap().map().clone();
    map.mission = None;
    map.ai.clear();
    map.fog_of_war = false;
    map.terrain = None;
    map.players = 2;
    map.initial_explored.clear();
    map.creation.clear();
    map.start_locations.clear();
    map.resources.clear();
    map.spawns = vec![
        straterust_engine::sim::Spawn {
            owner: PlayerId(0),
            unit_type: UnitTypeId(10),
            position: Position { x: 512, y: 512 },
            ..Default::default()
        },
        straterust_engine::sim::Spawn {
            owner: PlayerId(1),
            unit_type: UnitTypeId(7),
            position: Position { x: 656, y: 512 },
            ..Default::default()
        },
    ];
    let mut app = App::new(
        &package,
        Config {
            audio: false,
            zoom: 2.0,
            ..Default::default()
        },
        read_ron(&directory.join("presentation.ron")).unwrap(),
        AssetPack::load(&directory).unwrap(),
        None,
    )
    .unwrap();
    app.world = World::new(app.world.rules().clone(), map, 42).unwrap();
    app.visuals = Visuals::new(&app.world);
    app.camera.x = 584.0;
    app.camera.y = 512.0;
    let mut flight_seen = false;
    let mut impact_seen = false;
    for _ in 0..10 {
        advance(&mut app);
        let hash = app.world.state_hash();
        for shot in app
            .visuals
            .projectiles()
            .iter()
            .filter(|shot| shot.unit_type == UnitTypeId(10))
        {
            let effect = app
                .assets
                .as_ref()
                .unwrap()
                .projectile(shot.unit_type)
                .unwrap();
            let Some((frame, position)) = shot.sample(effect) else {
                continue;
            };
            assert!(
                frame
                    .image
                    .rgba
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .any(|pixel| pixel[3] > 0)
            );
            if position[0] < f64::from(shot.to.x)
                && position[1] < f64::from(shot.from.y)
                && !flight_seen
            {
                assert!(position[0] > f64::from(shot.from.x));
                capture(&app, "raynor-grenade-flight", [1100, 760], None);
                flight_seen = true;
            }
            if position == [f64::from(shot.to.x), f64::from(shot.to.y)] && !impact_seen {
                assert!(
                    frame
                        .image
                        .rgba
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .any(|pixel| pixel[3] > 0 && pixel[3] < 255)
                );
                capture(&app, "raynor-grenade-impact", [1100, 760], None);
                impact_seen = true;
            }
        }
        assert_eq!(app.world.state_hash(), hash, "drawing cannot change combat");
    }
    assert!(
        flight_seen && impact_seen,
        "the shot must travel and explode"
    );
}

#[test]
#[ignore = "requires a private mission 3 package; renders effects without a campaign playthrough"]
fn building_fire_turret_and_gas_use_source_art() {
    use straterust_engine::sim::{ResourceSpawn, Spawn};
    let directory = PathBuf::from(std::env::var_os("STRATERUST_ASSET_PACKAGE").unwrap());
    let package = Package::load(&directory).unwrap();
    let mut map = package.world(42).unwrap().map().clone();
    map.mission = None;
    map.ai.clear();
    map.fog_of_war = false;
    map.terrain = None;
    map.initial_explored.clear();
    map.creation.clear();
    map.start_locations.clear();
    map.spawns = vec![
        Spawn {
            unit_type: UnitTypeId(3),
            position: Position { x: 400, y: 512 },
            hp_percent: Some(20),
            ..Spawn::default()
        },
        Spawn {
            unit_type: UnitTypeId(14),
            position: Position { x: 560, y: 512 },
            ..Spawn::default()
        },
        Spawn {
            unit_type: UnitTypeId(36),
            position: Position { x: 720, y: 512 },
            ..Spawn::default()
        },
        Spawn {
            owner: PlayerId(1),
            unit_type: UnitTypeId(42),
            position: Position { x: 720, y: 672 },
            ..Spawn::default()
        },
    ];
    map.resources = vec![
        (560, 512, 1000),
        (400, 672, 1000),
        (560, 672, 1),
        (720, 672, 1000),
    ]
    .into_iter()
    .map(|(x, y, amount)| ResourceSpawn {
        terrain_corners: None,
        kind: "gas".into(),
        position: Position { x, y },
        amount,
        footprint: straterust_engine::sim::Footprint {
            width: 128,
            height: 64,
        },
        requires_extractor: true,
    })
    .collect();
    let mut app = App::new(
        &package,
        Config {
            audio: false,
            zoom: 1.8,
            ..Default::default()
        },
        read_ron(&directory.join("presentation.ron")).unwrap(),
        AssetPack::load(&directory).unwrap(),
        None,
    )
    .unwrap();
    app.world = World::new(app.world.rules().clone(), map, 42).unwrap();
    app.visuals = Visuals::new(&app.world);
    app.camera.x = 550.0;
    app.camera.y = 592.0;
    let assets = app.assets.as_ref().unwrap();
    assets.validate_for_world(&app.world).unwrap();
    assert!(
        assets
            .resources
            .iter()
            .any(|resource| resource.manifest.kind == "gas")
    );
    let building = &app.world.state().entities[0];
    let flames = visual::damage_frames(assets, building, &app.world, 0);
    assert!(!flames.is_empty());
    assert!(flames.iter().all(|(frame, _)| {
        frame
            .image
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .any(|pixel| pixel[3] > 0 && pixel[3] < 255)
    }));
    let mut repaired = building.clone();
    repaired.hp = app.world.unit_type(building.unit_type).unwrap().max_hp;
    assert!(visual::damage_frames(assets, &repaired, &app.world, 0).is_empty());
    for unit in [None, Some(UnitTypeId(14)), Some(UnitTypeId(42))] {
        assert!(
            (0..64).any(|tick| !visual::gas_frames(assets, unit, tick * 42, 0, false).is_empty())
        );
        assert!(
            (0..64).any(|tick| !visual::gas_frames(assets, unit, tick * 42, 0, true).is_empty())
        );
    }
    let turret = assets.sprite(UnitTypeId(36)).unwrap();
    let idle = turret
        .clips
        .iter()
        .find(|clip| clip.kind == straterust_engine::assets::ClipKind::Idle)
        .unwrap();
    assert_eq!(idle.frames.len(), 32);
    assert_ne!(
        turret.frames[usize::from(idle.frames[0].frame)].rgba,
        turret.frames[usize::from(idle.frames[8].frame)].rgba
    );
    let initial_turret = pose(&app, app.world.state().entities[2].id).unwrap();
    for (ticks, phase) in [(0, 0_u128), (8, 336), (24, 1344)] {
        for _ in 0..ticks {
            advance(&mut app);
        }
        if ticks == 8 {
            assert_ne!(
                initial_turret,
                pose(&app, app.world.state().entities[2].id).unwrap()
            );
        }
        let hash = app.world.state_hash();
        capture_at(
            &app,
            &format!("building-effects-{phase}"),
            [1100, 760],
            None,
            None,
            phase,
        );
        assert_eq!(hash, app.world.state_hash());
    }
}

#[test]
#[ignore = "requires a private mission 3 package; checks one Sunken attack and death"]
fn sunken_attacks_dies_and_has_creep_source_art() {
    use straterust_engine::sim::{Order, Spawn};
    let directory = PathBuf::from(std::env::var_os("STRATERUST_ASSET_PACKAGE").unwrap());
    let package = Package::load(&directory).unwrap();
    let mut map = package.world(7).unwrap().map().clone();
    map.mission = None;
    map.ai.clear();
    map.fog_of_war = false;
    map.terrain = None;
    map.creation.clear();
    map.initial_explored.clear();
    map.resources.clear();
    map.start_locations.clear();
    map.spawns = vec![
        Spawn {
            unit_type: UnitTypeId(41),
            position: Position { x: 512, y: 512 },
            hp_percent: Some(100),
            ..Spawn::default()
        },
        Spawn {
            owner: PlayerId(1),
            unit_type: UnitTypeId(1),
            position: Position { x: 672, y: 512 },
            ..Spawn::default()
        },
    ];
    let mut app = App::new(
        &package,
        Config {
            audio: false,
            zoom: 1.8,
            ..Default::default()
        },
        read_ron(&directory.join("presentation.ron")).unwrap(),
        AssetPack::load(&directory).unwrap(),
        None,
    )
    .unwrap();
    app.world = World::new(app.world.rules().clone(), map, 7).unwrap();
    app.visuals = Visuals::new(&app.world);
    app.camera.x = 568.0;
    app.camera.y = 552.0;
    assert!(app.world.creep_at(Position { x: 512, y: 512 }));
    assert!(app.assets.as_ref().unwrap().creep.is_some());
    for _ in 0..3 {
        advance(&mut app);
    }
    assert!(
        entity(&app, EntityId(2)).hp < 40,
        "the stationary Sunken must damage a Marine"
    );
    assert_eq!(
        app.visuals.get(EntityId(1)).unwrap().action,
        VisualAction::Attack
    );
    assert!(
        app.visuals
            .projectiles()
            .iter()
            .any(|shot| shot.unit_type == UnitTypeId(41))
    );
    capture(&app, "sunken-creep-attack", [1100, 760], None);
    let mut death_map = app.world.map().clone();
    death_map.spawns[0].hp_percent = Some(1);
    app.world = World::new(app.world.rules().clone(), death_map, 7).unwrap();
    app.visuals = Visuals::new(&app.world);
    app.world
        .step(&[Command {
            tick: app.world.tick(),
            player: PlayerId(1),
            sequence: 1,
            order: Order::Attack {
                entity: EntityId(2),
                target: EntityId(1),
            },
        }])
        .unwrap();
    app.visuals.update(&app.world);
    for _ in 0..30 {
        if !app
            .world
            .state()
            .entities
            .iter()
            .any(|entity| entity.id == EntityId(1))
        {
            break;
        }
        advance(&mut app);
    }
    assert!(
        !app.world
            .state()
            .entities
            .iter()
            .any(|entity| entity.id == EntityId(1))
    );
    assert!(
        app.visuals
            .deaths()
            .iter()
            .any(|death| death.unit_type == UnitTypeId(41))
    );
    app.visuals
        .advance_effects(Duration::from_millis(300), app.assets.as_ref());
    let frame =
        visual::death_image(app.assets.as_ref().unwrap(), &app.visuals.deaths()[0]).unwrap();
    assert!(
        frame
            .image
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .any(|pixel| pixel[3] > 0)
    );
    capture(&app, "sunken-creep-death", [1100, 760], None);
}

#[test]
#[ignore = "requires a private campaign import and writes fixed flight/fire review screenshots"]
fn flying_shadows_and_fire_presentation_review() {
    use straterust_engine::sim::{Spawn, World};
    let directory = PathBuf::from(
        std::env::var_os("STRATERUST_ASSET_PACKAGE").expect("set STRATERUST_ASSET_PACKAGE"),
    );
    let package = Package::load(&directory).unwrap();
    let original = package.world(42).unwrap();
    let mut map = original.map().clone();
    map.mission = None;
    map.fog_of_war = false;
    map.terrain = None;
    map.resources.clear();
    map.start_locations.clear();
    let mut rules = original.rules().clone();
    rules.victory = false;
    let mut app = App::new(
        &package,
        Config {
            width: 1280,
            height: 800,
            audio: false,
            ..Config::default()
        },
        read_ron(&directory.join("presentation.ron")).unwrap(),
        AssetPack::load(&directory).unwrap(),
        None,
    )
    .unwrap();
    let home = Position { x: 1000, y: 1100 };
    app.camera = Camera {
        x: f64::from(home.x),
        y: f64::from(home.y),
        viewport: None,
        zoom: 1.0,
    };
    for kind in [3, 5, 15] {
        map.spawns = vec![Spawn {
            owner: PlayerId(0),
            unit_type: UnitTypeId(kind),
            position: home,
            ..Spawn::default()
        }];
        app.world = World::new(rules.clone(), map.clone(), 42).unwrap();
        app.visuals = Visuals::new(&app.world);
        app.queue = CommandQueue::default();
        app.sequence = 0;
        app.selected = BTreeSet::from([EntityId(1)]);
        let assets = app.assets.as_ref().unwrap();
        let shadow =
            visual::shadow_image(assets, entity(&app, EntityId(1)), None, &app.world).unwrap();
        let shadow_anchor = shadow.anchor;
        assert!(
            shadow
                .image
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel[3] == 100)
        );
        capture(&app, &format!("flight-{kind}-ground"), [1280, 800], None);
        app.issue(Order::Lift {
            entity: EntityId(1),
        })
        .unwrap();
        for _ in 0..21 {
            advance(&mut app);
        }
        let lifted = entity(&app, EntityId(1));
        assert!(lifted.airborne && lifted.flight_transition > 0);
        assert_eq!(
            visual::shadow_image(app.assets.as_ref().unwrap(), lifted, None, &app.world)
                .unwrap()
                .anchor,
            shadow_anchor
        );
        capture(&app, &format!("flight-{kind}-lifting"), [1280, 800], None);
        until(
            &mut app,
            |app| entity(app, EntityId(1)).flight_transition == 0,
            64,
        );
        capture(&app, &format!("flight-{kind}-air"), [1280, 800], None);
        let destination = Position {
            x: home.x + 96,
            y: home.y,
        };
        app.issue(Order::Move {
            entity: EntityId(1),
            target: destination,
        })
        .unwrap();
        until(
            &mut app,
            |app| entity(app, EntityId(1)).position == destination,
            200,
        );
        capture(&app, &format!("flight-{kind}-moved"), [1280, 800], None);
        app.issue(Order::Land {
            entity: EntityId(1),
            target: destination,
        })
        .unwrap();
        for _ in 0..21 {
            advance(&mut app);
        }
        assert!(entity(&app, EntityId(1)).airborne);
        assert_eq!(
            visual::shadow_image(
                app.assets.as_ref().unwrap(),
                entity(&app, EntityId(1)),
                None,
                &app.world,
            )
            .unwrap()
            .anchor,
            shadow_anchor
        );
        capture(&app, &format!("flight-{kind}-landing"), [1280, 800], None);
        until(&mut app, |app| !entity(app, EntityId(1)).airborne, 64);
        capture(&app, &format!("flight-{kind}-landed"), [1280, 800], None);
        let mut combat_rules = rules.clone();
        let marine = combat_rules
            .units
            .iter_mut()
            .find(|unit| unit.id == UnitTypeId(1))
            .unwrap();
        let weapon = marine.weapon.as_mut().unwrap();
        weapon.damage = 1_000_000;
        weapon.range = 256;
        weapon.strikes.clear();
        map.spawns[0].hp_percent = Some(1);
        map.spawns.push(Spawn {
            owner: PlayerId(1),
            unit_type: UnitTypeId(1),
            position: Position {
                x: home.x + 200,
                y: home.y,
            },
            ..Spawn::default()
        });
        app.world = World::new(combat_rules, map.clone(), 42).unwrap();
        app.visuals = Visuals::new(&app.world);
        app.world.step(&[]).unwrap();
        app.visuals.update(&app.world);
        app.visuals
            .advance_effects(Duration::from_millis(450), app.assets.as_ref());
        let death = app.visuals.deaths().first().unwrap();
        let frame = visual::death_image(app.assets.as_ref().unwrap(), death).unwrap();
        assert!(
            frame
                .image
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| (1..255).contains(&pixel[3]))
        );
        capture(&app, &format!("fire-{kind}"), [1280, 800], None);
    }
}
