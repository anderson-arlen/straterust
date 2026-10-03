//! Additional roles exercised by the first five original Terran missions.
use crate::{Archive, Files, formats, ron_bytes, terran, terran_media};
use anyhow::{Context, Result, ensure};
use std::collections::{BTreeMap, BTreeSet};
use straterust_engine::{
    assets::{
        AssetManifest, ClipFrame, ClipKind, DamageEffectsManifest, EffectManifest, EffectSpot,
        GasEffectsManifest, Image, ResourceManifest, SpriteClip, UnitEffectManifest,
    },
    media::{AudioCue, AudioMapping, MediaManifest, PortraitManifest},
    sim::*,
};

pub const MAPPING: &[(u16, u16)] = &[
    (0, 1),
    (7, 2),
    (106, 3),
    (109, 4),
    (111, 5),
    (37, 6),
    (38, 7),
    (130, 8),
    (143, 9),
    (19, 10),
    (32, 11),
    (112, 12),
    (125, 13),
    (110, 14),
    (122, 15),
    (107, 16),
    (101, 17),
    (13, 18),
    (1, 19),
    (2, 20),
    (3, 21),
    (5, 22),
    (8, 23),
    (15, 24),
    (16, 25),
    (20, 26),
    (41, 27),
    (42, 28),
    (43, 29),
    (89, 30),
    (95, 31),
    (113, 32),
    (114, 33),
    (115, 34),
    (120, 35),
    (124, 36),
    (131, 37),
    (135, 38),
    (141, 39),
    (142, 40),
    (146, 41),
    (149, 42),
    (195, 43),
    (203, 44),
    (205, 45),
    (206, 46),
    (207, 47),
    (208, 48),
    (209, 49),
    (211, 50),
    (212, 51),
    (218, 52),
    (11, 53),
];
pub fn native_id(source: u16) -> Option<UnitTypeId> {
    MAPPING
        .iter()
        .find(|(id, _)| *id == source)
        .map(|(_, id)| UnitTypeId(*id))
}
fn word(b: &[u8], p: usize) -> u16 {
    u16::from_le_bytes(b[p..p + 2].try_into().unwrap())
}
fn dword(b: &[u8], p: usize) -> u32 {
    u32::from_le_bytes(b[p..p + 4].try_into().unwrap())
}

