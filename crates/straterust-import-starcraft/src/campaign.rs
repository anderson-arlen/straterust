//! First five retail Terran CHKs; source player/force references are resolved
//! during import, leaving only native content and bounded mission/AI programs.
use crate::{
    Archive, Files, Payload, Source,
    backwater::{self, References, Sections, SourceTrigger, short, word},
    campaign_units::{self, MAPPING},
    map_formats, ron_bytes,
};
use anyhow::{Context, Result, bail, ensure};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};
use straterust_engine::{
    assets::AssetManifest, map::Terrain, media::MediaManifest, scenario::Scenario, sim::*,
};

pub const TITLES: [&str; 5] = [
    "Wasteland",
    "Backwater Station",
    "Desperate Alliance",
    "The Jacobs Installation",
    "Revolution",
];

/// Validate the complete campaign beside its destination before publishing it.
/// A failed fifth import must not leave a seemingly usable partial campaign.
pub fn publish(payload: &Payload, source: &Path, output: &Path) -> Result<bool> {
    use std::fs;
    use straterust_engine::content::{Campaign, CampaignMission};
    ensure!(
        output.file_name().is_some(),
        "output must name a campaign directory"
    );
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let stage = tempfile::Builder::new()
        .prefix(".straterust-campaign-")
        .tempdir_in(parent)?;
    let mut campaign = Campaign {
        schema_version: 1,
        id: "straterust.terran-first-five".into(),
        missions: Vec::new(),
    };
    for number in 1..=5 {
        let package = format!("terran{number:02}");
        let files = convert(payload, source, number)
            .with_context(|| format!("convert Terran mission {number}"))?;
        crate::publish(&stage.path().join(&package), &files)?;
        campaign.missions.push(CampaignMission {
            title: TITLES[usize::from(number - 1)].into(),
            package,
        });
    }
    campaign.validate()?;
    fs::write(stage.path().join("campaign.ron"), ron_bytes(&campaign)?)?;
    match fs::symlink_metadata(output) {
        Ok(metadata) => {
            ensure!(
                metadata.is_dir() && !metadata.file_type().is_symlink(),
                "existing campaign output is not a regular directory"
            );
            ensure!(
                same_directory(stage.path(), output)?,
                "existing campaign differs; choose a new output directory"
            );
            Ok(false)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            fs::rename(stage.path(), output)?;
            Ok(true)
        }
        Err(e) => Err(e).context("inspect campaign output"),
    }
}

