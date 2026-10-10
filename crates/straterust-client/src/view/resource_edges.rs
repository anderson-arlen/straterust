//! Reconnect authored terrain corners around disclosed resource depletion.
use super::*;
use straterust_engine::{assets::ResourceTerrainEdges, map::reconnect_resource_corners};

pub(super) fn replacements(
    grid: &TerrainGrid,
    edges: &ResourceTerrainEdges,
    cleared: &BTreeSet<(i32, i32)>,
) -> BTreeMap<(i32, i32), Option<u32>> {
    // Read only neighbours reached by edge changes, not the whole forest on
    // every redraw. Unchanged map art needs no allocation or traversal.
    let original = |x, y| {
        if x < 0 || y < 0 || x >= grid.columns as i32 || y >= grid.rows as i32 {
            return None;
        }
        grid.tiles
            .get((y as u32 * grid.columns + x as u32) as usize)
            .and_then(|tile| edges.corners.get(tile))
            .copied()
    };
    reconnect_resource_corners(grid.columns as i32, grid.rows as i32, original, cleared)
        .into_iter()
        .map(|(cell, mask)| {
            let tile = match mask {
                0 => None,
                28 => edges.isolated[1],
                35 => edges.isolated[2],
                63 => edges.isolated[3],
                _ => edges.tiles[usize::from(mask)],
            };
            (cell, tile)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cutting_a_narrow_column_caps_both_remaining_ends_without_flat_edges() {
        let mut grid = TerrainGrid {
            tile_size: 32,
            columns: 7,
            rows: 7,
            tiles: vec![0; 49],
        };
        for (y, tile) in [(1, 121), (2, 122), (3, 122), (4, 122), (5, 123)] {
            grid.tiles[y * 7 + 3] = tile;
        }
        let edges = ResourceTerrainEdges {
            corners: [(121, 3 + 32), (122, 15 + 48), (123, 12 + 16)].into(),
            tiles: [None; 16],
            isolated: [None, Some(123), Some(121), Some(122)],
        };
        let patches = replacements(&grid, &edges, &[(3, 3)].into());
        assert_eq!(patches[&(3, 2)], Some(123));
        assert_eq!(patches[&(3, 4)], Some(121));
        assert!(!patches.contains_key(&(3, 1)));
        assert!(!patches.contains_key(&(3, 5)));
        let patches = replacements(&grid, &edges, &[(3, 2), (3, 4)].into());
        assert_eq!(patches[&(3, 1)], None);
        assert_eq!(patches[&(3, 3)], None);
        assert_eq!(patches[&(3, 5)], None);
    }
    #[test]
    fn clearing_an_interior_cell_updates_all_shared_corners_but_preserves_unknown_trees() {
        let grid = TerrainGrid {
            tile_size: 32,
            columns: 5,
            rows: 5,
            tiles: vec![15; 25],
        };
        let edges = ResourceTerrainEdges {
            corners: (1..16).map(|i| (u32::from(i), i)).collect(),
            tiles: std::array::from_fn(|i| Some(i as u32)),
            isolated: [None; 4],
        };
        assert!(replacements(&grid, &edges, &BTreeSet::new()).is_empty());
        let patches = replacements(&grid, &edges, &[(2, 2)].into());
        assert_eq!(patches.len(), 8);
        assert_eq!(patches[&(1, 2)], Some(9));
        assert_eq!(patches[&(3, 2)], Some(6));
        assert_eq!(patches[&(2, 1)], Some(12));
        assert_eq!(patches[&(2, 3)], Some(3));
        assert_eq!(patches[&(1, 1)], Some(13));
        assert!(!patches.contains_key(&(0, 2)));
        assert!(!patches.contains_key(&(2, 2)));
    }
}
