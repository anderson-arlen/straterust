//! Original PCM recordings converted to bounded native media.
use super::{source::Source, stats};
use anyhow::{Context, Result, ensure};
use std::{collections::BTreeMap, fs, path::Path};
use straterust_engine::{media::*, sim::Rules};
use straterust_import_formats::Archive;

pub struct Library {
    pub audio: Vec<AudioMapping>,
    pub music: [Vec<AudioRef>; 2],
    pub menu: Vec<AudioRef>,
}

fn publish(root: &Path, bytes: &[u8]) -> Result<AudioRef> {
    let bytes = normalize(bytes)?;
    let hash = blake3::hash(&bytes).to_hex().to_string();
    let file = format!("{hash}.wav");
    if !root.join(&file).exists() {
        fs::write(root.join(&file), bytes)?;
    }
    Ok(AudioRef { file, blake3: hash })
}

fn recordings(
    archive: &mut Archive,
    root: &Path,
    names: &[String],
    cache: &mut BTreeMap<String, AudioRef>,
) -> Result<Vec<AudioRef>> {
    let mut result = Vec::new();
    for name in names {
        if !archive.has_file(name)? {
            continue;
        }
        let reference = if let Some(reference) = cache.get(name) {
            reference.clone()
        } else {
            let reference = publish(root, &archive.read_file(name, MAX_WAV_BYTES)?)
                .with_context(|| format!("converting recording {name}"))?;
            cache.insert(name.clone(), reference.clone());
            reference
        };
        result.push(reference);
    }
    ensure!(
        names.is_empty() || !result.is_empty(),
        "source recordings not found: {}",
        names.join(", ")
    );
    Ok(result)
}

fn numbered(prefix: &str, count: usize) -> Vec<String> {
    (1..=count)
        .map(|n| format!("Gamesfx\\{prefix}{n}.wav"))
        .collect()
}

