//! Liftable Terran buildings use finite source pose sequences.
//! The importer resolves their selected IScript instructions; the runtime receives
//! ordinary native images and clips, never a source script interpreter.
use std::{
    collections::BTreeMap,
    io::{Read, Seek},
    path::Path,
};

use anyhow::{Context, Result, ensure};
use serde::Serialize;
use straterust_engine::{
    assets::{AssetManifest, ClipFrame, ClipKind, SpriteClip},
    media::{AudioCue, AudioMapping, AudioRef, MediaManifest, decode_wav, encode_wav},
    sim::{Flight, Rules, UnitTypeId},
};

use crate::{
    Archive, Files, MemberReport, Source, add_image, formats, member, ron_bytes, terran,
    terran_media,
};

#[derive(Serialize)]
struct Report {
    schema_version: u32,
    members: Vec<MemberReport>,
    evidence: Vec<&'static str>,
    limitations: Vec<&'static str>,
}

pub fn convert(source_path: &Path, files: &mut Files) -> Result<()> {
    let source = Source::open(source_path)?;
    let mut installer = Archive::open_region(&source.path, source.offset, source.len)?;
    let mut members = Vec::new();
    let data = member(
        &mut installer,
        "files\\stardat.mpq",
        128 * 1024 * 1024,
        "install",
        "original building flight data/art",
        &mut members,
    )?;
    let mut archive = Archive::from_bytes(data)?;
    let mut rules: Rules = ron::de::from_bytes(&files["rules.ron"])?;
    let mut assets: AssetManifest = ron::de::from_bytes(&files["assets.ron"])?;
    refresh(&mut archive, files, &mut assets, &mut rules)
}

