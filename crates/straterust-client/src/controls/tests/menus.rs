use super::*;
use crate::view::BuildButton;
use straterust_engine::sim::{GarrisonStats, MovementClass, Spawn};

#[test]
#[ignore = "requires STRATERUST_WARCRAFT2 pointing to a refreshed retail import"]
fn warcraft2_native_tankers_build_then_gather_and_reject_occupied_platforms() {
    use straterust_engine::{
        map::{BUILDABLE, Terrain, WALKABLE, WATER},
        sim::{Footprint, Position, ResourceSpawn},
    };
    let root = std::path::PathBuf::from(std::env::var_os("STRATERUST_WARCRAFT2").unwrap());
    for (race, folder) in [(0, "human"), (1, "orc")] {
        let directory = root.join(folder).join("mission14");
        let package = Package::load(&directory).unwrap();
        let mut app = App::new(
            &package,
            crate::Config::default(),
            read_ron(&directory.join("presentation.ron")).unwrap(),
            straterust_engine::assets::AssetPack::load(&directory).unwrap(),
            None,
        )
        .unwrap();
        app.simulation = None;
        for name in [
            "move",
            "stop",
            "hold",
            "patrol",
            "attack",
            "rally",
            "gather",
            "repair",
            "build",
            "advanced-build",
            "unload",
            "cancel",
            "back",
        ] {
            assert!(
                app.assets
                    .as_ref()
                    .unwrap()
                    .ui_image(&format!("command.{name}"))
                    .is_some(),
                "missing {name} command artwork"
            );
        }
        let original = package.world(7).unwrap();
        let mut rules = original.rules().clone();
        rules.victory = false;
        rules.starting_resources = ["gold", "wood", "oil"]
            .into_iter()
            .map(|kind| straterust_engine::sim::ResourceAmount {
                kind: kind.into(),
                amount: 10000,
            })
            .collect();
        let mut map = original.map().clone();
        map.id = "native-tanker-buttons".into();
        map.width = 1024;
        map.height = 1024;
        map.fog_of_war = false;
        map.ai.clear();
        map.mission = None;
        map.creation.clear();
        map.initial_explored.clear();
        let tanker = UnitTypeId(27 + race);
        let platform = UnitTypeId(87 + race);
        let target = Position { x: 800, y: 256 };
        map.spawns = vec![
            Spawn {
                unit_type: tanker,
                position: Position { x: 608, y: 256 },
                ..Default::default()
            },
            Spawn {
                unit_type: UnitTypeId(73 + race),
                position: Position { x: 512, y: 256 },
                ..Default::default()
            },
        ];
        map.resources = vec![ResourceSpawn {
            terrain_corners: None,
            kind: "oil".into(),
            position: target,
            footprint: Footprint {
                width: 96,
                height: 96,
            },
            amount: 10000,
            requires_extractor: true,
        }];
        map.terrain = Some(Terrain {
            cell_size: 32,
            columns: 32,
            rows: 32,
            flags: (0..1024)
                .map(|i| {
                    if i % 32 < 16 {
                        WALKABLE | BUILDABLE
                    } else {
                        WATER
                    }
                })
                .collect(),
        });
        map.start_locations.clear();
        app.world = World::new(rules, map, 7).unwrap();
        app.initial_world = app.world.clone();
        app.selected = [EntityId(1)].into();
        let buttons = app.buttons();
        let build = buttons
            .iter()
            .find(|b| b.action == Action::Build(platform))
            .unwrap();
        assert_eq!((build.slot, build.key.as_str()), (3, "B"));
        assert!(build.disabled.is_none());
        assert!(!buttons.iter().any(|b| b.action == Action::BuildMenu));
        assert_eq!(
            buttons
                .iter()
                .map(|b| b.slot)
                .collect::<BTreeSet<_>>()
                .len(),
            buttons.len()
        );
        app.activate(build.action).unwrap();
        assert_eq!(app.target_mode, Some(TargetMode::Build(platform)));
        app.targeting_click(target).unwrap();
        step(&mut app);
        assert!(
            matches!(app.recorded.last().unwrap().order, Order::Build { unit_type, .. } if unit_type == platform)
        );
        for _ in 0..200 {
            if app
                .world
                .state()
                .entities
                .iter()
                .any(|e| e.unit_type == platform)
            {
                break;
            }
            step(&mut app);
        }
        assert!(
            app.world
                .state()
                .entities
                .iter()
                .any(|e| e.unit_type == platform
                    && e.position == target
                    && e.construction.is_some())
        );
        let original_resources = app.world.resource_balance(PlayerId(0), "oil");
        for _ in 0..4000 {
            if app
                .world
                .state()
                .entities
                .iter()
                .any(|e| e.unit_type == platform && e.construction.is_none())
            {
                break;
            }
            step(&mut app);
        }
        assert!(
            app.world
                .state()
                .entities
                .iter()
                .any(|e| e.unit_type == platform && e.construction.is_none())
        );
        assert_eq!(
            app.world
                .state()
                .entities
                .iter()
                .find(|e| e.id == EntityId(1))
                .unwrap()
                .order,
            straterust_engine::sim::UnitOrder::Gather {
                resource: straterust_engine::sim::ResourceId(1)
            }
        );
        for _ in 0..1500 {
            if app.world.resource_balance(PlayerId(0), "oil") > original_resources {
                break;
            }
            step(&mut app);
        }
        assert!(
            app.world.resource_balance(PlayerId(0), "oil") > original_resources,
            "{folder} tanker must deliver oil after construction without a Gather click"
        );

        let mut occupied = app.world.map().clone();
        occupied.spawns.push(Spawn {
            owner: PlayerId(1),
            unit_type: UnitTypeId(88 - race),
            position: target,
            ..Default::default()
        });
        app.world = World::new(app.world.rules().clone(), occupied, 7).unwrap();
        app.target_mode = Some(TargetMode::Build(platform));
        app.camera = crate::view::Camera {
            x: f64::from(target.x),
            y: f64::from(target.y),
            viewport: None,
            zoom: 1.0,
        };
        let screen = app.camera.world_to_screen(
            f64::from(target.x),
            f64::from(target.y),
            app.logical_size(),
        );
        app.cursor = winit::dpi::PhysicalPosition::new(screen[0], screen[1]);
        assert_eq!(
            app.placement(),
            Some((platform, target, false)),
            "enemy platform must invalidate the native placement preview"
        );
        let before = app.recorded.len();
        app.targeting_click(target).unwrap();
        assert_eq!(
            app.recorded.len(),
            before,
            "invalid placement must not issue a Build command"
        );
    }
}

