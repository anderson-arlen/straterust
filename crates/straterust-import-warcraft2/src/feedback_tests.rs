//! Short ordinary-command checks of reported cases against private native imports.
use super::{
    gameplay_tests::{load, root},
    stats::id,
};
use straterust_engine::session::{SavedGame, ServerSession};
use straterust_engine::sim::*;

fn send(world: &mut World, order: Order) {
    let outcomes = world
        .step(&[Command {
            tick: world.tick(),
            player: PlayerId(0),
            sequence: world.state().last_sequences[0] + 1,
            order,
        }])
        .unwrap();
    assert!(outcomes[0].rejection.is_none(), "{outcomes:?}");
}

#[test]
#[ignore = "requires STRATERUST_WARCRAFT2 pointing to a refreshed retail import"]
fn prebuilt_oil_deposits_resume_from_older_saves_and_survive_platform_destruction_and_rebuild() {
    let original = load(&root(), "human", 3);
    let platforms: Vec<_> = original
        .state()
        .entities
        .iter()
        .filter(|e| {
            original
                .unit_type(e.unit_type)
                .unwrap()
                .extracts
                .as_ref()
                .is_some_and(|x| x.resource == "oil")
        })
        .collect();
    assert_eq!(platforms.len(), 2);
    for platform in &platforms {
        assert!(
            original
                .state()
                .resources
                .iter()
                .any(|r| r.kind == "oil" && r.position == platform.position && r.amount > 0)
        );
    }
    let mut old_map = original.map().clone();
    old_map.resources.truncate(old_map.resources.len() - 2);
    let mut legacy = World::new(original.rules().clone(), old_map, 7).unwrap();
    for _ in 0..32 {
        legacy.step(&[]).unwrap();
    }
    let save =
        SavedGame::capture(&ServerSession::new(legacy.clone(), 7, vec![PlayerId(0)]).unwrap())
            .unwrap();
    let restored = ServerSession::restore_saved(&original, save).unwrap();
    assert_eq!(restored.world().tick(), legacy.tick());
    assert_eq!(
        &restored.world().state().resources[..legacy.state().resources.len()],
        legacy.state().resources
    );
    let node = original
        .state()
        .resources
        .iter()
        .find(|r| r.position == platforms[0].position && r.kind == "oil")
        .unwrap()
        .clone();
    let mut rules = original.rules().clone();
    rules.victory = false;
    rules.starting_resources = ["gold", "wood", "oil"]
        .map(|kind| ResourceAmount {
            kind: kind.into(),
            amount: 10000,
        })
        .to_vec();
    // A short ordinary shot tests destruction without a whole campaign battle.
    rules
        .units
        .iter_mut()
        .find(|u| u.id == platforms[0].unit_type)
        .unwrap()
        .max_hp = 1;
    let mut map = original.map().clone();
    map.ai.clear();
    map.mission = None;
    map.fog_of_war = false;
    map.creation.clear();
    map.spawns = vec![Spawn {
        owner: PlayerId(1),
        unit_type: platforms[0].unit_type,
        position: node.position,
        ..Default::default()
    }];
    let probe = World::new(rules.clone(), map.clone(), 7).unwrap();
    let battleship = rules.units.iter().find(|u| u.id == id(30)).unwrap();
    let ship_position = (-8..=8)
        .flat_map(|y| {
            (-8..=8).map(move |x| Position {
                x: node.position.x + 32 * x,
                y: node.position.y + 32 * y,
            })
        })
        .find(|p| {
            probe.can_place(*p, battleship.footprint, MovementClass::Water, None)
                && i64::from(p.x - node.position.x).pow(2) + i64::from(p.y - node.position.y).pow(2)
                    <= 192 * 192
        })
        .unwrap();
    map.spawns.push(Spawn {
        unit_type: id(30),
        position: ship_position,
        ..Default::default()
    });
    let probe = World::new(rules.clone(), map.clone(), 7).unwrap();
    let tanker = probe.unit_type(id(26)).unwrap();
    let position = (-8..=8)
        .flat_map(|y| {
            (-8..=8).map(move |x| Position {
                x: node.position.x + 32 * x,
                y: node.position.y + 32 * y,
            })
        })
        .find(|p| probe.can_place(*p, tanker.footprint, MovementClass::Water, None))
        .unwrap();
    map.spawns.push(Spawn {
        unit_type: id(26),
        position,
        ..Default::default()
    });
    let mut w = World::new(rules, map, 7).unwrap();
    assert_eq!(
        w.build_rejection(PlayerId(0), EntityId(3), id(86), node.position),
        Some(Rejection::InvalidPlacement),
        "an enemy platform already occupies this oil field"
    );
    send(
        &mut w,
        Order::Attack {
            entity: EntityId(2),
            target: EntityId(1),
        },
    );
    for _ in 0..1000 {
        if !w.state().entities.iter().any(|e| e.id == EntityId(1)) {
            break;
        }
        w.step(&[]).unwrap();
    }
    assert!(!w.state().entities.iter().any(|e| e.id == EntityId(1)));
    let remaining = w
        .state()
        .resources
        .iter()
        .find(|r| r.id == node.id)
        .unwrap();
    assert_eq!(remaining.amount, node.amount);
    // Continue this destruction through a checkpoint, then rebuild on the
    // surviving deposit rather than restarting the map.
    let mut rebuilt = w.restore_snapshot(w.save_snapshot().unwrap()).unwrap();
    send(
        &mut rebuilt,
        Order::Build {
            entity: EntityId(3),
            unit_type: id(86),
            position: node.position,
        },
    );
    for _ in 0..3000 {
        if rebuilt
            .state()
            .entities
            .iter()
            .any(|e| e.unit_type == id(86) && e.construction.is_none())
        {
            break;
        }
        rebuilt.step(&[]).unwrap();
    }
    assert!(
        rebuilt
            .state()
            .entities
            .iter()
            .any(|e| e.unit_type == id(86) && e.construction.is_none())
    );
    assert_eq!(
        rebuilt
            .state()
            .entities
            .iter()
            .find(|e| e.id == EntityId(3))
            .unwrap()
            .order,
        UnitOrder::Gather { resource: node.id },
        "the constructing tanker must gather without another user order"
    );
    for _ in 0..1000 {
        if rebuilt
            .state()
            .entities
            .iter()
            .find(|e| e.id == EntityId(3))
            .unwrap()
            .cargo
            .is_some()
        {
            break;
        }
        rebuilt.step(&[]).unwrap();
    }
    assert_eq!(
        rebuilt
            .state()
            .entities
            .iter()
            .find(|e| e.id == EntityId(3))
            .unwrap()
            .cargo
            .as_ref()
            .unwrap()
            .amount,
        100
    );
    assert_eq!(
        rebuilt
            .state()
            .resources
            .iter()
            .find(|r| r.id == node.id)
            .unwrap()
            .amount,
        node.amount - 100
    );
}