pub fn convert(
    archive: &mut Archive<std::io::Cursor<Vec<u8>>>,
    files: &mut Files,
    selected: &[u16],
) -> Result<()> {
    let units = archive.read_file("arr\\units.dat", 19192)?;
    let weapons = archive.read_file("arr\\weapons.dat", 4200)?;
    let flingy = archive.read_file("arr\\flingy.dat", 2760)?;
    let sprites = archive.read_file("arr\\sprites.dat", 2081)?;
    let images = archive.read_file("arr\\images.dat", 28690)?;
    let table = archive.read_file("arr\\images.tbl", 65536)?;
    let names = archive.read_file("rez\\stat_txt.tbl", 65536)?;
    let scripts = archive.read_file("scripts\\iscript.bin", 65536)?;
    let palette = formats::palette(&archive.read_file("tileset\\badlands.wpe", 1024)?)?;
    ensure!(
        units.len() == 19192
            && weapons.len() == 4200
            && flingy.len() == 2760
            && sprites.len() == 2081
            && images.len() == 28690,
        "unsupported campaign DAT layout"
    );
    let mut rules: Rules = ron::de::from_bytes(&files["rules.ron"])?;
    let mut assets: AssetManifest = ron::de::from_bytes(&files["assets.ron"])?;
    let mut media: MediaManifest = ron::de::from_bytes(&files["media.ron"])?;
    let mut portrait_cache = BTreeMap::new();
    for &source in selected {
        let id = native_id(source).context("unknown campaign role")?;
        if rules.units.iter().any(|u| u.id == id) {
            continue;
        }
        let n = usize::from(source);
        let structure = dword(&units, 0x19b0 + n * 4) & 1 != 0;
        let flying = matches!(source, 8 | 11 | 42 | 43);
        let ext = (0..4)
            .map(|side| word(&units, 0x2f5c + n * 8 + side * 2))
            .collect::<Vec<_>>();
        let footprint = Footprint {
            width: ext[0] + ext[2] + 1,
            height: ext[1] + ext[3] + 1,
        };
        let placement = Footprint {
            width: word(&units, 0x2a4c + n * 4).max(1),
            height: word(&units, 0x2a4c + n * 4 + 2).max(1),
        };
        // Retail v1.00 predates the two max-hit arrays in later units.dat.
        // Goliath/Tank weapons belong to their attached subunit.
        let weapon_unit = if matches!(source, 3 | 5) {
            usize::from(word(&units, 228 + n * 2))
        } else {
            n
        };
        ensure!(weapon_unit < 228, "invalid campaign weapon subunit");
        let ground = units[0x1704 + weapon_unit];
        let air = units[0x17e8 + weapon_unit];
        let weapon_id = if ground < 100 { ground } else { air };
        let weapon = if weapon_id < 100 {
            let w = usize::from(weapon_id);
            let splash = [0x898, 0x960, 0xa28].map(|p| u32::from(word(&weapons, p + w * 2)));
            Some(Weapon {
                damage: u32::from(word(&weapons, 0xaf0 + w * 2))
                    * u32::from(weapons[0xce4 + w].max(1)),
                range: dword(&weapons, 0x514 + w * 4),
                cooldown: u32::from(weapons[0xc80 + w].max(1)),
                targets_air: air < 100,
                cooldown_jitter: Some([-1, 2]),
                damage_kind: match weapons[0x708 + w] {
                    1 => DamageKind::Explosive,
                    2 => DamageKind::Concussive,
                    _ => DamageKind::Normal,
                },
                splash: (splash[0] > 0).then_some(splash),
                strikes: Vec::new(),
            })
        } else {
            None
        };
        let f = usize::from(units[n]);
        ensure!(f < 184, "invalid campaign flingy");
        let speed = dword(&flingy, 368 + f * 4);
        let mobile = !structure && !matches!(source, 195 | 203..=218);
        let mut unit = UnitType {
            id,
            blocks_movement: !matches!(source, 195 | 218),
            phases_while_gathering: source == 41,
            structure,
            footprint,
            placement,
            max_hp: (dword(&units, 0xc54 + n * 4) / 256).max(1),
            armor: u32::from(units[0x20d0 + n]),
            size: match units[0x1fec + n] {
                1 => UnitSize::Small,
                2 => UnitSize::Medium,
                _ => UnitSize::Large,
            },
            speed: if mobile {
                if speed > 1 {
                    speed.div_ceil(256).min(1024) as i32
                } else {
                    4
                }
            } else {
                0
            },
            movement_class: if flying {
                MovementClass::Air
            } else {
                MovementClass::Ground
            },
            vision_range: u32::from(units[0x1e24 + n]) * 32,
            build_ticks: u32::from(word(&units, 0x3bd4 + n * 2)).max(1),
            supply_used: u32::from(units[0x412c + n]).div_ceil(2),
            supply_provided: u32::from(units[0x4048 + n]) / 2,
            attacks_ground: ground < 100,
            weapon,
            ..UnitType::default()
        };
        if mobile && speed > 1 {
            unit.motion = Some(Motion {
                speed,
                acceleration: u32::from(word(&flingy, 1104 + f * 2)),
                steps: Vec::new(),
            });
        }
        if matches!(source, 41 | 42 | 43 | 131 | 135 | 141 | 142 | 146 | 149) {
            unit.regeneration = 4;
        }
        if source == 41 {
            unit.worker = Some(WorkerStats {
                capacity: 8,
                harvest_amount: 8,
                harvest_ticks: 75,
                build_rate: 1,
                resource_kinds: vec!["minerals".into(), "gas".into()],
            });
        }
        if matches!(source, 131 | 135 | 141 | 142 | 146 | 149) {
            unit.consumes_builder = true;
        }
        if let Some(w) = &unit.weapon {
            unit.acquisition_range = Some((u32::from(units[0x1d40 + n]) * 32).max(w.range));
        }
        for (kind, amount) in [
            ("minerals", word(&units, 0x3844 + n * 2)),
            ("gas", word(&units, 0x3a0c + n * 2)),
        ] {
            if amount != 0 {
                unit.cost.push(ResourceAmount {
                    kind: kind.into(),
                    amount: u32::from(amount),
                });
            }
        }
        rules.units.push(unit);
        let sprite = usize::from(word(&flingy, f * 2));
        ensure!(sprite < 517, "invalid campaign sprite");
        let image = usize::from(word(&sprites, sprite * 2));
        ensure!(image < 755, "invalid campaign image");
        let path = format!(
            "unit\\{}",
            terran_media::table_string(&table, dword(&images, image * 4))?
        );
        let mut frames =
            formats::decode_grp(&archive.read_file(&path, 8 * 1024 * 1024)?, &palette)?;
        if matches!(source, 3 | 5) {
            let tf = usize::from(units[weapon_unit]);
            let ts = usize::from(word(&flingy, tf * 2));
            let ti = usize::from(word(&sprites, ts * 2));
            let tp = format!(
                "unit\\{}",
                terran_media::table_string(&table, dword(&images, ti * 4))?
            );
            let turret = formats::decode_grp(&archive.read_file(&tp, 8 * 1024 * 1024)?, &palette)?;
            ensure!(turret.len() >= 17, "missing turret directions");
            for (frame, body) in frames.iter_mut().enumerate() {
                *body = terran::composite(body, &turret[frame % 17])?;
            }
        }
        let script = dword(&images, 755 * 10 + image * 4) as u16;
        let name = terran_media::table_string(&names, u32::from(source) + 1)?.to_owned();
        let directions = images[755 * 4 + image] != 0;
        let mut kept = Vec::<Image>::new();
        let mut clips = Vec::new();
        let mut cache = BTreeMap::new();
        for (kind, animation) in [
            (ClipKind::Idle, 0),
            (ClipKind::Walk, 11),
            (ClipKind::Attack, 5),
            (ClipKind::Construction, 15),
            (ClipKind::Death, 1),
            (ClipKind::Disabled, 24),
        ] {
            let mut poses = pose_frames(&scripts, script, animation);
            if poses.is_empty() {
                if matches!(kind, ClipKind::Idle | ClipKind::Construction) {
                    poses.push(0);
                } else {
                    continue;
                }
            }
            poses.retain(|p| usize::from(*p) + if directions { 16 } else { 0 } < frames.len());
            if poses.is_empty() {
                continue;
            }
            let mut native = Vec::new();
            for pose in poses {
                let start = kept.len() as u16;
                let first = *cache.entry(pose).or_insert_with(|| {
                    kept.extend(
                        frames[usize::from(pose)
                            ..usize::from(pose) + if directions { 17 } else { 1 }]
                            .iter()
                            .cloned(),
                    );
                    start
                });
                native.push(first);
            }
            clips.push(if directions {
                terran::directional(kind, &native, 42)
            } else {
                terran::single_direction(kind, &native, 42)
            });
        }
        let slug = format!("campaign-unit-{source:03}");
        assets.extra_units.push(terran::compact_sprite(
            files, id.0, &name, &slug, &kept, clips,
        )?);
        add_media(
            archive,
            files,
            &units,
            source,
            id,
            &mut media,
            &mut portrait_cache,
        )?;
    }
    // Only the production paths needed by these missions, in ordinary native rules.
    let known: BTreeSet<_> = rules.units.iter().map(|u| u.id).collect();
    let ids = |sources: &[u16]| {
        sources
            .iter()
            .filter_map(|s| native_id(*s))
            .filter(|id| known.contains(id))
            .collect::<Vec<_>>()
    };
    for unit in &mut rules.units {
        match unit.id.0 {
            2 => {
                unit.builds.extend(ids(&[107, 113, 114, 124]));
                unit.repairs
                    .extend(ids(&[2, 3, 5, 8, 113, 114, 115, 120, 124]));
            }
            20 => {
                unit.prerequisites = ids(&[113]);
                unit.mine_layer = rules_mines();
            }
            21 => unit.prerequisites = ids(&[113, 124]),
            22 => unit.prerequisites = ids(&[113, 120]),
            23 => unit.prerequisites = ids(&[114]),
            32 => {
                unit.trains = ids(&[2, 3, 5]);
                unit.builds = ids(&[120]);
                unit.prerequisites = ids(&[111]);
            }
            33 => {
                unit.trains = ids(&[8]);
                unit.builds = ids(&[115]);
                unit.prerequisites = ids(&[113]);
            }
            34 => unit.addon_parent = native_id(114),
            35 => unit.addon_parent = native_id(113),
            36 => unit.prerequisites = ids(&[113]),
            6 if known.contains(&UnitTypeId(37)) => {
                unit.supply_used = 1;
                unit.prerequisites = ids(&[142]);
            }
            7 if known.contains(&UnitTypeId(37)) => {
                unit.supply_used = 1;
                unit.prerequisites = ids(&[135]);
            }
            9 if known.contains(&UnitTypeId(37)) => unit.consumes_builder = true,
            29 => unit.prerequisites = ids(&[141]),
            27 => unit.builds = ids(&[131, 135, 141, 142, 143, 146, 149]),
            37 => {
                unit.trains = ids(&[41, 42, 37, 38, 43]);
                unit.dropoff = vec!["minerals".into(), "gas".into()];
            }
            38 | 40 => unit.prerequisites = ids(&[131]),
            39 => unit.prerequisites = ids(&[131]),
            42 => {
                unit.extracts = Some(Extraction {
                    resource: "gas".into(),
                    harvest_ticks: 37,
                    depleted_amount: 2,
                })
            }
            _ => {}
        }
    }
    // Incremental roster updates also revisit existing production paths.
    // Extending them must not introduce duplicate references in native rules.
    for unit in &mut rules.units {
        for ids in [
            &mut unit.builds,
            &mut unit.trains,
            &mut unit.repairs,
            &mut unit.prerequisites,
        ] {
            let mut seen = BTreeSet::new();
            ids.retain(|id| seen.insert(*id));
        }
    }
    add_ui(archive, files, &mut assets, selected)?;
    crate::mission_terran::add_grenade_projectiles(
        archive,
        &mut Vec::new(),
        files,
        &mut assets,
        &rules,
    )?;
    apply_creep_rules(&mut rules);
    apply_combat_rules(archive, &mut rules)?;
    apply_transport_rules(archive, &mut rules)?;
    files.insert("media.ron".into(), ron_bytes(&media)?);
    refresh_effects(archive, files, &mut assets, &rules)?;
    refresh_combat(archive, files, &mut assets, &rules)?;
    refresh_buildings(archive, files, &mut assets, &rules)?;
    refresh_wireframes(archive, files, &mut assets, &rules)?;
    refresh_indicators(archive, files, &mut assets, &rules)?;
    files.insert("rules.ron".into(), ron_bytes(&rules)?);
    files.insert("assets.ron".into(), ron_bytes(&assets)?);
    Ok(())
}
pub(super) fn apply_creep_rules(rules: &mut Rules) {
    for &(source, native) in MAPPING {
        let Some(unit) = rules
            .units
            .iter_mut()
            .find(|unit| unit.id == UnitTypeId(native))
        else {
            continue;
        };
        unit.creep_radius = match source {
            131 | 135 | 143 | 146 => Some([320, 200]),
            141 | 142 | 149 => Some([0, 0]),
            _ => None,
        };
        unit.requires_creep = matches!(source, 135 | 141 | 142 | 143 | 146);
    }
}

