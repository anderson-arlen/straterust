use super::*;

pub(crate) fn refresh_effects(
    archive: &mut Archive<std::io::Cursor<Vec<u8>>>,
    files: &mut Files,
    assets: &mut AssetManifest,
    rules: &Rules,
) -> Result<()> {
    let units = archive.read_file("arr\\units.dat", 19192)?;
    let flingy = archive.read_file("arr\\flingy.dat", 2760)?;
    let sprites = archive.read_file("arr\\sprites.dat", 2081)?;
    let images = archive.read_file("arr\\images.dat", 28690)?;
    let names = archive.read_file("arr\\images.tbl", 65536)?;
    let scripts = archive.read_file("scripts\\iscript.bin", 65536)?;
    ensure!(
        units.len() == 19192
            && flingy.len() == 2760
            && sprites.len() == 2081
            && images.len() == 28690,
        "unsupported damage overlay source tables"
    );
    let palette = formats::palette(&archive.read_file("tileset\\badlands.wpe", 1024)?)?;
    let fire = terran::fire_palette(
        &archive.read_file("tileset\\badlands\\ofire.pcx", 8 * 1024 * 1024)?,
        &palette,
    )?;
    let mut small = Vec::new();
    let mut large = Vec::new();
    for (variant, name) in ["oFireC", "oFireF", "oFireV"].into_iter().enumerate() {
        let decoded = formats::decode_grp(
            &archive.read_file(&format!("unit\\thingy\\{name}.grp"), 8 * 1024 * 1024)?,
            &fire,
        )?;
        ensure!(decoded.len() == 24, "unexpected damage flame frames");
        let references = decoded
            .iter()
            .enumerate()
            .map(|(n, frame)| {
                crate::add_image(files, &format!("damage-fire-{variant}-{n:02}.srim"), frame)
            })
            .collect::<Result<Vec<_>>>()?;
        let effect = |start: usize| EffectManifest {
            frame_ms: 42,
            anchor: [decoded[0].width as i32 / 2, decoded[0].height as i32 / 2],
            frames: references[start..start + 12].to_vec(),
            sequence: (0..12).flat_map(|n| [n, n]).collect(),
        };
        small.push(effect(0));
        large.push(effect(12));
    }
    let mut overlays = Vec::new();
    for &(source, native) in MAPPING {
        if !rules
            .units
            .iter()
            .any(|unit| unit.id == UnitTypeId(native) && unit.structure)
        {
            continue;
        }
        let sprite = usize::from(word(&flingy, usize::from(units[usize::from(source)]) * 2));
        let image = usize::from(word(&sprites, sprite * 2));
        let damage_file = dword(&images, 755 * 22 + image * 4);
        if damage_file == 0 {
            continue;
        }
        let path = format!("unit\\{}", terran_media::table_string(&names, damage_file)?);
        let offsets = archive.read_file(&path, 1024 * 1024)?;
        let script = dword(&images, 755 * 10 + image * 4) as u16;
        let pose = pose_frames(&scripts, script, 0)
            .first()
            .copied()
            .unwrap_or(0);
        let spots = damage_spots(&offsets, usize::from(pose))?;
        if !spots.is_empty() {
            overlays.push(UnitEffectManifest {
                style: if (130..=153).contains(&source) {
                    1
                } else if (154..=175).contains(&source) {
                    2
                } else {
                    0
                },
                unit_type: UnitTypeId(native),
                spots,
            });
        }
    }
    let mut styles = Vec::new();
    for (style, color, names, scripts) in [
        (
            "blood",
            palette,
            ["bblood01", "bblood02", "bblood03"],
            [324, 328],
        ),
        (
            "blue",
            terran::fire_palette(
                &archive.read_file("tileset\\badlands\\bexpl.pcx", 1024 * 1024)?,
                &palette,
            )?,
            ["oFireC", "oFireF", "oFireV"],
            [322, 326],
        ),
    ] {
        let mut phases = [Vec::new(), Vec::new()];
        for (variant, name) in names.into_iter().enumerate() {
            let decoded = formats::decode_grp(
                &archive.read_file(&format!("unit\\thingy\\{name}.grp"), 8 * 1024 * 1024)?,
                &color,
            )?;
            let references = decoded
                .iter()
                .enumerate()
                .map(|(n, frame)| {
                    crate::add_image(files, &format!("damage-{style}-{variant}-{n}.srim"), frame)
                })
                .collect::<Result<Vec<_>>>()?;
            for (phase, script) in scripts.into_iter().enumerate() {
                let mut sequence = iscript::timeline(
                    &archive.read_file("scripts\\iscript.bin", 65536)?,
                    script,
                    0,
                );
                sequence.retain(|pose| usize::from(*pose) < decoded.len());
                if sequence.is_empty() {
                    sequence.push(0);
                }
                phases[phase].push(EffectManifest {
                    frame_ms: 42,
                    anchor: [decoded[0].width as i32 / 2, decoded[0].height as i32 / 2],
                    frames: references.clone(),
                    sequence,
                });
            }
        }
        let [small, large] = phases;
        styles.push(straterust_engine::assets::DamageStyleManifest {
            small: small.try_into().unwrap(),
            large: large.try_into().unwrap(),
        });
    }
    assets.damage_effects = Some(DamageEffectsManifest {
        styles,
        small: small.try_into().unwrap(),
        large: large.try_into().unwrap(),
        units: overlays,
    });
    if rules.units.iter().any(|unit| unit.id == UnitTypeId(36)) {
        terran::expect_animation(&scripts, 119, 16, &[8, 0x29, 1, 0, 0, 0, 2, 0])?;
        terran::expect_animation(&scripts, 120, 0, &[0, 0, 0, 5, 1, 0x21, 7])?;
        let base = formats::decode_grp(
            &archive.read_file("unit\\terran\\missile.grp", 8 * 1024 * 1024)?,
            &palette,
        )?;
        let top = formats::decode_grp(
            &archive.read_file("unit\\terran\\missileT.grp", 8 * 1024 * 1024)?,
            &palette,
        )?;
        ensure!(
            base.len() >= 3 && top.len() >= 17,
            "unexpected Missile Turret frames: base={} top={}",
            base.len(),
            top.len()
        );
        let frames = (0..32)
            .map(|heading| {
                let index = if heading <= 16 { heading } else { 32 - heading };
                let mut turret = top[index].clone();
                if heading > 16 {
                    for row in turret.rgba.chunks_exact_mut(turret.width as usize * 4) {
                        for x in 0..turret.width as usize / 2 {
                            for channel in 0..4 {
                                row.swap(
                                    x * 4 + channel,
                                    (turret.width as usize - 1 - x) * 4 + channel,
                                );
                            }
                        }
                    }
                }
                terran::composite(&base[2], &turret)
            })
            .collect::<Result<Vec<_>>>()?;
        let mut sprite = terran::compact_sprite(
            files,
            36,
            "Missile Turret",
            "missile-turret",
            &frames,
            vec![
                terran::single_direction(ClipKind::Idle, &(0..32).collect::<Vec<_>>(), 42),
                SpriteClip {
                    key_steps: Vec::new(),
                    kind: ClipKind::Attack,
                    frame_ms: 42,
                    directions: 32,
                    frames: (0..32)
                        .map(|frame| ClipFrame {
                            frame,
                            flip_x: false,
                            offset: [0, 0],
                        })
                        .collect(),
                    loop_start: None,
                    progress_starts: vec![],
                },
            ],
        )?;
        if let Some(existing) = assets
            .extra_units
            .iter_mut()
            .find(|sprite| sprite.unit_type == UnitTypeId(36))
        {
            // Keep the prior construction/death art while replacing the completed body.
            let mut remap = BTreeMap::new();
            for clip in existing
                .clips
                .iter()
                .filter(|clip| !matches!(clip.kind, ClipKind::Idle | ClipKind::Attack))
            {
                let mut clip = clip.clone();
                for frame in &mut clip.frames {
                    frame.frame = *remap.entry(frame.frame).or_insert_with(|| {
                        let index = sprite.frames.len() as u16;
                        sprite
                            .frames
                            .push(existing.frames[usize::from(frame.frame)].clone());
                        index
                    });
                }
                sprite.clips.push(clip);
            }
            *existing = sprite;
        }
    }
    // Geysers and extraction buildings emit the same source smoke from special LO vents.
    let geyser = formats::decode_grp(
        &archive.read_file("unit\\neutral\\geyser.grp", 8 * 1024 * 1024)?,
        &palette,
    )?;
    let geyser_image = geyser.first().context("missing geyser art")?;
    assets.resources.retain(|resource| resource.kind != "gas");
    assets.resources.push(ResourceManifest {
        terrain: false,
        terrain_edges: None,
        depleted_image: None,
        active_image: None,
        positions: Vec::new(),
        selection_circle: None,
        selection_y: 0,
        kind: "gas".into(),
        anchor: [
            geyser_image.width as i32 / 2,
            geyser_image.height as i32 / 2,
        ],
        image: crate::add_image(files, "vespene-geyser.srim", geyser_image)?,
    });
    let mut plumes = Vec::new();
    for name in ["GeySmok1", "GeySmok2", "GeySmok3", "GeySmoS1"] {
        let decoded = formats::decode_grp(
            &archive.read_file(&format!("unit\\thingy\\{name}.grp"), 8 * 1024 * 1024)?,
            &palette,
        )?;
        ensure!(
            !decoded.is_empty() && decoded.len() <= 32,
            "unexpected gas plume frames"
        );
        let frames = decoded
            .iter()
            .enumerate()
            .map(|(index, frame)| {
                crate::add_image(files, &format!("gas-{name}-{index:02}.srim"), frame)
            })
            .collect::<Result<Vec<_>>>()?;
        plumes.push(EffectManifest {
            frame_ms: 42,
            anchor: [decoded[0].width as i32 / 2, decoded[0].height as i32 / 2],
            sequence: (0..frames.len() as u16)
                .flat_map(|frame| [frame, frame])
                .collect(),
            frames,
        });
    }
    let depleted = plumes.pop().unwrap();
    let mut gas_units = Vec::new();
    for &(source, native) in MAPPING {
        if !rules
            .units
            .iter()
            .any(|unit| unit.id == UnitTypeId(native) && unit.extracts.is_some())
        {
            continue;
        }
        let sprite = usize::from(word(&flingy, usize::from(units[usize::from(source)]) * 2));
        let image = usize::from(word(&sprites, sprite * 2));
        let file = dword(&images, 755 * 26 + image * 4);
        ensure!(file != 0, "missing extraction building gas vents");
        let path = format!("unit\\{}", terran_media::table_string(&names, file)?);
        let spots = damage_spots(&archive.read_file(&path, 1024 * 1024)?, 0)?;
        gas_units.push(UnitEffectManifest {
            style: 0,
            unit_type: UnitTypeId(native),
            spots,
        });
    }
    let geyser_spots = damage_spots(
        &archive.read_file("unit\\neutral\\geyser.los", 1024 * 1024)?,
        0,
    )?;
    assets.gas_effects = Some(GasEffectsManifest {
        plumes: plumes.try_into().unwrap(),
        depleted,
        geyser_spots,
        units: gas_units,
    });
    files.insert("gas-effects-reference.ron".into(), ron_bytes(&(
        "Retail image344 neutral/geyser.grp; images430..432 GeySmok1/2/3 and depleted435..437 GeySmoS1. creategasoverlays uses special LO vent offsets on geysers, refineries and extractors.",
        "Native puffs retain source poses with two-tick holds; staggered 64-tick emission is deterministic presentation, not calibrated source RNG cadence."))?);
    if files.contains_key("media.ron") {
        refresh_bunker_audio(archive, files, rules)?;
    }
    refresh_creep_and_sunken(archive, files, assets, rules)?;
    files.insert("damage-effects-reference.ron".into(), ron_bytes(&(
        "Retail images.dat Damage overlay LO files, oFireC/F/V 24-frame GRPs; scripts322/326 hold each small/large pose for2frames. HP thresholds follow OpenBW update_unit_damage_overlay. Attachment order is stable rather than source-randomized.",
        "Missile Turret image296/script119 spawns image297/missileT.grp/script120 after construction; wait1/turn1cwise advances one of32 headings per frame. Grenade arc height24 is cosmetic and remains uncalibrated against source runtime."))?);
    Ok(())
}
