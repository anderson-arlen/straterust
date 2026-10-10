//! Sparse cursor packs and harvestable terrain at transparent fog boundaries.
use super::*;
use straterust_engine::{
    assets::{CursorManifest, IndicatorsManifest, IndicatorsPack, ResourceImage, ResourceManifest},
    sim::{Footprint, ResourceSpawn, Spawn},
};

fn image(color: [u8; 4], size: u32) -> Image {
    Image {
        width: size,
        height: size,
        rgba: color.repeat((size * size) as usize),
    }
}

fn assets() -> AssetPack {
    let reference = ImageRef {
        file: "test.srim".into(),
        blake3: "0".repeat(64),
    };
    let manifest: AssetManifest = ron::from_str(&format!(
        "(schema_version:1,terrain:(file:\"test.srim\",blake3:\"{}\"),unit_type:1,unit_name:\"Test\",frame_ms:100,anchor:(0,0),frames:[])", reference.blake3
    )).unwrap();
    AssetPack {
        manifest,
        terrain: image([0, 255, 0, 255], 1),
        frames: vec![],
        extra_units: vec![],
        resources: vec![ResourceImage {
            manifest: ResourceManifest {
                kind: "wood".into(),
                positions: vec![Position { x: 144, y: 144 }, Position { x: 176, y: 144 }],
                terrain: true,
                terrain_edges: None,
                image: reference.clone(),
                depleted_image: Some(reference),
                active_image: None,
                anchor: [16, 16],
                selection_circle: None,
                selection_y: 0,
            },
            image: image([0, 255, 0, 255], 32),
            depleted_image: Some(image([255, 0, 0, 255], 32)),
            active_image: None,
        }],
        carried_resources: vec![],
        ui: vec![],
        map_images: vec![],
        scan_effect: None,
        projectiles: vec![],
        damage_effects: None,
        gas_effects: None,
        creep: None,
        indicators: None,
    }
}

fn world(fog: bool) -> World {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/terran-demo");
    let original = Package::load(&path).unwrap().world(7).unwrap();
    let mut rules = original.rules().clone();
    rules.victory = false;
    let mut map = original.map().clone();
    map.terrain = None;
    map.mission = None;
    map.ai.clear();
    map.fog_of_war = fog;
    map.spawns = vec![Spawn {
        owner: PlayerId(0),
        unit_type: UnitTypeId(1),
        position: Position { x: 100, y: 100 },
        ..Default::default()
    }];
    map.resources = [144, 176]
        .map(|x| ResourceSpawn {
            terrain_corners: None,
            kind: "wood".into(),
            position: Position { x, y: 144 },
            amount: 100,
            footprint: Footprint {
                width: 32,
                height: 32,
            },
            requires_extractor: false,
        })
        .to_vec();
    World::new(rules, map, 7).unwrap()
}

fn make_view<'a>(
    world: &'a World,
    assets: &'a AssetPack,
    visuals: &'a Visuals,
    presentation: &'a Presentation,
    selected: &'a BTreeSet<EntityId>,
) -> View<'a> {
    View {
        world,
        visuals,
        assets: Some(assets),
        presentation,
        selected,
        camera: Camera {
            x: 160.0,
            y: 160.0,
            zoom: 1.0,
            viewport: None,
        },
        cursor: [-1.0; 2],
        targeting: false,
        map_art: None,
        media: None,
        speaking: None,
        mission: None,
        animation_ms: 0,
        portrait_ms: 0,
        selected_resource: None,
        drag_box: None,
        paused: false,
        playback: false,
        status: "",
        buttons: &[],
        help: "",
        placement: None,
        placement_type: None,
        ending_hint: "",
    }
}

