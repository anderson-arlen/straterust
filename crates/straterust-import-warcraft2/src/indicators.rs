//! Original cursor pixels and rectangular selection bounds.
use super::{gfx, pud::word, war::WarArchive};
use anyhow::{Result, ensure};
use std::{collections::BTreeMap, path::Path};
use straterust_engine::{assets::*, sim::*};

pub fn add(
    archive: &WarArchive,
    root: &Path,
    palette: &gfx::Palette,
    rules: &Rules,
    race: usize,
    assets: &mut AssetManifest,
) -> Result<()> {
    let mut circles = Vec::new();
    let mut units = Vec::new();
    let mut shapes = BTreeMap::new();
    for unit in &rules.units {
        let width = u32::from(unit.footprint.width).max(28) + 2;
        let height = u32::from(unit.footprint.height).max(28) + 2;
        let circle = rectangle(root, width, height, &mut shapes, &mut circles)?;
        units.push(UnitIndicator {
            unit_type: unit.id,
            circle,
            circle_y: 0,
            bar_y: (height / 2 + 3) as i16,
            bar_width: width as u16,
        });
    }
    for resource in &mut assets.resources {
        let width = if resource.kind == "wood" { 34 } else { 98 };
        resource.selection_circle = Some(rectangle(root, width, width, &mut shapes, &mut circles)?);
        resource.selection_y = 0;
    }
    let mut cursors = Vec::new();
    for (key, record) in [
        ("arrow", 301 + race),
        ("target-yellow", 305 + race),
        ("target-red", 307 + race),
        ("target-green", 309 + race),
        ("magnifier", 311),
        ("target", 312),
        ("drag", 301 + race),
        ("illegal", 303 + race),
        ("scroll-n", 315),
        ("scroll-ne", 316),
        ("scroll-e", 317),
        ("scroll-se", 318),
        ("scroll-s", 319),
        ("scroll-sw", 320),
        ("scroll-w", 321),
        ("scroll-nw", 322),
    ] {
        let bytes = archive.entry(record)?;
        let anchor = [word(&bytes, 0)?, word(&bytes, 2)?];
        let width = u32::from(word(&bytes, 4)?);
        let height = u32::from(word(&bytes, 6)?);
        ensure!(
            width > 0
                && height > 0
                && width <= 256
                && height <= 256
                && bytes.len() == 8 + (width * height) as usize,
            "invalid source cursor"
        );
        let rgba = bytes[8..]
            .iter()
            .flat_map(|n| {
                if *n == 0 {
                    [0; 4]
                } else {
                    palette[usize::from(*n)]
                }
            })
            .collect();
        cursors.push(CursorManifest {
            key: key.into(),
            image: gfx::write_image(
                root,
                &Image {
                    width,
                    height,
                    rgba,
                },
            )?,
            frames: 1,
            frame_ms: 100,
            anchor,
        });
    }
    assets.indicators = Some(IndicatorsManifest {
        segmented_bars: false,
        circles,
        units,
        cursors,
        health_colors: [
            0x00fc00, 0x00b800, 0x007c00, 0xfcfc00, 0xb8b800, 0x7c7c00, 0xfc0000, 0xb80000,
            0x7c0000, 0x0080fc, 0x0060b8, 0x00407c, 0xfcfcfc, 0xb8b8b8, 0x7c7c7c, 0x383838,
            0x282828, 0x181818, 0,
        ],
    });
    Ok(())
}

fn rectangle(
    root: &Path,
    width: u32,
    height: u32,
    shapes: &mut BTreeMap<(u32, u32), u8>,
    circles: &mut Vec<ImageRef>,
) -> Result<u8> {
    if let Some(index) = shapes.get(&(width, height)) {
        return Ok(*index);
    }
    let mut rgba = vec![0; (width * height * 3 * 4) as usize];
    for (row, color) in [[0, 252, 0, 255], [252, 252, 0, 255], [252, 0, 0, 255]]
        .into_iter()
        .enumerate()
    {
        for y in 0..height {
            for x in 0..width {
                if x == 0 || y == 0 || x + 1 == width || y + 1 == height {
                    let i = ((row as u32 * height + y) * width + x) as usize * 4;
                    rgba[i..i + 4].copy_from_slice(&color);
                }
            }
        }
    }
    let index = circles.len() as u8;
    circles.push(gfx::write_image(
        root,
        &Image {
            width,
            height: height * 3,
            rgba,
        },
    )?);
    shapes.insert((width, height), index);
    Ok(index)
}
