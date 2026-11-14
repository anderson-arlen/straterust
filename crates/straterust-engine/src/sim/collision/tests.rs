use super::*;
use crate::{content::Package, path};

fn world() -> World {
    Package::load(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/terran-demo"),
    )
    .unwrap()
    .world(7)
    .unwrap()
}

fn compare_movement(world: &World) {
    let except = world.state.entities[0].id;
    let view = world.obstacles(except);
    let legacy = world.collect_navigation_obstacles(except, None, false);
    let mut seed = 42_u64;
    for _ in 0..500 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let start = Position {
            x: ((seed >> 32) % world.map.width as u64) as i32,
            y: (seed % world.map.height as u64) as i32,
        };
        let end = Position {
            x: start.x + 12,
            y: start.y - 8,
        };
        for class in [
            MovementClass::Ground,
            MovementClass::Water,
            MovementClass::Air,
        ] {
            let footprint = Footprint {
                width: 17,
                height: 25,
            };
            assert_eq!(
                view.clear(footprint, class, start, end),
                path::segment_clear(&world.map, footprint, class, start, end, &legacy),
                "{class:?} {start:?}"
            );
        }
    }
}

#[test]
fn indexed_clearance_matches_existing_rules_after_movement_and_state_changes() {
    let mut w = world();
    compare_movement(&w);
    let at = w
        .state
        .entities
        .iter()
        .position(|e| w.unit_type(e.unit_type).unwrap().speed > 0)
        .unwrap();
    let id = w.state.entities[at].id;
    let old = w.state.entities[at].position;
    assert_eq!(
        w.step(&[Command {
            tick: w.tick(),
            player: w.state.entities[at].owner,
            sequence: 1,
            order: Order::Move {
                entity: id,
                target: Position {
                    x: old.x + 80,
                    y: old.y + 48
                }
            }
        }])
        .unwrap()[0]
            .rejection,
        None
    );
    for _ in 0..40 {
        w.step(&[]).unwrap();
    }
    assert_ne!(w.state.entities[at].position, old);
    compare_movement(&w);
    // Query predicates observe current flags without rebuilding memberships.
    w.state.entities[1].garrisoned_in = Some(id);
    compare_movement(&w);
    w.state.entities[1].garrisoned_in = None;
    w.state.entities[1].airborne = true;
    compare_movement(&w);
    w.state.entities[1].airborne = false;
    w.state.entities[1].doodad_enabled = Some(false);
    compare_movement(&w);
}

#[test]
fn reservations_footprints_removal_and_correction_keep_membership_current() {
    let mut w = world();
    let entity = w.state.entities[0].clone();
    let unit = w.unit_type(entity.unit_type).unwrap().clone();
    let bounds = unit.footprint.bounds(entity.position);
    assert!(
        w.collision_candidates(bounds)
            .contains(&ColliderId::Entity(entity.id))
    );
    let spot = Position { x: 400, y: 400 };
    w.state.entities[0].harvest_spot = Some(spot);
    w.collision.changed(entity.id);
    assert!(
        w.collision_candidates(unit.footprint.bounds(spot))
            .contains(&ColliderId::Reservation(entity.id))
    );
    w.assign(0, UnitOrder::Idle, true);
    assert!(
        !w.collision_candidates(unit.footprint.bounds(spot))
            .contains(&ColliderId::Reservation(entity.id))
    );
    let mut corrected = w.clone();
    assert!(
        corrected.collision.data.lock().unwrap().index.is_none(),
        "cloning must not copy the index"
    );
    corrected.state.entities[0].position = spot;
    corrected.reuse_collision_cache(&mut w);
    assert!(w.collision.data.lock().unwrap().index.is_none());
    assert!(
        corrected
            .collision_candidates(unit.footprint.bounds(spot))
            .contains(&ColliderId::Entity(entity.id))
    );
    assert!(
        !corrected
            .collision_candidates(bounds)
            .contains(&ColliderId::Entity(entity.id))
    );
    corrected.state.entities.remove(0);
    corrected.collision.changed(entity.id);
    assert!(
        !corrected
            .collision_candidates(unit.footprint.bounds(spot))
            .contains(&ColliderId::Entity(entity.id))
    );
}

#[test]
fn indexed_search_preserves_the_slice_pathfinder_and_candidate_order() {
    let w = world();
    let id = w.state.entities[0].id;
    let unit = w.unit_type(w.state.entities[0].unit_type).unwrap();
    let start = w.state.entities[0].position;
    let view = w.obstacles(id);
    let legacy = w.collect_navigation_obstacles(id, None, false);
    for target in [
        Position { x: 400, y: 240 },
        Position { x: 100, y: 400 },
        Position { x: 600, y: 300 },
    ] {
        assert_eq!(
            view.find_path(unit.footprint, unit.movement_class, start, target),
            path::find_path(
                &w.map,
                unit.footprint,
                unit.movement_class,
                start,
                target,
                &legacy
            )
        );
        assert_eq!(
            view.find_path_near(unit.footprint, unit.movement_class, start, target),
            path::find_path_near(
                &w.map,
                unit.footprint,
                unit.movement_class,
                start,
                target,
                &legacy
            )
        );
    }
}
