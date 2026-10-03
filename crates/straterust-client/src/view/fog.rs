//! Cosmetic, rounded fog borders. Gameplay still uses the exact visibility grid.
use std::sync::OnceLock;

use straterust_engine::assets::Image;

pub const CELL: i32 = 32;
const TILE: u32 = CELL as u32;
const ACROSS: u32 = 16;
const PATTERNS: u32 = 512;
pub const FULL: u16 = 511;

/// Bit order is the row-major 3×3 neighborhood. The first mask covers explored
/// cells outside current sight; the second covers cells never explored.
pub fn masks(grid: &[u8], columns: i32, rows: i32, x: i32, y: i32) -> [u16; 2] {
    let mut masks = [0; 2];
    for dy in -1..=1 {
        for dx in -1..=1 {
            let nx = x + dx;
            let ny = y + dy;
            let visibility = if nx >= 0 && ny >= 0 && nx < columns && ny < rows {
                grid[(ny * columns + nx) as usize]
            } else {
                0
            };
            let bit = 1 << ((dy + 1) * 3 + dx + 1);
            if visibility == 1 {
                masks[0] |= bit;
            }
            if visibility == 0 {
                masks[1] |= bit;
            }
        }
    }
    masks
}

pub fn region(mask: u16, layer: usize) -> [u32; 4] {
    let tile = u32::from(mask) + layer as u32 * PATTERNS;
    [tile % ACROSS * TILE, tile / ACROSS * TILE, TILE, TILE]
}

/// Both layers share this one immutable 4 MiB texture. It is generated and
/// uploaded once, rather than repainting/uploading a fog image every frame.
pub fn atlas() -> &'static Image {
    static ATLAS: OnceLock<Image> = OnceLock::new();
    ATLAS.get_or_init(|| {
        // Integral of a raised-cosine blur kernel, with compact 32px support.
        // Integrating cell areas (rather than interpolating centers) rounds
        // diagonal notches and guarantees adjacent tiles use the same field.
        let integral = |distance: f64| {
            let t = (distance / 32.0).clamp(-1.0, 1.0);
            0.5 + 0.5 * t + (std::f64::consts::PI * t).sin() / (2.0 * std::f64::consts::PI)
        };
        let weights: [[f64; 3]; TILE as usize] = std::array::from_fn(|pixel| {
            std::array::from_fn(|neighbor| {
                let left = (neighbor as f64 - 1.0) * f64::from(TILE) - pixel as f64 - 0.5;
                integral(left + f64::from(TILE)) - integral(left)
            })
        });
        let mut image = Image {
            width: ACROSS * TILE,
            height: PATTERNS * 2 / ACROSS * TILE,
            rgba: vec![0; (PATTERNS * 2 * TILE * TILE * 4) as usize],
        };
        for mask in 0..=FULL {
            for y in 0..TILE {
                for x in 0..TILE {
                    let mut coverage = 0.0;
                    for row in 0..3 {
                        for col in 0..3 {
                            if mask & (1 << (row * 3 + col)) != 0 {
                                coverage += weights[y as usize][row] * weights[x as usize][col];
                            }
                        }
                    }
                    for (layer, opacity) in [170.0, 255.0].into_iter().enumerate() {
                        let [left, top, _, _] = region(mask, layer);
                        let pixel = ((top + y) * image.width + left + x) as usize * 4;
                        image.rgba[pixel + 3] = (coverage * opacity).round() as u8;
                    }
                }
            }
        }
        image
    })
}

/// The minimap samples the same field without recording more draw commands.
pub fn opacity(masks: [u16; 2], x: u32, y: u32) -> u32 {
    let image = atlas();
    let alpha = |layer| {
        let [left, top, _, _] = region(masks[layer], layer);
        u32::from(image.rgba[((top + y) * image.width + left + x) as usize * 4 + 3])
    };
    255 - ((255 - alpha(0)) * (255 - alpha(1)) + 127) / 255
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alpha(mask: u16, x: u32, y: u32) -> u8 {
        let image = atlas();
        let [left, top, _, _] = region(mask, 1);
        image.rgba[((top + y) * image.width + left + x) as usize * 4 + 3]
    }

    #[test]
    fn interiors_remain_clear_dim_or_opaque_and_borders_are_gradual() {
        for y in 0..TILE {
            for x in 0..TILE {
                assert_eq!(opacity([0, 0], x, y), 0);
                assert_eq!(opacity([FULL, 0], x, y), 170);
                assert_eq!(opacity([FULL, FULL], x, y), 255);
            }
        }
        let right_column = 0b100_100_100;
        let mut previous = 0;
        for x in 0..TILE {
            let a = alpha(right_column, x, 16);
            assert!(a >= previous);
            assert!(a - previous <= 11, "feather must not contain a sharp step");
            previous = a;
        }
        assert!(previous > 100 && previous < 140);
        // Never-explored borders get one black feather, rather than a second
        // dim overlay that would turn half coverage into almost full darkness.
        let border = masks(&[2, 2, 0, 2, 2, 0, 2, 2, 0], 3, 3, 1, 1);
        assert_eq!(border[0], 0);
        assert!((110..=140).contains(&opacity(border, 31, 16)));
        // A convex corner falls off in both axes, rather than becoming a
        // square strip or a diagonal cut through four constant-alpha cells.
        let corner = 0b100_000_000;
        assert!(alpha(corner, 31, 31) > alpha(corner, 16, 31));
        assert!(alpha(corner, 16, 31) > alpha(corner, 0, 31));
        assert_eq!(alpha(corner, 20, 31), alpha(corner, 31, 20));
    }

    #[test]
    fn adjacent_masks_are_continuous_even_at_diagonal_notches() {
        // Check every possible pair of horizontal neighbors (a 4×3 region),
        // including concave corners, isolated cells and narrow sight gaps.
        for pattern in 0..4096 {
            let grid: Vec<_> = (0..12)
                .map(|bit| if pattern & (1 << bit) == 0 { 2 } else { 0 })
                .collect();
            let a = masks(&grid, 4, 3, 1, 1)[1];
            let b = masks(&grid, 4, 3, 2, 1)[1];
            for y in 0..TILE {
                assert!(alpha(a, 31, y).abs_diff(alpha(b, 0, y)) <= 11);
            }
        }
        assert_eq!(masks(&[2; 9], 3, 3, 1, 1), [0, 0]);
        assert_eq!(masks(&[1; 9], 3, 3, 1, 1), [FULL, 0]);
        assert_eq!(masks(&[0; 9], 3, 3, 1, 1), [0, FULL]);
        assert_ne!(masks(&[2; 9], 3, 3, 0, 0), [0, 0]);
    }
}
