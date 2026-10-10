//! Opt-in checks against privately imported content. No retail assets are fixtures.
use std::path::{Path, PathBuf};
use straterust_engine::{
    content::{Campaign, Package},
    map::*,
    sim::*,
};

pub(super) fn root() -> PathBuf {
    PathBuf::from(std::env::var_os("STRATERUST_WARCRAFT2").expect("set STRATERUST_WARCRAFT2"))
}
pub(super) fn load(root: &Path, race: &str, number: usize) -> World {
    Package::load(&root.join(race).join(format!("mission{number:02}")))
        .unwrap()
        .world(7)
        .unwrap()
}
fn send(w: &mut World, order: Order) {
    let command = Command {
        tick: w.tick(),
        player: PlayerId(0),
        sequence: w.state().last_sequences[0] + 1,
        order,
    };
    let result = w.step(&[command]).unwrap();
    assert!(result.iter().all(|o| o.rejection.is_none()), "{result:?}");
}
fn until(w: &mut World, limit: usize, condition: impl Fn(&World) -> bool) {
    for _ in 0..limit {
        if condition(w) {
            return;
        }
        w.step(&[]).unwrap();
    }
    assert!(
        condition(w),
        "condition not reached at tick {}: {:?}",
        w.tick().0,
        w.state()
            .entities
            .iter()
            .filter(|e| e.owner == PlayerId(0) && !w.unit_type(e.unit_type).unwrap().structure)
            .map(|e| (
                e.id,
                e.position,
                &e.order,
                &e.cargo,
                e.harvest_progress,
                e.garrisoned_in
            ))
            .collect::<Vec<_>>()
    );
}
fn entity(w: &World, unit: usize) -> EntityId {
    w.state()
        .entities
        .iter()
        .find(|e| e.owner == PlayerId(0) && e.unit_type == super::stats::id(unit))
        .unwrap()
        .id
}
fn near_build(w: &World, worker: EntityId, unit: UnitTypeId, origin: Position) -> Position {
    let mut positions: Vec<_> = (1..=12_i32)
        .flat_map(|r| {
            (-r..=r).flat_map(move |y| {
                (-r..=r)
                    .filter(move |x| x.abs() == r || y.abs() == r)
                    .map(move |x| Position {
                        x: origin.x / 32 * 32 + x * 32,
                        y: origin.y / 32 * 32 + y * 32,
                    })
            })
        })
        .filter(|p| w.build_rejection(PlayerId(0), worker, unit, *p).is_none())
        .collect();
    positions.sort_by_key(|p| {
        (
            i64::from(p.x - origin.x).pow(2) + i64::from(p.y - origin.y).pow(2),
            p.x,
            p.y,
        )
    });
    *positions
        .first()
        .expect("a buildable point around the town")
}

