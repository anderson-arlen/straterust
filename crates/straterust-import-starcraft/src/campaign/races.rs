//! Source campaign identity, tileset and racial presentation mappings.
use super::*;
use straterust_engine::media::{AudioCue, AudioMapping};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Race {
    Terran,
    Zerg,
    Protoss,
}
impl Race {
    pub const ALL: [Self; 3] = [Self::Terran, Self::Zerg, Self::Protoss];
    pub fn parse(value: &str) -> Result<Option<Self>> {
        match value {
            "all" => Ok(None),
            "terran" => Ok(Some(Self::Terran)),
            "zerg" => Ok(Some(Self::Zerg)),
            "protoss" => Ok(Some(Self::Protoss)),
            _ => bail!("--race must be all, terran, zerg or protoss"),
        }
    }
    pub fn folder(self) -> &'static str {
        match self {
            Self::Terran => "terran",
            Self::Zerg => "zerg",
            Self::Protoss => "protoss",
        }
    }
    pub fn titles(self) -> [&'static str; 10] {
        match self {
            Self::Terran => TITLES,
            Self::Zerg => [
                "Among the Ruins",
                "Egression",
                "The New Dominion",
                "Agent of the Swarm",
                "The Amerigo",
                "The Dark Templar",
                "The Culling",
                "Eye for an Eye",
                "The Invasion of Aiur",
                "Full Circle",
            ],
            Self::Protoss => [
                "First Strike",
                "Into the Flames",
                "Higher Ground",
                "The Hunt for Tassadar",
                "Choosing Sides",
                "Into the Darkness",
                "Homeland",
                "The Trial of Tassadar",
                "Shadow Hunters",
                "Eye of the Storm",
            ],
        }
    }
    // Retail terran07 is the cut Biting the Bullet scenario.
    pub fn source_number(self, number: u8) -> u8 {
        if self == Self::Terran && number >= 7 {
            number + 1
        } else {
            number
        }
    }
    pub fn prefix(self) -> &'static str {
        match self {
            Self::Terran => "t",
            Self::Zerg => "z",
            Self::Protoss => "p",
        }
    }
}

pub(super) const ROSTER: &[u16] = &[
    1, 2, 3, 5, 8, 11, 12, 15, 16, 20, 23, 29, 30, 35, 36, 40, 41, 42, 43, 44, 37, 38, 39, 45, 47,
    50, 51, 53, 59, 64, 65, 66, 67, 68, 69, 70, 77, 79, 83, 84, 87, 89, 90, 95, 113, 114, 115, 120,
    123, 124, 131, 132, 133, 135, 137, 138, 139, 140, 141, 142, 144, 146, 147, 148, 149, 150, 151,
    152, 154, 155, 156, 157, 160, 162, 163, 164, 165, 166, 167, 171, 172, 194, 195, 196, 203, 205,
    206, 207, 208, 209, 211, 212, 213, 216, 218, 9, 25, 28, 46, 71, 72, 74, 75, 78, 82, 108, 116,
    117, 118, 126, 134, 136, 159, 168, 169, 170, 173, 174, 210, 217, 219, 14, 73, 85,
];
pub(super) fn tileset(id: u16) -> Result<&'static str> {
    match id & 7 {
        0 => Ok("badlands"),
        1 => Ok("platform"),
        2 => Ok("install"),
        3 => Ok("ashworld"),
        4 => Ok("jungle"),
        _ => bail!("unsupported retail campaign tileset {id}"),
    }
}

