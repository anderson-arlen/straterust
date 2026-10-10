//! Original Mission 2's additional Terran roles, using bounded existing decoders.
//! DAT/IScript layout facts are shared with terran_data/terran; no legacy VM runs here.
use crate::{
    Archive, Files, MemberReport, Source, add_image, formats, member, ron_bytes,
    terran::{
        center_canvas, compact_sprite, composite, decode_expected, directional, expect_animation,
        fire_palette, single_direction,
    },
    terran_data::{self, ReferenceUnit},
    terran_media,
};
use anyhow::{Context, Result, ensure};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    io::{Read, Seek},
    path::Path,
};
use straterust_engine::{
    assets::{
        AssetManifest, ClipKind, EffectManifest, Image, ProjectileManifest, SpriteManifest,
        UiImageManifest,
    },
    media::{AudioCue, AudioMapping, MediaManifest, PortraitManifest},
    sim::{DamageKind, Footprint, ResourceAmount, Rules, UnitSize, UnitType, UnitTypeId, Weapon},
};

const ROLES: [(u16, u16, &str, &str); 7] = [
    (19, 10, "Jim Raynor", "raynor"),
    (32, 11, "Firebat", "firebat"),
    (112, 12, "Academy", "academy"),
    (125, 13, "Bunker", "bunker"),
    (110, 14, "Refinery", "refinery"),
    (122, 15, "Engineering Bay", "engineering-bay"),
    (107, 16, "Comsat Station", "comsat"),
];
#[derive(Serialize)]
struct Report {
    schema_version: u32,
    units: Vec<ReferenceUnit>,
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
        "Mission 2 extra Terran source archive",
        &mut members,
    )?;
    let mut archive = Archive::from_bytes(data)?;
    let units = read(&mut archive, &mut members, "arr\\units.dat")?;
    let weapons = read(&mut archive, &mut members, "arr\\weapons.dat")?;
    let mut source_units = terran_data::decode_selected(&units, &weapons, &ROLES.map(|r| r.0))?;
    source_units.extend(terran_data::decode_selected(&units, &weapons, &[13])?);
    let mut rules: Rules = ron::de::from_bytes(&files["rules.ron"])?;
    apply_rules(&mut rules, &source_units)?;
    crate::campaign_units::apply_creep_rules(&mut rules);
    add_mine_rules(&mut rules, &source_units, &units)?;
    files.insert("rules.ron".into(), ron_bytes(&rules)?);
    let scripts = read(&mut archive, &mut members, "scripts\\iscript.bin")?;
    verify_scripts(&scripts)?;
    let palette = formats::palette(&read(&mut archive, &mut members, "tileset\\badlands.wpe")?)?;
    let fire = fire_palette(
        &read(&mut archive, &mut members, "tileset\\badlands\\ofire.pcx")?,
        &palette,
    )?;
    let mut assets: AssetManifest = ron::de::from_bytes(&files["assets.ron"])?;
    let bang = grp(
        &mut archive,
        &mut members,
        "unit\\thingy\\tBangS.grp",
        &fire,
        [9, 128, 128],
    )?;
    let vulture = grp(
        &mut archive,
        &mut members,
        "unit\\terran\\Vulture.grp",
        &palette,
        [17, 100, 100],
    )?;
    let mut frames = vulture;
    frames.extend(bang.iter().cloned());
    assets.extra_units.push(compact_sprite(
        files,
        10,
        "Jim Raynor",
        "raynor",
        &frames,
        vec![
            directional(ClipKind::Idle, &[0], 100),
            directional(ClipKind::Walk, &[0], 42),
            directional(ClipKind::Attack, &[0], 42),
            single_direction(ClipKind::Death, &(17..26).collect::<Vec<_>>(), 150),
        ],
    )?);
    add_grenade_projectiles(&mut archive, &mut members, files, &mut assets, &rules)?;
    let body = grp(
        &mut archive,
        &mut members,
        "unit\\terran\\firebat.grp",
        &palette,
        [170, 32, 32],
    )?;
    let flame = grp(
        &mut archive,
        &mut members,
        "unit\\thingy\\flamer.grp",
        &fire,
        [221, 224, 224],
    )?;
    // Every heading keeps the source body/fire origin. Cropping happens after composition.
    let mut frames = body.clone();
    for (index, fire) in flame.iter().enumerate() {
        frames.push(composite(
            &center_canvas(
                &body[if index / 17 < 11 {
                    17 + index % 17
                } else {
                    index % 17
                }],
                224,
                224,
            )?,
            fire,
        )?);
    }
    let death_start = frames.len() as u16;
    frames.extend(bang);
    let attack: Vec<_> = std::iter::once(0)
        .chain((0..13).map(|step| 170 + step * 17))
        .collect();
    assets.extra_units.push(compact_sprite(
        files,
        11,
        "Firebat",
        "firebat",
        &frames,
        vec![
            directional(ClipKind::Idle, &[34], 100),
            directional(ClipKind::Walk, &[34, 51, 68, 85, 102, 119, 136, 153], 42),
            directional(ClipKind::Attack, &attack, 42),
            single_direction(
                ClipKind::Death,
                &(death_start..death_start + 9).collect::<Vec<_>>(),
                150,
            ),
        ],
    )?);
    let medium = grp(
        &mut archive,
        &mut members,
        "unit\\terran\\tBldMed.grp",
        &palette,
        [3, 96, 96],
    )?;
    let small = grp(
        &mut archive,
        &mut members,
        "unit\\terran\\tBldSml.grp",
        &palette,
        [3, 96, 96],
    )?;
    for (id, path, expected, construction, overlay_path, overlay_expected) in [
        (
            12,
            "unit\\terran\\Academy.grp",
            [2, 96, 128],
            Some(&medium),
            Some("unit\\terran\\AcademyT.grp"),
            [1, 96, 128],
        ),
        (
            13,
            "unit\\terran\\PillBox.grp",
            [2, 96, 128],
            Some(&small),
            None,
            [0, 0, 0],
        ),
        (
            14,
            "unit\\terran\\refinery.grp",
            [5, 192, 192],
            None,
            None,
            [0, 0, 0],
        ),
        (
            15,
            "unit\\terran\\weaponpl.grp",
            [6, 192, 160],
            Some(&medium),
            Some("unit\\terran\\weaponpT.grp"),
            [6, 192, 160],
        ),
        (
            16,
            "unit\\terran\\ComSat.grp",
            [2, 128, 64],
            Some(&small),
            Some("unit\\terran\\ComSatT.grp"),
            [10, 128, 64],
        ),
    ] {
        let (_, _, name, slug) = ROLES.iter().find(|r| r.1 == id).unwrap();
        let body = grp(&mut archive, &mut members, path, &palette, expected)?;
        let mut frames = body.clone();
        let construction_poses = if let Some(stages) = construction {
            let start = frames.len() as u16;
            frames.extend(stages.iter().cloned());
            vec![start, start + 1, start + 2, 1]
        } else {
            vec![1, 2, 3, 4]
        };
        let mut idle = 0;
        let mut clips = vec![single_direction(
            ClipKind::Construction,
            &construction_poses,
            100,
        )];
        if let Some(path) = overlay_path {
            let overlay = grp(&mut archive, &mut members, path, &palette, overlay_expected)?;
            let start = frames.len() as u16;
            for image in &overlay {
                frames.push(composite(&body[0], image)?);
            }
            if matches!(id, 15 | 16) {
                idle = start;
            }
            let production = if id == 12 {
                vec![start, 0, 0]
            } else {
                (start..start + overlay.len() as u16).collect()
            };
            clips.push(single_direction(
                ClipKind::Production,
                &production,
                if id == 15 { 300 } else { 150 },
            ));
        }
        clips.push(single_direction(ClipKind::Idle, &[idle], 100));
        let mut sprite = compact_sprite(files, id, name, slug, &frames, clips)?;
        if id == 14 {
            let large = grp(
                &mut archive,
                &mut members,
                "unit\\thingy\\tBangL.grp",
                &fire,
                [10, 200, 200],
            )?;
            let extra = compact_sprite(
                files,
                id,
                name,
                "refinery-death",
                &large,
                vec![single_direction(
                    ClipKind::Death,
                    &(0..10).collect::<Vec<_>>(),
                    150,
                )],
            )?;
            append_sprite_frames(&mut sprite, &extra)?;
        } else {
            // These four building scripts share Supply Depot's exact explosion/rubble sequence.
            let source = assets
                .extra_units
                .iter()
                .find(|s| s.unit_type == UnitTypeId(4))
                .context("missing shared Terran building death")?;
            copy_death(files, &mut sprite, source)?;
        }
        assets.extra_units.push(sprite);
    }
    add_ui(&mut archive, &mut members, files, &mut assets)?;
    add_mine_art(
        &mut archive,
        &mut members,
        files,
        &mut assets,
        &scripts,
        &palette,
        &fire,
    )?;
    add_scan_effect(
        &mut archive,
        &mut members,
        files,
        &mut assets,
        &scripts,
        &palette,
    )?;
    crate::campaign_units::refresh_effects(&mut archive, files, &mut assets, &rules)?;
    assets.validate()?;
    files.insert("assets.ron".into(), ron_bytes(&assets)?);
    add_media(&mut archive, &mut members, &units, files)?;
    crate::campaign_units::refresh_bunker_audio(&mut archive, files, &rules)?;
    files.insert("mission-terran-reference.ron".into(),ron_bytes(&Report{
        schema_version:1,units:source_units,members,
        evidence:vec![
            "Original units.dat maps source19/32/112/125/110/122/107 to native10..16. The v1.00 Bunker has350 HP; Engineering Bay850HP. Source23 and29 both map to Duke portrait14, imported as presentation-only1000.",
            "Firebat script69 emits forward strikes24,52,80; image421/script312 supplies13 directional flame poses. Native poses include the one-frame attack prefix, then precompose source body17 for flame steps0..10 and body0 for steps11..12 before transparent crop. Mobile walk/attack steps use the campaign42ms tick. Frame offsets preserve every visible source pixel under both orientations.",
            "Academy/EngineeringBay/Comsat production overlays come from source images264/323/273. Ordinary building deaths share the source Supply Depot explosion/rubble; Refinery death uses tBangL without rubble. Original command/wireframe unit IDs are retained.",
            "Scanner script81 launches nine sprite380 pulses at source signed offsets. Sprite380/image546/script253 is ordinary-palette eveCast.grp; its eight poses and launch waits are precomposed into a finite156-tick sequence at42ms. The final109ticks have no visible pulse while source detection continues. Scanner sound388 follows the source CastScannerSweep handler.",
            "Raynor starts with3mines and bypasses Spider Mines research as a hero. Source13 maps to native18:20HP,15x15 collision,125explosive friendly ground splash at40/60/80. Source script87 moves16pixels per wait1; source arming60ticks,burrow4ticks,unburrow3ticks,trigger square96,chase576,detonate30. The original waitN instruction stores N-1. Source behaviors: OpenBW order_PlaceMine/order_SpiderMine/unit_can_use_tech.",
        ],limitations:vec![
            "Campaign Motion records supersede base integer speeds: Raynor imports acceleration 100/256 and top speed 1707/256 pixels per frame, and Firebat imports its move4/wait1 cycle. Source steering/braking and first-attack pose transitions remain uncalibrated; see backwater-reference.ron.",
            "Firebat orange-fire blending uses the existing explicit black-backdrop RGBA approximation. Source shadows and randomized idle gestures/gas plumes are not included in these selected clips. Campaign flying-building art is supplied separately by flight-reference.ron.",
            "Construction art uses source selected stage poses with native progress fractions; random overlay waits use fixed native intervals. Rubble uses the existing shortened presentation-only lifetime. Source IScript is not executed at runtime. Mine dust is retained during burrow/emergence but its remaining attachment lifetime after emergence is omitted.",
        ],
    })?);
    Ok(())
}

