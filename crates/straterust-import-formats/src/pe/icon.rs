//! Windows application icon resources, read as data without executing the EXE.
use super::*;

#[derive(Debug)]
pub struct ApplicationIcon {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

pub fn windows_icon(data: &[u8]) -> Result<ApplicationIcon> {
    let (sections, resources) = resource_table(data)?;
    let types = directory(resources, 0)?;
    let groups = types
        .iter()
        .find(|(id, _)| *id == 14)
        .context("PE has no application icon group")?;
    let names = directory(resources, subdirectory(groups.1)?)?;
    let name = names.first().context("empty application icon group")?;
    let (_, group) = resource_data(data, sections, resources, name.1, 6 + 128 * 14)?;
    ensure!(
        word(group, 0)? == 0 && word(group, 2)? == 1,
        "invalid icon group"
    );
    let count = usize::from(word(group, 4)?);
    ensure!((1..=128).contains(&count), "invalid icon image count");
    let entries = bytes(group, 6, count * 14)?;
    let dimension = |v: u8| if v == 0 { 256 } else { u32::from(v) };
    let selected = entries
        .as_chunks::<14>()
        .0
        .iter()
        .max_by_key(|entry| {
            (
                dimension(entry[0]) * dimension(entry[1]),
                u16::from_le_bytes([entry[6], entry[7]]),
            )
        })
        .context("missing icon image")?;
    let width = dimension(selected[0]);
    let height = dimension(selected[1]);
    let id = u32::from(word(selected, 12)?);
    let icons = types
        .iter()
        .find(|(id, _)| *id == 3)
        .context("PE has no icon images")?;
    let names = directory(resources, subdirectory(icons.1)?)?;
    let name = names
        .iter()
        .find(|(name, _)| *name == id)
        .context("icon group references a missing image")?;
    let (_, image) = resource_data(data, sections, resources, name.1, 1024 * 1024)?;
    ensure!(
        image.len() == dword(selected, 8)? as usize,
        "icon group byte size mismatch"
    );
    let image = if image.starts_with(b"\x89PNG\r\n\x1a\n") {
        decode_png(image)?
    } else {
        decode_dib(image)?
    };
    ensure!(
        image.width == width && image.height == height,
        "icon group canvas mismatch"
    );
    Ok(image)
}

fn decode_png(bytes: &[u8]) -> Result<ApplicationIcon> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info()?;
    ensure!(
        (1..=256).contains(&reader.info().width) && (1..=256).contains(&reader.info().height),
        "icon PNG exceeds canvas bound"
    );
    let mut pixels = vec![
        0;
        reader
            .output_buffer_size()
            .context("icon PNG buffer overflow")?
    ];
    let info = reader.next_frame(&mut pixels)?;
    let pixels = &pixels[..info.buffer_size()];
    let mut rgba = Vec::with_capacity(info.width as usize * info.height as usize * 4);
    let channels = match info.color_type {
        png::ColorType::Rgba => 4,
        png::ColorType::Rgb => 3,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Grayscale => 1,
        _ => anyhow::bail!("unsupported icon PNG color format"),
    };
    for pixel in pixels.chunks_exact(channels) {
        match channels {
            4 => rgba.extend_from_slice(pixel),
            3 => rgba.extend_from_slice(&[pixel[0], pixel[1], pixel[2], 255]),
            2 => rgba.extend_from_slice(&[pixel[0], pixel[0], pixel[0], pixel[1]]),
            _ => rgba.extend_from_slice(&[pixel[0], pixel[0], pixel[0], 255]),
        }
    }
    Ok(ApplicationIcon {
        width: info.width,
        height: info.height,
        rgba,
    })
}

fn decode_dib(data: &[u8]) -> Result<ApplicationIcon> {
    let header = dword(data, 0)? as usize;
    ensure!(
        (40..=124).contains(&header),
        "unsupported icon bitmap header"
    );
    bytes(data, 0, header)?;
    let width = i32::from_le_bytes(bytes(data, 4, 4)?.try_into()?);
    let doubled_height = i32::from_le_bytes(bytes(data, 8, 4)?.try_into()?);
    ensure!(
        (1..=256).contains(&width)
            && doubled_height != 0
            && doubled_height.unsigned_abs() <= 512
            && doubled_height.unsigned_abs().is_multiple_of(2),
        "invalid icon bitmap dimensions"
    );
    let width = width as usize;
    let height = doubled_height.unsigned_abs() as usize / 2;
    let bits = usize::from(word(data, 14)?);
    ensure!(
        word(data, 12)? == 1 && matches!(bits, 1 | 4 | 8 | 24 | 32) && dword(data, 16)? == 0,
        "unsupported icon bitmap encoding"
    );
    let colors = if bits <= 8 {
        let used = dword(data, 32)? as usize;
        if used == 0 { 1 << bits } else { used }
    } else {
        0
    };
    ensure!(colors <= 256, "invalid icon palette");
    let palette = bytes(data, header, colors * 4)?;
    let start = header + colors * 4;
    let stride = (width * bits).next_multiple_of(32) / 8;
    let mask_stride = width.next_multiple_of(32) / 8;
    let pixels = bytes(data, start, stride * height)?;
    let mask = bytes(data, start + stride * height, mask_stride * height)?;
    let has_alpha = bits == 32 && pixels.as_chunks::<4>().0.iter().any(|p| p[3] != 0);
    let mut rgba = vec![0; width * height * 4];
    for y in 0..height {
        let row = if doubled_height > 0 {
            height - 1 - y
        } else {
            y
        };
        for x in 0..width {
            let offset = row * stride;
            let pixel = if bits >= 24 {
                &pixels[offset + x * (bits / 8)..][..bits / 8]
            } else {
                let byte = pixels[offset + x * bits / 8];
                let shift = 8 - bits - (x * bits % 8);
                let index = usize::from((byte >> shift) & ((1 << bits) - 1) as u8);
                bytes(palette, index * 4, 4)?
            };
            let alpha = if mask[row * mask_stride + x / 8] & (0x80 >> (x % 8)) != 0 {
                0
            } else if has_alpha {
                pixel[3]
            } else {
                255
            };
            rgba[(y * width + x) * 4..][..4]
                .copy_from_slice(&[pixel[2], pixel[1], pixel[0], alpha]);
        }
    }
    Ok(ApplicationIcon {
        width: width as u32,
        height: height as u32,
        rgba,
    })
}

#[cfg(test)]
mod tests;
