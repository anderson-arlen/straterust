use super::*;
use crate::visual::Visuals;
use std::time::Duration;
use straterust_engine::{
    assets::{Effect, EffectManifest, Image, Projectile, ProjectileManifest},
    sim::{AbilityEffect, AbilityId, AbilityTarget, Spawn, TargetedAbility},
};

fn combat_app() -> App {
    let mut app = demo();
    let mut rules = app.world.rules().clone();
    rules.victory = false;
    let shooter = rules
        .units
        .iter_mut()
        .find(|u| u.id == UnitTypeId(1))
        .unwrap();
    shooter.vision_range = 512;
    let weapon = shooter.weapon.as_mut().unwrap();
    weapon.projectile_speed = 8 * 256;
    weapon.range = 512;
    weapon.cooldown = 1000;
    let mut map = app.world.map().clone();
    map.terrain = None;
    map.resources.clear();
    map.ai.clear();
    map.mission = None;
    map.fog_of_war = false;
    map.spawns = vec![
        Spawn {
            unit_type: UnitTypeId(1),
            position: Position { x: 100, y: 200 },
            ..Default::default()
        },
        Spawn {
            owner: PlayerId(1),
            unit_type: UnitTypeId(3),
            position: Position { x: 352, y: 200 },
            ..Default::default()
        },
    ];
    app.world = World::new(rules, map, 42).unwrap();
    app.visuals = Visuals::new(&app.world);
    app
}

fn projectile(arc_height: u32) -> Projectile {
    let effect = EffectManifest {
        frame_ms: 100,
        anchor: [0, 0],
        frames: vec![],
        sequence: vec![0],
    };
    let animation = || Effect {
        frame_ms: 100,
        anchor: [0, 0],
        frames: vec![Image {
            width: 1,
            height: 1,
            rgba: vec![255; 4],
        }],
        sequence: vec![0],
    };
    Projectile {
        manifest: ProjectileManifest {
            unit_type: UnitTypeId(1),
            targets_air: false,
            ability: None,
            pattern: None,
            directional: false,
            speed_fp8: 8 * 256,
            forward_offset: 12,
            launch_offsets: vec![],
            arc_height,
            on_target: false,
            charge: None,
            marker: None,
            flight: effect.clone(),
            impact: effect,
            trail: None,
        },
        charge: None,
        marker: None,
        flight: animation(),
        impact: animation(),
        trail: None,
    }
}

#[test]
fn explicit_attack_hits_own_unit_without_right_click_attacks_or_self_targeting() {
    use crate::visual::VisualAction;
    let mut app = combat_app();
    let mut rules = app.world.rules().clone();
    rules
        .units
        .iter_mut()
        .find(|u| u.id == UnitTypeId(2))
        .unwrap()
        .weapon = None;
    let mut map = app.world.map().clone();
    map.spawns[1].owner = PlayerId(0);
    map.spawns[1].unit_type = UnitTypeId(1);
    map.spawns.push(Spawn {
        unit_type: UnitTypeId(2),
        position: Position { x: 224, y: 300 },
        ..Default::default()
    });
    app.world = World::new(rules, map, 42).unwrap();
    app.visuals = Visuals::new(&app.world);
    app.selected = [EntityId(1), EntityId(2), EntityId(3)].into();
    let position = app.world.state().entities[1].position;
    let hp = app.world.state().entities[1].hp;
    app.contextual_order(position).unwrap();
    assert!(
        app.recorded
            .iter()
            .all(|c| matches!(c.order, Order::Move { .. }))
    );
    let before = app.recorded.len();
    app.activate(Action::AttackMove).unwrap();
    app.targeting_click(position).unwrap();
    assert_eq!(
        app.recorded.len(),
        before + 1,
        "skip the selected victim and unarmed group members"
    );
    assert_eq!(
        app.recorded.last().unwrap().order,
        Order::Attack {
            entity: EntityId(1),
            target: EntityId(2)
        }
    );
    step(&mut app);
    app.visuals.update(&app.world);
    assert_eq!(
        app.visuals.get(EntityId(1)).unwrap().action,
        VisualAction::Attack
    );
    assert_eq!(
        app.world.state().entities[0].last_attack_target,
        Some(EntityId(2))
    );
    for _ in 0..40 {
        step(&mut app);
        app.visuals.update(&app.world);
    }
    assert!(app.world.state().entities[1].hp < hp);
    assert_eq!(
        app.world.state().entities[1].last_attack_target,
        None,
        "friendly fire must not trigger friendly automatic retaliation"
    );
}

