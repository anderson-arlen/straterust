//! Bounded conversion of the current five-role demo's source voices and portraits.
//! Legacy DAT layouts: https://github.com/poiuyqwert/PyMS/tree/master/PyMS/FileFormats/DAT
//! SMK header facts: https://github.com/FFmpeg/FFmpeg/blob/master/libavformat/smacker.c
//! FFmpeg is an import-time decoder only; native packages contain PCM WAV and SRIM.
use std::{
    collections::{BTreeMap, btree_map::Entry},
    fs::{self, File},
    io::{Read, Seek},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, ensure};
use serde::Serialize;
use straterust_engine::{
    assets::{Image, ImageRef},
    media::{
        AudioCue, AudioMapping, AudioRef, MediaManifest, PortraitManifest, decode_wav, encode_wav,
    },
    sim::UnitTypeId,
};

use crate::{Archive, Files, MemberReport, add_image, member, ron_bytes};

const MAX_SMK_BYTES: usize = 1024 * 1024;
const MAX_AUDIO_BYTES: usize = 64 * 1024 * 1024;
const MAX_PORTRAIT_BYTES: usize = 64 * 1024 * 1024;
const MAX_PCM_BYTES: usize = 256 * 1024 * 1024;

#[derive(Debug, Serialize)]
struct UnitMedia {
    source_id: u16,
    native_id: u16,
    portrait: u16,
    select: [u16; 2],
    order: Option<[u16; 2]>,
    ready: Option<u16>,
}

#[derive(Serialize)]
struct MediaReport {
    schema_version: u32,
    unit_mappings: Vec<UnitMedia>,
    sounds: Vec<(u16, String)>,
    portrait_sources: Vec<String>,
    music_sources: Vec<String>,
    decoder: String,
    evidence: Vec<&'static str>,
    limitations: Vec<&'static str>,
}

