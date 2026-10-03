//! The limited Terran HUD uses source pixels, not a recreation of the source UI code.
//! Frame meanings and palette-table layouts were cross-checked against the original
//! archive and https://github.com/Wargus/stargus/blob/master/scripts/icons.lua,
//! scripts/terran/icons.lua, scripts/gameui.lua and dataset/palettes.json.
//! Only mapping facts are used; no third-party implementation is incorporated.
//! Windows v1.00 statdata.cpp loads grpwire at 0x499278; its group draw callback
//! at 0x498a10 uses that GRP. statdata.bin controls 33..44 define twelve 33x34
//! slots in six columns/two rows; the client shares their geometry with picking.

use std::io::{Read, Seek};

use anyhow::{Context, Result, ensure};
use straterust_engine::assets::{AssetManifest, Image, UiImageManifest};

use crate::{Archive, Files, MemberReport, add_image, formats, member};

const UNIT_FRAMES: [(u16, usize); 5] = [(1, 0), (2, 7), (3, 106), (4, 109), (5, 111)];
const COMMAND_FRAMES: [(&str, usize); 11] = [
    ("move", 228),
    ("stop", 229),
    ("attack", 230),
    ("gather", 231),
    ("repair", 232),
    ("build", 234),
    ("cancel", 236),
    ("back", 236),
    ("patrol", 254),
    ("hold", 255),
    ("rally", 286),
];

pub fn convert<R: Read + Seek>(
    archive: &mut Archive<R>,
    files: &mut Files,
    assets: &mut AssetManifest,
    members: &mut Vec<MemberReport>,
) -> Result<()> {
    let mut read =
        |path, category| member(archive, path, 1024 * 1024, "stardat", category, members);
    let console = formats::decode_pcx(&read(
        "game\\tconsole.pcx",
        "Terran console artwork; original 640x480 layout retained",
    )?)
    .context("decode game/tconsole.pcx")?;
    ensure!(
        (console.width, console.height) == (640, 480),
        "unsupported Terran console dimensions"
    );
    let icon_colors = formats::decode_pcx(&read(
        "unit\\cmdbtns\\ticon.pcx",
        "Terran command/resource icon palette and enabled-color table",
    )?)
    .context("decode unit/cmdbtns/ticon.pcx")?;
    let palette = command_palette(&icon_colors)?;
    let commands = formats::decode_grp(
        &read(
            "unit\\cmdbtns\\cmdicons.grp",
            "selected Terran unit and command icons",
        )?,
        &palette,
    )
    .context("decode unit/cmdbtns/cmdicons.grp")?;
    expected_frames(&commands, 365, 36, 34)?;
    let resources = decode_resource_icons(
        &read(
            "game\\icons.grp",
            "minerals, gas and Terran supply bar icons",
        )?,
        &palette,
    )
    .context("decode uncompressed game/icons.grp")?;
    let wire_colors = formats::decode_pcx(&read(
        "game\\twire.pcx",
        "selection wireframe palette and health-color table",
    )?)
    .context("decode game/twire.pcx")?;
    let wireframes = formats::decode_grp(
        &read(
            "unit\\wirefram\\wirefram.grp",
            "five selected-unit HUD wireframes; healthy health-color mapping",
        )?,
        &wireframe_palette(&wire_colors)?,
    )
    .context("decode unit/wirefram/wirefram.grp")?;
    expected_frames(&wireframes, 228, 64, 64)?;
    let groupframes = formats::decode_grp(
        &read(
            "unit\\wirefram\\grpwire.grp",
            "32x32 group-selection wireframes",
        )?,
        &wireframe_palette(&wire_colors)?,
    )?;
    expected_frames(&groupframes, 131, 32, 32)?;

    let mut add = |key: &str, image: &Image| -> Result<()> {
        ensure!(
            !assets.ui.iter().any(|item| item.key == key),
            "duplicate imported UI key {key}"
        );
        assets.ui.push(UiImageManifest {
            key: key.into(),
            image: add_image(files, &format!("ui-{key}.srim"), image)?,
        });
        Ok(())
    };
    add("console", &console_image(&console))?;
    for (unit, frame) in UNIT_FRAMES {
        add(&format!("unit.{unit}"), &commands[frame])?;
        // Full wireframes are HUD art, not world selection circles. Buildings
        // retain full wireframes; grpwire includes blank building placeholders.
        add(&format!("wireframe.{unit}"), &wireframes[frame])?;
        if unit <= 2 {
            add(&format!("groupwire.{unit}"), &groupframes[frame])?;
        }
    }
    for (name, frame) in COMMAND_FRAMES {
        add(&format!("command.{name}"), &commands[frame])?;
    }
    for (name, frame) in [("minerals", 0), ("gas", 2), ("supply", 5)] {
        // These frames occupy the top-left 14x14 of a mostly empty 64x64 GRP canvas.
        add(
            &format!("resource.{name}"),
            &resource_icon(&resources[frame])?,
        )?;
    }
    Ok(())
}

