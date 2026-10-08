use super::*;

#[test]
#[ignore = "requires STRATERUST_SOURCE pointing to the owner's retail disc"]
fn strike_stages_use_original_charge_flight_dot_and_impact_art() -> Result<()> {
    let source_path = std::env::var_os("STRATERUST_SOURCE").context("set STRATERUST_SOURCE")?;
    let source = crate::Source::open(std::path::Path::new(&source_path))?;
    let mut installer = Archive::open_region(&source.path, source.offset, source.len)?;
    let mut archive =
        Archive::from_bytes(installer.read_file("files\\stardat.mpq", 128 * 1024 * 1024)?)?;
    let tables = Tables::read(&mut archive)?;
    let charge = instructions(&tables.scripts, tables.script(543), 0);
    let mut ticks = 0;
    let mut fire = None;
    let mut release = None;
    for i in charge {
        if i.op == 5 {
            ticks += u32::from(i.args[0]);
        }
        if i.op == 39 {
            fire = Some(ticks);
        }
        if i.op == 36 && i.args[0] & 2 != 0 {
            release = Some(ticks);
        }
    }
    assert_eq!((fire, release), (Some(44), Some(49)));
    let transit: u32 = instructions(&tables.scripts, tables.script(316), 21)
        .iter()
        .take_while(|i| !(i.op == 36 && i.args[0] & 2 != 0))
        .filter(|i| i.op == 5)
        .map(|i| u32::from(i.args[0]))
        .sum();
    assert_eq!(transit, 250);
    assert_eq!(tables.sprite_image(267), 318);
    assert_eq!(tables.sprite_image(309), 422);
    let mut graphics = Graphics {
        tables: &tables,
        palette: formats::palette(&archive.read_file("tileset\\badlands.wpe", 1024)?)?,
        remaps: BTreeMap::new(),
        cache: BTreeMap::new(),
    };
    let mut files = Files::new();
    let muzzle = graphics.casting_offsets(&mut archive, 12)?;
    assert_eq!(muzzle.len(), 32);
    assert!(muzzle[0][1] < -16 && muzzle[8][0] > 16 && muzzle[16][1] > 16);
    assert_eq!(muzzle[24], [-muzzle[8][0], muzzle[8][1]]);
    let exhaust = graphics
        .projectile_trail(&mut archive, &mut files, 541)?
        .context("Yamato trail")?;
    assert_eq!(
        (exhaust.start_ms, exhaust.interval_ms, exhaust.directional),
        (210, 126, true)
    );
    assert_eq!(exhaust.effect.frames.len(), 51);
    assert!(
        exhaust
            .effect
            .sequence
            .iter()
            .all(|&frame| usize::from(frame) + 16 < exhaust.effect.frames.len())
    );
    let trail = graphics
        .projectile_trail_animation(&mut archive, &mut files, 316, 11)?
        .context("nuclear missile smoke")?;
    assert_eq!(
        (trail.start_ms, trail.interval_ms, trail.rear_offset),
        (0, 126, 10)
    );
    assert_eq!(trail.effect.sequence.len(), 11);
    for (tick, frame) in trail.effect.sequence.iter().enumerate() {
        let image = straterust_engine::assets::decode_image(
            &files[&trail.effect.frames[usize::from(*frame)].file],
        )?;
        assert_eq!(
            image.rgba.as_chunks::<4>().0.iter().any(|p| p[3] > 0),
            tick >= 3,
            "smoke must stay hidden for three ticks, then animate through eight poses"
        );
    }
    for (image, directions) in [
        (543, 1),
        (541, 17),
        (542, 1),
        (544, 1),
        (316, 17),
        (233, 1),
        (318, 1),
        (422, 1),
    ] {
        let effect = graphics.effect(&mut archive, &mut files, image, 0, directions)?;
        assert!(effect.frames.len() >= directions);
        assert!(
            effect
                .frames
                .iter()
                .any(
                    |frame| straterust_engine::assets::decode_image(&files[&frame.file])
                        .unwrap()
                        .rgba
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .any(|pixel| pixel[3] > 0)
                ),
            "image {image}"
        );
    }
    for frame in files.values() {
        straterust_engine::assets::decode_image(frame)?;
    }
    Ok(())
}

