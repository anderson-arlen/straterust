//! Per-connection command identity, supersession, retry and queued dependencies.
//! The ordinary session still executes and records canonical simulation commands.
use super::*;
use crate::sim::{CommandOutcome, EntityId, Order, UnitOrder};

const MAX_PENDING: usize = 256;
const RETRY: Duration = Duration::from_millis(200);

/// Inclusive ranges encode individual ACKs compactly, including gaps. A larger
/// ID never implies that any missing smaller ID was received or processed.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Acknowledgements(Vec<[u64; 2]>);
impl Acknowledgements {
    pub fn contains(&self, id: u64) -> bool {
        self.0
            .iter()
            .any(|[first, last]| *first <= id && id <= *last)
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.0.len() <= 4096
                && self.0.iter().all(|[a, b]| *a > 0 && a <= b)
                && self
                    .0
                    .windows(2)
                    .all(|w| w[0][1] < w[1][0].saturating_sub(1)),
            "invalid command acknowledgements"
        );
        Ok(())
    }
    fn insert(&mut self, id: u64) {
        self.insert_range(id, id);
    }
    fn insert_range(&mut self, first: u64, last: u64) {
        self.0.push([first, last]);
        self.0.sort_unstable();
        let mut merged: Vec<[u64; 2]> = Vec::new();
        for [first, last] in self.0.drain(..) {
            if let Some(previous) = merged.last_mut()
                && first <= previous[1].saturating_add(1)
            {
                previous[1] = previous[1].max(last);
            } else {
                merged.push([first, last]);
            }
        }
        self.0 = merged;
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    pub command: Command,
    pub replace_orders: bool,
    /// Only queued orders wait for a predecessor belonging to this unit.
    pub after: Option<u64>,
}

/// Whether an order changes the unit's main activity. Production, research,
/// rally points and secondary toggles remain independent, once-only actions.
pub fn primary(order: &Order) -> bool {
    !matches!(
        order,
        Order::Train { .. }
            | Order::Research { .. }
            | Order::Rally { .. }
            | Order::RallyResource { .. }
            | Order::Cloak { .. }
            | Order::Stim { .. }
            | Order::Scan { .. }
            | Order::UnloadPassenger { .. }
            | Order::Cancel { .. }
            | Order::ChangeMode { .. }
    )
}

fn queued(order: &Order) -> Option<UnitOrder> {
    Some(match *order {
        Order::Move { target, .. } => UnitOrder::Move { target },
        Order::Attack { target, .. } => UnitOrder::Attack { target },
        Order::AttackMove { target, .. } => UnitOrder::AttackMove { target },
        Order::Patrol { target, .. } => UnitOrder::Patrol { target },
        Order::Gather { resource, .. } => UnitOrder::Gather { resource },
        Order::Repair { target, .. } => UnitOrder::Repair { target },
        Order::Load { target, .. } => UnitOrder::Load { target },
        Order::Land { target, .. } => UnitOrder::Land { target },
        Order::UnloadAt { target, .. } => UnitOrder::UnloadAt { target },
        Order::PlaceMine { target, .. } => UnitOrder::PlaceMine { target },
        Order::Hold { .. } => UnitOrder::Hold,
        Order::Stop { .. } => UnitOrder::Idle,
        Order::Build {
            unit_type,
            position,
            ..
        } => UnitOrder::PlaceBuilding {
            unit_type,
            target: position,
        },
        Order::Cast {
            ability, target, ..
        } => UnitOrder::Cast { ability, target },
        Order::ReceiveAbility {
            provider, ability, ..
        } => UnitOrder::ReceiveAbility { provider, ability },
        _ => return None,
    })
}
fn unqueue(entity: EntityId, order: UnitOrder) -> Result<Order> {
    Ok(match order {
        UnitOrder::Move { target } => Order::Move { entity, target },
        UnitOrder::Attack { target } => Order::Attack { entity, target },
        UnitOrder::AttackMove { target } => Order::AttackMove { entity, target },
        UnitOrder::Patrol { target } => Order::Patrol { entity, target },
        UnitOrder::Gather { resource } => Order::Gather { entity, resource },
        UnitOrder::Repair { target } => Order::Repair { entity, target },
        UnitOrder::Load { target } => Order::Load { entity, target },
        UnitOrder::Land { target } => Order::Land { entity, target },
        UnitOrder::UnloadAt { target } => Order::UnloadAt { entity, target },
        UnitOrder::PlaceMine { target } => Order::PlaceMine { entity, target },
        UnitOrder::Hold => Order::Hold { entity },
        UnitOrder::Idle => Order::Stop { entity },
        UnitOrder::PlaceBuilding { unit_type, target } => Order::Build {
            entity,
            unit_type,
            position: target,
        },
        UnitOrder::Cast { ability, target } => Order::Cast {
            entity,
            ability,
            target,
        },
        UnitOrder::ReceiveAbility { provider, ability } => Order::ReceiveAbility {
            entity,
            provider,
            ability,
        },
        _ => bail!("unsupported queued network order"),
    })
}

