//! The small playable economy/combat loop. All iteration and tie breaking use
//! stable IDs; presentation never writes these structures.
use super::*;
use crate::path::{Obstacle, find_path, find_path_near, segment_clear};

pub(in crate::sim) const MAX_ENTITIES: usize = 4096;
pub(super) const MAX_QUEUED_ORDERS: usize = 64;
const MAX_PRODUCTION: usize = 5;
pub(super) const PATH_RETRY_TICKS: u64 = 8;

/// Accumulate simultaneous damage by victim and source so surviving defenders
/// can react to actual attackers, including delayed and garrisoned shots.
#[derive(Default)]
pub(super) struct Damage {
    pub source_owners: BTreeMap<EntityId, PlayerId>,
    pub incoming: BTreeMap<EntityId, BTreeMap<EntityId, u64>>,
    pub hits: BTreeMap<EntityId, BTreeMap<EntityId, u64>>,
    pub shields: BTreeMap<EntityId, u64>,
    pub barriers: BTreeMap<(EntityId, AbilityId), u64>,
    pub weapon_feedback: Vec<(Vec<PlayerId>, WeaponFeedback)>,
}

pub(super) fn one() -> u32 {
    1
}
pub(super) fn bool_true() -> bool {
    true
}
pub(super) fn default_supply_limit() -> u32 {
    400
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceAmount {
    pub kind: String,
    pub amount: u32,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerStats {
    pub capacity: u32,
    pub harvest_amount: u32,
    pub harvest_ticks: u32,
    pub build_rate: u32,
    pub resource_kinds: Vec<String>,
    /// Radius around the ordered outdoor patch in which idle alternatives are sought.
    /// Zero disables automatic redistribution.
    #[serde(default = "default_idle_resource_radius")]
    pub idle_resource_radius: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HarvestProfile {
    pub kind: String,
    pub capacity: u8,
    pub inside: bool,
    pub amount: u32,
    pub ticks: u32,
    /// Distance between collision bounds for entering a resource or depot.
    /// Outdoor harvesting still uses exclusive contact points.
    #[serde(
        default = "default_entry_range",
        skip_serializing_if = "is_default_entry_range"
    )]
    pub entry_range: u32,
    /// Time spent delivering a full load after reaching a depot.
    #[serde(default, skip_serializing_if = "is_zero_ticks")]
    pub depot_ticks: u32,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub depot_inside: bool,
}

fn is_zero_ticks(ticks: &u32) -> bool {
    *ticks == 0
}
fn default_entry_range() -> u32 {
    1
}
fn is_default_entry_range(range: &u32) -> bool {
    *range == 1
}

fn default_idle_resource_radius() -> u32 {
    256
}
/// Native movement in 1/256 world units. A stride cycle supplies a speed for
/// each moving tick; an empty cycle accelerates toward the constant speed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Motion {
    /// Restrict travel to cardinal/45-degree segments for eight-facing artwork.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub eight_directions: bool,
    pub speed: u32,
    pub acceleration: u32,
    pub steps: Vec<u16>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Extraction {
    pub resource: String,
    pub harvest_ticks: u32,
    pub depleted_amount: u32,
}
/// Repair restores max_hp * rate_numerator / (build_ticks * rate_denominator)
/// per tick, costing cost / cost_divisor for a full health bar. Integer carry
/// preserves fractional work and prepaid resources without floating point.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepairRules {
    pub rate_numerator: u32,
    pub rate_denominator: u32,
    pub cost_divisor: u32,
    pub range: u32,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Weapon {
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub friendly_splash: bool,
    /// Missile speed in 1/256 world units per tick. Zero applies hits immediately.
    #[serde(default, skip_serializing_if = "projectiles::is_zero")]
    pub projectile_speed: u32,
    #[serde(default)]
    pub cooldown_jitter: Option<[i32; 2]>,
    #[serde(default)]
    pub targets_air: bool,
    /// Empty preserves the legacy ground/air policy; content may restrict naval targets.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub target_classes: Vec<MovementClass>,
    pub damage: u32,
    pub range: u32,
    pub cooldown: u32,
    #[serde(default)]
    pub damage_kind: DamageKind,
    #[serde(default)]
    pub splash: Option<[u32; 3]>,
    #[serde(default)]
    pub strikes: Vec<WeaponStrike>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WeaponStrike {
    pub delay: u32,
    pub forward: u32,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingStrike {
    pub remaining: u32,
    pub target: EntityId,
    pub aim: Position,
    pub forward: u32,
    #[serde(default)]
    pub air: bool,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum DamageKind {
    #[default]
    Normal,
    Explosive,
    Concussive,
    /// Basic damage is reduced by armor; this portion bypasses it.
    Split {
        piercing: u32,
        minimum_percent: u8,
    },
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnitSize {
    Small,
    Medium,
    #[default]
    Large,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ResourceId(pub u32);
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerState {
    pub resources: BTreeMap<String, u64>,
    pub completed_research: BTreeSet<ResearchId>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceNode {
    pub id: ResourceId,
    pub kind: String,
    pub position: Position,
    pub footprint: Footprint,
    pub amount: u32,
    pub requires_extractor: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Construction {
    pub worker: Option<EntityId>,
    pub remaining: u32,
    pub total: u32,
    /// None until the assigned worker first reaches the building. Thereafter
    /// this is its current work point or its next reposition destination.
    pub work_position: Option<Position>,
    /// Work pause remaining at the current point; travel does not consume it.
    pub work_ticks: u32,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductionJob {
    #[serde(default)]
    pub producer_type: Option<UnitTypeId>,
    pub unit_type: UnitTypeId,
    pub remaining: u32,
    pub total: u32,
    pub started: bool,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum UnitOrder {
    ReceiveAbility {
        provider: EntityId,
        ability: AbilityId,
    },
    Cast {
        ability: AbilityId,
        target: AbilityTarget,
    },
    /// Unpaid construction order; no foundation exists until the builder arrives.
    #[serde(alias = "PlaceAddon")]
    PlaceBuilding {
        unit_type: UnitTypeId,
        target: Position,
    },
    PlaceMine {
        target: Position,
    },
    Land {
        target: Position,
    },
    Load {
        target: EntityId,
    },
    /// A mobile transport rendezvous before continuing its previous orders.
    Pickup {
        target: EntityId,
    },
    /// Travel to the destination, then unload passengers as space becomes available.
    UnloadAt {
        target: Position,
    },
    #[default]
    Idle,
    Move {
        target: Position,
    },
    Attack {
        target: EntityId,
    },
    AttackMove {
        target: Position,
    },
    Hold,
    Patrol {
        target: Position,
    },
    Gather {
        resource: ResourceId,
    },
    Build {
        building: EntityId,
    },
    Repair {
        target: EntityId,
    },
}

impl Default for UnitType {
    fn default() -> Self {
        Self {
            builder_inside: false,
            repair_construction: false,
            builder_gathers_resource: false,
            neutral: false,
            energy_pool: None,
            idle_wander: None,
            mode: None,
            abilities: Vec::new(),
            cargo_size: 1,
            blocks_movement: true,
            phases_while_gathering: false,
            consumes_builder: false,
            attacks_ground: true,
            id: UnitTypeId(0),
            speed: 1,
            motion: None,
            footprint: Footprint::default(),
            movement_class: MovementClass::Ground,
            max_hp: 1,
            max_shields: 0,
            portable: false,
            transforms_on_production: false,
            production_form: None,
            destroyed_on_production_cancel: false,
            production_count: 1,
            production_capacity: 0,
            stored_weapon: None,
            offspring: None,
            provides_types: Vec::new(),
            shield_regeneration: 0,
            power_field: None,
            requires_power: false,
            autonomous_construction: false,
            armor: 0,
            size: UnitSize::default(),
            acquisition_range: None,
            vision_range: 0,
            revealer: false,
            regeneration: 0,
            extracts: None,
            addon_parent: None,
            creep_radius: None,
            requires_creep: false,
            scanner: None,
            cloak: None,
            concealment_field: None,
            detector_range: 0,
            flight: None,
            garrison: None,
            mine_layer: None,
            mine: None,
            triggers_mines: true,
            structure: false,
            placement_surface: crate::map::PlacementSurface::Land,
            placement: Footprint::default(),
            resource_clearance: 0,
            cost: Vec::new(),
            build_ticks: 1,
            supply_used: 0,
            supply_provided: 0,
            prerequisites: Vec::new(),
            builds: Vec::new(),
            repairs: Vec::new(),
            trains: Vec::new(),
            dropoff: Vec::new(),
            worker: None,
            harvest_bonus_percent: Vec::new(),
            harvest_profiles: Vec::new(),
            weapon: None,
            air_weapon: None,
        }
    }
}
impl Default for Rules {
    fn default() -> Self {
        Self {
            id: String::new(),
            prioritize_threats: false,
            tick_ms: 50,
            units: Vec::new(),
            starting_resources: Vec::new(),
            supply_limit: default_supply_limit(),
            victory: false,
            repair: None,
            research: Vec::new(),
        }
    }
}

fn valid_kind(kind: &str) -> bool {
    !kind.is_empty()
        && kind.len() <= 64
        && kind
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
}
pub(super) fn validate_amounts(amounts: &[ResourceAmount]) -> Result<()> {
    ensure!(amounts.len() <= 32, "too many resource kinds");
    let mut kinds = BTreeSet::new();
    for amount in amounts {
        ensure!(
            valid_kind(&amount.kind) && kinds.insert(&amount.kind),
            "invalid or duplicate resource kind"
        );
    }
    Ok(())
}
pub(super) fn validate_rts_rules(rules: &Rules) -> Result<()> {
    if let Some(repair) = &rules.repair {
        ensure!(
            (1..=1_000_000).contains(&repair.rate_numerator)
                && (1..=1_000_000).contains(&repair.rate_denominator)
                && (1..=1_000_000).contains(&repair.cost_divisor)
                && repair.range <= 32768,
            "invalid repair rate, cost divisor or range"
        );
    }
    ensure!(
        (1..=10000).contains(&rules.supply_limit),
        "invalid supply limit"
    );
    validate_amounts(&rules.starting_resources)?;
    for unit in &rules.units {
        ensure!(
            unit.max_shields <= 1_000_000 && (1..=2).contains(&unit.production_count),
            "invalid shields or production count"
        );
        ensure!(
            !unit.requires_power || unit.structure,
            "only structures require power"
        );
        ensure!(
            !unit.autonomous_construction || unit.structure,
            "autonomous construction requires a structure"
        );
        if let Some(power) = &unit.power_field {
            ensure!(
                (1..=256).contains(&power.cell_size)
                    && !power.rows.is_empty()
                    && power.rows.len() <= 16
                    && power.rows.iter().any(|row| *row != 0),
                "invalid power field"
            );
        }
        if let Some(config) = &unit.offspring {
            ensure!(
                rules
                    .units
                    .iter()
                    .any(|other| other.id == config.unit_type && !other.structure)
                    && (1..=1_000_000).contains(&config.interval)
                    && (1..=16).contains(&config.maximum)
                    && config.initial <= config.maximum,
                "invalid offspring rules"
            );
        }
        ensure!(
            unit.production_form
                .is_none_or(|id| unit.transforms_on_production
                    && rules.units.iter().any(|other| other.id == id)),
            "invalid production form"
        );
        if let Some(motion) = &unit.motion {
            ensure!(
                unit.speed > 0
                    && motion.speed > 0
                    && motion.speed <= 262144
                    && motion.acceleration <= 262144
                    && motion.steps.len() <= 256
                    && motion.steps.iter().all(|step| *step <= 1024),
                "invalid movement profile"
            );
        }
        if let Some(flight) = &unit.flight {
            ensure!(
                unit.structure
                    && (1..=1024).contains(&flight.speed)
                    && (1..=10000).contains(&flight.lift_ticks)
                    && (1..=10000).contains(&flight.land_ticks),
                "invalid flight rules"
            );
        }
        if let Some(cloak) = &unit.cloak {
            cloak.validate()?;
            ensure!(unit.scanner.is_none(), "overlapping energy abilities");
        }
        ensure!(unit.detector_range <= 32768, "invalid detector range");
        if let Some(scanner) = &unit.scanner {
            scanner.validate()?;
        }
        if let Some(extraction) = &unit.extracts {
            ensure!(
                unit.structure
                    && valid_kind(&extraction.resource)
                    && (1..=1_000_000).contains(&extraction.harvest_ticks)
                    && extraction.depleted_amount <= 1_000_000,
                "invalid extraction rules"
            );
        }
        ensure!(
            unit.creep_radius
                .is_none_or(|radius| radius.iter().all(|&value| value <= 1024)),
            "invalid creep radius"
        );
        ensure!(
            !unit.requires_creep || unit.structure,
            "only buildings require creep"
        );
        if let Some(parent) = unit.addon_parent {
            ensure!(
                unit.structure
                    && rules.units.iter().any(|other| other.id == parent
                        && other.structure
                        && other.builds.contains(&unit.id)),
                "invalid addon parent"
            );
        }
        ensure!(unit.vision_range <= 32768, "invalid vision range");
        if let Some(range) = unit.acquisition_range {
            ensure!(
                range <= 32768 && unit.weapon.is_some(),
                "invalid acquisition range"
            );
        }
        ensure!(
            (1..=1_000_000).contains(&unit.max_hp) && unit.armor <= 1_000_000,
            "invalid health or armor"
        );
        ensure!(
            (1..=1_000_000).contains(&unit.build_ticks),
            "invalid build time"
        );
        ensure!(
            unit.placement.width > 0 && unit.placement.height > 0,
            "invalid placement footprint"
        );
        ensure!(
            unit.resource_clearance <= 1024 && (unit.resource_clearance == 0 || unit.structure),
            "invalid resource clearance"
        );
        ensure!(
            unit.supply_used <= rules.supply_limit && unit.supply_provided <= rules.supply_limit,
            "invalid supply"
        );
        ensure!(
            !unit.structure || unit.speed == 0,
            "structures must be stationary"
        );
        validate_amounts(&unit.cost)?;
        validate_amounts(&unit.harvest_bonus_percent)?;
        ensure!(
            unit.harvest_bonus_percent
                .iter()
                .all(|a| unit.structure && a.amount <= 100),
            "resource bonuses require a facility and at most 100 percent"
        );
        for list in [
            &unit.provides_types,
            &unit.prerequisites,
            &unit.builds,
            &unit.trains,
            &unit.repairs,
        ] {
            let mut ids = BTreeSet::new();
            ensure!(list.len() <= rules.units.len(), "too many unit references");
            for id in list {
                ensure!(
                    ids.insert(id) && rules.units.iter().any(|other| other.id == *id),
                    "unknown or duplicate unit reference"
                );
            }
        }
        ensure!(
            unit.dropoff.len() <= 32 && unit.dropoff.iter().all(|kind| valid_kind(kind)),
            "invalid dropoff kinds"
        );
        if let Some(worker) = &unit.worker {
            ensure!(
                unit.harvest_profiles.len() <= worker.resource_kinds.len(),
                "too many harvest profiles"
            );
            let mut profile_kinds = BTreeSet::new();
            for profile in &unit.harvest_profiles {
                ensure!(
                    worker.resource_kinds.contains(&profile.kind)
                        && profile_kinds.insert(&profile.kind)
                        && (1..=16).contains(&profile.capacity)
                        && (1..=1_000_000).contains(&profile.ticks)
                        && profile.depot_ticks <= 1_000_000
                        && (1..=32768).contains(&profile.entry_range)
                        && (!profile.depot_inside || profile.depot_ticks > 0)
                        && profile.amount > 0
                        && profile.amount <= worker.capacity,
                    "invalid resource harvest profile"
                );
            }
            ensure!(
                (1..=1_000_000).contains(&worker.capacity)
                    && worker.harvest_amount > 0
                    && worker.harvest_amount <= worker.capacity,
                "invalid harvest capacity"
            );
            ensure!(
                (1..=1_000_000).contains(&worker.harvest_ticks)
                    && (1..=1_000_000).contains(&worker.build_rate),
                "invalid worker timing"
            );
            ensure!(
                worker.resource_kinds.len() <= 32
                    && worker.resource_kinds.iter().all(|kind| valid_kind(kind)),
                "invalid harvest kinds"
            );
            ensure!(
                worker.idle_resource_radius <= 32768,
                "invalid idle resource radius"
            );
        }
        ensure!(
            unit.worker.is_some() || unit.harvest_profiles.is_empty(),
            "harvest profiles require a worker"
        );
        ensure!(
            (!unit.builder_inside
                || (unit.structure && !unit.autonomous_construction && !unit.consumes_builder))
                && (!unit.repair_construction || (unit.structure && rules.repair.is_some())),
            "invalid construction service options"
        );
        ensure!(
            !unit.builder_gathers_resource
                || (unit.structure
                    && unit.extracts.is_some()
                    && !unit.autonomous_construction
                    && !unit.consumes_builder),
            "builder gathering requires an assisted extractor"
        );
        ensure!(
            unit.builds.is_empty()
                || unit.worker.is_some()
                || unit.builds.iter().all(|id| rules
                    .units
                    .iter()
                    .any(|other| other.id == *id && other.addon_parent == Some(unit.id))),
            "builders require worker stats"
        );
        ensure!(
            unit.repairs.is_empty() || (unit.speed > 0 && rules.repair.is_some()),
            "repairers require movement and repair rules"
        );
        ensure!(
            unit.builds.iter().all(|id| rules
                .units
                .iter()
                .any(|other| other.id == *id && other.structure)),
            "build targets must be structures"
        );
        ensure!(
            unit.trains.iter().all(|id| rules.units.iter().any(
                |other| other.id == *id && (!other.structure || unit.transforms_on_production)
            )),
            "train targets must be mobile units"
        );
        ensure!(
            unit.air_weapon.is_none()
                || (unit.weapon.is_some()
                    && unit.air_weapon.as_ref().is_some_and(|w| w.targets_air)),
            "invalid air weapon profile"
        );
        for weapon in [&unit.weapon, &unit.air_weapon].into_iter().flatten() {
            if let Some([low, high]) = weapon.cooldown_jitter {
                ensure!(
                    low <= high
                        && low >= -32
                        && high <= 32
                        && i64::from(weapon.cooldown) + i64::from(low) > 0,
                    "invalid weapon cooldown jitter"
                );
            }
            ensure!(weapon.strikes.len() <= 16, "too many weapon strikes");
            ensure!(
                weapon.projectile_speed <= 1024 * 256
                    && (!weapon.friendly_splash || weapon.splash.is_some())
                    && (weapon.projectile_speed == 0 || weapon.strikes.is_empty()),
                "invalid projectile weapon profile"
            );
            ensure!(
                weapon.target_classes.len() <= 3
                    && weapon
                        .target_classes
                        .iter()
                        .enumerate()
                        .all(|(i, c)| !weapon.target_classes[..i].contains(c)),
                "invalid weapon target classes"
            );
            ensure!(
                weapon
                    .strikes
                    .windows(2)
                    .all(|pair| pair[0].delay <= pair[1].delay)
                    && weapon
                        .strikes
                        .iter()
                        .all(|strike| strike.delay < weapon.cooldown && strike.forward <= 32768),
                "invalid weapon strike sequence"
            );
            if let Some(radii) = weapon.splash {
                ensure!(
                    radii[0] <= radii[1] && radii[1] <= radii[2] && radii[2] <= 32768,
                    "invalid splash radii"
                );
            }
            ensure!(
                weapon.damage <= 1_000_000
                    && (weapon.damage > 0
                        || matches!(unit.stored_weapon, Some(StoredWeapon::Fighters { .. })))
                    && weapon.range <= 32768
                    && (1..=1_000_000).contains(&weapon.cooldown),
                "invalid weapon"
            );
            if let DamageKind::Split {
                piercing,
                minimum_percent,
            } = weapon.damage_kind
            {
                ensure!(
                    piercing <= weapon.damage && (1..=100).contains(&minimum_percent),
                    "invalid split damage"
                );
            }
        }
    }
    Ok(())
}

mod combat;
mod projectiles;
pub use projectiles::PendingProjectile;
pub(in crate::sim) use projectiles::validate_weapon_projectiles;
mod construction;
mod economy;
mod harvesting;
mod navigation;
mod orders;
mod resource_terrain;
mod routing;
mod tick;

impl World {
    pub fn unit_type(&self, id: UnitTypeId) -> Option<&UnitType> {
        self.rules
            .units
            .binary_search_by_key(&id, |unit| unit.id)
            .ok()
            .map(|index| &self.rules.units[index])
    }
    pub(super) fn index(&self, id: EntityId) -> Option<usize> {
        self.state
            .entities
            .binary_search_by_key(&id, |entity| entity.id)
            .ok()
    }
    pub(super) fn unit_at(&self, index: usize) -> &UnitType {
        self.unit_type(self.state.entities[index].unit_type)
            .expect("validated type")
    }
    pub fn resource_balance(&self, player: PlayerId, kind: &str) -> u64 {
        self.state
            .players
            .get(usize::from(player.0))
            .and_then(|state| state.resources.get(kind))
            .copied()
            .unwrap_or(0)
    }
    /// Supply reserves only the active production item, including a finished
    /// unit waiting for an exit. Queued items wait for capacity before starting.
    pub fn supply(&self, player: PlayerId) -> (u32, u32) {
        let mut used = 0_u32;
        let mut provided = 0_u32;
        for entity in self
            .state
            .entities
            .iter()
            .filter(|entity| entity.owner == player && entity.illusion_remaining.is_none())
        {
            let unit = self.unit_type(entity.unit_type).expect("validated type");
            if entity.construction.is_none() {
                used += unit.supply_used;
                provided += unit.supply_provided;
            }
            if let Some(job) = entity.production.front().filter(|job| job.started) {
                let produced = self.unit_type(job.unit_type).expect("validated type");
                used += (produced.supply_used * u32::from(produced.production_count))
                    .saturating_sub(if job.producer_type.is_some() {
                        unit.supply_used
                    } else {
                        0
                    });
            }
        }
        (used, provided.min(self.rules.supply_limit))
    }
}

pub(super) fn attack_cooldown(weapon: &Weapon, rng: &mut u64) -> u32 {
    let jitter = weapon.cooldown_jitter.map_or(0, |[low, high]| {
        low + (splitmix64(rng) % (high - low + 1) as u64) as i32
    });
    (i64::from(weapon.cooldown) + i64::from(jitter)).max(1) as u32
}

pub(super) fn scaled_damage(
    raw: u64,
    kind: DamageKind,
    target: &UnitType,
    armor_bonus: u32,
) -> u64 {
    let quarters = match (kind, target.size) {
        (DamageKind::Explosive, UnitSize::Small) | (DamageKind::Concussive, UnitSize::Medium) => 2,
        (DamageKind::Explosive, UnitSize::Medium) => 3,
        (DamageKind::Concussive, UnitSize::Large) => 1,
        _ => 4,
    };
    (raw.saturating_sub(u64::from(target.armor + armor_bonus) * 256) * quarters / 4).max(128)
}

pub(in crate::sim) fn overlaps(a: Position, af: Footprint, b: Position, bf: Footprint) -> bool {
    let [al, at, ar, ab] = af.bounds(a);
    let [bl, bt, br, bb] = bf.bounds(b);
    al < br && ar > bl && at < bb && ab > bt
}
pub(super) fn distance(a: Position, b: Position) -> i64 {
    let x = i64::from(a.x) - i64::from(b.x);
    let y = i64::from(a.y) - i64::from(b.y);
    x * x + y * y
}
pub(super) fn in_range(a: Position, af: Footprint, b: Position, bf: Footprint, range: u32) -> bool {
    let [al, at, ar, ab] = af.bounds(a);
    let [bl, bt, br, bb] = bf.bounds(b);
    let x = (al - br).max(bl - ar).max(0);
    let y = (at - bb).max(bt - ab).max(0);
    x * x + y * y <= i64::from(range) * i64::from(range)
}
/// Candidate centers immediately outside each edge. Corners and the projected
/// current position make large structures accessible without special unit sizes.
pub(super) fn perimeter(
    position: Position,
    footprint: Footprint,
    actor: Footprint,
    reference: Position,
) -> Vec<Position> {
    let [l, t, r, b] = footprint.bounds(position);
    let left = l - i64::from(actor.width - actor.width / 2);
    let right = r + i64::from(actor.width / 2);
    let top = t - i64::from(actor.height - actor.height / 2);
    let bottom = b + i64::from(actor.height / 2);
    let mut result = Vec::new();
    for x in [
        left,
        right,
        i64::from(position.x),
        i64::from(reference.x).clamp(left, right),
    ] {
        for y in [top, bottom] {
            result.push(Position {
                x: x as i32,
                y: y as i32,
            });
        }
    }
    for y in [
        top,
        bottom,
        i64::from(position.y),
        i64::from(reference.y).clamp(top, bottom),
    ] {
        for x in [left, right] {
            result.push(Position {
                x: x as i32,
                y: y as i32,
            });
        }
    }
    let step = usize::from(actor.width.min(actor.height).clamp(1, 8));
    for x in (left..=right).step_by(step) {
        result.push(Position {
            x: x as i32,
            y: top as i32,
        });
        result.push(Position {
            x: x as i32,
            y: bottom as i32,
        });
    }
    for y in (top..=bottom).step_by(step) {
        result.push(Position {
            x: left as i32,
            y: y as i32,
        });
        result.push(Position {
            x: right as i32,
            y: y as i32,
        });
    }
    result.sort_by_key(|point| (distance(*point, reference), point.y, point.x));
    result.dedup();
    result
}

mod encoding;
pub(in crate::sim) use encoding::*;