#[test]
#[ignore = "requires STRATERUST_WARCRAFT2 pointing to a retail import"]
fn every_campaign_initializes_and_both_opening_economies_reach_victory() {
    let root = root();
    for folder in ["human", "orc", "human-expansion", "orc-expansion"] {
        let campaign = Campaign::load(&root.join(folder)).unwrap();
        for n in 1..=campaign.missions.len() {
            let mut w = load(&root, folder, n);
            for _ in 0..3 {
                w.step(&[]).unwrap();
            }
            assert!(
                w.state().defeated.is_empty(),
                "{folder} mission {n} loses immediately"
            );
            assert!(
                w.state().winner.is_none(),
                "{folder} mission {n} wins immediately"
            );
            w.restore_snapshot(w.save_snapshot().unwrap()).unwrap();
        }
    }
    for (folder, race) in [("human", 0), ("orc", 1)] {
        let mut w = load(&root, folder, 1);
        w.step(&[]).unwrap();
        let worker = entity(&w, 2 + race);
        let origin = w
            .state()
            .entities
            .iter()
            .find(|e| e.id == worker)
            .unwrap()
            .position;
        let gold = w
            .state()
            .resources
            .iter()
            .filter(|r| r.kind == "gold")
            .min_by_key(|r| {
                i64::from(r.position.x - origin.x).pow(2)
                    + i64::from(r.position.y - origin.y).pow(2)
            })
            .unwrap()
            .id;
        let before = w.resource_balance(PlayerId(0), "gold");
        send(
            &mut w,
            Order::Gather {
                entity: worker,
                resource: gold,
            },
        );
        until(&mut w, 3000, |w| {
            w.resource_balance(PlayerId(0), "gold") > before
        });
        // Fund the actual campaign goal through its native economy. Keep one
        // worker on gold and another on lumber before assigning the builder.
        let town = entity(&w, 74 + race);
        send(
            &mut w,
            Order::Train {
                entity: town,
                unit_type: super::stats::id(2 + race),
            },
        );
        until(&mut w, 1000, |w| {
            w.state()
                .entities
                .iter()
                .filter(|e| e.owner == PlayerId(0) && e.unit_type == super::stats::id(2 + race))
                .count()
                >= 2
        });
        let workers: Vec<_> = w
            .state()
            .entities
            .iter()
            .filter(|e| {
                e.owner == PlayerId(0) && w.unit_type(e.unit_type).unwrap().worker.is_some()
            })
            .map(|e| e.id)
            .collect();
        assert!(workers.len() >= 2);
        let lumber = w
            .state()
            .resources
            .iter()
            .filter(|r| r.kind == "wood" && w.gather_rejection(workers[1], r.id).is_none())
            .min_by_key(|r| {
                i64::from(r.position.x - origin.x).pow(2)
                    + i64::from(r.position.y - origin.y).pow(2)
            })
            .unwrap()
            .id;
        send(
            &mut w,
            Order::Gather {
                entity: workers[1],
                resource: lumber,
            },
        );
        until(&mut w, 12000, |w| {
            w.resource_balance(PlayerId(0), "gold") >= 3000
                && w.resource_balance(PlayerId(0), "wood") >= 1500
        });
        send(&mut w, Order::Stop { entity: worker });
        for (kind, count) in [(58 + race, 4), (60 + race, 1)] {
            while w
                .state()
                .entities
                .iter()
                .filter(|e| {
                    e.owner == PlayerId(0)
                        && e.unit_type == super::stats::id(kind)
                        && e.construction.is_none()
                })
                .count()
                < count
            {
                let unit_type = super::stats::id(kind);
                let position = near_build(&w, worker, unit_type, origin);
                send(
                    &mut w,
                    Order::Build {
                        entity: worker,
                        unit_type,
                        position,
                    },
                );
                until(&mut w, 4000, |w| {
                    w.state().entities.iter().any(|e| {
                        e.owner == PlayerId(0)
                            && e.unit_type == unit_type
                            && e.position == position
                            && e.construction.is_none()
                    })
                });
            }
        }
        until(&mut w, 3, |w| w.state().winner == Some(PlayerId(0)));
    }
}

fn coast_world(race: usize) -> World {
    let original = load(&root(), if race == 0 { "human" } else { "orc" }, 14);
    let mut rules = original.rules().clone();
    rules.starting_resources = ["gold", "wood", "oil"]
        .into_iter()
        .map(|kind| ResourceAmount {
            kind: kind.into(),
            amount: 10000,
        })
        .collect();
    let spawn = |kind, x, y| Spawn {
        owner: PlayerId(0),
        unit_type: super::stats::id(kind + race),
        position: Position { x, y },
        ..Default::default()
    };
    let mut flags = vec![0; 32 * 32];
    for y in 0..32 {
        for x in 0..32 {
            flags[y * 32 + x] = if x < 16 { WALKABLE | BUILDABLE } else { WATER };
        }
    }
    World::new(
        rules,
        Map {
            id: "native-warcraft-coast-check".into(),
            width: 1024,
            height: 1024,
            players: 2,
            spawns: vec![
                spawn(74, 160, 160),
                spawn(2, 320, 320),
                spawn(72, 512, 256),
                spawn(26, 608, 256),
                spawn(0, 448, 480),
                spawn(28, 608, 480),
            ],
            start_locations: vec![],
            resources: vec![ResourceSpawn {
                terrain_corners: None,
                kind: "oil".into(),
                position: Position { x: 800, y: 256 },
                amount: 10000,
                footprint: Footprint {
                    width: 96,
                    height: 96,
                },
                requires_extractor: true,
            }],
            fog_of_war: false,
            creation: Default::default(),
            initial_explored: Default::default(),
            ai: vec![],
            mission: None,
            terrain: Some(Terrain {
                cell_size: 32,
                columns: 32,
                rows: 32,
                flags,
            }),
        },
        7,
    )
    .unwrap()
}

