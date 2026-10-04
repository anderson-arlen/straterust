use super::tests::world;
use super::*;
use straterust_engine::assets::{ClipFrame, SpriteClip};
use straterust_engine::sim::MovementClass;

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
