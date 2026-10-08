//! Original spell command rows, artwork and sounds for all three races.
use super::*;
use serde::{Deserialize, Serialize};
mod rules;
pub(super) use rules::apply;

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

// ID, native tech (24 = command icon), slot, button string, label, image,
// impact image, unit overlay, order-triggered sound (0 = IScript sounds only).
type SpellRow = (u16, usize, u8, u32, &'static str, usize, usize, bool, u16);
const SPELLS: &[SpellRow] = &[
    (1, 2, 7, 341, "EMP Shockwave", 554, 555, false, 350),
    (2, 7, 8, 342, "Irradiate", 380, 380, true, 351),
    (3, 1, 7, 335, "Lockdown", 529, 361, false, 0),
    (4, 8, 6, 343, "Yamato Gun", 541, 544, false, 0),
    (5, 6, 6, 340, "Defensive Matrix", 371, 377, true, 349),
    (6, 24, 8, 685, "Nuclear Strike", 316, 318, false, 239),
    (7, 13, 7, 375, "Spawn Broodlings", 516, 516, false, 0),
    (8, 17, 8, 381, "Ensnare", 384, 383, true, 0),
    (9, 18, 6, 377, "Parasite", 516, 516, false, 0),
    (10, 12, 5, 374, "Infest Command Center", 101, 101, true, 0),
    (11, 14, 6, 376, "Dark Swarm", 337, 337, true, 0),
    (12, 15, 7, 378, "Plague", 388, 387, true, 0),
    (13, 16, 5, 379, "Consume", 517, 517, true, 0),
    (14, 19, 6, 400, "Psionic Storm", 525, 525, true, 0),
    (15, 20, 7, 401, "Hallucination", 545, 545, true, 0),
    (16, 21, 6, 402, "Recall", 391, 391, true, 0),
    (17, 22, 7, 403, "Stasis Field", 365, 364, true, 0),
    (18, 23, 8, 404, "Archon Warp", 134, 136, true, 0),
    (19, 24, 0, 673, "Recharge Shields", 368, 368, true, 0),
    (20, 24, 0, 629, "Build Exit", 0, 0, true, 19),
];

pub(super) fn refresh(
    archive: &mut SourceArchive,
    files: &mut Files,
    assets: &mut AssetManifest,
    rules: &Rules,
    graphics: &mut Graphics<'_>,
) -> Result<()> {
    if rules.units.iter().all(|u| u.abilities.is_empty()) {
        return Ok(());
    }
    let table = archive.read_file("rez\\stat_txt.tbl", 65536)?;
    let tech = archive.read_file("arr\\techdata.dat", 432)?;
    let colors = formats::decode_pcx(&archive.read_file("unit\\cmdbtns\\ticon.pcx", 1024 * 1024)?)?;
    let icons = formats::decode_grp(
        &archive.read_file("unit\\cmdbtns\\cmdicons.grp", 1024 * 1024)?,
        &crate::terran_ui::command_palette(&colors)?,
    )?;
    let sfx = archive.read_file("arr\\sfxdata.dat", 8712)?;
    let names = archive.read_file("arr\\sfxdata.tbl", 65536)?;
    let mut presentation = std::str::from_utf8(&files["presentation.ron"])?.to_owned();
    let mut controls: Controls = ron::from_str(&presentation)?;
    let mut media: MediaManifest = ron::de::from_bytes(&files["media.ron"])?;
    assets.projectiles.retain(|p| p.ability.is_none());
    media.audio.retain(|m| {
        !matches!(
            m.cue,
            AudioCue::Ability(_) | AudioCue::StrikeStage(..) | AudioCue::AbilityWarning(_)
        )
    });
    let mut cumulative = 0;
    for &(id, source, slot, string, label, image, impact_image, on_target, order_sound) in SPELLS {
        let ability = AbilityId(id);
        let casters: Vec<_> = rules
            .units
            .iter()
            .filter(|u| u.abilities.iter().any(|a| a.id == ability))
            .map(|u| u.id)
            .collect();
        let Some(&caster) = casters.first() else {
            continue;
        };
        let icon = if source < 24 {
            usize::from(word(&tech, 24 * 12 + source * 2))
        } else if id == 20 {
            134
        } else if id == 6 {
            311
        } else {
            308
        };
        let key = format!("ability.{id}");
        assets.ui.retain(|ui| ui.key != key);
        assets.ui.push(straterust_engine::assets::UiImageManifest {
            image: crate::add_image(
                files,
                &format!("ui-{key}.srim"),
                icons.get(icon).context("missing spell icon")?,
            )?,
            key: key.clone(),
        });
        controls.command_buttons.insert(
            key.clone(),
            Control {
                slot,
                key: crate::terran_media::table_string(&table, string)?
                    .chars()
                    .next()
                    .context("missing spell shortcut")?
                    .to_ascii_uppercase()
                    .to_string(),
                label: label.into(),
                tip: super::super::descriptions::ability(id).into(),
                icon: key,
            },
        );
        if id != 20 {
            let flight = graphics.effect(
                archive,
                files,
                image,
                0,
                if matches!(id, 4 | 6) { 17 } else { 1 },
            )?;
            let impact = graphics.effect(archive, files, impact_image, 0, 1)?;
            assets.projectiles.push(ProjectileManifest {
                ability: Some(ability),
                unit_type: caster,
                targets_air: false,
                directional: matches!(id, 4 | 6),
                speed_fp8: if matches!(id, 4 | 6) { 8533 } else { 256 * 12 },
                forward_offset: 0,
                launch_offsets: if id == 4 {
                    graphics.casting_offsets(archive, 12)?
                } else {
                    Vec::new()
                },
                arc_height: 0,
                on_target,
                charge: if id == 4 {
                    Some(graphics.effect(archive, files, 543, 0, 1)?)
                } else {
                    None
                },
                marker: if id == 6 {
                    Some(graphics.effect(archive, files, 233, 0, 1)?)
                } else {
                    None
                },
                flight,
                impact,
                trail: if id == 6 {
                    // Walking emits smoke every three ticks, ten pixels behind
                    // the engine. Its Init hides each puff for three ticks.
                    graphics.projectile_trail_animation(archive, files, image, 11)?
                } else if id == 4 {
                    graphics.projectile_trail(archive, files, image)?
                } else {
                    None
                },
            });
        }
        use straterust_engine::sim::StrikeStage;
        let stage_sounds: Vec<_> = match id {
            4 => vec![
                (AudioCue::StrikeStage(ability, StrikeStage::Charge), 178),
                (AudioCue::StrikeStage(ability, StrikeStage::Flight), 179),
            ],
            6 => vec![
                (AudioCue::StrikeStage(ability, StrikeStage::Ascent), 84),
                (AudioCue::AbilityWarning(ability), 127),
                (AudioCue::StrikeStage(ability, StrikeStage::Impact), 85),
            ],
            _ => Vec::new(),
        };
        for (cue, sound) in stage_sounds {
            let path = terran_media::sound_path(&sfx, &names, sound)?;
            let bytes = terran_media::normalize_wav(
                &archive.read_file(&path, 4 * 1024 * 1024)?,
                120000,
                &mut cumulative,
            )?;
            media.audio.push(AudioMapping {
                cue,
                unit_type: None,
                voice: false,
                variants: vec![terran_media::audio_file(
                    files,
                    format!("sound-{sound:03}.wav"),
                    bytes,
                )],
            });
        }
        let mut sound_ids = BTreeSet::new();
        if !matches!(id, 4 | 6 | 20) {
            sound_ids.extend(sounds(
                &graphics.tables.scripts,
                graphics.tables.script(image),
                0,
            ));
            sound_ids.extend(sounds(
                &graphics.tables.scripts,
                graphics.tables.script(impact_image),
                0,
            ));
        }
        if order_sound > 0 {
            sound_ids.insert(order_sound);
        }
        // Unit CastSpell scripts also own several sounds (Queen, Templar).
        for &caster in casters.iter().filter(|_| !matches!(id, 4 | 6)) {
            if let Some(&(source, _)) = MAPPING
                .iter()
                .find(|(_, native)| UnitTypeId(*native) == caster)
            {
                sound_ids.extend(sounds(
                    &graphics.tables.scripts,
                    graphics.tables.script(graphics.tables.image(source)),
                    7,
                ));
            }
        }
        let mut variants = Vec::new();
        for sound in sound_ids {
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
        for caster in casters {
            if !variants.is_empty() {
                media.audio.push(AudioMapping {
                    cue: AudioCue::Ability(ability),
                    unit_type: Some(caster),
                    voice: false,
                    variants: variants.clone(),
                });
            }
        }
    }
    set_map(
        &mut presentation,
        "command_buttons",
        &ron::ser::to_string(&controls.command_buttons)?,
    )?;
    files.insert("presentation.ron".into(), presentation.into_bytes());
    files.insert("media.ron".into(), ron_bytes(&media)?);
    Ok(())
}
