//! Retail multiplayer scenarios are encrypted nested MPQs in the installer.
use super::*;
use crate::{
    backwater::{Sections, short},
    campaign::{self, Race, races},
};
use std::collections::BTreeSet;
use straterust_engine::{map::Terrain, multiplayer::*, sim::*};

fn shared(root: &Path, cache: &Path, files: &Files) -> Result<()> {
    fs::create_dir_all(root)?;
    for (name, bytes) in files {
        let stored = cache.join(blake3::hash(bytes).to_hex().as_str());
        if !stored.exists() {
            fs::write(&stored, bytes)?;
        }
        fs::hard_link(stored, root.join(name))?;
    }
    Ok(())
}
pub fn publish(
    payload: &Payload,
    path: &Path,
    output: &Path,
    progress: &dyn Fn(&str),
) -> Result<()> {
    progress("Finding original StarCraft multiplayer maps");
    let source = Source::open(path)?;
    let mut installer = Archive::open_region(&source.path, source.offset, source.len)?;
    let mut maps = Vec::new();
    for (index, bytes) in installer.embedded_archives(8 * 1024 * 1024)? {
        let mut archive = Archive::from_bytes(bytes.clone())?;
        if archive.has_file("staredit\\scenario.chk")? {
            maps.push((
                index,
                archive.read_file("staredit\\scenario.chk", 8 * 1024 * 1024)?,
                bytes,
            ));
        }
    }
    ensure!(!maps.is_empty(), "no original multiplayer maps found");
    let mut archive =
        Archive::from_bytes(installer.read_file("files\\stardat.mpq", 128 * 1024 * 1024)?)?;
    let mut base = campaign::prepare(payload, path, Race::Zerg)?;
    let mut rules: Rules = ron::de::from_bytes(&base["rules.ron"])?;
    let mut assets: AssetManifest = ron::de::from_bytes(&base["assets.ron"])?;
    // Multiplayer slots use the source Random/User Select race values. Choose
    // a concrete listener while constructing the shared technology definitions.
    let mut technology = maps[0].1.clone();
    let mut at = 0;
    while at < technology.len() {
        let length = u32::from_le_bytes(technology[at + 4..at + 8].try_into()?) as usize;
        if &technology[at..at + 4] == b"SIDE" {
            technology[at + 8..at + 8 + length].fill(0);
        }
        at += 8 + length;
    }
    campaign_units::refresh_research(
        &mut archive,
        &mut base,
        &mut assets,
        &mut rules,
        &technology,
    )?;
    rules.id = "starcraft.multiplayer".into();
    rules.victory = true;
    let worker = rules
        .units
        .iter_mut()
        .find(|u| u.id == UnitTypeId(2))
        .unwrap();
    worker.builds.extend([
        UnitTypeId(12),
        UnitTypeId(13),
        UnitTypeId(14),
        UnitTypeId(15),
    ]);
    worker.builds.sort();
    worker.builds.dedup();
    worker.phases_while_gathering = true;
    base.insert("rules.ron".into(), ron_bytes(&rules)?);
    base.insert("assets.ron".into(), ron_bytes(&assets)?);
    let mut media: straterust_engine::media::MediaManifest =
        ron::de::from_bytes(&base["media.ron"])?;
    media.briefing.clear();
    media.mission_audio.clear();
    media.mission_texts.clear();
    base.insert("media.ron".into(), ron_bytes(&media)?);
    for name in [
        "map.ron",
        "mission.ron",
        "scenario.ron",
        "manifest.ron",
        "backwater-reference.ron",
        "import-report.ron",
    ] {
        base.remove(name);
    }
    let cache = tempfile::Builder::new()
        .prefix(".multiplayer-assets-")
        .tempdir_in(output)?;
    let id = |source| campaign_units::native_id(source).expect("full imported roster");
    let worker_positions = [
        Position { x: -48, y: 72 },
        Position { x: -16, y: 72 },
        Position { x: 16, y: 72 },
        Position { x: 48, y: 72 },
    ];
    let mut catalog = Catalog {
        colors: crate::colors::catalog(&mut archive)?,
        schema_version: 1,
        game: "starcraft".into(),
        maps: Vec::new(),
        factions: [
            (Race::Terran, 7, 106, 0, 111),
            (Race::Zerg, 41, 131, 37, 142),
            (Race::Protoss, 64, 154, 65, 160),
        ]
        .into_iter()
        .map(|(race, worker, base, soldier, barracks)| {
            let mut starting_units = vec![(id(base), Position { x: 0, y: 0 })];
            starting_units.extend(worker_positions.map(|p| (id(worker), p)));
            if race == Race::Zerg {
                starting_units.push((id(42), Position { x: 0, y: -96 }));
            }
            Faction {
                ai_difficulties: Default::default(),
                ai_research: Vec::new(),
                name: match race {
                    Race::Terran => "Terran",
                    Race::Zerg => "Zerg",
                    Race::Protoss => "Protoss",
                }
                .into(),
                presentation: format!("multiplayer/art-{}-0", race.folder()),
                tileset_presentations: (0..5)
                    .map(|era| format!("multiplayer/art-{}-{era}", race.folder()))
                    .collect(),
                worker: id(worker),
                starting_units,
                equivalents: BTreeMap::new(),
                ai: vec![
                    AiInstruction::Request {
                        unit_type: id(worker),
                        count: 12,
                        priority: 240,
                    },
                    AiInstruction::Request {
                        unit_type: id(barracks),
                        count: 1,
                        priority: 220,
                    },
                    AiInstruction::Request {
                        unit_type: id(soldier),
                        count: 12,
                        priority: 180,
                    },
                    AiInstruction::AttackClear,
                    AiInstruction::AttackAdd {
                        unit_type: id(soldier),
                        count: 8,
                    },
                    AiInstruction::AttackPrepare,
                    AiInstruction::Wait(20000),
                    AiInstruction::Attack,
                    AiInstruction::Jump(2),
                ],
            }
        })
        .collect(),
        resources: vec![ResourcePreset {
            name: "Standard".into(),
            amounts: vec![
                ResourceAmount {
                    kind: "minerals".into(),
                    amount: 50,
                },
                ResourceAmount {
                    kind: "gas".into(),
                    amount: 0,
                },
            ],
        }],
        speeds: [
            ("Slowest", 167),
            ("Slower", 111),
            ("Slow", 83),
            ("Normal", 67),
            ("Fast", 56),
            ("Faster", 48),
            ("Fastest", 42),
        ]
        .into_iter()
        .map(|(name, tick_ms)| Speed {
            name: name.into(),
            tick_ms,
        })
        .collect(),
        modes: vec![
            GameType::Melee,
            GameType::FreeForAll,
            GameType::OneOnOne,
            GameType::TopVsBottom,
            GameType::UseMapSettings,
            GameType::Greed,
            GameType::Slaughter,
            GameType::SuddenDeath,
            GameType::CaptureTheFlag,
            GameType::TeamMelee,
            GameType::TeamFreeForAll,
            GameType::TeamCaptureTheFlag,
            GameType::Teams,
        ],
        tilesets: Vec::new(),
        terrain_mappings: Vec::new(),
        allow_cheats: false,
        random_faction: true,
        standard_start: true,
        defeat_without_structures: true,
        flag_art: Some(FlagArt {
            item: id(215),
            homes: vec![id(194), id(196), id(195)],
        }),
    };
    for race in Race::ALL {
        let mut files = base.clone();
        races::presentation(&mut installer, &mut archive, &mut files, race)?;
        for era in 0..5 {
            let mut files = files.clone();
            if era != 0 && era != 2 {
                let mut assets: AssetManifest = ron::de::from_bytes(&files["assets.ron"])?;
                campaign_units::refresh_creep_tileset(
                    &mut archive,
                    &mut files,
                    &mut assets,
                    &rules,
                    races::tileset(era)?,
                )?;
                files.insert("assets.ron".into(), ron_bytes(&assets)?);
            }
            shared(
                &output.join(format!("multiplayer/art-{}-{era}", race.folder())),
                cache.path(),
                &files,
            )?;
        }
    }
    let mut seen = BTreeSet::new();
    for (index, chk, map_archive) in maps {
        let hash = blake3::hash(&chk).to_hex().to_string();
        if !seen.insert(hash.clone()) {
            continue;
        }
        let sections = Sections::read(&chk)?;
        let parsed = map_formats::parse_chk(&chk)?;
        let mut starts: Vec<_> = parsed
            .units
            .iter()
            .filter(|u| u.unit_type == 214 && u.owner < 8)
            .collect();
        starts.sort_by_key(|u| u.owner);
        ensure!(
            (2..=8).contains(&starts.len()),
            "invalid multiplayer starts"
        );
        let ids: BTreeMap<_, _> = starts
            .iter()
            .enumerate()
            .map(|(i, u)| (u.owner, PlayerId(i as u16)))
            .chain((8..12).map(|p| (p, PlayerId(u16::from(p)))))
            .collect();
        let strings = backwater::read_strings(sections.get("STR ")?)?;
        let title_index = short(sections.get("SPRP")?, 0);
        let title = strings
            .get(usize::from(title_index))
            .cloned()
            .unwrap_or_else(|| format!("Multiplayer map {index}"));
        let title: String = title
            .chars()
            .filter(|c| !c.is_control())
            .take(128)
            .collect();
        progress(&format!("Converting multiplayer map: {title}"));
        let read = |archive: &mut Archive<std::io::Cursor<Vec<u8>>>, ext: &str| {
            archive.read_file(
                &format!("tileset\\{}.{ext}", races::tileset(parsed.tileset)?),
                8 * 1024 * 1024,
            )
        };
        let terrain_payload = Payload {
            inventory: payload.inventory.clone(),
            wpe: read(&mut archive, "wpe")?,
            vx4: read(&mut archive, "vx4")?,
            vr4: read(&mut archive, "vr4")?,
            cv5: read(&mut archive, "cv5")?,
            vf4: read(&mut archive, "vf4")?,
            grp: Vec::new(),
        };
        let decoded =
            map_formats::decode_terrain(&parsed, &terrain_payload.cv5, &terrain_payload.vf4)?;
        let mut files = base.clone();
        let mut assets: AssetManifest = ron::de::from_bytes(&files["assets.ron"])?;
        assets.player_colors = crate::colors::players(&mut archive, &ids, Some(&sections))?;
        if parsed.tileset != 0 && parsed.tileset != 2 {
            campaign_units::refresh_creep_tileset(
                &mut archive,
                &mut files,
                &mut assets,
                &rules,
                races::tileset(parsed.tileset)?,
            )?;
        }
        files.insert("assets.ron".into(), ron_bytes(&assets)?);
        let map_id = format!("starcraft.multiplayer.{}", &hash[..16]);
        let mut map = Map {
            multiplayer: None,
            id: map_id.clone(),
            width: i32::from(parsed.width) * 32,
            height: i32::from(parsed.height) * 32,
            players: 12,
            spawns: Vec::new(),
            resources: Vec::new(),
            start_locations: Vec::new(),
            fog_of_war: true,
            mission: None,
            initial_explored: BTreeMap::new(),
            creation: BTreeMap::new(),
            ai: Vec::new(),
            terrain: Some(Terrain {
                cell_size: 8,
                columns: u32::from(parsed.width) * 4,
                rows: u32::from(parsed.height) * 4,
                flags: decoded.flags.clone(),
            }),
        };
        for (unit, raw) in parsed
            .units
            .iter()
            .zip(sections.get("UNIT")?.as_chunks::<36>().0)
        {
            let position = Position {
                x: i32::from(unit.x),
                y: i32::from(unit.y),
            };
            if let Some(resource) = campaign::placed_resource(unit)? {
                map.resources.push(resource);
            }
            match unit.unit_type {
                176..=178 | 188 => {}
                214 => {
                    if let Some(player) = ids.get(&unit.owner) {
                        map.start_locations.push(StartLocation {
                            player: *player,
                            position,
                        });
                    }
                }
                _ => map.spawns.push(Spawn {
                    owner: *ids
                        .get(&unit.owner)
                        .context("unknown placed multiplayer player")?,
                    unit_type: campaign_units::native_id(unit.unit_type)
                        .context("unsupported multiplayer unit placement")?,
                    position,
                    hp_percent: (short(raw, 14) & 2 != 0).then_some(raw[17]),
                    shield_percent: (short(raw, 14) & 4 != 0).then_some(raw[18]),
                    energy_percent: (short(raw, 14) & 8 != 0).then_some(raw[19]),
                    invincible: short(raw, 12) & short(raw, 26) & 16 != 0,
                    cloaked: short(raw, 12) & short(raw, 26) & 3 != 0
                        || matches!(unit.unit_type, 74 | 75),
                    ..Spawn::default()
                }),
            }
        }
        campaign::import_things(
            &mut archive,
            &mut files,
            sections.get("THG2")?,
            &ids,
            &mut map,
            parsed.tileset,
        )?;
        crate::write_map_terrain(&terrain_payload, &mut files, &parsed, &decoded)?;
        let locations = backwater::read_locations(sections.get("MRGN")?)?;
        let triggers = backwater::read_triggers(sections.get("TRIG")?, false)?;
        let mut refs = backwater::References::collect(&triggers, &[], &strings)?;
        let audio =
            refs.extract_map_audio(&mut Archive::from_bytes(map_archive)?, &strings, &mut files);
        let source_players = starts.iter().map(|u| u.owner).collect::<Vec<_>>();
        let scenario = campaign::triggers::translate(
            &triggers,
            sections.get("UPRP")?,
            &locations,
            &refs,
            &source_players,
            &ids,
            sections.get("FORC")?,
            &archive.read_file("scripts\\aiscript.bin", 65536)?,
            &mut map,
            Vec::new(),
            Vec::new(),
        );
        let mut scenario_error = match scenario {
            Ok(mission) => {
                map.mission = Some(mission);
                None
            }
            Err(error) => {
                map.ai.clear();
                Some(error.to_string())
            }
        };
        if scenario_error.is_none() {
            scenario_error = audio.err().map(|e| format!("scenario audio: {e}"));
        }
        if scenario_error.is_none()
            && sections
                .get("UNIS")?
                .get(..228)
                .context("truncated unit settings")?
                .contains(&0)
        {
            scenario_error = Some("custom scenario unit statistics are not supported yet".into());
        }
        let scenario_spawns = map.spawns.clone();
        if scenario_error.is_none() {
            scenario_error = World::new(rules.clone(), map.clone(), 0)
                .err()
                .map(|e| format!("scenario placement: {e}"));
        }
        // Ordinary matches use their own base/worker start. Preserve authored
        // scenario armies separately, including unusual editor placements.
        map.spawns
            .retain(|s| usize::from(s.owner.0) >= starts.len());
        map.terrain = None;
        if let Some(mission) = map.mission.take() {
            files.insert("mission.ron".into(), ron_bytes(&mission)?);
        }
        let mut media: straterust_engine::media::MediaManifest =
            ron::de::from_bytes(&files["media.ron"])?;
        media.mission_texts = refs.texts.clone();
        files.insert("media.ron".into(), ron_bytes(&media)?);
        files.insert("map.ron".into(), ron::ser::to_string(&map)?.into_bytes());
        files.insert(
            "manifest.ron".into(),
            format!("(schema_version:1,id:{map_id:?})").into_bytes(),
        );
        let setup = MapSetup {
            terrain_grid: None,
            scenario_spawns: Some(scenario_spawns),
            selectable_factions: starts
                .iter()
                .map(|u| parsed.races[usize::from(u.owner)] >= 5)
                .collect(),
            factions: starts
                .iter()
                .map(|u| match parsed.races[usize::from(u.owner)] {
                    0 => 1,
                    2 => 2,
                    _ => 0,
                })
                .collect(),
            resources: vec![catalog.resources[0].amounts.clone(); starts.len()],
            variants: Vec::new(),
            scenario_error,
        };
        files.insert("match.ron".into(), ron_bytes(&setup)?);
        let package = format!("multiplayer/map-{}", &hash[..16]);
        shared(&output.join(&package), cache.path(), &files)?;
        Package::load(&output.join(&package))?;
        catalog.maps.push(MapEntry {
            title,
            package,
            players: starts.len() as u8,
            size: [parsed.width, parsed.height],
            tileset: parsed.tileset as u8,
        });
    }
    catalog.maps.sort_by(|a, b| a.title.cmp(&b.title));
    catalog.validate()?;
    fs::write(output.join("multiplayer.ron"), ron_bytes(&catalog)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires STRATERUST_STARCRAFT_SOURCE pointing to the retail ISO"]
    fn retail_multiplayer_maps_prepare_each_race_with_native_art_and_settings() -> Result<()> {
        let source = std::env::var_os("STRATERUST_STARCRAFT_SOURCE").context("set source")?;
        let source = Path::new(&source);
        // Large retail map images do not fit reliably in a memory-backed /tmp.
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/research");
        fs::create_dir_all(&local)?;
        let output = tempfile::Builder::new()
            .prefix(".multiplayer-review-")
            .tempdir_in(local)?;
        publish(&crate::inspect(source)?, source, output.path(), &|s| {
            println!("{s}")
        })?;
        let catalog = Catalog::load(output.path())?;
        ensure!(catalog.maps.len() == 69, "retail map inventory changed");
        for (index, entry) in catalog.maps.iter().enumerate() {
            for faction in 0..3 {
                let mut settings = catalog.defaults(index);
                settings.random_locations = false;
                for slot in &mut settings.slots {
                    slot.faction = Some(faction);
                }
                let occupied = (0..usize::from(entry.players)).collect::<Vec<_>>();
                let prepared = catalog
                    .prepare(output.path(), index, &settings, &occupied, 42)
                    .with_context(|| format!("{} race {faction}", entry.title))?;
                prepared
                    .map_transfer()
                    .with_context(|| format!("{} race {faction}: map transfer", entry.title))?;
            }
        }
        for faction in &catalog.factions {
            straterust_engine::assets::AssetPack::load(&output.path().join(&faction.presentation))?;
        }
        Ok(())
    }
}
