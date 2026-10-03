//! Private campaign completion checks using ordinary human commands only.
//! Waypoints are an authored test strategy; they are not the opponent AI.
use anyhow::{Context, Result, ensure};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};
use straterust_engine::{content::Package, scenario::Scenario, sim::*};

fn distance(a: Position, b: Position) -> i64 {
    (i64::from(a.x) - i64::from(b.x)).pow(2) + (i64::from(a.y) - i64::from(b.y)).pow(2)
}

fn at_teleport_destination(p: Position) -> bool {
    (3008..=3168).contains(&p.x) && (224..=384).contains(&p.y)
}

fn main() -> Result<()> {
    let root = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .context("usage: record_campaign CAMPAIGN_DIRECTORY MISSION_NUMBER")?;
    let number: u8 = std::env::args()
        .nth(2)
        .context("missing mission number")?
        .parse()?;
    ensure!(
        matches!(number, 1 | 3 | 4 | 5),
        "Mission 2 uses record_backwater"
    );
    let package = Package::load(&root.join(format!("terran{number:02}")))?;
    let mut world = package.world(42)?;
    let goals: Vec<Position> = match number {
        1 => vec![(128, 704), (704, 800), (1008, 1024), (1056, 1584)],
        3 => Vec::new(),
        4 => vec![
            (3488, 3760),
            (2848, 3600),
            (2400, 3280),
            (1248, 3856),
            (352, 2608),
            (1056, 2416),
            (1248, 1936),
            (1728, 1488),
            (1728, 1232),
            (1376, 688),
            (1184, 592),
            (976, 192),
            (3104, 384),
            (3104, 656),
            (2848, 1680),
            (2448, 2016),
        ],
        5 => vec![
            (144, 1232),
            (656, 1776),
            (1376, 1456),
            (1600, 624),
            (2176, 320),
            (2720, 464),
        ],
        _ => unreachable!(),
    }
    .into_iter()
    .map(|(x, y)| Position { x, y })
    .collect();
    let mut stage = 0;
    let mut started = 0;
    let mut issued = BTreeMap::<EntityId, (usize, u64)>::new();
    let mut sequence = 0;
    let mut commands = Vec::new();
    let mut launched = false;
    while world.tick().0 < 90000
        && world.state().winner.is_none()
        && !world.state().defeated.contains(&PlayerId(0))
    {
        let mut orders = Vec::new();
        if !world.state().mission.as_ref().is_some_and(|m| m.paused)
            && world.tick().0.is_multiple_of(24)
        {
            let own: Vec<_> = world
                .state()
                .entities
                .iter()
                .filter(|e| e.owner == PlayerId(0))
                .cloned()
                .collect();
            let army: Vec<_> = own
                .iter()
                .filter(|e| {
                    e.garrisoned_in.is_none()
                        && world.unit_type(e.unit_type).is_some_and(|u| {
                            u.weapon.is_some()
                                && u.worker.is_none()
                                && !u.structure
                                && u.mine.is_none()
                        })
                })
                .collect();
            if world.tick().0.is_multiple_of(2000) && number == 4 {
                eprintln!(
                    "positions={:?}",
                    army.iter()
                        .map(|e| (e.id, e.position, &e.order))
                        .collect::<Vec<_>>()
                );
            }
            if number != 3 && stage < goals.len() && world.tick().0 > started + 12000 {
                break;
            }
            let rescued = own
                .iter()
                .any(|e| e.unit_type == UnitTypeId(if number == 1 { 10 } else { 25 }));
            let base = own.iter().find(|e| e.unit_type == UnitTypeId(3));
            if stage < goals.len() {
                let goal = goals[stage];
                let teleport_goal = number == 4 && goal == (Position { x: 976, y: 192 });
                let reached = army
                    .iter()
                    .filter(|e| {
                        !teleport_goal
                            && distance(e.position, goal)
                                < if number == 4 {
                                    64_i64.pow(2)
                                } else {
                                    160_i64.pow(2)
                                }
                    })
                    .count();
                let door_open = number != 4
                    || !world.state().entities.iter().any(|e| {
                        e.doodad_enabled == Some(true)
                            && matches!(e.unit_type.0, 45..=48)
                            && distance(e.position, goal) < 48_i64.pow(2)
                    });
                let teleported = teleport_goal
                    && !army.is_empty()
                    && army.iter().all(|e| at_teleport_destination(e.position));
                let hero_ready = match (number, stage) {
                    (1, 2) => rescued,
                    (5, 0) => rescued,
                    (5, 1) => own.iter().any(|e| {
                        e.unit_type == UnitTypeId(25) && distance(e.position, goal) < 128_i64.pow(2)
                    }),
                    (5, 5) => base.is_some(),
                    _ => true,
                };
                if (reached >= army.len().div_ceil(2).clamp(1, 6) || teleported)
                    && hero_ready
                    && door_open
                    && world.tick().0 > started + 120
                {
                    stage += 1;
                    started = world.tick().0;
                }
            }
            // The Confederate bases are on disconnected southern terrain.
            // Use a paid Wraith assault rather than issuing impossible ground routes.
            launched |= number == 5
                && army
                    .iter()
                    .filter(|e| e.unit_type == UnitTypeId(23))
                    .count()
                    >= 20;
            let attack = if stage < goals.len() {
                Some(goals[stage])
            } else if number == 5 && launched {
                world
                    .state()
                    .entities
                    .iter()
                    .filter(|e| {
                        world.is_enemy(PlayerId(0), e.owner)
                            && world.entity_visible(PlayerId(0), e.id)
                            && !e.invincible
                    })
                    .min_by_key(|e| {
                        distance(
                            e.position,
                            base.map_or(Position { x: 2720, y: 464 }, |b| b.position),
                        )
                    })
                    .map(|e| e.position)
                    .or(Some(Position { x: 256, y: 2768 }))
            } else if number == 3 {
                Some(Position { x: 448, y: 2640 })
            } else if number == 5 && base.is_some() {
                Some(Position { x: 2912, y: 1184 })
            } else {
                None
            };
            let focus = if number == 4 {
                world
                    .state()
                    .entities
                    .iter()
                    .filter(|e| {
                        world.is_enemy(PlayerId(0), e.owner)
                            && world.entity_visible(PlayerId(0), e.id)
                            && !e.invincible
                            && e.doodad_enabled != Some(false)
                            && world
                                .unit_type(e.unit_type)
                                .is_some_and(|u| u.weapon.is_some())
                            && army
                                .iter()
                                .any(|a| distance(a.position, e.position) < 192_i64.pow(2))
                    })
                    .min_by_key(|e| (e.hp, e.id))
                    .map(|e| e.id)
            } else {
                None
            };
            if let Some(target) = attack {
                for entity in &army {
                    let target =
                        if number == 5 && stage >= goals.len() && entity.unit_type == UnitTypeId(1)
                        {
                            Position { x: 2912, y: 1184 }
                        } else {
                            target
                        };
                    if number == 4
                        && target == (Position { x: 976, y: 192 })
                        && at_teleport_destination(entity.position)
                    {
                        continue;
                    }
                    if let Some(enemy) = focus {
                        orders.push(Order::Attack {
                            entity: entity.id,
                            target: enemy,
                        });
                        issued.insert(entity.id, (stage, world.tick().0));
                        continue;
                    }
                    if number == 3
                        && base
                            .is_some_and(|b| distance(entity.position, b.position) > 700_i64.pow(2))
                    {
                        continue;
                    }
                    if issued.get(&entity.id).is_none_or(|(s, t)| {
                        *s != stage
                            + if number == 5 && base.is_some() && stage == goals.len() {
                                if launched { 2 } else { 1 }
                            } else {
                                0
                            }
                            || (number == 4 && matches!(entity.order, UnitOrder::Attack { .. }))
                            || (number == 4
                                && entity.unit_type == UnitTypeId(26)
                                && world.tick().0 > *t + 24)
                            || (entity.order == UnitOrder::Idle
                                && distance(entity.position, target)
                                    > if number == 4 {
                                        40_i64.pow(2)
                                    } else {
                                        96_i64.pow(2)
                                    }
                                && world.tick().0 > *t + 96)
                    }) {
                        orders.push(
                            if (number == 5 && matches!(entity.unit_type.0, 10 | 25))
                                || (number == 4 && entity.unit_type == UnitTypeId(26))
                            {
                                Order::Move {
                                    entity: entity.id,
                                    target: if number == 5 && base.is_some() {
                                        Position { x: 2960, y: 432 }
                                    } else {
                                        target
                                    },
                                }
                            } else {
                                Order::AttackMove {
                                    entity: entity.id,
                                    target,
                                }
                            },
                        );
                        issued.insert(
                            entity.id,
                            (
                                stage
                                    + if number == 5 && base.is_some() && stage == goals.len() {
                                        if launched { 2 } else { 1 }
                                    } else {
                                        0
                                    },
                                world.tick().0,
                            ),
                        );
                    }
                }
                if number == 1 && stage < goals.len() {
                    for worker in own.iter().filter(|e| e.unit_type == UnitTypeId(2)) {
                        if issued.get(&worker.id).is_none_or(|(s, _)| *s != stage) {
                            orders.push(Order::Move {
                                entity: worker.id,
                                target,
                            });
                            issued.insert(worker.id, (stage, world.tick().0));
                        }
                    }
                }
            }
            if number == 3
                || (number == 1 && stage >= goals.len())
                || (number == 5 && base.is_some())
            {
                economy(&world, &own, number, stage >= goals.len(), &mut orders);
            }
        }
        let batch: Vec<_> = orders
            .into_iter()
            .map(|order| {
                sequence += 1;
                Command {
                    tick: world.tick(),
                    player: PlayerId(0),
                    sequence,
                    order,
                }
            })
            .collect();
        world.step(&batch)?;
        // A second order can lose a race for supply/placement within a batch;
        // Keep those envelopes too: rejected orders still consume sequence state.
        commands.extend(batch);
        if world.tick().0.is_multiple_of(2000) {
            std::fs::write(
                root.join(format!("terran{number:02}-incomplete-state.ron")),
                ron::ser::to_string_pretty(world.state(), ron::ser::PrettyConfig::default())?,
            )?;
            eprintln!("supply={:?}", world.supply(PlayerId(0)));
            eprintln!(
                "mission={number} tick={} stage={stage} units={} minerals={} ai={:?}",
                world.tick().0,
                world
                    .state()
                    .entities
                    .iter()
                    .filter(|e| e.owner == PlayerId(0))
                    .count(),
                world.resource_balance(PlayerId(0), "minerals"),
                world
                    .state()
                    .ai
                    .iter()
                    .map(|a| (a.instruction, a.accepted_orders, a.deployed.len()))
                    .collect::<Vec<_>>()
            );
        }
    }
    if world.state().winner != Some(PlayerId(0)) {
        std::fs::write(
            root.join(format!("terran{number:02}-incomplete-state.ron")),
            ron::ser::to_string_pretty(world.state(), ron::ser::PrettyConfig::default())?,
        )?;
    }
    ensure!(
        world.state().winner == Some(PlayerId(0)),
        "mission {number} incomplete: tick={} stage={stage} defeated={:?}",
        world.tick().0,
        world.state().defeated
    );
    let scenario = Scenario {
        schema_version: 1,
        seed: 42,
        ticks: world.tick().0,
        commands,
    };
    let mut replay = package.world(42)?;
    let mut queue = straterust_engine::scenario::CommandQueue::from_scenario(&scenario)?;
    for _ in 0..scenario.ticks {
        let batch = queue.take(replay.tick());
        replay.step(&batch)?;
    }
    ensure!(
        world.state_hash() == replay.state_hash(),
        "recording did not replay identically"
    );
    let output = root.join(format!("terran{number:02}-playthrough.ron"));
    std::fs::write(
        &output,
        ron::ser::to_string_pretty(&scenario, ron::ser::PrettyConfig::default())?,
    )?;
    println!(
        "mission={number} commands={} victory_tick={} hash={} recording={}",
        scenario.commands.len(),
        scenario.ticks,
        world.state_hash(),
        output.display()
    );
    Ok(())
}

