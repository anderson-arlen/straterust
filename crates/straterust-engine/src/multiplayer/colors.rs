//! Lobby color choices are presentation data, separate from simulation ownership.
use super::*;
use crate::assets::{ColorRemap, MapArtwork};
use std::collections::BTreeSet;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlayerColor {
    pub name: String,
    pub shades: Vec<[u8; 3]>,
}
impl PlayerColor {
    pub fn rgb(&self) -> u32 {
        let rgb = self.shades.iter().max_by_key(|rgb| {
            // A bright shade makes the marker legible, including dark palettes.
            u32::from(rgb[0]) + u32::from(rgb[1]) + u32::from(rgb[2])
        });
        rgb.map_or(0xaaaaaa, |[r, g, b]| {
            (u32::from(*r) << 16) | (u32::from(*g) << 8) | u32::from(*b)
        })
    }
}
impl Catalog {
    /// Old imports already contain player ramps in their map art. Recover their
    /// selectable palette without source media or rewriting a read-only game.
    pub fn load_legacy_colors(&mut self, root: &Path) -> Result<()> {
        if !self.colors.is_empty() {
            return Ok(());
        }
        let count = self.maps.iter().map(|m| m.players).max().unwrap_or(0);
        for map in self.maps.iter().filter(|m| m.players == count) {
            let directory = package_path(root, &map.package)?;
            if !directory.join("assets.ron").is_file() {
                continue;
            }
            let manifest = crate::assets::AssetManifest::load_manifest(&directory)?;
            let mut colors = Vec::new();
            for (_, ramp) in manifest
                .player_colors
                .into_iter()
                .filter(|(p, _)| p.0 < u16::from(count))
            {
                let shades: Vec<_> = ramp.colors.into_iter().map(|p| p[1]).collect();
                if !colors.iter().any(|c: &PlayerColor| c.shades == shades) {
                    colors.push(PlayerColor {
                        name: format!("Color {}", colors.len() + 1),
                        shades,
                    });
                }
            }
            if colors.len() == usize::from(count) {
                self.colors = colors;
                return self.validate_colors();
            }
        }
        Ok(())
    }

    pub(super) fn validate_colors(&self) -> Result<()> {
        ensure!(
            self.colors.len() <= 8
                && (self.colors.is_empty()
                    || self
                        .maps
                        .iter()
                        .all(|m| usize::from(m.players) <= self.colors.len())),
            "not enough distinct player colors"
        );
        let mut ramps = BTreeSet::new();
        ensure!(
            self.colors.iter().all(|color| {
                !color.name.is_empty()
                    && color.name.len() <= 32
                    && (1..=16).contains(&color.shades.len())
                    && color.shades.len() == self.colors[0].shades.len()
                    && ramps.insert(&color.shades)
            }),
            "invalid or duplicate player colors"
        );
        Ok(())
    }

    /// Older settings without a choice receive the first remaining palette.
    /// Explicit choices reserve their color before automatic assignments.
    pub fn slot_colors(&self, settings: &Settings) -> Result<Vec<Option<usize>>> {
        let mut used = BTreeSet::new();
        let mut colors = vec![None; settings.slots.len()];
        for (i, slot) in settings.slots.iter().enumerate() {
            if let Some(color) = slot.color {
                ensure!(color < self.colors.len(), "unknown player color");
                if slot.kind != SlotKind::Closed {
                    ensure!(used.insert(color), "player color is already in use");
                    colors[i] = Some(color);
                }
            }
        }
        if !self.colors.is_empty() {
            for (i, slot) in settings.slots.iter().enumerate() {
                if slot.kind != SlotKind::Closed && colors[i].is_none() {
                    let color = (0..self.colors.len())
                        .find(|c| !used.contains(c))
                        .context("no unused player color")?;
                    used.insert(color);
                    colors[i] = Some(color);
                }
            }
        }
        Ok(colors)
    }

    pub fn assign_colors(&self, settings: &mut Settings) -> Result<()> {
        let colors = self.slot_colors(settings)?;
        for (slot, color) in settings.slots.iter_mut().zip(colors) {
            slot.color = color;
        }
        Ok(())
    }

    pub fn choose_color(&self, settings: &mut Settings, slot: usize, color: usize) -> Result<()> {
        ensure!(color < self.colors.len(), "unknown player color");
        ensure!(
            slot < settings.slots.len() && settings.slots[slot].kind != SlotKind::Closed,
            "position is closed"
        );
        ensure!(
            !self
                .slot_colors(settings)?
                .iter()
                .enumerate()
                .any(|(i, c)| i != slot && *c == Some(color)),
            "player color is already in use"
        );
        settings.slots[slot].color = Some(color);
        Ok(())
    }

    pub(super) fn apply_colors(&self, settings: &Settings, artwork: &mut MapArtwork) -> Result<()> {
        let Some(source) = artwork.player_colors.values().next() else {
            anyhow::bail!("map artwork lacks player color markings");
        };
        let sources: Vec<_> = source.colors.iter().map(|pair| pair[0]).collect();
        for (slot, color) in self.slot_colors(settings)?.into_iter().enumerate() {
            let Some(color) = color else { continue };
            let palette = &self.colors[color];
            ensure!(
                sources.len() == palette.shades.len(),
                "player palette size mismatch"
            );
            artwork.player_colors.insert(
                PlayerId(slot as u16),
                ColorRemap {
                    colors: sources
                        .iter()
                        .zip(&palette.shades)
                        .map(|(from, to)| [*from, *to])
                        .collect(),
                },
            );
        }
        Ok(())
    }
}
