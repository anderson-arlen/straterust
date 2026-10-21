//! One map layout, common terrain mapping tables, and legacy import migration.
use super::*;
use std::collections::BTreeMap;
use straterust_engine::{
    assets::MapArtwork,
    content::{Package, read_ron},
};

pub(super) struct MappingBuilder {
    indices: BTreeMap<Vec<u32>, u32>,
    pub(super) mappings: Vec<TileMapping>,
}
impl MappingBuilder {
    fn new(colors: Vec<Vec<[u8; 3]>>) -> Self {
        Self {
            indices: BTreeMap::new(),
            mappings: colors
                .into_iter()
                .map(|color_sources| TileMapping {
                    tiles: Vec::new(),
                    color_sources,
                })
                .collect(),
        }
    }
    pub(super) fn layout(&mut self, grids: &[TerrainGrid]) -> Result<TerrainGrid> {
        ensure!(
            grids.len() == self.mappings.len() && !grids.is_empty(),
            "tileset grid count mismatch"
        );
        let first = &grids[0];
        ensure!(
            grids.iter().all(|g| g.columns == first.columns
                && g.rows == first.rows
                && g.tile_size == first.tile_size
                && g.tiles.len() == first.tiles.len()),
            "tileset layouts differ"
        );
        let mut tiles = Vec::with_capacity(first.tiles.len());
        for cell in 0..first.tiles.len() {
            let key: Vec<_> = grids.iter().map(|g| g.tiles[cell]).collect();
            let id = if let Some(id) = self.indices.get(&key) {
                *id
            } else {
                ensure!(
                    self.indices.len() < 65536,
                    "native tile mappings exceed limit"
                );
                let id = self.indices.len() as u32;
                for (mapping, tile) in self.mappings.iter_mut().zip(&key) {
                    mapping.tiles.push(*tile);
                }
                self.indices.insert(key, id);
                id
            };
            tiles.push(id);
        }
        Ok(TerrainGrid {
            tiles,
            ..first.clone()
        })
    }
}

pub(super) fn builder(sets: &[Tileset]) -> MappingBuilder {
    MappingBuilder::new(
        sets.iter()
            .map(|s| {
                s.palette[208..212]
                    .iter()
                    .map(|rgb| rgb[..3].try_into().unwrap())
                    .collect()
            })
            .collect(),
    )
}

/// Operates only in the caller's replacement staging directory.
pub fn upgrade(root: &Path, progress: &dyn Fn(&str)) -> Result<()> {
    let mut catalog = Catalog::load(root)?;
    super::ai::upgrade_catalog(&mut catalog);
    if !catalog.terrain_mappings.is_empty() {
        ron(&root.join("multiplayer.ron"), &catalog)?;
        return Ok(());
    }
    ensure!(
        catalog.tilesets.len() == 4,
        "legacy Warcraft II tilesets missing"
    );
    let first = package_path(root, &catalog.maps[0].package)?;
    let first_setup: MapSetup = read_ron(&first.join("match.ron"))?;
    ensure!(
        first_setup.variants.len() == 4,
        "legacy terrain variants missing"
    );
    let colors = first_setup
        .variants
        .iter()
        .map(|v| {
            let assets = AssetManifest::load_manifest(&package_path(root, v)?)?;
            Ok(assets
                .player_colors
                .values()
                .next()
                .context("missing player palette")?
                .colors
                .iter()
                .map(|p| p[0])
                .collect())
        })
        .collect::<Result<Vec<_>>>()?;
    let mut builder = MappingBuilder::new(colors);
    let entries = catalog.maps.clone();
    for (number, entry) in entries.iter().enumerate() {
        progress(&format!(
            "Migrating map layout {} / {}: {}",
            number + 1,
            catalog.maps.len(),
            entry.title
        ));
        let directory = package_path(root, &entry.package)?;
        let mut setup: MapSetup = read_ron(&directory.join("match.ron"))?;
        ensure!(setup.variants.len() == 4, "legacy terrain variants missing");
        let mut grids = Vec::new();
        for variant in &setup.variants {
            let path = package_path(root, variant)?;
            for file in ["map.ron", "rules.ron", "terrain.srtm"] {
                // Definition wrappers are read as native content before comparison.
                let read = |p: &Path| {
                    if file == "rules.ron" {
                        straterust_engine::content::read_document(p, 4 * 1024 * 1024)
                    } else {
                        Ok(fs::read(p)?)
                    }
                };
                ensure!(
                    read(&path.join(file))? == read(&directory.join(file))?,
                    "legacy terrain changes gameplay data: {file}"
                );
            }
            grids.push(
                AssetManifest::load_manifest(&path)?
                    .terrain_grid
                    .context("missing terrain grid")?,
            );
        }
        setup.terrain_grid = Some(builder.layout(&grids)?);
        let variants = std::mem::take(&mut setup.variants);
        ron(&directory.join("match.ron"), &setup)?;
        // Use the same in-memory assembly as the host and compare original bytes.
        catalog.terrain_mappings = builder.mappings.clone();
        let world = Package::load(&directory)?.world(42)?;
        for (era, variant) in variants.iter().enumerate() {
            let expected = MapArtwork::load(&package_path(root, variant)?)?
                .context("missing original map artwork")?;
            let mut map = world.map().clone();
            let actual = catalog
                .assemble_artwork(root, &directory, &setup, era, &mut map)?
                .context("missing assembled artwork")?;
            actual.validate(&world)?;
            ensure!(
                expected.terrain == actual.terrain
                    && ron::ser::to_string(&expected.grid)? == ron::ser::to_string(&actual.grid)?
                    && ron::ser::to_string(&expected.player_colors)?
                        == ron::ser::to_string(&actual.player_colors)?
                    && expected.images == actual.images
                    && ron::ser::to_string(&expected.decorations)?
                        == ron::ser::to_string(&actual.decorations)?
                    && ron::ser::to_string(world.map())? == ron::ser::to_string(&map)?,
                "assembled terrain differs from original variant"
            );
        }
        for variant in variants {
            if variant != entry.package {
                fs::remove_dir_all(package_path(root, &variant)?)?;
            }
        }
    }
    catalog.terrain_mappings = builder.mappings;
    catalog.validate()?;
    ron(&root.join("multiplayer.ron"), &catalog)
}
