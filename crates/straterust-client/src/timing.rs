use std::time::Duration;
use std::{collections::VecDeque, time::Instant};

#[derive(Clone, Copy)]
pub struct FrameStats {
    pub fps: f64,
    pub average_ms: f64,
    pub worst_ms: f64,
}

/// Actual redraw cadence, including stalls; independent of simulation speed.
#[derive(Default)]
pub struct FrameRate {
    previous: Option<Instant>,
    intervals: VecDeque<Duration>,
    total: Duration,
}
impl FrameRate {
    pub fn frame(&mut self, now: Instant) -> Option<FrameStats> {
        let previous = self.previous.replace(now)?;
        let interval = now.saturating_duration_since(previous);
        if !interval.is_zero() {
            self.intervals.push_back(interval);
            self.total += interval;
            while self.intervals.len() > 1
                && (self.total - self.intervals[0] >= Duration::from_millis(500)
                    || self.intervals.len() > 240)
            {
                self.total -= self.intervals.pop_front().unwrap();
            }
        }
        if self.total.is_zero() {
            return None;
        }
        let average = self.total.as_secs_f64() / self.intervals.len() as f64;
        Some(FrameStats {
            fps: 1.0 / average,
            average_ms: average * 1000.0,
            worst_ms: self.intervals.iter().max().unwrap().as_secs_f64() * 1000.0,
        })
    }
}

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
    fn frame_rate_reports_and_retains_stalls_then_recovers() {
        let mut rate = FrameRate::default();
        let mut now = Instant::now();
        assert!(rate.frame(now).is_none());
        for _ in 0..40 {
            now += Duration::from_millis(20);
            let stats = rate.frame(now).unwrap();
            assert!((stats.fps - 50.0).abs() < 0.01);
        }
        now += Duration::from_millis(200);
        let slow = rate.frame(now).unwrap();
        assert!(slow.fps < 40.0);
        assert_eq!(slow.worst_ms, 200.0);
        now += Duration::from_millis(20);
        assert_eq!(rate.frame(now).unwrap().worst_ms, 200.0);
        for _ in 0..30 {
            now += Duration::from_millis(20);
            rate.frame(now);
        }
        assert_eq!(rate.frame(now).unwrap().worst_ms, 20.0);
    }

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
