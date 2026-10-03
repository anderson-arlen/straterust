use super::*;

#[test]
fn emerging_enemies_do_not_appear_in_the_world_or_minimap_before_emergence() {
    use straterust_engine::sim::{
        Mission, MissionAction, MissionCondition, MissionLocation, MissionTrigger, Spawn,
    };
    let package = Package::load(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/fixtures"),
    )
    .unwrap();
    let original = package.world(42).unwrap();
    let mut rules = original.rules().clone();
    rules.units[0].vision_range = 128;
    rules.units[0].acquisition_range = Some(128);
    rules.units[0].unburrow_ticks = 3;
    rules.units[0].speed = 0;
    rules.units[0].weapon = Some(straterust_engine::sim::Weapon {
        damage: 6,
        range: 20,
        cooldown: 5,
        cooldown_jitter: None,
        targets_air: false,
        damage_kind: Default::default(),
        splash: None,
        strikes: vec![],
    });
    let mut map = original.map().clone();
    map.fog_of_war = false;
    map.spawns = vec![
        Spawn {
            owner: PlayerId(0),
            unit_type: UnitTypeId(1),
            position: Position { x: 400, y: 400 },
            ..Default::default()
        },
        Spawn {
            owner: PlayerId(1),
            unit_type: UnitTypeId(1),
            position: Position { x: 460, y: 400 },
            burrowed: true,
            ..Default::default()
        },
    ];
    map.mission = Some(Mission {
        schema_version: 1,
        player: PlayerId(0),
        rescuable_players: vec![],
        rescuers: vec![PlayerId(0)],
        alliances: vec![],
        poll_ticks: 1,
        wait_step_ms: 1,
        locations: vec![MissionLocation {
            excluded_elevations: 0,
            left: 0,
            top: 0,
            right: map.width,
            bottom: map.height,
        }],
        triggers: vec![MissionTrigger {
            conditions: vec![MissionCondition::Switch {
                index: 0,
                set: true,
            }],
            actions: vec![MissionAction::Victory],
        }],
    });
    let mut world = World::new(rules, map, 42).unwrap();
    let presentation = Presentation::default();
    let selected = BTreeSet::new();
    for (tick, visible) in [(0, false), (1, false), (2, false), (3, false), (4, true)] {
        if tick != 0 {
            world.step(&[]).unwrap();
        }
        let visuals = Visuals::new(&world);
        let view = View {
            world: &world,
            visuals: &visuals,
            cursor: [-1.0, -1.0],
            targeting: false,
            presentation: &presentation,
            assets: None,
            media: None,
            speaking: None,
            mission: None,
            animation_ms: 0,
            portrait_ms: 0,
            camera: Camera {
                x: 430.0,
                y: 400.0,
                zoom: 1.0,
            },
            selected: &selected,
            selected_resource: None,
            drag_box: None,
            paused: true,
            playback: false,
            status: "",
            buttons: &[],
            help: "",
            placement: None,
            ending_hint: "F5 RESTART",
        };
        let mut pixels = vec![0; 800 * 600];
        view.draw(&mut pixels, 800, 600, 1.0);
        assert_eq!(
            pixels.contains(&presentation.opposing),
            visible,
            "world sprite visibility at tick {tick}"
        );
        assert_eq!(
            pixels.contains(&0xe06751),
            visible,
            "minimap enemy visibility at tick {tick}"
        );
    }
}

