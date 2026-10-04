use super::*;
use straterust_engine::assets::{ClipFrame, SpriteClip};
use straterust_engine::sim::{Command, Map, Order, Rules, Spawn, UnitType, Weapon};

pub(super) fn world() -> World {
    World::new(
        Rules {
            id: "visual-test".into(),
            units: vec![UnitType {
                id: UnitTypeId(1),
                speed: 4,
                max_hp: 40,
                weapon: Some(Weapon {
                    cooldown_jitter: None,
                    targets_air: false,
                    damage_kind: Default::default(),
                    splash: None,
                    strikes: Vec::new(),
                    damage: 6,
                    range: 20,
                    cooldown: 5,
                }),
                ..UnitType::default()
            }],
            ..Rules::default()
        },
        Map {
            initial_explored: Default::default(),
            creation: Default::default(),
            ai: Vec::new(),
            mission: None,
            fog_of_war: false,
            id: "visual-test".into(),
            width: 128,
            height: 128,
            players: 2,
            spawns: vec![
                Spawn {
                    owner: PlayerId(0),
                    unit_type: UnitTypeId(1),
                    position: Position { x: 40, y: 40 },
                    ..Spawn::default()
                },
                Spawn {
                    owner: PlayerId(1),
                    unit_type: UnitTypeId(1),
                    position: Position { x: 100, y: 40 },
                    ..Spawn::default()
                },
            ],
            resources: vec![],
            start_locations: vec![],
            terrain: None,
        },
        42,
    )
    .unwrap()
}

#[test]
fn projectile_flight_reaches_a_fixed_target_then_expires() {
    use straterust_engine::assets::{Effect, EffectManifest, ProjectileManifest};
    let manifest = EffectManifest {
        frame_ms: 42,
        anchor: [0, 0],
        frames: vec![],
        sequence: vec![0],
    };
    let animation = || Effect {
        frame_ms: 42,
        anchor: [0, 0],
        frames: vec![Image {
            width: 1,
            height: 1,
            rgba: vec![255; 4],
        }],
        sequence: vec![0],
    };
    let mut effect = Projectile {
        manifest: ProjectileManifest {
            targets_air: false,
            directional: false,
            unit_type: UnitTypeId(1),
            speed_fp8: 2560,
            forward_offset: 20,
            arc_height: 0,
            on_target: false,
            flight: manifest.clone(),
            impact: manifest,
        },
        flight: animation(),
        impact: animation(),
    };
    let mut shot = ProjectileVisual {
        targets_air: false,
        unit_type: UnitTypeId(1),
        owner: PlayerId(0),
        from: Position { x: 40, y: 40 },
        to: Position { x: 100, y: 40 },
        elapsed: Duration::ZERO,
        tick_ms: 42,
    };
    assert!(
        shot.sample(&effect).is_none(),
        "attack startup is not a muzzle flash"
    );
    shot.elapsed = Duration::from_millis(42);
    assert_eq!(shot.sample(&effect).unwrap().1, [60.0, 40.0]);
    shot.elapsed = Duration::from_millis(126);
    assert_eq!(shot.sample(&effect).unwrap().1, [80.0, 40.0]);
    effect.manifest.arc_height = 24;
    assert_eq!(shot.sample(&effect).unwrap().1, [80.0, 16.0]);
    shot.elapsed = Duration::from_millis(210);
    assert_eq!(shot.sample(&effect).unwrap().1, [100.0, 40.0]);
    shot.elapsed = Duration::from_millis(252);
    assert!(
        shot.sample(&effect).is_none(),
        "impact finishes instead of looping"
    );
}