#[test]
fn unknown_forest_stays_under_alpha_fog_and_known_depletion_draws_stumps() {
    let server = world(true);
    let mut packet = server.player_view(PlayerId(0)).unwrap();
    let columns = (server.map().width / 32) as usize;
    packet.fog.fill(0x11);
    packet.terrain_fog.fill(2);
    for y in 0..(server.map().height / 32) as usize {
        for x in 5..columns {
            packet.fog[y * columns + x] = 0;
            packet.terrain_fog[y * columns + x] = 0;
        }
    }
    packet.resources = vec![server.state().resources[0].clone()];
    packet.resources[0].amount = 0;
    let client = packet.into_world(&server).unwrap();
    assert_eq!(
        client.state().resources.len(),
        1,
        "unknown resource contents stay private"
    );
    let assets = assets();
    let visuals = Visuals::new(&client);
    let presentation = Presentation::default();
    let selected = BTreeSet::new();
    let view = make_view(&client, &assets, &visuals, &presentation, &selected);
    let size = [640.0, 480.0];
    let mut pixels = vec![0; 640 * 480];
    let mut canvas = Canvas {
        scene: None,
        pixels: &mut pixels,
        width: 640,
        height: 480,
        scale: 1.0,
    };
    view.paint_terrain(&mut canvas, size);
    view.paint_terrain_resources(&mut canvas, size);
    view.draw_fog(&mut canvas, size);
    for (x, channel) in [(144.0, 16), (162.0, 8)] {
        let p = view.camera.world_to_screen(x, 144.0, size);
        let pixel = pixels[p[1] as usize * 640 + p[0] as usize];
        assert!(pixel >> channel & 255 > 0);
        assert_eq!(
            pixel & !(255 << channel),
            0,
            "fog must shade the resource art, not replace it with ground"
        );
    }
    assert!(
        fog::atlas()
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .all(|rgba| rgba[..3] == [0, 0, 0])
    );
}

#[test]
fn occupied_resource_uses_the_active_image_only_for_the_disclosed_node() {
    let server = world(false);
    let mut packet = server.player_view(PlayerId(0)).unwrap();
    let resource = packet.resources[0].id;
    let mut assets = assets();
    assets.resources[0].manifest.terrain = false;
    assets.resources[0].active_image = Some(image([0, 0, 255, 255], 32));
    let selected = BTreeSet::new();
    let presentation = Presentation::default();
    for active in [true, false] {
        packet.active_resources.clear();
        if active {
            packet.active_resources.insert(resource);
        }
        let client = packet.clone().into_world(&server).unwrap();
        let visuals = Visuals::new(&client);
        let view = make_view(&client, &assets, &visuals, &presentation, &selected);
        let mut pixels = vec![0; 640 * 480];
        let size = [640.0, 480.0];
        view.paint_resources(
            &mut Canvas {
                scene: None,
                pixels: &mut pixels,
                width: 640,
                height: 480,
                scale: 1.0,
            },
            size,
        );
        for (x, color) in [
            (144.0, if active { 0x0000ff } else { 0x00ff00 }),
            (176.0, 0x00ff00),
        ] {
            let p = view.camera.world_to_screen(x, 144.0, size);
            assert_eq!(pixels[p[1] as usize * 640 + p[0] as usize], color);
        }
    }
}

#[test]
fn sparse_cursor_pack_draws_hover_drag_and_targeting_cursors_with_an_arrow_fallback() {
    let world = world(false);
    let mut assets = assets();
    let reference = assets.resources[0].manifest.image.clone();
    let keys = ["arrow", "magnifier", "target-green"];
    assets.indicators = Some(IndicatorsPack {
        manifest: IndicatorsManifest {
            segmented_bars: false,
            circles: vec![],
            units: vec![],
            health_colors: [0; 19],
            cursors: keys
                .iter()
                .map(|key| CursorManifest {
                    key: (*key).into(),
                    image: reference.clone(),
                    frames: 1,
                    frame_ms: 100,
                    anchor: [0, 0],
                })
                .collect(),
        },
        circles: vec![],
        cursors: vec![
            image([255, 0, 0, 255], 1),
            image([0, 255, 0, 255], 1),
            image([0, 0, 255, 255], 1),
        ],
    });
    let visuals = Visuals::new(&world);
    let presentation = Presentation::default();
    let selected = BTreeSet::new();
    let mut view = make_view(&world, &assets, &visuals, &presentation, &selected);
    let size = [640.0, 480.0];
    for (position, targeting, drag, color) in [
        (Position { x: 100, y: 100 }, false, false, 0x00ff00),
        (Position { x: 144, y: 144 }, false, false, 0x00ff00),
        (Position { x: 220, y: 200 }, true, false, 0x0000ff),
        (Position { x: 220, y: 200 }, false, true, 0x0000ff),
        (Position { x: 220, y: 200 }, false, false, 0xff0000),
    ] {
        view.cursor =
            view.camera
                .world_to_screen(f64::from(position.x), f64::from(position.y), size);
        view.targeting = targeting;
        view.drag_box = drag.then_some([[0.0; 2]; 2]);
        let mut pixels = vec![0; 640 * 480];
        view.draw_cursor(
            &mut Canvas {
                scene: None,
                pixels: &mut pixels,
                width: 640,
                height: 480,
                scale: 1.0,
            },
            size,
        );
        assert!(
            pixels.contains(&color),
            "cursor disappeared for {position:?} targeting={targeting} drag={drag}"
        );
    }
    // A pack with only an arrow must still draw on an entity.
    assets
        .indicators
        .as_mut()
        .unwrap()
        .manifest
        .cursors
        .truncate(1);
    let mut view = make_view(&world, &assets, &visuals, &presentation, &selected);
    view.cursor = view.camera.world_to_screen(100.0, 100.0, size);
    let mut pixels = vec![0; 640 * 480];
    view.draw_cursor(
        &mut Canvas {
            scene: None,
            pixels: &mut pixels,
            width: 640,
            height: 480,
            scale: 1.0,
        },
        size,
    );
    assert!(pixels.contains(&0xff0000));
}

