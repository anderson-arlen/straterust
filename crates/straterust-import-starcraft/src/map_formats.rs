//! The original Badlands CHK subset used by the explicit terrain-only preview.
//!
//! Layout references: <https://docs.scmjs.dev/chk/> (the editor author's format
//! investigation), especially UNIT/MTXM, and
//! <https://wiki.staredit.net/wiki/Terrain_Format> (CV5/VF4).
//! Duplicate sections, protected maps, expansion revisions and other tilesets
//! are rejected. Unconverted scenario features are reported, never executed.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, ensure};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct SectionInfo {
    pub name: String,
    pub bytes: usize,
}

#[derive(Debug, Serialize)]
pub struct PlacedUnit {
    pub serial: u32,
    /// Original pixel coordinates, measured from the top-left map corner.
    pub x: u16,
    pub y: u16,
    pub unit_type: u16,
    /// Original zero-based player slot. Neutral resources normally use slot 11.
    pub owner: u8,
    /// None means the record does not set an amount; no game default is guessed.
    pub resource_amount: Option<u32>,
}

#[derive(Debug)]
pub struct ParsedMap {
    /// Dimensions in 32-pixel source tiles.
    pub width: u16,
    pub height: u16,
    pub tiles: Vec<u16>,
    pub owners: [u8; 12],
    pub races: [u8; 12],
    pub units: Vec<PlacedUnit>,
    pub sections: Vec<SectionInfo>,
    pub unsupported: Vec<String>,
}

#[derive(Debug)]
pub struct DecodedTerrain {
    /// Row-major VX4 indices, one per 32-pixel tile.
    pub megatile_indices: Vec<u16>,
    /// Row-major 8-pixel cells: walkable 1, buildable 2, height in bits 2..3,
    /// blocks-sight 16 and ramp 32. Occupancy is not included.
    pub flags: Vec<u8>,
}

