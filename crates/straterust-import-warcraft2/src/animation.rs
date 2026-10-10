//! Source pose assignments. Ship sinking frames must never become walking poses.
use super::art::clip;
use straterust_engine::assets::*;

pub fn clips(graphic: usize, poses: usize, directional: bool, structure: bool) -> Vec<SpriteClip> {
    if !directional {
        let mut result = vec![clip(ClipKind::Idle, &[0], false, 100)];
        if structure && poses > 1 {
            result.push(clip(ClipKind::Construction, &[1], false, 100));
        }
        if structure
            && poses > 2
            && matches!(graphic, 114 | 115 | 156 | 157 | 177 | 178 | 501 | 502)
        {
            result.push(clip(ClipKind::Production, &[2], false, 100));
        }
        return result;
    }
    let (walk, attack, death): (Vec<usize>, Vec<usize>, Vec<usize>) = match graphic {
        39..=44 | 59..=62 | 182 | 183 => (
            vec![0],
            if matches!(graphic, 43 | 44 | 182 | 183) {
                vec![1, 2, 1, 0]
            } else {
                vec![0]
            },
            if matches!(graphic, 43 | 44 | 182 | 183) {
                vec![0]
            } else {
                vec![1, 2]
            },
        ),
        49 | 50 => (vec![0, 1], vec![2, 3, 3, 3, 3, 3, 0, 0], Vec::new()),
        33 | 34 => (
            vec![0, 2, 5, 8, 11],
            vec![3, 6, 9, 0],
            if graphic == 33 {
                vec![1, 4, 7, 10, 12]
            } else {
                vec![1, 4, 7, 10, 12, 14]
            },
        ),
        69 => (
            vec![0, 2, 5, 8, 11],
            vec![3, 6, 9, 12, 0],
            vec![1, 4, 7, 10, 13],
        ),
        35 => ((0..4).collect(), (0..7).collect(), (7..poses).collect()),
        36 => ((0..4).collect(), (0..5).collect(), (5..poses).collect()),
        37 | 38 | 63 => ((0..poses.min(4)).collect(), Vec::new(), Vec::new()),
        64..=66 | 470 => (vec![0], Vec::new(), (1..poses).collect()),
        53 => ((0..5).collect(), vec![5, 6, 0], (7..poses).collect()),
        47 | 48 => (
            (0..5).collect(),
            vec![5, 6, 7, 8, 9, 5],
            (10..poses).collect(),
        ),
        70 => ((0..5).collect(), (5..10).collect(), (10..poses).collect()),
        _ => (
            (0..poses.min(5)).collect(),
            (5..poses.min(9)).collect(),
            (9..poses).collect(),
        ),
    };
    let airborne = matches!(graphic, 35..=38 | 63);
    let mut result = vec![
        clip(
            ClipKind::Idle,
            if airborne { &walk } else { &[0] },
            true,
            198,
        ),
        clip(ClipKind::Walk, &walk, true, 99),
    ];
    if !attack.is_empty() {
        let mut attack_clip = clip(ClipKind::Attack, &attack, true, 99);
        attack_clip.key_steps = vec![0];
        result.push(attack_clip);
        result.push(clip(ClipKind::Cast, &attack, true, 99));
        if matches!(graphic, 47 | 48) {
            result.push(clip(ClipKind::Work, &attack, true, 99));
        }
    }
    if !death.is_empty() {
        if matches!(graphic, 39..=42 | 59..=62) {
            result.push(clip(ClipKind::Death, &death, true, 1650));
            return result;
        }
        let mut held = death.clone();
        held.extend(std::iter::repeat_n(*death.last().unwrap(), 32));
        result.push(clip(ClipKind::Death, &held, true, 99));
    }
    result
}
