use super::*;

pub(crate) fn load_grp<R: std::io::Read + std::io::Seek>(
    archive: &mut Archive<R>,
    members: &mut Vec<MemberReport>,
    path: &str,
    palette: &[[u8; 4]; 256],
    expected: [u16; 3],
) -> Result<Vec<Image>> {
    let bytes = member(
        archive,
        path,
        crate::ASSET_LIMIT,
        "stardat",
        "source poses for native state and direction clips",
        members,
    )?;
    decode_expected(&bytes, palette, expected).with_context(|| format!("unsupported art {path}"))
}

pub(crate) fn decode_expected(
    bytes: &[u8],
    palette: &[[u8; 4]; 256],
    expected: [u16; 3],
) -> Result<Vec<Image>> {
    // Reject other layouts before decoding. These verified source canvases keep the combined
    // working set small even if an archive supplies a different, otherwise valid large GRP.
    let header: Vec<_> = expected.into_iter().flat_map(u16::to_le_bytes).collect();
    ensure!(
        bytes.starts_with(&header),
        "unexpected GRP frame count or canvas"
    );
    formats::decode_grp(bytes, palette)
}

pub(crate) fn write_sprite(
    files: &mut Files,
    id: u16,
    name: &str,
    slug: &str,
    images: &[Image],
    clips: Vec<SpriteClip>,
) -> Result<SpriteManifest> {
    let first = images.first().context("sprite has no source images")?;
    Ok(SpriteManifest {
        unit_type: UnitTypeId(id),
        unit_name: name.into(),
        frame_ms: 100,
        anchor: [first.width as i32 / 2, first.height as i32 / 2],
        frames: images
            .iter()
            .enumerate()
            .map(|(index, image)| add_image(files, &format!("{slug}-{index:03}.srim"), image))
            .collect::<Result<_>>()?,
        clips,
    })
}

/// Images may have different source canvases; each source origin is its canvas center.
pub(crate) fn compact_sprite(
    files: &mut Files,
    id: u16,
    name: &str,
    slug: &str,
    images: &[Image],
    mut clips: Vec<SpriteClip>,
) -> Result<SpriteManifest> {
    ensure!(
        !images.is_empty() && images.len() <= straterust_engine::assets::MAX_FRAMES,
        "too many compact frames"
    );
    let mut frames = Vec::new();
    let mut offsets = Vec::new();
    for (index, image) in images.iter().enumerate() {
        let (image, offset) = crop(image)?;
        frames.push(add_image(
            files,
            &format!("{slug}-{index:03}.srim"),
            &image,
        )?);
        offsets.push(offset);
    }
    for frame in clips.iter_mut().flat_map(|c| &mut c.frames) {
        let offset = offsets
            .get(usize::from(frame.frame))
            .context("compact clip frame outside source")?;
        frame.offset = [if frame.flip_x { -offset[0] } else { offset[0] }, offset[1]];
    }
    Ok(SpriteManifest {
        unit_type: UnitTypeId(id),
        unit_name: name.into(),
        frame_ms: 100,
        anchor: [0, 0],
        frames,
        clips,
    })
}
pub(crate) fn crop(image: &Image) -> Result<(Image, [i16; 2])> {
    ensure!(
        image.width > 0
            && image.height > 0
            && image.width <= 2048
            && image.height <= 2048
            && image.rgba.len() == (image.width * image.height * 4) as usize,
        "invalid crop source"
    );
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (image.width, image.height, 0, 0);
    for (index, pixel) in image.rgba.as_chunks::<4>().0.iter().enumerate() {
        if pixel[3] != 0 {
            let x = index as u32 % image.width;
            let y = index as u32 / image.width;
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x + 1);
            max_y = max_y.max(y + 1);
        }
    }
    if min_x == image.width {
        return Ok((
            Image {
                width: 1,
                height: 1,
                rgba: vec![0; 4],
            },
            [0, 0],
        ));
    }
    let (width, height) = (max_x - min_x, max_y - min_y);
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    for y in min_y..max_y {
        let start = ((y * image.width + min_x) * 4) as usize;
        rgba.extend_from_slice(&image.rgba[start..start + (width * 4) as usize]);
    }
    Ok((
        Image {
            width,
            height,
            rgba,
        },
        [
            (min_x as i32 - image.width as i32 / 2) as i16,
            (min_y as i32 - image.height as i32 / 2) as i16,
        ],
    ))
}

