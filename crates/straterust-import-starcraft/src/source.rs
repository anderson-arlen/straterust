//! Read-only source resolution; no mounting or extraction is required.
//!
//! Accepts a direct installer/MPQ, a directory containing INSTALL.EXE, or an
//! ISO9660 image with a single-volume primary descriptor in sectors 16..=32,
//! 2048-byte logical blocks, and one contiguous root INSTALL.EXE[;1] entry.
//! Nested paths, multi-extent/interleaved files, extended attributes, raw CD
//! sectors, UDF, and supplementary-only directory trees are unsupported.
//! Layout reference: ECMA-119, sections 8.4 and 9.1:
//! https://www.ecma-international.org/wp-content/uploads/ECMA-119_3rd_edition_december_2017.pdf

use std::{
    fs::{self, File},
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail, ensure};

const SECTOR_BYTES: u64 = 2048;
const MAX_ROOT_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Debug)]
pub struct Source {
    pub path: PathBuf,
    pub offset: u64,
    pub len: u64,
    pub kind: String,
    pub volume_label: Option<String>,
}

impl Source {
    pub fn open(path: &Path) -> Result<Self> {
        let metadata = fs::metadata(path)
            .with_context(|| format!("cannot inspect source {}", path.display()))?;
        let path = if metadata.is_dir() {
            let mut installer = None;
            for (index, entry) in fs::read_dir(path)?.enumerate() {
                ensure!(index < 65_536, "source directory exceeds 65536 entries");
                let entry = entry?;
                if entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| name.eq_ignore_ascii_case("INSTALL.EXE"))
                {
                    ensure!(
                        installer.is_none(),
                        "multiple case variants of INSTALL.EXE in source directory"
                    );
                    installer = Some(entry.path());
                }
            }
            installer.context("source directory must contain INSTALL.EXE directly; nested installation paths are unsupported")?
        } else {
            path.to_path_buf()
        };
        let metadata = fs::metadata(&path)?;
        ensure!(metadata.is_file(), "source must be a regular file");
        let len = metadata.len();
        ensure!(len > 0, "source file is empty");
        let mut file =
            File::open(&path).with_context(|| format!("cannot open source {}", path.display()))?;
        let iso_extension = path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("iso"));
        let mut signature = [0; 7];
        if len >= 16 * SECTOR_BYTES + signature.len() as u64 {
            file.seek(SeekFrom::Start(16 * SECTOR_BYTES))?;
            file.read_exact(&mut signature)?;
        }
        if iso_extension || &signature[1..6] == b"CD001" {
            let (offset, len, label) = iso_installer(&mut file, len)
                .with_context(|| format!("unsupported or malformed ISO {}", path.display()))?;
            return Ok(Self {
                path,
                offset,
                len,
                kind: "iso9660-install-exe".into(),
                volume_label: Some(label),
            });
        }
        let installer = path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case("INSTALL.EXE"));
        Ok(Self {
            path,
            offset: 0,
            len,
            kind: if installer { "install-exe" } else { "mpq" }.into(),
            volume_label: None,
        })
    }
}

