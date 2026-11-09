//! Deterministic ground/air navigation on the native terrain grid.
//!
//! Search uses cell centers, with exact world-coordinate endpoints. Routes
//! narrower than this lattice can be missed; this is not reference-game
//! pathfinding. Swept rectangles deliberately forbid diagonal corner cutting.
//! Cardinal travel costs 10 per world unit, diagonal travel 14. Equal heap
//! priorities prefer smaller remaining distance, then the row-major cell ID.
use std::{cmp::Reverse, collections::BinaryHeap};

mod queries;
pub(crate) use queries::{NavigationData, Pathfinder};

use crate::{
    map::{Footprint, MAX_TERRAIN_DIMENSION, MovementClass, WALKABLE, WATER},
    sim::{Map, Position},
};

/// The package limits permit 4096 entities plus 4096 resource placements.
pub const MAX_OBSTACLES: usize = 20480;

#[cfg(test)]
thread_local! {
    static SEARCH_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
    let data = NavigationData::default();
    Pathfinder::new(map, obstacles, &data).find_path(footprint, class, start, target)
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
    let data = NavigationData::default();
    Pathfinder::new(map, obstacles, &data)
        .find_path_in_range(footprint, class, start, target, range)
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
    let data = NavigationData::default();
    Pathfinder::new(map, obstacles, &data).find_path_to_any(footprint, class, start, targets)
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
    let data = NavigationData::default();
    Pathfinder::new(map, obstacles, &data).find_path_near(footprint, class, start, target)
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

#[derive(Default)]
struct SearchLimits {
    range: Option<u32>,
    cost: Option<u64>,
}

fn search(
    clearance: &Clearance<'_>,
    start: Position,
    target: Position,
    nearest: Option<u64>,
    allow_near: bool,
    exhausted: Option<&mut Option<Vec<bool>>>,
    limits: SearchLimits,
) -> Option<Vec<Position>> {
    #[cfg(test)]
    SEARCH_COUNT.with(|count| count.set(count.get() + 1));
    let range = limits.range;
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
    let mut entry_turns = Vec::new();
    let mut endpoint = target;
    let mut budget_hit = false;
    // Octile cost is at most 11 per unit of Euclidean distance. Subtracting
    // this radius yields an admissible lower bound to the circular goal area.
    let remaining_cost = |p| distance(p, target).saturating_sub(u64::from(range.unwrap_or(0)) * 11);
    let mut best = (distance(start, target), 0, usize::MAX);
    for node in grid.near(start) {
        let position = grid.position(node);
        let Some((cost, turn)) = start_connection(start, position, |a, b| clearance.clear(a, b))
        else {
            continue;
        };
        if let Some(turn) = turn {
            entry_turns.push((node, turn));
        }
        if limits.cost.is_some_and(|limit| cost > limit) {
            budget_hit = true;
            continue;
        }
        costs[node] = cost;
        let remaining = remaining_cost(position);
        queue.push(Reverse((cost + remaining, remaining, node)));
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
            let mut path = trace_path(grid, start, parents[goal], &parents, &entry_turns);
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
                if limits.cost.is_some_and(|limit| cost > limit) {
                    budget_hit = true;
                } else {
                    costs[goal] = cost;
                    parents[goal] = node;
                    endpoint = destination;
                    queue.push(Reverse((cost, 0, goal)));
                }
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
            if limits.cost.is_some_and(|limit| cost > limit) {
                budget_hit = true;
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
    if budget_hit && nearest != Some(best.0) {
        // A partial component cannot prove arrival, but a checked path that
        // gets closer is useful local progress. An empty partial path is never
        // returned as a completed movement order.
        let progress = trace_path(grid, start, best.2, &parents, &entry_turns);
        return (allow_near && !progress.is_empty()).then_some(progress);
    }
    if let Some(exhausted) = exhausted {
        *exhausted = Some(closed);
    }
    allow_near.then(|| trace_path(grid, start, best.2, &parents, &entry_turns))
}

// Off-grid units can touch a corner while still having a cardinal exit.
// Use the same checked bends for search entry and nearest-stop validation.
fn start_connection(
    start: Position,
    position: Position,
    clear: impl Fn(Position, Position) -> bool,
) -> Option<(u64, Option<Position>)> {
    if clear(start, position) {
        return Some((distance(start, position), None));
    }
    let turn = [
        Position {
            x: position.x,
            y: start.y,
        },
        Position {
            x: start.x,
            y: position.y,
        },
    ]
    .into_iter()
    .find(|&turn| clear(start, turn) && clear(turn, position))?;
    Some((distance(start, turn) + distance(turn, position), Some(turn)))
}

fn trace_path(
    grid: Grid,
    start: Position,
    mut node: usize,
    parents: &[usize],
    entry_turns: &[(usize, Position)],
) -> Vec<Position> {
    let mut path = Vec::new();
    while node != usize::MAX {
        let position = grid.position(node);
        if position != start {
            path.push(position);
        }
        if parents[node] == usize::MAX
            && let Some((_, turn)) = entry_turns.iter().find(|(entry, _)| *entry == node)
            && *turn != start
        {
            path.push(*turn);
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
            obstacle.movement_class.collides(class)
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
    data: &'a ClearanceData,
}

#[derive(Debug)]
struct ClearanceData {
    obstacles: Vec<[i64; 4]>,
    // Sorted left edges plus prefix maximum right edges bound exact rectangle
    // checks to nearby columns. Storage stays linear even for map-wide walls.
    obstacle_rights: Vec<i64>,
    terrain: Vec<u32>,
    obstacle_cells: Vec<u32>,
}

impl ClearanceData {
    fn new(map: &Map, grid: Grid, class: MovementClass, obstacles: &[Obstacle]) -> Self {
        let stride = grid.columns + 1;
        let length = stride * (grid.rows + 1);
        let mut terrain = vec![0; length];
        for y in 0..grid.rows {
            let mut row = 0;
            for x in 0..grid.columns {
                row += u32::from(
                    class != MovementClass::Air
                        && map.terrain.as_ref().is_some_and(|terrain| {
                            terrain.flags[y * grid.columns + x]
                                & if class == MovementClass::Water {
                                    WATER
                                } else {
                                    WALKABLE
                                }
                                == 0
                        }),
                );
                terrain[(y + 1) * stride + x + 1] = terrain[y * stride + x + 1] + row;
            }
        }
        let mut obstacles: Vec<_> = obstacles
            .iter()
            .filter(|obstacle| {
                obstacle.movement_class.collides(class)
                    && obstacle.footprint.width != 0
                    && obstacle.footprint.height != 0
            })
            .map(|obstacle| obstacle.footprint.bounds(obstacle.position))
            .filter(|bounds| {
                // Forest cells are already blocked by terrain. Keep exact
                // rectangles only where they can obstruct otherwise clear cells.
                let [left, top, right, bottom] = grid.cells(*bounds);
                let blocked = (terrain[bottom * stride + right] - terrain[top * stride + right])
                    - (terrain[bottom * stride + left] - terrain[top * stride + left]);
                blocked as usize != (right - left) * (bottom - top)
            })
            .collect();
        obstacles.sort_unstable_by_key(|bounds| bounds[0]);
        let mut right = i64::MIN;
        let obstacle_rights = obstacles
            .iter()
            .map(|bounds| {
                right = right.max(bounds[2]);
                right
            })
            .collect();
        let obstacle_cells = obstacle_prefix(grid, &obstacles);
        Self {
            obstacles,
            obstacle_rights,
            terrain,
            obstacle_cells,
        }
    }
}

impl Clearance<'_> {
    fn clear(&self, start: Position, end: Position) -> bool {
        if !self.map.contains_footprint(start, self.footprint)
            || !self.map.contains_footprint(end, self.footprint)
        {
            return false;
        }
        let bounds = swept_bounds(self.footprint, start, end);
        let cells = self.grid.cells(bounds);
        if self.count(&self.data.terrain, cells) != 0 {
            return false;
        }
        // This coarse test only proves emptiness. Occupied cells still use
        // exact pixel rectangles, so partial cells and touching edges retain
        // their original collision semantics.
        self.data.obstacle_cells.is_empty()
            || self.count(&self.data.obstacle_cells, cells) == 0
            || !self
                .horizontal_candidates(bounds)
                .iter()
                .any(|&obstacle| overlaps(bounds, obstacle))
    }

    fn horizontal_candidates(&self, bounds: [i64; 4]) -> &[[i64; 4]] {
        let end = self
            .data
            .obstacles
            .partition_point(|obstacle| obstacle[0] < bounds[2]);
        let start = self.data.obstacle_rights[..end].partition_point(|right| *right <= bounds[0]);
        &self.data.obstacles[start..end]
    }

    fn count(&self, prefix: &[u32], [left, top, right, bottom]: [usize; 4]) -> u32 {
        let stride = self.grid.columns + 1;
        // Group subtractions to avoid unsigned intermediate underflow.
        (prefix[bottom * stride + right] - prefix[top * stride + right])
            - (prefix[bottom * stride + left] - prefix[top * stride + left])
    }
}

/// One collision-data broad phase. Rectangle difference updates make building
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