pub fn convert<I: Read + Seek, A: Read + Seek>(
    installer: &mut Archive<I>,
    archive: &mut Archive<A>,
    units: &[u8],
    scripts: &[u8],
    files: &mut Files,
    members: &mut Vec<MemberReport>,
) -> Result<()> {
    let mappings = unit_mappings(units)?;
    verify_sound_scripts(scripts)?;
    let portdata = member(
        archive,
        "arr\\portdata.dat",
        1080,
        "stardat",
        "legacy portrait mappings",
        members,
    )?;
    let portrait_tbl = member(
        archive,
        "arr\\portdata.tbl",
        65536,
        "stardat",
        "portrait path prefixes",
        members,
    )?;
    let sfxdata = member(
        archive,
        "arr\\sfxdata.dat",
        8712,
        "stardat",
        "legacy sound mappings",
        members,
    )?;
    let sound_tbl = member(
        archive,
        "arr\\sfxdata.tbl",
        65536,
        "stardat",
        "sound paths",
        members,
    )?;
    ensure!(
        portdata.len() == 1080 && sfxdata.len() == 8712,
        "unsupported legacy portrait/sound DAT layout"
    );
    let mut manifest = MediaManifest {
        schema_version: 1,
        audio: Vec::new(),
        music: Vec::new(),
        mission_audio: Vec::new(),
        mission_texts: Vec::new(),
        briefing: Vec::new(),
        portraits: Vec::new(),
    };
    let mut sound_cache = BTreeMap::<u16, AudioRef>::new();
    let mut sounds = Vec::new();
    let mut pcm_bytes = 0;
    let mut add_cue = |cue, unit_type, ids: &[u16], voice| -> Result<()> {
        let mut variants = Vec::new();
        for &id in ids {
            let reference = if let Some(reference) = sound_cache.get(&id) {
                reference.clone()
            } else {
                let path = sound_path(&sfxdata, &sound_tbl, id)?;
                let source = member(
                    archive,
                    &path,
                    MAX_AUDIO_BYTES,
                    "stardat",
                    "native sound cue",
                    members,
                )?;
                let bytes = normalize_wav(&source, 30_000, &mut pcm_bytes)?;
                let reference = audio_file(files, format!("sound-{id:03}.wav"), bytes);
                sounds.push((id, path));
                sound_cache.insert(id, reference.clone());
                reference
            };
            variants.push(reference);
        }
        manifest.audio.push(AudioMapping {
            cue,
            unit_type,
            voice,
            variants,
        });
        Ok(())
    };
    for unit in &mappings {
        let id = Some(UnitTypeId(unit.native_id));
        add_cue(
            AudioCue::Select,
            id,
            &inclusive(unit.select)?,
            unit.ready.is_some(),
        )?;
        if let Some(range) = unit.order {
            add_cue(AudioCue::Order, id, &inclusive(range)?, true)?;
        }
        if let Some(ready) = unit.ready {
            add_cue(AudioCue::Ready, id, &[ready], true)?;
        }
    }
    add_cue(AudioCue::Attack, Some(UnitTypeId(1)), &[69], false)?;
    add_cue(AudioCue::Death, Some(UnitTypeId(1)), &[276, 277], false)?;
    add_cue(
        AudioCue::Attack,
        Some(UnitTypeId(2)),
        &[35, 36, 37, 38, 39],
        false,
    )?;
    add_cue(
        AudioCue::Work,
        Some(UnitTypeId(2)),
        &[35, 36, 37, 38, 39],
        false,
    )?;
    add_cue(AudioCue::Death, Some(UnitTypeId(2)), &[369], false)?;
    add_cue(AudioCue::Complete, Some(UnitTypeId(2)), &[136], true)?;
    for id in [3, 4, 5] {
        add_cue(AudioCue::Death, Some(UnitTypeId(id)), &[7], false)?;
    }
    add_cue(AudioCue::Error, None, &[2], false)?;

    let mut music_sources = Vec::new();
    for number in 1..=3 {
        let path = format!("music\\terran{number}.wav");
        let source = member(
            installer,
            &path,
            MAX_AUDIO_BYTES,
            "install",
            "Terran soundtrack",
            members,
        )?;
        let bytes = normalize_wav(&source, 600_000, &mut pcm_bytes)?;
        manifest.music.push(audio_file(
            files,
            format!("music-terran-{number}.wav"),
            bytes,
        ));
        music_sources.push(path);
    }

    let mut portrait_sources = Vec::new();
    let mut portrait_cache = BTreeMap::<u16, (u32, Vec<ImageRef>, Vec<ImageRef>)>::new();
    let mut rgba_bytes = 0;
    for unit in &mappings {
        if let Entry::Vacant(entry) = portrait_cache.entry(unit.portrait) {
            let mut clips = [Vec::new(), Vec::new()];
            let mut frame_ms = None;
            for (state, variants) in [(0, 4), (1, 3)] {
                let prefix = portrait_prefix(&portdata, &portrait_tbl, unit.portrait, state)?;
                for variant in 0..variants {
                    let path = format!("portrait\\{prefix}{variant}.smk");
                    let source = member(
                        archive,
                        &path,
                        MAX_SMK_BYTES,
                        "stardat",
                        "native selected-unit portrait",
                        members,
                    )?;
                    let decoded =
                        decode_smk(&source).with_context(|| format!("portrait {path}"))?;
                    ensure!(
                        frame_ms.is_none_or(|ms| ms == decoded.frame_ms),
                        "portrait variants have differing frame durations"
                    );
                    frame_ms = Some(decoded.frame_ms);
                    for image in decoded.frames {
                        rgba_bytes += image.rgba.len();
                        ensure!(
                            rgba_bytes <= MAX_PORTRAIT_BYTES,
                            "converted portraits exceed 64 MiB"
                        );
                        let name = format!(
                            "portrait-{}-{state}-{:03}.srim",
                            unit.portrait,
                            clips[state].len()
                        );
                        clips[state].push(add_image(files, &name, &image)?);
                    }
                    ensure!(
                        clips[state].len() <= 256,
                        "portrait sequence exceeds 256 frames"
                    );
                    portrait_sources.push(path);
                }
            }
            let [idle, talk] = clips;
            entry.insert((frame_ms.context("empty portrait")?, idle, talk));
        }
        let (frame_ms, idle, talk) = &portrait_cache[&unit.portrait];
        manifest.portraits.push(PortraitManifest {
            portrait_only: false,
            unit_type: UnitTypeId(unit.native_id),
            frame_ms: *frame_ms,
            idle: idle.clone(),
            talk: talk.clone(),
        });
    }
    manifest.validate()?;
    files.insert("media.ron".into(), ron_bytes(&manifest)?);
    files.insert("media-reference.ron".into(), ron_bytes(&MediaReport {
        schema_version: 1,
        unit_mappings: mappings,
        sounds,
        portrait_sources,
        music_sources,
        decoder: "FFmpeg smackvideo to RGBA8; one thread, bitexact flags, unchanged 60x56 canvas and source 100 ms frame duration. PCM WAV is parsed and re-encoded directly without resampling.".into(),
        evidence: vec![
            "Original units.dat selects portrait 0 for Marine, 7 for SCV, and 17 (Terran advisor) for all three buildings. Source portdata.dat contains 90 records and sfxdata.dat 968 records; both use 1-based TBL references.",
            "Mobile selection/order ranges and ready sounds come from units.dat. Marine death script78 selects sounds276..277 and attack uses69. SCV death script84 uses369; its attack/mining image526 script237 selects35..39. All building death scripts use7. Building selection uses DAT sound15 for Command Center/Barracks and397 for Depot. Construction completion uses SCV acknowledgement136.",
            "Portraits retain all four idle and three talking variants for each of the Marine, SCV and advisor source families. Their native idle/talk sequences have45/30 frames respectively. Talking is selected only for voice responses; mechanical responses remain idle.",
            "Three original Terran music tracks come from the installer archive. Original sound and portrait member hashes are recorded in terran-reference.ron; native bytes and hashes are recorded in media.ron/import-report.ron.",
        ],
        limitations: vec![
            "Current-role event categories are selection, orders, readiness, attacks, work, death, construction completion and generic invalid-action feedback. Repeated-selection annoyed lines, specialized advisor warnings, attack alerts, research, upgrades, transport and unsupported unit abilities are not mapped.",
            "Idle and talking portrait variants play in source filename order rather than reproducing the original weighted/random variant selection. Speech duration drives talking; original portrait length adjustments, transitions and lip synchronization are not reproduced.",
            "Audio cue cadence, priority, spatial attenuation and music scheduling follow the native demonstration, not a verified reproduction of the original mixer. Source ADPCM is already lossy; PCM conversion cannot recover discarded detail.",
            "Native voice classification follows the actual response: Depot's mechanical selection remains an idle portrait, while the SCV's spoken completion response talks. This does not directly copy sfxdata's unit_speech flag, which marks the former and not the latter.",
        ],
    })?);
    Ok(())
}

