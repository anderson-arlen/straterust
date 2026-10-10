use super::tests::world;
use super::*;
use straterust_engine::assets::{ClipFrame, SpriteClip};
use straterust_engine::sim::MovementClass;

#[test]
#[ignore = "requires STRATERUST_CAMPAIGNS pointing to a refreshed retail import"]
fn retail_missile_turret_and_goliath_shots_reach_air_targets_and_play_explosions() {
    use crate::view::{Camera, Presentation, View};
    use straterust_engine::{
        content::Package,
        sim::{Command, Order, Spawn, Visibility},
    };
    let directory = std::path::PathBuf::from(std::env::var_os("STRATERUST_CAMPAIGNS").unwrap())
        .join("terran06");
    let base = Package::load(&directory).unwrap().world(42).unwrap();
    let assets = AssetPack::load(&directory).unwrap().unwrap();
    let presentation: Presentation =
        ron::de::from_bytes(&std::fs::read(directory.join("presentation.ron")).unwrap()).unwrap();
    let mut rules = base.rules().clone();
    rules.victory = false;
    for target in [2, 23, 28, 29, 60, 61, 62] {
        let victim = rules
            .units
            .iter_mut()
            .find(|u| u.id == UnitTypeId(target))
            .unwrap();
        victim.weapon = None;
        victim.air_weapon = None;
        victim.acquisition_range = None;
    }
    let mut map = base.map().clone();
    map.terrain = None;
    map.mission = None;
    map.ai.clear();
    map.resources.clear();
    map.creation.clear();
    map.start_locations.clear();
    map.initial_explored.clear();
    map.fog_of_war = false;
    map.width = 768;
    map.height = 512;
    map.players = 3;
    let from = Position { x: 256, y: 256 };
    for (actor, owner, target, cliff) in [
        (36, 0, 23, false),
        (21, 0, 23, false),
        (21, 1, 23, false),
        (21, 0, 28, false),
        (21, 0, 29, false),
        (21, 0, 60, false),
        (21, 0, 61, false),
        (21, 0, 62, false),
        (21, 0, 29, true),
        (21, 1, 29, true),
    ] {
        let to = Position {
            x: if target == 62 { 416 } else { 448 },
            y: 256,
        };
        map.fog_of_war = cliff;
        map.terrain = cliff.then(|| straterust_engine::map::Terrain {
            cell_size: 32,
            columns: 24,
            rows: 16,
            flags: (0..24 * 16)
                .map(|i| {
                    straterust_engine::map::WALKABLE
                        | if i % 24 >= to.x / 32 {
                            1 << straterust_engine::map::HEIGHT_SHIFT
                        } else {
                            0
                        }
                })
                .collect(),
        });
        map.spawns = [
            (owner, actor, from),
            (if cliff { 2 } else { 1 - owner }, target, to),
        ]
        .map(|(owner, unit_type, position)| Spawn {
            owner: PlayerId(owner),
            unit_type: UnitTypeId(unit_type),
            position,
            ..Default::default()
        })
        .to_vec();
        if cliff && owner != 0 {
            map.spawns.push(Spawn {
                owner: PlayerId(0),
                unit_type: UnitTypeId(2),
                position: Position {
                    x: from.x,
                    y: from.y - 64,
                },
                ..Default::default()
            });
        }
        let mut server = World::new(rules.clone(), map.clone(), 42).unwrap();
        if cliff {
            assert_eq!(server.visibility(PlayerId(0), to), Visibility::Unexplored);
            assert!(
                server.entity_visible(PlayerId(0), EntityId(2)),
                "flyer is visible above cliff"
            );
        }
        let initial = server
            .player_view(PlayerId(0))
            .unwrap()
            .into_world(&server)
            .unwrap();
        let mut visuals = Visuals::new(&initial);
        let outcomes = server
            .step(&[Command {
                tick: server.tick(),
                player: PlayerId(owner),
                sequence: 1,
                order: Order::Attack {
                    entity: EntityId(1),
                    target: EntityId(2),
                },
            }])
            .unwrap();
        assert!(outcomes[0].rejection.is_none());
        let firing = server
            .player_view(PlayerId(0))
            .unwrap()
            .into_world(&server)
            .unwrap();
        visuals.update(&firing);
        assert_eq!(
            visuals.projectiles().len(),
            1,
            "actor {actor}, owner {owner}, target {target}, cliff {cliff}"
        );
        let missile = assets.projectile_for(UnitTypeId(actor), true).unwrap();
        if actor == 36 {
            assert!(assets.projectile_for(UnitTypeId(actor), false).is_none());
        }
        let shot = &mut visuals.projectiles[0];
        assert!(shot.targets_air);
        assert_eq!((shot.from, shot.to), (from, to));
        let flight_ms = shot.flight_ms(missile);
        shot.elapsed =
            Duration::from_secs_f64((f64::from(shot.tick_ms) + flight_ms / 2.0) / 1000.0);
        let (frame, position) = shot.sample(missile).unwrap();
        assert!(position[0] > f64::from(from.x) && position[0] < f64::from(to.x));
        assert!(frame.image.rgba.as_chunks::<4>().0.iter().any(|p| p[3] > 0));
        let impact_phase = flight_ms.max(210.0) + 1.0;
        shot.elapsed = Duration::from_secs_f64((f64::from(shot.tick_ms) + impact_phase) / 1000.0);
        let (frame, position) = shot.sample(missile).unwrap();
        assert_eq!(position, [f64::from(to.x), f64::from(to.y)]);
        assert!(frame.image.rgba.as_chunks::<4>().0.iter().any(|p| p[3] > 0));
        let smoke = shot.trail_samples(missile);
        let visible_smoke: Vec<_> = smoke
            .iter()
            .filter(|(frame, _)| frame.image.rgba.as_chunks::<4>().0.iter().any(|p| p[3] > 0))
            .collect();
        assert!(
            !visible_smoke.is_empty(),
            "source smoke continues after impact"
        );
        assert!(
            visible_smoke
                .iter()
                .all(|(_, p)| p[0] > f64::from(from.x) && p[0] < f64::from(to.x))
        );
        // Sampling alone does not establish that the world renderer submits
        // the pictures. Check both flight and impact/trail drawing commands.
        for phase in [flight_ms / 2.0, impact_phase] {
            visuals.projectiles[0].elapsed =
                Duration::from_secs_f64((f64::from(firing.rules().tick_ms) + phase) / 1000.0);
            let selected = std::collections::BTreeSet::new();
            let scene = View {
                world: &firing,
                visuals: &visuals,
                presentation: &presentation,
                assets: Some(&assets),
                map_art: None,
                media: None,
                mission: None,
                speaking: None,
                camera: Camera {
                    x: 352.0,
                    y: 256.0,
                    viewport: None,
                    zoom: 2.0,
                },
                cursor: [-1.0; 2],
                targeting: false,
                selected: &selected,
                selected_resource: None,
                drag_box: None,
                paused: false,
                playback: false,
                animation_ms: 42,
                portrait_ms: 0,
                status: "",
                help: "",
                buttons: &[],
                placement: None,
                placement_type: None,
                ending_hint: "",
            }
            .scene(768, 512, 1.0);
            let shot = &visuals.projectiles[0];
            for (frame, _) in shot
                .trail_samples(missile)
                .into_iter()
                .chain(shot.sample(missile))
            {
                assert!(
                    scene.commands.iter().any(|command| matches!(command,
                        crate::gpu::Draw::Image { image, .. } if std::ptr::eq(*image, frame.image)
                    )),
                    "{actor}, owner {owner}: missing projectile draw at {phase}ms"
                );
            }
        }
        visuals.advance_effects(Duration::from_secs(1), Some(&assets));
        assert!(visuals.projectiles().is_empty());
    }
}

