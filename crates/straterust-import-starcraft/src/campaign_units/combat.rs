//! Weapon permissions, sounds and child destruction artwork from retail DAT/IScript.
use super::iscript::{instructions, sounds, timeline};
use super::*;
use straterust_engine::assets::{ProjectileManifest, ProjectileTrailManifest, SpriteManifest};
mod aircraft;
mod emissions;
#[cfg(test)]
mod tests;
mod units;

type SourceArchive = Archive<std::io::Cursor<Vec<u8>>>;
struct Tables {
    units: Vec<u8>,
    weapons: Vec<u8>,
    flingy: Vec<u8>,
    sprites: Vec<u8>,
    images: Vec<u8>,
    names: Vec<u8>,
    scripts: Vec<u8>,
}
impl Tables {
    fn read(archive: &mut SourceArchive) -> Result<Self> {
        Ok(Self {
            units: archive.read_file("arr\\units.dat", 19192)?,
            weapons: archive.read_file("arr\\weapons.dat", 4200)?,
            flingy: archive.read_file("arr\\flingy.dat", 2760)?,
            sprites: archive.read_file("arr\\sprites.dat", 2081)?,
            images: archive.read_file("arr\\images.dat", 28690)?,
            names: archive.read_file("arr\\images.tbl", 65536)?,
            scripts: archive.read_file("scripts\\iscript.bin", 65536)?,
        })
    }
    fn image(&self, source: u16) -> usize {
        self.flingy_image(usize::from(self.units[usize::from(source)]))
    }
    fn flingy_image(&self, flingy: usize) -> usize {
        self.sprite_image(word(&self.flingy, flingy * 2))
    }
    fn sprite_image(&self, sprite: u16) -> usize {
        usize::from(word(&self.sprites, usize::from(sprite) * 2))
    }
    fn script(&self, image: usize) -> u16 {
        dword(&self.images, 755 * 10 + image * 4) as u16
    }
    fn weapons(&self, source: u16) -> [u8; 2] {
        let n = usize::from(source);
        let n = if word(&self.units, 228 + n * 2) < 228 {
            usize::from(word(&self.units, 228 + n * 2))
        } else {
            n
        };
        [self.units[0x1704 + n], self.units[0x17e8 + n]]
    }
    fn weapon_image(&self, weapon: u8) -> Option<usize> {
        let flingy = dword(&self.weapons, 200 + usize::from(weapon) * 4) as usize;
        // weapons.dat uses zero for no projectile. Flingy 0 itself is Scourge,
        // so resolving the sentinel would make melee attacks launch Scourge.
        (flingy != 0).then(|| self.flingy_image(flingy))
    }
    fn weapon(&self, weapon: u8) -> Weapon {
        let w = usize::from(weapon);
        let splash = [0x898, 0x960, 0xa28].map(|p| u32::from(word(&self.weapons, p + w * 2)));
        Weapon {
            damage: u32::from(word(&self.weapons, 0xaf0 + w * 2))
                * u32::from(self.weapons[0xce4 + w].max(1)),
            range: dword(&self.weapons, 0x514 + w * 4),
            cooldown: u32::from(self.weapons[0xc80 + w].max(1)),
            targets_air: true,
            cooldown_jitter: (self.weapons[0xc80 + w] > 1).then_some([-1, 2]),
            damage_kind: match self.weapons[0x708 + w] {
                1 => DamageKind::Explosive,
                2 => DamageKind::Concussive,
                _ => DamageKind::Normal,
            },
            splash: (splash[0] > 0).then_some(splash),
            strikes: Vec::new(),
        }
    }
}
pub(crate) fn apply_combat_rules(archive: &mut SourceArchive, rules: &mut Rules) -> Result<()> {
    let tables = Tables::read(archive)?;
    let tech = archive.read_file("arr\\techdata.dat", 432)?;
    ensure!(tech.len() == 432, "unsupported retail technology layout");
    for &(source, native) in MAPPING {
        let Some(unit) = rules.units.iter_mut().find(|u| u.id == UnitTypeId(native)) else {
            continue;
        };
        let [ground, air] = tables.weapons(source);
        unit.attacks_ground = ground < 100;
        if let Some(weapon) = &mut unit.weapon {
            weapon.targets_air = air < 100;
        }
        unit.air_weapon = (air < 100 && ground < 100 && air != ground).then(|| tables.weapon(air));
        let flags = dword(&tables.units, 0x19b0 + usize::from(source) * 4);
        unit.resource_clearance = if flags & 0x1000 != 0 { 96 } else { 0 };
        unit.detector_range = if flags & 0x8000 != 0 {
            unit.vision_range
        } else {
            0
        };
        // Hero Kerrigan can use Personnel Cloaking without researching it.
        if source == 16 {
            unit.cloak = Some(Cloak {
                energy_max: 250,
                activation_cost: u32::from(word(&tech, 24 * 6 + 10 * 2)),
                regeneration: 8,
                drain: 10,
                ..Cloak::default()
            });
        }
        if source == 8 {
            unit.cloak = Some(Cloak {
                energy_max: 200,
                activation_cost: u32::from(word(&tech, 24 * 6 + 9 * 2)),
                regeneration: 8,
                drain: 10,
                ..Cloak::default()
            });
        }
    }
    Ok(())
}