fn unit_mappings(bytes: &[u8]) -> Result<Vec<UnitMedia>> {
    ensure!(
        bytes.len() == 19192,
        "unsupported legacy units.dat media layout"
    );
    Ok([(0, 1), (7, 2), (106, 3), (109, 4), (111, 5)]
        .into_iter()
        .map(|(source_id, native_id)| {
            let index = usize::from(source_id) * 2;
            UnitMedia {
                source_id,
                native_id,
                portrait: word(bytes, 0x367c + index),
                select: [word(bytes, 0x236c + index), word(bytes, 0x2534 + index)],
                order: (source_id < 106)
                    .then(|| [word(bytes, 0x28a4 + index), word(bytes, 0x2978 + index)]),
                ready: (source_id < 106).then(|| word(bytes, 0x2298 + index)),
            }
        })
        .collect())
}

fn inclusive([first, last]: [u16; 2]) -> Result<Vec<u16>> {
    ensure!(
        first != 0 && first <= last && last - first < 16,
        "invalid source voice range"
    );
    Ok((first..=last).collect())
}

fn word(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(bytes[offset..offset + 2].try_into().unwrap())
}

pub(super) fn table_string(bytes: &[u8], index: u32) -> Result<&str> {
    ensure!(
        bytes.len() >= 2 && bytes.len() <= 65536,
        "invalid TBL length"
    );
    let count = usize::from(word(bytes, 0));
    ensure!(
        bytes.len() >= 2 + count * 2 && index > 0 && index as usize <= count,
        "invalid TBL reference"
    );
    let offset = usize::from(word(bytes, index as usize * 2));
    ensure!(
        offset >= 2 + count * 2 && offset < bytes.len(),
        "TBL string offset outside data"
    );
    let tail = &bytes[offset..];
    let end = tail
        .iter()
        .position(|byte| *byte == 0)
        .context("unterminated TBL path")?;
    ensure!(
        end > 0 && end <= 128 && tail[..end].is_ascii(),
        "invalid TBL path"
    );
    let path = std::str::from_utf8(&tail[..end])?;
    ensure!(
        path.split(['/', '\\'])
            .all(|part| !part.is_empty() && part != "." && part != "..")
            && !path.contains(':'),
        "unsafe TBL path"
    );
    Ok(path)
}