#[test]
fn brief_attack_poses_survive_skipped_redraws_and_are_consumed_once() {
    for draws in [vec![1, 2, 3, 4, 5, 6, 7, 8], vec![1, 3, 5, 7, 8], vec![8]] {
        let base = world();
        let mut rules = base.rules().clone();
        let weapon = rules.units[0].weapon.as_mut().unwrap();
        weapon.range = 80;
        weapon.cooldown = 100;
        weapon.damage = 1;
        let mut victim = rules.units[0].clone();
        victim.id = UnitTypeId(2);
        victim.weapon = None;
        rules.units.push(victim);
        let mut map = base.map().clone();
        map.spawns[1].unit_type = UnitTypeId(2);
        let mut world = World::new(rules, map, 42).unwrap();
        let mut visuals = Visuals::new(&world);
        let images: Vec<_> = (0..3)
            .map(|red| Image {
                width: 1,
                height: 1,
                rgba: vec![red, 0, 0, 255],
            })
            .collect();
        let clips = [
            SpriteClip {
                kind: ClipKind::Attack,
                directions: 1,
                frame_ms: world.rules().tick_ms,
                frames: [1, 2, 1, 2, 1, 2, 1]
                    .into_iter()
                    .map(|frame| ClipFrame {
                        frame,
                        flip_x: false,
                        offset: [0, 0],
                    })
                    .collect(),
                key_steps: vec![1, 3, 5],
                loop_start: None,
                progress_starts: vec![],
            },
            SpriteClip {
                kind: ClipKind::Idle,
                directions: 1,
                frame_ms: 42,
                frames: vec![ClipFrame {
                    frame: 0,
                    flip_x: false,
                    offset: [0, 0],
                }],
                key_steps: vec![],
                loop_start: None,
                progress_starts: vec![],
            },
        ];
        let sprite = SpriteRef {
            name: "brief shot",
            frame_ms: 42,
            anchor: [0, 0],
            frames: &images,
            clips: &clips,
        };
        let mut flashes = 0;
        for tick in 1..=8 {
            world.step(&[]).unwrap();
            visuals.update(&world);
            if draws.contains(&tick) {
                let (kind, phase) = action_clip(&sprite, visuals.get(EntityId(1)), &world);
                flashes +=
                    usize::from(sample(&sprite, kind, 8, phase, None).unwrap().image.rgba[0] == 2);
                visuals.mark_rendered();
            }
        }
        assert_eq!(flashes, if draws.len() == 1 { 1 } else { 3 }, "{draws:?}");
        let (kind, phase) = action_clip(&sprite, visuals.get(EntityId(1)), &world);
        assert_eq!(
            sample(&sprite, kind, 8, phase, None).unwrap().image.rgba[0],
            0,
            "completed flash must not replay on the next draw at the same tick"
        );
        assert_eq!(world.state().entities[1].hp, 39);
    }
}

