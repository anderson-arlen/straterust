//! Read version evidence without loading or executing a Windows executable.
//!
//! Layout references: <https://learn.microsoft.com/en-us/windows/win32/debug/pe-format>,
//! <https://learn.microsoft.com/en-us/windows/win32/menurc/vs-versioninfo>, and
//! <https://learn.microsoft.com/en-us/windows/win32/api/verrsrc/ns-verrsrc-vs_fixedfileinfo>.

use anyhow::{Context, Result, ensure};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct WindowsVersion {
    /// The fixed file version; this is not necessarily the game's patch label.
    pub file_version: String,
    /// The distinct product version from VS_FIXEDFILEINFO.
    pub product_version: String,
    /// Language of the selected version resource, not inferred from text.
    pub language: String,
}

/// Inspect a PE32 i386 executable with one VERSIONINFO resource. This is a
/// deliberately limited metadata reader, not a general executable loader.
pub fn windows_version(data: &[u8]) -> Result<WindowsVersion> {
    ensure!(bytes(data, 0, 2)? == b"MZ", "missing DOS MZ signature");
    let pe_offset = usize::try_from(dword(data, 0x3c)?)?;
    let pe = bytes(data, pe_offset, 24).context("truncated PE header")?;
    ensure!(&pe[..4] == b"PE\0\0", "missing PE signature");
    ensure!(word(pe, 4)? == 0x14c, "expected Windows i386 executable");
    let section_count = usize::from(word(pe, 6)?);
    ensure!(
        (1..=96).contains(&section_count),
        "invalid PE section count"
    );
    let optional_length = usize::from(word(pe, 20)?);
    let optional_offset = pe_offset.checked_add(24).context("PE offset overflow")?;
    let optional = bytes(data, optional_offset, optional_length)?;
    ensure!(word(optional, 0)? == 0x10b, "expected PE32 optional header");
    ensure!(
        dword(optional, 92)? >= 3,
        "PE has no resource data directory"
    );
    let resource_rva = dword(optional, 112)?;
    let resource_length = usize::try_from(dword(optional, 116)?)?;
    ensure!(
        resource_rva != 0 && (16..=16 * 1024 * 1024).contains(&resource_length),
        "PE resource directory is absent or exceeds the 16 MiB limit"
    );
    let section_offset = optional_offset
        .checked_add(optional_length)
        .context("PE section offset overflow")?;
    let sections = bytes(data, section_offset, section_count * 40)?;
    let resources = rva_bytes(data, sections, resource_rva, resource_length)?;
    let types = directory(resources, 0)?;
    let version_type = types
        .iter()
        .find(|(id, _)| *id == 16)
        .context("PE has no RT_VERSION resource")?;
    let names = directory(resources, subdirectory(version_type.1)?)?;
    ensure!(
        names.len() == 1,
        "expected exactly one VERSIONINFO resource"
    );
    let languages = directory(resources, subdirectory(names[0].1)?)?;
    let &(language, leaf_offset) = languages
        .iter()
        .find(|(id, _)| *id == 0x0409)
        .or_else(|| languages.first())
        .context("VERSIONINFO resource has no language entry")?;
    ensure!(
        language <= u16::MAX.into(),
        "invalid VERSIONINFO language identifier"
    );
    ensure!(
        leaf_offset & 0x8000_0000 == 0,
        "VERSIONINFO language points to a directory"
    );
    let leaf = bytes(resources, usize::try_from(leaf_offset)?, 16)?;
    let version_length = usize::try_from(dword(leaf, 4)?)?;
    ensure!(
        (6..=65535).contains(&version_length),
        "VERSIONINFO size is outside the 6..=65535 byte limit"
    );
    let version = rva_bytes(data, sections, dword(leaf, 0)?, version_length)?;
    let declared_length = usize::from(word(version, 0)?);
    let version = bytes(version, 0, declared_length).context("invalid VERSIONINFO length")?;
    ensure!(
        word(version, 2)? == 52,
        "VERSIONINFO has no complete fixed version record"
    );
    ensure!(
        word(version, 4)? == 0,
        "VERSIONINFO fixed value must be binary"
    );
    let key: Vec<u8> = "VS_VERSION_INFO\0"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    ensure!(
        bytes(version, 6, key.len())? == key,
        "invalid VS_VERSION_INFO key"
    );
    let fixed_offset = (6 + key.len()).next_multiple_of(4);
    let fixed = bytes(version, fixed_offset, 52)?;
    ensure!(
        dword(fixed, 0)? == 0xfeef04bd,
        "invalid fixed version signature"
    );
    ensure!(
        dword(fixed, 4)? == 0x0001_0000,
        "unsupported fixed version structure"
    );
    Ok(WindowsVersion {
        file_version: version_string(dword(fixed, 8)?, dword(fixed, 12)?),
        product_version: version_string(dword(fixed, 16)?, dword(fixed, 20)?),
        language: if language == 0x0409 {
            "English (United States, resource language 0x0409)".into()
        } else {
            format!("resource language 0x{language:04x}")
        },
    })
}

