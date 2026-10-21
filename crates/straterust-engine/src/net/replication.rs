//! Per-peer changes newer than the last snapshot the window has applied.
use super::*;
use crate::session::MatchResult;
use crate::sim::*;
use std::collections::{BTreeSet, VecDeque};
mod units;
pub use units::UnitChange;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Delta {
    pub time: Option<super::clock::FrameTime>,
    pub sequence: u64,
    pub base: u64,
    pub command_acks: super::commands::Acknowledgements,
    pub tick: Tick,
    pub player: PlayerId,
    header: Option<Header>,
    pub entities: Vec<(EntityId, Option<UnitChange>)>,
    pub resources: Vec<(ResourceId, Option<ResourceNode>)>,
    fog: GridDelta,
    terrain_fog: GridDelta,
    creep: GridDelta,
    feedback: Vec<(u64, Feedback)>,
    pub(super) result: Option<MatchResult>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Header {
    home: Option<Position>,
    creation: Option<Vec<UnitTypeId>>,
    remembered_structures: Vec<RememberedStructure>,
    economy: PlayerState,
    active_resources: BTreeSet<ResourceId>,
    scans: Vec<Scan>,
    ability_fields: Vec<AbilityField>,
    strikes: Vec<StrikeAppearance>,
    winner: Option<PlayerId>,
    defeated: Vec<PlayerId>,
    mission: Option<MissionView>,
}
impl Header {
    fn of(view: &PlayerView) -> Self {
        Self {
            home: view.home,
            creation: view.creation.clone(),
            remembered_structures: view.remembered_structures.clone(),
            economy: view.economy.clone(),
            active_resources: view.active_resources.clone(),
            scans: view.scans.clone(),
            ability_fields: view.ability_fields.clone(),
            strikes: view.strikes.clone(),
            winner: view.winner,
            defeated: view.defeated.clone(),
            mission: view.mission.clone(),
        }
    }
    fn apply(self, view: &mut PlayerView) {
        view.home = self.home;
        view.creation = self.creation;
        view.remembered_structures = self.remembered_structures;
        view.economy = self.economy;
        view.active_resources = self.active_resources;
        view.scans = self.scans;
        view.ability_fields = self.ability_fields;
        view.strikes = self.strikes;
        view.winner = self.winner;
        view.defeated = self.defeated;
        view.mission = self.mission;
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Feedback {
    captured_ms: Option<u64>,
    outcomes: Vec<CommandOutcome>,
    removed: Vec<EntityId>,
    shots: Vec<ContainerShot>,
    weapons: Vec<WeaponFeedback>,
    mission: Vec<MissionEvent>,
}
impl Feedback {
    fn take(update: &mut PlayerUpdate) -> Self {
        Self {
            captured_ms: update.time.map(|t| t.captured_ms),
            outcomes: std::mem::take(&mut update.outcomes),
            removed: std::mem::take(&mut update.view.removed),
            shots: std::mem::take(&mut update.view.shots),
            weapons: std::mem::take(&mut update.view.weapon_feedback),
            mission: update
                .view
                .mission
                .as_mut()
                .map(|m| std::mem::take(&mut m.events))
                .unwrap_or_default(),
        }
    }
    fn len(&self) -> usize {
        self.outcomes.len()
            + self.removed.len()
            + self.shots.len()
            + self.weapons.len()
            + self.mission.len()
    }
    fn append(self, update: &mut PlayerUpdate) {
        update.outcomes.extend(self.outcomes);
        update.view.removed.extend(self.removed);
        update.view.shots.extend(self.shots);
        update.view.weapon_feedback.extend(self.weapons);
        if let Some(mission) = &mut update.view.mission {
            mission.events.extend(self.mission);
        }
    }
}

struct Item<T> {
    changed: u64,
    value: Option<T>,
}
fn refresh<K: Ord + Copy, T: PartialEq + Clone>(
    items: &mut BTreeMap<K, Item<T>>,
    values: impl Iterator<Item = (K, T)>,
    sequence: u64,
) {
    let mut present = BTreeSet::new();
    for (id, value) in values {
        present.insert(id);
        if items
            .get(&id)
            .is_none_or(|old| old.value.as_ref() != Some(&value))
        {
            items.insert(
                id,
                Item {
                    changed: sequence,
                    value: Some(value),
                },
            );
        }
    }
    for (id, item) in items {
        if !present.contains(id) && item.value.is_some() {
            item.value = None;
            item.changed = sequence;
        }
    }
}
fn changes<K: Copy, T: Clone>(items: &BTreeMap<K, Item<T>>, ack: u64) -> Vec<(K, Option<T>)> {
    items
        .iter()
        .filter(|(_, i)| i.changed > ack)
        .map(|(&id, i)| (id, i.value.clone()))
        .collect()
}
fn entity_id(entity: &ViewedEntity) -> EntityId {
    match entity {
        ViewedEntity::Owned(e) => e.id,
        ViewedEntity::Visible(e) => e.id,
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct GridDelta {
    reset: Option<Vec<u8>>,
    cells: Vec<(usize, u8)>,
}
struct Grid {
    values: Vec<u8>,
    changed: Vec<u64>,
    resized: u64,
}
impl Grid {
    fn new(values: Vec<u8>, sequence: u64) -> Self {
        Self {
            changed: vec![sequence; values.len()],
            values,
            resized: sequence,
        }
    }
    fn refresh(&mut self, values: &[u8], sequence: u64) {
        if values.len() != self.values.len() {
            *self = Self::new(values.to_vec(), sequence);
        } else {
            for (i, &value) in values.iter().enumerate() {
                if self.values[i] != value {
                    self.values[i] = value;
                    self.changed[i] = sequence;
                }
            }
        }
    }
    fn delta(&self, ack: u64) -> GridDelta {
        if self.resized > ack {
            GridDelta {
                reset: Some(self.values.clone()),
                cells: vec![],
            }
        } else {
            GridDelta {
                reset: None,
                cells: self
                    .changed
                    .iter()
                    .enumerate()
                    .filter(|(_, s)| **s > ack)
                    .map(|(i, _)| (i, self.values[i]))
                    .collect(),
            }
        }
    }
}
impl GridDelta {
    fn apply(self, values: &mut Vec<u8>) -> Result<()> {
        if let Some(reset) = self.reset {
            *values = reset;
        }
        for (index, value) in self.cells {
            *values.get_mut(index).context("invalid fog delta index")? = value;
        }
        Ok(())
    }
}

impl Delta {
    pub(super) fn expire_cosmetics(&mut self, server_ms: Option<f64>, limit: u64) {
        for (_, feedback) in &mut self.feedback {
            if let (Some(now), Some(at)) = (server_ms, feedback.captured_ms)
                && now - at as f64 > limit as f64
            {
                feedback.shots.clear();
                feedback.weapons.clear();
                feedback.removed.clear();
            }
        }
    }
}

pub struct Sender {
    time: Option<super::clock::FrameTime>,
    sequence: u64,
    sent: u64,
    ack: u64,
    command_acks: super::commands::Acknowledgements,
    player: PlayerId,
    tick: Tick,
    header: Header,
    header_changed: u64,
    entities: units::Units,
    resources: BTreeMap<ResourceId, Item<ResourceNode>>,
    fog: Grid,
    terrain_fog: Grid,
    creep: Grid,
    feedback: VecDeque<(u64, Feedback)>,
    result: Option<MatchResult>,
}
impl Sender {
    pub fn new(mut initial: PlayerUpdate) -> Self {
        let sequence = initial.sequence;
        Feedback::take(&mut initial);
        let view = initial.view;
        let mut entities = units::Units::default();
        let mut resources = BTreeMap::new();
        entities.refresh(view.entities.clone(), sequence);
        refresh(
            &mut resources,
            view.resources.iter().cloned().map(|r| (r.id, r)),
            sequence,
        );
        Self {
            time: initial.time,
            sequence,
            sent: sequence,
            ack: sequence,
            command_acks: initial.command_acks,
            player: view.player,
            tick: view.tick,
            header: Header::of(&view),
            header_changed: sequence,
            entities,
            resources,
            fog: Grid::new(view.fog, sequence),
            terrain_fog: Grid::new(view.terrain_fog, sequence),
            creep: Grid::new(view.creep, sequence),
            feedback: VecDeque::new(),
            result: initial.result,
        }
    }
    pub fn observe(&mut self, mut update: PlayerUpdate) -> Result<()> {
        ensure!(
            update.sequence > self.sequence && update.view.player == self.player,
            "invalid replication update"
        );
        self.time = update.time;
        self.command_acks = update.command_acks.clone();
        let sequence = update.sequence;
        let feedback = Feedback::take(&mut update);
        if feedback.len() > 0 {
            self.feedback.push_back((sequence, feedback));
        }
        // Same bound used by the local latest-view mailbox for transient feedback.
        while self.feedback.iter().map(|(_, f)| f.len()).sum::<usize>() > 4096 {
            self.feedback.pop_front();
        }
        let view = update.view;
        let header = Header::of(&view);
        if header != self.header {
            self.header = header;
            self.header_changed = sequence;
        }
        self.entities.refresh(view.entities, sequence);
        refresh(
            &mut self.resources,
            view.resources.into_iter().map(|r| (r.id, r)),
            sequence,
        );
        self.fog.refresh(&view.fog, sequence);
        self.terrain_fog.refresh(&view.terrain_fog, sequence);
        self.creep.refresh(&view.creep, sequence);
        self.tick = view.tick;
        self.sequence = sequence;
        self.result = update.result;
        Ok(())
    }
    pub fn acknowledge(&mut self, sequence: u64) -> Result<()> {
        ensure!(
            sequence <= self.sent,
            "acknowledgement exceeds sent snapshot"
        );
        if sequence <= self.ack {
            return Ok(());
        }
        self.ack = sequence;
        self.entities.acknowledge(sequence);
        self.resources
            .retain(|_, i| i.value.is_some() || i.changed > sequence);
        self.feedback.retain(|(s, _)| *s > sequence);
        Ok(())
    }
    pub fn pending(&self) -> bool {
        self.sequence > self.sent
    }
    pub(super) fn progress(&self) -> (u64, u64, u64) {
        (self.sequence, self.sent, self.ack)
    }
    pub(super) fn unacknowledged(&self) -> bool {
        self.sequence > self.ack
    }
    pub(super) fn repeat(&mut self) -> Option<Delta> {
        if !self.unacknowledged() {
            return None;
        }
        self.sent = self.sequence.saturating_sub(1);
        self.packet()
    }
    pub fn packet(&mut self) -> Option<Delta> {
        if self.sequence <= self.sent {
            return None;
        }
        self.sent = self.sequence;
        Some(Delta {
            time: self.time,
            sequence: self.sequence,
            base: self.ack,
            command_acks: self.command_acks.clone(),
            tick: self.tick,
            player: self.player,
            header: (self.header_changed > self.ack).then(|| self.header.clone()),
            entities: self.entities.changes(self.ack),
            resources: changes(&self.resources, self.ack),
            fog: self.fog.delta(self.ack),
            terrain_fog: self.terrain_fog.delta(self.ack),
            creep: self.creep.delta(self.ack),
            feedback: self.feedback.iter().cloned().collect(),
            result: self.result.clone(),
        })
    }
}

pub struct Receiver {
    view: PlayerView,
    sequence: u64,
}
impl Receiver {
    pub fn new(initial: &PlayerUpdate) -> Self {
        Self {
            view: initial.view.clone(),
            sequence: initial.sequence,
        }
    }
    pub fn apply(&mut self, delta: Delta) -> Result<Option<PlayerUpdate>> {
        self.apply_for(delta, self.sequence)
    }
    pub(super) fn apply_for(
        &mut self,
        delta: Delta,
        accepted: u64,
    ) -> Result<Option<PlayerUpdate>> {
        delta.command_acks.validate()?;
        ensure!(
            delta.player == self.view.player,
            "player update ownership mismatch"
        );
        if delta.sequence <= self.sequence {
            return Ok(None);
        }
        ensure!(
            delta.base <= self.sequence && delta.tick > self.view.tick,
            "invalid snapshot baseline or tick"
        );
        if let Some(header) = delta.header {
            header.apply(&mut self.view);
        }
        let mut entities: BTreeMap<_, _> = std::mem::take(&mut self.view.entities)
            .into_iter()
            .map(|e| (entity_id(&e), e))
            .collect();
        for (id, value) in delta.entities {
            if let Some(value) = value {
                value.apply(id, &mut entities)?;
            } else {
                entities.remove(&id);
            }
        }
        self.view.entities = entities.into_values().collect();
        let mut resources: BTreeMap<_, _> = std::mem::take(&mut self.view.resources)
            .into_iter()
            .map(|r| (r.id, r))
            .collect();
        for (id, value) in delta.resources {
            if let Some(value) = value {
                ensure!(id == value.id, "invalid resource delta ID");
                resources.insert(id, value);
            } else {
                resources.remove(&id);
            }
        }
        self.view.resources = resources.into_values().collect();
        delta.fog.apply(&mut self.view.fog)?;
        delta.terrain_fog.apply(&mut self.view.terrain_fog)?;
        delta.creep.apply(&mut self.view.creep)?;
        self.view.tick = delta.tick;
        let mut update = PlayerUpdate {
            time: delta.time,
            sequence: delta.sequence,
            command_acks: delta.command_acks,
            delivery: Some(Vec::new()),
            view: self.view.clone(),
            outcomes: vec![],
            result: delta.result,
        };
        Feedback::take(&mut update);
        for (sequence, feedback) in delta.feedback {
            ensure!(sequence <= delta.sequence, "future snapshot event");
            if sequence > accepted {
                update
                    .delivery
                    .as_mut()
                    .unwrap()
                    .push((sequence, feedback.clone()));
                feedback.append(&mut update);
            }
        }
        self.sequence = delta.sequence;
        Ok(Some(update))
    }
}

impl PlayerUpdate {
    /// The window can reject a decoded frame. Redelivery retains event IDs until
    /// acceptance, so a later fresh snapshot neither loses nor repeats feedback.
    pub fn retain_feedback_after(&mut self, accepted: u64) {
        if let Some(delivery) = self.delivery.take() {
            Feedback::take(self);
            for (sequence, feedback) in delivery {
                if sequence > accepted {
                    feedback.append(self);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
