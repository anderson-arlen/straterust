use super::*;

pub(super) fn rgba_length(width: u32, height: u32) -> Result<usize> {
    ensure!(
        (1..=MAX_IMAGE_DIMENSION).contains(&width) && (1..=MAX_IMAGE_DIMENSION).contains(&height),
        "image dimensions must be 1..={MAX_IMAGE_DIMENSION}, got {width}x{height}"
    );
    Ok(width as usize * height as usize * 4)
}

/// SRIM v1: magic, little-endian u32 version/width/height, then exact RGBA8 bytes.
pub fn encode_image(image: &Image) -> Result<Vec<u8>> {
    let length = rgba_length(image.width, image.height)?;
    ensure!(image.rgba.len() == length, "incorrect RGBA byte count");
    let mut bytes = Vec::with_capacity(HEADER_BYTES + length);
    bytes.extend_from_slice(b"SRIM");
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(&image.width.to_le_bytes());
    bytes.extend_from_slice(&image.height.to_le_bytes());
    bytes.extend_from_slice(&image.rgba);
    Ok(bytes)
}

pub fn decode_image(bytes: &[u8]) -> Result<Image> {
    ensure!(bytes.len() >= HEADER_BYTES, "truncated SRIM header");
    ensure!(&bytes[..4] == b"SRIM", "invalid SRIM magic");
    let word = |offset| u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
    ensure!(word(4) == 1, "unsupported SRIM version {}", word(4));
    let width = word(8);
    let height = word(12);
    let length = rgba_length(width, height)?;
    ensure!(
        bytes.len() == HEADER_BYTES + length,
        "incorrect SRIM byte count for {width}x{height}"
    );
    Ok(Image {
        width,
        height,
        rgba: bytes[HEADER_BYTES..].to_vec(),
    })
}
