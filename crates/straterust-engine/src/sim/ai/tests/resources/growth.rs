use super::*;
use crate::multiplayer::{AiDifficulty, GameType, MatchRules};

fn growing_town(difficulty: AiDifficulty) -> World {
    let original = town(false, 100_000);
    let mut rules = original.rules().clone();
    rules.units[2].supply_provided = 200;
    rules.supply_limit = 200;
    let mut map = original.map().clone();
    map.width = 4096;
    map.height = 1536;
    map.spawns.last_mut().unwrap().position.x = 3584;
    map.resources
        .extend([(1984, 768), (2752, 1024)].map(|(x, y)| {
            let mut node = map.resources[0].clone();
            node.position = Position { x, y };
            node
        }));
    map.resources[1].amount = 100_000;
    map.multiplayer = Some(MatchRules {
        participants: vec![PlayerId(0), PlayerId(1)],
        teams: vec![],
        mode: GameType::FreeForAll,
        parameter: 1,
        resources: BTreeMap::new(),
        bases: vec![UnitTypeId(3)],
        cheats: false,
        ai_difficulty: Some(difficulty),
        flags: vec![],
        defeat_without_structures: false,
    });
    World::new(rules, map, 42).unwrap()
}

fn advance(world: &mut World, ticks: usize) {
    for _ in 0..ticks {
        world.step(&[]).unwrap();
    }
}

#[test]
fn normal_operates_two_healthy_mines_and_replaces_an_exhausted_one() {
    let mut world = growing_town(AiDifficulty::Normal);
    // Expansion must save up while military/extra workers also want funding.
    world.state.players[1]
        .resources
        .insert("minerals".into(), 100);
    advance(&mut world, 2000);
    let sites = world.ai_mining_sites(&world.map.ai[0], "minerals");
    assert_eq!(
        sites.len(),
        2,
        "{sites:?} {:?}",
        world.state.ai[0].expansion
    );
    assert!(sites.iter().all(|(_, workers)| *workers >= 2), "{sites:?}");
    assert_eq!(bases(&world), 2);
    assert!(world.state.resources[0].amount > 50_000);
    assert!(world.state.resources[1].amount < 100_000);
    assert!(
        world.state.resources[2..]
            .iter()
            .all(|r| r.amount == 100_000)
    );
    world.state.resources[0].amount = 0;
    world.collision.resource_changed(ResourceId(1));
    for _ in 0..2500 {
        world.step(&[]).unwrap();
        assert!(world.ai_mining_sites(&world.map.ai[0], "minerals").len() <= 2);
    }
    assert!(
        world.state.resources[2..]
            .iter()
            .any(|r| r.amount < 100_000)
    );
    assert_eq!(bases(&world), 3, "{:?}", world.state.ai[0].expansion);
}

#[test]
fn hard_adds_workers_and_more_than_two_healthy_mining_operations() {
    let mut world = growing_town(AiDifficulty::Hard);
    advance(&mut world, 3500);
    let sites = world.ai_mining_sites(&world.map.ai[0], "minerals");
    assert_eq!(
        sites.len(),
        4,
        "{sites:?} {:?}",
        world.state.ai[0].expansion
    );
    assert!(sites.iter().all(|(_, workers)| *workers >= 2), "{sites:?}");
    assert!(
        world
            .state
            .resources
            .iter()
            .all(|r| (50_000..100_000).contains(&r.amount))
    );
    assert!(bases(&world) >= 3);
    assert!(
        world
            .state
            .entities
            .iter()
            .filter(|e| e.owner == PlayerId(1) && e.unit_type == UnitTypeId(2))
            .count()
            > 16
    );
}

