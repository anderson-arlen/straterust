use super::*;

pub(crate) fn add_mine_art<R: Read + Seek>(
    archive: &mut Archive<R>,
    members: &mut Vec<MemberReport>,
    files: &mut Files,
    assets: &mut AssetManifest,
    scripts: &[u8],
    palette: &[[u8; 4]; 256],
    fire: &[[u8; 4]; 256],
) -> Result<()> {
    expect_animation(scripts, 87, 0, &[9, 3, 1, 0, 0, 0, 0, 0])?;
    expect_animation(scripts, 87, 1, &[8, 173, 1, 0, 0, 5, 1, 22])?;
    expect_animation(scripts, 87, 13, &[0x27, 0x24, 1])?;
    let body = grp(
        archive,
        members,
        "unit\\terran\\Spider.grp",
        palette,
        [12, 36, 36],
    )?;
    let explosion = grp(
        archive,
        members,
        "unit\\thingy\\tmnExplo.grp",
        fire,
        [10, 80, 80],
    )?;
    let dust = grp(
        archive,
        members,
        "unit\\thingy\\bDust.grp",
        palette,
        [7, 128, 128],
    )?;
    let mut frames = body;
    frames.extend(explosion);
    let mut transitions = Vec::new();
    for (kind, poses) in [
        (ClipKind::Conceal, vec![8, 9, 10, 11]),
        (ClipKind::Reveal, vec![10, 9, 8]),
    ] {
        let mut indices = Vec::new();
        for (tick, pose) in poses.into_iter().enumerate() {
            let body = center_canvas(&frames[pose], 128, 128)?;
            let image = if kind == ClipKind::Conceal {
                composite(&body, &dust[tick / 2])?
            } else {
                composite(&dust[tick / 2], &body)?
            };
            indices.push(frames.len() as u16);
            frames.push(image);
        }
        transitions.push(single_direction(kind, &indices, 42));
    }
    let mut clips = vec![
        single_direction(ClipKind::Idle, &[0], 42),
        single_direction(ClipKind::Walk, &[0, 1, 2, 3, 4, 5, 6, 7], 42),
        single_direction(ClipKind::Death, &(12..22).collect::<Vec<_>>(), 42),
    ];
    clips.extend(transitions);
    assets.extra_units.push(compact_sprite(
        files,
        18,
        "Spider Mine",
        "spider-mine",
        &frames,
        clips,
    )?);
    Ok(())
}

pub(crate) fn read<R: Read + Seek>(
    archive: &mut Archive<R>,
    members: &mut Vec<MemberReport>,
    path: &str,
) -> Result<Vec<u8>> {
    member(
        archive,
        path,
        8 * 1024 * 1024,
        "stardat",
        "Mission 2 original Terran data/art",
        members,
    )
}
pub(crate) fn grp<R: Read + Seek>(
    archive: &mut Archive<R>,
    members: &mut Vec<MemberReport>,
    path: &str,
    palette: &[[u8; 4]; 256],
    expected: [u16; 3],
) -> Result<Vec<Image>> {
    decode_expected(&read(archive, members, path)?, palette, expected)
        .with_context(|| format!("decode {path}"))
}

