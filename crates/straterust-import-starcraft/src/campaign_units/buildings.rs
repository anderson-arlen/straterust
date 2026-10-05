//! Building work overlays and the bridges drawn when an addon is connected.
use super::*;
use straterust_engine::assets::SpriteManifest;

pub(crate) fn refresh_buildings(
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
    let scripts = archive.read_file("scripts\\iscript.bin", 65536)?;
    let palette = formats::palette(&archive.read_file("tileset\\badlands.wpe", 1024)?)?;
    for &(source, native) in MAPPING {
        if !(106..=175).contains(&source) {
            continue;
        }
        let Some(unit) = rules.units.iter().find(|u| u.id == UnitTypeId(native)) else {
            continue;
        };
        let Some(sprite) = assets
            .extra_units
            .iter_mut()
            .find(|s| s.unit_type == unit.id)
        else {
            continue;
        };
        let source_sprite = usize::from(word(&flingy, usize::from(units[usize::from(source)]) * 2));
        let image = usize::from(word(&sprites, source_sprite * 2));
        let script = |image| dword(&images, 755 * 10 + image * 4) as u16;
        let mut decode = |image: usize| -> Result<Vec<Image>> {
            let path = format!(
                "unit\\{}",
                terran_media::table_string(&names, dword(&images, image * 4))?
            );
            let palette = terran::image_palette(archive, &images, image, &palette)?;
            formats::decode_grp(&archive.read_file(&path, 8 * 1024 * 1024)?, &palette)
        };
        // Keep calibrated base-role clips. Rebuild our own clips on refresh so
        // their frame indices and package bytes remain stable.
        if matches!(source, 113 | 114 | 115 | 120)
            || !sprite.clips.iter().any(|c| c.kind == ClipKind::Production)
        {
            let instructions = iscript::instructions(&scripts, script(image), 19);
            let body = decode(image)?;
            let idle = iscript::timeline(&scripts, script(image), 16)
                .first()
                .copied()
                .unwrap_or(0);
            let mut frames = Vec::new();
            for child in instructions.iter().filter(|i| matches!(i.op, 8 | 9)) {
                let child_image = usize::from(word(&child.args, 0));
                ensure!(
                    child.args[2..] == [0, 0],
                    "unsupported work overlay displacement"
                );
                let overlay = decode(child_image)?;
                for pose in iscript::timeline(&scripts, script(child_image), 0) {
                    let body = &body[usize::from(idle)];
                    let overlay = &overlay[usize::from(pose)];
                    let width = body.width.max(overlay.width);
                    let height = body.height.max(overlay.height);
                    let body = terran::center_canvas(body, width, height)?;
                    let overlay = terran::center_canvas(overlay, width, height)?;
                    frames.push(if child.op == 9 {
                        terran::composite(&overlay, &body)?
                    } else {
                        terran::composite(&body, &overlay)?
                    });
                }
            }
            if frames.is_empty() {
                let sequence = iscript::timeline(&scripts, script(image), 19);
                if sequence.iter().any(|&pose| pose != idle) {
                    frames.extend(
                        sequence
                            .into_iter()
                            .map(|pose| body[usize::from(pose)].clone()),
                    );
                }
            }
            if !frames.is_empty() {
                replace_clip(files, sprite, source, ClipKind::Production, "work", &frames)?;
            }
        }
        if source >= 131 {
            let body = decode(image)?;
            let idle = iscript::timeline(&scripts, script(image), 16);
            let mut frames = idle
                .iter()
                .filter_map(|pose| body.get(usize::from(*pose)).cloned())
                .collect::<Vec<_>>();
            let base = frames.first().cloned().unwrap_or_else(|| body[0].clone());
            for child in iscript::instructions(&scripts, script(image), 16)
                .iter()
                .filter(|i| matches!(i.op, 8 | 9))
            {
                let child_image = usize::from(word(&child.args, 0));
                let overlay = decode(child_image)?;
                ensure!(
                    child.args[2..] == [0, 0],
                    "unsupported idle overlay displacement"
                );
                frames.clear();
                for pose in iscript::timeline(&scripts, script(child_image), 0) {
                    let overlay = overlay
                        .get(usize::from(pose))
                        .context("invalid idle overlay frame")?;
                    let width = base.width.max(overlay.width);
                    let height = base.height.max(overlay.height);
                    let bottom = terran::center_canvas(&base, width, height)?;
                    let top = terran::center_canvas(overlay, width, height)?;
                    frames.push(if child.op == 9 {
                        terran::composite(&top, &bottom)?
                    } else {
                        terran::composite(&bottom, &top)?
                    });
                }
            }
            if !frames.is_empty() {
                replace_clip(files, sprite, source, ClipKind::Idle, "idle", &frames)?;
            }
            let start =
                iscript::instructions(&scripts, script(image), if source >= 154 { 21 } else { 0 });
            if let Some(child) = start.iter().find(|i| {
                matches!(i.op, 8..=10) && images[755 * 8 + usize::from(word(&i.args, 0))] != 10
            }) {
                let child_image = usize::from(word(&child.args, 0));
                let frames = decode(child_image)?;
                let sequence = iscript::timeline(&scripts, script(child_image), 0);
                if !sequence.is_empty()
                    && sequence
                        .iter()
                        .all(|pose| usize::from(*pose) < frames.len())
                {
                    let frames = sequence
                        .into_iter()
                        .map(|pose| frames[usize::from(pose)].clone())
                        .collect::<Vec<_>>();
                    replace_clip(
                        files,
                        sprite,
                        source,
                        ClipKind::Construction,
                        "construction",
                        &frames,
                    )?;
                }
            }
        }
        if unit.addon_parent.is_some() {
            let instructions = iscript::instructions(&scripts, script(image), 17);
            if let Some(child) = instructions.iter().find(|i| i.op == 8) {
                let child_image = usize::from(word(&child.args, 0));
                ensure!(
                    child.args[2..] == [0, 0],
                    "unsupported connector displacement"
                );
                let overlay = decode(child_image)?;
                let last = iscript::timeline(&scripts, script(child_image), 0)
                    .last()
                    .copied()
                    .unwrap_or(0);
                replace_clip(
                    files,
                    sprite,
                    source,
                    ClipKind::AddonConnector,
                    "connector",
                    &[overlay[usize::from(last)].clone()],
                )?;
            }
        }
    }
    if let Some(bytes) = files.get("media.ron") {
        let mut media: MediaManifest = ron::de::from_bytes(bytes)?;
        // Existing campaigns already carry the correct race's rescue sound.
        // Supply the Terran default only when importing or upgrading without one.
        if !media
            .audio
            .iter()
            .any(|mapping| mapping.cue == AudioCue::Capture && mapping.unit_type.is_none())
        {
            let bytes = terran_media::normalize_wav(
                &archive.read_file("sound\\misc\\trescue.wav", 8 * 1024 * 1024)?,
                10000,
                &mut 0,
            )?;
            let reference = terran_media::audio_file(files, "capture-terran.wav".into(), bytes);
            media.audio.push(AudioMapping {
                cue: AudioCue::Capture,
                unit_type: None,
                voice: false,
                variants: vec![reference],
            });
            files.insert("media.ron".into(), ron_bytes(&media)?);
        }
    }
    files.insert("building-activity-reference.ron".into(), ron_bytes(&(
        "Retail Factory image285/script111 IsWorking creates image286 factoryT.grp, script112 frames0..2 wait5. Starport image319/script134 creates image320 StarpoT.grp, script135 frames0..2 wait2. Machine Shop script117 anim17 creates image294 machineC.grp; Control Tower script107 anim17 creates image282 DryDockC.grp. Draw the final connected pose at the addon origin only while attached and complete. All six retail addons use DAT offset128,32 from parent placement upper-left, giving center displacement96,16 for a128x96 parent and64x64 addon. Connector extension/retraction timing remains approximate."))?);
    Ok(())
}

