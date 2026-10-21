//! Preserve imported shared files in an independent, validated snapshot.
use crate::{
    asset_archive,
    content::{Campaign, Package, read_ron},
    game::GameInfo,
    menus::MenuManifest,
    multiplayer::Catalog,
};
use anyhow::{Context, Result, ensure};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

mod definitions;
mod validation;

#[cfg(unix)]
pub(super) type FileId = (u64, u64);
#[cfg(not(unix))]
pub(super) type FileId = PathBuf;

pub(super) fn file_id(path: &Path, metadata: &fs::Metadata) -> FileId {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let _ = path;
        (metadata.dev(), metadata.ino())
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        path.to_path_buf()
    }
}

struct Snapshot {
    directories: Vec<(PathBuf, Vec<std::ffi::OsString>)>,
    total: u64,
    last_progress: Instant,
}

impl Snapshot {
    fn scan(&mut self, root: &Path, relative: &Path, progress: &dyn Fn(&str)) -> Result<()> {
        let mut files = Vec::new();
        let mut directories = Vec::new();
        for entry in fs::read_dir(root.join(relative))? {
            let entry = entry?;
            let kind = entry.file_type()?;
            ensure!(
                kind.is_file() || kind.is_dir(),
                "game data must be regular files/directories, not links: {}",
                entry.path().display()
            );
            if kind.is_dir() {
                directories.push(entry.file_name());
            } else {
                files.push(entry.file_name());
                self.total += 1;
            }
            if self.last_progress.elapsed() >= Duration::from_millis(500) {
                progress(&format!("Counting source files: {}...", self.total));
                self.last_progress = Instant::now();
            }
        }
        files.sort();
        directories.sort();
        self.directories.push((relative.to_path_buf(), files));
        for name in directories {
            self.scan(root, &relative.join(name), progress)?;
        }
        Ok(())
    }
}

pub fn copy_game(source: &Path, target: &Path, progress: &dyn Fn(&str)) -> Result<()> {
    copy_game_with_update(source, target, progress, |_| Ok(()))
}

