use super::*;
use std::collections::HashMap;

type Cell = (i64, i64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CellRect([i64; 4]);

impl CellRect {
    fn new(bounds: Bounds, size: i64) -> Self {
        Self([
            bounds[0].div_euclid(size),
            bounds[1].div_euclid(size),
            (bounds[2] - 1).div_euclid(size),
            (bounds[3] - 1).div_euclid(size),
        ])
    }
    fn contains(self, (x, y): Cell) -> bool {
        x >= self.0[0] && y >= self.0[1] && x <= self.0[2] && y <= self.0[3]
    }
    fn cells(self) -> impl Iterator<Item = Cell> {
        (self.0[1]..=self.0[3]).flat_map(move |y| (self.0[0]..=self.0[2]).map(move |x| (x, y)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logical_cell_differences_preserve_overlap_and_remove_empty_memberships() {
        let mut index = SpatialIndex {
            size: 32,
            buckets: HashMap::new(),
            entries: HashMap::new(),
            generation: 1,
        };
        let body = ColliderId::Entity(EntityId(1));
        let resource = ColliderId::Resource(ResourceId(1));
        index.upsert(body, [0, 0, 64, 64]);
        assert_eq!(index.buckets.len(), 4);
        index.upsert(body, [32, 0, 96, 64]);
        assert_eq!(index.buckets.len(), 4);
        assert!(index.candidates([0, 0, 32, 64]).is_empty());
        assert_eq!(index.candidates([32, 0, 96, 64]), [body]);
        index.upsert(resource, [32, 0, 64, 32]);
        assert_eq!(index.candidates([32, 0, 64, 32]), [body, resource]);
        index.remove(body);
        index.remove(resource);
        assert!(index.entries.is_empty() && index.buckets.is_empty());
        // Negative coordinates and half-open edges use mathematical floor.
        index.upsert(body, [-33, -1, 0, 1]);
        assert_eq!(index.buckets.len(), 4);
        assert!(index.candidates([0, 0, 1, 1]).is_empty());
    }
}

#[derive(Debug)]
struct Entry {
    cells: CellRect,
    seen: u64,
}

#[derive(Debug)]
pub(super) struct SpatialIndex {
    // Coordinates are logical cells in map space, independent of the renderer.
    size: i64,
    buckets: HashMap<Cell, Vec<ColliderId>>,
    entries: HashMap<ColliderId, Entry>,
    generation: u64,
}

impl SpatialIndex {
    pub fn new(world: &World) -> Self {
        // Group fine terrain cells to keep membership bounded for large bodies.
        let native = world.map.terrain.as_ref().map_or(8, |t| t.cell_size);
        let size = i64::from(native * 32_u32.div_ceil(native));
        Self {
            size,
            buckets: HashMap::new(),
            entries: HashMap::new(),
            generation: 0,
        }
    }
    pub fn upsert(&mut self, id: ColliderId, bounds: Bounds) {
        let cells = CellRect::new(bounds, self.size);
        let old = self.entries.get(&id).map(|e| e.cells);
        if old != Some(cells) {
            if let Some(old) = old {
                for cell in old.cells().filter(|&cell| !cells.contains(cell)) {
                    self.remove_membership(cell, id);
                }
            }
            for cell in cells
                .cells()
                .filter(|&cell| old.is_none_or(|old| !old.contains(cell)))
            {
                self.buckets.entry(cell).or_default().push(id);
            }
        }
        self.entries.insert(
            id,
            Entry {
                cells,
                seen: self.generation,
            },
        );
    }
    fn remove_membership(&mut self, cell: Cell, id: ColliderId) {
        if let Some(bucket) = self.buckets.get_mut(&cell) {
            if let Some(at) = bucket.iter().position(|&item| item == id) {
                bucket.swap_remove(at);
            }
            if bucket.is_empty() {
                self.buckets.remove(&cell);
            }
        }
    }
    pub fn remove(&mut self, id: ColliderId) {
        if let Some(entry) = self.entries.remove(&id) {
            for cell in entry.cells.cells() {
                self.remove_membership(cell, id);
            }
        }
    }
    pub fn candidates(&self, bounds: Bounds) -> Vec<ColliderId> {
        if bounds[0] >= bounds[2] || bounds[1] >= bounds[3] {
            return Vec::new();
        }
        let cells = CellRect::new(bounds, self.size);
        let mut found = Vec::new();
        // A long diagonal can cover most of the map. Never enumerate a huge
        // empty coordinate rectangle when scanning occupied buckets is cheaper.
        let area = (cells.0[2] - cells.0[0] + 1) as u64 * (cells.0[3] - cells.0[1] + 1) as u64;
        if area > self.buckets.len() as u64 {
            for (&cell, bucket) in &self.buckets {
                if cells.contains(cell) {
                    found.extend_from_slice(bucket);
                }
            }
        } else {
            for cell in cells.cells() {
                if let Some(bucket) = self.buckets.get(&cell) {
                    found.extend_from_slice(bucket);
                }
            }
        }
        found.sort_unstable();
        found.dedup();
        found
    }
    pub fn sync_entity(&mut self, world: &World, id: EntityId) {
        let body = ColliderId::Entity(id);
        let reservation = ColliderId::Reservation(id);
        let Some(at) = world.index(id) else {
            self.remove(body);
            self.remove(reservation);
            return;
        };
        let actor = &world.state.entities[at];
        let unit = world.unit_at(at);
        let a = unit.footprint.bounds(actor.position);
        let b = unit.placement.bounds(actor.position);
        self.upsert(
            body,
            [
                a[0].min(b[0]),
                a[1].min(b[1]),
                a[2].max(b[2]),
                a[3].max(b[3]),
            ],
        );
        if let Some(spot) = actor.harvest_spot {
            self.upsert(reservation, unit.footprint.bounds(spot));
        } else {
            self.remove(reservation);
        }
    }
    pub fn sync_resource(&mut self, world: &World, id: ResourceId) {
        let key = ColliderId::Resource(id);
        if let Some(node) = world
            .collision_resource(id)
            .filter(|n| n.amount > 0 || n.requires_extractor)
        {
            self.upsert(key, node.footprint.bounds(node.position));
        } else {
            self.remove(key);
        }
    }
    pub fn reconcile(&mut self, world: &World) {
        self.generation += 1;
        for actor in &world.state.entities {
            self.sync_entity(world, actor.id);
        }
        for node in &world.state.resources {
            self.sync_resource(world, node.id);
        }
        let removed: Vec<_> = self
            .entries
            .iter()
            .filter(|(_, e)| e.seen != self.generation)
            .map(|(&id, _)| id)
            .collect();
        for id in removed {
            self.remove(id);
        }
    }
}