pub(crate) fn refresh(
    archive: &mut Archive<std::io::Cursor<Vec<u8>>>,
    files: &mut Files,
    assets: &mut AssetManifest,
    rules: &mut Rules,
) -> Result<()> {
    let mut members = Vec::new();
    let units = read(archive, &mut members, "arr\\units.dat")?;
    let flingy = read(archive, &mut members, "arr\\flingy.dat")?;
    let scripts = read(archive, &mut members, "scripts\\iscript.bin")?;
    let images = read(archive, &mut members, "arr\\images.dat")?;
    let palette = formats::palette(&read(archive, &mut members, "tileset\\badlands.wpe")?)?;
    ensure!(
        units.len() == 19192 && flingy.len() == 2760 && images.len() == 28690,
        "unsupported building flight DAT layout"
    );

    let shadow_palette = [[0, 0, 0, 100]; 256]; // Shadow drawing ignores source color indices.
    let mut audio_units = Vec::new();
    for (
        source,
        native,
        flingy_id,
        script,
        path,
        shadow_path,
        shadow_id,
        dimensions,
        hold,
        delay,
        poses,
    ) in [
        (
            106,
            3,
            94,
            102,
            "unit\\terran\\control.grp",
            "unit\\terran\\tccShad.grp",
            277,
            [6, 128, 160],
            5,
            18,
            &[5, 2, 3, 4][..],
        ),
        (
            111,
            5,
            91,
            96,
            "unit\\terran\\TBarrack.grp",
            "unit\\terran\\tbrShad.grp",
            267,
            [9, 192, 160],
            8,
            15,
            &[5, 2, 3, 4][..],
        ),
        (
            122,
            15,
            111,
            136,
            "unit\\terran\\weaponpl.grp",
            "unit\\terran\\twpShad.grp",
            324,
            [6, 192, 160],
            4,
            25,
            &[5, 3, 2, 4][..],
        ),
        (
            113,
            32,
            97,
            111,
            "unit\\terran\\factory.grp",
            "unit\\terran\\tfaShad.grp",
            287,
            [7, 128, 160],
            5,
            15,
            &[6, 2, 3, 4, 5][..],
        ),
        (
            114,
            33,
            110,
            134,
            "unit\\terran\\starport.grp",
            "unit\\terran\\tspShad.grp",
            321,
            [6, 128, 160],
            5,
            20,
            &[4, 1, 2, 3][..],
        ),
    ] {
        if !rules.units.iter().any(|unit| unit.id == UnitTypeId(native)) {
            continue;
        }
        ensure!(
            units[source] == flingy_id as u8
                && i32::from_le_bytes(
                    flingy[368 + flingy_id * 4..372 + flingy_id * 4]
                        .try_into()
                        .unwrap()
                ) == 427
                && i16::from_le_bytes(
                    flingy[1104 + flingy_id * 2..1106 + flingy_id * 2]
                        .try_into()
                        .unwrap()
                ) == 33,
            "unsupported source building flight speed"
        );
        verify_script(&scripts, script, hold, delay, poses)?;
        audio_units.push((UnitTypeId(native), delay));
        let shadow_instruction = [9, shadow_id as u8, (shadow_id >> 8) as u8, 0, 0];
        ensure!(
            images[755 * 8 + shadow_id] == 10
                && terran::script_animation(&scripts, script, 0)?
                    .windows(5)
                    .take(4)
                    .any(|bytes| bytes == shadow_instruction),
            "unsupported source building shadow instruction"
        );
        let bytes = read(archive, &mut members, path)?;
        let body = terran::decode_expected(&bytes, &palette, dimensions).with_context(|| {
            format!(
                "flight body {path}: header {:?}, expected {dimensions:?}",
                &bytes[..6]
            )
        })?;
        let sprite = assets
            .extra_units
            .iter_mut()
            .find(|sprite| sprite.unit_type == UnitTypeId(native))
            .context("missing liftable native sprite")?;
        sprite.clips.retain(|clip| {
            !matches!(
                clip.kind,
                ClipKind::Lift
                    | ClipKind::Land
                    | ClipKind::Airborne
                    | ClipKind::Shadow
                    | ClipKind::LiftShadow
                    | ClipKind::LandShadow
            )
        });
        let mut kept = Vec::new();
        let mut remap = std::collections::BTreeMap::new();
        for frame in sprite.clips.iter_mut().flat_map(|clip| &mut clip.frames) {
            frame.frame = *remap.entry(frame.frame).or_insert_with(|| {
                let n = kept.len() as u16;
                kept.push(sprite.frames[usize::from(frame.frame)].clone());
                n
            });
        }
        sprite.frames = kept;
        let start = u16::try_from(sprite.frames.len())?;
        for (index, image) in body.iter().enumerate() {
            sprite.frames.push(add_image(
                files,
                &format!("flight-{native}-{index}.srim"),
                image,
            )?);
        }
        let offset = [
            sprite.anchor[0] as i16 - dimensions[1] as i16 / 2,
            sprite.anchor[1] as i16 - dimensions[2] as i16 / 2,
        ];
        let land_ticks = (u32::from(delay) + poses.len() as u32 * u32::from(hold)).max(42);
        sprite
            .clips
            .extend(clips(start, offset, hold, delay, poses, land_ticks));
        let shadow =
            formats::decode_grp(&read(archive, &mut members, shadow_path)?, &shadow_palette)?;
        ensure!(
            shadow.len() >= body.len() && shadow.len() <= body.len() + 1,
            "unsupported building shadow poses: {shadow_path} has {} (body {})",
            shadow.len(),
            body.len()
        );
        let image = &shadow[0];
        let shadow_start = u16::try_from(sprite.frames.len())?;
        let shadow_offset = [
            sprite.anchor[0] as i16 - image.width as i16 / 2,
            sprite.anchor[1] as i16 - image.height as i16 / 2,
        ];
        for (index, image) in shadow.iter().take(body.len()).enumerate() {
            sprite.frames.push(add_image(
                files,
                &format!("flight-{native}-shadow-{index}.srim"),
                image,
            )?);
        }
        let mut shadows = clips(shadow_start, shadow_offset, hold, delay, poses, land_ticks);
        for clip in &mut shadows {
            clip.kind = match clip.kind {
                ClipKind::Lift => ClipKind::LiftShadow,
                ClipKind::Land => ClipKind::LandShadow,
                _ => ClipKind::Shadow,
            };
            for frame in &mut clip.frames {
                frame.offset = shadow_offset;
            }
        }
        sprite.clips.extend(shadows);
        rules
            .units
            .iter_mut()
            .find(|unit| unit.id == UnitTypeId(native))
            .context("missing liftable native definition")?
            .flight = Some(Flight {
            speed: 1,
            lift_ticks: 42,
            land_ticks,
        });
    }
    refresh_audio(archive, files, &audio_units, &mut members)?;
    assets.validate()?;
    files.insert("assets.ron".into(), ron_bytes(&assets)?);
    files.insert("rules.ron".into(), ron_bytes(&rules)?);
    files.insert("flight-reference.ron".into(), ron_bytes(&Report {
        schema_version: 1, members,
        evidence: vec![
            "Original units 106/111/122/113/114 use flingy 94/91/111/97/110: top_speed 427/256, acceleration 33/256. BuildingLiftoff/LiftingOff overrides speed to 1 pixel per source frame and moves 42 pixels vertically; a building retains that override after takeoff. Reference: OpenBW bwgame.h order_BuildingLiftoff/order_LiftingOff/order_BuildingLand.",
            "Selected scripts 102/96/136/111/134, LiftOff animation 18 and Landing animation 17 are verified byte-for-byte through sigorder 16. Original v1.00 dispatcher 0x409a20 decrements the wait byte; opcode 5 handler 0x40ae46 stores operand minus 1, so wait N holds exactly N frames. Native tick and clip step are 42 ms.",
            "Final airborne body poses are 4/4/4/5/3, held at 42 pixels elevation. Source holds and landing delays are retained; completion also requires 42-pixel travel. Native lift is 42 ticks and land is 42/47/42/42/42 ticks.",
            "Verified LiftOff scripts play sound471 immediately; Landing scripts wait18/15/25/15/20 source frames before sound472. Native lift/land cues use those original WAVs, with the landing wait baked as leading PCM silence. Cues follow visible flight transitions, including automatic addon relocation, rather than attempted orders.",
        ],
        limitations: vec![
            "The native entity remains at its ground anchor during vertical transitions; source body travel is represented by clip offsets. Original turn/acceleration and order-dispatch startup phase still require trajectory calibration.",
            "Source body poses are preserved. Source shadow masks (images277/267/324/287/321, draw10) are native ground-anchored Shadow clips with alpha100; destination-palette shadow darkening remains approximated. Original landing dust is not included.",
        ],
    })?);
    Ok(())
}

