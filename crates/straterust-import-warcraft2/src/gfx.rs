//! Palette and sparse row decoding for original WAR sprite records.
use super::pud::word;
use anyhow::{Context, Result, ensure};
use std::{fs, path::Path};
use straterust_engine::assets::{Image, ImageRef, encode_image};

pub type Palette = [[u8; 4]; 256];

pub fn palette(bytes: &[u8]) -> Result<Palette> {
    ensure!(bytes.len() == 768, "invalid palette length");
    let mut result = [[0; 4]; 256];
    for (rgba, rgb) in result.iter_mut().zip(bytes.as_chunks::<3>().0) {
        ensure!(
            rgb.iter().all(|v| *v <= 63),
            "palette component exceeds six bits"
        );
        *rgba = [rgb[0] * 4, rgb[1] * 4, rgb[2] * 4, 255];
    }
    Ok(result)
}

pub fn sprites(bytes: &[u8], palette: &Palette) -> Result<Vec<Image>> {
    let count = usize::from(word(bytes, 0)?);
    let width = usize::from(word(bytes, 2)?);
    let height = usize::from(word(bytes, 4)?);
    ensure!(
        (1..=512).contains(&count) && (1..=512).contains(&width) && (1..=512).contains(&height),
        "invalid sprite canvas"
    );
    ensure!(
        count * width * height * 4 <= 64 * 1024 * 1024,
        "sprite allocation exceeds limit"
    );
    let mut result = Vec::with_capacity(count);
    for i in 0..count {
        let header = bytes
            .get(6 + i * 8..14 + i * 8)
            .context("truncated sprite frame table")?;
        let [x, y, w, h] = [
            header[0] as usize,
            header[1] as usize,
            header[2] as usize,
            header[3] as usize,
        ];
        ensure!(
            x + w <= width && y + h <= height,
            "sprite frame outside canvas"
        );
        let base = u32::from_le_bytes(header[4..].try_into()?) as usize;
        let mut rgba = vec![0; width * height * 4];
        for row in 0..h {
            let mut p = base
                .checked_add(usize::from(word(bytes, base + row * 2)?))
                .context("sprite row overflow")?;
            let mut col = 0;
            while col < w {
                let code = *bytes.get(p).context("truncated sprite run")?;
                p += 1;
                let skip = code & 0x80 != 0;
                let repeat = !skip && code & 0x40 != 0;
                let length = usize::from(code & if skip { 0x7f } else { 0x3f });
                ensure!(length > 0 && col + length <= w, "invalid sprite run length");
                if !skip {
                    for pixel in 0..length {
                        let index = *bytes
                            .get(p + if repeat { 0 } else { pixel })
                            .context("truncated sprite pixels")?;
                        let dest = ((y + row) * width + x + col + pixel) * 4;
                        rgba[dest..dest + 4].copy_from_slice(&palette[index as usize]);
                    }
                    p += if repeat { 1 } else { length };
                }
                col += length;
            }
        }
        result.push(Image {
            width: width as u32,
            height: height as u32,
            rgba,
        });
    }
    Ok(result)
}

pub fn write_image(root: &Path, image: &Image) -> Result<ImageRef> {
    let bytes = encode_image(image)?;
    let hash = blake3::hash(&bytes).to_hex().to_string();
    let file = format!("{hash}.srim");
    let path = root.join(&file);
    if !path.exists() {
        fs::write(path, bytes)?;
    }
    Ok(ImageRef { file, blake3: hash })
}

/// GFU console panels share the frame table, with packed pixels instead of RLE rows.
pub fn raw_sprites(bytes: &[u8], palette: &Palette) -> Result<Vec<Image>> {
    let count = usize::from(word(bytes, 0)?);
    let width = usize::from(word(bytes, 2)?);
    let height = usize::from(word(bytes, 4)?);
    ensure!(
        (1..=512).contains(&count)
            && (1..=512).contains(&width)
            && (1..=512).contains(&height)
            && count * width * height * 4 <= 64 * 1024 * 1024,
        "invalid uncompressed sprite canvas"
    );
    (0..count)
        .map(|i| {
            let header = bytes
                .get(6 + i * 8..14 + i * 8)
                .context("truncated uncompressed sprite table")?;
            let [x, y, w, h] = [header[0], header[1], header[2], header[3]].map(usize::from);
            ensure!(
                x + w <= width && y + h <= height,
                "uncompressed sprite outside canvas"
            );
            let start = u32::from_le_bytes(header[4..].try_into()?) as usize;
            let pixels = bytes
                .get(start..start + w * h)
                .context("truncated uncompressed sprite pixels")?;
            let mut rgba = vec![0; width * height * 4];
            for (index, pixel) in pixels.iter().enumerate() {
                let target = ((y + index / w) * width + x + index % w) * 4;
                rgba[target..target + 4].copy_from_slice(&palette[*pixel as usize]);
            }
            Ok(Image {
                width: width as u32,
                height: height as u32,
                rgba,
            })
        })
        .collect()
}

/// Keep source coordinates while allowing a shared unit anchor on smaller overlays.
pub fn pad_for_anchor(image: &Image, anchor: [i32; 2]) -> Image {
    let width = image.width.max((anchor[0] + 1) as u32);
    let height = image.height.max((anchor[1] + 1) as u32);
    let mut rgba = vec![0; (width * height * 4) as usize];
    for y in 0..image.height as usize {
        let source = y * image.width as usize * 4;
        let target = y * width as usize * 4;
        rgba[target..target + image.width as usize * 4]
            .copy_from_slice(&image.rgba[source..source + image.width as usize * 4]);
    }
    Image {
        width,
        height,
        rgba,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sparse_pixels_keep_alpha_while_real_black_remains_opaque() {
        let mut bytes = vec![
            1, 0, 3, 0, 1, 0, 0, 0, 3, 1, 14, 0, 0, 0, 2, 0, 0x81, 1, 0, 0x81,
        ];
        let mut pal = [[0; 4]; 256];
        pal[0] = [0, 0, 0, 255];
        let image = sprites(&bytes, &pal).unwrap().remove(0);
        assert_eq!(image.rgba, [0, 0, 0, 0, 0, 0, 0, 255, 0, 0, 0, 0]);
        bytes[16] = 0;
        assert!(sprites(&bytes, &pal).is_err());
    }
}
