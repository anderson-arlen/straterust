use super::*;
use crate::view::BuildButton;
use straterust_engine::sim::{GarrisonStats, MovementClass, Spawn};

#[test]
fn basic_and_advanced_structures_use_separate_menus_with_unique_slots() {
    let mut app = demo();
    let mut rules = app.world.rules().clone();
    let mut factory = rules
        .units
        .iter()
        .find(|u| u.id == UnitTypeId(5))
        .unwrap()
        .clone();
    factory.id = UnitTypeId(6);
    rules.units.push(factory);
    rules
        .units
        .iter_mut()
        .find(|u| u.id == UnitTypeId(2))
        .unwrap()
        .builds
        .push(UnitTypeId(6));
    app.world = World::new(rules, app.world.map().clone(), 0).unwrap();
    for (unit, slot, key) in [(3, 0, "C"), (4, 1, "S"), (5, 3, "B"), (6, 0, "F")] {
        app.presentation.build_buttons.insert(
            UnitTypeId(unit),
            BuildButton {
                advanced: unit == 6,
                slot,
                key: key.into(),
            },
        );
    }
    app.selected = BTreeSet::from([EntityId(2)]);
    let buttons = app.buttons();
    assert!(
        buttons
            .iter()
            .any(|b| b.action == Action::AdvancedBuildMenu && b.slot == 7)
    );
    let slots: BTreeSet<_> = buttons.iter().map(|b| b.slot).collect();
    assert_eq!(slots.len(), buttons.len());
    app.activate(Action::BuildMenu).unwrap();
    let builds: Vec<_> = app
        .buttons()
        .into_iter()
        .filter_map(|b| match b.action {
            Action::Build(id) => Some(id),
            _ => None,
        })
        .collect();
    assert_eq!(builds, vec![UnitTypeId(3), UnitTypeId(4), UnitTypeId(5)]);
    app.activate(Action::Back).unwrap();
    app.activate(Action::AdvancedBuildMenu).unwrap();
    let buttons = app.buttons();
    assert_eq!(buttons.len(), 2);
    assert_eq!(buttons[0].action, Action::Build(UnitTypeId(6)));
    assert_eq!(buttons[0].key, "F");
    assert_eq!(buttons[1].action, Action::Back);
}

#[test]
fn mobile_transport_unload_all_button_empties_every_passenger() {
    let mut app = demo();
    let mut rules = app.world.rules().clone();
    rules.victory = false;
    let mut transport = rules
        .units
        .iter()
        .find(|u| u.id == UnitTypeId(1))
        .unwrap()
        .clone();
    transport.id = UnitTypeId(6);
    transport.movement_class = MovementClass::Air;
    transport.weapon = None;
    transport.garrison = Some(GarrisonStats {
        capacity: 4,
        passengers: vec![UnitTypeId(1), UnitTypeId(2)],
        attackers: vec![],
        range_bonus: 0,
    });
    rules.units.push(transport);
    let mut map = app.world.map().clone();
    map.spawns.retain(|s| s.owner == PlayerId(0));
    for (unit, x, y) in [(1, 320, 300), (6, 320, 340)] {
        map.spawns.push(Spawn {
            unit_type: UnitTypeId(unit),
            position: Position { x, y },
            ..Default::default()
        });
    }
    app.world = World::new(rules, map, 0).unwrap();
    app.selected = BTreeSet::from([EntityId(4)]);
    assert!(
        app.buttons()
            .iter()
            .any(|b| b.action == Action::Unload && b.disabled.is_some())
    );
    for passenger in [EntityId(2), EntityId(3)] {
        app.issue(Order::Load {
            entity: passenger,
            target: EntityId(4),
        })
        .unwrap();
    }
    for _ in 0..100 {
        step(&mut app);
    }
    assert_eq!(
        crate::selection::panel_members(&app.world, &app.selected)
            .1
            .len(),
        2
    );
    let button = app
        .buttons()
        .into_iter()
        .find(|b| b.action == Action::Unload)
        .unwrap();
    assert_eq!(button.label, "Unload All");
    assert_eq!(button.key, "U");
    assert!(button.disabled.is_none());
    app.activate(Action::Unload).unwrap();
    step(&mut app);
    assert!(
        crate::selection::panel_members(&app.world, &app.selected)
            .1
            .is_empty()
    );
    assert_eq!(app.selected, BTreeSet::from([EntityId(4)]));
}
