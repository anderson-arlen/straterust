//! Siege/unsiege are finite source sequences, not looping idle animations.
use super::*;
use serde::{Deserialize, Serialize};

fn poses(tables: &Tables, image: usize, animation: usize) -> Vec<u16> {
    let mut pose = 0;
    let mut result = Vec::new();
    for instruction in instructions(&tables.scripts, tables.script(image), animation) {
        match instruction.op {
            0 | 1 => pose = word(&instruction.args, 0),
            5 | 6 => {
                let wait = if instruction.op == 5 {
                    instruction.args[0]
                } else {
                    (instruction.args[0] / 2) + (instruction.args[1] / 2)
                };
                result.extend(std::iter::repeat_n(pose, usize::from(wait)));
            }
            36 if instruction.args[0] & 1 != 0 => break,
            47 => break,
            _ => {}
        }
    }
    result
}
pub(super) fn apply(tables: &Tables, rules: &mut Rules) {
    let body = tables.image(30);
    let top = tables.image(31);
    let known: BTreeSet<_> = rules.units.iter().map(|u| u.id).collect();
    for (from, to, animation, research) in [
        (22, 56, 0, Some(ResearchId(21))),
        (56, 22, 14, None),
        (54, 113, 0, None),
        (113, 54, 14, None),
    ] {
        if let Some(unit) = rules.units.iter_mut().find(|u| u.id == UnitTypeId(from))
            && known.contains(&UnitTypeId(to))
        {
            unit.mode = Some(ModeChange {
                target: UnitTypeId(to),
                research,
                ticks: poses(tables, body, animation)
                    .len()
                    .max(poses(tables, top, animation).len()) as u32,
            });
        }
    }
}
#[derive(Deserialize, Default)]
#[serde(default)]
struct Controls {
    command_buttons: BTreeMap<String, Control>,
}
#[derive(Deserialize, Serialize)]
struct Control {
    slot: u8,
    key: String,
    label: String,
    tip: String,
    icon: String,
}