fn expected_frames(images: &[Image], count: usize, width: u32, height: u32) -> Result<()> {
    ensure!(
        images.len() == count
            && images
                .iter()
                .all(|image| image.width == width && image.height == height),
        "unexpected source UI GRP frame count or dimensions"
    );
    Ok(())
}

// game/icons.grp has ordinary GRP headers but raw, row-major indexed rectangles,
// without row-offset tables or RLE. This known source path is decoded explicitly;
// a failed compressed GRP is never retried as raw. Shared frame offsets are legal.
fn decode_resource_icons(data: &[u8], palette: &[[u8; 4]; 256]) -> Result<Vec<Image>> {
    const COUNT: usize = 12;
    const TABLE_END: usize = 6 + COUNT * 8;
    ensure!(
        (TABLE_END..=64 * 1024).contains(&data.len()) && data[..6] == [12, 0, 64, 0, 64, 0],
        "unexpected resource GRP size or header"
    );
    let mut images = Vec::with_capacity(COUNT);
    for frame in data[6..TABLE_END].as_chunks::<8>().0 {
        let [x, y, width, height] = [frame[0], frame[1], frame[2], frame[3]].map(usize::from);
        ensure!(
            width > 0 && height > 0 && x + width <= 64 && y + height <= 64,
            "resource GRP rectangle exceeds its canvas"
        );
        let offset = usize::try_from(u32::from_le_bytes(frame[4..8].try_into()?))?;
        ensure!(
            offset >= TABLE_END,
            "resource GRP pixels overlap frame headers"
        );
        let end = offset
            .checked_add(width * height)
            .context("resource GRP offset overflow")?;
        let pixels = data
            .get(offset..end)
            .context("truncated resource GRP pixels")?;
        let mut rgba = vec![0; 64 * 64 * 4];
        for (row, indices) in pixels.chunks_exact(width).enumerate() {
            for (column, index) in indices.iter().enumerate() {
                if *index != 0 {
                    let target = ((y + row) * 64 + x + column) * 4;
                    rgba[target..target + 4].copy_from_slice(&palette[usize::from(*index)]);
                }
            }
        }
        images.push(Image {
            width: 64,
            height: 64,
            rgba,
        });
    }
    Ok(images)
}

pub(super) fn command_palette(image: &formats::IndexedImage) -> Result<[[u8; 4]; 256]> {
    ensure!(
        (image.width, image.height) == (96, 1) && image.pixels.len() == 96,
        "unsupported Terran icon color table"
    );
    let mut palette = image.palette;
    for (entry, index) in palette[..16].iter_mut().zip(&image.pixels[..16]) {
        *entry = image.palette[usize::from(*index)];
    }
    Ok(palette)
}

pub(super) fn wireframe_palette(image: &formats::IndexedImage) -> Result<[[u8; 4]; 256]> {
    ensure!(
        (image.width, image.height) == (24, 1) && image.pixels.len() == 24,
        "unsupported Terran wireframe color table"
    );
    let mut palette = image.palette;
    // Base images use healthy table slot 1. Campaign damage atlases separately
    // preserve the four sections and original executable's health tables.
    palette[208..212].fill(image.palette[usize::from(image.pixels[1])]);
    Ok(palette)
}

fn console_image(image: &formats::IndexedImage) -> Image {
    Image {
        width: image.width,
        height: image.height,
        rgba: image
            .pixels
            .iter()
            .flat_map(|index| {
                if *index == 0 {
                    [0; 4]
                } else {
                    image.palette[usize::from(*index)]
                }
            })
            .collect(),
    }
}

