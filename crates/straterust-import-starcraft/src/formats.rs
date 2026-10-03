//! Decoders for the classic paletted formats used by the limited import preview.
//!
//! Binary layout references (no third-party implementation is incorporated):
//! - GRP: <https://sourceforge.net/p/stratlas/wiki/GRP/> and the header/byte-stream
//!   descriptions in <https://ftp.war2.ru/war2/Modding/Graphics/GRP_Format.pdf>.
//! - WPE, VX4 and VR4: <https://wiki.staredit.net/wiki/Terrain_Format>.
//!
//! GRP returns full canvases so frame offsets survive conversion and all frames
//! retain the same center anchor. Only compressed GRP and classic 16-bit VX4
//! references are supported; palette cycling and player-color remapping are not
//! applied. Transparent GRP runs do not designate a transparent palette index.

use anyhow::{Context, Result, ensure};
use straterust_engine::assets::Image;

const MAX_CANVAS_DIMENSION: usize = 1024;
const MAX_DECODED_BYTES: usize = 64 * 1024 * 1024;

/// Indexed pixels are retained because source PCX files also contain remapping tables.
pub struct IndexedImage {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
    pub palette: [[u8; 4]; 256],
}

/// ZSoft PCX version 5, one 8-bit plane, RLE rows and a trailing 256-color palette.
/// Layout: https://www.fileformat.info/format/pcx/egff.htm . Other PCX modes are rejected.
pub fn decode_pcx(data: &[u8]) -> Result<IndexedImage> {
    ensure!(
        (897..=4 * 1024 * 1024).contains(&data.len()),
        "invalid PCX file size"
    );
    ensure!(
        data[..4] == [10, 5, 1, 8] && data[65] == 1,
        "unsupported PCX encoding"
    );
    let word = |offset| u16::from_le_bytes([data[offset], data[offset + 1]]);
    let width = usize::from(
        word(8)
            .checked_sub(word(4))
            .context("reversed PCX x bounds")?,
    ) + 1;
    let height = usize::from(
        word(10)
            .checked_sub(word(6))
            .context("reversed PCX y bounds")?,
    ) + 1;
    let stride = usize::from(word(66));
    ensure!(
        (1..=MAX_CANVAS_DIMENSION).contains(&width) && (1..=MAX_CANVAS_DIMENSION).contains(&height),
        "PCX dimensions exceed 1024"
    );
    ensure!(
        stride >= width && stride <= MAX_CANVAS_DIMENSION + 1,
        "invalid PCX row stride"
    );
    let palette_offset = data.len() - 769;
    ensure!(data[palette_offset] == 12, "missing PCX 256-color palette");
    let mut palette = [[0; 4]; 256];
    for (color, rgb) in palette
        .iter_mut()
        .zip(data[palette_offset + 1..].as_chunks::<3>().0)
    {
        *color = [rgb[0], rgb[1], rgb[2], 255];
    }
    let mut pixels = Vec::with_capacity(width * height);
    let mut cursor = 128;
    for _ in 0..height {
        let mut column = 0;
        while column < stride {
            ensure!(cursor < palette_offset, "truncated PCX row");
            let control = data[cursor];
            cursor += 1;
            let (count, value) = if control & 0xc0 == 0xc0 {
                ensure!(cursor < palette_offset, "truncated PCX run");
                let value = data[cursor];
                cursor += 1;
                (usize::from(control & 0x3f), value)
            } else {
                (1, control)
            };
            ensure!(
                count != 0 && column + count <= stride,
                "PCX run exceeds its row"
            );
            pixels.extend(std::iter::repeat_n(
                value,
                (column + count)
                    .min(width)
                    .saturating_sub(column.min(width)),
            ));
            column += count;
        }
    }
    ensure!(cursor == palette_offset, "unexpected data after PCX pixels");
    Ok(IndexedImage {
        width: width as u32,
        height: height as u32,
        pixels,
        palette,
    })
}

/// Convert WPE RGB entries to opaque RGBA, ignoring the unused fourth byte.
pub fn palette(data: &[u8]) -> Result<[[u8; 4]; 256]> {
    ensure!(
        data.len() == 1024,
        "WPE palette must contain exactly 1024 bytes"
    );
    let mut result = [[0; 4]; 256];
    for (color, entry) in result.iter_mut().zip(data.as_chunks::<4>().0) {
        *color = [entry[0], entry[1], entry[2], 255];
    }
    Ok(result)
}