#[test]
#[ignore = "requires STRATERUST_WARCRAFT2 pointing to a refreshed retail import"]
fn warcraft2_native_forest_fog_stumps_and_mine_activity_render_for_review() {
    use std::io::Write;
    let root = std::path::PathBuf::from(std::env::var_os("STRATERUST_WARCRAFT2").unwrap());
    let directory = root.join("human/mission02");
    let server = Package::load(&directory).unwrap().world(7).unwrap();
    let assets = AssetPack::load(&directory).unwrap().unwrap();
    let presentation: Presentation =
        straterust_engine::content::read_ron(&directory.join("presentation.ron")).unwrap();
    let home = server
        .map()
        .start_locations
        .iter()
        .find(|s| s.player == PlayerId(0))
        .unwrap()
        .position;
    let nearest = |kind| {
        server
            .state()
            .resources
            .iter()
            .filter(|r| r.kind == kind)
            .min_by_key(|r| {
                i64::from(r.position.x - home.x).pow(2) + i64::from(r.position.y - home.y).pow(2)
            })
            .unwrap()
            .clone()
    };
    let wood = nearest("wood");
    let gold = nearest("gold");
    let mut idle_pixels = None;
    for (name, node, active) in [
        ("forest", wood.clone(), false),
        ("forest-cut", wood, false),
        ("mine-idle", gold.clone(), false),
        ("mine-active", gold, true),
    ] {
        let mut packet = server.player_view(PlayerId(0)).unwrap();
        packet.resources = vec![node.clone()];
        packet.active_resources.clear();
        packet.fog.fill(0x11);
        packet.terrain_fog.fill(2);
        if name.starts_with("forest") {
            packet.resources[0].amount = 0;
            if name == "forest-cut" {
                packet.resources = server
                    .state()
                    .resources
                    .iter()
                    .filter(|r| {
                        r.kind == "wood"
                            && r.position.x >= node.position.x
                            && r.position.x <= node.position.x + 64
                            && r.position.y >= node.position.y - 64
                            && r.position.y <= node.position.y + 128
                    })
                    .cloned()
                    .map(|mut r| {
                        r.amount = 0;
                        r
                    })
                    .collect();
            }
            let columns = (server.map().width / 32) as usize;
            let boundary = node.position.x as usize / 32 + 1;
            for y in 0..server.map().height as usize / 32 {
                for x in boundary..columns {
                    if name == "forest" {
                        packet.fog[y * columns + x] = 0;
                        packet.terrain_fog[y * columns + x] = 0;
                    }
                }
            }
        } else if active {
            packet.active_resources.insert(node.id);
        }
        let client = packet.into_world(&server).unwrap();
        assert_eq!(
            client.visibility(PlayerId(0), node.position),
            Visibility::Visible
        );
        let visuals = Visuals::new(&client);
        let selected = BTreeSet::new();
        let mut view = make_view(&client, &assets, &visuals, &presentation, &selected);
        view.camera.viewport = assets.manifest.console_layout.map(|layout| layout.viewport);
        view.camera.x = f64::from(node.position.x);
        view.camera.y = f64::from(node.position.y);
        view.camera.zoom = 1.5;
        view.selected_resource = (!name.starts_with("forest")).then_some(node.id);
        let size = [1000, 760];
        view.cursor =
            view.camera
                .world_to_screen(view.camera.x, view.camera.y, size.map(f64::from));
        let mut pixels = vec![0; 1000 * 760];
        view.draw(&mut pixels, size[0], size[1], 1.0);
        if name == "mine-idle" {
            idle_pixels = Some(pixels.clone());
        } else if active {
            assert_ne!(
                idle_pixels.as_ref().unwrap(),
                &pixels,
                "occupied native mine must light up"
            );
        }
        let mut file =
            std::fs::File::create(format!("/tmp/straterust-review-warcraft2-{name}.ppm")).unwrap();
        file.write_all(b"P6\n1000 760\n255\n").unwrap();
        file.write_all(
            &pixels
                .into_iter()
                .flat_map(|p| [(p >> 16) as u8, (p >> 8) as u8, p as u8])
                .collect::<Vec<_>>(),
        )
        .unwrap();
    }
}

