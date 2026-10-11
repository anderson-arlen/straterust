//! Last observed structure bodies are presentation knowledge, never live targets.
use super::*;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RememberedStructure {
    pub id: EntityId,
    pub owner: PlayerId,
    pub unit_type: UnitTypeId,
    pub position: Position,
    pub hp: u32,
    pub construction: Option<[u32; 2]>,
    pub doodad_enabled: Option<bool>,
}

impl RememberedStructure {
    /// An isolated body for drawing; never insert it into the live entity list.
    pub fn body(&self) -> Entity {
        Entity {
            id: self.id,
            owner: self.owner,
            unit_type: self.unit_type,
            position: self.position,
            hp: self.hp,
            doodad_enabled: self.doodad_enabled,
            construction: self.construction.map(|[remaining, total]| Construction {
                worker: None,
                remaining,
                total,
                work_position: None,
                work_ticks: 0,
            }),
            ..Default::default()
        }
    }

    pub(super) fn valid(&self, world: &World, player: PlayerId) -> bool {
        self.id.0 > 0
            && self.owner != player
            && self.owner.0 < world.map.players
            && world.map.contains(self.position)
            && world.unit_type(self.unit_type).is_some_and(|u| u.structure)
            && self.hp > 0
            && self
                .construction
                .is_none_or(|[remaining, total]| total > 0 && remaining <= total)
    }
}

impl World {
    pub fn remembered_structures(
        &self,
        player: PlayerId,
    ) -> impl Iterator<Item = &RememberedStructure> {
        self.state
            .structure_memory
            .get(usize::from(player.0))
            .into_iter()
            .flatten()
            .filter(move |memory| self.visibility(player, memory.position) == Visibility::Explored)
    }

