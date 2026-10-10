use super::*;

pub(crate) fn convert_research<R: std::io::Read + std::io::Seek>(
    archive: &mut Archive<R>,
    units: &[u8],
    availability: &[Availability],
    rules: &mut Rules,
    files: &mut Files,
    members: &mut Vec<MemberReport>,
) -> Result<()> {
    use straterust_engine::sim::{Research, ResearchEffect, ResearchId};
    let upgrades = member(
        archive,
        "arr\\upgrades.dat",
        920,
        "stardat",
        "original46-entry upgrade costs and timing",
        members,
    )?;
    let tech = member(
        archive,
        "arr\\techdata.dat",
        432,
        "stardat",
        "original24-entry research costs and timing",
        members,
    )?;
    let weapons = member(
        archive,
        "arr\\weapons.dat",
        4200,
        "stardat",
        "infantry weapon upgrade mappings",
        members,
    )?;
    ensure!(
        upgrades.len() == 920 && tech.len() == 432 && units.len() == 19192 && weapons.len() == 4200,
        "unsupported campaign research DAT layout"
    );
    let human = availability
        .iter()
        .find(|a| a.source_player == 1)
        .context("human availability missing")?;
    ensure!(
        [0, 7, 16].iter().all(|id| human.upgrades[*id] == [0, 1])
            && [0, 1, 3].iter().all(|id| human.technologies[*id] == [1, 0]),
        "unsupported campaign research overrides"
    );
    let mut armed = Vec::new();
    let mut armored = Vec::new();
    let mut bonus = None;
    for &(source, native) in &UNIT_IDS {
        let source = usize::from(source);
        if units[0x1f08 + source] == 0 {
            armored.push(UnitTypeId(native));
        }
        let weapon = usize::from(units[0x1704 + source]);
        if weapon < 100 && weapons[0x6a4 + weapon] == 7 {
            let amount = u32::from(short(&weapons, 0xbb8 + weapon * 2));
            ensure!(
                bonus.is_none_or(|value| value == amount),
                "unequal infantry damage upgrades require individual effects"
            );
            bonus = Some(amount);
            armed.push(UnitTypeId(native));
        }
    }
    let cost = |minerals: u16, gas: u16| -> Vec<ResourceAmount> {
        [("minerals", minerals), ("gas", gas)]
            .into_iter()
            .filter(|(_, amount)| *amount != 0)
            .map(|(kind, amount)| ResourceAmount {
                kind: kind.into(),
                amount: u32::from(amount),
            })
            .collect()
    };
    let upgrade = |source: usize, native: u16, facility: u16, effect: ResearchEffect| -> Research {
        Research {
            available: true,
            id: ResearchId(native),
            facility: UnitTypeId(facility),
            previous: None,
            prerequisites: Vec::new(),
            cost: cost(
                short(&upgrades, source * 2),
                short(&upgrades, 46 * 4 + source * 2),
            ),
            ticks: u32::from(short(&upgrades, 46 * 8 + source * 2)),
            effect,
        }
    };
    rules.research = vec![
        upgrade(
            7,
            1,
            15,
            ResearchEffect::WeaponDamage {
                units: armed.clone(),
                amount: bonus.context("missing infantry weapon upgrade")?,
            },
        ),
        upgrade(
            0,
            2,
            15,
            ResearchEffect::Armor {
                units: armored,
                amount: 1,
            },
        ),
        upgrade(
            16,
            3,
            12,
            ResearchEffect::WeaponRange {
                units: vec![UnitTypeId(1)],
                amount: 32,
                sight: 0,
            },
        ),
        Research {
            available: true,
            id: ResearchId(4),
            facility: UnitTypeId(12),
            previous: None,
            prerequisites: Vec::new(),
            cost: cost(short(&tech, 0), short(&tech, 24 * 2)),
            ticks: u32::from(short(&tech, 24 * 4)),
            effect: ResearchEffect::Stim {
                units: armed,
                hp_cost: 10,
                duration_ticks: 296,
            },
        },
    ];
    // Display names and keys stay out of authoritative rules and hashes.
    let mut presentation = std::str::from_utf8(&files["presentation.ron"])?.to_owned();
    let names = BTreeMap::from([
        (ResearchId(1), "Terran Infantry Weapons"),
        (ResearchId(2), "Terran Infantry Armor"),
        (ResearchId(3), "U-238 Shells"),
        (ResearchId(4), "Stim Packs"),
    ]);
    let keys = BTreeMap::from([
        (ResearchId(1), "W"),
        (ResearchId(2), "A"),
        (ResearchId(3), "U"),
        (ResearchId(4), "T"),
    ]);
    let train_keys = BTreeMap::from([
        (UnitTypeId(1), "M"),
        (UnitTypeId(2), "S"),
        (UnitTypeId(11), "F"),
    ]);
    for (name, value) in [
        ("research_names", ron::ser::to_string(&names)?),
        ("research_keys", ron::ser::to_string(&keys)?),
        ("train_keys", ron::ser::to_string(&train_keys)?),
    ] {
        crate::campaign_units::set_map(&mut presentation, name, &value)?;
    }
    files.insert("presentation.ron".into(), presentation.into_bytes());
    Ok(())
}

