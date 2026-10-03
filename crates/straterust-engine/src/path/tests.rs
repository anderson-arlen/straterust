use super::*;
use crate::map::Terrain;

fn map(columns: u32, rows: u32) -> Map {
    Map {
        initial_explored: Default::default(),
        creation: Default::default(),
        ai: Vec::new(),
        mission: None,
        fog_of_war: false,
        id: "navigation-test".into(),
        width: (columns * 8) as i32,
        height: (rows * 8) as i32,
        players: 1,
        spawns: Vec::new(),
        start_locations: Vec::new(),
        resources: Vec::new(),
        terrain: Some(Terrain {
            cell_size: 8,
            columns,
            rows,
            flags: vec![WALKABLE; (columns * rows) as usize],
        }),
    }
}

fn block(map: &mut Map, x: usize, y: usize) {
    let terrain = map.terrain.as_mut().unwrap();
    terrain.flags[y * terrain.columns as usize + x] = 0;
}

fn assert_clear(
    map: &Map,
    footprint: Footprint,
    start: Position,
    path: &[Position],
    obstacles: &[Obstacle],
) {
    let mut previous = start;
    for &point in path {
        assert!(
            segment_clear(
                map,
                footprint,
                MovementClass::Ground,
                previous,
                point,
                obstacles
            ),
            "blocked segment {previous:?} -> {point:?}"
        );
        previous = point;
    }
}

mod clearance;

mod routing;

mod approach;
mod approach_batch;
