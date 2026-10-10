//! Original missile, fire, death and resource-carrier artwork.
use super::{gfx, war::WarArchive};
use anyhow::Result;
use std::path::Path;
use straterust_engine::{assets::*, map::MovementClass, sim::Rules};

pub(super) fn effect(
    archive: &WarArchive,
    root: &Path,
    palette: &gfx::Palette,
    index: usize,
) -> Result<EffectManifest> {
    let images = gfx::sprites(&archive.entry(index)?, palette)?;
    Ok(EffectManifest {
        frame_ms: 66,
        anchor: [images[0].width as i32 / 2, images[0].height as i32 / 2],
        frames: images
            .iter()
            .map(|i| gfx::write_image(root, i))
            .collect::<Result<_>>()?,
        sequence: (0..images.len() as u16).collect(),
    })
}
pub(super) fn append(
    root: &Path,
    sprite: &mut SpriteManifest,
    kind: ClipKind,
    effect: &EffectManifest,
) -> Result<()> {
    let offset = sprite.frames.len() as u16;
    for reference in &effect.frames {
        let image = decode_image(&std::fs::read(root.join(&reference.file))?)?;
        sprite.frames.push(gfx::write_image(
            root,
            &gfx::pad_for_anchor(&image, sprite.anchor),
        )?);
    }
    sprite.clips.push(SpriteClip {
        kind,
        frame_ms: effect.frame_ms,
        directions: 1,
        frames: effect
            .sequence
            .iter()
            .map(|i| ClipFrame {
                frame: offset + i,
                flip_x: false,
                offset: [
                    (sprite.anchor[0] - effect.anchor[0]) as i16,
                    (sprite.anchor[1] - effect.anchor[1]) as i16,
                ],
            })
            .collect(),
        key_steps: Vec::new(),
        loop_start: None,
        progress_starts: Vec::new(),
    });
    Ok(())
}

// Sites are source artwork, drawn by the existing finite death clip below living
// units. Pad both phases to one centred canvas; the scar heals over three seconds.
fn building_death(
    archive: &WarArchive,
    root: &Path,
    palette: &gfx::Palette,
    explosion: &EffectManifest,
    record: usize,
) -> Result<EffectManifest> {
    let blasts = explosion
        .frames
        .iter()
        .map(|r| decode_image(&std::fs::read(root.join(&r.file))?))
        .collect::<Result<Vec<_>>>()?;
    let ruins = gfx::sprites(&archive.entry(record)?, palette)?;
    let width = blasts.iter().chain(&ruins).map(|i| i.width).max().unwrap();
    let height = blasts.iter().chain(&ruins).map(|i| i.height).max().unwrap();
    let pad = |source: &Image, alpha: u32| {
        let mut image = Image {
            width,
            height,
            rgba: vec![0; (width * height * 4) as usize],
        };
        for y in 0..source.height {
            for x in 0..source.width {
                let start = ((y * source.width + x) * 4) as usize;
                let dest =
                    (((y + (height - source.height) / 2) * width + x + (width - source.width) / 2)
                        * 4) as usize;
                image.rgba[dest..dest + 4].copy_from_slice(&source.rgba[start..start + 4]);
                image.rgba[dest + 3] = (u32::from(image.rgba[dest + 3]) * alpha / 45) as u8;
            }
        }
        image
    };
    let mut frames = blasts
        .iter()
        .map(|i| gfx::write_image(root, &pad(i, 45)))
        .collect::<Result<Vec<_>>>()?;
    for step in 0..45 {
        frames.push(gfx::write_image(
            root,
            &pad(&ruins[step * ruins.len() / 45], 45 - step as u32),
        )?);
    }
    Ok(EffectManifest {
        frame_ms: explosion.frame_ms,
        anchor: [width as i32 / 2, height as i32 / 2],
        sequence: (0..frames.len() as u16).collect(),
        frames,
    })
}

