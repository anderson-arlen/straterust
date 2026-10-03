use super::tests::world;
use super::*;
use straterust_engine::sim::MovementClass;
#[test]
fn ground_weapon_visual_does_not_select_aircraft_during_cooldown() {
    let base = world();
    let mut rules = base.rules().clone();
    let mut flyer = rules.units[0].clone();
    flyer.id = UnitTypeId(2);
    flyer.movement_class = MovementClass::Air;
    rules.units.push(flyer);
    let mut map = base.map().clone();
    map.spawns[1].unit_type = UnitTypeId(2);
    map.spawns[1].position = Position { x: 52, y: 40 };
    let world = World::new(rules, map, 42).unwrap();
    let mut actor = world.state().entities[0].clone();
    actor.cooldown = 4;
    actor.auto_attack_target = Some(EntityId(2));
    assert_eq!(Visuals::new(&world).attack_target(&world, &actor), None);
}