pub(crate) fn add_grenade_projectiles<R: Read + Seek>(
    archive: &mut Archive<R>,
    members: &mut Vec<MemberReport>,
    files: &mut Files,
    assets: &mut AssetManifest,
    rules: &Rules,
) -> Result<()> {
    let ids: Vec<_> = [UnitTypeId(10), UnitTypeId(20)]
        .into_iter()
        .filter(|id| rules.units.iter().any(|unit| unit.id == *id))
        .collect();
    if ids.is_empty() {
        return Ok(());
    }
    let weapons = read(archive, members, "arr\\weapons.dat")?;
    let flingy = read(archive, members, "arr\\flingy.dat")?;
    let sprites = read(archive, members, "arr\\sprites.dat")?;
    let images = read(archive, members, "arr\\images.dat")?;
    ensure!(
        weapons.len() == 4200
            && flingy.len() == 2760
            && sprites.len() == 2081
            && images.len() == 28690,
        "unsupported grenade source tables"
    );
    let word = |b: &[u8], p| u16::from_le_bytes(b[p..p + 2].try_into().unwrap());
    let dword = |b: &[u8], p| u32::from_le_bytes(b[p..p + 4].try_into().unwrap());
    // Raynor weapon 5 and Vulture weapon 4 share flingy 145 / sprite 343 / image 532.
    ensure!(
        dword(&weapons, 200 + 4 * 4) == 145
            && dword(&weapons, 200 + 5 * 4) == 145
            && word(&flingy, 145 * 2) == 343
            && word(&sprites, 343 * 2) == 532
            && dword(&images, 755 * 10 + 532 * 4) == 242,
        "unsupported grenade source mapping"
    );
    let scripts = read(archive, members, "scripts\\iscript.bin")?;
    expect_animation(&scripts, 242, 1, &[8, 0xb8, 1, 0, 0, 0x1b, 5, 1, 0x16])?;
    let mut impact_prefix = vec![0x1a, 107, 0, 109, 0];
    for frame in 0..9 {
        impact_prefix.extend([0, frame, 0, 5, 2]);
    }
    impact_prefix.push(0x16);
    expect_animation(&scripts, 283, 0, &impact_prefix)?;
    let palette = formats::palette(&read(archive, members, "tileset\\badlands.wpe")?)?;
    let fire = fire_palette(
        &read(archive, members, "tileset\\badlands\\ofire.pcx")?,
        &palette,
    )?;
    let flight = formats::decode_grp(
        &read(archive, members, "unit\\bullet\\grenade.grp")?,
        &palette,
    )?;
    let impact = formats::decode_grp(&read(archive, members, "unit\\thingy\\efgHit.grp")?, &fire)?;
    ensure!(
        flight.len() == 4 && impact.len() == 9,
        "unexpected grenade frame count"
    );
    let mut effect = |name: &str, frames: &[Image]| -> Result<EffectManifest> {
        Ok(EffectManifest {
            frame_ms: 42,
            anchor: [frames[0].width as i32 / 2, frames[0].height as i32 / 2],
            frames: frames
                .iter()
                .enumerate()
                .map(|(n, image)| add_image(files, &format!("{name}-{n:02}.srim"), image))
                .collect::<Result<_>>()?,
            sequence: (0..frames.len() as u16).flat_map(|n| [n, n]).collect(),
        })
    };
    let flight = effect("grenade-flight", &flight)?;
    let impact = effect("grenade-impact", &impact)?;
    assets
        .projectiles
        .retain(|effect| !ids.contains(&effect.unit_type));
    for unit_type in ids {
        assets.projectiles.push(ProjectileManifest {
            targets_air: false,
            directional: false,
            unit_type,
            speed_fp8: dword(&flingy, 368 + 145 * 4),
            forward_offset: u32::from(weapons[0xe10 + 5]),
            arc_height: 24,
            on_target: false,
            flight: flight.clone(),
            impact: impact.clone(),
        });
    }
    files.insert("weapon-effects-reference.ron".into(), ron_bytes(&(
        "Grenade: weapons 4/5 -> flingy145 -> sprite343 -> image532/script242; death image440/script283.",
        "Original flight and impact frames, 8533/256 pixels per tick, forward offset20, wait2 poses. Cosmetic flight does not yet delay authoritative damage until arrival.", &members))?);
    Ok(())
}

pub(crate) fn add_scan_effect<R: Read + Seek>(
    archive: &mut Archive<R>,
    members: &mut Vec<MemberReport>,
    files: &mut Files,
    assets: &mut AssetManifest,
    scripts: &[u8],
    palette: &[[u8; 4]; 256],
) -> Result<()> {
    let sprites = read(archive, members, "arr\\sprites.dat")?;
    let images = read(archive, members, "arr\\images.dat")?;
    ensure!(
        sprites.len() == 2081 && images.len() == 28690,
        "unsupported scanner source tables"
    );
    ensure!(
        u16::from_le_bytes(sprites[760..762].try_into().unwrap()) == 546
            && images[755 * 8 + 546] == 0
            && u32::from_le_bytes(
                images[755 * 10 + 546 * 4..755 * 10 + 546 * 4 + 4]
                    .try_into()
                    .unwrap()
            ) == 253,
        "unsupported scanner pulse definition"
    );
    let mut prefix = Vec::new();
    for ((x, y), wait) in SCAN_LAUNCHES.iter().zip([6, 2, 5, 2, 2, 5, 3, 5, 63]) {
        prefix.extend([0x0f, 124, 1, *x as u8, *y as u8, 5, wait]);
    }
    prefix.extend([5, 63, 0x24, 4]);
    expect_animation(scripts, 81, 0, &prefix)?;
    let mut pulse_prefix = Vec::new();
    for frame in 0..8 {
        pulse_prefix.extend([0, frame, 0, 5, 2]);
    }
    pulse_prefix.extend([5, 1, 0x16]);
    expect_animation(scripts, 253, 0, &pulse_prefix)?;
    let pulses = grp(
        archive,
        members,
        "unit\\thingy\\eveCast.grp",
        palette,
        [8, 48, 48],
    )?;
    let (frames, sequence) = scan_frames(&pulses)?;
    let references = frames
        .iter()
        .enumerate()
        .map(|(index, image)| add_image(files, &format!("scanner-{index:03}.srim"), image))
        .collect::<Result<Vec<_>>>()?;
    assets.scan_effect = Some(EffectManifest {
        frame_ms: 42,
        anchor: [72, 72],
        frames: references,
        sequence,
    });
    Ok(())
}

