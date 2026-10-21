use super::*;
use crate::menus::catalog::campaign_directory;
use straterust_engine::content::Package;

fn fixture() -> GameEntry {
    GameEntry::read(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/terran-demo"))
        .unwrap()
}

#[cfg(unix)]
fn script(path: &Path, body: &str) {
    fs::write(path, format!("#!/bin/sh\nset -eu\n{body}\n")).unwrap();
    executable_mode(path).unwrap();
}

#[test]
fn relocated_game_keeps_its_profile_and_copied_native_content() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("moved game");
    let original = fixture();
    content::copy_game(&original.directory, &root, &|_| {}).unwrap();
    let moved = GameEntry::read(&root).unwrap();
    assert_eq!(game_id(&original).unwrap(), game_id(&moved).unwrap());
    assert_eq!(
        settings_path(&original).unwrap(),
        settings_path(&moved).unwrap()
    );
    assert!(!settings_path(&moved).unwrap().starts_with(&root));
    assert_eq!(
        Package::load(&root).unwrap().world(0).unwrap().state_hash(),
        Package::load(&original.directory)
            .unwrap()
            .world(0)
            .unwrap()
            .state_hash()
    );
    fs::write(root.join("map.ron"), "broken map").unwrap();
    assert!(content::validate_directory(&root).is_err());
}

