//! Retail menu BIN geometry and PCX/Smacker art end at this importer. Native
//! menus keep a small set of client actions rather than emulating dialog code.
use crate::{Archive, Files, Source, add_image, formats, ron_bytes, terran_media};
use anyhow::{Context, Result, ensure};
use std::{
    io::{Read, Seek},
    path::Path,
};
use straterust_engine::{
    assets::Image,
    menus::{MenuAction, MenuAnimation, MenuButton, MenuManifest, MenuPack, MenuScreen},
};

pub(super) fn refresh(source: &Path, output: &Path) -> Result<()> {
    let source = Source::open(source)?;
    let mut installer = Archive::open_region(&source.path, source.offset, source.len)?;
    let mut archive =
        Archive::from_bytes(installer.read_file("files\\stardat.mpq", 128 * 1024 * 1024)?)?;
    let mut files = convert(&mut installer, &mut archive)?;
    let mut menu: MenuManifest = ron::de::from_bytes(&files["menus.ron"])?;
    menu.campaigns.clear();
    for (race, title) in [
        ("terran", "Terran - Episode I"),
        ("zerg", "Zerg - Episode II"),
        ("protoss", "Protoss - Episode III"),
    ] {
        let directory = if output.join(race).join("campaign.ron").is_file() {
            race
        } else {
            "."
        };
        if let Ok(campaign) = straterust_engine::content::Campaign::load(&output.join(directory))
            && (campaign.id == format!("straterust.{race}-first-five")
                || campaign.id == format!("stratarust.{race}-first-five"))
        {
            menu.campaigns.push(straterust_engine::menus::MenuCampaign {
                title: title.into(),
                directory: directory.into(),
            });
            if let Some(button) = menu
                .screens
                .iter_mut()
                .find(|screen| screen.id == "campaigns")
                .and_then(|screen| {
                    screen
                        .buttons
                        .iter_mut()
                        .find(|button| button.label == title)
                })
            {
                button.action = MenuAction::Campaign(directory.into());
            }
        }
    }
    menu.validate()?;
    files.insert("menus.ron".into(), ron_bytes(&menu)?);
    ensure!(
        output.is_dir(),
        "menu output must be an existing native game/campaign package"
    );
    publish(output, &files)?;
    MenuPack::load(output)?.context("missing published menu")?;
    println!("Updated game menus: {}", output.display());
    Ok(())
}

pub(super) fn publish(output: &Path, files: &Files) -> Result<()> {
    // Publish artwork first and the referring document last.
    for (name, bytes) in files
        .iter()
        .filter(|(name, _)| name.as_str() != "menus.ron")
    {
        std::fs::write(output.join(name), bytes)?;
    }
    let temporary = output.join(".menus.ron.tmp");
    std::fs::write(&temporary, &files["menus.ron"])?;
    std::fs::rename(temporary, output.join("menus.ron"))?;
    Ok(())
}

