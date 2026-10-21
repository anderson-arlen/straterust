//! Full views replace pending full views; transient feedback survives replacement.
use crate::session::PlayerUpdate;

const MAX_EVENTS: usize = 4096;

pub fn retain_latest(pending: &mut Option<PlayerUpdate>, mut update: PlayerUpdate) {
    if pending
        .as_ref()
        .is_some_and(|old| old.sequence >= update.sequence)
    {
        return;
    }
    // Network deltas already carry all feedback since the accepted ACK. Merging
    // two such deliveries would duplicate the same unacknowledged events.
    if update.delivery.is_some() {
        *pending = Some(update);
        return;
    }
    if let Some(mut old) = pending.take() {
        if let (Some(new_time), Some(old_time)) = (update.time, old.time)
            && new_time.captured_ms.saturating_sub(old_time.captured_ms)
                > super::clock::MAX_FRAME_AGE_MS
        {
            old.view.shots.clear();
            old.view.weapon_feedback.clear();
            old.view.removed.clear();
        }
        append_recent(&mut update.outcomes, old.outcomes);
        append_recent(&mut update.view.removed, old.view.removed);
        update.view.removed.sort_unstable();
        update.view.removed.dedup();
        append_recent(&mut update.view.shots, old.view.shots);
        append_recent(&mut update.view.weapon_feedback, old.view.weapon_feedback);
        if let (Some(new), Some(old)) = (&mut update.view.mission, old.view.mission) {
            append_recent(&mut new.events, old.events);
        }
    }
    *pending = Some(update);
}

fn append_recent<T>(new: &mut Vec<T>, mut old: Vec<T>) {
    old.append(new);
    if old.len() > MAX_EVENTS {
        old.drain(..old.len() - MAX_EVENTS);
    }
    *new = old;
}
