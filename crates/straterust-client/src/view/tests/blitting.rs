use super::*;

#[test]
fn rgba_blending_clips_and_preserves_transparent_pixels() {
    let image = Image {
        width: 2,
        height: 2,
        rgba: vec![
            255, 0, 0, 255, 0, 255, 0, 0, 0, 0, 255, 128, 255, 255, 255, 255,
        ],
    };
    let mut pixels = [0x202020; 6];
    let mut canvas = Canvas {
        scene: None,
        pixels: &mut pixels,
        width: 3,
        height: 2,
        scale: 2.0,
    };
    canvas.image(&image, [0.0, 0.0], [2, 2], 0.5);
    assert_eq!(
        pixels,
        [0xff0000, 0x202020, 0x202020, 0x101090, 0xffffff, 0x202020]
    );
    pixels.fill(0x202020);
    let mut canvas = Canvas {
        scene: None,
        pixels: &mut pixels,
        width: 3,
        height: 2,
        scale: 1.0,
    };
    canvas.image(&image, [-1.0, 0.0], [2, 2], 1.0);
    canvas.image(&image, [3.0, 2.0], [2, 2], 1.0);
    assert_eq!(
        pixels,
        [0x202020, 0x202020, 0x202020, 0xffffff, 0x202020, 0x202020]
    );
}

#[test]
fn atlas_blitting_samples_only_the_chosen_tile_under_clipping() {
    let image = Image {
        width: 4,
        height: 2,
        rgba: [
            [255, 0, 0, 255],
            [255, 0, 0, 255],
            [0, 0, 255, 255],
            [0, 0, 255, 255],
        ]
        .concat()
        .repeat(2),
    };
    let mut pixels = [0; 6];
    Canvas {
        scene: None,
        pixels: &mut pixels,
        width: 3,
        height: 2,
        scale: 1.0,
    }
    .image_region(&image, [-1.0, 0.0], [2, 2], 1.0, [2, 0, 2, 2]);
    assert_eq!(pixels, [0xff, 0, 0, 0xff, 0, 0]);
}

