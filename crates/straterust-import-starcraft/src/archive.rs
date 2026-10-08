//! Bounded reader for the classic MPQ subset used by the reference disc.
//!
//! Format references: StormLib's `SBaseCommon.cpp` (hashing, encryption, tables)
//! and `SFileReadFile.cpp` (sector layout and decompression):
//! https://github.com/ladislav-zezula/StormLib/tree/master/src
//! This is deliberately not a general MPQ implementation. Unsupported archive
//! versions, file flags, locales, and compression methods produce errors.
//! Supported sector codecs are PKWARE and Huffman/mono/stereo ADPCM audio;
//! each decoded sector is limited to the size declared by the bounded archive.

use anyhow::{Context, Result, bail, ensure};
use implode::symbol::{DEFAULT_CODE_TABLE, Symbol, decode_bits};
use std::fs::File;
use std::io::{Cursor, Read, Seek, SeekFrom};
use std::path::Path;

#[path = "archive_audio.rs"]
mod audio;

const MAX_HEADER_SCAN: u64 = 16 * 1024 * 1024;
const MAX_TABLE_ENTRIES: u32 = 65_536;
const MAX_MEMBER_BYTES: usize = 128 * 1024 * 1024;
const IMPLODE: u32 = 0x100;
const COMPRESS: u32 = 0x200;
const ENCRYPTED: u32 = 0x1_0000;
const FIX_KEY: u32 = 0x2_0000;
const EXISTS: u32 = 0x8000_0000;
const ALLOWED_FLAGS: u32 = EXISTS | IMPLODE | COMPRESS | ENCRYPTED | FIX_KEY;

#[derive(Clone, Copy, Debug, serde::Serialize)]
pub struct ArchiveMetadata {
    /// Offset relative to the supplied region, not necessarily the outer file.
    pub header_offset: u64,
    pub archive_size: u32,
    pub hash_count: u32,
    pub block_count: u32,
    pub sector_size: u32,
}

#[derive(Clone, Copy)]
struct HashEntry {
    name_a: u32,
    name_b: u32,
    locale_platform: u32,
    block_index: u32,
}

#[derive(Clone, Copy)]
struct Block {
    offset: u32,
    packed_size: u32,
    size: u32,
    flags: u32,
}

pub struct Archive<R = File> {
    reader: R,
    archive_start: u64,
    metadata: ArchiveMetadata,
    hashes: Vec<HashEntry>,
    blocks: Vec<Block>,
}

impl Archive<File> {
    pub fn open(path: &Path) -> Result<Self> {
        let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
        let size = file.metadata()?.len();
        Self::from_region(file, 0, size)
    }

    /// Read an MPQ embedded in a bounded source file, such as an ISO entry.
    pub fn open_region(path: &Path, base_offset: u64, region_len: u64) -> Result<Self> {
        let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
        let size = file.metadata()?.len();
        ensure!(
            base_offset <= size && region_len <= size - base_offset,
            "MPQ source region exceeds the source file"
        );
        Self::from_region(file, base_offset, region_len)
    }
}

impl Archive<Cursor<Vec<u8>>> {
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self> {
        let size = bytes.len() as u64;
        Self::from_region(Cursor::new(bytes), 0, size)
    }
}

