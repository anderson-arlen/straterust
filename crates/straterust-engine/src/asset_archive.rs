//! Game-level artwork/audio storage, addressed by existing asset content hashes.
use anyhow::{Context, Result, ensure};
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::Arc,
};

const MAGIC: &[u8; 8] = b"SRAS0001";
const HEADER: u64 = 56;
const ENTRY: usize = 48;
const MAX_INDEX: u64 = 32 * 1024 * 1024;
const MAX_ARCHIVE: u64 = 128 * 1024 * 1024 * 1024;

mod snapshot;
pub use snapshot::{copy_game, copy_game_with_update, validate_directory};

#[derive(Clone, Copy)]
struct Range {
    offset: u64,
    length: u64,
}
struct Index {
    owner: PathBuf,
    path: PathBuf,
    members: BTreeMap<[u8; 32], Range>,
}
#[derive(Clone)]
pub struct Reader {
    root: PathBuf,
    archive: Option<Arc<Index>>,
}
pub struct Location {
    pub path: PathBuf,
    pub offset: u64,
    pub length: u64,
    pub was_symlink: bool,
}

impl Location {
    pub fn read(&self, limit: usize) -> Result<Vec<u8>> {
        ensure!(
            self.length <= limit as u64,
            "asset exceeds {limit} byte limit"
        );
        ensure!(
            fs::metadata(&self.path)?.is_file(),
            "asset must be a regular file"
        );
        let mut file = File::open(&self.path)?;
        let metadata = file.metadata()?;
        ensure!(metadata.is_file(), "asset must be a regular file");
        ensure!(
            self.offset
                .checked_add(self.length)
                .is_some_and(|end| end <= metadata.len()),
            "asset file was truncated"
        );
        file.seek(SeekFrom::Start(self.offset))?;
        let mut bytes = vec![0; usize::try_from(self.length)?];
        file.read_exact(&mut bytes)?;
        Ok(bytes)
    }
}

impl Reader {
    pub fn open(directory: &Path) -> Result<Self> {
        let root = directory.canonicalize()?;
        let mut ancestor = Some(root.as_path());
        for _ in 0..16 {
            let Some(game) = ancestor else { break };
            if let Some(info) = crate::game::GameInfo::load(game)?
                && let Some(name) = info.asset_archive
            {
                let path = crate::assets::package_file(game, &name)?;
                ensure!(
                    fs::symlink_metadata(game.join(&name))?.is_file(),
                    "asset archive must be a regular file"
                );
                let mut file = File::open(&path)?;
                let length = file.metadata()?.len();
                ensure!(
                    file.metadata()?.is_file() && length <= MAX_ARCHIVE,
                    "invalid native asset archive"
                );
                let mut header = [0; HEADER as usize];
                file.read_exact(&mut header)?;
                ensure!(&header[..8] == MAGIC, "unsupported native asset archive");
                let offset = u64::from_le_bytes(header[8..16].try_into()?);
                let count = u64::from_le_bytes(header[16..24].try_into()?);
                ensure!(
                    offset >= HEADER
                        && count <= MAX_INDEX
                        && count.is_multiple_of(ENTRY as u64)
                        && offset.checked_add(count) == Some(length),
                    "invalid native asset index bounds"
                );
                file.seek(SeekFrom::Start(offset))?;
                let mut bytes = vec![0; usize::try_from(count)?];
                file.read_exact(&mut bytes)?;
                ensure!(
                    blake3::hash(&bytes).as_bytes() == &header[24..56],
                    "native asset index hash mismatch"
                );
                let mut members = BTreeMap::new();
                for entry in bytes.as_chunks::<ENTRY>().0 {
                    let digest: [u8; 32] = entry[..32].try_into()?;
                    let range = Range {
                        offset: u64::from_le_bytes(entry[32..40].try_into()?),
                        length: u64::from_le_bytes(entry[40..48].try_into()?),
                    };
                    ensure!(
                        range.offset >= HEADER
                            && range.length <= 256 * 1024 * 1024
                            && range
                                .offset
                                .checked_add(range.length)
                                .is_some_and(|end| end <= offset),
                        "native asset range exceeds data bounds"
                    );
                    ensure!(
                        members.insert(digest, range).is_none(),
                        "duplicate native asset hash"
                    );
                }
                let archive = Arc::new(Index {
                    owner: game.to_owned(),
                    path,
                    members,
                });
                return Ok(Self {
                    root,
                    archive: Some(archive),
                });
            }
            ancestor = game.parent();
        }
        Ok(Self {
            root,
            archive: None,
        })
    }

    /// Validation can reuse one game's index across its many map packages.
    pub fn for_directory(&self, directory: &Path) -> Result<Self> {
        let root = directory.canonicalize()?;
        if let Some(archive) = &self.archive
            && root.starts_with(&archive.owner)
        {
            let mut ancestor = root.as_path();
            while ancestor != archive.owner {
                if crate::game::GameInfo::load(ancestor)?
                    .is_some_and(|info| info.asset_archive.is_some())
                {
                    return Self::open(&root);
                }
                ancestor = ancestor.parent().context("invalid shared asset owner")?;
            }
            return Ok(Self {
                root,
                archive: Some(Arc::clone(archive)),
            });
        }
        Self::open(&root)
    }