fn add_mine_rules(rules: &mut Rules, source: &[ReferenceUnit], units: &[u8]) -> Result<()> {
    use straterust_engine::sim::{MineLayer, MineStats};
    let reference = source
        .iter()
        .find(|unit| unit.source_id == 13)
        .context("missing source mine stats")?;
    let weapon = reference.weapon.as_ref().context("missing mine weapon")?;
    ensure!(
        weapon.id == 6 && weapon.effect == 2 && weapon.damage_type == 1,
        "unsupported mine weapon"
    );
    let [left, top, right, bottom] = reference.collision_extents;
    rules.units.push(UnitType {
        id: UnitTypeId(18),
        speed: 16,
        max_hp: reference.hitpoints,
        armor: u32::from(reference.armor),
        size: UnitSize::Small,
        footprint: Footprint {
            width: left + right + 1,
            height: top + bottom + 1,
        },
        placement: Footprint {
            width: reference.placement_size[0],
            height: reference.placement_size[1],
        },
        vision_range: u32::from(units[0x1e24 + 13]) * 32,
        triggers_mines: false,
        mine: Some(MineStats {
            arm_ticks: 60,
            conceal_ticks: 4,
            reveal_ticks: 3,
            trigger_range: 96,
            chase_range: 576,
            detonation_range: 30,
        }),
        weapon: Some(Weapon {
            friendly_splash: false,
            projectile_speed: 0,
            cooldown_jitter: None,
            targets_air: false,
            target_classes: Vec::new(),
            damage: u32::from(weapon.damage),
            range: weapon.maximum_range,
            cooldown: u32::from(weapon.cooldown_frames),
            damage_kind: DamageKind::Explosive,
            splash: Some(weapon.splash_radii.map(u32::from)),
            strikes: vec![],
        }),
        ..UnitType::default()
    });
    rules
        .units
        .iter_mut()
        .find(|u| u.id == UnitTypeId(10))
        .context("missing Raynor")?
        .mine_layer = Some(MineLayer {
        unit_type: UnitTypeId(18),
        initial_count: 3,
        deploy_range: 20,
    });
    // Source units.dat unknown1==0xc1 identifies hover units ignored by mine acquisition.
    for (legacy, native) in [(0, 1), (7, 2), (19, 10), (32, 11), (37, 6), (38, 7)] {
        let unit = rules
            .units
            .iter_mut()
            .find(|u| u.id == UnitTypeId(native))
            .context("missing mobile role")?;
        unit.triggers_mines = units[0x10c8 + legacy] != 0xc1;
    }
    Ok(())
}

