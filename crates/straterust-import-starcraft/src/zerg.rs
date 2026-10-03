//! The four original preplaced enemy roles in Terran Mission 2.
//! Source mapping and selected iscript instructions are checked; no Zerg economy or VM.
use std::{
    collections::BTreeMap,
    io::{Read, Seek},
    path::Path,
};

use anyhow::{Context, Result, ensure};
use serde::Serialize;
use straterust_engine::{
    assets::{AssetManifest, ClipKind, Image, SpriteClip, SpriteManifest},
    media::{AudioCue, AudioMapping, AudioRef, MediaManifest, decode_wav, encode_wav},
    sim::{DamageKind, Footprint, ResourceAmount, Rules, UnitSize, UnitType, UnitTypeId, Weapon},
};

use crate::{
    Archive, Files, MemberReport, Source, add_image, formats, member, ron_bytes,
    terran::{
        center_canvas, compact_sprite, composite, decode_expected, directional, expect_animation,
        single_direction, write_sprite,
    },
    terran_data::{self, ReferenceUnit},
};

const MAPPING: [(u16, u16); 4] = [(37, 6), (38, 7), (130, 8), (143, 9)];

#[derive(Serialize)]
struct Report {
    schema_version: u32,
    source_to_native_units: Vec<(u16, u16)>,
    units: Vec<ReferenceUnit>,
    members: Vec<MemberReport>,
    source_canvases: Vec<(String, [u32; 2])>,
    evidence: Vec<&'static str>,
    limitations: Vec<&'static str>,
}

