//! Optional top-level game presentation/version metadata, separate from gameplay.
use crate::{
    assets::{Image, ImageRef, decode_image, encode_image},
    content::read_ron,
};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImportBuild {
    pub id: String,
    /// Monotonic converter build; advance when its exported data changes.
    pub build: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GameInfo {
    pub schema_version: u32,
    /// Original game patch label, or an authored game's own version.
    pub version: String,
    #[serde(default)]
    pub importer: Option<ImportBuild>,
    #[serde(default)]
    pub icon: Option<ImageRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asset_archive: Option<String>,
}

impl GameInfo {
    pub fn load(root: &Path) -> Result<Option<Self>> {
        let path = root.join("game.ron");
        if !path.exists() {
            return Ok(None);
        }
        ensure!(
            fs::symlink_metadata(&path)?.is_file(),
            "game.ron must be a regular file"
        );
        let info: Self = read_ron(&path)?;
        info.validate()?;
        Ok(Some(info))
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(self.schema_version == 1, "unsupported game metadata schema");
        ensure!(
            !self.version.is_empty()
                && self.version.len() <= 64
                && self
                    .version
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b".+_-".contains(&b)),
            "invalid game version"
        );
        if let Some(importer) = &self.importer {
            ensure!(
                importer.build > 0
                    && !importer.id.is_empty()
                    && importer.id.len() <= 64
                    && importer
                        .id
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b)),
                "invalid importer build"
            );
        }
        if let Some(icon) = &self.icon {
            crate::assets::validate_reference(icon)?;
        }
        if let Some(name) = &self.asset_archive {
            crate::assets::validate_filename(name)?;
        }
        Ok(())
    }

    pub fn package_version(&self) -> String {
        match &self.importer {
            Some(importer) => format!("{}+importer.{}", self.version, importer.build),
            None => self.version.clone(),
        }
    }

    pub fn load_icon(&self, root: &Path) -> Result<Option<Image>> {
        let Some(icon) = &self.icon else {
            return Ok(None);
        };
        let path = crate::assets::package_file(&root.canonicalize()?, &icon.file)?;
        let bytes = crate::assets::read_bounded(&path, 16 + 1024 * 1024 * 4)?;
        ensure!(
            blake3::hash(&bytes).to_hex().as_str() == icon.blake3,
            "game icon hash mismatch"
        );
        let image = decode_image(&bytes)?;
        ensure!(
            image.width <= 1024 && image.height <= 1024,
            "game icon exceeds 1024 pixels"
        );
        Ok(Some(image))
    }

    pub fn imported(
        root: &Path,
        version: String,
        id: &str,
        build: u32,
        image: Image,
    ) -> Result<()> {
        fs::create_dir_all(root)?;
        let asset_archive = Self::load(root)?.and_then(|info| info.asset_archive);
        let bytes = encode_image(&image)?;
        // Content-addressed icon publication keeps old metadata valid until the
        // new metadata is atomically replaced, including during metadata refresh.
        let digest = blake3::hash(&bytes).to_hex().to_string();
        let icon = ImageRef {
            file: format!("game-icon-{digest}.srim"),
            blake3: digest,
        };
        let info = Self {
            schema_version: 1,
            version,
            importer: Some(ImportBuild {
                id: id.into(),
                build,
            }),
            icon: Some(icon.clone()),
            asset_archive,
        };
        info.validate()?;
        let icon_temporary = root.join(".game-icon.srim.new");
        fs::write(&icon_temporary, bytes)?;
        fs::rename(icon_temporary, root.join(&icon.file))?;
        let temporary = root.join(".game.ron.new");
        fs::write(
            &temporary,
            ron::ser::to_string_pretty(&info, ron::ser::PrettyConfig::default())?,
        )?;
        fs::rename(temporary, root.join("game.ron"))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn optional_version_icon_metadata_preserves_gameplay_identity_and_checks_integrity() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path();
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/terran-demo");
        for name in ["manifest.ron", "rules.ron", "map.ron"] {
            fs::copy(fixture.join(name), root.join(name)).unwrap();
        }
        let before = crate::content::Package::load(root)
            .unwrap()
            .world(42)
            .unwrap()
            .state_hash();
        assert!(GameInfo::load(root).unwrap().is_none());
        let image = Image {
            width: 2,
            height: 2,
            rgba: vec![255; 16],
        };
        GameInfo::imported(root, "2.02b".into(), "warcraft2", 1, image.clone()).unwrap();
        let info = GameInfo::load(root).unwrap().unwrap();
        assert_eq!(info.package_version(), "2.02b+importer.1");
        assert_eq!(info.load_icon(root).unwrap().unwrap(), image);
        assert_eq!(
            before,
            crate::content::Package::load(root)
                .unwrap()
                .world(42)
                .unwrap()
                .state_hash()
        );
        fs::write(root.join(&info.icon.unwrap().file), "corrupt icon").unwrap();
        assert!(
            GameInfo::load(root)
                .unwrap()
                .unwrap()
                .load_icon(root)
                .is_err()
        );
    }
}