#[test]
fn committed_shot_finishes_flash_after_target_dies_without_repeating_or_blocking_movement() {
    let base = world();
    let mut rules = base.rules().clone();
    let weapon = rules.units[0].weapon.as_mut().unwrap();
    weapon.range = 80;
    weapon.damage = 100;
    let mut victim = rules.units[0].clone();
    victim.id = UnitTypeId(2);
    victim.weapon = None;
    rules.units.push(victim);
    let mut map = base.map().clone();
    map.width = 256;
    map.height = 256;
    map.spawns[1].unit_type = UnitTypeId(2);
    let mut reserve = map.spawns[1].clone();
    reserve.position = Position { x: 220, y: 180 };
    map.spawns.push(reserve);
    let mut world = World::new(rules, map, 42).unwrap();
    let mut visuals = Visuals::new(&world);
    let images: Vec<_> = (0..3)
        .map(|red| Image {
            width: 1,
            height: 1,
            rgba: vec![red, 0, 0, 255],
        })
        .collect();
    let clip = |kind, frames: &[u16]| SpriteClip {
        key_steps: Vec::new(),
        kind,
        frame_ms: world.rules().tick_ms,
        directions: 1,
        frames: frames
            .iter()
            .map(|&frame| ClipFrame {
                frame,
                flip_x: false,
                offset: [0, 0],
            })
            .collect(),
        loop_start: None,
        progress_starts: vec![],
    };
    let clips = vec![
        clip(ClipKind::Idle, &[0]),
        clip(ClipKind::Attack, &[1, 2]),
        clip(ClipKind::Walk, &[0]),
    ];
    let sprite = SpriteRef {
        name: "shot",
        frame_ms: 42,
        anchor: [0, 0],
        frames: &images,
        clips: &clips,
    };
    world.step(&[]).unwrap();
    visuals.update(&world);
    assert!(!world.state().entities.iter().any(|e| e.id == EntityId(2)));
    world.step(&[]).unwrap();
    visuals.update(&world);
    let observed = visuals.get(EntityId(1)).unwrap();
    assert_eq!(observed.action, VisualAction::Idle);
    let (kind, phase) = action_clip(&sprite, Some(observed), &world);
    assert_eq!(
        sample(&sprite, kind, observed.facing, phase, None)
            .unwrap()
            .image
            .rgba[0],
        2
    );
    let mut moving = *observed;
    moving.action = VisualAction::Move;
    assert_eq!(
        action_clip(&sprite, Some(&moving), &world).0,
        ClipKind::Attack
    );
    world.step(&[]).unwrap();
    visuals.update(&world);
    assert_eq!(
        action_clip(&sprite, visuals.get(EntityId(1)), &world).0,
        ClipKind::Idle
    );
    assert_eq!(
        action_clip(&sprite, Some(&moving), &world).0,
        ClipKind::Walk
    );
}
#[test]
fn ground_weapon_visual_does_not_select_aircraft_during_cooldown() {
    let base = world();
    let mut rules = base.rules().clone();
    let mut flyer = rules.units[0].clone();
    flyer.id = UnitTypeId(2);
    flyer.movement_class = MovementClass::Air;
    rules.units.push(flyer);
    let mut map = base.map().clone();
    map.spawns[1].unit_type = UnitTypeId(2);
    map.spawns[1].position = Position { x: 52, y: 40 };
    let world = World::new(rules, map, 42).unwrap();
    let mut actor = world.state().entities[0].clone();
    actor.cooldown = 4;
    actor.auto_attack_target = Some(EntityId(2));
    assert_eq!(Visuals::new(&world).attack_target(&world, &actor), None);
}