fn refresh_audio(
    archive: &mut Archive<std::io::Cursor<Vec<u8>>>,
    files: &mut Files,
    units: &[(UnitTypeId, u8)],
    members: &mut Vec<MemberReport>,
) -> Result<()> {
    let Some(bytes) = files.get("media.ron") else {
        return Ok(());
    };
    let mut media: MediaManifest = ron::de::from_bytes(bytes)?;
    let sounds = read(archive, members, "arr\\sfxdata.dat")?;
    let names = read(archive, members, "arr\\sfxdata.tbl")?;
    let mut references = BTreeMap::<(AudioCue, u8), AudioRef>::new();
    let mut cumulative = 0;
    for &(unit_type, delay) in units {
        for (cue, sound, wait, name) in [
            (AudioCue::Lift, 471, 0, "flight-lift.wav".into()),
            (
                AudioCue::Land,
                472,
                delay,
                format!("flight-land-{delay}.wav"),
            ),
        ] {
            let reference = if let Some(reference) = references.get(&(cue, wait)) {
                reference.clone()
            } else {
                let path = terran_media::sound_path(&sounds, &names, sound)?;
                let bytes = terran_media::normalize_wav(
                    &read(archive, members, &path)?,
                    10000,
                    &mut cumulative,
                )?;
                let bytes = delayed_wav(&bytes, wait)?;
                let reference = terran_media::audio_file(files, name, bytes);
                references.insert((cue, wait), reference.clone());
                reference
            };
            media
                .audio
                .retain(|mapping| !(mapping.cue == cue && mapping.unit_type == Some(unit_type)));
            media.audio.push(AudioMapping {
                cue,
                unit_type: Some(unit_type),
                voice: false,
                variants: vec![reference],
            });
        }
    }
    media.validate()?;
    files.insert("media.ron".into(), ron_bytes(&media)?);
    Ok(())
}

/// Preserve source animation timing in native PCM, without a runtime script or
/// another audio scheduler. Clip offsets use the same 42-ms source frame.
fn delayed_wav(bytes: &[u8], wait: u8) -> Result<Vec<u8>> {
    let pcm = decode_wav(bytes)?;
    let frames = u64::from(pcm.sample_rate) * u64::from(wait) * 42 / 1000;
    let mut samples = vec![0; frames as usize * usize::from(pcm.channels)];
    samples.extend_from_slice(&pcm.samples);
    encode_wav(pcm.channels, pcm.sample_rate, &samples)
}

