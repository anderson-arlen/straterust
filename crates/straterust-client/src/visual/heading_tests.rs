use super::*;
use straterust_engine::{map::*, sim::*};

#[test]
fn subpixel_travel_keeps_a_stable_heading_and_movement_pose_between_pixel_steps() {
    let base = super::tests::world();
    let mut rules = base.rules().clone();
    rules.victory = false;
    rules.units[0].weapon = None;
    rules.units[0].movement_class = MovementClass::Water;
    rules.units[0].motion = Some(Motion {
        eight_directions: true,
        speed: 64,
        acceleration: 0,
        steps: vec![],
    });
    let mut map = base.map().clone();
    map.spawns.truncate(1);
    map.terrain = Some(Terrain {
        cell_size: 32,
        columns: 4,
        rows: 4,
        flags: vec![WALKABLE | WATER; 16],
    });
    let mut world = World::new(rules, map, 42).unwrap();
    let mut client = world
        .player_view(PlayerId(0))
        .unwrap()
        .into_world(&world)
        .unwrap();
    let mut visuals = Visuals::new(&client);
    world
        .step(&[Command {
            tick: world.tick(),
            player: PlayerId(0),
            sequence: 1,
            order: Order::Move {
                entity: EntityId(1),
                target: Position { x: 96, y: 96 },
            },
        }])
        .unwrap();
    for _ in 0..100 {
        client = world
            .player_view(PlayerId(0))
            .unwrap()
            .into_world(&client)
            .unwrap();
        visuals.update(&client);
        let visual = visuals.get(EntityId(1)).unwrap();
        assert_eq!(visual.facing, 12);
        assert!(visual.moving);
        assert_eq!(visual.action, VisualAction::Move);
        world.step(&[]).unwrap();
    }
}