#[derive(Default)]
pub(super) struct Outbox {
    pending: BTreeMap<u64, (Input, Option<Instant>)>,
    previous: BTreeMap<EntityId, u64>,
    highest: u64,
}
impl Outbox {
    pub fn push(&mut self, mut command: Command) -> Result<()> {
        let id = command.sequence;
        ensure!(id > 0, "invalid command ID");
        if id <= self.highest {
            return Ok(());
        }
        let replace_orders = !matches!(command.order, Order::Queue { .. });
        if let Order::Queue { entity, order } = command.order {
            command.order = unqueue(entity, order)?;
        }
        let entity = command.order.entity();
        let mut after = None;
        if primary(&command.order) {
            if replace_orders {
                self.pending.retain(|old, (input, _)| {
                    *old > id
                        || input.command.order.entity() != entity
                        || !primary(&input.command.order)
                });
            } else {
                after = self.previous.get(&entity).copied();
            }
        }
        ensure!(
            self.pending.len() < MAX_PENDING,
            "too many unacknowledged commands"
        );
        if primary(&command.order) {
            self.previous.insert(entity, id);
        }
        self.pending.insert(
            id,
            (
                Input {
                    command,
                    replace_orders,
                    after,
                },
                None,
            ),
        );
        self.highest = id;
        Ok(())
    }
    /// Nothing below the oldest still-valid pending input can be retried. This
    /// bounds server deduplication history even after many cancelled orders.
    pub fn retired(&self) -> u64 {
        self.pending.keys().next().map_or(self.highest, |id| id - 1)
    }
    pub fn accept(&mut self, acks: &Acknowledgements) -> Result<()> {
        acks.validate()?;
        self.pending.retain(|id, _| !acks.contains(*id));
        self.previous.retain(|_, id| self.pending.contains_key(id));
        Ok(())
    }
    pub fn due(&mut self) -> Option<Input> {
        let (_, (input, sent)) = self
            .pending
            .iter_mut()
            .find(|(_, (_, sent))| sent.is_none_or(|at| at.elapsed() >= RETRY))?;
        *sent = Some(Instant::now());
        Some(input.clone())
    }
}

#[derive(Default)]
pub(super) struct Inbox {
    pub acks: Acknowledgements,
    waiting: BTreeMap<u64, Input>,
    cutoff: BTreeMap<EntityId, u64>,
    scheduled: BTreeMap<u64, Command>,
}
impl Inbox {
    pub fn retire(&mut self, through: u64) {
        if through == 0 {
            return;
        }
        self.acks.insert_range(1, through);
        self.waiting.retain(|id, _| *id > through);
    }
    pub fn receive(&mut self, input: Input) -> Result<()> {
        let id = input.command.sequence;
        ensure!(
            id > 0
                && input
                    .after
                    .is_none_or(|prior| prior < id && !input.replace_orders)
                && !matches!(input.command.order, Order::Queue { .. })
                && (input.replace_orders
                    || !primary(&input.command.order)
                    || queued(&input.command.order).is_some()),
            "invalid command ID or dependency"
        );
        if self.acks.contains(id)
            || self.waiting.contains_key(&id)
            || self.scheduled.values().any(|c| c.sequence == id)
        {
            return Ok(());
        }
        let entity = input.command.order.entity();
        if primary(&input.command.order) {
            let cutoff = self.cutoff.entry(entity).or_default();
            if id < *cutoff {
                self.acks.insert(id);
                return Ok(());
            }
            if input.replace_orders {
                *cutoff = id;
            }
        }
        ensure!(
            self.waiting.len() + self.scheduled.len() < MAX_PENDING,
            "too many pending network commands"
        );
        self.waiting.insert(id, input);
        Ok(())
    }
    /// Missing IDs on other units do not block this unit. Queued dependencies
    /// preserve waypoint order without requiring a global contiguous sequence.
    pub fn ready(&mut self, limit: usize) -> Vec<Input> {
        let mut ready = Vec::new();
        let mut admitted = std::collections::BTreeSet::new();
        for id in self.waiting.keys().copied().collect::<Vec<_>>() {
            let input = &self.waiting[&id];
            let cutoff = self
                .cutoff
                .get(&input.command.order.entity())
                .copied()
                .unwrap_or(0);
            if primary(&input.command.order) && id < cutoff {
                self.waiting.remove(&id);
                self.acks.insert(id);
            } else if ready.len() < limit
                && input.after.is_none_or(|prior| {
                    self.acks.contains(prior)
                        || admitted.contains(&prior)
                        || self.scheduled.values().any(|c| c.sequence == prior)
                })
            {
                admitted.insert(id);
                ready.push(self.waiting.remove(&id).unwrap());
            }
        }
        ready
    }
    pub fn schedule(
        &mut self,
        input: Input,
        sequence: u64,
        tick: crate::sim::Tick,
    ) -> Result<Command> {
        let mut command = input.command;
        command.tick = tick;
        if !input.replace_orders && primary(&command.order) {
            command.order = Order::Queue {
                entity: command.order.entity(),
                order: queued(&command.order).context("order cannot be queued")?,
            };
        }
        self.scheduled.insert(sequence, command.clone());
        command.sequence = sequence;
        Ok(command)
    }
    pub fn finish(&mut self, outcomes: &[CommandOutcome]) -> Vec<CommandOutcome> {
        outcomes
            .iter()
            .filter_map(|outcome| {
                let original = self.scheduled.get(&outcome.command.sequence)?;
                if original.player != outcome.command.player {
                    return None;
                }
                let command = self.scheduled.remove(&outcome.command.sequence).unwrap();
                self.acks.insert(command.sequence);
                Some(CommandOutcome {
                    command,
                    rejection: outcome.rejection.clone(),
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests;
