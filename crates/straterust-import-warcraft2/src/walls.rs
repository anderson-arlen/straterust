//! Destructible source wall tiles use ordinary structure combat and collision.
use super::{art, effects, gfx, terrain::Tileset, war::WarArchive};
use anyhow::Result;
use std::{collections::BTreeSet, path::Path};
use straterust_engine::{assets::*, map::*, sim::*};

pub fn add(
    archive: &WarArchive,
    set: &Tileset,
    root: &Path,
    rules: &mut Rules,
    map: &mut Map,
    terrain: &mut Terrain,
    assets: &mut AssetManifest,
) -> Result<()> {
    let grid = assets.terrain_grid.as_mut().expect("source map tiles");
    let mut variants = BTreeSet::new();
    // Original tables group eighteen connected wall shapes per race, followed
    // by damaged shapes and fourteen rubble tiles. Rubble is passable terrain.
    const RUBBLE: [u32; 18] = [
        88, 89, 90, 91, 92, 94, 99, 93, 94, 95, 96, 97, 100, 97, 98, 99, 100, 101,
    ];
    for (cell, tile) in grid.tiles.clone().into_iter().enumerate() {
        if !(16..=101).contains(&tile) {
            continue;
        }
        terrain.flags[cell] = WALKABLE | BUILDABLE;
        if tile >= 88 {
            continue;
        }
        let shape = if tile >= 52 { tile - 36 } else { tile };
        grid.tiles[cell] = RUBBLE[((shape - 16) % 18) as usize];
        let unit_type = UnitTypeId(1000 + tile as u16);
        map.spawns.push(Spawn {
            owner: PlayerId(15),
            unit_type,
            position: Position {
                x: cell as i32 % grid.columns as i32 * 32 + 16,
                y: cell as i32 / grid.columns as i32 * 32 + 16,
            },
            ..Default::default()
        });
        variants.insert(tile);
    }
    if variants.is_empty() {
        return Ok(());
    }
    let explosion = effects::effect(archive, root, &set.palette, 347)?;
    for tile in variants {
        let unit_type = UnitTypeId(1000 + tile as u16);
        rules.units.push(UnitType {
            id: unit_type,
            structure: true,
            speed: 0,
            blocks_movement: true,
            max_hp: if tile >= 52 { 20 } else { 40 },
            armor: 20,
            vision_range: 0,
            footprint: Footprint {
                width: 32,
                height: 32,
            },
            placement: Footprint {
                width: 32,
                height: 32,
            },
            ..Default::default()
        });
        let mut sprite = SpriteManifest {
            unit_type,
            unit_name: "Wall".into(),
            frame_ms: 100,
            anchor: [16, 16],
            frames: vec![gfx::write_image(root, &set.tile_image(tile))?],
            clips: vec![art::clip(ClipKind::Idle, &[0], false, 100)],
        };
        effects::append(root, &mut sprite, ClipKind::Death, &explosion)?;
        assets.extra_units.push(sprite);
    }
    Ok(())
}
