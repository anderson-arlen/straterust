//! Small map-specific overrides of a shared native artwork definition.
use super::*;
use crate::{content::FileRef, sim::PlayerId};
use std::collections::BTreeMap;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SharedAssets {
    pub shared: FileRef,
    pub terrain_grid: Option<TerrainGrid>,
    #[serde(default)]
    pub player_colors: BTreeMap<PlayerId, ColorRemap>,
    #[serde(default)]
    pub map_images: Vec<MapImageManifest>,
}

impl AssetManifest {
    pub fn load_manifest(directory: &Path) -> Result<Self> {
        let path = directory.join("assets.ron");
        let bytes = read_bounded(&path, MAX_ASSET_MANIFEST_BYTES)?;
        if let Ok(overlay) = ron::de::from_bytes::<SharedAssets>(&bytes) {
            let source = crate::asset_archive::Reader::open(directory)?;
            let base = source.read(
                &overlay.shared.file,
                &overlay.shared.blake3,
                MAX_ASSET_MANIFEST_BYTES,
            )?;
            let mut manifest: Self = ron::de::from_bytes(&base)?;
            manifest.terrain_grid = overlay.terrain_grid;
            manifest.player_colors = overlay.player_colors;
            manifest.map_images = overlay.map_images;
            return Ok(manifest);
        }
        ron::de::from_bytes(&bytes).context("invalid assets.ron")
    }
}
