use super::*;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Tick(pub u64);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PlayerId(pub u16);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EntityId(pub u32);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct UnitTypeId(pub u16);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Position {
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UnitType {
    /// Excluded from enemy acquisition and highlights; explicit attacks are allowed.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub neutral: bool,
    /// Ambient movement while idle; explicit orders always take priority.
    #[serde(default)]
    pub idle_wander: Option<IdleWander>,
    #[serde(default)]
    pub mode: Option<ModeChange>,
    #[serde(default)]
    pub energy_pool: Option<EnergyPool>,
    #[serde(default)]
    pub abilities: Vec<TargetedAbility>,
    #[serde(default)]
    pub max_shields: u32,
    #[serde(default)]
    pub portable: bool,
    #[serde(default)]
    pub transforms_on_production: bool,
    #[serde(default)]
    pub production_form: Option<UnitTypeId>,
    /// Cancelling production destroys this intermediate body instead of reverting it.
    #[serde(default)]
    pub destroyed_on_production_cancel: bool,
    #[serde(default = "super::garrison::default_cargo_size")]
    pub production_count: u8,
    /// Nonzero stores completed production inside the producer instead of using an exit.
    #[serde(default)]
    pub production_capacity: u8,
    #[serde(default)]
    pub stored_weapon: Option<StoredWeapon>,
    #[serde(default)]
    pub offspring: Option<Offspring>,
    #[serde(default)]
    pub provides_types: Vec<UnitTypeId>,
    #[serde(default)]
    pub shield_regeneration: u16,
    #[serde(default)]
    pub power_field: Option<PowerField>,
    #[serde(default)]
    pub requires_power: bool,
    #[serde(default)]
    pub autonomous_construction: bool,
    /// The primary builder works inside the foundation until completion.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub builder_inside: bool,
    /// Repair orders on an unfinished structure contribute construction work.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub repair_construction: bool,
    /// An extractor's primary builder gathers its resource after completion.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub builder_gathers_resource: bool,

    #[serde(default = "super::garrison::default_cargo_size")]
    pub cargo_size: u8,
    #[serde(default = "bool_true")]
    pub blocks_movement: bool,
    /// Resource orders may pass mobile traffic while retaining static collision.
    #[serde(default)]
    pub phases_while_gathering: bool,
    #[serde(default)]
    pub consumes_builder: bool,
    #[serde(default = "bool_true")]
    pub attacks_ground: bool,
    #[serde(default)]
    pub mine_layer: Option<MineLayer>,
    #[serde(default)]
    pub mine: Option<MineStats>,
    #[serde(default = "mines::triggers_mines_default")]
    pub triggers_mines: bool,
    #[serde(default)]
    pub garrison: Option<GarrisonStats>,
    pub id: UnitTypeId,
    pub speed: i32,
    #[serde(default)]
    pub motion: Option<Motion>,
    #[serde(default)]
    pub footprint: Footprint,
    #[serde(default)]
    pub movement_class: MovementClass,
    #[serde(default = "one")]
    pub max_hp: u32,
    #[serde(default)]
    pub armor: u32,
    #[serde(default)]
    pub size: UnitSize,
    /// None retains weapon-range acquisition. An explicit radius also permits pursuit.
    #[serde(default)]
    pub acquisition_range: Option<u32>,
    #[serde(default)]
    pub vision_range: u32,
    #[serde(default)]
    pub revealer: bool,
    /// HP restored each tick, measured in 1/256 HP.
    #[serde(default)]
    pub regeneration: u16,
    #[serde(default)]
    pub extracts: Option<Extraction>,
    #[serde(default)]
    pub addon_parent: Option<UnitTypeId>,
    #[serde(default)]
    pub creep_radius: Option<[u16; 2]>,
    #[serde(default)]
    pub requires_creep: bool,
    #[serde(default)]
    pub scanner: Option<Scanner>,
    #[serde(default)]
    pub cloak: Option<Cloak>,
    #[serde(default)]
    pub concealment_field: Option<ConcealmentField>,
    #[serde(default)]
    pub detector_range: u32,
    #[serde(default)]
    pub flight: Option<Flight>,
    #[serde(default)]
    pub structure: bool,
    #[serde(default)]
    pub placement_surface: crate::map::PlacementSurface,
    #[serde(default)]
    pub placement: Footprint,
    /// Minimum gap from a resource's collision rectangle when constructing/landing.
    #[serde(default)]
    pub resource_clearance: u16,
    #[serde(default)]
    pub cost: Vec<ResourceAmount>,
    #[serde(default = "one")]
    pub build_ticks: u32,
    #[serde(default)]
    pub supply_used: u32,
    #[serde(default)]
    pub supply_provided: u32,
    #[serde(default)]
    pub prerequisites: Vec<UnitTypeId>,
    #[serde(default)]
    pub builds: Vec<UnitTypeId>,
    #[serde(default)]
    pub repairs: Vec<UnitTypeId>,
    #[serde(default)]
    pub trains: Vec<UnitTypeId>,
    #[serde(default)]
    pub dropoff: Vec<String>,
    #[serde(default)]
    pub worker: Option<WorkerStats>,
    /// Optional per-resource timing and shared access, overriding worker defaults.
    #[serde(default)]
    pub harvest_profiles: Vec<HarvestProfile>,
    /// Completed owned facilities improve deposits by the strongest bonus per kind.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub harvest_bonus_percent: Vec<ResourceAmount>,
    #[serde(default)]
    pub weapon: Option<Weapon>,
    /// A distinct profile for air targets; None retains the shared weapon.
    #[serde(default)]
    pub air_weapon: Option<Weapon>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rules {
    pub id: String,
    /// Automatic acquisition ranks threats before distance when enabled.
    #[serde(default)]
    pub prioritize_threats: bool,
    pub tick_ms: u32,
    pub units: Vec<UnitType>,
    #[serde(default)]
    pub starting_resources: Vec<ResourceAmount>,
    #[serde(default = "default_supply_limit")]
    pub supply_limit: u32,
    #[serde(default)]
    pub victory: bool,
    #[serde(default)]
    pub repair: Option<RepairRules>,
    #[serde(default)]
    pub research: Vec<Research>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spawn {
    #[serde(default)]
    pub stored_units: u8,
    #[serde(default)]
    pub linked_to: Option<Position>,
    #[serde(default)]
    pub doodad_enabled: Option<bool>,
    pub owner: PlayerId,
    pub unit_type: UnitTypeId,
    pub position: Position,
    #[serde(default)]
    pub hp_percent: Option<u8>,
    #[serde(default)]
    pub shield_percent: Option<u8>,
    #[serde(default)]
    pub energy_percent: Option<u8>,
    #[serde(default)]
    pub invincible: bool,
    #[serde(default)]
    pub cloaked: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartLocation {
    pub player: PlayerId,
    pub position: Position,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceSpawn {
    /// Optional occupied terrain corners. Depletion reconnects adjoining cells
    /// and removes unsupported fragments; ordinary standalone resources omit it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terrain_corners: Option<u8>,
    #[serde(default)]
    pub footprint: Footprint,
    pub kind: String,
    pub position: Position,
    pub amount: u32,
    #[serde(default)]
    pub requires_extractor: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Map {
    #[serde(default)]
    pub initial_explored: BTreeMap<PlayerId, Vec<u32>>,
    #[serde(default)]
    pub creation: BTreeMap<PlayerId, Vec<UnitTypeId>>,
    #[serde(default)]
    pub ai: Vec<AiController>,
    pub id: String,
    pub width: i32,
    pub height: i32,
    pub players: u16,
    pub spawns: Vec<Spawn>,
    #[serde(default)]
    pub start_locations: Vec<StartLocation>,
    #[serde(default)]
    pub resources: Vec<ResourceSpawn>,
    #[serde(default)]
    pub fog_of_war: bool,
    /// Loaded from optional native mission.ron alongside the map.
    #[serde(skip)]
    pub mission: Option<Mission>,
    /// Loaded from native terrain.srtm at the package boundary, never from RON.
    #[serde(skip)]
    pub terrain: Option<Terrain>,
}

impl Map {
    pub fn contains(&self, p: Position) -> bool {
        p.x >= 0 && p.y >= 0 && p.x < self.width && p.y < self.height
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Order {
    ReceiveAbility {
        entity: EntityId,
        provider: EntityId,
        ability: AbilityId,
    },
    ChangeMode {
        entity: EntityId,
    },
    Cast {
        entity: EntityId,
        ability: AbilityId,
        target: AbilityTarget,
    },
    Move {
        entity: EntityId,
        target: Position,
    },
    Stop {
        entity: EntityId,
    },
    /// Fixture command exercising authoritative RNG, not a game-specific ability.
    Wander {
        entity: EntityId,
    },
    Attack {
        entity: EntityId,
        target: EntityId,
    },
    AttackMove {
        entity: EntityId,
        target: Position,
    },
    Hold {
        entity: EntityId,
    },
    Patrol {
        entity: EntityId,
        target: Position,
    },
    Gather {
        entity: EntityId,
        resource: ResourceId,
    },
    Build {
        entity: EntityId,
        unit_type: UnitTypeId,
        position: Position,
    },
    Resume {
        entity: EntityId,
        building: EntityId,
    },
    Repair {
        entity: EntityId,
        target: EntityId,
    },
    Train {
        entity: EntityId,
        unit_type: UnitTypeId,
    },
    Cancel {
        entity: EntityId,
    },
    Rally {
        entity: EntityId,
        target: Position,
    },
    RallyResource {
        entity: EntityId,
        resource: ResourceId,
    },
    Cloak {
        entity: EntityId,
        enabled: bool,
    },
    Scan {
        entity: EntityId,
        target: Position,
    },
    PlaceMine {
        entity: EntityId,
        target: Position,
    },
    Load {
        entity: EntityId,
        target: EntityId,
    },
    Unload {
        entity: EntityId,
    },
    UnloadAt {
        entity: EntityId,
        target: Position,
    },
    UnloadPassenger {
        entity: EntityId,
        passenger: EntityId,
    },
    Research {
        entity: EntityId,
        research: ResearchId,
    },
    Stim {
        entity: EntityId,
    },
    Lift {
        entity: EntityId,
    },
    Land {
        entity: EntityId,
        target: Position,
    },
    Queue {
        entity: EntityId,
        order: UnitOrder,
    },
}

impl Order {
    pub fn entity(&self) -> EntityId {
        match *self {
            Self::ReceiveAbility { entity, .. } => entity,
            Self::ChangeMode { entity } => entity,
            Self::Cast { entity, .. } => entity,
            Self::Move { entity, .. }
            | Self::Stop { entity }
            | Self::Wander { entity }
            | Self::Attack { entity, .. }
            | Self::AttackMove { entity, .. }
            | Self::Hold { entity }
            | Self::Patrol { entity, .. }
            | Self::Gather { entity, .. }
            | Self::Build { entity, .. }
            | Self::Resume { entity, .. }
            | Self::Repair { entity, .. }
            | Self::Train { entity, .. }
            | Self::Cancel { entity }
            | Self::Rally { entity, .. }
            | Self::RallyResource { entity, .. }
            | Self::Cloak { entity, .. }
            | Self::Scan { entity, .. }
            | Self::Research { entity, .. }
            | Self::Stim { entity }
            | Self::Lift { entity }
            | Self::Land { entity, .. }
            | Self::Load { entity, .. }
            | Self::Unload { entity }
            | Self::UnloadAt { entity, .. }
            | Self::UnloadPassenger { entity, .. }
            | Self::PlaceMine { entity, .. }
            | Self::Queue { entity, .. } => entity,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Command {
    pub tick: Tick,
    pub player: PlayerId,
    pub sequence: u64,
    pub order: Order,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Rejection {
    ComputerControlled,
    WrongTick,
    UnknownPlayer,
    DuplicateSequence,
    StaleSequence,
    UnknownEntity,
    NotOwner,
    OutOfBounds,
    InvalidTarget,
    UnsupportedOrder,
    InsufficientResources,
    InsufficientSupply,
    NotPowered,
    MissingPrerequisite,
    InvalidPlacement,
    QueueFull,
    Cooldown,
    Unfinished,
    EntityLimit,
    GameOver,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandOutcome {
    pub command: Command,
    pub rejection: Option<Rejection>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entity {
    #[serde(default)]
    pub linked_to: Option<EntityId>,
    #[serde(default)]
    pub wander: Option<WanderState>,
    #[serde(default)]
    pub mode_transition: Option<ModeTransition>,
    #[serde(default)]
    pub ability_auras: Vec<AbilityAura>,
    #[serde(default)]
    pub last_cast: Option<CastAppearance>,
    /// Temporary copies deal no damage, receive doubled damage and use no supply.
    /// This state is disclosed only to the owning player.
    #[serde(default)]
    pub illusion_remaining: Option<u32>,
    #[serde(default)]
    pub lifetime_remaining: Option<u32>,
    #[serde(default)]
    pub carried_by: Option<EntityId>,
    #[serde(default)]
    pub doodad_enabled: Option<bool>,
    pub mine_count: u8,
    pub mine_state: Option<MineState>,
    pub garrisoned_in: Option<EntityId>,
    pub id: EntityId,
    pub owner: PlayerId,
    pub unit_type: UnitTypeId,
    pub position: Position,
    pub target: Option<Position>,
    pub hp: u32,
    #[serde(default)]
    pub shields: u32,
    #[serde(default)]
    pub offspring_remaining: u32,
    /// Damage below one displayed HP, measured in 1/256 HP. Display HP rounds up.
    pub damage_fraction: u8,
    pub invincible: bool,
    pub strikes: Vec<PendingStrike>,
    pub gathering_inside: bool,
    pub parent: Option<EntityId>,
    pub energy: u32,
    #[serde(default)]
    pub cloaked: bool,
    #[serde(default)]
    pub last_attack_air: bool,
    #[serde(default)]
    pub last_attack_target: Option<EntityId>,
    #[serde(default)]
    pub last_attack_position: Option<Position>,
    pub cloak_transition: u32,
    pub airborne: bool,
    pub flight_transition: u32,
    pub construction: Option<Construction>,
    pub production: VecDeque<ProductionJob>,
    pub research: Option<ResearchJob>,
    pub stim_remaining: u32,
    pub cargo: Option<ResourceAmount>,
    pub dropoff_target: Option<EntityId>,
    pub rally: Option<Position>,
    #[serde(default)]
    pub rally_resource: Option<ResourceId>,
    pub order: UnitOrder,
    pub queued_orders: VecDeque<UnitOrder>,
    /// Remember acquired enemies and attackers without replacing the player's order.
    #[serde(default)]
    pub auto_attack_target: Option<EntityId>,
    /// Last damage-source/seen position during retaliation, including beyond sight.
    #[serde(default)]
    pub retaliation_position: Option<Position>,
    pub path: VecDeque<Position>,
    pub path_retry: Tick,
    /// Static geometry against which the remaining route was last checked.
    #[serde(default)]
    pub path_geometry: [u8; 32],
    #[serde(default)]
    pub route_wait: Option<RouteWait>,
    pub cooldown: u32,
    #[serde(default)]
    pub unload_remaining: u32,
    pub harvest_progress: u32,
    #[serde(default)]
    pub harvest_waiting_since: Option<Tick>,
    /// Exclusive stopping point beside an outdoor resource, including approach time.
    #[serde(default)]
    pub harvest_spot: Option<Position>,
    /// Original ordered resource position; automatic patch switches retain it.
    #[serde(default)]
    pub gather_origin: Option<Position>,
    /// Subpixel displacement relative to the displayed integer position (1/256 px).
    #[serde(default)]
    pub motion_fraction: [i32; 2],
    #[serde(default)]
    pub motion_speed: u32,
    #[serde(default)]
    pub motion_phase: u32,
    /// Fractional repair HP, in the current target's repair-time denominator.
    pub repair_progress: u64,
    /// Prepaid fractional repair resources, indexed by this unit type's costs.
    /// Kept on the target so changing workers/orders cannot avoid payment.
    pub repair_credit: Vec<u64>,
    pub patrol_origin: Option<Position>,
    pub patrol_returning: bool,
}

/// A temporary collision retains the preferred route while a longer alternative
/// waits for its distance-proportional deadline. Times are simulation ticks.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteWait {
    pub since: Tick,
    pub origin: Position,
    pub alternate: VecDeque<Position>,
    pub ready_at: Tick,
}

/// Authoritative state. Local saves wrap this in a versioned integrity envelope;
/// new fields need defaults and changed meanings need saved-game migrations.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct State {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub projectiles: Vec<rts::PendingProjectile>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub remains: Vec<Remains>,
    #[serde(default)]
    pub ability_fields: Vec<AbilityField>,
    #[serde(default)]
    pub pending_effects: Vec<PendingEffect>,
    /// Match reporting only; excluded from the gameplay state hash.
    #[serde(default)]
    pub statistics: Vec<PlayerStatistics>,
    #[serde(default)]
    pub kills: BTreeMap<PlayerId, BTreeMap<UnitTypeId, u32>>,
    #[serde(default)]
    pub deaths: BTreeMap<PlayerId, BTreeMap<UnitTypeId, u32>>,
    #[serde(default)]
    pub ai: Vec<AiState>,
    pub tick: Tick,
    pub rng_state: u64,
    pub next_entity_id: u32,
    pub last_sequences: Vec<u64>,
    pub entities: Vec<Entity>,
    pub players: Vec<PlayerState>,
    pub resources: Vec<ResourceNode>,
    pub winner: Option<PlayerId>,
    pub defeated: Vec<PlayerId>,
    pub mission: Option<MissionState>,
    /// 32-pixel object visibility cells. Bits 0..3 remember explored terrain
    /// heights 0..3; bits 4..7 mark those heights currently visible. Query
    /// `World::visibility` rather than treating this byte as a scalar fog state.
    pub fog: Vec<Vec<u8>>,
    /// Terrain artwork discovery: 0 unexplored, 1 explored, 2 currently visible.
    /// A visible cliff boundary need not reveal occupants on its upper height.
    pub terrain_fog: Vec<Vec<u8>>,
    pub scans: Vec<Scan>,
    #[serde(default)]
    pub creep: Vec<u8>,
    #[serde(default)]
    pub creep_seen: Vec<Vec<u8>>,
}