#[test]
fn attack_animation_never_targets_hidden_enemies_even_with_an_old_cooldown() {
    let original = world();
    let mut map = original.map().clone();
    map.spawns[1].position = Position { x: 52, y: 40 };
    map.spawns[1].cloaked = true;
    let world = World::new(original.rules().clone(), map, 42).unwrap();
    let visuals = Visuals::new(&world);
    let mut actor = world.state().entities[0].clone();
    actor.cooldown = 4;
    for order in [
        UnitOrder::Idle,
        UnitOrder::Hold,
        UnitOrder::Attack {
            target: EntityId(2),
        },
        UnitOrder::AttackMove {
            target: Position { x: 100, y: 100 },
        },
    ] {
        actor.order = order;
        for target in [None, Some(EntityId(2))] {
            actor.auto_attack_target = target;
            assert_eq!(visuals.attack_target(&world, &actor), None);
        }
    }
    let mut map = world.map().clone();
    map.spawns[1].cloaked = false;
    let visible = World::new(world.rules().clone(), map, 42).unwrap();
    assert_eq!(
        Visuals::new(&visible).attack_target(&visible, &actor),
        Some(Position { x: 52, y: 40 })
    );
}

#[test]
fn flight_and_mine_clips_follow_authoritative_transition_progress() {
    use straterust_engine::assets::{AssetManifest, ImageRef};
    use straterust_engine::sim::{Flight, MineState, MineStats};
    let reference = ImageRef {
        file: "synthetic.srim".into(),
        blake3: "0".repeat(64),
    };
    let clip = |kind, frames: &[u16], offsets: &[i16]| SpriteClip {
        key_steps: Vec::new(),
        kind,
        directions: 1,
        frame_ms: 42,
        frames: frames
            .iter()
            .enumerate()
            .map(|(i, &frame)| ClipFrame {
                frame,
                flip_x: false,
                offset: [0, offsets[i]],
            })
            .collect(),
    };
    let assets = AssetPack {
        manifest: AssetManifest {
            schema_version: 1,
            terrain: reference.clone(),
            terrain_grid: None,
            unit_type: UnitTypeId(1),
            unit_name: "Transitions".into(),
            frame_ms: 42,
            anchor: [0, 0],
            frames: vec![reference; 7],
            clips: vec![
                clip(ClipKind::Lift, &[0, 1], &[0, -42]),
                clip(ClipKind::Airborne, &[2], &[-42]),
                clip(ClipKind::Land, &[3, 4], &[-42, 0]),
                clip(ClipKind::Conceal, &[5, 6], &[0, 0]),
                clip(ClipKind::Reveal, &[6, 5], &[0, 0]),
                clip(ClipKind::Idle, &[5], &[0]),
                clip(ClipKind::Walk, &[6], &[0]),
            ],
            extra_units: vec![],
            resources: vec![],
            carried_resources: Vec::new(),
            ui: vec![],
            map_images: vec![],
            scan_effect: None,
            projectiles: Vec::new(),
            damage_effects: None,
            gas_effects: None,
            creep: None,
            indicators: None,
        },
        terrain: Image {
            width: 1,
            height: 1,
            rgba: vec![0; 4],
        },
        frames: (0..7)
            .map(|red| Image {
                width: 1,
                height: 1,
                rgba: vec![red, 0, 0, 255],
            })
            .collect(),
        extra_units: vec![],
        resources: vec![],
        carried_resources: Vec::new(),
        ui: vec![],
        map_images: vec![],
        scan_effect: None,
        projectiles: Vec::new(),
        damage_effects: None,
        gas_effects: None,
        creep: None,
        indicators: None,
    };
    let base = world();
    let mut rules = base.rules().clone();
    rules.units[0].structure = true;
    rules.units[0].speed = 0;
    rules.units[0].flight = Some(Flight {
        speed: 4,
        lift_ticks: 2,
        land_ticks: 2,
    });
    let flight_world = World::new(rules, base.map().clone(), 42).unwrap();
    let hash = flight_world.state_hash();
    let mut entity = flight_world.state().entities[0].clone();
    entity.airborne = true;
    for (remaining, expected, anchor) in [(2, 0, 0), (1, 1, 42), (0, 2, 42)] {
        entity.flight_transition = remaining;
        let image = unit_image(&assets, &entity, None, &flight_world).unwrap();
        assert_eq!((image.image.rgba[0], image.anchor[1]), (expected, anchor));
    }
    entity.order = UnitOrder::Land {
        target: entity.position,
    };
    entity.flight_transition = 1;
    assert_eq!(
        unit_image(&assets, &entity, None, &flight_world)
            .unwrap()
            .image
            .rgba[0],
        4
    );
    assert_eq!(flight_world.state_hash(), hash);
    let mut rules = base.rules().clone();
    rules.units[0].weapon.as_mut().unwrap().splash = Some([10, 20, 30]);
    rules.units[0].mine = Some(MineStats {
        arm_ticks: 60,
        conceal_ticks: 2,
        reveal_ticks: 2,
        trigger_range: 20,
        chase_range: 60,
        detonation_range: 4,
    });
    let mine_world = World::new(rules, base.map().clone(), 42).unwrap();
    let mut entity = mine_world.state().entities[0].clone();
    for (phase, remaining, burrowed, expected) in [
        (MinePhase::Concealing, 2, false, 5),
        (MinePhase::Concealing, 1, false, 6),
        (MinePhase::Armed, 0, true, 6),
        (MinePhase::Emerging, 2, false, 6),
        (MinePhase::Emerging, 1, false, 5),
    ] {
        entity.cloaked = burrowed;
        entity.mine_state = Some(MineState {
            phase,
            remaining,
            target: None,
        });
        assert_eq!(
            unit_image(&assets, &entity, None, &mine_world)
                .unwrap()
                .image
                .rgba[0],
            expected
        );
    }
}