impl Library {
    pub fn load(source: &mut Source, root: &Path, rules: &Rules) -> Result<Self> {
        let mut cache = BTreeMap::new();
        let mut audio = Vec::new();
        for unit in &rules.units {
            let original = usize::from(unit.id.0 - 1);
            let race = original % 2;
            let mut add = |cue, voice, names: Vec<String>| -> Result<()> {
                let variants = recordings(&mut source.data, root, &names, &mut cache)?;
                if !variants.is_empty() {
                    audio.push(AudioMapping {
                        cue,
                        unit_type: Some(unit.id),
                        voice,
                        variants,
                    });
                }
                Ok(())
            };
            if unit.structure {
                let building = match original {
                    58 => "hfarm",
                    59 => "ofarm",
                    62 => "hchant",
                    63 => "ochant",
                    66 => "stables",
                    67 => "ogrecamp",
                    68 => "inventor",
                    69 => "alchemst",
                    70 => "aviary",
                    71 => "dragon",
                    72 | 73 => "shipbell",
                    76 | 77 => "lumbmill",
                    78 | 79 => "foundry",
                    80 => "wzrdtowr",
                    81 => "dthtower",
                    82 | 83 => "smith",
                    84 | 85 => "oilrefin",
                    86 | 87 => "oilplat",
                    _ => "",
                };
                add(
                    AudioCue::Select,
                    false,
                    vec![if building.is_empty() {
                        "Sfx\\Button.wav".into()
                    } else {
                        format!("Gamesfx\\bldg\\{building}.wav")
                    }],
                )?;
                add(AudioCue::Death, false, numbered("misc\\bldexpl", 3))?;
                for cue in [AudioCue::Transform, AudioCue::SelectConstruction] {
                    add(cue, false, vec!["Gamesfx\\misc\\constrct.wav".into()])?;
                }
                add(
                    AudioCue::Complete,
                    true,
                    super::voices::paths(2 + race, AudioCue::Complete),
                )?;
            } else {
                for cue in [AudioCue::Select, AudioCue::Order, AudioCue::Ready] {
                    let mut names = super::voices::paths(original, cue);
                    if original == 3 && cue == AudioCue::Ready {
                        names = vec!["Gamesfx\\peon\\pnready.wav".into()];
                    }
                    if original == 5 && cue == AudioCue::Ready {
                        names = vec!["Gamesfx\\orc\\oready.wav".into()];
                    }
                    add(
                        cue,
                        !matches!(original, 4 | 5 | 40 | 41 | 45 | 55 | 56),
                        names,
                    )?;
                }
                let dead = match original {
                    26..=33 | 36..=39 => Some("Gamesfx\\ships\\shipsink.wav"),
                    4 | 5 | 14 | 15 | 35 | 40..=43 | 56 => Some("Gamesfx\\misc\\explode.wav"),
                    45 | 57 => None,
                    55 => Some("Gamesfx\\misc\\Skeleton Death.wav"),
                    _ if race == 0 => Some("Gamesfx\\human\\hdead.wav"),
                    _ => Some("Gamesfx\\orc\\odead.wav"),
                };
                if let Some(dead) = dead {
                    add(AudioCue::Death, false, vec![dead.into()])?;
                }
                if matches!(original, 2 | 3 | 16 | 17) {
                    add(AudioCue::Work, false, numbered("misc\\tree", 4))?;
                    add(
                        AudioCue::Complete,
                        true,
                        super::voices::paths(original, AudioCue::Complete),
                    )?;
                }
                if unit.garrison.is_some() {
                    for cue in [AudioCue::Load, AudioCue::Unload] {
                        add(cue, false, vec!["Gamesfx\\misc\\dock.wav".into()])?;
                    }
                }
            }
            if unit.weapon.is_some() {
                let weapon = match original {
                    2 | 3 | 16 | 17 => "peonatak",
                    4 | 5 => "catapult",
                    7 | 13 | 23 | 49 => "punch",
                    8 | 18 | 20 | 96 => "bowfire",
                    9 | 19 | 53 | 97 => "axe",
                    10 | 24 | 22 | 42 => "../spells/thunder",
                    11 | 21 | 51 => "../spells/touchdrk",
                    43 | 35 | 56 => "fireball",
                    26..=39 | 98 | 99 => "fireball",
                    55 => "fist",
                    _ => "sword1",
                };
                add(
                    AudioCue::Attack,
                    false,
                    if weapon == "sword1" {
                        numbered("misc\\sword", 3)
                    } else {
                        vec![format!(
                            "Gamesfx\\{}.wav",
                            match weapon {
                                "../spells/thunder" => "spells\\thunder".into(),
                                "../spells/touchdrk" => "spells\\touchdrk".into(),
                                _ => format!("misc\\{weapon}"),
                            }
                        )]
                    },
                )?;
            }
            if let Some(sound) = match original {
                8 | 18 | 20 | 96 => Some("bowhit"),
                4 | 5 | 30..=33 | 38 | 39 | 98 | 99 => Some("explode"),
                10 | 11 | 21 | 22 | 24 | 35 | 42 | 43 | 51 | 56 => Some("firehit"),
                _ => None,
            } {
                add(
                    AudioCue::Impact,
                    false,
                    vec![format!("Gamesfx\\misc\\{sound}.wav")],
                )?;
            }
        }
        for (cue, names) in [
            (AudioCue::Error, vec!["Gamesfx\\misc\\error.wav".into()]),
            (
                AudioCue::Capture,
                vec![
                    "Gamesfx\\misc\\hrescue.wav".into(),
                    "Gamesfx\\misc\\orescue.wav".into(),
                ],
            ),
        ] {
            let variants = recordings(&mut source.data, root, &names, &mut cache)?;
            if !variants.is_empty() {
                audio.push(AudioMapping {
                    cue,
                    unit_type: None,
                    voice: false,
                    variants,
                });
            }
        }
        let mut music = [Vec::new(), Vec::new()];
        let mut menu = Vec::new();
        if let Some(disc) = source.disc.as_mut() {
            for (race, tracks) in music.iter_mut().enumerate() {
                *tracks = recordings(
                    disc,
                    root,
                    &(1..=6)
                        .map(|n| {
                            format!("Music\\{}{n}.WAV", if race == 0 { "HUMAN" } else { "ORC" })
                        })
                        .collect::<Vec<_>>(),
                    &mut cache,
                )?;
            }
            menu = recordings(disc, root, &["Music\\OWARROOM.WAV".into()], &mut cache)?;
        }
        Ok(Self { audio, music, menu })
    }

