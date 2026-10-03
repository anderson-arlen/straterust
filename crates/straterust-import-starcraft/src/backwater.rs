//! Direct translation of the original Windows v1.00 Terran mission 2.
//! Only the source records used by this mission are accepted. Unknown mechanics
//! fail the import instead of silently replacing the campaign with a scenario.
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use crate::{
    Archive, Files, MapReport, MemberReport, Payload, Source, append_report,
    map_formats::{self, DecodedTerrain, ParsedMap},
    member, ron_bytes,
};
use anyhow::{Context, Result, bail, ensure};
use serde::Serialize;
use straterust_engine::{
    assets::{AssetManifest, MapImageManifest},
    map::Terrain,
    media::{AudioRef, MediaManifest, decode_wav, encode_wav},
    scenario::Scenario,
    sim::{
        Footprint, Map, Mission, MissionAction, MissionComparison, MissionCondition,
        MissionLocation, MissionTrigger, MissionUnits, PlayerId, Position, ResourceAmount,
        ResourceSpawn, Rules, Spawn, StartLocation, UnitType, UnitTypeId, World,
    },
};

const MEMBER: &str = "campaign\\terran\\terran02\\staredit\\scenario.chk";
const UNIT_IDS: [(u16, u16); 18] = [
    (0, 1),
    (7, 2),
    (106, 3),
    (109, 4),
    (111, 5),
    (37, 6),
    (38, 7),
    (130, 8),
    (143, 9),
    (19, 10),
    (32, 11),
    (112, 12),
    (125, 13),
    (110, 14),
    (122, 15),
    (107, 16),
    (101, 17),
    (13, 18),
];
const PLAYER_IDS: [(u8, u16); 4] = [(1, 0), (4, 1), (2, 2), (3, 3)];

#[derive(Clone, Debug, Serialize)]
pub(super) struct SourceCondition {
    pub(super) location: u32,
    pub(super) player: u32,
    pub(super) amount: u32,
    pub(super) unit: u16,
    pub(super) comparison: u8,
    pub(super) kind: u8,
    pub(super) switch: u8,
    pub(super) flags: u8,
}
#[derive(Clone, Debug, Serialize)]
pub(super) struct SourceAction {
    pub(super) location: u32,
    pub(super) text: u32,
    pub(super) sound: u32,
    pub(super) time: u32,
    pub(super) player: u32,
    pub(super) second: u32,
    pub(super) unit: u16,
    pub(super) kind: u8,
    pub(super) modifier: u8,
    pub(super) flags: u8,
}
#[derive(Debug, Serialize)]
pub(super) struct SourceTrigger {
    pub(super) conditions: Vec<SourceCondition>,
    pub(super) actions: Vec<SourceAction>,
    pub(super) owners: Vec<u8>,
}
#[derive(Serialize)]
pub(super) struct Availability {
    source_player: u8,
    enabled_units: Vec<u16>,
    upgrades: Vec<[u8; 2]>,
    technologies: Vec<[u8; 2]>,
}
#[derive(Serialize)]
struct ReferenceReport<'a> {
    schema_version: u32,
    members: Vec<MemberReport>,
    source_to_native_players: &'a [(u8, u16)],
    source_to_native_units: &'a [(u16, u16)],
    source_entity_serials: Vec<u32>,
    locations: Vec<(u16, MissionLocation)>,
    availability: Vec<Availability>,
    movement: Vec<crate::terran_data::ReferenceMotion>,
    triggers: Vec<SourceTrigger>,
    briefing: Vec<SourceTrigger>,
    evidence: Vec<&'static str>,
    engine_gaps: Vec<&'static str>,
}

