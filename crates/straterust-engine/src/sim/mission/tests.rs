use super::*;
use crate::content::Package;
use std::path::Path;

fn trigger(actions: Vec<MissionAction>) -> MissionTrigger {
    MissionTrigger {
        conditions: vec![MissionCondition::Countdown {
            comparison: MissionComparison::AtLeast,
            milliseconds: 0,
        }],
        actions,
    }
}
fn definition(triggers: Vec<MissionTrigger>) -> Mission {
    Mission {
        schema_version: 1,
        player: PlayerId(0),
        rescuable_players: Vec::new(),
        rescuers: Vec::new(),
        alliances: Vec::new(),
        poll_ticks: 31,
        wait_step_ms: 42,
        locations: vec![MissionLocation {
            excluded_elevations: 0,
            left: 0,
            top: 0,
            right: 1600,
            bottom: 1000,
        }],
        triggers,
    }
}
fn world(mission: Mission) -> World {
    let package =
        Package::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/fixtures"))
            .unwrap();
    let base = package.world(42).unwrap();
    let mut map = base.map().clone();
    map.mission = Some(mission);
    World::new(base.rules().clone(), map, 42).unwrap()
}
fn step(world: &mut World, count: usize) {
    for _ in 0..count {
        world.step(&[]).unwrap();
    }
}

#[test]
fn elapsed_zero_initialization_runs_on_the_first_trigger_poll() {
    let mut initialize = trigger(vec![MissionAction::Objectives { text: 7 }]);
    initialize.conditions = vec![MissionCondition::Elapsed {
        comparison: MissionComparison::AtMost,
        milliseconds: 0,
    }];
    let mut world = world(definition(vec![initialize]));
    step(&mut world, 2);
    assert!(world.state.mission.as_ref().unwrap().triggers[0].complete);
    assert!(
        world
            .state
            .mission
            .as_ref()
            .unwrap()
            .events
            .contains(&MissionEvent::Objectives { text: 7 })
    );
}

#[test]
fn recurring_doodad_triggers_open_passages_and_hash_their_state() {
    let base = world(definition(vec![trigger(vec![MissionAction::Cosmetic])]));
    let mut rules = base.rules().clone();
    rules.units[0].blocks_movement = false;
    let mut map = base.map().clone();
    map.spawns[0].doodad_enabled = Some(true);
    let door_trigger = trigger(vec![
        MissionAction::ToggleDoodad {
            players: vec![map.spawns[0].owner],
            units: MissionUnits::Type(map.spawns[0].unit_type),
            location: 0,
        },
        MissionAction::Preserve,
    ]);
    map.mission = Some(definition(vec![door_trigger; 6]));
    let mut world = World::new(rules, map, 42).unwrap();
    let first = world.state_hash();
    step(&mut world, 2);
    assert_eq!(world.state.entities[0].doodad_enabled, Some(false));
    assert_ne!(world.state_hash(), first);
    assert!(!world.state.mission.as_ref().unwrap().triggers[0].complete);
    step(&mut world, 31);
    assert_eq!(world.state.entities[0].doodad_enabled, Some(true));
}

#[test]
fn resource_kill_and_elevation_conditions_use_authoritative_data() {
    let mut world = world(definition(vec![trigger(vec![MissionAction::Cosmetic])]));
    let player = PlayerId(0);
    world.state.players[0]
        .resources
        .insert("minerals".into(), 399);
    world
        .state
        .kills
        .entry(player)
        .or_default()
        .insert(UnitTypeId(1), 2);
    let state = world.state.mission.as_ref().unwrap();
    assert!(world.mission_condition(
        &MissionCondition::Resources {
            players: vec![player],
            kinds: vec!["minerals".into()],
            comparison: MissionComparison::AtMost,
            amount: 399
        },
        state
    ));
    assert!(!world.mission_condition(
        &MissionCondition::Resources {
            players: vec![player],
            kinds: vec!["minerals".into()],
            comparison: MissionComparison::AtLeast,
            amount: 400
        },
        state
    ));
    assert!(world.mission_condition(
        &MissionCondition::Kills {
            players: vec![player],
            units: MissionUnits::Type(UnitTypeId(1)),
            comparison: MissionComparison::Exactly,
            amount: 2
        },
        state
    ));
    let actor = &world.state.entities[0];
    let area = MissionLocation {
        excluded_elevations: 7,
        ..state.locations[0]
    };
    assert!(world.mission_matches(actor, &[actor.owner], MissionUnits::Any, Some(area)));
    let mut flyer = actor.clone();
    flyer.airborne = true;
    assert!(!world.mission_matches(&flyer, &[flyer.owner], MissionUnits::Any, Some(area)));
}

