//! Exact legacy DAT fields needed by the small Terran demonstration.
//! Original Windows v1.00 uses 19192-byte units.dat and 100-entry weapons.dat,
//! not the later Brood War layouts. Field layout evidence:
//! https://github.com/poiuyqwert/PyMS/blob/master/PyMS/FileFormats/DAT/UnitsDAT.py
//! https://github.com/poiuyqwert/PyMS/blob/master/PyMS/FileFormats/DAT/WeaponsDAT.py
//! The selected legacy offsets were checked against the reference executable/data.
use anyhow::{Context, Result, ensure};
use serde::Serialize;
use straterust_engine::sim::{Motion, Rules, UnitTypeId, WeaponStrike};

#[derive(Debug, Serialize)]
pub struct ReferenceMotion {
    pub source_id: u16,
    pub flingy_id: u8,
    pub movement_control: u8,
    pub top_speed_fp8: u32,
    pub acceleration_fp8: u16,
    pub native: Motion,
}

/// Decode the six campaign movers. IScript-controlled units have dummy DAT
/// speeds; their bounded move/wait1/play-frame loop supplies the real strides.
pub fn decode_motion(
    units: &[u8],
    flingy: &[u8],
    scripts: &[u8],
    ids: &[u16],
) -> Result<Vec<ReferenceMotion>> {
    const COUNT: usize = 184;
    ensure!(
        units.len() == 19192 && flingy.len() == COUNT * 15,
        "unsupported original movement DAT layout"
    );
    ensure!(ids.len() <= 6, "too many selected source movers");
    ids.iter()
        .map(|&source_id| {
            let (expected_flingy, expected_sprite, script) = match source_id {
                0 => (78, 235, Some(78)),
                7 => (81, 239, None),
                19 => (88, 244, None),
                32 => (73, 228, Some(69)),
                37 => (15, 159, Some(31)),
                38 => (8, 146, Some(18)),
                _ => anyhow::bail!("unsupported source mover {source_id}"),
            };
            let flingy_id = units[usize::from(source_id)];
            let index = usize::from(flingy_id);
            ensure!(
                flingy_id == expected_flingy && word(flingy, index * 2) == expected_sprite,
                "unexpected source mover mapping"
            );
            let top_speed_fp8 = dword(flingy, COUNT * 2 + index * 4);
            let acceleration_fp8 = word(flingy, COUNT * 6 + index * 2);
            let movement_control = flingy[COUNT * 14 + index];
            let native = if let Some(script) = script {
                ensure!(
                    movement_control == 2 && top_speed_fp8 == 1,
                    "unexpected script-controlled movement"
                );
                let steps = stride_cycle(scripts, script)?;
                let total: u32 = steps.iter().map(|step| u32::from(*step)).sum();
                let count = steps.len() as u32;
                let speed = (total * 256 + count / 2) / count;
                // A constant cycle needs no phase state.
                let steps = if steps.iter().all(|step| *step == steps[0]) {
                    Vec::new()
                } else {
                    steps
                };
                Motion {
                    speed,
                    acceleration: 0,
                    steps,
                }
            } else {
                ensure!(
                    movement_control == 0 && (1..=262144).contains(&top_speed_fp8),
                    "unsupported flingy movement"
                );
                Motion {
                    speed: top_speed_fp8,
                    acceleration: u32::from(acceleration_fp8),
                    steps: Vec::new(),
                }
            };
            Ok(ReferenceMotion {
                source_id,
                flingy_id,
                movement_control,
                top_speed_fp8,
                acceleration_fp8,
                native,
            })
        })
        .collect()
}

fn stride_cycle(scripts: &[u8], script: u16) -> Result<Vec<u16>> {
    let program = crate::terran::script_animation(scripts, script, 11)?;
    let start = scripts.len() - program.len();
    let mut cursor = 0;
    let mut steps = Vec::new();
    for _ in 0..32 {
        let step = program
            .get(cursor..cursor + 7)
            .context("truncated walking stride")?;
        ensure!(
            step[0] == 0x29 && (1..=32).contains(&step[1]) && step[2..5] == [5, 1, 0],
            "unsupported walking stride instructions"
        );
        steps.push(u16::from(step[1]));
        cursor += 7;
        if program.get(cursor) == Some(&7) {
            let target = program
                .get(cursor + 1..cursor + 3)
                .context("truncated walking loop")?;
            ensure!(
                usize::from(u16::from_le_bytes(target.try_into().unwrap())) == start,
                "walking loop changes entry point"
            );
            return Ok(steps);
        }
    }
    anyhow::bail!("walking cycle exceeds 32 strides")
}