pub(super) fn refresh_bunker_audio(
    archive: &mut Archive<std::io::Cursor<Vec<u8>>>,
    files: &mut Files,
    rules: &Rules,
) -> Result<()> {
    let containers: Vec<_> = rules
        .units
        .iter()
        .filter(|unit| matches!(unit.id.0, 13 | 53) && unit.garrison.is_some())
        .map(|unit| unit.id)
        .collect();
    if containers.is_empty() {
        return Ok(());
    }
    let mut media: MediaManifest = ron::de::from_bytes(&files["media.ron"])?;
    let sounds = archive.read_file("arr\\sfxdata.dat", 8712)?;
    let table = archive.read_file("arr\\sfxdata.tbl", 65536)?;
    let mut cumulative = 0;
    for (cue, id, name) in [
        (AudioCue::Load, 41, "bunker-load.wav"),
        (AudioCue::Unload, 44, "bunker-unload.wav"),
    ] {
        let path = terran_media::sound_path(&sounds, &table, id)?;
        let bytes = terran_media::normalize_wav(
            &archive.read_file(&path, 8 * 1024 * 1024)?,
            10000,
            &mut cumulative,
        )?;
        let reference = terran_media::audio_file(files, name.into(), bytes);
        for &container in &containers {
            media
                .audio
                .retain(|mapping| !(mapping.cue == cue && mapping.unit_type == Some(container)));
            media.audio.push(AudioMapping {
                cue,
                unit_type: Some(container),
                voice: false,
                variants: vec![reference.clone()],
            });
        }
    }
    files.insert("media.ron".into(), ron_bytes(&media)?);
    files.insert("bunker-audio-reference.ron".into(), ron_bytes(&(
        "Retail sfxdata41 Misc/TDrTra00.wav and44 Misc/TDrTra01.wav. OpenBW unit_load_target selects40+race and unit_unload_impl43+race; Terran race1. Emit on successful visible state transitions, not attempted orders or container destruction."))?);
    Ok(())
}