fn read<R: Read + Seek>(
    archive: &mut Archive<R>,
    members: &mut Vec<MemberReport>,
    path: &str,
) -> Result<Vec<u8>> {
    member(
        archive,
        path,
        8 * 1024 * 1024,
        "stardat",
        "original building flight data/art",
        members,
    )
}

fn verify_script(scripts: &[u8], id: u16, hold: u8, delay: u8, poses: &[u16]) -> Result<()> {
    let mut lift = vec![0x2e, 0x18, 0xd7, 1]; // nobrkcodestart; playsnd471
    for (index, pose) in poses.iter().copied().enumerate() {
        lift.push(0);
        lift.extend(pose.to_le_bytes());
        if index + 1 != poses.len() {
            lift.extend([5, hold]);
        }
    }
    lift.extend([0x24, 16, 0x2f]); // sigorder16; nobrkcodeend
    terran::expect_animation(scripts, id, 18, &lift)?;
    let mut land = vec![0x2e, 5, delay, 0x18, 0xd8, 1];
    for pose in poses.iter().copied().rev() {
        land.push(0);
        land.extend(pose.to_le_bytes());
        land.extend([5, hold]);
    }
    land.extend([0, 0, 0, 0x24, 16, 0x2f]);
    terran::expect_animation(scripts, id, 17, &land)
}

fn clips(
    start: u16,
    offset: [i16; 2],
    hold: u8,
    delay: u8,
    poses: &[u16],
    land_ticks: u32,
) -> [SpriteClip; 3] {
    let frame = |pose, height: i16| ClipFrame {
        frame: start + pose,
        flip_x: false,
        offset: [offset[0], offset[1] - height],
    };
    let clip = |kind, frames| SpriteClip {
        key_steps: Vec::new(),
        kind,
        directions: 1,
        frame_ms: 42,
        frames,
    };
    let lift = (0..42)
        .map(|tick| {
            let pose = poses[(tick / u32::from(hold)).min(poses.len() as u32 - 1) as usize];
            frame(pose, tick as i16)
        })
        .collect();
    let land = (0..land_ticks)
        .map(|tick| {
            let pose = if tick < u32::from(delay) {
                *poses.last().unwrap()
            } else {
                let index = (tick - u32::from(delay)) / u32::from(hold);
                if index < poses.len() as u32 {
                    poses[poses.len() - 1 - index as usize]
                } else {
                    0
                }
            };
            frame(pose, 42 - tick.min(42) as i16)
        })
        .collect();
    [
        clip(ClipKind::Lift, lift),
        clip(ClipKind::Land, land),
        clip(ClipKind::Airborne, vec![frame(*poses.last().unwrap(), 42)]),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flight_landing_audio_preserves_samples_after_source_frame_delay() {
        for channels in [1, 2] {
            let samples = vec![1234; 100 * channels as usize];
            let wav = encode_wav(channels, 22050, &samples).unwrap();
            for wait in [0, 15, 18, 20, 25] {
                let delayed = decode_wav(&delayed_wav(&wav, wait).unwrap()).unwrap();
                let silence = 22050 * usize::from(wait) * 42 / 1000 * channels as usize;
                assert!(delayed.samples[..silence].iter().all(|sample| *sample == 0));
                assert_eq!(&delayed.samples[silence..], &samples);
                assert_eq!(delayed.channels, channels);
                assert_eq!(delayed.sample_rate, 22050);
            }
        }
    }

    #[test]
    fn finite_flight_clips_hold_source_poses_and_preserve_ground_anchor() {
        let [lift, land, air] = clips(20, [6, -2], 8, 15, &[5, 2, 3, 4][..], 47);
        assert_eq!(lift.frames.len(), 42);
        assert!(lift.frames[..8].iter().all(|frame| frame.frame == 25));
        assert_eq!(lift.frames[8].frame, 22);
        assert_eq!(lift.frames[16].frame, 23);
        assert_eq!(lift.frames[24].frame, 24);
        assert_eq!(lift.frames[41].offset, [6, -43]);
        assert_eq!(air.frames[0].offset, [6, -44]);
        assert_eq!(land.frames.len(), 47);
        assert_eq!(land.frames[0].offset, air.frames[0].offset);
        assert_eq!(land.frames[23].frame, 23);
        assert_eq!(land.frames[31].frame, 22);
        assert_eq!(land.frames[39].frame, 25);
        assert_eq!(land.frames[46].offset, [6, -2]);
    }
}
