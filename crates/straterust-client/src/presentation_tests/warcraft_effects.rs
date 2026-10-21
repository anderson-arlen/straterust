use super::*;
use straterust_engine::sim::{AbilityId, AbilityTarget, Command, Order, Spawn};

#[test]
#[ignore = "requires STRATERUST_WARCRAFT2 pointing to a refreshed retail import"]
fn warcraft2_flying_attacks_and_spells_render_native_finite_and_continuous_phases() {
    let root = PathBuf::from(std::env::var_os("STRATERUST_WARCRAFT2").unwrap());
    let mut app = App::load(
        &root.join("human/mission14"),
        Config {
            audio: false,
            zoom: 2.0,
            ..Default::default()
        },
        None,
    )
    .unwrap();
    app.mission_ui = None;
    app.simulation = None;
    app.map_art = None;
    let assets = app.assets.as_mut().unwrap();
    assets.manifest.terrain_grid = None;
    assets.terrain = straterust_engine::assets::Image {
        width: 1,
        height: 1,
        rgba: vec![80, 80, 72, 255],
    };
    let mut rules = app.world.rules().clone();
    rules.victory = false;
    rules.research.clear();
    for unit in &mut rules.units {
        for ability in &mut unit.abilities {
            ability.research = None;
            ability.energy = 0;
        }
    }
    let mut map = app.world.map().clone();
    map.width = 1024;
    map.height = 1024;
    map.mission = None;
    map.ai.clear();
    map.terrain = None;
    map.resources.clear();
    map.creation.clear();
    map.initial_explored.clear();
    map.start_locations.clear();
    map.fog_of_war = false;
    app.camera.x = 512.0;
    app.camera.y = 512.0;
    for (name, caster, ability, unit_target, enemy) in [
        ("healing", 13, 2, true, false),
        ("bloodlust", 14, 5, true, false),
        ("runes", 14, 6, false, false),
        ("fireball", 11, 7, false, false),
        ("flame-shield", 11, 9, true, false),
        ("blizzard", 11, 12, false, false),
        ("death-coil", 12, 13, true, true),
        ("whirlwind", 12, 17, false, false),
        ("death-decay", 12, 18, false, false),
        ("gryphon", 23, 0, true, true),
        ("dragon", 36, 0, true, true),
    ] {
        map.spawns = vec![
            Spawn {
                unit_type: UnitTypeId(caster),
                position: Position { x: 416, y: 512 },
                ..Default::default()
            },
            Spawn {
                owner: PlayerId(u16::from(enemy)),
                unit_type: UnitTypeId(1),
                position: Position { x: 608, y: 512 },
                hp_percent: Some(60),
                ..Default::default()
            },
        ];
        app.world = World::new(rules.clone(), map.clone(), 7).unwrap();
        app.visuals = Visuals::new(&app.world);
        app.selected = [EntityId(1)].into();
        let order = if ability == 0 {
            Order::Attack {
                entity: EntityId(1),
                target: EntityId(2),
            }
        } else {
            Order::Cast {
                entity: EntityId(1),
                ability: AbilityId(ability),
                target: if unit_target {
                    AbilityTarget::Unit(EntityId(2))
                } else {
                    AbilityTarget::Point(Position { x: 608, y: 512 })
                },
            }
        };
        app.queue
            .push(Command {
                tick: app.world.tick(),
                player: PlayerId(0),
                sequence: 1,
                order,
            })
            .unwrap();
        let mut observed = false;
        for tick in 0..65 {
            advance(&mut app);
            if ![3, 7, 11, 17, 40, 60].contains(&tick) {
                continue;
            }
            let pixels = capture_at(
                &app,
                &format!("warcraft2-{name}-{tick}"),
                [1024, 768],
                None,
                None,
                0,
            );
            let effects = std::mem::take(&mut app.assets.as_mut().unwrap().projectiles);
            let baseline = capture_at(&app, "warcraft2-effect-control", [1024, 768], None, None, 0);
            app.assets.as_mut().unwrap().projectiles = effects;
            let visible = pixels.iter().zip(&baseline).any(|(a, b)| a != b);
            if name == "bloodlust" && tick >= 40 {
                assert!(
                    visible,
                    "blood drop must remain after the cast flash expires"
                );
            }
            observed |= visible;
        }
        assert!(observed, "{name} must draw a visible native effect");
    }
}

