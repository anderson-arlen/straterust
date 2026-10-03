//! Retail v1.00 selection metrics and context pointer artwork.
use super::*;
use straterust_engine::assets::{CursorManifest, IndicatorsManifest, UnitIndicator};

pub(crate) fn refresh_indicators(
    archive: &mut Archive<std::io::Cursor<Vec<u8>>>,
    files: &mut Files,
    assets: &mut AssetManifest,
    rules: &Rules,
) -> Result<()> {
    let units = archive.read_file("arr\\units.dat", 19192)?;
    let flingy = archive.read_file("arr\\flingy.dat", 2760)?;
    let sprites = archive.read_file("arr\\sprites.dat", 2081)?;
    let images = archive.read_file("arr\\images.dat", 28690)?;
    let names = archive.read_file("arr\\images.tbl", 65536)?;
    ensure!(
        units.len() == 19192
            && flingy.len() == 2760
            && sprites.len() == 2081
            && images.len() == 28690,
        "unsupported indicator DAT layout"
    );
    let select = formats::decode_pcx(&archive.read_file("game\\tselect.pcx", 1024 * 1024)?)?;
    let health = formats::decode_pcx(&archive.read_file("game\\thpbar.pcx", 1024 * 1024)?)?;
    ensure!(
        select.pixels.len() == 24 && health.pixels.len() == 19,
        "unsupported indicator palettes"
    );
    let health_colors = std::array::from_fn(|i| {
        let [r, g, b, _] = health.palette[usize::from(health.pixels[i])];
        u32::from(r) << 16 | u32::from(g) << 8 | u32::from(b)
    });
    let mut circles = Vec::new();
    let mut circle_heights = Vec::new();
    for circle in 0..10 {
        let name = terran_media::table_string(&names, dword(&images, (561 + circle) * 4))?;
        let bytes = archive.read_file(&format!("unit\\{name}"), 1024 * 1024)?;
        let mut atlas = None;
        for allegiance in 0..3 {
            let palette = std::array::from_fn(|i| {
                select.palette[usize::from(select.pixels[allegiance * 8 + i.saturating_sub(1) % 8])]
            });
            let mut frames = formats::decode_grp(&bytes, &palette)?;
            ensure!(frames.len() == 1, "selection circle must have one frame");
            let image = frames.remove(0);
            let output = atlas.get_or_insert_with(|| Image {
                width: image.width,
                height: image.height * 3,
                rgba: Vec::new(),
            });
            output.rgba.extend(image.rgba);
        }
        let atlas = atlas.unwrap();
        circles.push(crate::add_image(
            files,
            &format!("selection-circle-{circle}.srim"),
            &atlas,
        )?);
        // Source bar placement uses the cropped frame height, not its canvas.
        circle_heights.push(i16::from(bytes[9]));
    }
    let mut metrics = Vec::new();
    for &(source, native) in MAPPING {
        let unit_type = UnitTypeId(native);
        if !rules.units.iter().any(|unit| unit.id == unit_type) {
            continue;
        }
        let sprite = usize::from(word(&flingy, usize::from(units[usize::from(source)]) * 2));
        // Executable DAT descriptor 0x4e58a0: 386 images and 179 selectable
        // entries, indexed by sprite IDs 130..308 (not a modern 517-row DAT).
        if !(130..309).contains(&sprite) {
            continue;
        }
        let row = sprite - 130;
        let circle = sprites[1723 + row];
        let circle_y = i16::from(sprites[1902 + row]);
        let height = *circle_heights
            .get(usize::from(circle))
            .context("unknown source circle")?;
        let width = u16::from(sprites[772 + row]).max(19);
        metrics.push(UnitIndicator {
            unit_type,
            circle,
            circle_y,
            bar_y: circle_y + height / 2 + 8,
            bar_width: width - (width - 1) % 3,
        });
    }
    let palette = formats::palette(&archive.read_file("tileset\\badlands.wpe", 1024)?)?;
    for resource in &mut assets.resources {
        let source = match resource.kind.as_str() {
            "minerals" => 176,
            "gas" => 188,
            _ => continue,
        };
        let sprite = usize::from(word(&flingy, usize::from(units[source]) * 2));
        ensure!((130..309).contains(&sprite), "invalid resource sprite");
        resource.selection_circle = Some(sprites[1723 + sprite - 130]);
        resource.selection_y = i16::from(sprites[1902 + sprite - 130]);
    }
    let mut cursors = Vec::new();
    for (key, member) in [
        ("arrow", "arrow"),
        ("hover-green", "MagG"),
        ("hover-yellow", "MagY"),
        ("hover-red", "MagR"),
        ("target-green", "TargG"),
        ("target-yellow", "TargY"),
        ("target-red", "TargR"),
        ("target", "TargN"),
        ("drag", "Drag"),
        ("illegal", "Illegal"),
    ] {
        let bytes = archive.read_file(&format!("cursor\\{member}.grp"), 1024 * 1024)?;
        let frames = formats::decode_grp(&bytes, &palette)?;
        let first = &frames[0];
        let mut atlas = Image {
            width: first.width,
            height: first.height * frames.len() as u32,
            rgba: Vec::new(),
        };
        for frame in &frames {
            atlas.rgba.extend_from_slice(&frame.rgba);
        }
        cursors.push(CursorManifest {
            key: key.into(),
            image: crate::add_image(files, &format!("cursor-{key}.srim"), &atlas)?,
            frames: frames.len() as u16,
            // Executable 0x439bb3 advances every 100ms; 0x439c98/0x439cad
            // position each GRP frame relative to the mouse hotspot (63,63).
            frame_ms: 100,
            anchor: [63, 63],
        });
    }
    assets.indicators = Some(IndicatorsManifest {
        circles,
        units: metrics,
        health_colors,
        cursors,
    });
    Ok(())
}
