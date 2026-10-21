//! Native destination chooser and background AppImage export.
use super::*;
use std::{process::Command as Process, sync::mpsc};

impl Client {
    pub(super) fn start_export(&mut self, index: usize) -> Result<()> {
        ensure!(cfg!(target_os = "linux"), "AppImage export requires Linux");
        let game = self
            .menus
            .games
            .get(index)
            .context("game is no longer installed")?
            .clone();
        let (sender, receiver) = mpsc::channel();
        std::thread::Builder::new()
            .name("straterust-export".into())
            .spawn(move || {
                let result = (|| {
                    let Some(output) = choose_destination(&game)? else {
                        return Ok(None);
                    };
                    crate::appimage::export(&game, &output, &|message| {
                        let _ = sender.send(imports::Update::Progress(message.into()));
                    })?;
                    Ok(Some(output))
                })();
                let _ = sender.send(imports::Update::Done(result));
            })?;
        self.exporting = Some(receiver);
        self.menus.page = Page::Exporting;
        self.menus.message = "Choose the AppImage destination in the file dialog...".into();
        self.menus.focus = None;
        Ok(())
    }

    pub(super) fn poll_export(&mut self) {
        let Some(receiver) = &self.exporting else {
            return;
        };
        let mut done = None;
        loop {
            match receiver.try_recv() {
                Ok(imports::Update::Progress(message)) => self.menus.message = message,
                Ok(imports::Update::Done(result)) => {
                    done = Some(result);
                    break;
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    done = Some(Err(anyhow::anyhow!("export worker stopped unexpectedly")));
                    break;
                }
            }
        }
        let Some(result) = done else {
            return;
        };
        self.exporting = None;
        self.menus.page = Page::ExportGames;
        self.menus.reset();
        match result {
            Ok(Some(path)) => self.menus.message = format!("AppImage created: {}", path.display()),
            Ok(None) => {}
            Err(error) => {
                log::error!("export: {error:#}");
                self.menus.message = format!("Export failed: {error:#}");
            }
        }
    }
}

fn choose_destination(game: &GameEntry) -> Result<Option<PathBuf>> {
    let default = std::env::current_dir()?.join(crate::appimage::filename(game));
    let output = match Process::new("zenity")
        .args([
            "--file-selection",
            "--save",
            "--confirm-overwrite",
            "--title",
            "Export game AppImage",
            "--file-filter=AppImage | *.AppImage",
            "--filename",
        ])
        .arg(&default)
        .output()
    {
        Ok(output) => output,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Process::new("kdialog")
            .arg("--getsavefilename")
            .arg(&default)
            .args(["*.AppImage", "--title", "Export game AppImage"])
            .output()
            .context("install zenity or kdialog to choose the AppImage destination")?,
        Err(error) => return Err(error).context("open AppImage file chooser"),
    };
    imports::chooser_path(output)
}