pub fn convert(source_path: &Path, files: &mut Files) -> Result<()> {
    let source = Source::open(source_path)?;
    let mut installer = Archive::open_region(&source.path, source.offset, source.len)?;
    let mut members = Vec::new();
    let stardat = member(
        &mut installer,
        "files\\stardat.mpq",
        128 * 1024 * 1024,
        "install",
        "Backwater Zerg reference archive",
        &mut members,
    )?;
    let mut archive = Archive::from_bytes(stardat)?;
    let units = read(&mut archive, &mut members, "arr\\units.dat")?;
    let weapons = read(&mut archive, &mut members, "arr\\weapons.dat")?;
    let reference = terran_data::decode_selected(&units, &weapons, &[37, 38, 130, 143])?;
    let mut rules: Rules = ron::de::from_bytes(&files["rules.ron"])?;
    apply_rules(&mut rules, &reference)?;
    files.insert("rules.ron".into(), ron_bytes(&rules)?);

    let scripts = read(&mut archive, &mut members, "scripts\\iscript.bin")?;
    let images = read(&mut archive, &mut members, "arr\\images.dat")?;
    verify_source(&scripts, &images)?;
    let palette = formats::palette(&read(&mut archive, &mut members, "tileset\\badlands.wpe")?)?;
    let mut assets: AssetManifest = ron::de::from_bytes(&files["assets.ron"])?;
    ensure!(
        MAPPING
            .iter()
            .all(|(_, id)| assets.unit_type != UnitTypeId(*id)
                && assets
                    .extra_units
                    .iter()
                    .all(|s| s.unit_type != UnitTypeId(*id))),
        "Zerg art IDs already exist"
    );
    let mut crops = Vec::new();
    for (id, name, slug, path, count, body_end, death_start, corpse_path, corpse_count) in [
        (
            6,
            "Zergling",
            "zergling",
            "unit\\zerg\\zergling.grp",
            296,
            204,
            289,
            "unit\\zerg\\zzeDeath.grp",
            5,
        ),
        (
            7,
            "Hydralisk",
            "hydralisk",
            "unit\\zerg\\hydra.grp",
            297,
            204,
            204,
            "unit\\zerg\\zhyDeath.grp",
            4,
        ),
    ] {
        let source = grp(
            &mut archive,
            &mut members,
            path,
            &palette,
            [count, 128, 128],
        )?;
        let death_count = if id == 6 { 7 } else { 8 };
        let mut frames = source[..usize::from(body_end)].to_vec();
        frames.extend_from_slice(
            &source[usize::from(death_start)..usize::from(death_start + death_count)],
        );
        frames.extend(grp(
            &mut archive,
            &mut members,
            corpse_path,
            &palette,
            [corpse_count, 128, 128],
        )?);
        // Keep the original five burrow poses after the selected death/corpse images.
        let burrow_native = frames.len() as u16;
        let burrow_source = if id == 6 { 204 } else { 212 };
        frames.extend_from_slice(&source[burrow_source..burrow_source + 85]);
        crops.push((name.into(), [128, 128]));
        let attack = if id == 6 {
            vec![0, 17, 34, 51, 68]
        } else {
            vec![51, 68, 51]
        };
        let death = corpse_clip(body_end, death_count, corpse_count);
        assets.extra_units.push(compact_sprite(
            files,
            id,
            name,
            slug,
            &frames,
            vec![
                directional(ClipKind::Idle, &[85], 100),
                directional(ClipKind::Walk, &[85, 102, 119, 136, 153, 170, 187], 42),
                directional(ClipKind::Attack, &attack, 42),
                directional(
                    ClipKind::Burrow,
                    &[
                        burrow_native,
                        burrow_native + 17,
                        burrow_native + 34,
                        burrow_native + 51,
                        burrow_native + 68,
                    ],
                    42,
                ),
                directional(
                    ClipKind::Unburrow,
                    &[
                        burrow_native + 68,
                        burrow_native + 68,
                        burrow_native + 68,
                        burrow_native + 51,
                        burrow_native + 34,
                        burrow_native + 17,
                        burrow_native,
                    ],
                    42,
                ),
                death,
            ],
        )?);
    }

    let control = grp(
        &mut archive,
        &mut members,
        "unit\\terran\\control.grp",
        &palette,
        [6, 128, 160],
    )?;
    let infestation = grp(
        &mut archive,
        &mut members,
        "unit\\zerg\\Infest03.grp",
        &palette,
        [3, 128, 128],
    )?;
    let command_center = assets
        .extra_units
        .iter()
        .find(|s| s.unit_type == UnitTypeId(3))
        .context("Terran art lacks Command Center for infested death mapping")?;
    let infested = infested_sprite(files, &control[0], &infestation, command_center)?;
    assets.extra_units.push(infested);

    let colony = grp(
        &mut archive,
        &mut members,
        "unit\\zerg\\fcolony.grp",
        &palette,
        [4, 128, 64],
    )?;
    let burst = grp(
        &mut archive,
        &mut members,
        "unit\\thingy\\zBldDthS.grp",
        &palette,
        [12, 200, 200],
    )?;
    let rubble = grp(
        &mut archive,
        &mut members,
        "unit\\thingy\\ZRubbleS.grp",
        &palette,
        [4, 96, 96],
    )?;
    let frames = colony
        .iter()
        .chain(burst.iter())
        .chain(rubble.iter())
        .map(|image| center_canvas(image, 200, 200))
        .collect::<Result<Vec<_>>>()?;
    let death: Vec<_> = (4..16)
        .chain((16..20).flat_map(|f| std::iter::repeat_n(f, 10)))
        .collect();
    assets.extra_units.push(write_sprite(
        files,
        9,
        "Creep Colony",
        "creep-colony",
        &frames,
        vec![
            single_direction(ClipKind::Idle, &[0, 1, 2, 3], 150),
            single_direction(ClipKind::Death, &death, 150),
        ],
    )?);
    assets.validate()?;
    files.insert("assets.ron".into(), ron_bytes(&assets)?);
    add_audio(&mut archive, files, &mut members)?;
    files.insert("zerg-reference.ron".into(),ron_bytes(&Report {
        schema_version:1, source_to_native_units:MAPPING.to_vec(), units:reference, members,
        source_canvases:crops,
        evidence:vec![
            "Legacy units.dat/flingy.dat/sprites.dat/images.dat mapping: Zergling37 -> flingy15/sprite159/image54/script31; Hydra38 ->8/146/29/18; InfestedCommandCenter130 ->17/162/63/39; CreepColony143 ->21/166/68/42. Selected image script/drawing fields and script instructions are checked. DAT layout reference: https://github.com/poiuyqwert/PyMS/tree/master/PyMS/FileFormats/DAT .",
            "Mobile Idle base85; Walk bases85/102/119/136/153/170/187 share17 source headings with mirrored native headings17..31. Zergling Attack bases0/17/34/51/68; Hydra repeated attack uses68/51. Source Burrow uses five directional poses; Unburrow reverses four poses then returns to idle. All85burrowposeframes are retained after death/corpse images,301native images per mobile unit. Random fidgets, unburrow startupdelay1..5 and dustimage423 are not reproduced.",
            "Zergling death uses body289..295 and sprite160/image57 zzeDeath frames0..4. Hydra death uses204..211 and sprite147/image32 zhyDeath frames0..3. Source corpse wait50 lasts50frames. Native finite corpse durations13.8s and11.4s remain explicit presentation approximations independent of the campaign42ms tick.",
            "Infested Command Center script39 creates ordinary image101 Infest03 at0,0 over control frame0. Script60 loops three overlay frames withwait2; composited native Idle preserves that loop. Death shares existing Command Center explosion/rubble images and clip with remapped references. Colony Built script42 loopsfcolony frames0..3; death creates ordinaryimage60 zBldDthS12frames plus sprite186/image110 ZRubbleS four poses.",
            "Verified script sound IDs: Zergling attack894 anddeath896; Hydra attack64 anddeath867; Colonydeath774; InfestedCommandCenterdeath7 already present. Source archive audio decoding and native PCM16 WAV validation are reused. No enemy selection/acknowledgement voices or portraits are required by this scenario.",
        ],
        limitations:vec![
            "These four source enemy roles are preplaced by Mission2. This module supplies no enemy training/economy, larvae, upgrades or airborne units; mission placements, creep rules and original triggers are supplied by the mission converter. Enemy native supply is0 because these roles cannot be trained; raw half-supply and pair-production cost fields remain in this report.",
            "Base integer speeds are superseded by campaign Motion records in backwater-reference.ron. Source move/wait1 cycles total 39 Zergling pixels and 26 Hydralisk pixels per seven frames; flingy movement_control2 top_speed1 is a dummy. Campaign movement preserves these stride cycles with normalized fixed-point navigation, while source steering and collision behavior remain approximations.",
            "Hydra uses explosive damage: subtract armor then apply target size factors50%/75%/100%, retaining1/256HP and minimum0.5HP. Zerg regeneration is4/256HP pernative tick. Unburrow duration7ticks fixes the midpoint of source randomized5..9ticks. The original wait opcode stores N-1, so waitN lasts Nframes. Campaign repeat-attack startup delays and cooldown variation are recorded in backwater-reference.ron. Source projectile travel, first-attack transitions, muzzle effects, turning and RNG sequencing remain uncalibrated. Source reference: https://github.com/OpenBW/openbw/blob/master/bwgame.h .",
            "Source death effects/corpses are represented sequentially without an iscript VM. Colony rubble is shortened to6seconds (native total7.8seconds); infested death retains existing8.1second Terran approximation and translucent RGBA orange-fire blending. Shadows and player-color remapping remain omitted. Individual transparent crops with per-frame offsets preserve the source origin and every visible pixel, including mirrored poses; collision dimensions remain centered approximations of DAT asymmetry.",
        ],
    })?);
    Ok(())
}