pub fn parse_chk(data: &[u8]) -> Result<ParsedMap> {
    ensure!(data.len() <= 8 * 1024 * 1024, "CHK exceeds the 8 MiB limit");
    let mut sections = Vec::new();
    let mut contents = BTreeMap::new();
    let mut offset = 0;
    while offset < data.len() {
        ensure!(sections.len() < 256, "CHK exceeds 256 sections");
        let header = bytes(data, offset, 8).context("truncated CHK section header")?;
        let name: [u8; 4] = header[..4].try_into()?;
        ensure!(
            name.iter().all(|byte| (32..=126).contains(byte)),
            "non-ASCII CHK section name"
        );
        let label = String::from_utf8(name.to_vec())?;
        let length = usize::try_from(u32_at(header, 4))?;
        let body = bytes(data, offset + 8, length)
            .with_context(|| format!("truncated CHK section {label:?}"))?;
        ensure!(
            contents.insert(name, body).is_none(),
            "duplicate CHK section {label:?}; protected/overlaid sections are unsupported"
        );
        sections.push(SectionInfo {
            name: label,
            bytes: length,
        });
        offset += 8 + length;
    }
    let required = |name: &[u8; 4], length: usize| -> Result<&[u8]> {
        let label = String::from_utf8_lossy(name);
        let body = contents
            .get(name)
            .with_context(|| format!("missing CHK {label:?} section"))?;
        ensure!(
            body.len() == length,
            "CHK {label:?} must contain {length} bytes"
        );
        Ok(body)
    };
    ensure!(
        u16_at(required(b"VER ", 2)?, 0) == 59,
        "only original StarCraft CHK revision 59 is supported"
    );
    ensure!(
        matches!(u16_at(required(b"ERA ", 2)?, 0), 0 | 2),
        "only Badlands and Installation tilesets are supported"
    );
    let dimensions = required(b"DIM ", 4)?;
    let width = u16_at(dimensions, 0);
    let height = u16_at(dimensions, 2);
    ensure!(
        (1..=256).contains(&width) && (1..=256).contains(&height),
        "CHK dimensions must be between 1 and 256 tiles"
    );
    let tile_count = usize::from(width) * usize::from(height);
    let tiles = required(b"MTXM", tile_count * 2)?
        .as_chunks::<2>()
        .0
        .iter()
        .map(|tile| u16_at(tile, 0))
        .collect();
    let owners: [u8; 12] = required(b"OWNR", 12)?.try_into()?;
    let races: [u8; 12] = required(b"SIDE", 12)?.try_into()?;
    ensure!(
        owners.iter().all(|owner| *owner <= 8),
        "unsupported player controller in OWNR"
    );
    ensure!(races.iter().all(|race| *race <= 7), "invalid race in SIDE");
    let unit_data = contents.get(b"UNIT").context("missing CHK UNIT section")?;
    ensure!(
        unit_data.len().is_multiple_of(36) && unit_data.len() / 36 <= 4096,
        "CHK UNIT must contain at most 4096 complete 36-byte records"
    );
    let mut units = Vec::with_capacity(unit_data.len() / 36);
    let mut serials = BTreeSet::new();
    let mut unsupported = Vec::new();
    let mut property_overrides = 0;
    let mut relationships = 0;
    for (index, record) in unit_data.as_chunks::<36>().0.iter().enumerate() {
        let serial = u32_at(record, 0);
        ensure!(
            serials.insert(serial),
            "CHK UNIT {index} repeats serial number {serial}"
        );
        let x = u16_at(record, 4);
        let y = u16_at(record, 6);
        let unit_type = u16_at(record, 8);
        let owner = record[16];
        ensure!(
            x < width * 32 && y < height * 32,
            "CHK UNIT {index} is outside the map"
        );
        ensure!(
            unit_type < 228,
            "CHK UNIT {index} has unsupported unit type {unit_type}"
        );
        ensure!(
            owner < 12,
            "CHK UNIT {index} has invalid player slot {owner}"
        );
        let states_set = u16_at(record, 12);
        let fields_set = u16_at(record, 14);
        for (flag, value) in [(2, record[17]), (4, record[18]), (8, record[19])] {
            ensure!(
                fields_set & flag == 0 || value <= 100,
                "CHK UNIT {index} has percentage above 100"
            );
        }
        if (fields_set & 2 != 0 && record[17] != 100)
            || (fields_set & 4 != 0 && record[18] != 100)
            || fields_set & 8 != 0
            || (fields_set & 0x20 != 0 && u16_at(record, 24) != 0)
            || states_set & u16_at(record, 26) != 0
            || fields_set & !0x7f != 0
            || states_set & !0x1f != 0
        {
            property_overrides += 1;
        }
        if u16_at(record, 10) != 0 || u32_at(record, 32) != 0 {
            relationships += 1;
        }
        units.push(PlacedUnit {
            serial,
            x,
            y,
            unit_type,
            owner,
            resource_amount: (fields_set & 0x10 != 0).then(|| u32_at(record, 20)),
        });
    }
    if property_overrides != 0 {
        unsupported.push(format!("UNIT: {property_overrides} records set health/shield/energy/hangar/state properties not imported"));
    }
    if relationships != 0 {
        unsupported.push(format!(
            "UNIT: {relationships} building/add-on/nydus relationships are not imported"
        ));
    }
    for section in &sections {
        let name: &[u8; 4] = section.name.as_bytes().try_into()?;
        let body = contents[name];
        match name {
            b"VER " | b"ERA " | b"DIM " | b"MTXM" | b"OWNR" | b"SIDE" | b"UNIT" => {}
            b"TRIG" | b"MBRF" => {
                ensure!(
                    body.len().is_multiple_of(2400),
                    "CHK {} has a partial trigger record",
                    section.name
                );
                if !body.is_empty() {
                    let actions: BTreeSet<u8> = body
                        .as_chunks::<2400>()
                        .0
                        .iter()
                        .flat_map(|trigger| trigger[320..2368].as_chunks::<32>().0.iter())
                        .map(|action| action[26])
                        .filter(|action| *action != 0)
                        .collect();
                    unsupported.push(format!(
                        "{}: {} trigger/briefing records are not executed (action IDs {actions:?})",
                        section.name,
                        body.len() / 2400
                    ));
                }
            }
            b"THG2" | b"DD2 " => {
                let record_size = if name == b"THG2" { 10 } else { 8 };
                ensure!(
                    body.len().is_multiple_of(record_size),
                    "CHK {} has a partial decoration record",
                    section.name
                );
                if !body.is_empty() {
                    unsupported.push(format!("{}: {} sprite/doodad records are not imported; flattened MTXM terrain is retained", section.name, body.len() / record_size));
                }
            }
            b"UNIS" | b"UPGS" | b"TECS" => {
                let (length, defaults) = match name {
                    b"UNIS" => (4048, 228),
                    b"UPGS" => (598, 46),
                    _ => (216, 24),
                };
                ensure!(
                    body.len() == length,
                    "CHK {} has an invalid settings-table size",
                    section.name
                );
                ensure!(
                    body[..defaults].iter().all(|flag| *flag <= 1),
                    "CHK {} has an invalid defaults flag",
                    section.name
                );
                let overrides = body[..defaults].iter().filter(|flag| **flag == 0).count();
                if overrides != 0 {
                    unsupported.push(format!("{}: {overrides} custom unit/upgrade/technology definitions are not imported", section.name));
                }
            }
            // Editor-only or descriptive sections have no effect on this preview.
            // The complete source-section inventory still records their presence.
            b"IVER" | b"VCOD" | b"IOWN" | b"ISOM" | b"TILE" | b"STR " | b"SPRP" | b"WAV "
            | b"UPUS" => {}
            b"PUNI" | b"UPGR" | b"PTEC" | b"FORC" | b"MASK" | b"MRGN" | b"UPRP" => {
                let feature = match name {
                    b"PUNI" => "player unit availability",
                    b"UPGR" => "player upgrade levels and limits",
                    b"PTEC" => "player technology availability and research",
                    b"FORC" => "forces and alliance settings",
                    b"MASK" => "initial fog of war",
                    b"MRGN" => "scenario locations",
                    _ => "trigger-created unit properties",
                };
                unsupported.push(format!("{}: {feature} not imported", section.name));
            }
            _ => unsupported.push(format!(
                "{}: unrecognized section, not imported ({} bytes)",
                section.name,
                body.len()
            )),
        }
    }
    Ok(ParsedMap {
        width,
        height,
        tiles,
        owners,
        races,
        units,
        sections,
        unsupported,
    })
}

