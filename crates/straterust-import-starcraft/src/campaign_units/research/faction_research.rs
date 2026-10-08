//! Campaign-enabled faction technologies, translated to shared native effects.
use super::*;

pub(crate) const SOURCES: &[(u16, bool, usize)] = &[
    (1, false, 7),
    (2, false, 0),
    (3, false, 16),
    (4, true, 0),
    (5, true, 9),
    (6, false, 22),
    (7, false, 17),
    (8, true, 3),
    (9, true, 11),
    (10, false, 24),
    (11, false, 26),
    (12, false, 27),
    (13, false, 29),
    (14, false, 30),
    (15, false, 33),
    (16, false, 34),
    (17, false, 37),
    (18, true, 2),
    (19, true, 7),
    (20, false, 19),
    (21, true, 5),
    (22, true, 1),
    (23, true, 10),
    (24, false, 20),
    (25, false, 21),
    (26, true, 8),
    (27, false, 23),
    (28, false, 25),
    (29, false, 28),
    (30, false, 31),
    (31, false, 32),
    (32, false, 35),
    (33, false, 36),
    (34, false, 38),
    (35, false, 39),
    (36, false, 40),
    (37, false, 41),
    (38, false, 42),
    (39, false, 43),
    (40, false, 44),
    (41, true, 13),
    (42, true, 17),
    (43, true, 15),
    (44, true, 16),
    (45, true, 19),
    (46, true, 20),
    (47, true, 21),
    (48, true, 22),
    (49, false, 1),
    (50, false, 2),
    (51, false, 3),
    (52, false, 4),
    (53, false, 5),
    (54, false, 6),
    (55, false, 8),
    (56, false, 9),
    (57, false, 10),
    (58, false, 11),
    (59, false, 12),
    (60, false, 13),
    (61, false, 14),
    (62, false, 15),
];

pub(crate) fn level_id(base: u16, level: u8) -> ResearchId {
    ResearchId(base + 200 * u16::from(level - 1))
}

pub(crate) fn available(
    sections: &crate::backwater::Sections<'_>,
    player: usize,
    technology: bool,
    source: usize,
) -> Result<(u8, u8)> {
    let (key, count, size) = if technology {
        ("PTEC", 24, 912)
    } else {
        ("UPGR", 46, 1748)
    };
    let flags = sections.exact(key, size)?;
    let defaults = 24 * count;
    Ok(
        if flags[defaults + 2 * count + player * count + source] != 0 {
            (flags[defaults + source], flags[defaults + count + source])
        } else {
            (
                flags[player * count + source],
                flags[12 * count + player * count + source],
            )
        },
    )
}