pub fn apply_motion(
    rules: &mut Rules,
    source: &[ReferenceMotion],
    mapping: &[(u16, u16)],
) -> Result<()> {
    for reference in source {
        let native = mapping
            .iter()
            .find(|(id, _)| *id == reference.source_id)
            .context("missing movement mapping")?
            .1;
        rules
            .units
            .iter_mut()
            .find(|u| u.id == UnitTypeId(native))
            .context("missing native mover")?
            .motion = Some(reference.native.clone());
    }
    Ok(())
}

pub fn apply_acquisition(rules: &mut Rules, units: &[u8], mapping: &[(u16, u16)]) -> Result<()> {
    ensure!(units.len() == 19192, "unsupported acquisition DAT layout");
    for &(source, native) in mapping {
        ensure!(source < 228, "invalid acquisition unit ID");
        let unit = rules
            .units
            .iter_mut()
            .find(|u| u.id == UnitTypeId(native))
            .context("missing native acquisition unit")?;
        if unit.mine.is_none() {
            unit.acquisition_range = unit.weapon.as_ref().map(|weapon| {
                (u32::from(units[0x1d40 + usize::from(source)]) * 32).max(weapon.range)
            });
        }
    }
    Ok(())
}

/// Repeat-attack entry prefixes are verified before scheduling their fire point.
/// The original executable starts cooldown before the animation fires.
pub fn apply_attack_timing(
    rules: &mut Rules,
    scripts: &[u8],
    mapping: &[(u16, u16)],
) -> Result<()> {
    for &(source, native) in mapping {
        let (script, prefix, delay): (u16, &[u8], u32) = match source {
            0 => (78, &[5, 1, 0x2e, 0x18, 69, 0, 0x25, 1], 1),
            7 => (84, &[3, 0, 5, 1, 0, 34, 0, 0x25, 1], 1),
            19 => (86, &[5, 1, 0x25, 1], 1),
            32 => (
                69,
                &[
                    5, 1, 0x2e, 8, 165, 1, 0, 0, 0, 17, 0, 0x31, 24, 5, 2, 0x31, 52, 5, 1, 0x31, 80,
                ],
                1,
            ),
            37 => (
                31,
                &[
                    0, 0, 0, 5, 1, 0x2e, 0, 17, 0, 5, 1, 0, 34, 0, 0x1c, 1, 126, 3,
                ],
                2,
            ),
            38 => (18, &[5, 1, 0, 68, 0, 0x18, 64, 0, 0x15, 76, 1, 0, 0x26], 1),
            _ => continue,
        };
        crate::terran::expect_animation(scripts, script, 5, prefix)?;
        let weapon = rules
            .units
            .iter_mut()
            .find(|u| u.id == UnitTypeId(native))
            .and_then(|u| u.weapon.as_mut())
            .context("missing native source weapon")?;
        weapon.cooldown_jitter = Some([-1, 2]);
        weapon.strikes = if source == 32 {
            [(1, 24), (3, 52), (4, 80)]
                .map(|(delay, forward)| WeaponStrike { delay, forward })
                .to_vec()
        } else {
            vec![WeaponStrike { delay, forward: 0 }]
        };
    }
    Ok(())
}

#[derive(Debug, Serialize)]
pub struct ReferenceWeapon {
    pub id: u8,
    pub damage_type: u8,
    pub behavior: u8,
    pub effect: u8,
    pub splash_radii: [u16; 3],
    pub forward_offset: u8,
    pub target_flags: u16,
    pub damage: u16,
    pub cooldown_frames: u8,
    pub minimum_range: u32,
    pub maximum_range: u32,
}

#[derive(Debug, Serialize)]
pub struct ReferenceUnit {
    pub source_id: u16,
    pub minerals: u16,
    pub gas: u16,
    pub hitpoints: u32,
    pub armor: u8,
    pub unit_size: u8,
    pub build_frames: u16,
    pub supply_required_half_units: u8,
    pub supply_provided_half_units: u8,
    pub collision_extents: [u16; 4],
    pub placement_size: [u16; 2],
    pub weapon: Option<ReferenceWeapon>,
}