pub(super) fn refresh(
    archive: &mut SourceArchive,
    files: &mut Files,
    assets: &mut AssetManifest,
    rules: &Rules,
    graphics: &mut Graphics<'_>,
) -> Result<()> {
    if !rules
        .units
        .iter()
        .any(|u| matches!(u.id.0, 22 | 56 | 54 | 113))
    {
        return Ok(());
    }
    let body_image = graphics.tables.image(30);
    let top_image = graphics.tables.image(31);
    let body = graphics.decode(archive, body_image)?.to_vec();
    let turret = graphics.decode(archive, top_image)?.to_vec();
    let colors = formats::decode_pcx(&archive.read_file("unit\\cmdbtns\\ticon.pcx", 1024 * 1024)?)?;
    let icons = formats::decode_grp(
        &archive.read_file("unit\\cmdbtns\\cmdicons.grp", 1024 * 1024)?,
        &crate::terran_ui::command_palette(&colors)?,
    )?;
    let tech = archive.read_file("arr\\techdata.dat", 432)?;
    let table = archive.read_file("rez\\stat_txt.tbl", 65536)?;
    let mut presentation = std::str::from_utf8(&files["presentation.ron"])?.to_owned();
    let mut controls: Controls = ron::from_str(&presentation)?;
    for native in [22, 56, 54, 113] {
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
        let siege = matches!(native, 56 | 113);
        let mut clips = Vec::new();
        let mut frames = Vec::new();
        if siege {
            let body = body.get(5).context("missing deployed tank body")?;
            let bases = vec![0];
            for top in turret.iter().take(17) {
                frames.push(terran::composite(body, top)?);
            }
            clips.push(terran::directional(ClipKind::Idle, &bases, 42));
        }
        if let Some(mode) = &unit.mode {
            let animation = if siege { 14 } else { 0 };
            let body_poses = poses(graphics.tables, body_image, animation);
            let top_poses = poses(graphics.tables, top_image, animation);
            ensure!(
                !body_poses.is_empty() && !top_poses.is_empty(),
                "missing source tank mode sequence"
            );
            let mut bases = Vec::new();
            let mut cache = BTreeMap::new();
            for tick in 0..mode.ticks as usize {
                let b = body_poses[tick.min(body_poses.len() - 1)];
                let t = top_poses[tick.min(top_poses.len() - 1)];
                let next = frames.len() as u16;
                let base = *cache.entry((b, t)).or_insert(next);
                bases.push(base);
                if base == next {
                    for heading in 0..17 {
                        frames.push(terran::composite(
                            body.get(usize::from(b)).context("invalid tank body pose")?,
                            turret
                                .get(usize::from(t) + heading)
                                .context("invalid tank turret pose")?,
                        )?);
                    }
                }
            }
            clips.push(terran::directional(ClipKind::Transform, &bases, 42));
            let key = format!("mode.{native}");
            let icon = usize::from(word(&tech, 24 * 12 + 5 * 2)) + usize::from(siege);
            assets.ui.retain(|ui| ui.key != key);
            assets.ui.push(straterust_engine::assets::UiImageManifest {
                key: key.clone(),
                image: crate::add_image(
                    files,
                    &format!("ui-{key}.srim"),
                    icons.get(icon).context("missing tank mode icon")?,
                )?,
            });
            let key_code = terran_media::table_string(&table, if siege { 339 } else { 338 })?
                .chars()
                .next()
                .context("missing tank shortcut")?
                .to_ascii_uppercase()
                .to_string();
            controls.command_buttons.insert(
                key.clone(),
                Control {
                    slot: 7,
                    key: key_code,
                    label: if siege { "Tank Mode" } else { "Siege Mode" }.into(),
                    tip: if siege {
                        "Retract the siege platform and restore movement."
                    } else {
                        "Deploy the siege platform for long-range artillery; the tank cannot move."
                    }
                    .into(),
                    icon: key,
                },
            );
        }
        if !clips.is_empty() {
            let extra = terran::compact_sprite(
                files,
                native,
                &sprite.unit_name,
                &format!("tank-modes-{native}"),
                &frames,
                clips,
            )?;
            sprite
                .clips
                .retain(|c| !(c.kind == ClipKind::Transform || siege && c.kind == ClipKind::Idle));
            let mut kept = Vec::new();
            let mut remap = BTreeMap::new();
            for frame in sprite.clips.iter_mut().flat_map(|c| &mut c.frames) {
                frame.frame = *remap.entry(frame.frame).or_insert_with(|| {
                    let n = kept.len() as u16;
                    kept.push(sprite.frames[usize::from(frame.frame)].clone());
                    n
                });
            }
            sprite.frames = kept;
            let offset = sprite.frames.len() as u16;
            sprite.frames.extend(extra.frames);
            for mut clip in extra.clips {
                for frame in &mut clip.frames {
                    frame.frame += offset;
                }
                sprite.clips.push(clip);
            }
        }
    }
    set_map(
        &mut presentation,
        "command_buttons",
        &ron::ser::to_string(&controls.command_buttons)?,
    )?;
    files.insert("presentation.ron".into(), presentation.into_bytes());
    if let Some(bytes) = files.get("media.ron") {
        let mut media: MediaManifest = ron::de::from_bytes(bytes)?;
        let sfx = archive.read_file("arr\\sfxdata.dat", 8712)?;
        let names = archive.read_file("arr\\sfxdata.tbl", 65536)?;
        let mut cumulative = 0;
        for native in [22, 56, 54, 113] {
            if !rules
                .units
                .iter()
                .any(|u| u.id == UnitTypeId(native) && u.mode.is_some())
            {
                continue;
            }
            let animation = if matches!(native, 56 | 113) { 14 } else { 0 };
            let mut variants = Vec::new();
            for sound in sounds(
                &graphics.tables.scripts,
                graphics.tables.script(body_image),
                animation,
            ) {
                let path = terran_media::sound_path(&sfx, &names, sound)?;
                let bytes = terran_media::normalize_wav(
                    &archive.read_file(&path, 4 * 1024 * 1024)?,
                    120000,
                    &mut cumulative,
                )?;
                variants.push(terran_media::audio_file(
                    files,
                    format!("sound-{sound:03}.wav"),
                    bytes,
                ));
            }
            if !variants.is_empty() {
                media.audio.retain(|m| {
                    !(m.cue == AudioCue::ChangeMode && m.unit_type == Some(UnitTypeId(native)))
                });
                media.audio.push(AudioMapping {
                    cue: AudioCue::ChangeMode,
                    unit_type: Some(UnitTypeId(native)),
                    voice: false,
                    variants,
                });
            }
        }
        files.insert("media.ron".into(), ron_bytes(&media)?);
    }
    Ok(())
}
