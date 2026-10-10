use super::*;

impl<'a> View<'a> {
    pub(super) fn draw_mission(&self, canvas: &mut Canvas<'_, 'a>, size: [f64; 2]) {
        let (Some(mission), Some(media)) = (self.mission, self.media) else {
            return;
        };
        let text = 0xd3d8bf;
        let edge = 0x8f9b7a;
        let lines = |value: &str, width: usize| -> Vec<String> {
            value
                .lines()
                .flat_map(|line| wrapped_lines(line.trim(), width))
                .collect()
        };
        if mission.briefing {
            canvas.rect(0.0, 0.0, size[0], size[1], 0x111819);
            canvas.outline(16.0, 16.0, size[0] - 32.0, size[1] - 32.0, edge);
            canvas.text("MISSION BRIEFING", 36.0, 34.0, 2.0, text);
            if let Some(objectives) = mission.objectives(media) {
                for (line, value) in lines(objectives, ((size[0] - 72.0) / 8.0) as usize)
                    .iter()
                    .take(5)
                    .enumerate()
                {
                    canvas.text(value, 36.0, 65.0 + line as f64 * 13.0, 1.0, 0x8fd9b3);
                }
            }
            let slot_width = ((size[0] - 100.0) / 4.0).min(150.0);
            let row_width = slot_width * 4.0;
            let left = (size[0] - row_width) / 2.0;
            for (slot, portrait) in mission.portraits.iter().enumerate() {
                if media.portraits.is_empty() {
                    break;
                }
                let x = left + slot as f64 * slot_width;
                let rect = [x + 8.0, 145.0, slot_width - 16.0, 104.0];
                canvas.rect(rect[0], rect[1], rect[2], rect[3], 0x070d0d);
                canvas.outline(
                    rect[0],
                    rect[1],
                    rect[2],
                    rect[3],
                    if mission.active_slot == Some(slot) {
                        0x9ee878
                    } else {
                        0x48554a
                    },
                );
                if let Some(portrait) = portrait {
                    self.draw_portrait(canvas, *portrait, rect);
                }
            }
            let caption_top = if media.portraits.is_empty() {
                145.0
            } else {
                270.0
            };
            if let Some(value) = mission.text(media) {
                for (line, value) in lines(value, ((size[0] - 88.0) / 8.0) as usize)
                    .iter()
                    .take(((size[1] - caption_top - 75.0) / 13.0).max(1.0) as usize)
                    .enumerate()
                {
                    canvas.text(value, 44.0, caption_top + line as f64 * 13.0, 1.0, text);
                }
            }
            let [x, y, w, h] = crate::mission::start_button(size);
            canvas.rect(x, y, w, h, 0x26342b);
            canvas.outline(x, y, w, h, edge);
            canvas.text(
                if mission.briefing_finished {
                    "START MISSION  ENTER"
                } else {
                    "SKIP / START  ENTER"
                },
                x + 30.0,
                y + 15.0,
                1.0,
                text,
            );
            return;
        }
        let [map_left, map_top, map_right, map_bottom] = self.camera.viewport_bounds(size);
        let map_width = map_right - map_left;
        if let Some(objectives) = mission.objectives(media) {
            let width = (map_width - 32.0).min(520.0);
            let lines = lines(objectives, ((width - 20.0) / 8.0) as usize);
            let height = (lines.len().min(8) as f64 * 11.0) + 12.0;
            canvas.rect(map_left + 12.0, map_top + 6.0, width, height, 0x142021);
            for (line, value) in lines.iter().take(8).enumerate() {
                canvas.text(
                    value,
                    map_left + 22.0,
                    map_top + 12.0 + line as f64 * 11.0,
                    1.0,
                    text,
                );
            }
        }
        if let Some(remaining) = self
            .world
            .state()
            .mission
            .as_ref()
            .map(|state| state.countdown_ms)
            .filter(|remaining| *remaining > 0)
        {
            let seconds = remaining.div_ceil(1000);
            let x = map_right - 180.0;
            canvas.rect(x, map_top + 6.0, 168.0, 28.0, 0x142021);
            canvas.text(
                &format!("TIME LEFT {:02}:{:02}", seconds / 60, seconds % 60),
                x + 8.0,
                map_top + 14.0,
                1.0,
                text,
            );
        }
        if let Some(value) = mission.text(media) {
            let width = (map_width - 32.0).min(860.0);
            let x = map_left + (map_width - width) / 2.0;
            let portrait = mission.active_slot.and_then(|slot| mission.portraits[slot]);
            let inset = if portrait.is_some() { 110.0 } else { 16.0 };
            let lines = lines(value, ((width - inset - 16.0) / 8.0) as usize);
            let max_lines = ((map_bottom - map_top - 36.0) / 11.0).max(1.0) as usize;
            let height = (lines.len().min(max_lines) as f64 * 11.0 + 24.0)
                .max(if portrait.is_some() { 100.0 } else { 35.0 });
            let y = map_bottom - height - 8.0;
            canvas.rect(x, y, width, height, 0x121d20);
            canvas.outline(x, y, width, height, edge);
            if let Some(portrait) = portrait {
                self.draw_portrait(canvas, portrait, [x + 12.0, y + 10.0, 84.0, 80.0]);
            }
            for (line, value) in lines.iter().take(max_lines).enumerate() {
                canvas.text(value, x + inset, y + 12.0 + line as f64 * 11.0, 1.0, text);
            }
        }
    }
}