fn apply_rules(rules: &mut Rules, source: &[ReferenceUnit]) -> Result<()> {
    rules.prioritize_threats = true;
    for &(legacy, id, _, _) in &ROLES {
        ensure!(
            rules.units.iter().all(|u| u.id != UnitTypeId(id)),
            "duplicate mission Terran role"
        );
        let r = source
            .iter()
            .find(|r| r.source_id == legacy)
            .context("missing source role stats")?;
        ensure!(
            r.supply_required_half_units.is_multiple_of(2)
                && r.supply_provided_half_units.is_multiple_of(2),
            "fractional Terran supply"
        );
        let [left, up, right, down] = r.collision_extents;
        let weapon = r
            .weapon
            .as_ref()
            .map(|w| -> Result<Weapon> {
                ensure!(
                    w.damage_type == 2 && w.minimum_range == 0,
                    "unexpected extra Terran weapon"
                );
                Ok(Weapon {
                    friendly_splash: false,
                    projectile_speed: 0,
                    cooldown_jitter: None,
                    targets_air: false,
                    target_classes: Vec::new(),
                    damage: u32::from(w.damage),
                    range: w.maximum_range,
                    cooldown: u32::from(w.cooldown_frames),
                    damage_kind: DamageKind::Concussive,
                    splash: (w.effect == 3).then(|| w.splash_radii.map(u32::from)),
                    strikes: if legacy == 32 {
                        [(0, 24), (2, 52), (3, 80)]
                            .map(|(delay, forward)| straterust_engine::sim::WeaponStrike {
                                delay,
                                forward,
                            })
                            .to_vec()
                    } else {
                        Vec::new()
                    },
                })
            })
            .transpose()?;
        rules.units.push(UnitType {
            id: UnitTypeId(id),
            speed: match id {
                10 => 7,
                11 => 4,
                _ => 0,
            },
            footprint: Footprint {
                width: left + right + 1,
                height: up + down + 1,
            },
            placement: Footprint {
                width: r.placement_size[0],
                height: r.placement_size[1],
            },
            garrison: (id == 13).then(|| straterust_engine::sim::GarrisonStats {
                boarding_range: 1,
                capacity: 4,
                passengers: vec![UnitTypeId(1), UnitTypeId(2), UnitTypeId(11)],
                attackers: vec![UnitTypeId(1), UnitTypeId(11)],
                range_bonus: 64,
                unload_ticks: 0,
            }),
            max_hp: r.hitpoints,
            armor: u32::from(r.armor),
            size: match r.unit_size {
                1 => UnitSize::Small,
                2 => UnitSize::Medium,
                3 => UnitSize::Large,
                _ => anyhow::bail!("unsupported unit size"),
            },
            acquisition_range: if id <= 11 { Some(256) } else { None },
            structure: id >= 12,
            cost: [("minerals", r.minerals), ("gas", r.gas)]
                .into_iter()
                .filter(|(_, amount)| *amount != 0)
                .map(|(kind, amount)| ResourceAmount {
                    kind: kind.into(),
                    amount: u32::from(amount),
                })
                .collect(),
            build_ticks: u32::from(r.build_frames),
            supply_used: u32::from(r.supply_required_half_units / 2),
            supply_provided: u32::from(r.supply_provided_half_units / 2),
            weapon,
            prerequisites: match id {
                11 => vec![UnitTypeId(5), UnitTypeId(12)],
                16 => vec![UnitTypeId(12)],
                _ => Vec::new(),
            },
            ..UnitType::default()
        });
    }
    // Source Mission2 permits these SCV structures; rescued Academy permits Firebat training.
    let worker = rules
        .units
        .iter_mut()
        .find(|u| u.id == UnitTypeId(2))
        .context("missing SCV")?;
    worker.builds.extend([UnitTypeId(14), UnitTypeId(15)]);
    worker
        .repairs
        .extend((10..=16).filter(|id| *id != 11).map(UnitTypeId));
    rules
        .units
        .iter_mut()
        .find(|u| u.id == UnitTypeId(5))
        .context("missing Barracks")?
        .trains
        .push(UnitTypeId(11));
    Ok(())
}