fn version_string(ms: u32, ls: u32) -> String {
    format!("{}.{}.{}.{}", ms >> 16, ms & 0xffff, ls >> 16, ls & 0xffff)
}

fn subdirectory(offset: u32) -> Result<usize> {
    ensure!(offset & 0x8000_0000 != 0, "expected resource subdirectory");
    Ok(usize::try_from(offset & 0x7fff_ffff)?)
}

fn directory(data: &[u8], offset: usize) -> Result<Vec<(u32, u32)>> {
    let header = bytes(data, offset, 16).context("invalid PE resource directory offset")?;
    let count = usize::from(word(header, 12)?) + usize::from(word(header, 14)?);
    ensure!(count <= 4096, "PE resource directory exceeds 4096 entries");
    let entries_offset = offset.checked_add(16).context("resource offset overflow")?;
    bytes(data, entries_offset, count * 8)?
        .as_chunks::<8>()
        .0
        .iter()
        .map(|entry| Ok((dword(entry, 0)?, dword(entry, 4)?)))
        .collect()
}

fn rva_bytes<'a>(data: &'a [u8], sections: &[u8], rva: u32, length: usize) -> Result<&'a [u8]> {
    let mut found = None;
    for section in sections.as_chunks::<40>().0 {
        let virtual_address = dword(section, 12)?;
        let raw_length = usize::try_from(dword(section, 16)?)?;
        let Some(relative) = rva.checked_sub(virtual_address) else {
            continue;
        };
        let relative = usize::try_from(relative)?;
        if relative
            .checked_add(length)
            .is_none_or(|end| end > raw_length)
        {
            continue;
        }
        ensure!(
            found.is_none(),
            "resource RVA maps to overlapping PE sections"
        );
        let offset = usize::try_from(dword(section, 20)?)?
            .checked_add(relative)
            .context("resource file offset overflow")?;
        found = Some(bytes(data, offset, length)?);
    }
    found.context("resource RVA or size is outside the file-backed PE sections")
}

fn bytes(data: &[u8], offset: usize, length: usize) -> Result<&[u8]> {
    let end = offset
        .checked_add(length)
        .context("PE byte range overflow")?;
    data.get(offset..end)
        .with_context(|| format!("PE byte range {offset}..{end} exceeds {} bytes", data.len()))
}

fn word(data: &[u8], offset: usize) -> Result<u16> {
    Ok(u16::from_le_bytes(bytes(data, offset, 2)?.try_into()?))
}

