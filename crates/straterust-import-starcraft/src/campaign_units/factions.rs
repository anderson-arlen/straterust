//! Production, shields and construction data for the source campaign factions.
use super::*;

pub(crate) fn apply(
    archive: &mut Archive<std::io::Cursor<Vec<u8>>>,
    rules: &mut Rules,
) -> Result<()> {
    let source = archive.read_file("arr\\units.dat", 19192)?;
    let known: BTreeSet<_> = rules.units.iter().map(|u| u.id).collect();
    let fractional_supply = native_id(35).is_some_and(|id| known.contains(&id));
    if fractional_supply {
        // Retail DAT stores doubled supply. Preserve it as integer gameplay
        // units, and let presentation divide by two; no fractional simulation.
        rules.supply_limit = 400;
    }
    let ids = |values: &[u16]| {
        values
            .iter()
            .filter_map(|id| native_id(*id))
            .filter(|id| known.contains(id))
            .collect::<Vec<_>>()
    };
    for &(original, native) in MAPPING {
        let Some(unit) = rules.units.iter_mut().find(|u| u.id == UnitTypeId(native)) else {
            continue;
        };
        let n = usize::from(original);
        if matches!(original, 41 | 64) {
            unit.phases_while_gathering = true;
        }
        if fractional_supply {
            unit.supply_used = u32::from(source[0x412c + n]);
            unit.supply_provided = u32::from(source[0x4048 + n]);
        }
        if source[0x9a8 + n] != 0 {
            unit.max_shields = u32::from(word(&source, 0xa8c + n * 2));
            unit.shield_regeneration = 7;
        }
        if matches!(original, 37 | 47) {
            unit.cost = [
                ("minerals", word(&source, 0x3844 + n * 2)),
                ("gas", word(&source, 0x3a0c + n * 2)),
            ]
            .into_iter()
            .filter(|(_, amount)| *amount != 0)
            .map(|(kind, amount)| ResourceAmount {
                kind: kind.into(),
                amount: u32::from(amount) * 2,
            })
            .collect();
        }
        unit.requires_power = dword(&source, 0x19b0 + n * 4) & 0x80000 != 0;
        if (154..=175).contains(&original) {
            unit.autonomous_construction = true;
        }
        // Narrow existing Terran imports do not include larva/eggs; keep their
        // working hatchery production until that roster is available.
        if matches!(original, 131..=133) && native_id(35).is_some_and(|id| known.contains(&id)) {
            unit.offspring = native_id(35)
                .filter(|id| known.contains(id))
                .map(|unit_type| Offspring {
                    unit_type,
                    interval: 342,
                    maximum: 3,
                    initial: 3,
                });
            unit.trains = ids(match original {
                131 => &[132],
                132 => &[133],
                _ => &[],
            });
            unit.transforms_on_production = true;
            unit.dropoff = vec!["minerals".into(), "gas".into()];
            unit.creep_radius = Some([320, 200]);
            unit.provides_types = ids(match original {
                132 => &[131],
                133 => &[131, 132],
                _ => &[],
            });
        }
        if matches!(
            original,
            131 | 135 | 138 | 139 | 140 | 141 | 142 | 143 | 149
        ) {
            unit.consumes_builder = true;
        }
        match original {
            1 => unit.prerequisites = ids(&[112, 117]),
            7 => {
                let addons = ids(&[107, 108, 115, 117, 118, 120]);
                unit.builds.retain(|id| !addons.contains(id));
                unit.builds.extend(ids(&[116]));
                unit.repairs.extend(ids(&[
                    2, 3, 5, 7, 8, 9, 11, 12, 19, 23, 25, 28, 29, 30, 106, 107, 108, 109, 110, 111,
                    112, 113, 114, 115, 116, 117, 118, 120, 122, 123, 124, 125, 126,
                ]));
                unit.repairs.sort();
                unit.repairs.dedup();
                unit.builds.sort();
                unit.builds.dedup();
            }
            9 => unit.prerequisites = ids(&[116, 115]),
            12 => unit.prerequisites = ids(&[118, 115]),
            25 => {
                unit.speed = 0;
                unit.motion = None;
            }
            71 => unit.prerequisites = ids(&[170]),
            72 => {
                unit.prerequisites = ids(&[169]);
                unit.trains = ids(&[73]);
                unit.production_capacity = 4;
            }
            106 => {
                unit.builds.extend(ids(&[107, 108]));
                unit.builds.sort();
                unit.builds.dedup();
            }
            108 => {
                unit.addon_parent = native_id(106);
                unit.prerequisites = ids(&[116, 117]);
                unit.trains = ids(&[14]);
                unit.production_capacity = 1;
            }
            111 => unit.trains.extend(ids(&[1])),
            114 => unit.trains.extend(ids(&[9, 12])),
            116 => {
                unit.builds = ids(&[117, 118]);
                unit.prerequisites = ids(&[114]);
            }
            117 | 118 => unit.addon_parent = native_id(116),
            134 => {
                unit.blocks_movement = false;
                unit.autonomous_construction = true;
                unit.requires_creep = true;
                unit.prerequisites = ids(&[133]);
            }
            136 => {
                unit.requires_creep = true;
                unit.prerequisites = ids(&[133]);
            }
            159 => unit.prerequisites = ids(&[155]),
            169 => unit.prerequisites = ids(&[167]),
            170 => unit.prerequisites = ids(&[167, 165]),
            30 => {
                unit.speed = 0;
                unit.motion = None;
            }
            36 | 59 => {
                unit.speed = 0;
                unit.motion = None;
                unit.destroyed_on_production_cancel = original == 36;
            }
            35 => {
                unit.trains = ids(&[41, 37, 38, 42, 43, 45, 47, 39, 46]);
                unit.transforms_on_production = true;
                unit.production_form = native_id(36);
                unit.speed = 0;
                unit.motion = None;
            }
            37 => {
                unit.production_count = 2;
                unit.cloak = Some(crate::burrow::rules(7));
            }
            38 => unit.cloak = Some(crate::burrow::rules(7)),
            53 => unit.cloak = Some(crate::burrow::rules(7)),
            39 => unit.prerequisites = ids(&[140]),
            41 => {
                unit.cloak = Some(crate::burrow::rules(7));
                unit.builds = ids(&[131, 134, 135, 136, 138, 139, 140, 141, 142, 143, 149]);
            }
            43 => {
                unit.trains = ids(&[44]);
                unit.transforms_on_production = true;
                unit.production_form = native_id(59).filter(|id| known.contains(id));
            }
            44 => unit.prerequisites = ids(&[137]),
            45 => unit.prerequisites = ids(&[138]),
            46 => {
                unit.prerequisites = ids(&[136]);
                unit.cloak = Some(crate::burrow::rules(7));
            }
            47 => {
                unit.production_count = 2;
                unit.prerequisites = ids(&[141]);
            }
            50 => {
                unit.cloak = Some(crate::burrow::rules(7));
                unit.prerequisites = ids(&[130]);
            }
            64 => {
                unit.builds = ids(&[
                    154, 155, 156, 157, 159, 160, 162, 163, 164, 165, 166, 167, 169, 170, 171, 172,
                ])
            }
            66 => unit.prerequisites = ids(&[164]),
            67 => unit.prerequisites = ids(&[165]),
            42 => {
                unit.garrison = Some(GarrisonStats {
                    capacity: source[0x42f4 + n],
                    passengers: vec![],
                    attackers: vec![],
                    range_bonus: 0,
                    unload_ticks: 15,
                })
            }
            69 => unit.prerequisites = ids(&[155]),
            70 => unit.prerequisites = ids(&[167]),
            84 => unit.prerequisites = ids(&[159]),
            83 => {
                unit.prerequisites = ids(&[171]);
                unit.trains = ids(&[85]);
                unit.production_capacity = 5;
            }
            130 => unit.trains = ids(&[50]),
            132 => unit.prerequisites = ids(&[142]),
            133 => unit.prerequisites = ids(&[138]),
            137 => {
                unit.provides_types = ids(&[141]);
                unit.requires_creep = true;
            }
            138 => {
                unit.prerequisites = ids(&[132]);
                unit.requires_creep = true;
            }
            139 => unit.requires_creep = true,
            140 => {
                unit.prerequisites = ids(&[133]);
                unit.requires_creep = true;
            }
            141 => {
                unit.trains = ids(&[137]);
                unit.transforms_on_production = true;
                unit.prerequisites = ids(&[132]);
            }
            143 => {
                unit.trains = ids(&[144, 146]);
                unit.transforms_on_production = true;
            }
            144 => {
                unit.prerequisites = ids(&[139]);
                unit.requires_creep = true;
                unit.creep_radius = Some([0, 0]);
            }
            146 => unit.prerequisites = ids(&[142]),
            154 => {
                unit.trains = ids(&[64]);
                unit.dropoff = vec!["minerals".into(), "gas".into()];
            }
            155 => {
                unit.trains = ids(&[69, 83, 84]);
                unit.prerequisites = ids(&[164]);
            }
            156 => {
                unit.power_field = Some(PowerField {
                    cell_size: 32,
                    rows: vec![255, 255, 127, 63, 7],
                })
            }
            157 => {
                unit.extracts = Some(Extraction {
                    resource: "gas".into(),
                    harvest_ticks: 37,
                    depleted_amount: 2,
                })
            }
            160 => {
                unit.trains = ids(&[65, 66, 67]);
                unit.prerequisites = ids(&[154]);
            }
            162 => unit.prerequisites = ids(&[166]),
            163 => unit.prerequisites = ids(&[164]),
            164 => unit.prerequisites = ids(&[160]),
            165 => unit.prerequisites = ids(&[163]),
            167 => {
                unit.trains = ids(&[70, 71, 72]);
                unit.prerequisites = ids(&[164]);
            }
            171 => unit.prerequisites = ids(&[155]),
            172 => unit.prerequisites = ids(&[160]),
            216 | 217 | 219 => {
                unit.portable = true;
                unit.blocks_movement = false;
                unit.speed = 0;
                unit.weapon = None;
            }
            194..=196 => {
                unit.blocks_movement = false;
                unit.speed = 0;
                unit.weapon = None;
            }
            _ => {}
        }
        if original == 69 {
            unit.garrison = Some(GarrisonStats {
                capacity: source[0x42f4 + n],
                passengers: vec![],
                attackers: Vec::new(),
                range_bonus: 0,
                unload_ticks: 15,
            });
        }
    }
    for unit in &mut rules.units {
        unit.trains.sort();
        unit.trains.dedup();
    }
    let passengers = rules
        .units
        .iter()
        .filter(|u| {
            !u.structure && u.movement_class == MovementClass::Ground && u.speed > 0 && !u.revealer
        })
        .map(|u| u.id)
        .collect::<Vec<_>>();
    for source in [42, 69] {
        if let Some(container) = rules
            .units
            .iter_mut()
            .find(|u| Some(u.id) == native_id(source))
            && let Some(garrison) = &mut container.garrison
        {
            garrison.passengers = passengers.clone();
        }
    }
    Ok(())
}
