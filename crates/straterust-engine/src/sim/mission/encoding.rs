use super::*;

pub(in crate::sim) fn put_location(bytes: &mut Vec<u8>, location: MissionLocation) {
    bytes.push(location.excluded_elevations);
    for value in [location.left, location.top, location.right, location.bottom] {
        bytes.extend(value.to_le_bytes());
    }
}
pub(in crate::sim) fn put_players(bytes: &mut Vec<u8>, players: &[PlayerId]) {
    bytes.extend((players.len() as u32).to_le_bytes());
    for player in players {
        bytes.extend(player.0.to_le_bytes());
    }
}
pub(in crate::sim) fn put_units(bytes: &mut Vec<u8>, units: MissionUnits) {
    match units {
        MissionUnits::Men => bytes.push(3),
        MissionUnits::Any => bytes.push(0),
        MissionUnits::Structures => bytes.push(1),
        MissionUnits::Type(id) => {
            bytes.push(2);
            bytes.extend(id.0.to_le_bytes());
        }
    }
}
pub(in crate::sim) fn put_comparison(bytes: &mut Vec<u8>, comparison: MissionComparison) {
    bytes.push(match comparison {
        MissionComparison::AtLeast => 0,
        MissionComparison::AtMost => 1,
        MissionComparison::Exactly => 2,
    });
}
pub(in crate::sim) fn put_mission_definition(bytes: &mut Vec<u8>, mission: &Option<Mission>) {
    bytes.push(u8::from(mission.is_some()));
    let Some(mission) = mission else {
        return;
    };
    bytes.extend(mission.player.0.to_le_bytes());
    bytes.extend(mission.poll_ticks.to_le_bytes());
    bytes.extend(mission.wait_step_ms.to_le_bytes());
    put_players(bytes, &mission.rescuable_players);
    put_players(bytes, &mission.rescuers);
    bytes.extend((mission.alliances.len() as u32).to_le_bytes());
    for pair in &mission.alliances {
        put_players(bytes, pair);
    }
    bytes.extend((mission.locations.len() as u32).to_le_bytes());
    for location in &mission.locations {
        put_location(bytes, *location);
    }
    bytes.extend((mission.triggers.len() as u32).to_le_bytes());
    for trigger in &mission.triggers {
        bytes.extend((trigger.conditions.len() as u32).to_le_bytes());
        for condition in &trigger.conditions {
            match condition {
                MissionCondition::Resources {
                    players,
                    kinds,
                    comparison,
                    amount,
                } => {
                    bytes.push(5);
                    put_players(bytes, players);
                    bytes.extend((kinds.len() as u32).to_le_bytes());
                    for kind in kinds {
                        put_string(bytes, kind);
                    }
                    bytes.push(match comparison {
                        MissionComparison::AtLeast => 0,
                        MissionComparison::AtMost => 1,
                        MissionComparison::Exactly => 2,
                    });
                    bytes.extend(amount.to_le_bytes());
                }
                MissionCondition::RankedCount {
                    player,
                    units,
                    location,
                    most,
                } => {
                    bytes.push(7);
                    bytes.extend(player.0.to_le_bytes());
                    put_units(bytes, *units);
                    bytes.extend(location.to_le_bytes());
                    bytes.push(u8::from(*most));
                }
                MissionCondition::Deaths {
                    players,
                    units,
                    comparison,
                    amount,
                }
                | MissionCondition::Kills {
                    players,
                    units,
                    comparison,
                    amount,
                } => {
                    bytes.push(if matches!(condition, MissionCondition::Deaths { .. }) {
                        6
                    } else {
                        4
                    });
                    put_players(bytes, players);
                    put_units(bytes, *units);
                    bytes.push(match comparison {
                        MissionComparison::AtLeast => 0,
                        MissionComparison::AtMost => 1,
                        MissionComparison::Exactly => 2,
                    });
                    bytes.extend(amount.to_le_bytes());
                }
                MissionCondition::Elapsed {
                    comparison,
                    milliseconds,
                } => {
                    bytes.push(3);
                    bytes.push(match comparison {
                        MissionComparison::AtLeast => 0,
                        MissionComparison::AtMost => 1,
                        MissionComparison::Exactly => 2,
                    });
                    bytes.extend(milliseconds.to_le_bytes());
                }
                MissionCondition::Countdown {
                    comparison,
                    milliseconds,
                } => {
                    bytes.push(0);
                    put_comparison(bytes, *comparison);
                    bytes.extend(milliseconds.to_le_bytes());
                }
                MissionCondition::Count {
                    players,
                    units,
                    location,
                    comparison,
                    amount,
                } => {
                    bytes.push(1);
                    put_players(bytes, players);
                    put_units(bytes, *units);
                    bytes.push(u8::from(location.is_some()));
                    if let Some(id) = location {
                        bytes.extend(id.to_le_bytes());
                    }
                    put_comparison(bytes, *comparison);
                    bytes.extend(amount.to_le_bytes());
                }
                MissionCondition::Switch { index, set } => {
                    bytes.push(2);
                    bytes.extend(index.to_le_bytes());
                    bytes.push(u8::from(*set));
                }
            }
        }
        bytes.extend((trigger.actions.len() as u32).to_le_bytes());
        for action in &trigger.actions {
            match action {
                MissionAction::GrantResearch { player, research } => {
                    bytes.push(28);
                    bytes.extend(player.0.to_le_bytes());
                    bytes.extend(research.0.to_le_bytes());
                }
                MissionAction::Resume => bytes.push(12),
                MissionAction::Preserve => bytes.push(13),
                MissionAction::Cosmetic => bytes.push(10),
                MissionAction::Countdown { milliseconds } => {
                    bytes.push(14);
                    bytes.extend(milliseconds.to_le_bytes());
                }
                MissionAction::Rescue { players } | MissionAction::Assault { players } => {
                    bytes.push(if matches!(action, MissionAction::Rescue { .. }) {
                        15
                    } else {
                        16
                    });
                    put_players(bytes, players);
                }
                MissionAction::EnterBunkers { players, location } => {
                    bytes.push(17);
                    put_players(bytes, players);
                    bytes.extend(location.to_le_bytes());
                }
                MissionAction::Remove {
                    players,
                    units,
                    location,
                } => {
                    bytes.push(18);
                    put_players(bytes, players);
                    put_units(bytes, *units);
                    bytes.push(u8::from(location.is_some()));
                    if let Some(location) = location {
                        bytes.extend(location.to_le_bytes());
                    }
                }
                MissionAction::Teleport {
                    players,
                    units,
                    location,
                    destination,
                } => {
                    bytes.push(19);
                    put_players(bytes, players);
                    put_units(bytes, *units);
                    bytes.extend(location.to_le_bytes());
                    bytes.extend(destination.to_le_bytes());
                }
                MissionAction::ToggleDoodad {
                    players,
                    units,
                    location,
                    enabled,
                } => {
                    bytes.push(20);
                    put_players(bytes, players);
                    put_units(bytes, *units);
                    bytes.extend(location.to_le_bytes());
                    bytes.push(enabled.map_or(0, |enabled| if enabled { 2 } else { 1 }));
                }
                MissionAction::StartAi { controller } => {
                    bytes.push(11);
                    bytes.extend(controller.to_le_bytes());
                }
                MissionAction::Victory => bytes.push(0),
                MissionAction::Defeat => bytes.push(1),
                MissionAction::Wait { milliseconds }
                | MissionAction::Transmission { milliseconds, .. } => {
                    bytes.push(2);
                    bytes.extend(milliseconds.to_le_bytes());
                }
                MissionAction::Pause => bytes.push(3),
                MissionAction::SetSwitch { index, set } => {
                    bytes.push(4);
                    bytes.extend(index.to_le_bytes());
                    bytes.push(u8::from(*set));
                }
                MissionAction::SetResources { players, resources } => {
                    bytes.push(5);
                    put_players(bytes, players);
                    bytes.extend((resources.len() as u32).to_le_bytes());
                    for amount in resources {
                        put_string(bytes, &amount.kind);
                        bytes.extend(amount.amount.to_le_bytes());
                    }
                }
                MissionAction::Create {
                    player,
                    unit_type,
                    location,
                    properties,
                } => {
                    bytes.push(6);
                    bytes.extend(player.0.to_le_bytes());
                    bytes.extend(unit_type.0.to_le_bytes());
                    bytes.extend(location.to_le_bytes());
                    for percent in [
                        properties.hp_percent,
                        properties.shield_percent,
                        properties.energy_percent,
                    ] {
                        bytes.push(u8::from(percent.is_some()));
                        bytes.push(percent.unwrap_or(0));
                    }
                    bytes.push(u8::from(properties.invincible));
                    bytes.push(u8::from(properties.cloaked));
                }
                MissionAction::Kill {
                    players,
                    units,
                    location,
                } => {
                    bytes.push(7);
                    put_players(bytes, players);
                    put_units(bytes, *units);
                    bytes.extend(location.to_le_bytes());
                }
                MissionAction::MoveLocation {
                    location,
                    players,
                    units,
                    search_location,
                } => {
                    bytes.push(8);
                    bytes.extend(location.to_le_bytes());
                    put_players(bytes, players);
                    put_units(bytes, *units);
                    bytes.extend(search_location.to_le_bytes());
                }
                MissionAction::Invincibility {
                    players,
                    units,
                    location,
                    enabled,
                } => {
                    bytes.push(9);
                    put_players(bytes, players);
                    put_units(bytes, *units);
                    bytes.extend(location.to_le_bytes());
                    bytes.push(u8::from(*enabled));
                }
                // These actions are all gameplay no-ops; neither cosmetic type nor IDs
                // affect authoritative content identity, but their PC slots remain.
                MissionAction::Objectives { .. }
                | MissionAction::Text { .. }
                | MissionAction::Sound { .. }
                | MissionAction::CenterView { .. }
                | MissionAction::Speech { .. } => bytes.push(10),
            }
        }
    }
}
pub(in crate::sim) fn put_mission_state(bytes: &mut Vec<u8>, state: &Option<MissionState>) {
    bytes.push(u8::from(state.is_some()));
    let Some(state) = state else {
        return;
    };
    bytes.extend((state.triggers.len() as u32).to_le_bytes());
    for trigger in &state.triggers {
        bytes.push(u8::from(trigger.preserve));
        bytes.extend(trigger.action.to_le_bytes());
        bytes.push(u8::from(trigger.started));
        bytes.push(u8::from(trigger.complete));
    }
    put_players(bytes, &state.rescue_players);
    bytes.extend((state.switches.len() as u32).to_le_bytes());
    for switch in &state.switches {
        bytes.push(u8::from(*switch));
    }
    bytes.extend((state.locations.len() as u32).to_le_bytes());
    for location in &state.locations {
        put_location(bytes, *location);
    }
    bytes.extend(state.countdown_ms.to_le_bytes());
    bytes.extend(state.poll_remaining.to_le_bytes());
    bytes.push(u8::from(state.paused));
    bytes.push(u8::from(state.wait.is_some()));
    if let Some(wait) = &state.wait {
        bytes.extend(wait.trigger.to_le_bytes());
        bytes.extend(wait.remaining_ms.to_le_bytes());
    }
}
