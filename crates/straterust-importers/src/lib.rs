//! Bundled converters and safe, fixed-name installation. Source executables
//! are archive inputs only; no imported executable game logic is run.
use anyhow::{Context, Result, ensure};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Importer {
    StarCraft,
    Warcraft2,
}

pub const IMPORTERS: [Importer; 2] = [Importer::StarCraft, Importer::Warcraft2];

impl Importer {
    pub fn name(self) -> &'static str {
        match self {
            Self::StarCraft => "StarCraft",
            Self::Warcraft2 => "Warcraft II",
        }
    }
    pub fn id(self) -> &'static str {
        match self {
            Self::StarCraft => "starcraft",
            Self::Warcraft2 => "warcraft2",
        }
    }
    pub fn description(self) -> &'static str {
        match self {
            Self::StarCraft => "English retail disc: ISO, INSTALL.EXE or its directory.",
            Self::Warcraft2 => "Battle.net Edition: GOG EXE, disc ISO or installed directory.",
        }
    }
    pub fn install(self, source: &Path, root: &Path, progress: &dyn Fn(&str)) -> Result<PathBuf> {
        install_with(self, root, |output| match self {
            Self::StarCraft => straterust_import_starcraft::import_game(source, output, progress),
            Self::Warcraft2 => straterust_import_warcraft2::import_game(source, output, progress),
        })
    }
}

/// Persistent game data, independent of the executable and working directory.
pub fn data_directory() -> Result<PathBuf> {
    #[cfg(target_os = "windows")]
    let path = PathBuf::from(std::env::var_os("LOCALAPPDATA").context("LOCALAPPDATA is not set")?);
    #[cfg(target_os = "macos")]
    let path = PathBuf::from(std::env::var_os("HOME").context("HOME is not set")?)
        .join("Library/Application Support");
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let path = match std::env::var_os("XDG_DATA_HOME").filter(|p| Path::new(p).is_absolute()) {
        Some(path) => PathBuf::from(path),
        None => {
            PathBuf::from(std::env::var_os("HOME").context("HOME is not set")?).join(".local/share")
        }
    };
    ensure!(
        path.is_absolute(),
        "application data directory must be absolute"
    );
    Ok(path.join("straterust"))
}

pub fn games_directory() -> Result<PathBuf> {
    Ok(data_directory()?.join("games"))
}

fn managed_directory(path: &Path, importer: Importer) -> Result<bool> {
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error).context("inspect installation"),
    };
    ensure!(
        meta.is_dir() && !meta.file_type().is_symlink(),
        "installation is not a regular directory: {}",
        path.display()
    );
    ensure!(
        fs::read_to_string(path.join(".straterust-importer"))
            .ok()
            .as_deref()
            == Some(importer.id()),
        "refusing to replace an unmanaged directory: {}",
        path.display()
    );
    Ok(true)
}

fn install_with(
    importer: Importer,
    root: &Path,
    convert: impl FnOnce(&Path) -> Result<()>,
) -> Result<PathBuf> {
    fs::create_dir_all(root).context("cannot create installed games directory")?;
    let root = root.canonicalize()?;
    let target = root.join(importer.name());
    let lock = root.join(format!(".{}.import-lock", importer.id()));
    // Keep the inode: unlinking a locked file could let another process lock
    // a replacement. The OS releases this lock even if conversion crashes.
    let lock = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock)?;
    fs2::FileExt::try_lock_exclusive(&lock).context("another import of this game is running")?;
    let previous = root.join(format!(".{}.previous", importer.id()));
    let had_previous = managed_directory(&previous, importer)?;
    if had_previous && !managed_directory(&target, importer)? {
        fs::rename(&previous, &target)?;
    }
    let replacing = managed_directory(&target, importer)?;
    let staging = tempfile::Builder::new()
        .prefix(".import-")
        .tempdir_in(&root)?;
    let converted = staging.path().join("game");
    convert(&converted)?;
    ensure!(
        converted.join("menus.ron").is_file(),
        "import produced no game menu"
    );
    fs::write(converted.join(".straterust-importer"), importer.id())?;
    // Keep the previous installation until the complete replacement is published.
    if previous.exists() {
        fs::remove_dir_all(&previous)?;
    }
    if replacing {
        fs::rename(&target, &previous)?;
    }
    if let Err(error) = fs::rename(&converted, &target) {
        if replacing {
            fs::rename(&previous, &target).context("restore previous installation")?;
        }
        return Err(error).context("publish imported game");
    }
    if replacing {
        fs::remove_dir_all(previous)?;
    }
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reimports_replace_one_installation_and_failures_preserve_it() {
        let root = tempfile::tempdir().unwrap();
        let convert = |value: &str, output: &Path| -> Result<()> {
            fs::create_dir(output)?;
            fs::write(output.join("menus.ron"), value)?;
            Ok(())
        };
        let path = install_with(Importer::StarCraft, root.path(), |p| convert("old", p)).unwrap();
        assert_eq!(path.file_name().unwrap(), "StarCraft");
        assert!(
            install_with(Importer::StarCraft, root.path(), |p| {
                convert("partial", p)?;
                anyhow::bail!("bad source")
            })
            .is_err()
        );
        assert_eq!(fs::read_to_string(path.join("menus.ron")).unwrap(), "old");
        assert_eq!(
            install_with(Importer::StarCraft, root.path(), |p| convert("new", p)).unwrap(),
            path
        );
        assert_eq!(fs::read_to_string(path.join("menus.ron")).unwrap(), "new");
        assert_eq!(
            fs::read_dir(root.path())
                .unwrap()
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().unwrap().is_dir())
                .count(),
            1
        );
    }
}