pub(crate) fn convert_decorations<R: std::io::Read + std::io::Seek>(
    archive: &mut Archive<R>,
    records: &[u8],
    files: &mut Files,
    members: &mut Vec<MemberReport>,
) -> Result<()> {
    use crate::{
        add_image, formats,
        terran::{center_canvas, composite, expect_animation},
        terran_media::table_string,
    };
    ensure!(
        records.len().is_multiple_of(10) && records.len() / 10 <= 4096,
        "invalid THG2 decoration records"
    );
    let sprites = member(
        archive,
        "arr\\sprites.dat",
        2081,
        "stardat",
        "campaign static sprite mappings",
        members,
    )?;
    let images = member(
        archive,
        "arr\\images.dat",
        28690,
        "stardat",
        "campaign static image mappings",
        members,
    )?;
    let table = member(
        archive,
        "arr\\images.tbl",
        65536,
        "stardat",
        "campaign static image paths",
        members,
    )?;
    let scripts = member(
        archive,
        "scripts\\iscript.bin",
        65536,
        "stardat",
        "campaign static image initialization",
        members,
    )?;
    let colors = member(
        archive,
        "tileset\\badlands.wpe",
        1024,
        "stardat",
        "campaign decoration palette",
        members,
    )?;
    ensure!(
        sprites.len() == 2081 && images.len() == 28690,
        "unsupported decoration DAT layout"
    );
    expect_animation(&scripts, 337, 0, &[0, 0, 0, 0x3d, 0, 0, 7, 0xba, 0x79])?;
    expect_animation(&scripts, 336, 0, &[0, 0, 0, 7, 0xba, 0x79])?;
    expect_animation(&scripts, 275, 0, &[5, 1, 0x1d, 7, 0x12, 0x7a])?;
    ensure!(
        scripts.get(0x79ba..0x79bf) == Some(&[5, 125, 7, 0xba, 0x79]),
        "unsupported static decoration wait loop"
    );
    let palette = formats::palette(&colors)?;
    let mut assets: AssetManifest = ron::de::from_bytes(&files["assets.ron"])?;
    ensure!(
        assets.map_images.is_empty(),
        "world decorations already assigned"
    );
    let mut cache = BTreeMap::new();
    for record in records.as_chunks::<10>().0 {
        let source = short(record, 0);
        let flags = short(record, 8);
        ensure!(
            matches!(source,76..=79|85..=89)
                && flags & 0x1000 != 0
                && flags & 0x8000 == 0
                && record[6] == 0,
            "unsupported campaign decoration"
        );
        if let std::collections::btree_map::Entry::Vacant(entry) = cache.entry(source) {
            let image = usize::from(short(&sprites, usize::from(source) * 2));
            ensure!(
                image < 755 && images[755 * 8 + image] == 0 && images[755 * 9 + image] == 0,
                "unsupported decoration drawing mode"
            );
            let has_shadow = source != 89;
            ensure!(
                word(&images, 755 * 10 + image * 4) == if has_shadow { 337 } else { 336 },
                "unsupported decoration animation"
            );
            let mut frames = Vec::new();
            for offset in 0..=usize::from(has_shadow) {
                let index = image + offset;
                if offset != 0 {
                    ensure!(
                        images[755 * 8 + index] == 10 && word(&images, 755 * 10 + index * 4) == 275,
                        "unsupported decoration shadow"
                    );
                }
                let path = format!("unit\\{}", table_string(&table, word(&images, index * 4))?);
                let bytes = member(
                    archive,
                    &path,
                    4 * 1024 * 1024,
                    "stardat",
                    "campaign static body/shadow GRP",
                    members,
                )?;
                let mut decoded = formats::decode_grp(&bytes, &palette)?;
                ensure!(
                    decoded.len() == 1,
                    "animated campaign decoration requires explicit conversion"
                );
                let mut frame = decoded.remove(0);
                if offset != 0 {
                    for pixel in frame.rgba.as_chunks_mut::<4>().0 {
                        if pixel[3] != 0 {
                            pixel.copy_from_slice(&[0, 0, 0, 128]);
                        }
                    }
                }
                frames.push(frame);
            }
            let body = frames.remove(0);
            let merged = if let Some(shadow) = frames.pop() {
                let width = body.width.max(shadow.width);
                let height = body.height.max(shadow.height);
                composite(
                    &center_canvas(&shadow, width, height)?,
                    &center_canvas(&body, width, height)?,
                )?
            } else {
                body
            };
            let anchor = [(merged.width / 2) as i32, (merged.height / 2) as i32];
            let image = add_image(files, &format!("map-sprite-{source:03}.srim"), &merged)?;
            entry.insert((anchor, image));
        }
        let (anchor, image) = &cache[&source];
        assets.map_images.push(MapImageManifest {
            position: Position {
                x: i32::from(short(record, 2)),
                y: i32::from(short(record, 4)),
            },
            anchor: *anchor,
            image: image.clone(),
        });
    }
    assets.validate()?;
    files.insert("assets.ron".into(), ron_bytes(&assets)?);
    Ok(())
}
