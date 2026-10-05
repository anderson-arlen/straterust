use super::*;

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
