//! Hidden attackers still cause audible, visible impacts on disclosed targets.
use super::*;
use crate::audio::{Audio, Cue};
use straterust_engine::{
    assets::{Effect, EffectManifest, ProjectileManifest},
    session::{PlayerUpdate, ServerSession},
    sim::{MovementClass, Spawn, UnitTypeId, WeaponStrike},
};

#[test]
fn air_attack_feedback_uses_air_visibility_above_cliffs() {
    use straterust_engine::{
        map::{HEIGHT_SHIFT, Terrain, WALKABLE},
        sim::{Command, Order, UnitType, Visibility},
    };
    for air in [false, true] {
        for hidden_source in [false, true] {
            let original = super::tests::world();
            let mut rules = original.rules().clone();
            rules.victory = false;
            let attacker = &mut rules.units[0];
            attacker.speed = 0;
            attacker.vision_range = 224;
            // An air observer can fire on a plateau's ground occupants, but
            // that must not disclose those occupants to the ground viewer.
            attacker.movement_class = if air {
                MovementClass::Ground
            } else {
                MovementClass::Air
            };
            let weapon = attacker.weapon.as_mut().unwrap();
            weapon.range = 160;
            weapon.targets_air = true;
            rules.units.extend([
                UnitType {
                    id: UnitTypeId(2),
                    speed: 0,
                    max_hp: 100,
                    vision_range: 0,
                    movement_class: if air {
                        MovementClass::Air
                    } else {
                        MovementClass::Ground
                    },
                    ..Default::default()
                },
                UnitType {
                    id: UnitTypeId(3),
                    vision_range: 224,
                    ..Default::default()
                },
            ]);
            let target = Position { x: 112, y: 112 };
            let source = Position {
                x: if hidden_source { 208 } else { 80 },
                y: 112,
            };
            let mut map = original.map().clone();
            map.width = 256;
            map.height = 256;
            map.players = 4;
            map.fog_of_war = true;
            map.terrain = Some(Terrain {
                cell_size: 8,
                columns: 32,
                rows: 32,
                flags: (0..32 * 32)
                    .map(|i| WALKABLE | if i % 32 >= 12 { 1 << HEIGHT_SHIFT } else { 0 })
                    .collect(),
            });
            map.spawns = [
                (1, 1, source),
                (2, 2, target),
                (0, 3, Position { x: 48, y: 112 }),
            ]
            .map(|(owner, unit_type, position)| Spawn {
                owner: PlayerId(owner),
                unit_type: UnitTypeId(unit_type),
                position,
                ..Default::default()
            })
            .to_vec();
            let mut server = World::new(rules, map, 42).unwrap();
            assert_eq!(
                server.visibility(PlayerId(0), target),
                Visibility::Unexplored
            );
            assert_eq!(
                server.terrain_visibility(PlayerId(0), target),
                Visibility::Visible
            );
            assert_eq!(server.entity_visible(PlayerId(0), EntityId(2)), air);
            assert_eq!(
                server.entity_visible(PlayerId(0), EntityId(1)),
                !hidden_source
            );
            let initial = server
                .player_view(PlayerId(0))
                .unwrap()
                .into_world(&server)
                .unwrap();
            let mut visuals = Visuals::new(&initial);
            let mut audio = Audio::new(false);
            audio.reset(&initial);
            let outcomes = server
                .step(&[Command {
                    tick: server.tick(),
                    player: PlayerId(1),
                    sequence: 1,
                    order: Order::Attack {
                        entity: EntityId(1),
                        target: EntityId(2),
                    },
                }])
                .unwrap();
            assert!(outcomes[0].rejection.is_none());
            let view = server.player_view(PlayerId(0)).unwrap();
            let view = ron::from_str::<straterust_engine::sim::PlayerView>(
                &ron::ser::to_string(&view).unwrap(),
            )
            .unwrap();
            let firing = view.into_world(&server).unwrap();
            if hidden_source {
                assert!(!firing.state().entities.iter().any(|e| e.id == EntityId(1)));
                assert_eq!(
                    firing.public_weapon_feedback().len(),
                    if air { 2 } else { 0 }
                );
            } else {
                let actor = firing
                    .state()
                    .entities
                    .iter()
                    .find(|e| e.id == EntityId(1))
                    .unwrap();
                assert_eq!(actor.last_attack_position, air.then_some(target));
            }
            visuals.update(&firing);
            audio.observe(&firing);
            assert_eq!(visuals.projectiles().len(), usize::from(air));
            if air {
                let shot = &visuals.projectiles()[0];
                assert!(shot.targets_air);
                assert_eq!(shot.to, target);
                assert_eq!(shot.impact_only, hidden_source);
                assert!(
                    audio
                        .events
                        .contains(&(Cue::AttackAir, Some(UnitTypeId(1))))
                );
            }
            let unseen = server.player_view(PlayerId(3)).unwrap();
            assert!(unseen.entities.is_empty() && unseen.weapon_feedback.is_empty());
        }
    }
}

