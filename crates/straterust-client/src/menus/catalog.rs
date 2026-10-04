use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use straterust_engine::{
    content::{Campaign, Manifest, read_ron},
    menus::{MenuManifest, MenuPack},
};

#[derive(Clone, Debug)]
pub struct GameEntry {
    pub directory: PathBuf,
    pub title: String,
    pub campaign: bool,
}

impl GameEntry {
    pub fn read(directory: &Path) -> Result<Self> {
        let campaign = directory.join("campaign.ron").is_file();
        let title = if directory.join("menus.ron").is_file() {
            let menu: MenuManifest = read_ron(&directory.join("menus.ron"))?;
            menu.validate()?;
            menu.title
        } else if campaign {
            Campaign::load(directory)?.id.replace(['.', '-', '_'], " ")
        } else {
            let manifest: Manifest = read_ron(&directory.join("manifest.ron"))?;
            anyhow::ensure!(manifest.schema_version == 1, "unsupported package schema");
            manifest.id.replace(['.', '-', '_'], " ")
        };
        Ok(Self {
            directory: directory.canonicalize()?,
            title,
            campaign,
        })
    }
    pub fn menus(&self) -> Result<MenuPack> {
        Ok(MenuPack::load(&self.directory)?
            .unwrap_or_else(|| MenuPack::plain(&self.title, self.campaign)))
    }
}

/// Only metadata is read. Campaign roots stop recursion, so their individual
/// missions do not masquerade as separate installed games. Never follow links.
pub fn discover(roots: &[PathBuf]) -> Vec<GameEntry> {
    let mut games = Vec::new();
    let mut seen = BTreeSet::new();
    let mut remaining = 512;
    for root in roots {
        scan(root, 0, &mut remaining, &mut seen, &mut games);
    }
    games.sort_by(|a, b| a.title.cmp(&b.title).then(a.directory.cmp(&b.directory)));
    games
}

fn scan(
    path: &Path,
    depth: usize,
    remaining: &mut usize,
    seen: &mut BTreeSet<PathBuf>,
    games: &mut Vec<GameEntry>,
) {
    if depth > 4
        || *remaining == 0
        || !fs::symlink_metadata(path).is_ok_and(|m| m.is_dir() && !m.file_type().is_symlink())
    {
        return;
    }
    *remaining -= 1;
    if path.join("campaign.ron").is_file()
        || path.join("manifest.ron").is_file()
        || path.join("menus.ron").is_file()
    {
        match GameEntry::read(path) {
            Ok(game) => {
                if seen.insert(game.directory.clone()) {
                    games.push(game);
                }
            }
            Err(error) => log::warn!("skip game package {}: {error:#}", path.display()),
        }
        return;
    }
    let Ok(entries) = fs::read_dir(path) else {
        return;
    };
    let mut children = entries
        .filter_map(Result::ok)
        .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| e.path())
        .collect::<Vec<_>>();
    children.sort();
    for child in children {
        scan(&child, depth + 1, remaining, seen, games);
    }
}

pub fn campaign_directory(game: &GameEntry, directory: &str) -> Result<PathBuf> {
    let root = game.directory.canonicalize()?;
    let path = root
        .join(directory)
        .canonicalize()
        .context("cannot find campaign")?;
    anyhow::ensure!(path.starts_with(&root), "campaign escapes its game package");
    Ok(path)
}
