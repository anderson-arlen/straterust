//! Focused checks against imported retail combat definitions.
use straterust_engine::sim::*;

#[test]
#[ignore = "requires STRATERUST_CAMPAIGNS pointing to a retail import"]
fn retail_three_powered_cannons_defeat_two_zerglings_without_losing_a_cannon() {
    use straterust_engine::content::Package;
    let root = std::env::var_os("STRATERUST_CAMPAIGNS").unwrap();
    let package = Package::load(&std::path::Path::new(&root).join("protoss/protoss01")).unwrap();
    let original = package.world(42).unwrap();
    let mut rules = original.rules().clone();
    rules.victory = false;
    let ling = rules.units.iter().find(|u| u.id == UnitTypeId(6)).unwrap();
    assert_eq!(
        (
            ling.max_hp,
            ling.weapon.as_ref().unwrap().damage,
            ling.weapon.as_ref().unwrap().cooldown
        ),
        (35, 5, 8)
    );
    let cannon = rules.units.iter().find(|u| u.id == UnitTypeId(96)).unwrap();
    assert_eq!((cannon.max_hp, cannon.max_shields), (100, 100));
    assert_eq!(
        (
            cannon.weapon.as_ref().unwrap().damage,
            cannon.weapon.as_ref().unwrap().cooldown
        ),
        (20, 22)
    );
    let mut map = original.map().clone();
    map.width = 1024;
    map.height = 768;
    map.terrain = None;
    map.mission = None;
    map.ai.clear();
    map.creation.clear();
    map.initial_explored.clear();
    map.resources.clear();
    map.start_locations.clear();
    map.fog_of_war = false;
    map.spawns = [
        (0, 93, 448, 320),
        (0, 96, 384, 256),
        (0, 96, 384, 384),
        (0, 96, 480, 256),
        (1, 6, 320, 256),
        (1, 6, 320, 272),
    ]
    .map(|(owner, kind, x, y)| Spawn {
        owner: PlayerId(owner),
        unit_type: UnitTypeId(kind),
        position: Position { x, y },
        ..Default::default()
    })
    .to_vec();
    let mut world = World::new(rules, map, 42).unwrap();
    for _ in 0..120 {
        world.step(&[]).unwrap();
    }
    assert!(
        world
            .state()
            .entities
            .iter()
            .all(|e| e.unit_type != UnitTypeId(6))
    );
    assert_eq!(
        world
            .state()
            .entities
            .iter()
            .filter(|e| e.unit_type == UnitTypeId(96))
            .count(),
        3
    );
}
