//! Hash-addressed native definitions shared by installed map packages.
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileRef {
    pub file: String,
    pub blake3: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SharedDocument {
    pub shared: FileRef,
}

pub fn read_document(path: &Path, limit: usize) -> Result<Vec<u8>> {
    let bytes = crate::assets::read_bounded(path, limit)?;
    if let Ok(reference) = ron::de::from_bytes::<SharedDocument>(&bytes) {
        return crate::asset_archive::Reader::open(
            path.parent().context("missing document parent")?,
        )?
        .read(&reference.shared.file, &reference.shared.blake3, limit);
    }
    Ok(bytes)
}

pub(crate) fn already_shared(path: &Path) -> Result<bool> {
    let mut prefix = Vec::new();
    File::open(path)?.take(32).read_to_end(&mut prefix)?;
    Ok(String::from_utf8_lossy(&prefix)
        .trim_start()
        .starts_with("(shared:"))
}