#[test]
#[ignore = "requires STRATERUST_WARCRAFT2 pointing to a refreshed retail import"]
fn real_wildlife_is_neutral_and_ignored_by_nearby_troops() {
    let original = load(&root(), "human", 1);
    let wildlife = original
        .rules()
        .units
        .iter()
        .find(|u| u.id == id(57))
        .unwrap();
    assert!(wildlife.neutral);
    assert!(wildlife.idle_wander.is_some());
    let mut map = original.map().clone();
    map.ai.clear();
    map.mission = None;
    map.fog_of_war = false;
    map.terrain = None;
    map.resources.clear();
    map.spawns = vec![
        Spawn {
            owner: PlayerId(0),
            unit_type: id(0),
            position: Position { x: 256, y: 256 },
            ..Default::default()
        },
        Spawn {
            owner: PlayerId(15),
            unit_type: id(57),
            position: Position { x: 288, y: 256 },
            ..Default::default()
        },
    ];
    let mut world = World::new(original.rules().clone(), map, 7).unwrap();
    for _ in 0..20 {
        world.step(&[]).unwrap();
        assert!(world.state().entities[0].auto_attack_target.is_none());
        assert_eq!(world.state().entities[1].hp, wildlife.max_hp);
    }
}

#[test]
#[ignore = "requires STRATERUST_WARCRAFT2 pointing to a refreshed retail import"]
fn real_mines_stay_lit_and_workers_deliver_inside_the_depot_for_both_races() {
    let root = root();
    for (race, folder) in [(0, "human"), (1, "orc")] {
        let mut world = load(&root, folder, 1);
        // Campaign initialization sets each player's starting resources over
        // several script steps; that must not look like a mining deposit.
        for _ in 0..32 {
            world.step(&[]).unwrap();
        }
        let worker = world
            .state()
            .entities
            .iter()
            .find(|e| e.owner == PlayerId(0) && e.unit_type == id(2 + race))
            .unwrap()
            .clone();
        let mine = world
            .state()
            .resources
            .iter()
            .filter(|r| r.kind == "gold")
            .min_by_key(|r| {
                i64::from(r.position.x - worker.position.x).pow(2)
                    + i64::from(r.position.y - worker.position.y).pow(2)
            })
            .unwrap()
            .clone();
        let balance = world.resource_balance(PlayerId(0), "gold");
        send(
            &mut world,
            Order::Gather {
                entity: worker.id,
                resource: mine.id,
            },
        );
        let mut mine_ticks = 0;
        let mut depot_ticks = 0;
        for _ in 0..2000 {
            let entity = world
                .state()
                .entities
                .iter()
                .find(|e| e.id == worker.id)
                .unwrap();
            if entity.gathering_inside {
                if entity.dropoff_target.is_some() {
                    depot_ticks += 1;
                    assert!(!world.resource_working(mine.id));
                } else {
                    mine_ticks += 1;
                    let packet = world.player_view(PlayerId(0)).unwrap();
                    assert!(packet.active_resources.contains(&mine.id));
                    assert_eq!(
                        world.visibility(PlayerId(0), mine.position),
                        Visibility::Visible
                    );
                }
            }
            if world.resource_balance(PlayerId(0), "gold") > balance {
                break;
            }
            world.step(&[]).unwrap();
        }
        assert_eq!(world.resource_balance(PlayerId(0), "gold"), balance + 100);
        assert_eq!(mine_ticks, 149);
        assert_eq!(depot_ticks, 149);
    }
}

