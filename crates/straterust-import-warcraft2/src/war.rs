//! Warcraft II WAR/TOME directory and LZSS decoding, implemented from the
//! on-disc layout. Reference: Wargus/wargus wartool.cpp (format documentation).
use anyhow::{Context, Result, ensure};
use std::{fs, path::Path};

pub fn u16_at(bytes: &[u8], offset: usize) -> Result<u16> {
    Ok(u16::from_le_bytes(
        bytes
            .get(offset..offset + 2)
            .context("truncated u16")?
            .try_into()?,
    ))
}
pub fn u32_at(bytes: &[u8], offset: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(
        bytes
            .get(offset..offset + 4)
            .context("truncated u32")?
            .try_into()?,
    ))
}

pub struct WarArchive {
    bytes: Vec<u8>,
    offsets: Vec<usize>,
}

impl WarArchive {
    pub fn open(path: &Path, kind: u16) -> Result<Self> {
        ensure!(
            fs::metadata(path)?.len() <= 128 * 1024 * 1024,
            "WAR archive exceeds 128 MiB"
        );
        Self::decode(fs::read(path)?, kind)
    }
    fn decode(bytes: Vec<u8>, kind: u16) -> Result<Self> {
        ensure!(u32_at(&bytes, 0)? == 0x19, "unsupported WAR archive magic");
        ensure!(u16_at(&bytes, 6)? == kind, "unsupported WAR archive type");
        let count = usize::from(u16_at(&bytes, 4)?);
        ensure!(
            count > 0 && 8 + count * 4 <= bytes.len(),
            "truncated WAR directory"
        );
        let offsets = (0..count)
            .map(|i| u32_at(&bytes, 8 + i * 4).map(|v| v as usize))
            .collect::<Result<Vec<_>>>()?;
        Ok(Self { bytes, offsets })
    }
    pub fn entry(&self, index: usize) -> Result<Vec<u8>> {
        let offset = *self
            .offsets
            .get(index)
            .context("WAR entry index out of bounds")?;
        let header = u32_at(&self.bytes, offset)?;
        let length = (header & 0xffffff) as usize;
        ensure!(length <= 16 * 1024 * 1024, "WAR entry exceeds limit");
        let end = self
            .offsets
            .iter()
            .copied()
            .filter(|n| *n > offset)
            .min()
            .unwrap_or(self.bytes.len());
        let bytes = self
            .bytes
            .get(offset + 4..end)
            .context("WAR entry outside archive")?;
        match header >> 24 {
            0 => Ok(bytes.get(..length).context("truncated WAR entry")?.to_vec()),
            0x20 => unpack(bytes, length),
            _ => anyhow::bail!("unsupported WAR compression"),
        }
    }
}

fn unpack(bytes: &[u8], length: usize) -> Result<Vec<u8>> {
    let mut output = Vec::with_capacity(length);
    let mut cursor = 0;
    let mut dictionary = [0u8; 4096];
    while output.len() < length {
        let control = *bytes.get(cursor).context("truncated LZSS control")?;
        cursor += 1;
        for bit in 0..8 {
            if output.len() == length {
                break;
            }
            if control & (1 << bit) != 0 {
                let byte = *bytes.get(cursor).context("truncated LZSS literal")?;
                cursor += 1;
                dictionary[output.len() % 4096] = byte;
                output.push(byte);
            } else {
                let reference = u16_at(bytes, cursor)?;
                cursor += 2;
                let count = usize::from(reference >> 12) + 3;
                let start = usize::from(reference & 0xfff);
                for i in 0..count.min(length - output.len()) {
                    let byte = dictionary[(start + i) % 4096];
                    dictionary[output.len() % 4096] = byte;
                    output.push(byte);
                }
            }
        }
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lzss_checks_truncation_and_supports_overlapping_references() {
        assert_eq!(unpack(&[1, b'a', 0, 0x20], 6).unwrap(), b"aaaaaa");
        assert!(unpack(&[1], 1).is_err());
        assert!(unpack(&[0, 1], 3).is_err());
        assert_eq!(unpack(&[0, 0, 0], 3).unwrap(), [0, 0, 0]);
    }
}
