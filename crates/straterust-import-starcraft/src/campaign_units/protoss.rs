//! Native Probe mining, construction warp textures and Pylon field quadrants.
use super::*;
use buildings::replace_clip;

pub(crate) fn refresh_protoss(
    archive: &mut Archive<std::io::Cursor<Vec<u8>>>,
    files: &mut Files,
    assets: &mut AssetManifest,
    rules: &Rules,
) -> Result<()> {
    if !rules.units.iter().any(|u| Some(u.id) == native_id(64)) {
        return Ok(());
    }
    let units = archive.read_file("arr\\units.dat", 19192)?;
    let flingy = archive.read_file("arr\\flingy.dat", 2760)?;
    let sprites = archive.read_file("arr\\sprites.dat", 2081)?;
    let images = archive.read_file("arr\\images.dat", 28690)?;
    let names = archive.read_file("arr\\images.tbl", 65536)?;
    let scripts = archive.read_file("scripts\\iscript.bin", 65536)?;
    let palette = formats::palette(&archive.read_file("tileset\\badlands.wpe", 1024)?)?;
    let script = |image: usize| dword(&images, 755 * 10 + image * 4) as u16;
    let mut cache = BTreeMap::<usize, Vec<Image>>::new();
    let mut decode = |image: usize| -> Result<Vec<Image>> {
        if let Some(frames) = cache.get(&image) {
            return Ok(frames.clone());
        }
        let path = format!(
            "unit\\{}",
            terran_media::table_string(&names, dword(&images, image * 4))?
        );
        let palette = terran::image_palette(archive, &images, image, &palette)?;
        let frames = formats::decode_grp(&archive.read_file(&path, 8 * 1024 * 1024)?, &palette)?;
        cache.insert(image, frames.clone());
        Ok(frames)
    };
    let anchor = decode(211)?;
    let texture = decode(210)?;
    let sequence = |image, animation, finite| {
        iscript::timeline_parts(&scripts, script(image), animation, finite).0
    };
    let (growing, growing_loop) = iscript::timeline_parts(&scripts, script(211), 0, false);
    let growing = select(&anchor, &growing)?;
    ensure!(
        growing_loop.is_some(),
        "warp anchor lacks its construction loop"
    );
    let closing = select(&anchor, &sequence(211, 13, true))?;
    for &(source, native) in MAPPING {
        if !(154..=175).contains(&source) {
            continue;
        }
        let Some(sprite) = assets
            .extra_units
            .iter_mut()
            .find(|s| s.unit_type == UnitTypeId(native))
        else {
            continue;
        };
        replace_clip(
            files,
            sprite,
            source,
            ClipKind::Construction,
            "warp",
            &growing,
        )?;
        let clip = sprite
            .clips
            .iter_mut()
            .find(|c| c.kind == ClipKind::Construction)
            .unwrap();
        clip.progress_starts = vec![(0, 0)];
        clip.loop_start = growing_loop;
        let image = usize::from(word(
            &sprites,
            usize::from(word(&flingy, usize::from(units[usize::from(source)]) * 2)) * 2,
        ));
        let body = decode(image)?;
        let pose = sequence(image, 16, false).first().copied().unwrap_or(0);
        let body = &body[usize::from(pose)];
        let mut finish = closing.clone();
        // Retail uses script193 to replace every opaque building pixel with
        // the original Warp Texture, preserving the building's silhouette.
        for pattern in texture.iter().take(20) {
            finish.push(warp_texture(body, pattern));
        }
        replace_clip(
            files,
            sprite,
            source,
            ClipKind::ConstructionEnd,
            "materialize",
            &finish,
        )?;
    }
    if let Some(probe) = assets
        .extra_units
        .iter_mut()
        .find(|s| Some(s.unit_type) == native_id(64))
    {
        // AlmostBuilt calls weapon63; its flingy153 uses image520/script230.
        let effect = decode(520)?;
        let frames = select(&effect, &sequence(520, 0, false))?;
        replace_clip(files, probe, 64, ClipKind::WorkEffect, "mining", &frames)?;
        let clip = probe
            .clips
            .iter_mut()
            .find(|c| c.kind == ClipKind::WorkEffect)
            .unwrap();
        clip.frames = clip
            .frames
            .iter()
            .flat_map(|frame| {
                (0..32).map(move |heading| {
                    let angle = f64::from(heading) * std::f64::consts::TAU / 32.0;
                    let mut frame = *frame;
                    frame.offset[0] += (angle.sin() * 20.0).round() as i16;
                    frame.offset[1] -= (angle.cos() * 20.0).round() as i16;
                    frame
                })
            })
            .collect();
        clip.directions = 32;
    }
    if let Some(pylon) = assets
        .extra_units
        .iter_mut()
        .find(|s| Some(s.unit_type) == native_id(156))
    {
        let upper = decode(584)?;
        let lower = decode(585)?;
        let field = coverage_image(&upper[0], &lower[0]);
        replace_clip(
            files,
            pylon,
            156,
            ClipKind::Coverage,
            "power-field",
            &[field],
        )?;
    }
    sounds(archive, files, rules)?;
    Ok(())
}

