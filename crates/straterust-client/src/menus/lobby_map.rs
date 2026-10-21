//! The lobby edits seats at their actual starting positions.
use super::*;
use straterust_engine::multiplayer::{GameType, Settings, SlotKind};

pub const MAP_RECT: [u16; 4] = [24, 100, 384, 240];
pub struct Marker {
    pub slot: usize,
    pub point: [u16; 2],
    pub rect: [u16; 4],
}
fn add(out: &mut Vec<Choice>, label: String, rect: [u16; 4], pick: Pick) {
    let mut button = straterust_engine::menus::button(&label, rect[1], MenuAction::Resume);
    button.rect = rect;
    out.push(Choice { button, pick });
}
impl MenuUi {
    pub fn slot_settings(&self) -> Option<&Settings> {
        self.lobby
            .as_ref()
            .map(|(_, v)| &v.settings)
            .or(self.match_settings.as_ref())
    }
    pub fn slot_map_rect(&self) -> [u16; 4] {
        let [x, y, w, h] = MAP_RECT;
        let Some(p) = &self.map_browser.preview else {
            return MAP_RECT;
        };
        let scale =
            (f64::from(w) / f64::from(p.image.width)).min(f64::from(h) / f64::from(p.image.height));
        let iw = (f64::from(p.image.width) * scale) as u16;
        let ih = (f64::from(p.image.height) * scale) as u16;
        [x + (w - iw) / 2, y + (h - ih) / 2, iw, ih]
    }
    pub fn slot_markers(&self) -> Vec<Marker> {
        let Some(preview) = &self.map_browser.preview else {
            return Vec::new();
        };
        if self.map_browser.preview_map != Some(self.match_map) {
            return Vec::new();
        }
        let Some(settings) = self.slot_settings() else {
            return Vec::new();
        };
        let [x, y, w, h] = self.slot_map_rect();
        let mut markers: Vec<Marker> = Vec::new();
        for slot in 0..settings.slots.len() {
            let Some(start) = preview
                .starts
                .iter()
                .find(|s| usize::from(s.player.0) == slot)
            else {
                continue;
            };
            let point = [
                x + (i64::from(start.position.x) * i64::from(w) / i64::from(preview.size[0]))
                    .clamp(0, i64::from(w)) as u16,
                y + (i64::from(start.position.y) * i64::from(h) / i64::from(preview.size[1]))
                    .clamp(0, i64::from(h)) as u16,
            ];
            let mut positions: Vec<_> = (-8..=8)
                .flat_map(|dy: i32| (-4..=4).map(move |dx: i32| (dx * 82, dy * 24)))
                .collect();
            positions.sort_by_key(|(dx, dy)| (dx * dx + dy * dy, *dy, *dx));
            let rect = positions
                .into_iter()
                .map(|(dx, dy)| {
                    [
                        (i32::from(point[0]) - 38 + dx).clamp(24, 330) as u16,
                        (i32::from(point[1]) - 10 + dy).clamp(100, 318) as u16,
                        78,
                        22,
                    ]
                })
                .find(|r| {
                    !markers.iter().any(|m| {
                        r[0] < m.rect[0] + m.rect[2] + 2
                            && r[0] + r[2] + 2 > m.rect[0]
                            && r[1] < m.rect[1] + m.rect[3] + 2
                            && r[1] + r[3] + 2 > m.rect[1]
                    })
                })
                .unwrap_or([24, 100 + slot as u16 * 26, 78, 22]);
            markers.push(Marker { slot, point, rect });
        }
        markers
    }
    pub fn slot_name(&self, slot: usize) -> String {
        self.lobby
            .as_ref()
            .and_then(|(_, v)| v.players.get(slot))
            .and_then(Option::as_ref)
            .map(|p| p.name.clone())
            .unwrap_or_else(|| match self.slot_settings().unwrap().slots[slot].kind {
                SlotKind::Open => "Open".into(),
                SlotKind::Computer => "Computer".into(),
                SlotKind::Closed => "Closed".into(),
            })
    }
    pub fn slot_ready(&self, slot: usize) -> bool {
        self.lobby
            .as_ref()
            .and_then(|(_, v)| v.players.get(slot))
            .and_then(Option::as_ref)
            .map_or(
                self.slot_settings().unwrap().slots[slot].kind == SlotKind::Computer,
                |p| p.ready,
            )
    }
    pub fn slot_color(&self, slot: usize) -> Option<&straterust_engine::multiplayer::PlayerColor> {
        let catalog = self.match_catalog.as_ref()?;
        let colors = catalog.slot_colors(self.slot_settings()?).ok()?;
        catalog.colors.get(colors.get(slot).copied().flatten()?)
    }
    pub(super) fn slot_map_choices(&self) -> Vec<Choice> {
        let mut out = Vec::new();
        let Some(settings) = self.slot_settings() else {
            return out;
        };
        let catalog = self.match_catalog.as_ref().unwrap();
        let running = self.lobby.as_ref().is_some_and(|(_, v)| v.running);
        let local = self.lobby.as_ref().map(|(s, _)| *s);
        let host = self
            .lobby
            .as_ref()
            .is_none_or(|(s, v)| *s == v.host && !v.running);
        for marker in self.slot_markers() {
            let name = self.slot_name(marker.slot);
            let short: String = name.chars().take(8).collect();
            add(
                &mut out,
                format!("{}. {short}", marker.slot + 1),
                marker.rect,
                Pick::SelectSlot(marker.slot),
            );
        }
        if let Some(slot) = self.selected_slot.filter(|i| *i < settings.slots.len()) {
            let own = local == Some(slot);
            let occupied = self
                .lobby
                .as_ref()
                .is_some_and(|(_, v)| v.players[slot].is_some());
            if let Some(field) = self.slot_field {
                let values: Vec<(usize, String)> = match field {
                    0 => [
                        (0, "Open".into()),
                        (1, "Computer".into()),
                        (2, "Closed".into()),
                    ]
                    .into_iter()
                    .chain(self.lobby.as_ref().into_iter().flat_map(|(_, v)| {
                        v.players
                            .iter()
                            .enumerate()
                            .filter_map(|(i, p)| p.as_ref().map(|p| (i + 3, p.name.clone())))
                    }))
                    .collect(),
                    1 => std::iter::once((
                        0,
                        if catalog.random_faction {
                            "Random"
                        } else {
                            "Map default"
                        }
                        .into(),
                    ))
                    .chain(
                        catalog
                            .factions
                            .iter()
                            .enumerate()
                            .map(|(i, f)| (i + 1, f.name.clone())),
                    )
                    .collect(),
                    2 => (0..8).map(|i| (i, format!("Team {}", i + 1))).collect(),
                    _ => catalog
                        .colors
                        .iter()
                        .enumerate()
                        .map(|(i, color)| (i, color.name.clone()))
                        .collect(),
                };
                for (i, (value, label)) in values.into_iter().enumerate() {
                    add(
                        &mut out,
                        label,
                        [424, 110 + i as u16 * 18, 192, 17],
                        if field == 3
                            && catalog.slot_colors(settings).is_ok_and(|colors| {
                                colors
                                    .iter()
                                    .enumerate()
                                    .any(|(i, c)| i != slot && *c == Some(value))
                            })
                        {
                            Pick::Action(MenuAction::Unavailable(
                                "This color is already in use.".into(),
                            ))
                        } else if field == 0 && value < 3 && occupied {
                            Pick::Action(MenuAction::Unavailable(
                                "Move this player to another position first.".into(),
                            ))
                        } else {
                            Pick::SetSlot(slot, field, value)
                        },
                    );
                }
                add(
                    &mut out,
                    "Back to position".into(),
                    [424, 320, 192, 20],
                    Pick::SlotField(4),
                );
            } else {
                let unavailable = || {
                    Pick::Action(MenuAction::Unavailable(
                        "Only the host can change this position.".into(),
                    ))
                };
                add(
                    &mut out,
                    format!("Player: {}", self.slot_name(slot)),
                    [424, 124, 192, 28],
                    if host {
                        Pick::SlotField(0)
                    } else {
                        unavailable()
                    },
                );
                let race = settings.slots[slot].faction.map_or(
                    if catalog.random_faction {
                        "Random"
                    } else {
                        "Map default"
                    },
                    |f| catalog.factions[f].name.as_str(),
                );
                add(
                    &mut out,
                    format!("Race: {race}"),
                    [424, 158, 192, 28],
                    if !running && (host || own) {
                        Pick::SlotField(1)
                    } else {
                        unavailable()
                    },
                );
                if catalog.modes[settings.mode] == GameType::Teams {
                    add(
                        &mut out,
                        format!("Team {}", settings.slots[slot].team + 1),
                        [424, 192, 192, 28],
                        if !running && (host || own) {
                            Pick::SlotField(2)
                        } else {
                            unavailable()
                        },
                    );
                }
                if !occupied
                    && settings.slots[slot].kind != SlotKind::Closed
                    && self.lobby.is_some()
                {
                    add(
                        &mut out,
                        "Join this position".into(),
                        [424, 252, 192, 26],
                        Pick::TakeSlot(slot),
                    );
                }
                if let Some(color) = self.slot_color(slot) {
                    add(
                        &mut out,
                        format!("Color: {}", color.name),
                        [424, 220, 192, 26],
                        if !running && (host || own) {
                            Pick::SlotField(3)
                        } else {
                            unavailable()
                        },
                    );
                }
                if host {
                    add(
                        &mut out,
                        "Match settings".into(),
                        [424, 286, 192, 24],
                        Pick::MatchOptions,
                    );
                    add(
                        &mut out,
                        "Change map".into(),
                        [424, 316, 192, 24],
                        Pick::ChooseLanMap,
                    );
                }
            }
        } else if host {
            add(
                &mut out,
                format!(
                    "Format: {}",
                    if catalog.modes[settings.mode] == GameType::Teams {
                        "Teams"
                    } else {
                        "Free for all"
                    }
                ),
                [424, 124, 192, 28],
                Pick::MatchOption(0),
            );
            add(
                &mut out,
                "Match settings".into(),
                [424, 164, 192, 28],
                Pick::MatchOptions,
            );
            add(
                &mut out,
                "Change map".into(),
                [424, 204, 192, 28],
                Pick::ChooseLanMap,
            );
        }
        if let Some(local) = local.filter(|s| *s < settings.slots.len()) {
            if !running {
                add(
                    &mut out,
                    if self.slot_ready(local) {
                        "Ready"
                    } else {
                        "Not ready"
                    }
                    .into(),
                    [24, 438, 180, 28],
                    Pick::Ready,
                );
            }
            if host {
                add(
                    &mut out,
                    "Start match".into(),
                    [218, 438, 180, 28],
                    Pick::StartMatch,
                );
            }
        }
        add(
            &mut out,
            if self.page == Page::Lobby {
                "Leave (Esc)"
            } else {
                "Back (Esc)"
            }
            .into(),
            [424, 438, 192, 28],
            Pick::Back,
        );
        out
    }
}
