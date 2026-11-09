use super::{
    presentation,
    stats::{self, id},
    technology,
};
use straterust_engine::sim::*;

#[test]
fn both_building_trees_and_caster_buttons_match_native_dependencies() {
    let mut rules = Rules {
        units: (0..103)
            .map(|n| UnitType {
                id: id(n),
                structure: n >= 58,
                ..Default::default()
            })
            .collect(),
        ..Default::default()
    };
    stats::upgrade(&mut rules);
    let first = ron::ser::to_string(&rules).unwrap();
    stats::upgrade(&mut rules);
    assert_eq!(
        ron::ser::to_string(&rules).unwrap(),
        first,
        "upgrades must be idempotent"
    );
    // Every worker-built structure, including the structures with no dependencies.
    for race in 0..2 {
        for (source, kind) in [
            (74, "gold"),
            (88, "gold"),
            (90, "gold"),
            (72, "oil"),
            (84, "oil"),
        ] {
            let unit = rules
                .units
                .iter()
                .find(|unit| unit.id == id(source + race))
                .unwrap();
            assert_eq!(unit.resource_clearance, 96);
            assert_eq!(unit.resource_clearance_tile_size, 32);
            assert_eq!(unit.resource_clearance_kinds, [kind]);
        }
        for (source, needs) in [
            (58, vec![]),
            (60, vec![]),
            (62, vec![90]),
            (64, vec![]),
            (66, vec![88]),
            (68, vec![88]),
            (70, vec![90]),
            (72, vec![76]),
            (74, vec![]),
            (76, vec![]),
            (78, vec![72]),
            (80, vec![90]),
            (82, vec![]),
            (84, vec![72]),
            (86, vec![]),
            (88, vec![60]),
            (90, vec![66, 82, 76]),
            (96, vec![76]),
            (98, vec![82]),
        ] {
            let unit = rules
                .units
                .iter()
                .find(|u| u.id == id(source + race))
                .unwrap();
            assert_eq!(
                unit.prerequisites,
                needs.into_iter().map(|n| id(n + race)).collect::<Vec<_>>(),
                "building {source}, race {race}"
            );
        }
        let upgrade = technology::definitions()
            .into_iter()
            .find(|t| t.rule.id == ResearchId(17 + race as u16 * 64))
            .unwrap()
            .rule;
        assert_eq!(upgrade.facility, id(62 + race));
        assert_eq!(upgrade.prerequisites, vec![id(90 + race)]);
        assert_eq!(
            upgrade.effect,
            ResearchEffect::UnitUpgrade {
                units: vec![id(6 + race)],
                to: id(12 + race)
            }
        );
    }
    let names = vec!["Test".to_string(); 2048];
    let p = presentation::build(&rules, &names, "Test".into());
    assert!(super::descriptions::unit(2).unwrap().contains("constructs"));
    assert!(super::descriptions::unit(62).unwrap().contains("Paladins"));
    assert!(
        super::descriptions::unit(63)
            .unwrap()
            .contains("Ogre-Mages")
    );
    let value = ron::ser::to_string(&p).unwrap();
    let mut old: presentation::Presentation = ron::from_str(&value).unwrap();
    presentation::upgrade(&mut old);
    // Fresh imports and upgraded installations must produce identical commands.
    assert_eq!(ron::ser::to_string(&old).unwrap(), value);
}
