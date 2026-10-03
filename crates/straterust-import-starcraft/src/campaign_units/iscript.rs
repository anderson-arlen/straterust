//! Bounded static extraction of presentation instructions, not an IScript VM.
use super::*;

pub(super) struct Instruction {
    pub op: u8,
    pub args: Vec<u8>,
}
pub(super) fn instructions(bytes: &[u8], script: u16, animation: usize) -> Vec<Instruction> {
    let Ok(tail) = terran::script_animation(bytes, script, animation) else {
        return Vec::new();
    };
    let mut p = bytes.len() - tail.len();
    let mut visited = BTreeSet::new();
    let mut stack = Vec::new();
    let mut result = Vec::new();
    for _ in 0..512 {
        if !visited.insert((p, stack.clone())) {
            break;
        }
        let Some(&op) = bytes.get(p) else { break };
        p += 1;
        let size = match op {
            0 | 1 | 4 | 6 | 7 | 10 | 11 | 18 | 24 | 53 | 55 | 57 | 61 | 63 | 64 => 2,
            2 | 3 | 5 | 23 | 31 | 32 | 34 | 35 | 36 | 37 | 40 | 41 | 43 | 44 | 49 | 52 | 56
            | 65 => 1,
            8 | 9 | 13 | 14 | 15 | 16 | 17 | 19 | 20 | 26 | 58 | 66 => 4,
            21 | 30 => 3,
            59 | 60 => 6,
            25 | 28 => {
                let Some(&count) = bytes.get(p) else { break };
                1 + usize::from(count) * 2
            }
            12 | 22 | 27 | 29 | 33 | 38 | 39 | 42 | 45 | 46 | 47 | 48 | 50 | 51 | 54 | 62 | 67
            | 68 => 0,
            _ => break,
        };
        let Some(args) = bytes.get(p..p + size) else {
            break;
        };
        result.push(Instruction {
            op,
            args: args.to_vec(),
        });
        p += size;
        match op {
            7 => p = usize::from(word(args, 0)),
            53 if stack.len() < 8 => {
                stack.push(p);
                p = usize::from(word(args, 0));
            }
            54 => {
                let Some(ret) = stack.pop() else { break };
                p = ret;
            }
            22 | 48 => break,
            _ => {}
        }
    }
    result
}
pub(super) fn timeline(bytes: &[u8], script: u16, animation: usize) -> Vec<u16> {
    let mut frame = None;
    let mut frames = Vec::new();
    for instruction in instructions(bytes, script, animation) {
        match instruction.op {
            0 | 1 => frame = Some(word(&instruction.args, 0)),
            5 | 6 => {
                if let Some(frame) = frame {
                    let wait = if instruction.op == 5 {
                        u16::from(instruction.args[0])
                    } else {
                        (u16::from(instruction.args[0]) + u16::from(instruction.args[1])) / 2
                    };
                    frames.extend(std::iter::repeat_n(frame, usize::from(wait.clamp(1, 24))));
                }
            }
            _ => {}
        }
        if frames.len() >= 256 {
            break;
        }
    }
    if frames.is_empty()
        && let Some(frame) = frame
    {
        frames.push(frame)
    }
    frames.truncate(256);
    frames
}
pub(super) fn sounds(bytes: &[u8], script: u16, animation: usize) -> BTreeSet<u16> {
    let mut sounds = BTreeSet::new();
    for instruction in instructions(bytes, script, animation) {
        let args = &instruction.args;
        match instruction.op {
            24 => {
                sounds.insert(word(args, 0));
            }
            25 | 28 => {
                sounds.extend(args[1..].as_chunks::<2>().0.iter().map(|s| word(s, 0)));
            }
            26 => {
                sounds.extend(word(args, 0)..=word(args, 2).min(word(args, 0) + 15));
            }
            _ => {}
        }
    }
    sounds
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn follows_calls_and_correct_overlay_sizes_without_reading_operands_as_ops() {
        let mut bytes = vec![0xff; 100];
        bytes[..8].copy_from_slice(&[7, 0, 8, 0, 255, 255, 0, 0]);
        bytes[8..20].copy_from_slice(&[b'S', b'C', b'P', b'E', 0, 0, 0, 0, 20, 0, 20, 0]);
        bytes[20..34].copy_from_slice(&[11, 22, 0, 12, 13, 22, 0, 0, 0, 53, 40, 0, 22, 0]);
        bytes[40..53].copy_from_slice(&[24, 98, 0, 43, 7, 0, 3, 0, 5, 2, 54, 22, 0]);
        assert_eq!(timeline(&bytes, 7, 0), vec![3, 3]);
        assert_eq!(sounds(&bytes, 7, 0), BTreeSet::from([98]));
    }
}
