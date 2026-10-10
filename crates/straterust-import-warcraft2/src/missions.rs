//! Campaign goals authored as native triggers. Warcraft II stores its campaign
//! objective programs in the game executable, separate from the PUD maps.
use super::{
    pud::{Pud, word},
    stats::id,
};
use anyhow::{Result, ensure};
use straterust_engine::sim::*;

fn count(
    players: Vec<PlayerId>,
    unit: Option<usize>,
    comparison: MissionComparison,
    amount: u32,
) -> MissionCondition {
    MissionCondition::Count {
        players,
        units: unit.map_or(MissionUnits::Any, |u| MissionUnits::Type(id(u))),
        location: None,
        comparison,
        amount,
    }
}
fn trigger(mission: &mut Mission, conditions: Vec<MissionCondition>, action: MissionAction) {
    mission.triggers.push(MissionTrigger {
        conditions,
        actions: vec![action],
    });
}
fn own(unit: usize, amount: u32) -> MissionCondition {
    count(
        vec![PlayerId(0)],
        Some(unit),
        MissionComparison::AtLeast,
        amount,
    )
}
fn absent(pud: &Pud, players: &[usize], unit: Option<usize>) -> MissionCondition {
    count(
        players
            .iter()
            .map(|p| PlayerId(u16::from(pud.player(*p))))
            .collect(),
        unit,
        MissionComparison::Exactly,
        0,
    )
}
fn survive(mission: &mut Mission, units: &[usize]) {
    for unit in units {
        trigger(
            mission,
            vec![count(
                (0..16).map(PlayerId).collect(),
                Some(*unit),
                MissionComparison::Exactly,
                0,
            )],
            MissionAction::Defeat,
        );
    }
}
fn location(mission: &mut Mission, rect: [i32; 4]) -> u16 {
    let id = mission.locations.len() as u16;
    mission.locations.push(MissionLocation {
        left: rect[0],
        top: rect[1],
        right: rect[2],
        bottom: rect[3],
        excluded_elevations: 0,
    });
    id
}
fn at(unit: usize, amount: u32, location: u16) -> MissionCondition {
    MissionCondition::Count {
        players: vec![PlayerId(0)],
        units: MissionUnits::Type(id(unit)),
        location: Some(location),
        comparison: MissionComparison::AtLeast,
        amount,
    }
}
fn escort(
    mission: &mut Mission,
    pud: &Pud,
    units: &[(usize, u32)],
    extra: &[MissionCondition],
) -> Result<()> {
    let circles: Vec<_> = pud.units.iter().filter(|u| u.kind == 100).collect();
    ensure!(
        !circles.is_empty(),
        "escort mission has no destination circle"
    );
    for circle in circles {
        let (x, y) = (i32::from(circle.x) * 32 + 32, i32::from(circle.y) * 32 + 32);
        let loc = location(
            mission,
            [
                (x - 64).max(0),
                (y - 64).max(0),
                (x + 64).min(i32::from(pud.width) * 32),
                (y + 64).min(i32::from(pud.height) * 32),
            ],
        );
        let mut conditions = extra.to_vec();
        conditions.extend(units.iter().map(|(u, n)| at(*u, *n, loc)));
        trigger(mission, conditions, MissionAction::Victory);
    }
    Ok(())
}

