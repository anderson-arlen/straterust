use super::*;
use straterust_engine::assets::{ClipFrame, SpriteClip};
use straterust_engine::sim::{Command, Order, UnitType};

#[test]
#[ignore = "requires STRATERUST_CAMPAIGNS pointing to a refreshed retail import"]
fn retail_warp_anchor_keeps_its_opening_loop_and_native_sound_mappings() {
    use straterust_engine::{
        content::Package,
        media::{AudioCue, MediaPack, decode_wav},
    };
    let root = std::env::var_os("STRATERUST_CAMPAIGNS").unwrap();
    let directory = std::path::Path::new(&root).join("protoss/protoss01");
    let world = Package::load(&directory).unwrap().world(42).unwrap();
    for id in [2, 27, 65] {
        let worker = world
            .unit_type(UnitTypeId(id))
            .unwrap()
            .worker
            .as_ref()
            .unwrap();
        assert_eq!((worker.harvest_ticks, worker.capacity), (75, 8));
    }
    let assets = AssetPack::load(&directory).unwrap().unwrap();
    let larva = world.unit_type(UnitTypeId(57)).unwrap();
    assert_eq!(larva.speed, 0, "player movement stays disabled");
    let larva = assets.sprite(UnitTypeId(57)).unwrap();
    let walk = larva.clip(ClipKind::Walk).unwrap();
    assert_eq!(
        (walk.directions, walk.frame_ms, walk.frames.len()),
        (32, 42, 160)
    );
    for heading in [0, 8, 16, 24] {
        let poses = (0..5)
            .map(|step| {
                sample(&larva, ClipKind::Walk, heading, step * 42, None)
                    .unwrap()
                    .image
            })
            .collect::<Vec<_>>();
        assert!(poses.windows(2).all(|p| p[0].rgba != p[1].rgba));
    }
    let media = MediaPack::load(&directory).unwrap().unwrap();
    let zealot = assets.sprite(UnitTypeId(66)).unwrap();
    let death = zealot.clip(ClipKind::Death).unwrap();
    assert_eq!(
        (death.directions, death.frame_ms, death.frames.len()),
        (1, 42, 14)
    );
    assert_eq!(death.loop_start, None);
    for facing in [0, 8, 16, 24, 31] {
        let frames = (0..7)
            .map(|step| {
                sample(&zealot, ClipKind::Death, facing, step * 84, None)
                    .unwrap()
                    .image
            })
            .collect::<Vec<_>>();
        assert!(frames.windows(2).all(|p| !std::ptr::eq(p[0], p[1])));
        assert!(sample(&zealot, ClipKind::Death, facing, 588, None).is_none());
    }
    let death_sound = media
        .audio
        .iter()
        .find(|a| a.cue == AudioCue::Death && a.unit_type == Some(UnitTypeId(66)))
        .unwrap();
    let original = decode_wav(&std::fs::read(directory.join("sound-678.wav")).unwrap()).unwrap();
    assert_eq!(
        death_sound.variants[0].samples.as_ref(),
        original.samples.as_ref()
    );
    for id in [91, 92, 93, 94, 95] {
        let sprite = assets.sprite(UnitTypeId(id)).unwrap();
        let clip = sprite.clip(ClipKind::Construction).unwrap();
        assert_eq!(
            (clip.frame_ms, clip.frames.len(), clip.loop_start),
            (42, 18, Some(12))
        );
        assert_eq!(clip.progress_starts, vec![(0, 0)]);
        let pose = |time| sample(&sprite, ClipKind::Construction, 0, time, Some(50)).unwrap();
        assert!(std::ptr::eq(pose(504).image, pose(756).image));
        assert!(std::ptr::eq(pose(504).image, pose(1008).image));
        assert!(
            !std::ptr::eq(pose(0).image, pose(756).image),
            "opening does not repeat"
        );
        assert!(
            !std::ptr::eq(pose(504).image, pose(588).image),
            "construction continues animating"
        );
        assert!(sample(&sprite, ClipKind::ConstructionEnd, 0, 0, None).is_some());
        for (cue, sound) in [(AudioCue::Transform, 528), (AudioCue::Complete, 529)] {
            let native = media
                .audio
                .iter()
                .find(|a| a.cue == cue && a.unit_type == Some(UnitTypeId(id)))
                .unwrap();
            assert!(!native.voice);
            let bytes = std::fs::read(directory.join(format!("sound-{sound:03}.wav"))).unwrap();
            let original = decode_wav(&bytes).unwrap();
            assert_eq!(
                native.variants[0].samples.as_ref(),
                original.samples.as_ref()
            );
            assert!(original.samples.iter().any(|s| *s != 0));
        }
    }
}

