use super::*;
use straterust_engine::{
    assets::{AssetPack, ClipKind},
    media::{AudioCue, MediaPack},
    sim::{AbilityId, ResearchId, Spawn},
};

#[test]
#[ignore = "requires STRATERUST_CAMPAIGNS pointing to a refreshed retail import"]
fn retail_ghost_cloaking_sight_and_strike_media_are_usable() {
    use straterust_engine::sim::{AbilityEffect, StrikeStage, Visibility};
    let directory = std::path::PathBuf::from(std::env::var_os("STRATERUST_CAMPAIGNS").unwrap())
        .join("terran09");
    let package = Package::load(&directory).unwrap();
    let base = package.world(42).unwrap();
    let mut rules = base.rules().clone();
    rules.starting_resources = vec![
        ResourceAmount {
            kind: "minerals".into(),
            amount: 10000,
        },
        ResourceAmount {
            kind: "gas".into(),
            amount: 10000,
        },
    ];
    for id in [23, 24] {
        rules
            .research
            .iter_mut()
            .find(|r| r.id == ResearchId(id))
            .unwrap()
            .ticks = 1;
    }
    let mut map = base.map().clone();
    map.mission = None;
    map.terrain = None;
    map.ai.clear();
    map.resources.clear();
    map.creation.clear();
    map.initial_explored.clear();
    map.start_locations.clear();
    map.fog_of_war = true;
    map.width = 1536;
    map.height = 768;
    map.players = 2;
    let placement = rules
        .units
        .iter()
        .find(|u| u.id == UnitTypeId(123))
        .unwrap()
        .placement;
    let addon = (
        128 + i32::from(placement.width) / 2 + 32,
        128 + i32::from(placement.height) / 2 - 32,
    );
    map.spawns = [
        (123, 0, 128, 128),
        (124, 0, addon.0, addon.1),
        (19, 0, 800, 512),
        (1, 1, 1400, 700),
    ]
    .into_iter()
    .map(|(unit, owner, x, y)| Spawn {
        unit_type: UnitTypeId(unit),
        owner: PlayerId(owner),
        position: Position { x, y },
        energy_percent: Some(100),
        ..Default::default()
    })
    .collect();
    let mut app = demo();
    app.world = World::new(rules, map, 42).unwrap();
    app.presentation = read_ron(&directory.join("presentation.ron")).unwrap();
    app.presentation.validate().unwrap();
    let covert = app
        .world
        .state()
        .entities
        .iter()
        .find(|e| e.unit_type == UnitTypeId(124))
        .unwrap()
        .id;
    let ghost = app
        .world
        .state()
        .entities
        .iter()
        .find(|e| e.unit_type == UnitTypeId(19))
        .unwrap()
        .id;
    app.selected = BTreeSet::from([covert]);
    let buttons = app.buttons();
    for id in [23, 24] {
        let b = buttons
            .iter()
            .find(|b| b.action == Action::Research(ResearchId(id)))
            .unwrap();
        assert!(b.disabled.is_none(), "{:?}", b.disabled);
        assert!(b.tooltip.iter().any(|s| s.len() > 70));
    }
    app.activate(Action::Research(ResearchId(23))).unwrap();
    step(&mut app);
    app.selected = BTreeSet::from([ghost]);
    let buttons = app.buttons();
    let cloak = buttons
        .iter()
        .find(|b| b.action == Action::Cloak(true))
        .expect("Lockdown must not overwrite Cloak");
    assert_eq!((cloak.slot, cloak.key.as_str()), (6, "C"));
    assert_eq!(
        buttons
            .iter()
            .find(|b| b.action == Action::Cast(AbilityId(3)))
            .unwrap()
            .slot,
        7
    );
    assert_eq!(
        buttons
            .iter()
            .find(|b| b.action == Action::Cast(AbilityId(6)))
            .unwrap()
            .slot,
        8
    );
    app.activate(Action::Cloak(true)).unwrap();
    step(&mut app);
    assert!(
        app.world
            .state()
            .entities
            .iter()
            .find(|e| e.id == ghost)
            .unwrap()
            .cloaked
    );
    assert_eq!(
        app.buttons()
            .iter()
            .find(|b| b.action == Action::Cloak(false))
            .unwrap()
            .key,
        "D"
    );
    app.activate(Action::Cloak(false)).unwrap();
    step(&mut app);
    let edge = Position { x: 1152, y: 512 };
    assert_ne!(
        app.world.terrain_visibility(PlayerId(0), edge),
        Visibility::Visible
    );
    assert_eq!(
        app.world.vision_range(
            app.world
                .state()
                .entities
                .iter()
                .find(|e| e.id == ghost)
                .unwrap()
        ),
        288
    );
    app.selected = BTreeSet::from([covert]);
    app.activate(Action::Research(ResearchId(24))).unwrap();
    step(&mut app);
    assert_eq!(
        app.world.vision_range(
            app.world
                .state()
                .entities
                .iter()
                .find(|e| e.id == ghost)
                .unwrap()
        ),
        352
    );
    assert_eq!(
        app.world.terrain_visibility(PlayerId(0), edge),
        Visibility::Visible
    );
    let assets = AssetPack::load(&directory).unwrap().unwrap();
    assets.validate_for_world(&base).unwrap();
    let media = MediaPack::load(&directory).unwrap().unwrap();
    assert!(
        assets
            .sprite(UnitTypeId(19))
            .unwrap()
            .clip(ClipKind::Cast)
            .is_some()
    );
    for id in [4, 6] {
        let effect = assets
            .projectiles
            .iter()
            .find(|p| p.manifest.ability == Some(AbilityId(id)))
            .unwrap();
        assert!(effect.trail.is_some() && effect.manifest.directional);
        assert!(effect.impact.sequence.len() >= 20);
        if id == 4 {
            assert!(effect.charge.is_some());
            assert_eq!(effect.manifest.launch_offsets.len(), 32);
            assert!(effect.manifest.trail.as_ref().unwrap().directional);
            assert_eq!(
                (
                    effect.manifest.trail.as_ref().unwrap().start_ms,
                    effect.manifest.trail.as_ref().unwrap().interval_ms
                ),
                (210, 126)
            );
            assert_eq!(effect.trail.as_ref().unwrap().frames.len(), 51);
        } else {
            assert!(effect.marker.is_some());
            let trail = effect.manifest.trail.as_ref().unwrap();
            assert_eq!((trail.interval_ms, trail.rear_offset), (126, 10));
            let smoke = effect.trail.as_ref().unwrap();
            assert_eq!(smoke.sequence.len(), 11);
            for (tick, frame) in smoke.sequence.iter().enumerate() {
                assert_eq!(
                    smoke.frames[usize::from(*frame)]
                        .rgba
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .any(|p| p[3] > 0),
                    tick >= 3
                );
            }
        }
        let definition = app
            .world
            .rules()
            .units
            .iter()
            .flat_map(|u| &u.abilities)
            .find(|a| a.id == AbilityId(id))
            .unwrap();
        assert!(matches!(
            definition.effect,
            AbilityEffect::Strike {
                delivery: Some(_),
                ..
            }
        ));
    }
    for cue in [
        AudioCue::StrikeStage(AbilityId(4), StrikeStage::Charge),
        AudioCue::StrikeStage(AbilityId(4), StrikeStage::Flight),
        AudioCue::StrikeStage(AbilityId(6), StrikeStage::Ascent),
        AudioCue::StrikeStage(AbilityId(6), StrikeStage::Impact),
        AudioCue::AbilityWarning(AbilityId(6)),
    ] {
        assert!(
            media
                .audio
                .iter()
                .any(|m| m.cue == cue && !m.variants.is_empty()),
            "{cue:?}"
        );
    }
}

