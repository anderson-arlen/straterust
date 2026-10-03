//! One sequential simulation owner. The window only polls completed ticks;
//! requests/results are bounded and no mutex or join blocks input/rendering.
use super::*;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, SyncSender, TryRecvError},
};
use straterust_engine::sim::{CommandOutcome, Tick};

struct Batch {
    ticks: Vec<Vec<Command>>,
    stop_on_finish: bool,
}

#[derive(Debug)]
struct CompletedTick {
    world: World,
    outcomes: Vec<CommandOutcome>,
    batch_done: bool,
}

pub(super) struct SimulationWorker {
    requests: SyncSender<Batch>,
    results: Receiver<Result<CompletedTick>>,
    cancelled: Arc<AtomicBool>,
    pending: bool,
    next_tick: Tick,
}

impl SimulationWorker {
    fn new(world: World) -> Result<Self> {
        Self::spawn(world, World::step)
    }

    // The private step seam lets tests hold a tick unfinished without sleeps.
    fn spawn(
        mut world: World,
        mut step: impl FnMut(&mut World, &[Command]) -> Result<Vec<CommandOutcome>> + Send + 'static,
    ) -> Result<Self> {
        let next_tick = world.tick();
        let (requests, incoming) = mpsc::sync_channel::<Batch>(1);
        let (outgoing, results) = mpsc::sync_channel(8);
        let cancelled = Arc::new(AtomicBool::new(false));
        let stop = Arc::clone(&cancelled);
        std::thread::Builder::new()
            .name("straterust-simulation".into())
            .spawn(move || {
                while let Ok(batch) = incoming.recv() {
                    let count = batch.ticks.len();
                    for (index, commands) in batch.ticks.into_iter().enumerate() {
                        if stop.load(Ordering::Acquire) {
                            return;
                        }
                        let outcomes = match step(&mut world, &commands) {
                            Ok(outcomes) => outcomes,
                            Err(error) => {
                                let _ = outgoing.send(Err(error));
                                return;
                            }
                        };
                        if stop.load(Ordering::Acquire) {
                            return;
                        }
                        let batch_done = index + 1 == count
                            || (batch.stop_on_finish
                                && (world.state().winner.is_some()
                                    || world.state().defeated.contains(&PlayerId(0))));
                        if outgoing
                            .send(Ok(CompletedTick {
                                world: world.snapshot(),
                                outcomes,
                                batch_done,
                            }))
                            .is_err()
                        {
                            return;
                        }
                        if batch_done {
                            break;
                        }
                    }
                }
            })
            .context("cannot start simulation worker")?;
        Ok(Self {
            requests,
            results,
            cancelled,
            pending: false,
            next_tick,
        })
    }

    pub(super) fn command_tick(&self) -> Tick {
        self.next_tick
    }

    fn submit(&mut self, ticks: Vec<Vec<Command>>, stop_on_finish: bool) -> Result<()> {
        ensure!(!self.pending, "simulation batch already pending");
        ensure!(
            (1..=8).contains(&ticks.len()),
            "invalid simulation batch size"
        );
        let next = self
            .next_tick
            .0
            .checked_add(ticks.len() as u64)
            .context("simulation tick exhausted")?;
        self.requests
            .try_send(Batch {
                ticks,
                stop_on_finish,
            })
            .context("cannot submit simulation batch")?;
        self.next_tick = Tick(next);
        self.pending = true;
        Ok(())
    }

    fn accept(&mut self, result: Result<CompletedTick>) -> Result<CompletedTick> {
        let result = result?;
        if result.batch_done {
            self.pending = false;
            self.next_tick = result.world.tick();
        }
        Ok(result)
    }

    fn poll(&mut self) -> Result<Option<CompletedTick>> {
        match self.results.try_recv() {
            Ok(result) => self.accept(result).map(Some),
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => bail!("simulation worker stopped unexpectedly"),
        }
    }
}

impl Drop for SimulationWorker {
    fn drop(&mut self) {
        // A search already in progress may finish; its result belongs to this
        // receiver only and cannot enter a restarted/replaced session.
        self.cancelled.store(true, Ordering::Release);
    }
}

impl App {
    pub(super) fn advance_simulation(&mut self, elapsed: Duration) -> Result<()> {
        let briefing = self
            .mission_ui
            .as_ref()
            .is_some_and(|mission| mission.briefing);
        if self.paused || briefing {
            return Ok(());
        }
        while let Some(completed) = self
            .simulation
            .as_mut()
            .map(SimulationWorker::poll)
            .transpose()?
            .flatten()
        {
            self.world = completed.world;
            self.observe_tick(completed.outcomes)?;
            if self.playback_end == Some(self.world.tick().0)
                || (self.playback_end.is_none()
                    && (self.world.state().winner.is_some()
                        || self.world.state().defeated.contains(&PlayerId(0))))
            {
                self.paused = true;
                return Ok(());
            }
        }
        if self.playback_end == Some(self.world.tick().0) {
            self.paused = true;
            return Ok(());
        }
        let pending = self
            .simulation
            .as_ref()
            .is_some_and(|worker| worker.pending);
        let remaining = self.playback_end.map_or(8, |end| {
            end.saturating_sub(self.world.tick().0).min(8) as u32
        });
        let count = self
            .clock
            .advance(elapsed, if pending { 0 } else { remaining });
        if count == 0 {
            return Ok(());
        }
        if self.simulation.is_none() {
            self.simulation = Some(SimulationWorker::new(self.world.snapshot())?);
        }
        let start = self.world.tick().0;
        let ticks = (0..count)
            .map(|offset| self.queue.take(Tick(start + offset as u64)))
            .collect();
        self.simulation
            .as_mut()
            .unwrap()
            .submit(ticks, self.playback_end.is_none())
    }

    fn observe_tick(&mut self, outcomes: Vec<CommandOutcome>) -> Result<()> {
        for outcome in outcomes {
            if let Some(reason) = outcome.rejection {
                self.status = format!("Order rejected: {reason:?}");
                self.audio.event(Cue::Error, None);
                log::warn!("{:?}: {reason:?}", outcome.command);
                ensure!(
                    self.playback_end.is_none(),
                    "playback command rejected: {reason:?}"
                );
            } else {
                let actor = outcome.command.order.entity();
                let unit_type = self
                    .world
                    .state()
                    .entities
                    .iter()
                    .find(|entity| entity.id == actor)
                    .map(|entity| entity.unit_type);
                if matches!(outcome.command.order, Order::Cancel { .. }) && unit_type.is_none() {
                    self.audio.forget_entity(actor);
                    self.visuals.forget_entity(actor);
                }
                if self.playback_end.is_none() {
                    self.audio.event(Cue::Order, unit_type);
                }
            }
        }
        self.visuals.update(&self.world);
        self.audio.observe(&self.world);
        if let (Some(mission), Some(media)) = (&mut self.mission_ui, &self.media)
            && let Some(position) = mission.observe(&self.world, media, &mut self.audio)
        {
            self.camera.x = f64::from(position.x);
            self.camera.y = f64::from(position.y);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
