use super::*;

type StopKey = ([u16; 2], u8, Position, Position);
type EstimateKey = (StopKey, bool, Option<u64>);
type RouteEstimate = Option<(u64, Position)>;

#[derive(Debug)]
struct ExhaustedRegion {
    footprint: Footprint,
    class: MovementClass,
    cells: std::sync::Arc<[bool]>,
}

#[derive(Debug, Default)]
struct TerrainRegions {
    ground: std::sync::OnceLock<Vec<u32>>,
    water: std::sync::OnceLock<Vec<u32>>,
}

/// Immutable collision grids shared by searches until static geometry changes.
/// Each movement class is prepared lazily; moving-unit detours use their own data.
#[derive(Debug, Default)]
pub(crate) struct NavigationData {
    terrain_regions: std::sync::Arc<TerrainRegions>,
    ground: std::sync::OnceLock<ClearanceData>,
    water: std::sync::OnceLock<ClearanceData>,
    air: std::sync::OnceLock<ClearanceData>,
    /// Exact nearest-stop proofs for stationary blocked units.
    /// Reset with the geometry; bound storage independently of map size.
    nearest_stops: std::sync::Mutex<std::collections::BTreeSet<StopKey>>,
    /// Repeated traffic probes need the ideal cost, not another copy of its path.
    estimates: std::sync::Mutex<std::collections::BTreeMap<EstimateKey, RouteEstimate>>,
    /// Complete failed-search proofs, shared across entrances and stationary
    /// workers. At most eight native grids (8 MiB at the map-size limit).
    exhausted_regions: std::sync::Mutex<Vec<ExhaustedRegion>>,
}

impl NavigationData {
    /// Buildings and resource depletion change collision, not the underlying
    /// terrain. Preserve island labels when discarding obstacle-dependent data.
    pub(crate) fn with_unchanged_terrain(&self) -> Self {
        Self {
            terrain_regions: std::sync::Arc::clone(&self.terrain_regions),
            ..Self::default()
        }
    }
}

pub(crate) struct Pathfinder<'a> {
    map: &'a Map,
    obstacles: &'a [Obstacle],
    data: &'a NavigationData,
    cost_limit: Option<u64>,
}