#[test]
fn motion_drives_walk_and_stopped_units_retain_facing_without_changing_world() {
    let mut world = world();
    let mut visuals = Visuals::new(&world);
    let initial = world.state_hash();
    visuals.update(&world);
    assert_eq!(world.state_hash(), initial);
    assert_eq!(visuals.get(EntityId(1)).unwrap().action, VisualAction::Idle);
    world
        .step(&[Command {
            tick: world.tick(),
            player: PlayerId(0),
            sequence: 1,
            order: Order::Move {
                entity: EntityId(1),
                target: Position { x: 20, y: 40 },
            },
        }])
        .unwrap();
    visuals.update(&world);
    assert_eq!(visuals.get(EntityId(1)).unwrap().action, VisualAction::Move);
    assert_eq!(visuals.get(EntityId(1)).unwrap().facing, 24);
    for _ in 0..6 {
        world.step(&[]).unwrap();
        visuals.update(&world);
    }
    assert_eq!(visuals.get(EntityId(1)).unwrap().action, VisualAction::Idle);
    assert_eq!(visuals.get(EntityId(1)).unwrap().facing, 24);
}

#[test]
fn walking_heading_debounce_filters_small_turns_but_keeps_corners_responsive() {
    let original = world();
    let mut rules = original.rules().clone();
    rules.units[0].speed = 8;
    let mut map = original.map().clone();
    map.width = 1024;
    map.height = 1024;
    map.spawns[0].position = Position { x: 300, y: 300 };
    let mut world = World::new(rules, map, 42).unwrap();
    let mut visuals = Visuals::new(&world);
    let mut unfiltered = Visuals::new(&world);
    unfiltered.movement_heading_debounce_ms = 0;
    let move_to = |world: &mut World, x, y| {
        world
            .step(&[Command {
                tick: world.tick(),
                player: PlayerId(0),
                sequence: world.state().last_sequences[0] + 1,
                order: Order::Move {
                    entity: EntityId(1),
                    target: Position { x, y },
                },
            }])
            .unwrap();
    };
    move_to(&mut world, 900, 200);
    visuals.update(&world);
    unfiltered.update(&world);
    let first = visuals.get(EntityId(1)).unwrap().facing;
    for index in 0..9 {
        move_to(&mut world, 900, if index % 2 == 0 { 400 } else { 200 });
        let hash = world.state_hash();
        visuals.update(&world);
        unfiltered.update(&world);
        assert_eq!(world.state_hash(), hash);
        assert_eq!(visuals.get(EntityId(1)).unwrap().facing, first);
        if index % 2 == 0 {
            assert_ne!(unfiltered.get(EntityId(1)).unwrap().facing, first);
        }
    }
    move_to(&mut world, 900, 400);
    visuals.update(&world);
    assert_ne!(visuals.get(EntityId(1)).unwrap().facing, first);
    let x = world.state().entities[0].position.x;
    move_to(&mut world, x, 900);
    visuals.update(&world);
    assert_eq!(visuals.get(EntityId(1)).unwrap().facing, 16);
}