#[test]
fn explicit_and_contextual_repairs_accept_allies_in_the_filtered_player_view() {
    use straterust_engine::multiplayer::{GameType, MatchRules};
    let mut app = damaged_base(3000);
    let mut map = app.world.map().clone();
    map.players = 3;
    map.ai.clear();
    map.mission = None;
    map.fog_of_war = false;
    map.spawns[0].owner = PlayerId(1);
    map.spawns[2].owner = PlayerId(2);
    map.multiplayer = Some(MatchRules {
        participants: vec![PlayerId(0), PlayerId(1), PlayerId(2)],
        teams: vec![vec![PlayerId(0), PlayerId(1)], vec![PlayerId(2)]],
        mode: GameType::Teams,
        parameter: 1,
        resources: Default::default(),
        bases: vec![],
        cheats: false,
        ai_difficulty: None,
        flags: vec![],
        defeat_without_structures: false,
    });
    let world = World::new(app.world.rules().clone(), map, 42).unwrap();
    app.world = world
        .player_view(PlayerId(0))
        .unwrap()
        .into_world(&world)
        .unwrap();
    app.visuals = Visuals::new(&app.world);
    let position = app.world.state().entities[0].position;
    app.contextual_order(position).unwrap();
    assert_eq!(
        app.recorded.last().unwrap().order,
        Order::Repair {
            entity: EntityId(2),
            target: EntityId(1),
        }
    );
    app.bound_key(KeyCode::KeyR).unwrap();
    app.keys.insert(KeyCode::ShiftLeft);
    app.targeting_click(position).unwrap();
    assert_eq!(
        app.recorded.last().unwrap().order,
        Order::Queue {
            entity: EntityId(2),
            order: UnitOrder::Repair {
                target: EntityId(1)
            },
        }
    );
    // Commands are chosen from the filtered view and executed by the server.
    app.world = world;
    step(&mut app);
    assert_eq!(app.world.state().entities[0].owner, PlayerId(1));
}

#[test]
fn siege_damage_visual_impact_and_sound_agree_at_slow_normal_and_fast_game_rates() {
    for rate in [0.5, 1.0, 2.0] {
        for arc in [0, 32] {
            let mut app = combat_app();
            app.config.game_speed = rate;
            add_resource_art(&mut app);
            app.assets.as_mut().unwrap().projectiles = vec![projectile(arc)];
            let hp = app.world.state().entities[1].hp;
            app.world
                .step(&[crate::Command {
                    tick: app.world.tick(),
                    player: PlayerId(0),
                    sequence: 1,
                    order: Order::Attack {
                        entity: EntityId(1),
                        target: EntityId(2),
                    },
                }])
                .unwrap();
            app.visuals.update(&app.world);
            assert_eq!(app.world.state().entities[1].hp, hp);
            let frame =
                Duration::from_secs_f64(f64::from(app.world.rules().tick_ms) / 1000.0 / rate);
            let mut hit = false;
            for _ in 0..40 {
                app.advance_presentation(frame);
                app.world.step(&[]).unwrap();
                app.visuals.update(&app.world);
                let effect = &app.assets.as_ref().unwrap().projectiles[0];
                let shot = &app.visuals.projectiles()[0];
                let (_, position) = shot.sample(effect).unwrap();
                let impacts = app
                    .audio
                    .events
                    .iter()
                    .filter(|e| e.0 == crate::audio::Cue::Impact)
                    .count();
                if app.world.state().entities[1].hp < hp {
                    assert_eq!(position, [352., 200.], "rate {rate}, arc {arc}");
                    assert_eq!(impacts, 1);
                    hit = true;
                    break;
                }
                assert_ne!(
                    position,
                    [352., 200.],
                    "visual impact preceded damage at {rate}"
                );
                assert_eq!(impacts, 0);
            }
            assert!(hit);
        }
    }
}

#[test]
fn demolish_button_can_target_a_building_directly() {
    let mut app = combat_app();
    let mut rules = app.world.rules().clone();
    rules
        .units
        .iter_mut()
        .find(|u| u.id == UnitTypeId(1))
        .unwrap()
        .abilities
        .push(TargetedAbility {
            id: AbilityId(99),
            research: None,
            energy: 0,
            range: 32,
            effect: AbilityEffect::Demolish {
                radius: 96,
                damage: 400,
                resources: vec!["wood".into(), "rock".into()],
            },
        });
    app.world = World::new(rules, app.world.map().clone(), 42).unwrap();
    let hp = app.world.state().entities[1].hp;
    app.selected = [EntityId(1)].into();
    app.target_mode = Some(TargetMode::Cast(AbilityId(99)));
    app.targeting_click(Position { x: 352, y: 200 }).unwrap();
    assert!(matches!(
        app.recorded.last().unwrap().order,
        Order::Cast {
            target: AbilityTarget::Unit(EntityId(2)),
            ..
        }
    ));
    for _ in 0..100 {
        step(&mut app);
    }
    assert!(
        !app.world
            .state()
            .entities
            .iter()
            .any(|e| e.id == EntityId(1))
    );
    assert_eq!(
        app.world
            .state()
            .entities
            .iter()
            .find(|e| e.id == EntityId(2))
            .map_or(0, |e| e.hp),
        hp.saturating_sub(400)
    );
}