pub(super) fn replace_clip(
    files: &mut Files,
    sprite: &mut SpriteManifest,
    source: u16,
    kind: ClipKind,
    slug: &str,
    images: &[Image],
) -> Result<()> {
    sprite.clips.retain(|clip| clip.kind != kind);
    let mut remap = BTreeMap::new();
    let mut kept = Vec::new();
    for frame in sprite.clips.iter_mut().flat_map(|c| &mut c.frames) {
        let next = kept.len() as u16;
        frame.frame = *remap.entry(frame.frame).or_insert_with(|| {
            kept.push(sprite.frames[usize::from(frame.frame)].clone());
            next
        });
    }
    sprite.frames = kept;
    ensure!(
        sprite.anchor == [0, 0],
        "building clips must retain source origin"
    );
    let mut unique = Vec::new();
    let mut image_ids = BTreeMap::new();
    let sequence = images
        .iter()
        .map(|image| {
            let key = (
                image.width,
                image.height,
                *blake3::hash(&image.rgba).as_bytes(),
            );
            *image_ids.entry(key).or_insert_with(|| {
                let index = unique.len() as u16;
                unique.push(image.clone());
                index
            })
        })
        .collect::<Vec<_>>();
    let extra = terran::compact_sprite(
        files,
        sprite.unit_type.0,
        &sprite.unit_name,
        &format!("building-{source}-{slug}"),
        &unique,
        vec![terran::single_direction(kind, &sequence, 42)],
    )?;
    let start = sprite.frames.len() as u16;
    sprite.frames.extend(extra.frames);
    for mut clip in extra.clips {
        for frame in &mut clip.frames {
            frame.frame += start;
        }
        sprite.clips.push(clip);
    }
    Ok(())
}