fn read<R: Read + Seek>(
    archive: &mut Archive<R>,
    members: &mut Vec<MemberReport>,
    path: &str,
) -> Result<Vec<u8>> {
    member(
        archive,
        path,
        crate::ASSET_LIMIT,
        "stardat",
        "selected Backwater enemy source",
        members,
    )
}
fn grp<R: Read + Seek>(
    archive: &mut Archive<R>,
    members: &mut Vec<MemberReport>,
    path: &str,
    palette: &[[u8; 4]; 256],
    expected: [u16; 3],
) -> Result<Vec<Image>> {
    decode_expected(&read(archive, members, path)?, palette, expected)
        .with_context(|| format!("unsupported Zerg art {path}"))
}

fn corpse_clip(start: u16, body_count: u16, corpse_count: u16) -> SpriteClip {
    let end = start + body_count;
    let frames: Vec<_> = (start..end)
        .chain((end..end + corpse_count).flat_map(|f| std::iter::repeat_n(f, 17)))
        .collect();
    single_direction(ClipKind::Death, &frames, 150)
}

fn apply_rules(rules: &mut Rules, reference: &[ReferenceUnit]) -> Result<()> {
    ensure!(reference.len() == 4, "expected four source enemy units");
    for unit in &mut rules.units {
        if matches!(unit.id.0, 1 | 2) {
            unit.size = UnitSize::Small;
        } else if matches!(unit.id.0, 3..=5) {
            unit.size = UnitSize::Large;
        }
    }
    for (source, id) in MAPPING {
        ensure!(
            rules.units.iter().all(|u| u.id != UnitTypeId(id)),
            "enemy unit ID already exists"
        );
        let reference = reference
            .iter()
            .find(|u| u.source_id == source)
            .context("missing enemy DAT record")?;
        let [left, up, right, down] = reference.collision_extents;
        let size = match reference.unit_size {
            1 => UnitSize::Small,
            2 => UnitSize::Medium,
            3 => UnitSize::Large,
            _ => anyhow::bail!("unsupported source size"),
        };
        let weapon = reference
            .weapon
            .as_ref()
            .map(|w| -> Result<Weapon> {
                ensure!(
                    w.minimum_range == 0,
                    "enemy minimum weapon range is unsupported"
                );
                Ok(Weapon {
                    cooldown_jitter: None,
                    targets_air: source == 38,
                    damage: u32::from(w.damage),
                    range: w.maximum_range,
                    cooldown: u32::from(w.cooldown_frames),
                    splash: None,
                    strikes: Vec::new(),
                    damage_kind: match w.damage_type {
                        1 => DamageKind::Explosive,
                        3 => DamageKind::Normal,
                        _ => anyhow::bail!("unsupported damage kind"),
                    },
                })
            })
            .transpose()?;
        rules.units.push(UnitType {
            id: UnitTypeId(id),
            speed: match id {
                6 => 5,
                7 => 3,
                _ => 0,
            },
            size,
            acquisition_range: if id <= 7 { Some(256) } else { None },
            footprint: Footprint {
                width: left + right + 1,
                height: up + down + 1,
            },
            placement: Footprint {
                width: reference.placement_size[0],
                height: reference.placement_size[1],
            },
            max_hp: reference.hitpoints,
            regeneration: 4,
            unburrow_ticks: if id <= 7 { 7 } else { 0 },
            armor: u32::from(reference.armor),
            structure: id >= 8,
            cost: [("minerals", reference.minerals), ("gas", reference.gas)]
                .into_iter()
                .filter(|(_, v)| *v != 0)
                .map(|(kind, v)| ResourceAmount {
                    kind: kind.into(),
                    amount: u32::from(v),
                })
                .collect(),
            build_ticks: u32::from(reference.build_frames),
            weapon,
            ..UnitType::default()
        });
    }
    Ok(())
}

