//! Original TOME console images, composed around their 448x448 map aperture.
use super::{gfx, pud::word, war::WarArchive};
use anyhow::{Result, ensure};
use std::path::Path;
use straterust_engine::assets::*;

pub fn add(
    archive: &WarArchive,
    root: &Path,
    race: usize,
    assets: &mut AssetManifest,
) -> Result<()> {
    let palette = gfx::palette(&archive.entry(2)?)?;
    let mut console = Image {
        width: 640,
        height: 480,
        rgba: vec![0; 640 * 480 * 4],
    };
    for (record, x, y) in [
        (293, 0, 0),
        (295, 0, 24),
        (297, 0, 336),
        (287, 176, 0),
        (289, 624, 0),
        (291, 176, 464),
    ] {
        let bytes = archive.entry(record + race)?;
        let (width, height) = (u32::from(word(&bytes, 0)?), u32::from(word(&bytes, 2)?));
        ensure!(
            bytes.len() == 4 + (width * height) as usize && x + width <= 640 && y + height <= 480,
            "invalid console image"
        );
        let image = Image {
            width,
            height,
            rgba: bytes[4..]
                .iter()
                .flat_map(|i| palette[*i as usize])
                .collect(),
        };
        paste(&mut console, &image, x, y);
    }
    let panels = gfx::raw_sprites(&archive.entry(354 + race)?, &palette)?;
    paste(&mut console, &panels[0], 0, 160);
    assets.ui.push(UiImageManifest {
        key: "console".into(),
        image: gfx::write_image(root, &console)?,
    });
    assets.console_layout = Some(ConsoleLayout {
        menu: [0, 0, 176, 24],
        viewport: ConsoleViewport {
            canvas: [640, 480],
            margins: [176, 16, 16, 16],
        },
        minimap: [24, 26, 128, 128],
        selection: [8, 166, 160, 164],
        group: [9, 170, 46, 34],
        group_step: [56, 40],
        group_columns: 3,
        buttons: [9, 340, 46, 38],
        button_step: [56, 47],
    });
    Ok(())
}

fn paste(dest: &mut Image, src: &Image, x: u32, y: u32) {
    for row in 0..src.height {
        for col in 0..src.width {
            let from = ((row * src.width + col) * 4) as usize;
            if src.rgba[from + 3] != 0 {
                let to = (((y + row) * dest.width + x + col) * 4) as usize;
                dest.rgba[to..to + 4].copy_from_slice(&src.rgba[from..from + 4]);
            }
        }
    }
}
