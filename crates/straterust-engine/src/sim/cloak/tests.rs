use super::*;
use crate::{content::Package, session::ServerSession};

fn world() -> World {
    let initial = Package::load(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/terran-demo"),
    )
    .unwrap()
    .world(42)
    .unwrap();
    let mut rules = initial.rules().clone();
    for unit in &mut rules.units {
        unit.vision_range = 256;
    }
    rules
        .units
        .iter_mut()
        .find(|u| u.id == UnitTypeId(1))
        .unwrap()
        .cloak = Some(Cloak {
        can_move: false,
        can_attack: false,
        blocks_movement: false,
        reveal_ticks: 3,
        reveal_on_order: true,
        auto_reveal: true,
        ..Cloak::default()
    });
    let mut map = initial.map().clone();
    map.fog_of_war = true;
    map.resources.clear();
    map.spawns = ron::from_str(
        r"[
        (owner:0, unit_type:1, position:(x:100,y:100)),
        (owner:1, unit_type:1, position:(x:164,y:100)),
        (owner:1, unit_type:2, position:(x:240,y:100)),
        (owner:1, unit_type:3, position:(x:320,y:100)),
    ]",
    )
    .unwrap();
    // A former mission guard slot can become a human multiplayer participant.
    map.mission = Some(
        ron::from_str(
            r"(
        schema_version:1, player:0, rescuers:[0], poll_ticks:1, wait_step_ms:50,
        locations:[(left:0,top:0,right:1536,bottom:960)], triggers:[(conditions:[Elapsed(comparison:AtLeast,
        milliseconds:300000)], actions:[Victory])],
    )",
        )
        .unwrap(),
    );
    World::new(rules, map, 42).unwrap()
}

fn submit(server: &mut ServerSession, player: u16, sequence: u64, order: Order) {
    let command = Command {
        tick: server.world().tick(),
        player: PlayerId(player),
        sequence,
        order,
    };
    // Exercise the serialized command envelope, as used by remote clients.
    let command = ron::from_str(&ron::ser::to_string(&command).unwrap()).unwrap();
    server.submit(PlayerId(player), command).unwrap();
}

#[test]
fn configured_stationary_cloak_stays_hidden_and_waits_before_moving() {
    let initial = world();
    let mut server =
        ServerSession::new(initial.clone(), 42, vec![PlayerId(0), PlayerId(1)]).unwrap();
    let id = EntityId(2);
    let origin = server.world().state.entities[1].position;
    submit(
        &mut server,
        1,
        1,
        Order::Move {
            entity: id,
            target: Position { x: 500, y: 100 },
        },
    );
    submit(
        &mut server,
        1,
        2,
        Order::Cloak {
            entity: id,
            enabled: true,
        },
    );
    let outcomes = server.advance(&[]).unwrap();
    assert!(outcomes.iter().all(|o| o.rejection.is_none()));
    for _ in 0..4 {
        server.advance(&[]).unwrap();
    }
    let actor = &server.world().state.entities[1];
    assert!(actor.cloaked);
    assert_eq!(actor.position, origin);
    assert!(actor.path.is_empty() && actor.queued_orders.is_empty());
    assert_eq!(actor.order, UnitOrder::Hold);
    let guest = server
        .update(PlayerId(1), &[])
        .unwrap()
        .view
        .into_world(&initial)
        .unwrap();
    assert!(
        guest
            .state
            .entities
            .iter()
            .find(|e| e.id == id)
            .unwrap()
            .cloaked
    );
    let enemy = server
        .update(PlayerId(0), &[])
        .unwrap()
        .view
        .into_world(&initial)
        .unwrap();
    assert!(!enemy.state.entities.iter().any(|e| e.id == id));

    submit(
        &mut server,
        1,
        3,
        Order::Cloak {
            entity: id,
            enabled: false,
        },
    );
    assert!(server.advance(&[]).unwrap()[0].rejection.is_none());
    assert_eq!(server.world().state.entities[1].cloak_transition, 2);
    assert!(!server.world().entity_visible(PlayerId(0), id));
    submit(
        &mut server,
        1,
        4,
        Order::Cloak {
            entity: id,
            enabled: true,
        },
    );
    assert_eq!(
        server.advance(&[]).unwrap()[0].rejection,
        Some(Rejection::Cooldown)
    );
    submit(
        &mut server,
        1,
        5,
        Order::Move {
            entity: id,
            target: Position { x: 500, y: 100 },
        },
    );
    assert!(server.advance(&[]).unwrap()[0].rejection.is_none());
    assert_eq!(server.world().state.entities[1].position, origin);
    assert!(server.world().entity_visible(PlayerId(0), id));
    server.advance(&[]).unwrap();
    assert_ne!(server.world().state.entities[1].position, origin);
    let replay = server.replay().play(&initial).unwrap();
    assert_eq!(replay.state_hash(), server.world().state_hash());
    let mut restored = ServerSession::restore(&initial, server.save_snapshot().unwrap()).unwrap();
    server.advance(&[]).unwrap();
    restored.advance(&[]).unwrap();
    assert_eq!(server.world().state_hash(), restored.world().state_hash());
}

#[test]
fn concealment_rejects_wrong_owner_and_unconfigured_units_and_preserves_ambush() {
    let mut world = world();
    let command = |sequence, entity| Command {
        tick: Tick(0),
        player: PlayerId(0),
        sequence,
        order: Order::Cloak {
            entity: EntityId(entity),
            enabled: true,
        },
    };
    assert_eq!(world.apply(&command(1, 2)), Some(Rejection::NotOwner));
    for id in [3, 4] {
        assert_eq!(
            world.cloak_rejection(EntityId(id), true),
            Some(Rejection::UnsupportedOrder)
        );
    }
    world.state.entities[1].cloaked = true;
    world.state.entities[1].order = UnitOrder::Idle;
    world.step(&[]).unwrap();
    assert!(
        !world.state.entities[1].cloaked,
        "preplaced idle guards still emerge"
    );
    assert_eq!(world.state.entities[1].cloak_transition, 3);
}
