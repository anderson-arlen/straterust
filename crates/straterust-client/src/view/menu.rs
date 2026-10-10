use super::*;
use crate::{
    Config,
    menus::{MenuUi, Page, Pick},
};
use straterust_engine::menus::MenuAction;

pub(crate) fn menu_rect(rect: [u16; 4], size: [f64; 2]) -> [f64; 4] {
    let scale = (size[0] / 640.0).min(size[1] / 480.0);
    let [x, y, w, h] = rect.map(f64::from);
    [
        (size[0] - 640.0 * scale) / 2.0 + x * scale,
        (size[1] - 480.0 * scale) / 2.0 + y * scale,
        w * scale,
        h * scale,
    ]
}

pub(crate) fn draw_menu<'a>(
    scene: &mut crate::gpu::Scene<'a>,
    menu: &'a MenuUi,
    config: &Config,
    cursor: [f64; 2],
    elapsed: u128,
    dpi: f64,
    overlay: bool,
) {
    let size = [f64::from(scene.width) / dpi, f64::from(scene.height) / dpi];
    let mut canvas = Canvas {
        width: scene.width as usize,
        height: scene.height as usize,
        scene: Some(scene),
        pixels: &mut [],
        scale: dpi,
    };
    paint(&mut canvas, menu, config, cursor, elapsed, size, overlay);
}