fn add_ui<R: Read + Seek>(
    archive: &mut Archive<R>,
    members: &mut Vec<MemberReport>,
    files: &mut Files,
    assets: &mut AssetManifest,
) -> Result<()> {
    let colors = formats::decode_pcx(&read(archive, members, "unit\\cmdbtns\\ticon.pcx")?)?;
    let commands = grp(
        archive,
        members,
        "unit\\cmdbtns\\cmdicons.grp",
        &crate::terran_ui::command_palette(&colors)?,
        [365, 36, 34],
    )?;
    let colors = formats::decode_pcx(&read(archive, members, "game\\twire.pcx")?)?;
    let wires = grp(
        archive,
        members,
        "unit\\wirefram\\wirefram.grp",
        &crate::terran_ui::wireframe_palette(&colors)?,
        [228, 64, 64],
    )?;
    let groups = grp(
        archive,
        members,
        "unit\\wirefram\\grpwire.grp",
        &crate::terran_ui::wireframe_palette(&colors)?,
        [131, 32, 32],
    )?;
    for (source, id) in
        ROLES
            .iter()
            .map(|r| (r.0, r.1))
            .chain([(37, 6), (38, 7), (130, 8), (143, 9), (13, 18)])
    {
        for (kind, image) in [
            ("unit", &commands[usize::from(source)]),
            ("wireframe", &wires[usize::from(source)]),
        ] {
            assets.ui.push(UiImageManifest {
                key: format!("{kind}.{id}"),
                image: add_image(files, &format!("ui-{kind}-{id}.srim"), image)?,
            });
        }
        if [19, 32, 37, 38, 13].contains(&source) {
            assets.ui.push(UiImageManifest {
                key: format!("groupwire.{id}"),
                image: add_image(
                    files,
                    &format!("ui-groupwire-{id}.srim"),
                    &groups[usize::from(source)],
                )?,
            });
        }
    }
    // DAT upgrade icons and source command-icon IDs (Stargus scripts/terran/icons.lua).
    for (key, frame) in [
        ("research.1", 288),
        ("research.2", 292),
        ("research.3", 238),
        ("research.4", 237),
        ("command.stim", 237),
        ("command.scan", 250),
        ("command.mine", 243),
        ("command.lift", 282),
        ("command.land", 283),
        ("command.unload", 312),
    ] {
        assets.ui.push(UiImageManifest {
            key: key.into(),
            image: add_image(files, &format!("ui-{key}.srim"), &commands[frame])?,
        });
    }
    Ok(())
}

