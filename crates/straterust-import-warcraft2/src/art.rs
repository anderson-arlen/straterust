//! Source sprite assignments and directional clips, published as native art.
use super::{gfx, stats, terrain::Tileset, war::WarArchive};
use anyhow::{Context, Result};
use std::path::Path;
use straterust_engine::{assets::*, sim::Rules};

pub fn graphic(unit: usize, era: usize) -> Option<usize> {
    let alias = match unit {
        12 => 6,
        13 => 7,
        16 => 2,
        17 => 3,
        18 | 20 => 8,
        19 | 53 => 9,
        21 | 51 => 11,
        22 => 42,
        23 | 49 => 7,
        24 => 10,
        25 | 47 => 1,
        35 => 43,
        36 => 26,
        37 => 27,
        44 | 50 | 52 => 6,
        46 => 0,
        _ => unit,
    };
    let result = match alias {
        0..=10 => 45 + alias,
        11 => 58,
        14 | 15 => 33 + alias - 14,
        26 | 27 => 59 + alias - 26,
        28 | 29 => 39 + alias - 28,
        30 | 31 => 61 + alias - 30,
        32 | 33 => 41 + alias - 32,
        38 | 39 => {
            if era == 2 {
                182 + alias - 38
            } else {
                43 + alias - 38
            }
        }
        40 => 38,
        41 => 63,
        42 | 43 => 35 + alias - 42,
        45 => 37,
        55 | 56 => 69 + alias - 55,
        57 => [64, 66, 65, 470][era],
        58..=91 => {
            let pair = alias % 2;
            let base = match alias - pair {
                58 => 92,
                60 => 94,
                62 => 96,
                64 => 98,
                66 => 104,
                68 => 90,
                70 => 88,
                72 => 108,
                74 => 100,
                76 => 102,
                78 => 110,
                80 => 84,
                82 => 106,
                84 => 112,
                86 => 114,
                88 => 86,
                90 => 116,
                _ => return None,
            };
            match era {
                1 => match alias - pair {
                    80 => 160 + pair,
                    90 => 158 + pair,
                    _ => base + 42 + pair,
                },
                2 => match alias - pair {
                    58 => 173 + pair,
                    76 => 175 + pair,
                    86 => 177 + pair,
                    _ => base + pair,
                },
                3 => match alias - pair {
                    88 => 473 + pair,
                    70 => 475 + pair,
                    68 => 477 + pair,
                    58 => 479 + pair,
                    60 => 481 + pair,
                    62 => 483 + pair,
                    64 => 485 + pair,
                    74 => 487 + pair,
                    76 => 489 + pair,
                    66 => 491 + pair,
                    82 => 493 + pair,
                    72 => 495 + pair,
                    78 => 497 + pair,
                    84 => 499 + pair,
                    86 => 501 + pair,
                    90 => 503 + pair,
                    80 => 505 + pair,
                    _ => return None,
                },
                _ => base + pair,
            }
        }
        92 => [119, 162, 179, 511][era],
        93 => [118, 118, 180, 515][era],
        96 | 97 => [80, 169, 80, 507][era] + alias - 96,
        98 | 99 => [82, 171, 82, 509][era] + alias - 98,
        100 => 166,
        101 => [167, 184, 185, 513][era],
        102 => [181, 186, 181, 514][era],
        _ => return None,
    };
    Some(result)
}

pub(super) fn clip(kind: ClipKind, poses: &[usize], directions: bool, frame_ms: u32) -> SpriteClip {
    let mut frames = Vec::new();
    for pose in poses {
        for heading in 0..if directions { 32 } else { 1 } {
            let facing = (heading + 2) / 4 % 8;
            frames.push(ClipFrame {
                frame: (if directions {
                    pose * 5 + if facing <= 4 { facing } else { 8 - facing }
                } else {
                    *pose
                }) as u16,
                flip_x: directions && facing > 4,
                offset: [0, 0],
            });
        }
    }
    SpriteClip {
        kind,
        directions: if directions { 32 } else { 1 },
        frame_ms,
        frames,
        key_steps: Vec::new(),
        loop_start: None,
        progress_starts: Vec::new(),
    }
}