#[test]
fn single_structure_builder_can_expose_a_direct_authored_button_and_target_it() {
    let mut app = demo();
    let mut rules = app.world.rules().clone();
    let worker = rules
        .units
        .iter_mut()
        .find(|u| u.id == UnitTypeId(2))
        .unwrap();
    worker.builds = vec![UnitTypeId(4)];
    worker.repairs.clear();
    app.world = World::new(rules, app.world.map().clone(), 0).unwrap();
    app.selected = [EntityId(2)].into();
    app.presentation.unit_commands.insert(
        UnitTypeId(2),
        [("build".into(), Some(3)), ("attack".into(), None)].into(),
    );
    app.presentation.build_buttons.insert(
        UnitTypeId(4),
        BuildButton {
            advanced: false,
            slot: 3,
            key: "P".into(),
        },
    );
    let buttons = app.buttons();
    assert!(!buttons.iter().any(|b| b.action == Action::BuildMenu));
    let build = buttons
        .iter()
        .find(|b| b.action == Action::Build(UnitTypeId(4)))
        .unwrap();
    assert_eq!(build.slot, 3);
    assert_eq!(build.key, "P");
    assert_eq!(
        buttons
            .iter()
            .map(|b| b.slot)
            .collect::<BTreeSet<_>>()
            .len(),
        buttons.len()
    );
    app.activate(build.action).unwrap();
    assert_eq!(app.target_mode, Some(TargetMode::Build(UnitTypeId(4))));
}

#[test]
fn basic_and_advanced_structures_use_separate_menus_with_unique_slots() {
    let mut app = demo();
    let mut rules = app.world.rules().clone();
    let mut factory = rules
        .units
        .iter()
        .find(|u| u.id == UnitTypeId(5))
        .unwrap()
        .clone();
    factory.id = UnitTypeId(6);
    rules.units.push(factory);
    rules
        .units
        .iter_mut()
        .find(|u| u.id == UnitTypeId(2))
        .unwrap()
        .builds
        .push(UnitTypeId(6));
    app.world = World::new(rules, app.world.map().clone(), 0).unwrap();
    for (unit, slot, key) in [(3, 0, "C"), (4, 1, "S"), (5, 3, "B"), (6, 0, "F")] {
        app.presentation.build_buttons.insert(
            UnitTypeId(unit),
            BuildButton {
                advanced: unit == 6,
                slot,
                key: key.into(),
            },
        );
    }
    app.selected = BTreeSet::from([EntityId(2)]);
    let buttons = app.buttons();
    assert!(
        buttons
            .iter()
            .any(|b| b.action == Action::AdvancedBuildMenu && b.slot == 7)
    );
    let slots: BTreeSet<_> = buttons.iter().map(|b| b.slot).collect();
    assert_eq!(slots.len(), buttons.len());
    app.activate(Action::BuildMenu).unwrap();
    let builds: Vec<_> = app
        .buttons()
        .into_iter()
        .filter_map(|b| match b.action {
            Action::Build(id) => Some(id),
            _ => None,
        })
        .collect();
    assert_eq!(builds, vec![UnitTypeId(3), UnitTypeId(4), UnitTypeId(5)]);
    app.activate(Action::Back).unwrap();
    app.activate(Action::AdvancedBuildMenu).unwrap();
    let buttons = app.buttons();
    assert_eq!(buttons.len(), 2);
    assert_eq!(buttons[0].action, Action::Build(UnitTypeId(6)));
    assert_eq!(buttons[0].key, "F");
    assert_eq!(buttons[1].action, Action::Back);
}