struct Graphics<'a> {
    tables: &'a Tables,
    palette: [[u8; 4]; 256],
    remaps: BTreeMap<u8, [[u8; 4]; 256]>,
    cache: BTreeMap<usize, Vec<Image>>,
}
impl Graphics<'_> {
    fn shadow(
        &mut self,
        archive: &mut SourceArchive,
        files: &mut Files,
        sprite: &mut SpriteManifest,
        source: u16,
    ) -> Result<()> {
        let image = self.tables.image(source);
        let Some(child) = instructions(&self.tables.scripts, self.tables.script(image), 0)
            .into_iter()
            .find(|instruction| {
                instruction.op == 9
                    && self.tables.images[755 * 8 + usize::from(word(&instruction.args, 0))] == 10
            })
        else {
            return Ok(());
        };
        let image = usize::from(word(&child.args, 0));
        let path = format!(
            "unit\\{}",
            terran_media::table_string(&self.tables.names, dword(&self.tables.images, image * 4))?
        );
        let frames = formats::decode_grp(
            &archive.read_file(&path, 8 * 1024 * 1024)?,
            &[[0, 0, 0, 100]; 256],
        )?;
        let directional = self.tables.images[755 * 4 + image] != 0;
        let count = if directional { 17 } else { 1 };
        ensure!(
            frames.len() >= count,
            "incomplete aircraft shadow directions"
        );
        let displacement = [
            i32::from(child.args[2] as i8),
            i32::from(child.args[3] as i8),
        ];
        // Refresh replaces the clip; reuse matching frame files on subsequent updates.
        let mut native = Vec::new();
        for (index, frame) in frames.iter().take(count).enumerate() {
            let reference =
                crate::add_image(files, &format!("air-shadow-{source}-{index}.srim"), frame)?;
            let position = sprite
                .frames
                .iter()
                .position(|existing| existing.file == reference.file)
                .unwrap_or_else(|| {
                    sprite.frames.push(reference);
                    sprite.frames.len() - 1
                });
            native.push(position as u16);
        }
        let directions = if directional { 32 } else { 1 };
        let frames = (0..directions)
            .map(|direction| {
                let pose = if direction > 16 {
                    32 - direction
                } else {
                    direction
                };
                let image = &frames[pose];
                let flip_x = direction > 16;
                // Sampling mirrors the body anchor. Compensate the shadow's
                // canvas origin too, while keeping its ground displacement fixed.
                let anchor_x = if flip_x {
                    image.width as i32 - sprite.anchor[0]
                } else {
                    sprite.anchor[0]
                };
                ClipFrame {
                    frame: native[pose],
                    flip_x,
                    offset: [
                        (anchor_x - image.width as i32 / 2 + displacement[0]) as i16,
                        (sprite.anchor[1] - image.height as i32 / 2 + displacement[1]) as i16,
                    ],
                }
            })
            .collect();
        sprite.clips.retain(|clip| clip.kind != ClipKind::Shadow);
        sprite.clips.push(SpriteClip {
            key_steps: Vec::new(),
            kind: ClipKind::Shadow,
            frame_ms: 42,
            directions: directions as u8,
            frames,
            loop_start: None,
            progress_starts: vec![],
        });
        Ok(())
    }
    fn decode(&mut self, archive: &mut SourceArchive, image: usize) -> Result<&[Image]> {
        if !self.cache.contains_key(&image) {
            let path = format!(
                "unit\\{}",
                terran_media::table_string(
                    &self.tables.names,
                    dword(&self.tables.images, image * 4)
                )?
            );
            let palette = if self.tables.images[755 * 8 + image] == 9 {
                self.remaps
                    .get(&self.tables.images[755 * 9 + image])
                    .unwrap_or(&self.palette)
            } else {
                &self.palette
            };
            let frames = formats::decode_grp(&archive.read_file(&path, 8 * 1024 * 1024)?, palette)?;
            self.cache.insert(image, frames);
        }
        Ok(&self.cache[&image])
    }
    fn effect(
        &mut self,
        archive: &mut SourceArchive,
        files: &mut Files,
        image: usize,
        animation: usize,
        directions: usize,
    ) -> Result<EffectManifest> {
        let mut sequence = timeline(&self.tables.scripts, self.tables.script(image), animation);
        let decoded = self.decode(archive, image)?;
        sequence.retain(|&frame| usize::from(frame) + directions <= decoded.len());
        if sequence.is_empty() {
            sequence.push(0);
        }
        let mut poses = BTreeMap::new();
        let mut used = Vec::new();
        for frame in &mut sequence {
            let next = used.len() as u16;
            let original = *frame;
            *frame = *poses.entry(original).or_insert_with(|| {
                used.extend(
                    decoded[usize::from(original)..usize::from(original) + directions]
                        .iter()
                        .cloned(),
                );
                next
            });
        }
        let cropped = used.iter().map(terran::crop).collect::<Result<Vec<_>>>()?;
        let left = cropped
            .iter()
            .map(|(_, offset)| i32::from(offset[0]))
            .min()
            .unwrap()
            .min(0);
        let top = cropped
            .iter()
            .map(|(_, offset)| i32::from(offset[1]))
            .min()
            .unwrap()
            .min(0);
        let right = cropped
            .iter()
            .map(|(frame, offset)| i32::from(offset[0]) + frame.width as i32)
            .max()
            .unwrap()
            .max(1);
        let bottom = cropped
            .iter()
            .map(|(frame, offset)| i32::from(offset[1]) + frame.height as i32)
            .max()
            .unwrap()
            .max(1);
        let width = (right - left) as u32;
        let height = (bottom - top) as u32;
        let frames = cropped
            .into_iter()
            .enumerate()
            .map(|(index, (source, offset))| {
                let mut frame = Image {
                    width,
                    height,
                    rgba: vec![0; (width * height * 4) as usize],
                };
                for row in 0..source.height {
                    let start = ((i32::from(offset[1]) - top + row as i32) as u32 * width
                        + (i32::from(offset[0]) - left) as u32)
                        as usize
                        * 4;
                    frame.rgba[start..start + source.width as usize * 4].copy_from_slice(
                        &source.rgba[(row * source.width * 4) as usize
                            ..((row + 1) * source.width * 4) as usize],
                    );
                }
                crate::add_image(
                    files,
                    &format!("combat-image-{image}-{animation}-{directions}-{index:03}.srim"),
                    &frame,
                )
            })
            .collect::<Result<_>>()?;
        Ok(EffectManifest {
            frame_ms: 42,
            anchor: [-left, -top],
            sequence,
            frames,
        })
    }
    fn projectile_trail(
        &mut self,
        archive: &mut SourceArchive,
        files: &mut Files,
        image: usize,
    ) -> Result<Option<ProjectileTrailManifest>> {
        let init = instructions(&self.tables.scripts, self.tables.script(image), 0);
        let Some((index, spawn)) = init.iter().enumerate().find(|(_, i)| i.op == 15) else {
            return Ok(None);
        };
        // Repeated sprite overlays at the bullet position become stationary
        // trail effects. Other attachments and one-time children are retained
        // by their existing import paths.
        let Some(jump) = init[index + 1..]
            .iter()
            .position(|i| i.op == 7 && usize::from(word(&i.args, 0)) == spawn.offset)
        else {
            return Ok(None);
        };
        if spawn.args[2..] != [0, 0] {
            return Ok(None);
        }
        let waits = |instructions: &[super::iscript::Instruction]| {
            instructions
                .iter()
                .filter(|i| i.op == 5)
                .map(|i| u32::from(i.args[0]))
                .sum::<u32>()
        };
        let interval = waits(&init[index + 1..index + 1 + jump]);
        if interval == 0 {
            return Ok(None);
        }
        let child = self.tables.sprite_image(word(&spawn.args, 0));
        let mut effect = self.effect(archive, files, child, 0, 1)?;
        // Missile smoke's source Init hides its graphic for three ticks before
        // its eight visible poses. Keep that delay in the native frame sequence.
        let child_init = instructions(&self.tables.scripts, self.tables.script(child), 0);
        let hidden = child_init
            .iter()
            .position(|i| i.op == 50)
            .and_then(|start| {
                child_init[start + 1..]
                    .iter()
                    .position(|i| i.op == 51)
                    .map(|end| waits(&child_init[start + 1..start + 1 + end]))
            });
        if let Some(hidden) = hidden.filter(|&ticks| ticks > 0) {
            let blank = Image {
                width: (effect.anchor[0] + 1) as u32,
                height: (effect.anchor[1] + 1) as u32,
                rgba: vec![0; ((effect.anchor[0] + 1) * (effect.anchor[1] + 1) * 4) as usize],
            };
            let frame = effect.frames.len() as u16;
            effect.frames.push(crate::add_image(
                files,
                &format!("combat-image-{child}-hidden.srim"),
                &blank,
            )?);
            effect
                .sequence
                .splice(0..0, std::iter::repeat_n(frame, hidden.min(256) as usize));
        }
        Ok(Some(ProjectileTrailManifest {
            start_ms: waits(&init[..index]) * 42,
            interval_ms: interval * 42,
            effect,
        }))
    }

    fn death(
        &mut self,
        archive: &mut SourceArchive,
        files: &mut Files,
        sprite: &mut SpriteManifest,
        source: u16,
    ) -> Result<()> {
        let image = self.tables.image(source);
        let mut children = Vec::new();
        for instruction in instructions(&self.tables.scripts, self.tables.script(image), 1) {
            let child = match instruction.op {
                8..=10 => Some((usize::from(word(&instruction.args, 0)), false)),
                15..=17 | 19..=21 => {
                    Some((self.tables.sprite_image(word(&instruction.args, 0)), true))
                }
                _ => None,
            };
            if let Some(child) = child {
                children.push(child);
            }
        }
        if children.is_empty() {
            // setfldirect(0) selects directionless frames in an otherwise
            // directional body GRP (for example the Zealot's energy death).
            // Those tail frames have no seventeen-heading block to retain.
            if instructions(&self.tables.scripts, self.tables.script(image), 1)
                .iter()
                .any(|i| i.op == 52 && i.args == [0])
            {
                let sequence = timeline(&self.tables.scripts, self.tables.script(image), 1);
                let body = self.decode(archive, image)?;
                let frames = sequence
                    .into_iter()
                    .map(|pose| {
                        body.get(usize::from(pose))
                            .cloned()
                            .context("invalid directionless death frame")
                    })
                    .collect::<Result<Vec<_>>>()?;
                if !frames.is_empty() {
                    super::buildings::replace_clip(
                        files,
                        sprite,
                        source,
                        ClipKind::Death,
                        "death",
                        &frames,
                    )?;
                }
            }
            return Ok(());
        }
        let directions = if children
            .iter()
            .any(|(image, _)| self.tables.images[755 * 4 + image] != 0)
        {
            32
        } else {
            1
        };
        // Remove the previous death-only references before rebuilding: a refresh
        // must not accumulate duplicate frames or grow the package each time.
        sprite.clips.retain(|clip| clip.kind != ClipKind::Death);
        // Revision 17 normalized the SCV's 128px canvas with the unmirrored
        // sign for both halves. Repair those published clips before refresh.
        if source == 7 && sprite.anchor == [0, 0] {
            for frame in sprite.clips.iter_mut().flat_map(|clip| &mut clip.frames) {
                if frame.flip_x
                    && frame.offset[0] < 0
                    && sprite.frames[usize::from(frame.frame)]
                        .file
                        .starts_with("scv-")
                {
                    frame.offset[0] += 128;
                }
            }
        }
        let old_anchor = sprite.anchor;
        sprite.anchor = [0, 0];
        let mut remap = BTreeMap::new();
        let mut frames = Vec::new();
        for clip in &mut sprite.clips {
            for frame in &mut clip.frames {
                frame.offset[0] += if frame.flip_x {
                    old_anchor[0] as i16
                } else {
                    -old_anchor[0] as i16
                };
                frame.offset[1] -= old_anchor[1] as i16;
                frame.frame = *remap.entry(frame.frame).or_insert_with(|| {
                    let index = frames.len() as u16;
                    frames.push(sprite.frames[usize::from(frame.frame)].clone());
                    index
                });
            }
        }
        sprite.frames = frames;
        let mut death_frames = Vec::new();
        for (image, is_rubble) in children {
            let sequence = timeline(&self.tables.scripts, self.tables.script(image), 0);
            if sequence.is_empty() {
                continue;
            }
            let turns = self.tables.images[755 * 4 + image] != 0;
            let decoded = self.decode(archive, image)?;
            let mut remap = BTreeMap::new();
            // Rubble is deliberately shortened, as in existing imported corpses.
            let sequence = if is_rubble {
                (0..decoded.len() as u16)
                    .flat_map(|n| std::iter::repeat_n(n, 24))
                    .collect()
            } else {
                sequence
            };
            for pose in sequence {
                for direction in 0..u16::from(directions) {
                    let frame = pose
                        + if turns {
                            if direction <= 16 {
                                direction
                            } else {
                                32 - direction
                            }
                        } else {
                            0
                        };
                    let Some(body) = decoded.get(usize::from(frame)) else {
                        continue;
                    };
                    let (native, offset) = if let Some(&entry) = remap.get(&frame) {
                        entry
                    } else {
                        let (body, offset) = terran::crop(body)?;
                        let native = sprite.frames.len() as u16;
                        sprite.frames.push(crate::add_image(
                            files,
                            &format!("combat-image-{image}-{frame:03}.srim"),
                            &body,
                        )?);
                        remap.insert(frame, (native, offset));
                        (native, offset)
                    };
                    let flip_x = turns && direction > 16;
                    death_frames.push(ClipFrame {
                        frame: native,
                        flip_x,
                        offset: [if flip_x { -offset[0] } else { offset[0] }, offset[1]],
                    });
                }
            }
        }
        if !death_frames.is_empty() {
            sprite.clips.retain(|clip| clip.kind != ClipKind::Death);
            sprite.clips.push(SpriteClip {
                key_steps: Vec::new(),
                kind: ClipKind::Death,
                frame_ms: 42,
                directions,
                frames: death_frames,
                loop_start: None,
                progress_starts: vec![],
            });
        }
        Ok(())
    }
}