#[test]
#[ignore = "requires STRATERUST_SOURCE pointing to the owner's retail disc"]
fn wildlife_import_retains_original_idle_and_walking_timing() -> Result<()> {
    let source_path = std::env::var_os("STRATERUST_SOURCE").context("set STRATERUST_SOURCE")?;
    let source = crate::Source::open(std::path::Path::new(&source_path))?;
    let mut installer = Archive::open_region(&source.path, source.offset, source.len)?;
    let mut archive =
        Archive::from_bytes(installer.read_file("files\\stardat.mpq", 128 * 1024 * 1024)?)?;
    let tables = Tables::read(&mut archive)?;
    let mut graphics = Graphics {
        tables: &tables,
        palette: formats::palette(&archive.read_file("tileset\\badlands.wpe", 1024)?)?,
        remaps: BTreeMap::new(),
        cache: BTreeMap::new(),
    };
    let mut files = Files::new();
    let mut wildlife_frames = Vec::new();
    for source in [89, 90, 95] {
        let mut sprite = SpriteManifest {
            unit_type: native_id(source).unwrap(),
            unit_name: source.to_string(),
            frame_ms: 42,
            anchor: [0, 0],
            frames: vec![],
            clips: vec![],
        };
        graphics.wildlife(&mut archive, &mut files, &mut sprite, source)?;
        for (kind, animation) in [(ClipKind::Idle, 0), (ClipKind::Walk, 11)] {
            let clip = sprite.clips.iter().find(|c| c.kind == kind).unwrap();
            assert_eq!(clip.directions, 32);
            assert_eq!(
                clip.frames.len() / usize::from(clip.directions),
                timeline(
                    &tables.scripts,
                    tables.script(tables.image(source)),
                    animation
                )
                .len()
            );
        }
        let walk = sprite
            .clips
            .iter()
            .find(|c| c.kind == ClipKind::Walk)
            .unwrap();
        assert!(
            walk.frames
                .iter()
                .map(|f| f.frame)
                .collect::<BTreeSet<_>>()
                .len()
                > 1
        );
        for frame in &sprite.frames {
            let image = straterust_engine::assets::decode_image(&files[&frame.file])?;
            assert!(image.rgba.as_chunks::<4>().0.iter().any(|p| p[3] > 0));
        }
        wildlife_frames.extend(sprite.frames);
    }
    // Loading another species must not overwrite an earlier species' artwork.
    for frame in wildlife_frames {
        assert_eq!(
            blake3::hash(&files[&frame.file]).to_hex().as_str(),
            frame.blake3,
            "{}",
            frame.file
        );
    }
    Ok(())
}