#[test]
fn group_status_panel_paints_all_members_and_health_at_each_scale() {
    use straterust_engine::{
        assets::UiImage,
        sim::{Spawn, UnitType},
    };
    let package = Package::load(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/fixtures"),
    )
    .unwrap();
    let original = package.world(42).unwrap();
    let mut rules = original.rules().clone();
    rules.units = (1..=3)
        .map(|id| UnitType {
            id: UnitTypeId(id),
            max_hp: 100,
            ..Default::default()
        })
        .collect();
    let mut map = original.map().clone();
    map.spawns = (0..12)
        .map(|slot| Spawn {
            owner: PlayerId(0),
            unit_type: UnitTypeId(slot % 3 + 1),
            position: Position {
                x: 100 + i32::from(slot) * 40,
                y: 100,
            },
            hp_percent: Some([100, 50, 20][usize::from(slot % 3)]),
            ..Default::default()
        })
        .collect();
    let world = World::new(rules, map, 0).unwrap();
    let selected: BTreeSet<_> = world
        .state()
        .entities
        .iter()
        .map(|entity| entity.id)
        .chain([EntityId(999)])
        .collect();
    let reference = ImageRef {
        file: "synthetic.srim".into(),
        blake3: "0".repeat(64),
    };
    let image = |size: u32, color: [u8; 4]| Image {
        width: size,
        height: size,
        rgba: color.repeat((size * size) as usize),
    };
    let assets = AssetPack {
        manifest: AssetManifest {
            schema_version: 1,
            terrain: reference.clone(),
            terrain_grid: None,
            unit_type: UnitTypeId(1),
            unit_name: "Synthetic".into(),
            frame_ms: 100,
            anchor: [0, 0],
            frames: vec![reference],
            clips: vec![],
            extra_units: vec![],
            resources: vec![],
            ui: vec![],
            map_images: vec![],
            scan_effect: None,
            projectiles: Vec::new(),
            damage_effects: None,
            gas_effects: None,
            creep: None,
            indicators: None,
        },
        terrain: image(1, [0, 0, 0, 255]),
        frames: vec![image(1, [0, 0, 0, 0])],
        extra_units: vec![],
        resources: vec![],
        map_images: vec![],
        scan_effect: None,
        projectiles: Vec::new(),
        damage_effects: None,
        gas_effects: None,
        creep: None,
        indicators: None,
        ui: vec![
            UiImage {
                key: "console".into(),
                image: Image {
                    width: 640,
                    height: 480,
                    rgba: vec![0; 640 * 480 * 4],
                },
            },
            UiImage {
                key: "groupwire.1".into(),
                image: image(32, [250, 10, 10, 255]),
            },
            UiImage {
                key: "groupwire.2".into(),
                image: image(32, [10, 10, 250, 255]),
            },
            UiImage {
                key: "wireframe.3".into(),
                image: image(64, [250, 10, 250, 255]),
            },
        ],
    };
    let visuals = Visuals::new(&world);
    let presentation = Presentation::default();
    let mut view = View {
        world: &world,
        visuals: &visuals,
        cursor: [-1.0, -1.0],
        targeting: false,
        presentation: &presentation,
        assets: Some(&assets),
        media: None,
        speaking: None,
        mission: None,
        animation_ms: 0,
        portrait_ms: 0,
        camera: Camera {
            x: 420.0,
            y: 420.0,
            zoom: 1.0,
        },
        selected: &selected,
        selected_resource: None,
        drag_box: None,
        paused: true,
        playback: false,
        status: "",
        buttons: &[],
        help: "",
        placement: None,
        ending_hint: "F5 RESTART",
    };
    let hash = world.state_hash();
    for (width, height, dpi) in [
        (640, 480, 1.0),
        (800, 600, 1.0),
        (1280, 800, 1.0),
        (1600, 1200, 2.0),
    ] {
        let size = [f64::from(width) / dpi, f64::from(height) / dpi];
        let mut pixels = vec![0; (width * height) as usize];
        for native in [true, false] {
            view.assets = if native { Some(&assets) } else { None };
            view.draw(&mut pixels, width, height, dpi);
            for slot in 0..12 {
                let [x, y, w, h] = selection_rect(slot, size, native);
                assert!(x >= 0.0 && y >= size[1] - FOOTER && x + w <= size[0] && y + h <= size[1]);
                let color = if native {
                    [0xfa0a0a, 0x0a0afa, 0xfa0afa][slot % 3]
                } else {
                    presentation.friendly
                };
                let left = (x * dpi).ceil() as usize;
                let right = ((x + w) * dpi).floor() as usize;
                let top = (y * dpi).ceil() as usize;
                let bottom = ((y + h) * dpi).floor() as usize;
                assert!(
                    (top..bottom).any(|row| pixels
                        [row * width as usize + left..row * width as usize + right]
                        .contains(&color)),
                    "missing member {slot} at {size:?}, native={native}"
                );
                let health = [0x76bd67, 0xd4ac63, 0xdb655b][slot % 3];
                assert!(
                    (top..bottom).any(|row| pixels
                        [row * width as usize + left..row * width as usize + right]
                        .contains(&health)),
                    "missing health indicator {slot}"
                );
            }
            if native {
                let scene = view.scene(width, height, dpi);
                let group_images = scene.commands.iter().filter(|command| matches!(command,
                    crate::gpu::Draw::Image { image, .. } if assets.ui[1..].iter().any(|entry| std::ptr::eq(*image, &entry.image)))).count();
                assert_eq!(group_images, 12, "GPU also receives the entire group");
            }
        }
    }
    assert_eq!(world.state_hash(), hash);
}

