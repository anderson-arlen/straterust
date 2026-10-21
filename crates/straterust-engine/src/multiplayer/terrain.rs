//! Apply shared native tileset mappings to one map layout in memory.
use super::*;
use crate::assets::{AssetManifest, MapArtwork, TerrainGrid};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TileMapping {
    pub tiles: Vec<u32>,
    #[serde(default)]
    pub color_sources: Vec<[u8; 3]>,
}

impl Catalog {
    pub fn assemble_artwork(
        &self,
        root: &Path,
        directory: &Path,
        setup: &MapSetup,
        era: usize,
        map: &mut Map,
    ) -> Result<Option<MapArtwork>> {
        let Some(layout) = &setup.terrain_grid else {
            return Ok(None);
        };
        let mapping = self
            .terrain_mappings
            .get(era)
            .context("missing terrain mapping")?;
        layout.validate()?;
        ensure!(
            layout.columns as i64 * layout.tile_size as i64 == map.width as i64
                && layout.rows as i64 * layout.tile_size as i64 == map.height as i64,
            "map layout dimensions mismatch"
        );
        let presentation = self.factions[0]
            .tileset_presentations
            .get(era)
            .context("missing tileset presentation")?;
        let template = package_path(root, presentation)?;
        let assets = AssetManifest::load_manifest(&template)?;
        let tiles = layout
            .tiles
            .iter()
            .map(|id| {
                mapping
                    .tiles
                    .get(*id as usize)
                    .copied()
                    .context("map tile outside shared mapping")
            })
            .collect::<Result<Vec<_>>>()?;
        let grid = TerrainGrid {
            tiles,
            ..layout.clone()
        };
        let mut artwork = MapArtwork::load(directory)?.context("map artwork missing")?;
        artwork.terrain = crate::asset_archive::Reader::open(&template)?.read(
            &assets.terrain.file,
            &assets.terrain.blake3,
            16 + 2048 * 2048 * 4,
        )?;
        artwork.grid = Some(grid.clone());
        for colors in artwork.player_colors.values_mut() {
            if !mapping.color_sources.is_empty() {
                ensure!(
                    colors.colors.len() == mapping.color_sources.len(),
                    "tileset color ramp mismatch"
                );
                for (pair, source) in colors.colors.iter_mut().zip(&mapping.color_sources) {
                    pair[0] = *source;
                }
            }
        }
        for resource in &mut map.resources {
            if let Some(edges) = assets
                .resources
                .iter()
                .find(|r| r.kind == resource.kind)
                .and_then(|r| r.terrain_edges.as_ref())
            {
                let x = resource.position.x / grid.tile_size as i32;
                let y = resource.position.y / grid.tile_size as i32;
                ensure!(
                    x >= 0 && y >= 0 && x < grid.columns as i32 && y < grid.rows as i32,
                    "resource outside tile layout"
                );
                resource.terrain_corners = edges
                    .corners
                    .get(&grid.tiles[(y as u32 * grid.columns + x as u32) as usize])
                    .copied();
            }
        }
        Ok(Some(artwork))
    }
}

impl Prepared {
    pub fn map_transfer(&self) -> Result<crate::net::MapTransfer> {
        if let Some(artwork) = &self.artwork {
            artwork.validate(&self.world)?;
            let mut transfer = crate::net::MapTransfer::of(&self.world)?;
            transfer.artwork = Some(artwork.clone());
            Ok(transfer)
        } else {
            crate::net::MapTransfer::load(&self.directory, &self.world)
        }
    }
}
