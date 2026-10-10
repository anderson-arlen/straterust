use super::*;
use crate::{
    content::Package,
    session::{SavedGame, ServerSession},
};
use std::path::Path;

fn world() -> World {
    Package::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/fixtures"))
        .unwrap()
        .world(42)
        .unwrap()
}

#[test]
fn appended_deposits_migrate_without_replenishing_or_reordering_saved_resources() {
    let base = world();
    let mut map = base.map().clone();
    map.resources = [200, 300]
        .map(|x| ResourceSpawn {
            kind: "minerals".into(),
            position: Position { x, y: 300 },
            amount: 1000,
            footprint: Footprint {
                width: 32,
                height: 32,
            },
            requires_extractor: false,
            terrain_corners: None,
        })
        .to_vec();
    let mut old = World::new(base.rules().clone(), map, 42).unwrap();
    old.state.tick = Tick(432);
    old.state.resources[0].amount = 0;
    let saved =
        SavedGame::capture(&ServerSession::new(old.clone(), 42, vec![PlayerId(0)]).unwrap())
            .unwrap();
    let mut map = old.map().clone();
    let mut oil = map.resources[0].clone();
    oil.kind = "oil".into();
    oil.position = Position { x: 400, y: 300 };
    oil.requires_extractor = true;
    oil.amount = 120000;
    map.resources.push(oil);
    let current = World::new(old.rules().clone(), map.clone(), 42).unwrap();
    let resumed = ServerSession::restore_saved(&current, saved.clone()).unwrap();
    assert_eq!(resumed.world().tick(), old.tick());
    let nodes = &resumed.world().state().resources;
    assert_eq!(&nodes[..old.state.resources.len()], old.state.resources);
    assert_eq!(nodes.last().unwrap().amount, 120000);
    let saved_again = SavedGame::capture(&resumed).unwrap();
    assert_eq!(
        ServerSession::restore_saved(&current, saved_again)
            .unwrap()
            .world()
            .state_hash(),
        resumed.world().state_hash()
    );
    map.resources.swap(0, 1);
    let reordered = World::new(old.rules().clone(), map, 42).unwrap();
    let error = ServerSession::restore_saved(&reordered, saved)
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains("changed saved resource deposit"), "{error}");
}

#[test]
fn updates_add_new_static_types_to_older_saves_without_resurrecting_destroyed_structures() {
    let old = world();
    let saved =
        SavedGame::capture(&ServerSession::new(old.clone(), 42, vec![PlayerId(0)]).unwrap())
            .unwrap();
    let mut rules = old.rules().clone();
    rules.units.push(UnitType {
        id: UnitTypeId(1000),
        structure: true,
        speed: 0,
        blocks_movement: true,
        max_hp: 40,
        ..Default::default()
    });
    let mut map = old.map().clone();
    map.spawns.push(Spawn {
        unit_type: UnitTypeId(1000),
        owner: PlayerId(1),
        position: Position { x: 400, y: 300 },
        ..Default::default()
    });
    let current = World::new(rules, map, 42).unwrap();
    let restored = ServerSession::restore_saved(&current, saved).unwrap();
    let added = restored.world().state().entities.last().unwrap();
    assert_eq!(added.unit_type, UnitTypeId(1000));
    assert_eq!(added.id.0, old.state().next_entity_id);
    let mut destroyed = restored.world().clone();
    destroyed.state.entities.pop();
    let saved =
        SavedGame::capture(&ServerSession::new(destroyed, 42, vec![PlayerId(0)]).unwrap()).unwrap();
    let mut changed = current.rules().clone();
    changed.units.last_mut().unwrap().armor += 1;
    let changed = World::new(changed, current.map().clone(), 42).unwrap();
    let resumed = ServerSession::restore_saved(&changed, saved).unwrap();
    assert!(
        !resumed
            .world()
            .state()
            .entities
            .iter()
            .any(|e| e.unit_type == UnitTypeId(1000))
    );
}