#[test]
fn teleport_preserves_identity_and_keeps_units_in_distinct_clear_spaces() {
    let mut mission = definition(vec![trigger(vec![MissionAction::Teleport {
        players: vec![PlayerId(0)],
        units: MissionUnits::Men,
        location: 0,
        destination: 1,
    }])]);
    mission.locations.push(MissionLocation {
        excluded_elevations: 0,
        left: 900,
        top: 500,
        right: 1000,
        bottom: 600,
    });
    let mut world = world(mission);
    let original: Vec<_> = world
        .state
        .entities
        .iter()
        .filter(|e| e.owner == PlayerId(0))
        .map(|e| e.id)
        .collect();
    step(&mut world, 2);
    let actors: Vec<_> = world
        .state
        .entities
        .iter()
        .filter(|e| e.owner == PlayerId(0))
        .collect();
    assert_eq!(actors.iter().map(|e| e.id).collect::<Vec<_>>(), original);
    assert!(
        actors
            .iter()
            .all(|e| e.position.x >= 850 && e.position.y >= 450)
    );
    for a in &actors {
        for b in &actors {
            if a.id != b.id {
                assert_ne!(a.position, b.position);
            }
        }
    }
}

#[test]
fn shared_owner_waits_resume_once_and_other_triggers_can_start() {
    let mut world = world(definition(vec![
        trigger(vec![
            MissionAction::Wait { milliseconds: 42 },
            MissionAction::SetSwitch {
                index: 0,
                set: true,
            },
        ]),
        trigger(vec![
            MissionAction::Text { text: 0 },
            MissionAction::Wait { milliseconds: 0 },
            MissionAction::SetSwitch {
                index: 1,
                set: true,
            },
        ]),
    ]));
    step(&mut world, 1);
    assert!(world.state.mission.as_ref().unwrap().events.is_empty());
    step(&mut world, 1);
    let state = world.state.mission.as_ref().unwrap();
    assert_eq!(state.events, vec![MissionEvent::Text { text: 0 }]);
    assert_eq!(state.wait.as_ref().unwrap().trigger, 0);
    assert_eq!(state.triggers[1].action, 1);
    step(&mut world, 1);
    assert!(
        !world.state.mission.as_ref().unwrap().switches[0],
        "42ms expires only after remaining falls below42"
    );
    step(&mut world, 1);
    let state = world.state.mission.as_ref().unwrap();
    assert!(state.switches[0]);
    assert!(!state.switches[1]);
    assert_eq!(state.wait.as_ref().unwrap().trigger, 1);
    step(&mut world, 1);
    assert!(world.state.mission.as_ref().unwrap().switches[1]);
    step(&mut world, 100);
    assert_eq!(world.state.mission.as_ref().unwrap().events.len(), 1);
}

#[test]
fn source_pause_stops_units_but_not_final_wait_or_defeat() {
    let mut world = world(definition(vec![trigger(vec![
        MissionAction::Pause,
        MissionAction::Wait { milliseconds: 84 },
        MissionAction::Defeat,
    ])]));
    world
        .step(&[Command {
            tick: Tick(0),
            player: PlayerId(0),
            sequence: 1,
            order: Order::Move {
                entity: EntityId(1),
                target: Position { x: 800, y: 420 },
            },
        }])
        .unwrap();
    step(&mut world, 1);
    let position = world.state.entities[0].position;
    assert!(world.state.mission.as_ref().unwrap().paused);
    let mut restored = world.clone();
    restored.state = ron::from_str(&ron::to_string(&world.state).unwrap()).unwrap();
    step(&mut world, 3);
    step(&mut restored, 3);
    assert_eq!(world.state.entities[0].position, position);
    assert_eq!(world.state.defeated, vec![PlayerId(0)]);
    assert_eq!(world.state_hash(), restored.state_hash());
}

#[test]
fn create_uses_free_positions_and_kill_switch_resources_are_authoritative() {
    let mut mission = definition(vec![trigger(vec![
        MissionAction::SetResources {
            players: vec![PlayerId(0)],
            resources: vec![ResourceAmount {
                kind: "minerals".into(),
                amount: 200,
            }],
        },
        MissionAction::Create {
            player: PlayerId(0),
            unit_type: UnitTypeId(1),
            location: 1,
        },
        MissionAction::Create {
            player: PlayerId(0),
            unit_type: UnitTypeId(1),
            location: 1,
        },
        MissionAction::Invincibility {
            players: vec![PlayerId(0)],
            units: MissionUnits::Type(UnitTypeId(1)),
            location: 1,
            enabled: true,
        },
        MissionAction::Wait { milliseconds: 0 },
        MissionAction::Kill {
            players: vec![PlayerId(0)],
            units: MissionUnits::Type(UnitTypeId(1)),
            location: 1,
        },
    ])]);
    mission.locations.push(MissionLocation {
        excluded_elevations: 0,
        left: 750,
        top: 450,
        right: 850,
        bottom: 550,
    });
    let mut world = world(mission);
    step(&mut world, 2);
    assert_eq!(world.resource_balance(PlayerId(0), "minerals"), 200);
    assert_eq!(world.resource_balance(PlayerId(1), "minerals"), 0);
    let created: Vec<_> = world
        .state
        .entities
        .iter()
        .filter(|entity| entity.id.0 >= 6)
        .collect();
    assert_eq!(created.len(), 2);
    assert_ne!(created[0].position, created[1].position);
    assert!(created.iter().all(|entity| entity.invincible));
    step(&mut world, 1);
    assert_eq!(world.state.entities.len(), 5);
}