fn paint<'a>(
    canvas: &mut Canvas<'_, 'a>,
    menu: &'a MenuUi,
    config: &Config,
    cursor: [f64; 2],
    elapsed: u128,
    size: [f64; 2],
    overlay: bool,
) {
    let scale = (size[0] / 640.0).min(size[1] / 480.0);
    let rect = |r| menu_rect(r, size);
    if overlay {
        // Keep the stopped battlefield visible around the modal panel.
        let [x, y, w, h] = rect([60, 44, 520, 424]);
        canvas.rect(x, y, w, h, 0x090f16);
        canvas.outline(x, y, w, h, 0x6b8697);
    } else {
        canvas.clear(0x080d15);
        let background = match &menu.page {
            Page::Authored(id) => menu
                .pack
                .manifest
                .screen(id)
                .and_then(|s| s.background.as_ref()),
            _ => None,
        }
        .or(menu.pack.manifest.background.as_ref());
        if let Some(image) = background.and_then(|r| menu.pack.image(r)) {
            let cover = (size[0] / f64::from(image.width)).max(size[1] / f64::from(image.height));
            canvas.image(
                image,
                [
                    (size[0] - f64::from(image.width) * cover) / 2.0,
                    (size[1] - f64::from(image.height) * cover) / 2.0,
                ],
                [image.width, image.height],
                cover,
            );
        }
    }
    let heading = menu.title();
    let title_y = match &menu.page {
        Page::Authored(id) => menu.pack.manifest.screen(id).map_or(44, |s| s.title_y),
        _ => 44,
    };
    let [_, y, _, _] = rect([20, title_y, 600, 24]);
    let text_scale = if matches!(menu.page, Page::Authored(_)) {
        1.8
    } else {
        2.0
    } * scale;
    canvas.text(
        &heading,
        (size[0] - heading.len() as f64 * 8.0 * text_scale) / 2.0,
        y,
        text_scale,
        0xf1cf73,
    );
    if menu.page == Page::Packages && menu.message.is_empty() {
        let [x, y, _, _] = rect([60, 82, 520, 16]);
        canvas.text(
            if menu.games.is_empty() {
                "No games installed. Choose Import a game to begin."
            } else {
                "Select an installed game package"
            },
            x,
            y,
            scale,
            0xb6c9d6,
        );
    }
    if let Page::ImportSource(importer) | Page::Importing(importer) = menu.page {
        let text = if matches!(menu.page, Page::Importing(_)) {
            "Converting game data. Please keep this window open."
        } else {
            importer.description()
        };
        let [x, y, _, _] = rect([60, 105, 520, 16]);
        canvas.text(text, x, y, scale, 0xb6c9d6);
        if let Ok(root) = straterust_importers::games_directory() {
            let text = format!("Install to: {}", root.join(importer.name()).display());
            for (i, line) in wrapped_lines(&text, 66).iter().take(3).enumerate() {
                let [x, y, _, _] = rect([60, 300 + i as u16 * 14, 520, 16]);
                canvas.text(line, x, y, scale, 0xb6c9d6);
            }
            let [x, y, _, _] = rect([60, 356, 520, 16]);
            canvas.text(
                "Reimporting replaces this game's installed data.",
                x,
                y,
                scale,
                0xb6c9d6,
            );
        }
    }
    if menu.page == Page::Multiplayer {
        let [x, y, w, h] = rect([80, 90, 480, 28]);
        canvas.rect(x, y, w, h, 0x172431);
        canvas.outline(x, y, w, h, 0xe4b957);
        canvas.text(
            &format!("Address: {}_", menu.address),
            x + 8.0 * scale,
            y + 8.0 * scale,
            scale,
            0xe2eee4,
        );
    }
    if menu.page == Page::Results {
        super::results::draw_results(canvas, menu, size);
    }
    for (index, choice) in menu.choices(config).iter().enumerate() {
        let [x, y, w, h] = rect(choice.button.rect);
        let hovered = crate::controls::contains([x, y, w, h], cursor) || menu.focus == Some(index);
        let disabled = matches!(choice.pick, Pick::Action(MenuAction::Unavailable(_)));
        let animation = if hovered {
            choice.button.hover.as_ref().or(choice.button.idle.as_ref())
        } else {
            choice.button.idle.as_ref()
        };
        if let Some(image) = animation.and_then(|a| menu.pack.frame(a, elapsed)) {
            // Native transparent animation canvases fit without distorting art.
            let zoom = (w / f64::from(image.width)).min(h / f64::from(image.height));
            canvas.image(
                image,
                [
                    x + (w - f64::from(image.width) * zoom) / 2.0,
                    y + (h - f64::from(image.height) * zoom) / 2.0,
                ],
                [image.width, image.height],
                zoom,
            );
        } else if let Some(image) = menu
            .pack
            .manifest
            .button_image
            .as_ref()
            .and_then(|r| menu.pack.image(r))
        {
            canvas.image_stretched(
                image,
                [x, y, w, h],
                if disabled {
                    0xff666666
                } else if hovered {
                    0xffffffff
                } else {
                    0xffbac4ce
                },
            );
        } else {
            canvas.rect(
                x,
                y,
                w,
                h,
                if hovered && !disabled {
                    0x30445a
                } else {
                    0x172431
                },
            );
            canvas.outline(
                x,
                y,
                w,
                h,
                if hovered && !disabled {
                    0xe4b957
                } else {
                    0x526779
                },
            );
        }
        let label_scale = scale.min(w / (choice.button.label.len().max(1) as f64 * 8.0 + 16.0));
        let label_y = if animation.is_some() {
            y + h - 16.0 * scale
        } else {
            y + (h - 8.0 * label_scale) / 2.0
        };
        canvas.text(
            &choice.button.label,
            x + (w - choice.button.label.len() as f64 * 8.0 * label_scale) / 2.0,
            label_y,
            label_scale,
            if disabled {
                0x718092
            } else if hovered {
                0xffe49e
            } else {
                0xdce5ed
            },
        );
    }
    if matches!(menu.page, Page::Objectives | Page::Help | Page::Confirm(_)) {
        for (i, line) in menu
            .details
            .iter()
            .flat_map(|s| wrapped_lines(s, 56))
            .take(24)
            .enumerate()
        {
            let [x, y, _, _] = rect([96, 105 + i as u16 * 12, 448, 12]);
            canvas.text(&line, x, y, scale, 0xdce5ed);
        }
    }
    if !menu.message.is_empty() {
        let lines = wrapped_lines(&menu.message, 70);
        for (i, line) in lines.iter().take(2).enumerate() {
            let top = if menu.page == Page::Packages { 80 } else { 410 };
            let [x, y, _, _] = rect([40, top + i as u16 * 12, 560, 12]);
            canvas.text(line, x, y, scale, 0xffc78a);
        }
    }
    if let Some(image) = menu
        .pack
        .manifest
        .cursor
        .as_ref()
        .and_then(|a| menu.pack.frame(a, elapsed))
    {
        let anchor = menu.pack.manifest.cursor_anchor.map(f64::from);
        canvas.image(
            image,
            [cursor[0] - anchor[0] * scale, cursor[1] - anchor[1] * scale],
            [image.width, image.height],
            scale,
        );
    }
}

