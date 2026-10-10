//! Read archive payloads only. GOG installers are unpacked with innoextract;
//! disc images with 7-Zip. Neither the installer nor game EXE is executed.
use anyhow::{Context, Result, ensure};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};
use straterust_import_formats::Archive;

pub struct Source {
    pub main: super::war::WarArchive,
    pub interface: super::war::WarArchive,
    pub data: Archive,
    pub disc: Option<Archive>,
    _temporary: Option<tempfile::TempDir>,
}

pub fn case_path(root: &Path, name: &str) -> Result<PathBuf> {
    let mut path = root.to_owned();
    for component in name.split('/') {
        path = fs::read_dir(&path)?
            .take(65536)
            .filter_map(Result::ok)
            .find(|e| {
                e.file_name()
                    .to_string_lossy()
                    .eq_ignore_ascii_case(component)
            })
            .with_context(|| format!("missing {name} in {}", root.display()))?
            .path();
        ensure!(
            !fs::symlink_metadata(&path)?.file_type().is_symlink(),
            "source links are unsupported"
        );
    }
    Ok(path)
}

impl Source {
    pub fn open(path: &Path, progress: &dyn Fn(&str)) -> Result<Self> {
        let mut temporary = None;
        let root = if path.is_dir() {
            path.to_owned()
        } else if path
            .file_name()
            .is_some_and(|s| s.eq_ignore_ascii_case("INSTALL.EXE"))
            && path
                .parent()
                .is_some_and(|p| case_path(p, "Support/TOMES/TOME.1").is_ok())
        {
            path.parent().unwrap().to_owned()
        } else {
            ensure!(path.is_file(), "source file does not exist");
            let stage = tempfile::tempdir()?;
            progress("Unpacking source archives (the installer is not executed)");
            let iso = path
                .extension()
                .is_some_and(|s| s.eq_ignore_ascii_case("iso"));
            let output = if iso {
                Command::new("7z")
                    .args(["x", "-y", "-ssc-"])
                    .arg(format!("-o{}", stage.path().display()))
                    .arg(path)
                    .args([
                        "Support/TOMES/TOME.1",
                        "Support/TOMES/TOME.2",
                        "War2Dat.mpq",
                        "Install.mpq",
                        "INSTALL.EXE",
                    ])
                    .output()
                    .context("install 7-Zip to import Warcraft II disc images")?
            } else {
                Command::new("innoextract")
                    .args(["--extract", "--silent", "--output-dir"])
                    .arg(stage.path())
                    .args([
                        "--include",
                        "War2Dat.mpq",
                        "--include",
                        "Install.mpq",
                        "--include",
                        "Support/TOMES/TOME.1",
                        "--include",
                        "Support/TOMES/TOME.2",
                    ])
                    .arg(path)
                    .output()
                    .context("install innoextract to read GOG installers")?
            };
            ensure!(
                output.status.success(),
                "could not unpack source: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            let root = stage.path().to_owned();
            temporary = Some(stage);
            root
        };
        let data = match case_path(&root, "War2Dat.mpq") {
            Ok(path) => Archive::open(&path)?,
            Err(_) => {
                let installer = case_path(&root, "INSTALL.EXE")?;
                let mut archive = Archive::open(&installer)?;
                let bytes = archive.read_file("files\\War2Dat.mpq", 128 * 1024 * 1024)?;
                let stage = temporary.get_or_insert(tempfile::tempdir()?);
                let path = stage.path().join("War2Dat.mpq");
                fs::write(&path, bytes)?;
                Archive::open(&path)?
            }
        };
        let main = super::war::WarArchive::open(&case_path(&root, "Support/TOMES/TOME.1")?, 1000)?;
        let interface =
            super::war::WarArchive::open(&case_path(&root, "Support/TOMES/TOME.2")?, 3000)?;
        let disc = case_path(&root, "Install.mpq")
            .or_else(|_| case_path(&root, "INSTALL.EXE"))
            .ok()
            .map(|p| Archive::open(&p))
            .transpose()?;
        Ok(Self {
            main,
            interface,
            data,
            disc,
            _temporary: temporary,
        })
    }
}
