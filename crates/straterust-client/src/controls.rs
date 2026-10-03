//! Local bindings and command buttons; gameplay remains in the command queue.
use std::collections::BTreeSet;

use anyhow::{Result, ensure};
use serde::Deserialize;
use straterust_engine::{
    map::Footprint,
    sim::{
        EntityId, Order, PlayerId, Position, Rejection, ResearchEffect, ResearchId, ResourceId,
        UnitTypeId,
    },
};
use winit::keyboard::KeyCode;

use crate::{App, audio::Cue, home_position, view::FOOTER};

/// Build tiles are distinct from the finer terrain walkability grid.
pub const BUILD_GRID: i32 = 32;

/// Native cursor policy: round the footprint's upper-left corner to the nearest
/// build tile, with midpoint ties toward the positive axis. Keep out-of-map
/// candidates outside the map so the ordinary placement check rejects them.
fn snap_build_position(position: Position, footprint: Footprint) -> Position {
    let snap = |coordinate: i32, extent: u16| {
        let half = i64::from(extent / 2);
        let grid = i64::from(BUILD_GRID);
        let origin = (i64::from(coordinate) - half + grid / 2).div_euclid(grid) * grid;
        // Only limit the integer representation, never clamp to map edges.
        (origin + half).clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
    };
    Position {
        x: snap(position.x, footprint.width),
        y: snap(position.y, footprint.height),
    }
}

#[derive(Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Bindings {
    pub move_unit: String,
    pub gather: String,
    pub repair: String,
    pub rally: String,
    pub build_menu: String,
    pub advanced_build_menu: String,
    pub attack: String,
    pub hold: String,
    pub patrol: String,
    pub stop: String,
    pub cancel: String,
    pub build_1: String,
    pub build_2: String,
    pub build_3: String,
    pub build_4: String,
    pub build_5: String,
    pub build_6: String,
    pub build_7: String,
    pub build_8: String,
    pub train_1: String,
    pub train_2: String,
    pub restart: String,
    pub pause: String,
    pub home: String,
}

impl Default for Bindings {
    fn default() -> Self {
        Self {
            move_unit: "M".into(),
            gather: "G".into(),
            repair: "R".into(),
            rally: "R".into(),
            build_menu: "B".into(),
            advanced_build_menu: "V".into(),
            attack: "A".into(),
            hold: "H".into(),
            patrol: "P".into(),
            stop: "S".into(),
            cancel: "X".into(),
            build_1: "C".into(),
            build_2: "S".into(),
            build_3: "B".into(),
            build_4: "A".into(),
            build_5: "U".into(),
            build_6: "R".into(),
            build_7: "E".into(),
            build_8: "T".into(),
            train_1: "V".into(),
            train_2: "M".into(),
            restart: "F5".into(),
            pause: "Space".into(),
            home: "Home".into(),
        }
    }
}

impl Bindings {
    pub fn validate(&self) -> Result<()> {
        // A key may be reused in a different menu (B opens Build, then B builds).
        for menu in [
            vec![
                &self.move_unit,
                &self.attack,
                &self.hold,
                &self.patrol,
                &self.stop,
                &self.gather,
                &self.repair,
                &self.build_menu,
                &self.advanced_build_menu,
                &self.cancel,
            ],
            vec![&self.train_1, &self.train_2, &self.rally, &self.cancel],
            vec![
                &self.build_1,
                &self.build_2,
                &self.build_3,
                &self.build_4,
                &self.build_5,
                &self.build_6,
                &self.build_7,
                &self.build_8,
                &self.cancel,
            ],
        ] {
            let mut seen = BTreeSet::new();
            for name in menu
                .into_iter()
                .chain([&self.restart, &self.pause, &self.home])
            {
                let key = parse_key(name).ok_or_else(|| {
                    anyhow::anyhow!(
                        "unsupported binding {name:?}; use A..Z, Space, Home or F1..F10/F12"
                    )
                })?;
                ensure!(
                    seen.insert(key),
                    "duplicate keyboard binding {name} in one menu"
                );
            }
        }
        Ok(())
    }
}

