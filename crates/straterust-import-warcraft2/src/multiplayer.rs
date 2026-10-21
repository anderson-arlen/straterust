//! All built-in and installer-supplied multiplayer PUDs, with native setup data.
use super::{
    publish::{map, ron},
    pud::{Pud, word},
    source::{Source, case_path},
    stats,
    terrain::Tileset,
};
use anyhow::{Context, Result, ensure};
use std::{collections::BTreeSet, fs, path::Path};
use straterust_engine::{assets::*, map::encode_terrain, multiplayer::*, sim::*};

mod ai;
pub use ai::upgrade_catalog;
#[cfg(test)]
mod ai_tests;
mod storage;
pub use storage::upgrade;

fn link_files(from: &Path, to: &Path) -> Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        if entry.file_type()?.is_file() && !to.join(entry.file_name()).exists() {
            if entry.path().extension().is_some_and(|e| e == "ron") {
                fs::copy(entry.path(), to.join(entry.file_name()))?;
            } else {
                fs::hard_link(entry.path(), to.join(entry.file_name()))?;
            }
        }
    }
    Ok(())
}
fn loose(root: &Path, files: &mut Vec<(String, Vec<u8>)>, depth: usize) -> Result<()> {
    ensure!(depth <= 8, "map directories exceed nesting limit");
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            loose(&entry.path(), files, depth + 1)?;
        } else if entry.file_type()?.is_file()
            && entry
                .path()
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("pud"))
        {
            ensure!(
                files.len() < 1024 && entry.metadata()?.len() <= 4 * 1024 * 1024,
                "multiplayer source exceeds limits"
            );
            files.push((
                entry.path().file_stem().unwrap().to_string_lossy().into(),
                fs::read(entry.path())?,
            ));
        }
    }
    Ok(())
}
#[allow(clippy::too_many_arguments)]
pub fn publish(
    source: &mut Source,
    output: &Path,
    cache: &Path,
    sets: &[Tileset],
    templates: &[AssetManifest],
    rules: &Rules,
    names: &[String],
    media: &super::media::Library,
    audio: &Path,
    progress: &dyn Fn(&str),
) -> Result<()> {
    let mut files = Vec::new();
    for index in 220..=247 {
        files.push((format!("Built-in {index}"), source.main.entry(index)?));
    }
    if let Ok(root) = case_path(&source.root, "Maps") {
        loose(&root, &mut files, 0)?;
    }
    let mut native_rules = rules.clone();
    native_rules.id = "warcraft2.multiplayer".into();
    native_rules.victory = true;
    let mut catalog = Catalog {
        colors: super::colors::catalog(),
        terrain_mappings: Vec::new(),
        schema_version: 1,
        game: "warcraft2".into(),
        maps: Vec::new(),
        factions: (0..2)
            .map(|race| Faction {
                ai_difficulties: Default::default(),
                ai_research: Vec::new(),
                name: if race == 0 { "Human" } else { "Orc" }.into(),
                presentation: format!("multiplayer/art-{race}-0"),
                worker: stats::id(2 + race),
                tileset_presentations: (0..4)
                    .map(|era| format!("multiplayer/art-{race}-{era}"))
                    .collect(),
                starting_units: vec![(stats::id(2 + race), Position { x: 0, y: 0 })],
                equivalents: equivalent_sources()
                    .map(|i| (stats::id(i), stats::id((i & !1) + race)))
                    .collect(),
                ai: ai::program(race, AiDifficulty::Normal),
            })
            .collect(),
        resources: [
            ("Low", 2000, 1000, 1000),
            ("Medium", 5000, 2000, 2000),
            ("High", 10000, 5000, 5000),
        ]
        .into_iter()
        .map(|(name, gold, wood, oil)| ResourcePreset {
            name: name.into(),
            amounts: vec![
                ResourceAmount {
                    kind: "gold".into(),
                    amount: gold,
                },
                ResourceAmount {
                    kind: "wood".into(),
                    amount: wood,
                },
                ResourceAmount {
                    kind: "oil".into(),
                    amount: oil,
                },
            ],
        })
        .collect(),
        speeds: [
            ("Slowest", 100),
            ("Slower", 67),
            ("Slow", 50),
            ("Normal", 40),
            ("Fast", 33),
            ("Faster", 25),
            ("Fastest", 20),
        ]
        .into_iter()
        .map(|(name, tick_ms)| Speed {
            name: name.into(),
            tick_ms,
        })
        .collect(),
        modes: vec![
            GameType::Melee,
            GameType::FreeForAll,
            GameType::OneOnOne,
            GameType::TopVsBottom,
            GameType::UseMapSettings,
            GameType::Teams,
        ],
        tilesets: ["Forest", "Winter", "Wasteland", "Orc Swamp"]
            .into_iter()
            .map(str::to_owned)
            .collect(),
        allow_cheats: true,
        random_faction: false,
        standard_start: false,
        defeat_without_structures: false,
        flag_art: None,
    };
    ai::upgrade_catalog(&mut catalog);
    // Each faction/era has one presentation package. Maps hard-link these assets.
    for era in 0..4 {
        for race in 0..2 {
            let directory = output.join(format!("multiplayer/art-{race}-{era}"));
            link_files(&cache.join(era.to_string()), &directory)?;
            let mut assets = templates[era].clone();
            assets.resources.push(sets[era].forest(&directory)?);
            assets.resources.push(sets[era].rocks(&directory, era)?);
            let dummy = Pud::decode(&source.main.entry(220)?)?;
            let mut dummy_map = map(&dummy, &native_rules, &[])?;
            dummy_map.spawns.clear();
            let mut dummy_terrain = straterust_engine::map::Terrain {
                cell_size: 32,
                columns: 72,
                rows: 1,
                flags: vec![3; 72],
            };
            assets.terrain_grid = Some(TerrainGrid {
                tile_size: 32,
                columns: 72,
                rows: 1,
                tiles: (16..88).collect(),
            });
            let mut full_rules = native_rules.clone();
            super::walls::add(
                &source.main,
                &sets[era],
                &directory,
                &mut full_rules,
                &mut dummy_map,
                &mut dummy_terrain,
                &mut assets,
            )?;
            super::indicators::add(
                &source.main,
                &directory,
                &sets[era].palette,
                &full_rules,
                race,
                &mut assets,
            )?;
            super::console::add(&source.main, &directory, race, &mut assets)?;
            fs::write(
                directory.join("assets.ron"),
                ::ron::ser::to_string(&assets)?,
            )?;
            ron(&directory.join("rules.ron"), &full_rules)?;
            ron(
                &directory.join("presentation.ron"),
                &super::presentation::build(&native_rules, names, "Defeat your opponents.".into()),
            )?;
            let mut recording = media.mission(source, &directory, audio, (race, 1, false), "")?;
            recording.briefing.clear();
            recording.mission_audio.clear();
            recording.mission_texts.clear();
            ron(&directory.join("media.ron"), &recording)?;
        }
    }
    native_rules =
        straterust_engine::content::read_ron(&output.join("multiplayer/art-0-0/rules.ron"))?;
    let mut mappings = storage::builder(sets);
    let mut seen = BTreeSet::new();
    for (fallback, bytes) in files {
        let hash = blake3::hash(&bytes).to_hex().to_string();
        if !seen.insert(hash.clone()) {
            continue;
        }
        let mut pud = Pud::decode(&bytes).with_context(|| format!("multiplayer map {fallback}"))?;
        let active: Vec<_> = (0..8)
            .filter(|p| {
                matches!(pud.owners[*p], 4..=7)
                    && pud.units.iter().any(|u| usize::from(u.owner) == *p)
            })
            .collect();
        ensure!(
            (2..=8).contains(&active.len()),
            "multiplayer map {fallback} has invalid player slots"
        );
        let title = if pud.title.is_empty() || pud.title == "No Description" {
            fallback
        } else {
            pud.title.clone()
        };
        let title: String = title.chars().take(128).collect();
        progress(&format!("Converting multiplayer map: {title}"));
        let resources = active
            .iter()
            .map(|p| {
                [(*b"SGLD", "gold"), (*b"SLBR", "wood"), (*b"SOIL", "oil")]
                    .into_iter()
                    .map(|(chunk, kind)| {
                        Ok(ResourceAmount {
                            kind: kind.into(),
                            amount: u32::from(word(
                                pud.chunks
                                    .get(&chunk)
                                    .context("missing starting resources")?,
                                p * 2,
                            )?),
                        })
                    })
                    .collect::<Result<Vec<_>>>()
            })
            .collect::<Result<Vec<_>>>()?;
        let factions = active
            .iter()
            .map(|p| usize::from(pud.sides[*p]).min(1))
            .collect();
        for unit in &mut pud.units {
            unit.owner = active
                .iter()
                .position(|p| *p == usize::from(unit.owner))
                .map_or(15, |p| p as u8);
        }
        pud.local = 0;
        let package = format!("multiplayer/map-{}", &hash[..16]);
        let original = pud.era;
        let grids = sets
            .iter()
            .map(|set| {
                let mut grid = set.map(&pud)?.0;
                for tile in &mut grid.tiles {
                    *tile = super::walls::background_tile(*tile);
                }
                Ok(grid)
            })
            .collect::<Result<Vec<_>>>()?;
        let layout = mappings.layout(&grids)?;
        let relative = format!("{package}/era-{original}");
        {
            let era = original;
            let set = &sets[era];
            let directory = output.join(&relative);
            link_files(&output.join(format!("multiplayer/art-0-{era}")), &directory)?;
            let (grid, mut terrain, trees) = set.map(&pud)?;
            // Asset manifests have their own 32 MiB bound, separate from
            // gameplay documents. The complete directional roster is larger.
            let manifest_bytes = fs::read(directory.join("assets.ron"))?;
            ensure!(
                manifest_bytes.len() <= MAX_ASSET_MANIFEST_BYTES,
                "multiplayer asset manifest exceeds limit"
            );
            let mut assets: AssetManifest = ::ron::de::from_bytes(&manifest_bytes)?;
            assets.terrain_grid = Some(grid.clone());
            let original = Pud::decode(&bytes)?;
            let colors = super::colors::players(&original, &set.palette);
            assets.player_colors = active
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    (
                        PlayerId(i as u16),
                        colors[&PlayerId(u16::from(original.player(*p)))].clone(),
                    )
                })
                .collect();
            let mut map = map(&pud, &native_rules, &trees)?;
            super::rocks::add(&mut map, &mut terrain, &grid, &super::rocks::edges(era));
            // Custom PUDs can start with an army instead of start markers.
            // Keep their authored terrain/placements and anchor each slot at
            // its depot, worker or first placed unit.
            for player in 0..active.len() {
                let player = PlayerId(player as u16);
                if !map.start_locations.iter().any(|s| s.player == player) {
                    let spawn = map
                        .spawns
                        .iter()
                        .filter(|s| s.owner == player)
                        .min_by_key(|s| {
                            let unit = native_rules
                                .units
                                .iter()
                                .find(|u| u.id == s.unit_type)
                                .unwrap();
                            if unit.structure && !unit.dropoff.is_empty() {
                                0
                            } else if unit.worker.is_some() {
                                1
                            } else {
                                2
                            }
                        })
                        .context("map player has no starting units")?;
                    map.start_locations.push(StartLocation {
                        player,
                        position: spawn.position,
                    });
                }
            }
            map.id = format!("warcraft2.multiplayer.{}", &hash[..16]);
            let edges = assets
                .resources
                .iter()
                .find(|r| r.kind == "wood")
                .unwrap()
                .terrain_edges
                .as_ref()
                .unwrap();
            for resource in map.resources.iter_mut().filter(|r| r.kind == "wood") {
                let i = (resource.position.y / 32) as usize * grid.columns as usize
                    + (resource.position.x / 32) as usize;
                resource.terrain_corners = edges.corners.get(&grid.tiles[i]).copied();
            }
            let mut rules = native_rules.clone();
            super::walls::add(
                &source.main,
                set,
                &directory,
                &mut rules,
                &mut map,
                &mut terrain,
                &mut assets,
            )?;
            fs::write(
                directory.join("manifest.ron"),
                format!("(schema_version:1,id:{:?})", map.id),
            )?;
            ron(&directory.join("rules.ron"), &rules)?;
            fs::write(directory.join("map.ron"), ::ron::ser::to_string(&map)?)?;
            fs::write(
                directory.join("assets.ron"),
                ::ron::ser::to_string(&assets)?,
            )?;
            fs::write(directory.join("terrain.srtm"), encode_terrain(&terrain)?)?;
        }
        let setup = MapSetup {
            terrain_grid: Some(layout),
            scenario_spawns: None,
            selectable_factions: Vec::new(),
            factions,
            resources,
            variants: Vec::new(),
            scenario_error: if word(pud.chunks.get(b"UDTA").context("missing unit settings")?, 0)?
                == 0
            {
                Some("custom scenario unit statistics are not supported yet".into())
            } else {
                None
            },
        };
        ron(&output.join(&relative).join("match.ron"), &setup)?;
        catalog.maps.push(MapEntry {
            title,
            package: relative,
            players: active.len() as u8,
            size: [pud.width, pud.height],
            tileset: original as u8,
        });
    }
    catalog.terrain_mappings = mappings.mappings;
    catalog.maps.sort_by(|a, b| a.title.cmp(&b.title));
    catalog.validate()?;
    ron(&output.join("multiplayer.ron"), &catalog)
}

pub(super) fn equivalent_sources() -> impl Iterator<Item = usize> {
    (0..=99).filter(|i| {
        *i < 16
            || (18..=23).contains(i)
            || (26..=33).contains(i)
            || (38..=43).contains(i)
            || (58..=91).contains(i)
            || (96..=99).contains(i)
    })
}
