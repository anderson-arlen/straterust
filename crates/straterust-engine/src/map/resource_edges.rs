//! Reconnect corner-based terrain resources after cells are removed.
use std::collections::{BTreeMap, BTreeSet};

/// Corners are TL=8, TR=4, BR=2, BL=1; narrow columns additionally connect
/// below (16) and above (32). Zero removes a fragment with no supported shape.
/// Unchanged cells retain their authored shape. Only known depletion starts
/// propagation, so presentation can use the same operation on remembered data.
pub fn reconnect_resource_corners(
    columns: i32,
    rows: i32,
    original: impl Fn(i32, i32) -> Option<u8>,
    cleared: &BTreeSet<(i32, i32)>,
) -> BTreeMap<(i32, i32), u8> {
    let mut result = BTreeMap::new();
    let corners = |x, y, patches: &BTreeMap<(i32, i32), u8>| {
        if x < 0 || y < 0 || x >= columns || y >= rows {
            return 15;
        }
        if cleared.contains(&(x, y)) {
            return 0;
        }
        patches
            .get(&(x, y))
            .copied()
            .or_else(|| original(x, y))
            .unwrap_or(0)
    };
    let mut pending = BTreeSet::new();
    for &(x, y) in cleared {
        pending.extend([(x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)]);
    }
    while let Some((x, y)) = pending.pop_first() {
        if cleared.contains(&(x, y)) || original(x, y).is_none() || result.get(&(x, y)) == Some(&0)
        {
            continue;
        }
        let up = corners(x, y - 1, &result);
        let right = corners(x + 1, y, &result);
        let down = corners(x, y + 1, &result);
        let left = corners(x - 1, y, &result);
        let mut mask = u8::from(up & 1 != 0 && left & 4 != 0) * 8
            + u8::from(up & 2 != 0 && right & 8 != 0) * 4
            + u8::from(right & 1 != 0 && down & 4 != 0) * 2
            + u8::from(left & 2 != 0 && down & 8 != 0);
        if down & 16 != 0 {
            mask |= u8::from(left & 6 != 0) + u8::from(right & 9 != 0) * 2;
        }
        if up & 32 != 0 {
            mask |= u8::from(left & 6 != 0) * 8 + u8::from(right & 9 != 0) * 4;
        }
        if mask == 0 {
            let column = usize::from(up & 3 != 0) + 2 * usize::from(down & 12 != 0);
            mask = [0, 12 + 16, 3 + 32, 15 + 48][column];
        }
        if corners(x, y, &result) != mask {
            result.insert((x, y), mask);
            pending.extend([(x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)]);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn isolated_fragments_disappear_and_remaining_columns_keep_both_caps() {
        let original: BTreeMap<_, _> = [(1, 35), (2, 63), (3, 63), (4, 63), (5, 28)]
            .map(|(y, mask)| ((3, y), mask))
            .into();
        let lookup = |x, y| original.get(&(x, y)).copied();
        let patches = reconnect_resource_corners(7, 7, lookup, &[(3, 3)].into());
        assert_eq!(patches[&(3, 2)], 28);
        assert_eq!(patches[&(3, 4)], 35);
        let patches = reconnect_resource_corners(7, 7, lookup, &[(3, 2), (3, 4)].into());
        assert_eq!(patches[&(3, 1)], 0);
        assert_eq!(patches[&(3, 3)], 0);
        assert_eq!(patches[&(3, 5)], 0);
        assert!(reconnect_resource_corners(7, 7, lookup, &BTreeSet::new()).is_empty());
    }
}
