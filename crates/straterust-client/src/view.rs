//! Shared GPU scene and reference painting. No mutable simulation access.
mod fog;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

use font8x8::UnicodeFonts;
use serde::Deserialize;
use straterust_engine::{
    assets::{AssetPack, ClipKind, Image, TerrainGrid},
    media::MediaPack,
    sim::{
        EntityId, PlayerId, Position, ResearchId, ResourceId, UnitOrder, UnitTypeId, Visibility,
        World,
    },
};

use crate::{
    controls::{
        Action, BUILD_GRID, Button, button_rect, command_width, contains, minimap_rect, native_ui,
        native_ui_scale, selection_rect,
    },
    visual::{self, VisualAction, Visuals},
};

pub const HEADER: f64 = 48.0;
pub const FOOTER: f64 = 224.0;

// Original packs can provide a directional shadow clip. Other flying units
// share one small translucent ellipse, scaled to their ground footprint.
static FLYING_SHADOW: LazyLock<Image> = LazyLock::new(|| {
    let mut rgba = vec![0; 32 * 16 * 4];
    for y in 0..16 {
        for x in 0..32 {
            let dx = 2 * x - 31;
            let dy = 2 * y - 15;
            if dx * dx + 4 * dy * dy <= 31 * 31 {
                rgba[((y * 32 + x) * 4 + 3) as usize] = 100;
            }
        }
    }
    Image {
        width: 32,
        height: 16,
        rgba,
    }
});

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Presentation {
    pub schema_version: u32,
    /// Gameplay supply is an integer budget; packs can display half-units.
    #[serde(default = "default_supply_divisor")]
    pub supply_divisor: u32,
    pub ground: u32,
    pub grid: u32,
    pub friendly: u32,
    pub opposing: u32,
    pub unit_radius: u32,
    #[serde(default)]
    pub unit_names: BTreeMap<UnitTypeId, String>,
    #[serde(default)]
    pub objective: Option<String>,
    #[serde(default)]
    pub research_names: BTreeMap<ResearchId, String>,
    #[serde(default)]
    pub research_keys: BTreeMap<ResearchId, String>,
    #[serde(default)]
    pub train_keys: BTreeMap<UnitTypeId, String>,
    #[serde(default)]
    pub train_slots: BTreeMap<UnitTypeId, u8>,
    #[serde(default)]
    pub build_buttons: BTreeMap<UnitTypeId, BuildButton>,
    #[serde(default)]
    pub command_buttons: BTreeMap<String, CommandButton>,
    #[serde(default)]
    pub command_keys: BTreeMap<String, String>,
}

