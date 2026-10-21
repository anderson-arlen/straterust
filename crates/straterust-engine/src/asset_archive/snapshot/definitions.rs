use super::*;
use crate::{
    assets::{AssetManifest, SharedAssets},
    content::{MAX_CONTENT_BYTES, SharedDocument, read_document},
};

pub(super) fn is_definition(name: &std::ffi::OsStr) -> bool {
    matches!(
        name.to_str(),
        Some("assets.ron" | "rules.ron" | "media.ron" | "presentation.ron")
    )
}

pub(super) fn pack(path: &Path, writer: &mut asset_archive::Writer) -> Result<Vec<u8>> {
    if path.file_name().is_some_and(|n| n == "assets.ron") {
        let mut base =
            AssetManifest::load_manifest(path.parent().context("missing asset parent")?)?;
        let grid = base.terrain_grid.take();
        let colors = std::mem::take(&mut base.player_colors);
        let images = std::mem::take(&mut base.map_images);
        let reference = writer.insert(ron::ser::to_string(&base)?.as_bytes())?;
        return Ok(ron::ser::to_string(&SharedAssets {
            shared: reference,
            terrain_grid: grid,
            player_colors: colors,
            map_images: images,
        })?
        .into_bytes());
    }
    let bytes = read_document(path, MAX_CONTENT_BYTES as usize)?;
    Ok(ron::ser::to_string(&SharedDocument {
        shared: writer.insert(&bytes)?,
    })?
    .into_bytes())
}