pub fn decode_terrain(map: &ParsedMap, cv5: &[u8], vf4: &[u8]) -> Result<DecodedTerrain> {
    ensure!(
        !cv5.is_empty() && cv5.len().is_multiple_of(52) && cv5.len() / 52 <= 2048,
        "classic CV5 must contain 1..=2048 complete 52-byte records"
    );
    ensure!(
        !vf4.is_empty() && vf4.len().is_multiple_of(32) && vf4.len() / 32 <= 65536,
        "VF4 must contain 1..=65536 complete 32-byte records"
    );
    ensure!(
        (1..=256).contains(&map.width)
            && (1..=256).contains(&map.height)
            && map.tiles.len() == usize::from(map.width) * usize::from(map.height),
        "invalid parsed map dimensions or tile count"
    );
    let columns = usize::from(map.width) * 4;
    let mut flags = vec![0; map.tiles.len() * 16];
    let mut megatile_indices = Vec::with_capacity(map.tiles.len());
    for (position, tile) in map.tiles.iter().copied().enumerate() {
        ensure!(
            tile & 0x8000 == 0,
            "MTXM tile {position} uses an unsupported high tile bit"
        );
        let group = usize::from(tile >> 4);
        let variation = usize::from(tile & 15);
        let cv5_record = bytes(cv5, group * 52, 52).with_context(|| {
            format!("MTXM tile {position} references missing CV5 group {group}")
        })?;
        let megatile = u16_at(cv5_record, 20 + variation * 2);
        megatile_indices.push(megatile);
        let minitiles = bytes(vf4, usize::from(megatile) * 32, 32).with_context(|| {
            format!("CV5 group {group} references missing VF4 megatile {megatile}")
        })?;
        let tile_buildable = u16_at(cv5_record, 2) & 0x80 == 0;
        let tile_x = position % usize::from(map.width);
        let tile_y = position / usize::from(map.width);
        for (mini, encoded) in minitiles.as_chunks::<2>().0.iter().enumerate() {
            let source = u16_at(encoded, 0);
            ensure!(
                source & !0x1f == 0,
                "VF4 megatile {megatile} has unsupported minitile flags {source:#x}"
            );
            ensure!(
                source & 6 != 6,
                "VF4 megatile {megatile} has conflicting elevation flags"
            );
            let walkable = source & 1 != 0;
            let height = if source & 4 != 0 {
                2
            } else if source & 2 != 0 {
                1
            } else {
                0
            };
            let native = u8::from(walkable)
                | (u8::from(tile_buildable && walkable) << 1)
                | (height << 2)
                | (u8::from(source & 8 != 0) << 4)
                | (u8::from(source & 16 != 0) << 5);
            let x = tile_x * 4 + mini % 4;
            let y = tile_y * 4 + mini / 4;
            flags[y * columns + x] = native;
        }
    }
    Ok(DecodedTerrain {
        megatile_indices,
        flags,
    })
}

