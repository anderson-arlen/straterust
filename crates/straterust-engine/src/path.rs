//! Deterministic ground/air navigation on the native terrain grid.
//!
//! Search uses cell centers, with exact world-coordinate endpoints. Routes
//! narrower than this lattice can be missed; this is not reference-game
//! pathfinding. Swept rectangles deliberately forbid diagonal corner cutting.
//! Cardinal travel costs 10 per world unit, diagonal travel 14. Equal heap
//! priorities prefer smaller remaining distance, then the row-major cell ID.
use std::{cmp::Reverse, collections::BinaryHeap};

use crate::{
    map::{Footprint, MAX_TERRAIN_DIMENSION, MovementClass, WALKABLE},
    sim::{Map, Position},
};

/// The package limits permit 4096 entities plus 4096 resource placements.
pub const MAX_OBSTACLES: usize = 8192;

#[derive(Clone, Copy, Debug)]
pub struct Obstacle {
    pub position: Position,
    pub footprint: Footprint,
    pub movement_class: MovementClass,
}

/// Returns waypoints excluding `start` and ending at the exact `target`.
/// An already reached target returns an empty path. Invalid, occupied, or
/// unreachable endpoints return None. The caller must exclude the moving
/// entity from obstacles. Air units collide only with other air obstacles.
pub fn find_path(
    map: &Map,
    footprint: Footprint,
    class: MovementClass,
    start: Position,
    target: Position,
    obstacles: &[Obstacle],
) -> Option<Vec<Position>> {
    let grid = Grid::new(map)?;
    if obstacles.len() > MAX_OBSTACLES
        || !map.contains_footprint(start, footprint)
        || !map.contains_footprint(target, footprint)
    {
        return None;
    }
    if !segment_clear(map, footprint, class, start, start, obstacles)
        || !segment_clear(map, footprint, class, target, target, obstacles)
    {
        return None;
    }
    if start == target {
        return Some(Vec::new());
    }
    if segment_clear(map, footprint, class, start, target, obstacles) {
        return Some(vec![target]);
    }
    let clearance = Clearance::new(map, grid, footprint, class, obstacles);
    search(&clearance, start, target, None, false, None, None)
}

/// Stop at the nearest reachable edge of a circular interaction range. Search
/// the whole goal region, rather than routing to its center and stopping late.
pub fn find_path_in_range(
    map: &Map,
    footprint: Footprint,
    class: MovementClass,
    start: Position,
    target: Position,
    range: u32,
    obstacles: &[Obstacle],
) -> Option<Vec<Position>> {
    let grid = Grid::new(map)?;
    if obstacles.len() > MAX_OBSTACLES
        || !map.contains_footprint(start, footprint)
        || !map.contains(target)
        || !segment_clear(map, footprint, class, start, start, obstacles)
    {
        return None;
    }
    let endpoint = range_endpoint(start, target, range);
    if start == endpoint {
        return Some(Vec::new());
    }
    if map.contains_footprint(endpoint, footprint)
        && segment_clear(map, footprint, class, start, endpoint, obstacles)
    {
        return Some(vec![endpoint]);
    }
    let clearance = Clearance::new(map, grid, footprint, class, obstacles);
    search(&clearance, start, target, None, false, None, Some(range))
}

fn range_endpoint(from: Position, target: Position, range: u32) -> Position {
    let dx = i64::from(from.x - target.x);
    let dy = i64::from(from.y - target.y);
    let square = (dx * dx + dy * dy) as u64;
    if square <= u64::from(range).pow(2) {
        return from;
    }
    let root = square.isqrt();
    let length = (root + u64::from(root * root != square)) as i64;
    Position {
        x: target.x + (dx * i64::from(range) / length) as i32,
        y: target.y + (dy * i64::from(range) / length) as i32,
    }
}

