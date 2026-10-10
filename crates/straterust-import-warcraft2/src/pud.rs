//! Bounded campaign-map decoding. PUD chunks remain an importer detail.
use anyhow::{Context, Result, ensure};
use std::collections::BTreeMap;

pub struct Pud {
    pub chunks: BTreeMap<[u8; 4], Vec<u8>>,
    pub width: u16,
    pub height: u16,
    pub era: usize,
    pub units: Vec<Placed>,
    pub owners: [u8; 16],
    pub sides: [u8; 16],
    pub local: usize,
    pub title: String,
}

#[derive(Clone, Copy)]
pub struct Placed {
    pub x: u16,
    pub y: u16,
    pub kind: u8,
    pub owner: u8,
    pub data: u16,
}

pub fn word(bytes: &[u8], offset: usize) -> Result<u16> {
    Ok(u16::from_le_bytes(
        bytes
            .get(offset..offset + 2)
            .context("truncated Warcraft II word")?
            .try_into()?,
    ))
}

impl Pud {
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        ensure!(bytes.len() <= 4 * 1024 * 1024, "PUD exceeds size limit");
        let mut chunks = BTreeMap::new();
        let mut p = 0;
        while p < bytes.len() {
            let head = bytes.get(p..p + 8).context("truncated PUD chunk header")?;
            let name: [u8; 4] = head[..4].try_into()?;
            let length = u32::from_le_bytes(head[4..].try_into()?) as usize;
            p += 8;
            let end = p.checked_add(length).context("PUD chunk length overflow")?;
            let body = bytes.get(p..end).context("truncated PUD chunk")?;
            ensure!(
                chunks.insert(name, body.to_vec()).is_none(),
                "duplicate PUD chunk"
            );
            p = end;
        }
        let chunk = |name: &[u8; 4]| -> Result<&[u8]> {
            chunks
                .get(name)
                .map(Vec::as_slice)
                .context("missing PUD section")
        };
        ensure!(
            chunk(b"TYPE")?.starts_with(b"WAR2 MAP\0"),
            "not a Warcraft II map"
        );
        let width = word(chunk(b"DIM ")?, 0)?;
        let height = word(chunk(b"DIM ")?, 2)?;
        ensure!(
            (16..=128).contains(&width) && (16..=128).contains(&height),
            "invalid PUD dimensions"
        );
        ensure!(
            chunk(b"MTXM")?.len() == usize::from(width) * usize::from(height) * 2,
            "invalid tile count"
        );
        let owners: [u8; 16] = chunk(b"OWNR")?.try_into().context("invalid player table")?;
        let sides: [u8; 16] = chunk(b"SIDE")?.try_into().context("invalid race table")?;
        let local = owners
            .iter()
            .position(|o| *o == 5)
            .context("campaign has no human player")?;
        let era = usize::from(word(chunk(b"ERAX").or_else(|_| chunk(b"ERA "))?, 0)?);
        ensure!(era < 4, "unsupported tileset");
        let mut units = Vec::new();
        let raw = chunk(b"UNIT")?;
        ensure!(
            raw.len().is_multiple_of(8) && raw.len() / 8 <= 4096,
            "invalid placed unit count"
        );
        for entry in raw.as_chunks::<8>().0 {
            let placed = Placed {
                x: word(entry, 0)?,
                y: word(entry, 2)?,
                kind: entry[4],
                owner: entry[5],
                data: word(entry, 6)?,
            };
            ensure!(
                placed.x < width && placed.y < height && placed.kind < 110 && placed.owner < 16,
                "invalid placed unit"
            );
            units.push(placed);
        }
        let title = chunks
            .get(b"DESC")
            .map(|b| {
                String::from_utf8_lossy(b.split(|v| *v == 0).next().unwrap_or_default())
                    .trim()
                    .to_owned()
            })
            .unwrap_or_default();
        Ok(Self {
            chunks,
            width,
            height,
            era,
            units,
            owners,
            sides,
            local,
            title,
        })
    }

    pub fn words(&self, name: &[u8; 4]) -> Result<Vec<u16>> {
        let bytes = self.chunks.get(name).context("missing map grid")?;
        ensure!(bytes.len().is_multiple_of(2), "odd map grid length");
        bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|b| word(b, 0))
            .collect()
    }

    /// Swap the local source slot with zero, preserving all other source slots.
    pub fn player(&self, source: usize) -> u8 {
        if source == self.local {
            0
        } else if source == 0 {
            self.local as u8
        } else {
            source as u8
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn malformed_maps_fail_without_allocating_the_declared_length() {
        assert!(Pud::decode(b"TYPE\xff\xff\xff\xff").is_err());
        assert!(Pud::decode(b"TYPE\x04\0\0\0test").is_err());
    }
}
