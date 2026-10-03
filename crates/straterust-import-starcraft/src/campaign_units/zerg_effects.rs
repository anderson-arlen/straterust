use super::*;

pub(crate) fn refresh_creep_and_sunken(
    archive: &mut Archive<std::io::Cursor<Vec<u8>>>,
    files: &mut Files,
    assets: &mut AssetManifest,
    rules: &Rules,
) -> Result<()> {
    use straterust_engine::assets::{CreepManifest, ProjectileManifest};
    let palette = formats::palette(&archive.read_file("tileset\\badlands.wpe", 1024)?)?;
    if rules.units.iter().any(|unit| unit.creep_radius.is_some()) {
        let cv5 = archive.read_file("tileset\\badlands.cv5", 1024 * 1024)?;
        let vx4 = archive.read_file("tileset\\badlands.vx4", 4 * 1024 * 1024)?;
        let vr4 = archive.read_file("tileset\\badlands.vr4", 8 * 1024 * 1024)?;
        ensure!(cv5.len() >= 104, "missing creep tile group");
        let mut tiles = Vec::new();
        for variant in 0..13 {
            let image = formats::decode_tile(
                &vx4,
                &vr4,
                &palette,
                usize::from(word(&cv5, 52 + 20 + variant * 2)),
            )?;
            tiles.push(crate::add_image(
                files,
                &format!("creep-tile-{variant:02}.srim"),
                &image,
            )?);
        }
        let decoded = formats::decode_grp(
            &archive.read_file("tileset\\badlands.grp", 8 * 1024 * 1024)?,
            &palette,
        )?;
        let edges = decoded
            .iter()
            .enumerate()
            .map(|(index, image)| {
                crate::add_image(files, &format!("creep-edge-{index:02}.srim"), image)
            })
            .collect::<Result<Vec<_>>>()?;
        let patterns = (0..256_u16)
            .map(|mask| {
                let mut v = 0_u8;
                if mask & 2 != 0 {
                    v |= 0x10;
                }
                if mask & 8 != 0 {
                    v |= 0x24;
                }
                if mask & 0x10 != 0 {
                    v |= 9;
                }
                if mask & 0x40 != 0 {
                    v |= 2;
                }
                if mask & 0xc0 == 0xc0 {
                    v |= 1;
                }
                if mask & 0x60 == 0x60 {
                    v |= 4;
                }
                if mask & 3 == 3 {
                    v |= 0x20;
                }
                if mask & 6 == 6 {
                    v |= 8;
                }
                if v & 0x21 == 0x21 || v & 0xc == 0xc {
                    v |= 0x40;
                }
                v
            })
            .collect::<Vec<_>>();
        let ordered = patterns
            .iter()
            .copied()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let mask_frames = patterns
            .iter()
            .map(|pattern| ordered.iter().position(|value| value == pattern).unwrap() as u8)
            .collect();
        assets.creep = Some(CreepManifest {
            tiles,
            edges,
            mask_frames,
        });
    }
    if rules.units.iter().any(|unit| unit.id == UnitTypeId(41)) {
        let body = formats::decode_grp(
            &archive.read_file("unit\\zerg\\Lurker.grp", 8 * 1024 * 1024)?,
            &palette,
        )?;
        let scripts = archive.read_file("scripts\\iscript.bin", 65536)?;
        let poses = pose_frames(&scripts, 46, 5);
        ensure!(
            !poses.is_empty() && poses.iter().all(|&frame| usize::from(frame) < body.len()),
            "missing Sunken attack poses"
        );
        let burst = formats::decode_grp(
            &archive.read_file("unit\\thingy\\zBldDthS.grp", 8 * 1024 * 1024)?,
            &palette,
        )?;
        let rubble = formats::decode_grp(
            &archive.read_file("unit\\thingy\\ZRubbleS.grp", 8 * 1024 * 1024)?,
            &palette,
        )?;
        let start = body.len() as u16;
        let rubble_start = start + burst.len() as u16;
        let death = (start..rubble_start)
            .chain(
                (rubble_start..rubble_start + rubble.len() as u16)
                    .flat_map(|frame| std::iter::repeat_n(frame, 10)),
            )
            .collect::<Vec<_>>();
        let frames = body
            .into_iter()
            .chain(burst)
            .chain(rubble)
            .collect::<Vec<_>>();
        let sprite = terran::compact_sprite(
            files,
            41,
            "Zerg Sunken Colony",
            "sunken-colony",
            &frames,
            vec![
                terran::single_direction(ClipKind::Idle, &[0, 0, 1, 1, 2, 2], 42),
                terran::single_direction(ClipKind::Construction, &[0, 1, 2], 42),
                terran::single_direction(
                    ClipKind::Attack,
                    &poses
                        .into_iter()
                        .flat_map(|frame| [frame, frame])
                        .collect::<Vec<_>>(),
                    42,
                ),
                terran::single_direction(ClipKind::Death, &death, 150),
            ],
        )?;
        let target = assets
            .extra_units
            .iter_mut()
            .find(|sprite| sprite.unit_type == UnitTypeId(41))
            .context("missing Sunken sprite mapping")?;
        *target = sprite;
        let hit = formats::decode_grp(
            &archive.read_file("unit\\thingy\\ecaHit.grp", 8 * 1024 * 1024)?,
            &palette,
        )?;
        let effect = EffectManifest {
            frame_ms: 42,
            anchor: [hit[0].width as i32 / 2, hit[0].height as i32 / 2],
            frames: hit
                .iter()
                .enumerate()
                .map(|(index, image)| {
                    crate::add_image(files, &format!("sunken-hit-{index:02}.srim"), image)
                })
                .collect::<Result<Vec<_>>>()?,
            sequence: (0..hit.len() as u16)
                .flat_map(|frame| [frame, frame])
                .collect(),
        };
        assets
            .projectiles
            .retain(|projectile| projectile.unit_type != UnitTypeId(41));
        assets.projectiles.push(ProjectileManifest {
            targets_air: false,
            directional: false,
            unit_type: UnitTypeId(41),
            speed_fp8: 256,
            forward_offset: 0,
            arc_height: 0,
            on_target: true,
            flight: effect.clone(),
            impact: effect,
        });
    }
    files.insert("creep-sunken-reference.ron".into(), ron_bytes(&(
        "Creep uses Badlands CV5 group1 variants0..12 and tileset/badlands.grp with OpenBW's 8-neighbor edge mapping. Providers cover source ellipse320x200; other Zerg buildings sustain foundation tiles. Completed providers fill their bounds immediately; unsupported tiles recede over128ticks. Source RNG growth/recession timing is uncalibrated.",
        "Sunken146 image76 Lurker.grp/script46 uses trgtarccondjmp branches before its attack poses; default branch imported. Weapon53 -> flingy143/sprite346/image531 ecaHit.grp/script241 appears on target. Death uses zBldDthS/ZRubbleS; rubble duration remains shortened."))?);
    Ok(())
}
