use super::*;

fn mining_world() -> World {
    let package = crate::content::Package::load(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/terran-demo"),
    )
    .unwrap();
    let source = package.world(42).unwrap();
    let mut rules = source.rules().clone();
    let worker = rules
        .units
        .iter_mut()
        .find(|u| u.id == UnitTypeId(2))
        .unwrap();
    worker.harvest_profiles = vec![HarvestProfile {
        kind: "minerals".into(),
        capacity: 5,
        inside: true,
        amount: 8,
        ticks: 4,
        entry_range: 1,
        depot_ticks: 3,
        depot_inside: true,
    }];
    let mut map = source.map().clone();
    map.resources.truncate(1);
    map.resources[0].position = Position { x: 480, y: 192 };
    map.spawns[0].position = Position { x: 320, y: 192 };
    map.spawns[1].position = Position { x: 400, y: 192 };
    World::new(rules, map, 42).unwrap()
}

#[test]
fn near_mine_worker_emerges_and_walks_without_a_server_update() {
    let mut server = mining_world();
    let worker = server
        .state
        .entities
        .iter()
        .find(|e| e.unit_type == UnitTypeId(2))
        .unwrap()
        .id;
    let resource = server.state.resources[0].id;
    server
        .step(&[Command {
            tick: server.tick(),
            player: PlayerId(0),
            sequence: 1,
            order: Order::Gather {
                entity: worker,
                resource,
            },
        }])
        .unwrap();
    // Start the prediction while the worker is hidden inside the mine.
    for _ in 0..100 {
        if server
            .state
            .entities
            .iter()
            .find(|e| e.id == worker)
            .unwrap()
            .gathering_inside
        {
            break;
        }
        server.step(&[]).unwrap();
    }
    assert!(
        server
            .state
            .entities
            .iter()
            .find(|e| e.id == worker)
            .unwrap()
            .gathering_inside
    );
    let mut predicted = server
        .player_view(PlayerId(0))
        .unwrap()
        .into_world(&server)
        .unwrap();
    let mut seen_outside = false;
    let mut delivered = false;
    let mut visited_depot = false;
    for _ in 0..80 {
        server.step(&[]).unwrap();
        predicted.predict_step().unwrap();
        let expected = server
            .state
            .entities
            .iter()
            .find(|e| e.id == worker)
            .unwrap();
        let actual = predicted
            .state
            .entities
            .iter()
            .find(|e| e.id == worker)
            .unwrap();
        assert_eq!(actual.position, expected.position);
        assert_eq!(actual.gathering_inside, expected.gathering_inside);
        assert_eq!(actual.cargo, expected.cargo);
        seen_outside |= !actual.gathering_inside && actual.cargo.is_some();
        visited_depot |= actual.gathering_inside && actual.dropoff_target.is_some();
        delivered |= visited_depot && !actual.gathering_inside && actual.cargo.is_none();
    }
    assert!(seen_outside && visited_depot && delivered);
    assert_eq!(predicted.state.resources, server.state.resources);
    assert_eq!(predicted.state.players[0], server.state.players[0]);
    assert!(predicted.state.players[1].resources.is_empty());
    assert!(server.predict_step().is_err());
}

#[test]
fn ordinary_production_timers_advance_between_authoritative_frames() {
    let mut server = mining_world();
    let hall = server
        .state
        .entities
        .iter_mut()
        .find(|e| e.unit_type == UnitTypeId(3))
        .unwrap();
    let id = hall.id;
    hall.production.push_back(ProductionJob {
        producer_type: None,
        unit_type: UnitTypeId(2),
        remaining: 100,
        total: 100,
        started: true,
    });
    let mut predicted = server
        .player_view(PlayerId(0))
        .unwrap()
        .into_world(&server)
        .unwrap();
    for remaining in (90..100).rev() {
        server.step(&[]).unwrap();
        predicted.predict_step().unwrap();
        let actual = predicted
            .state
            .entities
            .iter()
            .find(|e| e.id == id)
            .unwrap();
        let expected = server.state.entities.iter().find(|e| e.id == id).unwrap();
        assert_eq!(actual.production.front().unwrap().remaining, remaining);
        assert_eq!(actual.production, expected.production);
    }
    // Prediction may reach completion but cannot invent server entity identities.
    let count = predicted.state.entities.len();
    for _ in 0..120 {
        predicted.predict_step().unwrap();
    }
    assert_eq!(predicted.state.entities.len(), count);
    assert_eq!(
        predicted
            .state
            .entities
            .iter()
            .find(|e| e.id == id)
            .unwrap()
            .production
            .front()
            .unwrap()
            .remaining,
        0
    );
}