fn dword(data: &[u8], offset: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(bytes(data, offset, 4)?.try_into()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put_word(data: &mut [u8], offset: usize, value: u16) {
        data[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }

    fn put_dword(data: &mut [u8], offset: usize, value: u32) {
        data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    // Original minimal metadata-only PE fixture. It contains no executable code.
    fn fixture() -> Vec<u8> {
        let mut data = vec![0; 0x400];
        data[..2].copy_from_slice(b"MZ");
        put_dword(&mut data, 0x3c, 0x80);
        data[0x80..0x84].copy_from_slice(b"PE\0\0");
        put_word(&mut data, 0x84, 0x14c);
        put_word(&mut data, 0x86, 1);
        put_word(&mut data, 0x94, 224);
        put_word(&mut data, 0x98, 0x10b);
        put_dword(&mut data, 0x98 + 92, 16);
        put_dword(&mut data, 0x98 + 112, 0x1000);
        put_dword(&mut data, 0x98 + 116, 180);
        data[0x178..0x180].copy_from_slice(b".rsrc\0\0\0");
        put_dword(&mut data, 0x178 + 8, 180);
        put_dword(&mut data, 0x178 + 12, 0x1000);
        put_dword(&mut data, 0x178 + 16, 0x200);
        put_dword(&mut data, 0x178 + 20, 0x200);
        for offset in [0, 24, 48] {
            put_word(&mut data, 0x200 + offset + 14, 1);
        }
        for (offset, id, target) in [(16, 16, 0x8000_0018), (40, 1, 0x8000_0030), (64, 0x409, 72)] {
            put_dword(&mut data, 0x200 + offset, id);
            put_dword(&mut data, 0x200 + offset + 4, target);
        }
        put_dword(&mut data, 0x200 + 72, 0x1000 + 88);
        put_dword(&mut data, 0x200 + 76, 92);
        let version = &mut data[0x258..0x2b4];
        put_word(version, 0, 92);
        put_word(version, 2, 52);
        for (index, unit) in "VS_VERSION_INFO\0".encode_utf16().enumerate() {
            put_word(version, 6 + index * 2, unit);
        }
        for (offset, value) in [
            (40, 0xfeef04bd),
            (44, 0x10000),
            (48, 0x1000a),
            (52, 0x20003),
            (56, 0x10000),
        ] {
            put_dword(version, offset, value);
        }
        data
    }

    #[test]
    fn reports_distinct_fixed_versions_and_resource_language() {
        let version = windows_version(&fixture()).unwrap();
        assert_eq!(version.file_version, "1.10.2.3");
        assert_eq!(version.product_version, "1.0.0.0");
        assert!(version.language.contains("English (United States"));
        let mut other = fixture();
        put_dword(&mut other, 0x240, 0x407);
        assert_eq!(
            windows_version(&other).unwrap().language,
            "resource language 0x0407"
        );
    }

    #[test]
    fn rejects_truncated_headers_tables_and_version_record() {
        let data = fixture();
        for end in 0..0x2b4 {
            assert!(windows_version(&data[..end]).is_err(), "truncated at {end}");
        }
    }

    #[test]
    fn rejects_corrupt_pe_and_resource_offsets_and_signatures() {
        for (offset, value) in [
            (0x3c, u32::MAX),     // PE offset
            (0x108, 0xffff_f000), // resource RVA
            (0x10c, u32::MAX),    // resource length
            (0x214, 0x8000_ffff), // resource subdirectory
            (0x244, 0x8000_0000), // language leaf is a directory
            (0x248, 0x5000),      // version RVA
            (0x24c, 65536),       // version size
            (0x280, 0),           // fixed signature
        ] {
            let mut data = fixture();
            put_dword(&mut data, offset, value);
            assert!(
                windows_version(&data).is_err(),
                "corrupt offset {offset:#x}"
            );
        }
        let mut data = fixture();
        data[0] = 0;
        assert!(windows_version(&data).is_err());
        let mut data = fixture();
        put_word(&mut data, 0x98, 0x20b);
        assert!(windows_version(&data).is_err());
    }
}
