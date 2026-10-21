//! Native destructible rock tiles, using the same saved depletion as forests.
use straterust_engine::{
    assets::*,
    map::{Terrain, WALKABLE},
    sim::*,
};

pub(super) fn edges(era: usize) -> ResourceTerrainEdges {
    // Source megatile IDs differ by tileset; the fourteen corner shapes agree.
    let offset = [0, 5, 3, 5][era];
    let rows: [&[u32]; 15] = [
        &[150, 173],
        &[142, 167],
        &[164, 176],
        &[147, 171],
        &[149, 172],
        &[154, 175],
        &[151],
        &[144, 169],
        &[153, 174],
        &[143, 168],
        &[152],
        &[146, 170],
        &[148],
        &[145],
        &[165, 177, 178, 179],
    ];
    let masks = [8, 4, 12, 1, 9, 5, 13, 2, 10, 6, 14, 3, 11, 7, 15];
    let mut corners = std::collections::BTreeMap::new();
    let mut tiles = [None; 16];
    for (row, mask) in rows.into_iter().zip(masks) {
        tiles[mask as usize] = Some(row[0] - offset);
        for tile in row {
            corners.insert(tile - offset, mask);
        }
    }
    let top = [161, 156, 158, 155][era];
    corners.extend([(top, 35), (top + 1, 63), (top + 2, 28)]);
    ResourceTerrainEdges {
        corners,
        tiles,
        isolated: [None, Some(top + 2), Some(top), Some(top + 1)],
    }
}

pub(super) fn add(
    map: &mut Map,
    terrain: &mut Terrain,
    grid: &TerrainGrid,
    edges: &ResourceTerrainEdges,
) {
    let mut existing: std::collections::BTreeSet<_> = map
        .resources
        .iter()
        .filter(|r| r.kind == "rock")
        .map(|r| r.position)
        .collect();
    for (cell, tile) in grid.tiles.iter().enumerate() {
        let Some(corners) = edges.corners.get(tile) else {
            continue;
        };
        let position = Position {
            x: (cell % grid.columns as usize) as i32 * 32 + 16,
            y: (cell / grid.columns as usize) as i32 * 32 + 16,
        };
        if existing.insert(position) {
            // Append, never reorder existing gold/oil/tree IDs from older saves.
            map.resources.push(ResourceSpawn {
                kind: "rock".into(),
                position,
                amount: 1,
                footprint: Footprint {
                    width: 32,
                    height: 32,
                },
                requires_extractor: false,
                terrain_corners: Some(*corners),
            });
        }
        // Rock ground is unbuildable dirt; the live resource blocks travel.
        terrain.flags[cell] = WALKABLE;
    }
}

pub(super) fn image(atlas: &Image, tile: u32) -> Image {
    let columns = atlas.width / 32;
    let mut rgba = Vec::with_capacity(4096);
    for y in tile / columns * 32..tile / columns * 32 + 32 {
        let start = ((y * atlas.width + tile % columns * 32) * 4) as usize;
        rgba.extend_from_slice(&atlas.rgba[start..start + 128]);
    }
    Image {
        width: 32,
        height: 32,
        rgba,
    }
}

pub(super) fn manifest(era: usize, image: ImageRef, depleted: ImageRef) -> ResourceManifest {
    ResourceManifest {
        kind: "rock".into(),
        positions: Vec::new(),
        anchor: [16, 16],
        image,
        terrain: true,
        terrain_edges: Some(edges(era)),
        depleted_image: Some(depleted),
        active_image: None,
        selection_circle: None,
        selection_y: 0,
    }
}

pub(super) fn tiles(era: usize) -> (u32, u32) {
    ([165, 160, 162, 160][era], [166, 161, 163, 161][era])
}
