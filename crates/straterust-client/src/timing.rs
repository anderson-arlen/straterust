use std::time::Duration;

/// Presentation supplies elapsed time. Debt is retained when catch-up is capped.
pub struct TickClock {
    step: Duration,
    debt: Duration,
}

impl TickClock {
    pub fn new(tick_ms: u32) -> Self {
        Self {
            step: Duration::from_millis(u64::from(tick_ms)),
            debt: Duration::ZERO,
        }
    }

    /// A busy worker uses a zero limit to retain debt without taking new ticks.
    pub fn advance(&mut self, elapsed: Duration, limit: u32) -> usize {
        self.debt = self.debt.saturating_add(elapsed);
        let ticks =
            (self.debt.as_nanos() / self.step.as_nanos()).min(u128::from(limit.min(8))) as u32;
        self.debt -= self.step * ticks;
        ticks as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use straterust_engine::{
        content::{Package, read_ron},
        scenario::{CommandQueue, Scenario},
    };

    #[test]
    fn frame_frequency_and_stalls_do_not_change_simulation() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/fixtures");
        let package = Package::load(&path).unwrap();
        let scenario: Scenario = read_ron(&path.join("scenario.ron")).unwrap();
        let mut hashes = Vec::new();
        for frame_ms in [5, 16, 33, 250, 2000] {
            let mut world = package.world(scenario.seed).unwrap();
            let mut queue = CommandQueue::from_scenario(&scenario).unwrap();
            let mut clock = TickClock::new(world.rules().tick_ms);
            for _ in 0..(12_000 / frame_ms) {
                for _ in 0..clock.advance(Duration::from_millis(frame_ms), 8) {
                    world.step(&queue.take(world.tick())).unwrap();
                }
            }
            loop {
                let ticks = clock.advance(Duration::ZERO, 8);
                if ticks == 0 {
                    break;
                }
                for _ in 0..ticks {
                    world.step(&queue.take(world.tick())).unwrap();
                }
            }
            for _ in 0..clock.advance(Duration::from_millis(12_000 % frame_ms), 8) {
                world.step(&queue.take(world.tick())).unwrap();
            }
            assert_eq!(world.tick().0, scenario.ticks);
            hashes.push(world.state_hash());
        }
        assert!(hashes.iter().all(|hash| *hash == hashes[0]));
    }
}
