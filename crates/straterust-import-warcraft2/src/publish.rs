use super::{
    art, gfx,
    pud::{Pud, word},
    source::Source,
    stats,
    terrain::Tileset,
};
use anyhow::{Context, Result, ensure};
use serde::Serialize;
use std::{collections::BTreeMap, fs, path::Path};
use straterust_engine::{
    assets::*,
    content::{Campaign, CampaignMission, Package},
    map::encode_terrain,
    menus::{MenuCampaign, MenuManifest},
    sim::*,
};

pub fn ron<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    fs::write(
        path,
        ron::ser::to_string_pretty(value, ron::ser::PrettyConfig::default())?,
    )?;
    Ok(())
}

pub fn publish(source: &mut Source, output: &Path, progress: &dyn Fn(&str)) -> Result<()> {
    let names = stats::names(&source.data.read_file("rez\\stat_txt.tbl", 1024 * 1024)?)?;
    let objectives = stats::names(&source.data.read_file("rez\\objctivs.tbl", 1024 * 1024)?)?;
    let rules = stats::rules(&source.main.entry(472)?)?;
    fs::create_dir_all(output)?;
    let cache = tempfile::Builder::new()
        .prefix(".native-cache-")
        .tempdir_in(output)?;
    let mut sets = Vec::new();
    let mut templates = Vec::new();
    for era in 0..4 {
        progress(&format!(
            "Converting Warcraft II tileset {} and unit artwork",
            era + 1
        ));
        let set = Tileset::load(&source.main, era)?;
        let directory = cache.path().join(era.to_string());
        fs::create_dir(&directory)?;
        let template = art::assets(
            &source.main,
            &directory,
            &set,
            TerrainGrid {
                tile_size: 32,
                columns: 1,
                rows: 1,
                tiles: vec![16],
            },
            &rules,
            &names,
            era,
        )?;
        sets.push(set);
        templates.push(template);
    }
    progress("Converting Warcraft II music and voices");
    let audio_root = cache.path().join("audio");
    fs::create_dir(&audio_root)?;
    let media = super::media::Library::load(source, &audio_root, &rules)?;
    let mut menus = MenuManifest::basic("Warcraft II", true);
    menus.campaigns.clear();
    for (folder, race, expansion, count) in [
        ("human", 0, false, 14),
        ("orc", 1, false, 14),
        ("human-expansion", 0, true, 12),
        ("orc-expansion", 1, true, 12),
    ] {
        let root = output.join(folder);
        fs::create_dir_all(&root)?;
        let mut campaign = Campaign {
            schema_version: 1,
            id: format!("warcraft2.{folder}"),
            missions: Vec::new(),
        };
        for number in 1..=count {
            let index = if expansion { 446 } else { 192 } + (number - 1) * 2 + race;
            let mut pud = Pud::decode(&source.main.entry(index)?)
                .with_context(|| format!("{folder} mission {number}"))?;
            let unit_data = pud.chunks.get(b"UDTA").context("missing unit data")?;
            let mut rules = if word(unit_data, 0)? == 0 {
                stats::rules(&unit_data[2..])?
            } else {
                rules.clone()
            };
            let package = format!("mission{number:02}");
            let text_index = if expansion { 28 } else { 0 } + (number - 1) * 2 + race;
            let title = if pud.title.is_empty() || pud.title == "No Description" {
                objectives
                    .get(59 + text_index)
                    .context("missing campaign title")?
                    .clone()
            } else {
                pud.title.clone()
            };
            progress(&format!(
                "Converting {folder} mission {number}/{count}: {title}"
            ));
            let directory = root.join(&package);
            fs::create_dir_all(&directory)?;
            // This escort map stores the captives as ordinary peasants. Its
            // mission program gives only source player four the fighting form.
            if !expansion && race == 0 && number == 10 {
                for placed in &mut pud.units {
                    if placed.owner == 4 && placed.kind == 2 {
                        placed.kind = 16;
                    }
                }
            }
            let (grid, mut terrain, trees) = sets[pud.era].map(&pud)?;
            let mut assets = templates[pud.era].clone();
            assets.player_colors = super::colors::players(&pud, &sets[pud.era].palette);
            assets.resources.push(sets[pud.era].forest(&directory)?);
            assets.terrain_grid = Some(grid);
            for entry in fs::read_dir(cache.path().join(pud.era.to_string()))? {
                let entry = entry?;
                let path = directory.join(entry.file_name());
                if !path.exists() {
                    fs::hard_link(entry.path(), path)?;
                }
            }
            let mut map = map(&pud, &rules, &trees)?;
            let grid = assets.terrain_grid.as_ref().expect("authored terrain grid");
            let edges = assets
                .resources
                .iter()
                .find(|r| r.kind == "wood")
                .and_then(|r| r.terrain_edges.as_ref())
                .expect("authored forest edges");
            for resource in &mut map.resources {
                if resource.kind == "wood" {
                    let index = (resource.position.y / 32) as usize * grid.columns as usize
                        + (resource.position.x / 32) as usize;
                    resource.terrain_corners = edges.corners.get(&grid.tiles[index]).copied();
                    ensure!(
                        resource.terrain_corners.is_some(),
                        "forest cell has no native corner shape"
                    );
                }
            }
            let mut mission = super::missions::convert(&pud, number, expansion)?;
            super::availability::apply(&pud, number, expansion, &mut rules, &mut map, &mut mission);
            super::opponent::configure(&pud, number, expansion, &rules, &mut map);
            super::walls::add(
                &source.main,
                &sets[pud.era],
                &directory,
                &mut rules,
                &mut map,
                &mut terrain,
                &mut assets,
            )?;
            assets
                .extra_units
                .retain(|s| rules.units.iter().any(|u| u.id == s.unit_type));
            assets.projectiles.retain(|s| {
                rules.units.iter().any(|u| {
                    u.id == s.unit_type
                        && s.ability
                            .is_none_or(|a| u.abilities.iter().any(|v| v.id == a))
                })
            });
            if let Some(damage) = &mut assets.damage_effects {
                damage
                    .units
                    .retain(|s| rules.units.iter().any(|u| u.id == s.unit_type));
            }
            for unit in &rules.units {
                if unit.id != assets.unit_type
                    && !assets.extra_units.iter().any(|s| s.unit_type == unit.id)
                {
                    let source_id = usize::from(unit.id.0 - 1);
                    assets.extra_units.push(art::sprite(
                        &source.main,
                        &directory,
                        &sets[pud.era].palette,
                        source_id,
                        pud.era,
                        &names[source_id + 1],
                        unit.structure,
                    )?);
                }
            }
            super::indicators::add(
                &source.main,
                &directory,
                &sets[pud.era].palette,
                &rules,
                race,
                &mut assets,
            )?;
            super::console::add(&source.main, &directory, race, &mut assets)?;
            fs::write(
                directory.join("manifest.ron"),
                format!("(schema_version:1,id:{:?})", map.id),
            )?;
            ron(&directory.join("rules.ron"), &rules)?;
            ron(&directory.join("map.ron"), &map)?;
            ron(&directory.join("mission.ron"), &mission)?;
            fs::write(directory.join("terrain.srtm"), encode_terrain(&terrain)?)?;
            ron(&directory.join("assets.ron"), &assets)?;
            let objective = objectives
                .get(text_index)
                .context("missing mission objectives")?
                .clone();
            let presentation = super::presentation::build(&rules, &names, objective.clone());
            ron(&directory.join("presentation.ron"), &presentation)?;
            let mut native_media = media.mission(
                source,
                &directory,
                &audio_root,
                (race, number, expansion),
                &objective,
            )?;
            if let Some(death) = native_media
                .audio
                .iter()
                .find(|a| {
                    a.cue == straterust_engine::media::AudioCue::Death
                        && a.unit_type
                            .is_some_and(|id| rules.units.iter().any(|u| u.id == id && u.structure))
                })
                .cloned()
            {
                for wall in rules.units.iter().filter(|u| u.id.0 >= 1000) {
                    let mut mapping = death.clone();
                    mapping.unit_type = Some(wall.id);
                    native_media.audio.push(mapping);
                }
            }
            native_media.audio.retain(|a| {
                a.unit_type
                    .is_none_or(|id| rules.units.iter().any(|u| u.id == id))
            });
            ron(&directory.join("media.ron"), &native_media)?;
            let world = Package::load(&directory)
                .with_context(|| format!("validating {folder} mission {number}"))?
                .world(0)?;
            AssetPack::load(&directory)?
                .context("missing converted artwork")?
                .validate_for_world(&world)?;
            straterust_engine::media::MediaPack::load(&directory)?
                .context("missing converted media")?
                .validate_world(&world)?;
            campaign.missions.push(CampaignMission { title, package });
        }
        ron(&root.join("campaign.ron"), &campaign)?;
        menus.campaigns.push(MenuCampaign {
            title: format!(
                "{} - {}",
                if race == 0 { "Human" } else { "Orc" },
                if expansion {
                    "Beyond the Dark Portal"
                } else {
                    "Tides of Darkness"
                }
            ),
            directory: folder.into(),
        });
    }
    for reference in &media.menu {
        if !output.join(&reference.file).exists() {
            fs::hard_link(
                audio_root.join(&reference.file),
                output.join(&reference.file),
            )?;
        }
    }
    menus.music = media.menu;
    let menu_palette = gfx::palette(&source.interface.entry(14)?)?;
    let image = source.interface.entry(13)?;
    let (width, height) = (u32::from(word(&image, 0)?), u32::from(word(&image, 2)?));
    ensure!(
        width <= 2048 && height <= 2048 && image.len() == 4 + (width * height) as usize,
        "invalid menu background"
    );
    let rgba = image[4..]
        .iter()
        .flat_map(|i| menu_palette[usize::from(*i)])
        .collect();
    menus.background = Some(gfx::write_image(
        output,
        &Image {
            width,
            height,
            rgba,
        },
    )?);
    menus.validate()?;
    ron(&output.join("menus.ron"), &menus)?;
    Ok(())
}

