//! Author the public demonstration's command schedule by exercising the real simulation.
//! This small test controller is not game AI and is not used by the interactive client.
use anyhow::{Context, Result, ensure};
use std::path::Path;
use straterust_engine::{
    content::Package,
    scenario::Scenario,
    sim::{Command, EntityId, Order, PlayerId, Position, UnitTypeId, World},
};

fn entity(world: &World, unit_type: u16) -> Option<EntityId> {
    world
        .state()
        .entities
        .iter()
        .find(|entity| {
            entity.owner == PlayerId(0)
                && entity.unit_type == UnitTypeId(unit_type)
                && entity.construction.is_none()
        })
        .map(|entity| entity.id)
}

fn main() -> Result<()> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/terran-demo");
    let package = Package::load(&path)?;
    let mut world = package.world(42)?;
    let worker = entity(&world, 2).context("demo lacks a worker")?;
    let resource = world
        .state()
        .resources
        .first()
        .context("demo lacks minerals")?
        .id;
    let enemy = world
        .state()
        .entities
        .iter()
        .find(|entity| entity.owner == PlayerId(1))
        .context("demo lacks a defending unit")?
        .id;
    let mut commands = Vec::new();
    let mut stage = 0;
    let mut sequence = 0;
    while world.tick().0 < 20000 && world.state().winner.is_none() {
        let balance = world.resource_balance(PlayerId(0), "minerals");
        let mut orders = Vec::new();
        match stage {
            0 => {
                orders.push(Order::Gather {
                    entity: worker,
                    resource,
                });
                stage = 1;
            }
            1 if balance >= 100 => {
                orders.push(Order::Build {
                    entity: worker,
                    unit_type: UnitTypeId(4),
                    position: Position { x: 352, y: 416 },
                });
                stage = 2;
            }
            2 if entity(&world, 4).is_some() => {
                orders.push(Order::Gather {
                    entity: worker,
                    resource,
                });
                stage = 3;
            }
            3 if balance >= 150 => {
                orders.push(Order::Build {
                    entity: worker,
                    unit_type: UnitTypeId(5),
                    position: Position { x: 576, y: 416 },
                });
                stage = 4;
            }
            4 if entity(&world, 5).is_some() => {
                orders.push(Order::Gather {
                    entity: worker,
                    resource,
                });
                stage = 5;
            }
            5 => {
                let fighters: Vec<_> = world
                    .state()
                    .entities
                    .iter()
                    .filter(|entity| {
                        entity.owner == PlayerId(0) && entity.unit_type == UnitTypeId(1)
                    })
                    .map(|entity| entity.id)
                    .collect();
                let producer = entity(&world, 5).context("demo producer was lost")?;
                let pending = world
                    .state()
                    .entities
                    .iter()
                    .find(|entity| entity.id == producer)
                    .unwrap()
                    .production
                    .len();
                if fighters.len() >= 2 {
                    for fighter in fighters {
                        orders.push(Order::Attack {
                            entity: fighter,
                            target: enemy,
                        });
                    }
                    stage = 6;
                } else if fighters.len() + pending < 2 && balance >= 50 {
                    orders.push(Order::Train {
                        entity: producer,
                        unit_type: UnitTypeId(1),
                    });
                }
            }
            _ => {}
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
                "demo command rejected: {:?}",
                outcome
            );
        }
        commands.extend(batch);
        if world.tick().0.is_multiple_of(1000) {
            eprintln!(
                "demo tick={} stage={stage} minerals={} entities={}",
                world.tick().0,
                world.resource_balance(PlayerId(0), "minerals"),
                world.state().entities.len()
            );
        }
    }
    ensure!(
        world.state().winner == Some(PlayerId(0)),
        "demo did not complete at stage {stage}: {:?}",
        world.state()
    );
    ensure!(
        entity(&world, 4).is_some() && entity(&world, 5).is_some(),
        "demo skipped construction"
    );
    ensure!(
        world.state().resources[0].amount < 1500,
        "demo skipped gathering"
    );
    let scenario = Scenario {
        schema_version: 1,
        seed: 42,
        ticks: world.tick().0,
        commands,
    };
    eprintln!(
        "completed gather/build/train/fight at tick={} hash={}",
        world.tick().0,
        world.state_hash()
    );
    println!(
        "{}",
        ron::ser::to_string_pretty(&scenario, ron::ser::PrettyConfig::default())?
    );
    Ok(())
}
