//! Advisor recordings follow the listener's race and the original tech/upgrade
//! catalog. They are presentation mappings, never simulation classifications.
use super::*;

pub(crate) fn refresh(
    archive: &mut Archive<std::io::Cursor<Vec<u8>>>,
    files: &mut Files,
    rules: &Rules,
    race: u8,
) -> Result<()> {
    ensure!(race <= 2, "unsupported advisor race");
    let mut media: MediaManifest = ron::de::from_bytes(&files["media.ron"])?;
    let data = archive.read_file("arr\\sfxdata.dat", 8712)?;
    let names = archive.read_file("arr\\sfxdata.tbl", 65536)?;
    let mut recordings = BTreeMap::new();
    for base in [117, 120, 127] {
        let sound = base + u16::from(race);
        let path = terran_media::sound_path(&data, &names, sound)?;
        let bytes = terran_media::normalize_wav(
            &archive.read_file(&path, 4 * 1024 * 1024)?,
            30000,
            &mut 0,
        )?;
        recordings.insert(
            base,
            terran_media::audio_file(files, format!("sound-{sound:03}.wav"), bytes),
        );
    }
    media
        .audio
        .retain(|m| !matches!(m.cue, AudioCue::ResearchComplete(_)));
    for &(base, technology, _) in faction_research::SOURCES {
        for level in 1..=3 {
            let id = faction_research::level_id(base, level);
            if !rules.research.iter().any(|r| r.id == id) {
                continue;
            }
            media.audio.push(AudioMapping {
                cue: AudioCue::ResearchComplete(id),
                unit_type: None,
                voice: true,
                variants: vec![recordings[&if technology { 117 } else { 120 }].clone()],
            });
        }
    }
    for warning in media
        .audio
        .iter_mut()
        .filter(|m| m.cue == AudioCue::AbilityWarning(AbilityId(6)))
    {
        warning.variants = vec![recordings[&127].clone()];
    }
    files.insert("media.ron".into(), ron_bytes(&media)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires STRATERUST_SOURCE pointing to the owner's retail disc"]
    fn advisor_research_upgrade_and_warning_recordings_follow_the_listener_race() -> Result<()> {
        let source_path = std::env::var_os("STRATERUST_SOURCE").context("set STRATERUST_SOURCE")?;
        let source = crate::Source::open(std::path::Path::new(&source_path))?;
        let mut installer = Archive::open_region(&source.path, source.offset, source.len)?;
        let mut archive =
            Archive::from_bytes(installer.read_file("files\\stardat.mpq", 128 * 1024 * 1024)?)?;
        let mut rules = Rules::default();
        for id in [21, 24, 224, 424] {
            rules.research.push(Research {
                id: ResearchId(id),
                facility: UnitTypeId(1),
                previous: None,
                prerequisites: vec![],
                cost: vec![],
                ticks: 3,
                effect: ResearchEffect::Mode {
                    units: vec![UnitTypeId(1)],
                },
            });
        }
        let template = MediaManifest {
            schema_version: 1,
            audio: vec![AudioMapping {
                cue: AudioCue::AbilityWarning(AbilityId(6)),
                unit_type: None,
                voice: false,
                variants: vec![straterust_engine::media::AudioRef {
                    file: "wrong.wav".into(),
                    blake3: "0".repeat(64),
                }],
            }],
            music: vec![],
            mission_audio: vec![],
            mission_texts: vec![],
            briefing: vec![],
            portraits: vec![],
        };
        for race in 0..=2 {
            let mut files = Files::new();
            files.insert("media.ron".into(), ron_bytes(&template)?);
            for _ in 0..2 {
                refresh(&mut archive, &mut files, &rules, race)?;
            }
            let media: MediaManifest = ron::de::from_bytes(&files["media.ron"])?;
            media.validate()?;
            assert_eq!(media.audio.len(), 5, "refresh is idempotent");
            for (cue, base) in [
                (AudioCue::ResearchComplete(ResearchId(21)), 117),
                (AudioCue::ResearchComplete(ResearchId(24)), 120),
                (AudioCue::ResearchComplete(ResearchId(224)), 120),
                (AudioCue::ResearchComplete(ResearchId(424)), 120),
                (AudioCue::AbilityWarning(AbilityId(6)), 127),
            ] {
                let mapping = media.audio.iter().find(|m| m.cue == cue).unwrap();
                assert_eq!(
                    mapping.variants[0].file,
                    format!("sound-{:03}.wav", base + u16::from(race))
                );
                assert!(mapping.unit_type.is_none());
                let bytes = &files[&mapping.variants[0].file];
                assert_eq!(
                    mapping.variants[0].blake3,
                    blake3::hash(bytes).to_hex().to_string()
                );
                assert!(
                    !straterust_engine::media::decode_wav(bytes)?
                        .samples
                        .is_empty()
                );
            }
        }
        Ok(())
    }
}
