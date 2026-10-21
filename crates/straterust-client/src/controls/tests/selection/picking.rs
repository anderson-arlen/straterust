use super::*;
use straterust_engine::{
    assets::{ClipFrame, ClipKind, Image, SpriteClip},
    sim::Spawn,
};

fn sprite_app(flip_x: bool, offset: [i16; 2]) -> App {
    let mut app = demo();
    let mut rules = app.world.rules().clone();
    rules.victory = false;
    for unit in &mut rules.units {
        unit.footprint = straterust_engine::map::Footprint {
            width: 24,
            height: 24,
        };
    }
    let mut map = app.world.map().clone();
    map.spawns = vec![
        Spawn {
            owner: PlayerId(1),
            unit_type: UnitTypeId(2),
            position: Position { x: 320, y: 320 },
            ..Default::default()
        },
        // A neighbor's ground footprint lies directly beneath the other body.
        Spawn {
            owner: PlayerId(0),
            unit_type: UnitTypeId(1),
            position: Position { x: 296, y: 280 },
            ..Default::default()
        },
    ];
    app.world = World::new(rules, map, 42).unwrap();
    app.visuals = crate::visual::Visuals::new(&app.world);
    app.presentation.unit_radius = 1;
    add_resource_art(&mut app);
    let assets = app.assets.as_mut().unwrap();
    assets.manifest.unit_type = UnitTypeId(2);
    assets.manifest.anchor = [48, 64];
    assets.manifest.clips = vec![SpriteClip {
        kind: ClipKind::Idle,
        directions: 1,
        frame_ms: 100,
        frames: vec![ClipFrame {
            frame: 0,
            flip_x,
            offset,
        }],
        key_steps: vec![],
        loop_start: None,
        progress_starts: vec![],
    }];
    let mut image = Image {
        width: 96,
        height: 96,
        rgba: vec![0; 96 * 96 * 4],
    };
    // A visible head/wing above and to the side of the collision footprint;
    // the remaining sprite canvas is transparent.
    for y in 16..32 {
        for x in 16..32 {
            image.rgba[(y * 96 + x) * 4..(y * 96 + x) * 4 + 4]
                .copy_from_slice(&[80, 180, 240, 255]);
        }
    }
    assets.frames = vec![image];
    app.camera.x = 320.0;
    app.camera.y = 320.0;
    app
}

#[test]
fn sprite_picking_selects_and_targets_visible_frames_at_each_zoom_and_dpi() {
    for (flip, offset, point) in [
        (false, [0, 0], Position { x: 296, y: 280 }),
        (true, [5, -7], Position { x: 348, y: 273 }),
    ] {
        let mut app = sprite_app(flip, offset);
        let before = app.world.state_hash();
        for zoom in [0.4, 1.0, 2.0] {
            app.camera.zoom = zoom;
            for dpi in [1.0, 1.5, 2.0] {
                let size = [1280.0, 800.0];
                let screen =
                    app.camera
                        .world_to_screen(f64::from(point.x), f64::from(point.y), size);
                let physical = screen.map(|x| x * dpi);
                let logical = physical.map(|x| x / dpi);
                app.select_screen(logical, logical, size, Instant::now());
                assert_eq!(
                    app.selected,
                    BTreeSet::from([EntityId(1)]),
                    "visible body, flip={flip}, zoom={zoom}, dpi={dpi}"
                );
                assert_eq!(app.entity_at(point), Some(EntityId(1)));
                app.selected = BTreeSet::from([EntityId(2)]);
                app.contextual_order(point).unwrap();
                assert!(matches!(
                    app.recorded.last().unwrap().order,
                    Order::Attack {
                        entity: EntityId(2),
                        target: EntityId(1)
                    }
                ));
            }
        }
        assert_eq!(
            app.world.state_hash(),
            before,
            "picking cannot change simulation or collision geometry"
        );
    }
}

#[test]
fn sprite_picking_padding_is_in_screen_pixels_and_excludes_blank_canvas_and_fog() {
    let mut app = sprite_app(false, [0, 0]);
    for zoom in [0.4, 1.0, 2.0] {
        app.camera.zoom = zoom;
        assert_eq!(
            app.entity_at(Position {
                x: 288 - (4.0 / zoom) as i32,
                y: 280
            }),
            Some(EntityId(1)),
            "four screen pixels from visible art"
        );
        assert_ne!(
            app.entity_at(Position {
                x: 288 - (6.0 / zoom) as i32,
                y: 280
            }),
            Some(EntityId(1)),
            "outside the five-pixel margin"
        );
        assert_eq!(
            app.entity_at(Position { x: 350, y: 260 }),
            None,
            "transparent sprite canvas is not clickable"
        );
    }
    let mut map = app.world.map().clone();
    map.fog_of_war = true;
    map.spawns[0].position = Position { x: 1280, y: 256 };
    app.world = World::new(app.world.rules().clone(), map, 42).unwrap();
    app.visuals = crate::visual::Visuals::new(&app.world);
    assert_eq!(
        app.entity_at(Position { x: 1256, y: 216 }),
        None,
        "hidden bodies cannot be picked through fog"
    );
}