fn damage_spots(bytes: &[u8], pose: usize) -> Result<Vec<EffectSpot>> {
    ensure!(bytes.len() >= 12, "truncated damage LO header");
    let frames = dword(bytes, 0) as usize;
    let count = dword(bytes, 4) as usize;
    ensure!(
        (1..=4096).contains(&frames) && (1..=24).contains(&count) && bytes.len() >= 8 + frames * 4,
        "invalid damage LO dimensions"
    );
    let offset = dword(bytes, 8 + pose.min(frames - 1) * 4) as usize;
    let data = bytes
        .get(offset..offset.saturating_add(count * 2))
        .context("truncated damage LO frame")?;
    Ok(data
        .as_chunks::<2>()
        .0
        .iter()
        .enumerate()
        .filter(|(_, point)| **point != [127, 127])
        .map(|(index, point)| EffectSpot {
            offset: [i32::from(point[0] as i8), i32::from(point[1] as i8)],
            variant: if index == 0 || index == 16 || index == 19 || index == 22 {
                0
            } else if index == 1 || index == 17 || index == 20 || index == 23 {
                1
            } else {
                2
            },
        })
        .collect())
}

fn rules_mines() -> Option<MineLayer> {
    Some(MineLayer {
        unit_type: UnitTypeId(18),
        initial_count: 3,
        deploy_range: 16,
    })
}

