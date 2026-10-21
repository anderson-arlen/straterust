use super::*;
fn initial() -> PlayerUpdate {
    let package = crate::content::Package::load(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/lan-demo"),
    )
    .unwrap();
    let world = package.world(42).unwrap();
    ServerSession::new(world, 42, vec![PlayerId(0), PlayerId(1)])
        .unwrap()
        .update(PlayerId(0), &[])
        .unwrap()
}
fn at(initial: &PlayerUpdate, sequence: u64) -> PlayerUpdate {
    let mut update = initial.clone();
    update.sequence = sequence;
    update.view.tick = Tick(sequence);
    update
}

#[test]
fn cumulative_ack_preserves_newer_changes_and_skipped_snapshots() {
    let initial = initial();
    let mut sender = Sender::new(initial.clone());
    let mut receiver = Receiver::new(&initial);
    let mut first = at(&initial, 1);
    first.view.resources[0].amount -= 10;
    sender.observe(first.clone()).unwrap();
    let packet1 = sender.packet().unwrap();
    assert_eq!(packet1.resources.len(), 1);
    let mut second = first.clone();
    second.sequence = 2;
    second.view.tick = Tick(2);
    second.view.resources[0].amount -= 10;
    second.view.fog[0] = 2;
    sender.observe(second.clone()).unwrap();
    let packet2 = sender.packet().unwrap();
    receiver.apply(packet1.clone()).unwrap();
    sender.acknowledge(1).unwrap();
    let mut third = second.clone();
    third.sequence = 3;
    third.view.tick = Tick(3);
    sender.observe(third.clone()).unwrap();
    let packet3 = sender.packet().unwrap();
    assert_eq!(packet3.base, 1);
    assert_eq!(packet3.resources.len(), 1);
    let applied = receiver.apply(packet3.clone()).unwrap().unwrap();
    assert_eq!(applied.view, third.view);
    assert!(receiver.apply(packet2).unwrap().is_none());
    assert!(receiver.apply(packet1).unwrap().is_none());
    sender.acknowledge(3).unwrap();
    sender.acknowledge(1).unwrap();
    let mut fourth = third;
    fourth.sequence = 4;
    fourth.view.tick = Tick(4);
    sender.observe(fourth).unwrap();
    let quiet = sender.packet().unwrap();
    assert_eq!(quiet.base, 3);
    assert!(quiet.resources.is_empty());
    assert!(quiet.entities.is_empty());
    assert!(quiet.header.is_none());
    assert!(quiet.fog.cells.is_empty());
    assert!(sender.acknowledge(5).is_err());
}

#[test]
fn removals_redisclosure_and_events_survive_ack_delay_without_replaying() {
    let initial = initial();
    let mut sender = Sender::new(initial.clone());
    let mut receiver = Receiver::new(&initial);
    let mut first = at(&initial, 1);
    first.view.resources.clear();
    first.view.entities.clear();
    first.view.weapon_feedback.push(WeaponFeedback {
        weapon: UnitTypeId(1),
        position: Position { x: 32, y: 32 },
        targets_air: false,
        impact: true,
    });
    sender.observe(first.clone()).unwrap();
    let packet1 = sender.packet().unwrap();
    assert!(packet1.resources.iter().all(|(_, v)| v.is_none()));
    let seen = receiver.apply(packet1).unwrap().unwrap();
    assert!(seen.view.resources.is_empty());
    assert_eq!(seen.view.weapon_feedback.len(), 1);
    let mut second = first.clone();
    second.view.weapon_feedback.clear();
    second.sequence = 2;
    second.view.tick = Tick(2);
    sender.observe(second).unwrap();
    let packet2 = sender.packet().unwrap();
    assert_eq!(packet2.feedback.len(), 1);
    assert!(
        receiver
            .apply(packet2)
            .unwrap()
            .unwrap()
            .view
            .weapon_feedback
            .is_empty()
    );
    sender.acknowledge(2).unwrap();
    let third = at(&initial, 3);
    sender.observe(third.clone()).unwrap();
    let packet3 = sender.packet().unwrap();
    assert!(!packet3.resources.is_empty());
    assert!(!packet3.entities.is_empty());
    assert_eq!(receiver.apply(packet3).unwrap().unwrap().view, third.view);
}

#[test]
fn rejects_future_baselines_and_invalid_grid_cells() {
    let initial = initial();
    let mut sender = Sender::new(initial.clone());
    sender.observe(at(&initial, 1)).unwrap();
    let packet = sender.packet().unwrap();
    let mut invalid = packet.clone();
    invalid.base = 2;
    assert!(Receiver::new(&initial).apply(invalid).is_err());
    let mut invalid = packet;
    invalid.fog.cells.push((usize::MAX, 1));
    assert!(Receiver::new(&initial).apply(invalid).is_err());
}