#[test]
#[cfg(target_os = "linux")]
fn successful_bundle_has_relocatable_launchers_and_preserves_arguments() {
    let temporary = tempfile::tempdir().unwrap();
    let source = temporary.path().join("source game");
    content::copy_game(&fixture().directory, &source, &|_| {}).unwrap();
    let original_icon = straterust_engine::assets::Image {
        width: 2,
        height: 2,
        rgba: vec![
            255, 0, 0, 0, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
        ],
    };
    GameInfo::imported(
        &source,
        "2.02b".into(),
        "warcraft2",
        1,
        original_icon.clone(),
    )
    .unwrap();
    let game = GameEntry::read(&source).unwrap();
    assert_eq!(
        filename(&game),
        "straterust-terran-loop-demo-2.02b+importer.1-x86_64.AppImage"
            .replace("x86_64", std::env::consts::ARCH)
    );
    let destination = temporary.path().join("space ' $() export.AppImage");
    let client = temporary.path().join("client");
    script(&client, "printf '%s\\n' \"$@\"");
    let deploy = temporary.path().join("linuxdeploy");
    script(
        &deploy,
        r#"
if [ "$1" = --version ]; then exit 0; fi
test "$LINUXDEPLOY_OUTPUT_VERSION" = '2.02b+importer.1'
appdir=
while [ $# -gt 0 ]; do
    if [ "$1" = --appdir ]; then shift; appdir=$1; fi
    shift
done
cp -R "$appdir" "$(dirname "$LDAI_OUTPUT")/../snapshot"
printf '\177ELF\002\001\001\000AI\002\000' > "$LDAI_OUTPUT"
"#,
    );
    let runtime = temporary.path().join("runtime");
    fs::write(&runtime, "cached runtime").unwrap();
    // The output plugin must use the cached runtime rather than downloading it.
    let body = fs::read_to_string(&deploy).unwrap();
    fs::write(
        &deploy,
        body.replace("appdir=", "test -f \"$LDAI_RUNTIME_FILE\"\nappdir="),
    )
    .unwrap();
    let tools = tools::Toolchain {
        deploy,
        runtime: Some(runtime),
    };
    export_with(&game, &destination, &client, &tools, &|_| {}).unwrap();
    assert!(destination.is_file());
    let snapshot = temporary.path().join("snapshot");
    let relocated = temporary.path().join("relocated ' $() bundle");
    fs::rename(snapshot, &relocated).unwrap();
    let copied = GameEntry::read(&relocated.join("usr/share/straterust/game")).unwrap();
    let bundle: Bundle = read_ron(&relocated.join("usr/share/straterust/bundle.ron")).unwrap();
    assert_eq!(bundle.game_id, game_id(&fixture()).unwrap());
    assert_eq!(bundle.package_version, "2.02b+importer.1");
    assert_eq!(bundle.game_version, "2.02b");
    assert_eq!(bundle.importer.unwrap().build, 1);
    let png = File::open(relocated.join(format!("{}.png", bundle.game_id))).unwrap();
    let mut reader = png::Decoder::new(std::io::BufReader::new(png))
        .read_info()
        .unwrap();
    let mut rgba = vec![0; reader.output_buffer_size().unwrap()];
    let image = reader.next_frame(&mut rgba).unwrap();
    assert_eq!(&rgba[..image.buffer_size()], original_icon.rgba);
    assert!(relocated.join("LICENSE.straterust").is_file());
    let installed = temporary.path().join("installed-launcher");
    std::os::unix::fs::symlink(relocated.join("usr/bin").join(&bundle.game_id), &installed)
        .unwrap();
    for launcher in [
        relocated.join("AppRun"),
        relocated.join("usr/bin").join(&bundle.game_id),
        installed,
    ] {
        let output = Command::new(&launcher)
            .args(["--config", "a path with spaces ' $()"])
            .current_dir("/")
            .output()
            .unwrap();
        assert!(output.status.success());
        assert_eq!(
            String::from_utf8(output.stdout)
                .unwrap()
                .lines()
                .collect::<Vec<_>>(),
            [
                "--game",
                copied.directory.to_str().unwrap(),
                "--config",
                "a path with spaces ' $()"
            ]
        );
    }
    let desktop =
        fs::read_to_string(relocated.join(format!("{}.desktop", bundle.game_id))).unwrap();
    assert!(desktop.contains(&format!("Exec={}", bundle.game_id)));
    assert!(desktop.contains("X-AppImage-Version=2.02b+importer.1"));
}

#[test]
#[cfg(target_os = "linux")]
fn failed_export_preserves_the_old_artifact_and_cleans_staging() {
    let temporary = tempfile::tempdir().unwrap();
    let destination = temporary.path().join("old.AppImage");
    fs::write(&destination, "old export").unwrap();
    let deploy = temporary.path().join("linuxdeploy");
    script(
        &deploy,
        "if [ \"$1\" = --version ]; then exit 0; fi\nprintf 'compression failed\\n' >&2\nexit 7",
    );
    let tools = tools::Toolchain {
        deploy,
        runtime: None,
    };
    let error = export_with(
        &fixture(),
        &destination,
        &std::env::current_exe().unwrap(),
        &tools,
        &|_| {},
    )
    .unwrap_err();
    assert!(error.to_string().contains("compression failed"));
    assert_eq!(fs::read_to_string(&destination).unwrap(), "old export");
    assert_eq!(fs::read_dir(temporary.path()).unwrap().count(), 2);
}

#[test]
#[cfg(unix)]
fn linked_content_is_rejected_without_reading_outside_the_game() {
    let temporary = tempfile::tempdir().unwrap();
    let game = temporary.path().join("source");
    fs::create_dir(&game).unwrap();
    std::os::unix::fs::symlink("/etc/passwd", game.join("escape")).unwrap();
    assert!(
        content::copy_game(&game, &temporary.path().join("target"), &|_| {})
            .unwrap_err()
            .to_string()
            .contains("not links")
    );
}

#[test]
#[cfg(target_os = "linux")]
#[ignore = "requires internet, a display and STRATERUST_APPIMAGE_CLIENT; downloads tools and builds a real public demo AppImage"]
fn real_appimage_runs_without_the_source_directory_or_fuse() {
    let temporary = tempfile::tempdir().unwrap();
    let output = temporary.path().join("demo.AppImage");
    let client = PathBuf::from(std::env::var_os("STRATERUST_APPIMAGE_CLIENT").unwrap());
    let cache = temporary.path().join("cache");
    let progress = |m: &str| eprintln!("{m}");
    let downloaded = tools::prepare_in(&cache, None, None, &progress, &tools::download).unwrap();
    export_with(&fixture(), &output, &client, &downloaded, &progress).unwrap();
    // A second export must not fetch either build tool or the output runtime.
    let cached = tools::prepare_in(&cache, None, None, &progress, &|_, _, _| {
        panic!("cached export attempted a download")
    })
    .unwrap();
    let offline = temporary.path().join("offline.AppImage");
    let offline_tools = temporary.path().join("offline tools");
    fs::create_dir(&offline_tools).unwrap();
    std::os::unix::fs::symlink(&cached.deploy, offline_tools.join("linuxdeploy.AppImage")).unwrap();
    let wrapper = offline_tools.join("deploy");
    script(
        &wrapper,
        r#"
export http_proxy=http://127.0.0.1:9
export https_proxy=http://127.0.0.1:9
export HTTP_PROXY=http://127.0.0.1:9
export HTTPS_PROXY=http://127.0.0.1:9
export ALL_PROXY=http://127.0.0.1:9
export no_proxy=
exec "$(dirname "$0")/linuxdeploy.AppImage" "$@"
"#,
    );
    let offline_toolchain = tools::Toolchain {
        deploy: wrapper,
        runtime: cached.runtime,
    };
    export_with(&fixture(), &offline, &client, &offline_toolchain, &progress).unwrap();
    assert_eq!(
        fs::read(&output).unwrap()[..12],
        fs::read(&offline).unwrap()[..12]
    );
    let result = Command::new(&output)
        .args(["--appimage-extract-and-run", "--help"])
        .current_dir("/")
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(String::from_utf8_lossy(&result.stdout).contains("--game DIR"));
    let extraction = temporary.path().join("extraction");
    fs::create_dir(&extraction).unwrap();
    assert!(
        Command::new(&output)
            .arg("--appimage-extract")
            .current_dir(&extraction)
            .output()
            .unwrap()
            .status
            .success()
    );
    let relocated = temporary.path().join("relocated game");
    fs::rename(extraction.join("squashfs-root"), &relocated).unwrap();
    let root = relocated.join("usr/share/straterust/game");
    assert_eq!(
        game_id(&GameEntry::read(&root).unwrap()).unwrap(),
        game_id(&fixture()).unwrap()
    );
    // Exercise the bundled binary, artwork and library search paths through an
    // actual window after relocating the extracted payload and changing cwd.
    let result = Command::new(relocated.join("usr/bin/straterust-client"))
        .arg("--package")
        .arg(&root)
        .arg("--scenario")
        .arg(root.join("smoke.ron"))
        .args(["--smoke-test", "--screenshot"])
        .arg(temporary.path().join("exported.ppm"))
        .env("LD_LIBRARY_PATH", relocated.join("usr/lib"))
        .env_remove("WAYLAND_DISPLAY")
        .current_dir("/")
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(
        fs::metadata(temporary.path().join("exported.ppm"))
            .unwrap()
            .len()
            > 1000
    );
}

#[test]
#[cfg(target_os = "linux")]
#[ignore = "requires retail AppImages in local/exports (or STRATERUST_REVIEW_APPIMAGE) and a native display; extracts beside each image"]
fn exported_retail_appimage_opens_campaigns_and_multiplayer() {
    let images = if let Some(path) = std::env::var_os("STRATERUST_REVIEW_APPIMAGE") {
        vec![PathBuf::from(path)]
    } else {
        ["Warcraft II", "StarCraft"]
            .into_iter()
            .map(|name| {
                let exports = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/exports");
                let versioned = straterust_importers::games_directory()
                    .ok()
                    .and_then(|root| GameEntry::read(&root.join(name)).ok())
                    .map(|game| exports.join(filename(&game)))
                    .filter(|path| path.is_file());
                versioned
                    .unwrap_or_else(|| exports.join(format!("{}.AppImage", name.replace(' ', "-"))))
            })
            .filter(|p| p.is_file())
            .collect::<Vec<_>>()
    };
    assert!(!images.is_empty(), "no exported game to review");
    for image in images {
        let image = image.canonicalize().unwrap();
        // Large imports have millions of directory entries. Extract on the image's
        // filesystem, since a small tmpfs can exhaust its metadata quota.
        let temporary = tempfile::Builder::new()
            .prefix(".straterust-review-")
            .tempdir_in(image.parent().unwrap())
            .unwrap();
        let extraction = Command::new(&image)
            .arg("--appimage-extract")
            .current_dir(temporary.path())
            .stdout(Stdio::null())
            .output()
            .unwrap();
        assert!(
            extraction.status.success(),
            "{}",
            String::from_utf8_lossy(&extraction.stderr)
        );
        let appdir = temporary.path().join("squashfs-root");
        let root = appdir.join("usr/share/straterust/game");
        let game = GameEntry::read(&root).unwrap();
        if game
            .info
            .as_ref()
            .is_some_and(|info| info.asset_archive.is_some())
        {
            let mut directories = vec![appdir.clone()];
            let mut entries = 0u64;
            let mut expanded = 0u64;
            while let Some(directory) = directories.pop() {
                for item in fs::read_dir(directory).unwrap() {
                    let item = item.unwrap();
                    entries += 1;
                    let kind = item.file_type().unwrap();
                    if kind.is_dir() {
                        directories.push(item.path());
                    } else if kind.is_file() {
                        expanded += item.metadata().unwrap().len();
                    }
                }
            }
            eprintln!(
                "{} export: {entries} entries, {expanded} expanded bytes",
                game.title
            );
            assert!(entries < 100000, "export exceeds PacSmith's entry limit");
            assert!(
                expanded <= 32 * 1024 * 1024 * 1024,
                "export exceeds PacSmith's expanded-byte limit"
            );
        }
        let menu = game.menus().unwrap();
        let catalog = Catalog::load(&root).unwrap();
        let bundle: Bundle = read_ron(&appdir.join("usr/share/straterust/bundle.ron")).unwrap();
        assert_eq!(bundle.game_id, game_id(&game).unwrap());
        if let Some(info) = &game.info {
            assert_eq!(bundle.game_version, info.version);
            assert_eq!(bundle.package_version, info.package_version());
            let icon = game
                .icon
                .as_ref()
                .expect("missing imported application icon");
            let icon_name = format!("{}.png", bundle.game_id);
            let icon_path = appdir.join(&icon_name);
            let mut reader =
                png::Decoder::new(std::io::BufReader::new(File::open(&icon_path).unwrap()))
                    .read_info()
                    .unwrap();
            let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
            let frame = reader.next_frame(&mut pixels).unwrap();
            assert_eq!([frame.width, frame.height], [icon.width, icon.height]);
            assert_eq!(&pixels[..frame.buffer_size()], icon.rgba);
            assert_eq!(
                fs::read(appdir.join(".DirIcon")).unwrap(),
                fs::read(&icon_path).unwrap()
            );
            let hicolor = appdir.join(format!(
                "usr/share/icons/hicolor/{}x{}/apps/{icon_name}",
                icon.width, icon.height
            ));
            assert_eq!(fs::read(hicolor).unwrap(), fs::read(&icon_path).unwrap());
            let desktop =
                fs::read_to_string(appdir.join(format!("{}.desktop", bundle.game_id))).unwrap();
            assert!(desktop.contains(&format!("Icon={}\n", bundle.game_id)));
            assert!(desktop.contains(&format!("X-AppImage-Version={}\n", info.package_version())));
        }
        assert!(!catalog.maps.is_empty());
        assert!(!menu.manifest.campaigns.is_empty());
        eprintln!(
            "Reviewing {}: {} campaigns, {} multiplayer maps",
            game.title,
            menu.manifest.campaigns.len(),
            catalog.maps.len()
        );
        let config = temporary.path().join("client-settings.ron");
        fs::write(
            &config,
            ron::ser::to_string(&crate::Config::default()).unwrap(),
        )
        .unwrap();
        let screenshot = temporary.path().join("review.ppm");
        for campaign in &menu.manifest.campaigns {
            let directory = campaign_directory(&game, &campaign.directory).unwrap();
            let manifest = Campaign::load(&directory).unwrap();
            eprintln!(
                "Opening {} ({} missions) from the exported bundle",
                campaign.title,
                manifest.missions.len()
            );
            let result = Command::new(appdir.join("usr/bin/straterust-client"))
                .arg("--campaign")
                .arg(directory)
                .args(["--mission", "1", "--benchmark-frames", "30", "--config"])
                .arg(&config)
                .arg("--screenshot")
                .arg(&screenshot)
                .env("LD_LIBRARY_PATH", appdir.join("usr/lib"))
                .env_remove("WAYLAND_DISPLAY")
                .current_dir(temporary.path())
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
            assert!(fs::metadata(&screenshot).unwrap().len() > 1000);
        }
        let map =
            straterust_engine::multiplayer::package_path(&root, &catalog.maps[0].package).unwrap();
        let result = Command::new(appdir.join("usr/bin/straterust-client"))
            .arg("--package")
            .arg(map)
            .args(["--benchmark-frames", "30", "--config"])
            .arg(config)
            .arg("--screenshot")
            .arg(&screenshot)
            .env("LD_LIBRARY_PATH", appdir.join("usr/lib"))
            .env_remove("WAYLAND_DISPLAY")
            .current_dir(temporary.path())
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(fs::metadata(&screenshot).unwrap().len() > 1000);
    }
}