#[test]
#[ignore = "requires STRATERUST_WARCRAFT2 pointing to a refreshed retail import"]
fn warcraft2_native_irregular_cuts_keep_complete_single_trees_in_all_four_tilesets() {
    use straterust_engine::assets::{ConsoleViewport, TerrainGrid};
    use straterust_engine::sim::{ResourceId, ResourceNode};
    let root = std::path::PathBuf::from(std::env::var_os("STRATERUST_WARCRAFT2").unwrap());
    let server = world(false);
    let mut capture = vec![0u32; 896 * 896];
    for (era, package) in [
        "human/mission01",
        "human/mission05",
        "human/mission08",
        "human-expansion/mission04",
    ]
    .into_iter()
    .enumerate()
    {
        let mut assets = AssetPack::load(&root.join(package)).unwrap().unwrap();
        let map: straterust_engine::sim::Map =
            straterust_engine::content::read_ron(&root.join(package).join("map.ron")).unwrap();
        let home = map
            .start_locations
            .iter()
            .find(|s| s.player == PlayerId(0))
            .unwrap()
            .position;
        let native_grid = assets.manifest.terrain_grid.as_ref().unwrap();
        let ground = native_grid.tiles
            [(home.y / 32) as usize * native_grid.columns as usize + (home.x / 32) as usize];
        let presentation: Presentation =
            straterust_engine::content::read_ron(&root.join(package).join("presentation.ron"))
                .unwrap();
        for (pattern, cuts) in [
            vec![],
            vec![(3, 3)],
            vec![(3, 2), (3, 4)],
            vec![(2, 2), (2, 4), (3, 3), (4, 1), (4, 5)],
        ]
        .into_iter()
        .enumerate()
        {
            let mut grid = TerrainGrid {
                tile_size: 32,
                columns: 7,
                rows: 7,
                tiles: vec![ground; 49],
            };
            if pattern == 3 {
                for y in 1..6 {
                    for x in 2..5 {
                        grid.tiles[y * 7 + x] = 125;
                    }
                }
            } else {
                for (y, tile) in [(1, 121), (2, 122), (3, 122), (4, 122), (5, 123)] {
                    grid.tiles[y * 7 + 3] = tile;
                }
            }
            let edges = assets
                .resources
                .iter()
                .find(|r| r.manifest.kind == "wood")
                .unwrap()
                .manifest
                .terrain_edges
                .as_ref()
                .unwrap();
            if pattern == 3 {
                for y in 1..6 {
                    for x in 2..5 {
                        let mask = match (x, y) {
                            (2, 1) => 1,
                            (4, 1) => 2,
                            (_, 1) => 3,
                            (2, 5) => 8,
                            (4, 5) => 4,
                            (_, 5) => 12,
                            (2, _) => 9,
                            (4, _) => 6,
                            _ => 15,
                        };
                        grid.tiles[y * 7 + x] = edges.tiles[mask].unwrap();
                    }
                }
            }
            let patches = super::super::resource_edges::replacements(
                &grid,
                edges,
                &cuts.iter().copied().collect(),
            );
            if pattern == 1 {
                assert_eq!(patches[&(3, 2)], Some(123));
                assert_eq!(patches[&(3, 4)], Some(121));
            } else if pattern == 2 {
                for y in [1, 3, 5] {
                    assert_eq!(patches[&(3, y)], None);
                }
            }
            assets.manifest.terrain_grid = Some(grid);
            let mut packet = server.player_view(PlayerId(0)).unwrap();
            packet.resources = cuts
                .into_iter()
                .enumerate()
                .map(|(i, (x, y))| ResourceNode {
                    id: ResourceId(i as u32 + 1),
                    kind: "wood".into(),
                    position: Position {
                        x: x * 32 + 16,
                        y: y * 32 + 16,
                    },
                    footprint: Footprint {
                        width: 32,
                        height: 32,
                    },
                    amount: 0,
                    requires_extractor: false,
                })
                .collect();
            let client = packet.into_world(&server).unwrap();
            let visuals = Visuals::new(&client);
            let selected = BTreeSet::new();
            let mut view = make_view(&client, &assets, &visuals, &presentation, &selected);
            view.camera = Camera {
                x: 112.0,
                y: 112.0,
                zoom: 1.0,
                viewport: Some(ConsoleViewport {
                    canvas: [224; 2],
                    margins: [0; 4],
                }),
            };
            let mut pixels = vec![0; 224 * 224];
            let mut canvas = Canvas {
                scene: None,
                pixels: &mut pixels,
                width: 224,
                height: 224,
                scale: 1.0,
            };
            view.paint_terrain(&mut canvas, [224.0; 2]);
            view.paint_terrain_resources(&mut canvas, [224.0; 2]);
            // Verify real rendered pixels, including inferred orphan removal;
            // remembered cuts alone must produce the same stumps as the server.
            let tile = if pattern == 1 { 123 } else { 126 };
            if pattern == 1 || pattern == 2 {
                let columns = assets.terrain.width / 32;
                let y = if pattern == 1 { 2 } else { 3 };
                for dy in 0..32 {
                    for dx in 0..32 {
                        let p = ((tile / columns * 32 + dy) * assets.terrain.width
                            + tile % columns * 32
                            + dx) as usize
                            * 4;
                        let rgba = &assets.terrain.rgba[p..p + 3];
                        let expected =
                            u32::from(rgba[0]) << 16 | u32::from(rgba[1]) << 8 | u32::from(rgba[2]);
                        assert_eq!(
                            pixels[(y * 32 + dy) as usize * 224 + 96 + dx as usize],
                            expected
                        );
                    }
                }
            }
            for y in 0..224 {
                let dest = (era * 224 + y) * 896 + pattern * 224;
                capture[dest..dest + 224].copy_from_slice(&pixels[y * 224..y * 224 + 224]);
            }
        }
    }
    let bytes: Vec<_> = b"P6\n896 896\n255\n"
        .iter()
        .copied()
        .chain(
            capture
                .into_iter()
                .flat_map(|p| [(p >> 16) as u8, (p >> 8) as u8, p as u8]),
        )
        .collect();
    std::fs::write(
        "/tmp/straterust-review-warcraft2-forest-patterns.ppm",
        bytes,
    )
    .unwrap();
}

