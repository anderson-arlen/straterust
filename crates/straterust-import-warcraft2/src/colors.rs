//! Original four-shade team palettes, remapped after the local-player slot swap.
use super::{gfx::Palette, pud::Pud};
use std::collections::BTreeMap;
use straterust_engine::{assets::ColorRemap, sim::PlayerId};

pub fn players(pud: &Pud, palette: &Palette) -> BTreeMap<PlayerId, ColorRemap> {
    const COLORS: [[[u8; 3]; 4]; 8] = [
        [[164, 0, 0], [124, 0, 0], [92, 4, 0], [68, 4, 0]],
        [[12, 72, 204], [4, 40, 160], [0, 20, 116], [0, 4, 76]],
        [[44, 180, 148], [20, 132, 92], [4, 84, 44], [0, 40, 12]],
        [[152, 72, 176], [116, 44, 132], [80, 24, 88], [44, 8, 44]],
        [[248, 140, 20], [200, 96, 16], [152, 60, 16], [108, 32, 12]],
        [[40, 40, 60], [28, 28, 44], [20, 20, 32], [12, 12, 20]],
        [
            [224, 224, 224],
            [152, 152, 180],
            [84, 84, 128],
            [36, 40, 76],
        ],
        [
            [252, 252, 72],
            [228, 204, 40],
            [204, 160, 16],
            [180, 116, 0],
        ],
    ];
    (0..16)
        .map(|source| {
            let colors = (0..4)
                .map(|shade| {
                    [
                        palette[208 + shade][..3].try_into().unwrap(),
                        COLORS[source % 8][shade],
                    ]
                })
                .collect();
            (
                PlayerId(u16::from(pud.player(source))),
                ColorRemap { colors },
            )
        })
        .collect()
}
