//! Typed field patches. Each field keeps its own last-change sequence until ACK.
use super::*;
use std::collections::VecDeque;

macro_rules! patch {
    ($name:ident, $ty:ident, {$($field:ident: $value:ty),* $(,)?}) => {
        #[derive(Clone, Debug, Default, Serialize, Deserialize)]
        #[serde(default, deny_unknown_fields)]
        pub struct $name {
            $(#[serde(skip_serializing_if = "Option::is_none")]
            $field: Option<$value>,)*
        }
        impl $name {
            fn mark(old: &$ty, new: &$ty, fields: &mut BTreeMap<&'static str, u64>, sequence: u64) {
                let $ty { $($field: _),* } = new;
                $(if old.$field != new.$field { fields.insert(stringify!($field), sequence); })*
            }
            fn since(value: &$ty, fields: &BTreeMap<&'static str, u64>, ack: u64) -> Self {
                Self { $($field: fields.get(stringify!($field)).filter(|s| **s > ack).map(|_| Clone::clone(&value.$field)),)* }
            }
            fn apply(self, value: &mut $ty) {
                $(if let Some(field) = self.$field { value.$field = field; })*
            }
        }
    };
}

patch!(OwnedPatch, Entity, {
    linked_to: Option<EntityId>,
    wander: Option<WanderState>,
    mode_transition: Option<ModeTransition>,
    ability_auras: Vec<AbilityAura>,
    last_cast: Option<CastAppearance>,
    illusion_remaining: Option<u32>,
    lifetime_remaining: Option<u32>,
    carried_by: Option<EntityId>,
    doodad_enabled: Option<bool>,
    mine_count: u8,
    mine_state: Option<MineState>,
    garrisoned_in: Option<EntityId>,
    id: EntityId,
    owner: PlayerId,
    unit_type: UnitTypeId,
    position: Position,
    target: Option<Position>,
    hp: u32,
    shields: u32,
    offspring_remaining: u32,
    damage_fraction: u8,
    invincible: bool,
    strikes: Vec<PendingStrike>,
    gathering_inside: bool,
    parent: Option<EntityId>,
    energy: u32,
    cloaked: bool,
    last_attack_air: bool,
    last_attack_target: Option<EntityId>,
    last_attack_position: Option<Position>,
    cloak_transition: u32,
    airborne: bool,
    flight_transition: u32,
    construction: Option<Construction>,
    production: VecDeque<ProductionJob>,
    research: Option<ResearchJob>,
    stim_remaining: u32,
    cargo: Option<ResourceAmount>,
    dropoff_target: Option<EntityId>,
    rally: Option<Position>,
    rally_resource: Option<ResourceId>,
    order: UnitOrder,
    queued_orders: VecDeque<UnitOrder>,
    auto_attack_target: Option<EntityId>,
    retaliation_position: Option<Position>,
    path: VecDeque<Position>,
    path_retry: Tick,
    path_geometry: [u8; 32],
    route_wait: Option<RouteWait>,
    cooldown: u32,
    unload_remaining: u32,
    harvest_progress: u32,
    harvest_waiting_since: Option<Tick>,
    harvest_spot: Option<Position>,
    gather_origin: Option<Position>,
    motion_fraction: [i32; 2],
    motion_speed: u32,
    motion_phase: u32,
    repair_progress: u64,
    repair_credit: Vec<u64>,
    patrol_origin: Option<Position>,
    patrol_returning: bool,
});

patch!(VisiblePatch, PublicEntity, {
    carried_by: Option<EntityId>,
    id: EntityId,
    owner: PlayerId,
    unit_type: UnitTypeId,
    position: Position,
    hp: u32,
    shields: u32,
    invincible: bool,
    cloaked: bool,
    airborne: bool,
    flight_transition: u32,
    doodad_enabled: Option<bool>,
    parent: Option<EntityId>,
    construction: Option<[u32; 2]>,
    working: bool,
    cooldown: u32,
    targets_air: bool,
    shot_position: Option<Position>,
    appearance: Appearance,
    mine: Option<(MinePhase, u32)>,
    motion: Option<PublicMotion>,
});

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum UnitChange {
    Full(ViewedEntity),
    Owned(Box<OwnedPatch>),
    Visible(Box<VisiblePatch>),
}
struct Record {
    value: Option<ViewedEntity>,
    changed: u64,
    reset: u64,
    fields: BTreeMap<&'static str, u64>,
}
#[derive(Default)]
pub(super) struct Units(BTreeMap<EntityId, Record>);
impl Units {
    pub fn refresh(&mut self, entities: Vec<ViewedEntity>, sequence: u64) {
        let mut present = BTreeSet::new();
        for value in entities {
            let id = entity_id(&value);
            present.insert(id);
            let record = self.0.entry(id).or_insert_with(|| Record {
                value: None,
                changed: sequence,
                reset: sequence,
                fields: BTreeMap::new(),
            });
            match (&record.value, &value) {
                (Some(ViewedEntity::Owned(old)), ViewedEntity::Owned(new)) => {
                    OwnedPatch::mark(old, new, &mut record.fields, sequence)
                }
                (Some(ViewedEntity::Visible(old)), ViewedEntity::Visible(new)) => {
                    VisiblePatch::mark(old, new, &mut record.fields, sequence)
                }
                _ => {
                    record.reset = sequence;
                    record.fields.clear();
                }
            }
            if record.value.as_ref() != Some(&value) {
                record.changed = sequence;
            }
            record.value = Some(value);
        }
        for (id, record) in &mut self.0 {
            if !present.contains(id) && record.value.is_some() {
                record.value = None;
                record.changed = sequence;
                record.reset = sequence;
                record.fields.clear();
            }
        }
    }
    pub fn changes(&self, ack: u64) -> Vec<(EntityId, Option<UnitChange>)> {
        self.0
            .iter()
            .filter(|(_, r)| r.changed > ack)
            .map(|(&id, r)| {
                let value = r.value.as_ref().map(|value| {
                    if r.reset > ack {
                        UnitChange::Full(value.clone())
                    } else {
                        match value {
                            ViewedEntity::Owned(value) => UnitChange::Owned(Box::new(
                                OwnedPatch::since(value, &r.fields, ack),
                            )),
                            ViewedEntity::Visible(value) => UnitChange::Visible(Box::new(
                                VisiblePatch::since(value, &r.fields, ack),
                            )),
                        }
                    }
                });
                (id, value)
            })
            .collect()
    }
    pub fn acknowledge(&mut self, ack: u64) {
        self.0.retain(|_, r| r.value.is_some() || r.changed > ack);
        for record in self.0.values_mut() {
            record.fields.retain(|_, s| *s > ack);
        }
    }
}
impl UnitChange {
    pub(super) fn apply(
        self,
        id: EntityId,
        entities: &mut BTreeMap<EntityId, ViewedEntity>,
    ) -> Result<()> {
        match self {
            Self::Full(value) => {
                ensure!(id == entity_id(&value), "invalid entity delta ID");
                entities.insert(id, value);
            }
            Self::Owned(patch) => {
                let Some(ViewedEntity::Owned(value)) = entities.get_mut(&id) else {
                    bail!("owned patch lacks baseline")
                };
                patch.apply(value);
                ensure!(id == value.id, "invalid owned patch ID");
            }
            Self::Visible(patch) => {
                let Some(ViewedEntity::Visible(value)) = entities.get_mut(&id) else {
                    bail!("visible patch lacks baseline")
                };
                patch.apply(value);
                ensure!(id == value.id, "invalid visible patch ID");
            }
        }
        Ok(())
    }
}
