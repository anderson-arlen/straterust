//! Authored console apertures shared by rendering and pointer hit testing.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConsoleViewport {
    pub canvas: [u16; 2],
    /// Left, top, right and bottom borders around the map aperture.
    pub margins: [u16; 4],
}

impl ConsoleViewport {
    pub fn scale(self, size: [f64; 2]) -> f64 {
        (size[0] / f64::from(self.canvas[0])).min(size[1] / f64::from(self.canvas[1]))
    }

    pub fn bounds(self, size: [f64; 2]) -> [f64; 4] {
        let s = self.scale(size);
        let [l, t, r, b] = self.margins.map(|v| f64::from(v) * s);
        [l, t, size[0] - r, size[1] - b]
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConsoleLayout {
    pub viewport: ConsoleViewport,
    pub menu: [u16; 4],
    pub minimap: [u16; 4],
    pub selection: [u16; 4],
    pub group: [u16; 4],
    pub group_step: [u16; 2],
    pub group_columns: u8,
    pub buttons: [u16; 4],
    pub button_step: [u16; 2],
}

impl ConsoleLayout {
    pub fn rect(self, rect: [u16; 4], size: [f64; 2]) -> [f64; 4] {
        let scale = self.viewport.scale(size);
        rect.map(|v| f64::from(v) * scale)
    }

    pub fn button_rect(self, slot: usize, size: [f64; 2]) -> [f64; 4] {
        self.cell(self.buttons, self.button_step, 3, slot, size)
    }

    pub fn group_rect(self, slot: usize, size: [f64; 2]) -> [f64; 4] {
        self.cell(
            self.group,
            self.group_step,
            self.group_columns as usize,
            slot,
            size,
        )
    }

    fn cell(
        self,
        mut rect: [u16; 4],
        step: [u16; 2],
        columns: usize,
        slot: usize,
        size: [f64; 2],
    ) -> [f64; 4] {
        rect[0] += (slot % columns) as u16 * step[0];
        rect[1] += (slot / columns) as u16 * step[1];
        self.rect(rect, size)
    }

    pub(super) fn validate(self) -> Result<()> {
        let [w, h] = self.viewport.canvas;
        let [l, t, r, b] = self.viewport.margins;
        ensure!(
            (320..=2048).contains(&w)
                && (240..=2048).contains(&h)
                && u32::from(l) + u32::from(r) < u32::from(w)
                && u32::from(t) + u32::from(b) < u32::from(h),
            "invalid console viewport"
        );
        ensure!(
            (1..=6).contains(&self.group_columns),
            "invalid console group columns"
        );
        for rect in [
            self.menu,
            self.minimap,
            self.selection,
            self.group,
            self.buttons,
        ] {
            ensure!(
                rect[2] > 0
                    && rect[3] > 0
                    && u32::from(rect[0]) + u32::from(rect[2]) <= u32::from(w)
                    && u32::from(rect[1]) + u32::from(rect[3]) <= u32::from(h),
                "console rectangle outside canvas"
            );
        }
        ensure!(
            self.group_step
                .iter()
                .chain(&self.button_step)
                .all(|s| *s > 0 && *s <= 256),
            "invalid console cell spacing"
        );
        for rect in [
            self.button_rect(8, [f64::from(w), f64::from(h)]),
            self.group_rect(11, [f64::from(w), f64::from(h)]),
        ] {
            ensure!(
                rect[0] + rect[2] <= f64::from(w) && rect[1] + rect[3] <= f64::from(h),
                "console cells outside canvas"
            );
        }
        Ok(())
    }
}