pub(super) fn presentation<I: std::io::Read + std::io::Seek>(
    installer: &mut Archive<I>,
    archive: &mut Archive<std::io::Cursor<Vec<u8>>>,
    files: &mut Files,
    race: Race,
) -> Result<()> {
    let mut assets: AssetManifest = ron::de::from_bytes(&files["assets.ron"])?;
    // Retain imported research/ability commands when replacing the console.
    assets.ui.retain(|entry| {
        entry.key.starts_with("research.")
            || entry.key.starts_with("ability.")
            || entry.key.starts_with("mode.")
    });
    crate::terran_ui::convert_race(archive, files, &mut assets, &mut Vec::new(), race.prefix())?;
    campaign_units::add_ui_race(
        archive,
        files,
        &mut assets,
        &MAPPING.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
        race.prefix(),
    )?;
    let rules: Rules = ron::de::from_bytes(&files["rules.ron"])?;
    campaign_units::refresh_wireframes(archive, files, &mut assets, &rules)?;
    crate::burrow::refresh(archive, files, &mut assets, &rules, &mut Vec::new())?;
    files.insert("assets.ron".into(), ron_bytes(&assets)?);
    let mut presentation = std::str::from_utf8(&files["presentation.ron"])?.to_owned();
    campaign_units::set_map(&mut presentation, "supply_divisor", "2")?;
    let slots: BTreeMap<_, _> = [
        (41, 0),
        (37, 1),
        (42, 2),
        (38, 3),
        (43, 4),
        (47, 5),
        (45, 6),
        (39, 7),
    ]
    .into_iter()
    .filter_map(|(source, slot)| campaign_units::native_id(source).map(|id| (id, slot)))
    .collect();
    campaign_units::set_map(
        &mut presentation,
        "train_slots",
        &ron::ser::to_string(&slots)?,
    )?;
    files.insert("presentation.ron".into(), presentation.into_bytes());
    let mut media: MediaManifest = ron::de::from_bytes(&files["media.ron"])?;
    let sfx = archive.read_file("arr\\sfxdata.dat", 8712)?;
    let names = archive.read_file("arr\\sfxdata.tbl", 65536)?;
    for (cue, sound) in [
        (AudioCue::Error, if race == Race::Zerg { 1 } else { 3 }),
        (AudioCue::Capture, if race == Race::Zerg { 32 } else { 34 }),
    ] {
        let path = crate::terran_media::sound_path(&sfx, &names, sound)?;
        let wav = crate::terran_media::normalize_wav(
            &archive.read_file(&path, 8 * 1024 * 1024)?,
            30000,
            &mut 0,
        )?;
        let audio = crate::terran_media::audio_file(
            files,
            format!("race-{}-{sound}.wav", race.folder()),
            wav,
        );
        media
            .audio
            .retain(|mapping| !(mapping.cue == cue && mapping.unit_type.is_none()));
        media.audio.push(AudioMapping {
            cue,
            unit_type: None,
            voice: false,
            variants: vec![audio],
        });
    }
    if race != Race::Terran {
        let path = crate::terran_media::sound_path(
            &sfx,
            &names,
            if race == Race::Zerg { 135 } else { 529 },
        )?;
        let wav = crate::terran_media::normalize_wav(
            &archive.read_file(&path, 8 * 1024 * 1024)?,
            30000,
            &mut 0,
        )?;
        let audio = crate::terran_media::audio_file(
            files,
            format!("building-complete-{}.wav", race.folder()),
            wav,
        );
        media
            .audio
            .retain(|m| !(m.cue == AudioCue::Complete && m.unit_type.is_none()));
        media.audio.push(AudioMapping {
            cue: AudioCue::Complete,
            unit_type: None,
            voice: false,
            variants: vec![audio],
        });
    }
    for (source, sound) in [(42, 40), (69, 42)] {
        let id = campaign_units::native_id(source).unwrap();
        let path = crate::terran_media::sound_path(&sfx, &names, sound)?;
        let wav = crate::terran_media::normalize_wav(
            &archive.read_file(&path, 8 * 1024 * 1024)?,
            30000,
            &mut 0,
        )?;
        let audio = crate::terran_media::audio_file(files, format!("transport-{source}.wav"), wav);
        for cue in [AudioCue::Load, AudioCue::Unload] {
            media
                .audio
                .retain(|mapping| !(mapping.cue == cue && mapping.unit_type == Some(id)));
            media.audio.push(AudioMapping {
                cue,
                unit_type: Some(id),
                voice: false,
                variants: vec![audio.clone()],
            });
        }
    }
    media.music.clear();
    files.retain(|name, _| !name.starts_with("music-terran-"));
    for number in 1..=3 {
        let path = format!("music\\{}{number}.wav", race.folder());
        let bytes = crate::terran_media::normalize_wav(
            &installer.read_file(&path, 32 * 1024 * 1024)?,
            600_000,
            &mut 0,
        )?;
        media.music.push(crate::terran_media::audio_file(
            files,
            format!("music-{}-{number}.wav", race.folder()),
            bytes,
        ));
    }
    files.insert("media.ron".into(), ron_bytes(&media)?);
    Ok(())
}