#[cfg(test)]
pub(crate) fn draw_menu_pixels(
    menu: &MenuUi,
    config: &Config,
    pixels: &mut [u32],
    width: u32,
    height: u32,
) {
    let mut canvas = Canvas {
        scene: None,
        pixels,
        width: width as usize,
        height: height as usize,
        scale: 1.0,
    };
    paint(
        &mut canvas,
        menu,
        config,
        [-1000.0, -1000.0],
        0,
        [f64::from(width), f64::from(height)],
        false,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gpu::{Draw, Scene};
    use straterust_engine::{assets::ImageRef, menus::MenuAnimation};

    #[test]
    #[ignore = "requires private campaign menus; set STRATERUST_CAMPAIGN_PACKAGE"]
    fn native_pause_menu_pointer_alignment_review() {
        use std::{
            fs::File,
            io::{BufWriter, Write},
            path::PathBuf,
        };
        let root = PathBuf::from(std::env::var_os("STRATERUST_CAMPAIGN_PACKAGE").unwrap());
        let mut menu = MenuUi::new(vec![]);
        menu.pack = straterust_engine::menus::MenuPack::load(&root)
            .unwrap()
            .unwrap();
        menu.open_pause();
        assert_eq!(menu.pack.manifest.cursor_anchor, [63, 63]);
        let config = Config::default();
        let [width, height] = [1886, 1344];
        let size = [f64::from(width), f64::from(height)];
        let choices = menu.choices(&config);
        let end = choices
            .iter()
            .find(|choice| choice.pick == Pick::Action(MenuAction::EndMission))
            .unwrap();
        let [x, y, w, h] = menu_rect(end.button.rect, size);
        let cursor = [x + w / 2.0, y + h / 2.0];
        let mut pixels = vec![0; width as usize * height as usize];
        paint(
            &mut Canvas {
                scene: None,
                pixels: &mut pixels,
                width: width as usize,
                height: height as usize,
                scale: 1.0,
            },
            &menu,
            &config,
            cursor,
            0,
            size,
            true,
        );
        let arrow = menu
            .pack
            .frame(menu.pack.manifest.cursor.as_ref().unwrap(), 0)
            .unwrap();
        let tip = (63 * arrow.width as usize + 63) * 4;
        assert_eq!(arrow.rgba[tip + 3], 255);
        let color = u32::from(arrow.rgba[tip]) << 16
            | u32::from(arrow.rgba[tip + 1]) << 8
            | u32::from(arrow.rgba[tip + 2]);
        assert_eq!(
            pixels[cursor[1].ceil() as usize * width as usize + cursor[0].ceil() as usize],
            color
        );
        let mut output = BufWriter::new(File::create("/tmp/straterust-menu-pointer.ppm").unwrap());
        writeln!(output, "P6\n{width} {height}\n255").unwrap();
        for pixel in pixels {
            output
                .write_all(&[(pixel >> 16) as u8, (pixel >> 8) as u8, pixel as u8])
                .unwrap();
        }
    }

    #[test]
    fn menu_cursor_tip_and_button_highlight_share_the_mouse_position_at_each_scale() {
        let mut menu = MenuUi::new(vec![]);
        menu.open_pause();
        let references: Vec<_> = (0..2)
            .map(|index| ImageRef {
                file: format!("arrow-{index}.srim"),
                blake3: "0".repeat(64),
            })
            .collect();
        for reference in &references {
            let mut image = Image {
                width: 128,
                height: 128,
                rgba: vec![0; 128 * 128 * 4],
            };
            image.rgba[(63 * 128 + 63) * 4..(63 * 128 + 63) * 4 + 4]
                .copy_from_slice(&[0, 255, 0, 255]);
            menu.pack.images.insert(reference.file.clone(), image);
        }
        menu.pack.manifest.cursor = Some(MenuAnimation {
            frame_ms: 66,
            frames: references,
        });
        menu.pack.manifest.cursor_anchor = [63, 63];
        let config = Config::default();
        let choices = menu.choices(&config);
        let end = choices
            .iter()
            .find(|choice| choice.pick == Pick::Action(MenuAction::EndMission))
            .unwrap();
        for [width, height] in [[640, 480], [1100, 760], [1886, 1344]] {
            for dpi in [1.0, 1.25, 1.5, 2.0] {
                let size = [f64::from(width) / dpi, f64::from(height) / dpi];
                let button = menu_rect(end.button.rect, size);
                let cursor = [button[0] + button[2] / 2.0, button[1] + button[3] / 2.0];
                let hit = choices
                    .iter()
                    .find(|choice| {
                        crate::controls::contains(menu_rect(choice.button.rect, size), cursor)
                    })
                    .unwrap();
                assert_eq!(hit.pick, end.pick);
                for elapsed in [0, 66] {
                    let mut scene = Scene {
                        width,
                        height,
                        clear: 0,
                        commands: vec![],
                    };
                    draw_menu(&mut scene, &menu, &config, cursor, elapsed, dpi, true);
                    let highlight: Vec<_> = scene
                        .commands
                        .iter()
                        .filter_map(|draw| match draw {
                            Draw::Rect {
                                rect,
                                color: 0x30445a,
                            } => Some(*rect),
                            _ => None,
                        })
                        .collect();
                    assert_eq!(highlight.len(), 1);
                    for (actual, expected) in highlight[0].iter().zip(button) {
                        assert!((f64::from(*actual) - expected * dpi).abs() < 2.0);
                    }
                    let Draw::Image { rect, .. } = scene.commands.last().unwrap() else {
                        panic!("cursor missing")
                    };
                    let tip = [
                        rect[0] + 63.0 * rect[2] / 128.0,
                        rect[1] + 63.0 * rect[3] / 128.0,
                    ];
                    for axis in 0..2 {
                        assert!(
                            (f64::from(tip[axis]) - cursor[axis] * dpi).abs() < 0.001,
                            "tip displaced at {width}x{height}, dpi={dpi}, phase={elapsed}"
                        );
                    }
                }
            }
        }
    }
}
