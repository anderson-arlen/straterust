//! Public map geometry, never initial placements, AI or executable scenarios.
use super::*;
use crate::map::{decode_terrain, encode_terrain};

impl World {
    pub(crate) fn empty_client(rules: Rules, map: Map) -> Result<Self> {
        let mut world = Self::initialize(rules, map, 0, true)?;
        world.state.last_sequences.clear();
        world.state.next_entity_id = 0;
        world
            .state
            .players
            .iter_mut()
            .for_each(|p| p.resources.clear());
        world.view = Some(ViewMetadata {
            player: PlayerId(0),
            working: BTreeSet::new(),
            removed: BTreeSet::new(),
            appearance: BTreeMap::new(),
            shots: Vec::new(),
        });
        Ok(world)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicMap {
    pub id: String,
    pub width: i32,
    pub height: i32,
    pub players: u16,
    pub fog_of_war: bool,
    pub alliances: Vec<[PlayerId; 2]>,
    pub terrain: Option<Vec<u8>>,
}

impl PublicMap {
    pub fn of(world: &World) -> Result<Self> {
        Ok(Self {
            id: world.map.id.clone(),
            width: world.map.width,
            height: world.map.height,
            players: world.map.players,
            fog_of_war: world.map.fog_of_war,
            alliances: world
                .map
                .mission
                .as_ref()
                .map_or_else(Vec::new, |m| m.alliances.clone()),
            terrain: world.map.terrain.as_ref().map(encode_terrain).transpose()?,
        })
    }

    /// Validate received geometry using the same native loader as local maps.
    /// No trigger/program fields exist in this type. The result cannot step.
    pub fn definitions(
        &self,
        rules: Rules,
        identity: &GameplayIdentity,
        player: PlayerId,
    ) -> Result<World> {
        ensure!(
            (16..=8192).contains(&self.width) && (16..=8192).contains(&self.height),
            "network map dimensions must be 16..=8192"
        );
        ensure!(
            (1..=32).contains(&self.players)
                && self.players == identity.players
                && player.0 < self.players,
            "invalid map player count"
        );
        ensure!(
            self.alliances.len() <= 1024
                && self.alliances.iter().flatten().all(|p| p.0 < self.players),
            "invalid public alliances"
        );
        ensure!(
            identity.map.len() == 64 && identity.map.bytes().all(|b| b.is_ascii_hexdigit()),
            "invalid scenario identity"
        );
        let map = Map {
            id: self.id.clone(),
            width: self.width,
            height: self.height,
            players: self.players,
            fog_of_war: self.fog_of_war,
            terrain: self.terrain.as_deref().map(decode_terrain).transpose()?,
            mission: (!self.alliances.is_empty()).then(|| Mission {
                schema_version: 1,
                player,
                rescuable_players: Vec::new(),
                rescuers: Vec::new(),
                alliances: self.alliances.clone(),
                poll_ticks: 1,
                wait_step_ms: 1,
                locations: Vec::new(),
                triggers: Vec::new(),
            }),
            spawns: Vec::new(),
            start_locations: Vec::new(),
            resources: Vec::new(),
            creation: BTreeMap::new(),
            initial_explored: BTreeMap::new(),
            ai: Vec::new(),
        };
        let mut world = World::initialize(rules, map, 0, true)?;
        ensure!(
            GameplayIdentity::of(&world).same_rules(identity),
            "downloaded map uses incompatible game rules"
        );
        world.map_hash = blake3::Hash::from_hex(&identity.map)?;
        world.state.last_sequences.clear();
        world.state.next_entity_id = 0;
        world
            .state
            .players
            .iter_mut()
            .for_each(|p| p.resources.clear());
        world.view = Some(ViewMetadata {
            player,
            working: BTreeSet::new(),
            removed: BTreeSet::new(),
            appearance: BTreeMap::new(),
            shots: Vec::new(),
        });
        Ok(world)
    }
}
