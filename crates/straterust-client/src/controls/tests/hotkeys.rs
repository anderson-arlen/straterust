use super::*;
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
