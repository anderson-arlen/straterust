//! Opt-in mission playthrough using ordinary player commands, never runtime AI.
//! The generated recording stays beside the user's private imported package.
use std::{collections::BTreeMap, path::PathBuf};

use anyhow::{Context, Result, ensure};
use straterust_engine::{
    content::Package,
    scenario::Scenario,
    sim::{Command, EntityId, Order, PlayerId, Position, UnitOrder, UnitTypeId, Visibility},
};

fn distance(a: Position, b: Position) -> i64 {
    (i64::from(a.x) - i64::from(b.x)).pow(2) + (i64::from(a.y) - i64::from(b.y)).pow(2)
}
fn main() -> Result<()> {
    let path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .context("usage: record_backwater PACKAGE_DIRECTORY")?;
    let output = path
        .parent()
        .context("package parent")?
        .join("backwater-playthrough.ron");
    let package = Package::load(&path)?;
    let mut world = package.world(42)?;
    ensure!(
        world.map().mission.is_some(),
        "recording requires imported mission"
    );
    let goals = [
        (128, 1152),
        (128, 704),
        (224, 160),
        (704, 384),
        (1088, 608),
        (1424, 352),
        (1824, 128),
    ];
    let mut stage = 0_usize;
    let mut stage_start = 0;
    let mut issued = BTreeMap::new();
    let mut sequence = 0;
    let mut commands = Vec::new();
    let mut rescued = false;
    while world.tick().0 < 60000
        && world.state().winner.is_none()
        && !world.state().defeated.contains(&PlayerId(0))
    {
        let mut orders = Vec::new();
        if !world
            .state()
            .mission
            .as_ref()
            .is_some_and(|mission| mission.paused)
        {
            let own: Vec<_> = world
                .state()
                .entities
                .iter()
                .filter(|entity| entity.owner == PlayerId(0))
                .cloned()
                .collect();
            let army: Vec<_> = own
                .iter()
                .filter(|entity| matches!(entity.unit_type.0, 1 | 10 | 11))
                .collect();
            rescued |= own.iter().any(|entity| entity.unit_type == UnitTypeId(12));
            if stage == 0 && army.len() >= 16 {
                stage = 1;
                stage_start = world.tick().0;
                issued.clear();
            }
            if stage > 0 && stage <= goals.len() {
                let (x, y) = goals[stage - 1];
                let goal = Position { x, y };
                let reached = army
                    .iter()
                    .filter(|entity| distance(entity.position, goal) < 192_i64.pow(2))
                    .count();
                // A growing rear reinforcement column must not prevent the
                // vanguard from advancing through the next cleared choke.
                if stage < goals.len()
                    && reached >= army.len().div_ceil(2).clamp(1, 8)
                    && world.tick().0 > stage_start + 240
                {
                    stage += 1;
                    stage_start = world.tick().0;
                    issued.clear();
                }
                let (x, y) = goals[stage - 1];
                for entity in &army {
                    // A near route may finish at a temporarily occupied
                    // choke. Resume idle stragglers, without repeatedly
                    // interrupting units that are moving or fighting.
                    let retry = entity.order == UnitOrder::Idle
                        && distance(entity.position, Position { x, y }) >= 96_i64.pow(2)
                        && issued
                            .get(&entity.id)
                            .is_some_and(|tick| world.tick().0 >= tick + 96);
                    if !issued.contains_key(&entity.id) || retry {
                        let unit = world.unit_type(entity.unit_type).unwrap();
                        let target = (-4_i32..=4)
                            .flat_map(|dy| {
                                (-4_i32..=4).map(move |dx| Position {
                                    x: x + dx * 8,
                                    y: y + dy * 8,
                                })
                            })
                            .filter(|p| {
                                world
                                    .map()
                                    .can_move(*p, unit.footprint, unit.movement_class)
                            })
                            .min_by_key(|p| distance(*p, Position { x, y }));
                        if let Some(target) = target {
                            orders.push(Order::AttackMove {
                                entity: entity.id,
                                target,
                            });
                            issued.insert(entity.id, world.tick().0);
                        }
                    }
                }
            }
            let mut minerals = world.resource_balance(PlayerId(0), "minerals");
            let (used, provided) = world.supply(PlayerId(0));
            let pending = own
                .iter()
                .map(|entity| entity.production.len() as u32)
                .sum::<u32>();
            let mut building_worker: Option<EntityId> = None;
            if used + pending + 4 >= provided
                && provided < 100
                && minerals >= 100
                && !own.iter().any(|entity| {
                    entity.unit_type == UnitTypeId(4) && entity.construction.is_some()
                })
                && let (Some(worker), Some(base)) = (
                    own.iter().find(|entity| {
                        entity.unit_type == UnitTypeId(2)
                            && !matches!(entity.order, UnitOrder::Build { .. })
                    }),
                    own.iter().find(|entity| entity.unit_type == UnitTypeId(3)),
                )
            {
                let depot = world.unit_type(UnitTypeId(4)).context("depot type")?;
                let mut positions: Vec<_> = (-8..=8)
                    .flat_map(|y| {
                        (-8..=8).map(move |x| Position {
                            x: (base.position.x / 32 + x) * 32
                                + i32::from(depot.placement.width) / 2,
                            y: (base.position.y / 32 + y) * 32
                                + i32::from(depot.placement.height) / 2,
                        })
                    })
                    .collect();
                positions.sort_by_key(|p| distance(*p, base.position));
                if let Some(position) = positions.into_iter().find(|p| {
                    world
                        .build_rejection(PlayerId(0), worker.id, UnitTypeId(4), *p)
                        .is_none()
                }) {
                    orders.push(Order::Build {
                        entity: worker.id,
                        unit_type: UnitTypeId(4),
                        position,
                    });
                    building_worker = Some(worker.id);
                    minerals -= 100;
                }
            }
            for worker in own.iter().filter(|entity| {
                entity.unit_type == UnitTypeId(2)
                    && entity.order == UnitOrder::Idle
                    && Some(entity.id) != building_worker
            }) {
                if let Some(resource) = world
                    .state()
                    .resources
                    .iter()
                    .filter(|node| {
                        node.kind == "minerals"
                            && node.amount > 0
                            && world.visibility(PlayerId(0), node.position)
                                != Visibility::Unexplored
                    })
                    .min_by_key(|node| distance(node.position, worker.position))
                {
                    orders.push(Order::Gather {
                        entity: worker.id,
                        resource: resource.id,
                    });
                }
            }
            for barracks in own.iter().filter(|entity| {
                entity.unit_type == UnitTypeId(5)
                    && entity.construction.is_none()
                    && entity.production.len() < 2
            }) {
                if minerals >= 50 && army.len() < 48 {
                    orders.push(Order::Train {
                        entity: barracks.id,
                        unit_type: UnitTypeId(1),
                    });
                    minerals -= 50;
                }
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
        for outcome in world.step(&batch)? {
            ensure!(
                outcome.rejection.is_none(),
                "playthrough order rejected: {outcome:?}"
            );
        }
        commands.extend(batch);
        if world.tick().0.is_multiple_of(1000) {
            let army: Vec<_> = world
                .state()
                .entities
                .iter()
                .filter(|entity| {
                    entity.owner == PlayerId(0) && matches!(entity.unit_type.0, 1 | 10 | 11)
                })
                .collect();
            let goal = goals
                .get(stage.saturating_sub(1))
                .map(|&(x, y)| Position { x, y });
            let reached = army
                .iter()
                .filter(|entity| {
                    goal.is_some_and(|goal| distance(entity.position, goal) < 192_i64.pow(2))
                })
                .count();
            let idle = army
                .iter()
                .filter(|entity| entity.order == UnitOrder::Idle)
                .count();
            let visible_enemies = world
                .state()
                .entities
                .iter()
                .filter(|entity| {
                    entity.owner == PlayerId(1) && world.entity_visible(PlayerId(0), entity.id)
                })
                .count();
            eprintln!(
                "tick={} stage={stage} army={} reached={reached} idle={idle} visible_enemies={visible_enemies} minerals={} rescued={rescued}",
                world.tick().0,
                army.len(),
                world.resource_balance(PlayerId(0), "minerals")
            );
        }
    }
    ensure!(
        world.state().winner == Some(PlayerId(0)) && rescued,
        "mission playthrough incomplete: tick={} stage={stage} winner={:?} defeated={:?}",
        world.tick().0,
        world.state().winner,
        world.state().defeated
    );
    let scenario = Scenario {
        schema_version: 1,
        seed: 42,
        ticks: world.tick().0,
        commands,
    };
    std::fs::write(
        &output,
        ron::ser::to_string_pretty(&scenario, ron::ser::PrettyConfig::default())?,
    )?;
    println!(
        "recorded {} commands through victory at tick={} hash={} to {}",
        scenario.commands.len(),
        world.tick().0,
        world.state_hash(),
        output.display()
    );
    Ok(())
}
