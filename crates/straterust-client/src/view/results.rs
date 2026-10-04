//! Shared postgame display for local and multiplayer server reports.
use super::*;
use crate::menus::MenuUi;
use straterust_engine::session::MatchOutcome;

pub(super) fn draw_results(canvas: &mut Canvas<'_, '_>, menu: &MenuUi, size: [f64; 2]) {
    let Some(result) = &menu.result else { return };
    let scale = (size[0] / 640.0).min(size[1] / 480.0);
    let origin = [
        (size[0] - 640.0 * scale) / 2.0,
        (size[1] - 480.0 * scale) / 2.0,
    ];
    let seconds = result.tick.0.saturating_mul(u64::from(result.tick_ms)) / 1000;
    let duration = format!("Duration: {}:{:02}", seconds / 60, seconds % 60);
    canvas.text(
        &duration,
        origin[0] + 60.0 * scale,
        origin[1] + 84.0 * scale,
        scale,
        0xb6c9d6,
    );
    canvas.rect(
        origin[0] + 48.0 * scale,
        origin[1] + 106.0 * scale,
        544.0 * scale,
        304.0 * scale,
        0x090f16,
    );
    canvas.outline(
        origin[0] + 48.0 * scale,
        origin[1] + 106.0 * scale,
        544.0 * scale,
        304.0 * scale,
        0x526779,
    );
    let column_width = 330.0 / result.players.len().max(1) as f64;
    for (index, player) in result.players.iter().enumerate() {
        let center = origin[0] + (250.0 + column_width * (index as f64 + 0.5)) * scale;
        let name = format!(
            "Player {}{}",
            player.player.0 + 1,
            if player.player == menu.result_player {
                " (You)"
            } else {
                ""
            }
        );
        canvas.text(
            &name,
            center - name.len() as f64 * 4.0 * scale,
            origin[1] + 120.0 * scale,
            scale,
            0xe2eee4,
        );
        let (outcome, color) = match player.outcome {
            MatchOutcome::Victory => ("Victory", 0x66e379),
            MatchOutcome::Defeat => ("Defeat", 0xe17065),
            MatchOutcome::Draw => ("Draw", 0xe4b957),
        };
        canvas.text(
            outcome,
            center - outcome.len() as f64 * 4.0 * scale,
            origin[1] + 138.0 * scale,
            scale,
            color,
        );
    }
    let mut row = |label: &str, y: f64, values: Vec<u64>, heading: bool| {
        canvas.text(
            label,
            origin[0] + 64.0 * scale,
            origin[1] + y * scale,
            scale,
            if heading { 0xf1cf73 } else { 0xb6c9d6 },
        );
        for (index, value) in values.iter().enumerate() {
            let text = value.to_string();
            let center = origin[0] + (250.0 + column_width * (index as f64 + 0.5)) * scale;
            canvas.text(
                &text,
                center - text.len() as f64 * 4.0 * scale,
                origin[1] + y * scale,
                scale,
                0xe2eee4,
            );
        }
    };
    row("Units", 162.0, vec![], true);
    for (label, y, field) in [
        ("Produced", 178.0, 0),
        ("Killed", 194.0, 1),
        ("Lost", 210.0, 2),
        ("Constructed", 246.0, 3),
        ("Razed", 262.0, 4),
        ("Lost", 278.0, 5),
    ] {
        if field == 3 {
            row("Structures", 230.0, vec![], true);
        }
        row(
            label,
            y,
            result
                .players
                .iter()
                .map(|p| {
                    let s = &p.statistics;
                    u64::from(match field {
                        0 => s.units_produced,
                        1 => s.units_killed,
                        2 => s.units_lost,
                        3 => s.structures_built,
                        4 => s.structures_razed,
                        _ => s.structures_lost,
                    })
                })
                .collect(),
            false,
        );
    }
    row("Resources gathered", 298.0, vec![], true);
    let kinds: BTreeSet<_> = result
        .players
        .iter()
        .flat_map(|p| p.statistics.resources_collected.keys())
        .collect();
    if kinds.is_empty() {
        row("Total", 314.0, vec![0; result.players.len()], false);
    }
    for (index, kind) in kinds.iter().take(4).enumerate() {
        row(
            kind,
            314.0 + index as f64 * 16.0,
            result
                .players
                .iter()
                .map(|p| {
                    p.statistics
                        .resources_collected
                        .get(*kind)
                        .copied()
                        .unwrap_or(0)
                })
                .collect(),
            false,
        );
    }
    if kinds.len() > 4 {
        row(
            "Other resources",
            378.0,
            result
                .players
                .iter()
                .map(|p| {
                    kinds
                        .iter()
                        .skip(4)
                        .map(|kind| {
                            p.statistics
                                .resources_collected
                                .get(*kind)
                                .copied()
                                .unwrap_or(0)
                        })
                        .fold(0_u64, u64::saturating_add)
                })
                .collect(),
            false,
        );
    }
}
