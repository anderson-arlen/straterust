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
    for unit in &rules.units {
        for ability in &unit.abilities {
            let number = ability.id.0;
            let (record, continuous, directional) = match number {
                1 | 4 | 14 => (352, false, false),
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
            assets.projectiles.push(ProjectileManifest {
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
                    effect(archive, root, palette, 346)?
                } else {
                    art
                },
            });
        }
    }
    Ok(())
}