pub(super) fn convert<I: Read + Seek, A: Read + Seek>(
    installer: &mut Archive<I>,
    archive: &mut Archive<A>,
) -> Result<Files> {
    let main = archive.read_file("rez\\gluMain.bin", 65536)?;
    let campaign = installer.read_file("rez\\gluCmpgn.bin", 65536)?;
    let pause = archive.read_file("rez\\gameMenu.bin", 65536)?;
    let options = archive.read_file("rez\\options.bin", 65536)?;
    ensure!(
        control(&pause, 0)?.rect == [184, 32, 264, 288]
            && control(&options, 0)?.rect == [184, 32, 264, 288],
        "unexpected retail menu geometry"
    );
    let palette =
        formats::decode_pcx(&archive.read_file("glue\\palmm\\backgnd.pcx", 4 * 1024 * 1024)?)?;
    let mut files = Files::new();
    let mut menu = MenuManifest::basic("StarCraft", true);
    // The retail executable's frontend soundtrack is music\\title.wav.
    let music = installer.read_file("music\\title.wav", 128 * 1024 * 1024)?;
    let music = terran_media::normalize_wav(&music, 600_000, &mut 0)?;
    menu.music.push(terran_media::audio_file(
        &mut files,
        "music-menu-title.wav".into(),
        music,
    ));
    menu.campaigns[0].title = "Terran - Episode I".into();
    menu.background = Some(add_image(
        &mut files,
        "menu-main-background.srim",
        &pcx_image(&palette),
    )?);
    let cs =
        formats::decode_pcx(&installer.read_file("glue\\palcs\\backgnd.pcx", 4 * 1024 * 1024)?)?;
    let cs_image = add_image(&mut files, "menu-campaign-background.srim", &pcx_image(&cs))?;
    let arrow = formats::decode_grp(
        &archive.read_file("glue\\palmm\\arrow.grp", 1024 * 1024)?,
        &palette.palette,
    )?;
    let mut frames = Vec::new();
    for (i, frame) in arrow.iter().enumerate() {
        frames.push(add_image(
            &mut files,
            &format!("menu-cursor-{i:03}.srim"),
            frame,
        )?);
    }
    menu.cursor = Some(MenuAnimation {
        frame_ms: 66,
        frames,
    });
    // Same source cursor hotspot as the in-game arrow; GRP canvases include padding.
    menu.cursor_anchor = [63, 63];
    let mut single = control(&main, 258)?;
    single.label = "Single Player".into();
    single.key = Some("S".into());
    single.action = MenuAction::Screen("campaigns".into());
    single.idle = Some(animation(
        archive,
        &mut files,
        "single",
        "glue\\mainmenu\\Single.smk",
        false,
    )?);
    single.hover = Some(animation(
        archive,
        &mut files,
        "single-hover",
        "glue\\mainmenu\\SingleOn.smk",
        false,
    )?);
    let mut exit = control(&main, 172)?;
    exit.label = "Exit".into();
    exit.key = Some("X".into());
    exit.action = MenuAction::Quit;
    exit.idle = Some(animation(
        archive,
        &mut files,
        "exit",
        "glue\\mainmenu\\Exit.smk",
        false,
    )?);
    exit.hover = Some(animation(
        archive,
        &mut files,
        "exit-hover",
        "glue\\mainmenu\\ExitOn.smk",
        false,
    )?);
    let mut multi = control(&main, 344)?;
    multi.label = "Multiplayer".into();
    multi.action = MenuAction::Multiplayer;
    multi.idle = Some(animation(
        archive,
        &mut files,
        "multi",
        "glue\\mainmenu\\Multi.smk",
        true,
    )?);
    let mut editor = control(&main, 430)?;
    editor.label = "Campaign Editor".into();
    editor.action =
        MenuAction::Unavailable("The original campaign editor is not part of this client.".into());
    editor.idle = Some(animation(
        archive,
        &mut files,
        "editor",
        "glue\\mainmenu\\Editor.smk",
        true,
    )?);
    let mut settings = straterust_engine::menus::button("Options", 410, MenuAction::Settings);
    settings.rect = [20, 410, 184, 28];
    settings.key = Some("O".into());
    let mut games =
        straterust_engine::menus::button("Choose another game", 450, MenuAction::ChooseGame);
    games.rect = [20, 450, 250, 26];
    menu.screens[0].buttons = vec![single, multi, editor, exit, settings, games];
    let mut races = Vec::new();
    for (offset, label, path, action, first) in [
        (
            430,
            "Terran - Episode I",
            "terr",
            MenuAction::Campaign(".".into()),
            false,
        ),
        (
            516,
            "Zerg - Episode II",
            "zerg",
            MenuAction::Unavailable("The Zerg campaign has not been imported.".into()),
            false,
        ),
        (
            344,
            "Protoss - Episode III",
            "prot",
            MenuAction::Unavailable("The Protoss campaign has not been imported.".into()),
            false,
        ),
    ] {
        let mut button = control(&campaign, offset)?;
        button.label = label.into();
        button.action = action;
        button.idle = Some(animation(
            archive,
            &mut files,
            path,
            &format!("glue\\campaign\\{path}.smk"),
            first,
        )?);
        {
            button.key = Some(
                match path {
                    "terr" => "T",
                    "zerg" => "Z",
                    _ => "P",
                }
                .into(),
            );
            button.hover = Some(animation(
                archive,
                &mut files,
                &format!("{path}-hover"),
                &format!("glue\\campaign\\{path}on.smk"),
                false,
            )?);
        }
        races.push(button);
    }
    let mut back =
        straterust_engine::menus::button("Cancel", 416, MenuAction::Screen("home".into()));
    back.rect = [501, 416, 101, 20];
    races.push(back);
    menu.screens.push(MenuScreen {
        id: "campaigns".into(),
        title: "Select Campaign".into(),
        title_y: 378,
        background: Some(cs_image),
        buttons: races,
    });
    let pause = &mut menu.screens[1];
    pause.buttons = [
        ("Return to Game (Esc)", MenuAction::Resume),
        (
            "Save Game",
            MenuAction::Unavailable("Saving games is not implemented yet.".into()),
        ),
        (
            "Load Game",
            MenuAction::Unavailable("Loading saved games is not implemented yet.".into()),
        ),
        ("Options", MenuAction::Settings),
        ("Mission Objectives", MenuAction::Objectives),
        ("Help", MenuAction::Help),
        ("Restart Mission", MenuAction::Restart),
        ("End Mission", MenuAction::EndMission),
        ("Quit", MenuAction::Quit),
    ]
    .into_iter()
    .enumerate()
    .map(|(i, (label, action))| straterust_engine::menus::button(label, 88 + i as u16 * 37, action))
    .collect();
    menu.validate()?;
    files.insert("menus.ron".into(), ron_bytes(&menu)?);
    Ok(files)
}