pub fn parse_key(name: &str) -> Option<KeyCode> {
    Some(match name {
        "A" => KeyCode::KeyA,
        "B" => KeyCode::KeyB,
        "C" => KeyCode::KeyC,
        "D" => KeyCode::KeyD,
        "E" => KeyCode::KeyE,
        "F" => KeyCode::KeyF,
        "G" => KeyCode::KeyG,
        "H" => KeyCode::KeyH,
        "I" => KeyCode::KeyI,
        "J" => KeyCode::KeyJ,
        "K" => KeyCode::KeyK,
        "L" => KeyCode::KeyL,
        "M" => KeyCode::KeyM,
        "N" => KeyCode::KeyN,
        "O" => KeyCode::KeyO,
        "P" => KeyCode::KeyP,
        "Q" => KeyCode::KeyQ,
        "R" => KeyCode::KeyR,
        "S" => KeyCode::KeyS,
        "T" => KeyCode::KeyT,
        "U" => KeyCode::KeyU,
        "V" => KeyCode::KeyV,
        "W" => KeyCode::KeyW,
        "X" => KeyCode::KeyX,
        "Y" => KeyCode::KeyY,
        "Z" => KeyCode::KeyZ,
        "Space" => KeyCode::Space,
        "Home" => KeyCode::Home,
        "F1" => KeyCode::F1,
        "F2" => KeyCode::F2,
        "F3" => KeyCode::F3,
        "F4" => KeyCode::F4,
        "F5" => KeyCode::F5,
        "F6" => KeyCode::F6,
        "F7" => KeyCode::F7,
        "F8" => KeyCode::F8,
        "F9" => KeyCode::F9,
        "F10" => KeyCode::F10,
        "F12" => KeyCode::F12,
        _ => return None,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Move,
    Gather,
    Repair,
    Rally,
    BuildMenu,
    AdvancedBuildMenu,
    Back,
    Build(UnitTypeId),
    Train(UnitTypeId),
    Research(ResearchId),
    Stim,
    Cloak(bool),
    Scan,
    Unload,
    Lift,
    Land,
    PlaceMine,
    AttackMove,
    Patrol,
    Hold,
    Stop,
    Cancel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetMode {
    Unload,
    Land,
    PlaceMine,
    Move,
    Scan,
    Gather,
    Repair,
    Rally,
    Build(UnitTypeId),
    AttackMove,
    Patrol,
}

pub struct Button {
    pub action: Action,
    pub slot: usize,
    pub label: String,
    pub key: String,
    pub tooltip: Vec<String>,
    pub disabled: Option<String>,
}

/// Shared native-console geometry keeps painted controls and input aligned.
/// Console strips preserve their source aperture sizes at every supported width.
pub fn native_ui(assets: Option<&straterust_engine::assets::AssetPack>) -> bool {
    assets
        .and_then(|assets| assets.ui_image("console"))
        .is_some_and(|image| image.width == 640 && image.height == 480)
}
pub fn native_ui_scale(size: [f64; 2]) -> f64 {
    (size[0] / 640.0).min(FOOTER / 186.0)
}

/// Original statdata.bin controls 33..44: six columns, filled top then bottom.
/// Painting and picking share the same logical coordinates, independent of DPI.
pub fn selection_rect(slot: usize, size: [f64; 2], native: bool) -> [f64; 4] {
    if native {
        let scale = native_ui_scale(size);
        return [
            (168.0 + (slot / 2) as f64 * 36.0) * scale,
            size[1] - (84.0 - (slot % 2) as f64 * 37.0) * scale,
            33.0 * scale,
            34.0 * scale,
        ];
    }
    let left = (size[0] * 0.23).clamp(150.0, 210.0) + 14.0;
    let width = (size[0] - command_width(size) - left - 12.0) / 6.0;
    [
        left + (slot / 2) as f64 * width,
        size[1] - FOOTER + 38.0 + (slot % 2) as f64 * 74.0,
        width - 4.0,
        68.0,
    ]
}

pub fn command_width(size: [f64; 2]) -> f64 {
    (size[0] * 0.29).clamp(264.0, 300.0)
}

pub fn button_rect(slot: usize, size: [f64; 2], native: bool) -> [f64; 4] {
    if native {
        let scale = native_ui_scale(size);
        return [
            size[0] - (132.0 - (slot % 3) as f64 * 46.0) * scale,
            size[1] - (120.0 - (slot / 3) as f64 * 40.0) * scale,
            36.0 * scale,
            34.0 * scale,
        ];
    }
    let panel = command_width(size);
    let width = (panel - 24.0) / 3.0;
    [
        size[0] - panel + 10.0 + (slot % 3) as f64 * width,
        size[1] - FOOTER + 28.0 + (slot / 3) as f64 * 57.0,
        width - 5.0,
        52.0,
    ]
}

pub fn contains(rect: [f64; 4], point: [f64; 2]) -> bool {
    point[0] >= rect[0]
        && point[0] < rect[0] + rect[2]
        && point[1] >= rect[1]
        && point[1] < rect[1] + rect[3]
}

pub fn button_at(
    buttons: &[Button],
    point: [f64; 2],
    size: [f64; 2],
    native: bool,
) -> Option<Action> {
    buttons
        .iter()
        .find(|button| contains(button_rect(button.slot, size, native), point))
        .map(|button| button.action)
}

/// Fit the whole map inside the minimap well; hit testing uses the same rectangle.
pub fn minimap_rect(size: [f64; 2], map: [i32; 2], native: bool) -> [f64; 4] {
    if native {
        let scale = native_ui_scale(size);
        let zoom = 128.0 * scale / f64::from(map[0].max(map[1]));
        let width = f64::from(map[0]) * zoom;
        let height = f64::from(map[1]) * zoom;
        return [
            6.0 * scale + (128.0 * scale - width) / 2.0,
            size[1] - 132.0 * scale + (128.0 * scale - height) / 2.0,
            width,
            height,
        ];
    }
    let available = (size[0] * 0.23).clamp(150.0, 210.0) - 20.0;
    let zoom = (available / f64::from(map[0])).min(164.0 / f64::from(map[1]));
    let width = f64::from(map[0]) * zoom;
    let height = f64::from(map[1]) * zoom;
    [
        10.0 + (available - width) / 2.0,
        size[1] - FOOTER + 28.0 + (164.0 - height) / 2.0,
        width,
        height,
    ]
}

pub fn minimap_position(
    point: [f64; 2],
    size: [f64; 2],
    map: [i32; 2],
    native: bool,
) -> Option<Position> {
    let rect = minimap_rect(size, map, native);
    contains(rect, point).then(|| Position {
        x: ((point[0] - rect[0]) / rect[2] * f64::from(map[0])) as i32,
        y: ((point[1] - rect[1]) / rect[3] * f64::from(map[1])) as i32,
    })
}

pub fn group_index(key: KeyCode) -> Option<usize> {
    match key {
        KeyCode::Digit1 => Some(0),
        KeyCode::Digit2 => Some(1),
        KeyCode::Digit3 => Some(2),
        KeyCode::Digit4 => Some(3),
        KeyCode::Digit5 => Some(4),
        KeyCode::Digit6 => Some(5),
        KeyCode::Digit7 => Some(6),
        KeyCode::Digit8 => Some(7),
        KeyCode::Digit9 => Some(8),
        KeyCode::Digit0 => Some(9),
        _ => None,
    }
}

mod buttons;
mod orders;
impl App {}

#[cfg(test)]
mod tests;