#[test]
fn saved_games_use_updated_rules_while_replays_require_exact_identity() {
    let mut old = world();
    old.state.entities[0].cargo = Some(ResourceAmount {
        kind: "minerals".into(),
        amount: 8,
    });
    old.state.entities[0].order = UnitOrder::Move {
        target: Position { x: 700, y: 400 },
    };
    let server = ServerSession::new(old.clone(), 42, vec![PlayerId(0)]).unwrap();
    let mut saved = SavedGame::capture(&server).unwrap();
    saved.checkpoint.world.identity.simulation = "straterust-sim-38".into();
    let mut rules = old.rules().clone();
    rules.units[0].speed += 1;
    rules.units[0].energy_pool = Some(EnergyPool {
        maximum: 200,
        initial: 50,
        regeneration: 8,
    });
    let updated = World::new(rules, old.map().clone(), 42).unwrap();
    assert!(ServerSession::restore(&updated, saved.checkpoint.clone()).is_err());
    let mut restored = ServerSession::restore_saved(
        &updated,
        SavedGame::decode(&saved.encode().unwrap()).unwrap(),
    )
    .unwrap();
    assert_eq!(
        restored.world().state().entities[0].cargo,
        old.state.entities[0].cargo
    );
    assert_eq!(
        restored.world().state().entities[0].order,
        old.state.entities[0].order
    );
    assert_eq!(
        restored.world().rules().units[0].speed,
        old.rules().units[0].speed + 1
    );
    assert_eq!(restored.world().state().entities[0].energy, 50 * 256);
    let mut resumed = ServerSession::restore_saved(
        &updated,
        SavedGame::decode(&SavedGame::capture(&restored).unwrap().encode().unwrap()).unwrap(),
    )
    .unwrap();
    for _ in 0..20 {
        restored.advance(&[]).unwrap();
        resumed.advance(&[]).unwrap();
        assert_eq!(restored.world().state_hash(), resumed.world().state_hash());
    }
}

#[test]
fn save_integrity_uses_original_bytes_before_adding_default_fields() {
    let server = ServerSession::new(world(), 42, vec![PlayerId(0)]).unwrap();
    let snapshot = server.save_snapshot().unwrap();
    let state = ron::ser::to_string(&snapshot.world.state).unwrap();
    let legacy_state = state.replace("mode_transition:None,ability_auras:[],last_cast:None,", "");
    assert_ne!(state, legacy_state);
    let legacy = ron::ser::to_string(&snapshot)
        .unwrap()
        .replace(&state, &legacy_state)
        .replace(
            &snapshot.world.checksum,
            blake3::hash(legacy_state.as_bytes()).to_hex().as_str(),
        )
        .replace(SIMULATION_REVISION, "straterust-sim-38");
    let saved = SavedGame::decode(legacy.as_bytes()).unwrap();
    let restored = ServerSession::restore_saved(&world(), saved).unwrap();
    assert_eq!(
        restored.world().state().entities,
        snapshot.world.state.entities
    );
    let damaged = legacy.replace("rng_state:42", "rng_state:43");
    assert_ne!(damaged, legacy);
    assert!(SavedGame::decode(damaged.as_bytes()).is_err());
    let encoded =
        String::from_utf8(SavedGame::capture(&server).unwrap().encode().unwrap()).unwrap();
    assert!(SavedGame::decode(encoded.replace("rng_state:42", "rng_state:43").as_bytes()).is_err());
}

