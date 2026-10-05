//! Bounded native campaign triggers. Source formats are translated by the importer.
//! Trigger execution is authoritative; text, sound and camera events are cosmetic.
use super::*;

const MAX_EVENTS: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MissionLocation {
    /// Excluded elevation bits: flying levels 0..2, ground levels 3..5.
    #[serde(default)]
    pub excluded_elevations: u8,
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}
impl MissionLocation {
    pub fn center(self) -> Position {
        Position {
            x: (self.left + self.right) / 2,
            y: (self.top + self.bottom) / 2,
        }
    }
    fn overlaps(self, position: Position, footprint: Footprint) -> bool {
        let [left, top, right, bottom] = footprint.bounds(position);
        left < i64::from(self.right)
            && right > i64::from(self.left)
            && top < i64::from(self.bottom)
            && bottom > i64::from(self.top)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MissionComparison {
    AtLeast,
    AtMost,
    Exactly,
}
impl MissionComparison {
    fn test(self, actual: u32, amount: u32) -> bool {
        match self {
            Self::AtLeast => actual >= amount,
            Self::AtMost => actual <= amount,
            Self::Exactly => actual == amount,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MissionUnits {
    Men,
    Any,
    Structures,
    Type(UnitTypeId),
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum MissionCondition {
    Resources {
        players: Vec<PlayerId>,
        kinds: Vec<String>,
        comparison: MissionComparison,
        amount: u32,
    },
    Deaths {
        players: Vec<PlayerId>,
        units: MissionUnits,
        comparison: MissionComparison,
        amount: u32,
    },
    RankedCount {
        player: PlayerId,
        units: MissionUnits,
        location: u16,
        most: bool,
    },
    Kills {
        players: Vec<PlayerId>,
        units: MissionUnits,
        comparison: MissionComparison,
        amount: u32,
    },
    Elapsed {
        comparison: MissionComparison,
        milliseconds: u32,
    },
    Countdown {
        comparison: MissionComparison,
        milliseconds: u32,
    },
    Count {
        players: Vec<PlayerId>,
        units: MissionUnits,
        location: Option<u16>,
        comparison: MissionComparison,
        amount: u32,
    },
    Switch {
        index: u16,
        set: bool,
    },
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UnitProperties {
    pub hp_percent: Option<u8>,
    pub shield_percent: Option<u8>,
    pub energy_percent: Option<u8>,
    pub invincible: bool,
    pub cloaked: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum MissionAction {
    GrantResearch {
        player: PlayerId,
        research: ResearchId,
    },
    Resume,
    Preserve,
    Cosmetic,
    Countdown {
        milliseconds: u32,
    },
    Rescue {
        players: Vec<PlayerId>,
    },
    Assault {
        players: Vec<PlayerId>,
    },
    EnterBunkers {
        players: Vec<PlayerId>,
        location: u16,
    },
    Remove {
        players: Vec<PlayerId>,
        units: MissionUnits,
        location: Option<u16>,
    },
    Teleport {
        players: Vec<PlayerId>,
        units: MissionUnits,
        location: u16,
        destination: u16,
    },
    ToggleDoodad {
        players: Vec<PlayerId>,
        units: MissionUnits,
        location: u16,
        /// None toggles; Some explicitly enables or disables the object.
        #[serde(default)]
        enabled: Option<bool>,
    },
    StartAi {
        controller: u16,
    },
    Victory,
    Defeat,
    Wait {
        milliseconds: u32,
    },
    Pause,
    SetSwitch {
        index: u16,
        set: bool,
    },
    SetResources {
        players: Vec<PlayerId>,
        resources: Vec<ResourceAmount>,
    },
    Create {
        player: PlayerId,
        unit_type: UnitTypeId,
        location: u16,
        #[serde(default)]
        properties: UnitProperties,
    },
    Kill {
        players: Vec<PlayerId>,
        units: MissionUnits,
        location: u16,
    },
    MoveLocation {
        location: u16,
        players: Vec<PlayerId>,
        units: MissionUnits,
        search_location: u16,
    },
    Invincibility {
        players: Vec<PlayerId>,
        units: MissionUnits,
        location: u16,
        enabled: bool,
    },
    Objectives {
        text: u16,
    },
    Text {
        text: u16,
    },
    Sound {
        sound: u16,
    },
    CenterView {
        location: u16,
    },
    Speech {
        muted: bool,
    },
    Transmission {
        text: u16,
        sound: Option<u16>,
        portrait: UnitTypeId,
        location: u16,
        milliseconds: u32,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MissionTrigger {
    pub conditions: Vec<MissionCondition>,
    pub actions: Vec<MissionAction>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Mission {
    pub schema_version: u32,
    /// Current-player actions and the shared wait belong to this player.
    pub player: PlayerId,
    #[serde(default)]
    pub rescuable_players: Vec<PlayerId>,
    #[serde(default)]
    pub rescuers: Vec<PlayerId>,
    #[serde(default)]
    pub alliances: Vec<[PlayerId; 2]>,
    pub poll_ticks: u32,
    pub wait_step_ms: u32,
    pub locations: Vec<MissionLocation>,
    pub triggers: Vec<MissionTrigger>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MissionTriggerState {
    #[serde(default)]
    pub preserve: bool,
    pub action: u16,
    pub started: bool,
    pub complete: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MissionWait {
    pub trigger: u16,
    pub remaining_ms: u32,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MissionEvent {
    Objectives {
        text: u16,
    },
    Text {
        text: u16,
    },
    Sound {
        sound: u16,
    },
    CenterView {
        position: Position,
    },
    Speech {
        muted: bool,
    },
    Transmission {
        text: u16,
        sound: Option<u16>,
        portrait: UnitTypeId,
        position: Position,
        milliseconds: u32,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MissionState {
    #[serde(default)]
    pub rescue_players: Vec<PlayerId>,
    pub triggers: Vec<MissionTriggerState>,
    pub switches: Vec<bool>,
    pub locations: Vec<MissionLocation>,
    pub countdown_ms: u32,
    pub poll_remaining: u32,
    pub wait: Option<MissionWait>,
    pub paused: bool,
    /// One-shot triggers append at most MAX_EVENTS events. Excluded from hashes.
    pub events: Vec<MissionEvent>,
}
impl MissionState {
    pub fn new(mission: &Mission) -> Self {
        Self {
            rescue_players: mission.rescuable_players.clone(),
            triggers: vec![MissionTriggerState::default(); mission.triggers.len()],
            switches: vec![false; 256],
            locations: mission.locations.clone(),
            countdown_ms: 0,
            poll_remaining: 1,
            wait: None,
            paused: false,
            events: Vec::new(),
        }
    }
    fn emit(&mut self, event: MissionEvent) {
        // Validation bounds the total one-shot action count, so events cannot overflow.
        if self.events.len() < MAX_EVENTS {
            self.events.push(event);
        }
    }
}

impl Mission {
    pub fn validate(&self, rules: &Rules, map: &Map) -> Result<()> {
        ensure!(self.schema_version == 1, "unsupported mission schema");
        ensure!(self.player.0 < map.players, "invalid mission player");
        for players in [&self.rescuable_players, &self.rescuers] {
            let mut unique = BTreeSet::new();
            ensure!(
                players.len() <= usize::from(map.players)
                    && players
                        .iter()
                        .all(|p| p.0 < map.players && unique.insert(p)),
                "invalid mission rescue players"
            );
        }
        ensure!(
            !self
                .rescuable_players
                .iter()
                .any(|p| self.rescuers.contains(p)),
            "a rescuer cannot be rescuable"
        );
        let mut pairs = BTreeSet::new();
        ensure!(
            self.alliances.len() <= 496
                && self
                    .alliances
                    .iter()
                    .all(|[a, b]| a < b && b.0 < map.players && pairs.insert((*a, *b))),
            "invalid mission alliance pairs"
        );
        ensure!(
            (1..=1000).contains(&self.poll_ticks) && (1..=1000).contains(&self.wait_step_ms),
            "invalid mission clock"
        );
        ensure!(
            !self.locations.is_empty() && self.locations.len() <= 256,
            "invalid mission location count"
        );
        for location in &self.locations {
            ensure!(
                location.excluded_elevations & !63 == 0
                    && location.left >= 0
                    && location.top >= 0
                    && location.right <= map.width
                    && location.bottom <= map.height
                    && location.left < location.right
                    && location.top < location.bottom,
                "mission location outside map"
            );
        }
        ensure!(
            !self.triggers.is_empty() && self.triggers.len() <= 256,
            "invalid mission trigger count"
        );
        ensure!(
            self.triggers
                .iter()
                .map(|trigger| trigger.actions.len())
                .sum::<usize>()
                <= MAX_EVENTS,
            "too many mission actions"
        );
        let location = |id: u16| -> Result<()> {
            ensure!(
                usize::from(id) < self.locations.len(),
                "unknown mission location"
            );
            Ok(())
        };
        let unit = |id: UnitTypeId| -> Result<()> {
            ensure!(
                rules.units.iter().any(|unit| unit.id == id),
                "unknown mission unit"
            );
            Ok(())
        };
        let filter = |players: &[PlayerId], units: MissionUnits| -> Result<()> {
            ensure!(
                !players.is_empty() && players.len() <= usize::from(map.players),
                "invalid mission players"
            );
            let mut unique = BTreeSet::new();
            ensure!(
                players
                    .iter()
                    .all(|p| p.0 < map.players && unique.insert(p)),
                "unknown or duplicate mission player"
            );
            if let MissionUnits::Type(id) = units {
                unit(id)?;
            }
            Ok(())
        };
        let time = |ms: u32| -> Result<()> {
            ensure!(ms <= 3_600_000, "mission wait exceeds one hour");
            Ok(())
        };
        for trigger in &self.triggers {
            ensure!(
                !trigger.conditions.is_empty()
                    && trigger.conditions.len() <= 16
                    && !trigger.actions.is_empty()
                    && trigger.actions.len() <= 64,
                "invalid mission trigger size"
            );
            for condition in &trigger.conditions {
                match condition {
                    MissionCondition::Resources { players, kinds, .. } => {
                        filter(players, MissionUnits::Any)?;
                        ensure!(
                            !kinds.is_empty()
                                && kinds.len() <= 32
                                && kinds
                                    .iter()
                                    .all(|kind| !kind.is_empty() && kind.len() <= 64),
                            "invalid mission resource kinds"
                        );
                    }
                    MissionCondition::Kills { players, units, .. }
                    | MissionCondition::Deaths { players, units, .. } => filter(players, *units)?,
                    MissionCondition::RankedCount {
                        player,
                        units,
                        location: at,
                        ..
                    } => {
                        filter(&[*player], *units)?;
                        location(*at)?;
                    }
                    MissionCondition::Elapsed { milliseconds, .. } => {
                        time(*milliseconds)?;
                        ensure!(
                            milliseconds.is_multiple_of(1000),
                            "elapsed time uses whole seconds"
                        );
                    }
                    MissionCondition::Countdown { milliseconds, .. } => time(*milliseconds)?,
                    MissionCondition::Switch { index, .. } => {
                        ensure!(*index < 256, "invalid mission switch")
                    }
                    MissionCondition::Count {
                        players,
                        units,
                        location: at,
                        amount,
                        ..
                    } => {
                        filter(players, *units)?;
                        if let Some(at) = at {
                            location(*at)?;
                        }
                        ensure!(*amount <= 4096, "invalid mission unit count");
                    }
                }
            }
            for action in &trigger.actions {
                match action {
                    MissionAction::GrantResearch { player, research } => {
                        ensure!(
                            player.0 < map.players
                                && rules.research.iter().any(|r| r.id == *research),
                            "invalid granted research"
                        );
                    }
                    MissionAction::Countdown { milliseconds } => time(*milliseconds)?,
                    MissionAction::Rescue { players } | MissionAction::Assault { players } => {
                        filter(players, MissionUnits::Any)?
                    }
                    MissionAction::EnterBunkers {
                        players,
                        location: at,
                    } => {
                        filter(players, MissionUnits::Any)?;
                        location(*at)?;
                    }
                    MissionAction::Remove {
                        players,
                        units,
                        location: at,
                    } => {
                        filter(players, *units)?;
                        if let Some(at) = at {
                            location(*at)?;
                        }
                    }
                    MissionAction::Teleport {
                        players,
                        units,
                        location: at,
                        destination,
                    } => {
                        filter(players, *units)?;
                        location(*at)?;
                        location(*destination)?;
                    }
                    MissionAction::ToggleDoodad {
                        players,
                        units,
                        location: at,
                        ..
                    } => {
                        filter(players, *units)?;
                        location(*at)?;
                    }
                    MissionAction::Resume | MissionAction::Preserve | MissionAction::Cosmetic => {}
                    MissionAction::StartAi { controller } => ensure!(
                        usize::from(*controller) < map.ai.len(),
                        "unknown mission AI town"
                    ),
                    MissionAction::Wait { milliseconds } => time(*milliseconds)?,
                    MissionAction::SetSwitch { index, .. } => {
                        ensure!(*index < 256, "invalid mission switch")
                    }
                    MissionAction::SetResources { players, resources } => {
                        filter(players, MissionUnits::Any)?;
                        ensure!(
                            !resources.is_empty() && resources.len() <= 32,
                            "invalid mission resource count"
                        );
                        let mut kinds = BTreeSet::new();
                        ensure!(
                            resources.iter().all(|r| !r.kind.is_empty()
                                && r.kind.len() <= 64
                                && r.kind
                                    .bytes()
                                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
                                && kinds.insert(&r.kind)),
                            "invalid mission resource kind"
                        );
                    }
                    MissionAction::Create {
                        player,
                        unit_type,
                        location: at,
                        properties,
                    } => {
                        filter(&[*player], MissionUnits::Type(*unit_type))?;
                        location(*at)?;
                        ensure!(
                            [
                                properties.hp_percent,
                                properties.shield_percent,
                                properties.energy_percent
                            ]
                            .into_iter()
                            .flatten()
                            .all(|v| v <= 100),
                            "invalid created unit properties"
                        );
                        ensure!(
                            !properties.cloaked
                                || rules
                                    .units
                                    .iter()
                                    .find(|u| u.id == *unit_type)
                                    .is_some_and(|u| u.cloak.is_some()),
                            "created unit cannot conceal"
                        );
                    }
                    MissionAction::Kill {
                        players,
                        units,
                        location: at,
                    }
                    | MissionAction::Invincibility {
                        players,
                        units,
                        location: at,
                        ..
                    } => {
                        filter(players, *units)?;
                        location(*at)?;
                    }
                    MissionAction::MoveLocation {
                        location: at,
                        players,
                        units,
                        search_location,
                    } => {
                        filter(players, *units)?;
                        location(*at)?;
                        location(*search_location)?;
                    }
                    MissionAction::CenterView { location: at } => location(*at)?,
                    MissionAction::Objectives { text } | MissionAction::Text { text } => {
                        ensure!(*text < 4096, "invalid mission text ID")
                    }
                    MissionAction::Sound { sound } => {
                        ensure!(*sound < 128, "invalid mission sound ID")
                    }
                    MissionAction::Transmission {
                        text,
                        sound,
                        location: at,
                        milliseconds,
                        ..
                    } => {
                        location(*at)?;
                        time(*milliseconds)?;
                        ensure!(
                            *text < 4096 && sound.is_none_or(|id| id < 128),
                            "invalid mission transmission IDs"
                        );
                    }
                    MissionAction::Victory
                    | MissionAction::Defeat
                    | MissionAction::Pause
                    | MissionAction::Speech { .. } => {}
                }
            }
        }
        Ok(())
    }
}

mod runtime;
impl World {}

#[cfg(test)]
mod tests;

mod encoding;
pub(in crate::sim) use encoding::*;
