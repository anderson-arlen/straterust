//! Source Goliath poses and bunker firing ports; no runtime script execution.
use super::*;

impl Graphics<'_> {
    pub(super) fn larva_walk(
        &mut self,
        archive: &mut SourceArchive,
        files: &mut Files,
        sprite: &mut SpriteManifest,
    ) -> Result<()> {
        let image = self.tables.image(35);
        // The script sets its final pose immediately before jumping back to a
        // wait. A linear wait extractor loses that fifth repeating pose.
        let poses: Vec<_> = instructions(&self.tables.scripts, self.tables.script(image), 11)
            .iter()
            .filter(|i| matches!(i.op, 0 | 1))
            .map(|i| word(&i.args, 0))
            .collect();
        ensure!(
            poses == [0, 17, 34, 51, 68],
            "unsupported Larva walk sequence"
        );
        let body = self.decode(archive, image)?;
        let mut frames = Vec::new();
        let mut bases = Vec::new();
        for pose in poses {
            bases.push(frames.len() as u16);
            frames.extend_from_slice(
                body.get(usize::from(pose)..usize::from(pose) + 17)
                    .context("missing Larva walking directions")?,
            );
        }
        let extra = terran::compact_sprite(
            files,
            sprite.unit_type.0,
            &sprite.unit_name,
            "larva-walk",
            &frames,
            vec![terran::directional(ClipKind::Walk, &bases, 42)],
        )?;
        sprite.clips.retain(|c| c.kind != ClipKind::Walk);
        let first = sprite.frames.len() as u16;
        sprite.frames.extend(extra.frames);
        for mut clip in extra.clips {
            for frame in &mut clip.frames {
                frame.frame += first;
            }
            sprite.clips.push(clip);
        }
        Ok(())
    }

    pub(super) fn drone_work(
        &mut self,
        archive: &mut SourceArchive,
        files: &mut Files,
        sprite: &mut SpriteManifest,
    ) -> Result<()> {
        let image = self.tables.image(41);
        let poses = timeline(&self.tables.scripts, self.tables.script(image), 15);
        ensure!(!poses.is_empty(), "missing Drone working animation");
        let decoded = self.decode(archive, image)?;
        let mut frames = Vec::new();
        let mut bases = Vec::new();
        let mut cache = BTreeMap::new();
        for pose in poses {
            let next = frames.len() as u16;
            let base = *cache.entry(pose).or_insert(next);
            bases.push(base);
            if base == next {
                frames.extend_from_slice(
                    decoded
                        .get(usize::from(pose)..usize::from(pose) + 17)
                        .context("invalid Drone work directions")?,
                );
            }
        }
        let extra = terran::compact_sprite(
            files,
            sprite.unit_type.0,
            &sprite.unit_name,
            "drone-work",
            &frames,
            vec![terran::directional(ClipKind::Work, &bases, 42)],
        )?;
        let first = sprite.frames.len() as u16;
        sprite.frames.extend(extra.frames);
        for mut clip in extra.clips {
            for frame in &mut clip.frames {
                frame.frame += first;
            }
            sprite.clips.push(clip);
        }
        Ok(())
    }

    pub(super) fn tank_attack(
        &mut self,
        archive: &mut SourceArchive,
        files: &mut Files,
        sprite: &mut SpriteManifest,
        source: u16,
    ) -> Result<()> {
        let image = self.tables.image(source);
        let turret_image = self
            .tables
            .image(word(&self.tables.units, 228 + usize::from(source) * 2));
        let idle = timeline(&self.tables.scripts, self.tables.script(image), 0)
            .first()
            .copied()
            .unwrap_or(0);
        let mut sequence = Vec::new();
        let mut pose = 0;
        let mut flash = None;
        for instruction in instructions(&self.tables.scripts, self.tables.script(turret_image), 5) {
            match instruction.op {
                0 | 1 => pose = word(&instruction.args, 0),
                8 | 13 => {
                    let image = usize::from(word(&instruction.args, 0));
                    let poses = timeline(&self.tables.scripts, self.tables.script(image), 0);
                    flash = Some((image, poses, sequence.len()));
                }
                5 | 6 => {
                    let wait = if instruction.op == 5 {
                        u16::from(instruction.args[0])
                    } else {
                        (u16::from(instruction.args[0]) + u16::from(instruction.args[1])) / 2
                    };
                    for _ in 0..wait.clamp(1, 24) {
                        let overlay = flash.as_ref().and_then(|(image, frames, start)| {
                            frames
                                .get(sequence.len() - start)
                                .map(|frame| (*image, *frame))
                        });
                        sequence.push((pose, overlay));
                    }
                }
                42 | 48 => break,
                _ => {}
            }
        }
        ensure!(!sequence.is_empty(), "missing tank firing instructions");
        let directional_body = self.tables.images[755 * 4 + image] != 0;
        let body = self.decode(archive, image)?.to_vec();
        let turret = self.decode(archive, turret_image)?.to_vec();
        let mut cache = BTreeMap::new();
        let mut frames = Vec::new();
        let mut bases = Vec::new();
        for (pose, flash) in sequence {
            let next = frames.len() as u16;
            let base = *cache.entry((pose, flash)).or_insert_with(|| next);
            bases.push(base);
            if base != next {
                continue;
            }
            for heading in 0..17 {
                let body = body
                    .get(usize::from(idle) + if directional_body { heading } else { 0 })
                    .context("invalid tank body pose")?;
                let top = turret
                    .get(usize::from(pose) + heading)
                    .context("invalid tank turret pose")?;
                let flash = if let Some((image, pose)) = flash {
                    let directional = self.tables.images[755 * 4 + image] != 0;
                    Some(
                        self.decode(archive, image)?
                            .get(usize::from(pose) + if directional { heading } else { 0 })
                            .context("invalid tank flash pose")?
                            .clone(),
                    )
                } else {
                    None
                };
                let width = body
                    .width
                    .max(top.width)
                    .max(flash.as_ref().map_or(0, |f| f.width));
                let height = body
                    .height
                    .max(top.height)
                    .max(flash.as_ref().map_or(0, |f| f.height));
                let composite = terran::composite(
                    &terran::center_canvas(body, width, height)?,
                    &terran::center_canvas(top, width, height)?,
                )?;
                frames.push(if let Some(flash) = flash {
                    terran::composite(&composite, &terran::center_canvas(&flash, width, height)?)?
                } else {
                    composite
                });
            }
        }
        let extra = terran::compact_sprite(
            files,
            sprite.unit_type.0,
            &sprite.unit_name,
            &format!("tank-{source}-attack"),
            &frames,
            vec![terran::directional(ClipKind::Attack, &bases, 42)],
        )?;
        sprite.clips.retain(|c| c.kind != ClipKind::Attack);
        let mut kept = Vec::new();
        let mut remap = BTreeMap::new();
        for frame in sprite.clips.iter_mut().flat_map(|c| &mut c.frames) {
            let next = kept.len() as u16;
            frame.frame = *remap.entry(frame.frame).or_insert_with(|| {
                kept.push(sprite.frames[usize::from(frame.frame)].clone());
                next
            });
        }
        sprite.frames = kept;
        let offset = sprite.frames.len() as u16;
        sprite.frames.extend(extra.frames);
        for mut clip in extra.clips {
            for frame in &mut clip.frames {
                frame.frame += offset;
            }
            sprite.clips.push(clip);
        }
        Ok(())
    }

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