#[test]
#[ignore = "requires STRATERUST_WARCRAFT2 pointing to a refreshed retail import"]
fn warcraft2_native_building_scar_renders_at_the_destroyed_site_then_heals() {
    use std::io::Write;
    use straterust_engine::sim::{Command, Order};
    let root = std::path::PathBuf::from(std::env::var_os("STRATERUST_WARCRAFT2").unwrap());
    let directory = root.join("human/mission02");
    let initial = Package::load(&directory).unwrap().world(7).unwrap();
    let assets = AssetPack::load(&directory).unwrap().unwrap();
    let presentation: Presentation =
        straterust_engine::content::read_ron(&directory.join("presentation.ron")).unwrap();
    let farm = UnitTypeId(59);
    let position = initial
        .map()
        .start_locations
        .iter()
        .find(|s| s.player == PlayerId(0))
        .unwrap()
        .position;
    let mut rules = initial.rules().clone();
    rules.victory = false;
    rules
        .units
        .iter_mut()
        .find(|u| u.id == farm)
        .unwrap()
        .max_hp = 1;
    let mut map = initial.map().clone();
    map.terrain = None;
    map.ai.clear();
    map.mission = None;
    map.fog_of_war = false;
    map.resources.clear();
    map.initial_explored.clear();
    map.spawns = vec![
        Spawn {
            unit_type: farm,
            position,
            ..Default::default()
        },
        Spawn {
            owner: PlayerId(1),
            unit_type: UnitTypeId(1),
            position: Position {
                x: position.x + 56,
                y: position.y,
            },
            ..Default::default()
        },
    ];
    let mut server = World::new(rules, map, 7).unwrap();
    let mut client = server
        .player_view(PlayerId(0))
        .unwrap()
        .into_world(&server)
        .unwrap();
    let mut visuals = Visuals::new(&client);
    let outcomes = server
        .step(&[Command {
            tick: server.tick(),
            player: PlayerId(1),
            sequence: 1,
            order: Order::Attack {
                entity: EntityId(2),
                target: EntityId(1),
            },
        }])
        .unwrap();
    assert!(outcomes[0].rejection.is_none());
    for _ in 0..100 {
        let mut packet = server.player_view(PlayerId(0)).unwrap();
        // The session protocol supplies disclosed removals; a bare stateless
        // world view does not retain death history between calls.
        if !server.state().entities.iter().any(|e| e.id == EntityId(1)) {
            packet.removed.push(EntityId(1));
        }
        client = packet.into_world(&client).unwrap();
        visuals.update(&client);
        if !visuals.deaths().is_empty() {
            break;
        }
        server.step(&[]).unwrap();
    }
    assert_eq!(visuals.deaths().len(), 1);
    assert_eq!(visuals.deaths()[0].position, position);
    let clip = assets
        .sprite(farm)
        .unwrap()
        .clip(ClipKind::Death)
        .unwrap()
        .clone();
    let scar_start = (clip.frames.len() - 45) as u64 * u64::from(clip.frame_ms);
    let hash = client.state_hash();
    let mut previous_pixels = None;
    for (name, elapsed) in [
        ("scar-start", scar_start),
        ("scar-fading", 1485),
        ("scar-healed", 1500),
    ] {
        visuals.advance_effects(std::time::Duration::from_millis(elapsed), Some(&assets));
        assert_eq!(visuals.deaths().is_empty(), name == "scar-healed");
        let selected = BTreeSet::new();
        let mut view = make_view(&client, &assets, &visuals, &presentation, &selected);
        view.camera.viewport = assets.manifest.console_layout.map(|l| l.viewport);
        view.camera.x = f64::from(position.x);
        view.camera.y = f64::from(position.y);
        view.camera.zoom = 2.0;
        let mut pixels = vec![0; 1000 * 760];
        view.draw(&mut pixels, 1000, 760, 1.0);
        if let Some(previous) = &previous_pixels {
            assert_ne!(previous, &pixels);
        }
        let mut file =
            std::fs::File::create(format!("/tmp/straterust-review-warcraft2-{name}.ppm")).unwrap();
        file.write_all(b"P6\n1000 760\n255\n").unwrap();
        file.write_all(
            &pixels
                .iter()
                .flat_map(|p| [(p >> 16) as u8, (p >> 8) as u8, *p as u8])
                .collect::<Vec<_>>(),
        )
        .unwrap();
        previous_pixels = Some(pixels);
    }
    assert_eq!(client.state_hash(), hash);
}