/// Decode compressed GRP frames with bounded dimensions and total allocation.
pub fn decode_grp(data: &[u8], palette: &[[u8; 4]; 256]) -> Result<Vec<Image>> {
    let header = bytes(data, 0, 6).context("truncated GRP header")?;
    let frame_count = usize::from(u16::from_le_bytes([header[0], header[1]]));
    let width = usize::from(u16::from_le_bytes([header[2], header[3]]));
    let height = usize::from(u16::from_le_bytes([header[4], header[5]]));
    ensure!(frame_count != 0, "GRP has no frames");
    ensure!(
        (1..=MAX_CANVAS_DIMENSION).contains(&width) && (1..=MAX_CANVAS_DIMENSION).contains(&height),
        "GRP canvas {width}x{height} is outside the supported 1..=1024 dimensions"
    );
    let frame_bytes = width * height * 4;
    ensure!(
        frame_bytes
            .checked_mul(frame_count)
            .is_some_and(|total| total <= MAX_DECODED_BYTES),
        "GRP decoded frames exceed the 64 MiB limit"
    );
    let table_end = 6 + frame_count * 8;
    bytes(data, 6, frame_count * 8).context("truncated GRP frame table")?;

    let mut frames = Vec::with_capacity(frame_count);
    for frame_index in 0..frame_count {
        let frame = &data[6 + frame_index * 8..6 + (frame_index + 1) * 8];
        let x_offset = usize::from(frame[0]);
        let y_offset = usize::from(frame[1]);
        let frame_width = usize::from(frame[2]);
        let frame_height = usize::from(frame[3]);
        let data_offset =
            usize::try_from(u32::from_le_bytes([frame[4], frame[5], frame[6], frame[7]]))?;
        ensure!(
            x_offset + frame_width <= width && y_offset + frame_height <= height,
            "GRP frame {frame_index} extends beyond its {width}x{height} canvas"
        );
        ensure!(
            data_offset >= table_end,
            "GRP frame {frame_index} data overlaps the frame headers"
        );
        let rows = bytes(data, data_offset, frame_height * 2)
            .with_context(|| format!("GRP frame {frame_index}: invalid row-offset table"))?;
        let mut rgba = vec![0; frame_bytes];
        for (row_index, row_offset) in rows.as_chunks::<2>().0.iter().enumerate() {
            let relative = usize::from(u16::from_le_bytes([row_offset[0], row_offset[1]]));
            ensure!(
                relative >= rows.len(),
                "GRP frame {frame_index} row {row_index} data overlaps its row-offset table"
            );
            let row_start = data_offset
                .checked_add(relative)
                .context("GRP row offset overflow")?;
            let row_data = data.get(row_start..).with_context(|| {
                format!("GRP frame {frame_index} row {row_index} offset is outside the file")
            })?;
            let pixel_start = ((y_offset + row_index) * width + x_offset) * 4;
            decode_row(
                row_data,
                &mut rgba[pixel_start..pixel_start + frame_width * 4],
                palette,
            )
            .with_context(|| format!("GRP frame {frame_index} row {row_index}"))?;
        }
        frames.push(Image {
            width: width as u32,
            height: height as u32,
            rgba,
        });
    }
    Ok(frames)
}

fn decode_row(mut data: &[u8], mut output: &mut [u8], palette: &[[u8; 4]; 256]) -> Result<()> {
    while !output.is_empty() {
        let (&control, remaining) = data.split_first().context("truncated RLE control")?;
        data = remaining;
        let count = usize::from(if control & 0x80 != 0 {
            control & 0x7f
        } else if control & 0x40 != 0 {
            control & 0x3f
        } else {
            control
        });
        ensure!(count != 0, "zero-length RLE run (control 0x{control:02x})");
        ensure!(count * 4 <= output.len(), "RLE run exceeds frame row width");
        let (run, rest) = output.split_at_mut(count * 4);
        output = rest;
        if control & 0x80 != 0 {
            // The canvas starts transparent, including padding outside the frame.
            continue;
        }
        if control & 0x40 != 0 {
            let (&index, remaining) = data.split_first().context("truncated RLE repeat color")?;
            data = remaining;
            for pixel in run.as_chunks_mut::<4>().0 {
                pixel.copy_from_slice(&palette[usize::from(index)]);
            }
        } else {
            let indices = bytes(data, 0, count).context("truncated RLE literal colors")?;
            data = &data[count..];
            for (pixel, index) in run.as_chunks_mut::<4>().0.iter_mut().zip(indices) {
                pixel.copy_from_slice(&palette[usize::from(*index)]);
            }
        }
    }
    Ok(())
}