/// Probe approach points in their supplied order, reusing the terrain and
/// obstacle prefix grids. The first successful path is exactly the one that
/// separate `find_path` calls would return; disconnected sides stay eligible.
pub fn find_path_to_any(
    map: &Map,
    footprint: Footprint,
    class: MovementClass,
    start: Position,
    targets: &[Position],
    obstacles: &[Obstacle],
) -> Option<(Position, Vec<Position>)> {
    let grid = Grid::new(map)?;
    if obstacles.len() > MAX_OBSTACLES
        || !map.contains_footprint(start, footprint)
        || !segment_clear(map, footprint, class, start, start, obstacles)
    {
        return None;
    }
    let mut clearance = None;
    let mut reachable: Option<Vec<bool>> = None;
    for &target in targets {
        if !map.contains_footprint(target, footprint)
            || !segment_clear(map, footprint, class, target, target, obstacles)
        {
            continue;
        }
        if start == target {
            return Some((target, Vec::new()));
        }
        if segment_clear(map, footprint, class, start, target, obstacles) {
            return Some((target, vec![target]));
        }
        let grid =
            clearance.get_or_insert_with(|| Clearance::new(map, grid, footprint, class, obstacles));
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
        if let Some(path) = search(grid, start, target, None, false, Some(&mut reachable), None) {
            return Some((target, path));
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
pub fn find_path_near(
    map: &Map,
    footprint: Footprint,
    class: MovementClass,
    start: Position,
    target: Position,
    obstacles: &[Obstacle],
) -> Option<Vec<Position>> {
    let grid = Grid::new(map)?;
    if !map.contains_footprint(target, Footprint::default())
        || !segment_clear(map, footprint, class, start, start, obstacles)
    {
        return None;
    }
    if segment_clear(map, footprint, class, target, target, obstacles) {
        if start == target {
            return Some(Vec::new());
        }
        if segment_clear(map, footprint, class, start, target, obstacles) {
            return Some(vec![target]);
        }
        // Keep the closest reached node during the exact search. A clear
        // destination on a disconnected island then needs no second search.
        let clearance = Clearance::new(map, grid, footprint, class, obstacles);
        return search(&clearance, start, target, None, true, None, None);
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
            let position = grid.position(node);
            if segment_clear(map, footprint, class, position, position, obstacles) {
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
    if segment_clear(map, footprint, class, start, position, obstacles) {
        return Some(vec![position]);
    }

    let clearance = Clearance::new(map, grid, footprint, class, obstacles);
    search(&clearance, start, target, Some(nearest), true, None, None)
}

fn search(
    clearance: &Clearance<'_>,
    start: Position,
    target: Position,
    nearest: Option<u64>,
    allow_near: bool,
    exhausted: Option<&mut Option<Vec<bool>>>,
    range: Option<u32>,
) -> Option<Vec<Position>> {
    let grid = clearance.grid;

    // Heap keys order by estimated cost, remaining cost, then row-major ID.
    // Every cell is expanded at most once; storage and work are bounded by
    // the native grid (at most 1024 x 1024), eight edges per cell, and the
    // bounded obstacle list. Prefix sums avoid scanning large footprints.
    let goal = grid.columns * grid.rows;
    let mut costs = vec![u64::MAX; goal + 1];
    let mut parents = vec![usize::MAX; goal + 1];
    let mut closed = vec![false; goal + 1];
    let mut queue = BinaryHeap::new();
    let mut endpoint = target;
    // Octile cost is at most 11 per unit of Euclidean distance. Subtracting
    // this radius yields an admissible lower bound to the circular goal area.
    let remaining_cost = |p| distance(p, target).saturating_sub(u64::from(range.unwrap_or(0)) * 11);
    let mut best = (distance(start, target), 0, usize::MAX);
    for node in grid.near(start) {
        let position = grid.position(node);
        if clearance.clear(start, position) {
            let cost = distance(start, position);
            costs[node] = cost;
            let remaining = remaining_cost(position);
            queue.push(Reverse((cost + remaining, remaining, node)));
        }
    }
    // A packed spawn exit can temporarily prevent an off-grid unit from
    // reaching any search node. That is a failed route, not proof that its
    // current position is the closest reachable stop. Let callers retry after
    // neighboring units move; no grid expansion is needed in this case.
    if queue.is_empty() {
        if let Some(exhausted) = exhausted {
            *exhausted = Some(closed);
        }
        return None;
    }
    let goal_neighbors: Vec<_> = grid.near(target).collect();
    while let Some(Reverse((estimate, remaining, node))) = queue.pop() {
        if closed[node] || estimate - remaining != costs[node] {
            continue;
        }
        if nearest == Some(best.0) && estimate > best.1 + best.0 {
            break;
        }
        if node == goal {
            let mut path = trace_path(grid, start, parents[goal], &parents);
            if path.last() != Some(&endpoint) {
                path.push(endpoint);
            }
            return Some(path);
        }
        if allow_near {
            best = best.min((remaining, costs[node], node));
            if nearest == Some(remaining) {
                // Settle equal-cost alternatives as well, so row-major ID is
                // a tie-break across endpoints, not merely heap discovery.
                closed[node] = true;
                continue;
            }
        }
        closed[node] = true;
        let position = grid.position(node);
        let destination = range.map_or(target, |range| range_endpoint(position, target, range));
        if nearest.is_none()
            && (range.is_some() || goal_neighbors.contains(&node))
            && clearance
                .map
                .contains_footprint(destination, clearance.footprint)
            && clearance.clear(position, destination)
        {
            let cost = costs[node] + distance(position, destination);
            if cost < costs[goal] {
                costs[goal] = cost;
                parents[goal] = node;
                endpoint = destination;
                queue.push(Reverse((cost, 0, goal)));
            }
        }
        for neighbor in grid.neighbors(node) {
            if closed[neighbor] {
                continue;
            }
            let next = grid.position(neighbor);
            let cost = costs[node] + distance(position, next);
            if cost >= costs[neighbor] || !clearance.clear(position, next) {
                continue;
            }
            costs[neighbor] = cost;
            parents[neighbor] = node;
            let remaining = remaining_cost(next);
            queue.push(Reverse((cost + remaining, remaining, neighbor)));
        }
    }
    // A closer clear center may be on an inaccessible island. Only in that
    // case must this one search exhaust the reachable component to establish
    // its nearest reachable stop; no candidate starts a second A* search.
    if let Some(exhausted) = exhausted {
        *exhausted = Some(closed);
    }
    allow_near.then(|| trace_path(grid, start, best.2, &parents))
}

fn trace_path(grid: Grid, start: Position, mut node: usize, parents: &[usize]) -> Vec<Position> {
    let mut path = Vec::new();
    while node != usize::MAX {
        let position = grid.position(node);
        if position != start {
            path.push(position);
        }
        node = parents[node];
    }
    path.reverse();
    path
}

/// Checks the whole swept rectangle, not just the endpoint. Call this against
/// current obstacles before each movement step, including steps shorter than
/// a waypoint or faster than one terrain cell. Do not advance beyond a
/// waypoint without checking the following segment separately.
pub fn segment_clear(
    map: &Map,
    footprint: Footprint,
    class: MovementClass,
    start: Position,
    end: Position,
    obstacles: &[Obstacle],
) -> bool {
    if Grid::new(map).is_none()
        || obstacles.len() > MAX_OBSTACLES
        || !map.contains_footprint(start, footprint)
        || !map.contains_footprint(end, footprint)
    {
        return false;
    }
    let bounds = swept_bounds(footprint, start, end);
    let width = (bounds[2] - bounds[0]) as u16;
    let height = (bounds[3] - bounds[1]) as u16;
    let position = Position {
        x: (bounds[0] + i64::from(width / 2)) as i32,
        y: (bounds[1] + i64::from(height / 2)) as i32,
    };
    map.can_move(position, Footprint { width, height }, class)
        && !obstacles.iter().any(|obstacle| {
            obstacle.movement_class == class
                && overlaps(bounds, obstacle.footprint.bounds(obstacle.position))
        })
}

fn swept_bounds(footprint: Footprint, a: Position, b: Position) -> [i64; 4] {
    let a = footprint.bounds(a);
    let b = footprint.bounds(b);
    [
        a[0].min(b[0]),
        a[1].min(b[1]),
        a[2].max(b[2]),
        a[3].max(b[3]),
    ]
}

fn overlaps(a: [i64; 4], b: [i64; 4]) -> bool {
    b[0] < b[2] && b[1] < b[3] && a[0] < b[2] && a[2] > b[0] && a[1] < b[3] && a[3] > b[1]
}

fn distance(a: Position, b: Position) -> u64 {
    let dx = (i64::from(a.x) - i64::from(b.x)).unsigned_abs();
    let dy = (i64::from(a.y) - i64::from(b.y)).unsigned_abs();
    10 * dx.max(dy) + 4 * dx.min(dy)
}

#[derive(Clone, Copy)]
struct Grid {
    size: i64,
    columns: usize,
    rows: usize,
    width: i32,
    height: i32,
}

impl Grid {
    fn new(map: &Map) -> Option<Self> {
        if !(1..=32768).contains(&map.width) || !(1..=32768).contains(&map.height) {
            return None;
        }
        let (size, columns, rows) = if let Some(terrain) = &map.terrain {
            if terrain.cell_size == 0
                || terrain.columns == 0
                || terrain.rows == 0
                || terrain.columns > MAX_TERRAIN_DIMENSION
                || terrain.rows > MAX_TERRAIN_DIMENSION
                || terrain.flags.len() != terrain.columns as usize * terrain.rows as usize
                || u64::from(terrain.columns) * u64::from(terrain.cell_size) != map.width as u64
                || u64::from(terrain.rows) * u64::from(terrain.cell_size) != map.height as u64
            {
                return None;
            }
            (
                i64::from(terrain.cell_size),
                terrain.columns as usize,
                terrain.rows as usize,
            )
        } else {
            // Asset-independent maps use eight-world-unit cells, enlarged
            // only if needed to retain the same native grid bound.
            let size = 8.max((map.width.max(map.height) + 1023) / 1024);
            (
                i64::from(size),
                ((map.width + size - 1) / size) as usize,
                ((map.height + size - 1) / size) as usize,
            )
        };
        Some(Self {
            size,
            columns,
            rows,
            width: map.width,
            height: map.height,
        })
    }

    fn position(self, node: usize) -> Position {
        Position {
            x: ((node % self.columns) as i64 * self.size + self.size / 2)
                .min(i64::from(self.width - 1)) as i32,
            y: ((node / self.columns) as i64 * self.size + self.size / 2)
                .min(i64::from(self.height - 1)) as i32,
        }
    }

    fn near(self, position: Position) -> impl Iterator<Item = usize> {
        let x = i64::from(position.x) / self.size;
        let y = i64::from(position.y) / self.size;
        self.neighborhood(x, y, true)
    }

    fn neighbors(self, node: usize) -> impl Iterator<Item = usize> {
        self.neighborhood(
            (node % self.columns) as i64,
            (node / self.columns) as i64,
            false,
        )
    }

    fn neighborhood(self, x: i64, y: i64, include_center: bool) -> impl Iterator<Item = usize> {
        (-1..=1).flat_map(move |dy| {
            (-1..=1).filter_map(move |dx| {
                let (nx, ny) = (x + dx, y + dy);
                ((include_center || dx != 0 || dy != 0)
                    && nx >= 0
                    && ny >= 0
                    && nx < self.columns as i64
                    && ny < self.rows as i64)
                    .then(|| ny as usize * self.columns + nx as usize)
            })
        })
    }

    fn cells(self, bounds: [i64; 4]) -> [usize; 4] {
        [
            (bounds[0].max(0) / self.size).min(self.columns as i64) as usize,
            (bounds[1].max(0) / self.size).min(self.rows as i64) as usize,
            ((bounds[2].max(0) + self.size - 1) / self.size).min(self.columns as i64) as usize,
            ((bounds[3].max(0) + self.size - 1) / self.size).min(self.rows as i64) as usize,
        ]
    }
}

struct Clearance<'a> {
    map: &'a Map,
    grid: Grid,
    footprint: Footprint,
    obstacles: Vec<[i64; 4]>,
    terrain: Vec<u32>,
    obstacle_cells: Vec<u32>,
}

impl<'a> Clearance<'a> {
    fn new(
        map: &'a Map,
        grid: Grid,
        footprint: Footprint,
        class: MovementClass,
        obstacles: &[Obstacle],
    ) -> Self {
        let stride = grid.columns + 1;
        let length = stride * (grid.rows + 1);
        let obstacles: Vec<_> = obstacles
            .iter()
            .filter(|obstacle| {
                obstacle.movement_class == class
                    && obstacle.footprint.width != 0
                    && obstacle.footprint.height != 0
            })
            .map(|obstacle| obstacle.footprint.bounds(obstacle.position))
            .collect();
        let obstacle_cells = obstacle_prefix(grid, &obstacles);
        let mut terrain = vec![0; length];
        for y in 0..grid.rows {
            let mut row = 0;
            for x in 0..grid.columns {
                row += u32::from(
                    class == MovementClass::Ground
                        && map.terrain.as_ref().is_some_and(|terrain| {
                            terrain.flags[y * grid.columns + x] & WALKABLE == 0
                        }),
                );
                terrain[(y + 1) * stride + x + 1] = terrain[y * stride + x + 1] + row;
            }
        }
        Self {
            map,
            grid,
            footprint,
            obstacles,
            terrain,
            obstacle_cells,
        }
    }

    fn clear(&self, start: Position, end: Position) -> bool {
        if !self.map.contains_footprint(start, self.footprint)
            || !self.map.contains_footprint(end, self.footprint)
        {
            return false;
        }
        let bounds = swept_bounds(self.footprint, start, end);
        let cells = self.grid.cells(bounds);
        if self.count(&self.terrain, cells) != 0 {
            return false;
        }
        // This coarse test only proves emptiness. Occupied cells still use
        // exact pixel rectangles, so partial cells and touching edges retain
        // their original collision semantics.
        self.obstacle_cells.is_empty()
            || self.count(&self.obstacle_cells, cells) == 0
            || !self
                .obstacles
                .iter()
                .any(|&obstacle| overlaps(bounds, obstacle))
    }

    fn count(&self, prefix: &[u32], [left, top, right, bottom]: [usize; 4]) -> u32 {
        let stride = self.grid.columns + 1;
        // Group subtractions to avoid unsigned intermediate underflow.
        (prefix[bottom * stride + right] - prefix[top * stride + right])
            - (prefix[bottom * stride + left] - prefix[top * stride + left])
    }
}

/// One search-local broad phase. Rectangle difference updates make building
/// this table O(grid cells + obstacles), even for map-spanning rectangles.
fn obstacle_prefix(grid: Grid, obstacles: &[[i64; 4]]) -> Vec<u32> {
    if obstacles.is_empty() {
        return Vec::new();
    }
    let stride = grid.columns + 1;
    let length = stride * (grid.rows + 1);
    let mut coverage = vec![0_i32; length];
    for &bounds in obstacles {
        let [left, top, right, bottom] = grid.cells(bounds);
        coverage[top * stride + left] += 1;
        coverage[top * stride + right] -= 1;
        coverage[bottom * stride + left] -= 1;
        coverage[bottom * stride + right] += 1;
    }
    let mut prefix = vec![0_u32; length];
    for y in 0..grid.rows {
        let mut row = 0;
        for x in 0..grid.columns {
            let index = y * stride + x;
            if x != 0 {
                coverage[index] += coverage[index - 1];
            }
            if y != 0 {
                coverage[index] += coverage[index - stride];
            }
            if x != 0 && y != 0 {
                coverage[index] -= coverage[index - stride - 1];
            }
            row += u32::from(coverage[index] != 0);
            prefix[(y + 1) * stride + x + 1] = prefix[y * stride + x + 1] + row;
        }
    }
    prefix
}

#[cfg(test)]
mod tests;
