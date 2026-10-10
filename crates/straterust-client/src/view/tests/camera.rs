use super::*;

#[test]
fn terrain_repeats_and_clips_to_the_map_edge() {
    let image = Image {
        width: 2,
        height: 1,
        rgba: vec![255, 0, 0, 255, 0, 0, 255, 255],
    };
    let mut pixels = [0; 7];
    Canvas {
        scene: None,
        pixels: &mut pixels,
        width: 7,
        height: 1,
        scale: 1.0,
    }
    .image(&image, [0.0, 0.0], [5, 1], 1.0);
    assert_eq!(
        pixels,
        [0xff0000, 0x0000ff, 0xff0000, 0x0000ff, 0xff0000, 0, 0]
    );
}

#[test]
fn camera_clamps_the_viewport_and_visits_only_visible_tiles() {
    let grid = TerrainGrid {
        tile_size: 32,
        columns: 4,
        rows: 4,
        tiles: vec![0; 16],
    };
    let size = [64.0, 64.0 + HEADER + FOOTER];
    let mut camera = Camera {
        x: 64.0,
        y: 64.0,
        viewport: None,
        zoom: 1.0,
    };
    assert_eq!(camera.visible_tiles(&grid, size), [1, 1, 3, 3]);
    camera.x = -50.0;
    camera.y = 500.0;
    camera.clamp_to_map([128, 128], size);
    assert_eq!([camera.x, camera.y], [32.0, 96.0]);
    assert_eq!(camera.visible_tiles(&grid, size), [0, 2, 2, 4]);
    camera.zoom = 0.25;
    camera.clamp_to_map([128, 128], size);
    assert_eq!([camera.x, camera.y], [64.0, 64.0]);
    assert_eq!(camera.visible_tiles(&grid, size), [0, 0, 4, 4]);
    for zoom in [0.5, 1.0, 3.0] {
        camera.zoom = zoom;
        camera.x = -1000.0;
        camera.y = 10000.0;
        camera.clamp_to_map([1600, 1000], [800.0, 600.0]);
        let bounds = camera.visible_world([800.0, 600.0]);
        assert!(bounds[0] >= 0.0 && bounds[2] <= 1600.0);
        assert!(bounds[1] >= 0.0 && bounds[3] <= 1000.0);
    }
}