#[test]
#[ignore = "requires STRATERUST_WARCRAFT2 pointing to a refreshed retail import"]
fn warcraft2_native_oil_platform_uses_active_source_frame_for_owned_and_enemy_views() {
    use std::io::Write;
    use straterust_engine::sim::{Command, Order};
    let root = std::path::PathBuf::from(std::env::var_os("STRATERUST_WARCRAFT2").unwrap());
    for (race, folder) in [(0, "human"), (1, "orc")] {
        let directory = root.join(folder).join("mission03");
        let original = Package::load(&directory).unwrap().world(7).unwrap();
        let assets = AssetPack::load(&directory).unwrap().unwrap();
        let presentation: Presentation =
            straterust_engine::content::read_ron(&directory.join("presentation.ron")).unwrap();
        let node = original
            .state()
            .resources
            .iter()
            .find(|r| r.kind == "oil")
            .unwrap()
            .clone();
        let platform = UnitTypeId(87 + race);
        let tanker = UnitTypeId(27 + race);
        let mut rules = original.rules().clone();
        rules.victory = false;
        let mut map = original.map().clone();
        map.ai.clear();
        map.mission = None;
        map.fog_of_war = false;
        map.spawns = vec![Spawn {
            unit_type: platform,
            position: node.position,
            ..Default::default()
        }];
        let probe = World::new(rules.clone(), map.clone(), 7).unwrap();
        let size = probe.unit_type(tanker).unwrap().footprint;
        let position = (-8..=8)
            .flat_map(|y| {
                (-8..=8).map(move |x| Position {
                    x: node.position.x + x * 32,
                    y: node.position.y + y * 32,
                })
            })
            .find(|p| probe.can_place(*p, size, straterust_engine::sim::MovementClass::Water, None))
            .unwrap();
        map.spawns.push(Spawn {
            unit_type: tanker,
            position,
            ..Default::default()
        });
        let mut server = World::new(rules, map, 7).unwrap();
        let mut visuals = Visuals::new(
            &server
                .player_view(PlayerId(0))
                .unwrap()
                .into_world(&server)
                .unwrap(),
        );
        for (stage, active) in [("idle", false), ("active", true), ("finished", false)] {
            if stage == "active" {
                server
                    .step(&[Command {
                        tick: server.tick(),
                        player: PlayerId(0),
                        sequence: 1,
                        order: Order::Gather {
                            entity: EntityId(2),
                            resource: node.id,
                        },
                    }])
                    .unwrap();
                for _ in 0..1000 {
                    if server.resource_working(node.id) {
                        break;
                    }
                    server.step(&[]).unwrap();
                }
            } else if stage == "finished" {
                for _ in 0..200 {
                    if !server.resource_working(node.id) {
                        break;
                    }
                    server.step(&[]).unwrap();
                }
            }
            for player in [PlayerId(0), PlayerId(1)] {
                let client = server
                    .player_view(player)
                    .unwrap()
                    .into_world(&server)
                    .unwrap();
                assert_eq!(client.entity_working(EntityId(1)), active);
                visuals.update(&client);
                let entity = client
                    .state()
                    .entities
                    .iter()
                    .find(|e| e.id == EntityId(1))
                    .unwrap();
                let frame =
                    crate::visual::unit_image(&assets, entity, visuals.get(entity.id), &client)
                        .unwrap();
                let sprite = assets.sprite(platform).unwrap();
                assert_eq!(
                    frame.image.rgba,
                    sprite.frames[if active { 2 } else { 0 }].rgba
                );
                if player == PlayerId(0) {
                    let selected = BTreeSet::new();
                    let mut view = make_view(&client, &assets, &visuals, &presentation, &selected);
                    view.camera.x = f64::from(node.position.x);
                    view.camera.y = f64::from(node.position.y);
                    view.camera.viewport = assets.manifest.console_layout.map(|l| l.viewport);
                    view.camera.zoom = 1.5;
                    let mut pixels = vec![0; 1000 * 760];
                    view.draw(&mut pixels, 1000, 760, 1.0);
                    let mut file = std::fs::File::create(format!(
                        "/tmp/straterust-review-warcraft2-oil-{folder}-{stage}.ppm"
                    ))
                    .unwrap();
                    file.write_all(b"P6\n1000 760\n255\n").unwrap();
                    file.write_all(
                        &pixels
                            .iter()
                            .flat_map(|p| [(p >> 16) as u8, (p >> 8) as u8, *p as u8])
                            .collect::<Vec<_>>(),
                    )
                    .unwrap();
                }
            }
        }
    }
}