#[test]
fn normal_counts_an_outdoor_mineral_field_as_one_operation() {
    let original = growing_town(AiDifficulty::Normal);
    let mut rules = original.rules().clone();
    rules.units[1].harvest_profiles[0].inside = false;
    rules.units[1].harvest_profiles[0].depot_inside = false;
    let mut map = original.map().clone();
    let mut patch = map.resources[0].clone();
    patch.position.y += 96;
    map.resources.push(patch);
    let mut world = World::new(rules, map, 42).unwrap();
    world.state.entities[1].order = UnitOrder::Gather {
        resource: ResourceId(1),
    };
    world.state.entities[2].order = UnitOrder::Gather {
        resource: ResourceId(2),
    };
    // The distant second operation already has a finished depot.
    let mut base = world.state.entities[0].clone();
    base.id = EntityId(100);
    base.position = Position { x: 1088, y: 512 };
    world.state.entities.push(base);
    world.collision.changed(EntityId(100));
    let actor = &world.state.entities[3];
    let allowed = world.ai_allowed_mines(
        &world.map.ai[0],
        &world.state.ai[0],
        actor,
        &[ResourceId(3), ResourceId(5)],
    );
    assert_eq!(allowed, vec![ResourceId(5)]);
    assert_eq!(world.ai_mining_sites(&world.map.ai[0], "minerals").len(), 2);
}

#[test]
fn difficulty_is_saved_hashed_and_optional_for_older_matches() {
    let world = growing_town(AiDifficulty::Normal);
    let encoded = ron::to_string(world.map.multiplayer.as_ref().unwrap()).unwrap();
    let old = encoded.replace("ai_difficulty:Some(Normal),", "");
    assert!(
        ron::from_str::<MatchRules>(&old)
            .unwrap()
            .ai_difficulty
            .is_none()
    );
    let mut hard = world.map().clone();
    hard.multiplayer.as_mut().unwrap().ai_difficulty = Some(AiDifficulty::Hard);
    assert_ne!(hash_map(&world.map), hash_map(&hard));
    let mut running = world;
    advance(&mut running, 128);
    let encoded = ron::to_string(&running.save_snapshot().unwrap()).unwrap();
    let mut restored = running
        .restore_snapshot(ron::from_str(&encoded).unwrap())
        .unwrap();
    for _ in 0..256 {
        running.step(&[]).unwrap();
        restored.step(&[]).unwrap();
        assert_eq!(running.state_hash(), restored.state_hash());
    }
}

#[test]
fn busy_woodcutters_resume_gold_at_a_second_mine_in_their_existing_base() {
    for difficulty in [AiDifficulty::Easy, AiDifficulty::Normal, AiDifficulty::Hard] {
        let original = growing_town(difficulty);
        let mut rules = original.rules().clone();
        rules.units[1]
            .worker
            .as_mut()
            .unwrap()
            .resource_kinds
            .push("wood".into());
        let mut wood_profile = rules.units[1].harvest_profiles[0].clone();
        wood_profile.kind = "wood".into();
        wood_profile.inside = false;
        wood_profile.depot_inside = false;
        rules.units[1].harvest_profiles.push(wood_profile);
        rules.units[2].dropoff.push("wood".into());
        let mut map = original.map().clone();
        map.resources.truncate(2);
        map.resources[0].amount = 1;
        map.resources[1].position = Position { x: 448, y: 768 };
        let mut wood = map.resources[1].clone();
        wood.kind = "wood".into();
        wood.position.y = 256;
        map.resources.push(wood);
        let mut world = World::new(rules, map, 42).unwrap();
        world.state.resources[0].amount = 0;
        world.collision.resource_changed(ResourceId(1));
        // Reproduce the exhausted economy: nobody idle, no outstanding unit
        // deficit, and a rich second mine that needs no expansion building.
        for worker in world
            .state
            .entities
            .iter_mut()
            .filter(|e| e.owner == PlayerId(1) && e.unit_type == UnitTypeId(2))
        {
            worker.order = UnitOrder::Gather {
                resource: ResourceId(3),
            };
        }
        advance(&mut world, 512);
        assert!(world.state.resources[1].amount < 100_000, "{difficulty:?}");
        assert!(
            world.state.statistics[1]
                .resources_collected
                .get("minerals")
                .copied()
                .unwrap_or(0)
                > 0,
            "{difficulty:?}"
        );
        assert_eq!(bases(&world), 1, "the second mine already has a depot");
    }
}
