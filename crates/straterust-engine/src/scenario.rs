//! An in-memory input schedule shared by fixture playback and the headless harness.
//! This is deliberately not the durable replay format planned for Milestone 5.
use std::collections::BTreeMap;

use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

use crate::sim::{Command, MAX_COMMANDS_PER_TICK, Tick};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scenario {
    pub schema_version: u32,
    pub seed: u64,
    pub ticks: u64,
    pub commands: Vec<Command>,
}

impl Scenario {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.schema_version == 1,
            "unsupported scenario schema {}",
            self.schema_version
        );
        ensure!(
            (1..=10_000_000).contains(&self.ticks),
            "scenario ticks must be 1..=10000000"
        );
        ensure!(self.commands.len() <= 65536, "too many scenario commands");
        let mut counts = BTreeMap::new();
        for command in &self.commands {
            let count = counts.entry(command.tick).or_insert(0);
            *count += 1;
            ensure!(
                *count <= MAX_COMMANDS_PER_TICK,
                "too many commands at tick {}",
                command.tick.0
            );
            ensure!(
                command.tick.0 < self.ticks,
                "command at tick {} is outside the scenario",
                command.tick.0
            );
        }
        Ok(())
    }
}

#[derive(Debug, Default)]
pub struct CommandQueue {
    pending: BTreeMap<Tick, Vec<Command>>,
}

impl CommandQueue {
    pub fn from_scenario(scenario: &Scenario) -> Result<Self> {
        scenario.validate()?;
        let mut queue = Self::default();
        for command in &scenario.commands {
            queue.push(command.clone())?;
        }
        Ok(queue)
    }

    pub fn push(&mut self, command: Command) -> Result<()> {
        let commands = self.pending.entry(command.tick).or_default();
        ensure!(
            commands.len() < MAX_COMMANDS_PER_TICK,
            "command batch is full"
        );
        commands.push(command);
        Ok(())
    }

    pub fn take(&mut self, tick: Tick) -> Vec<Command> {
        self.pending.remove(&tick).unwrap_or_default()
    }
}
