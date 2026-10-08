//! Source trigger and player references converted to bounded native missions.
use super::*;

#[allow(clippy::too_many_arguments)]
pub(super) fn translate(
    triggers: &[SourceTrigger],
    properties: &[u8],
    locations: &BTreeMap<u16, MissionLocation>,
    refs: &References,
    source_players: &[u8],
    ids: &BTreeMap<u8, PlayerId>,
    forces: &[u8],
    ai: &[u8],
    map: &mut Map,
    rescue: Vec<PlayerId>,
    alliances: Vec<[PlayerId; 2]>,
) -> Result<Mission> {
    // Early retail scenarios omit the four optional force flag bytes.
    ensure!(
        matches!(forces.len(), 16 | 20) && forces[..8].iter().all(|f| *f < 4),
        "invalid campaign force records"
    );
    // Conditions can count an unused slot (usually expecting zero). Give
    // explicitly referenced slots empty native players, without running
    // triggers or AI for those absent participants.
    let mut ids = ids.clone();
    for player in triggers.iter().flat_map(|t| {
        t.conditions
            .iter()
            .filter(|c| matches!(c.kind, 2..=5 | 15))
            .map(|c| c.player)
            .chain(
                t.actions
                    .iter()
                    .filter(|a| matches!(a.kind, 11 | 22 | 23 | 25 | 26 | 38 | 39 | 42 | 43))
                    .map(|a| a.player),
            )
    }) {
        if player < 12 && !ids.contains_key(&(player as u8)) {
            ids.insert(player as u8, PlayerId(map.players));
            map.players += 1;
        }
    }
    let indices: BTreeMap<_, _> = locations
        .keys()
        .enumerate()
        .map(|(i, id)| (*id, i as u16))
        .collect();
    let loc = |id: u32| -> Result<u16> {
        indices
            .get(&u16::try_from(id)?)
            .copied()
            .context("missing campaign location")
    };
    let unit = |id: u16| -> Result<UnitTypeId> {
        campaign_units::native_id(id)
            .with_context(|| format!("unsupported campaign unit reference {id}"))
    };
    let filter = |id: u16| -> Result<MissionUnits> {
        Ok(match id {
            229 => MissionUnits::Any,
            230 => MissionUnits::Men,
            231 => MissionUnits::Structures,
            _ => MissionUnits::Type(unit(id)?),
        })
    };
    let resolve = |source: u32, owner: u8| -> Result<Vec<PlayerId>> {
        if source < 12 {
            return ids
                .get(&(source as u8))
                .map(|id| vec![*id])
                .context("missing explicit campaign player");
        }
        let mut p: Vec<_> = ids
            .keys()
            .filter(|p| match source {
                13 => **p == owner,
                // Campaign alliances are fixed; passive/rescuable players and
                // neutral objects never count towards eliminating opponents.
                14 | 26 => {
                    source_players.contains(p)
                        && **p != owner
                        && !alliances
                            .iter()
                            .any(|pair| pair.contains(&ids[&owner]) && pair.contains(&ids[p]))
                }
                17 => true,
                18..=21 => **p < 8 && u32::from(forces[usize::from(**p)]) == source - 18,
                _ => u32::from(**p) == source,
            })
            .map(|p| ids[p])
            .collect();
        p.sort();
        ensure!(!p.is_empty(), "empty campaign player reference {source}");
        Ok(p)
    };
    let mut native = Vec::new();
    for trigger in triggers {
        if trigger.actions.is_empty() {
            continue;
        }
        let owners: BTreeSet<_> = trigger
            .owners
            .iter()
            .flat_map(|p| resolve(u32::from(*p), source_players[0]).unwrap_or_default())
            .filter_map(|id| source_players.iter().find(|p| ids[p] == id).copied())
            .filter(|p| *p < 8)
            .collect();
        for owner in owners {
            let mut conditions = Vec::new();
            for c in &trigger.conditions {
                conditions.push(match c.kind {
                    1 => MissionCondition::Countdown {
                        comparison: backwater::comparison(c.comparison)?,
                        milliseconds: c
                            .amount
                            .checked_mul(1000)
                            .context("source countdown exceeds native timer range")?,
                    },
                    2 | 3 => MissionCondition::Count {
                        players: resolve(c.player, owner)?,
                        units: filter(c.unit)?,
                        location: if c.kind == 3 {
                            Some(loc(c.location)?)
                        } else {
                            None
                        },
                        comparison: backwater::comparison(c.comparison)?,
                        amount: c.amount,
                    },
                    4 => MissionCondition::Resources {
                        players: resolve(c.player, owner)?,
                        kinds: ["minerals", "gas"]
                            .into_iter()
                            .enumerate()
                            .filter(|(kind, _)| c.unit == 2 || c.unit == *kind as u16)
                            .map(|(_, kind)| kind.to_owned())
                            .collect(),
                        comparison: backwater::comparison(c.comparison)?,
                        amount: c.amount,
                    },
                    7 | 17 => MissionCondition::RankedCount {
                        player: ids[&owner],
                        units: filter(c.unit)?,
                        location: loc(c.location)?,
                        most: c.kind == 7,
                    },
                    15 => MissionCondition::Deaths {
                        players: resolve(c.player, owner)?,
                        units: filter(c.unit)?,
                        comparison: backwater::comparison(c.comparison)?,
                        amount: c.amount,
                    },
                    5 => MissionCondition::Kills {
                        players: resolve(c.player, owner)?,
                        units: filter(c.unit)?,
                        comparison: backwater::comparison(c.comparison)?,
                        amount: c.amount,
                    },
                    11 => MissionCondition::Switch {
                        index: u16::from(c.switch),
                        set: c.comparison == 2,
                    },
                    12 => MissionCondition::Elapsed {
                        comparison: backwater::comparison(c.comparison)?,
                        milliseconds: c
                            .amount
                            .checked_mul(1000)
                            .context("source elapsed guard exceeds native timer range")?,
                    },
                    _ => bail!("unsupported campaign condition {}", c.kind),
                });
            }
            if conditions.is_empty() {
                conditions.push(MissionCondition::Elapsed {
                    comparison: MissionComparison::AtLeast,
                    milliseconds: 0,
                });
            }
            let mut actions = Vec::new();
            for a in &trigger.actions {
                let action = match a.kind {
                    1 => MissionAction::Victory,
                    2 => MissionAction::Defeat,
                    3 => MissionAction::Preserve,
                    4 => MissionAction::Wait {
                        milliseconds: a.time,
                    },
                    5 => MissionAction::Pause,
                    6 => MissionAction::Resume,
                    7 => MissionAction::Transmission {
                        text: refs.text(a.text)?,
                        sound: if a.sound == 0 {
                            None
                        } else {
                            Some(refs.sound(a.sound)?)
                        },
                        portrait: if matches!(a.unit, 23 | 29) {
                            UnitTypeId(1000)
                        } else if a.unit == 27 {
                            UnitTypeId(1001)
                        } else {
                            unit(a.unit)?
                        },
                        location: loc(a.location)?,
                        milliseconds: backwater::duration(a)?,
                    },
                    8 => MissionAction::Sound {
                        sound: refs.sound(a.sound)?,
                    },
                    9 => MissionAction::Text {
                        text: refs.text(a.text)?,
                    },
                    10 => MissionAction::CenterView {
                        location: loc(a.location)?,
                    },
                    11 => {
                        ensure!(
                            a.modifier == 0 && (a.second == 0 || a.flags & 8 != 0),
                            "unsupported campaign create properties"
                        );
                        for player in resolve(a.player, owner)? {
                            actions.push(MissionAction::Create {
                                player,
                                unit_type: unit(a.unit)?,
                                location: loc(a.location)?,
                                properties: {
                                    let mut properties = created_properties(properties, a.second)?;
                                    if matches!(a.unit, 74 | 75) {
                                        properties.cloaked = true;
                                    }
                                    properties
                                },
                            });
                        }
                        continue;
                    }
                    12 => MissionAction::Objectives {
                        text: refs.text(a.text)?,
                    },
                    13 => MissionAction::SetSwitch {
                        index: a.second as u16,
                        set: a.modifier == 4,
                    },
                    14 => {
                        ensure!(a.modifier == 7, "unsupported countdown arithmetic");
                        MissionAction::Countdown {
                            milliseconds: a
                                .time
                                .checked_mul(1000)
                                .context("source countdown exceeds native timer range")?,
                        }
                    }
                    15 | 16 => {
                        let script = a.second.to_le_bytes();
                        let targets = resolve(13, owner)?;
                        match &script {
                            b"Suic" | b"SuiR" => MissionAction::Assault { players: targets },
                            b"Rscu" => MissionAction::Rescue { players: targets },
                            b"EnBk" => MissionAction::EnterBunkers {
                                players: targets,
                                location: loc(a.location)?,
                            },
                            b"ClrC" | b"VluA" => MissionAction::Cosmetic,
                            b"MvTe" => MissionAction::OrderMove {
                                players: targets,
                                units: MissionUnits::Type(unit(74)?),
                                destination: loc(a.location)?,
                            },
                            _ => {
                                let home = locations[&(a.location as u16)].center();
                                let index = map.ai.len() as u16;
                                map.ai.push(AiController {
                                    player: targets[0],
                                    home,
                                    radius: 640,
                                    active: false,
                                    program: crate::ai::translate(ai, script, MAPPING)?,
                                });
                                MissionAction::StartAi { controller: index }
                            }
                        }
                    }
                    17 | 28 | 32 => MissionAction::Cosmetic,
                    22 | 24 | 25 => MissionAction::Remove {
                        players: if a.kind == 24 {
                            resolve(17, owner)?
                        } else {
                            resolve(a.player, owner)?
                        },
                        units: filter(a.unit)?,
                        location: if a.kind == 25 {
                            Some(loc(a.location)?)
                        } else {
                            None
                        },
                    },
                    23 => MissionAction::Kill {
                        players: resolve(a.player, owner)?,
                        units: filter(a.unit)?,
                        location: loc(a.location)?,
                    },
                    26 => {
                        ensure!(a.modifier == 7, "unsupported resource arithmetic");
                        MissionAction::SetResources {
                            players: resolve(a.player, owner)?,
                            resources: [("minerals", 0), ("gas", 1)]
                                .into_iter()
                                .filter(|(_, kind)| a.unit == 2 || a.unit == *kind)
                                .map(|(kind, _)| ResourceAmount {
                                    kind: kind.into(),
                                    amount: a.second,
                                })
                                .collect(),
                        }
                    }
                    30 | 31 => MissionAction::Speech {
                        muted: a.kind == 30,
                    },
                    38 => MissionAction::MoveLocation {
                        location: loc(a.second)?,
                        players: resolve(a.player, owner)?,
                        units: filter(a.unit)?,
                        search_location: loc(a.location)?,
                    },
                    39 => MissionAction::Teleport {
                        players: resolve(a.player, owner)?,
                        units: filter(a.unit)?,
                        location: loc(a.location)?,
                        destination: loc(a.second)?,
                    },
                    42 => {
                        ensure!(
                            matches!(a.modifier, 0 | 4..=6),
                            "unsupported doodad state modifier"
                        );
                        MissionAction::ToggleDoodad {
                            players: resolve(a.player, owner)?,
                            units: filter(a.unit)?,
                            location: loc(a.location)?,
                            enabled: match a.modifier {
                                4 => Some(true),
                                5 => Some(false),
                                _ => None,
                            },
                        }
                    }
                    43 => MissionAction::Invincibility {
                        players: resolve(a.player, owner)?,
                        units: filter(a.unit)?,
                        location: loc(a.location)?,
                        enabled: a.modifier == 4,
                    },
                    _ => bail!("unsupported campaign action {}", a.kind),
                };
                actions.push(action);
            }
            native.push(MissionTrigger {
                conditions: conditions.clone(),
                actions,
            });
        }
    }
    Ok(Mission {
        schema_version: 1,
        player: PlayerId(0),
        poll_ticks: 31,
        wait_step_ms: 42,
        locations: locations.values().copied().collect(),
        triggers: native,
        rescuable_players: rescue,
        rescuers: vec![PlayerId(0)],
        alliances,
    })
}

