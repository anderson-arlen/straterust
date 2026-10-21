use super::*;
use crate::diagnostics::{Diagnostics, Sample, rate};

pub(crate) fn draw(
    scene: &mut crate::gpu::Scene<'_>,
    diagnostics: &Diagnostics,
    dpi: f64,
    elapsed: u128,
) {
    let size = [f64::from(scene.width) / dpi, f64::from(scene.height) / dpi];
    let mut canvas = Canvas {
        width: scene.width as usize,
        height: scene.height as usize,
        scene: Some(scene),
        pixels: &mut [],
        scale: dpi,
    };
    paint(&mut canvas, diagnostics, size, elapsed);
}
fn graph(
    canvas: &mut Canvas<'_, '_>,
    samples: &std::collections::VecDeque<Sample>,
    rect: [f64; 4],
    maximum: f64,
    field: fn(&Sample) -> f64,
    color: u32,
) {
    let [x, y, w, h] = rect;
    let mut previous: Option<[f64; 2]> = None;
    for (index, sample) in samples.iter().enumerate() {
        let point = [
            x + w * (120 - samples.len() + index) as f64 / 119.,
            y + h * (1. - (field(sample) / maximum.max(1.)).clamp(0., 1.)),
        ];
        if let Some(old) = previous {
            canvas.rect(old[0], old[1], (point[0] - old[0]).max(1.), 1., color);
            canvas.rect(
                point[0],
                old[1].min(point[1]),
                1.,
                (point[1] - old[1]).abs().max(1.),
                color,
            );
        }
        previous = Some(point);
    }
}
fn grid(canvas: &mut Canvas<'_, '_>, rect: [f64; 4]) {
    let [x, y, w, h] = rect;
    for row in [0., 0.5, 1.] {
        canvas.rect(x, y + h * row, w, 1., 0x283642);
    }
}
fn paint(canvas: &mut Canvas<'_, '_>, diagnostics: &Diagnostics, size: [f64; 2], elapsed: u128) {
    let scale = (size[0] / 960.).clamp(0.8, 1.4);
    if diagnostics.fps || diagnostics.network {
        let width = 344. * scale;
        let x = size[0] - width - 8. * scale;
        let height = (if diagnostics.fps { 74. } else { 0. }
            + if diagnostics.network { 82. } else { 0. })
            * scale;
        canvas.rect(x, 8. * scale, width, height, 0x0c141d);
        canvas.outline(x, 8. * scale, width, height, 0x526779);
        let mut y = 18. * scale;
        if diagnostics.fps {
            let maximum = diagnostics
                .samples
                .iter()
                .map(|s| s.fps)
                .fold(60., f64::max);
            canvas.text(
                &format!("FPS {:.1}   peak {:.0}", diagnostics.current.fps, maximum),
                x + 10. * scale,
                y,
                scale,
                0xeee9cf,
            );
            grid(
                canvas,
                [
                    x + 10. * scale,
                    y + 16. * scale,
                    width - 20. * scale,
                    42. * scale,
                ],
            );
            graph(
                canvas,
                &diagnostics.samples,
                [
                    x + 10. * scale,
                    y + 16. * scale,
                    width - 20. * scale,
                    42. * scale,
                ],
                maximum,
                |s| s.fps,
                0xe4cf73,
            );
            y += 74. * scale;
        }
        if diagnostics.network {
            let maximum = diagnostics
                .samples
                .iter()
                .map(|s| s.up.max(s.down))
                .fold(1., f64::max);
            canvas.text(
                &format!(
                    "UP {}   DOWN {}",
                    rate(diagnostics.current.up),
                    rate(diagnostics.current.down)
                ),
                x + 10. * scale,
                y,
                scale,
                0xb6c9d6,
            );
            canvas.text(
                "upload: orange   download: teal",
                x + 10. * scale,
                y + 14. * scale,
                scale,
                0x7f99aa,
            );
            let rect = [
                x + 10. * scale,
                y + 29. * scale,
                width - 20. * scale,
                40. * scale,
            ];
            grid(canvas, rect);
            graph(
                canvas,
                &diagnostics.samples,
                rect,
                maximum,
                |s| s.up,
                0xffae69,
            );
            graph(
                canvas,
                &diagnostics.samples,
                rect,
                maximum,
                |s| s.down,
                0x54d8c7,
            );
        }
    }
    if diagnostics.height <= 0. {
        return;
    }
    let full = size[1] * 0.5;
    let offset = size[1] * diagnostics.height - full;
    let text_scale = (size[0] / 960.).clamp(1., 1.4);
    let line_height = 14. * text_scale;
    canvas.rect(0., offset, size[0], full, 0x0b111a);
    canvas.rect(0., offset + full - 2., size[0], 2., 0x6b8697);
    canvas.text(
        "STRATERUST CONSOLE  ~ close  Tab complete  Up/Down history",
        12.,
        offset + 12.,
        text_scale,
        0xf1cf73,
    );
    let columns = ((size[0] - 24.) / (8. * text_scale)).floor().max(1.) as usize;
    let output: Vec<_> = diagnostics
        .output
        .iter()
        .flat_map(|line| wrapped_lines(line, columns))
        .collect();
    let rows = ((full - 120. * text_scale) / line_height).floor().max(1.) as usize;
    let end = output.len().saturating_sub(diagnostics.scroll);
    let start = end.saturating_sub(rows);
    for (row, line) in output[start..end].iter().enumerate() {
        canvas.text(
            line,
            12.,
            offset + 38. * text_scale + row as f64 * line_height,
            text_scale,
            0xcbdce6,
        );
    }
    let suggestions = diagnostics.suggestions();
    let first = diagnostics.completion.saturating_sub(1);
    for (index, command) in suggestions.iter().enumerate().skip(first).take(3) {
        let y = offset + full - 82. * text_scale + (index - first) as f64 * line_height;
        if index == diagnostics.completion {
            canvas.rect(8., y - 2., size[0] - 16., line_height, 0x253849);
        }
        let description = crate::diagnostics::COMMANDS
            .iter()
            .find(|(name, _)| name == command)
            .unwrap()
            .1;
        canvas.text(
            &format!("{command}  -  {description}"),
            12.,
            y,
            text_scale,
            0x9db9ca,
        );
    }
    let chars_before = diagnostics.input[..diagnostics.caret].chars().count();
    let first = chars_before.saturating_sub(columns.saturating_sub(3));
    let input: String = diagnostics
        .input
        .chars()
        .skip(first)
        .take(columns.saturating_sub(2))
        .collect();
    let y = offset + full - 24. * text_scale;
    canvas.text(&format!("> {input}"), 12., y, text_scale, 0xe2eee4);
    if (elapsed / 500).is_multiple_of(2) {
        canvas.rect(
            12. + (2 + chars_before - first) as f64 * 8. * text_scale,
            y + 9. * text_scale,
            7. * text_scale,
            1.,
            0xe2eee4,
        );
    }
}

#[cfg(test)]
mod tests;
