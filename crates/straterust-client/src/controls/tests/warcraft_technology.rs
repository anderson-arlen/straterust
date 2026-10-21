use super::*;
use straterust_engine::sim::{ResearchId, Spawn};

#[test]
fn upgraded_build_prerequisites_are_accepted_by_the_menu_and_simulation() {
    let mut app = demo();
    let mut rules = app.world.rules().clone();
    let mut upgraded = app.world.unit_type(UnitTypeId(4)).unwrap().clone();
    upgraded.id = UnitTypeId(20);
    upgraded.provides_types = vec![UnitTypeId(4)];
    rules.units.push(upgraded);
    let mut map = app.world.map().clone();
    map.spawns.push(Spawn {
        unit_type: UnitTypeId(20),
        position: Position { x: 640, y: 320 },
        ..Default::default()
    });
    app.world = World::new(rules, map, 42).unwrap();
    app.selected = [EntityId(2)].into();
    assert!(
        app.unit_action_rejection(Action::Build(UnitTypeId(5)), UnitTypeId(5))
            .is_none()
    );
    assert_ne!(
        app.world.build_rejection(
            PlayerId(0),
            EntityId(2),
            UnitTypeId(5),
            Position { x: 800, y: 500 }
        ),
        Some(straterust_engine::sim::Rejection::MissingPrerequisite)
    );
}

#[test]
#[ignore = "requires STRATERUST_WARCRAFT2 pointing to a refreshed retail import"]
fn native_buildings_keep_their_commands_after_hall_upgrades_and_caster_buttons_do_not_overlap() {
    let root = std::path::PathBuf::from(std::env::var_os("STRATERUST_WARCRAFT2").unwrap());
    for race in 0..2 {
        let directory = root
            .join(if race == 0 { "human" } else { "orc" })
            .join("mission14");
        let base = Package::load(&directory).unwrap().world(42).unwrap();
        let mut rules = base.rules().clone();
        rules.victory = false;
        for research in &mut rules.research {
            research.ticks = 2;
        }
        rules.starting_resources = ["gold", "wood", "oil"]
            .map(|kind| ResourceAmount {
                kind: kind.into(),
                amount: 100000,
            })
            .to_vec();
        let mut map = base.map().clone();
        map.terrain = None;
        map.mission = None;
        map.ai.clear();
        map.resources.clear();
        map.creation.clear();
        map.initial_explored.clear();
        map.fog_of_war = false;
        map.spawns = [2, 90, 62, 80, 68, 76, 82, 66]
            .into_iter()
            .enumerate()
            .map(|(index, source)| Spawn {
                owner: PlayerId(0),
                unit_type: UnitTypeId(source + race + 1),
                position: Position {
                    x: 160 + index as i32 * 160,
                    y: 160,
                },
                ..Default::default()
            })
            .collect();
        let mut app = demo();
        app.world = World::new(rules, map, 42).unwrap();
        app.presentation = read_ron(&directory.join("presentation.ron")).unwrap();
        app.selected = [EntityId(1)].into();
        app.build_menu = true;
        app.advanced_build_menu = true;
        for source in [66, 68] {
            let button = app
                .buttons()
                .into_iter()
                .find(|b| b.action == Action::Build(UnitTypeId(source + race + 1)))
                .unwrap();
            assert!(
                button.disabled.is_none(),
                "Castle must satisfy Keep: {:?}",
                button.disabled
            );
            assert!(
                button
                    .tooltip
                    .iter()
                    .any(|text| text.contains("Requires a")),
                "building must explain its role, not just its cost"
            );
        }
        app.build_menu = false;
        app.advanced_build_menu = false;
        // Altar and Church upgrades must be offered and accepted by authority.
        app.selected = [EntityId(3)].into();
        let research = ResearchId(17 + race * 64);
        let button = app
            .buttons()
            .into_iter()
            .find(|b| b.action == Action::Research(research))
            .unwrap();
        assert!(button.disabled.is_none(), "{race}: {:?}", button.disabled);
        assert!(
            app.world
                .research_rejection(PlayerId(0), EntityId(3), research)
                .is_none()
        );
        // Check all imported producers for command-slot collisions, including
        // the Mage/Slow and Death Knight/Death Coil training/research cases.
        for entity in 2..=8 {
            app.selected = [EntityId(entity)].into();
            let buttons = app.buttons();
            let mut occupied = std::collections::BTreeSet::new();
            for b in &buttons {
                assert!(
                    occupied.insert(b.slot),
                    "race {race}, entity {entity}: overlapping {}",
                    b.label
                );
            }
        }
        app.selected = [EntityId(4)].into();
        assert!(
            app.buttons()
                .iter()
                .any(|b| b.slot == 0 && b.action == Action::Train(UnitTypeId(11 + race)))
        );
        let assets = straterust_engine::assets::AssetPack::load(&directory)
            .unwrap()
            .unwrap();
        let light = assets.projectile(UnitTypeId(31 + race)).unwrap();
        let heavy = assets.projectile(UnitTypeId(33 + race)).unwrap();
        assert_eq!(light.manifest.arc_height, 0);
        assert_eq!(heavy.manifest.arc_height, 0);
        assert!(heavy.manifest.directional);
        assert_ne!(
            light.manifest.flight.frames[0].blake3,
            heavy.manifest.flight.frames[0].blake3
        );
        for source in [30, 32] {
            let sprite = assets.sprite(UnitTypeId(source + race + 1)).unwrap();
            let attack = sprite
                .clip(straterust_engine::assets::ClipKind::Attack)
                .unwrap();
            let idle = sprite
                .clip(straterust_engine::assets::ClipKind::Idle)
                .unwrap();
            assert_eq!(
                attack.frames[0], idle.frames[8],
                "ship must turn broadside to aim north"
            );
            assert_eq!(attack.loop_start, Some(0));
        }
        let ship = app.world.unit_type(UnitTypeId(33 + race)).unwrap();
        assert_eq!(ship.motion.as_ref().unwrap().speed, 32 * 256 / 26);
        app.assets = Some(assets);
        app.visuals = crate::visual::Visuals::new(&app.world);
        app.selected = [EntityId(3)].into();
        crate::presentation_tests::capture(
            &app,
            &format!("warcraft2-research-available-{race}"),
            [1100, 760],
            Some(Action::Research(research)),
        );
        app.activate(Action::Research(research)).unwrap();
        step(&mut app);
        step(&mut app);
        let completed = app
            .buttons()
            .into_iter()
            .find(|b| b.action == Action::Research(research))
            .unwrap();
        assert!(completed.completed);
        assert!(completed.disabled.is_some());
        crate::presentation_tests::capture(
            &app,
            &format!("warcraft2-research-completed-{race}"),
            [1100, 760],
            Some(Action::Research(research)),
        );
    }
}
