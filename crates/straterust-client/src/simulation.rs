//! One sequential simulation owner. The window only polls completed ticks;
//! requests/results are bounded and no mutex or join blocks input/rendering.
use super::*;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, SyncSender, TryRecvError},
};
use straterust_engine::{
    session::{PlayerUpdate, ServerSession},
    sim::{CommandOutcome, Tick},
};

struct Batch {
    generation: u64,
    ticks: Vec<Vec<Command>>,
    stop_on_finish: bool,
}

enum Request {
    Batch(Batch),
    Restart(u64),
    Record(PathBuf),
    Save {
        path: PathBuf,
        header: Box<saves::SaveHeader>,
        commands: Vec<Command>,
    },
}

#[derive(Debug)]
struct CompletedTick {
    generation: u64,
    update: PlayerUpdate,
    batch_done: bool,
}

pub(super) struct SimulationWorker {
    requests: SyncSender<Request>,
    results: Receiver<Result<CompletedTick>>,
    cancelled: Arc<AtomicBool>,
    pending: bool,
    generation: u64,
    next_tick: Tick,
    thread: Option<std::thread::JoinHandle<()>>,
    report: Arc<AtomicBool>,
    saved: Receiver<Result<PathBuf>>,
}

impl SimulationWorker {
    pub(super) fn local(
        world: World,
        seed: u64,
        scenario: Option<Scenario>,
    ) -> Result<(Self, World)> {
        let mut server = ServerSession::new(world, seed, vec![PlayerId(0)])?;
        let hello = straterust_engine::session::Hello {
            protocol: straterust_engine::session::PROTOCOL_VERSION,
            identity: straterust_engine::sim::GameplayIdentity::of(server.world()),
            player: PlayerId(0),
            expected_map: Some(server.world().map_hash().to_hex().to_string()),
        };
        let (handshake, update) = server.connect(&hello)?;
        handshake.validate(server.world())?;
        let initial = update.view.into_world(server.world())?;
        let worker = Self::spawn_server(server, scenario)?;
        Ok((worker, initial))
    }

    pub(super) fn restored(
        definitions: World,
        snapshot: straterust_engine::session::SavedGame,
    ) -> Result<(Self, World, World)> {
        let mut server = ServerSession::restore_saved(&definitions, snapshot)?;
        let initial = server
            .update(PlayerId(0), &[])?
            .view
            .into_world(&definitions)?;
        let restart_view = definitions
            .player_view(PlayerId(0))?
            .into_world(&definitions)?;
        let worker = Self::spawn_inner(server, None, Some(definitions), || Ok(()))?;
        Ok((worker, initial, restart_view))
    }

    fn spawn_server(server: ServerSession, scenario: Option<Scenario>) -> Result<Self> {
        Self::spawn_with_hook(server, scenario, || Ok(()))
    }

    fn spawn_with_hook(
        server: ServerSession,
        scenario: Option<Scenario>,
        before_tick: impl FnMut() -> Result<()> + Send + 'static,
    ) -> Result<Self> {
        Self::spawn_inner(server, scenario, None, before_tick)
    }

    fn spawn_inner(
        mut server: ServerSession,
        scenario: Option<Scenario>,
        restart_world: Option<World>,
        mut before_tick: impl FnMut() -> Result<()> + Send + 'static,
    ) -> Result<Self> {
        let next_tick = server.world().tick();
        let mut replay_path = None;
        let mut playback = scenario
            .as_ref()
            .map(CommandQueue::from_scenario)
            .transpose()?
            .unwrap_or_default();
        let (requests, incoming) = mpsc::sync_channel::<Request>(2);
        let (outgoing, results) = mpsc::sync_channel(8);
        let (save_results, saved) = mpsc::sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        let stop = Arc::clone(&cancelled);
        let report = Arc::new(AtomicBool::new(false));
        let server_report = Arc::clone(&report);
        let thread = std::thread::Builder::new()
            .name("straterust-local-server".into())
            .spawn(move || {
                let mut run = || -> Result<()> {
                    while let Ok(request) = incoming.recv() {
                        let batch = match request {
                            Request::Batch(batch) => batch,
                            Request::Record(path) => {
                                replay_path = Some(path);
                                continue;
                            }
                            Request::Save {
                                path,
                                header,
                                commands,
                            } => {
                                let result = (|| -> Result<PathBuf> {
                                    for command in commands {
                                        server.submit(PlayerId(0), command)?;
                                    }
                                    saves::write(
                                        &path,
                                        &header,
                                        &straterust_engine::session::SavedGame::capture(&server)?,
                                    )?;
                                    Ok(path)
                                })();
                                let _ = save_results.try_send(result);
                                continue;
                            }
                            Request::Restart(generation) => {
                                if let Some(world) = &restart_world {
                                    server = ServerSession::new(
                                        world.clone(),
                                        server.replay().seed,
                                        vec![PlayerId(0)],
                                    )?;
                                } else {
                                    server.restart()?;
                                }
                                playback = scenario
                                    .as_ref()
                                    .map(CommandQueue::from_scenario)
                                    .transpose()?
                                    .unwrap_or_default();
                                let update = server.update(PlayerId(0), &[])?;
                                if outgoing
                                    .send(Ok(CompletedTick {
                                        generation,
                                        update,
                                        batch_done: true,
                                    }))
                                    .is_err()
                                {
                                    return Ok(());
                                }
                                continue;
                            }
                        };
                        let count = batch.ticks.len();
                        for (index, commands) in batch.ticks.into_iter().enumerate() {
                            if stop.load(Ordering::Acquire) {
                                return Ok(());
                            }
                            for command in commands {
                                server.submit(PlayerId(0), command)?;
                            }
                            let scripted = playback.take(server.world().tick());
                            before_tick()?;
                            let outcomes = server.advance(&scripted)?;
                            if stop.load(Ordering::Acquire) {
                                return Ok(());
                            }
                            let batch_done = index + 1 == count
                                || (batch.stop_on_finish
                                    && (server.world().state().winner.is_some()
                                        || server.world().state().defeated.contains(&PlayerId(0))));
                            let update = server.update(PlayerId(0), &outcomes)?;
                            if outgoing
                                .send(Ok(CompletedTick {
                                    generation: batch.generation,
                                    update,
                                    batch_done,
                                }))
                                .is_err()
                            {
                                return Ok(());
                            }
                            if batch_done {
                                break;
                            }
                        }
                    }
                    Ok(())
                };
                if let Err(error) = run() {
                    let _ = outgoing.send(Err(error));
                }
                // Explicit developer smoke mode reports from the server itself,
                // never from the filtered window's world.
                if server_report.load(Ordering::Acquire) {
                    println!(
                        "tick={} hash={}",
                        server.world().tick().0,
                        server.world().state_hash()
                    );
                }
                if let Some(path) = replay_path
                    && let Err(error) = straterust_engine::net::save_replay(&path, server.replay())
                {
                    log::error!("cannot save replay: {error:#}");
                }
            })
            .context("cannot start local server")?;
        Ok(Self {
            requests,
            results,
            cancelled,
            pending: false,
            generation: 0,
            next_tick,
            thread: Some(thread),
            report,
            saved,
        })
    }