/// Apply a staged content migration before validating the final replacement.
/// The source is never mutated; callers publish only after this returns success.
pub fn copy_game_with_update(
    source: &Path,
    target: &Path,
    progress: &dyn Fn(&str),
    update: impl FnOnce(&Path) -> Result<()>,
) -> Result<()> {
    progress("Counting source files...");
    let mut snapshot = Snapshot {
        directories: Vec::new(),
        total: 0,
        last_progress: Instant::now(),
    };
    snapshot.scan(source, Path::new(""), progress)?;
    let mut info = GameInfo::load(source)?;
    let is_asset = |relative: &Path, name: &std::ffi::OsStr| {
        let icon = info.as_ref().and_then(|g| g.icon.as_ref());
        !(relative.as_os_str().is_empty()
            && icon.is_some_and(|i| name == std::ffi::OsStr::new(&i.file)))
            && Path::new(name)
                .extension()
                .is_some_and(|e| e == "srim" || e == "wav" || e == "srpcm" || e == "srdef")
    };
    let mut compact = false;
    for (relative, files) in &snapshot.directories {
        for name in files {
            if is_asset(relative, name)
                || (definitions::is_definition(name)
                    && !crate::content::already_shared(&source.join(relative).join(name))?)
            {
                compact = true;
                break;
            }
        }
        if compact {
            break;
        }
    }
    fs::create_dir_all(target)?;
    let archive_name = "shared-assets.sras";
    ensure!(
        !compact
            || !source.join(archive_name).exists()
            || info.as_ref().and_then(|g| g.asset_archive.as_deref()) == Some(archive_name),
        "reserved export asset archive filename already exists"
    );
    let mut archive = compact
        .then(|| asset_archive::Writer::create(&target.join(archive_name)))
        .transpose()?;
    if let Some(writer) = &mut archive {
        writer.extend(&asset_archive::Reader::open(source)?)?;
    }
    let mut packed = HashSet::new();
    let mut shared = HashMap::<FileId, PathBuf>::new();
    let mut processed = 0;
    let mut output_files = u64::from(compact);
    let mut last_progress = Instant::now();
    progress(&format!(
        "Processing source files: 0 / {}; output files: {output_files}",
        snapshot.total
    ));
    for (relative, files) in &snapshot.directories {
        let destination = target.join(relative);
        fs::create_dir_all(&destination)?;
        for name in files {
            let path = source.join(relative).join(name);
            let to = destination.join(name);
            let metadata = fs::symlink_metadata(&path)?;
            ensure!(
                metadata.is_file(),
                "game file changed during export: {}",
                path.display()
            );
            let id = file_id(&path, &metadata);
            if compact
                && relative.as_os_str().is_empty()
                && info
                    .as_ref()
                    .and_then(|g| g.asset_archive.as_deref())
                    .is_some_and(|a| name == a)
            {
                // Existing payloads were verified and copied into the new archive.
            } else if compact && definitions::is_definition(name) {
                fs::write(&to, definitions::pack(&path, archive.as_mut().unwrap())?)?;
                output_files += 1;
            } else if compact && is_asset(relative, name) {
                if packed.insert(id) {
                    archive.as_mut().unwrap().copy(&path)?;
                }
            } else if let Some(first) = shared.get(&id) {
                // Link only within staging; never link to the mutable source.
                fs::hard_link(first, &to)
                    .with_context(|| format!("share copied file {}", path.display()))?;
                output_files += 1;
            } else {
                fs::copy(&path, &to).with_context(|| format!("copy {}", path.display()))?;
                shared.insert(id, to);
                output_files += 1;
            }
            processed += 1;
            if last_progress.elapsed() >= Duration::from_millis(500) || processed == snapshot.total
            {
                progress(&format!(
                    "Processing source files: {processed} / {} ({}%); output files: {output_files}",
                    snapshot.total,
                    processed * 100 / snapshot.total.max(1)
                ));
                last_progress = Instant::now();
            }
        }
    }
    drop(shared);
    drop(packed);
    if let Some(archive) = archive {
        progress("Finishing shared game asset index...");
        archive.finish()?;
        if info.is_none() {
            output_files += 1;
        }
        let metadata = info.get_or_insert_with(|| GameInfo {
            schema_version: 1,
            version: env!("CARGO_PKG_VERSION").into(),
            importer: None,
            icon: None,
            asset_archive: None,
        });
        metadata.asset_archive = Some(archive_name.into());
        metadata.validate()?;
        fs::write(target.join("game.ron"), ron::ser::to_string(metadata)?)?;
    }
    progress(&format!("Game data ready: {output_files} output files"));
    update(target)?;
    let packages: Vec<_> = snapshot
        .directories
        .iter()
        .filter(|(_, files)| {
            files.iter().any(|f| {
                matches!(
                    f.to_str(),
                    Some(
                        "manifest.ron"
                            | "campaign.ron"
                            | "menus.ron"
                            | "multiplayer.ron"
                            | "assets.ron"
                            | "media.ron"
                            | "game.ron"
                    )
                )
            })
        })
        .map(|(relative, _)| relative)
        .collect();
    let mut validation = validation::Validation::default();
    for (index, relative) in packages.iter().enumerate() {
        let name = if relative.as_os_str().is_empty() {
            "game".into()
        } else {
            relative.to_string_lossy()
        };
        progress(&format!(
            "Validating package {} / {}: {name}",
            index + 1,
            packages.len()
        ));
        let path = target.join(relative);
        validate_directory(&path)
            .and_then(|_| validation.directory(&path))
            .with_context(|| format!("invalid game data: {}", source.join(relative).display()))?;
    }
    Ok(())
}

pub fn validate_directory(path: &Path) -> Result<()> {
    if path.join("manifest.ron").is_file() {
        Package::load(path)?;
    }
    if path.join("campaign.ron").is_file() {
        for mission in Campaign::load(path)?.missions {
            ensure!(
                path.join(mission.package).join("manifest.ron").is_file(),
                "campaign mission package is missing"
            );
        }
    }
    if path.join("menus.ron").is_file() {
        let menu: MenuManifest = read_ron(&path.join("menus.ron"))?;
        menu.validate()?;
        for campaign in menu.campaigns {
            let root = path.canonicalize()?;
            let directory = root.join(&campaign.directory).canonicalize()?;
            ensure!(
                directory.starts_with(&root),
                "campaign escapes its game package"
            );
            Campaign::load(&directory)?;
        }
    }
    if path.join("multiplayer.ron").is_file() {
        let catalog = Catalog::load(path)?;
        for map in catalog.maps {
            crate::multiplayer::package_path(path, &map.package)?;
        }
        for faction in catalog.factions {
            crate::multiplayer::package_path(path, &faction.presentation)?;
            for variant in faction.tileset_presentations {
                crate::multiplayer::package_path(path, &variant)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
