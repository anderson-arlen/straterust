//! Short original-art harvest/deposit and structure-menu review.
use super::*;
use straterust_engine::sim::{Footprint, Order, ResourceAmount, ResourceId, ResourceSpawn, Spawn};

#[test]
#[ignore = "requires private mission 5 assets; one mineral and gas round trip"]
fn native_carried_resources_and_structure_menus() {
    let directory = PathBuf::from(std::env::var_os("STRATERUST_ASSET_PACKAGE").unwrap());
    let package = Package::load(&directory).unwrap();
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
    app.mission_ui = None;
    let mut rules = app.world.rules().clone();
    rules.victory = false;
    let mut map = app.world.map().clone();
    map.mission = None;
    map.ai.clear();
    map.terrain = None;
    map.fog_of_war = false;
    map.initial_explored.clear();
    map.start_locations.clear();
    map.spawns = [(3, 512, 512), (2, 704, 512), (14, 1024, 736)]
        .into_iter()
        .map(|(unit, x, y)| Spawn {
            unit_type: UnitTypeId(unit),
            position: Position { x, y },
            ..Default::default()
        })
        .collect();
    map.resources = [
        (
            "minerals",
            Position { x: 1024, y: 512 },
            Footprint {
                width: 64,
                height: 32,
            },
            false,
        ),
        (
            "gas",
            Position { x: 1024, y: 736 },
            Footprint {
                width: 128,
                height: 64,
            },
            true,
        ),
    ]
    .into_iter()
    .map(
        |(kind, position, footprint, requires_extractor)| ResourceSpawn {
            kind: kind.into(),
            position,
            footprint,
            amount: 1500,
            requires_extractor,
        },
    )
    .collect();
    for (kind, resource) in [("minerals", ResourceId(1)), ("gas", ResourceId(2))] {
        app.world = World::new(rules.clone(), map.clone(), 42).unwrap();
        app.visuals = Visuals::new(&app.world);
        app.selected = BTreeSet::from([EntityId(2)]);
        app.camera.x = 900.0;
        app.camera.y = 560.0;
        let assets = app.assets.as_ref().unwrap();
        assert!(
            visual::carried_resource_frame(
                assets,
                entity(&app, EntityId(2)),
                app.visuals.get(EntityId(2)),
                &app.world
            )
            .is_none()
        );
        let starting = app.world.resource_balance(PlayerId(0), kind);
        app.issue(Order::Gather {
            entity: EntityId(2),
            resource,
        })
        .unwrap();
        until(
            &mut app,
            |app| entity(app, EntityId(2)).cargo.is_some(),
            500,
        );
        assert_eq!(
            entity(&app, EntityId(2)).cargo,
            Some(ResourceAmount {
                kind: kind.into(),
                amount: 8
            })
        );
        let assets = app.assets.as_ref().unwrap();
        let worker = entity(&app, EntityId(2));
        let mut observed = *app.visuals.get(worker.id).unwrap();
        for action in [
            VisualAction::Idle,
            VisualAction::Move,
            VisualAction::Work,
            VisualAction::Attack,
        ] {
            observed.action = action;
            observed.since_tick = app.world.tick().0;
            for facing in 0..32 {
                observed.facing = facing;
                let frame =
                    visual::carried_resource_frame(assets, worker, Some(&observed), &app.world)
                        .unwrap();
                assert!(
                    frame.anchor.iter().all(|coordinate| coordinate.abs() <= 40),
                    "cargo attachment must use the body origin: {action:?}/{facing}: {:?}",
                    frame.anchor
                );
                assert!(
                    frame
                        .image
                        .rgba
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .any(|pixel| pixel[3] > 0)
                );
            }
        }
        let mut partial = worker.clone();
        partial.cargo.as_mut().unwrap().amount = 2;
        if kind == "gas" {
            let full = visual::carried_resource_frame(assets, worker, Some(&observed), &app.world)
                .unwrap();
            let small =
                visual::carried_resource_frame(assets, &partial, Some(&observed), &app.world)
                    .unwrap();
            assert_ne!(
                full.image.rgba, small.image.rgba,
                "depleted gas has a different tank"
            );
        }
        partial.garrisoned_in = Some(EntityId(1));
        assert!(
            visual::carried_resource_frame(assets, &partial, Some(&observed), &app.world).is_none()
        );
        app.issue(Order::Stop {
            entity: EntityId(2),
        })
        .unwrap();
        advance(&mut app);
        assert!(
            visual::carried_resource_frame(
                app.assets.as_ref().unwrap(),
                entity(&app, EntityId(2)),
                app.visuals.get(EntityId(2)),
                &app.world
            )
            .is_some(),
            "Stop must not discard cargo art"
        );
        app.issue(Order::Move {
            entity: EntityId(2),
            target: Position { x: 800, y: 640 },
        })
        .unwrap();
        for _ in 0..10 {
            advance(&mut app);
        }
        assert!(
            visual::carried_resource_frame(
                app.assets.as_ref().unwrap(),
                entity(&app, EntityId(2)),
                app.visuals.get(EntityId(2)),
                &app.world
            )
            .is_some(),
            "a manual move retains cargo"
        );
        capture(&app, &format!("carried-{kind}"), [1280, 800], None);
        app.issue(Order::Gather {
            entity: EntityId(2),
            resource,
        })
        .unwrap();
        until(
            &mut app,
            |app| app.world.resource_balance(PlayerId(0), kind) > starting,
            500,
        );
        assert_eq!(app.world.resource_balance(PlayerId(0), kind), starting + 8);
        assert!(
            visual::carried_resource_frame(
                app.assets.as_ref().unwrap(),
                entity(&app, EntityId(2)),
                app.visuals.get(EntityId(2)),
                &app.world
            )
            .is_none(),
            "deposit removes cargo art"
        );
    }
    // Enable the supported structures in this review, independent of mission PUNI.
    map.creation.clear();
    app.world = World::new(rules, map, 42).unwrap();
    app.visuals = Visuals::new(&app.world);
    app.activate(Action::BuildMenu).unwrap();
    let basic = app.buttons();
    for (unit, slot, key) in [(36, 5, "T"), (12, 6, "A"), (13, 7, "U")] {
        let button = basic
            .iter()
            .find(|button| button.action == Action::Build(UnitTypeId(unit)))
            .unwrap();
        assert_eq!(button.slot, slot);
        assert_eq!(button.key, key);
    }
    assert!(
        !basic
            .iter()
            .any(|button| button.action == Action::Build(UnitTypeId(16)))
    );
    capture(&app, "carried-basic-menu", [1280, 800], None);
    app.activate(Action::AdvancedBuildMenu).unwrap();
    assert!(
        !app.buttons()
            .iter()
            .any(|button| button.action == Action::Build(UnitTypeId(36)))
    );
    for (unit, slot, key) in [(32, 0, "F"), (33, 1, "S")] {
        let buttons = app.buttons();
        let button = buttons
            .iter()
            .find(|button| button.action == Action::Build(UnitTypeId(unit)))
            .unwrap();
        assert_eq!(button.slot, slot);
        assert_eq!(button.key, key);
    }
}
