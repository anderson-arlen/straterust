use super::*;

fn guards() -> World {
    let base = economy();
    let mut rules = base.rules().clone();
    for u in &mut rules.units {
        u.vision_range = 32;
    }
    let mut map = base.map().clone();
    map.ai[0].active = false;
    map.ai[0].program.clear();
    map.ai[0].home = Position { x: 640, y: 640 };
    map.spawns = vec![
        Spawn {
            owner: PlayerId(0),
            unit_type: UnitTypeId(1),
            position: Position { x: 384, y: 640 },
            ..Default::default()
        },
        Spawn {
            owner: PlayerId(1),
            unit_type: UnitTypeId(2),
            position: Position { x: 576, y: 640 },
            ..Default::default()
        },
        Spawn {
            owner: PlayerId(1),
            unit_type: UnitTypeId(1),
            position: Position { x: 640, y: 640 },
            ..Default::default()
        },
    ];
    map.resources.clear();
    map.fog_of_war = true;
    World::new(rules, map, 42).unwrap()
}

#[test]
fn ai_hit_workers_and_unarmed_buildings_call_nearby_defenders() {
    for unit_type in [2, 3] {
        let mut world = guards();
        world.state.entities[1].unit_type = UnitTypeId(unit_type);
        world.state.entities[1].order = UnitOrder::Gather {
            resource: ResourceId(1),
        };
        assert!(!world.entity_visible(PlayerId(1), EntityId(1)));
        world.ai_help_on_damage(1, &BTreeMap::from([(EntityId(1), 256)]));
        assert_eq!(
            world.state.entities[2].auto_attack_target,
            Some(EntityId(1))
        );
        assert_eq!(
            world.state.entities[2].retaliation_position,
            Some(Position { x: 384, y: 640 })
        );
        let origin = world.state.entities[2].retaliation_position;
        world.state.entities[0].position.y += 64;
        world.automatic_target(2);
        assert_eq!(
            world.state.entities[2].retaliation_position, origin,
            "unseen attackers must not be tracked"
        );
    }
}

#[test]
fn ai_help_respects_attack_capability_cloak_human_ownership_and_guard_leash() {
    let mut world = guards();
    world.state.entities[0].cloaked = true;
    world.ai_help_on_damage(1, &BTreeMap::from([(EntityId(1), 256)]));
    assert_eq!(world.state.entities[2].auto_attack_target, None);
    world.state.entities[0].cloaked = false;
    world.state.entities[0].airborne = true;
    // This fixture weapon is ground-only.
    world.ai_help_on_damage(1, &BTreeMap::from([(EntityId(1), 256)]));
    assert_eq!(world.state.entities[2].auto_attack_target, None);
    world.state.entities[0].airborne = false;
    world.state.ai[0]
        .guards
        .insert(EntityId(3), Position { x: 1024, y: 640 });
    world.ai_help_on_damage(1, &BTreeMap::from([(EntityId(1), 256)]));
    assert_eq!(world.state.entities[2].auto_attack_target, None);
    world.state.ai[0].guards.clear();
    world.state.entities[2].owner = PlayerId(0);
    world.ai_help_on_damage(1, &BTreeMap::from([(EntityId(1), 256)]));
    assert_eq!(world.state.entities[2].auto_attack_target, None);
}

#[test]
fn ai_burrowed_helpers_emerge_and_dying_victims_still_call_for_help() {
    let mut world = guards();
    world.state.entities[1].hp = 0;
    world.state.entities[2].burrowed = true;
    world.ai_help_on_damage(1, &BTreeMap::from([(EntityId(1), 256)]));
    assert!(!world.state.entities[2].burrowed);
    assert_eq!(
        world.state.entities[2].auto_attack_target,
        Some(EntityId(1))
    );
}