fn animation<R: Read + Seek>(
    archive: &mut Archive<R>,
    files: &mut Files,
    name: &str,
    path: &str,
    first_only: bool,
) -> Result<MenuAnimation> {
    let decoded = terran_media::decode_menu_smk(&archive.read_file(path, 8 * 1024 * 1024)?)?;
    let mut frames = Vec::new();
    for (i, mut image) in decoded
        .frames
        .into_iter()
        .take(if first_only { 1 } else { 128 })
        .enumerate()
    {
        // Source menu videos draw palette zero as transparent over the PCX.
        for pixel in image.rgba.as_chunks_mut::<4>().0 {
            if pixel[..3] == [0, 0, 0] {
                pixel[3] = 0;
            }
        }
        frames.push(add_image(
            files,
            &format!("menu-{name}-{i:03}.srim"),
            &image,
        )?);
    }
    Ok(MenuAnimation {
        frame_ms: decoded.frame_ms,
        frames,
    })
}

fn pcx_image(image: &formats::IndexedImage) -> Image {
    Image {
        width: image.width,
        height: image.height,
        rgba: image
            .pixels
            .iter()
            .flat_map(|p| image.palette[usize::from(*p)])
            .collect(),
    }
}

/// Retail BIN controls are 86-byte records; string offsets and the next-control
/// pointer are file-relative. We only import the verified control rectangles.
fn control(bytes: &[u8], offset: usize) -> Result<MenuButton> {
    let data = bytes
        .get(offset..offset + 86)
        .context("truncated menu control")?;
    let word = |n| u16::from_le_bytes([data[n], data[n + 1]]);
    let next = u32::from_le_bytes(data[..4].try_into().unwrap()) as usize;
    ensure!(
        next == 0 || next + 86 <= bytes.len(),
        "invalid menu control link"
    );
    let text = u32::from_le_bytes(data[20..24].try_into().unwrap()) as usize;
    ensure!(
        text < bytes.len() && bytes[text..].contains(&0),
        "invalid menu control string"
    );
    Ok(MenuButton {
        label: "Menu".into(),
        rect: [word(4), word(6), word(12), word(14)],
        action: MenuAction::Resume,
        key: None,
        idle: None,
        hover: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn menu_controls_validate_bounds_before_import() {
        assert!(control(&[0; 85], 0).is_err());
        let mut bytes = vec![0; 88];
        bytes[20..24].copy_from_slice(&86_u32.to_le_bytes());
        bytes[86] = b'X';
        bytes[4..6].copy_from_slice(&12_u16.to_le_bytes());
        assert_eq!(control(&bytes, 0).unwrap().rect[0], 12);
        bytes[..4].copy_from_slice(&80_u32.to_le_bytes());
        assert!(control(&bytes, 0).is_err());
    }

    #[test]
    #[ignore = "requires STRATERUST_SOURCE pointing to the retail disc"]
    fn original_menu_title_music_decodes_as_native_pcm() -> Result<()> {
        let path = std::env::var_os("STRATERUST_SOURCE").context("set STRATERUST_SOURCE")?;
        let source = Source::open(Path::new(&path))?;
        let mut installer = Archive::open_region(&source.path, source.offset, source.len)?;
        let source = installer.read_file("music\\title.wav", 128 * 1024 * 1024)?;
        let wav = terran_media::normalize_wav(&source, 600_000, &mut 0)?;
        let pcm = straterust_engine::media::decode_wav(&wav)?;
        ensure!(
            pcm.duration_ms() > 10_000 && pcm.samples.iter().any(|s| *s != 0),
            "empty or truncated source title music"
        );
        if let Some(root) = std::env::var_os("STRATERUST_CAMPAIGNS") {
            let pack = MenuPack::load(Path::new(&root))?.context("missing menu pack")?;
            ensure!(
                pack.music.len() == 1 && pack.music[0].samples == pcm.samples,
                "published title music differs from original"
            );
        }
        println!(
            "Retail title music: {} ms, {} channels",
            pcm.duration_ms(),
            pcm.channels
        );
        Ok(())
    }
}
