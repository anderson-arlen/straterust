use super::*;
use crate::chat::Chat;
use straterust_engine::net::chat::Channel;

pub(super) fn paint(
    canvas: &mut Canvas<'_, '_>,
    chat: &Chat,
    rect: [f64; 4],
    scale: f64,
    lobby: bool,
) {
    let [x, y, w, h] = rect;
    let columns = ((w / scale - 16.0) / 8.0).floor().max(8.0) as usize;
    let footer = if chat.open {
        56.0
    } else if lobby {
        26.0
    } else {
        0.0
    } * scale;
    let rows = ((h - footer - 16.0 * scale) / (12.0 * scale))
        .floor()
        .max(1.0) as usize;
    let rows = if !lobby && !chat.open {
        rows.min(6)
    } else {
        rows
    };
    let lines = chat
        .history
        .iter()
        .take(chat.history.len().saturating_sub(chat.scroll))
        .filter(|(time, _)| lobby || chat.open || time.elapsed().as_secs() < 12)
        .flat_map(|(_, m)| {
            let line = format!("[{}] {}: {}", m.channel.label(), m.name, m.text);
            let color = if m.channel == Channel::Team {
                0x89dfb0
            } else {
                0xe2eee4
            };
            wrapped_lines(&line, columns)
                .into_iter()
                .map(move |s| (s, color))
        })
        .collect::<Vec<_>>();
    if lines.is_empty() && !lobby && !chat.open {
        return;
    }
    let end = lines.len();
    let start = end.saturating_sub(rows);
    let visible = &lines[start..end];
    let height = if lobby || chat.open {
        h
    } else {
        (visible.len() * 12 + 16) as f64 * scale
    };
    canvas.rect(x, y, w, height, 0x09141d);
    canvas.outline(x, y, w, height, 0x526779);
    for (i, (line, color)) in visible.iter().enumerate() {
        canvas.text(
            line,
            x + 8.0 * scale,
            y + (8 + i * 12) as f64 * scale,
            scale,
            *color,
        );
    }
    if chat.open {
        let input_y = y + h - 48.0 * scale;
        canvas.rect(
            x + 4.0 * scale,
            input_y,
            w - 8.0 * scale,
            32.0 * scale,
            0x172431,
        );
        let prefix = format!("{}: ", chat.channel.label());
        let width = columns.saturating_sub(prefix.len() + 1);
        let tail = chat
            .draft
            .chars()
            .skip(chat.draft.chars().count().saturating_sub(width))
            .collect::<String>();
        canvas.text(
            &format!("{prefix}{tail}_"),
            x + 8.0 * scale,
            input_y + 6.0 * scale,
            scale,
            0xffe49e,
        );
        canvas.text(
            "Enter send | Tab All/Team",
            x + 8.0 * scale,
            input_y + 19.0 * scale,
            scale,
            0xb6c9d6,
        );
        canvas.text(
            "Esc cancel | PgUp/PgDn history",
            x + 8.0 * scale,
            y + h - 12.0 * scale,
            scale,
            0xb6c9d6,
        );
    } else if lobby {
        canvas.text(
            "Enter to chat",
            x + 8.0 * scale,
            y + h - 18.0 * scale,
            scale,
            0xffe49e,
        );
    }
}

pub(super) fn paint_game(canvas: &mut Canvas<'_, '_>, chat: &Chat, camera: Camera, size: [f64; 2]) {
    let [l, t, r, b] = camera.viewport_bounds(size);
    let rect = [
        l + 8.0,
        t + 8.0,
        (r - l - 16.0).min(560.0),
        (b - t - 16.0).min(220.0),
    ];
    paint(canvas, chat, rect, 1.0, false);
}
