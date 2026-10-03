use super::*;

#[test]
fn worker_rally_skips_disconnected_mineral_patch_and_keeps_intent_if_none_reachable() {
    let mut map = map(vec![spawn(2, 40, 64)]);
    map.resources = vec![node("ore", 160, 64, 100), node("ore", 112, 128, 100)];
    let mut flags = vec![straterust_engine::map::WALKABLE; 128 * 80];
    for y in 0..80 {
        flags[y * 128 + 18] = 0;
    }
    map.terrain = Some(Terrain {
        cell_size: 8,
        columns: 128,
        rows: 80,
        flags,
    });
    let mut world = World::new(economic_rules(), map.clone(), 7).unwrap();
    send(
        &mut world,
        Order::RallyResource {
            entity: EntityId(1),
            resource: ResourceId(1),
        },
    );
    let worker = train(&mut world, 1);
    run(&mut world, 30);
    assert_eq!(
        entity(&world, worker).order,
        UnitOrder::Gather {
            resource: ResourceId(2)
        }
    );
    assert!(world.state().resources[1].amount < 100);
    assert_eq!(world.state().resources[0].amount, 100);
    map.resources.truncate(1);
    let mut blocked = World::new(economic_rules(), map, 7).unwrap();
    send(
        &mut blocked,
        Order::RallyResource {
            entity: EntityId(1),
            resource: ResourceId(1),
        },
    );
    let worker = train(&mut blocked, 1);
    run(&mut blocked, 12);
    assert_eq!(
        entity(&blocked, worker).order,
        UnitOrder::Gather {
            resource: ResourceId(1)
        }
    );
}
