use super::*;
use crate::map::{MovementClass, Terrain, WALKABLE};

#[test]
fn ai_island_party_builds_paid_transports_keeps_defenders_and_attacks_after_unloading() {
    let base = economy();
    let mut rules = base.rules().clone();
    rules.starting_resources = vec![ResourceAmount {
        kind: "minerals".into(),
        amount: 2000,
    }];
    rules
        .units
        .iter_mut()
        .find(|u| u.id == UnitTypeId(3))
        .unwrap()
        .trains
        .push(UnitTypeId(9));
    rules.units.push(UnitType {
        id: UnitTypeId(9),
        speed: 16,
        movement_class: MovementClass::Air,
        build_ticks: 16,
        cost: vec![ResourceAmount {
            kind: "minerals".into(),
            amount: 150,
        }],
        garrison: Some(GarrisonStats {
            capacity: 2,
            passengers: vec![UnitTypeId(1)],
            attackers: vec![],
            range_bonus: 0,
            unload_ticks: 15,
        }),
        ..Default::default()
    });
    let mut map = base.map().clone();
    map.fog_of_war = false;
    map.resources.clear();
    map.ai[0].program = vec![
        AiInstruction::Defense {
            unit_type: UnitTypeId(1),
            count: 1,
        },
        AiInstruction::AttackAdd {
            unit_type: UnitTypeId(1),
            count: 3,
        },
        AiInstruction::AttackPrepare,
        AiInstruction::Attack,
        AiInstruction::Stop,
    ];
    map.spawns = vec![
        Spawn {
            owner: PlayerId(0),
            unit_type: UnitTypeId(3),
            position: Position { x: 384, y: 640 },
            ..Default::default()
        },
        Spawn {
            owner: PlayerId(1),
            unit_type: UnitTypeId(3),
            position: Position { x: 1280, y: 640 },
            ..Default::default()
        },
    ];
    map.spawns.extend((0..4).map(|n| Spawn {
        owner: PlayerId(1),
        unit_type: UnitTypeId(1),
        position: Position {
            x: 1120 + n * 32,
            y: 560,
        },
        ..Default::default()
    }));
    let columns = map.width as u32 / 32;
    let rows = map.height as u32 / 32;
    let mut flags = vec![WALKABLE; (columns * rows) as usize];
    for y in 0..rows {
        flags[(y * columns + 25) as usize] = 0;
    }
    map.terrain = Some(Terrain {
        cell_size: 32,
        columns,
        rows,
        flags,
    });
    let mut world = World::new(rules, map, 42).unwrap();
    let mut replay = world.clone();
    let start_hp = world.state.entities[0].hp;
    let mut boarded = false;
    for step in 0..300 {
        world.step(&[]).unwrap();
        replay.step(&[]).unwrap();
        assert_eq!(world.state_hash(), replay.state_hash());
        boarded |= world
            .state
            .entities
            .iter()
            .any(|e| e.garrisoned_in.is_some());
        if step == 80 {
            replay.state = ron::from_str(&ron::to_string(&world.state).unwrap()).unwrap();
        }
    }
    assert!(boarded, "party never boarded: {:?}", world.state.ai);
    assert_eq!(
        world
            .state
            .entities
            .iter()
            .filter(|e| e.unit_type == UnitTypeId(9))
            .count(),
        2
    );
    assert!(world.resource_balance(PlayerId(1), "minerals") <= 1700);
    let guard = world
        .state
        .entities
        .iter()
        .find(|e| e.id == EntityId(3))
        .unwrap();
    assert!(guard.position.x > 800, "reserved defender left the island");
    assert!(
        world
            .state
            .entities
            .iter()
            .filter(|e| e.owner == PlayerId(1)
                && e.unit_type == UnitTypeId(1)
                && e.position.x < 800)
            .count()
            >= 2
    );
    assert!(
        world
            .state
            .entities
            .iter()
            .find(|e| e.id == EntityId(1))
            .is_none_or(|e| e.hp < start_hp),
        "unloaded party did not attack"
    );
    assert!(world.state.ai[0].transports.is_empty());
}