#[test]
fn actual_shots_and_damage_trigger_feedback_but_walking_does_not() {
    let mut world = world();
    let mut visuals = Visuals::new(&world);
    world
        .step(&[Command {
            tick: world.tick(),
            player: PlayerId(0),
            sequence: 1,
            order: Order::Attack {
                entity: EntityId(1),
                target: EntityId(2),
            },
        }])
        .unwrap();
    visuals.update(&world);
    assert_eq!(visuals.get(EntityId(1)).unwrap().action, VisualAction::Move);
    assert_eq!(visuals.get(EntityId(1)).unwrap().shot_tick, None);
    for _ in 0..30 {
        world.step(&[]).unwrap();
        visuals.update(&world);
        if visuals.get(EntityId(1)).unwrap().shot_tick.is_some() {
            break;
        }
    }
    let marine = visuals.get(EntityId(1)).unwrap();
    assert_eq!(marine.action, VisualAction::Attack);
    assert_eq!(marine.facing, 8);
    assert!(marine.shot_tick.is_some());
    assert!(visuals.get(EntityId(2)).unwrap().hit_tick.is_some());
}

#[test]
fn jitter_stim_and_delayed_strikes_start_attack_art_once_per_attack() {
    use straterust_engine::sim::{Research, ResearchEffect, ResearchId, WeaponStrike};
    for (jitter, stimulated) in [(-1, false), (2, false), (2, true)] {
        let baseline = world();
        let mut rules = baseline.rules().clone();
        rules.units[0].max_hp = 1000;
        let weapon = rules.units[0].weapon.as_mut().unwrap();
        weapon.cooldown = 15;
        weapon.cooldown_jitter = Some([jitter, jitter]);
        weapon.strikes = vec![WeaponStrike {
            delay: 3,
            forward: 0,
        }];
        let mut target = rules.units[0].clone();
        target.id = UnitTypeId(2);
        target.weapon = None;
        rules.units.push(target);
        rules.units.push(UnitType {
            id: UnitTypeId(3),
            structure: true,
            speed: 0,
            ..UnitType::default()
        });
        rules.research = vec![Research {
            id: ResearchId(1),
            facility: UnitTypeId(3),
            cost: vec![],
            ticks: 1,
            effect: ResearchEffect::Stim {
                units: vec![UnitTypeId(1)],
                hp_cost: 1,
                duration_ticks: 100,
            },
        }];
        let mut map = baseline.map().clone();
        map.spawns[1].unit_type = UnitTypeId(2);
        map.spawns.push(Spawn {
            owner: PlayerId(0),
            unit_type: UnitTypeId(3),
            position: Position { x: 20, y: 100 },
            ..Spawn::default()
        });
        let mut world = World::new(rules, map, 42).unwrap();
        if stimulated {
            world
                .step(&[Command {
                    tick: world.tick(),
                    player: PlayerId(0),
                    sequence: 1,
                    order: Order::Research {
                        entity: EntityId(3),
                        research: ResearchId(1),
                    },
                }])
                .unwrap();
            for _ in 0..3 {
                world.step(&[]).unwrap();
            }
            assert!(world.has_research(PlayerId(0), ResearchId(1)));
            assert!(
                world
                    .step(&[Command {
                        tick: world.tick(),
                        player: PlayerId(0),
                        sequence: 2,
                        order: Order::Stim {
                            entity: EntityId(1)
                        }
                    }])
                    .unwrap()[0]
                    .rejection
                    .is_none()
            );
        }
        let mut visuals = Visuals::new(&world);
        world
            .step(&[Command {
                tick: world.tick(),
                player: PlayerId(0),
                sequence: 3,
                order: Order::Attack {
                    entity: EntityId(1),
                    target: EntityId(2),
                },
            }])
            .unwrap();
        visuals.update(&world);
        for _ in 0..30 {
            if visuals.get(EntityId(1)).unwrap().shot_tick.is_some() {
                break;
            }
            world.step(&[]).unwrap();
            visuals.update(&world);
        }
        let first = visuals
            .get(EntityId(1))
            .unwrap()
            .shot_tick
            .expect("observed first attack");
        assert_eq!(
            first,
            world.tick().0,
            "jitter must not delay the first animation frame"
        );
        assert!(
            !world.state().entities[0].strikes.is_empty(),
            "art starts during wind-up"
        );
        for _ in 0..4 {
            world.step(&[]).unwrap();
            visuals.update(&world);
            assert_eq!(
                visuals.get(EntityId(1)).unwrap().shot_tick,
                Some(first),
                "countdown and strike damage must not restart art"
            );
        }
        assert!(world.state().entities[1].hp < 1000, "delayed strike landed");
    }
}