impl<R: Read + Seek> Archive<R> {
    fn from_region(mut reader: R, base: u64, region_len: u64) -> Result<Self> {
        ensure!(region_len >= 32, "source is too short for an MPQ header");
        let mut header = [0; 32];
        let mut header_offset = None;
        for offset in (0..region_len.min(MAX_HEADER_SCAN)).step_by(512) {
            if region_len - offset < 32 {
                break;
            }
            reader.seek(SeekFrom::Start(base + offset))?;
            reader.read_exact(&mut header)?;
            if header.starts_with(b"MPQ\x1a") {
                header_offset = Some(offset);
                break;
            }
        }
        let header_offset = header_offset.context("no classic MPQ header in first 16 MiB")?;
        ensure!(u32_at(&header, 4) == 32, "unsupported MPQ header size");
        ensure!(
            u16_at(&header, 12) == 0,
            "only classic MPQ version 0 is supported"
        );
        let shift = u16_at(&header, 14);
        ensure!(shift <= 7, "MPQ sectors larger than 64 KiB are unsupported");
        let metadata = ArchiveMetadata {
            header_offset,
            archive_size: u32_at(&header, 8),
            hash_count: u32_at(&header, 24),
            block_count: u32_at(&header, 28),
            sector_size: 512 << shift,
        };
        ensure!(
            metadata.archive_size >= 32
                && u64::from(metadata.archive_size) <= region_len - header_offset,
            "MPQ archive size exceeds its source region"
        );
        ensure!(
            metadata.hash_count.is_power_of_two() && metadata.hash_count <= MAX_TABLE_ENTRIES,
            "MPQ hash count must be a power of two between 1 and 65536"
        );
        ensure!(
            (1..=MAX_TABLE_ENTRIES).contains(&metadata.block_count),
            "MPQ block count must be between 1 and 65536"
        );
        let hash_offset = u32_at(&header, 16);
        let block_offset = u32_at(&header, 20);
        let hash_bytes = metadata.hash_count * 16;
        let block_bytes = metadata.block_count * 16;
        check_range(hash_offset, hash_bytes, metadata.archive_size, "hash table")?;
        check_range(
            block_offset,
            block_bytes,
            metadata.archive_size,
            "block table",
        )?;
        ensure!(
            u64::from(hash_offset) + u64::from(hash_bytes) <= u64::from(block_offset)
                || u64::from(block_offset) + u64::from(block_bytes) <= u64::from(hash_offset),
            "MPQ hash and block tables overlap"
        );
        let archive_start = base + header_offset;
        let mut table = read_at(
            &mut reader,
            archive_start + u64::from(hash_offset),
            hash_bytes as usize,
        )?;
        decrypt(&mut table, hash_name("(hash table)", 3));
        let hashes: Vec<_> = table
            .as_chunks::<16>()
            .0
            .iter()
            .map(|entry| HashEntry {
                name_a: u32_at(entry, 0),
                name_b: u32_at(entry, 4),
                locale_platform: u32_at(entry, 8),
                block_index: u32_at(entry, 12),
            })
            .collect();
        ensure!(
            hashes.iter().all(|entry| entry.block_index >= 0xffff_fffe
                || entry.block_index < metadata.block_count),
            "MPQ hash entry references an invalid block index"
        );
        let mut table = read_at(
            &mut reader,
            archive_start + u64::from(block_offset),
            block_bytes as usize,
        )?;
        decrypt(&mut table, hash_name("(block table)", 3));
        let blocks: Vec<_> = table
            .as_chunks::<16>()
            .0
            .iter()
            .map(|entry| Block {
                offset: u32_at(entry, 0),
                packed_size: u32_at(entry, 4),
                size: u32_at(entry, 8),
                flags: u32_at(entry, 12),
            })
            .collect();
        for block in blocks.iter().filter(|block| block.flags & EXISTS != 0) {
            check_range(
                block.offset,
                block.packed_size,
                metadata.archive_size,
                "file block",
            )?;
        }
        Ok(Self {
            reader,
            archive_start,
            metadata,
            hashes,
            blocks,
        })
    }

    pub fn metadata(&self) -> ArchiveMetadata {
        self.metadata
    }

    pub fn read_file(&mut self, name: &str, max_bytes: usize) -> Result<Vec<u8>> {
        self.read_member(name, max_bytes)
            .with_context(|| format!("MPQ member {name:?}"))
    }

    pub fn has_file(&self, name: &str) -> Result<bool> {
        match self.lookup(name) {
            Ok(_) => Ok(true),
            Err(error) if error.to_string() == "member not found" => Ok(false),
            Err(error) => Err(error),
        }
    }