#[test]
fn save_migration_keeps_completed_initialization_and_waits_and_applies_location_fixes() {
    let base = world();
    let mut rules = base.rules().clone();
    rules.units[0].max_hp = 100;
    rules.units[1].structure = true;
    rules.units[1].speed = 0;
    let mut map = base.map().clone();
    let condition = |ms| {
        vec![MissionCondition::Elapsed {
            comparison: MissionComparison::AtLeast,
            milliseconds: ms,
        }]
    };
    map.mission = Some(Mission {
        schema_version: 1,
        player: PlayerId(0),
        rescuable_players: vec![],
        rescuers: vec![],
        alliances: vec![],
        poll_ticks: 1,
        wait_step_ms: 50,
        locations: vec![MissionLocation {
            excluded_elevations: 32,
            left: 0,
            top: 0,
            right: 1600,
            bottom: 1000,
        }],
        triggers: vec![
            MissionTrigger {
                conditions: condition(0),
                actions: vec![MissionAction::SetResources {
                    players: vec![PlayerId(0)],
                    resources: vec![ResourceAmount {
                        kind: "minerals".into(),
                        amount: 50,
                    }],
                }],
            },
            MissionTrigger {
                conditions: condition(1000),
                actions: vec![
                    MissionAction::Wait { milliseconds: 5000 },
                    MissionAction::Victory,
                ],
            },
        ],
    });
    let mut old = World::new(rules.clone(), map.clone(), 42).unwrap();
    for _ in 0..25 {
        old.step(&[]).unwrap();
    }
    assert!(old.state.mission.as_ref().unwrap().triggers[0].complete);
    let wait = old.state.mission.as_ref().unwrap().wait.clone().unwrap();
    let saved =
        SavedGame::capture(&ServerSession::new(old.clone(), 42, vec![PlayerId(0)]).unwrap())
            .unwrap();
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
    rules.units[0].max_hp = 80;
    let mission = map.mission.as_mut().unwrap();
    mission.locations[0].excluded_elevations = 0;
    mission.triggers[0].actions[0] = MissionAction::SetResources {
        players: vec![PlayerId(0)],
        resources: vec![ResourceAmount {
            kind: "minerals".into(),
            amount: 9999,
        }],
    };
    mission.triggers.insert(
        0,
        MissionTrigger {
            conditions: condition(0),
            actions: vec![MissionAction::GrantResearch {
                player: PlayerId(0),
                research: ResearchId(1),
            }],
        },
    );
    let updated = World::new(rules, map, 42).unwrap();
    let mut restored = ServerSession::restore_saved(
        &updated,
        SavedGame::decode(&saved.encode().unwrap()).unwrap(),
    )
    .unwrap();
    let progress = restored.world().state().mission.as_ref().unwrap();
    assert_eq!(
        progress.wait,
        Some(MissionWait {
            trigger: wait.trigger + 1,
            remaining_ms: wait.remaining_ms
        })
    );
    assert!(progress.triggers[0].complete && progress.triggers[1].complete);
    assert_eq!(progress.locations[0].excluded_elevations, 0);
    assert_eq!(
        restored.world().state().players[0].resources,
        old.state.players[0].resources
    );
    assert!(restored.world().has_research(PlayerId(0), ResearchId(1)));
    assert_eq!(restored.world().state().entities[0].hp, 80);
    // A later update can extend an existing completed starting-research
    // trigger as well as prepend a new one.
    let saved = SavedGame::capture(&restored).unwrap();
    let mut rules = updated.rules().clone();
    rules.research.push(Research {
        available: true,
        id: ResearchId(2),
        facility: UnitTypeId(2),
        previous: None,
        prerequisites: Vec::new(),
        cost: vec![],
        ticks: 5,
        effect: ResearchEffect::Armor {
            units: vec![UnitTypeId(2)],
            amount: 1,
        },
    });
    let mut map = updated.map().clone();
    map.mission.as_mut().unwrap().triggers[0]
        .actions
        .push(MissionAction::GrantResearch {
            player: PlayerId(0),
            research: ResearchId(2),
        });
    let updated = World::new(rules, map, 42).unwrap();
    restored = ServerSession::restore_saved(&updated, saved).unwrap();
    assert!(restored.world().has_research(PlayerId(0), ResearchId(2)));
    for _ in 0..120 {
        restored.advance(&[]).unwrap();
    }
    assert_eq!(restored.world().state().winner, Some(PlayerId(0)));
    assert_eq!(
        restored.world().state().players[0].resources,
        old.state.players[0].resources
    );
}

#[test]
fn incompatible_saved_maps_have_specific_errors() {
    let original = world();
    let saved =
        SavedGame::capture(&ServerSession::new(original.clone(), 42, vec![PlayerId(0)]).unwrap())
            .unwrap();
    let mut map = original.map().clone();
    map.width += 32;
    let changed = World::new(original.rules().clone(), map, 42).unwrap();
    assert!(
        ServerSession::restore_saved(&changed, saved.clone())
            .err()
            .unwrap()
            .to_string()
            .contains("dimensions changed")
    );
    let mut map = original.map().clone();
    map.id = "another-map".into();
    let changed = World::new(original.rules().clone(), map, 42).unwrap();
    assert!(
        ServerSession::restore_saved(&changed, saved)
            .err()
            .unwrap()
            .to_string()
            .contains("different ruleset or map")
    );
}
