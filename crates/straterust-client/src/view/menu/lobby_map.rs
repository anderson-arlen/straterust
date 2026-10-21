use super::*;

fn line(canvas: &mut Canvas<'_, '_>, from: [f64; 2], to: [f64; 2], width: f64, color: u32) {
    let steps = (from[0] - to[0])
        .abs()
        .max((from[1] - to[1]).abs())
        .ceil()
        .max(1.0) as usize;
    for i in 0..=steps {
        let t = i as f64 / steps as f64;
        canvas.rect(
            from[0] + (to[0] - from[0]) * t,
            from[1] + (to[1] - from[1]) * t,
            width,
            width,
            color,
        );
    }
}
fn readiness(canvas: &mut Canvas<'_, '_>, x: f64, y: f64, scale: f64, ready: bool) {
    if ready {
        line(
            canvas,
            [x, y + 5.0 * scale],
            [x + 3.0 * scale, y + 8.0 * scale],
            2.0 * scale,
            0x67e883,
        );
        line(
            canvas,
            [x + 3.0 * scale, y + 8.0 * scale],
            [x + 10.0 * scale, y],
            2.0 * scale,
            0x67e883,
        );
    } else {
        let c = 0xf1cf73;
        canvas.rect(x, y, 10.0 * scale, scale, c);
        canvas.rect(x, y + 10.0 * scale, 10.0 * scale, scale, c);
        line(
            canvas,
            [x, y + scale],
            [x + 9.0 * scale, y + 9.0 * scale],
            scale,
            c,
        );
        line(
            canvas,
            [x + 9.0 * scale, y + scale],
            [x, y + 9.0 * scale],
            scale,
            c,
        );
    }
}
pub(super) fn draw<'a>(canvas: &mut Canvas<'_, 'a>, menu: &'a MenuUi, size: [f64; 2]) {
    let scale = (size[0] / 640.0).min(size[1] / 480.0);
    let [x, y, w, h] = menu_rect(crate::menus::lobby_map::MAP_RECT, size);
    canvas.rect(x, y, w, h, 0x0a141c);
    canvas.outline(x, y, w, h, 0x526779);
    if let Some(preview) = menu
        .map_browser
        .preview
        .as_ref()
        .filter(|_| menu.map_browser.preview_map == Some(menu.match_map))
    {
        let [px, py, pw, ph] = menu_rect(menu.slot_map_rect(), size);
        canvas.image_stretched(&preview.image, [px, py, pw, ph], 0xffffff);
    } else {
        canvas.text(
            if menu.map_browser.preview_error.is_empty() {
                "Loading map..."
            } else {
                "Preview unavailable"
            },
            x + 20.0 * scale,
            y + h / 2.0,
            scale,
            0xb6c9d6,
        );
    }
    for marker in menu.slot_markers() {
        let [px, py, _, _] = menu_rect([marker.point[0], marker.point[1], 1, 1], size);
        let [mx, my, mw, mh] = menu_rect(marker.rect, size);
        let color = menu.slot_color(marker.slot).map(|c| c.rgb()).unwrap_or(
            [
                0x599bff, 0xff6255, 0xf2d05b, 0xc787f0, 0x67d878, 0xffa34d, 0xa6e4e4, 0xededed,
            ][marker.slot % 8],
        );
        line(
            canvas,
            [px, py],
            [mx + mw / 2.0, my + mh / 2.0],
            scale,
            color,
        );
        canvas.rect(
            px - 2.0 * scale,
            py - 2.0 * scale,
            5.0 * scale,
            5.0 * scale,
            color,
        );
    }
    let [x, y, _, _] = menu_rect([424, 100, 192, 12], size);
    let title = menu.selected_slot.map_or_else(
        || "Click a starting position".into(),
        |s| format!("Position {}", s + 1),
    );
    canvas.text(&title, x, y, scale, 0xf1cf73);
    if menu.page == Page::MatchSlots {
        let [x, y, _, _] = menu_rect([24, 370, 592, 30], size);
        canvas.text(
            "Choose a position, then set its player, race and team.",
            x,
            y,
            scale,
            0xb6c9d6,
        );
    }
}
pub(super) fn indicators(canvas: &mut Canvas<'_, '_>, menu: &MenuUi, size: [f64; 2]) {
    let scale = (size[0] / 640.0).min(size[1] / 480.0);
    for marker in menu.slot_markers() {
        let [x, y, w, h] = menu_rect(marker.rect, size);
        if let Some(color) = menu.slot_color(marker.slot) {
            canvas.outline(x, y, w, h, color.rgb());
        }
        if menu.selected_slot == Some(marker.slot) {
            canvas.outline(
                x - scale,
                y - scale,
                w + 2.0 * scale,
                h + 2.0 * scale,
                0xf1cf73,
            );
        }
        let ready = menu.slot_ready(marker.slot);
        canvas.rect(
            x + w - 12.0 * scale,
            y - 5.0 * scale,
            13.0 * scale,
            13.0 * scale,
            0x0a141c,
        );
        readiness(canvas, x + w - 11.0 * scale, y - 4.0 * scale, scale, ready);
    }
    if let Some(slot) = menu.selected_slot {
        if menu.slot_field == Some(3) {
            if let Some(catalog) = &menu.match_catalog {
                for (i, color) in catalog.colors.iter().enumerate() {
                    let [x, y, _, _] = menu_rect([429, 114 + i as u16 * 18, 8, 9], size);
                    canvas.rect(x, y, 8.0 * scale, 9.0 * scale, color.rgb());
                }
            }
        } else if menu.slot_field.is_none()
            && let Some(color) = menu.slot_color(slot)
        {
            let [x, y, _, _] = menu_rect([430, 228, 9, 9], size);
            canvas.rect(x, y, 9.0 * scale, 9.0 * scale, color.rgb());
        }
    }
    if let Some((slot, view)) = &menu.lobby
        && *slot < view.players.len()
        && !view.running
    {
        let [x, y, _, _] = menu_rect([34, 446, 12, 12], size);
        readiness(canvas, x, y, scale, menu.slot_ready(*slot));
    }
}
