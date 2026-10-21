//! Validate every package's references while decoding each shared payload once.
//! Cache only digests and dimensions/durations, never resident artwork or PCM.
use super::{FileId, file_id};
use crate::{
    asset_archive::{Location, Reader},
    assets::{AssetManifest, ImageRef, MAX_IMAGE_DIMENSION, MAX_PACK_RGBA_BYTES, decode_image},
    content::read_ron,
    media::{
        AudioRef, MAX_PCM_BYTES, MAX_PORTRAIT_BYTES, MAX_WAV_BYTES, MediaManifest, decode_wav,
    },
    menus::{MenuAnimation, MenuManifest},
};
use anyhow::{Context, Result, ensure};
use std::{
    collections::{HashMap, HashSet, hash_map::Entry},
    fs,
    path::Path,
};

#[derive(Clone, Copy, Debug)]
struct ImageInfo {
    width: u32,
    height: u32,
    rgba_bytes: usize,
}
#[derive(Clone, Copy, Debug)]
struct AudioInfo {
    duration_ms: u64,
    pcm_bytes: usize,
}
struct Checked<T> {
    digest: String,
    info: T,
}

#[derive(Default)]
pub(super) struct Validation {
    images: HashMap<(FileId, u64), Checked<ImageInfo>>,
    audio: HashMap<(FileId, u64), Checked<AudioInfo>>,
    source: Option<(std::path::PathBuf, Reader)>,
}

impl Validation {
    fn location(&mut self, root: &Path, name: &str, digest: &str) -> Result<Location> {
        if self
            .source
            .as_ref()
            .is_none_or(|(directory, _)| directory != root)
        {
            let reader = match &self.source {
                Some((_, reader)) => reader.for_directory(root)?,
                None => Reader::open(root)?,
            };
            self.source = Some((root.to_owned(), reader));
        }
        self.source.as_ref().unwrap().1.location(name, digest)
    }
    pub(super) fn directory(&mut self, root: &Path) -> Result<()> {
        if let Some(info) = crate::game::GameInfo::load(root)? {
            info.load_icon(root)?;
        }
        if root.join("assets.ron").is_file() {
            self.assets(root)?;
        }
        if root.join("media.ron").is_file() {
            self.media(root)?;
        }
        if root.join("menus.ron").is_file() {
            self.menus(root)?;
        }
        Ok(())
    }