#[test]
#[ignore = "requires STRATERUST_WARCRAFT2 pointing to a retail import"]
fn native_tankers_build_and_gather_oil_and_transports_load_and_unload_on_coast() {
    for race in 0..2 {
        let mut w = coast_world(race);
        let tanker = entity(&w, 26 + race);
        let position = Position { x: 800, y: 256 };
        send(
            &mut w,
            Order::Build {
                entity: tanker,
                unit_type: super::stats::id(86 + race),
                position,
            },
        );
        until(&mut w, 4000, |w| {
            w.state()
                .entities
                .iter()
                .any(|e| e.unit_type == super::stats::id(86 + race) && e.construction.is_none())
        });
        let before = w.resource_balance(PlayerId(0), "oil");
        let resource = w.state().resources[0].id;
        send(
            &mut w,
            Order::Gather {
                entity: tanker,
                resource,
            },
        );
        until(&mut w, 3000, |w| {
            w.resource_balance(PlayerId(0), "oil") > before
        });
        let passenger = entity(&w, race);
        let ship = entity(&w, 28 + race);
        send(
            &mut w,
            Order::Load {
                entity: passenger,
                target: ship,
            },
        );
        until(&mut w, 1000, |w| {
            w.state()
                .entities
                .iter()
                .find(|e| e.id == passenger)
                .unwrap()
                .garrisoned_in
                == Some(ship)
        });
        send(
            &mut w,
            Order::UnloadAt {
                entity: ship,
                target: Position { x: 544, y: 704 },
            },
        );
        until(&mut w, 1000, |w| {
            w.state()
                .entities
                .iter()
                .find(|e| e.id == passenger)
                .unwrap()
                .garrisoned_in
                .is_none()
        });
        let p = w
            .state()
            .entities
            .iter()
            .find(|e| e.id == passenger)
            .unwrap();
        assert!(
            p.position.y > 600 && p.position.x < 512,
            "unloaded at {:?}",
            p.position
        );
    }
}

#[test]
#[ignore = "requires STRATERUST_WARCRAFT2 pointing to a retail import"]
fn late_game_research_promotes_troops_and_unlocks_usable_spells() {
    for race in 0..2 {
        let native = load(&root(), if race == 0 { "human" } else { "orc" }, 14);
        let mut rules = native.rules().clone();
        rules.starting_resources = ["gold", "wood", "oil"]
            .into_iter()
            .map(|kind| ResourceAmount {
                kind: kind.into(),
                amount: 100000,
            })
            .collect();
        let mut map = coast_world(race).map().clone();
        map.terrain = None;
        map.resources.clear();
        map.spawns = [
            (90, 128, 128),
            (62, 320, 128),
            (76, 512, 128),
            (60, 704, 128),
            (6, 128, 384),
            (8, 256, 384),
            (0, 384, 384),
            (80, 704, 384),
            (10, 704, 576),
            (58, 128, 704),
            (58, 320, 704),
        ]
        .into_iter()
        .map(|(source, x, y)| Spawn {
            owner: PlayerId(0),
            unit_type: super::stats::id(source + race),
            position: Position { x, y },
            hp_percent: (source == 0).then_some(50),
            energy_percent: Some(100),
            ..Default::default()
        })
        .collect();
        let mut w = World::new(rules, map, 7).unwrap();
        let cavalry = entity(&w, 6 + race);
        let archer = entity(&w, 8 + race);
        for ordinal in [17, 13, 14, 15, 18] {
            let research = ResearchId((race * 64 + ordinal) as u16);
            let facility = w
                .rules()
                .research
                .iter()
                .find(|r| r.id == research)
                .unwrap()
                .facility;
            let producer = w
                .state()
                .entities
                .iter()
                .find(|e| e.unit_type == facility)
                .unwrap()
                .id;
            send(
                &mut w,
                Order::Research {
                    entity: producer,
                    research,
                },
            );
            until(&mut w, 2000, |w| w.has_research(PlayerId(0), research));
        }
        assert_eq!(
            w.state()
                .entities
                .iter()
                .find(|e| e.id == cavalry)
                .unwrap()
                .unit_type,
            super::stats::id(12 + race)
        );
        let ranger = w.state().entities.iter().find(|e| e.id == archer).unwrap();
        assert_eq!(ranger.unit_type, super::stats::id(18 + race));
        assert_eq!(
            w.vision_range(ranger),
            w.unit_type(ranger.unit_type).unwrap().vision_range + 128
        );
        let barracks = entity(&w, 60 + race);
        send(
            &mut w,
            Order::Train {
                entity: barracks,
                unit_type: super::stats::id(18 + race),
            },
        );
        until(&mut w, 1000, |w| {
            w.state()
                .entities
                .iter()
                .filter(|e| e.unit_type == super::stats::id(18 + race))
                .count()
                == 2
        });
        let soldier = entity(&w, race);
        let before = w
            .state()
            .entities
            .iter()
            .find(|e| e.id == soldier)
            .unwrap()
            .hp;
        let ability = AbilityId(if race == 0 { 2 } else { 5 });
        send(
            &mut w,
            Order::Cast {
                entity: cavalry,
                ability,
                target: AbilityTarget::Unit(soldier),
            },
        );
        until(&mut w, 1000, |w| {
            let target = w.state().entities.iter().find(|e| e.id == soldier).unwrap();
            if race == 0 {
                target.hp > before
            } else {
                target.ability_auras.iter().any(|a| a.ability == ability)
            }
        });
        let caster = entity(&w, 10 + race);
        // Free starting spells use different delivery systems: a moving fireball
        // and a delayed draining projectile. Their native orders survive saves.
        let mut enemy = Spawn {
            owner: PlayerId(1),
            unit_type: super::stats::id(74 + 1 - race),
            position: Position { x: 704, y: 864 },
            ..Default::default()
        };
        if race == 1 {
            enemy.unit_type = super::stats::id(6);
        }
        let mut map = w.map().clone();
        let aim = enemy.position;
        map.spawns.push(enemy);
        let mut casting = World::new(w.rules().clone(), map, 7).unwrap();
        let victim = casting.state().entities.last().unwrap().id;
        let ability = AbilityId(if race == 0 { 7 } else { 13 });
        let target = if race == 0 {
            AbilityTarget::Point(aim)
        } else {
            AbilityTarget::Unit(victim)
        };
        send(
            &mut casting,
            Order::Cast {
                entity: caster,
                ability,
                target,
            },
        );
        until(&mut casting, 1000, |w| {
            !w.state().ability_fields.is_empty() || !w.state().pending_effects.is_empty()
        });
        let mut restored = casting
            .restore_snapshot(casting.save_snapshot().unwrap())
            .unwrap();
        for _ in 0..40 {
            casting.step(&[]).unwrap();
            restored.step(&[]).unwrap();
            assert_eq!(casting.state_hash(), restored.state_hash());
        }
        assert!(
            casting
                .state()
                .entities
                .iter()
                .find(|e| e.id == victim)
                .is_none_or(|e| e.hp < casting.unit_type(e.unit_type).unwrap().max_hp)
        );
    }
}

