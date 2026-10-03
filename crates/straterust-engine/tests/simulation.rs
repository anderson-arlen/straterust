use std::path::Path;

use straterust_engine::{
    content::{Package, read_ron},
    scenario::{CommandQueue, Scenario},
    sim::{
        Command, EntityId, Map, Order, PlayerId, Position, Rejection, Rules, Tick, UnitTypeId,
        World,
    },
};

fn fixture() -> World {
    Package::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/fixtures"))
        .unwrap()
        .world(42)
        .unwrap()
}

fn command(tick: u64, sequence: u64, order: Order) -> Command {
    Command {
        tick: Tick(tick),
        player: PlayerId(0),
        sequence,
        order,
    }
}

#[test]
fn move_arrives_and_stop_cancels_future_motion() {
    let mut world = fixture();
    let order = Order::Move {
        entity: EntityId(1),
        target: Position { x: 429, y: 420 },
    };
    assert_eq!(
        world.step(&[command(0, 1, order)]).unwrap()[0].rejection,
        None
    );
    assert_eq!(
        world.state().entities[0].position,
        Position { x: 424, y: 420 }
    );
    world.step(&[]).unwrap();
    world.step(&[]).unwrap();
    assert_eq!(world.state().entities[0].position.x, 429);
    assert_eq!(world.state().entities[0].target, None);
    world
        .step(&[command(
            3,
            2,
            Order::Move {
                entity: EntityId(1),
                target: Position { x: 900, y: 700 },
            },
        )])
        .unwrap();
    let stopped = world.state().entities[0].position;
    world
        .step(&[command(
            4,
            3,
            Order::Stop {
                entity: EntityId(1),
            },
        )])
        .unwrap();
    for _ in 0..20 {
        world.step(&[]).unwrap();
    }
    assert_eq!(world.state().entities[0].position, stopped);
}

#[test]
fn input_arrival_order_does_not_change_execution() {
    let first = command(
        0,
        1,
        Order::Wander {
            entity: EntityId(1),
        },
    );
    let second = command(
        0,
        2,
        Order::Move {
            entity: EntityId(1),
            target: Position { x: 900, y: 500 },
        },
    );
    let mut a = fixture();
    let mut b = a.clone();
    let outcomes_a = a.step(&[first.clone(), second.clone()]).unwrap();
    let outcomes_b = b.step(&[second, first]).unwrap();
    assert_eq!(outcomes_a, outcomes_b);
    assert_eq!(a.canonical_state(), b.canonical_state());
}

#[test]
fn conflicting_duplicate_sequences_are_all_rejected() {
    let first = command(
        0,
        1,
        Order::Wander {
            entity: EntityId(1),
        },
    );
    let second = command(
        0,
        1,
        Order::Stop {
            entity: EntityId(1),
        },
    );
    let mut a = fixture();
    let mut b = a.clone();
    let outcomes = a.step(&[first.clone(), second.clone()]).unwrap();
    assert!(
        outcomes
            .iter()
            .all(|outcome| outcome.rejection == Some(Rejection::DuplicateSequence))
    );
    assert_eq!(outcomes, b.step(&[second, first]).unwrap());
    assert_eq!(a.state_hash(), b.state_hash());
    assert_eq!(a.state().rng_state, 42);
}

#[test]
fn invalid_commands_cannot_mutate_entities_or_consume_randomness() {
    let cases = [
        (
            command(
                0,
                1,
                Order::Wander {
                    entity: EntityId(4),
                },
            ),
            Rejection::NotOwner,
        ),
        (
            command(
                0,
                1,
                Order::Move {
                    entity: EntityId(1),
                    target: Position { x: i32::MAX, y: -1 },
                },
            ),
            Rejection::OutOfBounds,
        ),
        (
            command(
                1,
                1,
                Order::Stop {
                    entity: EntityId(1),
                },
            ),
            Rejection::WrongTick,
        ),
        (
            command(
                0,
                1,
                Order::Stop {
                    entity: EntityId(999),
                },
            ),
            Rejection::UnknownEntity,
        ),
        (
            Command {
                player: PlayerId(99),
                ..command(
                    0,
                    1,
                    Order::Stop {
                        entity: EntityId(1),
                    },
                )
            },
            Rejection::UnknownPlayer,
        ),
        (
            command(
                0,
                0,
                Order::Stop {
                    entity: EntityId(1),
                },
            ),
            Rejection::StaleSequence,
        ),
    ];
    for (input, reason) in cases {
        let mut world = fixture();
        let before = world.state().entities.clone();
        assert_eq!(world.step(&[input]).unwrap()[0].rejection, Some(reason));
        assert_eq!(world.state().entities, before);
        assert_eq!(world.state().rng_state, 42);
    }
}

