//! StarCraft supplies the control layout, artwork and audio; the engine only
//! interprets the common cloak configuration and authoritative cloak orders. Icon frame
//! facts: Wargus/stargus scripts/zerg/icons.lua (259 down, 260 up). No code copied.
use crate::{Archive, Files, MemberReport, add_image, formats, member, ron_bytes};
mod migration;
use anyhow::{Context, Result};
pub(crate) use migration::upgrade;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    io::{Read, Seek},
};
use straterust_engine::{
    assets::{AssetManifest, UiImageManifest},
    media::{AudioCue, AudioMapping, MediaManifest},
    sim::{Cloak, Rules},
};

#[derive(Serialize, Deserialize)]
struct Button {
    slot: u8,
    key: String,
    label: String,
    tip: String,
    icon: String,
}

#[derive(Deserialize)]
struct Controls {
    #[serde(default)]
    command_buttons: BTreeMap<String, Button>,
}

pub(crate) fn refresh<R: Read + Seek>(
    archive: &mut Archive<R>,
    files: &mut Files,
    assets: &mut AssetManifest,
    rules: &Rules,
    members: &mut Vec<MemberReport>,
) -> Result<()> {
    if !rules.units.iter().any(|unit| unit.cloak.is_some()) {
        return Ok(());
    }
    let colors = member(
        archive,
        "unit\\cmdbtns\\ticon.pcx",
        1024 * 1024,
        "stardat",
        "command icon colors",
        members,
    )?;
    let palette = crate::terran_ui::command_palette(&formats::decode_pcx(&colors)?)?;
    let grp = member(
        archive,
        "unit\\cmdbtns\\cmdicons.grp",
        1024 * 1024,
        "stardat",
        "burrow and unburrow command icons",
        members,
    )?;
    let icons = formats::decode_grp(&grp, &palette)?;
    crate::terran_ui::expected_frames(&icons, 365, 36, 34)?;
    let bytes = files
        .get("presentation.ron")
        .context("missing burrow presentation")?;
    let mut text = std::str::from_utf8(bytes)?.to_owned();
    let mut controls: Controls = ron::from_str(&text)?;
    let mut media: MediaManifest =
        ron::de::from_bytes(files.get("media.ron").context("missing burrow media")?)?;
    let mut pcm_bytes = 0;
    for unit in rules.units.iter().filter(|u| u.cloak.is_some()) {
        // The game decides which concealed ability has the underground form.
        let underground = matches!(unit.id.0, 6 | 7 | 27 | 64 | 108);
        for enabled in [true, false] {
            let (name, label, frame, key, slot, tip) = if underground {
                if enabled {
                    (
                        "burrow",
                        "Burrow",
                        259,
                        "U",
                        8,
                        "Hide underground until ordered to emerge.",
                    )
                } else {
                    (
                        "unburrow",
                        "Unburrow",
                        260,
                        "U",
                        8,
                        "Emerge to resume movement and attacks.",
                    )
                }
            } else if enabled {
                (
                    "cloak",
                    "Cloak",
                    252,
                    "C",
                    7,
                    "Conceal this unit. Costs 25 energy and drains energy while active.",
                )
            } else {
                // Retail uses C for both states. The user's chosen C/D split
                // is intentional and survives fresh imports and refreshes.
                ("decloak", "Decloak", 252, "D", 7, "Deactivate cloaking.")
            };
            let icon = format!("command.{name}");
            controls.command_buttons.insert(
                format!("cloak.{}.{}", unit.id.0, if enabled { "on" } else { "off" }),
                Button {
                    slot,
                    key: key.into(),
                    label: label.into(),
                    tip: tip.into(),
                    icon: icon.clone(),
                },
            );
            assets.ui.retain(|image| image.key != icon);
            assets.ui.push(UiImageManifest {
                key: icon,
                image: add_image(files, &format!("ui-command-{name}.srim"), &icons[frame])?,
            });
            if underground {
                let (cue, path) = if enabled {
                    (AudioCue::Conceal, "sound\\misc\\burrowdn.wav")
                } else {
                    (AudioCue::Reveal, "sound\\misc\\burrowup.wav")
                };
                let source = member(
                    archive,
                    path,
                    4 * 1024 * 1024,
                    "stardat",
                    "concealment transition audio",
                    members,
                )?;
                let bytes = crate::terran_media::normalize_wav(&source, 30_000, &mut pcm_bytes)?;
                let sound =
                    crate::terran_media::audio_file(files, format!("sound-{name}.wav"), bytes);
                media
                    .audio
                    .retain(|m| m.cue != cue || m.unit_type != Some(unit.id));
                media.audio.push(AudioMapping {
                    cue,
                    unit_type: Some(unit.id),
                    voice: false,
                    variants: vec![sound],
                });
            }
        }
    }
    crate::campaign_units::set_map(
        &mut text,
        "command_buttons",
        &ron::ser::to_string(&controls.command_buttons)?,
    )?;
    files.insert("presentation.ron".into(), text.into_bytes());
    media.validate()?;
    files.insert("media.ron".into(), ron_bytes(&media)?);
    Ok(())
}

/// Native game logic: Zerg burrowing uses the common concealment ability.
pub(crate) fn rules(reveal_ticks: u32) -> Cloak {
    Cloak {
        can_move: false,
        can_attack: false,
        blocks_movement: false,
        reveal_ticks,
        reveal_on_order: true,
        auto_reveal: true,
        ..Cloak::default()
    }
}
