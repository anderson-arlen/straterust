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
        install_with(self, root, |output| {
            convert_compact(output, progress, |native| match self {
                Self::StarCraft => {
                    straterust_import_starcraft::import_game(source, native, progress)
                }
                Self::Warcraft2 => {
                    straterust_import_warcraft2::import_game(source, native, progress)
                }
            })
        })
    }

    /// Upgrade old managed installations atomically without reconverting maps.
    pub fn compact_installation(self, root: &Path, progress: &dyn Fn(&str)) -> Result<PathBuf> {
        self.compact_installation_with_source(root, None, progress)
    }

    pub fn compact_installation_with_source(
        self,
        root: &Path,
        source: Option<&Path>,
        progress: &dyn Fn(&str),
    ) -> Result<PathBuf> {
        let target = root.join(self.name());
        ensure!(managed_directory(&target, self)?, "game is not installed");
        install_with(self, root, |output| {
            straterust_engine::asset_archive::copy_game_with_update(
                &target,
                output,
                progress,
                |converted| {
                    if self == Self::Warcraft2 {
                        straterust_import_warcraft2::upgrade_installed_maps(
                            converted, source, progress,
                        )?;
                    }
                    let mut info = straterust_engine::game::GameInfo::load(converted)?
                        .context("compacted game metadata missing")?;
                    if let Some(build) = &mut info.importer {
                        ensure!(
                            build.id == self.id(),
                            "importer metadata does not match installation"
                        );
                        build.build = build.build.max(match self {
                            Self::StarCraft => straterust_import_starcraft::IMPORTER_BUILD,
                            Self::Warcraft2 => straterust_import_warcraft2::IMPORTER_BUILD,
                        });
                        fs::write(converted.join("game.ron"), ron::ser::to_string(&info)?)?;
                    }
                    Ok(())
                },
            )?;
            Ok(())
        })
    }

    pub fn update_metadata(
        self,
        source: &Path,
        root: &Path,
        progress: &dyn Fn(&str),
    ) -> Result<PathBuf> {
        let target = root.join(self.name());
        ensure!(managed_directory(&target, self)?, "game is not installed");
        let lock = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(root.join(format!(".{}.import-lock", self.id())))?;
        fs2::FileExt::try_lock_exclusive(&lock)
            .context("another import of this game is running")?;
        match self {
            Self::StarCraft => {
                straterust_import_starcraft::update_game_metadata(source, &target, progress)?
            }
            Self::Warcraft2 => {
                straterust_import_warcraft2::update_game_metadata(source, &target, progress)?
            }
        }
        Ok(target)
    }
}

