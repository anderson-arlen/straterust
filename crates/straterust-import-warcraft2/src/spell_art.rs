//! Native spell effects use the original missile artwork and shared renderer.
use super::{effects::effect, gfx, war::WarArchive};
use anyhow::Result;
use std::path::Path;
use straterust_engine::{assets::*, sim::*};

pub fn add(
    archive: &WarArchive,
    root: &Path,
    palette: &gfx::Palette,
    rules: &Rules,
    assets: &mut AssetManifest,
) -> Result<()> {
    let empty = EffectManifest {
        frame_ms: 33,
        anchor: [0, 0],
        frames: vec![gfx::write_image(
            root,
            &Image {
                width: 1,
                height: 1,
                rgba: vec![0; 4],
            },
        )?],
        sequence: vec![0],
    };
    let statuses = status_images(archive, palette)?
        .iter()
        .map(|image| gfx::write_image(root, image))
        .collect::<Result<Vec<_>>>()?;
    for unit in &rules.units {
        for ability in &unit.abilities {
            let number = ability.id.0;
            let (record, continuous, directional) = match number {
                1 | 4 | 14 => (346, false, false),
                2 => (333, false, false),
                3 => (332, false, false),
                5 | 8 | 10 | 11 | 15 | 16 => (346, false, false),
                6 => (335, true, false),
                7 => (327, false, true),
                9 => (328, true, false),
                12 => (329, true, false),
                13 => (334, false, true),
                17 => (336, true, false),
                18 => (330, true, false),
                19 => (347, false, false),
                _ => continue,
            };
            let mut art = effect(archive, root, palette, record)?;
            if directional {
                super::effects::directional(&mut art);
            }
            let on_target = !directional;
            let mut projectile = ProjectileManifest {
                pattern: pattern(number),
                unit_type: unit.id,
                ability: Some(ability.id),
                targets_air: false,
                directional,
                speed_fp8: 16 * 256,
                forward_offset: 12,
                launch_offsets: Vec::new(),
                arc_height: 0,
                on_target,
                charge: None,
                marker: None,
                trail: None,
                flight: if continuous || directional {
                    art.clone()
                } else {
                    empty.clone()
                },
                impact: if continuous {
                    empty.clone()
                } else if directional {
                    effect(archive, root, palette, 347)?
                } else {
                    art
                },
            };
            configure(&mut projectile);
            status(&mut projectile, &statuses);
            assets.projectiles.push(projectile);
        }
    }
    Ok(())
}

/// All required source effects already exist in older shared installations.
pub(super) fn upgrade(assets: &mut AssetManifest) {
    let normal = assets
        .projectiles
        .iter()
        .find(|p| p.ability == Some(AbilityId(5)))
        .map(|p| p.impact.clone());
    let blast = assets
        .projectiles
        .iter()
        .find(|p| p.ability == Some(AbilityId(19)))
        .map(|p| p.impact.clone());
    for p in &mut assets.projectiles {
        if let Some(id) = p.ability {
            p.pattern = pattern(id.0);
        }
        if matches!(p.ability, Some(AbilityId(1 | 4 | 14)))
            && let Some(art) = &normal
        {
            p.impact = art.clone();
        }
        if (p.ability == Some(AbilityId(7))
            || p.ability.is_none() && matches!(p.unit_type.0, 23 | 43 | 36 | 44))
            && let Some(art) = &blast
        {
            p.impact = art.clone();
        }
        configure(p);
    }
}

fn pattern(number: u16) -> Option<EffectPattern> {
    match number {
        9 => Some(EffectPattern::Orbit {
            count: 5,
            radius: 32,
            period_ms: 1188,
            phase_degrees: 210,
            spacing_degrees: -70,
        }),
        12 => Some(EffectPattern::Bombardment {
            count: 5,
            spread: 64,
            fall_height: 128,
            fall_ms: 396,
            cycle_ms: 495,
        }),
        18 => Some(EffectPattern::Bombardment {
            count: 5,
            spread: 64,
            fall_height: 0,
            fall_ms: 0,
            cycle_ms: 264,
        }),
        _ => None,
    }
}

/// Some native records contain travelling headings followed by impact poses.
/// Keep those phases separate so rings and shattered ice never fly as missiles.
pub(super) fn configure(p: &mut ProjectileManifest) {
    if p.ability.is_some() || matches!(p.unit_type.0, 12 | 22 | 23 | 36 | 43 | 44 | 52) {
        p.flight.frame_ms = if p.ability == Some(AbilityId(6)) {
            165
        } else {
            33
        };
        p.impact.frame_ms = if matches!(
            p.ability,
            Some(AbilityId(1 | 4 | 5 | 8 | 10 | 11 | 14 | 15 | 16))
        ) {
            165
        } else {
            33
        };
    }
    if p.ability == Some(AbilityId(9)) {
        p.flight.anchor = [16, 32];
    }
    if p.ability == Some(AbilityId(13))
        || p.ability.is_none() && matches!(p.unit_type.0, 12 | 22 | 52)
    {
        if p.flight.frames.len() >= 102 {
            p.impact = p.flight.clone();
            p.impact.sequence = (1..6).map(|pose| pose * 17).collect();
            p.flight.sequence = vec![0];
        }
    } else if p.ability == Some(AbilityId(12)) {
        p.impact = p.flight.clone();
        p.impact.sequence = vec![2, 3];
        p.flight.sequence = vec![0, 1];
    }
}

/// Native status decorations: blood drop, haste, slow, invisible, armour.
pub(super) fn status(p: &mut ProjectileManifest, images: &[ImageRef]) {
    let index = match p.ability.map(|id| id.0) {
        Some(5) => 0,
        Some(15) => 1,
        Some(8) => 2,
        Some(10) => 3,
        Some(16) => 4,
        _ => return,
    };
    p.marker = Some(EffectManifest {
        frame_ms: 165,
        anchor: [8, 8],
        sequence: vec![0],
        frames: vec![images[index].clone()],
    });
}

pub(super) fn status_images(archive: &WarArchive, palette: &gfx::Palette) -> Result<Vec<Image>> {
    let mut palette = *palette;
    palette[0] = [0; 4];
    let images = gfx::raw_sprites(&archive.entry(323)?, &palette)?;
    anyhow::ensure!(
        images.len() == 5,
        "expected five native spell status symbols"
    );
    Ok(images)
}