pub(super) fn sound_path(data: &[u8], table: &[u8], id: u16) -> Result<String> {
    ensure!(
        data.len() == 8712 && id < 968,
        "invalid legacy sound reference"
    );
    let offset = usize::from(id) * 4;
    let index = u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap());
    Ok(format!("sound\\{}", table_string(table, index)?))
}

pub(super) fn portrait_prefix<'a>(
    data: &[u8],
    table: &'a [u8],
    id: u16,
    state: usize,
) -> Result<&'a str> {
    ensure!(
        data.len() == 1080 && id < 90 && state <= 1,
        "invalid legacy portrait reference"
    );
    let offset = state * 90 * 4 + usize::from(id) * 4;
    let index = u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap());
    table_string(table, index)
}

fn verify_sound_scripts(bytes: &[u8]) -> Result<()> {
    // Check selected sound opcodes instead of interpreting a script program.
    for (id, animation, prefix) in [
        (78, 1, &[0x1a, 0x14, 1, 0x15, 1][..]),
        (78, 5, &[5, 1, 0x2e, 0x18, 69, 0][..]),
        (84, 1, &[0x18, 0x71, 1][..]),
        (237, 1, &[0, 0, 0, 0x1a, 35, 0, 39, 0][..]),
        (96, 1, &[0x18, 7, 0][..]),
        (102, 1, &[0x18, 7, 0][..]),
        (105, 1, &[0x18, 7, 0][..]),
    ] {
        ensure!(
            crate::terran::script_animation(bytes, id, animation)?.starts_with(prefix),
            "unrecognized source sound instructions for script {id}"
        );
    }
    Ok(())
}

pub(super) fn normalize_wav(bytes: &[u8], max_ms: u64, cumulative: &mut usize) -> Result<Vec<u8>> {
    ensure!(bytes.len() <= MAX_AUDIO_BYTES, "source WAV exceeds 64 MiB");
    let pcm = decode_wav(bytes)
        .context("source audio requires supported PCM16 WAV after MPQ decoding")?;
    ensure!(
        pcm.duration_ms() <= max_ms,
        "source audio exceeds duration limit"
    );
    *cumulative += pcm.samples.len() * 2;
    ensure!(
        *cumulative <= MAX_PCM_BYTES,
        "converted audio exceeds 256 MiB"
    );
    encode_wav(pcm.channels, pcm.sample_rate, &pcm.samples)
}

pub(super) fn audio_file(files: &mut Files, file: String, bytes: Vec<u8>) -> AudioRef {
    let blake3 = blake3::hash(&bytes).to_hex().to_string();
    files.insert(file.clone(), bytes);
    AudioRef { file, blake3 }
}

pub(super) struct PortraitFrames {
    pub frame_ms: u32,
    pub frames: Vec<Image>,
}

fn smk_header(bytes: &[u8]) -> Result<(u32, u32, u32, u32)> {
    smk_header_limits(bytes, 256, false)
}