fn infested_sprite(
    files: &mut Files,
    body: &Image,
    overlays: &[Image],
    control: &SpriteManifest,
) -> Result<SpriteManifest> {
    let old_death = control
        .clips
        .iter()
        .find(|c| c.kind == ClipKind::Death)
        .context("missing Terran death clip")?;
    let mut frames = Vec::new();
    for (i, overlay) in overlays.iter().enumerate() {
        let composed = composite(body, &center_canvas(overlay, body.width, body.height)?)?;
        let image = center_canvas(&composed, 252, 200)?;
        frames.push(add_image(
            files,
            &format!("infested-command-center-{i:03}.srim"),
            &image,
        )?);
    }
    let mut remap = BTreeMap::new();
    let mut death = old_death.clone();
    for pose in &mut death.frames {
        let next = frames.len() as u16;
        pose.frame = *remap.entry(pose.frame).or_insert_with(|| {
            frames.push(control.frames[usize::from(pose.frame)].clone());
            next
        });
    }
    Ok(SpriteManifest {
        unit_type: UnitTypeId(8),
        unit_name: "Infested Command Center".into(),
        frame_ms: 150,
        anchor: [126, 100],
        frames,
        clips: vec![single_direction(ClipKind::Idle, &[0, 1, 2], 150), death],
    })
}