pub fn convert(payload: &Payload, source_path: &Path) -> Result<Files> {
    let source = Source::open(source_path)?;
    let mut installer = Archive::open_region(&source.path, source.offset, source.len)?;
    let mut members = Vec::new();
    let chk = member(
        &mut installer,
        MEMBER,
        8 * 1024 * 1024,
        "install",
        "original Terran mission 2",
        &mut members,
    )?;
    let parsed = map_formats::parse_chk(&chk)?;
    ensure!(
        (parsed.width, parsed.height) == (64, 64)
            && parsed.owners == [0, 6, 3, 3, 5, 0, 0, 0, 0, 0, 0, 0],
        "unsupported Backwater dimensions or controllers"
    );
    let sections = Sections::read(&chk)?;
    let strings = read_strings(sections.get("STR ")?)?;
    let triggers = read_triggers(sections.get("TRIG")?, false)?;
    let briefing = read_triggers(sections.get("MBRF")?, true)?;
    ensure!(
        triggers.len() == 15 && briefing.len() == 1,
        "unsupported Backwater trigger inventory"
    );
    let locations = read_locations(sections.exact("MRGN", 1280)?)?;
    let availability = read_availability(&sections)?;
    check_defaults(&sections)?;
    let terrain = map_formats::decode_terrain(&parsed, &payload.cv5, &payload.vf4)?;
    let mut files = crate::terran::convert(payload, source_path)?;
    crate::zerg::convert(source_path, &mut files)?;
    crate::mission_terran::convert(source_path, &mut files)?;
    crate::flight::convert(source_path, &mut files)?;
    let mut rules: Rules = ron::de::from_bytes(&files["rules.ron"])?;
    rules.id = "straterust.backwater-station".into();
    rules.tick_ms = 42;
    rules.victory = false;
    rules.starting_resources.clear();
    rules.units.push(UnitType {
        id: UnitTypeId(17),
        revealer: true,
        speed: 0,
        vision_range: 320,
        ..UnitType::default()
    });
    let stardat = member(
        &mut installer,
        "files\\stardat.mpq",
        128 * 1024 * 1024,
        "install",
        "mission unit sight ranges",
        &mut members,
    )?;
    let mut archive = Archive::from_bytes(stardat)?;
    let unit_data = member(
        &mut archive,
        "arr\\units.dat",
        19192,
        "stardat",
        "mission unit sight ranges",
        &mut members,
    )?;
    ensure!(unit_data.len() == 19192, "unsupported units.dat layout");
    let flingy = member(
        &mut archive,
        "arr\\flingy.dat",
        2760,
        "stardat",
        "campaign fixed-point movement and acceleration",
        &mut members,
    )?;
    let scripts = member(
        &mut archive,
        "scripts\\iscript.bin",
        64 * 1024,
        "stardat",
        "campaign walking strides and repeat-attack fire points",
        &mut members,
    )?;
    let movement =
        crate::terran_data::decode_motion(&unit_data, &flingy, &scripts, &[0, 7, 19, 32, 37, 38])?;
    crate::terran_data::apply_motion(&mut rules, &movement, &UNIT_IDS)?;
    crate::terran_data::apply_attack_timing(&mut rules, &scripts, &UNIT_IDS)?;
    crate::terran_data::apply_acquisition(&mut rules, &unit_data, &UNIT_IDS)?;
    let mut assets: AssetManifest = ron::de::from_bytes(&files["assets.ron"])?;
    // The base Terran conversion uses the standalone fixture clock. The campaign
    // runs the same verified mobile pose steps at the original fastest speed.
    for clip in assets
        .clips
        .iter_mut()
        .chain(assets.extra_units.iter_mut().flat_map(|s| &mut s.clips))
    {
        if matches!(
            clip.kind,
            straterust_engine::assets::ClipKind::Walk | straterust_engine::assets::ClipKind::Attack
        ) {
            clip.frame_ms = rules.tick_ms;
        }
    }
    files.insert("assets.ron".into(), ron_bytes(&assets)?);
    for &(source, native) in &UNIT_IDS {
        rules
            .units
            .iter_mut()
            .find(|unit| unit.id == UnitTypeId(native))
            .context("missing campaign unit definition")?
            .vision_range = u32::from(unit_data[0x1e24 + usize::from(source)]) * 32;
    }
    apply_campaign_economy(&mut rules)?;
    // PUNI governs whether a new unit can be created, not whether a rescued
    // structure can operate. Keep Academy/Bunker definitions and placed units.
    let allowed = &availability
        .iter()
        .find(|a| a.source_player == 1)
        .context("missing human availability")?
        .enabled_units;
    let enabled: BTreeSet<_> = UNIT_IDS
        .iter()
        .filter(|(source, _)| allowed.contains(source))
        .map(|(_, native)| UnitTypeId(*native))
        .collect();
    for unit in &mut rules.units {
        unit.builds.retain(|id| enabled.contains(id));
        unit.trains.retain(|id| enabled.contains(id));
    }
    convert_research(
        &mut archive,
        &unit_data,
        &availability,
        &mut rules,
        &mut files,
        &mut members,
    )?;
    let mut map = convert_map(
        &parsed,
        &terrain,
        sections.exact("UNIT", parsed.units.len() * 36)?,
    )?;
    let mut refs = References::collect(&triggers, &briefing, &strings)?;
    refs.extract_audio(&mut installer, &strings, &mut files, &mut members, 2)?;
    let mission = translate_mission(&triggers, &locations, &refs)?;
    map.mission = Some(mission.clone());
    World::new(rules.clone(), map.clone(), 42)
        .context("validate original Backwater placements and mission")?;
    files.insert(
        "manifest.ron".into(),
        b"(schema_version:1,id:\"straterust.backwater-station\")\n".to_vec(),
    );
    files.insert("rules.ron".into(), ron_bytes(&rules)?);
    files.insert("map.ron".into(), ron_bytes(&map)?);
    files.insert("mission.ron".into(), ron_bytes(&mission)?);
    // An empty deterministic startup recording does not add commands to the mission.
    files.insert(
        "scenario.ron".into(),
        ron_bytes(&Scenario {
            schema_version: 1,
            seed: 42,
            ticks: 600,
            commands: Vec::new(),
        })?,
    );
    write_presentation(&mut files, &briefing, &refs)?;
    convert_decorations(
        &mut archive,
        sections.get("THG2")?,
        &mut files,
        &mut members,
    )?;
    let unique_megatiles = crate::write_map_terrain(payload, &mut files, &parsed, &terrain)?;
    let source_entity_serials = parsed
        .units
        .iter()
        .filter(|unit| !matches!(unit.unit_type, 176..=178 | 188 | 214))
        .map(|unit| unit.serial)
        .collect();
    files.insert("backwater-reference.ron".into(),ron_bytes(&ReferenceReport {
        schema_version:1,members,source_to_native_players:&PLAYER_IDS,source_to_native_units:&UNIT_IDS,
        source_entity_serials,locations:locations.into_iter().collect(),availability,movement,triggers,briefing,
        evidence:vec![
            "Original Windows v1.00 mission CHK, all active players and all placed gameplay units. Source player slots 1/4/2/3 become native 0/1/2/3. Coordinates and supported UNIT properties are preserved.",
            "Campaign rules use 42 ms native ticks, a 31-frame trigger poll and 42 ms trigger wait steps. This mission replaces the standalone Terran fixture map, starting economy and scenario; fixture-only limitations in terran-reference.ron do not describe the final campaign.",
            "Movement records preserve original flingy top speed/acceleration and IScript move/wait1 strides. Marine/Firebat use 4 pixels per frame; SCV uses top speed 1280/256 and acceleration 67/256; Raynor uses 1707/256 and 100/256. Zergling strides [2,8,9,5,6,7,2] total 39 pixels per seven frames; Hydralisk [2,2,2,6,6,6,2] totals 26. Native navigation uses normalized fixed-point movement; source steering, braking and collision behavior remain approximations.",
            "Source mineral work timer is 75 frames, gas 37, cargo 8 and depleted gas yield 2. Original executable instructions and OpenBW order_MiningMinerals/order_HarvestGas confirm these values. Original contention permits one active worker per resource, with queued handoff; missing-target mineral search is 12 tiles and busy-target search 8 tiles. Native scheduling and resource approach/exit remain approximations; no original-runtime speed measurement has been performed.",
            "Native repeat-attack strike delays preserve source waits: Marine/SCV/Raynor/Hydralisk 1 frame, Zergling 2, Firebat 1/3/4. Original executable VA0x4188a7..0x4188be confirms per-shot cooldown variation -1..+2. Base source cooldowns are Marine/SCV/Hydralisk 15, Zergling 8 and Raynor/Firebat 22. Acquisition is max(source DAT seek range, weapon range): Marine128/SCV32/Raynor160/Firebat96/Zergling96/Hydralisk128 pixels, replacing authored256-pixel enemy aggression. Native RNG sequencing, projectile travel, facing and original order-dispatch cadence remain uncalibrated. Mobile walk/attack clips use42ms wait1 poses, including repeat-attack prefix waits.",
            "All 15 source TRIG records and the original MBRF briefing are translated directly. No authored attacks, player commands, rescue omissions or substitute victory conditions are added.",
            "Original executable actions 11/44 at handler 0x4acc00 creates exactly one unit per resolved player; property 0 means no override. Action 23 at handler 0x4ad4b0 removes all matching units.",
            "Original action 7 at handler 0x4ac490: modifier 7 sets duration from field 20, 8 adds field 20 to WAV duration field 12, and 9 subtracts with zero saturation.",
            "Research imports upgrades.dat entries 0/7/16 and techdata.dat entry 0, bounded by source UPGR/PTEC. Infantry armor and damage recipients come from units.dat/weapons.dat. U-238 grants 32 range; Stim spends 10 HP and lasts 37 × 8 native frames, omitting the original per-unit 0..7 frame status-cycle phase. Source behavior reference: OpenBW actions.h action_stim_pack, bwgame.h weapon_max_range/update_unit_status_timers.",
            "Human PUNI restrictions apply to creation, preserving use of rescued Academy/Bunkers. Source UNIS/UPGS/TECS use DAT defaults; UPGR/PTEC effective player values are recorded.",
            "All 23 static THG2 decorations retain source body pixels and source shadow coverage. Native RGBA uses black at 50% opacity for drawing mode 10 shadows; the original destination-palette darkening cannot be represented exactly by an RGBA image.",
            "MASK begins unexplored. Rescuable players are neutral to all active players without shared vision. The source has no enemy AI-script or wave actions.",
            "Raynor retains the source hero's three Spider Mines without requiring research; source 13 is native 18 and has no initial placed instance. Reachable Command Center/Barracks/Engineering Bay lift/land rules and source poses are imported separately in flight-reference.ron.",
        ],
        engine_gaps: vec![
            "Source speed/acceleration, status-cycle phase, fog edge shapes and indexed-palette visual effects still require reference calibration. Source doodad shadow coverage is preserved with approximate RGBA opacity.",
        ],
    })?);
    append_report(
        &mut files,
        payload,
        Some(MapReport {
            member: MEMBER.into(),
            scm_blake3: None,
            chk_blake3: blake3::hash(&chk).to_hex().to_string(),
            dimensions_tiles: [parsed.width, parsed.height],
            unique_megatiles,
            source_unit_records: parsed.units.len(),
            resources: map.resources.len(),
            starts: map.start_locations.len(),
            added_preview_marines: 0,
            owners: parsed.owners,
            races: parsed.races,
            sections: parsed
                .sections
                .iter()
                .map(|s| (s.name.clone(), s.bytes))
                .collect(),
            unconverted: Vec::new(),
        }),
        Some(
            "Original Backwater Station campaign: source terrain, placements, properties, rescue players, triggers, briefing and objectives. No authored replacement waves or omitted mission units.",
        ),
    )?;
    Ok(files)
}

