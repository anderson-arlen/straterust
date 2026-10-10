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
fn starting_research_and_reinforcement_properties_survive_snapshot_continuation() {
    let baseline = world(definition(vec![trigger(vec![MissionAction::Cosmetic])]));
    let mut rules = baseline.rules().clone();
    rules.units[0].max_hp = 100;
    rules.units[0].max_shields = 80;
    rules.units[0].cloak = Some(Cloak {
        energy_max: 200,
        ..Cloak::default()
    });
    rules.units[1].structure = true;
    rules.units[1].speed = 0;
    rules.research.push(Research {
        available: true,
        id: ResearchId(1),
        facility: UnitTypeId(2),
        previous: None,
        prerequisites: Vec::new(),
        cost: vec![],
        ticks: 5,
        effect: ResearchEffect::Armor {
            units: vec![UnitTypeId(1)],
            amount: 1,
        },
    });
    let mut map = baseline.map().clone();
    map.mission = Some(definition(vec![trigger(vec![
        MissionAction::GrantResearch {
            player: PlayerId(0),
            research: ResearchId(1),
        },
        MissionAction::Create {
            player: PlayerId(0),
            unit_type: UnitTypeId(1),
            location: 0,
            properties: UnitProperties {
                illusion_ticks: None,
                hp_percent: Some(50),
                shield_percent: Some(25),
                energy_percent: Some(75),
                invincible: true,
                cloaked: true,
            },
        },
    ])]));
    let mut world = World::new(rules, map, 42).unwrap();
    step(&mut world, 2);
    assert!(world.has_research(PlayerId(0), ResearchId(1)));
    let reinforcement = world.state.entities.last().unwrap();
    assert_eq!(reinforcement.hp, 50);
    assert_eq!(reinforcement.shields, 20 * 256);
    assert_eq!(reinforcement.energy, 150 * 256);
    assert!(reinforcement.invincible && reinforcement.cloaked);
    let mut restored = world
        .restore_snapshot(world.save_snapshot().unwrap())
        .unwrap();
    step(&mut world, 32);
    step(&mut restored, 32);
    assert_eq!(world.state_hash(), restored.state_hash());
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
            enabled: None,
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
fn explicit_doodad_disable_overrides_a_toggle_and_stays_disabled_when_preserved() {
    let base = world(definition(vec![trigger(vec![MissionAction::Cosmetic])]));
    let mut rules = base.rules().clone();
    rules.units[0].blocks_movement = false;
    let mut map = base.map().clone();
    map.spawns[0].doodad_enabled = Some(true);
    let action = |enabled| MissionAction::ToggleDoodad {
        players: vec![map.spawns[0].owner],
        units: MissionUnits::Type(map.spawns[0].unit_type),
        location: 0,
        enabled,
    };
    map.mission = Some(definition(vec![
        trigger(vec![
            action(None),
            action(Some(false)),
            MissionAction::Preserve,
        ]);
        6
    ]));
    let mut world = World::new(rules, map, 42).unwrap();
    step(&mut world, 2);
    assert_eq!(world.state.entities[0].doodad_enabled, Some(false));
    step(&mut world, 31);
    assert_eq!(world.state.entities[0].doodad_enabled, Some(false));
    let mut restored = world
        .restore_snapshot(world.save_snapshot().unwrap())
        .unwrap();
    step(&mut world, 31);
    step(&mut restored, 31);
    assert_eq!(world.state_hash(), restored.state_hash());
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
    assert!(!world.mission_matches(actor, &[actor.owner], MissionUnits::Any, Some(area)));
    let mut flyer = actor.clone();
    flyer.airborne = true;
    assert!(world.mission_matches(&flyer, &[flyer.owner], MissionUnits::Any, Some(area)));
    let ground_only = MissionLocation {
        excluded_elevations: 56,
        ..area
    };
    assert!(world.mission_matches(actor, &[actor.owner], MissionUnits::Any, Some(ground_only)));
    assert!(!world.mission_matches(&flyer, &[flyer.owner], MissionUnits::Any, Some(ground_only)));
}

#[test]
fn temporary_copies_have_no_damage_or_supply_and_expire_after_restore() {
    let base = world(definition(vec![trigger(vec![MissionAction::Cosmetic])]));
    let mut rules = base.rules().clone();
    rules.units[0].supply_used = 2;
    rules.units[0].weapon = Some(Weapon {
        friendly_splash: false,
        projectile_speed: 0,
        damage: 4,
        range: 32,
        cooldown: 5,
        targets_air: false,
        target_classes: Vec::new(),
        cooldown_jitter: None,
        damage_kind: DamageKind::Normal,
        splash: None,
        strikes: vec![],
    });
    let definitions = World::new(rules, base.map().clone(), 42).unwrap();
    let mut original = definitions.clone();
    let source = original.state.entities[0].clone();
    let target = original
        .state
        .entities
        .iter()
        .find(|e| e.owner != source.owner)
        .unwrap()
        .clone();
    let weapon = original
        .unit_type(source.unit_type)
        .unwrap()
        .weapon
        .as_ref()
        .unwrap()
        .clone();
    let supply = original.supply(source.owner).0;
    original.state.entities[0].illusion_remaining = Some(2);
    assert_eq!(
        original.supply(source.owner).0,
        supply - original.unit_type(source.unit_type).unwrap().supply_used
    );
    let mut damage = rts::Damage::default();
    original.record_hit(
        &mut damage,
        (source.id, source.unit_type),
        &target,
        &weapon,
        1,
    );
    assert!(damage.hits.is_empty() && damage.shields.is_empty());
    original.state.entities[0].illusion_remaining = None;
    let mut copy = target.clone();
    copy.illusion_remaining = Some(2);
    let mut normal = rts::Damage::default();
    original.record_hit(
        &mut normal,
        (source.id, source.unit_type),
        &target,
        &weapon,
        1,
    );
    original.record_hit(
        &mut damage,
        (source.id, source.unit_type),
        &copy,
        &weapon,
        1,
    );
    assert_eq!(
        damage.incoming[&copy.id][&source.id],
        2 * normal.incoming[&target.id][&source.id]
    );
    original.state.entities[0].illusion_remaining = Some(2);
    let view = original.player_view(target.owner).unwrap();
    let visible = view.into_world(&definitions).unwrap();
    assert!(
        visible
            .state()
            .entities
            .iter()
            .all(|e| e.illusion_remaining.is_none())
    );
    let mut restored = definitions
        .restore_snapshot(original.save_snapshot().unwrap())
        .unwrap();
    step(&mut original, 2);
    step(&mut restored, 2);
    assert_eq!(original.state_hash(), restored.state_hash());
    assert!(original.index(source.id).is_none());
    assert_eq!(
        original.state.statistics[usize::from(source.owner.0)].units_lost,
        0
    );
}

#[test]
fn scripted_movement_filters_owners_and_survives_checkpoint_restore() {
    let mut mission = definition(vec![trigger(vec![MissionAction::OrderMove {
        players: vec![PlayerId(0)],
        units: MissionUnits::Type(UnitTypeId(1)),
        destination: 1,
    }])]);
    mission.locations.push(MissionLocation {
        excluded_elevations: 0,
        left: 900,
        top: 500,
        right: 1000,
        bottom: 600,
    });
    let definitions = world(mission);
    let mut original = definitions.clone();
    step(&mut original, 2);
    let target = Position { x: 950, y: 550 };
    for entity in &original.state.entities {
        if entity.owner == PlayerId(0) && entity.unit_type == UnitTypeId(1) {
            assert_eq!(entity.order, UnitOrder::Move { target });
        } else {
            assert_ne!(entity.order, UnitOrder::Move { target });
        }
    }
    let mut restored = definitions
        .restore_snapshot(original.save_snapshot().unwrap())
        .unwrap();
    step(&mut original, 12);
    step(&mut restored, 12);
    assert_eq!(original.state_hash(), restored.state_hash());
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
            properties: Default::default(),
            player: PlayerId(0),
            unit_type: UnitTypeId(1),
            location: 1,
        },
        MissionAction::Create {
            properties: Default::default(),
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
        available: true,
        id: ResearchId(1),
        facility: UnitTypeId(2),
        previous: None,
        prerequisites: Vec::new(),
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
                properties: Default::default(),
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
