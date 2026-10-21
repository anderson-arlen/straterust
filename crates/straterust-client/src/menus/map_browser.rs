//! Group native layouts while preserving each version's original match package.
use super::*;
use straterust_engine::multiplayer::{
    Catalog,
    browser::{Overview, Preview},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dropdown {
    Players,
    Size,
    Versions,
}

#[derive(Default)]
pub struct MapBrowser {
    pub indexed: bool,
    pub groups: Vec<Vec<usize>>,
    pub overviews: Vec<Option<Overview>>,
    pub selected: Option<usize>,
    pub dropdown: Option<Dropdown>,
    pub dropdown_offset: usize,
    pub preview: Option<Preview>,
    pub preview_map: Option<usize>,
    pub preview_error: String,
}

impl MapBrowser {
    pub fn index(&mut self, catalog: &Catalog, values: Vec<Result<Overview>>) {
        let mut layouts = std::collections::BTreeMap::<String, usize>::new();
        self.groups.clear();
        self.overviews = values.into_iter().map(|r| r.ok()).collect();
        self.overviews.resize_with(catalog.maps.len(), || None);
        for i in 0..catalog.maps.len() {
            if let Some(overview) = &self.overviews[i] {
                let next = self.groups.len();
                let group = *layouts.entry(overview.layout.clone()).or_insert(next);
                if group == next {
                    self.groups.push(Vec::new());
                }
                if self.groups[group].iter().any(|v| {
                    self.overviews[*v]
                        .as_ref()
                        .is_some_and(|v| v.settings == overview.settings)
                }) {
                    continue;
                }
                self.groups[group].push(i);
            } else {
                self.groups.push(vec![i]);
            }
        }
        // Preserve an already selected alias when identical copies collapse.
        if let Some(selected) = self.selected
            && let Some(value) = self.overviews.get(selected).and_then(Option::as_ref)
            && let Some(version) = self.groups.iter().flatten().find(|i| {
                self.overviews[**i]
                    .as_ref()
                    .is_some_and(|o| o.layout == value.layout && o.settings == value.settings)
            })
        {
            self.selected = Some(*version);
        }
        self.indexed = true;
    }
}

impl MenuUi {
    pub fn browse_map(&mut self, index: usize) {
        self.map_browser.selected = Some(index);
        self.map_browser.preview = None;
        self.map_browser.preview_map = None;
        self.map_browser.preview_error.clear();
        self.map_browser.dropdown = None;
        self.focus = None;
    }
    pub fn map_matches(&self, index: usize) -> bool {
        self.match_catalog
            .as_ref()
            .and_then(|c| c.maps.get(index))
            .is_some_and(|m| {
                self.map_players.is_none_or(|p| m.players == p)
                    && self.map_size.is_none_or(|s| m.size == s)
            })
    }
    pub fn map_versions(&self, index: usize) -> Vec<usize> {
        self.map_browser
            .groups
            .iter()
            .find(|g| g.contains(&index))
            .map_or_else(|| vec![index], Clone::clone)
            .into_iter()
            .filter(|i| self.map_matches(*i))
            .collect()
    }
    pub fn version_label(&self, index: usize) -> String {
        let versions = self
            .map_browser
            .groups
            .iter()
            .find(|g| g.contains(&index))
            .cloned()
            .unwrap_or_else(|| vec![index]);
        let base = versions
            .first()
            .and_then(|i| self.map_browser.overviews.get(*i))
            .and_then(Option::as_ref);
        let overview = self
            .map_browser
            .overviews
            .get(index)
            .and_then(Option::as_ref);
        let difference = match (base, overview) {
            (Some(a), Some(b)) => {
                let mut changes = b
                    .fields
                    .iter()
                    .filter(|(key, value)| {
                        a.fields.iter().any(|(k, v)| k == key && v != value)
                            || key == "Starting army" && a.army != b.army
                            || key == "Resource deposits" && a.resources != b.resources
                    })
                    .map(|(key, value)| {
                        if matches!(key.as_str(), "Players" | "Terrain") {
                            format!("{key}: {value}")
                        } else {
                            key.clone()
                        }
                    })
                    .collect::<Vec<_>>();
                if a.terrain != b.terrain {
                    changes.push("Terrain artwork".into());
                }
                changes.into_iter().take(2).collect::<Vec<_>>().join(", ")
            }
            _ => String::new(),
        };
        let position = versions.iter().position(|i| *i == index).unwrap_or(0) + 1;
        if position == 1 {
            let entry = &self.match_catalog.as_ref().unwrap().maps[index];
            let terrain = overview
                .and_then(|o| o.fields.iter().find(|(k, _)| k == "Terrain"))
                .map_or("Default", |(_, v)| v.as_str());
            format!("Version {position}: {terrain}, {} players", entry.players)
        } else if difference.is_empty() {
            format!("Version {position}: scenario settings")
        } else {
            format!("Version {position}: {difference}")
        }
    }
    pub fn map_filter_changed(&mut self) {
        self.offset = 0;
        self.focus = None;
        self.map_browser.dropdown = None;
        if self
            .map_browser
            .selected
            .is_none_or(|i| !self.map_matches(i))
        {
            if let Some(i) = self.map_indices().first().copied() {
                self.browse_map(i);
            } else {
                self.map_browser.selected = None;
                self.map_browser.preview = None;
            }
        }
    }
    pub fn toggle_map_dropdown(&mut self, dropdown: Dropdown) {
        self.map_browser.dropdown = if self.map_browser.dropdown == Some(dropdown) {
            None
        } else {
            Some(dropdown)
        };
        self.map_browser.dropdown_offset = 0;
        self.focus = None;
    }
    pub fn dropdown_pick(&self, pick: &Pick) -> bool {
        matches!(
            pick,
            Pick::MapFilter(_)
                | Pick::MapPlayers(_)
                | Pick::MapSize(_)
                | Pick::MapVersions
                | Pick::MapVersion(_)
                | Pick::DropdownPage(_)
        )
    }
    pub fn map_dropdown_choices(&self) -> Vec<Choice> {
        let Some(dropdown) = self.map_browser.dropdown else {
            return Vec::new();
        };
        let catalog = self.match_catalog.as_ref().unwrap();
        let (x, width, mut options) = match dropdown {
            Dropdown::Players => {
                let mut options = vec![("All players".into(), Pick::MapPlayers(None))];
                for p in catalog
                    .maps
                    .iter()
                    .map(|m| m.players)
                    .collect::<std::collections::BTreeSet<_>>()
                {
                    options.push((format!("{p} players"), Pick::MapPlayers(Some(p))));
                }
                (24, 282, options)
            }
            Dropdown::Size => {
                let mut options = vec![("All sizes".into(), Pick::MapSize(None))];
                for size in catalog
                    .maps
                    .iter()
                    .map(|m| m.size)
                    .collect::<std::collections::BTreeSet<_>>()
                {
                    options.push((
                        format!("{}x{}", size[0], size[1]),
                        Pick::MapSize(Some(size)),
                    ));
                }
                (330, 286, options)
            }
            Dropdown::Versions => {
                let options = self.map_browser.selected.map_or_else(Vec::new, |i| {
                    self.map_versions(i)
                        .into_iter()
                        .map(|v| (self.version_label(v), Pick::MapVersion(v)))
                        .collect()
                });
                (330, 286, options)
            }
        };
        let len = options.len();
        let offset = self.map_browser.dropdown_offset;
        let mut choices = Vec::new();
        let top = if dropdown == Dropdown::Versions {
            110
        } else {
            108
        };
        if offset > 0 {
            options.insert(
                offset,
                ("Previous options".into(), Pick::DropdownPage(false)),
            );
        }
        let count = len.saturating_sub(offset).min(12);
        let extra = usize::from(offset > 0);
        for (row, (label, pick)) in options
            .into_iter()
            .skip(offset)
            .take(count + extra)
            .enumerate()
        {
            push(
                &mut choices,
                label,
                [x, top + row as u16 * 22, width, 22],
                pick,
            );
        }
        if offset + count < len {
            push(
                &mut choices,
                "More options".into(),
                [x, top + (count + extra) as u16 * 22, width, 22],
                Pick::DropdownPage(true),
            );
        }
        choices
    }
    pub fn map_browser_choices(&self) -> Vec<Choice> {
        let catalog = self.match_catalog.as_ref().unwrap();
        let mut choices = Vec::new();
        push(
            &mut choices,
            format!(
                "Players: {} v",
                self.map_players.map_or("All".into(), |p| p.to_string())
            ),
            [24, 82, 282, 26],
            Pick::MapFilter(true),
        );
        push(
            &mut choices,
            format!(
                "Size: {} v",
                self.map_size
                    .map_or("All".into(), |[w, h]| format!("{w}x{h}"))
            ),
            [330, 82, 286, 26],
            Pick::MapFilter(false),
        );
        let maps = self.map_indices();
        for (row, i) in maps.iter().skip(self.offset).take(7).enumerate() {
            let versions = self.map_versions(*i).len();
            let suffix = if versions > 1 {
                format!(" ({versions} versions)")
            } else {
                String::new()
            };
            let limit = 32 - suffix.len();
            let primary = self
                .map_browser
                .groups
                .iter()
                .find(|g| g.contains(i))
                .and_then(|g| g.first())
                .copied()
                .unwrap_or(*i);
            let title = shortened(&catalog.maps[primary].title, limit);
            let versions = self.map_versions(*i);
            let min = versions
                .iter()
                .map(|v| catalog.maps[*v].players)
                .min()
                .unwrap();
            let max = versions
                .iter()
                .map(|v| catalog.maps[*v].players)
                .max()
                .unwrap();
            let players = if min == max {
                min.to_string()
            } else {
                format!("{min}-{max}")
            };
            let size = catalog.maps[*i].size;
            push(
                &mut choices,
                format!(
                    "{title}{suffix}\n{players} players | {}x{}",
                    size[0], size[1]
                ),
                [24, 120 + row as u16 * 34, 282, 28],
                Pick::LanMap(*i),
            );
        }
        if self.offset > 0 {
            push(
                &mut choices,
                "Previous".into(),
                [24, 366, 132, 26],
                Pick::Previous,
            );
        }
        if self.offset + 7 < maps.len() {
            push(&mut choices, "Next".into(), [174, 366, 132, 26], Pick::Next);
        }
        if let Some(index) = self.map_browser.selected {
            push(
                &mut choices,
                format!("{} v", self.version_label(index)),
                [330, 322, 286, 26],
                Pick::MapVersions,
            );
            push(
                &mut choices,
                "Use this map".into(),
                [330, 402, 286, 28],
                Pick::UseLanMap,
            );
            push(
                &mut choices,
                "Version settings".into(),
                [24, 402, 282, 28],
                if self
                    .map_browser
                    .overviews
                    .get(index)
                    .and_then(Option::as_ref)
                    .is_some()
                {
                    Pick::MapDetails
                } else {
                    Pick::Action(MenuAction::Unavailable(
                        "Map settings are still loading or could not be read.".into(),
                    ))
                },
            );
        }
        choices
    }
}
pub fn shortened(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        text.into()
    } else {
        format!(
            "{}...",
            text.chars()
                .take(limit.saturating_sub(3))
                .collect::<String>()
        )
    }
}
fn push(choices: &mut Vec<Choice>, label: String, rect: [u16; 4], pick: Pick) {
    let mut button = straterust_engine::menus::button(&label, rect[1], MenuAction::Resume);
    button.rect = rect;
    choices.push(Choice { button, pick });
}
