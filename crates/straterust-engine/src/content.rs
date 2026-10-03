//! Native directory packages. This module is the filesystem boundary, not part of a tick.
use std::{
    fs::{self, File},
    io::Read,
    path::Path,
};

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::{
    map::{MAX_TERRAIN_BYTES, Terrain, decode_terrain},
    sim::{Map, Mission, Rules, World},
};

pub const SCHEMA_VERSION: u32 = 1;
pub const MAX_CONTENT_BYTES: u64 = 4 * 1024 * 1024;

/// An ordered set of native packages. Campaign progression belongs to the
/// local client; mission results still come from the deterministic world.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Campaign {
    pub schema_version: u32,
    pub id: String,
    pub missions: Vec<CampaignMission>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CampaignMission {
    pub title: String,
    pub package: String,
}

impl Campaign {
    pub fn load(directory: &Path) -> Result<Self> {
        let campaign: Self = read_ron(&directory.join("campaign.ron"))?;
        campaign.validate()?;
        Ok(campaign)
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.schema_version == SCHEMA_VERSION,
            "unsupported campaign schema"
        );
        ensure!(
            !self.id.is_empty() && self.id.len() <= 128,
            "invalid campaign ID"
        );
        ensure!(
            !self.missions.is_empty() && self.missions.len() <= 64,
            "invalid campaign length"
        );
        let mut names = std::collections::BTreeSet::new();
        for mission in &self.missions {
            ensure!(
                !mission.title.is_empty() && mission.title.len() <= 128,
                "invalid mission title"
            );
            ensure!(
                !mission.package.is_empty()
                    && mission.package.len() <= 128
                    && mission.package != "."
                    && mission.package != ".."
                    && mission
                        .package
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b)),
                "campaign package must be a local directory name"
            );
            ensure!(names.insert(&mission.package), "duplicate campaign package");
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema_version: u32,
    pub id: String,
}

#[derive(Debug, Clone)]
pub struct Package {
    rules: Rules,
    map: Map,
}

impl Package {
    pub fn load(directory: &Path) -> Result<Self> {
        let manifest: Manifest = read_ron(&directory.join("manifest.ron"))?;
        ensure!(
            manifest.schema_version == SCHEMA_VERSION,
            "unsupported package schema {}; expected {SCHEMA_VERSION}",
            manifest.schema_version
        );
        ensure!(
            !manifest.id.is_empty() && manifest.id.len() <= 128,
            "invalid package ID"
        );
        let rules = read_ron(&directory.join("rules.ron"))?;
        let mut map: Map = read_ron(&directory.join("map.ron"))?;
        map.terrain = load_terrain(directory)?;
        map.mission = load_mission(directory)?;
        let package = Self { rules, map };
        package.world(0).context("invalid gameplay package")?;
        Ok(package)
    }

    pub fn world(&self, seed: u64) -> Result<World> {
        World::new(self.rules.clone(), self.map.clone(), seed)
    }
}

fn load_mission(directory: &Path) -> Result<Option<Mission>> {
    let path = directory.join("mission.ron");
    match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        metadata => {
            ensure!(
                metadata.context("cannot inspect mission.ron")?.is_file(),
                "mission.ron must be a regular file, not a symlink or special file"
            );
            read_ron(&path).map(Some)
        }
    }
}

fn load_terrain(directory: &Path) -> Result<Option<Terrain>> {
    let path = directory.join("terrain.srtm");
    match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        result => ensure!(
            result.context("cannot inspect terrain.srtm")?.is_file(),
            "terrain.srtm must be a regular file, not a symlink or special file"
        ),
    }
    let file = File::open(&path).context("cannot open terrain.srtm")?;
    ensure!(
        file.metadata()?.is_file(),
        "terrain.srtm must be a regular file"
    );
    let mut bytes = Vec::new();
    file.take(MAX_TERRAIN_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .context("cannot read terrain.srtm")?;
    decode_terrain(&bytes)
        .context("invalid terrain.srtm")
        .map(Some)
}

pub fn read_ron<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let file = File::open(path).with_context(|| format!("cannot open {}", path.display()))?;
    let mut bytes = Vec::new();
    file.take(MAX_CONTENT_BYTES + 1)
        .read_to_end(&mut bytes)
        .with_context(|| format!("cannot read {}", path.display()))?;
    ensure!(
        bytes.len() as u64 <= MAX_CONTENT_BYTES,
        "{} exceeds the 4 MiB content limit",
        path.display()
    );
    ron::de::from_bytes(&bytes).with_context(|| format!("invalid RON in {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn optional_mission_file_rejects_invalid_oversized_and_indirect_input() {
        let directory =
            std::env::temp_dir().join(format!("straterust-mission-package-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("mission.ron");
        let _ = fs::remove_file(&path);
        assert!(load_mission(&directory).unwrap().is_none());
        fs::write(&path, b"(schema_version: 1, unknown: true)").unwrap();
        assert!(load_mission(&directory).is_err());
        File::create(&path)
            .unwrap()
            .set_len(MAX_CONTENT_BYTES + 1)
            .unwrap();
        assert!(
            load_mission(&directory)
                .unwrap_err()
                .to_string()
                .contains("4 MiB")
        );
        fs::remove_file(&path).unwrap();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(directory.join("missing.ron"), &path).unwrap();
            assert!(
                load_mission(&directory)
                    .unwrap_err()
                    .to_string()
                    .contains("regular file")
            );
            fs::remove_file(&path).unwrap();
        }
        fs::remove_dir(&directory).unwrap();
    }
}