    fn read_member(&mut self, name: &str, max_bytes: usize) -> Result<Vec<u8>> {
        ensure!(
            !name.is_empty()
                && name.len() <= 260
                && name.is_ascii()
                && !name
                    .bytes()
                    .any(|byte| byte < 32 || byte == b':' || byte == 127)
                && name
                    .split(['/', '\\'])
                    .all(|part| !part.is_empty() && part != "." && part != ".."),
            "member name must be a relative ASCII archive path without traversal"
        );
        let block = self.lookup(name)?;
        ensure!(
            block.flags & EXISTS != 0,
            "member block is not marked as present"
        );
        ensure!(
            block.flags & !ALLOWED_FLAGS == 0,
            "unsupported MPQ file flags {:#010x}",
            block.flags
        );
        ensure!(
            block.flags & (IMPLODE | COMPRESS) != IMPLODE | COMPRESS,
            "conflicting MPQ compression flags"
        );
        ensure!(
            block.flags & FIX_KEY == 0 || block.flags & ENCRYPTED != 0,
            "MPQ fixed key requires encryption"
        );
        let size = block.size as usize;
        ensure!(
            size <= max_bytes.min(MAX_MEMBER_BYTES),
            "member size {size} exceeds limit {}",
            max_bytes.min(MAX_MEMBER_BYTES)
        );
        if size == 0 {
            ensure!(block.packed_size == 0, "empty member has nonempty storage");
            return Ok(Vec::new());
        }
        let encrypted = block.flags & ENCRYPTED != 0;
        let basename = name
            .rsplit(['/', '\\'])
            .next()
            .context("missing member basename")?;
        let mut key = hash_name(basename, 3);
        if block.flags & FIX_KEY != 0 {
            key = key.wrapping_add(block.offset) ^ block.size;
        }
        let sector_size = self.metadata.sector_size as usize;
        let start = self.archive_start + u64::from(block.offset);
        if block.flags & (IMPLODE | COMPRESS) == 0 {
            ensure!(
                block.packed_size == block.size,
                "uncompressed MPQ member has inconsistent sizes"
            );
            let mut output = read_at(&mut self.reader, start, size)?;
            if encrypted {
                for (index, sector) in output.chunks_mut(sector_size).enumerate() {
                    decrypt(sector, key.wrapping_add(index as u32));
                }
            }
            return Ok(output);
        }
        let sector_count = size.div_ceil(sector_size);
        let offset_bytes = (sector_count + 1) * 4;
        ensure!(
            offset_bytes <= block.packed_size as usize,
            "MPQ sector offset table exceeds member storage"
        );
        let mut offset_data = read_at(&mut self.reader, start, offset_bytes)?;
        if encrypted {
            decrypt(&mut offset_data, key.wrapping_sub(1));
        }
        let offsets: Vec<_> = offset_data
            .as_chunks::<4>()
            .0
            .iter()
            .map(|bytes| u32_at(bytes, 0))
            .collect();
        ensure!(
            offsets[0] as usize == offset_bytes,
            "invalid first MPQ sector offset"
        );
        ensure!(
            offsets[sector_count] == block.packed_size,
            "final MPQ sector offset does not match member size"
        );
        let mut output = Vec::with_capacity(size);
        for (index, pair) in offsets.windows(2).enumerate() {
            let expected = sector_size.min(size - output.len());
            ensure!(
                pair[0] < pair[1] && pair[1] <= block.packed_size,
                "invalid MPQ sector {index} offset range"
            );
            let packed = (pair[1] - pair[0]) as usize;
            ensure!(
                packed <= expected,
                "MPQ sector {index} is larger than its decoded size"
            );
            let mut sector = read_at(&mut self.reader, start + u64::from(pair[0]), packed)?;
            if encrypted {
                decrypt(&mut sector, key.wrapping_add(index as u32));
            }
            if packed == expected {
                output.extend_from_slice(&sector);
            } else {
                let decoded = if block.flags & COMPRESS == 0 {
                    explode(&sector, expected)
                } else if sector[0] == 0x08 {
                    explode(&sector[1..], expected)
                } else {
                    audio::decode(sector[0], &sector[1..], expected)
                };
                output.extend_from_slice(&decoded.with_context(|| format!("sector {index}"))?);
            }
        }
        ensure!(output.len() == size, "decoded MPQ member size mismatch");
        Ok(output)
    }

    fn lookup(&self, name: &str) -> Result<Block> {
        let start = hash_name(name, 0) as usize & (self.hashes.len() - 1);
        let name_a = hash_name(name, 1);
        let name_b = hash_name(name, 2);
        let mut english = None;
        let mut other_locale = false;
        for step in 0..self.hashes.len() {
            let entry = self.hashes[(start + step) & (self.hashes.len() - 1)];
            if entry.block_index == u32::MAX {
                break;
            }
            if entry.block_index == 0xffff_fffe || entry.name_a != name_a || entry.name_b != name_b
            {
                continue;
            }
            let block = self.blocks[entry.block_index as usize];
            match entry.locale_platform {
                0 => return Ok(block),
                0x0409 => english = Some(block),
                _ => other_locale = true,
            }
        }
        if let Some(block) = english {
            return Ok(block);
        }
        ensure!(
            !other_locale,
            "member has no supported neutral or English Windows locale"
        );
        bail!("member not found")
    }
}

fn check_range(offset: u32, bytes: u32, archive_size: u32, label: &str) -> Result<()> {
    ensure!(
        offset >= 32 && offset <= archive_size && bytes <= archive_size - offset,
        "MPQ {label} exceeds archive bounds"
    );
    Ok(())
}

fn read_at(reader: &mut (impl Read + Seek), offset: u64, size: usize) -> Result<Vec<u8>> {
    reader.seek(SeekFrom::Start(offset))?;
    let mut bytes = vec![0; size];
    reader
        .read_exact(&mut bytes)
        .context("truncated MPQ data")?;
    Ok(bytes)
}