#[test]
#[ignore = "requires STRATERUST_WARCRAFT2 pointing to a refreshed retail import"]
fn warcraft2_group_heal_casts_once_with_one_paladin_and_one_sound() {
    let root = PathBuf::from(std::env::var_os("STRATERUST_WARCRAFT2").unwrap());
    let mut app = App::load(
        &root.join("human/mission14"),
        Config {
            audio: false,
            ..Default::default()
        },
        None,
    )
    .unwrap();
    app.simulation = None;
    app.mission_ui = None;
    let mut rules = app.world.rules().clone();
    rules.victory = false;
    rules.research.clear();
    for unit in &mut rules.units {
        for ability in &mut unit.abilities {
            ability.research = None;
        }
    }
    let mut map = app.world.map().clone();
    map.mission = None;
    map.ai.clear();
    map.terrain = None;
    map.resources.clear();
    map.creation.clear();
    map.initial_explored.clear();
    map.fog_of_war = false;
    map.spawns = vec![
        Spawn {
            unit_type: UnitTypeId(13),
            position: Position { x: 448, y: 512 },
            energy_percent: Some(100),
            ..Default::default()
        },
        Spawn {
            unit_type: UnitTypeId(13),
            position: Position { x: 448, y: 576 },
            energy_percent: Some(100),
            ..Default::default()
        },
        Spawn {
            unit_type: UnitTypeId(13),
            position: Position { x: 608, y: 512 },
            hp_percent: Some(10),
            ..Default::default()
        },
    ];
    app.world = World::new(rules, map, 7).unwrap();
    app.visuals = Visuals::new(&app.world);
    app.selected = [EntityId(1), EntityId(2)].into();
    app.audio.reset(&app.world);
    app.audio.events.clear();
    app.target_mode = Some(crate::TargetMode::Cast(AbilityId(2)));
    app.targeting_click(Position { x: 608, y: 512 }).unwrap();
    let commands = app.queue.take(app.world.tick());
    assert_eq!(commands.len(), 1);
    let before = entity(&app, EntityId(3)).hp;
    let outcomes = app.world.step(&commands).unwrap();
    assert!(outcomes[0].rejection.is_none(), "{outcomes:?}");
    app.audio.observe(&app.world);
    assert_eq!(entity(&app, EntityId(3)).hp, before + 42);
    assert_eq!(entity(&app, EntityId(2)).energy, 255 * 256);
    assert!(entity(&app, EntityId(2)).last_cast.is_none());
    let healed = entity(&app, EntityId(3)).hp;
    for _ in 0..60 {
        app.world.step(&[]).unwrap();
        app.audio.observe(&app.world);
    }
    assert_eq!(
        entity(&app, EntityId(3)).hp,
        healed,
        "one click must not keep healing"
    );
    assert_eq!(
        app.audio
            .events
            .iter()
            .filter(|(cue, _)| *cue == crate::audio::Cue::Ability(AbilityId(2)))
            .count(),
        1
    );
    app.target_mode = Some(crate::TargetMode::Cast(AbilityId(2)));
    app.targeting_click(Position { x: 608, y: 512 }).unwrap();
    let commands = app.queue.take(app.world.tick());
    assert_eq!(commands.len(), 1);
    assert!(
        matches!(
            commands[0].order,
            Order::Cast {
                entity: EntityId(2),
                ..
            }
        ),
        "the next click uses the next charged paladin"
    );
    app.world.step(&commands).unwrap();
    assert_eq!(entity(&app, EntityId(3)).hp, healed + 39);
}