#[test]
fn sequences_cannot_be_reused_after_an_illegal_order() {
    let mut world = fixture();
    world
        .step(&[command(
            0,
            5,
            Order::Wander {
                entity: EntityId(4),
            },
        )])
        .unwrap();
    let outcome = world
        .step(&[command(
            1,
            5,
            Order::Wander {
                entity: EntityId(1),
            },
        )])
        .unwrap();
    assert_eq!(outcome[0].rejection, Some(Rejection::StaleSequence));
}

#[test]
fn fixture_playback_reproduces_every_tick_and_detects_changed_input() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/fixtures/scenario.ron");
    let scenario: Scenario = read_ron(&path).unwrap();
    let mut queue = CommandQueue::from_scenario(&scenario).unwrap();
    let mut a = fixture();
    let mut b = fixture();
    let mut changed = fixture();
    for _ in 0..scenario.ticks {
        let commands = queue.take(a.tick());
        a.step(&commands).unwrap();
        b.step(&commands.iter().cloned().rev().collect::<Vec<_>>())
            .unwrap();
        let mut altered = commands;
        if let Some(command) = altered.first_mut()
            && let Order::Move { target, .. } = &mut command.order
        {
            target.x += 1;
        }
        changed.step(&altered).unwrap();
        assert_eq!(
            a.state_hash(),
            b.state_hash(),
            "tick {} diverged",
            a.tick().0
        );
    }
    assert_ne!(a.state_hash(), changed.state_hash());
}

#[test]
fn canonical_hash_covers_rng_orders_sequences_and_gameplay() {
    let world = fixture();
    let different_seed = World::new(world.rules().clone(), world.map().clone(), 43).unwrap();
    assert_ne!(world.state_hash(), different_seed.state_hash());
    let mut rules = world.rules().clone();
    rules.units[0].speed += 1;
    let different_rules = World::new(rules, world.map().clone(), 42).unwrap();
    assert_ne!(world.state_hash(), different_rules.state_hash());
    let mut map = world.map().clone();
    map.width += 1;
    assert_ne!(
        world.state_hash(),
        World::new(world.rules().clone(), map, 42)
            .unwrap()
            .state_hash()
    );
    let mut a = world.clone();
    let mut b = world;
    a.step(&[command(
        0,
        1,
        Order::Move {
            entity: EntityId(1),
            target: Position { x: 800, y: 420 },
        },
    )])
    .unwrap();
    b.step(&[command(
        0,
        1,
        Order::Move {
            entity: EntityId(1),
            target: Position { x: 900, y: 420 },
        },
    )])
    .unwrap();
    assert_eq!(
        a.state().entities[0].position,
        b.state().entities[0].position
    );
    assert_ne!(a.state_hash(), b.state_hash());
    let mut a = fixture();
    let mut b = fixture();
    a.step(&[command(
        0,
        1,
        Order::Stop {
            entity: EntityId(1),
        },
    )])
    .unwrap();
    b.step(&[]).unwrap();
    assert_eq!(a.state().entities, b.state().entities);
    assert_ne!(a.state_hash(), b.state_hash());
}

#[test]
fn definition_order_is_normalized_and_bad_gameplay_is_rejected() {
    let world = fixture();
    let mut rules = world.rules().clone();
    rules.units.reverse();
    assert_eq!(
        world.state_hash(),
        World::new(rules, world.map().clone(), 42)
            .unwrap()
            .state_hash()
    );
    let mut rules = world.rules().clone();
    rules.units.push(rules.units[0].clone());
    assert!(World::new(rules, world.map().clone(), 42).is_err());
    let mut map = world.map().clone();
    map.spawns[0].unit_type = UnitTypeId(99);
    assert!(World::new(world.rules().clone(), map, 42).is_err());
    let mut map = world.map().clone();
    map.width = 0;
    assert!(World::new(world.rules().clone(), map, 42).is_err());
    assert!(ron::from_str::<Rules>("(id: \"bad\", tick_ms: 50, units: [], typo: 2)").is_err());
    assert!(ron::from_str::<Map>("(width: 10,").is_err());
}

#[test]
fn oversized_batches_fail_without_advancing_state() {
    let mut world = fixture();
    let before = world.canonical_state();
    let commands = vec![
        command(
            0,
            1,
            Order::Stop {
                entity: EntityId(1)
            }
        );
        4097
    ];
    assert!(world.step(&commands).is_err());
    assert_eq!(world.canonical_state(), before);
}

#[test]
fn scenarios_reject_unsupported_versions_and_out_of_range_ticks() {
    let mut scenario = Scenario {
        schema_version: 2,
        seed: 0,
        ticks: 10,
        commands: vec![],
    };
    assert!(scenario.validate().is_err());
    scenario.schema_version = 1;
    scenario.commands.push(command(
        10,
        1,
        Order::Stop {
            entity: EntityId(1),
        },
    ));
    assert!(scenario.validate().is_err());
}
