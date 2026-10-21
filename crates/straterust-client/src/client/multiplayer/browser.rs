//! Disk inspection and thumbnail generation never block menu input/drawing.
use super::*;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use straterust_engine::multiplayer::browser::{self, Overview, Preview};

pub(in crate::client) struct BrowserJobs {
    root: PathBuf,
    index: Option<Receiver<Vec<Result<Overview>>>>,
    preview: Option<(usize, Receiver<Result<Preview>>)>,
    cancelled: Arc<AtomicBool>,
}

impl Drop for BrowserJobs {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}

impl Client {
    pub(in crate::client) fn poll_map_browser(&mut self) {
        if !matches!(
            self.menus.page,
            Page::LanMaps | Page::Lobby | Page::MatchSlots
        ) || self.menus.match_catalog.is_none()
        {
            return;
        }
        if self.menus.page != Page::LanMaps
            && self.menus.map_browser.selected != Some(self.menus.match_map)
        {
            self.menus.browse_map(self.menus.match_map);
        }
        let root = self.menus.game.as_ref().unwrap().directory.clone();
        if self
            .map_browsing
            .as_ref()
            .is_none_or(|job| job.root != root)
        {
            self.map_browsing = Some(BrowserJobs {
                root: root.clone(),
                index: None,
                preview: None,
                cancelled: Arc::new(AtomicBool::new(false)),
            });
        }
        let job = self.map_browsing.as_mut().unwrap();
        if self.menus.page == Page::LanMaps
            && !self.menus.map_browser.indexed
            && job.index.is_none()
        {
            let catalog = self.menus.match_catalog.clone().unwrap();
            let root = root.clone();
            let cancelled = Arc::clone(&job.cancelled);
            let (send, receive) = std::sync::mpsc::sync_channel(1);
            std::thread::spawn(move || {
                let values = catalog
                    .maps
                    .iter()
                    .take_while(|_| !cancelled.load(Ordering::Relaxed))
                    .map(|m| {
                        let directory =
                            straterust_engine::multiplayer::package_path(&root, &m.package)?;
                        browser::inspect(&directory, m, &catalog)
                    })
                    .collect();
                let _ = send.send(values);
            });
            job.index = Some(receive);
        }
        if let Some(receive) = &job.index {
            match receive.try_recv() {
                Ok(values) => {
                    for (i, value) in values.iter().enumerate() {
                        if let Err(error) = value {
                            log::warn!(
                                "map browser {}: {error:#}",
                                self.menus.match_catalog.as_ref().unwrap().maps[i].title
                            );
                        }
                    }
                    self.menus
                        .map_browser
                        .index(self.menus.match_catalog.as_ref().unwrap(), values);
                    self.menus.offset = 0;
                    self.menus.focus = None;
                    job.index = None;
                }
                Err(TryRecvError::Disconnected) => {
                    self.menus
                        .map_browser
                        .index(self.menus.match_catalog.as_ref().unwrap(), Vec::new());
                    self.menus.message =
                        "Map comparison failed; original maps are still available.".into();
                    job.index = None;
                }
                Err(TryRecvError::Empty) => {}
            }
        }
        if self.menus.map_browser.selected.is_none()
            && let Some(i) = self.menus.map_indices().first().copied()
        {
            self.menus.browse_map(i);
        }
        let Some(index) = self.menus.map_browser.selected else {
            return;
        };
        if self.menus.map_browser.preview_map != Some(index) && job.preview.is_none() {
            let relative = self.menus.match_catalog.as_ref().unwrap().maps[index]
                .package
                .clone();
            let (send, receive) = std::sync::mpsc::sync_channel(1);
            std::thread::spawn(move || {
                let result = straterust_engine::multiplayer::package_path(&root, &relative)
                    .and_then(|p| browser::preview(&p));
                let _ = send.send(result);
            });
            job.preview = Some((index, receive));
        }
        if let Some((loading, receive)) = &job.preview {
            match receive.try_recv() {
                Ok(result) => {
                    if *loading == index {
                        self.menus.map_browser.preview_map = Some(index);
                        match result {
                            Ok(preview) => {
                                // The GPU caches allocation identities. A replaced
                                // thumbnail can reuse the previous pixel allocation.
                                if let Some(surface) = &mut self.surface {
                                    surface.reset_assets();
                                }
                                if let Some(surface) =
                                    self.session.as_mut().and_then(|app| app.surface.as_mut())
                                {
                                    surface.reset_assets();
                                }
                                self.menus.map_browser.preview = Some(preview);
                            }
                            Err(error) => {
                                self.menus.map_browser.preview_error =
                                    format!("Preview unavailable: {error:#}")
                            }
                        }
                    }
                    job.preview = None;
                }
                Err(TryRecvError::Disconnected) => {
                    self.menus.map_browser.preview_map = Some(index);
                    self.menus.map_browser.preview_error = "Preview worker stopped.".into();
                    job.preview = None;
                }
                Err(TryRecvError::Empty) => {}
            }
        }
    }
}
