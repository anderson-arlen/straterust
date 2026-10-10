//! Exploration persists in authoritative state; current sight is rebuilt from units.
use super::*;

// Derived sight data must not grow with every accepted source's full-map view.
// Oversized views are still evaluated, but do not stay resident in the cache.
const MAX_CACHED_VISION_CELLS: usize = 262_144;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Visibility {
    Unexplored,
    Explored,
    Visible,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scanner {
    pub energy_max: u32,
    pub energy_initial: u32,
    /// Energy in 1/256 units restored per simulation tick.
    pub energy_regeneration: u32,
    pub cost: u32,
    pub radius: u32,
    pub duration: u32,
}
impl Scanner {
    pub(super) fn validate(&self) -> Result<()> {
        ensure!(
            (1..=10000).contains(&self.energy_max)
                && self.energy_initial <= self.energy_max
                && self.energy_regeneration <= 256
                && self.cost > 0
                && self.cost <= self.energy_max
                && self.radius <= 32768
                && (1..=10000).contains(&self.duration),
            "invalid scanner rules"
        );
        Ok(())
    }
    pub(super) fn put(&self, bytes: &mut Vec<u8>) {
        for value in [
            self.energy_max,
            self.energy_initial,
            self.energy_regeneration,
            self.cost,
            self.radius,
            self.duration,
        ] {
            bytes.extend(value.to_le_bytes());
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scan {
    pub owner: PlayerId,
    pub position: Position,
    pub radius: u32,
    pub remaining: u32,
}
pub(super) fn put_scans(bytes: &mut Vec<u8>, scans: &[Scan]) {
    bytes.extend((scans.len() as u32).to_le_bytes());
    for scan in scans {
        bytes.extend(scan.owner.0.to_le_bytes());
        put_position(bytes, scan.position);
        bytes.extend(scan.radius.to_le_bytes());
        bytes.extend(scan.remaining.to_le_bytes());
    }
}

impl World {
    pub fn scan_rejection(&self, entity: EntityId, target: Position) -> Option<Rejection> {
        let Some(actor) = self.state.entities.iter().find(|actor| actor.id == entity) else {
            return Some(Rejection::UnknownEntity);
        };
        let Some(scanner) = &self.unit_type(actor.unit_type)?.scanner else {
            return Some(Rejection::UnsupportedOrder);
        };
        if actor.construction.is_some() {
            return Some(Rejection::Unfinished);
        }
        if self.unit_type(actor.unit_type)?.addon_parent.is_some()
            && actor.parent.is_none_or(|parent| {
                !self.state.entities.iter().any(|entity| {
                    entity.id == parent && entity.owner == actor.owner && !entity.airborne
                })
            })
        {
            return Some(Rejection::MissingPrerequisite);
        }
        if !self.map.contains(target) {
            return Some(Rejection::OutOfBounds);
        }
        if actor.energy < scanner.cost * 256 {
            return Some(Rejection::InsufficientResources);
        }
        if self.state.scans.len() >= 4096 {
            return Some(Rejection::EntityLimit);
        }
        None
    }
    pub(super) fn start_scan(&mut self, index: usize, position: Position) -> Option<Rejection> {
        if let Some(reason) = self.scan_rejection(self.state.entities[index].id, position) {
            return Some(reason);
        }
        let scanner = self
            .unit_type(self.state.entities[index].unit_type)?
            .scanner
            .clone()?;
        self.state.entities[index].energy -= scanner.cost * 256;
        self.state.scans.push(Scan {
            owner: self.state.entities[index].owner,
            position,
            radius: scanner.radius,
            remaining: scanner.duration,
        });
        None
    }
    pub(super) fn advance_scanners(&mut self) {
        if self.state.winner.is_some()
            || self
                .state
                .mission
                .as_ref()
                .is_some_and(|mission| mission.paused)
        {
            return;
        }
        for scan in &mut self.state.scans {
            scan.remaining = scan.remaining.saturating_sub(1);
        }
        self.state.scans.retain(|scan| scan.remaining != 0);
        for entity in &mut self.state.entities {
            if entity.construction.is_some() {
                continue;
            }
            let unit = &self.rules.units[self
                .rules
                .units
                .binary_search_by_key(&entity.unit_type, |unit| unit.id)
                .expect("validated type")];
            if let Some(scanner) = &unit.scanner {
                entity.energy =
                    (entity.energy + scanner.energy_regeneration).min(scanner.energy_max * 256);
            }
        }
    }
    pub(super) fn detected(&self, player: PlayerId, position: Position) -> bool {
        self.state.entities.iter().any(|entity| {
            let unit = self.unit_type(entity.unit_type).expect("type");
            let range = if unit.detector_range == 0 {
                0
            } else {
                unit.detector_range + self.vision_range(entity).saturating_sub(unit.vision_range)
            };
            !self.is_enemy(player, entity.owner)
                && entity.hp > 0
                && entity.construction.is_none()
                && !self.disabled(entity)
                && entity.garrisoned_in.is_none()
                && entity.doodad_enabled != Some(false)
                && range > 0
                && distance(entity.position, position) <= i64::from(range).pow(2)
        }) || self.state.scans.iter().any(|scan| {
            let dx = i64::from(scan.position.x) - i64::from(position.x);
            let dy = i64::from(scan.position.y) - i64::from(position.y);
            scan.owner == player && dx * dx + dy * dy <= i64::from(scan.radius).pow(2)
        })
    }
    /// Visibility of ground objects at their exact terrain height. Terrain
    /// artwork has its own discovery layer, because the first cliff tile can
    /// be seen without revealing occupants of the plateau above it.
    pub fn visibility(&self, player: PlayerId, position: Position) -> Visibility {
        if player.0 >= self.map.players || !self.map.contains(position) {
            return Visibility::Unexplored;
        }
        if !self.map.fog_of_war {
            return Visibility::Visible;
        }
        let columns = (self.map.width + 31) / 32;
        let cell = self.state.fog[usize::from(player.0)]
            [(position.y / 32 * columns + position.x / 32) as usize];
        let height = self.map.height_at(position).unwrap_or(0);
        let bit = 1 << height;
        if cell & (bit << 4) != 0 {
            Visibility::Visible
        } else if cell & bit != 0 {
            Visibility::Explored
        } else {
            Visibility::Unexplored
        }
    }

    pub fn terrain_visibility(&self, player: PlayerId, position: Position) -> Visibility {
        if player.0 >= self.map.players || !self.map.contains(position) {
            return Visibility::Unexplored;
        }
        if !self.map.fog_of_war {
            return Visibility::Visible;
        }
        let columns = (self.map.width + 31) / 32;
        match self.state.terrain_fog[usize::from(player.0)]
            [(position.y / 32 * columns + position.x / 32) as usize]
        {
            2 => Visibility::Visible,
            1 => Visibility::Explored,
            _ => Visibility::Unexplored,
        }
    }

    pub fn entity_visible(&self, player: PlayerId, id: EntityId) -> bool {
        let Some(entity) = self.state.entities.iter().find(|entity| entity.id == id) else {
            return false;
        };
        let unit = self.unit_type(entity.unit_type).expect("validated type");
        if unit.revealer
            || self.inside_structure(entity)
            || entity.garrisoned_in.is_some()
            || self.construction_pending(entity)
        {
            return false;
        }
        if entity.owner == player {
            return true;
        }
        if self.view.as_ref().is_some_and(|view| view.player == player) {
            // The server already applied visibility and detection. A client
            // does not receive all allied detectors or hidden detection state.
            return true;
        }
        let visibility = if entity.airborne || unit.movement_class == MovementClass::Air {
            self.terrain_visibility(player, entity.position)
        } else {
            self.visibility(player, entity.position)
        };
        // Concealment persists through a configured reveal transition.
        !self.undetected(player, entity) && visibility == Visibility::Visible
    }

    pub(super) fn undetected(&self, player: PlayerId, entity: &Entity) -> bool {
        (self.concealed(entity) || entity.cloak_transition != 0)
            && !self.detected(player, entity.position)
            && !entity.ability_auras.iter().any(|a| {
                matches!(
                    self.effect_definition(a.ability),
                    Some(AbilityEffect::SlowArea { .. })
                )
            })
    }

    pub(super) fn update_vision(&mut self) {
        if !self.map.fog_of_war {
            return;
        }
        for fog in &mut self.state.fog {
            for cell in fog {
                *cell &= 0x0f;
            }
        }
        for fog in &mut self.state.terrain_fog {
            for cell in fog {
                if *cell == 2 {
                    *cell = 1;
                }
            }
        }
        let sources: Vec<_> = self
            .state
            .entities
            .iter()
            .filter(|entity| {
                !entity.gathering_inside
                    && entity.garrisoned_in.is_none()
                    && entity.doodad_enabled != Some(false)
            })
            .map(|entity| {
                let unit = &self.rules.units[self
                    .rules
                    .units
                    .binary_search_by_key(&entity.unit_type, |unit| unit.id)
                    .expect("validated type")];
                (
                    entity.owner,
                    entity.position,
                    self.vision_range(entity),
                    unit.revealer || entity.airborne || unit.movement_class == MovementClass::Air,
                )
            })
            .chain(self.state.entities.iter().filter_map(|entity| {
                // An interior worker still sees the resource it occupies. Its
                // body stays hidden and enemy observers receive no worker data.
                if !entity.gathering_inside || entity.dropoff_target.is_some() {
                    return None;
                }
                let UnitOrder::Gather { resource } = entity.order else {
                    return None;
                };
                let node = self
                    .state
                    .resources
                    .iter()
                    .find(|node| node.id == resource)?;
                Some((
                    entity.owner,
                    node.position,
                    self.vision_range(entity),
                    false,
                ))
            }))
            .chain(
                self.state
                    .scans
                    .iter()
                    .map(|scan| (scan.owner, scan.position, scan.radius, true)),
            )
            .chain(self.state.pending_effects.iter().filter_map(|e| {
                let flight = e.flight.as_ref()?;
                let AbilityEffect::Strike {
                    delivery: Some(d), ..
                } = self.effect_definition(e.ability)?
                else {
                    return None;
                };
                (d.reveal_radius > 0
                    && matches!(
                        flight.stage,
                        StrikeStage::Ascent | StrikeStage::Flight | StrikeStage::Impact
                    ))
                .then_some((e.owner, flight.position, d.reveal_radius, true))
            }))
            .chain(self.state.entities.iter().flat_map(|e| {
                e.ability_auras.iter().filter_map(|a| {
                    matches!(
                        self.effect_definition(a.ability),
                        Some(AbilityEffect::Parasite)
                    )
                    .then_some((
                        a.owner,
                        e.position,
                        self.vision_range(e),
                        self.movement_class(e) == MovementClass::Air,
                    ))
                })
            }))
            .collect();
        let current: BTreeSet<_> = sources
            .iter()
            .map(|(_, position, radius, ignores_terrain)| (*position, *radius, *ignores_terrain))
            .collect();
        self.vision_cells.retain(|key, _| current.contains(key));
        let mut cached_cells: usize = self.vision_cells.values().map(Vec::len).sum();
        let mut terrain = None;
        for (owner, position, radius, ignores_terrain) in sources {
            let uncached;
            let cells: &[usize] = match self.vision_cells.entry((position, radius, ignores_terrain))
            {
                std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
                std::collections::btree_map::Entry::Vacant(entry) => {
                    let tiles = if ignores_terrain {
                        None
                    } else {
                        Some(
                            terrain
                                .get_or_insert_with(|| fog_terrain(&self.map))
                                .as_slice(),
                        )
                    };
                    let cells = propagated_cells(&self.map, position, radius, tiles);
                    if cached_cells + cells.len() <= MAX_CACHED_VISION_CELLS {
                        cached_cells += cells.len();
                        entry.insert(cells)
                    } else {
                        uncached = cells;
                        &uncached
                    }
                }
            };
            let heights = if ignores_terrain {
                0x0f
            } else {
                (1_u8 << (self.map.height_at(position).unwrap_or(0) + 1)) - 1
            };
            let fog = &mut self.state.fog[usize::from(owner.0)];
            let terrain_fog = &mut self.state.terrain_fog[usize::from(owner.0)];
            for &cell in cells {
                fog[cell] |= heights | (heights << 4);
                terrain_fog[cell] = 2;
            }
        }
    }
}

/// Fog propagation follows the source's 32px tile height (three quarters of
/// the tile's area at or above that level), while observers/objects use their
/// exact terrain cell height. Any opaque minitile blocks further propagation.
/// Facts: OpenBW bwgame.h load_tile_stuff/reveal_sight_at/generate_sight_values,
/// https://github.com/OpenBW/openbw/blob/master/bwgame.h . No source code copied.
#[derive(Clone, Copy, Default)]
struct SightTile {
    height: u8,
    opaque: bool,
}

fn fog_terrain(map: &Map) -> Vec<SightTile> {
    let columns = ((map.width + 31) / 32) as usize;
    let rows = ((map.height + 31) / 32) as usize;
    let mut tiles = vec![SightTile::default(); columns * rows];
    let Some(terrain) = &map.terrain else {
        return tiles;
    };
    let size = terrain.cell_size as usize;
    for y in 0..rows {
        for x in 0..columns {
            let (left, top) = (x * 32, y * 32);
            let right = ((x + 1) * 32).min(map.width as usize);
            let bottom = ((y + 1) * 32).min(map.height as usize);
            let mut areas = [0; 4];
            let tile = &mut tiles[y * columns + x];
            for ty in top / size..=(bottom - 1) / size {
                for tx in left / size..=(right - 1) / size {
                    let flags = terrain.flags[ty * terrain.columns as usize + tx];
                    let height = (flags & crate::map::HEIGHT_MASK) >> crate::map::HEIGHT_SHIFT;
                    let width = right.min((tx + 1) * size) - left.max(tx * size);
                    let height_pixels = bottom.min((ty + 1) * size) - top.max(ty * size);
                    areas[usize::from(height)] += width * height_pixels;
                    tile.opaque |= flags & crate::map::BLOCKS_SIGHT != 0;
                }
            }
            let total = (right - left) * (bottom - top);
            let mut above = 0;
            for height in (1..=3).rev() {
                above += areas[height];
                if above * 4 >= total * 3 {
                    tile.height = height as u8;
                    break;
                }
            }
        }
    }
    tiles
}

fn propagated_cells(
    map: &Map,
    origin: Position,
    radius: u32,
    terrain: Option<&[SightTile]>,
) -> Vec<usize> {
    let columns = (map.width + 31) / 32;
    let rows = (map.height + 31) / 32;
    let (ox, oy) = (origin.x / 32, origin.y / 32);
    let height = map.height_at(origin).unwrap_or(0);
    // The source stencil includes the surrounding 3x3 core and extends one
    // tile beyond its whole-tile range. This native circle approximates its
    // historical rounded fringe; propagation and predecessor rules match.
    let extent = radius as i32 / 32 + 1;
    let diameter = i64::from(extent * 2 + 1);
    let (left, top) = ((ox - extent).max(0), (oy - extent).max(0));
    let (right, bottom) = ((ox + extent).min(columns - 1), (oy + extent).min(rows - 1));
    let width = right - left + 1;
    let mut passes = vec![false; (width * (bottom - top + 1)) as usize];
    let mut cells = Vec::new();
    let mut visit = |dx: i32, dy: i32| {
        let (x, y) = (ox + dx, oy + dy);
        if x < left
            || x > right
            || y < top
            || y > bottom
            || 4 * (i64::from(dx).pow(2) + i64::from(dy).pow(2)) > diameter * diameter
        {
            return;
        }
        let can_pass = |px: i32, py: i32| {
            let (x, y) = (ox + px, oy + py);
            x >= left
                && x <= right
                && y >= top
                && y <= bottom
                && passes[((y - top) * width + x - left) as usize]
        };
        if terrain.is_some() && dx.abs().max(dy.abs()) > 1 {
            let (px, py) = (dx - dx.signum(), dy - dy.signum());
            let second = if dx == 0 || dy == 0 || dx.abs() == dy.abs() {
                false
            } else if dx.abs() > dy.abs() {
                can_pass(px, dy)
            } else {
                can_pass(dx, py)
            };
            if !can_pass(px, py) && !second {
                return;
            }
        }
        let index = (y * columns + x) as usize;
        // The first higher/opaque tile is visible; its flags stop propagation
        // to successors. Testing the destination first would erase cliff faces.
        cells.push(index);
        passes[((y - top) * width + x - left) as usize] =
            terrain.is_none_or(|tiles| tiles[index].height <= height && !tiles[index].opaque);
    };
    visit(0, 0);
    let rings = (ox - left).max(right - ox).max(oy - top).max(bottom - oy);
    for ring in 1..=rings {
        for dx in -ring..=ring {
            visit(dx, -ring);
            visit(dx, ring);
        }
        for dy in -ring + 1..ring {
            visit(-ring, dy);
            visit(ring, dy);
        }
    }
    cells
}

#[cfg(test)]
#[path = "vision/tests.rs"]
mod tests;