#[test]
fn cardinal_and_diagonal_directions_are_stable() {
    let origin = Position { x: 10, y: 10 };
    for (x, y, expected) in [
        (10, 0, 0),
        (20, 0, 4),
        (20, 10, 8),
        (10, 20, 16),
        (0, 10, 24),
        (0, 0, 28),
    ] {
        assert_eq!(facing_between(origin, Position { x, y }), expected);
    }
}

#[test]
fn final_kill_keeps_finite_death_art_after_removal_and_while_paused() {
    let baseline = world();
    let mut rules = baseline.rules().clone();
    rules.victory = true;
    rules.units[0].weapon = Some(Weapon {
        cooldown_jitter: None,
        targets_air: false,
        damage_kind: Default::default(),
        splash: None,
        strikes: Vec::new(),
        damage: 100,
        range: 1000,
        cooldown: 10,
    });
    let mut victim = rules.units[0].clone();
    victim.id = UnitTypeId(2);
    victim.weapon = None;
    rules.units.push(victim);
    let mut map = baseline.map().clone();
    map.spawns[1].unit_type = UnitTypeId(2);
    let mut world = World::new(rules, map, 42).unwrap();
    let mut visuals = Visuals::new(&world);
    let mut cancelled = Visuals::new(&world);
    cancelled.forget_entity(EntityId(2));
    world.step(&[]).unwrap();
    assert_eq!(world.state().winner, Some(PlayerId(0)));
    let hash = world.state_hash();
    visuals.update(&world);
    cancelled.update(&world);
    assert!(
        cancelled.deaths().is_empty(),
        "cancelled removal is not death"
    );
    assert!(visuals.get(EntityId(2)).is_none());
    assert_eq!(visuals.deaths().len(), 1);
    assert_eq!(visuals.deaths()[0].position, Position { x: 100, y: 40 });
    visuals.update(&world);
    assert_eq!(
        visuals.deaths().len(),
        1,
        "redrawing cannot duplicate a death"
    );
    visuals.advance_effects(Duration::from_millis(300), None);
    assert_eq!(visuals.deaths()[0].elapsed.as_millis(), 300);
    visuals.advance_effects(Duration::from_millis(300), None);
    assert!(visuals.deaths().is_empty());
    assert_eq!(
        world.state_hash(),
        hash,
        "paused presentation never changes gameplay"
    );
    world.step(&[]).unwrap();
    visuals.update(&world);
    assert!(
        visuals.deaths().is_empty(),
        "frozen victory does not repeat effects"
    );
}

#[test]
fn death_clip_plays_once_without_idle_fallback_or_looping() {
    let images: Vec<_> = (0..3)
        .map(|red| Image {
            width: 1,
            height: 1,
            rgba: vec![red, 0, 0, 255],
        })
        .collect();
    let clips = vec![SpriteClip {
        key_steps: Vec::new(),
        kind: ClipKind::Death,
        directions: 1,
        frame_ms: 100,
        frames: (0..3)
            .map(|frame| ClipFrame {
                frame,
                flip_x: false,
                offset: [0, 0],
            })
            .collect(),
    }];
    let sprite = SpriteRef {
        name: "death",
        frame_ms: 100,
        anchor: [0, 0],
        frames: &images,
        clips: &clips,
    };
    for (ms, red) in [(0, 0), (100, 1), (299, 2)] {
        assert_eq!(
            sample(&sprite, ClipKind::Death, 24, ms, None)
                .unwrap()
                .image
                .rgba[0],
            red
        );
    }
    assert!(sample(&sprite, ClipKind::Death, 24, 300, None).is_none());
    assert!(sample(&sprite, ClipKind::Death, 24, 10000, None).is_none());
}

