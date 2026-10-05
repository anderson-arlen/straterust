//! Retail transformation bodies, emergence and staged building mutations.
//! The runtime sees native clips only, never unit IDs or IScript instructions.
use super::*;
use buildings::replace_clip;

pub(crate) fn refresh_morphs(
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
    let image_for = |unit: usize| {
        usize::from(word(
            &sprites,
            usize::from(word(&flingy, usize::from(units[unit]) * 2)) * 2,
        ))
    };
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
    for &(source, native) in MAPPING {
        if !matches!(source, 35..=59 | 131..=153) {
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
        let body_image = image_for(usize::from(source));
        let construction = dword(&units, 1332 + usize::from(source) * 4) as usize;
        let poses = |image, animation, finite| {
            iscript::timeline_parts(&scripts, script(image), animation, finite)
        };
        if matches!(source, 36 | 59) {
            let frames = decode(body_image)?;
            let (timeline, start) = poses(body_image, 0, false);
            let start = usize::from(start.unwrap_or(0));
            let idle = select(&frames, &timeline[start..])?;
            replace_clip(files, sprite, source, ClipKind::Idle, "incubate", &idle)?;
            if start > 0 {
                replace_clip(
                    files,
                    sprite,
                    source,
                    ClipKind::Birth,
                    "form",
                    &select(&frames, &timeline[..start])?,
                )?;
            }
            let opening = select(&frames, &poses(body_image, 13, true).0)?;
            if !opening.is_empty() {
                replace_clip(files, sprite, source, ClipKind::Transform, "open", &opening)?;
            }
        } else if construction != 0 && !unit.structure {
            let frames = decode(construction)?;
            let opening = select(&frames, &poses(construction, 13, true).0)?;
            if !opening.is_empty() {
                replace_clip(files, sprite, source, ClipKind::Birth, "emerge", &opening)?;
            }
        } else if construction != 0 && unit.structure {
            let frames = decode(construction)?;
            let mut progress = Vec::new();
            let mut stages = Vec::new();
            for (percent, animation) in [(0, 0), (25, 13), (50, 14)] {
                let (timeline, start) = poses(construction, animation, false);
                let timeline = &timeline[usize::from(start.unwrap_or(0))..];
                if timeline.is_empty() {
                    continue;
                }
                stages.push((percent, progress.len() as u16));
                progress.extend(select(&frames, timeline)?);
            }
            if !progress.is_empty() {
                replace_clip(
                    files,
                    sprite,
                    source,
                    ClipKind::Construction,
                    "mutate",
                    &progress,
                )?;
                sprite
                    .clips
                    .iter_mut()
                    .find(|c| c.kind == ClipKind::Construction)
                    .unwrap()
                    .progress_starts = stages;
            }
            let (initial, loop_start) = poses(construction, 0, false);
            if let Some(start) = loop_start.filter(|start| *start > 0) {
                replace_clip(
                    files,
                    sprite,
                    source,
                    ClipKind::ConstructionStart,
                    "form",
                    &select(&frames, &initial[..usize::from(start)])?,
                )?;
            }
            let mut finish = select(&frames, &poses(construction, 15, true).0)?;
            let body = decode(body_image)?;
            let idle_pose = poses(body_image, 16, false).0.first().copied().unwrap_or(0);
            for child in iscript::instructions(&scripts, script(body_image), 15)
                .iter()
                .filter(|i| matches!(i.op, 8 | 9))
            {
                let child_image = usize::from(word(&child.args, 0));
                if images[755 * 8 + child_image] == 10 {
                    continue;
                }
                ensure!(
                    child.args[2..] == [0, 0],
                    "unsupported mutation overlay offset"
                );
                let overlay = decode(child_image)?;
                for pose in poses(child_image, 0, true).0 {
                    let bottom = &body[usize::from(idle_pose)];
                    let top = &overlay[usize::from(pose)];
                    let width = bottom.width.max(top.width);
                    let height = bottom.height.max(top.height);
                    let bottom = terran::center_canvas(bottom, width, height)?;
                    let top = terran::center_canvas(top, width, height)?;
                    finish.push(if child.op == 9 {
                        terran::composite(&top, &bottom)?
                    } else {
                        terran::composite(&bottom, &top)?
                    });
                }
            }
            if !finish.is_empty() {
                replace_clip(
                    files,
                    sprite,
                    source,
                    ClipKind::ConstructionEnd,
                    "finish",
                    &finish,
                )?;
                // Building upgrades keep identity, then change to their new body.
                replace_clip(files, sprite, source, ClipKind::Birth, "upgrade", &finish)?;
            }
        }
    }
    Ok(())
}

fn select(frames: &[Image], poses: &[u16]) -> Result<Vec<Image>> {
    poses
        .iter()
        .map(|pose| {
            frames
                .get(usize::from(*pose))
                .cloned()
                .context("invalid mutation pose")
        })
        .collect()
}

#[cfg(test)]
mod tests;