#[test]
fn targeted_unload_all_waits_for_a_destination_and_supports_cancellation() {
    let mut app = demo();
    let mut rules = app.world.rules().clone();
    rules.victory = false;
    let mut transport = rules
        .units
        .iter()
        .find(|u| u.id == UnitTypeId(1))
        .unwrap()
        .clone();
    transport.id = UnitTypeId(6);
    transport.movement_class = MovementClass::Air;
    transport.weapon = None;
    transport.garrison = Some(GarrisonStats {
        boarding_range: 1,
        capacity: 4,
        passengers: vec![UnitTypeId(1), UnitTypeId(2)],
        attackers: vec![],
        range_bonus: 0,
        unload_ticks: 15,
    });
    rules.units.push(transport);
    let mut map = app.world.map().clone();
    map.spawns.retain(|s| s.owner == PlayerId(0));
    for (unit, x, y) in [(1, 320, 300), (6, 320, 340)] {
        map.spawns.push(Spawn {
            unit_type: UnitTypeId(unit),
            position: Position { x, y },
            ..Default::default()
        });
    }
    app.world = World::new(rules, map, 0).unwrap();
    app.selected = BTreeSet::from([EntityId(4)]);
    assert!(
        app.buttons()
            .iter()
            .any(|b| b.action == Action::Unload && b.disabled.is_some())
    );
    for passenger in [EntityId(2), EntityId(3)] {
        app.issue(Order::Load {
            entity: passenger,
            target: EntityId(4),
        })
        .unwrap();
    }
    for _ in 0..100 {
        step(&mut app);
    }
    assert_eq!(
        crate::selection::panel_members(&app.world, &app.selected)
            .1
            .len(),
        2
    );
    let button = app
        .buttons()
        .into_iter()
        .find(|b| b.action == Action::Unload)
        .unwrap();
    assert_eq!(button.label, "Unload All");
    assert_eq!(button.key, "U");
    assert!(button.disabled.is_none());
    app.activate(Action::Unload).unwrap();
    assert_eq!(app.target_mode, Some(TargetMode::Unload));
    assert!(
        app.buttons()
            .iter()
            .all(|button| button.action == Action::Cancel)
    );
    let commands = app.recorded.len();
    step(&mut app);
    assert_eq!(app.recorded.len(), commands);
    assert_eq!(
        crate::selection::panel_members(&app.world, &app.selected)
            .1
            .len(),
        2
    );
    app.targeting_click(Position { x: -1, y: -1 }).unwrap();
    assert_eq!(app.target_mode, Some(TargetMode::Unload));
    assert_eq!(app.recorded.len(), commands);
    app.activate(Action::Cancel).unwrap();
    assert_eq!(app.target_mode, None);
    app.activate(Action::Unload).unwrap();
    app.contextual_order(Position { x: 600, y: 600 }).unwrap();
    assert_eq!(app.target_mode, None);
    assert_eq!(app.recorded.len(), commands);
    app.activate(Action::Unload).unwrap();
    let destination = Position { x: 600, y: 600 };
    app.targeting_click(destination).unwrap();
    assert_eq!(app.target_mode, None);
    assert_eq!(
        app.visuals.command_feedback().unwrap().target,
        crate::visual::CommandTarget::Ground(destination)
    );
    step(&mut app);
    assert_eq!(
        crate::selection::panel_members(&app.world, &app.selected)
            .1
            .len(),
        2
    );
    for _ in 0..200 {
        step(&mut app);
    }
    assert!(
        crate::selection::panel_members(&app.world, &app.selected)
            .1
            .is_empty()
    );
    assert_eq!(app.selected, BTreeSet::from([EntityId(4)]));
    assert_eq!(
        app.world
            .state()
            .entities
            .iter()
            .find(|entity| entity.id == EntityId(4))
            .unwrap()
            .position,
        destination
    );
}
