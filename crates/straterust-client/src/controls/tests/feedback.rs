use super::*;
use crate::visual::CommandTarget;
use std::time::Duration;

#[test]
fn command_feedback_blinks_targets_expires_and_leaves_gameplay_untouched() {
    let mut app = demo();
    app.selected = BTreeSet::from([EntityId(2)]);
    let before = app.world.state_hash();
    let ground = Position { x: 400, y: 400 };
    app.contextual_order(ground).unwrap();
    assert_eq!(
        app.visuals.command_feedback().unwrap().target,
        CommandTarget::Ground(ground)
    );
    let building = app.world.state().entities[0].clone();
    app.contextual_order(building.position).unwrap();
    assert_eq!(
        app.visuals.command_feedback().unwrap().target,
        CommandTarget::Entity(building.id)
    );
    assert!(app.visuals.command_feedback().unwrap().visible());
    app.visuals
        .advance_effects(Duration::from_millis(100), None);
    assert!(!app.visuals.command_feedback().unwrap().visible());
    app.visuals
        .advance_effects(Duration::from_millis(100), None);
    assert!(app.visuals.command_feedback().unwrap().visible());
    app.visuals
        .advance_effects(Duration::from_millis(400), None);
    assert!(app.visuals.command_feedback().is_none());
    assert_eq!(before, app.world.state_hash());
    app.selected.clear();
    app.contextual_order(ground).unwrap();
    assert!(app.visuals.command_feedback().is_none());
}

#[test]
fn resource_click_inspects_amount_without_selecting_workers_or_issuing_commands() {
    let mut app = demo();
    let node = app.world.state().resources[0].clone();
    let before = app.world.state_hash();
    app.select_at(node.position);
    assert_eq!(app.selected_resource, Some(node.id));
    assert!(app.selected.is_empty());
    assert!(app.buttons().is_empty());
    app.contextual_order(node.position).unwrap();
    assert!(app.recorded.is_empty());
    assert_eq!(app.world.state_hash(), before);
    app.select_at(app.world.state().entities[0].position);
    assert!(app.selected_resource.is_none());
    assert_eq!(app.selected, BTreeSet::from([EntityId(1)]));
}

#[test]
fn invincible_enemy_objectives_receive_move_orders_instead_of_attacks() {
    let mut app = demo();
    let mut rules = app.world.rules().clone();
    let objective = rules
        .units
        .iter_mut()
        .find(|u| u.id == UnitTypeId(1))
        .unwrap();
    objective.blocks_movement = false;
    objective.speed = 0;
    objective.weapon = None;
    let mut map = app.world.map().clone();
    map.spawns = vec![
        straterust_engine::sim::Spawn {
            owner: PlayerId(0),
            unit_type: UnitTypeId(2),
            position: Position { x: 96, y: 96 },
            ..Default::default()
        },
        straterust_engine::sim::Spawn {
            owner: PlayerId(1),
            unit_type: UnitTypeId(1),
            position: Position { x: 192, y: 96 },
            invincible: true,
            ..Default::default()
        },
    ];
    map.resources.clear();
    map.terrain = None;
    map.fog_of_war = false;
    map.initial_explored.clear();
    app.world = World::new(rules, map, 42).unwrap();
    app.selected = BTreeSet::from([EntityId(1)]);
    app.contextual_order(Position { x: 192, y: 96 }).unwrap();
    assert!(matches!(
        app.recorded.last().unwrap().order,
        Order::Move {
            entity: EntityId(1),
            target: Position { x: 192, y: 96 }
        }
    ));
    for _ in 0..100 {
        step(&mut app);
    }
    assert_eq!(
        app.world.state().entities[0].position,
        Position { x: 192, y: 96 }
    );
}