    pub fn location(&self, name: &str, digest: &str) -> Result<Location> {
        crate::assets::validate_filename(name)?;
        let hash = blake3::Hash::from_hex(digest).context("invalid asset content hash")?;
        if let Some(archive) = &self.archive
            && let Some(range) = archive.members.get(hash.as_bytes())
        {
            return Ok(Location {
                path: archive.path.clone(),
                offset: range.offset,
                length: range.length,
                was_symlink: false,
            });
        }
        let path = crate::assets::package_file(&self.root, name)?;
        let was_symlink = fs::symlink_metadata(self.root.join(name))?
            .file_type()
            .is_symlink();
        let metadata = fs::metadata(&path)?;
        ensure!(metadata.is_file(), "asset must be a regular file");
        Ok(Location {
            path,
            offset: 0,
            length: metadata.len(),
            was_symlink,
        })
    }

    pub fn read(&self, name: &str, digest: &str, limit: usize) -> Result<Vec<u8>> {
        let bytes = self.location(name, digest)?.read(limit)?;
        ensure!(
            blake3::hash(&bytes).to_hex().as_str() == digest,
            "asset hash mismatch: {name}"
        );
        Ok(bytes)
    }
}

/// The exporter stores each content hash once, including independently generated duplicates.
pub struct Writer {
    file: File,
    members: BTreeMap<[u8; 32], Range>,
}
impl Writer {
    pub fn create(path: &Path) -> Result<Self> {
        let mut file = File::options().write(true).create_new(true).open(path)?;
        file.write_all(&[0; HEADER as usize])?;
        Ok(Self {
            file,
            members: BTreeMap::new(),
        })
    }
    pub fn copy(&mut self, path: &Path) -> Result<()> {
        ensure!(
            fs::symlink_metadata(path)?.is_file(),
            "shared asset must be a regular file"
        );
        let source = File::open(path)?;
        let length = source.metadata()?.len();
        ensure!(
            source.metadata()?.is_file() && length <= 256 * 1024 * 1024,
            "shared asset exceeds file limit"
        );
        let mut bytes = Vec::new();
        source.take(length + 1).read_to_end(&mut bytes)?;
        ensure!(
            bytes.len() as u64 == length,
            "shared asset changed during export"
        );
        self.insert(&bytes)?;
        Ok(())
    }
    pub fn insert(&mut self, bytes: &[u8]) -> Result<crate::content::FileRef> {
        let length = bytes.len() as u64;
        ensure!(
            length <= 256 * 1024 * 1024,
            "shared asset exceeds file limit"
        );
        let digest = *blake3::hash(bytes).as_bytes();
        if !self.members.contains_key(&digest) {
            let offset = self.file.stream_position()?;
            ensure!(
                offset + length <= MAX_ARCHIVE,
                "native asset archive exceeds size limit"
            );
            self.file.write_all(bytes)?;
            self.members.insert(digest, Range { offset, length });
        }
        Ok(crate::content::FileRef {
            file: format!("shared-{}.srdef", blake3::Hash::from(digest)),
            blake3: blake3::Hash::from(digest).to_hex().to_string(),
        })
    }
    pub fn extend(&mut self, source: &Reader) -> Result<()> {
        if let Some(index) = &source.archive {
            for (digest, range) in &index.members {
                let bytes = Location {
                    path: index.path.clone(),
                    offset: range.offset,
                    length: range.length,
                    was_symlink: false,
                }
                .read(256 * 1024 * 1024)?;
                ensure!(
                    blake3::hash(&bytes).as_bytes() == digest,
                    "source shared asset hash mismatch"
                );
                self.insert(&bytes)?;
            }
        }
        Ok(())
    }
    pub fn finish(mut self) -> Result<()> {
        let mut bytes = Vec::new();
        for (digest, range) in self.members {
            bytes.extend_from_slice(&digest);
            bytes.extend_from_slice(&range.offset.to_le_bytes());
            bytes.extend_from_slice(&range.length.to_le_bytes());
        }
        ensure!(
            bytes.len() as u64 <= MAX_INDEX,
            "native asset index exceeds limit"
        );
        let offset = self.file.stream_position()?;
        self.file.write_all(&bytes)?;
        let length = bytes.len() as u64;
        ensure!(
            offset + length <= MAX_ARCHIVE,
            "native asset archive exceeds size limit"
        );
        self.file.seek(SeekFrom::Start(0))?;
        self.file.write_all(MAGIC)?;
        self.file.write_all(&offset.to_le_bytes())?;
        self.file.write_all(&length.to_le_bytes())?;
        self.file.write_all(blake3::hash(&bytes).as_bytes())?;
        self.file.sync_all()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
