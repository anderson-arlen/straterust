use std::{fs, path::PathBuf};

use anyhow::{Context, Result, bail, ensure};
use straterust_engine::{
    content::{Package, read_ron},
    scenario::{CommandQueue, Scenario},
    sim::SIMULATION_REVISION,
};

fn main() -> std::process::ExitCode {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp(None)
        .init();
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            log::error!("{error:#}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let mut package_path = PathBuf::from("content/fixtures");
    let mut scenario_path = None;
    let mut seed = None;
    let mut ticks = None;
    let mut hash_every = 20_u64;
    let mut dump = None;
    let mut expected = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--help" || arg == "-h" {
            println!(
                "straterust-headless [--package DIR] [--scenario FILE] [--seed N] [--ticks N]\n  [--hash-every N] [--expect-hash HEX] [--dump-state FILE]\nDefaults: content/fixtures, its scenario.ron, and a hash every 20 ticks.\nNo display or presentation assets are needed. Diagnostics go to stderr."
            );
            return Ok(());
        }
        let value = args
            .next()
            .with_context(|| format!("missing value for {arg}"))?;
        match arg.as_str() {
            "--package" => package_path = value.into(),
            "--scenario" => scenario_path = Some(PathBuf::from(value)),
            "--seed" => seed = Some(value.parse::<u64>().context("invalid seed")?),
            "--ticks" => ticks = Some(value.parse::<u64>().context("invalid tick count")?),
            "--hash-every" => hash_every = value.parse().context("invalid hash interval")?,
            "--dump-state" => dump = Some(PathBuf::from(value)),
            "--expect-hash" => expected = Some(value),
            _ => bail!("unknown argument {arg}; use --help"),
        }
    }
    ensure!(hash_every > 0, "hash interval must be positive");
    let package = Package::load(&package_path)?;
    let mut scenario: Scenario =
        read_ron(&scenario_path.unwrap_or_else(|| package_path.join("scenario.ron")))?;
    if let Some(seed) = seed {
        scenario.seed = seed;
    }
    if let Some(ticks) = ticks {
        scenario.ticks = ticks;
    }
    let mut queue = CommandQueue::from_scenario(&scenario)?;
    let mut world = package.world(scenario.seed)?;
    log::info!(
        "revision={SIMULATION_REVISION} seed={} rules={} map={}",
        scenario.seed,
        world.rules_hash(),
        world.map_hash()
    );
    println!("tick={} hash={}", world.tick().0, world.state_hash());
    for _ in 0..scenario.ticks {
        let commands = queue.take(world.tick());
        let outcomes = world.step(&commands)?;
        for outcome in outcomes {
            if let Some(reason) = outcome.rejection {
                bail!(
                    "command rejected at tick {}: {:?}: {reason:?}",
                    outcome.command.tick.0,
                    outcome.command
                );
            }
            log::debug!("applied {:?}", outcome.command);
        }
        if world.tick().0 % hash_every == 0 || world.tick().0 == scenario.ticks {
            println!("tick={} hash={}", world.tick().0, world.state_hash());
        }
    }
    if let Some(path) = dump {
        fs::write(
            &path,
            ron::ser::to_string_pretty(world.state(), ron::ser::PrettyConfig::default())?,
        )
        .with_context(|| format!("cannot write {}", path.display()))?;
    }
    if let Some(expected) = expected {
        ensure!(
            world.state_hash().to_hex().as_str() == expected,
            "state hash mismatch at tick {}: expected {expected}, actual {}; rerun with --dump-state FILE and compare scenario/seed/content identities",
            world.tick().0,
            world.state_hash()
        );
    }
    Ok(())
}
