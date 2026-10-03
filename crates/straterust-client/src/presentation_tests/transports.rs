use super::*;
use crate::controls::TargetMode;
use straterust_engine::sim::Spawn;

#[test]
#[ignore = "requires private mission 5 art; targeted Dropship cursor review"]
fn native_dropship_unload_target_cursor() {
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
    let mut map = app.world.map().clone();
    map.ai.clear();
    map.mission = None;
    map.terrain = None;
    map.fog_of_war = false;
    map.initial_explored.clear();
    map.resources.clear();
    map.spawns = [(53, 384, 384), (1, 400, 400)]
        .into_iter()
        .map(|(unit, x, y)| Spawn {
            unit_type: UnitTypeId(unit),
            position: Position { x, y },
            ..Default::default()
        })
        .collect();
    app.world = straterust_engine::sim::World::new(app.world.rules().clone(), map, 42).unwrap();
    app.visuals = visual::Visuals::new(&app.world);
    app.issue(straterust_engine::sim::Order::Load {
        entity: EntityId(2),
        target: EntityId(1),
    })
    .unwrap();
    until(
        &mut app,
        |app| entity(app, EntityId(2)).garrisoned_in.is_some(),
        80,
    );
    app.selected = BTreeSet::from([EntityId(1)]);
    app.camera.x = 384.0;
    app.camera.y = 384.0;
    app.cursor = winit::dpi::PhysicalPosition::new(640.0, 300.0);
    let before = capture_at(&app, "dropship-cursor-before", [1280, 800], None, None, 0);
    app.activate(Action::Unload).unwrap();
    assert_eq!(app.target_mode, Some(TargetMode::Unload));
    let target = capture_at(&app, "dropship-cursor-target", [1280, 800], None, None, 0);
    let cursor_pixels = |pixels: &[u32]| {
        (280..321)
            .flat_map(|y| (620..661).map(move |x| pixels[y * 1280 + x]))
            .collect::<Vec<_>>()
    };
    assert_ne!(
        cursor_pixels(&before),
        cursor_pixels(&target),
        "native target cursor must change at the mouse position"
    );
    assert!(entity(&app, EntityId(2)).garrisoned_in.is_some());
}