fn same_directory(a: &Path, b: &Path) -> Result<bool> {
    use std::fs;
    if fs::read_dir(a)?.count() != fs::read_dir(b)?.count() {
        return Ok(false);
    }
    for entry in fs::read_dir(a)? {
        let entry = entry?;
        let other = b.join(entry.file_name());
        let Ok(meta) = fs::symlink_metadata(&other) else {
            return Ok(false);
        };
        if meta.file_type().is_symlink() {
            return Ok(false);
        }
        let kind = entry.file_type()?;
        if kind.is_dir() {
            if !meta.is_dir() || !same_directory(&entry.path(), &other)? {
                return Ok(false);
            }
        } else if !kind.is_file()
            || !meta.is_file()
            || entry.metadata()?.len() != meta.len()
            || fs::read(entry.path())? != fs::read(other)?
        {
            return Ok(false);
        }
    }
    Ok(true)
}
pub fn convert(payload: &Payload, path: &Path, number: u8) -> Result<Files> {
    ensure!(
        (1..=5).contains(&number),
        "only Terran missions 1..=5 are supported"
    );
    let source = Source::open(path)?;
    let mut installer = Archive::open_region(&source.path, source.offset, source.len)?;
    let member = format!("campaign\\terran\\terran{number:02}\\staredit\\scenario.chk");
    let chk = installer.read_file(&member, 8 * 1024 * 1024)?;
    let sections = Sections::read(&chk)?;
    let parsed = map_formats::parse_chk(&chk)?;
    let human = parsed
        .owners
        .iter()
        .position(|o| *o == 6)
        .context("mission has no human controller")? as u8;
    let forces = sections.exact("FORC", 20)?;
    let mut players = vec![human];
    players.extend((0_u8..8).filter(|p| *p != human && parsed.owners[usize::from(*p)] != 0));
    if parsed
        .units
        .iter()
        .any(|u| u.owner == 11 && !matches!(u.unit_type, 176..=178 | 188))
    {
        players.push(11);
    }
    let ids: BTreeMap<_, _> = players
        .iter()
        .enumerate()
        .map(|(n, p)| (*p, PlayerId(n as u16)))
        .collect();
    let native = |id: u32| -> Result<PlayerId> {
        ids.get(&u8::try_from(id)?)
            .copied()
            .with_context(|| format!("inactive mission player {id}"))
    };
    let mut files = backwater::convert(payload, path)?;
    let stardat = installer.read_file("files\\stardat.mpq", 128 * 1024 * 1024)?;
    let mut archive = Archive::from_bytes(stardat)?;
    let selected: &[u16] = match number {
        1 | 2 => &[],
        3 => &[2, 11, 41, 42, 43, 124, 131, 135, 141, 142, 146, 149],
        4 => &[
            1, 2, 3, 15, 20, 89, 95, 195, 203, 205, 206, 207, 208, 209, 211, 212, 218,
        ],
        5 => &[2, 3, 5, 8, 11, 16, 20, 113, 114, 115, 120, 124, 195],
        _ => unreachable!(),
    };
    campaign_units::convert(&mut archive, &mut files, selected)?;
    {
        let mut assets = ron::de::from_bytes(&files["assets.ron"])?;
        let mut rules = ron::de::from_bytes(&files["rules.ron"])?;
        crate::flight::refresh(&mut archive, &mut files, &mut assets, &mut rules)?;
        campaign_units::refresh_research(&mut archive, &mut files, &mut assets, &mut rules, &chk)?;
        files.insert("rules.ron".into(), ron_bytes(&rules)?);
        files.insert("assets.ron".into(), ron_bytes(&assets)?);
    }
    if number >= 3 {
        campaign_units::add_mengsk(&mut archive, &mut files)?;
    }
    let mut rules: Rules = ron::de::from_bytes(&files["rules.ron"])?;
    rules
        .units
        .iter_mut()
        .find(|u| u.id == UnitTypeId(2))
        .unwrap()
        .phases_while_gathering = true;
    let id = format!("straterust.terran-{number:02}");
    rules.id = id.clone();
    rules.victory = false;
    // Restore base production paths pruned by the narrower Mission 2 import.
    rules
        .units
        .iter_mut()
        .find(|u| u.id == UnitTypeId(2))
        .unwrap()
        .builds
        .extend([
            UnitTypeId(12),
            UnitTypeId(13),
            UnitTypeId(14),
            UnitTypeId(15),
        ]);
    for u in &mut rules.units {
        u.builds.sort();
        u.builds.dedup();
        u.trains.sort();
        u.trains.dedup();
    }
    let terrain_payload;
    let terrain_source = if number == 4 {
        let read =
            |archive: &mut Archive<std::io::Cursor<Vec<u8>>>, ext: &str| -> Result<Vec<u8>> {
                archive.read_file(&format!("tileset\\install.{ext}"), 8 * 1024 * 1024)
            };
        terrain_payload = Payload {
            inventory: crate::inspect(path)?.inventory,
            wpe: read(&mut archive, "wpe")?,
            vx4: read(&mut archive, "vx4")?,
            vr4: read(&mut archive, "vr4")?,
            cv5: read(&mut archive, "cv5")?,
            vf4: read(&mut archive, "vf4")?,
            grp: payload.grp.clone(),
        };
        &terrain_payload
    } else {
        payload
    };
    let terrain = map_formats::decode_terrain(&parsed, &terrain_source.cv5, &terrain_source.vf4)?;
    let mut map = Map {
        id: id.clone(),
        width: i32::from(parsed.width) * 32,
        height: i32::from(parsed.height) * 32,
        players: players.len() as u16,
        spawns: Vec::new(),
        resources: Vec::new(),
        start_locations: Vec::new(),
        fog_of_war: true,
        mission: None,
        initial_explored: Default::default(),
        creation: Default::default(),
        ai: Vec::new(),
        terrain: Some(Terrain {
            cell_size: 8,
            columns: u32::from(parsed.width) * 4,
            rows: u32::from(parsed.height) * 4,
            flags: terrain.flags.clone(),
        }),
    };
    for (unit, r) in parsed
        .units
        .iter()
        .zip(sections.get("UNIT")?.as_chunks::<36>().0)
    {
        let position = Position {
            x: i32::from(unit.x),
            y: i32::from(unit.y),
        };
        if let Some(resource) = placed_resource(unit)? {
            map.resources.push(resource);
        }
        match unit.unit_type {
            176..=178 | 188 => {}
            214 => {
                if let Some(&player) = ids.get(&unit.owner) {
                    map.start_locations.push(StartLocation { player, position });
                }
            }
            _ => {
                let states = short(r, 12) & short(r, 26);
                map.spawns.push(Spawn {
                    owner: native(u32::from(unit.owner))?,
                    unit_type: campaign_units::native_id(unit.unit_type)
                        .context("unsupported mission placement")?,
                    position,
                    hp_percent: (short(r, 14) & 2 != 0).then_some(r[17]),
                    energy_percent: (short(r, 14) & 8 != 0).then_some(r[19]),
                    invincible: states & 16 != 0 || matches!(unit.unit_type, 195 | 218),
                    burrowed: states & 2 != 0,
                    doodad_enabled: None,
                });
            }
        }
    }
    map.start_locations.sort_by_key(|s| s.player);
    let mask = sections.exact(
        "MASK",
        usize::from(parsed.width) * usize::from(parsed.height),
    )?;
    for (&source, &player) in &ids {
        if source < 8 {
            let cells: Vec<_> = mask
                .iter()
                .enumerate()
                .filter(|(_, v)| **v & (1 << source) == 0)
                .map(|(i, _)| i as u32)
                .collect();
            if !cells.is_empty() {
                map.initial_explored.insert(player, cells);
            }
        }
    }
    let availability = sections.exact("PUNI", 5700)?;
    for (&source, &player) in &ids {
        let p = usize::from(source);
        let enabled = MAPPING
            .iter()
            .filter(|(source, _)| {
                let unit = usize::from(*source);
                if availability[2964 + p * 228 + unit] != 0 {
                    availability[2736 + unit] != 0
                } else {
                    availability[p * 228 + unit] != 0
                }
            })
            .filter(|(_, id)| rules.units.iter().any(|u| u.id == UnitTypeId(*id)))
            .map(|(_, id)| UnitTypeId(*id))
            .collect();
        map.creation.insert(player, enabled);
    }
    let locations = backwater::read_locations(sections.exact("MRGN", 1280)?)?;
    let triggers = backwater::read_triggers(sections.get("TRIG")?, false)?;
    let briefing = backwater::read_triggers(sections.get("MBRF")?, true)?;
    let strings = backwater::read_strings(sections.get("STR ")?)?;
    let mut media: MediaManifest = ron::de::from_bytes(&files["media.ron"])?;
    media.mission_audio.clear();
    media.mission_texts.clear();
    media.briefing.clear();
    files.insert("media.ron".into(), ron_bytes(&media)?);
    let mut refs = References::collect(&triggers, &briefing, &strings)?;
    let mut members = Vec::new();
    refs.extract_audio(&mut installer, &strings, &mut files, &mut members, number)?;
    let mut alliances = Vec::new();
    let rescue: Vec<_> = players
        .iter()
        .filter(|p| parsed.owners[usize::from(**p)] == 3)
        .map(|p| ids[p])
        .collect();
    for a in 0..players.len() {
        for b in a + 1..players.len() {
            let sa = usize::from(players[a]);
            let sb = usize::from(players[b]);
            if sa == 11
                || sb == 11
                || matches!(parsed.owners[sa], 3 | 7)
                || matches!(parsed.owners[sb], 3 | 7)
            {
                alliances.push([PlayerId(a as u16), PlayerId(b as u16)]);
            }
        }
    }
    // Computer players retain their placed guards even before town scripts start.
    for &p in &players {
        if parsed.owners[usize::from(p)] == 5 {
            let player = ids[&p];
            let home = map
                .start_locations
                .iter()
                .find(|s| s.player == player)
                .map(|s| s.position)
                .unwrap_or(Position {
                    x: map.width / 2,
                    y: map.height / 2,
                });
            map.ai.push(AiController {
                player,
                home,
                radius: 8192,
                active: false,
                program: Vec::new(),
            });
        }
    }
    let ai = archive.read_file("scripts\\aiscript.bin", 65536)?;
    let mission = translate(
        &triggers, &locations, &refs, &players, &ids, forces, &ai, &mut map, rescue, alliances,
    )?;
    map.mission = Some(mission.clone());
    import_things(
        &mut archive,
        &mut files,
        sections.get("THG2")?,
        &ids,
        &mut map,
        number,
    )?;
    World::new(rules.clone(), map.clone(), 42).context("validate campaign gameplay")?;
    files.insert(
        "manifest.ron".into(),
        format!("(schema_version:1,id:{id:?})\n").into_bytes(),
    );
    files.insert("rules.ron".into(), ron_bytes(&rules)?);
    files.insert("map.ron".into(), ron_bytes(&map)?);
    files.insert("mission.ron".into(), ron_bytes(&mission)?);
    files.insert(
        "scenario.ron".into(),
        ron_bytes(&Scenario {
            schema_version: 1,
            seed: 42,
            ticks: 600,
            commands: Vec::new(),
        })?,
    );
    backwater::write_presentation(&mut files, &briefing, &refs)?;
    let unique_megatiles = crate::write_map_terrain(terrain_source, &mut files, &parsed, &terrain)?;
    files.insert("campaign-reference.ron".into(),ron_bytes(&(number,TITLES[usize::from(number-1)],member,blake3::hash(&chk).to_hex().to_string(),&ids,&triggers,&briefing,&map.ai,&members,vec!["Original placements, source force/controller references, briefing, enabled triggers and objectives are translated. Source AI build/attack counts and wait operands are retained in bounded native programs.","Native town production and guard assistance approximate original engine policies. Zerg production uses paid hatchery queues rather than a larva/morph simulation. Mutalisk bounces, research-dependent cloaking, siege mode and exact creep growth/recession timing remain compatibility work; basic roles retain source art, numeric fields and paid production."]))?);
    files.insert("import-report.ron".into(), ron_bytes(&crate::ImportReport {
        schema_version:1, inventory: &payload.inventory, terrain_tile:None,
        map: Some(crate::MapReport {member: format!("campaign\\terran\\terran{number:02}\\staredit\\scenario.chk"), scm_blake3:None,chk_blake3:blake3::hash(&chk).to_hex().to_string(), dimensions_tiles:[parsed.width,parsed.height], unique_megatiles, source_unit_records:parsed.units.len(),resources:map.resources.len(),starts:map.start_locations.len(),added_preview_marines:0,owners:parsed.owners,races:parsed.races, sections:parsed.sections.iter().map(|s|(s.name.clone(),s.bytes)).collect(),unconverted:Vec::new()}),
        unit_grp_frames:&[], animation:"Native source clips for campaign roles; source iscript is interpreted only during import.",
        gameplay:"First-five Terran campaign content with native paid opponent production, source build/wave scripts, triggers and endings. Remaining mechanics and AI policy differences are listed in campaign-reference.ron.", outputs:Vec::new(),
    })?);
    files.remove("backwater-reference.ron");
    Ok(files)
}