pub(crate) fn mark_marine_flashes(clips: &mut [SpriteClip]) {
    if let Some(attack) = clips.iter_mut().find(|clip| clip.kind == ClipKind::Attack) {
        // Source repeat attack alternates aiming base34 and muzzle base51.
        attack.key_steps = vec![1, 3, 5];
    }
}

pub(crate) fn directional(kind: ClipKind, bases: &[u16], frame_ms: u32) -> SpriteClip {
    SpriteClip {
        key_steps: Vec::new(),
        kind,
        directions: 32,
        frame_ms,
        frames: bases
            .iter()
            .flat_map(|base| {
                (0..32).map(move |direction| ClipFrame {
                    frame: base
                        + if direction <= 16 {
                            direction
                        } else {
                            32 - direction
                        },
                    flip_x: direction > 16,
                    offset: [0, 0],
                })
            })
            .collect(),
    }
}

pub(crate) fn single_direction(kind: ClipKind, frames: &[u16], frame_ms: u32) -> SpriteClip {
    SpriteClip {
        key_steps: Vec::new(),
        kind,
        directions: 1,
        frame_ms,
        frames: frames
            .iter()
            .map(|frame| ClipFrame {
                frame: *frame,
                flip_x: false,
                offset: [0, 0],
            })
            .collect(),
    }
}

pub(crate) fn marine_death() -> SpriteClip {
    let poses: Vec<_> = (221..229)
        .chain((229..232).flat_map(|frame| std::iter::repeat_n(frame, 17)))
        .collect();
    single_direction(ClipKind::Death, &poses, 150)
}

pub(crate) fn building_death(start: u16) -> SpriteClip {
    // The four original decay poses are retained with a deliberately shorter six-second
    // rubble lifetime. The original script keeps each source rubble pose much longer than this presentation-only decay.
    let poses: Vec<_> = (start..start + 14)
        .chain((start + 14..start + 18).flat_map(|frame| std::iter::repeat_n(frame, 10)))
        .collect();
    single_direction(ClipKind::Death, &poses, 150)
}

pub(crate) fn fire_palette(data: &[u8], palette: &[[u8; 4]; 256]) -> Result<[[u8; 4]; 256]> {
    let table = formats::decode_pcx(data)?;
    ensure!(
        table.width == 256 && table.height == 63,
        "unexpected orange-fire remap dimensions"
    );
    // OpenBW ui.h draw_alpha uses table[(source_index - 1) * 256 + backdrop_index].
    // The table's black-backdrop colors are emitted light, not opaque coverage.
    // Unpremultiply by the strongest channel: over black this preserves the
    // source intensity, while dark edges let the actual terrain show through.
    // Destination-dependent palette remapping remains an RGBA approximation.
    let mut result = [[0; 4]; 256];
    for (index, color) in result.iter_mut().enumerate().take(64).skip(1) {
        *color = fire_color(palette[usize::from(table.pixels[(index - 1) * 256])]);
    }
    Ok(result)
}

pub(crate) fn fire_color(color: [u8; 4]) -> [u8; 4] {
    let alpha = color[..3].iter().copied().max().unwrap();
    if alpha == 0 {
        return [0; 4];
    }
    let channel =
        |value| ((u32::from(value) * 255 + u32::from(alpha) / 2) / u32::from(alpha)) as u8;
    [
        channel(color[0]),
        channel(color[1]),
        channel(color[2]),
        alpha,
    ]
}
