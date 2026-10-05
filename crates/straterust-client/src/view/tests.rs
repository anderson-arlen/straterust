use super::*;
mod factions;
mod privacy;
use straterust_engine::{
    assets::{AssetManifest, ImageRef},
    content::Package,
    sim::UnitTypeId,
};

fn reference_blit(
    canvas: &mut Canvas<'_, '_>,
    image: &Image,
    origin: [f64; 2],
    world_size: [u32; 2],
    zoom: f64,
    source_rect: [u32; 4],
) {
    let pixel_scale = zoom * canvas.scale;
    let left = origin[0] * canvas.scale;
    let top = origin[1] * canvas.scale;
    let x0 = (left - 0.5).ceil().clamp(0.0, canvas.width as f64) as usize;
    let y0 = (top - 0.5).ceil().clamp(0.0, canvas.height as f64) as usize;
    let x1 = (left + f64::from(world_size[0]) * pixel_scale - 0.5)
        .ceil()
        .clamp(0.0, canvas.width as f64) as usize;
    let y1 = (top + f64::from(world_size[1]) * pixel_scale - 0.5)
        .ceil()
        .clamp(0.0, canvas.height as f64) as usize;
    for y in y0..y1 {
        let source_y = source_rect[1] as usize
            + (((y as f64 + 0.5 - top) / pixel_scale).floor() as usize) % source_rect[3] as usize;
        for x in x0..x1 {
            let source_x = source_rect[0] as usize
                + (((x as f64 + 0.5 - left) / pixel_scale).floor() as usize)
                    % source_rect[2] as usize;
            let source = (source_y * image.width as usize + source_x) * 4;
            let alpha = u32::from(image.rgba[source + 3]);
            if alpha == 0 {
                continue;
            }
            let destination = &mut canvas.pixels[y * canvas.width + x];
            if alpha == 255 {
                *destination = (u32::from(image.rgba[source]) << 16)
                    | (u32::from(image.rgba[source + 1]) << 8)
                    | u32::from(image.rgba[source + 2]);
                continue;
            }
            let mut color = 0;
            for (channel, shift) in [16, 8, 0].into_iter().enumerate() {
                let foreground = u32::from(image.rgba[source + channel]);
                let background = (*destination >> shift) & 255;
                color |= ((foreground * alpha + background * (255 - alpha) + 127) / 255) << shift;
            }
            *destination = color;
        }
    }
}

mod units;

mod blitting;

mod camera;