#[test]
fn fog_keeps_last_confirmed_resource_state_without_disclosing_hidden_changes() {
    let original = mining_world();
    let mut map = original.map().clone();
    map.fog_of_war = true;
    map.resources.push(ResourceSpawn {
        kind: "minerals".into(),
        position: Position { x: 800, y: 192 },
        footprint: Footprint {
            width: 32,
            height: 32,
        },
        amount: 400,
        requires_extractor: false,
        terrain_corners: None,
    });
    let mut rules = original.rules().clone();
    for unit in &mut rules.units {
        unit.vision_range = if unit.worker.is_some() { 96 } else { 0 };
    }
    let mut server = World::new(rules, map, 42).unwrap();
    let resource = server.state.resources[0].id;
    let worker = server
        .state
        .entities
        .iter()
        .find(|e| e.unit_type == UnitTypeId(2))
        .unwrap()
        .id;
    let mut confirmed = server
        .player_view(PlayerId(0))
        .unwrap()
        .into_world(&server)
        .unwrap();
    assert_eq!(confirmed.state.resources.len(), 1);
    // Depletion while unseen must not update the remembered forest.
    server
        .state
        .entities
        .iter_mut()
        .find(|e| e.id == worker)
        .unwrap()
        .position
        .x = 120;
    server.state.resources[0].amount = 0;
    server.step(&[]).unwrap();
    assert_eq!(
        server.visibility(PlayerId(0), Position { x: 480, y: 192 }),
        Visibility::Explored
    );
    confirmed = server
        .player_view(PlayerId(0))
        .unwrap()
        .into_world(&confirmed)
        .unwrap();
    assert_eq!(confirmed.state.resources[0].amount, 1500);
    // Once depletion is seen, keep that observation when sight leaves again.
    server
        .state
        .entities
        .iter_mut()
        .find(|e| e.id == worker)
        .unwrap()
        .position
        .x = 400;
    server.step(&[]).unwrap();
    confirmed = server
        .player_view(PlayerId(0))
        .unwrap()
        .into_world(&confirmed)
        .unwrap();
    assert_eq!(confirmed.state.resources[0].amount, 0);
    server
        .state
        .entities
        .iter_mut()
        .find(|e| e.id == worker)
        .unwrap()
        .position
        .x = 120;
    for _ in 0..3 {
        server.step(&[]).unwrap();
        confirmed = server
            .player_view(PlayerId(0))
            .unwrap()
            .into_world(&confirmed)
            .unwrap();
        assert_eq!(confirmed.state.resources.len(), 1);
        assert_eq!(confirmed.state.resources[0].id, resource);
        assert_eq!(confirmed.state.resources[0].amount, 0);
    }
}

#[test]
fn prediction_keeps_build_validation_available_and_defers_only_foundation_allocation() {
    let original = mining_world();
    let mut rules = original.rules().clone();
    rules.starting_resources[0].amount = 2000;
    let mut server = World::new(rules, original.map().clone(), 42).unwrap();
    let worker = server
        .state
        .entities
        .iter()
        .find(|e| e.unit_type == UnitTypeId(2))
        .unwrap()
        .id;
    let target = Position { x: 640, y: 400 };
    let building_type = UnitTypeId(4);
    let mut predicted = server
        .player_view(PlayerId(0))
        .unwrap()
        .into_world(&server)
        .unwrap();
    let next_id = predicted.state.next_entity_id;
    for _ in 0..4 {
        predicted.predict_step().unwrap();
        assert_eq!(predicted.state.next_entity_id, next_id);
        assert_eq!(
            predicted.build_rejection(PlayerId(0), worker, building_type, target),
            None
        );
    }
    let outcome = server
        .step(&[Command {
            tick: server.tick(),
            player: PlayerId(0),
            sequence: 1,
            order: Order::Build {
                entity: worker,
                unit_type: building_type,
                position: target,
            },
        }])
        .unwrap();
    assert_eq!(outcome[0].rejection, None);
    let mut predicted = server
        .player_view(PlayerId(0))
        .unwrap()
        .into_world(&server)
        .unwrap();
    let count = predicted.state.entities.len();
    for _ in 0..100 {
        predicted.predict_step().unwrap();
    }
    assert_eq!(predicted.state.entities.len(), count);
    assert!(matches!(
        predicted
            .state
            .entities
            .iter()
            .find(|e| e.id == worker)
            .unwrap()
            .order,
        UnitOrder::PlaceBuilding { .. }
    ));
    for _ in 0..200 {
        server.step(&[]).unwrap();
    }
    assert!(
        server
            .state
            .entities
            .iter()
            .any(|e| e.unit_type == building_type)
    );
}