#[test]
fn hidden_source_hits_cross_the_wire_without_disclosing_the_attacker() {
    for air in [false, true] {
        for lethal in [false, true] {
            let original = super::tests::world();
            let mut rules = original.rules().clone();
            rules.victory = false;
            let attacker = &mut rules.units[0];
            attacker.speed = 0;
            attacker.vision_range = 320;
            let weapon = attacker.weapon.as_mut().unwrap();
            weapon.range = 384;
            weapon.cooldown = 100;
            weapon.targets_air = true;
            weapon.strikes = vec![WeaponStrike {
                delay: 1,
                forward: 0,
            }];
            rules.units.push(straterust_engine::sim::UnitType {
                id: UnitTypeId(2),
                speed: 0,
                max_hp: if lethal { 1 } else { 40 },
                max_shields: if lethal { 0 } else { 10 },
                vision_range: 64,
                movement_class: if air {
                    MovementClass::Air
                } else {
                    MovementClass::Ground
                },
                ..Default::default()
            });
            let mut map = original.map().clone();
            map.width = 512;
            map.height = 256;
            map.players = 3;
            map.fog_of_war = true;
            let victim = Position { x: 64, y: 64 };
            map.spawns = [
                (0, 2, victim),
                (1, 1, Position { x: 256, y: 64 }),
                (2, 2, Position { x: 448, y: 192 }),
            ]
            .map(|(owner, unit_type, position)| Spawn {
                owner: PlayerId(owner),
                unit_type: UnitTypeId(unit_type),
                position,
                ..Default::default()
            })
            .to_vec();
            let definitions = World::new(rules, map, 42).unwrap();
            let mut server = ServerSession::new(
                definitions.snapshot(),
                42,
                vec![PlayerId(0), PlayerId(1), PlayerId(2)],
            )
            .unwrap();
            let initial = server
                .update(PlayerId(0), &[])
                .unwrap()
                .view
                .into_world(&definitions)
                .unwrap();
            assert!(!initial.state().entities.iter().any(|e| e.id == EntityId(2)));
            let mut audio = Audio::new(false);
            audio.reset(&initial);
            let mut visuals = Visuals::new(&initial);
            server.advance(&[]).unwrap();
            let firing = server.update(PlayerId(0), &[]).unwrap().view;
            assert_eq!(firing.weapon_feedback.len(), 1);
            assert!(!firing.weapon_feedback[0].impact);
            let firing = firing.into_world(&definitions).unwrap();
            audio.observe(&firing);
            visuals.update(&firing);
            assert!(audio.events.contains(&(
                if air { Cue::AttackAir } else { Cue::Attack },
                Some(UnitTypeId(1))
            )));
            assert!(visuals.projectiles().is_empty(), "sound starts when fired");
            let outcomes = server.advance(&[]).unwrap();
            let update = server.update(PlayerId(0), &outcomes).unwrap();
            let wire = ron::ser::to_string(&update).unwrap();
            let update: PlayerUpdate = ron::from_str(&wire).unwrap();
            assert_eq!(update.view.weapon_feedback.len(), 1);
            assert!(update.view.weapon_feedback[0].impact);
            let payload = ron::ser::to_string(&update.view.weapon_feedback[0]).unwrap();
            for secret in ["source", "heading", "origin", "256", "EntityId"] {
                assert!(!payload.contains(secret), "leaked {secret}: {payload}");
            }
            assert!(
                server
                    .update(PlayerId(2), &outcomes)
                    .unwrap()
                    .view
                    .weapon_feedback
                    .is_empty()
            );
            let client = update.view.into_world(&definitions).unwrap();
            assert!(!client.state().entities.iter().any(|e| e.id == EntityId(2)));
            audio.observe(&client);
            visuals.update(&client);
            assert!(audio.events.contains(&(
                if air { Cue::AttackAir } else { Cue::Attack },
                Some(UnitTypeId(1))
            )));
            assert_eq!(visuals.projectiles().len(), 1);
            let effect = |red| Effect {
                frame_ms: 42,
                anchor: [0, 0],
                sequence: vec![0],
                frames: vec![Image {
                    width: 1,
                    height: 1,
                    rgba: vec![red, 0, 0, 255],
                }],
            };
            let manifest = EffectManifest {
                frame_ms: 42,
                anchor: [0, 0],
                sequence: vec![0],
                frames: vec![],
            };
            let projectile = Projectile {
                manifest: ProjectileManifest {
                    ability: None,
                    unit_type: UnitTypeId(1),
                    targets_air: air,
                    speed_fp8: 256,
                    forward_offset: 0,
                    launch_offsets: Vec::new(),
                    arc_height: 0,
                    on_target: false,
                    directional: false,
                    charge: None,
                    marker: None,
                    flight: manifest.clone(),
                    impact: manifest,
                    trail: None,
                },
                charge: None,
                marker: None,
                flight: effect(1),
                impact: effect(2),
                trail: None,
            };
            let shot = &visuals.projectiles()[0];
            assert!(shot.impact_only);
            assert_eq!((shot.from, shot.to), (victim, victim));
            let (frame, position) = shot.sample(&projectile).unwrap();
            assert_eq!(frame.image.rgba[0], 2, "only impact artwork");
            assert_eq!(position, [64.0, 64.0]);
            audio.observe(&client);
            visuals.update(&client);
            assert_eq!(
                audio
                    .events
                    .iter()
                    .filter(|e| matches!(e.0, Cue::Attack | Cue::AttackAir))
                    .count(),
                1
            );
            assert_eq!(visuals.projectiles().len(), 1);
            server.advance(&[]).unwrap();
            assert!(
                server
                    .update(PlayerId(0), &[])
                    .unwrap()
                    .view
                    .weapon_feedback
                    .is_empty()
            );
        }
    }
}
