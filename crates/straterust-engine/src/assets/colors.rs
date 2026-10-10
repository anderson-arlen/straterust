//! Exact replacement of a small source palette for player markings.
use super::*;
use crate::sim::PlayerId;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ColorRemap {
    /// Pairs of source RGB and replacement RGB; alpha is always preserved.
    pub colors: Vec<[[u8; 3]; 2]>,
}

impl ColorRemap {
    pub fn apply(&self, rgb: [u8; 3]) -> [u8; 3] {
        self.colors
            .iter()
            .find(|pair| pair[0] == rgb)
            .map_or(rgb, |pair| pair[1])
    }

    pub fn image(&self, source: &Image) -> Image {
        let mut image = source.clone();
        for pixel in image.rgba.as_chunks_mut::<4>().0 {
            let rgb = self.apply([pixel[0], pixel[1], pixel[2]]);
            pixel[..3].copy_from_slice(&rgb);
        }
        image
    }
}

pub(super) fn validate(players: &BTreeMap<PlayerId, ColorRemap>) -> Result<()> {
    ensure!(players.len() <= 64, "too many player color mappings");
    for (player, remap) in players {
        ensure!(
            player.0 < 64 && (1..=16).contains(&remap.colors.len()),
            "invalid player color mapping"
        );
        let mut sources = BTreeSet::new();
        ensure!(
            remap.colors.iter().all(|pair| sources.insert(pair[0])),
            "duplicate source player color"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_palette_mapping_preserves_other_colors_alpha_and_original_images() {
        let source = Image {
            width: 3,
            height: 1,
            rgba: vec![164, 0, 0, 255, 164, 0, 0, 0, 42, 41, 40, 110],
        };
        let map = ColorRemap {
            colors: vec![[[164, 0, 0], [12, 72, 204]]],
        };
        assert_eq!(
            map.image(&source).rgba,
            [12, 72, 204, 255, 12, 72, 204, 0, 42, 41, 40, 110]
        );
        assert_eq!(source.rgba[0], 164);
    }
}
