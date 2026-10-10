//! A picker and converter run off the window thread. Only bundled native
//! importers can run; selected executables are never launched.
use super::*;
use std::{process::Command as Process, sync::mpsc};

pub(super) enum Update {
    Progress(String),
    Done(Result<Option<PathBuf>>),
}

impl Client {
    pub(super) fn start_import(&mut self, directory: bool) -> Result<()> {
        let Page::ImportSource(importer) = self.menus.page else {
            return Ok(());
        };
        let root = straterust_importers::games_directory()?;
        let (sender, receiver) = mpsc::channel();
        std::thread::Builder::new()
            .name("straterust-import".into())
            .spawn(move || {
                let result = (|| {
                    let Some(source) = choose_source(directory)? else {
                        return Ok(None);
                    };
                    importer
                        .install(&source, &root, &|message| {
                            let _ = sender.send(Update::Progress(message.into()));
                        })
                        .map(Some)
                })();
                let _ = sender.send(Update::Done(result));
            })?;
        self.importing = Some(receiver);
        self.menus.page = Page::Importing(importer);
        self.menus.message = "Choose the source in the file dialog...".into();
        self.menus.focus = None;
        Ok(())
    }

    pub(super) fn poll_import(&mut self) {
        let Some(receiver) = &self.importing else {
            return;
        };
        let mut done = None;
        loop {
            match receiver.try_recv() {
                Ok(Update::Progress(message)) => self.menus.message = message,
                Ok(Update::Done(result)) => {
                    done = Some(result);
                    break;
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    done = Some(Err(anyhow::anyhow!("import worker stopped unexpectedly")));
                    break;
                }
            }
        }
        let Some(result) = done else {
            return;
        };
        self.importing = None;
        let Page::Importing(importer) = self.menus.page else {
            return;
        };
        self.menus.page = Page::ImportSource(importer);
        self.menus.reset();
        match result {
            Ok(Some(path)) => {
                self.menus.games = catalog::discover(&self.roots);
                self.menus.page = Page::Packages;
                self.menus.history.clear();
                self.menus.message = format!("{} imported to {}", importer.name(), path.display());
            }
            Ok(None) => {}
            Err(error) => {
                log::error!("import: {error:#}");
                self.menus.message = format!("Import failed: {error:#}");
            }
        }
    }
}

fn choose_source(directory: bool) -> Result<Option<PathBuf>> {
    // Linux is the current supported desktop. Use its native toolkit chooser,
    // passing paths as arguments, never as shell text.
    let title = if directory {
        "Choose game directory"
    } else {
        "Choose game ISO or EXE"
    };
    let mut command = Process::new("zenity");
    command.args(["--file-selection", "--title", title]);
    if directory {
        command.arg("--directory");
    } else {
        command.arg("--file-filter=Game source | *.iso *.ISO *.exe *.EXE");
    }
    let output = match command.output() {
        Ok(output) => output,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let mut command = Process::new("kdialog");
            if directory {
                command.args(["--getexistingdirectory", ".", "--title", title]);
            } else {
                command.args([
                    "--getopenfilename",
                    ".",
                    "*.iso *.ISO *.exe *.EXE",
                    "--title",
                    title,
                ]);
            }
            command
                .output()
                .context("install zenity or kdialog to choose import sources")?
        }
        Err(e) => return Err(e).context("open import file chooser"),
    };
    if output.status.code() == Some(1) {
        return Ok(None);
    }
    ensure!(
        output.status.success(),
        "file chooser failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    #[cfg(unix)]
    let path = {
        use std::os::unix::ffi::OsStringExt;
        let mut bytes = output.stdout;
        if bytes.last() == Some(&b'\n') {
            bytes.pop();
        }
        PathBuf::from(std::ffi::OsString::from_vec(bytes))
    };
    #[cfg(not(unix))]
    let path = PathBuf::from(String::from_utf8(output.stdout)?.trim_end());
    ensure!(
        path.is_absolute(),
        "file chooser returned a non-absolute path"
    );
    Ok(Some(path))
}