fn resource_icon(image: &Image) -> Result<Image> {
    ensure!(
        image.width == 64 && image.height == 64 && image.rgba.len() == 64 * 64 * 4,
        "unexpected resource icon canvas"
    );
    let mut rgba = Vec::with_capacity(14 * 14 * 4);
    for (y, row) in image.rgba.as_chunks::<{ 64 * 4 }>().0.iter().enumerate() {
        for (x, pixel) in row.as_chunks::<4>().0.iter().enumerate() {
            if y < 14 && x < 14 {
                rgba.extend_from_slice(pixel);
            } else {
                ensure!(
                    pixel[3] == 0,
                    "resource icon artwork exceeds its 14x14 crop"
                );
            }
        }
    }
    ensure!(
        rgba.as_chunks::<4>().0.iter().any(|pixel| pixel[3] != 0),
        "empty resource icon"
    );
    Ok(Image {
        width: 14,
        height: 14,
        rgba,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn indexed(width: u32) -> formats::IndexedImage {
        formats::IndexedImage {
            width,
            height: 1,
            pixels: (0..width).map(|index| index as u8).collect(),
            palette: std::array::from_fn(|index| [index as u8, 17, 23, 255]),
        }
    }

    #[test]
    fn command_palette_uses_original_entries_even_when_remapping_overlaps() {
        let mut image = indexed(96);
        image.pixels[..3].copy_from_slice(&[2, 0, 1]);
        let palette = command_palette(&image).unwrap();
        assert_eq!(
            &palette[..3],
            &[image.palette[2], image.palette[0], image.palette[1]]
        );
        assert_eq!(&palette[16..], &image.palette[16..]);
        image.pixels.pop();
        assert!(command_palette(&image).is_err());
    }

    #[test]
    fn wireframe_remaps_only_health_sections_and_console_preserves_holes() {
        let mut image = indexed(24);
        image.pixels[1] = 117;
        let palette = wireframe_palette(&image).unwrap();
        assert_eq!(&palette[208..212], &[image.palette[117]; 4]);
        assert_eq!(&palette[..208], &image.palette[..208]);
        assert_eq!(&palette[212..], &image.palette[212..]);
        let console = console_image(&image);
        assert_eq!(&console.rgba[..4], &[0; 4]);
        assert_eq!(&console.rgba[4..8], &image.palette[117]);
    }

    #[test]
    fn resource_crop_retains_alpha_and_rejects_art_that_would_be_lost() {
        let mut image = Image {
            width: 64,
            height: 64,
            rgba: vec![0; 64 * 64 * 4],
        };
        let last_pixel = (13 * 64 + 13) * 4;
        image.rgba[last_pixel..last_pixel + 4].copy_from_slice(&[12, 34, 56, 128]);
        let icon = resource_icon(&image).unwrap();
        assert_eq!(
            (icon.width, icon.height, icon.rgba.len()),
            (14, 14, 14 * 14 * 4)
        );
        assert_eq!(&icon.rgba[icon.rgba.len() - 4..], &[12, 34, 56, 128]);
        image.rgba[(14 * 64 + 13) * 4 + 3] = 255;
        assert!(resource_icon(&image).is_err());
        image.rgba.fill(0);
        assert!(resource_icon(&image).is_err());
    }

    #[test]
    fn raw_resource_frames_allow_shared_pixels_and_preserve_offsets_and_transparency() {
        let mut data = vec![12, 0, 64, 0, 64, 0];
        for _ in 0..12 {
            data.extend([3, 2, 2, 1]);
            data.extend(102_u32.to_le_bytes());
        }
        data.extend([0, 5]);
        let palette = indexed(1).palette;
        let images = decode_resource_icons(&data, &palette).unwrap();
        assert_eq!(images.len(), 12);
        let start = (2 * 64 + 3) * 4;
        assert_eq!(&images[0].rgba[start..start + 4], &[0; 4]);
        assert_eq!(&images[0].rgba[start + 4..start + 8], &palette[5]);
        assert_eq!(images[0], images[11]);
        for end in [0, 5, 101, 103] {
            assert!(decode_resource_icons(&data[..end], &palette).is_err());
        }
        for (offset, value) in [(0, 13), (2, 63), (6, 63), (8, 0), (10, 0), (13, 255)] {
            let mut bad = data.clone();
            bad[offset] = value;
            assert!(
                decode_resource_icons(&bad, &palette).is_err(),
                "offset {offset}"
            );
        }
    }
}