pub fn add(
    archive: &WarArchive,
    root: &Path,
    palette: &gfx::Palette,
    era: usize,
    rules: &Rules,
    assets: &mut AssetManifest,
) -> Result<()> {
    let explosion = effect(archive, root, palette, 347)?;
    let shadow = effect(archive, root, palette, 353)?;
    let small_death = building_death(
        archive,
        root,
        palette,
        &explosion,
        [189, 190, 188, 524][era],
    )?;
    let large_death = building_death(
        archive,
        root,
        palette,
        &explosion,
        [121, 163, 191, 512][era],
    )?;
    for sprite in &mut assets.extra_units {
        let unit = rules
            .units
            .iter()
            .find(|u| u.id == sprite.unit_type)
            .unwrap();
        if unit.structure && unit.blocks_movement && unit.movement_class == MovementClass::Ground {
            let small = unit.footprint.width <= 64 && unit.footprint.height <= 64;
            append(
                root,
                sprite,
                ClipKind::Death,
                if small { &small_death } else { &large_death },
            )?;
        } else if unit.structure && unit.blocks_movement {
            append(root, sprite, ClipKind::Death, &explosion)?;
        }
        if !unit.structure && matches!(unit.id.0 - 1, 4 | 5 | 40 | 41) {
            append(root, sprite, ClipKind::Death, &explosion)?;
        }
        if unit.movement_class == MovementClass::Air {
            append(root, sprite, ClipKind::Shadow, &shadow)?;
        }
    }
    let small = effect(archive, root, palette, 343)?;
    let large = effect(archive, root, palette, 344)?;
    assets.damage_effects = Some(DamageEffectsManifest {
        styles: Vec::new(),
        small: [small.clone(), small.clone(), small],
        large: [large.clone(), large.clone(), large],
        units: rules
            .units
            .iter()
            .filter(|u| u.structure && u.blocks_movement)
            .map(|u| UnitEffectManifest {
                style: 0,
                unit_type: u.id,
                spots: vec![EffectSpot {
                    offset: [0, -16],
                    variant: 0,
                }],
            })
            .collect(),
    });
    let empty = gfx::write_image(
        root,
        &Image {
            width: 1,
            height: 1,
            rgba: vec![0; 4],
        },
    )?;
    let empty = EffectManifest {
        frame_ms: 33,
        anchor: [0, 0],
        frames: vec![empty],
        sequence: vec![0],
    };
    for unit in &rules.units {
        let source = usize::from(unit.id.0 - 1);
        let Some(weapon) = &unit.weapon else {
            continue;
        };
        let Some((index, impact, arc, directional)) = (match source {
            4 => Some((338, 345, 0, true)),
            5 => Some((337, 345, 32, false)),
            8 | 18 | 20 | 96 => Some((339, 0, 0, true)),
            9 | 19 | 53 | 97 => Some((340, 0, 0, true)),
            10 | 24 => Some((324, 346, 0, true)),
            11 | 21 | 51 => Some((334, 346, 0, true)),
            22 | 42 => Some((325, 346, 0, true)),
            35 | 43 => Some((326, 346, 0, true)),
            30 | 31 => Some((348, 350, 32, false)),
            32 | 33 => Some((348, 350, 32, false)),
            98 | 99 => Some((331, 350, 0, true)),
            56 => Some((351, 346, 0, true)),
            38 => Some((341, 349, 0, true)),
            39 => Some((342, 349, 0, true)),
            _ => None,
        }) else {
            continue;
        };
        let mut flight = effect(archive, root, palette, index)?;
        if directional {
            self::directional(&mut flight);
        }
        let impact = if impact == 0 {
            empty.clone()
        } else {
            effect(archive, root, palette, impact)?
        };
        for air in [false, true] {
            if air && !weapon.targets_air {
                continue;
            }
            assets.projectiles.push(ProjectileManifest {
                ability: None,
                unit_type: unit.id,
                targets_air: air,
                directional,
                speed_fp8: weapon.projectile_speed.max(256),
                forward_offset: 12,
                launch_offsets: Vec::new(),
                arc_height: arc,
                on_target: false,
                charge: None,
                marker: None,
                flight: flight.clone(),
                impact: impact.clone(),
                trail: None,
            });
        }
    }
    for (source, index, kind) in [
        (2, 122, "wood"),
        (3, 123, "wood"),
        (2, 124, "gold"),
        (3, 125, "gold"),
    ] {
        let images = gfx::sprites(&archive.entry(index)?, palette)?;
        let frames = images
            .iter()
            .map(|i| gfx::write_image(root, i))
            .collect::<Result<Vec<_>>>()?;
        for carrier in [source, source + 14] {
            let base = assets
                .extra_units
                .iter()
                .find(|s| usize::from(s.unit_type.0 - 1) == carrier)
                .unwrap();
            let mut full = SpriteManifest {
                unit_type: base.unit_type,
                unit_name: base.unit_name.clone(),
                frame_ms: 100,
                anchor: [images[0].width as i32 / 2, images[0].height as i32 / 2],
                frames: frames.clone(),
                clips: vec![
                    super::art::clip(ClipKind::Idle, &[0], true, 100),
                    super::art::clip(
                        ClipKind::Walk,
                        &(0..(images.len() / 5).min(5)).collect::<Vec<_>>(),
                        true,
                        100,
                    ),
                ],
            };
            // Loaded artwork contains movement poses only. Reuse the body's
            // tool/repair poses when a carrying worker starts working again.
            if let Some(work) = base.clips.iter().find(|c| c.kind == ClipKind::Work) {
                let offset = full.frames.len() as u16;
                full.frames.extend(base.frames.iter().cloned());
                let mut work = work.clone();
                for frame in &mut work.frames {
                    frame.frame += offset;
                }
                full.clips.push(work);
            }
            assets.carried_resources.push(CarriedResourceManifest {
                replaces_body: true,
                kind: kind.into(),
                full_amount: 100,
                full,
                partial: if kind == "wood" {
                    Some(base.clone())
                } else {
                    None
                },
            });
        }
    }
    super::spell_art::add(archive, root, palette, rules, assets)?;
    Ok(())
}

/// Expand the five stored headings for each pose into the renderer's seventeen
/// mirrored headings, retaining every animation pose instead of cycling headings.
pub(super) fn directional(effect: &mut EffectManifest) {
    let originals = effect.frames.clone();
    let poses = originals.len().div_ceil(5);
    effect.frames = (0..poses)
        .flat_map(|pose| (0..17).map(move |heading| (pose, heading)))
        .map(|(pose, heading)| {
            originals[(pose * 5 + (heading + 2) / 4).min(originals.len() - 1)].clone()
        })
        .collect();
    effect.sequence = (0..poses as u16).collect();
}
