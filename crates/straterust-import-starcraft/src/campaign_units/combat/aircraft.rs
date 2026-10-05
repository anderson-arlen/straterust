//! Retail movement-only engine overlays baked into ordinary directional clips.
use super::*;

impl Graphics<'_> {
    pub(super) fn engines(
        &mut self,
        archive: &mut SourceArchive,
        files: &mut Files,
        sprite: &mut SpriteManifest,
        source: u16,
    ) -> Result<()> {
        let image = self.tables.image(source);
        let child = instructions(&self.tables.scripts, self.tables.script(image), 11)
            .into_iter()
            .find(|i| i.op == 8)
            .context("aircraft movement has no engine overlay")?;
        ensure!(child.args[2..] == [0, 0], "unsupported engine displacement");
        let engine = usize::from(word(&child.args, 0));
        let body = self.decode(archive, image)?.to_vec();
        let glow = self.decode(archive, engine)?.to_vec();
        ensure!(body.len() >= 17, "incomplete aircraft headings");
        let poses = engine_poses(&self.tables.scripts, self.tables.script(engine), body.len())?;
        let mut frames = body[..17].to_vec();
        let mut bases = Vec::new();
        let mut cache = BTreeMap::new();
        for pose in poses {
            if let Some(&base) = cache.get(&pose) {
                bases.push(base);
                continue;
            }
            let base = frames.len() as u16;
            cache.insert(pose, base);
            bases.push(base);
            for (heading, body) in body.iter().take(17).enumerate() {
                let glow = glow.get(pose + heading).context("invalid engine heading")?;
                let width = body.width.max(glow.width);
                let height = body.height.max(glow.height);
                frames.push(terran::composite(
                    &terran::center_canvas(body, width, height)?,
                    &terran::center_canvas(glow, width, height)?,
                )?);
            }
        }
        // Attack scripts keep the aircraft's body pose; only movement creates
        // the glow. The client already selects Walk and tints cloaked sprites.
        let mut clips = vec![
            terran::directional(ClipKind::Idle, &[0], 42),
            terran::directional(ClipKind::Walk, &bases, 42),
        ];
        if matches!(source, 8 | 12 | 29 | 70) {
            clips.push(terran::directional(ClipKind::Attack, &[0], 42));
        }
        *sprite = terran::compact_sprite(
            files,
            sprite.unit_type.0,
            &sprite.unit_name,
            &format!("aircraft-{source}"),
            &frames,
            clips,
        )?;
        Ok(())
    }
}

fn engine_poses(script: &[u8], id: u16, body_frames: usize) -> Result<Vec<usize>> {
    let mut poses = Vec::new();
    let mut pose = None;
    let mut leading_wait = 0;
    for i in instructions(script, id, 0) {
        match i.op {
            43 => pose = Some(usize::from(i.args[0])),
            // engset follows the body frame and its full frame-set stride.
            44 => pose = Some(body_frames * usize::from(i.args[0])),
            5 => {
                if let Some(pose) = pose {
                    poses.extend(std::iter::repeat_n(pose, usize::from(i.args[0])));
                } else {
                    leading_wait += usize::from(i.args[0]);
                }
            }
            _ => {}
        }
    }
    // Wraith's loop jumps back to a wait before its first engframe. That
    // wait holds the final pose, rather than dropping the second glow phase.
    if let Some(pose) = pose {
        poses.extend(std::iter::repeat_n(pose, leading_wait));
    }
    if poses.is_empty() && instructions(script, id, 0).iter().any(|i| i.op == 29) {
        // Shuttle's glow follows the parent heading with its default frame set.
        poses.push(0);
    }
    ensure!(
        !poses.is_empty() && poses.len() <= 256,
        "invalid engine animation"
    );
    Ok(poses)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn engine_loops_preserve_trailing_pose_and_body_frame_stride() {
        let mut bytes = vec![0xff; 40];
        bytes[..8].copy_from_slice(&[7, 0, 8, 0, 255, 255, 0, 0]);
        bytes[8..18].copy_from_slice(&[b'S', b'C', b'P', b'E', 0, 0, 0, 0, 20, 0]);
        bytes[20..31].copy_from_slice(&[5, 1, 43, 0, 5, 1, 43, 17, 7, 20, 0]);
        assert_eq!(engine_poses(&bytes, 7, 17).unwrap(), vec![0, 17]);
        bytes[20..31].copy_from_slice(&[44, 0, 5, 1, 44, 1, 5, 1, 7, 20, 0]);
        assert_eq!(engine_poses(&bytes, 7, 17).unwrap(), vec![0, 17]);
        assert_eq!(engine_poses(&bytes, 7, 34).unwrap(), vec![0, 34]);
    }
}