#[test]
fn state_clips_hold_idle_mirror_directions_and_do_not_loop_an_attack() {
    let images: Vec<_> = (0..5)
        .map(|color| Image {
            width: 2,
            height: 1,
            rgba: vec![color, 0, 0, 255, color, 0, 0, 255],
        })
        .collect();
    let directional = |kind, frames: &[u16]| SpriteClip {
        key_steps: Vec::new(),
        kind,
        directions: 32,
        frame_ms: 100,
        frames: frames
            .iter()
            .flat_map(|frame| {
                (0..32).map(move |direction| ClipFrame {
                    frame: *frame,
                    flip_x: direction > 16,
                    offset: [0, 0],
                })
            })
            .collect(),
    };
    let clips = vec![
        directional(ClipKind::Idle, &[0]),
        directional(ClipKind::Walk, &[1, 2]),
        directional(ClipKind::Attack, &[3, 4]),
    ];
    let sprite = SpriteRef {
        name: "test",
        frame_ms: 100,
        anchor: [0, 0],
        frames: &images,
        clips: &clips,
    };
    let idle = sample(&sprite, ClipKind::Idle, 24, 10000, None).unwrap();
    assert_eq!(idle.image.rgba[0], 0);
    assert_eq!(idle.anchor, [2, 0]);
    assert!(idle.flip_x);
    assert!(!sample(&sprite, ClipKind::Idle, 8, 0, None).unwrap().flip_x);
    assert_eq!(
        sample(&sprite, ClipKind::Walk, 8, 0, None)
            .unwrap()
            .image
            .rgba[0],
        1
    );
    assert_eq!(
        sample(&sprite, ClipKind::Walk, 8, 100, None)
            .unwrap()
            .image
            .rgba[0],
        2
    );
    assert_eq!(
        sample(&sprite, ClipKind::Walk, 8, 200, None)
            .unwrap()
            .image
            .rgba[0],
        1
    );
    assert_eq!(
        sample(&sprite, ClipKind::Attack, 8, 0, None)
            .unwrap()
            .image
            .rgba[0],
        3
    );
    assert_eq!(
        sample(&sprite, ClipKind::Attack, 8, 100, None)
            .unwrap()
            .image
            .rgba[0],
        4
    );
    assert_eq!(
        sample(&sprite, ClipKind::Attack, 8, 200, None)
            .unwrap()
            .image
            .rgba[0],
        0
    );
}

#[test]
fn incomplete_buildings_never_use_the_completed_state() {
    let mut building = world().state().entities[0].clone();
    for (remaining, expected) in [(100, 0), (81, 0), (80, 1), (60, 2), (40, 3), (1, 3)] {
        building.construction = Some(straterust_engine::sim::Construction {
            worker: None,
            remaining,
            total: 100,
            work_position: None,
            work_ticks: 0,
        });
        assert_eq!(construction_stage(&building), Some(expected));
    }
    building.construction = None;
    assert_eq!(construction_stage(&building), None);
}

#[test]
fn idle_building_clips_loop_and_effect_offsets_are_applied_after_mirroring() {
    let images: Vec<_> = (0..2)
        .map(|red| Image {
            width: 8,
            height: 8,
            rgba: [red, 0, 0, 255].repeat(64),
        })
        .collect();
    let clips = vec![
        SpriteClip {
            key_steps: Vec::new(),
            kind: ClipKind::Idle,
            directions: 1,
            frame_ms: 100,
            frames: vec![
                ClipFrame {
                    frame: 0,
                    flip_x: false,
                    offset: [0, 0],
                },
                ClipFrame {
                    frame: 1,
                    flip_x: false,
                    offset: [0, 0],
                },
            ],
        },
        SpriteClip {
            key_steps: Vec::new(),
            kind: ClipKind::WorkEffect,
            directions: 1,
            frame_ms: 100,
            frames: vec![ClipFrame {
                frame: 1,
                flip_x: true,
                offset: [-20, 5],
            }],
        },
    ];
    let sprite = SpriteRef {
        name: "test",
        frame_ms: 100,
        anchor: [3, 4],
        frames: &images,
        clips: &clips,
    };
    assert_eq!(
        sample(&sprite, ClipKind::Idle, 0, 0, None)
            .unwrap()
            .image
            .rgba[0],
        0
    );
    assert_eq!(
        sample(&sprite, ClipKind::Idle, 0, 100, None)
            .unwrap()
            .image
            .rgba[0],
        1
    );
    assert_eq!(
        sample(&sprite, ClipKind::Idle, 0, 200, None)
            .unwrap()
            .image
            .rgba[0],
        0
    );
    let effect = sample(&sprite, ClipKind::WorkEffect, 24, 0, None).unwrap();
    assert_eq!(
        effect.anchor,
        [25, -1],
        "mirrored anchor [5,4] minus resolved offset [-20,5]"
    );
}