pub fn decode(units: &[u8], weapons: &[u8]) -> Result<Vec<ReferenceUnit>> {
    let result = decode_selected(units, weapons, &[0, 7, 106, 109, 111])?;
    ensure!(
        result
            .iter()
            .filter_map(|unit| unit.weapon.as_ref())
            .all(|weapon| weapon.damage_type == 3
                && weapon.behavior == 2
                && weapon.effect == 1
                && weapon.splash_radii == [0; 3]),
        "Terran demo requires normal direct-hit weapons"
    );
    Ok(result)
}

pub fn decode_selected(units: &[u8], weapons: &[u8], ids: &[u16]) -> Result<Vec<ReferenceUnit>> {
    ensure!(
        units.len() == 19192,
        "Terran data requires original 19192-byte units.dat; later layouts are unsupported"
    );
    ensure!(
        weapons.len() == 4200,
        "Terran data requires original 100-entry weapons.dat"
    );
    ensure!(
        ids.len() <= 228 && ids.iter().all(|id| *id < 228),
        "invalid selected legacy unit IDs"
    );
    let mut result = Vec::new();
    for id in ids.iter().copied().map(usize::from) {
        let hitpoints = dword(units, 0xc54 + id * 4);
        ensure!(
            hitpoints.is_multiple_of(256),
            "unit {id} has fractional starting HP; unsupported demonstration"
        );
        let weapon_id = units[0x1704 + id];
        ensure!(weapon_id <= 100, "unit {id} has an invalid ground weapon");
        let weapon = if weapon_id == 100 {
            None
        } else {
            let w = usize::from(weapon_id);
            let target_flags = word(weapons, 0x2bc + w * 2);
            ensure!(
                matches!(target_flags, 2 | 3 | 18),
                "weapon {w} has unsupported target restrictions"
            );
            ensure!(
                matches!(weapons[0x708 + w], 1..=3)
                    && matches!(weapons[0x834 + w], 1..=3)
                    && matches!(weapons[0x76c + w], 0 | 2 | 5),
                "weapon {w} requires unsupported damage, effect or projectile behavior"
            );
            let splash_radii = [0x898, 0x960, 0xa28].map(|offset| word(weapons, offset + w * 2));
            ensure!(
                splash_radii[0] <= splash_radii[1]
                    && splash_radii[1] <= splash_radii[2]
                    && splash_radii[2] <= 1024,
                "weapon {w} has invalid splash radii"
            );
            ensure!(
                (weapons[0x834 + w] == 1 && splash_radii == [0; 3])
                    || (matches!(weapons[0x834 + w], 2 | 3) && splash_radii[0] > 0),
                "weapon {w} has inconsistent splash effect"
            );
            ensure!(
                weapons[0xce4 + w] == 1,
                "weapon {w} damage factors other than one are unsupported"
            );
            let cooldown_frames = weapons[0xc80 + w];
            ensure!(cooldown_frames > 0, "weapon {w} has zero cooldown");
            let minimum_range = dword(weapons, 0x384 + w * 4);
            let maximum_range = dword(weapons, 0x514 + w * 4);
            ensure!(
                minimum_range <= maximum_range && maximum_range <= 32768,
                "weapon {w} has unsupported range"
            );
            Some(ReferenceWeapon {
                id: weapon_id,
                damage_type: weapons[0x708 + w],
                behavior: weapons[0x76c + w],
                effect: weapons[0x834 + w],
                splash_radii,
                forward_offset: weapons[0xe10 + w],
                target_flags,
                damage: word(weapons, 0xaf0 + w * 2),
                cooldown_frames,
                minimum_range,
                maximum_range,
            })
        };
        let placement_size = [
            word(units, 0x2a4c + id * 4),
            word(units, 0x2a4c + id * 4 + 2),
        ];
        ensure!(
            placement_size
                .iter()
                .all(|dimension| (1..=1024).contains(dimension)),
            "unit {id} has invalid placement dimensions"
        );
        let collision_extents = std::array::from_fn(|side| word(units, 0x2f5c + id * 8 + side * 2));
        ensure!(
            collision_extents.iter().all(|extent| *extent <= 1024),
            "unit {id} has invalid collision extents"
        );
        let build_frames = word(units, 0x3bd4 + id * 2);
        ensure!(
            build_frames > 0 && hitpoints > 0,
            "unit {id} has zero build time or HP"
        );
        result.push(ReferenceUnit {
            source_id: id as u16,
            minerals: word(units, 0x3844 + id * 2),
            gas: word(units, 0x3a0c + id * 2),
            hitpoints: hitpoints / 256,
            armor: units[0x20d0 + id],
            unit_size: units[0x1fec + id],
            build_frames,
            supply_required_half_units: units[0x412c + id],
            supply_provided_half_units: units[0x4048 + id],
            collision_extents,
            placement_size,
            weapon,
        });
    }
    Ok(result)
}