#[test]
#[ignore = "requires STRATERUST_CAMPAIGNS pointing to a refreshed retail import"]
fn retail_flyer_shadows_and_zerg_work_and_spit_use_source_layers() {
    use crate::view::{Camera, Presentation, View};
    use std::{collections::BTreeSet, io::Write, path::Path};
    use straterust_engine::content::Package;
    use straterust_engine::sim::{Entity, Spawn};
    let root = std::env::var_os("STRATERUST_CAMPAIGNS").unwrap();
    let directory = Path::new(&root).join("zerg/zerg01");
    let package = Package::load(&directory).unwrap();
    let base = package.world(42).unwrap();
    let assets = AssetPack::load(&directory).unwrap().unwrap();
    let mut rules = base.rules().clone();
    rules.victory = false;
    let mut map = base.map().clone();
    map.width = 1024;
    map.height = 512;
    map.terrain = None;
    map.mission = None;
    map.ai.clear();
    map.resources.clear();
    map.creation.clear();
    map.start_locations.clear();
    map.initial_explored.clear();
    map.fog_of_war = false;
    map.spawns = (0..32)
        .map(|heading| Spawn {
            owner: PlayerId(0),
            unit_type: UnitTypeId(28),
            position: Position {
                x: 64 + (heading % 8) * 128,
                y: 48 + (heading / 8) * 128,
            },
            ..Default::default()
        })
        .collect();
    let world = World::new(rules, map, 42).unwrap();
    let mut visuals = Visuals::new(&world);
    for (heading, entity) in world.state().entities.iter().enumerate() {
        let visual = visuals.units.get_mut(&entity.id).unwrap();
        visual.facing = heading as u8;
        let shadow = shadow_image(&assets, entity, Some(visual), &world).unwrap();
        assert_eq!(
            [
                shadow.image.width as i32 / 2 - shadow.anchor[0],
                shadow.image.height as i32 / 2 - shadow.anchor[1]
            ],
            [0, 42],
            "heading {heading}"
        );
    }
    let drone = assets.sprite(UnitTypeId(27)).unwrap();
    let media = straterust_engine::media::MediaPack::load(&directory)
        .unwrap()
        .unwrap();
    assert!(
        media
            .audio
            .iter()
            .any(|a| a.cue == straterust_engine::media::AudioCue::Work
                && a.unit_type == Some(UnitTypeId(27))
                && !a.variants.is_empty())
    );
    let work = drone.clip(ClipKind::Work).unwrap();
    assert_eq!(work.directions, 32);
    assert_eq!(work.frame_ms, 42);
    assert_ne!(
        sample(&drone, ClipKind::Work, 8, 0, None)
            .unwrap()
            .image
            .rgba,
        sample(&drone, ClipKind::Work, 8, 84, None)
            .unwrap()
            .image
            .rgba
    );
    let hydra = assets.sprite(UnitTypeId(7)).unwrap();
    assert_eq!(hydra.clip(ClipKind::Attack).unwrap().key_steps, [0]);
    let emission = hydra.clip(ClipKind::AttackEffect).unwrap();
    assert_eq!(emission.directions, 32);
    assert!(emission.frames.len() / 32 >= 14);
    for target in [Position { x: 500, y: 300 }, Position { x: 100, y: 300 }] {
        let mut shot = ProjectileVisual {
            impact_only: false,
            unit_type: UnitTypeId(7),
            owner: PlayerId(0),
            targets_air: false,
            from: Position { x: 300, y: 300 },
            to: target,
            elapsed: Duration::from_millis(42),
            tick_ms: 42,
        };
        let (frame, position) = shot.launch_frame(&assets).unwrap();
        assert_eq!(position, [300.0, 300.0]);
        assert_eq!(frame.flip_x, target.x < 300);
        assert!(frame.image.rgba.as_chunks::<4>().0.iter().any(|p| p[3] > 0));
        // The mouth emission outlives the on-target hit, and stays at launch.
        shot.elapsed = Duration::from_millis(450);
        assert!(
            shot.sample(assets.projectile_for(UnitTypeId(7), false).unwrap())
                .is_none()
        );
        assert!(shot.launch_frame(&assets).is_some());
        visuals.projectiles = vec![shot];
        visuals.advance_effects(Duration::from_millis(1), Some(&assets));
        assert_eq!(visuals.projectiles.len(), 1);
        visuals.advance_effects(Duration::from_secs(1), Some(&assets));
        assert!(visuals.projectiles.is_empty());
    }
    let mut pending = Entity {
        unit_type: UnitTypeId(38),
        ..Default::default()
    };
    pending.construction = Some(straterust_engine::sim::Construction {
        worker: Some(EntityId(100)),
        remaining: 100,
        total: 100,
        work_position: None,
        work_ticks: 0,
    });
    assert!(unit_image(&assets, &pending, None, &world).is_none());
    pending.construction.as_mut().unwrap().work_position = Some(Position { x: 0, y: 0 });
    assert!(unit_image(&assets, &pending, None, &world).is_some());
    let presentation: Presentation =
        ron::de::from_bytes(&std::fs::read(directory.join("presentation.ron")).unwrap()).unwrap();
    let view = View {
        world: &world,
        visuals: &visuals,
        presentation: &presentation,
        assets: Some(&assets),
        map_art: None,
        media: None,
        mission: None,
        speaking: None,
        camera: Camera {
            x: 512.0,
            y: 256.0,
            viewport: None,
            zoom: 1.0,
        },
        cursor: [-1.0; 2],
        targeting: false,
        selected: &BTreeSet::new(),
        selected_resource: None,
        drag_box: None,
        paused: true,
        playback: false,
        animation_ms: 0,
        portrait_ms: 0,
        status: "",
        help: "",
        buttons: &[],
        placement: None,
        placement_type: None,
        ending_hint: "",
    };
    let mut pixels = vec![0; 1024 * 800];
    view.draw(&mut pixels, 1024, 800, 1.0);
    let mut file = std::fs::File::create("/tmp/stratarust-flyer-shadows.ppm").unwrap();
    file.write_all(b"P6\n1024 800\n255\n").unwrap();
    let bytes = pixels
        .into_iter()
        .flat_map(|p| [(p >> 16) as u8, (p >> 8) as u8, p as u8])
        .collect::<Vec<_>>();
    file.write_all(&bytes).unwrap();
}
