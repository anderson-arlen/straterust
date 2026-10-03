//! Native terrain and geometry queries, independent of source-game formats.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

use crate::sim::{Map, Position};

pub const MAX_TERRAIN_DIMENSION: u32 = 1024;
pub const TERRAIN_HEADER_BYTES: usize = 20;
pub const MAX_TERRAIN_BYTES: usize =
    TERRAIN_HEADER_BYTES + MAX_TERRAIN_DIMENSION as usize * MAX_TERRAIN_DIMENSION as usize;
pub const WALKABLE: u8 = 1;
pub const BUILDABLE: u8 = 2;
pub const HEIGHT_SHIFT: u8 = 2;
pub const HEIGHT_MASK: u8 = 3 << HEIGHT_SHIFT;
pub const BLOCKS_SIGHT: u8 = 16;
pub const RAMP: u8 = 32;
const KNOWN_FLAGS: u8 = WALKABLE | BUILDABLE | HEIGHT_MASK | BLOCKS_SIGHT | RAMP;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Footprint {
    /// Width and height in world coordinates, independent of image dimensions.
    pub width: u16,
    pub height: u16,
}

impl Default for Footprint {
    fn default() -> Self {
        Self {
            width: 1,
            height: 1,
        }
    }
}

impl Footprint {
    /// Half-open rectangle centered at position, rounded toward its upper left.
    /// i64 arithmetic keeps queries safe even for extreme i32 input positions.
    pub fn bounds(self, position: Position) -> [i64; 4] {
        let left = i64::from(position.x) - i64::from(self.width / 2);
        let top = i64::from(position.y) - i64::from(self.height / 2);
        [
            left,
            top,
            left + i64::from(self.width),
            top + i64::from(self.height),
        ]
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum MovementClass {
    #[default]
    Ground,
    Air,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Terrain {
    pub cell_size: u32,
    pub columns: u32,
    pub rows: u32,
    /// Row-major native flags; elevation occupies bits 2..3 (levels 0..3).
    pub flags: Vec<u8>,
}

impl Terrain {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            (1..=32768).contains(&self.cell_size),
            "terrain cell_size must be 1..=32768"
        );
        ensure!(
            (1..=MAX_TERRAIN_DIMENSION).contains(&self.columns)
                && (1..=MAX_TERRAIN_DIMENSION).contains(&self.rows),
            "terrain grid dimensions must be 1..={MAX_TERRAIN_DIMENSION}"
        );
        ensure!(
            self.flags.len() == self.columns as usize * self.rows as usize,
            "terrain flag count does not match grid dimensions"
        );
        ensure!(
            self.flags.iter().all(|flags| flags & !KNOWN_FLAGS == 0),
            "terrain contains unsupported flag bits"
        );
        Ok(())
    }

    pub fn validate_coverage(&self, width: i32, height: i32) -> Result<()> {
        self.validate()?;
        ensure!(
            i64::from(self.columns) * i64::from(self.cell_size) == i64::from(width)
                && i64::from(self.rows) * i64::from(self.cell_size) == i64::from(height),
            "terrain grid must cover map dimensions exactly"
        );
        Ok(())
    }

    pub fn flags_at(&self, position: Position) -> Option<u8> {
        let x = u32::try_from(position.x)
            .ok()?
            .checked_div(self.cell_size)?;
        let y = u32::try_from(position.y)
            .ok()?
            .checked_div(self.cell_size)?;
        if x >= self.columns || y >= self.rows {
            return None;
        }
        let index = usize::try_from(u64::from(y) * u64::from(self.columns) + u64::from(x)).ok()?;
        self.flags.get(index).copied()
    }
}

/// SRTM v1: magic, LE u32 version/cell_size/columns/rows, then exact cell flags.
pub fn encode_terrain(terrain: &Terrain) -> Result<Vec<u8>> {
    terrain.validate()?;
    let mut bytes = Vec::with_capacity(TERRAIN_HEADER_BYTES + terrain.flags.len());
    bytes.extend_from_slice(b"SRTM");
    for word in [1, terrain.cell_size, terrain.columns, terrain.rows] {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    bytes.extend_from_slice(&terrain.flags);
    Ok(bytes)
}

pub fn decode_terrain(bytes: &[u8]) -> Result<Terrain> {
    ensure!(bytes.len() >= TERRAIN_HEADER_BYTES, "truncated SRTM header");
    ensure!(
        bytes.len() <= MAX_TERRAIN_BYTES,
        "SRTM exceeds the terrain byte limit"
    );
    ensure!(&bytes[..4] == b"SRTM", "invalid SRTM magic");
    let word = |offset| u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
    ensure!(word(4) == 1, "unsupported SRTM version");
    let terrain = Terrain {
        cell_size: word(8),
        columns: word(12),
        rows: word(16),
        flags: bytes[TERRAIN_HEADER_BYTES..].to_vec(),
    };
    terrain.validate()?;
    Ok(terrain)
}

impl Map {
    pub fn contains_footprint(&self, position: Position, footprint: Footprint) -> bool {
        let [left, top, right, bottom] = footprint.bounds(position);
        footprint.width != 0
            && footprint.height != 0
            && left >= 0
            && top >= 0
            && right <= i64::from(self.width)
            && bottom <= i64::from(self.height)
    }

    /// Static terrain only. World::can_place also checks entity occupancy.
    pub fn can_move(&self, position: Position, footprint: Footprint, class: MovementClass) -> bool {
        if class == MovementClass::Air {
            self.contains_footprint(position, footprint)
        } else {
            self.footprint_has_flag(position, footprint, WALKABLE)
        }
    }

    pub fn can_build(&self, position: Position, footprint: Footprint) -> bool {
        self.footprint_has_flag(position, footprint, BUILDABLE)
    }

    pub fn height_at(&self, position: Position) -> Option<u8> {
        self.cell_flags(position)
            .map(|flags| (flags & HEIGHT_MASK) >> HEIGHT_SHIFT)
    }

    /// Outside the map blocks sight; actual visibility propagation comes later.
    pub fn blocks_sight(&self, position: Position) -> bool {
        self.cell_flags(position)
            .is_none_or(|flags| flags & BLOCKS_SIGHT != 0)
    }

    pub fn is_ramp(&self, position: Position) -> bool {
        self.cell_flags(position)
            .is_some_and(|flags| flags & RAMP != 0)
    }

    fn cell_flags(&self, position: Position) -> Option<u8> {
        if !self.contains(position) {
            return None;
        }
        self.terrain
            .as_ref()
            .map_or(Some(WALKABLE | BUILDABLE), |terrain| {
                terrain.flags_at(position)
            })
    }

    fn footprint_has_flag(&self, position: Position, footprint: Footprint, flag: u8) -> bool {
        if !self.contains_footprint(position, footprint) {
            return false;
        }
        let Some(terrain) = &self.terrain else {
            return true;
        };
        if terrain.cell_size == 0
            || terrain.columns > MAX_TERRAIN_DIMENSION
            || terrain.rows > MAX_TERRAIN_DIMENSION
        {
            return false;
        }
        let [left, top, right, bottom] = footprint.bounds(position);
        let size = i64::from(terrain.cell_size);
        let (start_x, end_x) = (left / size, (right - 1) / size);
        let (start_y, end_y) = (top / size, (bottom - 1) / size);
        if end_x >= i64::from(terrain.columns) || end_y >= i64::from(terrain.rows) {
            return false;
        }
        for y in start_y..=end_y {
            for x in start_x..=end_x {
                let index = y as usize * terrain.columns as usize + x as usize;
                if terrain
                    .flags
                    .get(index)
                    .is_none_or(|flags| flags & flag == 0)
                {
                    return false;
                }
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map() -> Map {
        let mut flags = vec![WALKABLE | BUILDABLE; 12];
        flags[1] = WALKABLE | (1 << HEIGHT_SHIFT) | BLOCKS_SIGHT;
        flags[5] = BUILDABLE | (2 << HEIGHT_SHIFT) | RAMP;
        flags[11] = 0;
        Map {
            initial_explored: Default::default(),
            creation: Default::default(),
            ai: Vec::new(),
            mission: None,
            fog_of_war: false,
            id: "synthetic-terrain".into(),
            width: 32,
            height: 24,
            players: 1,
            spawns: vec![],
            start_locations: vec![],
            resources: vec![],
            terrain: Some(Terrain {
                cell_size: 8,
                columns: 4,
                rows: 3,
                flags,
            }),
        }
    }

    #[test]
    fn native_terrain_round_trip_rejects_truncation_dimensions_and_unknown_flags() {
        let terrain = map().terrain.unwrap();
        let bytes = encode_terrain(&terrain).unwrap();
        assert_eq!(decode_terrain(&bytes).unwrap(), terrain);
        for length in 0..bytes.len() {
            assert!(decode_terrain(&bytes[..length]).is_err());
        }
        let mut extra = bytes.clone();
        extra.push(0);
        assert!(decode_terrain(&extra).is_err());
        for (offset, value) in [
            (0, 0),
            (4, 2),
            (8, 0),
            (8, u32::MAX),
            (12, 0),
            (12, 1025),
            (16, u32::MAX),
        ] {
            let mut invalid = bytes.clone();
            invalid[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
            assert!(decode_terrain(&invalid).is_err(), "field {offset}={value}");
        }
        let mut invalid = bytes;
        invalid[TERRAIN_HEADER_BYTES] = 64;
        assert!(decode_terrain(&invalid).is_err());
        assert!(terrain.validate_coverage(32, 24).is_ok());
        assert!(terrain.validate_coverage(31, 24).is_err());
        assert!(terrain.validate_coverage(32, 25).is_err());
    }

    #[test]
    fn terrain_queries_respect_cells_movement_classes_and_footprint_edges() {
        let map = map();
        let point = Footprint::default();
        assert!(map.can_move(Position { x: 0, y: 0 }, point, MovementClass::Ground));
        assert!(!map.can_move(Position { x: 8, y: 8 }, point, MovementClass::Ground));
        assert!(map.can_move(Position { x: 8, y: 8 }, point, MovementClass::Air));
        assert!(map.can_build(Position { x: 8, y: 8 }, point));
        assert!(!map.can_build(Position { x: 8, y: 0 }, point));
        let wide = Footprint {
            width: 2,
            height: 2,
        };
        assert!(map.can_build(Position { x: 7, y: 1 }, wide));
        assert!(!map.can_build(Position { x: 8, y: 1 }, wide));
        assert!(!map.can_move(Position { x: 8, y: 8 }, wide, MovementClass::Ground));
        for position in [
            Position { x: -1, y: 0 },
            Position { x: 32, y: 0 },
            Position { x: 0, y: 24 },
            Position {
                x: i32::MIN,
                y: i32::MAX,
            },
        ] {
            assert!(!map.can_move(position, point, MovementClass::Air));
            assert!(!map.can_build(position, point));
            assert_eq!(map.height_at(position), None);
            assert!(map.blocks_sight(position));
        }
        assert!(!map.contains_footprint(Position { x: 0, y: 1 }, wide));
        assert!(map.contains_footprint(Position { x: 31, y: 23 }, wide));
        assert!(!map.contains_footprint(
            Position { x: 31, y: 23 },
            Footprint {
                width: 4,
                height: 4
            }
        ));
        assert!(!map.contains_footprint(
            Position { x: 1, y: 1 },
            Footprint {
                width: 0,
                height: 1
            }
        ));
        assert_eq!(map.height_at(Position { x: 0, y: 0 }), Some(0));
        assert_eq!(map.height_at(Position { x: 8, y: 0 }), Some(1));
        assert_eq!(map.height_at(Position { x: 8, y: 8 }), Some(2));
        assert!(map.blocks_sight(Position { x: 8, y: 0 }));
        assert!(!map.blocks_sight(Position { x: 8, y: 8 }));
        assert!(map.is_ramp(Position { x: 8, y: 8 }));
        assert!(!map.is_ramp(Position { x: -1, y: 8 }));
    }

    #[test]
    fn original_maps_without_terrain_are_flat_open_ground() {
        let mut map = map();
        map.terrain = None;
        let position = Position { x: 8, y: 8 };
        assert!(map.can_move(position, Footprint::default(), MovementClass::Ground));
        assert!(map.can_build(position, Footprint::default()));
        assert_eq!(map.height_at(position), Some(0));
        assert!(!map.blocks_sight(position));
        assert!(!map.is_ramp(position));
    }
}
