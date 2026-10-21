//! Refresh native rules and command layouts without reconverting retail assets.
use anyhow::Result;
use std::{fs, path::Path};
use straterust_engine::{
    asset_archive,
    assets::{AssetManifest, ImageRef, SharedAssets, decode_image, encode_image},
    content::{SharedDocument, read_ron},
    sim::{Map, Rules},
};

/// The installer calls this only inside its atomic replacement directory.
pub(super) fn definitions(
    root: &Path,
    source: Option<&super::source::Source>,
    progress: &dyn Fn(&str),
) -> Result<()> {
    fn collect(directory: &Path, paths: &mut Vec<std::path::PathBuf>) -> Result<()> {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                collect(&entry.path(), paths)?;
            } else if ["rules.ron", "presentation.ron", "assets.ron", "mission.ron"]
                .iter()
                .any(|name| entry.file_name() == *name)
            {
                paths.push(entry.path());
            }
        }
        Ok(())
    }
    let catalog = straterust_engine::multiplayer::Catalog::load(root)?;
    let mut eras = std::collections::BTreeMap::new();
    for (era, relative) in catalog.factions[0].tileset_presentations.iter().enumerate() {
        let assets = AssetManifest::load_manifest(&root.join(relative))?;
        eras.insert(assets.terrain.blake3, era);
    }
    let mut paths = Vec::new();
    collect(root, &mut paths)?;
    paths.sort();
    progress("Updating Warcraft II rules, destructible rocks, flying attacks and spell effects");
    let archive = root.join(".updated-definitions.sras");
    let mut writer = asset_archive::Writer::create(&archive)?;
    writer.extend(&asset_archive::Reader::open(root)?)?;
    let mut art_cache =
        std::collections::BTreeMap::<String, (straterust_engine::content::FileRef, usize)>::new();
    for (index, path) in paths.iter().enumerate() {
        if index % 30 == 0 {
            progress(&format!(
                "Updating Warcraft II definitions {} / {}",
                index + 1,
                paths.len()
            ));
        }
        if path.file_name().unwrap() == "mission.ron" {
            let mut mission: straterust_engine::sim::Mission = read_ron(path)?;
            mission.rescue_margin.get_or_insert(32);
            fs::write(path, ron::ser::to_string(&mission)?)?;
            continue;
        }
        if path.file_name().unwrap() == "assets.ron" {
            let mut overlay: SharedAssets = ron::from_str(&fs::read_to_string(path)?)?;
            let directory = path.parent().unwrap();
            let (reference, era) = if let Some(reference) = art_cache.get(&overlay.shared.blake3) {
                reference.clone()
            } else {
                let reader = asset_archive::Reader::open(path.parent().unwrap())?;
                let bytes = reader.read(
                    &overlay.shared.file,
                    &overlay.shared.blake3,
                    straterust_engine::assets::MAX_ASSET_MANIFEST_BYTES,
                )?;
                let mut assets: AssetManifest = ron::de::from_bytes(&bytes)?;
                let era = eras[&assets.terrain.blake3];
                super::effects::upgrade(&mut assets);
                if let Some(source) = source {
                    let set = super::terrain::Tileset::load(&source.main, era)?;
                    let images = super::spell_art::status_images(&source.main, &set.palette)?
                        .iter()
                        .map(|image| {
                            let r = writer.insert(&encode_image(image)?)?;
                            Ok(ImageRef {
                                file: r.file,
                                blake3: r.blake3,
                            })
                        })
                        .collect::<Result<Vec<_>>>()?;
                    for projectile in &mut assets.projectiles {
                        super::spell_art::status(projectile, &images);
                    }
                } else {
                    anyhow::ensure!(
                        !assets
                            .projectiles
                            .iter()
                            .any(|p| p.ability == Some(straterust_engine::sim::AbilityId(5))
                                && p.marker.is_none()),
                        "native status artwork is missing; upgrade with --compact --source PATH or reimport the original Warcraft II source"
                    );
                }

                if !assets.resources.iter().any(|r| r.kind == "rock") {
                    let atlas = decode_image(&reader.read(
                        &assets.terrain.file,
                        &assets.terrain.blake3,
                        16 + 2048 * 2048 * 4,
                    )?)?;
                    let (live, dead) = super::rocks::tiles(era);
                    let mut insert = |tile| -> Result<ImageRef> {
                        let r =
                            writer.insert(&encode_image(&super::rocks::image(&atlas, tile))?)?;
                        Ok(ImageRef {
                            file: r.file,
                            blake3: r.blake3,
                        })
                    };
                    assets.resources.push(super::rocks::manifest(
                        era,
                        insert(live)?,
                        insert(dead)?,
                    ));
                }
                assets.validate()?;
                let reference = writer.insert(ron::ser::to_string(&assets)?.as_bytes())?;
                art_cache.insert(overlay.shared.blake3.clone(), (reference.clone(), era));
                (reference, era)
            };
            if directory.join("map.ron").is_file()
                && directory.join("terrain.srtm").is_file()
                && let Some(grid) = &overlay.terrain_grid
            {
                let mut map: Map = read_ron(&directory.join("map.ron"))?;
                let mut terrain = straterust_engine::map::decode_terrain(&fs::read(
                    directory.join("terrain.srtm"),
                )?)?;
                super::rocks::add(&mut map, &mut terrain, grid, &super::rocks::edges(era));
                fs::write(directory.join("map.ron"), ron::ser::to_string(&map)?)?;
                fs::write(
                    directory.join("terrain.srtm"),
                    straterust_engine::map::encode_terrain(&terrain)?,
                )?;
            }
            overlay.shared = reference;
            fs::write(path, ron::ser::to_string(&overlay)?)?;
            continue;
        }
        let bytes = if path.file_name().unwrap() == "rules.ron" {
            let mut rules: Rules = read_ron(path)?;
            super::stats::upgrade(&mut rules);
            super::spells::upgrade(&mut rules);
            ron::ser::to_string(&rules)?.into_bytes()
        } else {
            let mut presentation: super::presentation::Presentation = read_ron(path)?;
            super::presentation::upgrade(&mut presentation);
            ron::ser::to_string(&presentation)?.into_bytes()
        };
        let reference = SharedDocument {
            shared: writer.insert(&bytes)?,
        };
        fs::write(path, ron::ser::to_string(&reference)?)?;
    }
    writer.finish()?;
    fs::rename(archive, root.join("shared-assets.sras"))?;
    Ok(())
}
