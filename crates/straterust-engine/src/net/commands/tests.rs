use super::*;
use crate::sim::{EntityId, Position, Tick, UnitTypeId};

fn input(id: u64, entity: u32, replace_orders: bool, after: Option<u64>) -> Input {
    Input {
        command: Command {
            tick: Tick(0),
            player: PlayerId(0),
            sequence: id,
            order: Order::Move {
                entity: EntityId(entity),
                target: Position {
                    x: id as i32 * 20,
                    y: 100,
                },
            },
        },
        replace_orders,
        after,
    }
}
fn finish(inbox: &mut Inbox, ready: Vec<Input>) -> Vec<Command> {
    let commands: Vec<_> = ready
        .into_iter()
        .enumerate()
        .map(|(i, input)| inbox.schedule(input, i as u64 + 100, Tick(0)).unwrap())
        .collect();
    inbox.finish(
        &commands
            .iter()
            .cloned()
            .map(|command| CommandOutcome {
                command,
                rejection: None,
            })
            .collect::<Vec<_>>(),
    );
    commands
}

#[test]
fn newest_replacement_wins_and_both_old_and_new_ids_are_acked() {
    for order in [[2, 1], [1, 2]] {
        let mut inbox = Inbox::default();
        for id in order {
            inbox.receive(input(id, 1, true, None)).unwrap();
        }
        let ready = inbox.ready(usize::MAX);
        assert_eq!(
            ready.iter().map(|i| i.command.sequence).collect::<Vec<_>>(),
            [2]
        );
        finish(&mut inbox, ready);
        assert!(inbox.acks.contains(1) && inbox.acks.contains(2));
        inbox.receive(input(1, 1, true, None)).unwrap();
        assert!(inbox.ready(usize::MAX).is_empty());
    }
}

#[test]
fn queued_dependencies_sort_ids_without_blocking_independent_units() {
    let mut inbox = Inbox::default();
    inbox.receive(input(3, 1, false, Some(2))).unwrap();
    inbox.receive(input(10, 2, true, None)).unwrap();
    let ready = inbox.ready(usize::MAX);
    assert_eq!(ready[0].command.sequence, 10);
    finish(&mut inbox, ready);
    assert!(!inbox.acks.contains(3));
    inbox.receive(input(2, 1, false, Some(1))).unwrap();
    inbox.receive(input(1, 1, true, None)).unwrap();
    let ready = inbox.ready(usize::MAX);
    assert_eq!(
        ready.iter().map(|i| i.command.sequence).collect::<Vec<_>>(),
        [1, 2, 3]
    );
    let commands = finish(&mut inbox, ready);
    assert!(matches!(commands[1].order, Order::Queue { .. }));
    assert!(inbox.acks.contains(1) && inbox.acks.contains(3));
    assert!(
        !inbox.acks.contains(4),
        "ACKing 10 must not imply ACKing gaps"
    );
}

#[test]
fn late_replacement_preserves_later_queued_orders_and_supersedes_missing_chain() {
    let mut inbox = Inbox::default();
    inbox.receive(input(4, 1, false, Some(3))).unwrap();
    inbox.receive(input(2, 1, false, Some(1))).unwrap();
    inbox.receive(input(3, 1, true, None)).unwrap();
    let ready = inbox.ready(usize::MAX);
    assert_eq!(
        ready.iter().map(|i| i.command.sequence).collect::<Vec<_>>(),
        [3, 4]
    );
    assert!(inbox.acks.contains(2));
    finish(&mut inbox, ready);
    inbox.receive(input(1, 1, true, None)).unwrap();
    assert!(inbox.ready(usize::MAX).is_empty());
    assert!(inbox.acks.contains(1));
}

#[test]
fn duplicate_production_executes_once_and_retries_keep_the_same_id() {
    let mut inbox = Inbox::default();
    let mut production = input(1, 1, true, None);
    production.command.order = Order::Train {
        entity: EntityId(1),
        unit_type: UnitTypeId(2),
    };
    inbox.receive(production.clone()).unwrap();
    inbox.receive(production.clone()).unwrap();
    let ready = inbox.ready(usize::MAX);
    assert_eq!(ready.len(), 1);
    finish(&mut inbox, ready);
    inbox.receive(production.clone()).unwrap();
    assert!(inbox.ready(usize::MAX).is_empty());
    let mut outbox = Outbox::default();
    outbox.push(production.command).unwrap();
    let first = outbox.due().unwrap();
    assert!(outbox.due().is_none());
    outbox.pending.get_mut(&1).unwrap().1 = Some(Instant::now() - RETRY);
    assert_eq!(outbox.due().unwrap(), first);
    outbox.accept(&inbox.acks).unwrap();
    assert!(outbox.due().is_none());
}

#[test]
fn replacement_cancels_retries_but_independent_actions_still_send() {
    let mut outbox = Outbox::default();
    outbox.push(input(1, 1, true, None).command).unwrap();
    let mut queued = input(2, 1, false, Some(1)).command;
    queued.order = Order::Queue {
        entity: EntityId(1),
        order: UnitOrder::Move {
            target: Position { x: 200, y: 100 },
        },
    };
    outbox.push(queued).unwrap();
    assert_eq!(outbox.pending[&2].0.after, Some(1));
    outbox.push(input(3, 1, true, None).command).unwrap();
    assert_eq!(outbox.pending.keys().copied().collect::<Vec<_>>(), [3]);
}

#[test]
fn retirement_bounds_history_without_blocking_an_outstanding_older_command() {
    let mut outbox = Outbox::default();
    outbox.push(input(1, 1, true, None).command).unwrap();
    outbox.push(input(3, 2, true, None).command).unwrap();
    let mut acks = Acknowledgements::default();
    acks.insert(3);
    outbox.accept(&acks).unwrap();
    assert_eq!(outbox.retired(), 0, "command 1 is still valid and pending");
    acks.insert(1);
    outbox.accept(&acks).unwrap();
    assert_eq!(outbox.retired(), 3);
    let mut inbox = Inbox::default();
    inbox.acks.insert(3);
    inbox.retire(3);
    assert_eq!(inbox.acks.0, [[1, 3]]);
    inbox.receive(input(2, 1, true, None)).unwrap();
    assert!(inbox.ready(usize::MAX).is_empty());
}