pub fn convert(pud: &Pud, number: usize, expansion: bool) -> Result<Mission> {
    let own_player = PlayerId(0);
    let mut mission = Mission {
        schema_version: 1,
        player: own_player,
        rescuable_players: Vec::new(),
        rescuers: vec![own_player],
        alliances: Vec::new(),
        poll_ticks: 1,
        wait_step_ms: 33,
        locations: vec![MissionLocation {
            left: 0,
            top: 0,
            right: i32::from(pud.width) * 32,
            bottom: i32::from(pud.height) * 32,
            excluded_elevations: 0,
        }],
        triggers: Vec::new(),
    };
    let mut resources = Vec::new();
    let mut enemies = Vec::new();
    for source in 0..16 {
        let player = PlayerId(u16::from(pud.player(source)));
        if source != pud.local && pud.owners[source] == 4 {
            enemies.push(player);
        }
        if matches!(pud.owners[source], 6 | 7) {
            mission.rescuable_players.push(player);
            mission.alliances.push([own_player, player]);
        }
        resources.push(MissionAction::SetResources {
            players: vec![player],
            resources: [("gold", b"SGLD"), ("wood", b"SLBR"), ("oil", b"SOIL")]
                .into_iter()
                .map(|(kind, name)| {
                    Ok(ResourceAmount {
                        kind: kind.into(),
                        amount: u32::from(word(&pud.chunks[name], source * 2)?),
                    })
                })
                .collect::<Result<_>>()?,
        });
    }
    mission.triggers.push(MissionTrigger {
        conditions: vec![MissionCondition::Elapsed {
            comparison: MissionComparison::AtLeast,
            milliseconds: 0,
        }],
        actions: resources,
    });
    let race = usize::from(pud.sides[pud.local]);
    ensure!(race < 2, "invalid campaign race");
    // Computer factions on the opposing side cooperate. The Alterac peasant
    // revolt is the campaign's explicit exception.
    for (i, a) in enemies.iter().enumerate() {
        for b in enemies.iter().skip(i + 1) {
            let source_a = usize::from(pud.player(usize::from(a.0)));
            let source_b = usize::from(pud.player(usize::from(b.0)));
            let revolt = !expansion
                && race == 0
                && number == 8
                && (source_a == 4 && matches!(source_b, 2 | 6)
                    || source_b == 4 && matches!(source_a, 2 | 6));
            if !revolt {
                mission.alliances.push([(*a).min(*b), (*a).max(*b)]);
            }
        }
    }
    let eliminated = count(enemies, None, MissionComparison::Exactly, 0);
    let victory = match (expansion, race, number) {
        (false, _, 1) => vec![own(58 + race, 4), own(60 + race, 1)],
        (false, 0, 2) => {
            let extra = vec![absent(pud, &[6], Some(8)), absent(pud, &[6], Some(18))];
            escort(&mut mission, pud, &[(8, 1)], &extra)?;
            escort(&mut mission, pud, &[(18, 1)], &extra)?;
            Vec::new()
        }
        (false, 1, 2) => {
            escort(
                &mut mission,
                pud,
                &[(53, 1)],
                &[absent(pud, &[2], Some(53))],
            )?;
            survive(&mut mission, &[53]);
            Vec::new()
        }
        (false, _, 3) => vec![own(72 + race, 1), own(86 + race, 4)],
        (false, 0, 7) => vec![absent(pud, &[2], Some(85))],
        (false, 0, 8) => vec![own(90, 1), absent(pud, &[2, 4], Some(2)), eliminated],
        (false, 0, 9) => {
            escort(&mut mission, pud, &[(52, 1)], &[])?;
            survive(&mut mission, &[52]);
            Vec::new()
        }
        (false, 0, 10) => {
            escort(&mut mission, pud, &[(16, 4)], &[])?;
            trigger(
                &mut mission,
                vec![count(
                    (0..16).map(PlayerId).collect(),
                    Some(16),
                    MissionComparison::AtMost,
                    3,
                )],
                MissionAction::Defeat,
            );
            Vec::new()
        }
        (false, 0, 11) => vec![absent(pud, &[3], None), eliminated],
        (false, 0, 12) => [29, 85, 73]
            .into_iter()
            .map(|u| absent(pud, &[0], Some(u)))
            .collect(),
        (false, 0, 14) | (true, 0, 12) => {
            if expansion {
                survive(&mut mission, &[24]);
            }
            vec![absent(pud, &[15], Some(101))]
        }
        (false, 1, 6) => {
            escort(&mut mission, pud, &[(49, 1)], &[])?;
            survive(&mut mission, &[49]);
            Vec::new()
        }
        (false, 1, 8) => vec![absent(pud, &[7], Some(102)), absent(pud, &[1], Some(90))],
        (false, 1, 9) => {
            let fortress = location(&mut mission, [52 * 32, 15 * 32, 73 * 32, 41 * 32]);
            for rect in [
                [48, 20, 70, 43],
                [69, 31, 75, 40],
                [73, 13, 77, 35],
                [51, 16, 59, 21],
                [55, 12, 76, 17],
            ] {
                let shipyard = location(&mut mission, rect.map(|v| v * 32));
                trigger(
                    &mut mission,
                    vec![at(91, 1, fortress), at(73, 1, shipyard)],
                    MissionAction::Victory,
                );
            }
            Vec::new()
        }
        (true, 0, 1) => {
            escort(&mut mission, pud, &[(20, 1), (46, 1), (44, 1)], &[])?;
            survive(&mut mission, &[20, 46, 44]);
            Vec::new()
        }
        (true, 0, 2) => {
            survive(&mut mission, &[46]);
            vec![own(46, 1), eliminated]
        }
        (true, 0, 3) => {
            let extra = [89, 91]
                .into_iter()
                .map(|u| absent(pud, &[0, 3, 4], Some(u)))
                .collect::<Vec<_>>();
            escort(&mut mission, pud, &[(44, 1)], &extra)?;
            survive(&mut mission, &[44]);
            Vec::new()
        }
        (true, 0, 4) => vec![own(90, 1), eliminated],
        (true, 0, 5) => vec![own(72, 3), absent(pud, &[2, 3, 4, 7], Some(73))],
        (true, 0, 6) => {
            escort(
                &mut mission,
                pud,
                &[(46, 1), (44, 1)],
                &[absent(pud, &[5], None)],
            )?;
            survive(&mut mission, &[46, 44]);
            Vec::new()
        }
        (true, 0, 7) => {
            survive(&mut mission, &[24, 20, 22]);
            vec![
                absent(pud, &[2, 5, 6], Some(35)),
                absent(pud, &[2, 5, 6], Some(71)),
            ]
        }
        (true, 0, 8) => vec![absent(pud, &[3, 6, 7], None)],
        (true, 0, 9) => vec![absent(pud, &[4], Some(102)), absent(pud, &[5], Some(91))],
        (true, 0, 10) => {
            survive(&mut mission, &[24, 20, 22, 46, 44]);
            vec![absent(pud, &[2, 3, 6], None)]
        }
        (true, 0, 11) => {
            survive(&mut mission, &[24, 20, 44]);
            vec![absent(pud, &[2, 3], None)]
        }
        (true, 1, 1) => {
            survive(&mut mission, &[25]);
            vec![
                own(25, 1),
                absent(pud, &[6], Some(11)),
                absent(pud, &[6], Some(81)),
            ]
        }
        (true, 1, 2) => {
            survive(&mut mission, &[47, 23]);
            vec![own(47, 1), own(23, 1), eliminated]
        }
        (true, 1, 4 | 6) => {
            survive(&mut mission, &[21]);
            vec![own(21, 1), eliminated]
        }
        (true, 1, 5) => vec![own(71, 1)],
        (true, 1, 7) => vec![
            own(73, 5),
            absent(pud, &[1, 2], Some(32)),
            absent(pud, &[1, 2], Some(30)),
            absent(pud, &[1, 2], Some(28)),
        ],
        (true, 1, 9) => vec![absent(pud, &[1], Some(56))],
        (true, 1, 10) => {
            escort(&mut mission, pud, &[(10, 1)], &[eliminated])?;
            Vec::new()
        }
        (true, 1, 11) => vec![absent(pud, &[3], None), absent(pud, &[6], Some(80))],
        (false, 0, 4..=6 | 13) | (false, 1, 4 | 5 | 7 | 10..=14) | (true, 1, 3 | 8 | 12) => {
            vec![eliminated]
        }
        _ => anyhow::bail!(
            "missing campaign objective for race {race}, mission {number}, expansion {expansion}"
        ),
    };
    if !victory.is_empty() {
        trigger(&mut mission, victory, MissionAction::Victory);
    }
    trigger(
        &mut mission,
        vec![count(vec![own_player], None, MissionComparison::Exactly, 0)],
        MissionAction::Defeat,
    );
    Ok(mission)
}
