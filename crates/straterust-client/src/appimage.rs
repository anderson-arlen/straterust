//! Export a selected native game and the running client, never a developer checkout.
use std::{
    fs::{self, File},
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use straterust_engine::{
    content::{Campaign, Manifest, read_ron},
    game::{GameInfo, ImportBuild},
    menus::MenuManifest,
    multiplayer::Catalog,
};

use crate::menus::catalog::GameEntry;

use straterust_engine::asset_archive as content;
mod tools;

#[derive(Serialize, Deserialize)]
struct Bundle {
    schema_version: u32,
    game_id: String,
    title: String,
    engine_version: String,
    #[serde(default)]
    game_version: String,
    #[serde(default)]
    package_version: String,
    #[serde(default)]
    importer: Option<ImportBuild>,
    architecture: String,
}

/// Stable across relocation, engine updates and content fixes. It is a profile
/// name, not the strict multiplayer/replay compatibility identity.
pub fn game_id(game: &GameEntry) -> Result<String> {
    let root = &game.directory;
    let identity = if root.join("multiplayer.ron").is_file() {
        Catalog::load(root)?.game
    } else if root.join("campaign.ron").is_file() {
        Campaign::load(root)?.id
    } else if root.join("manifest.ron").is_file() {
        read_ron::<Manifest>(&root.join("manifest.ron"))?.id
    } else {
        // Menu-only games can aggregate multiple campaigns without a root ID.
        read_ron::<MenuManifest>(&root.join("menus.ron"))?.title
    };
    Ok(format!(
        "straterust-{}",
        &blake3::hash(identity.as_bytes()).to_hex()[..16]
    ))
}

pub fn settings_path(game: &GameEntry) -> Result<PathBuf> {
    Ok(straterust_importers::data_directory()?
        .join("profiles")
        .join(game_id(game)?)
        .join("client-settings.ron"))
}

pub fn filename(game: &GameEntry) -> String {
    let name: String = game
        .title
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    format!(
        "{}-{}-{}.AppImage",
        name.trim_matches('-'),
        package_version(game),
        std::env::consts::ARCH
    )
}

fn package_version(game: &GameEntry) -> String {
    game.info.as_ref().map_or_else(
        || env!("CARGO_PKG_VERSION").into(),
        GameInfo::package_version,
    )
}

pub fn export(game: &GameEntry, output: &Path, progress: &dyn Fn(&str)) -> Result<()> {
    // Reject an invalid destination before downloading tools or copying data.
    let output = checked_output(game, output)?;
    let tools = tools::prepare(progress)?;
    export_with(game, &output, &std::env::current_exe()?, &tools, progress)
}

fn export_with(
    game: &GameEntry,
    output: &Path,
    executable: &Path,
    tools: &tools::Toolchain,
    progress: &dyn Fn(&str),
) -> Result<()> {
    let output = checked_output(game, output)?;
    // Check the tool before copying a potentially large game installation.
    let status = Command::new(&tools.deploy)
        .arg("--version")
        .env("APPIMAGE_EXTRACT_AND_RUN", "1")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .context("start AppImage build tool")?;
    ensure!(status.success(), "linuxdeploy could not start");
    let staging = tempfile::Builder::new()
        .prefix(".straterust-appimage-")
        .tempdir_in(output.parent().unwrap())?;
    let appdir = staging.path().join("Game.AppDir");
    let game_dir = appdir.join("usr/share/straterust/game");
    let bin = appdir.join("usr/bin");
    fs::create_dir_all(&bin)?;
    content::copy_game(&game.directory, &game_dir, progress)?;
    let copied = GameEntry::read(&game_dir)?;
    let id = game_id(&copied)?;
    let bundle = Bundle {
        schema_version: 1,
        game_id: id.clone(),
        title: copied.title.clone(),
        engine_version: env!("CARGO_PKG_VERSION").into(),
        game_version: copied.info.as_ref().map_or_else(
            || env!("CARGO_PKG_VERSION").into(),
            |info| info.version.clone(),
        ),
        package_version: package_version(&copied),
        importer: copied.info.as_ref().and_then(|info| info.importer.clone()),
        architecture: std::env::consts::ARCH.into(),
    };
    fs::write(
        appdir.join("usr/share/straterust/bundle.ron"),
        ron::ser::to_string(&bundle)?,
    )?;
    fs::write(
        appdir.join("LICENSE.straterust"),
        include_str!("../../../LICENSE"),
    )?;
    fs::copy(executable, bin.join("straterust-client"))?;
    fs::write(bin.join(&id), LAUNCHER)?;
    executable_mode(&bin.join(&id))?;
    fs::write(appdir.join("AppRun"), APPRUN)?;
    executable_mode(&appdir.join("AppRun"))?;
    let desktop = appdir.join(format!("{id}.desktop"));
    fs::write(
        &desktop,
        format!(
            "[Desktop Entry]\nType=Application\nName={}\nExec={id}\nIcon={id}\nCategories=Game;StrategyGame;\nTerminal=false\nX-StrateRust-Game={id}\nX-AppImage-Version={}\n",
            desktop_text(&copied.title),
            bundle.package_version
        ),
    )?;
    let icon = if let Some(image) = &copied.icon {
        let icon = appdir.join(format!("{id}.png"));
        let mut encoder = png::Encoder::new(File::create(&icon)?, image.width, image.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.write_header()?.write_image_data(&image.rgba)?;
        icon
    } else {
        let icon = appdir.join(format!("{id}.svg"));
        fs::write(&icon, ICON)?;
        icon
    };
    let image = staging.path().join("game.AppImage");
    let log_path = staging.path().join("linuxdeploy.log");
    let log = File::create(&log_path)?;
    progress("Bundling libraries and compressing the AppImage...");
    let mut command = Command::new(&tools.deploy);
    command
        .arg("--appdir")
        .arg(&appdir)
        .arg("--executable")
        .arg(bin.join("straterust-client"))
        .arg("--desktop-file")
        .arg(&desktop)
        .arg("--icon-file")
        .arg(&icon)
        .args(["--output", "appimage"])
        .env("APPIMAGE_EXTRACT_AND_RUN", "1")
        .env("LDAI_OUTPUT", &image)
        .env("LDAI_NO_APPSTREAM", "1")
        .env("LINUXDEPLOY_OUTPUT_VERSION", &bundle.package_version)
        .env("NO_STRIP", "1")
        .current_dir(staging.path())
        .stdout(log.try_clone()?)
        .stderr(log);
    if let Some(runtime) = &tools.runtime {
        command.env("LDAI_RUNTIME_FILE", runtime);
    }
    // Winit and the audio backend also dlopen libraries that ldd cannot see.
    // Drivers (Vulkan/GL, DRI and their ICD configuration) remain host supplied.
    for library in dynamic_libraries()? {
        command.arg("--library").arg(library);
    }
    let mut child = command.spawn().context("run linuxdeploy")?;
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().context("wait for linuxdeploy")? {
            break status;
        }
        progress(&format!(
            "Building AppImage: bundling and compression ({} seconds)...",
            started.elapsed().as_secs()
        ));
        std::thread::sleep(Duration::from_secs(1));
    };
    ensure!(
        status.success(),
        "linuxdeploy failed: {}",
        log_tail(&log_path)?
    );
    let mut file = File::open(&image).context("linuxdeploy produced no AppImage")?;
    let mut header = [0; 12];
    file.read_exact(&mut header)?;
    ensure!(
        &header[..4] == b"\x7fELF" && &header[8..11] == b"AI\x02",
        "linuxdeploy produced an invalid type-2 AppImage"
    );
    executable_mode(&image)?;
    file.sync_all()?;
    fs::rename(image, &output).context("publish AppImage; previous export was preserved")?;
    progress("AppImage created. Removing temporary files...");
    drop(staging);
    progress("AppImage export complete.");
    Ok(())
}

fn resolve_tool(path: &Path) -> Result<PathBuf> {
    let found = if path.components().count() > 1 {
        path.canonicalize().ok()
    } else {
        std::env::var_os("PATH").and_then(|paths| {
            std::env::split_paths(&paths)
                .map(|directory| directory.join(path))
                .find(|candidate| candidate.is_file())
                .and_then(|candidate| candidate.canonicalize().ok())
        })
    };
    found.with_context(|| format!("AppImage tool does not exist: {}", path.display()))
}

fn checked_output(game: &GameEntry, output: &Path) -> Result<PathBuf> {
    ensure!(cfg!(target_os = "linux"), "AppImage export requires Linux");
    let output = absolute_output(output)?;
    ensure!(
        !output.starts_with(game.directory.canonicalize()?),
        "save the AppImage outside the game directory"
    );
    Ok(output)
}

fn absolute_output(output: &Path) -> Result<PathBuf> {
    let output = if output.is_absolute() {
        output.to_path_buf()
    } else {
        std::env::current_dir()?.join(output)
    };
    let name = output.file_name().context("choose an AppImage filename")?;
    ensure!(
        output.extension().is_some_and(|e| e == "AppImage"),
        "output filename must end in .AppImage"
    );
    Ok(output
        .parent()
        .context("missing output directory")?
        .canonicalize()
        .context("output directory must exist")?
        .join(name))
}

fn dynamic_libraries() -> Result<Vec<PathBuf>> {
    let output = Command::new("ldconfig")
        .arg("-p")
        .output()
        .context("ldconfig is required to locate desktop libraries for export")?;
    ensure!(output.status.success(), "cannot read shared library cache");
    let cache = String::from_utf8(output.stdout)?;
    let mut libraries = Vec::new();
    for name in [
        "libX11.so.6",
        "libX11-xcb.so.1",
        "libxcb.so.1",
        "libXcursor.so.1",
        "libXi.so.6",
        "libXrandr.so.2",
        "libxkbcommon.so.0",
        "libxkbcommon-x11.so.0",
        "libwayland-client.so.0",
        "libwayland-cursor.so.0",
        "libwayland-egl.so.1",
        "libasound.so.2",
    ] {
        if let Some(path) = cache.lines().find_map(|line| {
            let (description, path) = line.split_once(" => ")?;
            (description.split_whitespace().next()? == name).then(|| PathBuf::from(path))
        }) {
            libraries.push(path);
        }
    }
    ensure!(
        libraries
            .iter()
            .any(|p| p.file_name().is_some_and(|n| n == "libxkbcommon.so.0")),
        "libxkbcommon is required for the bundled desktop runtime"
    );
    Ok(libraries)
}

fn log_tail(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let start = file.metadata()?.len().saturating_sub(4096);
    file.seek(SeekFrom::Start(start))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn desktop_text(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

#[cfg(unix)]
fn executable_mode(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))?;
    Ok(())
}
#[cfg(not(unix))]
fn executable_mode(_: &Path) -> Result<()> {
    Ok(())
}

// No game title or selected path is interpolated into shell code. The fixed
// AppDir layout works both mounted as an AppImage and installed by a packager.
const LAUNCHER: &str = "#!/bin/sh\nset -eu\nself=$(readlink -f -- \"$0\")\nroot=$(CDPATH= cd -- \"$(dirname -- \"$self\")/../..\" && pwd)\nexport LD_LIBRARY_PATH=\"$root/usr/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}\"\nexec \"$root/usr/bin/straterust-client\" --game \"$root/usr/share/straterust/game\" \"$@\"\n";
const APPRUN: &str = "#!/bin/sh\nset -eu\nself=$(readlink -f -- \"$0\")\nroot=$(CDPATH= cd -- \"$(dirname -- \"$self\")\" && pwd)\nexport LD_LIBRARY_PATH=\"$root/usr/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}\"\nexec \"$root/usr/bin/straterust-client\" --game \"$root/usr/share/straterust/game\" \"$@\"\n";
const ICON: &str = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"256\" height=\"256\" viewBox=\"0 0 256 256\"><rect width=\"256\" height=\"256\" rx=\"32\" fill=\"#182632\"/><path d=\"M48 64h160v32H80v32h128v64H48v-32h128v-16H48z\" fill=\"#f0d277\"/></svg>";

#[cfg(test)]
mod tests;