fn bytes(data: &[u8], offset: usize, length: usize) -> Result<&[u8]> {
    let end = offset
        .checked_add(length)
        .context("map byte range overflow")?;
    data.get(offset..end).with_context(|| {
        format!(
            "map byte range {offset}..{end} exceeds {} bytes",
            data.len()
        )
    })
}

// Callers validate fixed-width records before using these field readers.
fn u16_at(data: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([data[offset], data[offset + 1]])
}

fn u32_at(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn section(chk: &mut Vec<u8>, name: &[u8; 4], body: &[u8]) {
        chk.extend_from_slice(name);
        chk.extend_from_slice(&(body.len() as u32).to_le_bytes());
        chk.extend_from_slice(body);
    }

    fn fixture() -> Vec<u8> {
        let mut chk = Vec::new();
        section(&mut chk, b"VER ", &59_u16.to_le_bytes());
        section(&mut chk, b"ERA ", &[0, 0]);
        section(&mut chk, b"DIM ", &[2, 0, 2, 0]);
        section(&mut chk, b"OWNR", &[6, 6, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        section(&mut chk, b"SIDE", &[5, 5, 2, 1, 0, 2, 1, 0, 7, 7, 7, 4]);
        section(&mut chk, b"MTXM", &[0, 0, 17, 0, 17, 0, 0, 0]);
        let mut units = vec![0; 72];
        units[0] = 1;
        units[4] = 16;
        units[6] = 48;
        units[8] = 214;
        units[36] = 2;
        units[40] = 48;
        units[42] = 16;
        units[44] = 176;
        units[50] = 16;
        units[52] = 11;
        units[56..60].copy_from_slice(&1500_u32.to_le_bytes());
        section(&mut chk, b"UNIT", &units);
        chk
    }

    #[test]
    fn original_chk_keeps_coordinates_slots_tiles_and_resource_validity() {
        let map = parse_chk(&fixture()).unwrap();
        assert_eq!((map.width, map.height), (2, 2));
        assert_eq!(map.tiles, [0, 17, 17, 0]);
        assert_eq!(&map.owners[..2], &[6, 6]);
        assert_eq!(&map.races[..2], &[5, 5]);
        assert_eq!(map.units.len(), 2);
        assert_eq!(
            (map.units[0].x, map.units[0].y, map.units[0].owner),
            (16, 48, 0)
        );
        assert_eq!(map.units[0].unit_type, 214);
        assert_eq!(map.units[0].resource_amount, None);
        assert_eq!(map.units[1].resource_amount, Some(1500));
        assert_eq!(map.units[1].owner, 11);
        assert!(map.unsupported.is_empty());
    }

    #[test]
    fn reports_triggers_decorations_custom_definitions_and_unknown_sections() {
        let mut chk = fixture();
        let mut trigger = vec![0; 2400];
        trigger[320 + 26] = 26;
        section(&mut chk, b"TRIG", &trigger);
        section(&mut chk, b"THG2", &[0; 10]);
        let mut settings = vec![0; 4048];
        settings[..228].fill(1);
        settings[0] = 0;
        section(&mut chk, b"UNIS", &settings);
        section(&mut chk, b"TEST", &[1, 2, 3]);
        let map = parse_chk(&chk).unwrap();
        assert_eq!(map.unsupported.len(), 4);
        assert!(map.unsupported[0].contains("action IDs {26}"));
        assert!(map.unsupported[1].contains("1 sprite/doodad"));
        assert!(map.unsupported[2].contains("1 custom"));
        assert!(map.unsupported[3].contains("unrecognized"));
        assert_eq!(map.sections.last().unwrap().bytes, 3);
    }

    #[test]
    fn rejects_truncation_duplicate_sections_invalid_extent_and_oversized_chunks() {
        let original = fixture();
        for end in 0..original.len() {
            assert!(parse_chk(&original[..end]).is_err(), "truncation at {end}");
        }
        let mut duplicate = original.clone();
        section(&mut duplicate, b"DIM ", &[2, 0, 2, 0]);
        assert!(parse_chk(&duplicate).is_err());
        let mut oversized = original.clone();
        oversized[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(parse_chk(&oversized).is_err());
        let mut outside = original.clone();
        let unit = outside
            .windows(4)
            .position(|window| window == b"UNIT")
            .unwrap()
            + 8;
        outside[unit + 4] = 64;
        assert!(parse_chk(&outside).is_err());
        outside[unit + 4] = 16;
        outside[unit + 16] = 12;
        assert!(parse_chk(&outside).is_err());
    }

    #[test]
    fn resource_bytes_without_validity_bit_do_not_become_an_amount() {
        let mut chk = fixture();
        let unit = chk.windows(4).position(|window| window == b"UNIT").unwrap() + 8;
        chk[unit + 36 + 14] = 0;
        let map = parse_chk(&chk).unwrap();
        assert_eq!(map.units[1].resource_amount, None);
        chk[unit + 14] = 2;
        chk[unit + 17] = 50;
        assert!(parse_chk(&chk).unwrap().unsupported[0].contains("health/shield/energy"));
    }

    fn terrain_tables() -> (Vec<u8>, Vec<u8>) {
        let mut cv5 = vec![0; 104];
        cv5[52 + 2] = 0x80; // unbuildable second group
        cv5[52 + 20 + 2] = 1; // variation 1 references megatile 1
        let mut vf4 = vec![0; 64];
        for record in vf4.as_chunks_mut::<2>().0 {
            record[0] = 1;
        }
        vf4[0] = 0; // blocked
        vf4[2] = 3; // walkable middle elevation
        vf4[8] = 0x1d; // walkable high elevation, sight blocker and ramp
        (cv5, vf4)
    }

    #[test]
    fn terrain_expands_minitiles_row_major_with_elevation_building_and_sight_flags() {
        let map = parse_chk(&fixture()).unwrap();
        let (cv5, vf4) = terrain_tables();
        let decoded = decode_terrain(&map, &cv5, &vf4).unwrap();
        assert_eq!(decoded.megatile_indices, [0, 1, 1, 0]);
        assert_eq!(decoded.flags.len(), 64);
        assert_eq!(decoded.flags[0], 0);
        assert_eq!(decoded.flags[1], 1 | 2 | 4);
        assert_eq!(decoded.flags[8], 1 | 2 | 8 | 16 | 32);
        assert_eq!(decoded.flags[4], 1); // unbuildable right-hand tile
        assert_eq!(decoded.flags[32], 1); // unbuildable lower-left tile
        assert_eq!(decoded.flags[37], 1 | 2 | 4); // bottom-right tile's middle cell
    }

    #[test]
    fn terrain_rejects_incomplete_tables_missing_references_and_unknown_flags() {
        let mut map = parse_chk(&fixture()).unwrap();
        let (mut cv5, mut vf4) = terrain_tables();
        assert!(decode_terrain(&map, &cv5[..103], &vf4).is_err());
        assert!(decode_terrain(&map, &cv5, &vf4[..63]).is_err());
        cv5[20] = 2;
        assert!(decode_terrain(&map, &cv5, &vf4).is_err());
        cv5[20] = 0;
        vf4[0] = 0x20;
        assert!(decode_terrain(&map, &cv5, &vf4).is_err());
        vf4[0] = 6;
        assert!(decode_terrain(&map, &cv5, &vf4).is_err());
        vf4[0] = 1;
        map.tiles[0] = 32;
        assert!(decode_terrain(&map, &cv5, &vf4).is_err());
        map.tiles[0] = 0x8000;
        assert!(decode_terrain(&map, &cv5, &vf4).is_err());
    }
}
