//! All authoritative changes happen in `World::step`, in canonical command order.
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    sync::Arc,
};

use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

use crate::map::Terrain;
pub use crate::map::{Footprint, MovementClass};

mod cloak;
pub use cloak::Cloak;
mod creep;
mod garrison;
mod mission;
pub use garrison::GarrisonStats;
mod mines;
pub use mines::{MineLayer, MinePhase, MineState, MineStats};
mod ai;
mod flight;
mod inspection;
mod player_view;
pub use player_view::*;
#[cfg(test)]
#[path = "sim/session/tests.rs"]
mod session_tests;
mod snapshot;
pub use snapshot::*;
mod statistics;
pub use statistics::PlayerStatistics;
mod map_data;
pub use map_data::*;
mod research;
mod rts;
mod vision;
pub use ai::*;
pub use flight::*;
pub use inspection::EntityInspection;
pub use mission::*;
pub use research::*;
pub use rts::*;
pub use vision::*;

pub const SIMULATION_REVISION: &str = "straterust-sim-20";
pub const MAX_COMMANDS_PER_TICK: usize = 4096;

#[derive(Clone, Debug)]
pub struct World {
    rules: Arc<Rules>,
    map: Arc<Map>,
    rules_hash: blake3::Hash,
    map_hash: blake3::Hash,
    state: State,
    /// Derived propagation only; static units reuse their visible tile lists.
    vision_cells: BTreeMap<(Position, u32, bool), Vec<usize>>,
    view: Option<ViewMetadata>,
}

impl World {
    pub fn is_enemy(&self, a: PlayerId, b: PlayerId) -> bool {
        a != b
            && !self.map.mission.as_ref().is_some_and(|mission| {
                mission
                    .alliances
                    .iter()
                    .any(|pair| *pair == [a, b] || *pair == [b, a])
            })
    }
    pub fn new(rules: Rules, map: Map, seed: u64) -> Result<Self> {
        Self::initialize(rules, map, seed, false)
    }

