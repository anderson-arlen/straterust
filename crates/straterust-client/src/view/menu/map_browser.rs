use super::*;
use crate::menus::map_browser::shortened;

pub(super) fn draw<'a>(canvas: &mut Canvas<'_, 'a>, menu: &'a MenuUi, size: [f64; 2]) {
    let scale = (size[0] / 640.0).min(size[1] / 480.0);
    let rect = |r| menu_rect(r, size);
    let [x, y, w, h] = rect([330, 112, 286, 204]);
    canvas.rect(x, y, w, h, 0x0a141c);
    canvas.outline(x, y, w, h, 0x526779);
    let maps = menu.map_indices();
    let status = if !menu.map_browser.indexed {
        "Comparing map layouts...".into()
    } else {
        format!("{} maps match these filters", maps.len())
    };
    let [sx, sy, _, _] = rect([24, 354, 282, 10]);
    canvas.rect(
        sx - 2.0 * scale,
        sy - scale,
        286.0 * scale,
        11.0 * scale,
        0x0a141c,
    );
    canvas.text(&status, sx, sy, scale * 0.85, 0xb6c9d6);
    let Some(index) = menu.map_browser.selected else {
        canvas.text(
            "No matching maps",
            x + 12.0 * scale,
            y + 88.0 * scale,
            scale,
            0xb6c9d6,
        );
        return;
    };
    let entry = &menu.match_catalog.as_ref().unwrap().maps[index];
    for (i, line) in wrapped_lines(&entry.title, 33).iter().take(2).enumerate() {
        canvas.text(
            line,
            x + 8.0 * scale,
            y + (6 + i * 11) as f64 * scale,
            scale,
            0xf1cf73,
        );
    }
    canvas.text(
        &format!(
            "{} players | {}x{}",
            entry.players, entry.size[0], entry.size[1]
        ),
        x + 8.0 * scale,
        y + 30.0 * scale,
        scale * 0.9,
        0xb6c9d6,
    );
    let [px, py, pw, ph] = rect([338, 156, 270, 152]);
    if let Some(preview) = &menu.map_browser.preview {
        let zoom = (pw / f64::from(preview.image.width)).min(ph / f64::from(preview.image.height));
        let iw = f64::from(preview.image.width) * zoom;
        let ih = f64::from(preview.image.height) * zoom;
        let origin = [px + (pw - iw) / 2.0, py + (ph - ih) / 2.0];
        canvas.image(
            &preview.image,
            origin,
            [preview.image.width, preview.image.height],
            zoom,
        );
        for start in &preview.starts {
            let color = [
                0x599bff, 0xff6255, 0xf2d05b, 0xc787f0, 0x67d878, 0xffa34d, 0xa6e4e4, 0xededed,
            ][usize::from(start.player.0) % 8];
            let sx = origin[0] + f64::from(start.position.x) / f64::from(preview.size[0]) * iw;
            let sy = origin[1] + f64::from(start.position.y) / f64::from(preview.size[1]) * ih;
            canvas.rect(
                sx - 4.0 * scale,
                sy - 4.0 * scale,
                9.0 * scale,
                9.0 * scale,
                0x080d15,
            );
            canvas.text(
                &(start.player.0 + 1).to_string(),
                sx - 3.0 * scale,
                sy - 3.0 * scale,
                scale * 0.85,
                color,
            );
        }
    } else {
        let text = if menu.map_browser.preview_error.is_empty() {
            "Loading preview..."
        } else {
            "Preview unavailable"
        };
        canvas.text(text, px + 18.0 * scale, py + 68.0 * scale, scale, 0xb6c9d6);
    }
    if let Some(overview) = menu
        .map_browser
        .overviews
        .get(index)
        .and_then(Option::as_ref)
    {
        let [bx, by, bw, bh] = rect([330, 350, 286, 44]);
        canvas.rect(bx, by, bw, bh, 0x0a141c);
        let resources = overview
            .fields
            .iter()
            .find(|(k, _)| k == "Starting resources")
            .map_or("", |(_, v)| v.as_str());
        let lines = wrapped_lines(&format!("Map resources: {resources}"), 35);
        for (i, line) in lines.iter().take(2).enumerate() {
            let [dx, dy, _, _] = rect([330, 354 + i as u16 * 11, 286, 12]);
            canvas.text(&shortened(line, 35), dx, dy, scale * 0.9, 0xdce5ed);
        }
    }
    let [lx, ly, _, _] = rect([330, 380, 286, 12]);
    canvas.text(
        "Numbers: starts | Yellow: resources",
        lx,
        ly,
        scale * 0.9,
        0xb6c9d6,
    );
}