#[test]
fn production_activity_starts_and_stops_with_actual_training() {
    let package = straterust_engine::content::Package::load(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/terran-demo"),
    )
    .unwrap();
    for blocked in [false, true] {
        let baseline = package.world(42).unwrap();
        let mut rules = baseline.rules().clone();
        if blocked {
            rules
                .units
                .iter_mut()
                .find(|unit| unit.id == UnitTypeId(3))
                .unwrap()
                .supply_provided = 1;
        }
        let mut world = World::new(rules, baseline.map().clone(), 42).unwrap();
        let mut visuals = Visuals::new(&world);
        let command = Command {
            tick: world.tick(),
            player: PlayerId(0),
            sequence: 1,
            order: Order::Train {
                entity: EntityId(1),
                unit_type: UnitTypeId(2),
            },
        };
        assert!(world.step(&[command]).unwrap()[0].rejection.is_none());
        visuals.update(&world);
        assert_eq!(
            visuals.get(EntityId(1)).unwrap().action,
            if blocked {
                VisualAction::Idle
            } else {
                VisualAction::Production
            }
        );
        if !blocked {
            for _ in 0..400 {
                world.step(&[]).unwrap();
                visuals.update(&world);
            }
            assert!(
                world
                    .state()
                    .entities
                    .iter()
                    .find(|entity| entity.id == EntityId(1))
                    .unwrap()
                    .production
                    .is_empty()
            );
            assert_eq!(visuals.get(EntityId(1)).unwrap().action, VisualAction::Idle);
        }
    }
}

#[test]
fn gathering_only_works_at_resource_and_stopping_returns_to_idle() {
    let package = straterust_engine::content::Package::load(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/terran-demo"),
    )
    .unwrap();
    let mut world = package.world(42).unwrap();
    let mut visuals = Visuals::new(&world);
    world
        .step(&[Command {
            tick: world.tick(),
            player: PlayerId(0),
            sequence: 1,
            order: Order::Gather {
                entity: EntityId(2),
                resource: world.state().resources[0].id,
            },
        }])
        .unwrap();
    visuals.update(&world);
    assert_eq!(visuals.get(EntityId(2)).unwrap().action, VisualAction::Move);
    for _ in 0..200 {
        world.step(&[]).unwrap();
        visuals.update(&world);
        if visuals.get(EntityId(2)).unwrap().action == VisualAction::Work {
            break;
        }
    }
    assert_eq!(visuals.get(EntityId(2)).unwrap().action, VisualAction::Work);
    let UnitOrder::Gather { resource } = world.state().entities[1].order else {
        panic!("expected gathering");
    };
    assert_eq!(
        visuals.get(EntityId(2)).unwrap().effect_target,
        Some(
            world
                .state()
                .resources
                .iter()
                .find(|node| node.id == resource)
                .unwrap()
                .position
        )
    );
    world
        .step(&[Command {
            tick: world.tick(),
            player: PlayerId(0),
            sequence: 2,
            order: Order::Stop {
                entity: EntityId(2),
            },
        }])
        .unwrap();
    visuals.update(&world);
    assert_eq!(visuals.get(EntityId(2)).unwrap().action, VisualAction::Idle);
    assert_eq!(visuals.get(EntityId(2)).unwrap().effect_target, None);
}