    fn initialize(mut rules: Rules, map: Map, seed: u64, client: bool) -> Result<Self> {
        ensure!(
            !rules.id.is_empty() && rules.id.len() <= 128,
            "invalid ruleset ID"
        );
        ensure!(
            (1..=1000).contains(&rules.tick_ms),
            "tick_ms must be 1..=1000"
        );
        ensure!(
            !rules.units.is_empty() && rules.units.len() <= 4096,
            "invalid unit type count"
        );
        rules.units.sort_by_key(|unit| unit.id);
        let mut ids = BTreeSet::new();
        for unit in &rules.units {
            ensure!(ids.insert(unit.id), "duplicate unit type {}", unit.id.0);
            ensure!(
                (0..=1024).contains(&unit.speed),
                "unit speed must be 0..=1024"
            );
            ensure!(
                unit.footprint.width != 0 && unit.footprint.height != 0,
                "unit footprint dimensions must be nonzero"
            );
        }
        validate_rts_rules(&rules)?;
        validate_ai(&rules, &map)?;
        validate_research_rules(&rules)?;
        garrison::validate_garrison_rules(&rules)?;
        mines::validate_mine_rules(&rules)?;
        ensure!(!map.id.is_empty() && map.id.len() <= 128, "invalid map ID");
        ensure!(
            (16..=32768).contains(&map.width) && (16..=32768).contains(&map.height),
            "map dimensions must be 16..=32768"
        );
        ensure!(
            (1..=32).contains(&map.players),
            "player count must be 1..=32"
        );
        for (player, units) in &map.creation {
            let unique: BTreeSet<_> = units.iter().copied().collect();
            ensure!(
                player.0 < map.players
                    && units.len() <= 4096
                    && unique.len() == units.len()
                    && unique.iter().all(|id| ids.contains(id)),
                "invalid player creation restrictions"
            );
        }
        ensure!(
            (client || !map.spawns.is_empty()) && map.spawns.len() <= 4096,
            "invalid spawn count"
        );
        if let Some(mission) = &map.mission
            && !client
        {
            mission.validate(&rules, &map)?;
        }
        if let Some(terrain) = &map.terrain {
            terrain.validate_coverage(map.width, map.height)?;
        }
        ensure!(
            map.start_locations.len() <= usize::from(map.players),
            "too many start locations"
        );
        let mut start_players = BTreeSet::new();
        for start in &map.start_locations {
            ensure!(
                start.player.0 < map.players,
                "unknown start location player {}",
                start.player.0
            );
            ensure!(
                start_players.insert(start.player),
                "duplicate start location for player {}",
                start.player.0
            );
            ensure!(map.contains(start.position), "start location outside map");
        }
        ensure!(map.resources.len() <= 4096, "too many resource placements");
        for resource in &map.resources {
            ensure!(
                !resource.kind.is_empty()
                    && resource.kind.len() <= 64
                    && resource
                        .kind
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte)),
                "invalid resource kind"
            );
            ensure!(
                map.contains_footprint(resource.position, resource.footprint),
                "resource placement outside map"
            );
            ensure!(resource.amount > 0, "resource amount must be nonzero");
        }
        let fog_cells = ((map.width + 31) / 32 * ((map.height + 31) / 32)) as u32;
        for (player, cells) in &map.initial_explored {
            ensure!(
                map.fog_of_war
                    && player.0 < map.players
                    && cells.len() <= fog_cells as usize
                    && cells.iter().all(|c| *c < fog_cells)
                    && cells.windows(2).all(|c| c[0] < c[1]),
                "invalid initial exploration"
            );
        }
        let mut state = State {
            statistics: vec![PlayerStatistics::default(); usize::from(map.players)],
            kills: BTreeMap::new(),
            ai: map.ai.iter().map(AiState::new).collect(),
            tick: Tick(0),
            rng_state: seed,
            next_entity_id: 1,
            last_sequences: vec![0; usize::from(map.players)],
            entities: Vec::new(),
            players: (0..map.players)
                .map(|_| PlayerState {
                    completed_research: BTreeSet::new(),
                    resources: rules
                        .starting_resources
                        .iter()
                        .map(|amount| (amount.kind.clone(), u64::from(amount.amount)))
                        .collect(),
                })
                .collect(),
            resources: map
                .resources
                .iter()
                .enumerate()
                .map(|(index, resource)| ResourceNode {
                    id: ResourceId(index as u32 + 1),
                    kind: resource.kind.clone(),
                    position: resource.position,
                    footprint: resource.footprint,
                    amount: resource.amount,
                    requires_extractor: resource.requires_extractor,
                })
                .collect(),
            winner: None,
            defeated: Vec::new(),
            mission: map.mission.as_ref().map(MissionState::new),
            fog: if map.fog_of_war {
                vec![
                    vec![0; ((map.width + 31) / 32 * ((map.height + 31) / 32)) as usize];
                    usize::from(map.players)
                ]
            } else {
                Vec::new()
            },
            terrain_fog: if map.fog_of_war {
                vec![
                    vec![0; ((map.width + 31) / 32 * ((map.height + 31) / 32)) as usize];
                    usize::from(map.players)
                ]
            } else {
                Vec::new()
            },
            scans: Vec::new(),
            creep: Vec::new(),
            creep_seen: Vec::new(),
        };
        for spawn in &map.spawns {
            ensure!(
                spawn.hp_percent.is_none_or(|hp| (1..=100).contains(&hp))
                    && spawn.energy_percent.is_none_or(|energy| energy <= 100),
                "invalid starting HP percentage"
            );
            ensure!(
                spawn.owner.0 < map.players,
                "unknown spawn owner {}",
                spawn.owner.0
            );
            ensure!(
                ids.contains(&spawn.unit_type),
                "unknown unit type {}",
                spawn.unit_type.0
            );
            let unit = &rules.units[rules
                .units
                .binary_search_by_key(&spawn.unit_type, |unit| unit.id)
                .expect("validated unit type")];
            ensure!(
                map.contains_footprint(spawn.position, unit.footprint),
                "spawn footprint outside map: {:?}",
                spawn.position
            );
            state.entities.push(Entity {
                doodad_enabled: spawn.doodad_enabled,
                id: EntityId(state.next_entity_id),
                owner: spawn.owner,
                unit_type: spawn.unit_type,
                position: spawn.position,
                hp: spawn.hp_percent.map_or(unit.max_hp, |hp| {
                    (u64::from(unit.max_hp) * u64::from(hp) / 100).max(1) as u32
                }),
                invincible: spawn.invincible,
                cloaked: spawn.cloaked,
                energy: spawn.energy_percent.map_or_else(
                    || unit.initial_energy(),
                    |percent| unit.energy_max() * 256 * u32::from(percent) / 100,
                ),
                mine_count: unit
                    .mine_layer
                    .as_ref()
                    .map_or(0, |layer| layer.initial_count),
                mine_state: unit.mine.as_ref().map(|mine| MineState {
                    phase: MinePhase::Arming,
                    remaining: mine.arm_ticks,
                    target: None,
                }),
                ..Entity::default()
            });
            state.statistics[usize::from(spawn.owner.0)].created(unit.structure);
            state.next_entity_id += 1;
        }
        for (player, cells) in &map.initial_explored {
            for &cell in cells {
                state.fog[usize::from(player.0)][cell as usize] = 0x0f;
                state.terrain_fog[usize::from(player.0)][cell as usize] = 1;
            }
        }
        let rules_hash = hash_rules(&rules);
        let map_hash = hash_map(&map);
        let mut world = Self {
            rules: Arc::new(rules),
            map: Arc::new(map),
            rules_hash,
            map_hash,
            state,
            vision_cells: BTreeMap::new(),
            view: None,
        };
        for entity in &world.state.entities {
            let unit = world.unit_type(entity.unit_type).expect("validated type");
            if unit.revealer
                || !unit.blocks_movement
                || entity.cloaked
                    && (unit.mine.is_some()
                        || unit.cloak.as_ref().is_some_and(|c| !c.blocks_movement))
                || entity.doodad_enabled.is_some()
            {
                continue;
            }
            ensure!(
                world.can_place(
                    entity.position,
                    unit.footprint,
                    unit.movement_class,
                    Some(entity.id)
                ),
                "spawn overlaps blocked terrain, an entity, or a resource: {:?}",
                entity.id
            );
        }
        world.initialize_addons();
        world.initialize_creep();
        world.update_vision();
        world.remember_creep();
        Ok(world)
    }

    pub fn tick(&self) -> Tick {
        self.state.tick
    }
    pub fn rules(&self) -> &Rules {
        &self.rules
    }
    pub fn map(&self) -> &Map {
        &self.map
    }
    pub fn creation_allowed(&self, player: PlayerId, unit: UnitTypeId) -> bool {
        self.map
            .creation
            .get(&player)
            .is_none_or(|units| units.contains(&unit))
    }
    pub fn state(&self) -> &State {
        &self.state
    }

    /// Independent completed state for presentation; share immutable definitions
    /// and omit derived propagation caches. Creating this copy does not alter
    /// gameplay, and stepping it rebuilds the caches normally.
    pub fn snapshot(&self) -> Self {
        Self {
            rules: Arc::clone(&self.rules),
            map: Arc::clone(&self.map),
            rules_hash: self.rules_hash,
            map_hash: self.map_hash,
            state: self.state.clone(),
            vision_cells: BTreeMap::new(),
            view: self.view.clone(),
        }
    }
    pub fn rules_hash(&self) -> blake3::Hash {
        self.rules_hash
    }
    pub fn map_hash(&self) -> blake3::Hash {
        self.map_hash
    }

    /// Occupancy is derived directly from current entity and resource rectangles.
    pub fn is_occupied(
        &self,
        position: Position,
        footprint: Footprint,
        class: MovementClass,
        except: Option<EntityId>,
    ) -> bool {
        self.occupied_except(position, footprint, class, except, None)
    }
    pub(in crate::sim) fn occupied_except(
        &self,
        position: Position,
        footprint: Footprint,
        class: MovementClass,
        except: Option<EntityId>,
        other_except: Option<EntityId>,
    ) -> bool {
        if footprint.width == 0 || footprint.height == 0 {
            return false;
        }
        let [left, top, right, bottom] = footprint.bounds(position);
        self.state.entities.iter().any(|entity| {
            if Some(entity.id) == except
                || Some(entity.id) == other_except
                || self.phases_collision(entity)
                || entity.gathering_inside
                || entity.garrisoned_in.is_some()
                || entity.doodad_enabled == Some(false)
            {
                return false;
            }
            let unit = &self.rules.units[self
                .rules
                .units
                .binary_search_by_key(&entity.unit_type, |unit| unit.id)
                .expect("validated unit type")];
            let [other_left, other_top, other_right, other_bottom] =
                unit.footprint.bounds(entity.position);
            !unit.revealer
                && unit.blocks_movement
                && self.movement_class(entity) == class
                && left < other_right
                && right > other_left
                && top < other_bottom
                && bottom > other_top
        }) || (class == MovementClass::Ground
            && self.state.resources.iter().any(|resource| {
                let [l, t, r, b] = resource.footprint.bounds(resource.position);
                self.resource_blocks_movement(resource)
                    && left < r
                    && right > l
                    && top < b
                    && bottom > t
            }))
    }

    pub fn can_place(
        &self,
        position: Position,
        footprint: Footprint,
        class: MovementClass,
        except: Option<EntityId>,
    ) -> bool {
        self.map.can_move(position, footprint, class)
            && !self.is_occupied(position, footprint, class, except)
    }

    fn resource_blocks_movement(&self, resource: &ResourceNode) -> bool {
        (resource.amount > 0 || resource.requires_extractor)
            && !(resource.requires_extractor
                && self.state.entities.iter().any(|e| {
                    e.position == resource.position
                        && !e.airborne
                        && self.unit_type(e.unit_type).is_some_and(|u| {
                            u.extracts
                                .as_ref()
                                .is_some_and(|x| x.resource == resource.kind)
                        })
                }))
    }

    /// Tick denotes the next tick to execute. Input batches are external inputs, not world state.
    pub fn step(&mut self, commands: &[Command]) -> Result<Vec<CommandOutcome>> {
        ensure!(
            self.view.is_none(),
            "a player view cannot run authoritative simulation"
        );
        ensure!(self.tick().0 < u64::MAX, "simulation tick exhausted");
        ensure!(
            commands.len() <= MAX_COMMANDS_PER_TICK,
            "too many commands in one tick"
        );
        let mut ordered = commands.to_vec();
        ordered.sort();
        let mut counts = BTreeMap::new();
        for command in &ordered {
            *counts
                .entry((command.player, command.sequence))
                .or_insert(0) += 1;
        }
        let mut outcomes = Vec::with_capacity(ordered.len());
        for command in ordered {
            let rejection = if command.tick != self.tick() {
                Some(Rejection::WrongTick)
            } else if command.player.0 >= self.map.players {
                Some(Rejection::UnknownPlayer)
            } else if self.map.ai.iter().any(|ai| ai.player == command.player) {
                Some(Rejection::ComputerControlled)
            } else if counts[&(command.player, command.sequence)] > 1 {
                Some(Rejection::DuplicateSequence)
            } else {
                let previous = &mut self.state.last_sequences[usize::from(command.player.0)];
                if command.sequence <= *previous {
                    Some(Rejection::StaleSequence)
                } else {
                    // Consume valid envelope sequences even when an order is illegal.
                    *previous = command.sequence;
                    self.apply(&command)
                }
            };
            outcomes.push(CommandOutcome { command, rejection });
        }
        self.advance_ai();
        self.advance_rescue();
        self.advance_scanners();
        self.advance_cloaks();
        self.advance_rts();
        self.advance_creep();
        self.advance_mission();
        self.update_vision();
        self.remember_creep();
        self.state.tick.0 += 1;
        Ok(outcomes)
    }

    /// Explicit little-endian encoding, independent of serde, pointers and native word size.
    pub fn canonical_state(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        put_string(&mut bytes, SIMULATION_REVISION);
        bytes.extend(self.rules_hash.as_bytes());
        bytes.extend(self.map_hash.as_bytes());
        bytes.extend(self.state.tick.0.to_le_bytes());
        bytes.extend(self.state.rng_state.to_le_bytes());
        bytes.extend(self.state.next_entity_id.to_le_bytes());
        bytes.extend((self.state.last_sequences.len() as u32).to_le_bytes());
        for sequence in &self.state.last_sequences {
            bytes.extend(sequence.to_le_bytes());
        }
        bytes.extend((self.state.entities.len() as u32).to_le_bytes());
        for entity in &self.state.entities {
            bytes.extend(entity.id.0.to_le_bytes());
            bytes.extend(entity.owner.0.to_le_bytes());
            bytes.extend(entity.unit_type.0.to_le_bytes());
            put_position(&mut bytes, entity.position);
            bytes.push(u8::from(entity.target.is_some()));
            if let Some(target) = entity.target {
                put_position(&mut bytes, target);
            }
        }
        put_rts_state(&mut bytes, &self.state);
        bytes.extend((self.state.creep.len() as u32).to_le_bytes());
        bytes.extend(&self.state.creep);
        for memory in &self.state.creep_seen {
            bytes.extend(memory);
        }
        put_research_state(&mut bytes, &self.state);
        for layer in [&self.state.fog, &self.state.terrain_fog] {
            bytes.extend((layer.len() as u32).to_le_bytes());
            for fog in layer {
                bytes.extend((fog.len() as u32).to_le_bytes());
                bytes.extend(fog);
            }
        }
        put_scans(&mut bytes, &self.state.scans);
        put_mission_state(&mut bytes, &self.state.mission);
        put_ai_state(&mut bytes, &self.state.ai);
        bytes.extend((self.state.kills.len() as u32).to_le_bytes());
        for (player, types) in &self.state.kills {
            bytes.extend(player.0.to_le_bytes());
            bytes.extend((types.len() as u32).to_le_bytes());
            for (unit, count) in types {
                bytes.extend(unit.0.to_le_bytes());
                bytes.extend(count.to_le_bytes());
            }
        }
        bytes
    }

    pub fn state_hash(&self) -> blake3::Hash {
        blake3::hash(&self.canonical_state())
    }
}

