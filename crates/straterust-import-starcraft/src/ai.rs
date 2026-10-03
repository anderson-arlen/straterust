//! Retail v1.00 AI table (16-byte records at offset zero) and the finite
//! town programs used by Terran missions 3 and 5. No proprietary VM at runtime.
use anyhow::{Context, Result, bail, ensure};
use std::collections::BTreeMap;
use straterust_engine::sim::{AiInstruction, UnitTypeId};

pub fn translate(bytes: &[u8], id: [u8; 4], units: &[(u16, u16)]) -> Result<Vec<AiInstruction>> {
    ensure!(
        bytes.len() <= 65536,
        "AI source exceeds retail address space"
    );
    let mut table = BTreeMap::new();
    let mut offset = 0;
    loop {
        let key: [u8; 4] = bytes
            .get(offset..offset + 4)
            .context("truncated AI table")?
            .try_into()?;
        if key == [0; 4] {
            offset += 4;
            break;
        }
        let record = bytes
            .get(offset..offset + 16)
            .context("truncated AI table record")?;
        let address = u32::from_le_bytes(record[4..8].try_into()?) as usize;
        ensure!(
            table.insert(key, address).is_none() && table.len() <= 256,
            "invalid AI table"
        );
        offset += 16;
    }
    let start = *table.get(&id).context("missing campaign AI script")?;
    let end = table
        .values()
        .copied()
        .filter(|p| *p > start)
        .min()
        .unwrap_or(bytes.len());
    ensure!(
        start >= offset && start < end && end <= bytes.len(),
        "AI entry outside program"
    );
    let native_unit = |source: u16| -> Result<UnitTypeId> {
        units
            .iter()
            .find(|(id, _)| *id == source)
            .map(|(_, native)| UnitTypeId(*native))
            .with_context(|| format!("unsupported AI unit {source}"))
    };
    let mut addresses = BTreeMap::new();
    let mut jumps = Vec::new();
    let mut program = Vec::new();
    let mut cursor = start;
    let mut terminated = false;
    while cursor < end {
        ensure!(program.len() < 4096, "AI program too long");
        addresses.insert(cursor, program.len() as u16);
        let opcode = bytes[cursor];
        cursor += 1;
        let length = match opcode {
            0 | 2 | 46 => 2,
            6 => 4,
            8 => 2,
            12 | 19..=26 => 3,
            34 => 1,
            3 | 4 | 11 | 13..=16 | 27..=30 | 35 | 36 | 56 => 0,
            _ => bail!("unsupported campaign AI opcode {opcode} at {cursor}"),
        };
        let operands = bytes
            .get(cursor..cursor + length)
            .filter(|_| cursor + length <= end)
            .context("truncated AI instruction")?;
        cursor += length;
        let word = |index| u16::from_le_bytes(operands[index..index + 2].try_into().unwrap());
        let instruction = match opcode {
            0 => {
                jumps.push((program.len(), usize::from(word(0))));
                Some(AiInstruction::Jump(0))
            }
            2 => Some(AiInstruction::Wait(u32::from(word(0)).max(1))),
            6 => Some(AiInstruction::Request {
                unit_type: native_unit(word(1))?,
                count: u16::from(operands[0]),
                priority: operands[3],
            }),
            19..=22 => Some(AiInstruction::Defense {
                unit_type: native_unit(word(1))?,
                count: u16::from(operands[0]),
            }),
            11 => Some(AiInstruction::AttackClear),
            12 => Some(AiInstruction::AttackAdd {
                unit_type: native_unit(word(1))?,
                count: u16::from(operands[0]),
            }),
            13 => Some(AiInstruction::AttackPrepare),
            14 => Some(AiInstruction::Attack),
            36 => Some(AiInstruction::Stop),
            // Town/campaign/default-build and defense flags select original
            // engine policies. Native town construction and guard assistance
            // provide their bounded approximation, recorded in the import report.
            _ => None,
        };
        if let Some(instruction) = instruction {
            program.push(instruction);
        }
        if matches!(opcode, 0 | 36) {
            terminated = true;
            break;
        }
    }
    ensure!(terminated, "unterminated campaign AI script");
    for (index, target) in jumps {
        let target = *addresses
            .get(&target)
            .context("AI jump outside translated script")?;
        ensure!(
            usize::from(target) < program.len(),
            "AI jump targets no instruction"
        );
        program[index] = AiInstruction::Jump(target);
    }
    ensure!(!program.is_empty(), "empty campaign AI program");
    Ok(program)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn source(program: &[u8]) -> Vec<u8> {
        let mut bytes = b"Test".to_vec();
        bytes.extend(20_u32.to_le_bytes());
        bytes.extend([0; 12]);
        bytes.extend(program);
        bytes
    }
    #[test]
    fn retail_table_and_native_loop_keep_counts_priorities_and_waits() {
        let bytes = source(&[
            56, 3, 6, 2, 0, 0, 130, 2, 50, 0, 11, 12, 3, 0, 0, 13, 14, 0, 30, 0,
        ]);
        let program = translate(&bytes, *b"Test", &[(0, 1)]).unwrap();
        assert_eq!(
            program,
            vec![
                AiInstruction::Request {
                    unit_type: UnitTypeId(1),
                    count: 2,
                    priority: 130
                },
                AiInstruction::Wait(50),
                AiInstruction::AttackClear,
                AiInstruction::AttackAdd {
                    unit_type: UnitTypeId(1),
                    count: 3
                },
                AiInstruction::AttackPrepare,
                AiInstruction::Attack,
                AiInstruction::Jump(2)
            ]
        );
        for length in 0..bytes.len() {
            assert!(translate(&bytes[..length], *b"Test", &[(0, 1)]).is_err());
        }
        assert!(translate(&bytes, *b"Test", &[]).is_err());
    }
    #[test]
    fn source_defense_builds_are_preserved_before_attack_preparation() {
        let bytes = source(&[19, 1, 0, 0, 23, 1, 0, 0, 20, 1, 1, 0, 24, 1, 1, 0, 36]);
        assert_eq!(
            translate(&bytes, *b"Test", &[(0, 1), (1, 2)]).unwrap(),
            vec![
                AiInstruction::Defense {
                    unit_type: UnitTypeId(1),
                    count: 1
                },
                AiInstruction::Defense {
                    unit_type: UnitTypeId(2),
                    count: 1
                },
                AiInstruction::Stop,
            ]
        );
    }
    #[test]
    fn rejects_unknown_opcodes_and_jumps_outside_the_script() {
        assert!(translate(&source(&[255]), *b"Test", &[]).is_err());
        assert!(translate(&source(&[0, 0, 0]), *b"Test", &[]).is_err());
    }
}