#[test]
#[ignore = "requires STRATERUST_WARCRAFT2 pointing to a refreshed retail import"]
fn campaign_wall_accepts_attacks_and_its_destroyed_cell_can_be_crossed() {
    let original = load(&root(), "human", 2);
    let mut map = original.map().clone();
    let wall = map
        .spawns
        .iter()
        .filter(|e| e.unit_type.0 >= 1000)
        .find_map(|wall| {
            for (dx, dy) in [(48, 0), (-48, 0), (0, 48), (0, -48)] {
                let position = Position {
                    x: wall.position.x + dx,
                    y: wall.position.y + dy,
                };
                if map.can_move(
                    position,
                    original.unit_type(id(0)).unwrap().footprint,
                    original.unit_type(id(0)).unwrap().movement_class,
                ) {
                    return Some((wall.clone(), position));
                }
            }
            None
        })
        .expect("attackable wall adjacent to open ground");
    map.ai.clear();
    map.mission = None;
    map.fog_of_war = false;
    map.resources.clear();
    map.spawns = vec![
        Spawn {
            owner: PlayerId(0),
            unit_type: id(0),
            position: wall.1,
            ..Default::default()
        },
        wall.0.clone(),
    ];
    let mut world = World::new(original.rules().clone(), map, 7).unwrap();
    send(
        &mut world,
        Order::Attack {
            entity: EntityId(1),
            target: EntityId(2),
        },
    );
    for _ in 0..1000 {
        if world.state().entities.len() == 1 {
            break;
        }
        world.step(&[]).unwrap();
    }
    assert_eq!(world.state().entities.len(), 1, "wall never destroyed");
    send(
        &mut world,
        Order::Move {
            entity: EntityId(1),
            target: wall.0.position,
        },
    );
    for _ in 0..100 {
        if world.state().entities[0].position == wall.0.position {
            break;
        }
        world.step(&[]).unwrap();
    }
    assert_eq!(world.state().entities[0].position, wall.0.position);
}
