//! Original command keys and source icon assignments are package presentation data.
use super::{spells, stats::id, technology};
use serde::Serialize;
use std::collections::BTreeMap;
use straterust_engine::sim::*;

#[derive(Serialize)]
pub struct Presentation {
    schema_version: u32,
    ground: u32,
    grid: u32,
    friendly: u32,
    opposing: u32,
    unit_radius: u32,
    unit_names: BTreeMap<UnitTypeId, String>,
    objective: Option<String>,
    research_slots: BTreeMap<ResearchId, u8>,
    research_names: BTreeMap<ResearchId, String>,
    research_keys: BTreeMap<ResearchId, String>,
    research_descriptions: BTreeMap<ResearchId, String>,
    train_keys: BTreeMap<UnitTypeId, String>,
    train_slots: BTreeMap<UnitTypeId, u8>,
    build_buttons: BTreeMap<UnitTypeId, BuildButton>,
    command_buttons: BTreeMap<String, CommandButton>,
    command_keys: BTreeMap<String, String>,
    unit_commands: BTreeMap<UnitTypeId, BTreeMap<String, Option<u8>>>,
}
#[derive(Serialize)]
struct BuildButton {
    advanced: bool,
    slot: u8,
    key: String,
}
#[derive(Serialize)]
struct CommandButton {
    slot: u8,
    key: String,
    label: String,
    tip: String,
    icon: String,
}

pub fn build(rules: &Rules, names: &[String], objective: String) -> Presentation {
    let technologies = technology::definitions();
    let mut result = Presentation {
        schema_version: 1,
        ground: 0x243b24,
        grid: 0x324d32,
        friendly: 0x00ff00,
        opposing: 0xff0000,
        unit_radius: 12,
        objective: Some(
            objective
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .chars()
                .take(160)
                .collect(),
        ),
        unit_names: rules
            .units
            .iter()
            .map(|u| {
                (
                    u.id,
                    super::stats::unit_name(usize::from(u.id.0 - 1), names),
                )
            })
            .collect(),
        research_slots: BTreeMap::new(),
        research_names: technologies
            .iter()
            .map(|t| (t.rule.id, t.name.clone()))
            .collect(),
        research_keys: technologies
            .iter()
            .map(|t| (t.rule.id, t.key.clone()))
            .collect(),
        research_descriptions: technologies
            .iter()
            .map(|t| (t.rule.id, t.description.clone()))
            .collect(),
        train_keys: BTreeMap::new(),
        train_slots: BTreeMap::new(),
        build_buttons: BTreeMap::new(),
        command_buttons: BTreeMap::new(),
        command_keys: [("gather".into(), "H".into()), ("hold".into(), "T".into())].into(),
        unit_commands: BTreeMap::new(),
    };
    for unit in rules.units.iter().filter(|u| !u.structure) {
        let caster = matches!(unit.id.0 - 1, 10 | 11 | 21 | 24 | 51);
        result.unit_commands.insert(
            unit.id,
            [
                ("attack".into(), unit.weapon.as_ref().map(|_| 2)),
                (
                    "hold".into(),
                    (!caster && unit.worker.is_none()).then_some(4),
                ),
                (
                    "patrol".into(),
                    (!caster && unit.worker.is_none()).then_some(3),
                ),
            ]
            .into(),
        );
    }
    for race in 0..2 {
        result
            .unit_commands
            .get_mut(&id(26 + race))
            .unwrap()
            .extend([("gather".into(), Some(4)), ("build".into(), Some(3))]);
        result.build_buttons.insert(
            id(86 + race),
            BuildButton {
                advanced: false,
                slot: 3,
                key: "B".into(),
            },
        );
        for (source, key) in [
            (0, "F"),
            (2, "P"),
            (4, "B"),
            (6, "K"),
            (8, "A"),
            (10, "T"),
            (12, "P"),
            (14, "D"),
            (18, "R"),
            (26, "O"),
            (28, "T"),
            (30, "D"),
            (32, "B"),
            (38, "S"),
            (40, "F"),
            (42, "G"),
            (88, "K"),
            (90, "C"),
            (96, "G"),
            (98, "C"),
        ] {
            result.train_keys.insert(id(source + race), key.into());
        }
        for (advanced, source, slot, key) in [
            (false, 58, 0, "F"),
            (false, 74, 1, "T"),
            (false, 60, 2, "B"),
            (false, 76, 3, "L"),
            (false, 82, 4, "S"),
            (false, 64, 5, "W"),
            (true, 72, 0, "S"),
            (true, 78, 1, "F"),
            (true, 84, 2, "R"),
            (true, 68, 3, "I"),
            (true, 66, 4, "A"),
            (true, 80, 5, "M"),
            (true, 62, 6, "C"),
            (true, 70, 7, "G"),
        ] {
            result.build_buttons.insert(
                id(source + race),
                BuildButton {
                    advanced,
                    slot,
                    key: key.into(),
                },
            );
        }
        for (source, slot) in [
            (0, 0),
            (8, 1),
            (18, 1),
            (4, 2),
            (6, 3),
            (12, 3),
            (40, 0),
            (14, 1),
            (26, 0),
            (30, 1),
            (28, 2),
            (38, 3),
            (32, 4),
            (88, 1),
            (90, 1),
            (96, 0),
            (98, 1),
            (86, 4),
        ] {
            result.train_slots.insert(id(source + race), slot);
        }
        for ordinal in 1..=17 {
            let slot = match ordinal {
                1 | 2 | 5 | 6 | 7 | 8 | 13 | 17 => 0,
                3 | 4 | 9 | 10 => 1,
                11 | 12 | 14 => 2,
                15 => 3,
                16 => 4,
                _ => unreachable!(),
            };
            result
                .research_slots
                .insert(ResearchId((race * 64 + ordinal) as u16), slot);
        }
    }
    for (source, key) in [
        (1, "G"),
        (5, "C"),
        (7, "O"),
        (11, "T"),
        (13, "O"),
        (15, "S"),
        (19, "B"),
        (33, "J"),
        (39, "G"),
        (41, "Z"),
        (43, "D"),
        (89, "S"),
        (91, "F"),
    ] {
        result.train_keys.insert(id(source), key.into());
    }
    for (source, key) in [
        (59, "P"),
        (75, "H"),
        (77, "L"),
        (65, "W"),
        (69, "A"),
        (67, "O"),
        (81, "T"),
        (63, "L"),
        (71, "D"),
    ] {
        result.build_buttons.get_mut(&id(source)).unwrap().key = key.into();
    }
    for spell in spells::definitions() {
        let description = match spell.id.0 {
            1 => "Temporarily reveals a distant area.",
            2 => "Restores one health per six mana until healed or interrupted.",
            3 => "Damages undead units.",
            4 => "Summons a temporary flying scout.",
            5 => "Temporarily triples weapon damage.",
            6 => "Places damaging runes at the target location.",
            7 => "Launches a fireball at the target area.",
            8 => "Temporarily halves movement and attack speed.",
            9 => "Surrounds a ground unit with flames that damage nearby units.",
            10 => "Conceals a unit until it attacks, casts or the effect expires.",
            11 => "Permanently transforms a living unit into a neutral critter.",
            12 => "Bombards the target area with ice; friendly units can be hurt.",
            13 => "Damages a living unit and restores the caster's health.",
            14 => "Raises a temporary skeleton from a fallen unit's remains.",
            15 => "Temporarily doubles movement and attack speed.",
            16 => "Spends half a unit's current health to grant temporary invulnerability.",
            17 => "Creates a damaging whirlwind.",
            18 => "Damages units and structures in the target area over time.",
            19 => "Detonates the unit, damaging everything nearby.",
            _ => unreachable!(),
        };
        result.command_buttons.insert(
            format!("ability.{}", spell.id.0),
            CommandButton {
                slot: spell.slot,
                key: spell.key.into(),
                label: spell.name.into(),
                tip: description.into(),
                icon: format!("ability.{}", spell.id.0),
            },
        );
        if let Some((research, _, _, _)) = spell.research {
            let slot = match spell.id.0 {
                2 | 5 => 1,
                3 | 6 => 2,
                8 | 15 => 0,
                9 | 14 => 1,
                10 | 16 => 2,
                11 | 17 => 3,
                12 | 18 => 4,
                _ => unreachable!(),
            };
            result.research_slots.insert(research, slot);
            result.research_names.insert(research, spell.name.into());
            result.research_keys.insert(research, spell.key.into());
            result
                .research_descriptions
                .insert(research, description.into());
        }
    }
    result
}