fn iso_installer(file: &mut File, source_len: u64) -> Result<(u64, u64, String)> {
    let mut descriptor = [0; SECTOR_BYTES as usize];
    let mut primary_sector = None;
    for sector in 16..=32 {
        let offset = sector * SECTOR_BYTES;
        check_range(offset, SECTOR_BYTES, source_len, "volume descriptor")?;
        file.seek(SeekFrom::Start(offset))?;
        file.read_exact(&mut descriptor)?;
        ensure!(
            &descriptor[1..6] == b"CD001" && descriptor[6] == 1,
            "expected ISO9660 CD001 version 1 descriptor at sector {sector}"
        );
        match descriptor[0] {
            1 => {
                primary_sector = Some(sector);
                break;
            }
            255 => break,
            0 | 2 | 3 => {}
            other => bail!("unsupported ISO descriptor type {other}"),
        }
    }
    let primary_sector =
        primary_sector.context("no primary volume descriptor in sectors 16..=32")?;
    ensure!(
        both_u16(&descriptor, 128)? == 2048,
        "only 2048-byte ISO logical blocks are supported"
    );
    ensure!(
        both_u16(&descriptor, 120)? == 1 && both_u16(&descriptor, 124)? == 1,
        "multi-volume ISO sets are unsupported"
    );
    ensure!(
        descriptor[881] == 1,
        "unsupported ISO file structure version"
    );
    let volume_len = u64::from(both_u32(&descriptor, 80)?) * SECTOR_BYTES;
    ensure!(
        volume_len <= source_len && volume_len >= (primary_sector + 1) * SECTOR_BYTES,
        "ISO volume space exceeds the source or omits its primary descriptor"
    );
    let label = std::str::from_utf8(&descriptor[40..72])
        .context("ISO volume label is not ASCII")?
        .trim_end_matches(' ')
        .to_string();
    ensure!(
        label.bytes().all(|byte| (32..=126).contains(&byte)),
        "invalid ISO volume label"
    );
    ensure!(
        descriptor[156] == 34,
        "invalid ISO root directory record size"
    );
    let root = directory_record(&descriptor[156..190], volume_len)?;
    ensure!(
        root.name == [0] && root.flags == 2,
        "invalid ISO root directory record"
    );
    ensure!(
        (1..=MAX_ROOT_BYTES).contains(&root.len),
        "ISO root directory must fit within 4 MiB"
    );
    let mut directory = vec![0; root.len as usize];
    file.seek(SeekFrom::Start(root.offset))?;
    file.read_exact(&mut directory)?;
    let mut cursor = 0;
    let mut installer = None;
    while cursor < directory.len() {
        let length = usize::from(directory[cursor]);
        if length == 0 {
            let next_sector = ((cursor / SECTOR_BYTES as usize) + 1) * SECTOR_BYTES as usize;
            let end = next_sector.min(directory.len());
            ensure!(
                directory[cursor..end].iter().all(|byte| *byte == 0),
                "nonzero ISO directory sector padding"
            );
            cursor = end;
            continue;
        }
        ensure!(
            length <= directory.len() - cursor
                && cursor % SECTOR_BYTES as usize + length <= SECTOR_BYTES as usize,
            "ISO directory record is truncated or crosses a sector boundary"
        );
        let record = directory_record(&directory[cursor..cursor + length], volume_len)?;
        if record.name.eq_ignore_ascii_case(b"INSTALL.EXE")
            || record.name.eq_ignore_ascii_case(b"INSTALL.EXE;1")
        {
            ensure!(
                record.flags & 2 == 0,
                "root INSTALL.EXE is a directory; nested paths are unsupported"
            );
            ensure!(
                record.flags & !1 == 0,
                "unsupported INSTALL.EXE flags (multi-extent or associated file)"
            );
            ensure!(record.len > 0, "ISO INSTALL.EXE is empty");
            ensure!(installer.is_none(), "multiple ISO root INSTALL.EXE entries");
            installer = Some((record.offset, record.len));
        }
        cursor += length;
    }
    let (offset, len) = installer.context(
        "no root INSTALL.EXE[;1] in ISO primary directory; nested paths are unsupported",
    )?;
    Ok((offset, len, label))
}

struct DirectoryRecord<'a> {
    offset: u64,
    len: u64,
    flags: u8,
    name: &'a [u8],
}