pub(super) fn created_properties(bytes: &[u8], slot: u32) -> Result<UnitProperties> {
    if slot == 0 {
        return Ok(UnitProperties::default());
    }
    ensure!((1..=64).contains(&slot), "invalid source property slot");
    let r = bytes
        .get((slot as usize - 1) * 20..slot as usize * 20)
        .context("missing source unit properties")?;
    let states = short(r, 0) & short(r, 14);
    let valid = short(r, 2);
    ensure!(
        states & !26 == 0 && valid & !15 == 0 && word(r, 8) == 0,
        "unsupported created unit properties"
    );
    Ok(UnitProperties {
        illusion_ticks: (states & 8 != 0).then_some(1350),
        hp_percent: (valid & 2 != 0).then_some(r[5]),
        shield_percent: (valid & 4 != 0).then_some(r[6]),
        energy_percent: (valid & 8 != 0).then_some(r[7]),
        invincible: states & 16 != 0,
        cloaked: states & 2 != 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backwater::{SourceAction, SourceCondition};

    #[test]
    fn source_hallucinations_preserve_percentages_and_native_lifetime() {
        let mut record = [0; 20];
        record[..2].copy_from_slice(&24_u16.to_le_bytes());
        record[2..4].copy_from_slice(&14_u16.to_le_bytes());
        record[5..8].fill(100);
        record[14..16].copy_from_slice(&8_u16.to_le_bytes());
        let properties = created_properties(&record, 1).unwrap();
        assert_eq!(properties.illusion_ticks, Some(1350));
        assert_eq!(properties.hp_percent, Some(100));
        assert_eq!(properties.shield_percent, Some(100));
        record[14] = 1;
        record[0] = 1;
        assert!(created_properties(&record, 1).is_err());
    }

    #[test]
    fn opponent_conditions_exclude_self_rescuable_and_neutral_players() -> Result<()> {
        let mut map: Map = ron::de::from_str(include_str!("../../../../content/fixtures/map.ron"))?;
        let refs = References::collect(&[], &[], &[])?;
        for group in [14, 26, 1] {
            map.players = 4;
            let mission = translate(
                &[SourceTrigger {
                    owners: vec![3],
                    conditions: vec![SourceCondition {
                        location: 0,
                        player: group,
                        amount: 0,
                        unit: 231,
                        comparison: 1,
                        kind: 2,
                        switch: 0,
                        flags: 0,
                    }],
                    actions: vec![SourceAction {
                        location: 0,
                        text: 0,
                        sound: 0,
                        time: 0,
                        player: 0,
                        second: 0,
                        unit: 0,
                        kind: 1,
                        modifier: 0,
                        flags: 0,
                    }],
                }],
                &[],
                &BTreeMap::new(),
                &refs,
                &[3, 0, 5, 11],
                &BTreeMap::from([
                    (3, PlayerId(0)),
                    (0, PlayerId(1)),
                    (5, PlayerId(2)),
                    (11, PlayerId(3)),
                ]),
                &[0; 16],
                &[],
                &mut map,
                vec![PlayerId(2)],
                vec![[PlayerId(0), PlayerId(2)], [PlayerId(0), PlayerId(3)]],
            )?;
            let MissionCondition::Count { players, .. } = &mission.triggers[0].conditions[0] else {
                panic!("expected foe count");
            };
            assert_eq!(players, &[PlayerId(if group == 1 { 4 } else { 1 })]);
            assert_eq!(mission.triggers.len(), 1);
            assert_eq!(map.players, if group == 1 { 5 } else { 4 });
        }
        Ok(())
    }
}
