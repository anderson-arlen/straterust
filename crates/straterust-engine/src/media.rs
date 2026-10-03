//! Optional native audio and portraits. These resources never enter simulation hashes.
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    path::Path,
    sync::Arc,
};

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

use crate::{
    assets::{Image, ImageRef, decode_image},
    content::read_ron,
    sim::{MissionAction, UnitTypeId, World},
};

pub const MAX_PCM_BYTES: usize = 256 * 1024 * 1024;
pub const MAX_PORTRAIT_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_WAV_BYTES: usize = 128 * 1024 * 1024;
const MAX_PORTRAIT_FILE: usize = 16 + 256 * 256 * 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum AudioCue {
    Select,
    Order,
    Ready,
    Attack,
    AttackAir,
    Work,
    Death,
    Complete,
    Error,
    Scan,
    Load,
    Unload,
    Capture,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AudioRef {
    pub file: String,
    pub blake3: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AudioMapping {
    pub cue: AudioCue,
    #[serde(default)]
    pub unit_type: Option<UnitTypeId>,
    /// Spoken acknowledgements use the voice channel and talking portrait.
    #[serde(default)]
    pub voice: bool,
    pub variants: Vec<AudioRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PortraitManifest {
    pub unit_type: UnitTypeId,
    /// Dialogue-only speaker ID; must not name a gameplay unit.
    #[serde(default)]
    pub portrait_only: bool,
    pub frame_ms: u32,
    pub idle: Vec<ImageRef>,
    pub talk: Vec<ImageRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum BriefingAction {
    Objectives {
        text: u16,
    },
    ShowPortrait {
        slot: u8,
        portrait: UnitTypeId,
    },
    HidePortrait {
        slot: u8,
    },
    Wait {
        milliseconds: u32,
    },
    Sound {
        sound: u16,
    },
    Transmission {
        slot: u8,
        text: u16,
        sound: Option<u16>,
        milliseconds: u32,
    },
    Text {
        text: u16,
        milliseconds: u32,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MediaManifest {
    pub schema_version: u32,
    #[serde(default)]
    pub audio: Vec<AudioMapping>,
    #[serde(default)]
    pub music: Vec<AudioRef>,
    /// Indexed sounds used by imported mission and briefing actions.
    #[serde(default)]
    pub mission_audio: Vec<AudioRef>,
    #[serde(default)]
    pub mission_texts: Vec<String>,
    #[serde(default)]
    pub briefing: Vec<BriefingAction>,
    #[serde(default)]
    pub portraits: Vec<PortraitManifest>,
}

#[derive(Debug, Clone)]
pub struct PcmClip {
    pub channels: u16,
    pub sample_rate: u32,
    /// Interleaved signed PCM16. Shared by mixer voices without copying music.
    pub samples: Arc<[i16]>,
}
impl PcmClip {
    pub fn duration_ms(&self) -> u64 {
        self.samples.len() as u64 * 1000 / (u64::from(self.channels) * u64::from(self.sample_rate))
    }
}
#[derive(Debug, Clone)]
pub struct AudioClips {
    pub cue: AudioCue,
    pub unit_type: Option<UnitTypeId>,
    pub voice: bool,
    pub variants: Vec<Arc<PcmClip>>,
}
#[derive(Debug)]
pub struct Portrait {
    pub unit_type: UnitTypeId,
    pub portrait_only: bool,
    pub frame_ms: u32,
    pub idle: Vec<Image>,
    pub talk: Vec<Image>,
}
#[derive(Debug)]
pub struct MediaPack {
    pub audio: Vec<AudioClips>,
    pub music: Vec<Arc<PcmClip>>,
    pub mission_audio: Vec<Arc<PcmClip>>,
    pub mission_texts: Vec<String>,
    pub briefing: Vec<BriefingAction>,
    pub portraits: Vec<Portrait>,
}

impl MediaManifest {
    pub fn validate(&self) -> Result<()> {
        ensure!(self.schema_version == 1, "unsupported native media schema");
        ensure!(
            self.audio.len() <= 128
                && self.music.len() <= 8
                && self.mission_audio.len() <= 128
                && self.portraits.len() <= 64,
            "too many media mappings"
        );
        ensure!(
            self.mission_texts.len() <= 256
                && self.mission_texts.iter().map(String::len).sum::<usize>() <= 256 * 1024
                && self.mission_texts.iter().all(|text| !text.is_empty()
                    && text.len() <= 8192
                    && !text
                        .chars()
                        .any(|c| c.is_control() && !matches!(c, '\r' | '\n' | '\t'))),
            "invalid mission text limits or characters"
        );
        ensure!(self.briefing.len() <= 256, "too many briefing actions");
        let text = |id: u16| -> Result<()> {
            ensure!(
                usize::from(id) < self.mission_texts.len(),
                "unknown briefing text"
            );
            Ok(())
        };
        let sound = |id: u16| -> Result<()> {
            ensure!(
                usize::from(id) < self.mission_audio.len(),
                "unknown briefing sound"
            );
            Ok(())
        };
        for action in &self.briefing {
            match action {
                BriefingAction::Objectives { text: id } => text(*id)?,
                BriefingAction::ShowPortrait { slot, portrait } => {
                    ensure!(
                        *slot < 4
                            && self
                                .portraits
                                .iter()
                                .any(|entry| entry.unit_type == *portrait),
                        "unknown briefing portrait or slot"
                    );
                }
                BriefingAction::HidePortrait { slot } => {
                    ensure!(*slot < 4, "invalid briefing portrait slot")
                }
                BriefingAction::Wait { milliseconds } => {
                    ensure!(*milliseconds <= 3_600_000, "briefing wait exceeds one hour")
                }
                BriefingAction::Sound { sound: id } => sound(*id)?,
                BriefingAction::Transmission {
                    slot,
                    text: id,
                    sound: clip,
                    milliseconds,
                } => {
                    ensure!(
                        *slot < 4 && *milliseconds <= 3_600_000,
                        "invalid briefing transmission"
                    );
                    text(*id)?;
                    if let Some(id) = clip {
                        sound(*id)?;
                    }
                }
                BriefingAction::Text {
                    text: id,
                    milliseconds,
                } => {
                    text(*id)?;
                    ensure!(*milliseconds <= 3_600_000, "briefing text exceeds one hour");
                }
            }
        }
        let mut cues = BTreeSet::new();
        for entry in &self.audio {
            ensure!(
                cues.insert((entry.cue, entry.unit_type)),
                "duplicate audio cue mapping"
            );
            ensure!(
                (1..=16).contains(&entry.variants.len()),
                "audio cue requires 1..=16 variants"
            );
            for reference in &entry.variants {
                validate_reference(&reference.file, &reference.blake3)?;
            }
        }
        for reference in self.music.iter().chain(&self.mission_audio) {
            validate_reference(&reference.file, &reference.blake3)?;
        }
        let mut units = BTreeSet::new();
        for portrait in &self.portraits {
            ensure!(
                units.insert(portrait.unit_type),
                "duplicate portrait mapping"
            );
            ensure!(
                (10..=10000).contains(&portrait.frame_ms),
                "invalid portrait frame duration"
            );
            ensure!(
                (1..=256).contains(&portrait.idle.len())
                    && (1..=256).contains(&portrait.talk.len()),
                "portrait requires 1..=256 idle and talk frames"
            );
            for reference in portrait.idle.iter().chain(&portrait.talk) {
                validate_reference(&reference.file, &reference.blake3)?;
            }
        }
        Ok(())
    }
}

impl MediaPack {
    pub fn load(directory: &Path) -> Result<Option<Self>> {
        match fs::symlink_metadata(directory.join("media.ron")) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            result => ensure!(
                result.context("cannot inspect media.ron")?.is_file(),
                "media.ron must be a regular file, not a symlink"
            ),
        }
        let root = directory
            .canonicalize()
            .context("cannot resolve media package")?;
        let manifest: MediaManifest = read_ron(&root.join("media.ron"))?;
        manifest.validate()?;
        let mut cache = BTreeMap::<String, (String, Arc<PcmClip>)>::new();
        let mut pcm_bytes = 0;
        let mut audio_file = |reference: &AudioRef| -> Result<Arc<PcmClip>> {
            if let Some((digest, clip)) = cache.get(&reference.file) {
                ensure!(*digest == reference.blake3, "conflicting media digest");
                return Ok(Arc::clone(clip));
            }
            let bytes = read_file(&root, &reference.file, &reference.blake3, MAX_WAV_BYTES)?;
            let clip = Arc::new(
                decode_wav(&bytes)
                    .with_context(|| format!("invalid PCM WAV {}", reference.file))?,
            );
            pcm_bytes += clip.samples.len() * 2;
            ensure!(
                pcm_bytes <= MAX_PCM_BYTES,
                "native audio exceeds resident PCM limit"
            );
            cache.insert(
                reference.file.clone(),
                (reference.blake3.clone(), Arc::clone(&clip)),
            );
            Ok(clip)
        };
        let mut audio = Vec::new();
        for entry in manifest.audio {
            let variants = entry
                .variants
                .iter()
                .map(&mut audio_file)
                .collect::<Result<Vec<_>>>()?;
            ensure!(
                variants.iter().all(|clip| clip.duration_ms() <= 30000),
                "audio cue exceeds 30 second limit"
            );
            audio.push(AudioClips {
                cue: entry.cue,
                unit_type: entry.unit_type,
                voice: entry.voice,
                variants,
            });
        }
        let music = manifest
            .music
            .iter()
            .map(&mut audio_file)
            .collect::<Result<Vec<_>>>()?;
        let mission_audio = manifest
            .mission_audio
            .iter()
            .map(&mut audio_file)
            .collect::<Result<Vec<_>>>()?;
        ensure!(
            mission_audio
                .iter()
                .all(|clip| clip.duration_ms() <= 300_000),
            "mission audio exceeds five minute limit"
        );
        let mut portrait_bytes = 0;
        let mut image_file = |reference: &ImageRef| -> Result<Image> {
            let bytes = read_file(&root, &reference.file, &reference.blake3, MAX_PORTRAIT_FILE)?;
            let image = decode_image(&bytes)?;
            ensure!(
                image.width <= 256 && image.height <= 256,
                "portrait exceeds 256 pixel dimension limit"
            );
            portrait_bytes += image.rgba.len();
            ensure!(
                portrait_bytes <= MAX_PORTRAIT_BYTES,
                "portraits exceed resident RGBA limit"
            );
            Ok(image)
        };
        let mut portraits = Vec::new();
        for entry in manifest.portraits {
            let idle = entry
                .idle
                .iter()
                .map(&mut image_file)
                .collect::<Result<Vec<_>>>()?;
            let talk = entry
                .talk
                .iter()
                .map(&mut image_file)
                .collect::<Result<Vec<_>>>()?;
            ensure!(
                idle.iter()
                    .chain(&talk)
                    .all(|image| image.width == idle[0].width && image.height == idle[0].height),
                "portrait frames must share dimensions"
            );
            portraits.push(Portrait {
                unit_type: entry.unit_type,
                portrait_only: entry.portrait_only,
                frame_ms: entry.frame_ms,
                idle,
                talk,
            });
        }
        Ok(Some(Self {
            audio,
            music,
            mission_audio,
            mission_texts: manifest.mission_texts,
            briefing: manifest.briefing,
            portraits,
        }))
    }

    pub fn validate_world(&self, world: &World) -> Result<()> {
        for id in self.audio.iter().filter_map(|entry| entry.unit_type).chain(
            self.portraits
                .iter()
                .filter(|portrait| !portrait.portrait_only)
                .map(|portrait| portrait.unit_type),
        ) {
            ensure!(
                world.unit_type(id).is_some(),
                "media references unknown unit type {}",
                id.0
            );
        }
        for portrait in self
            .portraits
            .iter()
            .filter(|portrait| portrait.portrait_only)
        {
            ensure!(
                world.unit_type(portrait.unit_type).is_none(),
                "dialogue portrait ID collides with gameplay unit"
            );
        }
        if let Some(mission) = &world.map().mission {
            for action in mission.triggers.iter().flat_map(|trigger| &trigger.actions) {
                match action {
                    MissionAction::Objectives { text } | MissionAction::Text { text } => ensure!(
                        usize::from(*text) < self.mission_texts.len(),
                        "unknown mission text"
                    ),
                    MissionAction::Sound { sound } => ensure!(
                        usize::from(*sound) < self.mission_audio.len(),
                        "unknown mission sound"
                    ),
                    MissionAction::Transmission {
                        text,
                        sound,
                        portrait,
                        ..
                    } => {
                        ensure!(
                            usize::from(*text) < self.mission_texts.len()
                                && sound
                                    .is_none_or(|id| usize::from(id) < self.mission_audio.len())
                                && self.portrait(*portrait).is_some(),
                            "unknown mission transmission text, sound or portrait"
                        );
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }

    pub fn portrait(&self, unit_type: UnitTypeId) -> Option<&Portrait> {
        self.portraits
            .iter()
            .find(|portrait| portrait.unit_type == unit_type)
    }
}

fn validate_reference(file: &str, digest: &str) -> Result<()> {
    ensure!(
        !file.is_empty()
            && file.len() <= 128
            && file != "."
            && file != ".."
            && file
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte)),
        "media path must be a simple package filename"
    );
    ensure!(
        digest.len() == 64
            && digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
        "invalid media BLAKE3 digest"
    );
    Ok(())
}

fn read_file(root: &Path, name: &str, digest: &str, limit: usize) -> Result<Vec<u8>> {
    validate_reference(name, digest)?;
    let path = root.join(name);
    ensure!(
        fs::symlink_metadata(&path)?.is_file(),
        "media member must be a regular file, not a symlink"
    );
    let file = fs::File::open(&path)?;
    ensure!(
        file.metadata()?.is_file() && file.metadata()?.len() <= limit as u64,
        "media member exceeds file bound"
    );
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1).read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= limit, "media member exceeds file bound");
    ensure!(
        blake3::hash(&bytes).to_hex().as_str() == digest,
        "media hash mismatch: {name}"
    );
    Ok(bytes)
}

/// Strict uncompressed, little-endian PCM16 RIFF/WAVE. Unknown bounded chunks
/// are skipped; compressed codecs and multiple fmt/data chunks are rejected.
pub fn decode_wav(bytes: &[u8]) -> Result<PcmClip> {
    ensure!(
        bytes.len() >= 12 && bytes.len() <= MAX_WAV_BYTES,
        "invalid WAV size"
    );
    ensure!(
        &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WAVE",
        "invalid WAV header"
    );
    let word = |offset| u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
    ensure!(
        u64::from(word(4)) + 8 == bytes.len() as u64,
        "WAV RIFF length mismatch"
    );
    let mut cursor = 12;
    let mut format = None;
    let mut data = None;
    let mut chunks = 0;
    while cursor < bytes.len() {
        chunks += 1;
        ensure!(
            chunks <= 64 && bytes.len() - cursor >= 8,
            "invalid WAV chunk count/header"
        );
        let length = word(cursor + 4) as usize;
        let start = cursor + 8;
        let end = start.checked_add(length).context("WAV chunk overflow")?;
        ensure!(end <= bytes.len(), "truncated WAV chunk");
        match &bytes[cursor..cursor + 4] {
            b"fmt " => {
                ensure!(
                    format.is_none() && (16..=40).contains(&length),
                    "invalid/duplicate WAV format"
                );
                let short = |offset| {
                    u16::from_le_bytes(
                        bytes[start + offset..start + offset + 2]
                            .try_into()
                            .unwrap(),
                    )
                };
                let channels = short(2);
                let sample_rate = word(start + 4);
                ensure!(
                    short(0) == 1 && short(14) == 16,
                    "native WAV requires PCM16"
                );
                ensure!(
                    (1..=2).contains(&channels) && (8000..=48000).contains(&sample_rate),
                    "unsupported WAV channel count/rate"
                );
                ensure!(
                    short(12) == channels * 2
                        && word(start + 8) == sample_rate * u32::from(channels) * 2,
                    "inconsistent WAV alignment/rate"
                );
                format = Some((channels, sample_rate));
            }
            b"data" => {
                ensure!(data.is_none(), "duplicate WAV data");
                data = Some(&bytes[start..end]);
            }
            _ => {}
        }
        cursor = end
            .checked_add(length & 1)
            .context("WAV padding overflow")?;
        ensure!(cursor <= bytes.len(), "missing WAV padding");
    }
    let (channels, sample_rate) = format.context("WAV lacks fmt chunk")?;
    let data = data.context("WAV lacks data chunk")?;
    ensure!(
        !data.is_empty() && data.len().is_multiple_of(usize::from(channels) * 2),
        "invalid WAV sample alignment"
    );
    ensure!(
        data.len() as u64 <= u64::from(channels) * u64::from(sample_rate) * 2 * 600,
        "WAV exceeds ten minute limit"
    );
    let samples: Vec<_> = data
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| i16::from_le_bytes(*pair))
        .collect();
    Ok(PcmClip {
        channels,
        sample_rate,
        samples: samples.into(),
    })
}

pub fn encode_wav(channels: u16, sample_rate: u32, samples: &[i16]) -> Result<Vec<u8>> {
    ensure!(
        (1..=2).contains(&channels) && (8000..=48000).contains(&sample_rate),
        "unsupported WAV channel count/rate"
    );
    ensure!(
        !samples.is_empty() && samples.len().is_multiple_of(usize::from(channels)),
        "invalid WAV samples"
    );
    ensure!(
        samples.len() <= sample_rate as usize * usize::from(channels) * 600
            && samples.len() * 2 + 44 <= MAX_WAV_BYTES,
        "WAV exceeds size/duration bound"
    );
    let size = (samples.len() * 2) as u32;
    let mut bytes = Vec::with_capacity(size as usize + 44);
    bytes.extend(b"RIFF");
    bytes.extend((size + 36).to_le_bytes());
    bytes.extend(b"WAVEfmt ");
    bytes.extend(16u32.to_le_bytes());
    bytes.extend(1u16.to_le_bytes());
    bytes.extend(channels.to_le_bytes());
    bytes.extend(sample_rate.to_le_bytes());
    bytes.extend((sample_rate * u32::from(channels) * 2).to_le_bytes());
    bytes.extend((channels * 2).to_le_bytes());
    bytes.extend(16u16.to_le_bytes());
    bytes.extend(b"data");
    bytes.extend(size.to_le_bytes());
    for sample in samples {
        bytes.extend(sample.to_le_bytes());
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct Directory(std::path::PathBuf);
    impl Directory {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "straterust-media-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn save(&self, manifest: &MediaManifest) {
            fs::write(self.0.join("media.ron"), ron::to_string(manifest).unwrap()).unwrap();
        }
    }
    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn fixture() -> (Directory, MediaManifest) {
        let root = Directory::new();
        let wav = encode_wav(1, 12000, &[0, 123, -123, i16::MAX, i16::MIN]).unwrap();
        fs::write(root.0.join("voice.wav"), &wav).unwrap();
        let audio = AudioRef {
            file: "voice.wav".into(),
            blake3: blake3::hash(&wav).to_hex().to_string(),
        };
        let image = crate::assets::encode_image(&Image {
            width: 2,
            height: 2,
            rgba: vec![255; 16],
        })
        .unwrap();
        fs::write(root.0.join("portrait.srim"), &image).unwrap();
        let image = ImageRef {
            file: "portrait.srim".into(),
            blake3: blake3::hash(&image).to_hex().to_string(),
        };
        let manifest = MediaManifest {
            mission_audio: Vec::new(),
            mission_texts: Vec::new(),
            briefing: Vec::new(),
            schema_version: 1,
            audio: vec![AudioMapping {
                cue: AudioCue::Select,
                unit_type: Some(UnitTypeId(1)),
                voice: true,
                variants: vec![audio.clone()],
            }],
            music: vec![audio],
            portraits: vec![PortraitManifest {
                portrait_only: false,
                unit_type: UnitTypeId(1),
                frame_ms: 100,
                idle: vec![image.clone()],
                talk: vec![image],
            }],
        };
        root.save(&manifest);
        (root, manifest)
    }

    #[test]
    fn mission_media_refs_are_bounded_and_portrait_only_speakers_stay_outside_rules() {
        let (root, mut manifest) = fixture();
        manifest.mission_audio = vec![manifest.music[0].clone()];
        manifest.mission_texts = vec!["Mission objective".into()];
        manifest.briefing = vec![
            BriefingAction::ShowPortrait {
                slot: 3,
                portrait: UnitTypeId(1000),
            },
            BriefingAction::Transmission {
                slot: 3,
                text: 0,
                sound: Some(0),
                milliseconds: 100,
            },
        ];
        let mut speaker = manifest.portraits[0].clone();
        speaker.unit_type = UnitTypeId(1000);
        speaker.portrait_only = true;
        manifest.portraits.push(speaker);
        root.save(&manifest);
        let pack = MediaPack::load(&root.0).unwrap().unwrap();
        assert!(Arc::ptr_eq(&pack.music[0], &pack.mission_audio[0]));
        let world = crate::content::Package::load(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/fixtures"),
        )
        .unwrap()
        .world(42)
        .unwrap();
        pack.validate_world(&world).unwrap();
        manifest.briefing.push(BriefingAction::Sound { sound: 1 });
        assert!(manifest.validate().is_err());
        manifest.briefing.pop();
        manifest
            .briefing
            .push(BriefingAction::HidePortrait { slot: 4 });
        assert!(manifest.validate().is_err());
        manifest.briefing.pop();
        manifest.mission_texts[0] = "x".repeat(8193);
        assert!(manifest.validate().is_err());
        manifest.mission_texts[0] = "valid".into();
        manifest.portraits[0].portrait_only = true;
        root.save(&manifest);
        assert!(
            MediaPack::load(&root.0)
                .unwrap()
                .unwrap()
                .validate_world(&world)
                .is_err()
        );
    }

    #[test]
    fn pcm_round_trip_rejects_truncation_compression_alignment_and_duplicate_chunks() {
        let samples = [0, 1, -1, i16::MIN, i16::MAX, 123];
        let wav = encode_wav(2, 22050, &samples).unwrap();
        let decoded = decode_wav(&wav).unwrap();
        assert_eq!(decoded.samples.as_ref(), samples);
        assert_eq!((decoded.channels, decoded.sample_rate), (2, 22050));
        for len in 0..wav.len() {
            assert!(decode_wav(&wav[..len]).is_err());
        }
        for (offset, bytes) in [
            (20, 2u16.to_le_bytes()),
            (22, 0u16.to_le_bytes()),
            (32, 3u16.to_le_bytes()),
            (34, 8u16.to_le_bytes()),
        ] {
            let mut bad = wav.clone();
            bad[offset..offset + 2].copy_from_slice(&bytes);
            assert!(decode_wav(&bad).is_err());
        }
        let mut duplicate = wav.clone();
        duplicate.extend(&wav[36..]);
        let size = duplicate.len() as u32 - 8;
        duplicate[4..8].copy_from_slice(&size.to_le_bytes());
        assert!(decode_wav(&duplicate).is_err());
        assert!(encode_wav(2, 22050, &[0]).is_err());
    }

    #[test]
    fn media_loads_shared_pcm_portraits_and_rejects_bad_content_and_mappings() {
        let (root, mut manifest) = fixture();
        let pack = MediaPack::load(&root.0).unwrap().unwrap();
        assert!(Arc::ptr_eq(&pack.audio[0].variants[0], &pack.music[0]));
        assert_eq!(pack.portrait(UnitTypeId(1)).unwrap().talk[0].width, 2);
        manifest.audio.push(manifest.audio[0].clone());
        root.save(&manifest);
        assert!(MediaPack::load(&root.0).is_err());
        manifest.audio.pop();
        manifest.audio[0].variants[0].file = "../voice.wav".into();
        root.save(&manifest);
        assert!(MediaPack::load(&root.0).is_err());
        manifest.audio[0].variants[0].file = "voice.wav".into();
        manifest.portraits[0].frame_ms = 0;
        root.save(&manifest);
        assert!(MediaPack::load(&root.0).is_err());
        manifest.portraits[0].frame_ms = 100;
        root.save(&manifest);
        fs::write(root.0.join("voice.wav"), b"bad wav").unwrap();
        assert!(MediaPack::load(&root.0).is_err());
        fs::remove_file(root.0.join("media.ron")).unwrap();
        assert!(MediaPack::load(&root.0).unwrap().is_none());
    }

    #[test]
    fn media_rejects_oversized_cues_and_unknown_units_without_affecting_gameplay() {
        let (root, mut manifest) = fixture();
        let package = crate::content::Package::load(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/fixtures"),
        )
        .unwrap();
        let world = package.world(42).unwrap();
        let before = world.state_hash();
        let pack = MediaPack::load(&root.0).unwrap().unwrap();
        pack.validate_world(&world).unwrap();
        assert_eq!(world.state_hash(), before);
        manifest.portraits[0].unit_type = UnitTypeId(u16::MAX);
        root.save(&manifest);
        assert!(
            MediaPack::load(&root.0)
                .unwrap()
                .unwrap()
                .validate_world(&world)
                .is_err()
        );
        let wav = encode_wav(1, 8000, &vec![0; 8000 * 31]).unwrap();
        fs::write(root.0.join("voice.wav"), &wav).unwrap();
        let digest = blake3::hash(&wav).to_hex().to_string();
        manifest.audio[0].variants[0].blake3 = digest.clone();
        manifest.music[0].blake3 = digest;
        root.save(&manifest);
        assert!(MediaPack::load(&root.0).is_err());
    }

    #[test]
    #[cfg(unix)]
    fn media_manifest_and_members_reject_symlinks() {
        use std::os::unix::fs::symlink;
        let (root, _) = fixture();
        fs::rename(root.0.join("voice.wav"), root.0.join("real.wav")).unwrap();
        symlink("real.wav", root.0.join("voice.wav")).unwrap();
        assert!(MediaPack::load(&root.0).is_err());
        fs::rename(root.0.join("media.ron"), root.0.join("real.ron")).unwrap();
        symlink("real.ron", root.0.join("media.ron")).unwrap();
        assert!(MediaPack::load(&root.0).is_err());
    }
}