#[test]
fn flying_shadow_is_translucent_at_ground_anchor_and_below_every_body() {
    use straterust_engine::{
        content::read_ron,
        sim::{Map, MovementClass, Rules},
    };
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/fixtures");
    let mut map: Map = read_ron(&directory.join("map.ron")).unwrap();
    let mut rules: Rules = read_ron(&directory.join("rules.ron")).unwrap();
    let mut flying = rules.units[0].clone();
    flying.id = UnitTypeId(99);
    flying.movement_class = MovementClass::Air;
    rules.units.push(flying);
    map.spawns.truncate(2);
    for spawn in &mut map.spawns {
        spawn.position = Position { x: 420, y: 420 };
    }
    map.spawns[1].unit_type = UnitTypeId(99);
    map.spawns[1].owner = PlayerId(1);
    let world = World::new(rules, map, 42).unwrap();
    let hash = world.state_hash();
    let visuals = Visuals::new(&world);
    for (zoom, scale) in [(1.0, 1.0), (2.0, 1.5)] {
        let view = View {
            world: &world,
            visuals: &visuals,
            cursor: [-1.0, -1.0],
            targeting: false,
            presentation: &Presentation::default(),
            assets: None,
            media: None,
            speaking: None,
            mission: None,
            animation_ms: 0,
            portrait_ms: 0,
            camera: Camera {
                x: 420.0,
                y: 420.0,
                zoom,
            },
            selected: &BTreeSet::new(),
            selected_resource: None,
            drag_box: None,
            paused: false,
            playback: false,
            status: "Shadow test",
            buttons: &[],
            help: "",
            placement: None,
            ending_hint: "F5 RESTART",
        };
        let scene = view.scene((800.0 * scale) as u32, (600.0 * scale) as u32, scale);
        let shadows: Vec<_> = scene
            .commands
            .iter()
            .enumerate()
            .filter_map(|(index, draw)| {
                if let crate::gpu::Draw::Image { image, rect, .. } = draw
                    && std::ptr::eq(*image, &*FLYING_SHADOW)
                {
                    Some((index, image, rect))
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(shadows.len(), 1, "ground units must not get flying shadows");
        let (index, image, rect) = shadows[0];
        assert!(
            image
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel[3] == 100)
        );
        assert!(
            image
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .all(|pixel| pixel[..3] == [0, 0, 0] && pixel[3] <= 100)
        );
        assert_eq!(rect[0] + rect[2] / 2.0, (400.0 * scale) as f32);
        assert_eq!(rect[1] + rect[3] / 2.0, (212.0 * scale) as f32);
        for color in [view.presentation.friendly, view.presentation.opposing] {
            assert!(scene.commands.iter().position(|draw| matches!(draw, crate::gpu::Draw::Rect { color: body, .. } if *body == color)).unwrap() > index);
        }
        assert_eq!(world.state_hash(), hash);
    }
}

#[test]
fn overlapping_units_draw_by_ground_position_instead_of_entity_id() {
    use straterust_engine::{
        content::read_ron,
        sim::{Map, PlayerId, Rules},
    };
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/fixtures");
    let mut map: Map = read_ron(&directory.join("map.ron")).unwrap();
    let rules: Rules = read_ron(&directory.join("rules.ron")).unwrap();
    map.spawns.truncate(2);
    map.spawns[0].position = Position { x: 420, y: 426 };
    map.spawns[1].position = Position { x: 420, y: 420 };
    map.spawns[1].owner = PlayerId(1);
    let world = World::new(rules, map, 42).unwrap();
    let art = Presentation::default();
    let view = View {
        world: &world,
        visuals: &Visuals::new(&world),
        cursor: [-1.0, -1.0],
        targeting: false,
        presentation: &art,
        assets: None,
        media: None,
        speaking: None,
        mission: None,
        animation_ms: 0,
        portrait_ms: 0,
        camera: Camera {
            x: 420.0,
            y: 420.0,
            zoom: 1.0,
        },
        selected: &BTreeSet::new(),
        selected_resource: None,
        drag_box: None,
        paused: false,
        playback: false,
        status: "Depth test",
        buttons: &[],
        help: "",
        placement: None,
        ending_hint: "F5 RESTART",
    };
    let mut pixels = vec![0; 800 * 600];
    view.draw(&mut pixels, 800, 600, 1.0);
    assert_eq!(
        pixels[((600.0 + HEADER - FOOTER) / 2.0 + 6.0) as usize * 800 + 404],
        art.friendly,
        "lower unit must cover the higher unit"
    );
}

#[test]
fn decomposing_native_corpses_stay_below_living_units_while_paused() {
    use straterust_engine::{
        assets::{ClipFrame, ClipKind, SpriteClip, SpriteManifest, SpritePack},
        content::read_ron,
        sim::{Map, Rules, Weapon},
    };
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/fixtures");
    let mut map: Map = read_ron(&directory.join("map.ron")).unwrap();
    let mut rules: Rules = read_ron(&directory.join("rules.ron")).unwrap();
    map.spawns.truncate(2);
    map.spawns[0].position = Position { x: 420, y: 420 };
    map.spawns[1].position = Position { x: 426, y: 420 };
    map.spawns[1].owner = PlayerId(1);
    map.spawns[1].unit_type = UnitTypeId(2);
    rules.units[0].weapon = Some(Weapon {
        targets_air: false,
        damage: 100,
        range: 100,
        cooldown: 10,
        cooldown_jitter: None,
        damage_kind: Default::default(),
        splash: None,
        strikes: vec![],
    });
    let mut world = World::new(rules, map, 42).unwrap();
    let mut visuals = Visuals::new(&world);
    world.step(&[]).unwrap();
    visuals.update(&world);
    assert_eq!(visuals.deaths().len(), 1);
    let reference = ImageRef {
        file: "synthetic.srim".into(),
        blake3: "0".repeat(64),
    };
    let frame = |width: u32, color: [u8; 4]| Image {
        width,
        height: width,
        rgba: color.repeat((width * width) as usize),
    };
    let assets = AssetPack {
        manifest: AssetManifest {
            schema_version: 1,
            terrain: reference.clone(),
            terrain_grid: None,
            unit_type: UnitTypeId(1),
            unit_name: "Living unit".into(),
            frame_ms: 100,
            anchor: [8, 8],
            frames: vec![reference.clone()],
            clips: vec![],
            extra_units: vec![],
            resources: vec![],
            ui: vec![],
            map_images: vec![],
            scan_effect: None,
            projectiles: Vec::new(),
            damage_effects: None,
            gas_effects: None,
            creep: None,
            indicators: None,
        },
        terrain: frame(1, [30, 40, 50, 255]),
        frames: vec![frame(16, [20, 100, 220, 255])],
        extra_units: vec![SpritePack {
            manifest: SpriteManifest {
                unit_type: UnitTypeId(2),
                unit_name: "Decomposing remains".into(),
                frame_ms: 500,
                anchor: [32, 32],
                frames: vec![reference.clone(), reference],
                clips: vec![SpriteClip {
                    kind: ClipKind::Death,
                    directions: 1,
                    frame_ms: 500,
                    frames: (0..2)
                        .map(|frame| ClipFrame {
                            frame,
                            flip_x: false,
                            offset: [0, 0],
                        })
                        .collect(),
                }],
            },
            frames: vec![frame(64, [120, 80, 40, 255]), frame(64, [80, 60, 30, 255])],
        }],
        resources: vec![],
        ui: vec![],
        map_images: vec![],
        scan_effect: None,
        projectiles: Vec::new(),
        damage_effects: None,
        gas_effects: None,
        creep: None,
        indicators: None,
    };
    let hash = world.state_hash();
    for (elapsed, corpse_color) in [(0, 0x785028), (700, 0x503c1e)] {
        visuals.advance_effects(std::time::Duration::from_millis(elapsed), Some(&assets));
        let view = View {
            world: &world,
            visuals: &visuals,
            cursor: [-1.0, -1.0],
            targeting: false,
            presentation: &Presentation::default(),
            assets: Some(&assets),
            media: None,
            speaking: None,
            mission: None,
            animation_ms: 0,
            portrait_ms: 0,
            camera: Camera {
                x: 420.0,
                y: 420.0,
                zoom: 1.0,
            },
            selected: &BTreeSet::new(),
            selected_resource: None,
            drag_box: None,
            paused: true,
            playback: false,
            status: "Depth test",
            buttons: &[],
            help: "",
            placement: None,
            ending_hint: "F5 RESTART",
        };
        let mut pixels = vec![0; 800 * 600];
        view.draw(&mut pixels, 800, 600, 1.0);
        let row = ((600.0 + HEADER - FOOTER) / 2.0) as usize;
        assert_eq!(
            pixels[row * 800 + 400],
            0x1464dc,
            "living sprite covers the corpse"
        );
        assert_eq!(
            pixels[row * 800 + 424],
            corpse_color,
            "corpse still draws on surrounding terrain"
        );
        if elapsed == 700 && std::env::var_os("STRATERUST_REVIEW_CORPSES").is_some() {
            let mut before = pixels.clone();
            let death = &visuals.deaths()[0];
            let frame = visual::death_image(&assets, death).unwrap();
            let origin = view.camera.world_to_screen(
                f64::from(death.position.x),
                f64::from(death.position.y),
                [800.0, 600.0],
            );
            // Reproduce the former post-unit corpse pass for a paired
            // raster review; production only uses the corrected ordering.
            Canvas {
                scene: None,
                pixels: &mut before,
                width: 800,
                height: 600,
                scale: 1.0,
            }
            .image(
                frame.image,
                [
                    origin[0] - f64::from(frame.anchor[0]),
                    origin[1] - f64::from(frame.anchor[1]),
                ],
                [frame.image.width, frame.image.height],
                1.0,
            );
            for (name, raster) in [("before", &before), ("after", &pixels)] {
                let mut ppm = b"P6\n800 600\n255\n".to_vec();
                for color in raster {
                    ppm.extend([(color >> 16) as u8, (color >> 8) as u8, *color as u8]);
                }
                std::fs::write(format!("/tmp/straterust-corpse-overlap-{name}.ppm"), ppm).unwrap();
            }
        }
        let scene = view.scene(800, 600, 1.0);
        let sprite_commands: Vec<_> = scene
            .commands
            .iter()
            .filter_map(|command| {
                if let crate::gpu::Draw::Image { image, .. } = command {
                    if std::ptr::eq(*image, &assets.frames[0]) {
                        return Some("living");
                    }
                    if assets.extra_units[0]
                        .frames
                        .iter()
                        .any(|frame| std::ptr::eq(*image, frame))
                    {
                        return Some("corpse");
                    }
                }
                None
            })
            .collect();
        assert_eq!(
            sprite_commands,
            ["corpse", "living"],
            "GPU and software use the same floor layer"
        );
        assert_eq!(world.state_hash(), hash);
    }
}

#[test]
fn animation_and_presentation_changes_do_not_change_simulation() {
    let package = Package::load(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/fixtures"),
    )
    .unwrap();
    let world = package.world(42).unwrap();
    let hash = world.state_hash();
    let reference = ImageRef {
        file: "synthetic.srim".into(),
        blake3: "0".repeat(64),
    };
    let frame = |color: [u8; 4]| Image {
        width: 16,
        height: 16,
        rgba: color.repeat(16 * 16),
    };
    let assets = AssetPack {
        map_images: vec![],
        scan_effect: None,
        projectiles: Vec::new(),
        damage_effects: None,
        gas_effects: None,
        creep: None,
        indicators: None,
        manifest: AssetManifest {
            map_images: vec![],
            scan_effect: None,
            projectiles: Vec::new(),
            damage_effects: None,
            gas_effects: None,
            creep: None,
            indicators: None,
            schema_version: 1,
            terrain: reference.clone(),
            terrain_grid: None,
            unit_type: UnitTypeId(1),
            unit_name: "Synthetic".into(),
            frame_ms: 100,
            anchor: [8, 8],
            frames: vec![reference.clone(), reference],
            clips: vec![],
            extra_units: vec![],
            resources: vec![],
            ui: vec![],
        },
        terrain: frame([30, 40, 50, 255]),
        frames: vec![frame([255, 0, 0, 255]), frame([0, 0, 255, 255])],
        extra_units: vec![],
        resources: vec![],
        ui: vec![straterust_engine::assets::UiImage {
            key: "console".into(),
            image: frame([10, 20, 30, 255]),
        }],
    };
    let mut view = View {
        world: &world,
        visuals: &Visuals::new(&world),
        cursor: [-1.0, -1.0],
        targeting: false,
        presentation: &Presentation::default(),
        assets: Some(&assets),
        media: None,
        speaking: None,
        mission: None,
        animation_ms: 0,
        portrait_ms: 0,
        camera: Camera {
            x: 420.0,
            y: 420.0,
            zoom: 1.0,
        },
        selected: &BTreeSet::new(),
        selected_resource: None,
        drag_box: None,
        paused: true,
        playback: false,
        status: "Synthetic asset test",
        buttons: &[],
        help: "",
        placement: None,
        ending_hint: "F5 RESTART",
    };
    let mut first = vec![0; 800 * 600];
    let mut next = first.clone();
    view.draw(&mut first, 800, 600, 1.0);
    assert_eq!(
        first[599 * 800 + 20],
        0x0a141e,
        "native console replaces the HUD background"
    );
    assert_eq!(
        first[((600.0 + HEADER - FOOTER) / 2.0) as usize * 800 + 400],
        0xff0000,
        "frame anchor must align with the entity position"
    );
    assert_eq!(
        first[((600.0 + HEADER - FOOTER) / 2.0 + 100.0) as usize * 800 + 380],
        0x213b3a,
        "unmapped unit types retain geometric art"
    );
    view.animation_ms = 100;
    view.draw(&mut next, 800, 600, 1.0);
    assert_eq!(
        next[((600.0 + HEADER - FOOTER) / 2.0) as usize * 800 + 400],
        0xff0000
    );
    assert_eq!(
        first, next,
        "idle art must not walk as presentation time advances"
    );
    view.animation_ms = 200;
    view.draw(&mut next, 800, 600, 1.0);
    assert_eq!(first, next, "presentation animation must loop");
    view.assets = None;
    view.draw(&mut next, 800, 600, 1.0);
    assert_ne!(first, next, "geometric fallback must remain available");
    let media = MediaPack {
        mission_audio: Vec::new(),
        mission_texts: Vec::new(),
        briefing: Vec::new(),
        audio: vec![],
        music: vec![],
        portraits: vec![straterust_engine::media::Portrait {
            portrait_only: false,
            unit_type: UnitTypeId(1),
            frame_ms: 100,
            idle: vec![
                frame([0x9a, 0x12, 0x34, 255]),
                frame([0xa5, 0x12, 0x34, 255]),
            ],
            talk: vec![
                frame([0xb6, 0x12, 0x34, 255]),
                frame([0xc7, 0x12, 0x34, 255]),
            ],
        }],
    };
    let selected = BTreeSet::from([world.state().entities[0].id]);
    view.selected = &selected;
    view.media = Some(&media);
    view.animation_ms = 0;
    view.portrait_ms = 0;
    view.draw(&mut first, 800, 600, 1.0);
    assert!(
        first.contains(&0x9a1234),
        "selected unit displays its idle portrait"
    );
    view.portrait_ms = 100;
    view.draw(&mut next, 800, 600, 1.0);
    assert!(
        next.contains(&0xa51234),
        "idle portrait advances independently of body pose"
    );
    view.speaking = Some(UnitTypeId(1));
    view.paused = true;
    view.portrait_ms = 0;
    view.draw(&mut next, 800, 600, 1.0);
    assert!(
        next.contains(&0xb61234),
        "voice response switches to talking portrait"
    );
    view.portrait_ms = 100;
    view.draw(&mut next, 800, 600, 1.0);
    assert!(
        next.contains(&0xc71234),
        "talking portrait advances while world animation and simulation are paused"
    );
    assert_eq!(view.animation_ms, 0);
    view.speaking = Some(UnitTypeId(2));
    view.draw(&mut next, 800, 600, 1.0);
    assert!(
        next.contains(&0xa51234),
        "another unit's voice does not animate this portrait"
    );
    assert_eq!(hash, world.state_hash());
}