#[test]
fn optimized_blit_matches_reference_for_fractional_scaling_clipping_and_alpha() {
    let mut rgba = Vec::new();
    for pixel in 0_u8..35 {
        rgba.extend([
            pixel.wrapping_mul(37),
            pixel.wrapping_mul(13),
            pixel.wrapping_mul(7),
            [0, 1, 128, 254, 255][pixel as usize % 5],
        ]);
    }
    let mut image = Image {
        width: 7,
        height: 5,
        rgba,
    };
    for opaque in [false, true] {
        if opaque {
            for pixel in image.rgba.as_chunks_mut::<4>().0 {
                pixel[3] = 255;
            }
        }
        for scale in [0.75, 1.0, 1.5, 2.0] {
            for zoom in [0.25, 0.6, 1.0, 1.7, 3.0] {
                for origin in [
                    [-4.0, -3.0],
                    [-0.7, 0.125],
                    [0.0, 0.0],
                    [3.25, 2.75],
                    [40.0, 30.0],
                ] {
                    for (world_size, source_rect) in [
                        ([7, 5], [0, 0, 7, 5]),
                        ([29, 23], [1, 1, 3, 2]),
                        ([1, 1], [2, 2, 2, 3]),
                    ] {
                        let mut expected = vec![0x294c73; 31 * 23];
                        let mut actual = expected.clone();
                        reference_blit(
                            &mut Canvas {
                                scene: None,
                                pixels: &mut expected,
                                width: 31,
                                height: 23,
                                scale,
                            },
                            &image,
                            origin,
                            world_size,
                            zoom,
                            source_rect,
                        );
                        Canvas {
                            scene: None,
                            pixels: &mut actual,
                            width: 31,
                            height: 23,
                            scale,
                        }
                        .image_region(
                            &image,
                            origin,
                            world_size,
                            zoom,
                            source_rect,
                        );
                        assert_eq!(
                            actual, expected,
                            "opaque={opaque} scale={scale} zoom={zoom} origin={origin:?} region={source_rect:?}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn solid_fill_matches_slice_fill_and_preserves_neighboring_pixels() {
    for length in [0, 1, 2, 3, 5, 31, 127, 511, 1024] {
        for color in [0, 0x123456, 0xffffff] {
            let mut actual = vec![0x765432; length + 2];
            let mut expected = actual.clone();
            expected[1..length + 1].fill(color);
            Canvas::fill_run(&mut actual[1..length + 1], color);
            assert_eq!(actual, expected);
        }
    }
}

#[test]
fn mirrored_blit_matches_flipped_source_with_clipping_and_fractional_zoom() {
    let image = Image {
        width: 3,
        height: 2,
        rgba: vec![
            255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 90, 10, 30, 0, 5, 80, 20, 128, 30, 50,
            90, 255,
        ],
    };
    let mirrored = Image {
        width: 3,
        height: 2,
        rgba: image
            .rgba
            .as_chunks::<12>()
            .0
            .iter()
            .flat_map(|row| row.as_chunks::<4>().0.iter().rev().flatten().copied())
            .collect(),
    };
    for origin in [[0.0, 0.0], [-1.25, -0.4], [3.5, 2.75]] {
        for zoom in [0.5, 1.0, 1.75, 3.0] {
            let mut actual = vec![0x102030; 15 * 11];
            let mut expected = actual.clone();
            Canvas {
                scene: None,
                pixels: &mut actual,
                width: 15,
                height: 11,
                scale: 1.5,
            }
            .image_mirrored(&image, origin, [3, 2], zoom, true);
            reference_blit(
                &mut Canvas {
                    scene: None,
                    pixels: &mut expected,
                    width: 15,
                    height: 11,
                    scale: 1.5,
                },
                &mirrored,
                origin,
                [3, 2],
                zoom,
                [0, 0, 3, 2],
            );
            assert_eq!(actual, expected, "origin={origin:?}, zoom={zoom}");
        }
    }
}

#[test]
fn gpu_scene_records_geometry_without_touching_a_framebuffer() {
    let image = Image {
        width: 3,
        height: 2,
        rgba: vec![
            255, 0, 0, 255, 0, 255, 0, 128, 0, 0, 255, 0, 50, 80, 20, 255, 60, 30, 90, 255, 0, 120,
            80, 255,
        ],
    };
    fn paint<'a>(canvas: &mut Canvas<'_, 'a>, image: &'a Image) {
        canvas.clear(0x294c73);
        canvas.rect(-1.0, 2.0, 6.0, 2.5, 0x506070);
        canvas.image_region(image, [-0.5, 0.25], [8, 6], 1.25, [1, 0, 2, 2]);
        canvas.image_mirrored(image, [5.25, 3.5], [3, 2], 1.5, true);
    }
    let mut expected = vec![0; 15 * 11];
    paint(
        &mut Canvas {
            scene: None,
            pixels: &mut expected,
            width: 15,
            height: 11,
            scale: 1.5,
        },
        &image,
    );
    let mut scene = crate::gpu::Scene {
        width: 15,
        height: 11,
        clear: 0,
        commands: Vec::new(),
    };
    paint(
        &mut Canvas {
            scene: Some(&mut scene),
            pixels: &mut [],
            width: 15,
            height: 11,
            scale: 1.5,
        },
        &image,
    );
    assert_eq!(
        scene.commands.len(),
        3,
        "one rect and two textured quads, independent of pixel count"
    );
    let mut actual = vec![scene.clear; 15 * 11];
    let mut canvas = Canvas {
        scene: None,
        pixels: &mut actual,
        width: 15,
        height: 11,
        scale: 1.0,
    };
    for command in scene.commands {
        match command {
            crate::gpu::Draw::Rect { rect, color } => canvas.rect(
                rect[0] as f64,
                rect[1] as f64,
                rect[2] as f64,
                rect[3] as f64,
                color,
            ),
            crate::gpu::Draw::Image {
                image,
                rect,
                world_size,
                source_rect,
                flip_x,
                ..
            } => canvas.blit(
                image,
                [rect[0] as f64, rect[1] as f64],
                world_size,
                rect[2] as f64 / f64::from(world_size[0]),
                source_rect,
                flip_x,
            ),
        }
    }
    assert_eq!(
        actual, expected,
        "recording preserves clipping, repetition, opacity, order and mirroring"
    );
}

#[test]
fn hud_images_stretch_and_tint_with_clipped_nearest_samples() {
    let image = Image {
        width: 2,
        height: 1,
        rgba: vec![255, 0, 0, 255, 0, 255, 0, 255],
    };
    let mut pixels = vec![0x112233; 4 * 5];
    let rect = [-1.0, 1.0, 4.0, 3.0];
    Canvas {
        scene: None,
        pixels: &mut pixels,
        width: 4,
        height: 5,
        scale: 1.0,
    }
    .image_stretched(&image, rect, 0x804020);
    assert_eq!(&pixels[..4], &[0x112233; 4]);
    for row in 1..4 {
        assert_eq!(
            &pixels[row * 4..row * 4 + 4],
            &[0x800000, 0x004000, 0x004000, 0x112233]
        );
    }
    assert_eq!(&pixels[16..], &[0x112233; 4]);
    let mut scene = crate::gpu::Scene {
        width: 4,
        height: 5,
        clear: 0,
        commands: vec![],
    };
    Canvas {
        scene: Some(&mut scene),
        pixels: &mut [],
        width: 4,
        height: 5,
        scale: 1.0,
    }
    .image_stretched(&image, rect, 0x804020);
    assert_eq!(scene.commands.len(), 1);
    let crate::gpu::Draw::Image {
        rect: recorded,
        world_size,
        color,
        ..
    } = scene.commands[0]
    else {
        panic!("textured HUD quad expected")
    };
    assert_eq!(recorded, rect.map(|value| value as f32));
    assert_eq!(world_size, [2, 1]);
    assert_eq!(color, 0x804020);
}
#[test]
fn gpu_font_atlas_uses_one_tinted_quad_per_visible_glyph() {
    for scale in [1.0, 2.0] {
        for size in [1.0, 2.0] {
            let mut expected = vec![0x294c73; 160 * 64];
            Canvas {
                scene: None,
                pixels: &mut expected,
                width: 160,
                height: 64,
                scale,
            }
            .text("AB C", 2.0, 2.0, size, 0x72b8de);
            let mut scene = crate::gpu::Scene {
                width: 160,
                height: 64,
                clear: 0x294c73,
                commands: Vec::new(),
            };
            Canvas {
                scene: Some(&mut scene),
                pixels: &mut [],
                width: 160,
                height: 64,
                scale,
            }
            .text("AB C", 2.0, 2.0, size, 0x72b8de);
            assert_eq!(scene.commands.len(), 3);
            let mut actual = vec![scene.clear; 160 * 64];
            for command in scene.commands {
                let crate::gpu::Draw::Image {
                    image,
                    rect,
                    world_size,
                    source_rect,
                    color,
                    flip_x,
                    ..
                } = command
                else {
                    panic!("font must be a textured glyph")
                };
                assert!(std::ptr::eq(image, font_atlas()));
                assert_eq!(color, 0x72b8de);
                let mut tinted = Image {
                    width: image.width,
                    height: image.height,
                    rgba: image.rgba.clone(),
                };
                for pixel in tinted.rgba.as_chunks_mut::<4>().0 {
                    pixel[..3].copy_from_slice(&[
                        (color >> 16) as u8,
                        (color >> 8) as u8,
                        color as u8,
                    ]);
                }
                Canvas {
                    scene: None,
                    pixels: &mut actual,
                    width: 160,
                    height: 64,
                    scale: 1.0,
                }
                .blit(
                    &tinted,
                    [rect[0] as f64, rect[1] as f64],
                    world_size,
                    rect[2] as f64 / 8.0,
                    source_rect,
                    flip_x,
                );
            }
            assert_eq!(actual, expected);
        }
    }
}

#[test]
fn cloak_fade_preserves_the_backdrop_and_source_transparency() {
    let image = Image {
        width: 3,
        height: 1,
        rgba: vec![255, 0, 0, 255, 0, 255, 0, 128, 0, 0, 255, 0],
    };
    let mut pixels = [0x204060; 3];
    Canvas {
        scene: None,
        pixels: &mut pixels,
        width: 3,
        height: 1,
        scale: 1.0,
    }
    .image_cloaked(&image, [0.0, 0.0], [3, 1], 1.0, false);
    assert_eq!(pixels, [0x802437, 0x19694b, 0x204060]);
}

#[test]
fn player_palette_matches_mirroring_fading_and_gpu_command_identity() {
    let image = Image {
        width: 2,
        height: 1,
        rgba: vec![164, 0, 0, 255, 42, 41, 40, 255],
    };
    let colors = straterust_engine::assets::ColorRemap {
        colors: vec![[[164, 0, 0], [12, 72, 204]]],
    };
    let mut pixels = vec![0; 2];
    Canvas {
        scene: None,
        pixels: &mut pixels,
        width: 2,
        height: 1,
        scale: 1.0,
    }
    .blit_remapped(
        &image,
        [0.0, 0.0],
        [2, 1],
        1.0,
        [0, 0, 2, 1],
        true,
        255,
        Some(&colors),
    );
    assert_eq!(pixels, [0x2a2928, 0x0c48cc]);
    let mut scene = crate::gpu::Scene {
        width: 2,
        height: 1,
        clear: 0,
        commands: vec![],
    };
    Canvas {
        scene: Some(&mut scene),
        pixels: &mut [],
        width: 2,
        height: 1,
        scale: 1.0,
    }
    .blit_remapped(
        &image,
        [0.0, 0.0],
        [2, 1],
        1.0,
        [0, 0, 2, 1],
        true,
        110,
        Some(&colors),
    );
    assert!(
        matches!(&scene.commands[0], crate::gpu::Draw::Image { colors: Some(p), flip_x: true, color: 0x6effffff, .. } if std::ptr::eq(*p, &colors))
    );
}