#[test]
fn wire_patches_keep_dirty_fields_until_ack_and_clear_optional_fields() {
    let initial = initial();
    let mut sender = Sender::new(initial.clone());
    let mut receiver = Receiver::new(&initial);
    let mut first = at(&initial, 1);
    let ViewedEntity::Owned(entity) = &mut first.view.entities[0] else {
        panic!("owned unit")
    };
    let id = entity.id;
    entity.target = Some(Position { x: 400, y: 400 });
    sender.observe(first.clone()).unwrap();
    let packet = sender.packet().unwrap();
    let bytes = ron::ser::to_string(&packet).unwrap();
    assert!(bytes.contains("target:Some(Some("));
    assert!(!bytes.contains("path_geometry"));
    let packet = ron::de::from_str(&bytes).unwrap();
    assert_eq!(receiver.apply(packet).unwrap().unwrap().view, first.view);
    let mut second = at(&first, 2);
    let ViewedEntity::Owned(entity) = &mut second.view.entities[0] else {
        unreachable!()
    };
    entity.position.x += 1;
    sender.observe(second.clone()).unwrap();
    let packet = sender.packet().unwrap();
    assert_eq!(packet.entities[0].0, id);
    let wire = ron::ser::to_string(&packet).unwrap();
    assert!(wire.contains("target:Some(Some(")); // not acknowledged yet
    sender.acknowledge(1).unwrap();
    let mut third = at(&second, 3);
    let ViewedEntity::Owned(entity) = &mut third.view.entities[0] else {
        unreachable!()
    };
    entity.target = None;
    sender.observe(third.clone()).unwrap();
    let wire = ron::ser::to_string(&sender.packet().unwrap()).unwrap();
    assert!(wire.contains("target:Some(None)"));
    assert!(!wire.contains("hp:"));
    let applied = receiver
        .apply(ron::de::from_str(&wire).unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(applied.view, third.view);
}

#[test]
fn a_fresh_frame_does_not_replay_cosmetics_from_a_stalled_delivery() {
    let initial = initial();
    let mut sender = Sender::new(initial.clone());
    let mut receiver = Receiver::new(&initial);
    let mut old = at(&initial, 1);
    old.time = Some(super::super::clock::FrameTime {
        captured_ms: 100,
        tick_ms: 40,
    });
    old.view.weapon_feedback.push(WeaponFeedback {
        weapon: UnitTypeId(1),
        position: Position { x: 32, y: 32 },
        targets_air: false,
        impact: true,
    });
    old.view.removed.push(EntityId(99));
    old.outcomes.push(CommandOutcome {
        command: Command {
            tick: Tick(0),
            player: PlayerId(0),
            sequence: 1,
            order: Order::Stop {
                entity: EntityId(1),
            },
        },
        rejection: None,
    });
    sender.observe(old).unwrap();
    let stale = sender.packet().unwrap();
    let mut fresh = at(&initial, 2);
    fresh.time = Some(super::super::clock::FrameTime {
        captured_ms: 1100,
        tick_ms: 40,
    });
    sender.observe(fresh).unwrap();
    let mut packet = sender.packet().unwrap();
    packet.expire_cosmetics(Some(1100.), 250);
    let seen = receiver.apply(packet).unwrap().unwrap();
    assert!(seen.view.weapon_feedback.is_empty());
    assert!(seen.view.removed.is_empty());
    assert_eq!(seen.outcomes.len(), 1);
    assert!(receiver.apply(stale).unwrap().is_none());
}

#[test]
fn decoded_but_unaccepted_feedback_is_redelivered_once_in_a_fresh_snapshot() {
    let initial = initial();
    let mut sender = Sender::new(initial.clone());
    let mut receiver = Receiver::new(&initial);
    let mut first = at(&initial, 1);
    first.outcomes.push(CommandOutcome {
        command: Command {
            tick: Tick(0),
            player: PlayerId(0),
            sequence: 1,
            order: Order::Stop {
                entity: EntityId(1),
            },
        },
        rejection: Some(Rejection::InvalidTarget),
    });
    sender.observe(first).unwrap();
    // Decoded on the I/O thread, but too old when the window gets to it.
    let old = receiver
        .apply_for(sender.packet().unwrap(), 0)
        .unwrap()
        .unwrap();
    sender.observe(at(&initial, 2)).unwrap();
    let fresh = receiver
        .apply_for(sender.packet().unwrap(), 0)
        .unwrap()
        .unwrap();
    let mut pending = Some(old);
    super::super::retain_latest(&mut pending, fresh);
    let mut accepted = pending.unwrap();
    accepted.retain_feedback_after(0);
    assert_eq!(accepted.outcomes.len(), 1);
    // Another packet was already decoded before that acceptance reached I/O.
    sender.observe(at(&initial, 3)).unwrap();
    let mut next = receiver
        .apply_for(sender.packet().unwrap(), 0)
        .unwrap()
        .unwrap();
    next.retain_feedback_after(2);
    assert!(next.outcomes.is_empty());
}
