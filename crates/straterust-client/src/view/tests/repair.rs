use super::*;
use straterust_engine::{
    assets::UiImage,
    multiplayer::{GameType, MatchRules},
    sim::{Command, Footprint, Order, RepairRules, ResourceSpawn, Spawn},
};

#[test]
fn selected_workers_and_allied_targets_show_assignments_without_foreign_orders() {
    let package = Package::load(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/terran-demo"),
    )
    .unwrap();
    let original = package.world(42).unwrap();
    let mut rules = original.rules().clone();
    rules.victory = false;
    rules.repair = Some(RepairRules {
        rate_numerator: 1,
        rate_denominator: 1,
        cost_divisor: 4,
        range: 5,
    });
    for unit in &mut rules.units {
        unit.manual_target_only = true;
        if unit.id == UnitTypeId(2) {
            unit.repairs = vec![UnitTypeId(3)];
        }
        if unit.id == UnitTypeId(3) {
            unit.footprint.width = 16;
            unit.footprint.height = 16;
        }
    }
    let mut map = original.map().clone();
    map.fog_of_war = false;
    map.ai.clear();
    map.mission = None;
    map.players = 3;
    map.multiplayer = Some(MatchRules {
        participants: vec![PlayerId(0), PlayerId(1), PlayerId(2)],
        teams: vec![vec![PlayerId(0), PlayerId(1)], vec![PlayerId(2)]],
        mode: GameType::Teams,
        parameter: 1,
        resources: Default::default(),
        bases: vec![],
        cheats: false,
        ai_difficulty: None,
        flags: vec![],
        defeat_without_structures: false,
    });
    map.resources.clear();
    map.resources.push(ResourceSpawn {
        kind: "minerals".into(),
        position: Position { x: 120, y: 350 },
        amount: 600,
        footprint: Footprint {
            width: 16,
            height: 16,
        },
        terrain_corners: None,
        requires_extractor: false,
    });
    map.spawns = [
        (0, 2, 200, 200),
        (0, 3, 224, 200),
        (0, 2, 200, 300),
        (1, 3, 224, 300),
        (1, 2, 480, 200),
        (1, 3, 504, 200),
        (2, 3, 900, 200),
    ]
    .into_iter()
    .map(|(owner, unit, x, y)| Spawn {
        owner: PlayerId(owner),
        unit_type: UnitTypeId(unit),
        position: Position { x, y },
        ..Default::default()
    })
    .collect();
    let mut world = World::new(rules, map, 42).unwrap();
    let orders = [
        (
            0,
            1,
            Order::Repair {
                entity: EntityId(1),
                target: EntityId(2),
            },
        ),
        (
            0,
            2,
            Order::Queue {
                entity: EntityId(1),
                order: UnitOrder::Repair {
                    target: EntityId(4),
                },
            },
        ),
        (
            0,
            3,
            Order::Gather {
                entity: EntityId(3),
                resource: ResourceId(1),
            },
        ),
        (
            0,
            4,
            Order::Queue {
                entity: EntityId(3),
                order: UnitOrder::Repair {
                    target: EntityId(4),
                },
            },
        ),
        (
            1,
            1,
            Order::Repair {
                entity: EntityId(5),
                target: EntityId(6),
            },
        ),
    ]
    .into_iter()
    .map(|(player, sequence, order)| Command {
        player: PlayerId(player),
        tick: world.tick(),
        sequence,
        order,
    })
    .collect::<Vec<_>>();
    assert!(
        world
            .step(&orders)
            .unwrap()
            .iter()
            .all(|o| o.rejection.is_none())
    );
    // Exercise exactly the private/public bodies used by a local or remote
    // player. Allied workers' assignments are not disclosed to this player.
    let world = world
        .player_view(PlayerId(0))
        .unwrap()
        .into_world(&world)
        .unwrap();
    assert_eq!(world.state().entities[4].order, UnitOrder::Idle);
    assert_eq!(world.repair_rejection(EntityId(1), EntityId(4)), None);
    let hash = world.state_hash();
    let mut assets = resources::assets();
    assets.ui.push(UiImage {
        key: "command.repair".into(),
        image: Image {
            width: 2,
            height: 2,
            rgba: [255, 0, 255, 255].repeat(4),
        },
    });
    let visuals = Visuals::new(&world);
    let presentation = Presentation::default();
    for (selection, expected) in [
        (vec![1], vec![2, 4]),
        (vec![1, 3], vec![2, 4]),
        (vec![4], vec![1, 3]),
        (vec![5], vec![]),
        (vec![], vec![]),
    ] {
        let selected = selection.into_iter().map(EntityId).collect();
        let mut view = resources::make_view(&world, &assets, &visuals, &presentation, &selected);
        view.camera.x = 320.0;
        view.camera.y = 250.0;
        let expected: BTreeSet<_> = expected.into_iter().map(EntityId).collect();
        assert_eq!(view.repair_markers(), expected);
        for dpi in [1.0, 1.5] {
            let (width, height) = ((640.0 * dpi) as usize, (480.0 * dpi) as usize);
            let mut pixels = vec![0; width * height];
            let mut canvas = Canvas {
                scene: None,
                pixels: &mut pixels,
                width,
                height,
                scale: dpi,
            };
            view.draw_repair_orders(&mut canvas, [640.0, 480.0]);
            assert_eq!(
                pixels.iter().filter(|p| **p == 0xff00ff).count(),
                expected.len() * (16.0 * dpi) as usize * (16.0 * dpi) as usize
            );
            for id in &expected {
                let entity = world.state().entities.iter().find(|e| e.id == *id).unwrap();
                let p = view.camera.world_to_screen(
                    f64::from(entity.position.x),
                    f64::from(entity.position.y),
                    [640.0, 480.0],
                );
                let top = unit_half_size(&world, entity.unit_type, &presentation)[1];
                let x = (p[0] * dpi) as usize;
                let y = ((p[1] - top - 13.0) * dpi) as usize;
                assert_eq!(pixels[y * width + x], 0xff00ff, "repair badge above {id:?}");
            }
        }
    }
    assert_eq!(world.state_hash(), hash, "selection markers are cosmetic");
}