/// Assemble one 32x32 terrain tile from sixteen 8x8 VR4 minitiles.
pub fn decode_tile(
    vx4: &[u8],
    vr4: &[u8],
    palette: &[[u8; 4]; 256],
    tile_index: usize,
) -> Result<Image> {
    ensure!(
        !vx4.is_empty() && vx4.len().is_multiple_of(32),
        "classic VX4 length must be a nonzero multiple of 32 bytes"
    );
    ensure!(
        !vr4.is_empty() && vr4.len().is_multiple_of(64),
        "VR4 length must be a nonzero multiple of 64 bytes"
    );
    let tile_offset = tile_index
        .checked_mul(32)
        .context("VX4 tile index overflow")?;
    let tile = bytes(vx4, tile_offset, 32)
        .with_context(|| format!("VX4 tile {tile_index} is outside the tile table"))?;
    let mut rgba = vec![0; 32 * 32 * 4];
    for (position, reference) in tile.as_chunks::<2>().0.iter().enumerate() {
        let reference = u16::from_le_bytes([reference[0], reference[1]]);
        let mini_index = usize::from(reference >> 1);
        let flipped = reference & 1 != 0;
        let mini = bytes(vr4, mini_index * 64, 64).with_context(|| {
            format!("VX4 tile {tile_index} minitile {position} references missing VR4 {mini_index}")
        })?;
        for y in 0..8 {
            for x in 0..8 {
                let source_x = if flipped { 7 - x } else { x };
                let color = palette[usize::from(mini[y * 8 + source_x])];
                let destination = ((position / 4 * 8 + y) * 32 + position % 4 * 8 + x) * 4;
                rgba[destination..destination + 4].copy_from_slice(&color);
            }
        }
    }
    Ok(Image {
        width: 32,
        height: 32,
        rgba,
    })
}