// Each caller supplies a fixed-size header or an exact table record.
fn u16_at(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}

/// Reuse the codec's symbol tables, but own output bounds and back references.
/// The dependency's higher-level Exploder accepts invalid dictionary sizes,
/// panics for ASCII mode, and cannot validate references before output begins.
fn explode(data: &[u8], expected: usize) -> Result<Vec<u8>> {
    ensure!(data.len() >= 2, "truncated PKWARE header");
    ensure!(data[0] == 0, "only binary PKWARE mode is supported");
    ensure!((4..=6).contains(&data[1]), "invalid PKWARE dictionary size");
    let dict_bits = u32::from(data[1]);
    let mut output = Vec::with_capacity(expected);
    let mut cursor = 2;
    let mut bits = 0_u64;
    let mut bit_count = 0;
    loop {
        while bit_count <= 56 && cursor < data.len() {
            bits |= u64::from(data[cursor]) << bit_count;
            bit_count += 8;
            cursor += 1;
        }
        let decoded = decode_bits(bits, bit_count, &DEFAULT_CODE_TABLE, dict_bits)
            .map_err(|_| anyhow::anyhow!("truncated PKWARE stream or missing end marker"))?;
        bits >>= decoded.used_bits;
        bit_count -= decoded.used_bits;
        match decoded.decoded {
            Symbol::End => {
                ensure!(
                    output.len() == expected,
                    "PKWARE decoded {} bytes, expected {expected}",
                    output.len()
                );
                return Ok(output);
            }
            Symbol::Literal(byte) => {
                ensure!(
                    output.len() < expected,
                    "PKWARE output exceeds declared sector size"
                );
                output.push(byte);
            }
            Symbol::Pair { distance, length } => {
                let distance = distance as usize;
                let length = length as usize;
                ensure!(
                    distance != 0 && distance <= output.len(),
                    "PKWARE back reference precedes decoded data"
                );
                ensure!(
                    length <= expected - output.len(),
                    "PKWARE output exceeds declared sector size"
                );
                for _ in 0..length {
                    output.push(output[output.len() - distance]);
                }
            }
        }
    }
}

const CRYPT: [u32; 1280] = crypt_table();

const fn crypt_table() -> [u32; 1280] {
    let mut table = [0; 1280];
    let mut seed = 0x10_0001;
    let mut byte = 0;
    while byte < 256 {
        let mut kind = 0;
        while kind < 5 {
            seed = (seed * 125 + 3) % 0x2a_aaab;
            let high = (seed & 0xffff) << 16;
            seed = (seed * 125 + 3) % 0x2a_aaab;
            table[byte + kind * 256] = high | (seed & 0xffff);
            kind += 1;
        }
        byte += 1;
    }
    table
}

fn hash_name(name: &str, kind: usize) -> u32 {
    let mut hash = 0x7fed_7fed_u32;
    let mut seed = 0xeeee_eeee_u32;
    for byte in name.bytes() {
        let byte = if byte == b'/' {
            b'\\'
        } else {
            byte.to_ascii_uppercase()
        };
        hash = CRYPT[kind * 256 + usize::from(byte)] ^ hash.wrapping_add(seed);
        seed = u32::from(byte)
            .wrapping_add(hash)
            .wrapping_add(seed)
            .wrapping_add(seed << 5)
            .wrapping_add(3);
    }
    hash
}

