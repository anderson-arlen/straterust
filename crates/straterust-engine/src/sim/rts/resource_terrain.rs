//! Authoritative removal of unsupported corner-based resource fragments.
use super::*;

impl World {
    pub(in crate::sim) fn reconnect_resource_terrain(&mut self) {
        let groups: BTreeSet<_> = self
            .map
            .resources
            .iter()
            .filter(|r| r.terrain_corners.is_some())
            .map(|r| (r.kind.clone(), i32::from(r.footprint.width)))
            .collect();
        for (kind, size) in groups {
            let original: BTreeMap<_, _> = self
                .map
                .resources
                .iter()
                .filter(|r| r.kind == kind && i32::from(r.footprint.width) == size)
                .filter_map(|r| {
                    r.terrain_corners
                        .map(|mask| ((r.position.x / size, r.position.y / size), mask))
                })
                .collect();
            let cleared: BTreeSet<_> = self
                .state
                .resources
                .iter()
                .filter(|r| r.kind == kind && r.amount == 0)
                .map(|r| (r.position.x / size, r.position.y / size))
                .collect();
            let patches = crate::map::reconnect_resource_corners(
                self.map.width / size,
                self.map.height / size,
                |x, y| original.get(&(x, y)).copied(),
                &cleared,
            );
            for node in &mut self.state.resources {
                if node.kind == kind
                    && patches.get(&(node.position.x / size, node.position.y / size)) == Some(&0)
                {
                    node.amount = 0;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        content::Package,
        session::{SavedGame, ServerSession},
    };

    fn world(corners: bool) -> World {
        let original = Package::load(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/terran-demo"),
        )
        .unwrap()
        .world(7)
        .unwrap();
        let mut rules = original.rules().clone();
        rules.victory = false;
        let worker = rules.units.iter_mut().find(|u| u.worker.is_some()).unwrap();
        let unit_type = worker.id;
        worker.harvest_profiles.clear();
        let stats = worker.worker.as_mut().unwrap();
        stats.capacity = 8;
        stats.harvest_amount = 8;
        stats.harvest_ticks = 1;
        stats.idle_resource_radius = 0;
        let mut map = original.map().clone();
        map.mission = None;
        map.ai.clear();
        map.terrain = None;
        map.fog_of_war = false;
        map.spawns = vec![Spawn {
            unit_type,
            position: Position { x: 272, y: 80 },
            ..Default::default()
        }];
        map.resources = [35, 63, 28]
            .into_iter()
            .enumerate()
            .map(|(i, mask)| ResourceSpawn {
                terrain_corners: corners.then_some(mask),
                kind: "minerals".into(),
                position: Position {
                    x: 336,
                    y: (i as i32 + 1) * 32 + 16,
                },
                amount: 8,
                footprint: Footprint {
                    width: 32,
                    height: 32,
                },
                requires_extractor: false,
            })
            .collect();
        map.resources.push(ResourceSpawn {
            terrain_corners: None,
            kind: "minerals".into(),
            position: Position { x: 48, y: 48 },
            amount: 99,
            footprint: Footprint {
                width: 32,
                height: 32,
            },
            requires_extractor: false,
        });
        World::new(rules, map, 7).unwrap()
    }

    #[test]
    fn harvesting_removes_unsupported_fragments_from_collision_and_player_updates() {
        let mut world = world(true);
        world
            .step(&[Command {
                tick: world.tick(),
                player: PlayerId(0),
                sequence: 1,
                order: Order::Gather {
                    entity: EntityId(1),
                    resource: ResourceId(2),
                },
            }])
            .unwrap();
        for _ in 0..100 {
            if world.state.resources[1].amount == 0 {
                break;
            }
            world.step(&[]).unwrap();
        }
        assert_eq!(
            world
                .state
                .resources
                .iter()
                .map(|r| r.amount)
                .collect::<Vec<_>>(),
            [0, 0, 0, 99]
        );
        assert_eq!(world.state.entities[0].cargo.as_ref().unwrap().amount, 8);
        assert!(world.can_place(
            Position { x: 336, y: 48 },
            world.unit_at(0).footprint,
            MovementClass::Ground,
            Some(EntityId(1))
        ));
        let view = world.player_view(PlayerId(0)).unwrap();
        assert_eq!(view.resources.iter().filter(|r| r.amount == 0).count(), 3);
        let resumed = world
            .restore_snapshot(world.save_snapshot().unwrap())
            .unwrap();
        assert_eq!(resumed.state_hash(), world.state_hash());
    }

    #[test]
    fn older_saves_keep_the_load_and_gain_current_resource_cleanup() {
        let mut old = world(false);
        old.state.resources[1].amount = 0;
        old.state.tick = Tick(342);
        old.state.entities[0].cargo = Some(ResourceAmount {
            kind: "minerals".into(),
            amount: 8,
        });
        let saved =
            SavedGame::capture(&ServerSession::new(old, 7, vec![PlayerId(0)]).unwrap()).unwrap();
        let bytes = saved.encode().unwrap();
        let current = world(true);
        assert!(ServerSession::restore(&current, saved.checkpoint.clone()).is_err());
        let resumed =
            ServerSession::restore_saved(&current, SavedGame::decode(&bytes).unwrap()).unwrap();
        assert_eq!(resumed.world().state.tick, Tick(342));
        assert_eq!(
            resumed.world().state.entities[0]
                .cargo
                .as_ref()
                .unwrap()
                .amount,
            8
        );
        assert_eq!(
            resumed
                .world()
                .state
                .resources
                .iter()
                .map(|r| r.amount)
                .collect::<Vec<_>>(),
            [0, 0, 0, 99]
        );
        assert_eq!(saved.encode().unwrap(), bytes);
    }
}