#[allow(clippy::too_many_arguments)]
fn translate(
    triggers: &[SourceTrigger],
    locations: &BTreeMap<u16, MissionLocation>,
    refs: &References,
    source_players: &[u8],
    ids: &BTreeMap<u8, PlayerId>,
    forces: &[u8],
    ai: &[u8],
    map: &mut Map,
    rescue: Vec<PlayerId>,
    alliances: Vec<[PlayerId; 2]>,
) -> Result<Mission> {
    let indices: BTreeMap<_, _> = locations
        .keys()
        .enumerate()
        .map(|(i, id)| (*id, i as u16))
        .collect();
    let loc = |id: u32| -> Result<u16> {
        indices
            .get(&u16::try_from(id)?)
            .copied()
            .context("missing campaign location")
    };
    let unit = |id: u16| -> Result<UnitTypeId> {
        campaign_units::native_id(id)
            .with_context(|| format!("unsupported campaign unit reference {id}"))
    };
    let filter = |id: u16| -> Result<MissionUnits> {
        Ok(match id {
            229 => MissionUnits::Any,
            230 => MissionUnits::Men,
            231 => MissionUnits::Structures,
            _ => MissionUnits::Type(unit(id)?),
        })
    };
    let resolve = |source: u32, owner: u8| -> Result<Vec<PlayerId>> {
        let p: Vec<_> = source_players
            .iter()
            .filter(|p| match source {
                13 => **p == owner,
                17 => true,
                18..=21 => **p < 8 && u32::from(forces[usize::from(**p)]) == source - 18,
                _ => u32::from(**p) == source,
            })
            .map(|p| ids[p])
            .collect();
        ensure!(!p.is_empty(), "empty campaign player reference {source}");
        Ok(p)
    };
    let mut native = Vec::new();
    for trigger in triggers {
        if trigger.actions.is_empty() {
            continue;
        }
        let owners: BTreeSet<_> = trigger
            .owners
            .iter()
            .flat_map(|p| resolve(u32::from(*p), source_players[0]).unwrap_or_default())
            .filter_map(|id| source_players.iter().find(|p| ids[p] == id).copied())
            .filter(|p| *p < 8)
            .collect();
        for owner in owners {
            let mut conditions = Vec::new();
            for c in &trigger.conditions {
                conditions.push(match c.kind {
                    1 => MissionCondition::Countdown {
                        comparison: backwater::comparison(c.comparison)?,
                        milliseconds: c.amount * 1000,
                    },
                    2 | 3 => MissionCondition::Count {
                        players: resolve(c.player, owner)?,
                        units: filter(c.unit)?,
                        location: if c.kind == 3 {
                            Some(loc(c.location)?)
                        } else {
                            None
                        },
                        comparison: backwater::comparison(c.comparison)?,
                        amount: c.amount,
                    },
                    4 => MissionCondition::Resources {
                        players: resolve(c.player, owner)?,
                        kinds: ["minerals", "gas"]
                            .into_iter()
                            .enumerate()
                            .filter(|(kind, _)| c.unit == 2 || c.unit == *kind as u16)
                            .map(|(_, kind)| kind.to_owned())
                            .collect(),
                        comparison: backwater::comparison(c.comparison)?,
                        amount: c.amount,
                    },
                    5 => MissionCondition::Kills {
                        players: resolve(c.player, owner)?,
                        units: filter(c.unit)?,
                        comparison: backwater::comparison(c.comparison)?,
                        amount: c.amount,
                    },
                    11 => MissionCondition::Switch {
                        index: u16::from(c.switch),
                        set: c.comparison == 2,
                    },
                    12 => MissionCondition::Elapsed {
                        comparison: backwater::comparison(c.comparison)?,
                        milliseconds: c.amount * 1000,
                    },
                    _ => bail!("unsupported campaign condition {}", c.kind),
                });
            }
            let mut actions = Vec::new();
            for a in &trigger.actions {
                let action = match a.kind {
                    1 => MissionAction::Victory,
                    2 => MissionAction::Defeat,
                    3 => MissionAction::Preserve,
                    4 => MissionAction::Wait {
                        milliseconds: a.time,
                    },
                    5 => MissionAction::Pause,
                    6 => MissionAction::Resume,
                    7 => MissionAction::Transmission {
                        text: refs.text(a.text)?,
                        sound: if a.sound == 0 {
                            None
                        } else {
                            Some(refs.sound(a.sound)?)
                        },
                        portrait: if matches!(a.unit, 23 | 29) {
                            UnitTypeId(1000)
                        } else if a.unit == 27 {
                            UnitTypeId(1001)
                        } else {
                            unit(a.unit)?
                        },
                        location: loc(a.location)?,
                        milliseconds: backwater::duration(a)?,
                    },
                    8 => MissionAction::Sound {
                        sound: refs.sound(a.sound)?,
                    },
                    9 => MissionAction::Text {
                        text: refs.text(a.text)?,
                    },
                    10 => MissionAction::CenterView {
                        location: loc(a.location)?,
                    },
                    11 => {
                        ensure!(
                            a.second == 0 && a.modifier == 0,
                            "unsupported campaign create properties"
                        );
                        MissionAction::Create {
                            player: resolve(a.player, owner)?[0],
                            unit_type: unit(a.unit)?,
                            location: loc(a.location)?,
                        }
                    }
                    12 => MissionAction::Objectives {
                        text: refs.text(a.text)?,
                    },
                    13 => MissionAction::SetSwitch {
                        index: a.second as u16,
                        set: a.modifier == 4,
                    },
                    14 => {
                        ensure!(a.modifier == 7, "unsupported countdown arithmetic");
                        MissionAction::Countdown {
                            milliseconds: a.time * 1000,
                        }
                    }
                    15 | 16 => {
                        let script = a.second.to_le_bytes();
                        let targets = resolve(13, owner)?;
                        match &script {
                            b"Ter3" | b"Ter5" | b"Te5H" => {
                                let home = locations[&(a.location as u16)].center();
                                let index = map.ai.len() as u16;
                                map.ai.push(AiController {
                                    player: targets[0],
                                    home,
                                    radius: 640,
                                    active: false,
                                    program: crate::ai::translate(ai, script, MAPPING)?,
                                });
                                MissionAction::StartAi { controller: index }
                            }
                            b"Suic" => MissionAction::Assault { players: targets },
                            b"Rscu" => MissionAction::Rescue { players: targets },
                            b"EnBk" => MissionAction::EnterBunkers {
                                players: targets,
                                location: loc(a.location)?,
                            },
                            b"ClrC" => MissionAction::Cosmetic,
                            _ => bail!("unsupported mission AI script {:?}", script),
                        }
                    }
                    17 | 28 | 32 => MissionAction::Cosmetic,
                    22 | 24 => MissionAction::Remove {
                        players: if a.kind == 24 {
                            resolve(17, owner)?
                        } else {
                            resolve(a.player, owner)?
                        },
                        units: filter(a.unit)?,
                        location: None,
                    },
                    23 => MissionAction::Kill {
                        players: resolve(a.player, owner)?,
                        units: filter(a.unit)?,
                        location: loc(a.location)?,
                    },
                    26 => {
                        ensure!(a.modifier == 7, "unsupported resource arithmetic");
                        MissionAction::SetResources {
                            players: resolve(a.player, owner)?,
                            resources: [("minerals", 0), ("gas", 1)]
                                .into_iter()
                                .filter(|(_, kind)| a.unit == 2 || a.unit == *kind)
                                .map(|(kind, _)| ResourceAmount {
                                    kind: kind.into(),
                                    amount: a.second,
                                })
                                .collect(),
                        }
                    }
                    30 | 31 => MissionAction::Speech {
                        muted: a.kind == 30,
                    },
                    38 => MissionAction::MoveLocation {
                        location: loc(a.second)?,
                        players: resolve(a.player, owner)?,
                        units: filter(a.unit)?,
                        search_location: loc(a.location)?,
                    },
                    39 => MissionAction::Teleport {
                        players: resolve(a.player, owner)?,
                        units: filter(a.unit)?,
                        location: loc(a.location)?,
                        destination: loc(a.second)?,
                    },
                    42 => {
                        ensure!(a.modifier == 0, "unsupported doodad state modifier");
                        MissionAction::ToggleDoodad {
                            players: resolve(a.player, owner)?,
                            units: filter(a.unit)?,
                            location: loc(a.location)?,
                        }
                    }
                    43 => MissionAction::Invincibility {
                        players: resolve(a.player, owner)?,
                        units: filter(a.unit)?,
                        location: loc(a.location)?,
                        enabled: a.modifier == 4,
                    },
                    _ => bail!("unsupported campaign action {}", a.kind),
                };
                actions.push(action);
            }
            native.push(MissionTrigger {
                conditions: conditions.clone(),
                actions,
            });
        }
    }
    Ok(Mission {
        schema_version: 1,
        player: PlayerId(0),
        poll_ticks: 31,
        wait_step_ms: 42,
        locations: locations.values().copied().collect(),
        triggers: native,
        rescuable_players: rescue,
        rescuers: vec![PlayerId(0)],
        alliances,
    })
}