fn convert_compact(
    output: &Path,
    progress: &dyn Fn(&str),
    convert: impl FnOnce(&Path) -> Result<()>,
) -> Result<()> {
    let staging = tempfile::Builder::new()
        .prefix(".convert-")
        .tempdir_in(output.parent().context("missing import parent")?)?;
    let native = staging.path().join("game");
    convert(&native)?;
    progress("Storing shared artwork and audio for the installed game");
    straterust_engine::asset_archive::copy_game(&native, output, progress)
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

/// Load native campaigns with importer-owned compatibility fixes. This also
/// applies fixes when continuing a save against an older read-only import.
pub fn load_package(directory: &Path) -> Result<straterust_engine::content::Package> {
    straterust_engine::content::Package::load_with_map_update(
        directory,
        straterust_import_warcraft2::upgrade_campaign_map,
    )
}

/// Upgrade converter-owned native multiplayer metadata in memory, including older installs
/// and read-only AppImages. Never rewrites maps or existing saved games.
pub fn multiplayer_catalog(root: &Path) -> Result<straterust_engine::multiplayer::Catalog> {
    let mut catalog = straterust_engine::multiplayer::Catalog::load(root)?;
    if catalog.game == "warcraft2" {
        straterust_import_warcraft2::upgrade_multiplayer_ai(&mut catalog);
    }
    catalog.load_legacy_colors(root)?;
    catalog.validate()?;
    Ok(catalog)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn old_layout(root: &Path) -> Result<()> {
        use straterust_engine::{
            assets::{Image, ImageRef, encode_image},
            game::GameInfo,
            menus::MenuManifest,
        };
        fs::create_dir_all(root)?;
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/terran-demo");
        for entry in fs::read_dir(fixture)? {
            let entry = entry?;
            if entry.file_type()?.is_file() {
                fs::copy(entry.path(), root.join(entry.file_name()))?;
            }
        }
        let image = Image {
            width: 2,
            height: 3,
            rgba: vec![127; 24],
        };
        GameInfo::imported(root, "1.00".into(), "starcraft", 40, image.clone())?;
        let bytes = encode_image(&image)?;
        let digest = blake3::hash(&bytes).to_hex().to_string();
        let mut menu = MenuManifest::basic("Test", false);
        menu.background = Some(ImageRef {
            file: "front.srim".into(),
            blake3: digest.clone(),
        });
        fs::write(root.join("front.srim"), &bytes)?;
        fs::write(root.join("menus.ron"), ron::ser::to_string(&menu)?)?;
        for (folder, name) in [("one", "one.srim"), ("two", "different-name.srim")] {
            let directory = root.join(folder);
            fs::create_dir(&directory)?;
            fs::write(directory.join(name), &bytes)?;
            let reference = ImageRef {
                file: name.into(),
                blake3: digest.clone(),
            };
            let encoded = ron::ser::to_string(&reference)?;
            fs::write(
                directory.join("assets.ron"),
                format!(
                    "(schema_version:1,terrain:{encoded},unit_type:1,unit_name:\"Test\",frame_ms:100,anchor:(0,0),frames:[{encoded}])"
                ),
            )?;
        }
        Ok(())
    }

    #[test]
    fn new_imports_publish_shared_storage_and_existing_imports_upgrade_without_gameplay_changes() {
        use straterust_engine::{
            assets::AssetPack, content::Package, game::GameInfo, menus::MenuPack,
        };
        let root = tempfile::tempdir().unwrap();
        let importer = Importer::StarCraft;
        let fresh = install_with(importer, root.path(), |output| {
            convert_compact(output, &|_| {}, old_layout)
        })
        .unwrap();
        let info = GameInfo::load(&fresh).unwrap().unwrap();
        assert!(info.asset_archive.is_some());
        assert!(info.load_icon(&fresh).unwrap().is_some());
        assert!(!fresh.join("front.srim").exists());
        assert!(!fresh.join("one/one.srim").exists());
        assert!(!fresh.join("two/different-name.srim").exists());
        assert!(MenuPack::load(&fresh).unwrap().is_some());
        assert_eq!(
            AssetPack::load(&fresh.join("one")).unwrap().unwrap().frames,
            AssetPack::load(&fresh.join("two")).unwrap().unwrap().frames
        );

        let legacy = install_with(importer, root.path(), old_layout).unwrap();
        let before = Package::load(&legacy)
            .unwrap()
            .world(42)
            .unwrap()
            .state_hash();
        importer.compact_installation(root.path(), &|_| {}).unwrap();
        assert_eq!(
            before,
            Package::load(&legacy)
                .unwrap()
                .world(42)
                .unwrap()
                .state_hash()
        );
        assert!(!legacy.join("front.srim").exists());
        let info = GameInfo::load(&legacy).unwrap().unwrap();
        assert!(info.asset_archive.is_some());
        assert_eq!(
            info.importer.unwrap().build,
            straterust_import_starcraft::IMPORTER_BUILD
        );
        assert!(!root.path().join(".starcraft.previous").exists());

        let legacy = install_with(importer, root.path(), old_layout).unwrap();
        let mut menu: straterust_engine::menus::MenuManifest =
            ron::de::from_bytes(&fs::read(legacy.join("menus.ron")).unwrap()).unwrap();
        menu.background.as_mut().unwrap().blake3 = "0".repeat(64);
        fs::write(
            legacy.join("menus.ron"),
            ron::ser::to_string(&menu).unwrap(),
        )
        .unwrap();
        let old_menu = fs::read(legacy.join("menus.ron")).unwrap();
        let old_metadata = fs::read(legacy.join("game.ron")).unwrap();
        assert!(importer.compact_installation(root.path(), &|_| {}).is_err());
        assert_eq!(fs::read(legacy.join("game.ron")).unwrap(), old_metadata);
        assert_eq!(fs::read(legacy.join("menus.ron")).unwrap(), old_menu);
        assert!(legacy.join("front.srim").is_file());
        assert!(!root.path().join(".starcraft.previous").exists());
    }
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