    fn image(&mut self, root: &Path, reference: &ImageRef) -> Result<ImageInfo> {
        let location = self.location(root, &reference.file, &reference.blake3)?;
        let path = &location.path;
        let metadata = fs::symlink_metadata(path)
            .with_context(|| format!("missing asset {}", path.display()))?;
        ensure!(
            metadata.is_file() && location.length <= 16 + u64::from(MAX_IMAGE_DIMENSION).pow(2) * 4,
            "invalid asset file {}",
            path.display()
        );
        let id = (file_id(path, &metadata), location.offset);
        let checked = match self.images.entry(id) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => {
                let bytes = location
                    .read(16 + MAX_IMAGE_DIMENSION as usize * MAX_IMAGE_DIMENSION as usize * 4)?;
                let image = decode_image(&bytes)
                    .with_context(|| format!("invalid asset {}", path.display()))?;
                entry.insert(Checked {
                    digest: blake3::hash(&bytes).to_hex().to_string(),
                    info: ImageInfo {
                        width: image.width,
                        height: image.height,
                        rgba_bytes: image.rgba.len(),
                    },
                })
            }
        };
        ensure!(
            checked.digest == reference.blake3,
            "asset hash mismatch: {}",
            path.display()
        );
        Ok(checked.info)
    }

    fn audio(&mut self, root: &Path, reference: &AudioRef) -> Result<AudioInfo> {
        let location = self.location(root, &reference.file, &reference.blake3)?;
        let path = &location.path;
        let metadata = fs::symlink_metadata(path)
            .with_context(|| format!("missing audio {}", path.display()))?;
        ensure!(
            metadata.is_file() && location.length <= MAX_WAV_BYTES as u64,
            "invalid audio file {}",
            path.display()
        );
        let id = (file_id(path, &metadata), location.offset);
        let checked = match self.audio.entry(id) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => {
                let bytes = location.read(MAX_WAV_BYTES)?;
                let clip = decode_wav(&bytes)
                    .with_context(|| format!("invalid audio {}", path.display()))?;
                entry.insert(Checked {
                    digest: blake3::hash(&bytes).to_hex().to_string(),
                    info: AudioInfo {
                        duration_ms: clip.duration_ms(),
                        pcm_bytes: clip.samples.len() * 2,
                    },
                })
            }
        };
        ensure!(
            checked.digest == reference.blake3,
            "media hash mismatch: {}",
            path.display()
        );
        Ok(checked.info)
    }

    fn charged_image(
        &mut self,
        root: &Path,
        image: &ImageRef,
        rgba: &mut usize,
    ) -> Result<ImageInfo> {
        let info = self.image(root, image)?;
        *rgba += info.rgba_bytes;
        ensure!(
            *rgba <= MAX_PACK_RGBA_BYTES,
            "native asset pack exceeds the 512 MiB RGBA limit"
        );
        Ok(info)
    }

    fn frames(
        &mut self,
        root: &Path,
        frames: &[ImageRef],
        anchor: [i32; 2],
        rgba: &mut usize,
    ) -> Result<()> {
        ensure!(
            !frames.is_empty() && anchor.iter().all(|v| *v >= 0),
            "invalid animation anchor"
        );
        for frame in frames {
            let image = self.charged_image(root, frame, rgba)?;
            ensure!(
                anchor[0] < image.width as i32 && anchor[1] < image.height as i32,
                "native animation anchor is outside its frame canvas"
            );
        }
        Ok(())
    }

    fn assets(&mut self, root: &Path) -> Result<()> {
        let manifest = AssetManifest::load_manifest(root)?;
        manifest.validate()?;
        let mut rgba = 0;
        let terrain = self.charged_image(root, &manifest.terrain, &mut rgba)?;
        if let Some(grid) = &manifest.terrain_grid {
            ensure!(
                terrain.width.is_multiple_of(grid.tile_size)
                    && terrain.height.is_multiple_of(grid.tile_size),
                "terrain atlas dimensions must be multiples of tile_size"
            );
            let count = (terrain.width / grid.tile_size) * (terrain.height / grid.tile_size);
            ensure!(
                grid.tiles.iter().all(|tile| *tile < count),
                "terrain grid references a tile outside its atlas"
            );
        }
        self.frames(root, &manifest.frames, manifest.anchor, &mut rgba)?;
        for sprite in &manifest.extra_units {
            self.frames(root, &sprite.frames, sprite.anchor, &mut rgba)?;
        }
        for resource in &manifest.resources {
            self.frames(
                root,
                std::slice::from_ref(&resource.image),
                resource.anchor,
                &mut rgba,
            )?;
            let original = self.image(root, &resource.image)?;
            for variant in resource.depleted_image.iter().chain(&resource.active_image) {
                self.frames(
                    root,
                    std::slice::from_ref(variant),
                    resource.anchor,
                    &mut rgba,
                )?;
                let image = self.image(root, variant)?;
                ensure!(
                    image.width == original.width && image.height == original.height,
                    "resource artwork variants need the same canvas"
                );
            }
        }
        for entry in &manifest.ui {
            self.charged_image(root, &entry.image, &mut rgba)?;
        }
        for entry in &manifest.map_images {
            self.frames(
                root,
                std::slice::from_ref(&entry.image),
                entry.anchor,
                &mut rgba,
            )?;
        }
        let mut effects: Vec<_> = manifest.scan_effect.iter().collect();
        for projectile in &manifest.projectiles {
            effects.extend([&projectile.flight, &projectile.impact]);
            effects.extend(projectile.charge.iter().chain(&projectile.marker));
            effects.extend(projectile.trail.iter().map(|trail| &trail.effect));
        }
        if let Some(damage) = &manifest.damage_effects {
            effects.extend(damage.small.iter().chain(&damage.large));
            for style in &damage.styles {
                effects.extend(style.small.iter().chain(&style.large));
            }
        }
        if let Some(gas) = &manifest.gas_effects {
            effects.extend(gas.plumes.iter().chain(std::iter::once(&gas.depleted)));
        }
        for effect in effects {
            self.frames(root, &effect.frames, effect.anchor, &mut rgba)?;
        }
        if let Some(creep) = &manifest.creep {
            for reference in creep.tiles.iter().chain(&creep.edges) {
                let image = self.charged_image(root, reference, &mut rgba)?;
                ensure!(
                    image.width == 32 && image.height == 32,
                    "creep art must use 32px source tiles"
                );
            }
        }
        if let Some(indicators) = &manifest.indicators {
            for reference in &indicators.circles {
                let image = self.charged_image(root, reference, &mut rgba)?;
                ensure!(image.height.is_multiple_of(3), "invalid circle atlas");
            }
            for cursor in &indicators.cursors {
                let image = self.charged_image(root, &cursor.image, &mut rgba)?;
                let frames = u32::from(cursor.frames);
                ensure!(
                    image.height.is_multiple_of(frames)
                        && u32::from(cursor.anchor[0]) < image.width
                        && u32::from(cursor.anchor[1]) < image.height / frames,
                    "invalid cursor atlas/anchor"
                );
            }
        }
        for cargo in &manifest.carried_resources {
            for sprite in std::iter::once(&cargo.full).chain(&cargo.partial) {
                self.frames(root, &sprite.frames, sprite.anchor, &mut rgba)?;
            }
        }
        Ok(())
    }

    fn media(&mut self, root: &Path) -> Result<()> {
        let manifest: MediaManifest = read_ron(&root.join("media.ron"))?;
        manifest.validate()?;
        let mut pcm = 0;
        let mut loaded = HashSet::new();
        for (reference, limit) in manifest
            .audio
            .iter()
            .flat_map(|entry| &entry.variants)
            .map(|r| (r, 30_000))
            .chain(manifest.music.iter().map(|r| (r, 600_000)))
            .chain(manifest.mission_audio.iter().map(|r| (r, 300_000)))
        {
            let audio = self.audio(root, reference)?;
            ensure!(
                audio.duration_ms <= limit,
                "audio duration exceeds its cue limit: {}",
                reference.file
            );
            if loaded.insert(&reference.file) {
                pcm += audio.pcm_bytes;
            }
            ensure!(
                pcm <= MAX_PCM_BYTES,
                "native audio exceeds resident PCM limit"
            );
        }
        let mut rgba = 0;
        for portrait in &manifest.portraits {
            let first = self.image(root, &portrait.idle[0])?;
            for reference in portrait.idle.iter().chain(&portrait.talk) {
                let image = self.image(root, reference)?;
                ensure!(
                    image.width <= 256 && image.height <= 256,
                    "portrait exceeds 256 pixel dimension limit"
                );
                ensure!(
                    image.width == first.width && image.height == first.height,
                    "portrait frames must share dimensions"
                );
                rgba += image.rgba_bytes;
                ensure!(
                    rgba <= MAX_PORTRAIT_BYTES,
                    "portraits exceed resident RGBA limit"
                );
            }
        }
        Ok(())
    }

    fn menu_animation(&mut self, root: &Path, animation: &MenuAnimation) -> Result<()> {
        for image in &animation.frames {
            self.image(root, image)?;
        }
        Ok(())
    }

    fn menus(&mut self, root: &Path) -> Result<()> {
        let manifest: MenuManifest = read_ron(&root.join("menus.ron"))?;
        manifest.validate()?;
        for image in manifest.background.iter().chain(&manifest.button_image) {
            self.image(root, image)?;
        }
        if let Some(animation) = &manifest.cursor {
            self.menu_animation(root, animation)?;
        }
        for screen in &manifest.screens {
            if let Some(image) = &screen.background {
                self.image(root, image)?;
            }
            for button in &screen.buttons {
                for animation in button.idle.iter().chain(&button.hover) {
                    self.menu_animation(root, animation)?;
                }
            }
        }
        let mut pcm = 0;
        for audio in &manifest.music {
            pcm += self.audio(root, audio)?.pcm_bytes;
            ensure!(pcm <= MAX_PCM_BYTES, "menu music exceeds PCM memory limit");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
