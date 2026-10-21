//! Native multiplayer catalog and match preparation. Source game formats end
//! at the importers; race rosters and setting values are package data.
use crate::{content::read_ron, sim::*};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

pub mod browser;
mod colors;
mod terrain;
pub use colors::PlayerColor;
pub use terrain::TileMapping;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    pub schema_version: u32,
    pub game: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub colors: Vec<PlayerColor>,
    pub maps: Vec<MapEntry>,
    pub factions: Vec<Faction>,
    pub resources: Vec<ResourcePreset>,
    pub speeds: Vec<Speed>,
    pub modes: Vec<GameType>,
    #[serde(default)]
    pub tilesets: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub terrain_mappings: Vec<TileMapping>,
    #[serde(default)]
    pub allow_cheats: bool,
    #[serde(default)]
    pub random_faction: bool,
    #[serde(default)]
    pub standard_start: bool,
    #[serde(default)]
    pub defeat_without_structures: bool,
    #[serde(default)]
    pub flag_art: Option<FlagArt>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FlagArt {
    pub item: UnitTypeId,
    pub homes: Vec<UnitTypeId>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FlagHome {
    pub item: EntityId,
    pub owner: PlayerId,
    pub position: Position,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MapEntry {
    pub title: String,
    pub package: String,
    pub players: u8,
    pub size: [u16; 2],
    pub tileset: u8,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Faction {
    pub name: String,
    pub presentation: String,
    #[serde(default)]
    pub tileset_presentations: Vec<String>,
    pub worker: UnitTypeId,
    pub starting_units: Vec<(UnitTypeId, Position)>,
    #[serde(default)]
    pub equivalents: BTreeMap<UnitTypeId, UnitTypeId>,
    #[serde(default)]
    pub ai: Vec<AiInstruction>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub ai_difficulties: BTreeMap<AiDifficulty, Vec<AiInstruction>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ai_research: Vec<ResearchId>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ResourcePreset {
    pub name: String,
    pub amounts: Vec<ResourceAmount>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Speed {
    pub name: String,
    pub tick_ms: u32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum GameType {
    Melee,
    FreeForAll,
    OneOnOne,
    TopVsBottom,
    UseMapSettings,
    Greed,
    Slaughter,
    SuddenDeath,
    CaptureTheFlag,
    TeamMelee,
    TeamFreeForAll,
    TeamCaptureTheFlag,
    Teams,
}
impl GameType {
    pub fn name(self) -> &'static str {
        match self {
            Self::Melee => "Melee",
            Self::FreeForAll => "Free For All",
            Self::Teams => "Teams",
            Self::OneOnOne => "One on One",
            Self::TopVsBottom => "Top vs. Bottom",
            Self::UseMapSettings => "Use Map Settings",
            Self::Greed => "Greed",
            Self::Slaughter => "Slaughter",
            Self::SuddenDeath => "Sudden Death",
            Self::CaptureTheFlag => "Capture the Flag",
            Self::TeamMelee => "Team Melee",
            Self::TeamFreeForAll => "Team Free For All",
            Self::TeamCaptureTheFlag => "Team Capture the Flag",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SlotKind {
    Open,
    Computer,
    Closed,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Slot {
    pub kind: SlotKind,
    pub faction: Option<usize>,
    pub team: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<usize>,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum AiDifficulty {
    Easy,
    #[default]
    Normal,
    Hard,
}
impl AiDifficulty {
    pub fn name(self) -> &'static str {
        match self {
            Self::Easy => "Easy",
            Self::Normal => "Normal",
            Self::Hard => "Hard",
        }
    }
    pub fn next(self) -> Self {
        match self {
            Self::Easy => Self::Normal,
            Self::Normal => Self::Hard,
            Self::Hard => Self::Easy,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default)]
    pub ai_difficulty: AiDifficulty,
    pub slots: Vec<Slot>,
    pub mode: usize,
    pub parameter: u32,
    pub resources: Option<usize>,
    pub map_units: bool,
    pub random_locations: bool,
    pub fog: bool,
    pub cheats: bool,
    pub speed: usize,
    pub tileset: Option<usize>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MapSetup {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terrain_grid: Option<crate::assets::TerrainGrid>,
    /// Scenario armies are separate from an ordinary match's starting army.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scenario_spawns: Option<Vec<Spawn>>,
    pub factions: Vec<usize>,
    #[serde(default)]
    pub selectable_factions: Vec<bool>,
    pub resources: Vec<Vec<ResourceAmount>>,
    #[serde(default)]
    pub variants: Vec<String>,
    /// Block unsupported scenario play explicitly, while retaining the map
    /// for ordinary multiplayer matches.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scenario_error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MatchRules {
    pub participants: Vec<PlayerId>,
    pub teams: Vec<Vec<PlayerId>>,
    pub mode: GameType,
    pub parameter: u32,
    pub resources: BTreeMap<PlayerId, Vec<ResourceAmount>>,
    pub bases: Vec<UnitTypeId>,
    pub cheats: bool,
    #[serde(default)]
    pub flags: Vec<FlagHome>,
    #[serde(default)]
    pub defeat_without_structures: bool,
}
impl MatchRules {
    pub(crate) fn validate(&self, rules: &Rules, map: &Map) -> Result<()> {
        let unique: std::collections::BTreeSet<_> = self.participants.iter().copied().collect();
        ensure!(
            (2..=8).contains(&unique.len())
                && unique.len() == self.participants.len()
                && unique.iter().all(|p| p.0 < map.players)
                && self.teams.len() <= 8,
            "invalid match participants"
        );
        let mut teammates = std::collections::BTreeSet::new();
        for team in &self.teams {
            ensure!(
                (1..=8).contains(&team.len())
                    && team
                        .iter()
                        .all(|p| p.0 < map.players && teammates.insert(*p)),
                "invalid match teams"
            );
        }
        ensure!(
            self.resources.len() <= 8
                && self.resources.iter().all(|(p, amounts)| unique.contains(p)
                    && amounts.len() <= 16
                    && amounts
                        .iter()
                        .all(|r| !r.kind.is_empty() && r.kind.len() <= 64))
                && self.bases.len() <= 32
                && self
                    .bases
                    .iter()
                    .all(|id| rules.units.iter().any(|u| u.id == *id && u.structure))
                && self.flags.len() <= 8
                && self.flags.iter().all(|f| unique.contains(&f.owner)
                    && map.contains(f.position)
                    && map
                        .spawns
                        .get(f.item.0.saturating_sub(1) as usize)
                        .is_some_and(|s| s.owner == f.owner
                            && rules
                                .units
                                .iter()
                                .any(|u| u.id == s.unit_type && u.portable)))
                && self.parameter > 0,
            "invalid match objectives or resources"
        );
        Ok(())
    }
    pub fn allied(&self, a: PlayerId, b: PlayerId) -> bool {
        self.teams
            .iter()
            .any(|team| team.contains(&a) && team.contains(&b))
    }
}

pub struct Prepared {
    pub artwork: Option<crate::assets::MapArtwork>,
    pub world: World,
    pub directory: PathBuf,
    pub factions: Vec<usize>,
    pub humans: Vec<PlayerId>,
    pub owners: Vec<Option<PlayerId>>,
    pub tick_ms: u32,
}
impl Catalog {
    pub fn prepare(
        &self,
        root: &Path,
        index: usize,
        settings: &Settings,
        occupied: &[usize],
        seed: u64,
    ) -> Result<Prepared> {
        self.validate_settings(index, settings)?;
        ensure!(
            occupied
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                == occupied.len(),
            "duplicate occupied slots"
        );
        ensure!(
            occupied.len() <= 8
                && occupied.iter().all(|i| settings
                    .slots
                    .get(*i)
                    .is_some_and(|s| s.kind == SlotKind::Open)),
            "invalid occupied slots"
        );
        let entry = &self.maps[index];
        let mut directory = package_path(root, &entry.package)?;
        let setup: MapSetup = read_ron(&directory.join("match.ron"))?;
        ensure!(
            setup.factions.len() == settings.slots.len()
                && setup.resources.len() == settings.slots.len()
                && (setup.selectable_factions.is_empty()
                    || setup.selectable_factions.len() == settings.slots.len())
                && setup.factions.iter().all(|f| *f < self.factions.len()),
            "invalid map setup"
        );
        if let Some(era) = settings.tileset.filter(|_| setup.terrain_grid.is_none()) {
            directory = package_path(
                root,
                setup
                    .variants
                    .get(era)
                    .context("map has no tileset variant")?,
            )?;
        }
        let original = crate::content::Package::load(&directory)?.world(seed)?;
        let mode = self.modes[settings.mode];
        if mode == GameType::UseMapSettings {
            ensure!(
                setup.scenario_error.is_none(),
                "Use Map Settings unavailable: {}",
                setup.scenario_error.as_deref().unwrap_or("")
            );
        }
        let rules = original.rules().clone();
        let mut map = original.map().clone();
        let mut artwork = self.assemble_artwork(
            root,
            &directory,
            &setup,
            settings.tileset.unwrap_or(usize::from(entry.tileset)),
            &mut map,
        )?;
        if !self.colors.is_empty() {
            if artwork.is_none() {
                artwork = crate::assets::MapArtwork::load(&directory)?;
            }
            if let Some(artwork) = &mut artwork {
                self.apply_colors(settings, artwork)?;
            }
        }
        let mut random = seed;
        let mut next = || {
            random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
            random
        };
        let factions: Vec<_> = settings
            .slots
            .iter()
            .enumerate()
            .map(|(i, s)| {
                if mode == GameType::UseMapSettings
                    && !setup.selectable_factions.get(i).copied().unwrap_or(false)
                {
                    return setup.factions[i];
                }
                s.faction.unwrap_or_else(|| {
                    if self.random_faction {
                        next() as usize % self.factions.len()
                    } else {
                        setup.factions[i]
                    }
                })
            })
            .collect();
        let active: Vec<_> = settings
            .slots
            .iter()
            .enumerate()
            .filter(|(_, s)| s.kind != SlotKind::Closed)
            .map(|(i, _)| PlayerId(i as u16))
            .collect();
        ensure!(
            active.len() >= 2
                && (self.modes[settings.mode] != GameType::OneOnOne || active.len() == 2),
            "match needs at least two sides; One on One needs exactly two"
        );
        let mut locations: Vec<_> = (0..settings.slots.len())
            .map(|i| {
                map.start_locations
                    .iter()
                    .find(|s| s.player == PlayerId(i as u16))
                    .map(|s| s.position)
                    .context("map lacks a player start")
            })
            .collect::<Result<_>>()?;
        if settings.random_locations && mode != GameType::UseMapSettings {
            for i in (1..locations.len()).rev() {
                locations.swap(i, next() as usize % (i + 1));
            }
        }
        let mut teams: Vec<Vec<PlayerId>> = Vec::new();
        if matches!(
            mode,
            GameType::Teams
                | GameType::TopVsBottom
                | GameType::TeamMelee
                | GameType::TeamFreeForAll
                | GameType::TeamCaptureTheFlag
        ) {
            let mut groups = BTreeMap::<u8, Vec<PlayerId>>::new();
            for p in &active {
                let slot = &settings.slots[usize::from(p.0)];
                let team = if mode == GameType::TopVsBottom {
                    u8::from(usize::from(p.0) >= usize::from(entry.players).div_ceil(2))
                } else {
                    slot.team
                };
                groups.entry(team).or_default().push(*p);
            }
            teams = groups.into_values().collect();
            ensure!(teams.len() >= 2, "choose at least two teams");
        }
        let shared = matches!(
            mode,
            GameType::TeamMelee | GameType::TeamFreeForAll | GameType::TeamCaptureTheFlag
        );
        if shared {
            ensure!(
                teams.iter().all(|team| {
                    !team.iter().any(|p| occupied.contains(&usize::from(p.0)))
                        || team
                            .iter()
                            .all(|p| settings.slots[usize::from(p.0)].kind != SlotKind::Computer)
                }),
                "shared-control teams cannot mix human and computer slots"
            );
        }
        let owner = |p: PlayerId| {
            if shared {
                teams.iter().find(|t| t.contains(&p)).unwrap()[0]
            } else {
                p
            }
        };
        let defaults =
            mode == GameType::UseMapSettings || (settings.map_units && !self.standard_start);
        let source_spawns = if mode == GameType::UseMapSettings {
            setup
                .scenario_spawns
                .as_deref()
                .unwrap_or(&original.map().spawns)
        } else {
            &original.map().spawns
        };
        ensure!(source_spawns.len() <= 8192, "too many scenario units");
        ensure!(
            source_spawns
                .iter()
                .all(|s| rules.units.iter().any(|u| u.id == s.unit_type)),
            "unknown scenario unit type"
        );
        if mode == GameType::UseMapSettings && setup.scenario_spawns.is_some() {
            map.spawns = source_spawns.to_vec();
        }
        map.spawns
            .retain(|s| usize::from(s.owner.0) >= settings.slots.len());
        if mode != GameType::UseMapSettings {
            map.ai.clear();
            map.creation.clear();
            map.initial_explored.clear();
        }
        map.fog_of_war = settings.fog;
        let mut generated = Vec::new();
        for p in &active {
            let i = usize::from(p.0);
            let faction = &self.factions[factions[i]];
            let source = original
                .map()
                .start_locations
                .iter()
                .find(|s| s.position == locations[i])
                .unwrap()
                .player;
            let mut placed = false;
            if defaults {
                for mut spawn in source_spawns.iter().filter(|s| s.owner == source).cloned() {
                    spawn.owner = *p;
                    if mode != GameType::UseMapSettings
                        && let Some(unit) = faction.equivalents.get(&spawn.unit_type)
                    {
                        spawn.unit_type = *unit;
                    }
                    map.spawns.push(spawn);
                    placed = true;
                }
            }
            if !placed || !defaults {
                let units = if defaults || self.standard_start {
                    faction.starting_units.clone()
                } else {
                    vec![(faction.worker, Position { x: 0, y: 0 })]
                };
                for (unit_type, offset) in units {
                    generated.push(Spawn {
                        owner: *p,
                        unit_type,
                        position: Position {
                            x: locations[i].x + offset.x,
                            y: locations[i].y + offset.y,
                        },
                        ..Default::default()
                    });
                }
            }
            if !map.ai.iter().any(|a| a.player == owner(*p)) {
                map.ai.push(AiController {
                    takeover: true,
                    player: owner(*p),
                    home: locations[i],
                    radius: 8192,
                    active: settings.slots[i].kind == SlotKind::Computer,
                    program: faction
                        .ai_difficulties
                        .get(&settings.ai_difficulty)
                        .unwrap_or(&faction.ai)
                        .clone(),
                    research: faction.ai_research.clone(),
                    abilities: Vec::new(),
                    harvest_weights: Vec::new(),
                });
            }
        }
        // Reserve every starting base before placing workers. Nearby starts
        // can otherwise let one player's workers occupy the next base's site.
        generated.sort_by_key(|spawn| {
            !rules
                .units
                .iter()
                .find(|u| u.id == spawn.unit_type)
                .is_none_or(|u| u.structure)
        });
        for mut spawn in generated {
            spawn.position = starting_position(&map, &rules, spawn.unit_type, spawn.position)?;
            map.spawns.push(spawn);
        }
        if shared {
            for spawn in &mut map.spawns {
                if active.contains(&spawn.owner) {
                    spawn.owner = owner(spawn.owner);
                }
            }
        }
        let mut resources = BTreeMap::<PlayerId, Vec<ResourceAmount>>::new();
        for p in &active {
            let amounts = settings.resources.map_or_else(
                || setup.resources[usize::from(p.0)].clone(),
                |i| self.resources[i].amounts.clone(),
            );
            let pool = resources.entry(owner(*p)).or_default();
            for amount in amounts {
                if let Some(existing) = pool.iter_mut().find(|r| r.kind == amount.kind) {
                    existing.amount = existing.amount.saturating_add(amount.amount);
                } else {
                    pool.push(amount);
                }
            }
        }
        let participants: Vec<_> = active
            .iter()
            .map(|p| owner(*p))
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        let mut flags = Vec::new();
        if matches!(
            mode,
            GameType::CaptureTheFlag | GameType::TeamCaptureTheFlag
        ) {
            let art = self
                .flag_art
                .as_ref()
                .context("game has no capture flag artwork")?;
            for p in &participants {
                let slot = usize::from(p.0);
                let position = starting_position(
                    &map,
                    &rules,
                    art.homes[factions[slot]],
                    Position {
                        x: locations[slot].x,
                        y: (locations[slot].y + 112).min(map.height - 16),
                    },
                )?;
                map.spawns.push(Spawn {
                    owner: *p,
                    unit_type: art.homes[factions[slot]],
                    position,
                    invincible: true,
                    ..Default::default()
                });
                let item = EntityId(map.spawns.len() as u32 + 1);
                map.spawns.push(Spawn {
                    owner: *p,
                    unit_type: art.item,
                    position,
                    invincible: true,
                    ..Default::default()
                });
                flags.push(FlagHome {
                    item,
                    owner: *p,
                    position,
                });
            }
        }
        let humans = occupied
            .iter()
            .map(|i| owner(PlayerId(*i as u16)))
            .collect();
        for start in &mut map.start_locations {
            if let Some(position) = locations.get(usize::from(start.player.0)) {
                start.position = *position;
            }
        }
        for ai in &mut map.ai {
            if active.iter().any(|p| owner(*p) == ai.player) {
                ai.takeover = true;
                ai.active = !occupied
                    .iter()
                    .any(|i| owner(PlayerId(*i as u16)) == ai.player);
            }
        }
        let owners = (0..settings.slots.len())
            .map(|i| {
                active
                    .contains(&PlayerId(i as u16))
                    .then(|| owner(PlayerId(i as u16)))
            })
            .collect();
        map.multiplayer = Some(MatchRules {
            participants,
            teams,
            mode,
            parameter: settings.parameter,
            resources,
            bases: rules
                .units
                .iter()
                .filter(|u| u.structure && !u.dropoff.is_empty())
                .map(|u| u.id)
                .collect(),
            cheats: settings.cheats,
            defeat_without_structures: self.defeat_without_structures,
            flags,
        });
        if mode != GameType::UseMapSettings {
            map.mission = None;
        }
        Ok(Prepared {
            artwork,
            world: World::new(rules, map, seed)?,
            directory,
            factions,
            humans,
            owners,
            tick_ms: self.speeds[settings.speed].tick_ms,
        })
    }
}
fn starting_position(
    map: &Map,
    rules: &Rules,
    id: UnitTypeId,
    target: Position,
) -> Result<Position> {
    let unit = rules
        .units
        .iter()
        .find(|u| u.id == id)
        .context("unknown starting unit")?;
    let overlap =
        |a: [i64; 4], b: [i64; 4]| a[0] < b[2] && a[2] > b[0] && a[1] < b[3] && a[3] > b[1];
    let clear = |point| {
        if !map.contains_footprint(
            point,
            if unit.structure {
                unit.placement
            } else {
                unit.footprint
            },
        ) {
            return false;
        }
        if !unit.blocks_movement {
            return true;
        }
        if !map.can_move(point, unit.footprint, unit.movement_class) {
            return false;
        }
        let bounds = unit.footprint.bounds(point);
        !map.spawns.iter().any(|spawn| {
            let other = rules
                .units
                .iter()
                .find(|u| u.id == spawn.unit_type)
                .unwrap();
            other.blocks_movement
                && !other.revealer
                && spawn.doodad_enabled != Some(false)
                && !(spawn.cloaked && other.cloak.as_ref().is_some_and(|c| !c.blocks_movement))
                && other.movement_class.collides(unit.movement_class)
                && overlap(bounds, other.footprint.bounds(spawn.position))
        }) && (unit.movement_class == MovementClass::Air
            || !map.resources.iter().any(|r| {
                (r.amount > 0 || r.requires_extractor)
                    && overlap(bounds, r.footprint.bounds(r.position))
            }))
    };
    if clear(target) {
        return Ok(target);
    }
    // Only match setup uses this search. Preserve terrain and authored armies;
    // move a newly generated starting unit to a nearby unoccupied position.
    for radius in (8..=512).step_by(8) {
        let mut candidates = Vec::new();
        for step in (-radius..=radius).step_by(8) {
            for (x, y) in [
                (step, -radius),
                (step, radius),
                (-radius, step),
                (radius, step),
            ] {
                let point = Position {
                    x: target.x + x,
                    y: target.y + y,
                };
                if clear(point) {
                    candidates.push((x * x + y * y, point));
                }
            }
        }
        if let Some((_, point)) = candidates.into_iter().min_by_key(|(distance, _)| *distance) {
            return Ok(point);
        }
    }
    anyhow::bail!("no clear starting position near {target:?} for unit {id:?}")
}
impl Catalog {
    pub fn load(root: &Path) -> Result<Self> {
        let mut catalog: Self = read_ron(&root.join("multiplayer.ron"))?;
        for mode in [GameType::FreeForAll, GameType::Teams] {
            if !catalog.modes.contains(&mode) {
                catalog.modes.push(mode);
            }
        }
        catalog.validate()?;
        Ok(catalog)
    }
    pub fn validate(&self) -> Result<()> {
        self.validate_colors()?;
        ensure!(
            self.schema_version == 1 && !self.game.is_empty() && self.game.len() <= 128,
            "invalid multiplayer catalog"
        );
        ensure!(
            !self.maps.is_empty()
                && self.maps.len() <= 1024
                && (1..=8).contains(&self.factions.len())
                && !self.speeds.is_empty()
                && self.speeds.len() <= 16
                && !self.modes.is_empty()
                && self.modes.len() <= 16
                && self.resources.len() <= 16
                && self.tilesets.len() <= 8,
            "invalid multiplayer catalog limits"
        );
        let mut paths = std::collections::BTreeSet::new();
        ensure!(
            self.terrain_mappings.is_empty()
                || (self.terrain_mappings.len() == self.tilesets.len()
                    && self.terrain_mappings.iter().all(|m| !m.tiles.is_empty()
                        && m.tiles.len() <= 65536
                        && m.tiles.iter().all(|t| *t < 4096)
                        && m.color_sources.len() <= 256)),
            "invalid native tileset mappings"
        );
        ensure!(
            self.flag_art
                .as_ref()
                .is_none_or(|a| a.homes.len() == self.factions.len()),
            "invalid capture flag homes"
        );
        for map in &self.maps {
            local_path(&map.package)?;
            ensure!(
                !map.title.is_empty()
                    && map.title.len() <= 128
                    && (2..=8).contains(&map.players)
                    && map.size.iter().all(|s| (1..=256).contains(s))
                    && paths.insert(&map.package),
                "invalid multiplayer map entry"
            );
        }
        for faction in &self.factions {
            local_path(&faction.presentation)?;
            for path in &faction.tileset_presentations {
                local_path(path)?;
            }
            ensure!(
                faction.tileset_presentations.is_empty()
                    || (faction.tileset_presentations.len() <= 8
                        && if self.tilesets.is_empty() {
                            self.maps.iter().all(|m| {
                                usize::from(m.tileset) < faction.tileset_presentations.len()
                            })
                        } else {
                            faction.tileset_presentations.len() == self.tilesets.len()
                        }),
                "invalid faction tilesets"
            );
            ensure!(
                !faction.name.is_empty()
                    && faction.name.len() <= 64
                    && faction.starting_units.len() <= 32
                    && faction.starting_units.iter().all(
                        |(_, p)| (-8192..=8192).contains(&p.x) && (-8192..=8192).contains(&p.y)
                    )
                    && faction.equivalents.len() <= 256
                    && faction.ai.len() <= 64
                    && faction.ai_difficulties.values().all(|p| p.len() <= 64)
                    && faction.ai_research.len() <= 256,
                "invalid faction"
            );
        }
        ensure!(
            self.speeds.iter().all(|s| (1..=1000).contains(&s.tick_ms)),
            "invalid multiplayer speed"
        );
        Ok(())
    }
    pub fn defaults(&self, map: usize) -> Settings {
        Settings {
            ai_difficulty: AiDifficulty::Normal,
            slots: (0..self.maps[map].players)
                .map(|i| Slot {
                    kind: SlotKind::Open,
                    faction: None,
                    team: i % 2,
                    color: (!self.colors.is_empty()).then_some(usize::from(i)),
                })
                .collect(),
            mode: 0,
            parameter: 5000,
            resources: None,
            map_units: true,
            random_locations: false,
            fog: true,
            cheats: false,
            speed: self.speeds.len() - 1,
            tileset: None,
        }
    }
    pub fn validate_settings(&self, map: usize, settings: &Settings) -> Result<()> {
        self.slot_colors(settings)?;
        let map = self.maps.get(map).context("unknown multiplayer map")?;
        ensure!(
            settings.slots.len() == usize::from(map.players)
                && settings.mode < self.modes.len()
                && settings.speed < self.speeds.len()
                && settings.resources.is_none_or(|r| r < self.resources.len())
                && settings.tileset.is_none_or(|t| t < self.tilesets.len())
                && (!settings.cheats || self.allow_cheats)
                && settings
                    .slots
                    .iter()
                    .all(|s| s.faction.is_none_or(|f| f < self.factions.len()) && s.team < 8),
            "invalid multiplayer settings"
        );
        Ok(())
    }
}
pub fn local_path(path: &str) -> Result<()> {
    ensure!(
        !path.is_empty()
            && path.len() <= 256
            && path.split('/').all(|part| !part.is_empty()
                && part != "."
                && part != ".."
                && part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))),
        "multiplayer path must be local"
    );
    Ok(())
}
pub fn package_path(root: &Path, relative: &str) -> Result<PathBuf> {
    local_path(relative)?;
    let path = root.join(relative).canonicalize()?;
    ensure!(
        path.starts_with(root.canonicalize()?),
        "multiplayer package escapes game directory"
    );
    Ok(path)
}