#[test]
#[ignore = "requires STRATERUST_CAMPAIGNS pointing to a refreshed retail import"]
fn retail_science_facility_and_siege_commands_have_effects_and_finite_source_art() {
    let directory = std::path::PathBuf::from(std::env::var_os("STRATERUST_CAMPAIGNS").unwrap())
        .join("terran08");
    let package = Package::load(&directory).unwrap();
    let base = package.world(42).unwrap();
    let assets = AssetPack::load(&directory).unwrap().unwrap();
    assets.validate_for_world(&base).unwrap();
    let media = MediaPack::load(&directory).unwrap().unwrap();
    let mut rules = base.rules().clone();
    rules.starting_resources = vec![
        ResourceAmount {
            kind: "minerals".into(),
            amount: 10000,
        },
        ResourceAmount {
            kind: "gas".into(),
            amount: 10000,
        },
    ];
    for id in [18, 19, 20, 21] {
        let research = rules
            .research
            .iter_mut()
            .find(|r| r.id == ResearchId(id))
            .unwrap();
        assert!(research.ticks > 100);
        assert_eq!(
            research.facility,
            UnitTypeId(if id == 21 { 35 } else { 123 })
        );
        research.ticks = 1;
    }
    let mut map = base.map().clone();
    map.mission = None;
    map.terrain = None;
    map.ai.clear();
    map.resources.clear();
    map.creation.clear();
    map.initial_explored.clear();
    map.start_locations.clear();
    map.fog_of_war = false;
    map.width = 1024;
    map.height = 768;
    map.players = 2;
    map.spawns = [
        (123, 0, 128, 128),
        (33, 0, 384, 128),
        (32, 0, 640, 128),
        (35, 0, 736, 144),
        (22, 0, 128, 400),
        (112, 0, 256, 400),
        (56, 1, 800, 400),
        (1, 1, 400, 400),
    ]
    .into_iter()
    .map(|(unit, owner, x, y)| Spawn {
        unit_type: UnitTypeId(unit),
        owner: PlayerId(owner),
        position: Position { x, y },
        energy_percent: Some(100),
        ..Default::default()
    })
    .collect();
    let mut app = demo();
    app.world = World::new(rules, map, 42).unwrap();
    app.presentation = read_ron(&directory.join("presentation.ron")).unwrap();
    app.presentation.validate().unwrap();
    app.visuals = crate::visual::Visuals::new(&app.world);
    let entity = |world: &World, unit| {
        world
            .state()
            .entities
            .iter()
            .find(|e| e.unit_type == UnitTypeId(unit))
            .unwrap()
            .id
    };
    let science = entity(&app.world, 123);
    app.selected = BTreeSet::from([science]);
    let buttons = app.buttons();
    for (action, key) in [
        (Action::Research(ResearchId(18)), "E"),
        (Action::Research(ResearchId(19)), "I"),
        (Action::Research(ResearchId(20)), "T"),
        (Action::Build(UnitTypeId(124)), "C"),
        (Action::Build(UnitTypeId(125)), "P"),
        (Action::Lift, "L"),
    ] {
        let button = buttons.iter().find(|b| b.action == action).unwrap();
        assert_eq!(button.key, key, "{:?}", action);
        assert!(
            button.disabled.is_none(),
            "{:?}: {:?}",
            action,
            button.disabled
        );
    }
    let slots: BTreeSet<_> = buttons.iter().map(|b| b.slot).collect();
    assert_eq!(slots.len(), buttons.len());
    for id in [18, 19, 20] {
        app.activate(Action::Research(ResearchId(id))).unwrap();
        step(&mut app);
    }
    let caster = entity(&app.world, 112);
    let vessel = app
        .world
        .state()
        .entities
        .iter()
        .find(|e| e.id == caster)
        .unwrap();
    assert_eq!(app.world.energy_max(vessel), 250);
    assert!(
        app.world.ability_ready(vessel, AbilityId(1))
            && app.world.ability_ready(vessel, AbilityId(2))
    );
    app.selected = BTreeSet::from([caster]);
    assert_eq!(
        app.buttons()
            .iter()
            .find(|b| b.action == Action::Cast(AbilityId(1)))
            .unwrap()
            .key,
        "E"
    );
    app.activate(Action::Cast(AbilityId(2))).unwrap();
    assert_eq!(app.target_mode, Some(TargetMode::Cast(AbilityId(2))));
    app.targeting_click(Position { x: 400, y: 400 }).unwrap();
    step(&mut app);
    assert!(
        app.world
            .state()
            .entities
            .iter()
            .find(|e| e.unit_type == UnitTypeId(1))
            .unwrap()
            .ability_auras
            .iter()
            .any(|a| a.ability == AbilityId(2))
    );
    for id in [1, 2] {
        let effect = assets
            .projectiles
            .iter()
            .find(|p| p.manifest.ability == Some(AbilityId(id)))
            .unwrap();
        assert!(
            effect
                .flight
                .frames
                .iter()
                .any(|f| f.rgba.as_chunks::<4>().0.iter().any(|p| p[3] > 0))
        );
        assert!(
            media
                .audio
                .iter()
                .any(|m| m.cue == AudioCue::Ability(AbilityId(id)))
        );
    }
    app.selected = BTreeSet::from([entity(&app.world, 35)]);
    app.activate(Action::Research(ResearchId(21))).unwrap();
    step(&mut app);
    let tank = entity(&app.world, 22);
    app.selected = BTreeSet::from([tank]);
    assert_eq!(
        app.buttons()
            .iter()
            .find(|b| b.action == Action::ChangeMode)
            .unwrap()
            .key,
        "O"
    );
    app.activate(Action::ChangeMode).unwrap();
    step(&mut app);
    assert!(
        app.world
            .state()
            .entities
            .iter()
            .find(|e| e.id == tank)
            .unwrap()
            .mode_transition
            .is_some()
    );
    let duration = app
        .world
        .unit_type(
            app.world
                .state()
                .entities
                .iter()
                .find(|e| e.id == tank)
                .unwrap()
                .unit_type,
        )
        .unwrap()
        .mode
        .as_ref()
        .unwrap()
        .ticks;
    for _ in 0..=duration {
        step(&mut app);
    }
    assert_eq!(
        app.world
            .state()
            .entities
            .iter()
            .find(|e| e.id == tank)
            .unwrap()
            .unit_type,
        UnitTypeId(56)
    );
    let deployed = assets.sprite(UnitTypeId(56)).unwrap();
    assert_eq!(deployed.clip(ClipKind::Idle).unwrap().frames.len(), 32);
    for unit in [22, 56] {
        let sprite = assets.sprite(UnitTypeId(unit)).unwrap();
        let clip = sprite.clip(ClipKind::Transform).unwrap();
        assert!(clip.loop_start.is_none() && clip.frames.len() > 32);
        assert!(
            media
                .audio
                .iter()
                .any(|m| m.unit_type == Some(UnitTypeId(unit)) && m.cue == AudioCue::ChangeMode)
        );
    }
    app.activate(Action::ChangeMode).unwrap();
    step(&mut app);
    let duration = app
        .world
        .unit_type(
            app.world
                .state()
                .entities
                .iter()
                .find(|e| e.id == tank)
                .unwrap()
                .unit_type,
        )
        .unwrap()
        .mode
        .as_ref()
        .unwrap()
        .ticks;
    for _ in 0..=duration {
        step(&mut app);
    }
    assert_eq!(
        app.world
            .state()
            .entities
            .iter()
            .find(|e| e.id == tank)
            .unwrap()
            .unit_type,
        UnitTypeId(22)
    );
    app.selected = BTreeSet::from([science]);
    app.activate(Action::Lift).unwrap();
    step(&mut app);
    assert!(
        app.world
            .state()
            .entities
            .iter()
            .find(|e| e.id == science)
            .unwrap()
            .airborne
    );
    let facility = assets.sprite(UnitTypeId(123)).unwrap();
    for kind in [
        ClipKind::Lift,
        ClipKind::Land,
        ClipKind::Shadow,
        ClipKind::LiftShadow,
        ClipKind::LandShadow,
    ] {
        assert!(facility.clip(kind).is_some());
    }
}

