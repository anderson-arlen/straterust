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

#[test]
fn discovers_encrypted_nested_map_archives_without_a_filename_list() {
    let nested = fixture("scenario", b"map", 3, EXISTS);
    let key = hash_name("map.scx", 3);
    let mut sectors = (8u32.to_le_bytes())
        .into_iter()
        .chain(((nested.len() + 8) as u32).to_le_bytes())
        .collect::<Vec<_>>();
    encrypt(&mut sectors, key.wrapping_sub(1));
    let mut encrypted = nested.clone();
    encrypt(&mut encrypted, key);
    sectors.extend(encrypted);
    let bytes = fixture(
        "unknown",
        &sectors,
        nested.len() as u32,
        EXISTS | IMPLODE | ENCRYPTED,
    );
    let mut archive = Archive::from_bytes(bytes).unwrap();
    assert_eq!(archive.embedded_archives(4096).unwrap(), vec![(0, nested)]);
    assert!(archive.embedded_archives(32).unwrap().is_empty());
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
    assert!(Archive::from_region(Cursor::new(valid.clone()), 0, valid.len() as u64 - 1).is_err());
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
        65, 4, 227, 230, 2, 232, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226,
        226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226,
        226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226,
        226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226,
        226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226,
        226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226,
        226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226,
        226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226,
        226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226,
        226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226,
        226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226,
        226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226,
        226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226,
        226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226, 226,
        226, 226, 226, 226, 226, 226, 226, 10,
    ];
    let end_first = 12 + first.len() as u32;
    let mut payload: Vec<_> = [12, end_first, end_first + 2]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
    payload.extend(first);
    payload.extend([0x12, 0x34]);
    let mut archive = Archive::from_bytes(fixture("sound.wav", &payload, 514, EXISTS | COMPRESS))?;
    let mut expected = vec![0; 512];
    expected.extend([0x12, 0x34]);
    assert_eq!(archive.read_file("sound.wav", 514)?, expected);
    // Corrupt the Huffman distribution without changing archive offsets.
    payload[13] = 255;
    let mut bad = Archive::from_bytes(fixture("sound.wav", &payload, 514, EXISTS | COMPRESS))?;
    assert!(bad.read_file("sound.wav", 514).is_err());
    let first = &[
        129, 7, 121, 231, 156, 115, 223, 247, 125, 223, 247, 125, 223, 247, 125, 223, 247, 125,
        223, 247, 125, 223, 247, 125, 223, 247, 125, 223, 247, 125, 223, 247, 125, 223, 247, 125,
        223, 247, 125, 223, 247, 125, 223, 247, 125, 223, 247, 125, 223, 247, 125, 223, 247, 125,
        223, 247, 125, 223, 247, 125, 223, 247, 125, 223, 247, 125, 223, 247, 125, 223, 247, 125,
        223, 247, 125, 223, 247, 125, 223, 247, 125, 223, 247, 125, 223, 247, 125, 223, 247, 125,
        223, 247, 125, 223, 247, 125, 223, 247, 125, 223, 247, 125, 223, 247, 125, 223, 247, 125,
        223, 247, 125, 223, 247, 125, 223, 247, 125, 223, 247, 125, 223, 247, 125, 223, 247, 125,
        223, 247, 125, 223, 247, 125, 223, 247, 125, 223, 247, 125, 223, 247, 125, 223, 247, 125,
        223, 247, 125, 223, 247, 125, 223, 247, 125, 223, 247, 125, 223, 247, 125, 223, 247, 125,
        223, 247, 125, 223, 247, 125, 223, 247, 125, 223, 247, 125, 223, 247, 125, 223, 247, 125,
        223, 247, 125, 223, 247, 125, 223, 247, 125, 223, 247, 125, 223, 247, 125, 223, 151, 10,
    ];
    let end_first = 12 + first.len() as u32;
    let mut payload: Vec<_> = [12, end_first, end_first + 2]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
    payload.extend(first);
    payload.extend([0x12, 0x34]);
    let mut archive = Archive::from_bytes(fixture("sound.wav", &payload, 514, EXISTS | COMPRESS))?;
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
        let mut archive = Archive::from_bytes(fixture("text", &payload, 64, EXISTS | COMPRESS))?;
        assert!(archive.read_file("text", 64).is_err());
    }
    for mask in [0, 1, 2, 0x0a, 0x10, 0x40] {
        let mut payload = [8_u32.to_le_bytes(), 12_u32.to_le_bytes()].concat();
        payload.extend_from_slice(&[mask, 0, 4, 0]);
        let mut archive = Archive::from_bytes(fixture("text", &payload, 64, EXISTS | COMPRESS))?;
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