fn map(pud: &Pud, rules: &Rules, trees: &[usize]) -> Result<Map> {
    let mut map = Map {
        id: format!(
            "warcraft2.{}",
            blake3::hash(pud.chunks.get(b"MTXM").unwrap()).to_hex()
        ),
        width: i32::from(pud.width) * 32,
        height: i32::from(pud.height) * 32,
        players: 16,
        spawns: Vec::new(),
        start_locations: Vec::new(),
        resources: Vec::new(),
        fog_of_war: true,
        terrain: None,
        mission: None,
        creation: BTreeMap::new(),
        initial_explored: BTreeMap::new(),
        ai: Vec::new(),
    };
    for placed in &pud.units {
        let kind = usize::from(placed.kind);
        let owner = PlayerId(u16::from(pud.player(placed.owner as usize)));
        if matches!(kind, 94 | 95) {
            map.start_locations.push(StartLocation {
                player: owner,
                position: Position {
                    x: i32::from(placed.x) * 32 + 16,
                    y: i32::from(placed.y) * 32 + 16,
                },
            });
        } else if matches!(kind, 92 | 93) {
            map.resources.push(ResourceSpawn {
                terrain_corners: None,
                kind: if kind == 92 { "gold" } else { "oil" }.into(),
                position: Position {
                    x: i32::from(placed.x) * 32 + 48,
                    y: i32::from(placed.y) * 32 + 48,
                },
                amount: u32::from(placed.data) * 2500,
                footprint: Footprint {
                    width: 96,
                    height: 96,
                },
                requires_extractor: kind == 93,
            });
        } else {
            let unit = rules
                .units
                .iter()
                .find(|u| u.id == stats::id(kind))
                .with_context(|| format!("unsupported map unit {kind}"))?;
            let area = unit.placement;
            map.spawns.push(Spawn {
                owner,
                unit_type: unit.id,
                invincible: kind == 100,
                position: Position {
                    x: i32::from(placed.x) * 32 + i32::from(area.width / 2),
                    y: i32::from(placed.y) * 32 + i32::from(area.height / 2),
                },
                ..Spawn::default()
            });
        }
    }
    for index in trees {
        map.resources.push(ResourceSpawn {
            terrain_corners: None,
            kind: "wood".into(),
            position: Position {
                x: (index % usize::from(pud.width)) as i32 * 32 + 16,
                y: (index / usize::from(pud.width)) as i32 * 32 + 16,
            },
            amount: 100,
            footprint: Footprint {
                width: 32,
                height: 32,
            },
            requires_extractor: false,
        });
    }
    // Prebuilt platforms carry their deposit amount in UNIT just like oil
    // patches. Append these after the legacy resources to preserve saved IDs.
    for placed in pud.units.iter().filter(|p| matches!(p.kind, 86 | 87)) {
        let position = Position {
            x: i32::from(placed.x) * 32 + 48,
            y: i32::from(placed.y) * 32 + 48,
        };
        if !map
            .resources
            .iter()
            .any(|r| r.kind == "oil" && r.position == position)
        {
            map.resources.push(ResourceSpawn {
                kind: "oil".into(),
                position,
                amount: u32::from(placed.data) * 2500,
                footprint: Footprint {
                    width: 96,
                    height: 96,
                },
                requires_extractor: true,
                terrain_corners: None,
            });
        }
    }
    Ok(map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pud::Placed;

    #[test]
    fn prebuilt_platforms_append_underlying_deposits_without_changing_existing_ids() {
        let mut rules =
            Package::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/fixtures"))
                .unwrap()
                .world(7)
                .unwrap()
                .rules()
                .clone();
        rules.units = [86, 87]
            .map(|source| UnitType {
                id: stats::id(source),
                structure: true,
                placement: Footprint {
                    width: 96,
                    height: 96,
                },
                ..Default::default()
            })
            .to_vec();
        let mut pud = Pud {
            chunks: BTreeMap::from([(*b"MTXM", vec![0; 32])]),
            width: 64,
            height: 64,
            era: 1,
            owners: [0; 16],
            sides: [0; 16],
            local: 0,
            title: String::new(),
            units: vec![
                Placed {
                    x: 10,
                    y: 10,
                    kind: 92,
                    owner: 15,
                    data: 40,
                },
                Placed {
                    x: 20,
                    y: 20,
                    kind: 86,
                    owner: 0,
                    data: 48,
                },
                Placed {
                    x: 30,
                    y: 30,
                    kind: 87,
                    owner: 1,
                    data: 44,
                },
                Placed {
                    x: 40,
                    y: 40,
                    kind: 93,
                    owner: 15,
                    data: 52,
                },
            ],
        };
        let current = map(&pud, &rules, &[100, 101]).unwrap();
        pud.units.retain(|p| !matches!(p.kind, 86 | 87));
        let legacy = map(&pud, &rules, &[100, 101]).unwrap();
        assert_eq!(
            ron::ser::to_string(&current.resources[..legacy.resources.len()]).unwrap(),
            ron::ser::to_string(&legacy.resources).unwrap()
        );
        assert_eq!(current.resources.len(), legacy.resources.len() + 2);
        for (node, amount, position) in [
            (&current.resources[4], 120000, Position { x: 688, y: 688 }),
            (&current.resources[5], 110000, Position { x: 1008, y: 1008 }),
        ] {
            assert_eq!(node.kind, "oil");
            assert_eq!(node.amount, amount);
            assert_eq!(node.position, position);
            assert!(node.requires_extractor);
            assert!(current.spawns.iter().any(|s| s.position == position));
        }
    }
}