fn add_media<R: Read + Seek>(
    archive: &mut Archive<R>,
    members: &mut Vec<MemberReport>,
    units: &[u8],
    files: &mut Files,
) -> Result<()> {
    let mut media: MediaManifest = ron::de::from_bytes(&files["media.ron"])?;
    let data = read(archive, members, "arr\\sfxdata.dat")?;
    let table = read(archive, members, "arr\\sfxdata.tbl")?;
    let mut pcm_bytes = 0;
    let mut cues: Vec<(AudioCue, u16, Vec<u16>, bool)> = Vec::new();
    for &(source, id, _, _) in &ROLES {
        let word = |offset| u16::from_le_bytes(units[offset..offset + 2].try_into().unwrap());
        let i = usize::from(source) * 2;
        let (first, last) = (word(0x236c + i), word(0x2534 + i));
        ensure!(
            first > 0 && first <= last && last - first < 16,
            "invalid unit selection voices"
        );
        cues.push((AudioCue::Select, id, (first..=last).collect(), id <= 11));
        if source < 106 {
            let (first, last) = (word(0x28a4 + i), word(0x2978 + i));
            ensure!(
                first > 0 && first <= last && last - first < 16,
                "invalid unit order voices"
            );
            cues.push((AudioCue::Order, id, (first..=last).collect(), true));
            let ready = word(0x2298 + i);
            if ready != 0 {
                cues.push((AudioCue::Ready, id, vec![ready], true));
            }
        }
    }
    cues.extend([
        (AudioCue::Death, 10, vec![353], false),
        (AudioCue::Death, 11, vec![296, 297, 298], false),
        (AudioCue::Attack, 11, vec![314, 315], false),
        (AudioCue::Scan, 16, vec![388], false),
        (AudioCue::Death, 18, vec![10], false),
        (AudioCue::Work, 18, vec![354], false),
        (AudioCue::Attack, 18, vec![355], false),
    ]);
    for id in 12..=16 {
        cues.push((AudioCue::Death, id, vec![7], false));
    }
    for (cue, id, sounds, voice) in cues {
        let mut variants = Vec::new();
        for sound in sounds {
            let name = format!("sound-{sound:03}.wav");
            let reference = if let Some(bytes) = files.get(&name) {
                straterust_engine::media::AudioRef {
                    file: name,
                    blake3: blake3::hash(bytes).to_hex().to_string(),
                }
            } else {
                let path = terran_media::sound_path(&data, &table, sound)?;
                let source = member(
                    archive,
                    &path,
                    64 * 1024 * 1024,
                    "stardat",
                    "Mission 2 source unit voices/effects",
                    members,
                )?;
                let normalized = terran_media::normalize_wav(&source, 30_000, &mut pcm_bytes)?;
                terran_media::audio_file(files, name, normalized)
            };
            variants.push(reference);
        }
        media.audio.push(AudioMapping {
            cue,
            unit_type: Some(UnitTypeId(id)),
            voice,
            variants,
        });
    }
    let portdata = read(archive, members, "arr\\portdata.dat")?;
    let porttable = read(archive, members, "arr\\portdata.tbl")?;
    let mut rgba_bytes = 0;
    for (portrait, id) in [(13, 10), (2, 11), (14, 1000)] {
        let mut sequences = [Vec::new(), Vec::new()];
        let mut frame_ms = None;
        // Firebat has two original talk variants; Raynor and Duke each have three.
        for (state, variants) in [(0, 4), (1, if portrait == 2 { 2 } else { 3 })] {
            let prefix = terran_media::portrait_prefix(&portdata, &porttable, portrait, state)?;
            for variant in 0..variants {
                let path = format!("portrait\\{prefix}{variant}.smk");
                let bytes = member(
                    archive,
                    &path,
                    1024 * 1024,
                    "stardat",
                    "Mission 2 selected or transmission portrait",
                    members,
                )?;
                let decoded =
                    terran_media::decode_smk(&bytes).with_context(|| format!("portrait {path}"))?;
                ensure!(
                    frame_ms.is_none_or(|ms| ms == decoded.frame_ms),
                    "inconsistent portrait timing"
                );
                frame_ms = Some(decoded.frame_ms);
                for image in decoded.frames {
                    rgba_bytes += image.rgba.len();
                    ensure!(
                        rgba_bytes <= 64 * 1024 * 1024,
                        "extra portraits exceed64MiB"
                    );
                    sequences[state].push(add_image(
                        files,
                        &format!(
                            "portrait-{portrait}-{state}-{:03}.srim",
                            sequences[state].len()
                        ),
                        &image,
                    )?);
                }
                ensure!(
                    sequences[state].len() <= 256,
                    "extra portrait sequence exceeds256frames"
                );
            }
        }
        let [idle, talk] = sequences;
        media.portraits.push(PortraitManifest {
            unit_type: UnitTypeId(id),
            portrait_only: id == 1000,
            frame_ms: frame_ms.context("empty portrait sequence")?,
            idle,
            talk,
        });
    }
    let advisor = media
        .portraits
        .iter()
        .find(|p| p.unit_type == UnitTypeId(3))
        .context("missing advisor portrait")?
        .clone();
    for id in 12..=16 {
        let mut portrait = advisor.clone();
        portrait.unit_type = UnitTypeId(id);
        media.portraits.push(portrait);
    }
    media.validate()?;
    files.insert("media.ron".into(), ron_bytes(&media)?);
    Ok(())
}

#[cfg(test)]
mod tests;

mod graphics;
pub(crate) use graphics::*;