fn decrypt(bytes: &mut [u8], mut key: u32) {
    let mut seed = 0xeeee_eeee_u32;
    for word in bytes.as_chunks_mut::<4>().0 {
        seed = seed.wrapping_add(CRYPT[1024 + (key & 0xff) as usize]);
        let plain = u32_at(word, 0) ^ key.wrapping_add(seed);
        word.copy_from_slice(&plain.to_le_bytes());
        key = ((!key << 21).wrapping_add(0x1111_1111)) | (key >> 11);
        seed = plain
            .wrapping_add(seed)
            .wrapping_add(seed << 5)
            .wrapping_add(3);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Fixtures contain only text generated here, never bytes from a game disc.
    fn fixture(name: &str, payload: &[u8], size: u32, flags: u32) -> Vec<u8> {
        let hash_offset = 32 + payload.len();
        let block_offset = hash_offset + 64;
        let mut bytes = vec![0; block_offset + 16];
        bytes[..4].copy_from_slice(b"MPQ\x1a");
        for (offset, value) in [
            (4, 32),
            (8, bytes.len() as u32),
            (16, hash_offset as u32),
            (20, block_offset as u32),
            (24, 4),
            (28, 1),
        ] {
            bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        bytes[32..hash_offset].copy_from_slice(payload);
        bytes[hash_offset..block_offset].fill(0xff);
        let slot = hash_offset + (hash_name(name, 0) as usize & 3) * 16;
        for (index, value) in [hash_name(name, 1), hash_name(name, 2), 0, 0]
            .iter()
            .enumerate()
        {
            bytes[slot + index * 4..slot + index * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
        for (index, value) in [32, payload.len() as u32, size, flags].iter().enumerate() {
            bytes[block_offset + index * 4..block_offset + index * 4 + 4]
                .copy_from_slice(&value.to_le_bytes());
        }
        encrypt(
            &mut bytes[hash_offset..block_offset],
            hash_name("(hash table)", 3),
        );
        encrypt(&mut bytes[block_offset..], hash_name("(block table)", 3));
        bytes
    }

    fn encrypt(bytes: &mut [u8], mut key: u32) {
        let mut seed = 0xeeee_eeee_u32;
        for word in bytes.as_chunks_mut::<4>().0 {
            seed = seed.wrapping_add(CRYPT[1024 + (key & 0xff) as usize]);
            let plain = u32_at(word, 0);
            word.copy_from_slice(&(plain ^ key.wrapping_add(seed)).to_le_bytes());
            key = ((!key << 21).wrapping_add(0x1111_1111)) | (key >> 11);
            seed = plain
                .wrapping_add(seed)
                .wrapping_add(seed << 5)
                .wrapping_add(3);
        }
    }

    fn edit_table(bytes: &mut [u8], hash_table: bool, edit: impl FnOnce(&mut [u8])) {
        let offset = u32_at(bytes, if hash_table { 16 } else { 20 }) as usize;
        let len = u32_at(bytes, if hash_table { 24 } else { 28 }) as usize * 16;
        let key = hash_name(
            if hash_table {
                "(hash table)"
            } else {
                "(block table)"
            },
            3,
        );
        decrypt(&mut bytes[offset..offset + len], key);
        edit(&mut bytes[offset..offset + len]);
        encrypt(&mut bytes[offset..offset + len], key);
    }

    // Construct binary-mode PKWARE streams from literal text and one distance-1
    // run. This tiny test writer exercises the reader's overlapping copies.
    fn compressed_run(byte: u8, run: u32) -> Vec<u8> {
        let mut bits = Vec::new();
        let mut append = |value: u32, count: u32| {
            bits.extend((0..count).map(|bit| (value >> bit) as u8 & 1));
        };
        append(u32::from(byte) << 1, 9);
        let code = (0..16)
            .find(|&code| {
                let base = code as u32 + u32::from(DEFAULT_CODE_TABLE.len_add[code]);
                let span = 1_u32 << DEFAULT_CODE_TABLE.extra_len_bits[code];
                (base..base + span).contains(&(run - 2))
            })
            .unwrap();
        let prefix = DEFAULT_CODE_TABLE
            .len_codes
            .iter()
            .position(|&entry| usize::from(entry) == code)
            .unwrap();
        append(1, 1);
        append(prefix as u32, u32::from(DEFAULT_CODE_TABLE.len_bits[code]));
        append(
            run - 2 - code as u32 - u32::from(DEFAULT_CODE_TABLE.len_add[code]),
            u32::from(DEFAULT_CODE_TABLE.extra_len_bits[code]),
        );
        // Distance 1: distance code 0 is the two-bit prefix 11; low bits are 0.
        append(3, 2);
        append(0, if run == 2 { 2 } else { 4 });
        // End marker: length code 15, extra value 255.
        append(1, 1);
        append(0, 7);
        append(255, 8);
        let mut output = vec![0, 4];
        output.extend(bits.chunks(8).map(|chunk| {
            chunk
                .iter()
                .enumerate()
                .fold(0, |value, (index, bit)| value | (bit << index))
        }));
        output
    }

    #[test]
    fn reads_embedded_archive_with_relative_metadata() -> Result<()> {
        let member = fixture("test/hello.txt", b"hello", 5, EXISTS);
        let mut embedded = vec![0; 1024];
        embedded.extend_from_slice(&member);
        embedded.extend_from_slice(b"outside source region");
        let mut archive =
            Archive::from_region(Cursor::new(embedded), 512, (512 + member.len()) as u64)?;
        assert_eq!(archive.metadata().header_offset, 512);
        assert_eq!(archive.metadata().archive_size, member.len() as u32);
        assert_eq!(archive.metadata().sector_size, 512);
        assert_eq!(archive.read_file("TEST\\HELLO.TXT", 5)?, b"hello");
        assert!(archive.read_file("test/hello.txt", 4).is_err());
        Ok(())
    }

    #[test]
    fn rejects_invalid_headers_tables_and_source_bounds() {
        let valid = fixture("text", b"hello", 5, EXISTS);
        for len in 0..valid.len() {
            assert!(Archive::from_bytes(valid[..len].to_vec()).is_err());
        }
        for (offset, value) in [
            (4, 44),
            (8, u32::MAX),
            (12, 1),
            (12, 0xffff_0000),
            (16, u32::MAX),
            (20, 1),
            (24, 0),
            (24, 3),
            (24, 1 << 30),
            (28, u32::MAX),
        ] {
            let mut bytes = valid.clone();
            bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
            assert!(
                Archive::from_bytes(bytes).is_err(),
                "accepted header field {offset}={value}"
            );
        }
        let mut bytes = valid.clone();
        edit_table(&mut bytes, true, |table| {
            let slot = (hash_name("text", 0) as usize & 3) * 16;
            table[slot + 12..slot + 16].copy_from_slice(&100_u32.to_le_bytes());
        });
        assert!(Archive::from_bytes(bytes).is_err());
        let mut bytes = valid.clone();
        edit_table(&mut bytes, false, |table| {
            table[..4].copy_from_slice(&u32::MAX.to_le_bytes())
        });
        assert!(Archive::from_bytes(bytes).is_err());
        assert!(
            Archive::from_region(Cursor::new(valid.clone()), 0, valid.len() as u64 - 1).is_err()
        );
    }

    #[test]
    fn hash_probe_wraps_and_prefers_neutral_locale() -> Result<()> {
        let name = (0..100)
            .map(|value| format!("text{value}"))
            .find(|name| hash_name(name, 0) & 3 == 3)
            .unwrap();
        let mut bytes = fixture(&name, b"hello", 5, EXISTS);
        edit_table(&mut bytes, true, |table| {
            let entry: Vec<_> = table[48..64].to_vec();
            table[..16].copy_from_slice(&entry);
            // Deleted entries must be skipped, and probing wraps index 3 to 0.
            table[60..64].copy_from_slice(&0xffff_fffe_u32.to_le_bytes());
        });
        assert_eq!(Archive::from_bytes(bytes)?.read_file(&name, 10)?, b"hello");
        let mut archive = Archive::from_bytes(fixture(&name, b"hello", 5, EXISTS))?;
        // Same name with English first and neutral after wrap: neutral wins.
        archive.blocks.push(Block {
            offset: 32,
            packed_size: 5,
            size: 5,
            flags: EXISTS,
        });
        archive.hashes[0] = HashEntry {
            block_index: 1,
            ..archive.hashes[3]
        };
        archive.hashes[3].locale_platform = 0x0409;
        archive.blocks[0].size = 999;
        assert_eq!(archive.read_file(&name, 10)?, b"hello");
        archive.hashes[0].block_index = u32::MAX;
        assert_eq!(archive.lookup(&name)?.size, 999);
        archive.hashes[3].locale_platform = 0x0407;
        assert!(archive.read_file(&name, 10).is_err());
        Ok(())
    }

    #[test]
    fn rejects_paths_unsupported_flags_and_oversized_members() -> Result<()> {
        let mut archive = Archive::from_bytes(fixture("text", b"hello", 5, EXISTS))?;
        for name in [
            "../text", "/text", "C:\\text", "text//x", "text/./x", "te\0xt", "téxt",
        ] {
            assert!(archive.read_file(name, 10).is_err());
        }
        for flags in [
            EXISTS | 0x0100_0000,
            EXISTS | 0x0400_0000,
            EXISTS | COMPRESS | IMPLODE,
            EXISTS | FIX_KEY,
        ] {
            let mut archive = Archive::from_bytes(fixture("text", b"hello", 5, flags))?;
            assert!(archive.read_file("text", 10).is_err());
        }
        let mut archive = Archive::from_bytes(fixture("text", b"hello", u32::MAX, EXISTS))?;
        assert!(archive.read_file("text", usize::MAX).is_err());
        Ok(())
    }

    #[test]
    fn decompresses_encrypted_fixed_key_sectors() -> Result<()> {
        let name = "test/run.txt";
        let mut compressed = vec![0x08];
        compressed.extend(compressed_run(b'X', 63));
        let mut payload = [
            8_u32.to_le_bytes(),
            (8 + compressed.len() as u32).to_le_bytes(),
        ]
        .concat();
        let key = hash_name("run.txt", 3).wrapping_add(32) ^ 64;
        encrypt(&mut payload, key.wrapping_sub(1));
        encrypt(&mut compressed, key);
        payload.extend(compressed);
        let mut archive = Archive::from_bytes(fixture(
            name,
            &payload,
            64,
            EXISTS | COMPRESS | ENCRYPTED | FIX_KEY,
        ))?;
        assert_eq!(archive.read_file(name, 64)?, vec![b'X'; 64]);

        // Mix a compressed sector and an uncompressed tail, with a distinct
        // encryption key for each sector and a fixed key that includes size.
        let mut first = vec![0x08];
        first.extend(compressed_run(b'X', 511));
        let end_first = 12 + first.len() as u32;
        let mut payload: Vec<_> = [12, end_first, end_first + 5]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect();
        let key = hash_name("run.txt", 3).wrapping_add(32) ^ 517;
        encrypt(&mut payload, key.wrapping_sub(1));
        encrypt(&mut first, key);
        let mut tail = *b"hello";
        encrypt(&mut tail, key.wrapping_add(1));
        payload.extend(first);
        payload.extend(tail);
        let mut archive = Archive::from_bytes(fixture(
            name,
            &payload,
            517,
            EXISTS | COMPRESS | ENCRYPTED | FIX_KEY,
        ))?;
        let mut expected = vec![b'X'; 512];
        expected.extend_from_slice(b"hello");
        assert_eq!(archive.read_file(name, 517)?, expected);
        Ok(())
    }

    #[test]
    fn reads_huffman_adpcm_sectors_with_raw_tail() -> Result<()> {
        // StormLib-compressed original silence, 512 bytes per audio sector.
        // The second sector is stored raw, as legal MPQ short tails may be.
        let first = &[
            65, 4, 227, 230, 2, 232, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226,
            226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226,
            226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226,
            226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226,
            226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226,
            226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226,
            226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226,
            226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226,
            226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226,
            226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226,
            226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226,
            226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226,
            226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226,
            226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226,
            226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226,
            226, 226, 226, 226, 10,
        ];
        let end_first = 12 + first.len() as u32;
        let mut payload: Vec<_> = [12, end_first, end_first + 2]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect();
        payload.extend(first);
        payload.extend([0x12, 0x34]);
        let mut archive =
            Archive::from_bytes(fixture("sound.wav", &payload, 514, EXISTS | COMPRESS))?;
        let mut expected = vec![0; 512];
        expected.extend([0x12, 0x34]);
        assert_eq!(archive.read_file("sound.wav", 514)?, expected);
        // Corrupt the Huffman distribution without changing archive offsets.
        payload[13] = 255;
        let mut bad = Archive::from_bytes(fixture("sound.wav", &payload, 514, EXISTS | COMPRESS))?;
        assert!(bad.read_file("sound.wav", 514).is_err());
        let first = &[
            129, 7, 121, 231, 156, 115, 223, 247, 125, 223, 247, 125, 223, 247, 125, 223, 247, 125,
            223, 247, 125, 223, 247, 125, 223, 247, 125, 223, 247, 125, 223, 247, 125, 223, 247,
            125, 223, 247, 125, 223, 247, 125, 223, 247, 125, 223, 247, 125, 223, 247, 125, 223,
            247, 125, 223, 247, 125, 223, 247, 125, 223, 247, 125, 223, 247, 125, 223, 247, 125,
            223, 247, 125, 223, 247, 125, 223, 247, 125, 223, 247, 125, 223, 247, 125, 223, 247,
            125, 223, 247, 125, 223, 247, 125, 223, 247, 125, 223, 247, 125, 223, 247, 125, 223,
            247, 125, 223, 247, 125, 223, 247, 125, 223, 247, 125, 223, 247, 125, 223, 247, 125,
            223, 247, 125, 223, 247, 125, 223, 247, 125, 223, 247, 125, 223, 247, 125, 223, 247,
            125, 223, 247, 125, 223, 247, 125, 223, 247, 125, 223, 247, 125, 223, 247, 125, 223,
            247, 125, 223, 247, 125, 223, 247, 125, 223, 247, 125, 223, 247, 125, 223, 247, 125,
            223, 247, 125, 223, 247, 125, 223, 247, 125, 223, 247, 125, 223, 247, 125, 223, 247,
            125, 223, 247, 125, 223, 247, 125, 223, 151, 10,
        ];
        let end_first = 12 + first.len() as u32;
        let mut payload: Vec<_> = [12, end_first, end_first + 2]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect();
        payload.extend(first);
        payload.extend([0x12, 0x34]);
        let mut archive =
            Archive::from_bytes(fixture("sound.wav", &payload, 514, EXISTS | COMPRESS))?;
        let mut expected = vec![0; 512];
        expected.extend([0x12, 0x34]);
        assert_eq!(archive.read_file("sound.wav", 514)?, expected);
        // Corrupt the Huffman distribution without changing archive offsets.
        payload[13] = 255;
        let mut bad = Archive::from_bytes(fixture("sound.wav", &payload, 514, EXISTS | COMPRESS))?;
        assert!(bad.read_file("sound.wav", 514).is_err());
        Ok(())
    }

    #[test]
    fn raw_sectors_and_encryption_preserve_short_tail() -> Result<()> {
        let name = "plain";
        let expected: Vec<_> = (0..1027).map(|index| index as u8).collect();
        let mut payload = expected.clone();
        for (index, sector) in payload.chunks_mut(512).enumerate() {
            encrypt(sector, hash_name(name, 3).wrapping_add(index as u32));
        }
        let mut archive = Archive::from_bytes(fixture(
            name,
            &payload,
            expected.len() as u32,
            EXISTS | ENCRYPTED,
        ))?;
        assert_eq!(archive.read_file(name, 2000)?, expected);
        // A compressed member can store an incompressible sector verbatim.
        let mut payload = [8_u32.to_le_bytes(), 13_u32.to_le_bytes()].concat();
        payload.extend_from_slice(b"hello");
        let mut archive = Archive::from_bytes(fixture(name, &payload, 5, EXISTS | COMPRESS))?;
        assert_eq!(archive.read_file(name, 5)?, b"hello");
        Ok(())
    }

    #[test]
    fn rejects_invalid_sector_ranges_and_methods() -> Result<()> {
        for offsets in [[0, 12], [8, 7], [8, 13], [9, 12], [8, 8]] {
            let mut payload: Vec<_> = offsets.into_iter().flat_map(u32::to_le_bytes).collect();
            payload.extend_from_slice(&[0x08, 0, 4, 0]);
            let mut archive =
                Archive::from_bytes(fixture("text", &payload, 64, EXISTS | COMPRESS))?;
            assert!(archive.read_file("text", 64).is_err());
        }
        for mask in [0, 1, 2, 0x0a, 0x10, 0x40] {
            let mut payload = [8_u32.to_le_bytes(), 12_u32.to_le_bytes()].concat();
            payload.extend_from_slice(&[mask, 0, 4, 0]);
            let mut archive =
                Archive::from_bytes(fixture("text", &payload, 64, EXISTS | COMPRESS))?;
            assert!(archive.read_file("text", 64).is_err());
        }
        Ok(())
    }

    #[test]
    fn malformed_pkware_terminates_without_panics_or_excess_output() -> Result<()> {
        let valid = compressed_run(b'X', 63);
        assert_eq!(explode(&valid, 64)?, vec![b'X'; 64]);
        for len in 0..valid.len() {
            assert!(explode(&valid[..len], 64).is_err());
        }
        assert!(explode(&valid, 63).is_err());
        assert!(explode(&valid, 65).is_err());
        // Remove the initial literal, leaving a distance-1 reference before
        // any output exists. The stream is structurally coded but invalid.
        let remaining_bits: Vec<_> = valid[2..]
            .iter()
            .flat_map(|byte| (0..8).map(move |bit| (byte >> bit) & 1))
            .skip(9)
            .collect();
        let mut bad_reference = vec![0, 4];
        bad_reference.extend(remaining_bits.chunks(8).map(|chunk| {
            chunk
                .iter()
                .enumerate()
                .fold(0, |value, (index, bit)| value | (bit << index))
        }));
        assert!(
            explode(&bad_reference, 64)
                .unwrap_err()
                .to_string()
                .contains("back reference")
        );
        for header in [[1, 4], [0, 0], [0, 3], [0, 7], [0, 255]] {
            let mut bytes = valid.clone();
            bytes[..2].copy_from_slice(&header);
            assert!(explode(&bytes, 64).is_err());
        }
        // Mutations include invalid back references, absent terminators, and
        // partial symbols. Every input must either decode exactly or error.
        for index in 2..valid.len() {
            for value in 0..=255 {
                let mut bytes = valid.clone();
                bytes[index] = value;
                if let Ok(output) = explode(&bytes, 64) {
                    assert_eq!(output.len(), 64);
                }
            }
        }
        Ok(())
    }
}