#[test]
fn construction_ranges_animate_independently_and_introductions_do_not_repeat() {
    let frames = (0..6)
        .map(|red| Image {
            width: 1,
            height: 1,
            rgba: vec![red, 0, 0, 255],
        })
        .collect::<Vec<_>>();
    let make_clip = |kind| SpriteClip {
        kind,
        directions: 1,
        frame_ms: 100,
        frames: (0..6)
            .map(|frame| ClipFrame {
                frame,
                flip_x: false,
                offset: [0, 0],
            })
            .collect(),
        key_steps: vec![],
        loop_start: None,
        progress_starts: vec![],
    };
    let mut construction = make_clip(ClipKind::Construction);
    construction.progress_starts = vec![(0, 0), (25, 2), (50, 4)];
    let mut introduction = make_clip(ClipKind::Idle);
    introduction.loop_start = Some(2);
    let clips = [construction, introduction, make_clip(ClipKind::Birth)];
    let sprite = SpriteRef {
        name: "original morph",
        frame_ms: 100,
        anchor: [0, 0],
        frames: &frames,
        clips: &clips,
    };
    let pixel =
        |kind, time, progress| sample(&sprite, kind, 0, time, progress).unwrap().image.rgba[0];
    assert_eq!(pixel(ClipKind::Construction, 0, Some(30)), 2);
    assert_eq!(pixel(ClipKind::Construction, 100, Some(30)), 3);
    assert_eq!(pixel(ClipKind::Construction, 200, Some(30)), 2);
    assert_eq!(pixel(ClipKind::Construction, 100, Some(90)), 5);
    assert_eq!(pixel(ClipKind::Idle, 600, None), 2);
    assert!(sample(&sprite, ClipKind::Birth, 0, 600, None).is_none());

    let mut warping = make_clip(ClipKind::Construction);
    warping.progress_starts = vec![(0, 0)];
    warping.loop_start = Some(2);
    let clips = [warping];
    let sprite = SpriteRef {
        clips: &clips,
        ..sprite
    };
    for (time, pose) in [(0, 0), (100, 1), (600, 2), (700, 3), (1000, 2)] {
        assert_eq!(
            sample(&sprite, ClipKind::Construction, 0, time, Some(50))
                .unwrap()
                .image
                .rgba[0],
            pose
        );
    }
}

#[test]
fn observed_type_changes_reset_the_clock_and_track_each_intermediate_body() {
    let base = super::tests::world();
    let mut rules = base.rules().clone();
    let parent = &mut rules.units[0];
    parent.weapon = None;
    parent.transforms_on_production = true;
    parent.production_form = Some(UnitTypeId(2));
    parent.trains = vec![UnitTypeId(3)];
    parent.supply_provided = 5;
    rules.units.push(UnitType {
        id: UnitTypeId(2),
        ..UnitType::default()
    });
    rules.units.push(UnitType {
        id: UnitTypeId(3),
        build_ticks: 4,
        ..UnitType::default()
    });
    let mut map = base.map().clone();
    map.spawns.truncate(1);
    let mut world = World::new(rules, map, 42).unwrap();
    let mut visuals = Visuals::new(&world);
    for _ in 0..10 {
        world.step(&[]).unwrap();
        visuals.update(&world);
    }
    world
        .step(&[Command {
            tick: world.tick(),
            player: PlayerId(0),
            sequence: 1,
            order: Order::Train {
                entity: EntityId(1),
                unit_type: UnitTypeId(3),
            },
        }])
        .unwrap();
    visuals.update(&world);
    let v = visuals.get(EntityId(1)).unwrap();
    assert_eq!(v.unit_type, UnitTypeId(2));
    assert_eq!(v.previous_type, Some(UnitTypeId(1)));
    assert_eq!(v.phase_ms(&world), 0);
    for _ in 0..4 {
        world.step(&[]).unwrap();
        visuals.update(&world);
    }
    let v = visuals.get(EntityId(1)).unwrap();
    assert_eq!(v.unit_type, UnitTypeId(3));
    assert_eq!(v.previous_type, Some(UnitTypeId(2)));
}
