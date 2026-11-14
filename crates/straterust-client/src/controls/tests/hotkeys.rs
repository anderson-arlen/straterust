use super::*;

#[test]
fn missing_addon_requirements_name_the_parent_in_tooltips_and_errors() {
    use straterust_engine::sim::{Spawn, UnitType};
    let mut app = demo();
    let mut rules = app.world.rules().clone();
    rules.units[0].prerequisites = vec![UnitTypeId(7)];
    for (id, parent) in [(6, None), (7, Some(UnitTypeId(6)))] {
        rules.units.push(UnitType {
            id: UnitTypeId(id),
            max_hp: 100,
            structure: true,
            speed: 0,
            addon_parent: parent,
            builds: if parent.is_none() {
                vec![UnitTypeId(7)]
            } else {
                vec![]
            },
            ..Default::default()
        });
    }
    let mut map = app.world.map().clone();
    map.spawns.push(Spawn {
        unit_type: UnitTypeId(5),
        position: Position { x: 640, y: 512 },
        ..Default::default()
    });
    map.spawns.push(Spawn {
        unit_type: UnitTypeId(6),
        position: Position { x: 384, y: 512 },
        ..Default::default()
    });
    app.world = World::new(rules, map, 42).unwrap();
    app.presentation.unit_names.extend([
        (UnitTypeId(1), "Battlecruiser".into()),
        (UnitTypeId(6), "Science Facility".into()),
        (UnitTypeId(7), "Physics Lab".into()),
    ]);
    app.selected = BTreeSet::from([EntityId(4)]);
    let button = app
        .buttons()
        .into_iter()
        .find(|b| b.action == Action::Train(UnitTypeId(1)))
        .unwrap();
    let label = "Physics Lab (add-on to Science Facility)";
    assert!(button.disabled.as_ref().unwrap().contains(label));
    assert!(button.tooltip.iter().any(|line| line.contains(label)));
    app.activate(Action::Train(UnitTypeId(1))).unwrap();
    assert!(app.status.contains(label));
    assert!(app.recorded.is_empty());
}
use std::collections::BTreeMap;

#[test]
fn hotkeys_use_game_commands_for_targeting_back_and_production_cancel() {
    let mut app = demo();
    app.presentation.command_keys = BTreeMap::from([
        ("attack".into(), "A".into()),
        ("build".into(), "B".into()),
        ("cancel".into(), "Esc".into()),
        ("back".into(), "Esc".into()),
    ]);
    app.config.bindings.attack = "K".into();
    app.presentation.validate().unwrap();
    app.selected = BTreeSet::from([EntityId(2)]);
    assert!(app.bound_key(KeyCode::KeyA).unwrap());
    assert_eq!(app.target_mode, Some(TargetMode::AttackMove));
    assert!(app.bound_key(KeyCode::Escape).unwrap());
    assert!(app.target_mode.is_none());
    assert!(app.recorded.is_empty());
    assert!(app.bound_key(KeyCode::KeyB).unwrap());
    assert!(app.build_menu);
    assert!(app.bound_key(KeyCode::Escape).unwrap());
    assert!(!app.build_menu);
    app.selected = BTreeSet::from([EntityId(1)]);
    app.activate(Action::Train(UnitTypeId(2))).unwrap();
    step(&mut app);
    assert_eq!(app.world.state().entities[0].production.len(), 1);
    assert!(app.bound_key(KeyCode::Escape).unwrap());
    assert_eq!(
        app.recorded.last().unwrap().order,
        Order::Cancel {
            entity: EntityId(1)
        }
    );
    step(&mut app);
    assert!(app.world.state().entities[0].production.is_empty());
}

#[test]
fn hotkeys_validate_command_keys_and_allow_authored_addon_controls() {
    let mut app = demo();
    app.presentation
        .command_keys
        .insert("build.4".into(), "N".into());
    let button = app.unit_button(0, Action::Build(UnitTypeId(4)), UnitTypeId(4), "C");
    assert_eq!(button.key, "N");
    app.presentation.validate().unwrap();
    app.presentation
        .command_keys
        .insert("cancel".into(), "Escape".into());
    app.presentation.validate().unwrap();
    app.presentation
        .command_keys
        .insert("cancel".into(), "bad key".into());
    assert!(app.presentation.validate().is_err());
}

#[test]
fn unit_command_layout_moves_buttons_and_hides_their_hotkeys() {
    let mut app = demo();
    app.selected = BTreeSet::from([EntityId(2)]);
    let unit = app
        .world
        .state()
        .entities
        .iter()
        .find(|e| e.id == EntityId(2))
        .unwrap()
        .unit_type;
    app.presentation.unit_commands.insert(
        unit,
        BTreeMap::from([
            ("attack".into(), Some(2)),
            ("hold".into(), None),
            ("patrol".into(), None),
        ]),
    );
    app.presentation.validate().unwrap();
    let buttons = app.buttons();
    assert_eq!(
        buttons
            .iter()
            .find(|b| b.action == Action::AttackMove)
            .unwrap()
            .slot,
        2
    );
    assert!(
        !buttons
            .iter()
            .any(|b| matches!(b.action, Action::Hold | Action::Patrol))
    );
    assert!(!app.bound_key(KeyCode::KeyP).unwrap());
    assert!(app.bound_key(KeyCode::KeyA).unwrap());
    assert_eq!(app.target_mode, Some(TargetMode::AttackMove));
}