#[test]
fn bring_uses_footprint_overlap_and_move_location_keeps_extent_in_bounds() {
    let mut definition = definition(vec![trigger(vec![MissionAction::MoveLocation {
        location: 1,
        players: vec![PlayerId(0)],
        units: MissionUnits::Type(UnitTypeId(1)),
        search_location: 0,
    }])]);
    definition.locations.push(MissionLocation {
        excluded_elevations: 0,
        left: 0,
        top: 0,
        right: 800,
        bottom: 900,
    });
    let mut world = world(definition);
    Arc::make_mut(&mut world.rules).units[0].footprint = Footprint {
        width: 20,
        height: 20,
    };
    let condition = MissionCondition::Count {
        players: vec![PlayerId(0)],
        units: MissionUnits::Type(UnitTypeId(1)),
        location: Some(1),
        comparison: MissionComparison::AtLeast,
        amount: 1,
    };
    let mut state = world.state.mission.clone().unwrap();
    state.locations[1] = MissionLocation {
        excluded_elevations: 0,
        left: 425,
        top: 415,
        right: 440,
        bottom: 425,
    };
    assert!(
        world.mission_condition(&condition, &state),
        "center420 liesoutside but footprint overlaps"
    );
    state.locations[1].left = 430;
    assert!(
        !world.mission_condition(&condition, &state),
        "touching half-open edge is not overlap"
    );
    step(&mut world, 2);
    let moved = world.state.mission.as_ref().unwrap().locations[1];
    assert_eq!(
        moved,
        MissionLocation {
            excluded_elevations: 0,
            left: 20,
            top: 0,
            right: 820,
            bottom: 900
        }
    );
}

#[test]
fn cosmetic_events_and_identifiers_do_not_change_gameplay_hashes() {
    let mut a = world(definition(vec![trigger(vec![
        MissionAction::Text { text: 1 },
        MissionAction::Wait { milliseconds: 42 },
    ])]));
    let mut b = world(definition(vec![trigger(vec![
        MissionAction::Sound { sound: 2 },
        MissionAction::Wait { milliseconds: 42 },
    ])]));
    assert_eq!(a.map_hash(), b.map_hash());
    for _ in 0..5 {
        step(&mut a, 1);
        step(&mut b, 1);
        assert_eq!(a.state_hash(), b.state_hash());
    }
    assert_ne!(
        a.state.mission.as_ref().unwrap().events,
        b.state.mission.as_ref().unwrap().events
    );
    let c = world(definition(vec![trigger(vec![
        MissionAction::Text { text: 1 },
        MissionAction::Wait { milliseconds: 43 },
    ])]));
    assert_ne!(a.map_hash(), c.map_hash());
}