pub(crate) fn refresh_combat(
    archive: &mut SourceArchive,
    files: &mut Files,
    assets: &mut AssetManifest,
    rules: &Rules,
) -> Result<()> {
    let tables = Tables::read(archive)?;
    let palette = formats::palette(&archive.read_file("tileset\\badlands.wpe", 1024)?)?;
    let mut remaps = BTreeMap::new();
    for (id, name) in [(1, "ofire"), (2, "gfire"), (3, "bfire"), (4, "bexpl")] {
        remaps.insert(
            id,
            terran::fire_palette(
                &archive.read_file(&format!("tileset\\badlands\\{name}.pcx"), 8 * 1024 * 1024)?,
                &palette,
            )?,
        );
    }
    let mut graphics = Graphics {
        tables: &tables,
        palette,
        remaps,
        cache: BTreeMap::new(),
    };
    for &(source, native) in MAPPING {
        let Some(unit) = rules.units.iter().find(|u| u.id == UnitTypeId(native)) else {
            continue;
        };
        // Existing calibrated Marine/SCV/building clips are retained when present.
        if let Some(sprite) = assets
            .extra_units
            .iter_mut()
            .find(|s| s.unit_type == UnitTypeId(native))
        {
            if source == 35 {
                graphics.larva_walk(archive, files, sprite)?;
            }
            if source == 3 {
                graphics.goliath(archive, files, sprite)?;
            }
            if matches!(source, 5 | 23 | 30) {
                graphics.tank_attack(archive, files, sprite, source)?;
            }
            if matches!(source, 8 | 11 | 12 | 29 | 69 | 70) {
                graphics.engines(archive, files, sprite, source)?;
            }
            if source == 41 {
                sprite.clips.retain(|c| c.kind != ClipKind::Work);
            }
            if source == 38 {
                sprite.clips.retain(|c| c.kind != ClipKind::AttackEffect);
            }
            graphics.death(archive, files, sprite, source)?;
            if source == 41 {
                graphics.drone_work(archive, files, sprite)?;
            }
            if source == 38 {
                graphics.hydralisk_spit(archive, files, sprite)?;
            }
            if matches!(source, 32 | 125) {
                graphics.garrison(archive, files, sprite, source == 32)?;
            }
            if unit.movement_class == MovementClass::Air {
                graphics.shadow(archive, files, sprite, source)?;
            }
        }
        // Native flight and target-hit art for the previously absent Wraith,
        // Ghost/Kerrigan and Missile Turret weapons. Grenades retain their arc.
        if !matches!(source, 1 | 3 | 8 | 16 | 124 | 38 | 43 | 146) && native < 54 {
            continue;
        }
        assets
            .projectiles
            .retain(|p| p.unit_type != UnitTypeId(native));
        let weapons = tables.weapons(source);
        for (air, weapon) in weapons.into_iter().enumerate().filter(|(_, w)| *w < 100) {
            let Some(image) = tables.weapon_image(weapon) else {
                continue;
            };
            let script = tables.script(image);
            let on_target = tables.weapons[0x76c + usize::from(weapon)] == 2;
            let impact_image = instructions(&tables.scripts, script, 1)
                .into_iter()
                .find(|i| matches!(i.op, 8..=10))
                .map_or(image, |i| usize::from(word(&i.args, 0)));
            let directional = tables.images[755 * 4 + image] != 0;
            let flight =
                graphics.effect(archive, files, image, 0, if directional { 17 } else { 1 })?;
            let impact = graphics.effect(
                archive,
                files,
                impact_image,
                if impact_image == image { 1 } else { 0 },
                1,
            )?;
            let flingy = dword(&tables.weapons, 200 + usize::from(weapon) * 4) as usize;
            assets.projectiles.push(ProjectileManifest {
                unit_type: UnitTypeId(native),
                targets_air: air == 1,
                directional,
                speed_fp8: dword(&tables.flingy, 368 + flingy * 4).clamp(256, 256 * 1024),
                forward_offset: u32::from(tables.weapons[0xe10 + usize::from(weapon)]),
                arc_height: 0,
                on_target,
                flight,
                impact,
                trail: graphics.projectile_trail(archive, files, image)?,
            });
        }
    }
    refresh_audio(archive, files, rules, &tables)?;
    add_cloak_icons(archive, files, assets)?;
    crate::burrow::refresh(archive, files, assets, rules, &mut Vec::new())?;
    files.insert("combat-reference.ron".into(), ron_bytes(&(
        "Retail units.dat ground/air weapon fields and subunit weapons audited for every selected role. Distinct Wraith/Goliath air profiles retained; Vulture/Raynor ground only. Weapon graphics follow weapons.dat flingy -> sprites.dat image -> images.dat IScript. Sounds follow attack body and bullet Init instructions, death body instructions.",
        "Death child images and sprite rubble follow source Death instructions with timed poses and original fire transparency. Static conditional branches use fallthrough; RNG/attachment timing and projectile damage arrival remain uncalibrated; rubble lifetime shortened.",
        "Hero Kerrigan: Personnel Cloaking tech10 activation25; max energy250, regeneration8/256, cloak drain10/256 per tick without regeneration. Mission UNIT energy property retained. Detectors use source flag and sight range."))?);
    Ok(())
}
fn refresh_audio(
    archive: &mut SourceArchive,
    files: &mut Files,
    rules: &Rules,
    tables: &Tables,
) -> Result<()> {
    let Some(bytes) = files.get("media.ron") else {
        return Ok(());
    };
    let mut media: MediaManifest = ron::de::from_bytes(bytes)?;
    let sfx = archive.read_file("arr\\sfxdata.dat", 8712)?;
    let names = archive.read_file("arr\\sfxdata.tbl", 65536)?;
    let mut cache = BTreeMap::<u16, straterust_engine::media::AudioRef>::new();
    let mut cumulative = 0;
    for &(source, native) in MAPPING {
        let Some(unit) = rules.units.iter().find(|u| u.id == UnitTypeId(native)) else {
            continue;
        };
        let image = tables.image(source);
        let body_script = tables.script(image);
        let attack_script = if matches!(source, 3 | 5 | 23 | 30) {
            tables.script(tables.image(word(&tables.units, 228 + usize::from(source) * 2)))
        } else {
            body_script
        };
        for (cue, air) in [
            (AudioCue::Work, false),
            (AudioCue::Attack, false),
            (AudioCue::AttackAir, true),
            (AudioCue::Death, false),
        ] {
            let mut sound_ids = if cue == AudioCue::Work {
                if unit.worker.is_none() {
                    continue;
                }
                // Original mineral mining invokes AlmostBuilt. Drone's loop
                // plays sfx847; existing SCV/Probe work mappings remain intact
                // when their sound is emitted by a separate work effect.
                sounds(&tables.scripts, body_script, 15)
            } else if cue == AudioCue::Death {
                sounds(&tables.scripts, body_script, 1)
            } else {
                let weapon = tables.weapons(source)[usize::from(air)];
                if weapon >= 100 || unit.weapon.is_none() {
                    continue;
                }
                let mut ids = sounds(&tables.scripts, attack_script, if air { 3 } else { 2 });
                ids.extend(sounds(
                    &tables.scripts,
                    attack_script,
                    if air { 6 } else { 5 },
                ));
                if let Some(image) = tables.weapon_image(weapon) {
                    ids.extend(sounds(&tables.scripts, tables.script(image), 0));
                }
                ids
            };
            if cue == AudioCue::Death {
                for child in instructions(&tables.scripts, body_script, 1) {
                    let child_image = match child.op {
                        8..=10 => Some(usize::from(word(&child.args, 0))),
                        15..=17 | 19..=21 => Some(tables.sprite_image(word(&child.args, 0))),
                        _ => None,
                    };
                    if let Some(image) = child_image {
                        sound_ids.extend(sounds(&tables.scripts, tables.script(image), 0));
                    }
                }
            }
            sound_ids.remove(&0);
            if sound_ids.is_empty() {
                continue;
            }
            let mut variants = Vec::new();
            for sound in sound_ids {
                let reference = if let Some(reference) = cache.get(&sound) {
                    reference.clone()
                } else {
                    let path = terran_media::sound_path(&sfx, &names, sound)?;
                    let bytes = terran_media::normalize_wav(
                        &archive.read_file(&path, 4 * 1024 * 1024)?,
                        120000,
                        &mut cumulative,
                    )?;
                    let reference =
                        terran_media::audio_file(files, format!("sound-{sound:03}.wav"), bytes);
                    cache.insert(sound, reference.clone());
                    reference
                };
                variants.push(reference);
            }
            media
                .audio
                .retain(|m| !(m.cue == cue && m.unit_type == Some(UnitTypeId(native))));
            media.audio.push(AudioMapping {
                cue,
                unit_type: Some(UnitTypeId(native)),
                voice: false,
                variants,
            });
        }
    }
    files.insert("media.ron".into(), ron_bytes(&media)?);
    Ok(())
}
fn add_cloak_icons(
    archive: &mut SourceArchive,
    files: &mut Files,
    assets: &mut AssetManifest,
) -> Result<()> {
    // Retail cloaking icon252, cross-checked with Stargus terran/icons.lua.
    let colors = formats::decode_pcx(&archive.read_file("unit\\cmdbtns\\ticon.pcx", 1024 * 1024)?)?;
    let icons = formats::decode_grp(
        &archive.read_file("unit\\cmdbtns\\cmdicons.grp", 1024 * 1024)?,
        &crate::terran_ui::command_palette(&colors)?,
    )?;
    for (key, frame) in [("command.cloak", 252), ("command.decloak", 252)] {
        if !assets.ui.iter().any(|i| i.key == key) {
            assets.ui.push(straterust_engine::assets::UiImageManifest {
                key: key.into(),
                image: crate::add_image(files, &format!("{key}.srim"), &icons[frame])?,
            });
        }
    }
    Ok(())
}