#[test]
#[ignore = "requires STRATERUST_WARCRAFT2 pointing to a refreshed native import"]
fn native_builders_stay_inside_and_repair_workers_speed_construction_for_both_races() {
    for (race, folder) in [(0, "human"), (1, "orc")] {
        let initial = load(&root(), folder, 2);
        let worker = super::stats::id(2 + race);
        let farm = super::stats::id(58 + race);
        assert!(initial.unit_type(farm).unwrap().builder_inside);
        assert!(initial.unit_type(farm).unwrap().repair_construction);
        let mut rules = initial.rules().clone();
        rules.victory = false;
        rules.starting_resources = ["gold", "wood", "oil"]
            .map(|kind| ResourceAmount {
                kind: kind.into(),
                amount: 10000,
            })
            .to_vec();
        let mut map = initial.map().clone();
        map.ai.clear();
        map.mission = None;
        map.terrain = None;
        map.fog_of_war = false;
        map.creation.clear();
        map.resources.clear();
        map.spawns = vec![
            Spawn {
                unit_type: worker,
                position: Position { x: 224, y: 192 },
                ..Default::default()
            },
            Spawn {
                unit_type: worker,
                position: Position { x: 320, y: 192 },
                ..Default::default()
            },
        ];
        let mut w = World::new(rules, map, 7).unwrap();
        send(
            &mut w,
            Order::Build {
                entity: EntityId(1),
                unit_type: farm,
                position: Position { x: 272, y: 192 },
            },
        );
        until(&mut w, 100, |w| !w.entity_visible(PlayerId(0), EntityId(1)));
        let mut solo = w.clone();
        send(
            &mut w,
            Order::Repair {
                entity: EntityId(2),
                target: EntityId(3),
            },
        );
        solo.step(&[]).unwrap();
        let remaining = |w: &World| {
            w.state().entities[2]
                .construction
                .as_ref()
                .map_or(0, |c| c.remaining)
        };
        for _ in 0..30 {
            w.step(&[]).unwrap();
            solo.step(&[]).unwrap();
        }
        assert!(
            remaining(&solo) >= remaining(&w) + 20,
            "{folder} helpers must speed progress"
        );
        assert_eq!(
            w.state().entities[2].construction.as_ref().unwrap().worker,
            Some(EntityId(1))
        );
        w = w.restore_snapshot(w.save_snapshot().unwrap()).unwrap();
        until(&mut w, 3000, |w| {
            w.state().entities[2].construction.is_none()
        });
        w.step(&[]).unwrap();
        assert!(w.entity_visible(PlayerId(0), EntityId(1)));
        assert_eq!(w.state().entities[0].order, UnitOrder::Idle);
        let actor = &w.state().entities[0];
        assert!(w.can_place(
            actor.position,
            w.unit_type(worker).unwrap().footprint,
            MovementClass::Ground,
            Some(actor.id)
        ));
    }
}