fn bytes(data: &[u8], offset: usize, length: usize) -> Result<&[u8]> {
    let end = offset.checked_add(length).context("byte range overflow")?;
    data.get(offset..end)
        .with_context(|| format!("byte range {offset}..{end} exceeds {} bytes", data.len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_palette() -> [[u8; 4]; 256] {
        std::array::from_fn(|index| [index as u8, 255 - index as u8, 11, 255])
    }

    // Original hand-encoded fixture, unrelated to proprietary artwork. Its two
    // rows exercise all three instructions and distinguish both spatial axes.
    fn grp_fixture() -> Vec<u8> {
        let mut data = vec![
            1, 0, 9, 0, 4, 0, // frame count and canvas
            1, 1, 7, 2, 14, 0, 0, 0, // frame position, extent, and data offset
            4, 0, 12, 0, // row offsets relative to the frame data
        ];
        data.extend_from_slice(&[0x81, 0x42, 4, 3, 0, 5, 6, 0x81]);
        data.extend_from_slice(&[0x47, 7]);
        data
    }

    fn pixel(image: &Image, x: usize, y: usize) -> &[u8] {
        let offset = (y * image.width as usize + x) * 4;
        &image.rgba[offset..offset + 4]
    }

    #[test]
    fn wpe_preserves_rgb_and_ignores_reserved_bytes() {
        let mut data = vec![0; 1024];
        data[..4].copy_from_slice(&[1, 2, 3, 0]);
        data[1020..].copy_from_slice(&[4, 5, 6, 99]);
        let colors = palette(&data).unwrap();
        assert_eq!(colors[0], [1, 2, 3, 255]);
        assert_eq!(colors[255], [4, 5, 6, 255]);
        assert!(palette(&data[..1023]).is_err());
        data.push(0);
        assert!(palette(&data).is_err());
    }

    #[test]
    fn grp_preserves_canvas_offsets_runs_and_literal_palette_zero() {
        let colors = test_palette();
        let frames = decode_grp(&grp_fixture(), &colors).unwrap();
        assert_eq!(frames.len(), 1);
        let image = &frames[0];
        assert_eq!((image.width, image.height), (9, 4));
        for (x, index) in [(2, 4), (3, 4), (4, 0), (5, 5), (6, 6)] {
            assert_eq!(pixel(image, x, 1), colors[index]);
        }
        for x in 1..8 {
            assert_eq!(pixel(image, x, 2), colors[7]);
        }
        for (x, y) in [(0, 0), (1, 1), (7, 1), (8, 1), (0, 2), (8, 2), (8, 3)] {
            assert_eq!(pixel(image, x, y), [0; 4]);
        }
    }

    #[test]
    fn grp_allows_shared_frame_and_row_data() {
        let original = grp_fixture();
        let mut shared = original[..14].to_vec();
        shared[0] = 2;
        shared[10] = 22;
        shared.extend_from_slice(&[1, 1, 7, 2, 22, 0, 0, 0]);
        shared.extend_from_slice(&original[14..]);
        // Both rows use the second row's repeated color instruction.
        shared[22] = 12;
        let frames = decode_grp(&shared, &test_palette()).unwrap();
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].rgba, frames[1].rgba);
        assert_eq!(pixel(&frames[0], 1, 1), test_palette()[7]);
        assert_eq!(pixel(&frames[0], 1, 2), test_palette()[7]);
    }

    #[test]
    fn grp_rejects_truncation_bad_offsets_and_out_of_canvas_frames() {
        let original = grp_fixture();
        for end in 0..original.len() {
            assert!(
                decode_grp(&original[..end], &test_palette()).is_err(),
                "{end}"
            );
        }
        for (offset, value) in [(6, 3), (7, 3), (10, 0), (10, 255), (14, 0), (14, 255)] {
            let mut corrupt = original.clone();
            corrupt[offset] = value;
            assert!(
                decode_grp(&corrupt, &test_palette()).is_err(),
                "{offset}={value}"
            );
        }
    }

    #[test]
    fn grp_rejects_nonprogressing_runs_and_row_overflows() {
        for control in [0, 0x40, 0x80, 0x88, 0x48, 8] {
            let mut corrupt = grp_fixture();
            corrupt[18] = control;
            assert!(
                decode_grp(&corrupt, &test_palette()).is_err(),
                "{control:#x}"
            );
        }
    }

    #[test]
    fn grp_rejects_excessive_canvas_and_decoded_size_before_allocating() {
        for header in [
            [0, 0, 9, 0, 4, 0],  // no frames
            [1, 0, 0, 0, 4, 0],  // zero width
            [1, 0, 1, 4, 4, 0],  // width 1025
            [17, 0, 0, 4, 0, 4], // 17 RGBA canvases of 1024x1024
        ] {
            assert!(decode_grp(&header, &test_palette()).is_err());
        }
    }

    #[test]
    fn terrain_uses_row_major_minitiles_and_horizontal_flip_only() {
        let colors = test_palette();
        let mut vx4 = vec![0; 32];
        vx4[2] = 1; // same first VR4, horizontally flipped
        vx4[8] = 2; // next row begins with second VR4
        let vr4: Vec<u8> = (0..128).collect();
        let image = decode_tile(&vx4, &vr4, &colors, 0).unwrap();
        assert_eq!((image.width, image.height), (32, 32));
        assert_eq!(pixel(&image, 0, 0), colors[0]);
        assert_eq!(pixel(&image, 7, 7), colors[63]);
        assert_eq!(pixel(&image, 8, 0), colors[7]);
        assert_eq!(pixel(&image, 15, 0), colors[0]);
        assert_eq!(pixel(&image, 8, 7), colors[63]);
        assert_eq!(pixel(&image, 0, 8), colors[64]);
        assert_eq!(pixel(&image, 7, 15), colors[127]);
        assert_eq!(pixel(&image, 31, 31), colors[63]);
    }

    #[test]
    fn terrain_rejects_partial_tables_missing_references_and_large_indices() {
        let colors = test_palette();
        let mut vx4 = vec![0; 32];
        let vr4 = vec![0; 64];
        assert!(decode_tile(&[], &vr4, &colors, 0).is_err());
        assert!(decode_tile(&vx4, &[], &colors, 0).is_err());
        assert!(decode_tile(&vx4[..31], &vr4, &colors, 0).is_err());
        assert!(decode_tile(&vx4, &vr4[..63], &colors, 0).is_err());
        assert!(decode_tile(&vx4, &vr4, &colors, 1).is_err());
        assert!(decode_tile(&vx4, &vr4, &colors, usize::MAX).is_err());
        vx4[0] = 2;
        assert!(decode_tile(&vx4, &vr4, &colors, 0).is_err());
    }
    fn pcx_fixture() -> Vec<u8> {
        let mut bytes = vec![0; 128];
        bytes[..4].copy_from_slice(&[10, 5, 1, 8]);
        bytes[8] = 2; // Three visible pixels, one padding byte per row.
        bytes[10] = 1;
        bytes[65] = 1;
        bytes[66] = 4;
        bytes.extend([0xc2, 7, 9, 0, 1, 2, 3, 0]);
        bytes.push(12);
        for i in 0..=255_u8 {
            bytes.extend([i, i / 2, 0]);
        }
        bytes
    }

    #[test]
    fn pcx_decodes_runs_palette_and_row_padding() {
        let decoded = decode_pcx(&pcx_fixture()).unwrap();
        assert_eq!((decoded.width, decoded.height), (3, 2));
        assert_eq!(decoded.pixels, [7, 7, 9, 1, 2, 3]);
        assert_eq!(decoded.palette[7], [7, 3, 0, 255]);
    }

    #[test]
    fn pcx_rejects_truncated_runs_row_overflow_and_unsupported_modes() {
        let original = pcx_fixture();
        for end in [0, 127, 896, original.len() - 1] {
            assert!(decode_pcx(&original[..end]).is_err());
        }
        for (offset, value) in [(1, 4), (3, 4), (65, 3), (66, 2), (128, 0xc0), (128, 0xc5)] {
            let mut bad = original.clone();
            bad[offset] = value;
            assert!(decode_pcx(&bad).is_err(), "offset {offset}");
        }
        let mut truncated = original.clone();
        truncated.drain(128..136);
        truncated.insert(128, 0xc1);
        assert!(decode_pcx(&truncated).is_err());
    }
}