pub fn sprite(
    archive: &WarArchive,
    root: &Path,
    palette: &gfx::Palette,
    unit: usize,
    era: usize,
    name: &str,
    structure: bool,
) -> Result<SpriteManifest> {
    let index = graphic(unit, era).with_context(|| format!("no source artwork for unit {unit}"))?;
    let images = gfx::sprites(&archive.entry(index)?, palette)
        .with_context(|| format!("sprite {index} for {name}"))?;
    let width = images[0].width as i32;
    let height = images[0].height as i32;
    let count = images.len();
    let directional = !structure && count >= 5 && count.is_multiple_of(5);
    let poses = if directional { count / 5 } else { count };
    let clips = super::animation::clips(index, poses, directional, structure);
    let mut sprite = SpriteManifest {
        unit_type: stats::id(unit),
        unit_name: if name.trim().is_empty() {
            format!("Unit {unit}")
        } else {
            name.trim().into()
        },
        frame_ms: 100,
        anchor: [width / 2, height / 2],
        frames: images
            .iter()
            .map(|image| gfx::write_image(root, image))
            .collect::<Result<_>>()?,
        clips,
    };
    if structure {
        super::construction::add(archive, root, palette, &mut sprite, unit, era)?;
    }
    Ok(sprite)
}

pub fn assets(
    archive: &WarArchive,
    root: &Path,
    set: &Tileset,
    grid: TerrainGrid,
    rules: &Rules,
    names: &[String],
    era: usize,
) -> Result<AssetManifest> {
    let mut sprites = Vec::new();
    for unit in &rules.units {
        let source = usize::from(unit.id.0 - 1);
        sprites.push(sprite(
            archive,
            root,
            &set.palette,
            source,
            era,
            &names[source + 1],
            unit.structure,
        )?);
    }
    let base = sprites.remove(0);
    let gold = sprite(archive, root, &set.palette, 92, era, "Gold Mine", true)?;
    let oil = sprite(archive, root, &set.palette, 93, era, "Oil Patch", true)?;
    let mut assets = AssetManifest {
        console_layout: None,
        player_colors: Default::default(),
        schema_version: 1,
        terrain: gfx::write_image(root, &set.atlas)?,
        terrain_grid: Some(grid),
        unit_type: base.unit_type,
        unit_name: base.unit_name,
        frame_ms: base.frame_ms,
        anchor: base.anchor,
        frames: base.frames,
        clips: base.clips,
        extra_units: sprites,
        resources: vec![
            ResourceManifest {
                positions: Vec::new(),
                kind: "gold".into(),
                anchor: gold.anchor,
                image: gold.frames[0].clone(),
                active_image: Some(gold.frames[1].clone()),
                terrain: false,
                terrain_edges: None,
                depleted_image: None,
                selection_circle: None,
                selection_y: 0,
            },
            ResourceManifest {
                positions: Vec::new(),
                kind: "oil".into(),
                anchor: oil.anchor,
                image: oil.frames[0].clone(),
                active_image: None,
                terrain: false,
                terrain_edges: None,
                depleted_image: None,
                selection_circle: None,
                selection_y: 0,
            },
        ],
        carried_resources: Vec::new(),
        ui: Vec::new(),
        map_images: Vec::new(),
        scan_effect: None,
        projectiles: Vec::new(),
        damage_effects: None,
        gas_effects: None,
        creep: None,
        indicators: None,
    };
    super::effects::add(archive, root, &set.palette, era, rules, &mut assets)?;
    super::icons::add(archive, root, &set.palette, era, rules, &mut assets)?;
    Ok(assets)
}
