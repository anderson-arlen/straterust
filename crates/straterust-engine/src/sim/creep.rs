//! Deterministic ground coverage. Source spread bounds are supplied by imported unit rules.
use super::*;
const CELL: u32 = 32;
const LIFETIME: u8 = 8;
const INTERVAL: u64 = 16;

impl World {
    pub(super) fn initialize_creep(&mut self) {
        if !self
            .rules
            .units
            .iter()
            .any(|unit| unit.creep_radius.is_some())
        {
            return;
        }
        let count = ((self.map.width as u32).div_ceil(CELL)
            * (self.map.height as u32).div_ceil(CELL)) as usize;
        self.state.creep = vec![0; count];
        self.state.creep_seen = vec![vec![0; count]; usize::from(self.map.players)];
        self.advance_creep();
    }
    pub fn creep_at(&self, position: Position) -> bool {
        if !self.map.contains(position) {
            return false;
        }
        let index = (position.y as u32 / CELL * (self.map.width as u32).div_ceil(CELL)
            + position.x as u32 / CELL) as usize;
        self.state.creep.get(index).is_some_and(|&life| life > 0)
    }
    pub fn known_creep(&self, player: PlayerId, x: u32, y: u32) -> bool {
        let width = (self.map.width as u32).div_ceil(CELL);
        x < width
            && y < (self.map.height as u32).div_ceil(CELL)
            && self
                .state
                .creep_seen
                .get(usize::from(player.0))
                .and_then(|memory| memory.get((y * width + x) as usize))
                .is_some_and(|&value| value > 0)
    }
    pub(super) fn creep_placement_allowed(&self, unit: &UnitType, position: Position) -> bool {
        if self.state.creep.is_empty() {
            return !unit.requires_creep;
        }
        let [left, top, right, bottom] = unit.placement.bounds(position);
        if left < 0
            || top < 0
            || right > i64::from(self.map.width)
            || bottom > i64::from(self.map.height)
        {
            return false;
        }
        for y in top / i64::from(CELL)..=(bottom - 1) / i64::from(CELL) {
            for x in left / i64::from(CELL)..=(right - 1) / i64::from(CELL) {
                let has = self.creep_at(Position {
                    x: x as i32 * 32 + 16,
                    y: y as i32 * 32 + 16,
                });
                if (unit.requires_creep && !has) || (unit.creep_radius.is_none() && has) {
                    return false;
                }
            }
        }
        true
    }
    pub(super) fn advance_creep(&mut self) {
        if self.state.creep.is_empty() || !self.state.tick.0.is_multiple_of(INTERVAL) {
            return;
        }
        for life in &mut self.state.creep {
            *life = life.saturating_sub(1);
        }
        let width = (self.map.width as u32).div_ceil(CELL);
        let height = (self.map.height as u32).div_ceil(CELL);
        for entity in &self.state.entities {
            let unit = self
                .unit_type(entity.unit_type)
                .expect("validated provider");
            let Some(mut radius) = unit.creep_radius else {
                continue;
            };
            if entity.hp == 0 || entity.airborne {
                continue;
            }
            if entity.construction.is_some() {
                radius = [0, 0];
            }
            let foundation = unit.placement.bounds(entity.position);
            let extent_x = i32::from(radius[0]).max(i32::from(unit.placement.width) / 2);
            let extent_y = i32::from(radius[1]).max(i32::from(unit.placement.height) / 2);
            let start_x = ((entity.position.x - extent_x).max(0) as u32 / CELL).min(width);
            let end_x = ((entity.position.x + extent_x).max(0) as u32 / CELL + 1).min(width);
            let start_y = ((entity.position.y - extent_y).max(0) as u32 / CELL).min(height);
            let end_y = ((entity.position.y + extent_y).max(0) as u32 / CELL + 1).min(height);
            for y in start_y..end_y {
                for x in start_x..end_x {
                    let position = Position {
                        x: x as i32 * 32 + 16,
                        y: y as i32 * 32 + 16,
                    };
                    let dx = i64::from(position.x - entity.position.x);
                    let dy = i64::from(position.y - entity.position.y);
                    let inside = i64::from(position.x) >= foundation[0]
                        && i64::from(position.x) < foundation[2]
                        && i64::from(position.y) >= foundation[1]
                        && i64::from(position.y) < foundation[3];
                    let spread = radius[0] > 0
                        && radius[1] > 0
                        && dx * dx * i64::from(radius[1]).pow(2)
                            + dy * dy * i64::from(radius[0]).pow(2)
                            <= i64::from(radius[0]).pow(2) * i64::from(radius[1]).pow(2);
                    if (inside || spread)
                        && self.map.can_build(
                            position,
                            Footprint {
                                width: 32,
                                height: 32,
                            },
                        )
                    {
                        self.state.creep[(y * width + x) as usize] = LIFETIME;
                    }
                }
            }
        }
    }
    pub(super) fn remember_creep(&mut self) {
        if self.state.creep.is_empty() {
            return;
        }
        for (player, memory) in self.state.creep_seen.iter_mut().enumerate() {
            for (index, life) in self.state.creep.iter().enumerate() {
                if !self.map.fog_of_war || self.state.terrain_fog[player][index] == 2 {
                    memory[index] = u8::from(*life > 0);
                }
            }
        }
    }
}