#[test]
#[ignore = "requires STRATERUST_SOURCE pointing to the owner's retail disc"]
fn aircraft_shadows_follow_original_explicit_and_next_image_underlays() -> Result<()> {
    let source_path = std::env::var_os("STRATERUST_SOURCE").context("set STRATERUST_SOURCE")?;
    let source = crate::Source::open(std::path::Path::new(&source_path))?;
    let mut installer = Archive::open_region(&source.path, source.offset, source.len)?;
    let mut archive =
        Archive::from_bytes(installer.read_file("files\\stardat.mpq", 128 * 1024 * 1024)?)?;
    let tables = Tables::read(&mut archive)?;
    let mut graphics = Graphics {
        tables: &tables,
        palette: [[0; 4]; 256],
        remaps: BTreeMap::new(),
        cache: BTreeMap::new(),
    };
    let mut files = Files::new();
    for source in [9, 12, 28, 29, 69, 70, 71, 72, 82] {
        let mut sprite = SpriteManifest {
            unit_type: native_id(source).unwrap(),
            unit_name: source.to_string(),
            frame_ms: 42,
            anchor: [0, 0],
            frames: Vec::new(),
            clips: Vec::new(),
        };
        graphics.shadow(&mut archive, &mut files, &mut sprite, source)?;
        let shadow = sprite
            .clips
            .first()
            .with_context(|| format!("{source}: shadow"))?;
        assert_eq!(shadow.kind, ClipKind::Shadow);
        assert!(matches!(shadow.directions, 1 | 32));
        let image = straterust_engine::assets::decode_image(&files[&sprite.frames[0].file])?;
        assert!(image.rgba.as_chunks::<4>().0.iter().any(|p| p[3] == 100));
        if matches!(source, 72 | 82) {
            // Carrier uses imgulnextid, whose two arguments are offsets.
            assert_eq!(shadow.frames[0].offset[1], 42 - image.height as i16 / 2);
        }
    }
    Ok(())
}

#[test]
#[ignore = "requires STRATERUST_SOURCE pointing to the owner's retail disc"]
fn missile_turret_imports_longbolt_flight_and_explosion() -> Result<()> {
    let source_path = std::env::var_os("STRATERUST_SOURCE").context("set STRATERUST_SOURCE")?;
    let source = crate::Source::open(std::path::Path::new(&source_path))?;
    let mut installer = Archive::open_region(&source.path, source.offset, source.len)?;
    let mut archive =
        Archive::from_bytes(installer.read_file("files\\stardat.mpq", 128 * 1024 * 1024)?)?;
    let tile = Image {
        width: 1,
        height: 1,
        rgba: vec![0, 0, 0, 255],
    };
    let mut files = crate::native_files(&tile, std::slice::from_ref(&tile))?;
    let mut assets: AssetManifest = ron::de::from_bytes(&files["assets.ron"])?;
    let mut rules: Rules = ron::de::from_bytes(&files["rules.ron"])?;
    rules.units.truncate(1);
    let turret = &mut rules.units[0];
    turret.id = native_id(124).unwrap();
    apply_combat_rules(&mut archive, &mut rules)?;

    refresh_combat(&mut archive, &mut files, &mut assets, &rules)?;
    assets.validate()?;
    assert_eq!(assets.projectiles.len(), 1);
    let missile = &assets.projectiles[0];
    assert_eq!(missile.unit_type, native_id(124).unwrap());
    assert!(missile.targets_air && missile.directional && !missile.on_target);
    assert_eq!(missile.flight.frames.len(), 17);
    assert_eq!(missile.speed_fp8, 8533);
    assert_eq!(missile.forward_offset, 10);
    assert_eq!(missile.impact.sequence.len(), 20);
    let trail = missile.trail.as_ref().unwrap();
    assert_eq!((trail.start_ms, trail.interval_ms), (84, 126));
    assert_eq!(trail.effect.sequence.len(), 11);
    for (index, &pose) in trail.effect.sequence.iter().enumerate() {
        let frame = &trail.effect.frames[usize::from(pose)];
        assert!(frame.file.starts_with("combat-image-422-"));
        let image = straterust_engine::assets::decode_image(&files[&frame.file])?;
        assert_eq!(
            image.rgba.as_chunks::<4>().0.iter().any(|p| p[3] > 0),
            index >= 3
        );
    }
    for (effect, source_image) in [(&missile.flight, 529), (&missile.impact, 530)] {
        for frame in &effect.frames {
            assert!(
                frame
                    .file
                    .starts_with(&format!("combat-image-{source_image}-"))
            );
            let image = straterust_engine::assets::decode_image(&files[&frame.file])?;
            assert!(image.rgba.as_chunks::<4>().0.iter().any(|p| p[3] > 0));
        }
    }
    Ok(())
}
