use super::*;
use crate::visual::CommandTarget;
use std::time::Duration;

#[test]
fn neutral_wildlife_gets_move_on_right_click_and_explicit_attack_on_attack_click() {
    let mut app = demo();
    let mut rules = app.world.rules().clone();
    let mut wildlife = rules.units[0].clone();
    wildlife.id = UnitTypeId(200);
    wildlife.neutral = true;
    wildlife.weapon = None;
    rules.units.push(wildlife);
    let mut map = app.world.map().clone();
    map.fog_of_war = false;
    map.ai.clear();
    map.mission = None;
    map.spawns = vec![
        straterust_engine::sim::Spawn {
            owner: PlayerId(0),
            unit_type: UnitTypeId(1),
            position: Position { x: 96, y: 96 },
            ..Default::default()
        },
        straterust_engine::sim::Spawn {
            owner: PlayerId(1),
            unit_type: UnitTypeId(200),
            position: Position { x: 192, y: 96 },
            ..Default::default()
        },
    ];
    map.resources.clear();
    map.terrain = None;
    app.world = World::new(rules, map, 42).unwrap();
    app.selected = BTreeSet::from([EntityId(1)]);
    let wildlife = app.world.state().entities[1].clone();
    app.contextual_order(wildlife.position).unwrap();
    assert!(matches!(
        app.recorded.last().unwrap().order,
        Order::Move { .. }
    ));
    app.target_mode = Some(TargetMode::AttackMove);
    app.targeting_click(wildlife.position).unwrap();
    assert!(matches!(
        app.recorded.last().unwrap().order,
        Order::Attack {
            target: EntityId(2),
            ..
        }
    ));
}

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

#[test]
fn another_worker_right_clicks_active_construction_to_assist_instead_of_replacing_the_builder() {
    let mut app = demo();
    let mut rules = app.world.rules().clone();
    rules.victory = false;
    rules.repair = Some(straterust_engine::sim::RepairRules {
        rate_numerator: 1,
        rate_denominator: 1,
        cost_divisor: 2,
        range: 16,
    });
    rules
        .units
        .iter_mut()
        .find(|u| u.id == UnitTypeId(2))
        .unwrap()
        .repairs = vec![UnitTypeId(4)];
    let building = rules
        .units
        .iter_mut()
        .find(|u| u.id == UnitTypeId(4))
        .unwrap();
    building.builder_inside = true;
    building.repair_construction = true;
    let mut map = app.world.map().clone();
    map.spawns.push(straterust_engine::sim::Spawn {
        unit_type: UnitTypeId(2),
        position: Position { x: 500, y: 384 },
        ..Default::default()
    });
    app.world = World::new(rules, map, 42).unwrap();
    app.selected = [EntityId(2)].into();
    app.activate(Action::Build(UnitTypeId(4))).unwrap();
    let position = Position { x: 384, y: 384 };
    app.targeting_click(position).unwrap();
    for _ in 0..100 {
        step(&mut app);
        if app.world.state().entities.iter().any(|e| {
            e.construction
                .as_ref()
                .is_some_and(|c| c.work_position.is_some())
        }) {
            break;
        }
    }
    let foundation = app
        .world
        .state()
        .entities
        .iter()
        .find(|e| e.construction.is_some())
        .unwrap()
        .clone();
    assert!(!app.world.entity_visible(PlayerId(0), EntityId(2)));
    app.selected = [EntityId(4)].into();
    app.contextual_order(foundation.position).unwrap();
    assert_eq!(
        app.recorded.last().unwrap().order,
        Order::Repair {
            entity: EntityId(4),
            target: foundation.id
        }
    );
    step(&mut app);
    assert_eq!(
        app.world
            .state()
            .entities
            .iter()
            .find(|e| e.id == foundation.id)
            .unwrap()
            .construction
            .as_ref()
            .unwrap()
            .worker,
        Some(EntityId(2))
    );
    app.activate(Action::Repair).unwrap();
    app.targeting_click(foundation.position).unwrap();
    assert!(matches!(
        app.recorded.last().unwrap().order,
        Order::Repair { .. }
    ));
}