const SCAN_LAUNCHES: [(i8, i8); 9] = [
    (0, 0),
    (32, 32),
    (48, 5),
    (32, -32),
    (-5, -48),
    (-32, -32),
    (-48, -2),
    (-32, 32),
    (3, 48),
];
const SCAN_STARTS: [usize; 9] = [0, 6, 8, 13, 15, 17, 22, 25, 30];

pub(crate) fn scan_frames(pulses: &[Image]) -> Result<(Vec<Image>, Vec<u16>)> {
    ensure!(
        pulses.len() == 8
            && pulses.iter().all(|image| image.width == 48
                && image.height == 48
                && image.rgba.len() == 48 * 48 * 4),
        "invalid scanner pulse images"
    );
    let mut frames: Vec<Image> = Vec::new();
    let mut sequence = Vec::with_capacity(156);
    for tick in 0_usize..156 {
        let mut image = Image {
            width: 144,
            height: 144,
            rgba: vec![0; 144 * 144 * 4],
        };
        for (&start, &(x, y)) in SCAN_STARTS.iter().zip(&SCAN_LAUNCHES) {
            let Some(age) = tick.checked_sub(start) else {
                continue;
            };
            if age >= 17 {
                continue;
            }
            let pulse = &pulses[(age / 2).min(7)];
            for row in 0..48 {
                for column in 0..48 {
                    let source = (row * 48 + column) * 4;
                    if pulse.rgba[source + 3] == 0 {
                        continue;
                    }
                    let destination = (((i32::from(y) + 48 + row as i32) as usize * 144)
                        + (i32::from(x) + 48 + column as i32) as usize)
                        * 4;
                    image.rgba[destination..destination + 4]
                        .copy_from_slice(&pulse.rgba[source..source + 4]);
                }
            }
        }
        let index = if let Some(index) = frames.iter().position(|previous| previous == &image) {
            index
        } else {
            frames.push(image);
            frames.len() - 1
        };
        sequence.push(index as u16);
    }
    Ok((frames, sequence))
}
pub(crate) fn verify_scripts(scripts: &[u8]) -> Result<()> {
    for (id, animation, prefix) in [
        (86, 0, &[9, 1, 1, 0, 7, 0, 0, 0][..]),
        (69, 0, &[9, 227, 0, 0, 0, 0, 34, 0][..]),
        (
            69,
            2,
            &[0, 0, 0, 5, 1, 0x2e, 8, 0xa5, 1, 0, 0, 0, 17, 0][..],
        ),
        (94, 16, &[0, 0, 0][..]),
        (99, 16, &[8, 0x11, 1, 0, 0, 0, 0, 0][..]),
        (123, 16, &[0, 0, 0][..]),
        (125, 16, &[0, 0, 0, 5, 5][..]),
        (136, 16, &[8, 0x43, 1, 0, 0, 0, 0, 0][..]),
    ] {
        expect_animation(scripts, id, animation, prefix)?;
    }
    Ok(())
}

pub(crate) fn append_sprite_frames(
    target: &mut SpriteManifest,
    extra: &SpriteManifest,
) -> Result<()> {
    ensure!(
        target.anchor == extra.anchor,
        "cannot combine differently anchored sprites"
    );
    let base = target.frames.len() as u16;
    target.frames.extend(extra.frames.iter().cloned());
    for clip in &extra.clips {
        let mut clip = clip.clone();
        for frame in &mut clip.frames {
            frame.frame += base;
        }
        target.clips.push(clip);
    }
    Ok(())
}
pub(crate) fn copy_death(
    files: &mut Files,
    target: &mut SpriteManifest,
    source: &SpriteManifest,
) -> Result<()> {
    let clip = source
        .clips
        .iter()
        .find(|c| c.kind == ClipKind::Death)
        .context("missing source death")?;
    let mut mapping = BTreeMap::new();
    let mut images = Vec::new();
    let mut clip = clip.clone();
    for frame in &mut clip.frames {
        let index = if let Some(index) = mapping.get(&frame.frame) {
            *index
        } else {
            let image = straterust_engine::assets::decode_image(
                &files[&source.frames[usize::from(frame.frame)].file],
            )?;
            // Existing shared death canvases use their center; compact conversion retains it.
            ensure!(
                source.anchor == [image.width as i32 / 2, image.height as i32 / 2],
                "shared death anchor changed"
            );
            let index = images.len() as u16;
            images.push(image);
            mapping.insert(frame.frame, index);
            index
        };
        frame.frame = index;
    }
    let extra = compact_sprite(
        files,
        target.unit_type.0,
        &target.unit_name,
        &format!("mission-death-{}", target.unit_type.0),
        &images,
        vec![clip],
    )?;
    append_sprite_frames(target, &extra)
}