pub fn unit_icon(source: usize) -> usize {
    match source {
        0 | 1 => 2 + source,
        2 | 3 | 16 | 17 => source % 2,
        4 | 5 => 16 + source - 4,
        6 | 7 => 8 + source - 6,
        8 | 9 => 4 + source - 8,
        10 | 11 => 14 + source - 10,
        12 | 13 => 10 + source - 12,
        14 | 15 => 12 + source - 14,
        18 | 19 => 6 + source - 18,
        20 => 187,
        21 => 189,
        22 => 191,
        23 => 194,
        24 => 193,
        25 => 190,
        26..=33 => 18 + source - 26,
        35 => 192,
        38 | 39 => 26 + source - 38,
        40 | 41 => 28 + source - 40,
        42 | 43 => 30 + source - 42,
        44 => 195,
        46 => 188,
        47 => 186,
        49 => 36,
        50 => 32,
        51 => 33,
        52 => 34,
        53 => 35,
        55 => 114,
        56 => 37,
        57 => 115,
        58 | 59 => 38 + source - 58,
        60 | 61 => 42 + source - 60,
        62 | 63 => 62 + source - 62,
        64 | 65 => 60 + source - 64,
        66 | 67 => 56 + source - 66,
        68 | 69 => 58 + source - 68,
        70 | 71 => 72 + source - 70,
        72 | 73 => 48 + source - 72,
        74 | 75 => 40 + source - 74,
        76 | 77 => 44 + source - 76,
        78 | 79 => 52 + source - 78,
        80 | 81 => 64 + source - 80,
        82 | 83 => 46 + source - 82,
        84 | 85 => 50 + source - 84,
        86 | 87 => 54 + source - 86,
        88 | 89 => 66 + source - 88,
        90 | 91 => 70 + source - 90,
        96 => 75,
        97 => 77,
        98 => 76,
        99 => 78,
        100 => 81,
        101 => 80,
        102 => 82,
        _ => 91,
    }
}
pub fn spell_icon(spell: u16) -> usize {
    [
        106, 107, 110, 111, 112, 97, 101, 94, 100, 95, 115, 105, 103, 114, 96, 98, 104, 108, 142,
    ][usize::from(spell - 1)]
}