fn economy(
    world: &World,
    own: &[Entity],
    number: u8,
    objectives_reached: bool,
    orders: &mut Vec<Order>,
) {
    let air_assault = number == 5 && objectives_reached;
    let player = PlayerId(0);
    let mut minerals = world
        .resource_balance(player, "minerals")
        .saturating_sub(if number == 3 { 50 } else { 0 });
    let workers: Vec<_> = own
        .iter()
        .filter(|e| e.unit_type == UnitTypeId(2))
        .collect();
    let army = own
        .iter()
        .filter(|e| matches!(e.unit_type.0, 1 | 10 | 11 | 20 | 21 | 23 | 25 | 26))
        .count();
    let Some(base) = own.iter().find(|e| e.unit_type == UnitTypeId(3)) else {
        return;
    };
    if number != 1 && workers.len() < 10 && base.production.is_empty() && minerals >= 50 {
        orders.push(Order::Train {
            entity: base.id,
            unit_type: UnitTypeId(2),
        });
        minerals -= 50;
    }
    let mut assigned = BTreeSet::new();
    let (used, provided) = world.supply(player);
    let pending = own.iter().map(|e| e.production.len() as u32).sum::<u32>();
    let barracks = own.iter().filter(|e| e.unit_type == UnitTypeId(5)).count();
    let goals = if used + pending + 4 >= provided && provided < 180 {
        vec![UnitTypeId(4)]
    } else if air_assault && own.iter().filter(|e| e.unit_type == UnitTypeId(33)).count() < 3 {
        vec![UnitTypeId(33)]
    } else if !air_assault && barracks < if number == 1 { 1 } else { 3 } {
        vec![UnitTypeId(5)]
    } else {
        Vec::new()
    };
    for unit_type in goals {
        let unit = world.unit_type(unit_type).unwrap();
        let cost = unit
            .cost
            .iter()
            .filter(|c| c.kind == "minerals")
            .map(|c| u64::from(c.amount))
            .sum::<u64>();
        if minerals < cost
            || own
                .iter()
                .any(|e| e.unit_type == unit_type && e.construction.is_some())
        {
            continue;
        }
        if let Some(worker) = workers
            .iter()
            .filter(|e| !matches!(e.order, UnitOrder::Build { .. } | UnitOrder::Repair { .. }))
            .min_by_key(|e| distance(e.position, base.position))
        {
            let mut spots: Vec<_> = (-24..=24)
                .flat_map(|dy| {
                    (-24..=24).map(move |dx| Position {
                        x: (base.position.x / 32 + dx) * 32 + i32::from(unit.placement.width) / 2,
                        y: (base.position.y / 32 + dy) * 32 + i32::from(unit.placement.height) / 2,
                    })
                })
                .collect();
            spots.sort_by_key(|p| {
                (
                    u8::from(p.y < base.position.y + 160),
                    distance(*p, base.position),
                )
            });
            if let Some(position) = spots.into_iter().find(|p| {
                world
                    .build_rejection(player, worker.id, unit_type, *p)
                    .is_none()
            }) {
                orders.push(Order::Build {
                    entity: worker.id,
                    unit_type,
                    position,
                });
                assigned.insert(worker.id);
                minerals -= cost;
            }
        }
    }
    if number == 3
        && base.hp + 100 < world.unit_type(base.unit_type).unwrap().max_hp
        && world.resource_balance(player, "minerals") >= 10
    {
        let repairing = workers
            .iter()
            .filter(|e| matches!(e.order, UnitOrder::Repair { .. }))
            .count();
        let repairers: Vec<_> = workers
            .iter()
            .filter(|e| {
                !assigned.contains(&e.id)
                    && !matches!(e.order, UnitOrder::Build { .. } | UnitOrder::Repair { .. })
            })
            .take(2usize.saturating_sub(repairing))
            .copied()
            .collect();
        for worker in repairers {
            orders.push(Order::Repair {
                entity: worker.id,
                target: base.id,
            });
            assigned.insert(worker.id);
        }
    }
    let mut gas_workers = workers
        .iter()
        .filter(|e| {
            if let UnitOrder::Gather { resource } = e.order {
                world
                    .state()
                    .resources
                    .iter()
                    .any(|r| r.id == resource && r.kind == "gas")
            } else {
                false
            }
        })
        .count();
    let gas_target = if air_assault {
        workers.len().saturating_sub(4).min(3)
    } else {
        0
    };
    for worker in &workers {
        if assigned.contains(&worker.id) {
            continue;
        }
        let on_gas = if let UnitOrder::Gather { resource } = worker.order {
            world
                .state()
                .resources
                .iter()
                .any(|r| r.id == resource && r.kind == "gas")
        } else {
            false
        };
        let wants_gas = air_assault && gas_workers < gas_target;
        let excess_gas = air_assault && on_gas && gas_workers > gas_target;
        if worker.order == UnitOrder::Idle
            || (wants_gas && matches!(worker.order, UnitOrder::Gather { .. }))
            || excess_gas
        {
            if wants_gas
                && let UnitOrder::Gather { resource } = worker.order
                && world
                    .state()
                    .resources
                    .iter()
                    .any(|r| r.id == resource && r.kind == "gas")
            {
                continue;
            }
            if let Some(shell) = own.iter().find(|e| {
                e.construction.as_ref().is_some_and(|c| c.worker.is_none())
                    && world
                        .unit_type(worker.unit_type)
                        .unwrap()
                        .builds
                        .contains(&e.unit_type)
            }) {
                orders.push(Order::Resume {
                    entity: worker.id,
                    building: shell.id,
                });
                continue;
            }
            if let Some(resource) = world
                .state()
                .resources
                .iter()
                .filter(|r| {
                    r.kind == if wants_gas { "gas" } else { "minerals" }
                        && r.amount > 0
                        && world.gather_rejection(worker.id, r.id).is_none()
                })
                .min_by_key(|r| {
                    let assigned = own
                        .iter()
                        .filter(|e| e.order == (UnitOrder::Gather { resource: r.id }))
                        .count();
                    distance(worker.position, r.position) + assigned as i64 * 10000
                })
            {
                orders.push(Order::Gather {
                    entity: worker.id,
                    resource: resource.id,
                });
                if resource.kind == "gas" {
                    gas_workers += 1;
                } else if on_gas {
                    gas_workers -= 1;
                }
            }
        }
    }
    for producer in own.iter().filter(|e| {
        e.unit_type == UnitTypeId(5) && e.construction.is_none() && e.production.len() < 2
    }) {
        if minerals >= 50
            && army
                < if number == 1 {
                    10
                } else if air_assault {
                    20
                } else {
                    96
                }
        {
            orders.push(Order::Train {
                entity: producer.id,
                unit_type: UnitTypeId(1),
            });
            minerals -= 50;
        }
    }
    if air_assault {
        let mut gas = world.resource_balance(player, "gas");
        let fighters = own.iter().filter(|e| e.unit_type == UnitTypeId(23)).count();
        for producer in own.iter().filter(|e| {
            e.unit_type == UnitTypeId(33) && e.construction.is_none() && e.production.len() < 2
        }) {
            if minerals >= 150 && gas >= 100 && fighters < 32 {
                orders.push(Order::Train {
                    entity: producer.id,
                    unit_type: UnitTypeId(23),
                });
                minerals -= 150;
                gas -= 100;
            }
        }
    }
}