#[test]
fn rescue_uses_footprint_square_and_depot_transfers_every_unit_with_orders_reset() {
    let mut mission = definition(vec![trigger(vec![MissionAction::Wait {
        milliseconds: 10000,
    }])]);
    mission.rescuable_players = vec![PlayerId(2), PlayerId(3)];
    mission.rescuers = vec![PlayerId(0)];
    mission.alliances = vec![[PlayerId(0), PlayerId(2)], [PlayerId(0), PlayerId(3)]];
    let base = world(definition(vec![trigger(vec![MissionAction::Wait {
        milliseconds: 0,
    }])]));
    let mut rules = base.rules().clone();
    rules.units[0].footprint = Footprint {
        width: 20,
        height: 20,
    };
    rules.units[1].speed = 0;
    rules.units[1].structure = true;
    rules.units[1].dropoff = vec!["minerals".into()];
    rules.research.push(Research {
        id: ResearchId(1),
        facility: UnitTypeId(2),
        cost: vec![ResourceAmount {
            kind: "minerals".into(),
            amount: 20,
        }],
        ticks: 100,
        effect: ResearchEffect::Armor {
            units: vec![UnitTypeId(1)],
            amount: 1,
        },
    });
    let mut map = base.map().clone();
    map.players = 4;
    map.mission = Some(mission);
    map.spawns = vec![
        Spawn {
            owner: PlayerId(0),
            unit_type: UnitTypeId(1),
            position: Position { x: 430, y: 430 },
            ..Spawn::default()
        },
        Spawn {
            owner: PlayerId(2),
            unit_type: UnitTypeId(1),
            position: Position { x: 500, y: 500 },
            ..Spawn::default()
        },
        Spawn {
            owner: PlayerId(2),
            unit_type: UnitTypeId(1),
            position: Position { x: 1200, y: 800 },
            ..Spawn::default()
        },
        Spawn {
            owner: PlayerId(3),
            unit_type: UnitTypeId(2),
            position: Position { x: 350, y: 350 },
            ..Spawn::default()
        },
        Spawn {
            owner: PlayerId(3),
            unit_type: UnitTypeId(1),
            position: Position { x: 1300, y: 800 },
            ..Spawn::default()
        },
    ];
    let mut world = World::new(rules, map, 42).unwrap();
    world.state.entities[1].order = UnitOrder::Hold;
    world.state.entities[1]
        .queued_orders
        .push_back(UnitOrder::Move {
            target: Position { x: 900, y: 800 },
        });
    world.advance_rescue();
    assert_eq!(
        world.state.entities[1].owner,
        PlayerId(0),
        "collision footprint intersects ±64 square even though center and radial distance do not"
    );
    assert_eq!(
        world.state.entities[2].owner,
        PlayerId(2),
        "ordinary rescued unit does not transfer distant allies"
    );
    assert!(matches!(world.state.entities[1].order, UnitOrder::Idle));
    assert!(world.state.entities[1].queued_orders.is_empty());
    world.state.entities[3].research = Some(ResearchJob {
        id: ResearchId(1),
        remaining: 50,
        total: 100,
    });
    world.state.entities[0].position = Position { x: 400, y: 400 };
    world.advance_rescue();
    assert_eq!(world.state.entities[3].owner, PlayerId(0));
    assert!(world.state.entities[3].research.is_none());
    assert_eq!(
        world.resource_balance(PlayerId(3), "minerals"),
        20,
        "refund belongs to the old owner"
    );
    assert_eq!(world.resource_balance(PlayerId(0), "minerals"), 0);
    assert_eq!(
        world.state.entities[4].owner,
        PlayerId(0),
        "completed grounded depot transfers its entire owner"
    );
    let mut restored = world.clone();
    restored.state = ron::from_str(&ron::to_string(&world.state).unwrap()).unwrap();
    step(&mut world, 50);
    step(&mut restored, 50);
    assert_eq!(world.state_hash(), restored.state_hash());
}

#[test]
fn mission_revealer_ignores_collision_but_does_not_create_a_visible_gameplay_unit() {
    let mut world = world(definition(vec![trigger(vec![MissionAction::Wait {
        milliseconds: 0,
    }])]));
    Arc::make_mut(&mut world.rules).units[1].revealer = true;
    let occupied = world.state.entities[0].position;
    let id = world
        .mission_spawn(PlayerId(0), UnitTypeId(2), occupied)
        .unwrap();
    let revealer = world
        .state
        .entities
        .iter()
        .find(|entity| entity.id == id)
        .unwrap();
    assert_eq!(revealer.position, occupied);
    assert!(!world.entity_visible(PlayerId(0), id));
}

#[test]
fn invalid_indices_geometry_counts_and_duplicate_alliances_are_rejected() {
    let valid = definition(vec![trigger(vec![MissionAction::Wait { milliseconds: 0 }])]);
    let base = world(valid.clone());
    for bad in [
        Mission {
            player: PlayerId(9),
            ..valid.clone()
        },
        Mission {
            poll_ticks: 0,
            ..valid.clone()
        },
        Mission {
            alliances: vec![[PlayerId(0), PlayerId(1)], [PlayerId(0), PlayerId(1)]],
            ..valid.clone()
        },
        Mission {
            triggers: vec![trigger(vec![MissionAction::Create {
                player: PlayerId(0),
                unit_type: UnitTypeId(1),
                location: 1,
            }])],
            ..valid.clone()
        },
        Mission {
            triggers: vec![trigger(vec![MissionAction::SetSwitch {
                index: 256,
                set: true,
            }])],
            ..valid.clone()
        },
        Mission {
            triggers: vec![trigger(vec![MissionAction::Wait {
                milliseconds: 3_600_001,
            }])],
            ..valid.clone()
        },
        Mission {
            locations: vec![MissionLocation {
                excluded_elevations: 0,
                left: -1,
                top: 0,
                right: 1600,
                bottom: 1000,
            }],
            ..valid.clone()
        },
    ] {
        assert!(bad.validate(base.rules(), base.map()).is_err());
    }
}
