use super::*;
use straterust_engine::{assets::ClipKind, sim::Spawn};

#[test]
#[ignore = "requires private mission 5; verifies a lethal Marine shot and aircraft movement art"]
fn native_marine_flash_survives_target_death_and_aircraft_engines_animate() {
    let directory = PathBuf::from(std::env::var_os("STRATERUST_ASSET_PACKAGE").unwrap());
    let mut app = App::load(
        &directory,
        Config {
            audio: false,
            ..Default::default()
        },
        None,
    )
    .unwrap();
    let mut map = app.world.map().clone();
    map.mission = None;
    map.ai.clear();
    map.terrain = None;
    map.fog_of_war = false;
    map.initial_explored.clear();
    map.creation.clear();
    map.resources.clear();
    map.start_locations.clear();
    map.players = 2;
    map.spawns = vec![
        Spawn {
            unit_type: UnitTypeId(1),
            position: Position { x: 512, y: 512 },
            ..Default::default()
        },
        Spawn {
            owner: PlayerId(1),
            unit_type: UnitTypeId(1),
            position: Position { x: 608, y: 512 },
            hp_percent: Some(1),
            ..Default::default()
        },
        Spawn {
            owner: PlayerId(1),
            unit_type: UnitTypeId(1),
            position: Position { x: 3000, y: 3000 },
            ..Default::default()
        },
    ];
    app.world = World::new(app.world.rules().clone(), map.clone(), 42).unwrap();
    app.visuals = Visuals::new(&app.world);
    app.mission_ui = None;
    app.issue(Order::AttackMove {
        entity: EntityId(1),
        target: Position { x: 900, y: 384 },
    })
    .unwrap();
    until(
        &mut app,
        |a| a.visuals.get(EntityId(1)).unwrap().shot_tick.is_some(),
        10,
    );
    let startup = pose(&app, EntityId(1)).unwrap();
    // Retail Marine damage has one tick of startup; then attack-move resumes.
    advance(&mut app);
    assert!(
        !app.world
            .state()
            .entities
            .iter()
            .any(|e| e.id == EntityId(2))
    );
    advance(&mut app);
    advance(&mut app);
    assert_eq!(
        app.visuals.get(EntityId(1)).unwrap().action,
        VisualAction::Move
    );
    let assets = app.assets.as_ref().unwrap();
    let sprite = assets.sprite(UnitTypeId(1)).unwrap();
    let flash = visual::unit_image(
        assets,
        entity(&app, EntityId(1)),
        app.visuals.get(EntityId(1)),
        &app.world,
    )
    .unwrap();
    let heading = 8;
    assert!(
        flash.image.rgba == sprite.frames[51 + heading].rgba,
        "retail muzzle-flash pose"
    );
    assert_ne!(startup, pose(&app, EntityId(1)).unwrap());
    app.camera.x = 512.0;
    app.camera.y = 512.0;
    capture(&app, "marine-lethal-muzzle-flash", [1100, 760], None);
    app.issue(Order::Stop {
        entity: EntityId(1),
    })
    .unwrap();
    for _ in 0..10 {
        advance(&mut app);
    }
    let assets = app.assets.as_ref().unwrap();
    let sprite = assets.sprite(UnitTypeId(1)).unwrap();
    let rest = visual::unit_image(
        assets,
        entity(&app, EntityId(1)),
        app.visuals.get(EntityId(1)),
        &app.world,
    )
    .unwrap();
    let facing = usize::from(app.visuals.get(EntityId(1)).unwrap().facing);
    let heading = if facing > 16 { 32 - facing } else { facing };
    assert!(
        rest.image.rgba == sprite.frames[68 + heading].rgba,
        "attack finishes instead of looping"
    );

    for unit in [23, 53] {
        map.spawns = vec![
            Spawn {
                unit_type: UnitTypeId(unit),
                position: Position { x: 512, y: 512 },
                ..Default::default()
            },
            Spawn {
                owner: PlayerId(1),
                unit_type: UnitTypeId(1),
                position: Position { x: 3000, y: 3000 },
                ..Default::default()
            },
        ];
        app.world = World::new(app.world.rules().clone(), map.clone(), 42).unwrap();
        app.world.step(&[]).unwrap();
        app.visuals = Visuals::new(&app.world);
        let assets = app.assets.as_ref().unwrap();
        let sprite = assets.sprite(UnitTypeId(unit)).unwrap();
        assert!(sprite.clip(ClipKind::Shadow).is_some());
        for facing in [0, 8, 16, 24] {
            let mut observed = *app.visuals.get(EntityId(1)).unwrap();
            observed.facing = facing;
            let sample = |v: &visual::UnitVisual| {
                visual::unit_image(assets, entity(&app, EntityId(1)), Some(v), &app.world).unwrap()
            };
            let idle = sample(&observed);
            observed.action = VisualAction::Move;
            let first = sample(&observed);
            observed.since_tick = 0;
            let second = sample(&observed);
            assert!(
                first.image.rgba != idle.image.rgba,
                "movement adds engine artwork: unit {unit}, heading {facing}"
            );
            assert!(
                first.image.rgba != second.image.rgba,
                "engine glow changes phase: unit {unit}, heading {facing}"
            );
            assert_eq!(first.flip_x, facing > 16);
        }
        app.issue(Order::Move {
            entity: EntityId(1),
            target: Position { x: 900, y: 512 },
        })
        .unwrap();
        until(
            &mut app,
            |a| a.visuals.get(EntityId(1)).unwrap().action == VisualAction::Move,
            12,
        );
        assert_eq!(
            app.visuals.get(EntityId(1)).unwrap().action,
            VisualAction::Move
        );
        capture(
            &app,
            &format!("aircraft-{unit}-engine-glow"),
            [1100, 760],
            None,
        );
        app.issue(Order::Stop {
            entity: EntityId(1),
        })
        .unwrap();
        until(
            &mut app,
            |a| a.visuals.get(EntityId(1)).unwrap().action == VisualAction::Idle,
            24,
        );
        assert_eq!(
            app.visuals.get(EntityId(1)).unwrap().action,
            VisualAction::Idle
        );
    }
}