fn select(frames: &[Image], sequence: &[u16]) -> Result<Vec<Image>> {
    sequence
        .iter()
        .map(|frame| {
            frames
                .get(usize::from(*frame))
                .cloned()
                .context("invalid Protoss effect pose")
        })
        .collect()
}

fn warp_texture(body: &Image, texture: &Image) -> Image {
    let mut result = body.clone();
    for y in 0..body.height {
        for x in 0..body.width {
            let p = ((y * body.width + x) * 4) as usize;
            if body.rgba[p + 3] == 0 {
                continue;
            }
            let t = (((y % texture.height) * texture.width + x % texture.width) * 4) as usize;
            result.rgba[p..p + 3].copy_from_slice(&texture.rgba[t..t + 3]);
        }
    }
    result
}

fn coverage_image(upper: &Image, lower: &Image) -> Image {
    let mut field = Image {
        width: 512,
        height: 320,
        rgba: vec![0; 512 * 320 * 4],
    };
    // Script333 places upper-right at126,-77 and creates the other three
    // quadrants at +/-126,+/-77. Script334 mirrors the left-hand quadrants.
    for (image, cx, cy, flip) in [
        (upper, 126, -77, false),
        (lower, 126, 77, false),
        (upper, -126, -77, true),
        (lower, -126, 77, true),
    ] {
        for y in 0..image.height {
            for x in 0..image.width {
                let tx = 256 + cx - image.width as i32 / 2 + x as i32;
                let ty = 160 + cy - image.height as i32 / 2 + y as i32;
                if !(0..512).contains(&tx) || !(0..320).contains(&ty) {
                    continue;
                }
                let sx = if flip { image.width - x - 1 } else { x };
                let p = ((y * image.width + sx) * 4) as usize;
                if image.rgba[p + 3] == 0 {
                    continue;
                }
                let target = ((ty * 512 + tx) * 4) as usize;
                field.rgba[target..target + 4].copy_from_slice(&image.rgba[p..p + 4]);
                field.rgba[target + 3] = 100;
            }
        }
    }
    field
}

fn sounds(
    archive: &mut Archive<std::io::Cursor<Vec<u8>>>,
    files: &mut Files,
    rules: &Rules,
) -> Result<()> {
    let Some(bytes) = files.get("media.ron") else {
        return Ok(());
    };
    let mut media: MediaManifest = ron::de::from_bytes(bytes)?;
    let sfx = archive.read_file("arr\\sfxdata.dat", 8712)?;
    let table = archive.read_file("arr\\sfxdata.tbl", 65536)?;
    for (cue, sound) in [
        (AudioCue::Work, 614),
        (AudioCue::Transform, 528),
        (AudioCue::Complete, 529),
    ] {
        let path = terran_media::sound_path(&sfx, &table, sound)?;
        let bytes = terran_media::normalize_wav(
            &archive.read_file(&path, 4 * 1024 * 1024)?,
            120000,
            &mut 0,
        )?;
        let reference = terran_media::audio_file(files, format!("sound-{sound:03}.wav"), bytes);
        for unit in rules.units.iter().filter(|u| {
            if cue == AudioCue::Work {
                Some(u.id) == native_id(64)
            } else {
                u.autonomous_construction && u.max_shields > 0
            }
        }) {
            media
                .audio
                .retain(|m| !(m.cue == cue && m.unit_type == Some(unit.id)));
            media.audio.push(AudioMapping {
                cue,
                unit_type: Some(unit.id),
                voice: false,
                variants: vec![reference.clone()],
            });
        }
    }
    files.insert("media.ron".into(), ron_bytes(&media)?);
    Ok(())
}