fn pose_frames(bytes: &[u8], script: u16, animation: usize) -> Vec<u16> {
    let mut previous = None;
    iscript::timeline(bytes, script, animation)
        .into_iter()
        .filter(|frame| {
            let changed = previous != Some(*frame);
            previous = Some(*frame);
            changed
        })
        .collect()
}

fn add_media(
    archive: &mut Archive<std::io::Cursor<Vec<u8>>>,
    files: &mut Files,
    units: &[u8],
    source: u16,
    id: UnitTypeId,
    media: &mut MediaManifest,
    cache: &mut BTreeMap<u16, PortraitManifest>,
) -> Result<()> {
    let n = usize::from(source);
    let sfx = archive.read_file("arr\\sfxdata.dat", 8712)?;
    let tbl = archive.read_file("arr\\sfxdata.tbl", 65536)?;
    let mut cumulative = 0;
    for (cue, first, last) in [
        (
            AudioCue::Select,
            word(units, 0x236c + n * 2),
            word(units, 0x2534 + n * 2),
        ),
        (
            AudioCue::Order,
            if source < 106 {
                word(units, 0x28a4 + n * 2)
            } else {
                0
            },
            if source < 106 {
                word(units, 0x2978 + n * 2)
            } else {
                0
            },
        ),
    ] {
        if first == 0 || first > last {
            continue;
        }
        ensure!(last - first < 16, "campaign sound range too large");
        let mut variants = Vec::new();
        for sound in first..=last {
            let path = terran_media::sound_path(&sfx, &tbl, sound)?;
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
        media.audio.push(AudioMapping {
            cue,
            unit_type: Some(id),
            voice: source < 106,
            variants,
        });
    }
    let portrait = word(units, 0x367c + n * 2);
    if portrait >= 90 {
        return Ok(());
    }
    if let Some(existing) = cache.get(&portrait) {
        let mut duplicate = existing.clone();
        duplicate.unit_type = id;
        media.portraits.push(duplicate);
        return Ok(());
    }
    let pd = archive.read_file("arr\\portdata.dat", 1080)?;
    let pt = archive.read_file("arr\\portdata.tbl", 65536)?;
    let mut seq = [Vec::new(), Vec::new()];
    let mut ms = 100;
    for (state, sequence) in seq.iter_mut().enumerate() {
        let prefix = terran_media::portrait_prefix(&pd, &pt, portrait, state)?;
        let path = format!("portrait\\{prefix}0.smk");
        let smk = terran_media::decode_smk(&archive.read_file(&path, 1024 * 1024)?)?;
        ms = smk.frame_ms;
        for (i, frame) in smk.frames.iter().enumerate() {
            sequence.push(crate::add_image(
                files,
                &format!("campaign-portrait-{portrait}-{state}-{i:03}.srim"),
                frame,
            )?);
        }
    }
    let [idle, talk] = seq;
    let p = PortraitManifest {
        unit_type: id,
        portrait_only: false,
        frame_ms: ms,
        idle,
        talk,
    };
    cache.insert(portrait, p.clone());
    media.portraits.push(p);
    Ok(())
}

pub fn add_mengsk(
    archive: &mut Archive<std::io::Cursor<Vec<u8>>>,
    files: &mut Files,
) -> Result<()> {
    let units = archive.read_file("arr\\units.dat", 19192)?;
    let mut media: MediaManifest = ron::de::from_bytes(&files["media.ron"])?;
    add_media(
        archive,
        files,
        &units,
        27,
        UnitTypeId(1001),
        &mut media,
        &mut BTreeMap::new(),
    )?;
    media
        .audio
        .retain(|a| a.unit_type != Some(UnitTypeId(1001)));
    if let Some(p) = media
        .portraits
        .iter_mut()
        .find(|p| p.unit_type == UnitTypeId(1001))
    {
        p.portrait_only = true;
    }
    files.insert("media.ron".into(), ron_bytes(&media)?);
    Ok(())
}

fn add_ui(
    archive: &mut Archive<std::io::Cursor<Vec<u8>>>,
    files: &mut Files,
    assets: &mut AssetManifest,
    selected: &[u16],
) -> Result<()> {
    let colors = formats::decode_pcx(&archive.read_file("unit\\cmdbtns\\ticon.pcx", 1024 * 1024)?)?;
    let commands = formats::decode_grp(
        &archive.read_file("unit\\cmdbtns\\cmdicons.grp", 1024 * 1024)?,
        &crate::terran_ui::command_palette(&colors)?,
    )?;
    let colors = formats::decode_pcx(&archive.read_file("game\\twire.pcx", 1024 * 1024)?)?;
    let palette = crate::terran_ui::wireframe_palette(&colors)?;
    let wire = formats::decode_grp(
        &archive.read_file("unit\\wirefram\\wirefram.grp", 1024 * 1024)?,
        &palette,
    )?;
    let group = formats::decode_grp(
        &archive.read_file("unit\\wirefram\\grpwire.grp", 1024 * 1024)?,
        &palette,
    )?;
    for &source in selected {
        let id = native_id(source).unwrap();
        for (kind, frames) in [
            ("unit", &commands),
            ("wireframe", &wire),
            ("groupwire", &group),
        ] {
            let key = format!("{kind}.{}", id.0);
            if let Some(image) = frames.get(usize::from(source))
                && !assets.ui.iter().any(|i| i.key == key)
            {
                assets.ui.push(straterust_engine::assets::UiImageManifest {
                    image: crate::add_image(
                        files,
                        &format!("campaign-{kind}-{source}.srim"),
                        image,
                    )?,
                    key,
                });
            }
        }
    }
    Ok(())
}

mod effects;
pub(crate) use effects::*;

mod zerg_effects;
pub(crate) use zerg_effects::*;

mod combat;
mod iscript;
pub(crate) use combat::{apply_combat_rules, refresh_combat};
mod buildings;
pub(crate) use buildings::refresh_buildings;
mod transport;
pub(crate) use transport::apply_transport_rules;
mod wireframes;
pub(crate) use wireframes::refresh_wireframes;
mod indicators;
pub(crate) use indicators::refresh_indicators;
mod research;
pub(crate) use research::refresh_research;
