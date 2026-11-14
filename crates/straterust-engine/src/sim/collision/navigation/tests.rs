use super::*;
use crate::content::Package;

fn world() -> World {
    let original = Package::load(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/terran-demo"),
    )
    .unwrap()
    .world(7)
    .unwrap();
    let mut rules = original.rules().clone();
    rules.victory = false;
    for unit in rules.units.iter_mut().filter(|u| u.structure) {
        unit.flight = Some(Flight {
            speed: 8,
            lift_ticks: 1,
            land_ticks: 1,
        });
        unit.cloak = Some(Cloak {
            blocks_movement: false,
            ..Default::default()
        });
    }
    rules.units[4].extracts = Some(rts::Extraction {
        resource: "oil".into(),
        harvest_ticks: 1,
        depleted_amount: 0,
    });
    let mut map = original.map().clone();
    map.ai.clear();
    map.fog_of_war = false;
    map.resources.push(ResourceSpawn {
        kind: "oil".into(),
        position: Position { x: 640, y: 640 },
        footprint: Footprint {
            width: 64,
            height: 64,
        },
        amount: 1000,
        requires_extractor: true,
        terrain_corners: None,
    });
    World::new(rules, map, 7).unwrap()
}

fn refresh(world: &mut World) {
    world.refresh_navigation_geometry();
    assert_eq!(
        world.navigation_geometry,
        world.static_navigation_obstacles()
    );
    assert!(world.collision.navigation_current(world));
}

#[test]
fn moving_and_harvesting_do_not_rebuild_or_rehash_unchanged_geometry() {
    let mut w = world();
    refresh(&mut w);
    let geometry = w.navigation_geometry.as_ptr();
    let fingerprint = w.navigation_geometry_hash;
    let prepared = Arc::clone(&w.navigation_data);
    let old_position = w.state.entities[1].position;
    let old_amount = w.state.resources[0].amount;
    let command = Command {
        tick: w.tick(),
        player: PlayerId(0),
        sequence: 1,
        order: Order::Gather {
            entity: EntityId(2),
            resource: ResourceId(1),
        },
    };
    assert!(w.step(&[command]).unwrap()[0].rejection.is_none());
    for _ in 0..200 {
        w.step(&[]).unwrap();
        assert_eq!(w.navigation_geometry.as_ptr(), geometry);
        assert_eq!(w.navigation_geometry_hash, fingerprint);
        assert!(Arc::ptr_eq(&w.navigation_data, &prepared));
    }
    assert_ne!(w.state.entities[1].position, old_position);
    assert!(w.state.resources[0].amount < old_amount);
}

#[test]
fn static_flags_depletion_and_extractor_changes_match_full_collection() {
    let mut w = world();
    refresh(&mut w);
    let first = w.navigation_geometry_hash;
    assert_eq!(w.toggle_cloak(0, true), None);
    refresh(&mut w);
    assert_ne!(w.navigation_geometry_hash, first);
    w.reveal(0);
    refresh(&mut w);
    assert_eq!(w.navigation_geometry_hash, first);
    assert_eq!(w.start_lift(0), None);
    refresh(&mut w);
    assert_ne!(w.navigation_geometry_hash, first);
    w.step(&[]).unwrap();
    w.assign(
        0,
        UnitOrder::Land {
            target: Position { x: 224, y: 256 },
        },
        true,
    );
    w.advance_land(0, Position { x: 224, y: 256 });
    w.advance_flight(0);
    assert!(!w.state.entities[0].airborne);
    refresh(&mut w);
    assert_eq!(w.navigation_geometry_hash, first);

    // A footprint-changing morph also covers an oil field. Destroying it
    // exposes the still-present field, including when its amount is zero.
    w.state.entities[0].unit_type = UnitTypeId(5);
    w.state.entities[0].position = Position { x: 640, y: 640 };
    w.collision.changed(EntityId(1));
    refresh(&mut w);
    assert!(
        !w.collision
            .data
            .lock()
            .unwrap()
            .navigation
            .geometry
            .contains_key(&ColliderId::Resource(ResourceId(4)))
    );
    w.state.resources[3].amount = 0;
    w.collision.resource_changed(ResourceId(4));
    let covered = w.navigation_geometry_hash;
    refresh(&mut w);
    assert_eq!(w.navigation_geometry_hash, covered);
    w.state.entities.remove(0);
    w.collision.changed(EntityId(1));
    refresh(&mut w);
    assert!(
        w.collision
            .data
            .lock()
            .unwrap()
            .navigation
            .geometry
            .contains_key(&ColliderId::Resource(ResourceId(4)))
    );
    w.state.resources[0].amount = 0;
    w.collision.resource_changed(ResourceId(1));
    refresh(&mut w);
    assert!(
        !w.collision
            .data
            .lock()
            .unwrap()
            .navigation
            .geometry
            .contains_key(&ColliderId::Resource(ResourceId(1)))
    );
}

#[test]
fn reverted_changes_and_corrected_worlds_preserve_their_own_baselines() {
    let mut w = world();
    refresh(&mut w);
    w.state.entities[0].doodad_enabled = Some(false);
    w.collision.changed(EntityId(1));
    assert!(!w.collision.navigation_current(&w));
    w.state.entities[0].doodad_enabled = None;
    w.collision.changed(EntityId(1));
    assert!(w.collision.navigation_current(&w));
    let geometry = w.navigation_geometry.as_ptr();
    refresh(&mut w);
    assert_eq!(w.navigation_geometry.as_ptr(), geometry);

    let mut corrected = w.player_view(PlayerId(0)).unwrap().into_world(&w).unwrap();
    let corrected_geometry = corrected.navigation_geometry.clone();
    w.state.entities[0].doodad_enabled = Some(false);
    w.collision.changed(EntityId(1));
    refresh(&mut w);
    corrected.reuse_collision_cache(&mut w);
    refresh(&mut corrected);
    assert_eq!(corrected.navigation_geometry, corrected_geometry);
    let mut cloned = corrected.clone();
    cloned.state.entities[0].doodad_enabled = Some(false);
    cloned.collision.changed(EntityId(1));
    refresh(&mut cloned);
    assert_ne!(cloned.navigation_geometry, corrected_geometry);
    refresh(&mut corrected);
    assert_eq!(corrected.navigation_geometry, corrected_geometry);
}