    pub(super) fn remember_structures(&mut self) {
        if !self.map.fog_of_war {
            self.state.structure_memory.clear();
            return;
        }
        let mut layers = std::mem::take(&mut self.state.structure_memory);
        layers.resize_with(usize::from(self.map.players), Vec::new);
        for (index, memory) in layers.iter_mut().enumerate() {
            let player = PlayerId(index as u16);
            let visible: BTreeSet<_> = self
                .state
                .entities
                .iter()
                .filter(|entity| {
                    self.unit_type(entity.unit_type)
                        .expect("validated type")
                        .structure
                        && self.entity_visible_to(player, entity)
                })
                .map(|entity| entity.id)
                .collect();
            // Seeing an empty location clears its old body. Looking elsewhere
            // must not disclose whether that structure still exists or its HP.
            memory.retain(|old| {
                self.visibility(player, old.position) != Visibility::Visible
                    && !visible.contains(&old.id)
            });
            memory.extend(
                self.state
                    .entities
                    .iter()
                    .filter(|entity| {
                        entity.owner != player
                            && entity.hp > 0
                            && !entity.airborne
                            && visible.contains(&entity.id)
                    })
                    .map(|entity| RememberedStructure {
                        id: entity.id,
                        owner: entity.owner,
                        unit_type: entity.unit_type,
                        position: entity.position,
                        hp: entity.hp,
                        construction: entity.construction.as_ref().map(|c| [c.remaining, c.total]),
                        doodad_enabled: entity.doodad_enabled,
                    }),
            );
            memory.sort_by_key(|body| body.id);
        }
        self.state.structure_memory = layers;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn world() -> World {
        World::new(
            Rules {
                id: "structure-memory".into(),
                victory: false,
                units: vec![
                    UnitType {
                        vision_range: 64,
                        weapon: None,
                        ..Default::default()
                    },
                    UnitType {
                        id: UnitTypeId(2),
                        structure: true,
                        speed: 0,
                        vision_range: 0,
                        max_hp: 100,
                        weapon: None,
                        manual_target_only: true,
                        ..Default::default()
                    },
                ],
                ..Default::default()
            },
            Map {
                id: "structure-memory".into(),
                width: 512,
                height: 256,
                players: 2,
                fog_of_war: true,
                terrain: None,
                start_locations: Vec::new(),
                resources: Vec::new(),
                mission: None,
                ai: Vec::new(),
                creation: Default::default(),
                initial_explored: Default::default(),
                spawns: vec![
                    Spawn {
                        position: Position { x: 128, y: 128 },
                        ..Default::default()
                    },
                    Spawn {
                        owner: PlayerId(1),
                        unit_type: UnitTypeId(2),
                        position: Position { x: 176, y: 128 },
                        ..Default::default()
                    },
                    Spawn {
                        owner: PlayerId(1),
                        unit_type: UnitTypeId(2),
                        position: Position { x: 400, y: 128 },
                        ..Default::default()
                    },
                    Spawn {
                        owner: PlayerId(1),
                        position: Position { x: 144, y: 160 },
                        ..Default::default()
                    },
                ],
            },
            7,
        )
        .unwrap()
    }

    #[test]
    fn last_seen_structures_survive_fog_hidden_destruction_and_checkpoint() {
        let mut server = world();
        assert!(server.entity_visible(PlayerId(0), EntityId(2)));
        assert!(!server.entity_visible(PlayerId(0), EntityId(3)));
        server.state.entities[0].position = Position { x: 32, y: 128 };
        server.step(&[]).unwrap();
        let view = server.player_view(PlayerId(0)).unwrap();
        assert_eq!(view.remembered_structures.len(), 1);
        let memory = view.remembered_structures[0].clone();
        assert_eq!((memory.id, memory.hp), (EntityId(2), 100));
        assert!(
            server
                .player_view(PlayerId(1))
                .unwrap()
                .remembered_structures
                .is_empty()
        );
        let client = view.into_world(&server).unwrap();
        assert!(
            client
                .state
                .entities
                .iter()
                .all(|entity| entity.id != memory.id),
            "memories must not be live targets"
        );
        assert!(!client.entity_visible(PlayerId(0), memory.id));
        assert_eq!(
            client.remembered_structures(PlayerId(0)).next(),
            Some(&memory)
        );
        assert!(!client.is_enemy_entity(PlayerId(0), &client.state.entities[0]));

        server.state.entities[1].hp = 20;
        server.step(&[]).unwrap();
        assert_eq!(
            server
                .player_view(PlayerId(0))
                .unwrap()
                .remembered_structures,
            std::slice::from_ref(&memory)
        );
        server.state.entities.retain(|e| e.id != memory.id);
        server.step(&[]).unwrap();
        assert_eq!(
            server
                .player_view(PlayerId(0))
                .unwrap()
                .remembered_structures,
            std::slice::from_ref(&memory)
        );
        let snapshot = server.save_snapshot().unwrap();
        let serialized = ron::ser::to_string(&snapshot).unwrap();
        let mut restored = world()
            .restore_snapshot(ron::from_str(&serialized).unwrap())
            .unwrap();
        assert_eq!(
            restored
                .player_view(PlayerId(0))
                .unwrap()
                .remembered_structures,
            [memory]
        );
        restored.state.entities[0].position = Position { x: 128, y: 128 };
        restored.step(&[]).unwrap();
        assert!(
            restored
                .player_view(PlayerId(0))
                .unwrap()
                .remembered_structures
                .is_empty()
        );
        assert!(
            !restored.entity_visible(PlayerId(0), EntityId(3)),
            "unvisited walls remain private"
        );
    }

    #[test]
    fn older_states_default_to_empty_memory_and_only_seed_currently_visible_bodies() {
        let server = world();
        let mut state = server.state.clone();
        state.structure_memory.clear();
        let text = ron::ser::to_string(&state).unwrap();
        assert!(!text.contains("structure_memory"));
        let old: State = ron::from_str(&text).unwrap();
        assert!(old.structure_memory.is_empty());
        let mut old_server = server.snapshot();
        old_server.state = old;
        let restored = server
            .restore_snapshot(old_server.save_snapshot().unwrap())
            .unwrap();
        assert_eq!(restored.state.structure_memory[0].len(), 1);
        assert_eq!(restored.state.structure_memory[0][0].id, EntityId(2));
        assert_eq!(
            restored.state_hash(),
            old_server.state_hash(),
            "cosmetic memory is not gameplay"
        );
        let mut invalid = restored.player_view(PlayerId(0)).unwrap();
        invalid.remembered_structures = vec![RememberedStructure {
            unit_type: UnitTypeId(1),
            ..restored.state.structure_memory[0][0].clone()
        }];
        assert!(
            invalid.into_world(&server).is_err(),
            "mobile memories must be rejected"
        );
    }
}