#[test]
fn picking_round_trips_at_different_dpi_zoom_and_window_sizes() {
    for dpi in [1.0, 1.5, 2.0] {
        for zoom in [0.5, 1.0, 3.0] {
            for size in [[800.0, 600.0], [1440.0, 900.0]] {
                let camera = Camera {
                    x: 800.0,
                    y: 500.0,
                    viewport: None,
                    zoom,
                };
                let screen = camera.world_to_screen(815.0, 530.0, size);
                let physical = [screen[0] * dpi, screen[1] * dpi];
                assert_eq!(
                    camera.screen_to_world([physical[0] / dpi, physical[1] / dpi], size),
                    Some(Position { x: 815, y: 530 })
                );
                assert_eq!(camera.screen_to_world([20.0, 20.0], size), None);
            }
        }
    }
}
#[test]
#[ignore = "repeatable software frame timing; optional STRATERUST_RENDER_PACKAGE"]
fn render_frame_measurement() {
    use std::{hint::black_box, time::Instant};
    let directory = std::env::var_os("STRATERUST_RENDER_PACKAGE")
        .map(|path| {
            let path = std::path::PathBuf::from(path);
            if path.is_absolute() {
                path
            } else {
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../..")
                    .join(path)
            }
        })
        .unwrap_or_else(|| {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/terran-demo")
        });
    let package = Package::load(&directory).unwrap();
    let assets = AssetPack::load(&directory).unwrap();
    let world = package.world(42).unwrap();
    let presentation = Presentation::default();
    let selected = BTreeSet::new();
    let view = View {
        world: &world,
        visuals: &Visuals::new(&world),
        cursor: [-1.0, -1.0],
        targeting: false,
        presentation: &presentation,
        assets: assets.as_ref(),
        map_art: None,
        media: None,
        speaking: None,
        mission: None,
        animation_ms: 0,
        portrait_ms: 0,
        camera: Camera {
            x: 768.0,
            y: 480.0,
            viewport: None,
            zoom: 1.0,
        },
        selected: &selected,
        selected_resource: None,
        drag_box: None,
        paused: false,
        playback: false,
        status: "Frame benchmark",
        buttons: &[],
        help: "",
        placement: None,
        placement_type: None,
        ending_hint: "F5 RESTART",
    };
    for [width, height] in [[1280, 800], [1920, 1080]] {
        let mut pixels = vec![0; (width * height) as usize];
        let mut samples = Vec::new();
        let mut recording_samples = Vec::new();
        let mut quads = 0;
        for frame in 0..36 {
            let start = Instant::now();
            view.draw(black_box(&mut pixels), width, height, 1.0);
            let elapsed = start.elapsed();
            black_box(&pixels);
            let start = Instant::now();
            let scene = black_box(view.scene(width, height, 1.0));
            let recording = start.elapsed();
            quads = scene.commands.len();
            if frame >= 4 {
                recording_samples.push(recording);
            }
            if frame >= 4 {
                samples.push(elapsed);
            }
        }
        samples.sort();
        recording_samples.sort();
        eprintln!(
            "{width}x{height} GPU scene recording: median={:.3}ms p95={:.3}ms quads={quads}",
            recording_samples[16].as_secs_f64() * 1000.0,
            recording_samples[30].as_secs_f64() * 1000.0
        );
        eprintln!(
            "{} {width}x{height} frame draw: median={:.3}ms p95={:.3}ms max={:.3}ms",
            directory.display(),
            samples[16].as_secs_f64() * 1000.0,
            samples[30].as_secs_f64() * 1000.0,
            samples[31].as_secs_f64() * 1000.0
        );
    }
}

#[test]
fn fog_tiles_join_without_gaps_or_double_blending_at_zoom_and_dpi() {
    let grid = [
        0, 0, 0, 0, 0, 0, 1, 1, 0, 0, 0, 1, 2, 2, 0, 0, 1, 2, 1, 0, 0, 0, 0, 0, 0,
    ];
    for scale in [1.0, 1.25, 2.0] {
        for zoom in [0.5, 0.75, 1.0, 1.25, 1.5, 2.0] {
            let origin = [-7.4, 2.3];
            let mut expected = vec![0xffffff; 180 * 160];
            let mut actual = expected.clone();
            for layer in 0..2 {
                let mut assembled = Image {
                    width: 96,
                    height: 96,
                    rgba: vec![0; 96 * 96 * 4],
                };
                for y in 0..3 {
                    for x in 0..3 {
                        let mask = fog::masks(&grid, 5, 5, x + 1, y + 1)[layer];
                        let [left, top, _, _] = fog::region(mask, layer);
                        for py in 0..32 {
                            for px in 0..32 {
                                assembled.rgba
                                    [((y * 32 + py) * 96 + x * 32 + px) as usize * 4 + 3] =
                                    fog::atlas().rgba[((top + py as u32) * fog::atlas().width
                                        + left
                                        + px as u32)
                                        as usize
                                        * 4
                                        + 3];
                            }
                        }
                        Canvas {
                            scene: None,
                            pixels: &mut actual,
                            width: 180,
                            height: 160,
                            scale,
                        }
                        .image_region(
                            fog::atlas(),
                            [
                                origin[0] + f64::from(x * 32) * zoom,
                                origin[1] + f64::from(y * 32) * zoom,
                            ],
                            [32, 32],
                            zoom,
                            fog::region(mask, layer),
                        );
                    }
                }
                reference_blit(
                    &mut Canvas {
                        scene: None,
                        pixels: &mut expected,
                        width: 180,
                        height: 160,
                        scale,
                    },
                    &assembled,
                    origin,
                    [96, 96],
                    zoom,
                    [0, 0, 96, 96],
                );
            }
            assert_eq!(actual, expected, "fog seams at zoom={zoom}, dpi={scale}");
        }
    }
}
