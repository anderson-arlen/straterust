//! Interior construction and repair assistance use ordinary saved worker orders.
use straterust_engine::{
    session::{SavedGame, ServerSession},
    sim::*,
};

fn world(inside: bool, assistance: bool) -> World {
    World::new(
        Rules {
            id: "interior-construction".into(),
            starting_resources: vec![ResourceAmount {
                kind: "gold".into(),
                amount: 500,
            }],
            repair: Some(RepairRules {
                rate_numerator: 4,
                rate_denominator: 1,
                cost_divisor: 2,
                range: 8,
            }),
            units: vec![
                UnitType {
                    id: UnitTypeId(1),
                    speed: 4,
                    max_hp: 40,
                    vision_range: 64,
                    footprint: Footprint {
                        width: 8,
                        height: 8,
                    },
                    builds: vec![UnitTypeId(2)],
                    repairs: vec![UnitTypeId(2)],
                    worker: Some(WorkerStats {
                        capacity: 10,
                        harvest_amount: 10,
                        harvest_ticks: 10,
                        build_rate: 1,
                        resource_kinds: vec!["gold".into()],
                        idle_resource_radius: 0,
                    }),
                    ..Default::default()
                },
                UnitType {
                    id: UnitTypeId(2),
                    speed: 0,
                    structure: true,
                    max_hp: 200,
                    build_ticks: 80,
                    builder_inside: inside,
                    repair_construction: assistance,
                    footprint: Footprint {
                        width: 24,
                        height: 24,
                    },
                    placement: Footprint {
                        width: 24,
                        height: 24,
                    },
                    cost: vec![ResourceAmount {
                        kind: "gold".into(),
                        amount: 100,
                    }],
                    ..Default::default()
                },
            ],
            ..Default::default()
        },
        Map {
            id: "interior-construction".into(),
            terrain: None,
            ai: vec![],
            mission: None,
            creation: Default::default(),
            initial_explored: Default::default(),
            start_locations: vec![],
            resources: vec![],
            fog_of_war: false,
            width: 256,
            height: 256,
            players: 2,
            spawns: vec![
                Spawn {
                    unit_type: UnitTypeId(1),
                    position: Position { x: 32, y: 64 },
                    ..Default::default()
                },
                Spawn {
                    unit_type: UnitTypeId(1),
                    position: Position { x: 96, y: 64 },
                    ..Default::default()
                },
            ],
        },
        7,
    )
    .unwrap()
}
fn command(w: &mut World, order: Order) -> Option<Rejection> {
    w.step(&[Command {
        tick: w.tick(),
        player: PlayerId(0),
        sequence: w.tick().0 + 1,
        order,
    }])
    .unwrap()[0]
        .rejection
        .clone()
}
fn begin(w: &mut World) {
    assert_eq!(
        command(
            w,
            Order::Build {
                entity: EntityId(1),
                unit_type: UnitTypeId(2),
                position: Position { x: 64, y: 64 }
            }
        ),
        None
    );
    for _ in 0..20 {
        if w.state().entities.iter().any(|e| {
            e.construction
                .as_ref()
                .is_some_and(|c| c.work_position.is_some())
        }) {
            return;
        }
        w.step(&[]).unwrap();
    }
    panic!("builder failed to reach foundation");
}
fn remaining(w: &World) -> u32 {
    w.state()
        .entities
        .iter()
        .find(|e| e.id == EntityId(3))
        .unwrap()
        .construction
        .as_ref()
        .map_or(0, |c| c.remaining)
}
#[test]
fn builder_hides_only_after_arrival_and_helpers_add_work_without_taking_over_the_job() {
    let mut w = world(true, true);
    assert!(w.entity_visible(PlayerId(0), EntityId(1)));
    begin(&mut w);
    assert!(!w.entity_visible(PlayerId(0), EntityId(1)));
    assert!(!w.entity_visible(PlayerId(1), EntityId(1)));
    let packet = w.player_view(PlayerId(1)).unwrap();
    assert!(!packet.entities.iter().any(|e| match e {
        ViewedEntity::Owned(e) => e.id == EntityId(1),
        ViewedEntity::Visible(e) => e.id == EntityId(1),
    }));
    let client = w.player_view(PlayerId(0)).unwrap().into_world(&w).unwrap();
    assert!(
        !client.entity_visible(PlayerId(0), EntityId(1)),
        "client derives the same hidden body"
    );
    let mut solo = w.clone();
    assert_eq!(
        command(
            &mut w,
            Order::Repair {
                entity: EntityId(2),
                target: EntityId(3)
            }
        ),
        None
    );
    solo.step(&[]).unwrap();
    for _ in 0..20 {
        solo.step(&[]).unwrap();
        w.step(&[]).unwrap();
    }
    assert!(
        remaining(&solo) >= remaining(&w) + 12,
        "helper must add work after arriving"
    );
    assert_eq!(w.resource_balance(PlayerId(0), "gold"), 400);
    assert_eq!(
        w.state().entities[2].construction.as_ref().unwrap().worker,
        Some(EntityId(1))
    );
    w = w.restore_snapshot(w.save_snapshot().unwrap()).unwrap();
    assert!(!w.entity_visible(PlayerId(0), EntityId(1)));
    for _ in 0..100 {
        w.step(&[]).unwrap();
    }
    assert_eq!(remaining(&w), 0);
    for id in [EntityId(1), EntityId(2)] {
        assert!(w.entity_visible(PlayerId(0), id));
        let e = w.state().entities.iter().find(|e| e.id == id).unwrap();
        assert_eq!(e.order, UnitOrder::Idle);
        assert!(w.can_place(
            e.position,
            w.unit_type(e.unit_type).unwrap().footprint,
            MovementClass::Ground,
            Some(id)
        ));
    }
}
#[test]
fn construction_options_apply_to_older_saves_and_builder_can_stop_resume_or_cancel() {
    let mut old = world(false, false);
    begin(&mut old);
    assert_eq!(
        old.repair_rejection(EntityId(2), EntityId(3)),
        Some(Rejection::InvalidTarget)
    );
    let session = ServerSession::new(old, 7, vec![PlayerId(0)]).unwrap();
    let saved =
        SavedGame::decode(&SavedGame::capture(&session).unwrap().encode().unwrap()).unwrap();
    let resumed = ServerSession::restore_saved(&world(true, true), saved).unwrap();
    let mut w = resumed.world().clone();
    assert!(!w.entity_visible(PlayerId(0), EntityId(1)));
    let before = remaining(&w);
    command(
        &mut w,
        Order::Stop {
            entity: EntityId(1),
        },
    );
    assert!(w.entity_visible(PlayerId(0), EntityId(1)));
    for _ in 0..10 {
        w.step(&[]).unwrap();
    }
    assert_eq!(remaining(&w), before);
    assert_eq!(
        command(
            &mut w,
            Order::Resume {
                entity: EntityId(1),
                building: EntityId(3)
            }
        ),
        None
    );
    for _ in 0..20 {
        if !w.entity_visible(PlayerId(0), EntityId(1)) {
            break;
        }
        w.step(&[]).unwrap();
    }
    assert!(!w.entity_visible(PlayerId(0), EntityId(1)));
    assert_eq!(
        command(
            &mut w,
            Order::Cancel {
                entity: EntityId(3)
            }
        ),
        None
    );
    assert!(w.entity_visible(PlayerId(0), EntityId(1)));
    assert_eq!(w.state().entities[0].order, UnitOrder::Idle);
}
