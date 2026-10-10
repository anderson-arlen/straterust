//! Complete retail building research rows. Slot/string operands were checked
//! against the v1.00 executable's button sets; costs and bonuses come from DAT.
use super::*;

pub(super) const UPGRADES: &[(usize, u16, u8)] = &[
    (16, 112, 0),
    (22, 115, 1),
    (19, 116, 2),
    (20, 117, 3),
    (21, 117, 4),
    (23, 118, 1),
    (17, 120, 0),
    (7, 122, 0),
    (0, 122, 1),
    (8, 123, 0),
    (9, 123, 1),
    (1, 123, 3),
    (2, 123, 4),
    (24, 132, 3),
    (25, 132, 4),
    (26, 132, 5),
    (29, 135, 0),
    (30, 135, 1),
    (32, 136, 2),
    (12, 141, 0),
    (4, 141, 1),
    (31, 138, 2),
    (10, 139, 0),
    (11, 139, 1),
    (3, 139, 2),
    (27, 142, 0),
    (28, 142, 1),
    (39, 159, 0),
    (38, 159, 1),
    (34, 163, 0),
    (14, 164, 0),
    (6, 164, 1),
    (33, 164, 2),
    (40, 165, 2),
    (13, 166, 0),
    (5, 166, 1),
    (15, 166, 2),
    (41, 169, 0),
    (42, 169, 1),
    (43, 169, 2),
    (44, 170, 2),
    (35, 171, 0),
    (36, 171, 1),
    (37, 171, 2),
];
pub(super) const TECHNOLOGIES: &[(usize, u16, u8, u32)] = &[
    (0, 112, 1, 324),
    (9, 115, 0, 332),
    (2, 116, 0, 326),
    (7, 116, 1, 330),
    (1, 117, 0, 325),
    (10, 117, 1, 333),
    (8, 118, 0, 331),
    (3, 120, 1, 327),
    (5, 120, 2, 328),
    (11, 131, 2, 364),
    (15, 136, 0, 369),
    (16, 136, 1, 371),
    (13, 138, 0, 366),
    (17, 138, 1, 370),
    (19, 165, 0, 395),
    (20, 165, 1, 396),
    (21, 170, 0, 397),
    (22, 170, 1, 398),
];

fn effects(
    rules: &Rules,
    units: &[u8],
    weapons: &[u8],
    source: usize,
    technology: bool,
) -> Option<ResearchEffect> {
    let ids = |sources: &[u16]| {
        sources
            .iter()
            .filter_map(|s| native_id(*s))
            .filter(|id| rules.units.iter().any(|u| u.id == *id))
            .collect::<Vec<_>>()
    };
    let ability = |sources: &[u16], ability| ResearchEffect::Ability {
        units: ids(sources),
        ability: AbilityId(ability),
    };
    let energy = |source| ResearchEffect::EnergyCapacity {
        units: ids(&[source]),
        amount: 50,
    };
    let speed = |source, percent| ResearchEffect::MovementSpeed {
        units: ids(&[source]),
        percent,
        acceleration_percent: 200,
    };
    let vision = |source| {
        let targets = ids(&[source]);
        ResearchEffect::VisionRange {
            amount: targets.first().map_or(0, |id| {
                352_u32.saturating_sub(
                    rules
                        .units
                        .iter()
                        .find(|u| u.id == *id)
                        .unwrap()
                        .vision_range,
                )
            }),
            units: targets,
        }
    };
    if technology {
        return Some(match source {
            0 => ResearchEffect::Stim {
                units: ids(&[0, 32]),
                hp_cost: 10,
                duration_ticks: 296,
            },
            1 => ability(&[1], 3),
            2 => ability(&[9], 1),
            3 => ResearchEffect::Mines { units: ids(&[2]) },
            5 => ResearchEffect::Mode { units: ids(&[5]) },
            7 => ability(&[9], 2),
            8 => ability(&[12], 4),
            9 => ResearchEffect::Cloak { units: ids(&[8]) },
            10 => ResearchEffect::Cloak { units: ids(&[1]) },
            11 => ResearchEffect::Cloak {
                units: ids(&[37, 38, 41, 46, 50]),
            },
            13 => ability(&[45], 7),
            15 => ability(&[46], 12),
            16 => ability(&[46], 13),
            17 => ability(&[45], 8),
            19 => ability(&[67], 14),
            20 => ability(&[67], 15),
            21 => ability(&[71], 16),
            22 => ability(&[71], 17),
            _ => return None,
        });
    }
    if matches!(source, 7..=14 | 35) {
        let mut targets = Vec::new();
        let mut bonuses = Vec::new();
        for &(original, native) in MAPPING {
            if !rules.units.iter().any(|u| u.id == UnitTypeId(native)) {
                continue;
            }
            let n = usize::from(original);
            let sub = usize::from(word(units, 228 + n * 2));
            let n = if sub < 228 { sub } else { n };
            let pair = [units[0x1704 + n], units[0x17e8 + n]].map(|w| {
                let w = usize::from(w);
                if w < 100 && usize::from(weapons[0x6a4 + w]) == source {
                    u32::from(word(weapons, 0xbb8 + w * 2)) * u32::from(weapons[0xce4 + w].max(1))
                } else {
                    0
                }
            });
            if pair != [0, 0] {
                targets.push(UnitTypeId(native));
                bonuses.push(pair);
            }
        }
        return Some(ResearchEffect::WeaponUpgrade {
            units: targets,
            bonuses,
        });
    }
    if source <= 6 {
        return Some(ResearchEffect::Armor {
            units: MAPPING
                .iter()
                .filter(|(s, n)| {
                    usize::from(units[0x1f08 + usize::from(*s)]) == source
                        && rules.units.iter().any(|u| u.id == UnitTypeId(*n))
                })
                .map(|(_, n)| UnitTypeId(*n))
                .collect(),
            amount: 1,
        });
    }
    Some(match source {
        15 => ResearchEffect::ShieldArmor {
            units: rules
                .units
                .iter()
                .filter(|u| u.max_shields > 0)
                .map(|u| u.id)
                .collect(),
            amount: 1,
        },
        16 => ResearchEffect::WeaponRange {
            units: ids(&[0]),
            amount: 32,
            sight: 0,
        },
        17 => speed(2, 150),
        19 => energy(9),
        20 => vision(1),
        21 => energy(1),
        22 => energy(8),
        23 => energy(12),
        24 => ResearchEffect::Transport { units: ids(&[42]) },
        25 => vision(42),
        26 => speed(42, 400),
        27 => speed(37, 150),
        28 => ResearchEffect::AttackRate {
            units: ids(&[37]),
            percent: 150,
        },
        29 => speed(38, 150),
        30 => ResearchEffect::WeaponRange {
            units: ids(&[38]),
            amount: 32,
            sight: 0,
        },
        31 => energy(45),
        32 => energy(46),
        33 => ResearchEffect::WeaponRange {
            units: ids(&[66]),
            amount: 64,
            sight: 0,
        },
        34 => speed(65, 150),
        36 => ResearchEffect::ProductionCapacity {
            units: ids(&[83]),
            amount: 5,
        },
        37 => speed(69, 150),
        38 => vision(84),
        39 => speed(84, 150),
        40 => energy(67),
        41 => vision(70),
        42 => speed(70, 150),
        43 => ResearchEffect::ProductionCapacity {
            units: ids(&[72]),
            amount: 4,
        },
        44 => energy(71),
        _ => return None,
    })
}