fn smk_header_limits(bytes: &[u8], dimension: u32, ring: bool) -> Result<(u32, u32, u32, u32)> {
    ensure!(
        (104..=MAX_SMK_BYTES).contains(&bytes.len()) && matches!(&bytes[..4], b"SMK2" | b"SMK4"),
        "invalid bounded SMK header"
    );
    let integer = |offset| u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
    let (width, height, frames) = (integer(4), integer(8), integer(12));
    ensure!(
        (1..=dimension).contains(&width)
            && (1..=dimension).contains(&height)
            && (1..=128).contains(&frames),
        "SMK portrait dimensions/frame count exceed limits"
    );
    ensure!(
        integer(20) == 0 || (ring && integer(20) == 1),
        "ring/interlaced/doubled SMK portraits are unsupported"
    );
    let time = i32::from_le_bytes(bytes[16..20].try_into().unwrap());
    let frame_ms = if time < 0 {
        ensure!(
            time != i32::MIN && time % 100 == 0,
            "SMK submillisecond timing is unsupported"
        );
        (-time / 100) as u32
    } else {
        time as u32
    };
    ensure!(
        (10..=10000).contains(&frame_ms),
        "SMK frame duration exceeds limits"
    );
    let encoded_frames = frames as usize + usize::from(integer(20) & 1 != 0);
    let table_end = 104 + encoded_frames * 5;
    let tree_bytes = integer(52) as usize;
    ensure!(
        [56, 60, 64, 68]
            .iter()
            .all(|offset| integer(*offset) as usize <= MAX_SMK_BYTES),
        "SMK expanded Huffman trees exceed limits"
    );
    ensure!(
        tree_bytes <= MAX_SMK_BYTES && table_end + tree_bytes <= bytes.len(),
        "SMK frame table/trees exceed input"
    );
    let mut extent = table_end + tree_bytes;
    for frame in 0..encoded_frames {
        let size = integer(104 + frame * 4) as usize & !3;
        extent = extent
            .checked_add(size)
            .context("SMK frame extent overflow")?;
        ensure!(extent <= bytes.len(), "SMK frame exceeds input");
    }
    ensure!(
        width as usize * height as usize * frames as usize * 4 <= MAX_PORTRAIT_BYTES,
        "SMK decoded size exceeds limit"
    );
    Ok((width, height, frames, frame_ms))
}

pub(super) fn decode_smk(bytes: &[u8]) -> Result<PortraitFrames> {
    decode_smk_frames(bytes, smk_header(bytes)?)
}

pub(super) fn decode_menu_smk(bytes: &[u8]) -> Result<PortraitFrames> {
    decode_smk_frames(bytes, smk_header_limits(bytes, 640, true)?)
}

