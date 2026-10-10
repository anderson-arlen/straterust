use straterust_engine::{content::Package, sim::*};

#[test]
fn resource_clearance_rejects_close_builds_and_landings_at_rectangle_boundaries() {
    let package = Package::load(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/terran-demo"),
    )
    .unwrap();
    let original = package.world(0).unwrap();
    let mut rules = original.rules().clone();
    rules.victory = false;
    let depot = rules
        .units
        .iter_mut()
        .find(|unit| unit.id == UnitTypeId(3))
        .unwrap();
    depot.resource_clearance = 96;
    depot.flight = Some(Flight {
        speed: 8,
        lift_ticks: 1,
        land_ticks: 1,
    });
    rules.starting_resources[0].amount = 5000;
    for (width, height, gas) in [(64, 32, false), (128, 64, true)] {
        let mut map = original.map().clone();
        map.width = 1024;
        map.height = 1024;
        map.terrain = None;
        map.fog_of_war = false;
        map.start_locations.clear();
        map.spawns = vec![
            Spawn {
                unit_type: UnitTypeId(2),
                position: Position { x: 32, y: 32 },
                ..Default::default()
            },
            Spawn {
                unit_type: UnitTypeId(3),
                position: Position { x: 128, y: 128 },
                ..Default::default()
            },
        ];
        map.resources = vec![ResourceSpawn {
            terrain_corners: None,
            kind: if gas { "gas" } else { "minerals" }.into(),
            position: Position { x: 640, y: 512 },
            footprint: Footprint { width, height },
            amount: 1000,
            requires_extractor: gas,
        }];
        let mut world = World::new(rules.clone(), map.clone(), 0).unwrap();
        let size = world.unit_type(UnitTypeId(3)).unwrap().placement;
        let allowed_x = 640 - i32::from(width) / 2 - i32::from(size.width) / 2 - 96;
        let allowed_y = 512 - i32::from(height) / 2 - i32::from(size.height) / 2 - 96;
        for position in [
            Position {
                x: allowed_x + 1,
                y: 512,
            },
            Position {
                x: 640,
                y: allowed_y + 1,
            },
        ] {
            assert_eq!(
                world.build_rejection(PlayerId(0), EntityId(1), UnitTypeId(3), position),
                Some(Rejection::InvalidPlacement)
            );
        }
        for position in [
            Position {
                x: allowed_x,
                y: 512,
            },
            Position {
                x: 640,
                y: allowed_y,
            },
        ] {
            assert_eq!(
                world.build_rejection(PlayerId(0), EntityId(1), UnitTypeId(3), position),
                None
            );
        }
        let outcome = world
            .step(&[Command {
                tick: world.tick(),
                player: PlayerId(0),
                sequence: 1,
                order: Order::Lift {
                    entity: EntityId(2),
                },
            }])
            .unwrap();
        assert_eq!(outcome[0].rejection, None);
        world.step(&[]).unwrap();
        assert_eq!(
            world.land_rejection(
                EntityId(2),
                Position {
                    x: allowed_x + 1,
                    y: 512
                }
            ),
            Some(Rejection::InvalidPlacement)
        );
        assert_eq!(
            world.land_rejection(
                EntityId(2),
                Position {
                    x: allowed_x,
                    y: 512
                }
            ),
            None
        );
        if !gas {
            map.resources.clear();
            let cleared = World::new(rules.clone(), map, 0).unwrap();
            assert_eq!(
                cleared.build_rejection(
                    PlayerId(0),
                    EntityId(1),
                    UnitTypeId(3),
                    Position { x: 640, y: 512 }
                ),
                None
            );
        }
    }
}