fn targets(effect: &ResearchEffect) -> &[UnitTypeId] {
    match effect {
        ResearchEffect::UnitUpgrade { units, .. }
        | ResearchEffect::Regeneration { units, .. }
        | ResearchEffect::WeaponUpgrade { units, .. }
        | ResearchEffect::VisionRange { units, .. }
        | ResearchEffect::ShieldArmor { units, .. }
        | ResearchEffect::AttackRate { units, .. }
        | ResearchEffect::ProductionCapacity { units, .. }
        | ResearchEffect::WeaponDamage { units, .. }
        | ResearchEffect::Armor { units, .. }
        | ResearchEffect::WeaponRange { units, .. }
        | ResearchEffect::Stim { units, .. }
        | ResearchEffect::Cloak { units }
        | ResearchEffect::Mines { units }
        | ResearchEffect::EnergyCapacity { units, .. }
        | ResearchEffect::MovementSpeed { units, .. }
        | ResearchEffect::Transport { units }
        | ResearchEffect::Mode { units }
        | ResearchEffect::Ability { units, .. } => units,
    }
}

pub(super) fn refresh(
    archive: &mut Archive<std::io::Cursor<Vec<u8>>>,
    files: &mut Files,
    assets: &mut AssetManifest,
    rules: &mut Rules,
    sections: &crate::backwater::Sections<'_>,
    human: usize,
) -> Result<()> {
    let units = archive.read_file("arr\\units.dat", 19192)?;
    let weapons = archive.read_file("arr\\weapons.dat", 4200)?;
    let tech = archive.read_file("arr\\techdata.dat", 432)?;
    let upgrades = archive.read_file("arr\\upgrades.dat", 920)?;
    let strings = archive.read_file("rez\\stat_txt.tbl", 65536)?;
    let palette =
        formats::decode_pcx(&archive.read_file("unit\\cmdbtns\\ticon.pcx", 1024 * 1024)?)?;
    let icons = formats::decode_grp(
        &archive.read_file("unit\\cmdbtns\\cmdicons.grp", 1024 * 1024)?,
        &crate::terran_ui::command_palette(&palette)?,
    )?;
    let mut presentation = std::str::from_utf8(&files["presentation.ron"])?.to_owned();
    let mut labels: Labels = ron::from_str(&presentation)?;
    let mut slots = BTreeMap::new();
    let mut descriptions = BTreeMap::new();
    rules.research.clear();
    labels.research_names.clear();
    labels.research_keys.clear();
    for (technology, source, facility, slot, string) in UPGRADES
        .iter()
        .map(|&(s, f, p)| (false, s, f, p, 456 + s as u32))
        .chain(TECHNOLOGIES.iter().map(|&(s, f, p, t)| (true, s, f, p, t)))
    {
        let Some(facility) =
            native_id(facility).filter(|id| rules.units.iter().any(|u| u.id == *id))
        else {
            continue;
        };
        let (maximum, initial) = faction_research::available(sections, human, technology, source)?;
        let count = if technology { 24 } else { 46 };
        let data = if technology { &tech } else { &upgrades };
        let levels = maximum
            .max(initial)
            .min(if technology || source > 15 { 1 } else { 3 });
        let Some(effect) = effects(rules, &units, &weapons, source, technology) else {
            continue;
        };
        if targets(&effect).is_empty() {
            continue;
        }
        // Do not expose buttons whose actual capability is not present in this roster.
        if let ResearchEffect::Ability { units, ability } = &effect
            && !units.iter().all(|id| {
                rules
                    .units
                    .iter()
                    .find(|u| u.id == *id)
                    .unwrap()
                    .abilities
                    .iter()
                    .any(|a| a.id == *ability)
            })
        {
            continue;
        }
        if let ResearchEffect::EnergyCapacity { units, .. } = &effect
            && !units.iter().all(|id| {
                rules
                    .units
                    .iter()
                    .find(|u| u.id == *id)
                    .unwrap()
                    .energy_max()
                    > 0
            })
        {
            continue;
        }
        if let ResearchEffect::ProductionCapacity { units, .. } = &effect
            && !units.iter().all(|id| {
                rules
                    .units
                    .iter()
                    .find(|u| u.id == *id)
                    .unwrap()
                    .production_capacity
                    > 0
            })
        {
            continue;
        }
        let base = faction_research::SOURCES
            .iter()
            .find(|(_, t, s)| *t == technology && *s == source)
            .unwrap()
            .0;
        for level in 1..=levels {
            let id = faction_research::level_id(base, level);
            let increment = u32::from(level - 1);
            let value = |base_field, multiplier_field| {
                u32::from(word(data, count * base_field + source * 2))
                    + if technology {
                        0
                    } else {
                        increment * u32::from(word(data, count * multiplier_field + source * 2))
                    }
            };
            let cost = [
                ("minerals", value(0, 2)),
                ("gas", value(if technology { 2 } else { 4 }, 6)),
            ]
            .into_iter()
            .filter(|(_, a)| *a > 0)
            .map(|(kind, amount)| ResourceAmount {
                kind: kind.into(),
                amount,
            })
            .collect();
            let ticks = value(if technology { 4 } else { 8 }, 10).max(1);
            let name = crate::terran_media::table_string(
                &strings,
                u32::from(word(
                    data,
                    count * if technology { 14 } else { 16 } + source * 2,
                )),
            )?;
            descriptions.insert(
                id,
                super::super::descriptions::research(&effect, source, technology),
            );
            labels.research_names.insert(
                id,
                if levels > 1 {
                    format!("{name} {level}")
                } else {
                    name.into()
                },
            );
            labels.research_keys.insert(
                id,
                crate::terran_media::table_string(&strings, string)?
                    .chars()
                    .next()
                    .context("missing research key")?
                    .to_ascii_uppercase()
                    .to_string(),
            );
            slots.insert(id, slot);
            let key = format!("research.{}", id.0);
            assets.ui.retain(|ui| ui.key != key);
            let icon = usize::from(word(
                data,
                count * if technology { 12 } else { 14 } + source * 2,
            ));
            assets.ui.push(straterust_engine::assets::UiImageManifest {
                image: crate::add_image(
                    files,
                    &format!("ui-{key}.srim"),
                    icons.get(icon).context("missing research icon")?,
                )?,
                key,
            });
            let requirement = if !technology && source == 28 {
                Some(133)
            } else if level > 1 {
                match source {
                    0..=2 | 7..=9 => Some(116),
                    3 | 4 | 10..=12 => Some(if level == 2 { 132 } else { 133 }),
                    5 | 13 => Some(165),
                    6 | 14 => Some(169),
                    15 => Some(164),
                    _ => None,
                }
            } else {
                None
            };
            rules.research.push(Research {
                available: true,
                id,
                facility,
                previous: (level > 1).then(|| faction_research::level_id(base, level - 1)),
                prerequisites: requirement
                    .and_then(native_id)
                    .filter(|id| rules.units.iter().any(|u| u.id == *id))
                    .into_iter()
                    .collect(),
                cost,
                ticks,
                effect: effect.clone(),
            });
        }
    }
    for (field, value) in [
        (
            "research_names",
            ron::ser::to_string(&labels.research_names)?,
        ),
        ("research_descriptions", ron::ser::to_string(&descriptions)?),
        ("research_keys", ron::ser::to_string(&labels.research_keys)?),
        ("research_slots", ron::ser::to_string(&slots)?),
    ] {
        set_map(&mut presentation, field, &value)?;
    }
    files.insert("presentation.ron".into(), presentation.into_bytes());
    let race = sections.exact("SIDE", 12)?[human];
    refresh_announcements(archive, files, rules, race)?;
    super::super::descriptions::refresh(files, rules)
}