fn add_audio<R: Read + Seek>(
    archive: &mut Archive<R>,
    files: &mut Files,
    members: &mut Vec<MemberReport>,
) -> Result<()> {
    let sfx = read(archive, members, "arr\\sfxdata.dat")?;
    let tbl = read(archive, members, "arr\\sfxdata.tbl")?;
    ensure!(sfx.len() == 8712, "unsupported sound DAT layout");
    let mut media: MediaManifest = ron::de::from_bytes(&files["media.ron"])?;
    for (id, cue, sound) in [
        (6, AudioCue::Attack, 894),
        (6, AudioCue::Death, 896),
        (7, AudioCue::Attack, 64),
        (7, AudioCue::Death, 867),
        (8, AudioCue::Death, 7),
        (9, AudioCue::Death, 774),
    ] {
        let file = format!("sound-{sound:03}.wav");
        let bytes = if let Some(existing) = files.get(&file) {
            existing.clone()
        } else {
            let tid = u32::from_le_bytes(sfx[sound * 4..sound * 4 + 4].try_into().unwrap());
            let path = format!("sound\\{}", crate::terran_media::table_string(&tbl, tid)?);
            let original = member(
                archive,
                &path,
                4 * 1024 * 1024,
                "stardat",
                "enemy combat audio",
                members,
            )?;
            let pcm = decode_wav(&original)?;
            ensure!(pcm.duration_ms() <= 30_000, "enemy sound exceeds30seconds");
            let bytes = encode_wav(pcm.channels, pcm.sample_rate, &pcm.samples)?;
            files.insert(file.clone(), bytes.clone());
            bytes
        };
        media.audio.push(AudioMapping {
            cue,
            unit_type: Some(UnitTypeId(id)),
            voice: false,
            variants: vec![AudioRef {
                file,
                blake3: blake3::hash(&bytes).to_hex().to_string(),
            }],
        });
    }
    media.validate()?;
    files.insert("media.ron".into(), ron_bytes(&media)?);
    Ok(())
}

