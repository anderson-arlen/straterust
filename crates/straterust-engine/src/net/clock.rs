//! Session-relative monotonic time; no system clock synchronization is required.
use serde::{Deserialize, Serialize};
use std::{collections::VecDeque, time::Instant};

pub const UPDATE_MS: u64 = 100;
pub const MAX_FRAME_AGE_MS: u64 = 250;
pub const MAX_PREDICTION_MS: u64 = 2000;

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct FrameTime {
    pub captured_ms: u64,
    pub tick_ms: u32,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Pong {
    pub echo_ms: u64,
    pub received_ms: u64,
    pub sent_ms: u64,
}
#[derive(Clone, Copy)]
struct Sample {
    at: u64,
    offset: f64,
    rtt: f64,
}

pub struct ClockSync {
    origin: Instant,
    samples: VecDeque<Sample>,
    offset: Option<f64>,
    last_adjusted: u64,
    last_probe: Option<u64>,
    rejected_frames: u64,
    last_rejection_report: Option<u64>,
    pub tick_ms: u32,
    pub max_frame_age_ms: u64,
}
impl Default for ClockSync {
    fn default() -> Self {
        Self {
            origin: Instant::now(),
            samples: VecDeque::new(),
            offset: None,
            last_adjusted: 0,
            last_probe: None,
            rejected_frames: 0,
            last_rejection_report: None,
            tick_ms: 33,
            max_frame_age_ms: MAX_FRAME_AGE_MS,
        }
    }
}
impl ClockSync {
    pub fn now_ms(&self) -> u64 {
        self.origin.elapsed().as_millis() as u64
    }
    pub fn probe(&mut self) -> Option<u64> {
        let now = self.now_ms();
        let interval = if self.samples.len() < 4 { 100 } else { 2000 };
        if self
            .last_probe
            .is_none_or(|last| now.saturating_sub(last) >= interval)
        {
            self.last_probe = Some(now);
            Some(now)
        } else {
            None
        }
    }
    pub fn observe(&mut self, pong: Pong, received: u64) {
        if pong.echo_ms > received || pong.sent_ms < pong.received_ms {
            return;
        }
        let elapsed = received - pong.echo_ms;
        let processing = pong.sent_ms - pong.received_ms;
        if processing > elapsed {
            return;
        }
        let rtt = (elapsed - processing) as f64;
        // A stall is not a clock adjustment. Keep the previous estimate through it.
        if rtt > 250. {
            return;
        }
        let offset = ((pong.received_ms as f64 - pong.echo_ms as f64)
            + (pong.sent_ms as f64 - received as f64))
            * 0.5;
        self.samples.push_back(Sample {
            at: received,
            offset,
            rtt,
        });
        while self
            .samples
            .front()
            .is_some_and(|s| received.saturating_sub(s.at) > 30_000)
            || self.samples.len() > 16
        {
            self.samples.pop_front();
        }
        let best = self
            .samples
            .iter()
            .min_by(|a, b| a.rtt.total_cmp(&b.rtt))
            .unwrap();
        self.offset = Some(match self.offset {
            None => best.offset,
            Some(previous) => {
                let allowance = received.saturating_sub(self.last_adjusted) as f64 * 0.002;
                previous + (best.offset - previous).clamp(-allowance, allowance)
            }
        });
        self.last_adjusted = received;
    }
    pub fn server_ms(&self, local: u64) -> Option<f64> {
        self.offset.map(|offset| local as f64 + offset)
    }
    pub fn age_ms(&self, stamp: FrameTime, local: u64) -> Option<f64> {
        self.server_ms(local)
            .map(|now| now - stamp.captured_ms as f64)
    }
    pub fn fresh(&self, stamp: FrameTime, local: u64, limit: u64) -> bool {
        self.age_ms(stamp, local)
            .is_some_and(|age| age >= -50. && age <= limit as f64)
    }
    /// Report rejected corrections in normal logs, without logging every frame.
    pub fn accepts_frame(&mut self, stamp: FrameTime, local: u64, limit: u64) -> bool {
        let fresh = self.fresh(stamp, local, limit);
        if !fresh && let Some(age) = self.age_ms(stamp, local) {
            self.rejected_frames += 1;
            if self
                .last_rejection_report
                .is_none_or(|at| local.saturating_sub(at) >= 10_000)
            {
                log::warn!(
                    "discarded {} server corrections: age_ms={age:.1}, limit_ms={limit}, clock_uncertainty_ms={:?}",
                    self.rejected_frames,
                    self.uncertainty_ms()
                );
                self.rejected_frames = 0;
                self.last_rejection_report = Some(local);
            }
        }
        fresh
    }
    pub fn uncertainty_ms(&self) -> Option<f64> {
        self.samples
            .iter()
            .map(|s| s.rtt)
            .min_by(f64::total_cmp)
            .map(|rtt| rtt * 0.5)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn arbitrary_epochs_and_stalls_do_not_make_old_frames_fresh() {
        let mut clock = ClockSync::default();
        clock.observe(
            Pong {
                echo_ms: 100,
                received_ms: 10_110,
                sent_ms: 10_112,
            },
            122,
        );
        assert_eq!(clock.server_ms(200), Some(10_200.));
        let stamp = FrameTime {
            captured_ms: 10_200,
            tick_ms: 40,
        };
        assert!(clock.fresh(stamp, 300, 250));
        assert!(!clock.fresh(stamp, 1200, 250));
        clock.observe(
            Pong {
                echo_ms: 200,
                received_ms: 10_210,
                sent_ms: 10_212,
            },
            1222,
        );
        assert_eq!(clock.server_ms(1200), Some(11_200.));
        assert!(!clock.fresh(stamp, 1200, 250));
        assert_eq!(clock.uncertainty_ms(), Some(10.));
    }
}