fn default_supply_divisor() -> u32 {
    1
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandButton {
    pub slot: u8,
    pub key: String,
    pub label: String,
    pub tip: String,
    pub icon: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BuildButton {
    pub advanced: bool,
    pub slot: u8,
    pub key: String,
}

impl Default for Presentation {
    fn default() -> Self {
        Self {
            schema_version: 1,
            supply_divisor: 1,
            ground: 0x182d2c,
            grid: 0x223a37,
            friendly: 0x85dfb5,
            opposing: 0xe8ad78,
            unit_radius: 10,
            unit_names: BTreeMap::new(),
            objective: None,
            research_names: BTreeMap::new(),
            research_keys: BTreeMap::new(),
            train_keys: BTreeMap::new(),
            train_slots: BTreeMap::new(),
            build_buttons: BTreeMap::new(),
            command_buttons: BTreeMap::new(),
            command_keys: BTreeMap::new(),
        }
    }
}

impl Presentation {
    pub fn supply_text(&self, amount: u32) -> String {
        if amount.is_multiple_of(self.supply_divisor) {
            (amount / self.supply_divisor).to_string()
        } else {
            format!("{}.5", amount / self.supply_divisor)
        }
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            matches!(self.supply_divisor, 1 | 2),
            "invalid supply divisor"
        );
        anyhow::ensure!(
            self.command_keys.len() <= 128
                && self.command_keys.iter().all(|(name, key)| {
                    !name.is_empty()
                        && name.len() <= 128
                        && !name.chars().any(char::is_control)
                        && crate::controls::parse_key(key).is_some()
                }),
            "invalid command hotkeys"
        );
        anyhow::ensure!(
            self.command_buttons.len() <= 128
                && self.command_buttons.iter().all(|(name, button)| {
                    !name.is_empty()
                        && name.len() <= 128
                        && button.slot < 9
                        && crate::controls::parse_key(&button.key).is_some()
                        && [&button.label, &button.tip, &button.icon]
                            .iter()
                            .all(|text| {
                                !text.is_empty()
                                    && text.len() <= 512
                                    && !text.chars().any(char::is_control)
                            })
                }),
            "invalid command buttons"
        );
        anyhow::ensure!(
            self.build_buttons.len() <= 128
                && self
                    .build_buttons
                    .values()
                    .all(|button| button.slot < 8
                        && crate::controls::parse_key(&button.key).is_some()),
            "invalid build buttons"
        );
        anyhow::ensure!(
            self.unit_names.len() <= 4096
                && self.unit_names.values().all(|name| !name.trim().is_empty()
                    && name.len() <= 128
                    && !name.chars().any(char::is_control)),
            "invalid presentation unit names"
        );
        anyhow::ensure!(
            self.objective
                .as_ref()
                .is_none_or(|text| !text.trim().is_empty()
                    && text.len() <= 160
                    && !text.chars().any(char::is_control)),
            "invalid presentation objective"
        );
        anyhow::ensure!(
            self.research_names.len() <= 128
                && self.research_keys.len() <= 128
                && self.research_names.values().all(|name| !name.is_empty()
                    && name.len() <= 128
                    && !name.chars().any(char::is_control))
                && self
                    .research_keys
                    .values()
                    .all(|key| crate::controls::parse_key(key).is_some()),
            "invalid research presentation"
        );
        anyhow::ensure!(
            self.train_slots.len() <= 128
                && self.train_slots.values().all(|slot| *slot < 9)
                && self.train_keys.len() <= 128
                && self
                    .train_keys
                    .values()
                    .all(|key| crate::controls::parse_key(key).is_some()),
            "invalid training hotkeys"
        );
        anyhow::ensure!(self.schema_version == 1, "unsupported presentation schema");
        anyhow::ensure!(
            (2..=32).contains(&self.unit_radius),
            "unit_radius must be 2..=32"
        );
        anyhow::ensure!(
            [self.ground, self.grid, self.friendly, self.opposing]
                .iter()
                .all(|color| *color <= 0xffffff),
            "presentation colours must be 24-bit RGB"
        );
        Ok(())
    }

    pub fn unit_name(&self, assets: Option<&AssetPack>, id: UnitTypeId) -> String {
        assets
            .and_then(|assets| assets.sprite(id))
            .map(|sprite| sprite.name.to_owned())
            .or_else(|| self.unit_names.get(&id).cloned())
            .unwrap_or_else(|| format!("Unit {}", id.0))
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Camera {
    pub x: f64,
    pub y: f64,
    pub zoom: f64,
}

impl Camera {
    /// World bounds of the map viewport, excluding the fixed HUD panels.
    fn visible_world(&self, size: [f64; 2]) -> [f64; 4] {
        let half_width = size[0] / (2.0 * self.zoom);
        let half_height = (size[1] - HEADER - FOOTER).max(0.0) / (2.0 * self.zoom);
        [
            self.x - half_width,
            self.y - half_height,
            self.x + half_width,
            self.y + half_height,
        ]
    }

    pub fn clamp_to_map(&mut self, map_size: [i32; 2], size: [f64; 2]) {
        let viewport = [size[0], (size[1] - HEADER - FOOTER).max(0.0)];
        for (center, extent, screen) in [
            (&mut self.x, f64::from(map_size[0]), viewport[0]),
            (&mut self.y, f64::from(map_size[1]), viewport[1]),
        ] {
            let half = screen / (2.0 * self.zoom);
            *center = if extent <= 2.0 * half {
                extent / 2.0
            } else {
                center.clamp(half, extent - half)
            };
        }
    }

    fn visible_tiles(&self, grid: &TerrainGrid, size: [f64; 2]) -> [u32; 4] {
        let bounds = self.visible_world(size);
        let tile = f64::from(grid.tile_size);
        [
            (bounds[0] / tile)
                .floor()
                .clamp(0.0, f64::from(grid.columns)) as u32,
            (bounds[1] / tile).floor().clamp(0.0, f64::from(grid.rows)) as u32,
            (bounds[2] / tile)
                .ceil()
                .clamp(0.0, f64::from(grid.columns)) as u32,
            (bounds[3] / tile).ceil().clamp(0.0, f64::from(grid.rows)) as u32,
        ]
    }

    pub fn world_to_screen(&self, x: f64, y: f64, size: [f64; 2]) -> [f64; 2] {
        [
            (x - self.x) * self.zoom + size[0] / 2.0,
            (y - self.y) * self.zoom + (size[1] + HEADER - FOOTER) / 2.0,
        ]
    }

    pub fn screen_to_world(&self, screen: [f64; 2], size: [f64; 2]) -> Option<Position> {
        if screen[0] < 0.0
            || screen[0] >= size[0]
            || screen[1] < HEADER
            || screen[1] >= size[1] - FOOTER
        {
            return None;
        }
        Some(Position {
            x: ((screen[0] - size[0] / 2.0) / self.zoom + self.x).round() as i32,
            y: ((screen[1] - (size[1] + HEADER - FOOTER) / 2.0) / self.zoom + self.y).round()
                as i32,
        })
    }
}

pub fn unit_half_size(world: &World, unit_type: UnitTypeId, art: &Presentation) -> [f64; 2] {
    let definition = world
        .rules()
        .units
        .iter()
        .find(|unit| unit.id == unit_type)
        .unwrap();
    [
        (f64::from(definition.footprint.width) / 2.0).max(f64::from(art.unit_radius)),
        (f64::from(definition.footprint.height) / 2.0).max(f64::from(art.unit_radius)),
    ]
}

pub struct View<'a> {
    pub world: &'a World,
    pub visuals: &'a Visuals,
    pub cursor: [f64; 2],
    pub targeting: bool,
    pub presentation: &'a Presentation,
    pub assets: Option<&'a AssetPack>,
    pub map_art: Option<&'a straterust_engine::assets::DecodedMapArtwork>,
    pub media: Option<&'a MediaPack>,
    pub speaking: Option<UnitTypeId>,
    pub mission: Option<&'a crate::mission::MissionUi>,
    /// Client wall-clock time only; never simulation time or gameplay animation timing.
    pub animation_ms: u128,
    pub portrait_ms: u128,
    pub camera: Camera,
    pub selected: &'a BTreeSet<EntityId>,
    pub selected_resource: Option<ResourceId>,
    pub drag_box: Option<[[f64; 2]; 2]>,
    pub paused: bool,
    pub playback: bool,
    pub status: &'a str,
    pub buttons: &'a [Button],
    pub help: &'a str,
    pub placement: Option<(UnitTypeId, Position, bool)>,
    /// Placement mode persists while the pointer is over the console.
    pub placement_type: Option<UnitTypeId>,
    pub ending_hint: &'a str,
}

mod fog_render;
mod hud;
mod indicators;
mod menu;
mod minimap;
mod mission_ui;
pub(crate) use menu::draw_menu;
#[cfg(test)]
pub(crate) use menu::draw_menu_pixels;
pub(crate) use menu::menu_rect;
mod coverage;
mod resources;
mod results;
mod selection;
mod terrain;
mod world;
impl<'a> View<'a> {
    /// Pixel reference renderer for tests and headless comparisons.
    #[cfg(test)]
    pub fn draw(&self, pixels: &mut [u32], width: u32, height: u32, scale: f64) {
        self.paint(
            Canvas {
                scene: None,
                pixels,
                width: width as usize,
                height: height as usize,
                scale,
            },
            width,
            height,
            scale,
        );
    }

    pub fn scene(&self, width: u32, height: u32, scale: f64) -> crate::gpu::Scene<'a> {
        let mut scene = crate::gpu::Scene {
            width,
            height,
            clear: 0,
            commands: Vec::new(),
        };
        self.paint(
            Canvas {
                scene: Some(&mut scene),
                pixels: &mut [],
                width: width as usize,
                height: height as usize,
                scale,
            },
            width,
            height,
            scale,
        );
        scene
    }
}

/// Shared hit box for the mission result panel.
pub fn ending_rect(size: [f64; 2]) -> [f64; 4] {
    [
        (size[0] - 360.0) / 2.0,
        (size[1] + HEADER - FOOTER) / 2.0 - 35.0,
        360.0,
        70.0,
    ]
}

#[cfg(test)]
mod tests;

mod canvas;
use canvas::*;

mod widgets;
pub(crate) use hud::draw_frame_stats;
use widgets::*;