fn directory_record(bytes: &[u8], volume_len: u64) -> Result<DirectoryRecord<'_>> {
    ensure!(
        bytes.len() >= 34 && usize::from(bytes[0]) == bytes.len(),
        "invalid ISO directory record size"
    );
    ensure!(
        bytes[1] == 0,
        "ISO extended attribute records are unsupported"
    );
    ensure!(
        bytes[26] == 0 && bytes[27] == 0,
        "interleaved ISO files are unsupported"
    );
    ensure!(
        both_u16(bytes, 28)? == 1,
        "directory record references another ISO volume"
    );
    let name_len = usize::from(bytes[32]);
    ensure!(
        name_len > 0 && 33 + name_len + usize::from(name_len % 2 == 0) <= bytes.len(),
        "invalid ISO file identifier length"
    );
    let offset = u64::from(both_u32(bytes, 2)?) * SECTOR_BYTES;
    let len = u64::from(both_u32(bytes, 10)?);
    check_range(offset, len, volume_len, "directory entry extent")?;
    Ok(DirectoryRecord {
        offset,
        len,
        flags: bytes[25],
        name: &bytes[33..33 + name_len],
    })
}

fn check_range(offset: u64, len: u64, end: u64, description: &str) -> Result<()> {
    ensure!(
        offset <= end && len <= end - offset,
        "ISO {description} exceeds its source bounds"
    );
    Ok(())
}

fn both_u32(bytes: &[u8], offset: usize) -> Result<u32> {
    let little = u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
    let big = u32::from_be_bytes(bytes[offset + 4..offset + 8].try_into().unwrap());
    ensure!(
        little == big,
        "ISO little/big-endian u32 fields disagree at byte {offset}"
    );
    Ok(little)
}

