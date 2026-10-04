//! One bounded native map download. No archive extraction, file paths, scripts,
//! libraries, executable payloads or downloaded game rules enter the client.
use super::*;
use crate::{assets::MapArtwork, sim::PublicMap};

pub const MAX_MAP_BYTES: usize = 96 * 1024 * 1024;
pub const MAP_CHUNK_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MapTransfer {
    pub version: u32,
    pub identity: GameplayIdentity,
    pub map: PublicMap,
    pub artwork: Option<MapArtwork>,
}

impl MapTransfer {
    pub fn of(world: &World) -> Result<Self> {
        Ok(Self {
            version: 1,
            identity: GameplayIdentity::of(world),
            map: PublicMap::of(world)?,
            artwork: None,
        })
    }

    pub fn load(directory: &Path, world: &World) -> Result<Self> {
        let mut transfer = Self::of(world)?;
        transfer.artwork = MapArtwork::load(directory)?;
        if let Some(art) = &transfer.artwork {
            art.validate(world)?;
        }
        Ok(transfer)
    }

    pub fn definitions(&self, installed: &World, player: PlayerId) -> Result<World> {
        ensure!(self.version == 1, "unsupported map transfer version");
        let world = self
            .map
            .definitions(installed.rules().clone(), &self.identity, player)?;
        if let Some(art) = &self.artwork {
            art.validate(&world)?;
        }
        Ok(world)
    }

    pub(super) fn encode(&self) -> Result<Vec<u8>> {
        let bytes = ron::ser::to_string(self)?.into_bytes();
        ensure!(
            bytes.len() <= MAX_MAP_BYTES,
            "map transfer exceeds 96 MiB limit"
        );
        Ok(bytes)
    }
}

pub(super) struct Download {
    length: usize,
    hash: String,
    bytes: Vec<u8>,
}

impl Download {
    pub fn new(length: usize, hash: String) -> Result<Self> {
        ensure!(
            (1..=MAX_MAP_BYTES).contains(&length)
                && hash.len() == 64
                && hash.bytes().all(|b| b.is_ascii_hexdigit()),
            "invalid map download header"
        );
        Ok(Self {
            length,
            hash,
            bytes: Vec::new(),
        })
    }

    pub fn push(&mut self, offset: usize, data: &[u8]) -> Result<Option<MapTransfer>> {
        ensure!(
            offset == self.bytes.len()
                && !data.is_empty()
                && data.len() <= MAP_CHUNK_BYTES
                && data.len() <= self.length - self.bytes.len(),
            "invalid map chunk position or length"
        );
        self.bytes.extend_from_slice(data);
        if self.bytes.len() < self.length {
            return Ok(None);
        }
        ensure!(
            blake3::hash(&self.bytes).to_hex().as_str() == self.hash,
            "map download checksum mismatch"
        );
        let map = ron::de::from_bytes(&self.bytes).context("invalid data-only map")?;
        Ok(Some(map))
    }

    pub fn hash(&self) -> &str {
        &self.hash
    }
}