    pub fn mission(
        &self,
        source: &mut Source,
        root: &Path,
        cache_root: &Path,
        campaign: (usize, usize, bool),
        objective: &str,
    ) -> Result<MediaManifest> {
        let (race, number, expansion) = campaign;
        let name = format!(
            "rez\\{}{}.tbl",
            if expansion {
                if race == 0 { "2xhum" } else { "2xorc" }
            } else if race == 0 {
                "human"
            } else {
                "orc"
            },
            number
        );
        let texts = stats::names(&source.data.read_file(&name, 1024 * 1024)?)?;
        let mut mission = MediaManifest {
            schema_version: 1,
            audio: self.audio.clone(),
            music: self.music[race].clone(),
            mission_audio: Vec::new(),
            mission_texts: vec![objective.into(), texts.join("\n")],
            briefing: vec![BriefingAction::Objectives { text: 0 }],
            portraits: Vec::new(),
        };
        mission.audio.retain(|a| a.cue != AudioCue::Capture);
        let mut cue_cache = BTreeMap::new();
        let mut add = |cue, unit_type, voice, names: Vec<String>| -> Result<()> {
            let variants = recordings(&mut source.data, cache_root, &names, &mut cue_cache)?;
            if !variants.is_empty() {
                mission.audio.push(AudioMapping {
                    cue,
                    unit_type,
                    voice,
                    variants,
                });
            }
            Ok(())
        };
        add(
            AudioCue::Capture,
            None,
            false,
            vec![format!(
                "Gamesfx\\misc\\{}rescue.wav",
                if race == 0 { "h" } else { "o" }
            )],
        )?;
        for spell in super::spells::definitions() {
            let name = match spell.id.0 {
                1 => "holyvisn",
                2 => "heal",
                3 => "exorcism",
                4 => "iokilrog",
                5 => "blodlust",
                6 => "touchdrk",
                7 => "fireball",
                8 => "slow",
                9 => "flamshld",
                10 => "invisibl",
                11 => "morph",
                12 => "icestorm",
                13 => "dethcoil",
                14 => continue,
                15 => "haste",
                16 => "unhlyarm",
                17 => "whrlwind",
                18 => "decay",
                19 => "explode",
                _ => unreachable!(),
            };
            add(
                AudioCue::Ability(spell.id),
                None,
                false,
                vec![format!(
                    "Gamesfx\\{}\\{name}.wav",
                    if matches!(spell.id.0, 7 | 19) {
                        "misc"
                    } else {
                        "spells"
                    }
                )],
            )?;
        }
        for research in super::technology::definitions()
            .into_iter()
            .map(|t| t.rule.id)
            .chain(
                super::spells::definitions()
                    .into_iter()
                    .filter_map(|s| s.research.map(|r| r.0)),
            )
        {
            add(
                AudioCue::ResearchComplete(research),
                None,
                true,
                vec![format!(
                    "Gamesfx\\{}\\{}wrkdone.wav",
                    if race == 0 { "human" } else { "orc" },
                    if race == 0 { "h" } else { "o" }
                )],
            )?;
        }
        let mut duration = 0;
        if let Some(disc) = source.disc.as_mut() {
            for part in 1..=8 {
                let file = format!(
                    "Speech\\{}{}{number}_{part}.wav",
                    if expansion { "war2x\\" } else { "" },
                    if race == 0 { "human" } else { "orc" }
                );
                if !disc.has_file(&file)? {
                    break;
                }
                let reference = publish(cache_root, &disc.read_file(&file, MAX_WAV_BYTES)?)?;
                let pcm = decode_wav(&fs::read(cache_root.join(&reference.file))?)?;
                let milliseconds = pcm.duration_ms() as u32;
                duration += milliseconds;
                let sound = mission.mission_audio.len() as u16;
                mission.mission_audio.push(reference);
                mission.briefing.push(BriefingAction::Sound { sound });
                mission.briefing.push(BriefingAction::Wait { milliseconds });
            }
        }
        mission.briefing.insert(
            1,
            BriefingAction::Text {
                text: 1,
                milliseconds: duration,
            },
        );
        for reference in mission
            .audio
            .iter()
            .flat_map(|a| &a.variants)
            .chain(&mission.music)
            .chain(&mission.mission_audio)
        {
            if !root.join(&reference.file).exists() {
                fs::hard_link(cache_root.join(&reference.file), root.join(&reference.file))?;
            }
        }
        mission.validate()?;
        Ok(mission)
    }
}