fn verify_source(scripts: &[u8], images: &[u8]) -> Result<()> {
    ensure!(images.len() == 28690, "unsupported images DAT layout");
    for (image, script) in [
        (54, 31),
        (29, 18),
        (63, 39),
        (68, 42),
        (57, 32),
        (32, 19),
        (101, 60),
        (60, 34),
        (110, 145),
    ] {
        ensure!(
            images[6040 + image] == 0 && images[6795 + image] == 0,
            "unsupported enemy image drawing mode"
        );
        ensure!(
            u32::from_le_bytes(
                images[7550 + 4 * image..7554 + 4 * image]
                    .try_into()
                    .unwrap()
            ) == script,
            "unsupported enemy image script"
        );
    }
    for (id, shadow, deltas) in [
        (31, 55, [2, 8, 9, 5, 6, 7, 2]),
        (18, 30, [2, 2, 2, 6, 6, 6, 2]),
    ] {
        expect_animation(scripts, id, 0, &[9, shadow, 0, 0, 0, 0, 85, 0])?;
        let mut walking = Vec::new();
        for (delta, frame) in deltas.into_iter().zip([102, 119, 136, 153, 170, 187, 85]) {
            walking.extend([0x29, delta, 5, 1, 0, frame, 0]);
        }
        expect_animation(scripts, id, 11, &walking)?;
        let start = if id == 31 { 204_u16 } else { 212_u16 };
        let mut burrow = vec![8, 0xa7, 1, 0, 0];
        for frame in (start..start + 85).step_by(17) {
            burrow.push(0);
            burrow.extend(frame.to_le_bytes());
            burrow.extend([5, 1]);
        }
        expect_animation(scripts, id, 25, &burrow)?;
        let mut unburrow = vec![6, 1, 5, 9, 0xa7, 1, 0, 0];
        for frame in [start + 51, start + 34, start + 17, start] {
            unburrow.push(0);
            unburrow.extend(frame.to_le_bytes());
            unburrow.extend([5, 1]);
        }
        expect_animation(scripts, id, 26, &unburrow)?;
    }
    expect_animation(
        scripts,
        31,
        2,
        &[
            0, 0, 0, 5, 1, 0x2e, 0, 17, 0, 5, 1, 0, 34, 0, 0x1c, 1, 0x7e, 3, 5, 1, 0, 51, 0, 5, 1,
            0, 68, 0, 5, 1,
        ],
    )?;
    expect_animation(
        scripts,
        18,
        5,
        &[
            5, 1, 0, 68, 0, 0x18, 64, 0, 0x15, 0x4c, 1, 0, 0x26, 5, 1, 0, 51, 0, 5, 1,
        ],
    )?;
    for (id, sound, start, count, sprite) in
        [(31, 896_u16, 289_u16, 7, 160_u16), (18, 867, 204, 8, 147)]
    {
        let mut death = vec![0x18];
        death.extend(sound.to_le_bytes());
        death.extend([0x34, 0]);
        for frame in start..start + count {
            death.push(0);
            death.extend(frame.to_le_bytes());
            death.extend([5, 2]);
        }
        death.push(0x11);
        death.extend(sprite.to_le_bytes());
        death.extend([0, 0, 5, 1, 0x16]);
        expect_animation(scripts, id, 1, &death)?;
    }
    for (id, count, wait) in [(32, 5, 50), (19, 4, 50), (34, 12, 2)] {
        let mut sequence = Vec::new();
        for frame in 0..count {
            sequence.extend([0, frame, 0, 5, wait]);
        }
        sequence.push(0x16);
        expect_animation(scripts, id, 0, &sequence)?;
    }
    expect_animation(
        scripts,
        39,
        0,
        &[9, 0x15, 1, 0, 0, 8, 101, 0, 0, 0, 0, 0, 0],
    )?;
    expect_animation(
        scripts,
        39,
        1,
        &[
            0x18, 7, 0, 8, 0x4e, 1, 0, 0, 5, 3, 0x3f, 0x20, 0x79, 0x11, 0x12, 1, 0, 0, 5, 1, 0x16,
        ],
    )?;
    expect_animation(
        scripts,
        60,
        0,
        &[0, 0, 0, 5, 2, 0, 1, 0, 5, 2, 0, 2, 0, 5, 2],
    )?;
    expect_animation(
        scripts,
        42,
        16,
        &[
            0, 0, 0, 6, 1, 3, 0, 1, 0, 5, 2, 0, 2, 0, 5, 2, 0, 3, 0, 5, 2, 0, 0, 0, 5, 2,
        ],
    )?;
    expect_animation(
        scripts,
        42,
        1,
        &[
            0x18, 6, 3, 8, 60, 0, 0, 0, 5, 3, 0x11, 186, 0, 0, 0, 5, 1, 0x16,
        ],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terran_data::ReferenceWeapon;

    #[test]
    fn finite_corpse_clips_fit_remapped_image_lists() {
        for (body, corpse, steps, duration) in [(7, 5, 92, 13_800), (8, 4, 76, 11_400)] {
            let clip = corpse_clip(204, body, corpse);
            assert_eq!(clip.kind, ClipKind::Death);
            assert_eq!(clip.directions, 1);
            assert_eq!(clip.frames.len(), steps);
            assert_eq!(clip.frames.len() as u32 * clip.frame_ms, duration);
            assert!(clip.frames.iter().all(|pose| pose.frame < 216));
            assert_eq!(clip.frames.last().unwrap().frame, 215);
        }
    }

    #[test]
    fn enemy_rules_use_sizes_explosive_damage_and_no_production_economy() {
        let mut rules = Rules {
            units: (1..=5)
                .map(|id| UnitType {
                    id: UnitTypeId(id),
                    ..UnitType::default()
                })
                .collect(),
            ..Rules::default()
        };
        let reference: Vec<_> = MAPPING
            .iter()
            .enumerate()
            .map(|(index, (id, _))| ReferenceUnit {
                source_id: *id,
                minerals: 50,
                gas: 0,
                hitpoints: 35,
                armor: 0,
                unit_size: if index == 0 {
                    1
                } else if index == 1 {
                    2
                } else {
                    3
                },
                build_frames: 420,
                supply_required_half_units: 1,
                supply_provided_half_units: 2,
                collision_extents: [8, 4, 7, 11],
                placement_size: [16, 16],
                weapon: if index < 2 {
                    Some(ReferenceWeapon {
                        id: 35,
                        damage_type: if index == 0 { 3 } else { 1 },
                        behavior: if index == 0 { 5 } else { 2 },
                        effect: 1,
                        splash_radii: [0; 3],
                        forward_offset: 0,
                        target_flags: 2,
                        damage: 5,
                        cooldown_frames: 8,
                        minimum_range: 0,
                        maximum_range: 15,
                    })
                } else {
                    None
                },
            })
            .collect();
        apply_rules(&mut rules, &reference).unwrap();
        assert_eq!(rules.units[0].size, UnitSize::Small);
        assert_eq!(rules.units[1].size, UnitSize::Small);
        assert_eq!(rules.units[2].size, UnitSize::Large);
        let ling = &rules.units[5];
        let hydra = &rules.units[6];
        assert_eq!((ling.speed, hydra.speed), (5, 3));
        assert_eq!(hydra.size, UnitSize::Medium);
        assert_eq!(
            hydra.weapon.as_ref().unwrap().damage_kind,
            DamageKind::Explosive
        );
        assert_eq!(
            ling.footprint,
            Footprint {
                width: 16,
                height: 16
            }
        );
        assert_eq!(ling.acquisition_range, Some(256));
        for enemy in &rules.units[5..] {
            assert_eq!((enemy.supply_used, enemy.supply_provided), (0, 0));
            assert!(enemy.builds.is_empty() && enemy.trains.is_empty() && enemy.repairs.is_empty());
        }
        assert!(rules.units[7].structure && rules.units[7].weapon.is_none());
        assert!(apply_rules(&mut rules, &reference).is_err());
    }

    #[test]
    fn infested_structure_reuses_death_images_and_remaps_repeated_poses() {
        use straterust_engine::assets::ImageRef;
        let refs: Vec<_> = (0..7)
            .map(|i| ImageRef {
                file: format!("source-{i}.srim"),
                blake3: "0".repeat(64),
            })
            .collect();
        let control = SpriteManifest {
            unit_type: UnitTypeId(3),
            unit_name: "Command Center".into(),
            frame_ms: 100,
            anchor: [126, 100],
            frames: refs,
            clips: vec![single_direction(ClipKind::Death, &[5, 5, 6], 150)],
        };
        let body = Image {
            width: 2,
            height: 2,
            rgba: vec![50; 16],
        };
        let overlay = Image {
            width: 2,
            height: 2,
            rgba: vec![0; 16],
        };
        let mut files = Files::new();
        let infested = infested_sprite(
            &mut files,
            &body,
            &[overlay.clone(), overlay.clone(), overlay],
            &control,
        )
        .unwrap();
        assert_eq!(files.len(), 3);
        assert_eq!(infested.frames.len(), 5);
        assert_eq!(infested.frames[3].file, "source-5.srim");
        assert_eq!(infested.frames[4].file, "source-6.srim");
        assert_eq!(
            infested.clips[1]
                .frames
                .iter()
                .map(|p| p.frame)
                .collect::<Vec<_>>(),
            [3, 3, 4]
        );
    }
}
