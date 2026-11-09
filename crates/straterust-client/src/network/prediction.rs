//! Confirmed network state stays separate from the speculative world on screen.
use super::*;
use straterust_engine::net::clock::{FrameTime, MAX_PREDICTION_MS};

pub(crate) struct Prediction {
    pub(super) confirmed: World,
    display_tick: u64,
    stamp: FrameTime,
    pending: Vec<Command>,
    applied_pending: BTreeSet<u64>,
}
impl Prediction {
    pub fn new(world: World, stamp: FrameTime) -> Self {
        Self {
            display_tick: world.tick().0,
            confirmed: world,
            stamp,
            pending: Vec::new(),
            applied_pending: BTreeSet::new(),
        }
    }
    pub fn command(&mut self, command: Command) {
        if !matches!(command.order, Order::Queue { .. }) && net::commands::primary(&command.order) {
            self.pending.retain(|old| {
                old.order.entity() != command.order.entity() || !net::commands::primary(&old.order)
            });
        }
        if self.pending.len() < 256 {
            self.pending.push(command);
        }
    }
    pub fn reject(&mut self, sequence: u64) {
        self.pending.retain(|command| command.sequence != sequence);
        self.applied_pending.remove(&sequence);
    }
    pub fn correct(
        &mut self,
        world: World,
        stamp: FrameTime,
        acks: &net::commands::Acknowledgements,
    ) -> World {
        self.pending
            .retain(|command| !acks.contains(command.sequence));
        self.applied_pending.clear();
        self.stamp = stamp;
        self.confirmed = world.clone();
        world
    }
    pub fn advance(&mut self, world: &mut World, estimated_server_ms: f64) -> Result<bool> {
        self.advance_until(
            world,
            estimated_server_ms,
            Instant::now() + Duration::from_millis(4),
        )
    }
    fn advance_until(
        &mut self,
        world: &mut World,
        estimated_server_ms: f64,
        deadline: Instant,
    ) -> Result<bool> {
        let age = (estimated_server_ms - self.stamp.captured_ms as f64)
            .max(0.)
            .min(MAX_PREDICTION_MS as f64);
        let target = self
            .confirmed
            .tick()
            .0
            .saturating_add((age / f64::from(self.stamp.tick_ms)) as u64)
            .max(self.display_tick);
        let mut changed = false;
        // Replay at each input's original local tick, not at an older baseline.
        for step in 0..=100 {
            for command in &self.pending {
                if command.tick <= world.tick() && self.applied_pending.insert(command.sequence) {
                    world.predict_command(command)?;
                    changed = true;
                }
            }
            // One expensive tick must not turn into a hundred-tick redraw
            // stall. Yield between ticks so input, animation and fresh views
            // can be processed before continuing speculative catch-up.
            if world.tick().0 >= target || step == 100 || step > 0 && Instant::now() >= deadline {
                break;
            }
            world.predict_step()?;
            changed = true;
        }
        self.display_tick = self.display_tick.max(world.tick().0);
        Ok(changed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn catch_up(prediction: &mut Prediction, world: &mut World, server_ms: f64) {
        for _ in 0..101 {
            let before = world.tick();
            prediction.advance(world, server_ms).unwrap();
            if world.tick() == before {
                return;
            }
        }
        panic!("prediction did not reach its bounded target");
    }
    fn view() -> World {
        let package =
            Package::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/terran-demo"))
                .unwrap();
        let world = package.world(42).unwrap();
        world
            .player_view(PlayerId(0))
            .unwrap()
            .into_world(&world)
            .unwrap()
    }
    #[test]
    fn prediction_yields_between_ticks_and_keeps_the_previous_display_target() {
        let mut world = view();
        let initial = world.clone();
        let stamp = FrameTime {
            captured_ms: 1000,
            tick_ms: 40,
        };
        let mut prediction = Prediction::new(world.clone(), stamp);
        let expired = Instant::now();
        prediction
            .advance_until(&mut world, 2000., expired)
            .unwrap();
        assert_eq!(world.tick().0, 1);
        for _ in 1..25 {
            prediction
                .advance_until(&mut world, 2000., expired)
                .unwrap();
        }
        assert_eq!(world.tick().0, 25);
        world = prediction.correct(initial, stamp, &Default::default());
        // Even a younger correction must eventually catch up to the last
        // displayed tick after yielding across multiple redraws.
        for _ in 0..25 {
            prediction
                .advance_until(&mut world, 1000., expired)
                .unwrap();
        }
        assert_eq!(world.tick().0, 25);
        assert_eq!(prediction.confirmed.tick().0, 0);
    }
    #[test]
    fn prediction_continues_through_a_gap_and_replays_unconfirmed_input_once() {
        let mut world = view();
        let actor = world
            .state()
            .entities
            .iter()
            .find(|e| world.unit_type(e.unit_type).unwrap().worker.is_some())
            .unwrap()
            .clone();
        let stamp = FrameTime {
            captured_ms: 1000,
            tick_ms: 40,
        };
        let mut prediction = Prediction::new(world.clone(), stamp);
        prediction.command(Command {
            tick: world.tick(),
            player: PlayerId(0),
            sequence: 1,
            order: Order::Move {
                entity: actor.id,
                target: Position { x: 900, y: 256 },
            },
        });
        let state = prediction.confirmed.state_hash();
        catch_up(&mut prediction, &mut world, 2000.);
        assert_eq!(world.tick().0, 25);
        let moved = world
            .state()
            .entities
            .iter()
            .find(|e| e.id == actor.id)
            .unwrap()
            .position;
        assert!(moved.x > actor.position.x);
        assert_eq!(prediction.confirmed.state_hash(), state);
        let confirmed = prediction.confirmed.clone();
        world = prediction.correct(confirmed, stamp, &Default::default());
        catch_up(&mut prediction, &mut world, 2000.);
        assert_eq!(
            world
                .state()
                .entities
                .iter()
                .find(|e| e.id == actor.id)
                .unwrap()
                .position,
            moved
        );
        catch_up(&mut prediction, &mut world, 30_000.);
        assert_eq!(world.tick().0, 50); // two-second prediction horizon
        catch_up(&mut prediction, &mut world, 40_000.);
        assert_eq!(world.tick().0, 50);
    }
    #[test]
    fn reconciliation_does_not_apply_input_before_its_original_tick() {
        let initial = view();
        let actor = initial
            .state()
            .entities
            .iter()
            .find(|e| initial.unit_type(e.unit_type).unwrap().worker.is_some())
            .unwrap()
            .clone();
        let stamp = FrameTime {
            captured_ms: 1000,
            tick_ms: 40,
        };
        let mut prediction = Prediction::new(initial.clone(), stamp);
        prediction.command(Command {
            tick: straterust_engine::sim::Tick(2),
            player: PlayerId(0),
            sequence: 1,
            order: Order::Move {
                entity: actor.id,
                target: Position { x: 900, y: 256 },
            },
        });
        let mut world = initial.clone();
        catch_up(&mut prediction, &mut world, 1040.);
        assert_eq!(
            world
                .state()
                .entities
                .iter()
                .find(|e| e.id == actor.id)
                .unwrap()
                .position,
            actor.position
        );
        catch_up(&mut prediction, &mut world, 1120.);
        let position = world
            .state()
            .entities
            .iter()
            .find(|e| e.id == actor.id)
            .unwrap()
            .position;
        assert_ne!(position, actor.position);
        assert!(
            (position.x - actor.position.x).abs() <= 5
                && (position.y - actor.position.y).abs() <= 5
        );
        world = prediction.correct(initial, stamp, &Default::default());
        catch_up(&mut prediction, &mut world, 1120.);
        assert_eq!(
            world
                .state()
                .entities
                .iter()
                .find(|e| e.id == actor.id)
                .unwrap()
                .position,
            position
        );
    }
}
