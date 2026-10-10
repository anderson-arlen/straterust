//! Original foundations, halfway artwork and destroyed building sites.
use super::{gfx, war::WarArchive};
use anyhow::{Context, Result};
use std::path::Path;
use straterust_engine::assets::*;

pub fn add(
    archive: &WarArchive,
    root: &Path,
    palette: &gfx::Palette,
    sprite: &mut SpriteManifest,
    source: usize,
    era: usize,
) -> Result<()> {
    if !(58..=99).contains(&source) {
        return Ok(());
    }
    let race = source % 2;
    let record = match source - race {
        72 => 253 + race + usize::from(era == 1) * 10,
        86 => match era {
            1 => 265 + race,
            2 => 271 + race,
            _ => 255 + race,
        },
        84 => 257 + race + usize::from(era == 1) * 10,
        78 => 259 + race + usize::from(era == 1) * 10,
        _ => {
            if era == 1 {
                262
            } else {
                252
            }
        }
    };
    let images = gfx::sprites(&archive.entry(record)?, palette)
        .with_context(|| format!("construction artwork {record}"))?;
    let mut frames = Vec::new();
    for image in images.iter().take(2) {
        let frame = sprite.frames.len() as u16;
        sprite.frames.push(gfx::write_image(
            root,
            &gfx::pad_for_anchor(image, sprite.anchor),
        )?);
        frames.push(ClipFrame {
            frame,
            flip_x: false,
            offset: [
                (sprite.anchor[0] - image.width as i32 / 2) as i16,
                (sprite.anchor[1] - image.height as i32 / 2) as i16,
            ],
        });
    }
    if frames.len() == 1 {
        frames.push(frames[0]);
    }
    frames.push(ClipFrame {
        frame: 1,
        flip_x: false,
        offset: [0, 0],
    });
    sprite.clips.retain(|c| c.kind != ClipKind::Construction);
    sprite.clips.push(SpriteClip {
        kind: ClipKind::Construction,
        directions: 1,
        frame_ms: 100,
        frames,
        key_steps: Vec::new(),
        loop_start: None,
        progress_starts: vec![(0, 0), (25, 1), (50, 2)],
    });
    Ok(())
}
