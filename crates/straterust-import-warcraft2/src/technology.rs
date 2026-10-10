//! Warcraft II's finite research graph, authored as native rule data.
use super::stats::id;
use straterust_engine::sim::*;

pub struct Technology {
    pub rule: Research,
    pub name: String,
    pub key: String,
    pub description: String,
}

pub fn definitions() -> Vec<Technology> {
    let mut result = Vec::new();
    for race in 0..2 {
        let unit = |n| id(n + race);
        let units = |sources: &[usize]| sources.iter().map(|s| unit(*s)).collect::<Vec<_>>();
        let melee = units(&[0, 6, 12, 14, 16]);
        let ranged = units(&[8, 18]);
        let warships = units(&[30, 32, 38]);
        let ships = units(&[28, 30, 32]);
        let mut ordinal = 0;
        let mut add = |facility,
                       previous,
                       prerequisites: Vec<UnitTypeId>,
                       name: &str,
                       key: &str,
                       cost: [u32; 3],
                       time: u32,
                       effect,
                       description: &str| {
            ordinal += 1;
            let research = ResearchId((race * 64 + ordinal) as u16);
            result.push(Technology {
                rule: Research {
                    available: true,
                    id: research,
                    facility: unit(facility),
                    previous,
                    prerequisites,
                    cost: ["gold", "wood", "oil"]
                        .into_iter()
                        .zip(cost)
                        .filter(|(_, amount)| *amount > 0)
                        .map(|(kind, amount)| ResourceAmount {
                            kind: kind.into(),
                            amount,
                        })
                        .collect(),
                    ticks: time * 6,
                    effect,
                },
                name: name.into(),
                key: key.into(),
                description: description.into(),
            });
            research
        };
        for (facility, name, key, first, second, targets, amount, armor) in [
            (
                82,
                if race == 0 { "Sword" } else { "Axe" },
                "W",
                if race == 0 {
                    [800, 0, 0]
                } else {
                    [500, 100, 0]
                },
                if race == 0 {
                    [2400, 0, 0]
                } else {
                    [1500, 300, 0]
                },
                melee.clone(),
                2,
                false,
            ),
            (
                82,
                "Shield",
                "S",
                [300, 300, 0],
                [900, 500, 0],
                melee.clone(),
                2,
                true,
            ),
            (
                76,
                if race == 0 { "Arrow" } else { "Throwing Axe" },
                "A",
                [300, 300, 0],
                [900, 500, 0],
                ranged.clone(),
                1,
                false,
            ),
            (
                78,
                "Ship Cannon",
                "C",
                [700, 100, 1000],
                [2000, 250, 3000],
                warships.clone(),
                5,
                false,
            ),
            (
                78,
                "Ship Armor",
                "A",
                [500, 500, 0],
                [1500, 900, 0],
                ships.clone(),
                5,
                true,
            ),
            (
                82,
                if race == 0 { "Ballista" } else { "Catapult" },
                "B",
                [1500, 0, 0],
                [4000, 0, 0],
                units(&[4]),
                15,
                false,
            ),
        ] {
            let effect = |units| {
                if armor {
                    ResearchEffect::Armor { units, amount }
                } else {
                    ResearchEffect::WeaponDamage { units, amount }
                }
            };
            let description = if armor {
                "Increases armor for all affected units."
            } else {
                "Increases piercing damage for all affected units."
            };
            let first_id = add(
                facility,
                None,
                Vec::new(),
                &format!("{name} +1"),
                key,
                first,
                200,
                effect(targets.clone()),
                description,
            );
            add(
                facility,
                Some(first_id),
                vec![unit(88)],
                &format!("{name} +2"),
                key,
                second,
                250,
                effect(targets),
                description,
            );
        }
        let ranger = add(
            76,
            None,
            vec![unit(88)],
            if race == 0 { "Ranger" } else { "Berserker" },
            "R",
            [1500, 0, 0],
            250,
            ResearchEffect::UnitUpgrade {
                units: units(&[8]),
                to: unit(18),
            },
            "Upgrades all existing and future archers or axethrowers.",
        );
        add(
            76,
            Some(ranger),
            Vec::new(),
            if race == 0 { "Longbow" } else { "Lighter Axes" },
            "L",
            [2000, 0, 0],
            250,
            ResearchEffect::WeaponRange {
                units: ranged.clone(),
                amount: 32,
                sight: 32,
            },
            "Extends ranged attack range and sight by one tile.",
        );
        add(
            76,
            Some(ranger),
            Vec::new(),
            "Scouting",
            "S",
            [1500, 0, 0],
            250,
            ResearchEffect::VisionRange {
                units: ranged.clone(),
                amount: 96,
            },
            "Extends sight by three tiles.",
        );
        add(
            76,
            Some(ranger),
            Vec::new(),
            if race == 0 {
                "Marksmanship"
            } else {
                "Regeneration"
            },
            "M",
            [if race == 0 { 2500 } else { 3000 }, 0, 0],
            250,
            if race == 0 {
                ResearchEffect::WeaponDamage {
                    units: ranged,
                    amount: 3,
                }
            } else {
                ResearchEffect::Regeneration {
                    units: ranged,
                    amount: 4,
                }
            },
            if race == 0 {
                "Increases piercing damage by three."
            } else {
                "Berserkers slowly regenerate health."
            },
        );
        add(
            62,
            None,
            vec![unit(90)],
            if race == 0 { "Paladin" } else { "Ogre-Mage" },
            "P",
            [1000, 0, 0],
            250,
            ResearchEffect::UnitUpgrade {
                units: units(&[6]),
                to: unit(12),
            },
            "Upgrades all existing and future cavalry, granting spellcasting.",
        );
    }
    result
}
