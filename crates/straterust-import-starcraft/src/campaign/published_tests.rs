//! Opt-in source audit of an already imported bundle; never redistributes retail data.
use super::*;
use crate::terran_media;
use straterust_engine::{
    assets::ClipKind,
    content::Package,
    media::{AudioCue, MediaPack},
};

#[test]
#[ignore = "requires STRATERUST_SOURCE and STRATERUST_CAMPAIGNS pointing to a published retail import"]
fn published_race_campaigns_preserve_source_placements_voices_and_action_art() -> Result<()> {
    let source = std::env::var_os("STRATERUST_SOURCE").context("set STRATERUST_SOURCE")?;
    let bundle = std::env::var_os("STRATERUST_CAMPAIGNS").context("set STRATERUST_CAMPAIGNS")?;
    let root = Path::new(&bundle);
    let source = Source::open(Path::new(&source))?;
    let mut installer = Archive::open_region(&source.path, source.offset, source.len)?;
    let mut archive =
        Archive::from_bytes(installer.read_file("files\\stardat.mpq", 128 * 1024 * 1024)?)?;
    let dat = archive.read_file("arr\\units.dat", 19192)?;
    let sfx = archive.read_file("arr\\sfxdata.dat", 8712)?;
    let sound_names = archive.read_file("arr\\sfxdata.tbl", 65536)?;
    for race in Race::ALL {
        let campaign_root = if race == Race::Terran {
            root.to_owned()
        } else {
            root.join(race.folder())
        };
        let campaign = straterust_engine::content::Campaign::load(&campaign_root)?;
        assert_eq!(campaign.missions.len(), 10);
        for (index, mission) in campaign.missions.iter().enumerate() {
            assert_eq!(mission.title, race.titles()[index]);
            let directory = campaign_root.join(&mission.package);
            let package = Package::load(&directory)?;
            let mut world = package.world(42)?;
            if let Some(hatchery) = world.unit_type(campaign_units::native_id(131).unwrap()) {
                let producer = hatchery.offspring.as_ref().map_or(hatchery, |offspring| {
                    world.unit_type(offspring.unit_type).unwrap()
                });
                assert!(
                    producer
                        .trains
                        .contains(&campaign_units::native_id(41).unwrap()),
                    "{}: worker production was disconnected",
                    mission.package
                );
            }
            let assets: AssetManifest =
                ron::de::from_bytes(&std::fs::read(directory.join("assets.ron"))?)?;
            let media: MediaManifest =
                straterust_engine::content::read_ron(&directory.join("media.ron"))?;
            let number = race.source_number((index + 1) as u8);
            let chk = installer.read_file(
                &format!(
                    "campaign\\{0}\\{0}{number:02}\\staredit\\scenario.chk",
                    race.folder()
                ),
                8 * 1024 * 1024,
            )?;
            let parsed = map_formats::parse_chk(&chk)?;
            let sections = Sections::read(&chk)?;
            let human = parsed.owners.iter().position(|owner| *owner == 6).unwrap() as u8;
            if race != Race::Terran {
                let mut expected = (0_u32, 0_u32);
                for original in parsed.units.iter().filter(|u| u.owner == human) {
                    expected.0 += u32::from(dat[0x412c + usize::from(original.unit_type)]);
                    expected.1 += u32::from(dat[0x4048 + usize::from(original.unit_type)]);
                }
                expected.1 = expected.1.min(400);
                assert_eq!(
                    world.supply(PlayerId(0)),
                    expected,
                    "{}: original doubled supply",
                    mission.package
                );
            }
            let placed = parsed
                .units
                .iter()
                .filter(|u| !matches!(u.unit_type, 176..=178 | 188 | 214))
                .count()
                + sections
                    .get("THG2")?
                    .as_chunks::<10>()
                    .0
                    .iter()
                    .filter(|r| short(*r, 8) & 0x1000 == 0)
                    .count();
            assert_eq!(
                world.map().spawns.len(),
                placed,
                "{} placement count",
                mission.package
            );
            for original in parsed
                .units
                .iter()
                .filter(|u| !matches!(u.unit_type, 176..=178 | 188 | 214))
            {
                let spawn = world
                    .map()
                    .spawns
                    .iter()
                    .find(|s| {
                        s.unit_type == campaign_units::native_id(original.unit_type).unwrap()
                            && s.position
                                == Position {
                                    x: i32::from(original.x),
                                    y: i32::from(original.y),
                                }
                    })
                    .context("missing source placement")?;
                if original.owner == human {
                    assert_eq!(spawn.owner, PlayerId(0));
                }
            }
            assert_eq!(
                world.map().terrain.as_ref().unwrap().flags,
                map_formats::decode_terrain(
                    &parsed,
                    &archive.read_file(
                        &format!("tileset\\{}.cv5", races::tileset(parsed.tileset)?),
                        8 * 1024 * 1024
                    )?,
                    &archive.read_file(
                        &format!("tileset\\{}.vf4", races::tileset(parsed.tileset)?),
                        8 * 1024 * 1024
                    )?
                )?
                .flags
            );
            assert_eq!(media.music.len(), 3);
            assert!(!media.briefing.is_empty());
            assert!(!media.mission_audio.is_empty());
            if race != Race::Terran {
                for &(original, native) in MAPPING
                    .iter()
                    .filter(|(_, native)| *native >= 54 || matches!(*native, 6 | 7 | 27 | 28 | 29))
                {
                    let id = UnitTypeId(native);
                    let unit = world.unit_type(id).unwrap();
                    assert_eq!(
                        unit.supply_used,
                        u32::from(dat[0x412c + usize::from(original)])
                    );
                    assert_eq!(
                        unit.supply_provided,
                        u32::from(dat[0x4048 + usize::from(original)])
                    );
                    let sprite = assets
                        .extra_units
                        .iter()
                        .find(|s| s.unit_type == id)
                        .context("missing faction sprite")?;
                    assert!(
                        sprite.clips.iter().any(|c| c.kind == ClipKind::Idle),
                        "{original}: idle"
                    );
                    if unit.max_shields > 0 {
                        assert_eq!(
                            unit.max_shields,
                            u32::from(short(&dat, 0xa8c + usize::from(original) * 2))
                        );
                    }
                    for (cue, first, last) in [
                        (
                            AudioCue::Select,
                            short(&dat, 0x236c + usize::from(original) * 2),
                            short(&dat, 0x2534 + usize::from(original) * 2),
                        ),
                        (
                            AudioCue::Ready,
                            if original < 106 {
                                short(&dat, 0x2298 + usize::from(original) * 2)
                            } else {
                                0
                            },
                            if original < 106 {
                                short(&dat, 0x2298 + usize::from(original) * 2)
                            } else {
                                0
                            },
                        ),
                        (
                            AudioCue::Order,
                            if original < 106 {
                                short(&dat, 0x28a4 + usize::from(original) * 2)
                            } else {
                                0
                            },
                            if original < 106 {
                                short(&dat, 0x2978 + usize::from(original) * 2)
                            } else {
                                0
                            },
                        ),
                    ] {
                        if first == 0 || first > last {
                            continue;
                        }
                        let mut expected = Vec::new();
                        for sound in first..=last {
                            let path = terran_media::sound_path(&sfx, &sound_names, sound)?;
                            if cue != AudioCue::Ready || archive.has_file(&path)? {
                                expected.push(format!("sound-{sound:03}.wav"));
                            }
                        }
                        if expected.is_empty() {
                            continue;
                        }
                        let mapping = media
                            .audio
                            .iter()
                            .find(|m| m.unit_type == Some(id) && m.cue == cue)
                            .context("missing source voice")?;
                        assert_eq!(
                            mapping
                                .variants
                                .iter()
                                .map(|v| v.file.clone())
                                .collect::<Vec<_>>(),
                            expected,
                            "{original}: {cue:?}"
                        );
                    }
                }
                for original in [9, 12, 29, 69, 70, 71, 72, 82] {
                    let sprite = assets
                        .extra_units
                        .iter()
                        .find(|s| Some(s.unit_type) == campaign_units::native_id(original))
                        .unwrap();
                    assert!(
                        sprite.clips.iter().any(|c| c.kind == ClipKind::Shadow),
                        "{original}: shadow"
                    );
                }
            }
            MediaPack::load(&directory)?
                .context("missing media pack")?
                .validate_world(&world)?;
            // Exercise initialization, trigger-created units and paused dialogue without a long playthrough.
            for _ in 0..64 {
                world.step(&[])?;
            }
            assert!(!sections.get("MBRF")?.is_empty());
            println!(
                "{}: placements, terrain, voices, artwork, briefing and initialization verified",
                mission.package
            );
        }
    }
    let menus = straterust_engine::menus::MenuPack::load(root)?.context("missing bundle menus")?;
    assert_eq!(menus.manifest.campaigns.len(), 3);
    assert_eq!(package_directories(root)?.len(), 30);
    Ok(())
}
