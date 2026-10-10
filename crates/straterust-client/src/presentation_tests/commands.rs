//! Focused source-art review for the current play-session reports.
use super::*;
use straterust_engine::{
    assets::ClipKind,
    sim::{ResourceAmount, Spawn},
};

#[test]
#[ignore = "requires private mission 5 assets; short command/animation review"]
fn native_command_feedback_resources_and_activity() {
    let directory = PathBuf::from(std::env::var_os("STRATERUST_ASSET_PACKAGE").unwrap());
    let package = Package::load(&directory).unwrap();
    let mut app = App::new(
        &package,
        Config {
            audio: false,
            ..Default::default()
        },
        read_ron(&directory.join("presentation.ron")).unwrap(),
        AssetPack::load(&directory).unwrap(),
        None,
    )
    .unwrap();
    app.mission_ui = None;
    let assets = app.assets.as_ref().unwrap();
    for (kind, circle) in [("minerals", 4), ("gas", 8)] {
        assert_eq!(
            assets
                .resources
                .iter()
                .find(|art| art.manifest.kind == kind)
                .unwrap()
                .manifest
                .selection_circle,
            Some(circle)
        );
    }
    let goliath = assets.sprite(UnitTypeId(21)).unwrap();
    let walk = goliath.clip(ClipKind::Walk).unwrap();
    assert!(walk.frames.len() >= 9 * 32);
    assert_ne!(
        goliath.frames[usize::from(walk.frames[8].frame)].rgba,
        goliath.frames[usize::from(walk.frames[40].frame)].rgba
    );
    assert!(
        assets
            .sprite(UnitTypeId(13))
            .unwrap()
            .clip(ClipKind::GarrisonAttack)
            .is_some()
    );
    assert!(
        assets
            .sprite(UnitTypeId(11))
            .unwrap()
            .clip(ClipKind::GarrisonAttack)
            .is_some()
    );
    let mut rules = app.world.rules().clone();
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
    let mut map = app.world.map().clone();
    map.mission = None;
    map.ai.clear();
    map.fog_of_war = false;
    map.terrain = None;
    map.spawns = [
        (33, 240, 240),
        (34, 336, 256),
        (3, 580, 580),
        (1, 210, 416),
        (13, 256, 416),
        (21, 400, 600),
        (13, 365, 416),
        (11, 210, 450),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, (unit, x, y))| Spawn {
        owner: if index == 6 { PlayerId(2) } else { PlayerId(0) },
        unit_type: UnitTypeId(unit),
        position: Position { x, y },
        ..Default::default()
    })
    .collect();
    app.world = World::new(rules, map, 42).unwrap();
    app.visuals = Visuals::new(&app.world);
    let commands = [
        Order::Train {
            entity: EntityId(1),
            unit_type: UnitTypeId(23),
        },
        Order::Load {
            entity: EntityId(4),
            target: EntityId(5),
        },
        Order::Load {
            entity: EntityId(8),
            target: EntityId(5),
        },
        Order::Move {
            entity: EntityId(6),
            target: Position { x: 450, y: 600 },
        },
    ];
    let outcomes = app
        .world
        .step(
            &commands
                .into_iter()
                .enumerate()
                .map(|(index, order)| Command {
                    tick: app.world.tick(),
                    player: PlayerId(0),
                    sequence: index as u64 + 1,
                    order,
                })
                .collect::<Vec<_>>(),
        )
        .unwrap();
    assert!(outcomes.iter().all(|outcome| outcome.rejection.is_none()));
    app.visuals.update(&app.world);
    assert_eq!(
        app.visuals.get(EntityId(2)).unwrap().action,
        visual::VisualAction::Production
    );
    assert_eq!(
        app.visuals.get(EntityId(6)).unwrap().action,
        visual::VisualAction::Move
    );
    for _ in 0..30 {
        let bunker = entity(&app, EntityId(5));
        if visual::garrison_frames(
            app.assets.as_ref().unwrap(),
            &app.world,
            &app.visuals,
            bunker,
        )
        .len()
            == 2
        {
            break;
        }
        app.world.step(&[]).unwrap();
        app.visuals.update(&app.world);
    }
    assert_eq!(
        visual::garrison_frames(
            app.assets.as_ref().unwrap(),
            &app.world,
            &app.visuals,
            entity(&app, EntityId(5))
        )
        .len(),
        2
    );
    app.camera = Camera {
        x: 300.25,
        y: 350.5,
        viewport: None,
        zoom: 1.5,
    };
    let size = [1100, 760];
    let before = app.world.state_hash();
    app.selected = BTreeSet::from([EntityId(5)]);
    let firing = capture_at(&app, "commands-bunker-fire", size, None, None, 0);
    app.visuals
        .show_command_feedback(visual::CommandTarget::Entity(EntityId(7)));
    let flashed = capture_at(&app, "commands-attack-target", size, None, None, 0);
    assert_ne!(firing, flashed);
    app.visuals
        .advance_effects(Duration::from_millis(100), app.assets.as_ref());
    let hidden = capture_at(&app, "commands-attack-off", size, None, None, 0);
    assert_eq!(firing, hidden);
    app.visuals
        .show_command_feedback(visual::CommandTarget::Ground(Position { x: 400, y: 350 }));
    let ground = capture_at(&app, "commands-move-target", size, None, None, 0);
    assert_ne!(firing, ground);
    let node = app
        .world
        .state()
        .resources
        .iter()
        .find(|node| node.kind == "gas")
        .unwrap();
    app.selected.clear();
    app.selected_resource = Some(node.id);
    app.camera.x = f64::from(node.position.x);
    app.camera.y = f64::from(node.position.y);
    capture_at(&app, "commands-resource-inspection", size, None, None, 0);
    assert_eq!(app.world.state_hash(), before);
}