    pub(super) fn finish(mut self, report: bool) -> Result<()> {
        self.report.store(report, Ordering::Release);
        self.cancelled.store(true, Ordering::Release);
        let (sender, _) = mpsc::sync_channel(1);
        let (_, receiver) = mpsc::sync_channel(1);
        drop(std::mem::replace(&mut self.requests, sender));
        drop(std::mem::replace(&mut self.results, receiver));
        if let Some(thread) = self.thread.take() {
            thread
                .join()
                .map_err(|_| anyhow::anyhow!("local server failed during shutdown"))?;
        }
        Ok(())
    }

    pub(super) fn record_to(&self, path: PathBuf) -> Result<()> {
        self.requests
            .try_send(Request::Record(path))
            .context("server request queue full")
    }

    pub(super) fn is_pending(&self) -> bool {
        self.pending
    }

    #[cfg(test)]
    pub(super) fn poll_for_test(&mut self) -> Result<Option<Tick>> {
        Ok(self.poll()?.map(|r| r.update.view.tick))
    }

    pub(super) fn save(
        &self,
        path: PathBuf,
        header: saves::SaveHeader,
        commands: Vec<Command>,
    ) -> Result<()> {
        ensure!(
            !self.pending,
            "wait for the current simulation batch before saving"
        );
        self.requests
            .try_send(Request::Save {
                path,
                header: Box::new(header),
                commands,
            })
            .context("server save request queue full")
    }

    pub(super) fn poll_save(&self) -> Result<Option<PathBuf>> {
        match self.saved.try_recv() {
            Ok(result) => result.map(Some),
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => bail!("save worker stopped"),
        }
    }

    pub(super) fn restart(&mut self) -> Result<()> {
        self.generation = self
            .generation
            .checked_add(1)
            .context("session generation exhausted")?;
        self.requests
            .try_send(Request::Restart(self.generation))
            .context("server restart request queue full")?;
        self.next_tick = Tick(0);
        self.pending = true;
        Ok(())
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
            .try_send(Request::Batch(Batch {
                generation: self.generation,
                ticks,
                stop_on_finish,
            }))
            .context("cannot submit simulation batch")?;
        self.next_tick = Tick(next);
        self.pending = true;
        Ok(())
    }

    fn accept(&mut self, result: Result<CompletedTick>) -> Result<CompletedTick> {
        let result = result?;
        if result.batch_done {
            self.pending = false;
            self.next_tick = result.update.view.tick;
        }
        Ok(result)
    }

    fn poll(&mut self) -> Result<Option<CompletedTick>> {
        loop {
            match self.results.try_recv() {
                Ok(Ok(result)) if result.generation != self.generation => continue,
                Ok(result) => return self.accept(result).map(Some),
                Err(TryRecvError::Empty) => return Ok(None),
                Err(TryRecvError::Disconnected) => bail!("local server stopped unexpectedly"),
            }
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
        self.poll_simulation()?;
        if self.paused {
            return Ok(());
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
        ensure!(self.simulation.is_some(), "local server is unavailable");
        let start = self.world.tick().0;
        let ticks = (0..count)
            .map(|offset| self.queue.take(Tick(start + offset as u64)))
            .collect();
        self.simulation
            .as_mut()
            .unwrap()
            .submit(ticks, self.playback_end.is_none())
    }

    pub(super) fn poll_simulation(&mut self) -> Result<()> {
        while let Some(completed) = self
            .simulation
            .as_mut()
            .map(SimulationWorker::poll)
            .transpose()?
            .flatten()
        {
            let outcomes = completed.update.outcomes;
            self.match_result = completed.update.result;
            self.world = completed.update.view.into_world(&self.world)?;
            self.observe_tick(outcomes)?;
            if self.playback_end == Some(self.world.tick().0)
                || (self.playback_end.is_none()
                    && (self.world.state().winner.is_some()
                        || self.world.state().defeated.contains(&PlayerId(0))))
            {
                self.paused = true;
                return Ok(());
            }
        }
        Ok(())
    }

    pub(super) fn observe_tick(&mut self, outcomes: Vec<CommandOutcome>) -> Result<()> {
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