fn put_string(bytes: &mut Vec<u8>, value: &str) {
    bytes.extend((value.len() as u32).to_le_bytes());
    bytes.extend(value.as_bytes());
}

fn put_position(bytes: &mut Vec<u8>, position: Position) {
    bytes.extend(position.x.to_le_bytes());
    bytes.extend(position.y.to_le_bytes());
}

fn hash_rules(rules: &Rules) -> blake3::Hash {
    let mut bytes = b"straterust-rules-3".to_vec();
    put_string(&mut bytes, &rules.id);
    bytes.extend(rules.tick_ms.to_le_bytes());
    bytes.extend((rules.units.len() as u32).to_le_bytes());
    for unit in &rules.units {
        bytes.extend(unit.id.0.to_le_bytes());
        bytes.extend(unit.speed.to_le_bytes());
        bytes.extend(unit.footprint.width.to_le_bytes());
        bytes.extend(unit.footprint.height.to_le_bytes());
        bytes.push(match unit.movement_class {
            MovementClass::Ground => 0,
            MovementClass::Air => 1,
        });
    }
    put_rts_rules(&mut bytes, rules);
    put_research_rules(&mut bytes, rules);
    blake3::hash(&bytes)
}

fn hash_map(map: &Map) -> blake3::Hash {
    let mut bytes = b"straterust-map-4".to_vec();
    put_string(&mut bytes, &map.id);
    bytes.extend(map.width.to_le_bytes());
    bytes.extend(map.height.to_le_bytes());
    bytes.extend(map.players.to_le_bytes());
    bytes.extend((map.creation.len() as u32).to_le_bytes());
    for (player, units) in &map.creation {
        bytes.extend(player.0.to_le_bytes());
        bytes.extend((units.len() as u32).to_le_bytes());
        for id in units {
            bytes.extend(id.0.to_le_bytes());
        }
    }
    bytes.extend((map.initial_explored.len() as u32).to_le_bytes());
    for (player, cells) in &map.initial_explored {
        bytes.extend(player.0.to_le_bytes());
        bytes.extend((cells.len() as u32).to_le_bytes());
        for cell in cells {
            bytes.extend(cell.to_le_bytes());
        }
    }
    put_ai_definition(&mut bytes, &map.ai);
    bytes.extend((map.spawns.len() as u32).to_le_bytes());
    for spawn in &map.spawns {
        bytes.extend(spawn.owner.0.to_le_bytes());
        bytes.extend(spawn.unit_type.0.to_le_bytes());
        put_position(&mut bytes, spawn.position);
        bytes.push(u8::from(spawn.energy_percent.is_some()));
        if let Some(energy) = spawn.energy_percent {
            bytes.push(energy);
        }
        bytes.push(u8::from(spawn.hp_percent.is_some()));
        if let Some(hp) = spawn.hp_percent {
            bytes.push(hp);
        }
        bytes.push(u8::from(spawn.invincible));
        bytes.push(u8::from(spawn.cloaked));
        bytes.push(match spawn.doodad_enabled {
            None => 0,
            Some(false) => 1,
            Some(true) => 2,
        });
    }
    put_mission_definition(&mut bytes, &map.mission);
    bytes.push(u8::from(map.fog_of_war));
    bytes.extend((map.start_locations.len() as u32).to_le_bytes());
    for start in &map.start_locations {
        bytes.extend(start.player.0.to_le_bytes());
        put_position(&mut bytes, start.position);
    }
    bytes.extend((map.resources.len() as u32).to_le_bytes());
    for resource in &map.resources {
        put_string(&mut bytes, &resource.kind);
        put_position(&mut bytes, resource.position);
        bytes.extend(resource.amount.to_le_bytes());
        bytes.push(u8::from(resource.requires_extractor));
        bytes.extend(resource.footprint.width.to_le_bytes());
        bytes.extend(resource.footprint.height.to_le_bytes());
    }
    bytes.push(u8::from(map.terrain.is_some()));
    if let Some(terrain) = &map.terrain {
        bytes.extend(terrain.cell_size.to_le_bytes());
        bytes.extend(terrain.columns.to_le_bytes());
        bytes.extend(terrain.rows.to_le_bytes());
        bytes.extend(&terrain.flags);
    }
    blake3::hash(&bytes)
}

// SplitMix64: fixed wrapping arithmetic, including for a zero seed. Not cryptographic.
fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9e3779b97f4a7c15);
    let mut value = *state;
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d049bb133111eb);
    value ^ (value >> 31)
}

#[cfg(test)]
mod tests;

mod types;
pub use types::*;
