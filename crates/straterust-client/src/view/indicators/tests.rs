use super::*;

#[test]
fn neutral_wildlife_uses_yellow_selection_and_hover_allegiance() {
    use straterust_engine::sim::{Map, Rules, Spawn, UnitType};
    let world = World::new(
        Rules {
            id: "wildlife-indicators".into(),
            units: vec![
                UnitType {
                    id: UnitTypeId(1),
                    ..Default::default()
                },
                UnitType {
                    id: UnitTypeId(2),
                    neutral: true,
                    ..Default::default()
                },
            ],
            ..Default::default()
        },
        Map {
            id: "wildlife-indicators".into(),
            width: 512,
            height: 256,
            players: 2,
            spawns: vec![
                Spawn {
                    owner: PlayerId(0),
                    unit_type: UnitTypeId(1),
                    position: Position { x: 32, y: 32 },
                    ..Default::default()
                },
                Spawn {
                    owner: PlayerId(1),
                    unit_type: UnitTypeId(1),
                    position: Position { x: 64, y: 32 },
                    ..Default::default()
                },
                Spawn {
                    owner: PlayerId(1),
                    unit_type: UnitTypeId(2),
                    position: Position { x: 96, y: 32 },
                    ..Default::default()
                },
            ],
            fog_of_war: false,
            resources: vec![],
            start_locations: vec![],
            terrain: None,
            initial_explored: Default::default(),
            creation: Default::default(),
            ai: vec![],
            mission: None,
        },
        7,
    )
    .unwrap();
    let client = world
        .player_view(PlayerId(0))
        .unwrap()
        .into_world(&world)
        .unwrap();
    for (entity, color, row) in [(0, 0x00ff00, 0), (1, 0xff0000, 2), (2, 0xffff00, 1)] {
        let entity = &client.state().entities[entity];
        assert_eq!(allegiance(&client, entity), row);
        assert_eq!(selection_color(&client, entity), color);
    }
}

#[test]
fn segmented_bars_preserve_gaps_at_fractional_origins_zoom_and_dpi() {
    let colors = std::array::from_fn(|index| if index == 18 { 0 } else { 0x00ff00 });
    for scale in [1.0, 1.25, 1.5, 2.0] {
        for zoom in [1.0, 1.2, 1.5, 2.0] {
            for offset in [0.0, 0.25, 0.5, 0.75] {
                let mut pixels = vec![0xffffff; 200 * 40];
                let origin = [10.0 + offset, 5.0 + offset];
                let mut canvas = Canvas {
                    scene: None,
                    pixels: &mut pixels,
                    width: 200,
                    height: 40,
                    scale,
                };
                draw_bar(&mut canvas, origin, 19, 1.0, &colors, 0, zoom);
                let y = ((origin[1] + 2.0 * zoom) * scale).round() as usize;
                for x in (3..18).step_by(3) {
                    let left = ((origin[0] + f64::from(x) * zoom) * scale).round() as usize;
                    let right = ((origin[0] + f64::from(x + 1) * zoom) * scale).round() as usize;
                    assert!(right > left);
                    assert!(
                        pixels[y * 200 + left..y * 200 + right]
                            .iter()
                            .all(|pixel| *pixel == 0),
                        "solid bar at zoom={zoom} dpi={scale} offset={offset}"
                    );
                    assert_eq!(pixels[y * 200 + right], 0x00ff00);
                }
            }
        }
    }
}
