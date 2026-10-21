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
mod embedded;
pub mod pe;

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
        self.read_block(block, name, max_bytes, None)
    }

    fn read_block(
        &mut self,
        block: Block,
        name: &str,
        max_bytes: usize,
        recovered_key: Option<u32>,
    ) -> Result<Vec<u8>> {
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
        if let Some(recovered) = recovered_key {
            key = recovered;
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
mod tests;