impl<'a> Pathfinder<'a> {
    pub(crate) fn new(map: &'a Map, obstacles: &'a [Obstacle], data: &'a NavigationData) -> Self {
        Self {
            map,
            obstacles,
            data,
            cost_limit: None,
        }
    }
    pub(crate) fn with_cost_limit(mut self, cost: u64) -> Self {
        self.cost_limit = Some(cost);
        self
    }
    fn within_limit(&self, start: Position, end: Position) -> bool {
        self.cost_limit
            .is_none_or(|limit| distance(start, end) <= limit)
    }
    fn limits(&self, range: Option<u32>) -> SearchLimits {
        SearchLimits {
            range,
            cost: self.cost_limit,
        }
    }
    fn slot(&self, class: MovementClass) -> &std::sync::OnceLock<ClearanceData> {
        match class {
            MovementClass::Ground => &self.data.ground,
            MovementClass::Water => &self.data.water,
            MovementClass::Air => &self.data.air,
        }
    }
    fn clear(
        &self,
        grid: Grid,
        footprint: Footprint,
        class: MovementClass,
        start: Position,
        end: Position,
    ) -> bool {
        if let Some(data) = self.slot(class).get() {
            Clearance {
                map: self.map,
                grid,
                footprint,
                data,
            }
            .clear(start, end)
        } else {
            // A first unobstructed route should not prepare a whole map.
            segment_clear(self.map, footprint, class, start, end, self.obstacles)
        }
    }
    fn clearance(&self, grid: Grid, footprint: Footprint, class: MovementClass) -> Clearance<'_> {
        Clearance {
            map: self.map,
            grid,
            footprint,
            data: self
                .slot(class)
                .get_or_init(|| ClearanceData::new(self.map, grid, class, self.obstacles)),
        }
    }
    fn same_terrain_region(
        &self,
        grid: Grid,
        class: MovementClass,
        start: Position,
        target: Position,
    ) -> bool {
        let Some(terrain) = &self.map.terrain else {
            return true;
        };
        let (slot, flag) = match class {
            MovementClass::Ground => (&self.data.terrain_regions.ground, WALKABLE),
            MovementClass::Water => (&self.data.terrain_regions.water, WATER),
            MovementClass::Air => return true,
        };
        let regions = slot.get_or_init(|| {
            let mut labels = vec![0; grid.columns * grid.rows];
            let mut next = 0;
            let mut pending = Vec::new();
            for cell in 0..labels.len() {
                if labels[cell] != 0 || terrain.flags[cell] & flag == 0 {
                    continue;
                }
                next += 1;
                labels[cell] = next;
                pending.push(cell);
                while let Some(at) = pending.pop() {
                    // A finite footprint cannot cut between diagonal islands
                    // touching only at a corner. Cardinal labels are a safe
                    // lower bound; exact footprint clearance still follows.
                    let x = at % grid.columns;
                    let y = at / grid.columns;
                    for neighbor in [
                        (x > 0).then(|| at - 1),
                        (x + 1 < grid.columns).then_some(at + 1),
                        (y > 0).then(|| at - grid.columns),
                        (y + 1 < grid.rows).then_some(at + grid.columns),
                    ]
                    .into_iter()
                    .flatten()
                    {
                        if labels[neighbor] == 0 && terrain.flags[neighbor] & flag != 0 {
                            labels[neighbor] = next;
                            pending.push(neighbor);
                        }
                    }
                }
            }
            labels
        });
        let cell = |p: Position| {
            p.y as usize / grid.size as usize * grid.columns + p.x as usize / grid.size as usize
        };
        let label = regions[cell(start)];
        label != 0 && label == regions[cell(target)]
    }
    fn exhausted_region(
        &self,
        clearance: &Clearance<'_>,
        class: MovementClass,
        start: Position,
    ) -> Option<std::sync::Arc<[bool]>> {
        if self.cost_limit.is_some() {
            return None;
        }
        let entries: Vec<_> = clearance
            .grid
            .near(start)
            .filter(|&node| {
                start_connection(start, clearance.grid.position(node), |a, b| {
                    clearance.clear(a, b)
                })
                .is_some()
            })
            .collect();
        if entries.is_empty() {
            return None;
        }
        self.data
            .exhausted_regions
            .lock()
            .expect("path cache poisoned")
            .iter()
            .rev()
            .find(|region| {
                region.footprint == clearance.footprint
                    && region.class == class
                    && entries.iter().all(|&node| region.cells[node])
            })
            .map(|region| std::sync::Arc::clone(&region.cells))
    }
    fn remember_exhausted_region(
        &self,
        footprint: Footprint,
        class: MovementClass,
        cells: std::sync::Arc<[bool]>,
    ) {
        if self.cost_limit.is_some() || !cells.iter().any(|&cell| cell) {
            return;
        }
        let mut regions = self
            .data
            .exhausted_regions
            .lock()
            .expect("path cache poisoned");
        if regions.len() == 8 {
            regions.remove(0);
        }
        regions.push(ExhaustedRegion {
            footprint,
            class,
            cells,
        });
    }
    /// Returns waypoints excluding `start` and ending at the exact `target`.
    /// An already reached target returns an empty path. Invalid, occupied, or
    /// unreachable endpoints return None. The caller must exclude the moving
    /// entity from obstacles. Air units collide only with other air obstacles.
    pub(crate) fn find_path(
        &self,
        footprint: Footprint,
        class: MovementClass,
        start: Position,
        target: Position,
    ) -> Option<Vec<Position>> {
        let map = self.map;
        let obstacles = self.obstacles;
        let grid = Grid::new(map)?;
        if obstacles.len() > MAX_OBSTACLES
            || !map.contains_footprint(start, footprint)
            || !map.contains_footprint(target, footprint)
        {
            return None;
        }
        if !self.clear(grid, footprint, class, start, start)
            || !self.clear(grid, footprint, class, target, target)
        {
            return None;
        }
        if !self.within_limit(start, target) {
            return None;
        }
        if start == target {
            return Some(Vec::new());
        }
        if self.clear(grid, footprint, class, start, target) {
            return Some(vec![target]);
        }
        if !self.same_terrain_region(grid, class, start, target) {
            return None;
        }
        let clearance = self.clearance(grid, footprint, class);
        search(
            &clearance,
            start,
            target,
            None,
            false,
            None,
            self.limits(None),
        )
    }

    /// Stop at the nearest reachable edge of a circular interaction range. Search
    /// the whole goal region, rather than routing to its center and stopping late.
    pub(crate) fn find_path_in_range(
        &self,
        footprint: Footprint,
        class: MovementClass,
        start: Position,
        target: Position,
        range: u32,
    ) -> Option<Vec<Position>> {
        let map = self.map;
        let obstacles = self.obstacles;
        let grid = Grid::new(map)?;
        if obstacles.len() > MAX_OBSTACLES
            || !map.contains_footprint(start, footprint)
            || !map.contains(target)
            || !self.clear(grid, footprint, class, start, start)
        {
            return None;
        }
        let endpoint = range_endpoint(start, target, range);
        if !self.within_limit(start, endpoint) {
            return None;
        }
        if start == endpoint {
            return Some(Vec::new());
        }
        if map.contains_footprint(endpoint, footprint)
            && self.clear(grid, footprint, class, start, endpoint)
        {
            return Some(vec![endpoint]);
        }
        let clearance = self.clearance(grid, footprint, class);
        search(
            &clearance,
            start,
            target,
            None,
            false,
            None,
            self.limits(Some(range)),
        )
    }

    /// Probe approach points in their supplied order, reusing the terrain and
    /// obstacle prefix grids. The first successful path is exactly the one that
    /// separate `find_path` calls would return; disconnected sides stay eligible.
    pub(crate) fn find_path_to_any(
        &self,
        footprint: Footprint,
        class: MovementClass,
        start: Position,
        targets: &[Position],
    ) -> Option<(Position, Vec<Position>)> {
        let map = self.map;
        let obstacles = self.obstacles;
        let grid = Grid::new(map)?;
        if obstacles.len() > MAX_OBSTACLES
            || !map.contains_footprint(start, footprint)
            || !self.clear(grid, footprint, class, start, start)
        {
            return None;
        }
        let mut clearance = None;
        let mut reachable: Option<std::sync::Arc<[bool]>> = None;
        let mut checked_cache = false;
        for &target in targets {
            if !map.contains_footprint(target, footprint)
                || !self.clear(grid, footprint, class, target, target)
            {
                continue;
            }
            if !self.within_limit(start, target) {
                continue;
            }
            if start == target {
                return Some((target, Vec::new()));
            }
            if self.clear(grid, footprint, class, start, target) {
                return Some((target, vec![target]));
            }
            if !self.same_terrain_region(grid, class, start, target) {
                continue;
            }
            let grid = clearance.get_or_insert_with(|| self.clearance(grid, footprint, class));
            if !checked_cache {
                reachable = self.exhausted_region(grid, class, start);
                checked_cache = true;
            }
            if let Some(visited) = &reachable {
                // An exhausted search has proved the whole start component.
                // Other isolated entrance points need no repeat flood of it.
                if !grid
                    .grid
                    .near(target)
                    .any(|node| visited[node] && grid.clear(grid.grid.position(node), target))
                {
                    continue;
                }
            }
            let mut exhausted = None;
            if let Some(path) = search(
                grid,
                start,
                target,
                None,
                false,
                Some(&mut exhausted),
                self.limits(None),
            ) {
                return Some((target, path));
            }
            if let Some(cells) = exhausted {
                let cells: std::sync::Arc<[bool]> = cells.into();
                self.remember_exhausted_region(footprint, class, std::sync::Arc::clone(&cells));
                reachable = Some(cells);
            }
        }
        None
    }

    /// Like `find_path`, but occupied or disconnected destinations resolve to the
    /// nearest reachable stop. Reachable clear targets retain exact-path behavior.
    /// Targets must be inside the map, though the moving footprint may overhang
    /// there (for example at an edge).
    /// Stops are grid centers reachable from `start`, ranked by octile distance to
    /// `target`, route cost, then row-major cell ID. Staying at `start` is preferred
    /// if no grid center gets closer. The caller must exclude the moving entity.
    pub(crate) fn find_path_near(
        &self,
        footprint: Footprint,
        class: MovementClass,
        start: Position,
        target: Position,
    ) -> Option<Vec<Position>> {
        if self.cost_limit.is_none() && self.known_nearest_stop(footprint, class, start, target) {
            return Some(Vec::new());
        }
        let route = self.find_path_near_uncached(footprint, class, start, target);
        if self.cost_limit.is_none() && route.as_ref().is_some_and(Vec::is_empty) {
            let mut stops = self.data.nearest_stops.lock().expect("path cache poisoned");
            if stops.len() == 4096 {
                stops.clear();
            }
            stops.insert(Self::stop_key(footprint, class, start, target));
        }
        route
    }

    pub(crate) fn route_estimate(
        &self,
        footprint: Footprint,
        class: MovementClass,
        start: Position,
        target: Position,
        near: bool,
    ) -> Option<(u64, Position)> {
        let key = (
            Self::stop_key(footprint, class, start, target),
            near,
            self.cost_limit,
        );
        if let Some(estimate) = self
            .data
            .estimates
            .lock()
            .expect("path cache poisoned")
            .get(&key)
        {
            return *estimate;
        }
        let route = if near {
            self.find_path_near(footprint, class, start, target)
        } else {
            self.find_path(footprint, class, start, target)
        };
        let estimate = route.map(|route| {
            let mut from = start;
            let mut cost = 0;
            for point in route {
                cost += distance(from, point);
                from = point;
            }
            (cost, from)
        });
        let mut estimates = self.data.estimates.lock().expect("path cache poisoned");
        if estimates.len() == 4096 {
            estimates.clear();
        }
        estimates.insert(key, estimate);
        estimate
    }

    pub(crate) fn has_start_connector(
        &self,
        footprint: Footprint,
        class: MovementClass,
        start: Position,
    ) -> bool {
        Grid::new(self.map).is_some_and(|grid| {
            grid.near(start).any(|node| {
                start_connection(start, grid.position(node), |a, b| {
                    self.clear(grid, footprint, class, a, b)
                })
                .is_some()
            })
        })
    }
    pub(crate) fn known_nearest_stop(
        &self,
        footprint: Footprint,
        class: MovementClass,
        start: Position,
        target: Position,
    ) -> bool {
        self.data
            .nearest_stops
            .lock()
            .expect("path cache poisoned")
            .contains(&Self::stop_key(footprint, class, start, target))
    }

    fn stop_key(
        footprint: Footprint,
        class: MovementClass,
        start: Position,
        target: Position,
    ) -> StopKey {
        (
            [footprint.width, footprint.height],
            match class {
                MovementClass::Ground => 0,
                MovementClass::Water => 1,
                MovementClass::Air => 2,
            },
            start,
            target,
        )
    }

    fn find_path_near_uncached(
        &self,
        footprint: Footprint,
        class: MovementClass,
        start: Position,
        target: Position,
    ) -> Option<Vec<Position>> {
        let map = self.map;
        let grid = Grid::new(map)?;
        if self.obstacles.len() > MAX_OBSTACLES
            || !map.contains_footprint(target, Footprint::default())
            || !self.clear(grid, footprint, class, start, start)
        {
            return None;
        }
        if self.clear(grid, footprint, class, target, target) {
            if start == target {
                return Some(Vec::new());
            }
            if self.clear(grid, footprint, class, start, target) {
                return self.within_limit(start, target).then(|| vec![target]);
            }
            // Keep the closest reached node during the exact search. A clear
            // destination on a disconnected island then needs no second search.
            let clearance = self.clearance(grid, footprint, class);
            return search(
                &clearance,
                start,
                target,
                None,
                true,
                None,
                self.limits(None),
            );
        }

        // Locate the closest clear centers without searching routes to each one.
        // Distance increases monotonically outward from the target's neighborhood,
        // so a single occupied unit only needs a small local scan. Do this before
        // constructing terrain prefix sums or A* arrays: unobstructed approaches to
        // the best center can return immediately, even on a maximum-sized map.
        let mut nearest = u64::MAX;
        let mut endpoints = Vec::new();
        {
            let mut seen = vec![false; grid.columns * grid.rows];
            let mut queue = BinaryHeap::new();
            for node in grid.near(target) {
                seen[node] = true;
                queue.push(Reverse((distance(grid.position(node), target), node)));
            }
            while let Some(Reverse((remaining, node))) = queue.pop() {
                if remaining > nearest {
                    break;
                }
                if self
                    .cost_limit
                    .is_some_and(|limit| remaining > distance(start, target).saturating_add(limit))
                {
                    return None;
                }
                let position = grid.position(node);
                if self.clear(grid, footprint, class, position, position) {
                    nearest = remaining;
                    endpoints.push(node);
                }
                for neighbor in grid.neighbors(node) {
                    if !seen[neighbor] {
                        seen[neighbor] = true;
                        queue.push(Reverse((
                            distance(grid.position(neighbor), target),
                            neighbor,
                        )));
                    }
                }
            }
        }
        if distance(start, target) <= nearest {
            return Some(Vec::new());
        }
        let direct = endpoints
            .iter()
            .min_by_key(|&&node| (distance(start, grid.position(node)), node))
            .copied()?;
        let position = grid.position(direct);
        if self.within_limit(start, position) && self.clear(grid, footprint, class, start, position)
        {
            return Some(vec![position]);
        }

        let clearance = self.clearance(grid, footprint, class);
        search(
            &clearance,
            start,
            target,
            Some(nearest),
            true,
            None,
            self.limits(None),
        )
    }
}