#[test]
#[ignore = "requires STRATERUST_CAMPAIGNS pointing to a retail import"]
fn retail_trump_card_worker_can_deliver_the_emitter_to_the_enemy_beacon() {
    use straterust_engine::sim::{MissionAction, MissionCondition, MissionUnits};
    let directory = std::path::PathBuf::from(std::env::var_os("STRATERUST_CAMPAIGNS").unwrap())
        .join("terran08");
    let base = Package::load(&directory).unwrap().world(42).unwrap();
    let mut map = base.map().clone();
    let beacon = map
        .spawns
        .iter()
        .find(|s| s.unit_type == UnitTypeId(43))
        .unwrap()
        .clone();
    assert!(beacon.invincible);
    assert!(!base.unit_type(beacon.unit_type).unwrap().blocks_movement);
    let mission = map.mission.as_mut().unwrap();
    let mut delivery = mission
        .triggers
        .iter()
        .find(|t| t.actions.contains(&MissionAction::Victory))
        .unwrap()
        .clone();
    assert!(delivery.conditions.iter().any(|c| matches!(
        c,
        MissionCondition::Count {
            units: MissionUnits::Type(UnitTypeId(135)),
            location: Some(0),
            ..
        }
    )));
    // Preserve the source delivery condition; omit the cinematic waits.
    delivery.actions = vec![MissionAction::Victory];
    mission.triggers = vec![delivery];
    let start = Position {
        x: beacon.position.x - 96,
        y: beacon.position.y,
    };
    map.spawns = vec![
        Spawn {
            owner: PlayerId(0),
            unit_type: UnitTypeId(2),
            position: start,
            ..Default::default()
        },
        beacon.clone(),
        Spawn {
            owner: PlayerId(0),
            unit_type: UnitTypeId(135),
            position: start,
            ..Default::default()
        },
    ];
    map.resources.clear();
    map.ai.clear();
    map.terrain = None;
    map.fog_of_war = false;
    map.initial_explored.clear();
    let mut app = demo();
    app.world = World::new(base.rules().clone(), map, 42).unwrap();
    assert!(app.world.is_enemy(PlayerId(0), beacon.owner));
    app.selected = BTreeSet::from([EntityId(1)]);
    step(&mut app);
    assert_eq!(
        app.world
            .state()
            .entities
            .iter()
            .find(|e| e.unit_type == UnitTypeId(135))
            .unwrap()
            .carried_by,
        Some(EntityId(1))
    );
    app.contextual_order(beacon.position).unwrap();
    assert!(matches!(
        app.recorded.last().unwrap().order,
        Order::Move {
            entity: EntityId(1),
            ..
        }
    ));
    for _ in 0..100 {
        if app.world.state().winner.is_some() {
            break;
        }
        step(&mut app);
    }
    assert_eq!(app.world.state().winner, Some(PlayerId(0)));
}