fn both_u16(bytes: &[u8], offset: usize) -> Result<u16> {
    let little = u16::from_le_bytes(bytes[offset..offset + 2].try_into().unwrap());
    let big = u16::from_be_bytes(bytes[offset + 2..offset + 4].try_into().unwrap());
    ensure!(
        little == big,
        "ISO little/big-endian u16 fields disagree at byte {offset}"
    );
    Ok(little)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "straterust-source-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }

        fn write(&self, name: &str, bytes: &[u8]) -> PathBuf {
            let path = self.0.join(name);
            fs::write(&path, bytes).unwrap();
            path
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    fn set_u32(bytes: &mut [u8], offset: usize, value: u32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        bytes[offset + 4..offset + 8].copy_from_slice(&value.to_be_bytes());
    }

    fn set_u16(bytes: &mut [u8], offset: usize, value: u16) {
        bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
        bytes[offset + 2..offset + 4].copy_from_slice(&value.to_be_bytes());
    }

    fn record(name: &[u8], sector: u32, length: u32, flags: u8) -> Vec<u8> {
        let size = 33 + name.len() + usize::from(name.len().is_multiple_of(2));
        let mut bytes = vec![0; size];
        bytes[0] = size as u8;
        set_u32(&mut bytes, 2, sector);
        set_u32(&mut bytes, 10, length);
        bytes[25] = flags;
        set_u16(&mut bytes, 28, 1);
        bytes[32] = name.len() as u8;
        bytes[33..33 + name.len()].copy_from_slice(name);
        bytes
    }

    fn iso() -> Vec<u8> {
        let mut bytes = vec![0; 40 * SECTOR_BYTES as usize];
        let descriptor = &mut bytes[16 * 2048..17 * 2048];
        descriptor[0] = 1;
        descriptor[1..6].copy_from_slice(b"CD001");
        descriptor[6] = 1;
        descriptor[40..72].fill(b' ');
        descriptor[40..49].copy_from_slice(b"SYNTHETIC");
        set_u32(descriptor, 80, 40);
        set_u16(descriptor, 120, 1);
        set_u16(descriptor, 124, 1);
        set_u16(descriptor, 128, 2048);
        descriptor[156..190].copy_from_slice(&record(&[0], 20, 2048, 2));
        descriptor[881] = 1;
        let mut offset = 20 * 2048;
        for entry in [
            record(&[0], 20, 2048, 2),
            record(&[1], 20, 2048, 2),
            record(b"INSTALL.EXE;1", 24, 5, 0),
        ] {
            bytes[offset..offset + entry.len()].copy_from_slice(&entry);
            offset += entry.len();
        }
        bytes[24 * 2048..24 * 2048 + 5].copy_from_slice(b"hello");
        bytes
    }

    #[test]
    fn resolves_iso_region_without_extracting_or_modifying_the_source() {
        let fixture = Fixture::new();
        let original = iso();
        let path = fixture.write("disc.iso", &original);
        let source = Source::open(&path).unwrap();
        assert_eq!(source.offset, 24 * SECTOR_BYTES);
        assert_eq!(source.len, 5);
        assert_eq!(source.kind, "iso9660-install-exe");
        assert_eq!(source.volume_label.as_deref(), Some("SYNTHETIC"));
        assert_eq!(source.path, path);
        assert_eq!(fs::read(&source.path).unwrap(), original);
        assert_eq!(fs::read_dir(&fixture.0).unwrap().count(), 1);
        let mut delayed = original;
        delayed.copy_within(16 * 2048..17 * 2048, 17 * 2048);
        delayed[16 * 2048] = 0;
        assert!(Source::open(&fixture.write("boot-first.data", &delayed)).is_ok());
    }

    #[test]
    fn direct_sources_and_case_insensitive_directory_lookup() {
        let fixture = Fixture::new();
        let path = fixture.write("install.Exe", b"MPQ\x1aexample");
        let source = Source::open(&fixture.0).unwrap();
        assert_eq!(source.path, path);
        assert_eq!(source.offset, 0);
        assert_eq!(source.len, 11);
        assert_eq!(source.kind, "install-exe");
        assert!(source.volume_label.is_none());
        let mpq = fixture.write("data.mpq", b"MPQ\x1aexample");
        assert_eq!(Source::open(&mpq).unwrap().kind, "mpq");
        fixture.write("INSTALL.EXE", b"duplicate");
        assert!(Source::open(&fixture.0).is_err());
    }

    #[test]
    fn rejects_truncation_and_mismatched_volume_fields() {
        let fixture = Fixture::new();
        let original = iso();
        for length in [0, 15 * 2048, 16 * 2048 + 100, original.len() - 1] {
            assert!(Source::open(&fixture.write("bad.iso", &original[..length])).is_err());
        }
        for offset in [
            16 * 2048 + 1,
            16 * 2048 + 6,
            16 * 2048 + 84,
            16 * 2048 + 130,
            16 * 2048 + 156 + 6,
        ] {
            let mut bad = original.clone();
            bad[offset] ^= 1;
            assert!(Source::open(&fixture.write("bad.iso", &bad)).is_err());
        }
        let mut bad = original;
        set_u16(&mut bad, 16 * 2048 + 128, 4096);
        assert!(Source::open(&fixture.write("bad.iso", &bad)).is_err());
    }

    #[test]
    fn rejects_directory_and_installer_extent_bounds_and_unsupported_storage() {
        let fixture = Fixture::new();
        let original = iso();
        let installer = 20 * 2048 + 68;
        for (offset, value) in [
            (16 * 2048 + 156 + 2, u32::MAX),
            (16 * 2048 + 156 + 10, (MAX_ROOT_BYTES + 1) as u32),
            (installer + 2, 40),
            (installer + 10, u32::MAX),
        ] {
            let mut bad = original.clone();
            set_u32(&mut bad, offset, value);
            assert!(Source::open(&fixture.write("bad.iso", &bad)).is_err());
        }
        for (offset, value) in [
            (installer + 25, 2),
            (installer + 25, 128),
            (installer + 26, 1),
            (installer + 1, 1),
            (installer + 32, 250),
        ] {
            let mut bad = original.clone();
            bad[offset] = value;
            assert!(Source::open(&fixture.write("bad.iso", &bad)).is_err());
        }
        let mut bad = original;
        let duplicate = record(b"install.exe", 24, 5, 0);
        let offset = installer + usize::from(bad[installer]);
        bad[offset..offset + duplicate.len()].copy_from_slice(&duplicate);
        assert!(Source::open(&fixture.write("bad.iso", &bad)).is_err());
    }
}
