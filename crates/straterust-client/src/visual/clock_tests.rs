use super::*;
#[test]
fn unit_animation_keeps_time_without_new_world_ticks() {
    let package = straterust_engine::content::Package::load(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/lan-demo"),
    )
    .unwrap();
    let world = package.world(42).unwrap();
    let mut visuals = Visuals::new(&world);
    let id = world.state().entities[0].id;
    let original = world.state_hash();
    visuals.advance_animations(Duration::from_millis(100), world.rules().tick_ms);
    assert_eq!(visuals.get(id).unwrap().phase_ms(&world), 100);
    visuals.advance_animations(Duration::from_secs(1), world.rules().tick_ms);
    assert_eq!(visuals.get(id).unwrap().phase_ms(&world), 1100);
    assert_eq!(world.state_hash(), original);
}

#[test]
fn local_sprite_frames_keep_advancing_and_missed_attack_steps_are_consumed_once() {
    use straterust_engine::assets::{ClipFrame, SpriteClip};
    let world = super::tests::world();
    let mut visuals = Visuals::new(&world);
    let id = EntityId(1);
    visuals.units.get_mut(&id).unwrap().action = VisualAction::Move;
    let images: Vec<_> = (0..2)
        .map(|color| Image {
            width: 1,
            height: 1,
            rgba: vec![color, 0, 0, 255],
        })
        .collect();
    let clips: Vec<_> = [ClipKind::Walk, ClipKind::Attack]
        .into_iter()
        .map(|kind| SpriteClip {
            kind,
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
            loop_start: None,
            progress_starts: Vec::new(),
            key_steps: if kind == ClipKind::Attack {
                vec![1]
            } else {
                Vec::new()
            },
        })
        .collect();
    let sprite = SpriteRef {
        name: "clock",
        anchor: [0, 0],
        frame_ms: 100,
        frames: &images,
        clips: &clips,
    };
    for _ in 0..10 {
        visuals.advance_animations(Duration::from_micros(10_100), world.rules().tick_ms);
    }
    let (kind, phase) = sprites::action_clip(&sprite, visuals.get(id), &world);
    assert_eq!(phase, 101); // fractional milliseconds must not be lost each redraw
    assert_eq!(
        sample(&sprite, kind, 0, phase, None).unwrap().image.rgba[0],
        1
    );
    let visual = visuals.units.get_mut(&id).unwrap();
    visual.shot_tick = Some(world.tick().0);
    visual.shot_phase = Some(0);
    assert_eq!(
        sprites::action_clip(&sprite, visuals.get(id), &world).0,
        ClipKind::Attack
    );
    visuals.mark_rendered();
    visuals.advance_animations(Duration::from_secs(1), world.rules().tick_ms);
    assert_eq!(
        sprites::action_clip(&sprite, visuals.get(id), &world),
        (ClipKind::Attack, 100)
    );
    visuals.mark_rendered();
    assert_eq!(
        sprites::action_clip(&sprite, visuals.get(id), &world).0,
        ClipKind::Walk
    );
    assert_eq!(world.tick().0, 0);
}