/// Convert source PCM8/PCM16 to mono PCM16, retaining its sample rate up to
/// 22050 Hz. Downsampling the music keeps the entire playlist within the native
/// resident audio budget without relying on an external media process.
fn normalize(bytes: &[u8]) -> Result<Vec<u8>> {
    ensure!(
        bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WAVE",
        "not a WAV recording"
    );
    let mut p = 12;
    let mut format = None;
    let mut data = None;
    while p + 8 <= bytes.len() {
        let length = u32::from_le_bytes(bytes[p + 4..p + 8].try_into()?) as usize;
        let chunk = bytes
            .get(p + 8..p + 8 + length)
            .context("truncated source WAV")?;
        if &bytes[p..p + 4] == b"fmt " {
            ensure!(chunk.len() >= 16, "truncated WAV format");
            let short = |i| u16::from_le_bytes([chunk[i], chunk[i + 1]]);
            let rate = u32::from_le_bytes(chunk[4..8].try_into()?);
            ensure!(
                short(0) == 1
                    && (1..=2).contains(&short(2))
                    && matches!(short(14), 8 | 16)
                    && (8000..=48000).contains(&rate),
                "unsupported source WAV format"
            );
            format = Some((short(2) as usize, rate, short(14) as usize));
        } else if &bytes[p..p + 4] == b"data" {
            data = Some(chunk);
        }
        p += 8 + length + (length & 1);
    }
    let (channels, rate, bits) = format.context("source WAV lacks format")?;
    let data = data.context("source WAV lacks samples")?;
    let stride = channels * bits / 8;
    ensure!(
        data.len().is_multiple_of(stride),
        "unaligned source WAV samples"
    );
    let step = if rate > 22050 { 2 } else { 1 };
    let mut samples = Vec::new();
    for frames in data.chunks(stride * step) {
        let frame = &frames[..stride];
        let mut sum = 0i32;
        for ch in 0..channels {
            sum += if bits == 8 {
                (i32::from(frame[ch]) - 128) * 256
            } else {
                i32::from(i16::from_le_bytes([frame[ch * 2], frame[ch * 2 + 1]]))
            };
        }
        samples.push((sum / channels as i32) as i16);
    }
    encode_wav(1, rate / step as u32, &samples)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires STRATERUST_WARCRAFT2 and STRATERUST_WARCRAFT2_SOURCE (retail installer or directory)"]
    fn native_construction_completion_and_fading_sites_match_original_assets() {
        use straterust_engine::assets::{AssetPack, ClipKind};
        let root = std::path::PathBuf::from(std::env::var_os("STRATERUST_WARCRAFT2").unwrap());
        let input =
            std::path::PathBuf::from(std::env::var_os("STRATERUST_WARCRAFT2_SOURCE").unwrap());
        let mut source = Source::open(&input, &|_| {}).unwrap();
        for (race, folder) in [(0, "human"), (1, "orc")] {
            let directory = root.join(folder).join("mission01");
            let media = MediaPack::load(&directory).unwrap().unwrap();
            for (cue, unit, recording) in [
                (
                    AudioCue::Transform,
                    58 + race,
                    "Gamesfx\\misc\\constrct.wav".to_owned(),
                ),
                (
                    AudioCue::SelectConstruction,
                    58 + race,
                    "Gamesfx\\misc\\constrct.wav".to_owned(),
                ),
                (
                    AudioCue::Complete,
                    2 + race,
                    super::super::voices::paths(2 + race, AudioCue::Complete)[0].clone(),
                ),
                (
                    AudioCue::Complete,
                    58 + race,
                    super::super::voices::paths(2 + race, AudioCue::Complete)[0].clone(),
                ),
            ] {
                let original = source.data.read_file(&recording, MAX_WAV_BYTES).unwrap();
                let expected = decode_wav(&normalize(&original).unwrap()).unwrap();
                let actual = &media
                    .audio
                    .iter()
                    .find(|a| a.cue == cue && a.unit_type == Some(stats::id(unit)))
                    .unwrap()
                    .variants[0];
                assert_eq!(actual.samples, expected.samples);
                assert_eq!(actual.sample_rate, expected.sample_rate);
            }
            let pud =
                super::super::pud::Pud::decode(&source.main.entry(192 + race).unwrap()).unwrap();
            let set = super::super::terrain::Tileset::load(&source.main, pud.era).unwrap();
            let assets = AssetPack::load(&directory).unwrap().unwrap();
            let blast_count =
                super::super::gfx::sprites(&source.main.entry(347).unwrap(), &set.palette)
                    .unwrap()
                    .len();
            for (unit, record) in [
                (58 + race, [189, 190, 188, 524][pud.era]),
                (74 + race, [121, 163, 191, 512][pud.era]),
            ] {
                let ruins =
                    super::super::gfx::sprites(&source.main.entry(record).unwrap(), &set.palette)
                        .unwrap();
                let sprite = assets.sprite(stats::id(unit)).unwrap();
                let death = sprite.clip(ClipKind::Death).unwrap();
                assert_eq!(death.frames.len(), blast_count + 45);
                assert!(death.loop_start.is_none());
                for step in [0, 22, 44] {
                    let original = &ruins[step * ruins.len() / 45];
                    let frame = &sprite.frames[usize::from(death.frames[blast_count + step].frame)];
                    let alpha = |rgba: &[u8]| {
                        rgba.as_chunks::<4>()
                            .0
                            .iter()
                            .map(|p| u64::from(p[3]))
                            .sum::<u64>()
                    };
                    let expected = original
                        .rgba
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .map(|p| u64::from(u32::from(p[3]) * (45 - step as u32) / 45))
                        .sum::<u64>();
                    assert_eq!(
                        alpha(&frame.rgba),
                        expected,
                        "native site fades at step {step}"
                    );
                }
                assert_eq!(45 * death.frame_ms, 2970);
            }
        }
    }
    #[test]
    fn source_pcm_normalization_is_bounded_and_preserves_signed_samples() {
        let wav = encode_wav(2, 44100, &[100, -100, 300, 500, 600, 800, 0, 0]).unwrap();
        let normalized = decode_wav(&normalize(&wav).unwrap()).unwrap();
        assert_eq!(normalized.sample_rate, 22050);
        assert_eq!(&*normalized.samples, &[0, 700]);
        assert!(normalize(&wav[..20]).is_err());
    }
}