fn decode_smk_frames(bytes: &[u8], header: (u32, u32, u32, u32)) -> Result<PortraitFrames> {
    let (width, height, frames, frame_ms) = header;
    let directory = tempfile::tempdir().context("cannot stage portrait conversion")?;
    let input = directory.path().join("portrait.smk");
    let output = directory.path().join("portrait.rgba");
    let errors = directory.path().join("ffmpeg.log");
    fs::write(&input, bytes)?;
    let mut child = Command::new("ffmpeg")
        .args([
            "-v",
            "error",
            "-nostdin",
            "-y",
            "-max_alloc",
            "33554432",
            "-threads",
            "1",
            "-protocol_whitelist",
            "file",
            "-f",
            "smk",
            "-i",
        ])
        .arg(&input)
        .args(["-map", "0:v:0", "-an", "-sn", "-dn", "-frames:v"])
        .arg(frames.to_string())
        .args([
            "-threads",
            "1",
            "-fflags",
            "+bitexact",
            "-flags:v",
            "+bitexact",
            "-pix_fmt",
            "rgba",
            "-f",
            "rawvideo",
        ])
        .arg(&output)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(File::create(&errors)?)
        .spawn()
        .context("portrait import requires ffmpeg with the Smacker decoder installed")?;
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error).context("cannot monitor FFmpeg portrait conversion");
            }
        }
        if started.elapsed() > Duration::from_secs(30)
            || !fs::metadata(&errors).is_ok_and(|metadata| metadata.len() <= 65536)
        {
            let _ = child.kill();
            let _ = child.wait();
            anyhow::bail!("FFmpeg portrait conversion exceeded time or diagnostic output limits");
        }
        thread::sleep(Duration::from_millis(10));
    };
    if !status.success() {
        let mut error = String::new();
        File::open(errors)?.take(4096).read_to_string(&mut error)?;
        anyhow::bail!("FFmpeg portrait decode failed: {}", error.trim());
    }
    let frame_bytes = width as usize * height as usize * 4;
    let expected = frame_bytes * frames as usize;
    ensure!(
        fs::metadata(&output)?.len() == expected as u64,
        "FFmpeg portrait output length differs from source header"
    );
    let rgba = fs::read(output)?;
    Ok(PortraitFrames {
        frame_ms,
        frames: rgba
            .chunks_exact(frame_bytes)
            .map(|rgba| Image {
                width,
                height,
                rgba: rgba.to_vec(),
            })
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tbl(path: &[u8]) -> Vec<u8> {
        let mut bytes = vec![1, 0, 4, 0];
        bytes.extend(path);
        bytes.push(0);
        bytes
    }

    #[test]
    fn legacy_media_tables_validate_offsets_ranges_and_paths() {
        let table = tbl(b"Terran\\SCV\\TSCRdy00.WAV");
        let mut sounds = vec![0; 8712];
        sounds[368 * 4..368 * 4 + 4].copy_from_slice(&1_u32.to_le_bytes());
        assert_eq!(
            sound_path(&sounds, &table, 368).unwrap(),
            "sound\\Terran\\SCV\\TSCRdy00.WAV"
        );
        assert!(sound_path(&sounds, &table, 968).is_err());
        assert!(sound_path(&sounds[..8711], &table, 368).is_err());
        assert!(table_string(&table, 0).is_err());
        assert!(table_string(&table, 2).is_err());
        assert!(table_string(&tbl(b"..\\escape"), 1).is_err());
        assert!(table_string(&[1, 0, 0, 0], 1).is_err());
        assert!(table_string(&table[..table.len() - 1], 1).is_err());
        assert_eq!(inclusive([287, 290]).unwrap(), [287, 288, 289, 290]);
        assert!(inclusive([290, 287]).is_err());
        assert!(inclusive([0, 1]).is_err());
        assert!(inclusive([1, 17]).is_err());
        let mut units = vec![0; 19192];
        for (offset, value) in [
            (0x2298, 275_u16),
            (0x236c, 287),
            (0x2534, 290),
            (0x28a4, 291),
            (0x2978, 294),
            (0x367c + 14, 7),
        ] {
            units[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
        }
        let decoded = unit_mappings(&units).unwrap();
        assert_eq!(decoded[0].ready, Some(275));
        assert_eq!(decoded[0].select, [287, 290]);
        assert_eq!(decoded[0].order, Some([291, 294]));
        assert_eq!(decoded[1].portrait, 7);
        assert!(decoded[2].ready.is_none());
        assert!(unit_mappings(&units[..19191]).is_err());
    }

    #[test]
    fn smk_bounds_reject_corruption_before_launching_decoder() {
        let mut bytes = vec![0; 109];
        bytes[..4].copy_from_slice(b"SMK2");
        for (offset, value) in [(4, 60_u32), (8, 56), (12, 1), (16, (-10000_i32) as u32)] {
            bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        assert_eq!(smk_header(&bytes).unwrap(), (60, 56, 1, 100));
        for (offset, value) in [
            (4, 257_u32),
            (8, 0),
            (12, 129),
            (16, i32::MIN as u32),
            (20, 1),
            (52, 1000),
            (56, MAX_SMK_BYTES as u32 + 1),
            (104, 1000),
        ] {
            let mut invalid = bytes.clone();
            invalid[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
            assert!(smk_header(&invalid).is_err(), "offset {offset}");
        }
        assert!(smk_header(&bytes[..103]).is_err());
    }

    #[test]
    fn audio_normalization_is_repeatable_and_enforces_duration_and_total_size() {
        let source = encode_wav(1, 8000, &[0, 200, -200, i16::MAX, i16::MIN]).unwrap();
        let mut total = 0;
        assert_eq!(normalize_wav(&source, 30000, &mut total).unwrap(), source);
        assert_eq!(total, 10);
        let source = encode_wav(1, 8000, &vec![0; 16000]).unwrap();
        assert!(normalize_wav(&source, 1000, &mut total).is_err());
        let mut total = MAX_PCM_BYTES;
        assert!(normalize_wav(&source, 30000, &mut total).is_err());
    }
}