fn placed_resource(unit: &map_formats::PlacedUnit) -> Result<Option<ResourceSpawn>> {
    let gas = matches!(unit.unit_type, 110 | 149 | 188);
    if !gas && !matches!(unit.unit_type, 176..=178) {
        return Ok(None);
    }
    // A prebuilt extractor replaces its geyser in UNIT, but its stored gas
    // still belongs to a native resource node at the building's position.
    Ok(Some(ResourceSpawn {
        kind: if gas { "gas" } else { "minerals" }.into(),
        position: Position {
            x: i32::from(unit.x),
            y: i32::from(unit.y),
        },
        amount: unit.resource_amount.context("resource amount missing")?,
        requires_extractor: gas,
        footprint: Footprint {
            width: if gas { 128 } else { 64 },
            height: if gas { 64 } else { 32 },
        },
    }))
}

fn import_things(
    archive: &mut Archive<std::io::Cursor<Vec<u8>>>,
    files: &mut Files,
    records: &[u8],
    ids: &BTreeMap<u8, PlayerId>,
    map: &mut Map,
    number: u8,
) -> Result<()> {
    use straterust_engine::assets::MapImageManifest;
    let mut assets: AssetManifest = ron::de::from_bytes(&files["assets.ron"])?;
    assets.map_images.clear();
    let sprites = archive.read_file("arr\\sprites.dat", 2081)?;
    let images = archive.read_file("arr\\images.dat", 28690)?;
    let table = archive.read_file("arr\\images.tbl", 65536)?;
    let palette = crate::formats::palette(&archive.read_file(
        if number == 4 {
            "tileset\\install.wpe"
        } else {
            "tileset\\badlands.wpe"
        },
        1024,
    )?)?;
    for r in records.as_chunks::<10>().0 {
        let source = short(r, 0);
        let flags = short(r, 8);
        let position = Position {
            x: i32::from(short(r, 2)),
            y: i32::from(short(r, 4)),
        };
        if flags & 0x1000 == 0 {
            map.spawns.push(Spawn {
                owner: *ids.get(&r[6]).context("unknown doodad owner")?,
                unit_type: campaign_units::native_id(source)
                    .context("unsupported interactive doodad")?,
                position,
                doodad_enabled: Some(flags & 0x8000 == 0),
                invincible: matches!(source, 205..=208),
                ..Spawn::default()
            });
            continue;
        }
        let image = usize::from(short(&sprites, usize::from(source) * 2));
        ensure!(image < 755, "invalid decoration image");
        let path = format!(
            "unit\\{}",
            crate::terran_media::table_string(&table, word(&images, image * 4))?
        );
        let frames =
            crate::formats::decode_grp(&archive.read_file(&path, 8 * 1024 * 1024)?, &palette)?;
        let frame = &frames[0];
        let file = format!("campaign-decoration-{source:03}.srim");
        let reference = crate::add_image(files, &file, frame)?;
        assets.map_images.push(MapImageManifest {
            position,
            anchor: [(frame.width / 2) as i32, (frame.height / 2) as i32],
            image: reference,
        });
    }
    files.insert("assets.ron".into(), ron_bytes(&assets)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prebuilt_extractors_preserve_harvestable_gas() {
        use straterust_engine::content::Package;
        let package =
            Package::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/terran-demo"))
                .unwrap();
        let original = package.world(42).unwrap();
        for source in [110, 149] {
            let resource = placed_resource(&map_formats::PlacedUnit {
                serial: 1,
                x: 128,
                y: 128,
                unit_type: source,
                owner: 0,
                resource_amount: Some(24),
            })
            .unwrap()
            .unwrap();
            let mut rules = original.rules().clone();
            rules.victory = false;
            rules
                .units
                .iter_mut()
                .find(|u| u.id == UnitTypeId(2))
                .unwrap()
                .phases_while_gathering = true;
            rules
                .units
                .iter_mut()
                .find(|u| u.id == UnitTypeId(2))
                .unwrap()
                .worker
                .as_mut()
                .unwrap()
                .resource_kinds
                .push("gas".into());
            rules
                .units
                .iter_mut()
                .find(|u| u.id == UnitTypeId(3))
                .unwrap()
                .dropoff
                .push("gas".into());
            rules.units.push(UnitType {
                id: UnitTypeId(14),
                structure: true,
                speed: 0,
                footprint: Footprint {
                    width: 96,
                    height: 48,
                },
                placement: resource.footprint,
                extracts: Some(Extraction {
                    resource: "gas".into(),
                    harvest_ticks: 3,
                    depleted_amount: 2,
                }),
                ..UnitType::default()
            });
            let mut map = original.map().clone();
            map.resources = vec![resource];
            map.spawns = vec![
                Spawn {
                    owner: PlayerId(0),
                    unit_type: UnitTypeId(14),
                    position: Position { x: 128, y: 128 },
                    ..Spawn::default()
                },
                Spawn {
                    owner: PlayerId(0),
                    unit_type: UnitTypeId(2),
                    position: Position { x: 128, y: 184 },
                    ..Spawn::default()
                },
                Spawn {
                    owner: PlayerId(0),
                    unit_type: UnitTypeId(3),
                    position: Position { x: 320, y: 128 },
                    ..Spawn::default()
                },
                Spawn {
                    owner: PlayerId(0),
                    unit_type: UnitTypeId(2),
                    position: Position { x: 128, y: 216 },
                    ..Spawn::default()
                },
            ];
            let mut world = World::new(rules, map, 42).unwrap();
            assert!(
                world.can_place(
                    Position { x: 188, y: 128 },
                    Footprint {
                        width: 23,
                        height: 23
                    },
                    straterust_engine::map::MovementClass::Ground,
                    None
                ),
                "extractor collision replaces the underlying geyser"
            );
            let mut bare_map = world.map().clone();
            bare_map.spawns.remove(0);
            let bare = World::new(world.rules().clone(), bare_map, 42).unwrap();
            assert!(
                !bare.can_place(
                    Position { x: 188, y: 128 },
                    Footprint {
                        width: 23,
                        height: 23
                    },
                    straterust_engine::map::MovementClass::Ground,
                    None
                ),
                "an uncovered geyser still blocks movement"
            );
            let outcome = world
                .step(&[
                    Command {
                        tick: Tick(0),
                        player: PlayerId(0),
                        sequence: 1,
                        order: Order::Gather {
                            entity: EntityId(2),
                            resource: ResourceId(1),
                        },
                    },
                    Command {
                        tick: Tick(0),
                        player: PlayerId(0),
                        sequence: 2,
                        order: Order::Gather {
                            entity: EntityId(4),
                            resource: ResourceId(1),
                        },
                    },
                ])
                .unwrap();
            assert!(outcome[0].rejection.is_none());
            assert!(outcome[1].rejection.is_none());
            for _ in 0..400 {
                if world.resource_balance(PlayerId(0), "gas") >= 24 {
                    break;
                }
                world.step(&[]).unwrap();
            }
            assert_eq!(
                world.resource_balance(PlayerId(0), "gas"),
                24,
                "source extractor {source}"
            );
            assert_eq!(world.state().resources[0].amount, 0);
        }
    }
}
