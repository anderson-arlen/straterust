use super::*;
use straterust_engine::{
    media::{AudioCue, MediaManifest, MediaPack},
    sim::{Command, Order, Spawn},
};

#[test]
#[ignore = "requires private mission 5; checks a few shots/deaths and cloak without playing the mission"]
fn campaign_combat_source_art_audio_and_cloak() {
    let directory = PathBuf::from(std::env::var_os("STRATERUST_ASSET_PACKAGE").unwrap());
    let package = Package::load(&directory).unwrap();
    let mut app = App::new(
        &package,
        Config {
            audio: false,
            zoom: 2.0,
            ..Default::default()
        },
        read_ron(&directory.join("presentation.ron")).unwrap(),
        AssetPack::load(&directory).unwrap(),
        None,
    )
    .unwrap();
    app.media = MediaPack::load(&directory).unwrap();
    app.audio.set_media(app.media.as_ref());
    let media: MediaManifest = read_ron(&directory.join("media.ron")).unwrap();
    for (unit, cue, sound) in [
        (10, AudioCue::Attack, 106),
        (20, AudioCue::Attack, 106),
        (25, AudioCue::Attack, 98),
        (23, AudioCue::Attack, 74),
        (23, AudioCue::AttackAir, 82),
        (36, AudioCue::AttackAir, 80),
    ] {
        let mapping = media
            .audio
            .iter()
            .find(|m| m.cue == cue && m.unit_type == Some(UnitTypeId(unit)))
            .expect("source weapon cue");
        assert!(!mapping.voice);
        assert!(
            mapping
                .variants
                .iter()
                .any(|v| v.file == format!("sound-{sound:03}.wav"))
        );
    }
    let original = app.world.map().clone();
    for (unit, target, air) in [
        (10, 36, false),
        (25, 36, false),
        (23, 36, false),
        (23, 23, true),
        (36, 23, true),
    ] {
        let mut map = original.clone();
        map.mission = None;
        map.ai.clear();
        map.terrain = None;
        map.fog_of_war = false;
        map.initial_explored.clear();
        map.creation.clear();
        map.resources.clear();
        map.start_locations.clear();
        map.players = 2;
        map.spawns = vec![
            Spawn {
                unit_type: UnitTypeId(unit),
                position: Position { x: 512, y: 512 },
                ..Default::default()
            },
            Spawn {
                owner: PlayerId(1),
                unit_type: UnitTypeId(target),
                position: Position { x: 640, y: 512 },
                ..Default::default()
            },
        ];
        app.world = World::new(app.world.rules().clone(), map, 42).unwrap();
        app.visuals = Visuals::new(&app.world);
        app.audio.reset(&app.world);
        app.camera.x = 576.0;
        app.camera.y = 512.0;
        app.selected = BTreeSet::from([EntityId(1)]);
        advance(&mut app);
        app.audio.observe(&app.world);
        assert!(app.audio.events.contains(&(
            if air {
                AudioCue::AttackAir
            } else {
                AudioCue::Attack
            },
            Some(UnitTypeId(unit))
        )));
        assert_eq!(entity(&app, EntityId(1)).last_attack_air, air);
        let shot = app
            .visuals
            .projectiles()
            .iter()
            .find(|p| p.unit_type == UnitTypeId(unit))
            .expect("weapon shot");
        assert_eq!(shot.targets_air, air);
        advance(&mut app);
        advance(&mut app);
        let shot = app
            .visuals
            .projectiles()
            .iter()
            .find(|p| p.unit_type == UnitTypeId(unit))
            .unwrap();
        let effect = app
            .assets
            .as_ref()
            .unwrap()
            .projectile_for(UnitTypeId(unit), air)
            .expect("source weapon effect");
        let frame = shot.sample(effect).expect("visible flight or impact").0;
        assert!(
            frame
                .image
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .any(|p| p[3] != 0)
        );
        capture(
            &app,
            &format!("combat-{unit}-{}", if air { "air" } else { "ground" }),
            [1100, 760],
            None,
        );
    }
    // The mission's placed Kerrigan has full hero energy, not the default quarter.
    let kerrigan = package
        .world(42)
        .unwrap()
        .state()
        .entities
        .iter()
        .find(|e| e.unit_type == UnitTypeId(25))
        .unwrap()
        .clone();
    assert_eq!(kerrigan.energy, 250 * 256);
    let mut map = original.clone();
    map.mission = None;
    map.ai.clear();
    map.terrain = None;
    map.fog_of_war = false;
    map.initial_explored.clear();
    map.creation.clear();
    map.resources.clear();
    map.start_locations.clear();
    map.players = 2;
    map.spawns = vec![Spawn {
        unit_type: UnitTypeId(25),
        position: Position { x: 512, y: 512 },
        energy_percent: Some(100),
        ..Default::default()
    }];
    app.world = World::new(app.world.rules().clone(), map.clone(), 42).unwrap();
    app.visuals = Visuals::new(&app.world);
    app.selected = BTreeSet::from([EntityId(1)]);
    app.activate(Action::Cloak(true)).unwrap();
    advance(&mut app);
    assert!(entity(&app, EntityId(1)).cloaked);
    assert!(!app.world.entity_visible(PlayerId(1), EntityId(1)));
    capture(
        &app,
        "kerrigan-cloaked",
        [1100, 760],
        Some(Action::Cloak(false)),
    );
    for unit in [3, 10, 13, 20, 23, 25, 32, 36] {
        let assets = app.assets.as_ref().unwrap();
        let sprite = assets.sprite(UnitTypeId(unit)).unwrap();
        let clip = sprite
            .clip(straterust_engine::assets::ClipKind::Death)
            .expect("source death clip");
        assert!(
            clip.frames.len() / usize::from(clip.directions) > 1,
            "finite source death poses"
        );
        map.spawns = vec![
            Spawn {
                unit_type: UnitTypeId(unit),
                position: Position { x: 512, y: 512 },
                ..Default::default()
            },
            Spawn {
                owner: PlayerId(1),
                unit_type: UnitTypeId(1),
                position: Position { x: 640, y: 512 },
                ..Default::default()
            },
        ];
        let mut rules = app.world.rules().clone();
        let weapon = rules
            .units
            .iter_mut()
            .find(|u| u.id == UnitTypeId(1))
            .unwrap()
            .weapon
            .as_mut()
            .unwrap();
        weapon.damage = 100000;
        weapon.strikes.clear();
        app.world = World::new(rules, map.clone(), 42).unwrap();
        app.visuals = Visuals::new(&app.world);
        app.world
            .step(&[Command {
                tick: app.world.tick(),
                player: PlayerId(1),
                sequence: 1,
                order: Order::Attack {
                    entity: EntityId(2),
                    target: EntityId(1),
                },
            }])
            .unwrap();
        app.visuals.update(&app.world);
        assert!(
            app.visuals
                .deaths()
                .iter()
                .any(|d| d.unit_type == UnitTypeId(unit))
        );
        app.visuals
            .advance_effects(Duration::from_millis(160), app.assets.as_ref());
        capture(&app, &format!("death-{unit}"), [1100, 760], None);
    }
}