// Callers validate the complete fixed layout first; selected IDs bound every offset.
fn word(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(bytes[offset..offset + 2].try_into().unwrap())
}
fn dword(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn script(id: u16, program: &[u8]) -> Vec<u8> {
        let mut bytes = vec![0; 80];
        bytes[..2].copy_from_slice(&id.to_le_bytes());
        bytes[2..4].copy_from_slice(&8_u16.to_le_bytes());
        bytes[4..6].copy_from_slice(&u16::MAX.to_le_bytes());
        bytes[8..12].copy_from_slice(b"SCPE");
        bytes[12] = 26;
        for animation in [5, 11] {
            bytes[16 + animation * 2..18 + animation * 2].copy_from_slice(&80_u16.to_le_bytes());
        }
        bytes.extend(program);
        bytes
    }
    fn movement_data(
        source: usize,
        flingy_id: u8,
        sprite: u16,
        speed: u32,
        accel: u16,
        control: u8,
    ) -> (Vec<u8>, Vec<u8>) {
        let mut units = vec![0; 19192];
        units[source] = flingy_id;
        let mut flingy = vec![0; 2760];
        let id = usize::from(flingy_id);
        flingy[id * 2..id * 2 + 2].copy_from_slice(&sprite.to_le_bytes());
        flingy[368 + id * 4..372 + id * 4].copy_from_slice(&speed.to_le_bytes());
        flingy[1104 + id * 2..1106 + id * 2].copy_from_slice(&accel.to_le_bytes());
        flingy[2576 + id] = control;
        (units, flingy)
    }
    #[test]
    fn source_motion_keeps_fractional_speed_acceleration_and_exact_stride_cycle() {
        let (units, flingy) = movement_data(19, 88, 244, 1707, 100, 0);
        let decoded = decode_motion(&units, &flingy, &[], &[19]).unwrap();
        assert_eq!(
            decoded[0].native,
            Motion {
                speed: 1707,
                acceleration: 100,
                steps: vec![]
            }
        );
        let (units, flingy) = movement_data(37, 15, 159, 1, 1, 2);
        let steps = [2, 8, 9, 5, 6, 7, 2];
        let mut program: Vec<u8> = steps
            .iter()
            .flat_map(|step| [0x29, *step, 5, 1, 0, 0, 0])
            .collect();
        program.extend([7, 80, 0]);
        let scripts = script(31, &program);
        let decoded = decode_motion(&units, &flingy, &scripts, &[37]).unwrap();
        assert_eq!(
            decoded[0].native,
            Motion {
                speed: 1426,
                acceleration: 0,
                steps: steps.map(u16::from).to_vec()
            }
        );
        let mut bad = scripts.clone();
        bad[83] = 2;
        assert!(decode_motion(&units, &flingy, &bad, &[37]).is_err());
        *bad.last_mut().unwrap() = 1;
        bad[83] = 1;
        assert!(decode_motion(&units, &flingy, &bad, &[37]).is_err());
        for end in [0, 83, scripts.len() - 1] {
            assert!(decode_motion(&units, &flingy, &scripts[..end], &[37]).is_err());
        }
        assert!(decode_motion(&units[..19191], &flingy, &scripts, &[37]).is_err());
        assert!(decode_motion(&units, &flingy[..2759], &scripts, &[37]).is_err());
    }
    #[test]
    fn firebat_timing_preserves_startup_before_all_three_strikes() {
        use straterust_engine::sim::{UnitType, Weapon};
        let prefix = [
            5, 1, 0x2e, 8, 165, 1, 0, 0, 0, 17, 0, 0x31, 24, 5, 2, 0x31, 52, 5, 1, 0x31, 80,
        ];
        let scripts = script(69, &prefix);
        let mut rules = Rules {
            units: vec![UnitType {
                id: UnitTypeId(11),
                weapon: Some(Weapon {
                    cooldown_jitter: None,
                    targets_air: false,
                    damage: 8,
                    range: 32,
                    cooldown: 22,
                    damage_kind: Default::default(),
                    splash: Some([15, 20, 25]),
                    strikes: Vec::new(),
                }),
                ..UnitType::default()
            }],
            ..Rules::default()
        };
        apply_attack_timing(&mut rules, &scripts, &[(32, 11)]).unwrap();
        let weapon = rules.units[0].weapon.as_ref().unwrap();
        assert_eq!(weapon.cooldown_jitter, Some([-1, 2]));
        assert_eq!(
            weapon
                .strikes
                .iter()
                .map(|s| (s.delay, s.forward))
                .collect::<Vec<_>>(),
            vec![(1, 24), (3, 52), (4, 80)]
        );
        let mut bad = scripts;
        bad[81] = 2;
        assert!(apply_attack_timing(&mut rules, &bad, &[(32, 11)]).is_err());
    }
    fn original() -> (Vec<u8>, Vec<u8>) {
        let mut units = vec![0; 19192];
        let mut weapons = vec![0; 4200];
        for id in [0_usize, 7, 106, 109, 111] {
            units[0xc54 + id * 4..0xc58 + id * 4].copy_from_slice(&(80_u32 * 256).to_le_bytes());
            units[0x3bd4 + id * 2..0x3bd6 + id * 2].copy_from_slice(&320_u16.to_le_bytes());
            units[0x3844 + id * 2..0x3846 + id * 2].copy_from_slice(&75_u16.to_le_bytes());
            units[0x2a4c + id * 4..0x2a50 + id * 4].copy_from_slice(&[24, 0, 32, 0]);
            units[0x1704 + id] = 100;
        }
        units[0x1704] = 0;
        weapons[0xc80] = 12;
        weapons[0xce4] = 1;
        weapons[0x708] = 3;
        weapons[0x834] = 1;
        weapons[0x76c] = 2;
        weapons[0x2bc] = 3;
        weapons[0xaf0..0xaf2].copy_from_slice(&7_u16.to_le_bytes());
        weapons[0x514..0x518].copy_from_slice(&96_u32.to_le_bytes());
        (units, weapons)
    }
    #[test]
    fn selected_enemy_records_allow_melee_and_explosive_without_relaxing_terran() {
        let (units, mut weapons) = original();
        weapons[0x708] = 1;
        weapons[0x76c] = 5;
        let records = decode_selected(&units, &weapons, &[0]).unwrap();
        let weapon = records[0].weapon.as_ref().unwrap();
        assert_eq!((weapon.damage_type, weapon.behavior), (1, 5));
        assert!(decode(&units, &weapons).is_err());
        assert!(decode_selected(&units, &weapons, &[228]).is_err());
        assert!(decode_selected(&units, &weapons, &[0; 229]).is_err());
    }

    #[test]
    fn legacy_arrays_remain_distinct_and_none_weapon_is_100() {
        let (units, weapons) = original();
        let decoded = decode(&units, &weapons).unwrap();
        assert_eq!(decoded.len(), 5);
        assert_eq!(decoded[0].minerals, 75);
        assert_eq!(decoded[0].hitpoints, 80);
        assert_eq!(decoded[0].build_frames, 320);
        assert_eq!(decoded[0].placement_size, [24, 32]);
        assert_eq!(decoded[0].weapon.as_ref().unwrap().damage, 7);
        assert_eq!(decoded[0].weapon.as_ref().unwrap().maximum_range, 96);
        assert!(decoded[1].weapon.is_none());
        assert_eq!(decoded[4].source_id, 111);
    }
    #[test]
    fn rejects_later_layouts_truncation_and_unhandled_weapon_properties() {
        let (mut units, mut weapons) = original();
        assert!(decode(&units[..19191], &weapons).is_err());
        assert!(decode(&units, &weapons[..4199]).is_err());
        assert!(decode(&vec![0; 19876], &weapons).is_err());
        weapons[0xce4] = 2;
        assert!(decode(&units, &weapons).is_err());
        weapons[0xce4] = 1;
        weapons[0x2bc] = 1;
        assert!(decode(&units, &weapons).is_err());
        weapons[0x2bc] = 3;
        units[0xc54] = 1;
        assert!(decode(&units, &weapons).is_err());
    }
}
