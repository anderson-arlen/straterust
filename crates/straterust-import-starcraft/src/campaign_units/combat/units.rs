//! Source Goliath poses and bunker firing ports; no runtime script execution.
use super::*;

impl Graphics<'_> {
    pub(super) fn goliath(
        &mut self,
        archive: &mut SourceArchive,
        files: &mut Files,
        sprite: &mut SpriteManifest,
    ) -> Result<()> {
        let body = self.decode(archive, 234)?.to_vec();
        let turret = self.decode(archive, 235)?.to_vec();
        let mut frames = Vec::new();
        let mut clips = Vec::new();
        let mut cache = BTreeMap::new();
        for (kind, poses) in [
            (ClipKind::Idle, vec![119]),
            (ClipKind::Walk, timeline(&self.tables.scripts, 75, 11)),
            (ClipKind::Attack, timeline(&self.tables.scripts, 76, 5)),
        ] {
            ensure!(!poses.is_empty(), "missing Goliath source poses");
            let mut bases = Vec::new();
            for pose in poses {
                let body_pose = if kind == ClipKind::Walk {
                    usize::from(pose)
                } else {
                    119
                };
                if let Some(&base) = cache.get(&(body_pose, pose)) {
                    bases.push(base);
                    continue;
                }
                let base = frames.len() as u16;
                bases.push(base);
                cache.insert((body_pose, pose), base);
                for heading in 0..17 {
                    frames.push(terran::composite(
                        body.get(body_pose + heading)
                            .context("invalid Goliath body pose")?,
                        turret
                            .get(usize::from(pose) + heading)
                            .context("invalid Goliath turret pose")?,
                    )?);
                }
            }
            clips.push(terran::directional(kind, &bases, 42));
        }
        *sprite = terran::compact_sprite(
            files,
            sprite.unit_type.0,
            &sprite.unit_name,
            "goliath-poses",
            &frames,
            clips,
        )?;
        Ok(())
    }

    pub(super) fn garrison(
        &mut self,
        archive: &mut SourceArchive,
        files: &mut Files,
        sprite: &mut SpriteManifest,
        flame: bool,
    ) -> Result<()> {
        // Source bunker image304 attack-overlay LO supplies eight firing ports.
        let name = terran_media::table_string(
            &self.tables.names,
            dword(&self.tables.images, 755 * 18 + 304 * 4),
        )?;
        let offsets = archive.read_file(&format!("unit\\{name}"), 1024 * 1024)?;
        ensure!(
            dword(&offsets, 0) > 0 && dword(&offsets, 4) == 8,
            "invalid bunker firing ports"
        );
        let start = dword(&offsets, 8) as usize;
        let vents = offsets
            .get(start..start + 16)
            .context("truncated bunker firing ports")?;
        let image = if flame { 421 } else { 306 };
        let mut frames = self.decode(archive, image)?.to_vec();
        let poses = if flame {
            timeline(&self.tables.scripts, self.tables.script(image), 0)
        } else {
            ensure!(frames.len() == 17, "invalid bunker flash directions");
            let blank = Image {
                width: frames[0].width,
                height: frames[0].height,
                rgba: vec![0; frames[0].rgba.len()],
            };
            frames.extend(std::iter::repeat_n(blank, 17));
            let mut visible = true;
            let mut poses = Vec::new();
            for instruction in instructions(&self.tables.scripts, 124, 0) {
                match instruction.op {
                    50 => visible = false,
                    51 => visible = true,
                    5 => poses.extend(std::iter::repeat_n(
                        if visible { 0 } else { 17 },
                        usize::from(instruction.args[0]),
                    )),
                    _ => {}
                }
            }
            poses
        };
        ensure!(!poses.is_empty(), "missing garrison firing animation");
        let mut clip = terran::directional(ClipKind::GarrisonAttack, &poses, 42);
        for (index, frame) in clip.frames.iter_mut().enumerate() {
            let direction = index % 32;
            let vent = (direction + 2) / 4 % 8;
            let heading = if flame {
                direction.div_ceil(2) % 16 * 2
            } else {
                vent * 4
            };
            let pose = poses[index / 32];
            frame.frame = pose
                + if heading > 16 {
                    (32 - heading) as u16
                } else {
                    heading as u16
                };
            frame.flip_x = heading > 16;
            frame.offset = [
                i16::from(vents[vent * 2] as i8),
                i16::from(vents[vent * 2 + 1] as i8),
            ];
        }
        let mut kept = Vec::new();
        let mut used = BTreeMap::new();
        for frame in &mut clip.frames {
            let next = kept.len() as u16;
            frame.frame = *used.entry(frame.frame).or_insert_with(|| {
                kept.push(frames[usize::from(frame.frame)].clone());
                next
            });
        }
        let mut extra = terran::compact_sprite(
            files,
            sprite.unit_type.0,
            &sprite.unit_name,
            if flame {
                "garrison-flame"
            } else {
                "garrison-flash"
            },
            &kept,
            vec![clip],
        )?;
        // compact_sprite resolves crop anchors. Add world-facing vent offsets
        // afterwards so neither cropping nor mirroring can move a firing port.
        for (index, frame) in extra.clips[0].frames.iter_mut().enumerate() {
            let vent = (index % 32 + 2) / 4 % 8;
            frame.offset[0] += i16::from(vents[vent * 2] as i8);
            frame.offset[1] += i16::from(vents[vent * 2 + 1] as i8);
        }
        sprite
            .clips
            .retain(|clip| clip.kind != ClipKind::GarrisonAttack);
        let mut remap = Vec::new();
        for reference in extra.frames {
            let index = sprite
                .frames
                .iter()
                .position(|frame| frame.file == reference.file)
                .unwrap_or_else(|| {
                    sprite.frames.push(reference.clone());
                    sprite.frames.len() - 1
                });
            sprite.frames[index] = reference;
            remap.push(index as u16);
        }
        for mut clip in extra.clips {
            for frame in &mut clip.frames {
                frame.frame = remap[usize::from(frame.frame)];
            }
            sprite.clips.push(clip);
        }
        Ok(())
    }
}
