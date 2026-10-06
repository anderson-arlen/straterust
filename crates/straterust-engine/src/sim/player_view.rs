//! The disclosure boundary shared by local and remote sessions. Public entities
//! have no orders, jobs, paths, passengers, resource balances or AI state.
use super::*;

#[derive(Clone, Debug)]
pub(super) struct ViewMetadata {
    pub player: PlayerId,
    pub working: BTreeSet<EntityId>,
    pub removed: BTreeSet<EntityId>,
    pub appearance: BTreeMap<EntityId, Appearance>,
    pub shots: Vec<ContainerShot>,
    pub weapon_feedback: Vec<WeaponFeedback>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Appearance {
    #[serde(default)]
    pub powered: Option<bool>,
    pub shot_heading: Option<[i16; 2]>,
    pub work_heading: Option<[i16; 2]>,
    /// Visible load artwork only; exact enemy amounts remain private.
    pub carried: Option<(String, bool)>,
    pub landing: bool,
    /// Observable transformation progress; never the production destination.
    #[serde(default)]
    pub transformation: Option<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContainerShot {
    pub container: EntityId,
    pub weapon: UnitTypeId,
    pub heading: [i16; 2],
    pub targets_air: bool,
}

/// A visible attack from a hidden source. No source identity, origin or heading is
/// disclosed; clients receive only the artwork/sound type and impact location.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WeaponFeedback {
    pub weapon: UnitTypeId,
    pub position: Position,
    pub targets_air: bool,
    /// False plays the firing cue; true renders the subsequent hit artwork.
    pub impact: bool,
}

pub(crate) fn public_heading(from: Position, to: Position) -> [i16; 2] {
    let dx = i64::from(to.x) - i64::from(from.x);
    let dy = i64::from(to.y) - i64::from(from.y);
    let scale = dx.abs().max(dy.abs()).max(1);
    [(dx * 128 / scale) as i16, (dy * 128 / scale) as i16]
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicEntity {
    pub carried_by: Option<EntityId>,
    pub id: EntityId,
    pub owner: PlayerId,
    pub unit_type: UnitTypeId,
    pub position: Position,
    pub hp: u32,
    pub shields: u32,
    pub invincible: bool,
    pub cloaked: bool,
    pub airborne: bool,
    pub flight_transition: u32,
    pub doodad_enabled: Option<bool>,
    pub parent: Option<EntityId>,
    pub construction: Option<[u32; 2]>,
    pub working: bool,
    /// Observable committed shot timing and its visible impact location.
    pub cooldown: u32,
    pub targets_air: bool,
    pub shot_position: Option<Position>,
    pub appearance: Appearance,
    pub mine: Option<(MinePhase, u32)>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ViewedEntity {
    Owned(Box<Entity>),
    Visible(PublicEntity),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MissionView {
    pub countdown_ms: u32,
    pub paused: bool,
    pub events: Vec<MissionEvent>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlayerView {
    pub tick: Tick,
    pub player: PlayerId,
    pub home: Option<Position>,
    pub creation: Option<Vec<UnitTypeId>>,
    pub entities: Vec<ViewedEntity>,
    pub economy: PlayerState,
    pub resources: Vec<ResourceNode>,
    pub fog: Vec<u8>,
    pub terrain_fog: Vec<u8>,
    pub creep: Vec<u8>,
    pub scans: Vec<Scan>,
    pub winner: Option<PlayerId>,
    pub defeated: Vec<PlayerId>,
    pub mission: Option<MissionView>,
    /// Only deaths previously disclosed to this player, not units leaving sight.
    pub removed: Vec<EntityId>,
    pub shots: Vec<ContainerShot>,
    #[serde(default)]
    pub weapon_feedback: Vec<WeaponFeedback>,
}

impl World {
    pub fn public_shots(&self) -> &[ContainerShot] {
        self.view.as_ref().map_or(&[], |view| &view.shots)
    }

    pub fn public_weapon_feedback(&self) -> &[WeaponFeedback] {
        self.view.as_ref().map_or(&[], |view| &view.weapon_feedback)
    }

    pub fn appearance(&self, id: EntityId) -> Option<&Appearance> {
        self.view.as_ref()?.appearance.get(&id)
    }

    pub fn carried_appearance<'a>(&'a self, entity: &'a Entity) -> Option<(&'a str, bool)> {
        if let Some(cargo) = &entity.cargo {
            let capacity = self.unit_type(entity.unit_type)?.worker.as_ref()?.capacity;
            return (cargo.amount > 0).then_some((cargo.kind.as_str(), cargo.amount >= capacity));
        }
        self.appearance(entity.id)?
            .carried
            .as_ref()
            .map(|(kind, full)| (kind.as_str(), *full))
    }

    pub fn is_landing(&self, entity: &Entity) -> bool {
        self.appearance(entity.id).map_or_else(
            || matches!(entity.order, UnitOrder::Land { .. }),
            |a| a.landing,
        )
    }

    pub fn transformation_progress(&self, entity: &Entity) -> Option<u8> {
        if let Some(appearance) = self.appearance(entity.id) {
            return appearance.transformation;
        }
        let job = entity.production.front()?;
        (job.started && job.producer_type.is_some()).then(|| {
            (u64::from(job.total.saturating_sub(job.remaining)) * 100 / u64::from(job.total.max(1)))
                as u8
        })
    }

    fn observable_appearance(&self, entity: &Entity) -> Appearance {
        let work_target = match entity.order {
            UnitOrder::Gather { resource } if entity.harvest_progress > 0 => self
                .state
                .resources
                .iter()
                .find(|r| r.id == resource)
                .map(|r| r.position),
            UnitOrder::Build { building } => self
                .state
                .entities
                .iter()
                .find(|e| {
                    e.id == building
                        && e.construction
                            .as_ref()
                            .is_some_and(|job| job.worker == Some(entity.id) && job.work_ticks > 0)
                })
                .map(|e| e.position),
            UnitOrder::Repair { target } if entity.repair_progress > 0 => self
                .state
                .entities
                .iter()
                .find(|e| e.id == target)
                .map(|e| e.position),
            _ => None,
        };
        Appearance {
            transformation: self.transformation_progress(entity),
            powered: Some(self.powered(entity)),
            landing: matches!(entity.order, UnitOrder::Land { .. }),
            shot_heading: entity
                .last_attack_position
                .map(|p| public_heading(entity.position, p)),
            work_heading: work_target.map(|p| public_heading(entity.position, p)),
            carried: self
                .carried_appearance(entity)
                .map(|(kind, full)| (kind.to_owned(), full)),
        }
    }
    /// The perspective is presentation metadata, never authoritative state.
    pub fn view_player(&self) -> PlayerId {
        self.view.as_ref().map_or(PlayerId(0), |view| view.player)
    }

    pub fn is_player_view(&self) -> bool {
        self.view.is_some()
    }

    pub fn disclosed_death(&self, id: EntityId) -> bool {
        self.view
            .as_ref()
            .is_none_or(|view| view.removed.contains(&id))
    }

    pub fn player_view(&self, player: PlayerId) -> Result<PlayerView> {
        ensure!(
            self.view.is_none(),
            "cannot project another player's view from a client view"
        );
        ensure!(player.0 < self.map.players, "unknown view player");
        let visible: BTreeSet<_> = self
            .state
            .entities
            .iter()
            .filter(|entity| self.entity_visible(player, entity.id))
            .map(|entity| entity.id)
            .collect();
        let entities = self
            .state
            .entities
            .iter()
            .filter_map(|entity| {
                if entity.owner == player {
                    let mut own = entity.clone();
                    // Retaliation/search state can refer to an unseen attacker.
                    // It is server implementation state, not owned-unit UI data.
                    own.retaliation_position = None;
                    own.route_wait = None;
                    own.path_geometry = [0; 32];
                    own.harvest_spot = None;
                    own.gather_origin = None;
                    own.strikes.clear();
                    own.auto_attack_target =
                        own.auto_attack_target.filter(|id| visible.contains(id));
                    own.last_attack_target =
                        own.last_attack_target.filter(|id| visible.contains(id));
                    return Some(ViewedEntity::Owned(Box::new(own)));
                }
                visible.contains(&entity.id).then(|| {
                    ViewedEntity::Visible(PublicEntity {
                        id: entity.id,
                        owner: entity.owner,
                        unit_type: entity.unit_type,
                        position: entity.position,
                        hp: entity.hp,
                        shields: entity.shields,
                        carried_by: entity.carried_by.filter(|id| visible.contains(id)),
                        invincible: entity.invincible,
                        cloaked: entity.cloaked,
                        airborne: entity.airborne,
                        flight_transition: entity.flight_transition,
                        doodad_enabled: entity.doodad_enabled,
                        parent: entity.parent.filter(|id| visible.contains(id)),
                        construction: entity
                            .construction
                            .as_ref()
                            .map(|job| [job.remaining, job.total]),
                        working: self.entity_working(entity.id),
                        cooldown: entity.cooldown,
                        targets_air: entity.last_attack_air,
                        shot_position: entity.last_attack_position.filter(|&position| {
                            self.visibility(player, position) == Visibility::Visible
                        }),
                        appearance: self.observable_appearance(entity),
                        mine: entity.mine_state.as_ref().map(|m| (m.phase, m.remaining)),
                    })
                })
            })
            .collect();
        let own = usize::from(player.0);
        Ok(PlayerView {
            tick: self.tick(),
            player,
            home: self
                .map
                .start_locations
                .iter()
                .find(|s| s.player == player)
                .map(|s| s.position),
            creation: self.map.creation.get(&player).cloned(),
            entities,
            economy: self.state.players[own].clone(),
            resources: self
                .state
                .resources
                .iter()
                .filter(|node| {
                    self.visibility(player, node.position) == Visibility::Visible
                        || (self.tick().0 == 0
                            && self.visibility(player, node.position) == Visibility::Explored)
                })
                .cloned()
                .collect(),
            fog: self.state.fog.get(own).cloned().unwrap_or_default(),
            terrain_fog: self.state.terrain_fog.get(own).cloned().unwrap_or_default(),
            creep: self.state.creep_seen.get(own).cloned().unwrap_or_default(),
            scans: self
                .state
                .scans
                .iter()
                .filter(|scan| {
                    scan.owner == player
                        || self.terrain_visibility(player, scan.position) == Visibility::Visible
                })
                .cloned()
                .collect(),
            winner: self.state.winner,
            defeated: self.state.defeated.clone(),
            mission: self.state.mission.as_ref().map(|mission| MissionView {
                countdown_ms: mission.countdown_ms,
                paused: mission.paused,
                events: if self
                    .map
                    .mission
                    .as_ref()
                    .is_some_and(|m| m.player == player)
                {
                    mission.events.clone()
                } else {
                    Vec::new()
                },
            }),
            removed: Vec::new(),
            shots: Vec::new(),
            weapon_feedback: self
                .weapon_feedback
                .iter()
                .filter(|(observers, _)| observers.contains(&player))
                .map(|(_, impact)| impact.clone())
                .collect(),
        })
    }
}

impl PlayerView {
    /// Reuse existing presentation queries on a read-only, filtered world. The
    /// immutable content definitions are local package data, never server state.
    pub fn into_world(self, definitions: &World) -> Result<World> {
        let map = &definitions.map;
        ensure!(self.player.0 < map.players, "unknown view player");
        let cells = ((map.width + 31) / 32 * ((map.height + 31) / 32)) as usize;
        ensure!(
            self.entities.len() <= 4096
                && self.resources.len() <= 4096
                && self.removed.len() <= 4096,
            "player view exceeds entity limits"
        );
        ensure!(
            (!map.fog_of_war && self.fog.is_empty() && self.terrain_fog.is_empty())
                || (self.fog.len() == cells
                    && self.terrain_fog.len() == cells
                    && self.terrain_fog.iter().all(|v| *v <= 2)),
            "invalid view fog dimensions"
        );
        ensure!(
            self.creep.is_empty()
                || (self.creep.len() == cells && self.creep.iter().all(|v| *v <= 1)),
            "invalid view creep"
        );
        ensure!(
            self.mission.as_ref().is_none_or(|m| m.events.len() <= 4096),
            "too many mission events"
        );
        ensure!(self.shots.len() <= 4096, "too many public weapon events");
        ensure!(
            self.weapon_feedback.len() <= 4096
                && self.weapon_feedback.iter().all(|impact| {
                    definitions.unit_type(impact.weapon).is_some()
                        && impact.position.x >= 0
                        && impact.position.x < map.width
                        && impact.position.y >= 0
                        && impact.position.y < map.height
                }),
            "invalid public impact events"
        );
        let mut working = BTreeSet::new();
        let mut appearance = BTreeMap::new();
        let mut entities = Vec::with_capacity(self.entities.len());
        let mut ids = BTreeSet::new();
        for viewed in self.entities {
            let entity = match viewed {
                ViewedEntity::Owned(entity) => {
                    ensure!(
                        entity.owner == self.player,
                        "private entity belongs to another player"
                    );
                    *entity
                }
                ViewedEntity::Visible(public) => {
                    ensure!(
                        public
                            .appearance
                            .carried
                            .as_ref()
                            .is_none_or(|(kind, _)| kind.len() <= 64),
                        "invalid carried appearance"
                    );
                    appearance.insert(public.id, public.appearance);
                    ensure!(
                        public.owner != self.player,
                        "own entity lacks private state"
                    );
                    if public.working {
                        working.insert(public.id);
                    }
                    Entity {
                        id: public.id,
                        owner: public.owner,
                        unit_type: public.unit_type,
                        position: public.position,
                        hp: public.hp,
                        shields: public.shields,
                        carried_by: public.carried_by,
                        invincible: public.invincible,
                        cloaked: public.cloaked,
                        airborne: public.airborne,
                        flight_transition: public.flight_transition,
                        doodad_enabled: public.doodad_enabled,
                        parent: public.parent,
                        construction: public.construction.map(|[remaining, total]| Construction {
                            worker: None,
                            remaining,
                            total,
                            work_position: None,
                            work_ticks: 0,
                        }),
                        cooldown: public.cooldown,
                        last_attack_air: public.targets_air,
                        last_attack_position: public.shot_position,
                        mine_state: public.mine.map(|(phase, remaining)| MineState {
                            phase,
                            remaining,
                            target: None,
                        }),
                        ..Entity::default()
                    }
                }
            };
            ensure!(
                entity.owner.0 < map.players
                    && map.contains(entity.position)
                    && definitions.unit_type(entity.unit_type).is_some()
                    && entity.id.0 != 0
                    && ids.insert(entity.id),
                "invalid view entity"
            );
            entities.push(entity);
        }
        entities.sort_by_key(|entity| entity.id);
        let own = usize::from(self.player.0);
        let mut players = vec![
            PlayerState {
                resources: BTreeMap::new(),
                completed_research: BTreeSet::new()
            };
            usize::from(map.players)
        ];
        players[own] = self.economy;
        let mut fog = if map.fog_of_war {
            vec![vec![0; cells]; players.len()]
        } else {
            Vec::new()
        };
        let mut terrain_fog = fog.clone();
        if map.fog_of_war {
            fog[own] = self.fog;
            terrain_fog[own] = self.terrain_fog;
        }
        let mut creep_seen = vec![Vec::new(); players.len()];
        creep_seen[own] = self.creep.clone();
        let mission = self.mission.map(|m| MissionState {
            rescue_players: Vec::new(),
            triggers: Vec::new(),
            switches: Vec::new(),
            locations: Vec::new(),
            countdown_ms: m.countdown_ms,
            poll_remaining: 0,
            wait: None,
            paused: m.paused,
            events: m.events,
        });
        let public_map = if definitions.is_player_view()
            && definitions.view_player() == self.player
            && map.start_locations.first().map(|s| s.position) == self.home
            && map.creation.get(&self.player) == self.creation.as_ref()
        {
            Arc::clone(map)
        } else {
            let mut map = (**map).clone();
            map.spawns.clear();
            map.ai.clear();
            map.resources.clear();
            map.initial_explored.clear();
            map.start_locations.clear();
            if let Some(position) = self.home {
                map.start_locations.push(StartLocation {
                    player: self.player,
                    position,
                });
            }
            map.creation.clear();
            if let Some(creation) = &self.creation {
                map.creation.insert(self.player, creation.clone());
            }
            if let Some(mission) = &mut map.mission {
                mission.triggers.clear();
                mission.locations.clear();
                mission.rescuable_players.clear();
                mission.rescuers.clear();
                mission.poll_ticks = 1;
                mission.wait_step_ms = 1;
            }
            Arc::new(map)
        };
        let mut world = World {
            rules: Arc::clone(&definitions.rules),
            map: public_map,
            rules_hash: definitions.rules_hash,
            map_hash: definitions.map_hash,
            state: State {
                statistics: Vec::new(),
                tick: self.tick,
                rng_state: 0,
                next_entity_id: 0,
                last_sequences: Vec::new(),
                kills: BTreeMap::new(),
                deaths: BTreeMap::new(),
                ai: Vec::new(),
                entities,
                players,
                resources: self.resources,
                winner: self.winner,
                defeated: self.defeated,
                mission,
                fog,
                terrain_fog,
                scans: self.scans,
                creep: self.creep,
                creep_seen,
            },
            vision_cells: BTreeMap::new(),
            navigation_geometry: Vec::new(),
            navigation_geometry_hash: [0; 32],
            weapon_feedback: Vec::new(),
            view: Some(ViewMetadata {
                player: self.player,
                working,
                removed: self.removed.into_iter().collect(),
                appearance,
                shots: self.shots,
                weapon_feedback: self.weapon_feedback,
            }),
        };
        // Owned addon work animation is also public; reuse the existing query
        // before installing the public flags, without adding private enemy jobs.
        let meta = world.view.take().unwrap();
        let owned_work: Vec<_> = world
            .state
            .entities
            .iter()
            .filter(|e| e.owner == self.player && world.entity_working(e.id))
            .map(|e| e.id)
            .collect();
        world.view = Some(meta);
        world.view.as_mut().unwrap().working.extend(owned_work);
        if definitions.is_player_view() && definitions.view_player() == self.player {
            let current: BTreeSet<_> = world.state.resources.iter().map(|r| r.id).collect();
            let remembered: Vec<_> = definitions
                .state
                .resources
                .iter()
                .filter(|node| {
                    !current.contains(&node.id)
                        && world.visibility(self.player, node.position) != Visibility::Visible
                })
                .cloned()
                .collect();
            world.state.resources.extend(remembered);
            world.state.resources.sort_by_key(|r| r.id);
        }
        Ok(world)
    }
}