pub(super) fn apply_campaign_economy(rules: &mut Rules) -> Result<()> {
    use straterust_engine::sim::Extraction;
    for unit in &mut rules.units {
        match unit.id.0 {
            2 => {
                let worker = unit.worker.as_mut().context("SCV worker role missing")?;
                if !worker.resource_kinds.iter().any(|kind| kind == "gas") {
                    worker.resource_kinds.push("gas".into());
                }
            }
            3 => {
                if !unit.dropoff.iter().any(|kind| kind == "gas") {
                    unit.dropoff.push("gas".into());
                }
                if !unit.builds.contains(&UnitTypeId(16)) {
                    unit.builds.push(UnitTypeId(16));
                }
            }
            14 => {
                unit.extracts = Some(Extraction {
                    resource: "gas".into(),
                    harvest_ticks: 37,
                    depleted_amount: 2,
                })
            }
            16 => {
                unit.addon_parent = Some(UnitTypeId(3));
                unit.scanner = Some(straterust_engine::sim::Scanner {
                    energy_max: 200,
                    energy_initial: 50,
                    energy_regeneration: 8,
                    cost: 75,
                    radius: 320,
                    duration: 156,
                });
            }
            _ => {}
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests;

mod graphics;
pub(crate) use graphics::*;

mod chk;
pub(crate) use chk::*;

mod triggers;
pub(crate) use triggers::*;
