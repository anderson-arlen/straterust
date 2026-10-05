//! Retail Hydralisk mouth emission, distinct from its on-target needle hit.
use super::*;

impl Graphics<'_> {
    pub(super) fn hydralisk_spit(
        &mut self,
        archive: &mut SourceArchive,
        files: &mut Files,
        sprite: &mut SpriteManifest,
    ) -> Result<()> {
        let body = self.tables.image(38);
        let attack = instructions(&self.tables.scripts, self.tables.script(body), 5);
        let child = attack
            .iter()
            .find(|i| i.op == 21)
            .context("missing Hydralisk attack emission")?;
        let pose = attack
            .iter()
            .take_while(|i| i.offset < child.offset)
            .filter(|i| i.op == 0)
            .last()
            .map(|i| word(&i.args, 0))
            .context("missing Hydralisk spitting pose")?;
        let path = format!(
            "unit\\{}",
            terran_media::table_string(
                &self.tables.names,
                dword(&self.tables.images, 755 * 18 + body * 4)
            )?
        );
        let lo = archive.read_file(&path, 1024 * 1024)?;
        let count = dword(&lo, 0) as usize;
        let points = dword(&lo, 4) as usize;
        let slot = usize::from(child.args[2]);
        ensure!(
            count <= 4096
                && usize::from(pose) + 17 <= count
                && points > slot
                && points <= 256
                && lo.len() >= 8 + count * 4,
            "invalid Hydralisk attack attachments"
        );
        let image = self.tables.sprite_image(word(&child.args, 0));
        let sequence = timeline(&self.tables.scripts, self.tables.script(image), 0);
        let directional = self.tables.images[755 * 4 + image] != 0;
        let decoded = self.decode(archive, image)?;
        let mut frames = Vec::new();
        let mut bases = Vec::new();
        let mut cache = BTreeMap::new();
        for pose in sequence {
            let next = frames.len() as u16;
            let base = *cache.entry(pose).or_insert(next);
            bases.push(base);
            if base == next {
                frames.extend_from_slice(
                    decoded
                        .get(
                            usize::from(pose)..usize::from(pose) + if directional { 17 } else { 1 },
                        )
                        .context("invalid Hydralisk spit pose")?,
                );
            }
        }
        ensure!(!bases.is_empty(), "empty Hydralisk spit animation");
        let clip = if directional {
            terran::directional(ClipKind::AttackEffect, &bases, 42)
        } else {
            terran::single_direction(ClipKind::AttackEffect, &bases, 42)
        };
        let mut extra = terran::compact_sprite(
            files,
            sprite.unit_type.0,
            &sprite.unit_name,
            "hydralisk-spit",
            &frames,
            vec![clip],
        )?;
        let clip = &mut extra.clips[0];
        if clip.directions == 1 {
            clip.frames = clip
                .frames
                .iter()
                .flat_map(|frame| std::iter::repeat_n(*frame, 32))
                .collect();
            clip.directions = 32;
        }
        for (index, frame) in clip.frames.iter_mut().enumerate() {
            let heading = index % 32;
            let mirrored = heading > 16;
            let heading = if mirrored { 32 - heading } else { heading };
            let entry = dword(&lo, 8 + (usize::from(pose) + heading) * 4) as usize + slot * 2;
            let point = lo
                .get(entry..entry + 2)
                .context("truncated Hydralisk mouth point")?;
            frame.offset[0] += i16::from(point[0] as i8) * if mirrored { -1 } else { 1 };
            frame.offset[1] += i16::from(point[1] as i8);
            frame.frame += sprite.frames.len() as u16;
        }
        sprite.frames.extend(extra.frames);
        sprite.clips.retain(|c| c.kind != ClipKind::AttackEffect);
        sprite.clips.extend(extra.clips);
        if let Some(clip) = sprite.clips.iter_mut().find(|c| c.kind == ClipKind::Attack) {
            clip.key_steps = vec![0];
        }
        Ok(())
    }
}
