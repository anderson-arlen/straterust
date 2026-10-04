//! Optional package-authored menus. Navigation is bounded data, never simulation
//! scripting or executable code. The client supplies the actions and settings.
use std::{collections::BTreeMap, path::Path};

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

use crate::{
    assets::{
        Image, ImageRef, MAX_IMAGE_DIMENSION, decode_image, package_file, read_bounded,
        validate_reference,
    },
    content::read_ron,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MenuManifest {
    pub schema_version: u32,
    pub title: String,
    pub home: String,
    pub pause: String,
    pub screens: Vec<MenuScreen>,
    #[serde(default)]
    pub campaigns: Vec<MenuCampaign>,
    #[serde(default)]
    pub background: Option<ImageRef>,
    #[serde(default)]
    pub button_image: Option<ImageRef>,
    #[serde(default)]
    pub cursor: Option<MenuAnimation>,
    /// Pixel in each cursor frame placed at the mouse position, before UI scaling.
    #[serde(default)]
    pub cursor_anchor: [u16; 2],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MenuCampaign {
    pub title: String,
    /// A local campaign directory, or "." for this game package itself.
    pub directory: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MenuScreen {
    pub id: String,
    pub title: String,
    /// Heading position on the reference canvas; avoids covering authored art.
    #[serde(default = "heading_y")]
    pub title_y: u16,
    #[serde(default)]
    pub background: Option<ImageRef>,
    pub buttons: Vec<MenuButton>,
}

fn heading_y() -> u16 {
    44
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MenuButton {
    pub label: String,
    /// Rectangles use a 640x480 reference canvas, scaled equally on both axes.
    pub rect: [u16; 4],
    pub action: MenuAction,
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub idle: Option<MenuAnimation>,
    #[serde(default)]
    pub hover: Option<MenuAnimation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MenuAnimation {
    pub frame_ms: u32,
    pub frames: Vec<ImageRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MenuAction {
    Screen(String),
    Campaigns,
    Campaign(String),
    Play,
    Settings,
    Resume,
    Restart,
    Objectives,
    Help,
    EndMission,
    ChooseGame,
    Quit,
    Unavailable(String),
}

pub fn button(label: &str, y: u16, action: MenuAction) -> MenuButton {
    MenuButton {
        label: label.into(),
        rect: [176, y, 288, 30],
        action,
        key: None,
        idle: None,
        hover: None,
    }
}

impl MenuManifest {
    pub fn basic(title: &str, campaign: bool) -> Self {
        Self {
            schema_version: 1,
            title: title.into(),
            home: "home".into(),
            pause: "pause".into(),
            campaigns: if campaign {
                vec![MenuCampaign {
                    title: "Campaign".into(),
                    directory: ".".into(),
                }]
            } else {
                Vec::new()
            },
            background: None,
            button_image: None,
            cursor: None,
            cursor_anchor: [0, 0],
            screens: vec![
                MenuScreen {
                    id: "home".into(),
                    title: title.into(),
                    title_y: heading_y(),
                    background: None,
                    buttons: vec![
                        button(
                            if campaign { "Campaigns" } else { "Play" },
                            156,
                            if campaign {
                                MenuAction::Campaigns
                            } else {
                                MenuAction::Play
                            },
                        ),
                        button("Settings", 198, MenuAction::Settings),
                        button("Choose another game", 240, MenuAction::ChooseGame),
                        button("Quit", 282, MenuAction::Quit),
                    ],
                },
                MenuScreen {
                    id: "pause".into(),
                    title: "Game Menu".into(),
                    title_y: heading_y(),
                    background: None,
                    buttons: vec![
                        button("Return to Game (Esc)", 112, MenuAction::Resume),
                        button("Options", 150, MenuAction::Settings),
                        button("Mission Objectives", 188, MenuAction::Objectives),
                        button("Help", 226, MenuAction::Help),
                        button("Restart Mission", 264, MenuAction::Restart),
                        button("End Mission", 302, MenuAction::EndMission),
                        button("Quit", 340, MenuAction::Quit),
                    ],
                },
            ],
        }
    }

    pub fn screen(&self, id: &str) -> Option<&MenuScreen> {
        self.screens.iter().find(|screen| screen.id == id)
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(self.schema_version == 1, "unsupported menu schema");
        ensure!(
            self.cursor_anchor
                .iter()
                .all(|&coordinate| u32::from(coordinate) < MAX_IMAGE_DIMENSION)
                && (self.cursor.is_some() || self.cursor_anchor == [0, 0]),
            "invalid menu cursor anchor"
        );
        valid_text(&self.title)?;
        ensure!(
            (1..=32).contains(&self.screens.len()) && self.campaigns.len() <= 32,
            "too many menu screens/campaigns"
        );
        let mut ids = std::collections::BTreeSet::new();
        for screen in &self.screens {
            valid_text(&screen.id)?;
            valid_text(&screen.title)?;
            ensure!(
                screen.title_y <= 456,
                "menu heading outside reference canvas"
            );
            ensure!(ids.insert(&screen.id), "duplicate menu screen");
            ensure!(screen.buttons.len() <= 32, "too many menu buttons");
            for button in &screen.buttons {
                valid_text(&button.label)?;
                let [x, y, w, h] = button.rect.map(u32::from);
                ensure!(
                    w > 0 && h > 0 && x + w <= 640 && y + h <= 480,
                    "menu button outside reference canvas"
                );
                if let Some(key) = &button.key {
                    ensure!(
                        key.len() == 1 && key.bytes().all(|b| b.is_ascii_alphanumeric()),
                        "menu hotkeys must be one ASCII letter/digit"
                    );
                }
                match &button.action {
                    MenuAction::Screen(id) => {
                        ensure!(self.screen(id).is_some(), "menu references missing screen")
                    }
                    MenuAction::Campaign(dir) => ensure!(
                        self.campaigns.iter().any(|c| &c.directory == dir),
                        "menu references missing campaign"
                    ),
                    MenuAction::Unavailable(reason) => valid_text(reason)?,
                    _ => {}
                }
            }
        }
        ensure!(
            self.screen(&self.home).is_some() && self.screen(&self.pause).is_some(),
            "missing home/pause screen"
        );
        let mut dirs = std::collections::BTreeSet::new();
        for campaign in &self.campaigns {
            valid_text(&campaign.title)?;
            let dir = &campaign.directory;
            ensure!(
                dir == "."
                    || (!dir.is_empty()
                        && dir.len() <= 128
                        && dir != ".."
                        && dir
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))),
                "campaign must name a local directory"
            );
            ensure!(dirs.insert(dir), "duplicate menu campaign");
        }
        for animation in self.animations() {
            ensure!(
                (10..=10000).contains(&animation.frame_ms)
                    && (1..=128).contains(&animation.frames.len()),
                "invalid menu animation"
            );
        }
        let references = self.references();
        ensure!(references.len() <= 2048, "too many menu images");
        for reference in references {
            validate_reference(reference)?;
        }
        Ok(())
    }

    fn animations(&self) -> Vec<&MenuAnimation> {
        self.cursor
            .iter()
            .chain(
                self.screens
                    .iter()
                    .flat_map(|s| &s.buttons)
                    .flat_map(|b| b.idle.iter().chain(b.hover.iter())),
            )
            .collect()
    }
    fn references(&self) -> Vec<&ImageRef> {
        self.background
            .iter()
            .chain(self.button_image.iter())
            .chain(self.screens.iter().filter_map(|s| s.background.as_ref()))
            .chain(self.animations().into_iter().flat_map(|a| &a.frames))
            .collect()
    }
}

fn valid_text(text: &str) -> Result<()> {
    ensure!(
        !text.trim().is_empty()
            && text.len() <= 160
            && text.is_ascii()
            && !text.chars().any(char::is_control),
        "invalid menu text"
    );
    Ok(())
}

pub struct MenuPack {
    pub manifest: MenuManifest,
    pub images: BTreeMap<String, Image>,
}

impl MenuPack {
    pub fn plain(title: &str, campaign: bool) -> Self {
        Self {
            manifest: MenuManifest::basic(title, campaign),
            images: BTreeMap::new(),
        }
    }

    pub fn load(directory: &Path) -> Result<Option<Self>> {
        if !directory.join("menus.ron").try_exists()? {
            return Ok(None);
        }
        let manifest: MenuManifest = read_ron(&directory.join("menus.ron"))?;
        manifest.validate()?;
        let root = directory.canonicalize()?;
        let mut images = BTreeMap::new();
        let mut total = 0_usize;
        let mut hashes = BTreeMap::new();
        for reference in manifest.references() {
            if let Some(hash) = hashes.get(&reference.file) {
                ensure!(hash == &reference.blake3, "conflicting menu image hashes");
                continue;
            }
            hashes.insert(reference.file.clone(), reference.blake3.clone());
            let bytes = read_bounded(
                &package_file(&root, &reference.file)?,
                16 * 1024 * 1024 + 16,
            )?;
            ensure!(
                blake3::hash(&bytes).to_hex().as_str() == reference.blake3,
                "menu image hash mismatch: {}",
                reference.file
            );
            let image = decode_image(&bytes).context("invalid menu image")?;
            total += image.rgba.len();
            ensure!(total <= 96 * 1024 * 1024, "menu images exceed 96 MiB");
            images.insert(reference.file.clone(), image);
        }
        if let Some(cursor) = &manifest.cursor {
            for reference in &cursor.frames {
                let image = &images[&reference.file];
                ensure!(
                    u32::from(manifest.cursor_anchor[0]) < image.width
                        && u32::from(manifest.cursor_anchor[1]) < image.height,
                    "menu cursor anchor outside its image"
                );
            }
        }
        Ok(Some(Self { manifest, images }))
    }

    pub fn image(&self, reference: &ImageRef) -> Option<&Image> {
        self.images.get(&reference.file)
    }
    pub fn frame(&self, animation: &MenuAnimation, elapsed: u128) -> Option<&Image> {
        let index =
            (elapsed / u128::from(animation.frame_ms) % animation.frames.len() as u128) as usize;
        self.image(&animation.frames[index])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn authored_menus_validate_navigation_and_package_boundaries() {
        let mut menu = MenuManifest::basic("Example", true);
        menu.validate().unwrap();
        let legacy = ron::ser::to_string(&menu)
            .unwrap()
            .replace(",cursor_anchor:(0,0)", "");
        assert!(!legacy.contains("cursor_anchor"));
        let legacy: MenuManifest = ron::from_str(&legacy).unwrap();
        assert_eq!(legacy.cursor_anchor, [0, 0]);
        menu.cursor_anchor = [63, 63];
        assert!(menu.validate().is_err());
        menu.cursor_anchor = [0, 0];
        menu.screens[0].buttons[0].action = MenuAction::Screen("missing".into());
        assert!(menu.validate().is_err());
        menu.screens[0].buttons[0].action = MenuAction::Campaign(".".into());
        menu.validate().unwrap();
        menu.campaigns[0].directory = "../outside".into();
        assert!(menu.validate().is_err());
        menu.campaigns[0].directory = ".".into();
        menu.screens[0].buttons[0].rect = [600, 0, 200, 30];
        assert!(menu.validate().is_err());
    }
}
