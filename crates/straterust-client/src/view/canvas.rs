use super::*;

pub(super) struct Canvas<'buffer, 'image> {
    pub(super) scene: Option<&'buffer mut crate::gpu::Scene<'image>>,
    pub(super) pixels: &'buffer mut [u32],
    pub(super) width: usize,
    pub(super) height: usize,
    pub(super) scale: f64,
}

impl<'image> Canvas<'_, 'image> {
    pub(super) fn clear(&mut self, color: u32) {
        if let Some(scene) = &mut self.scene {
            scene.clear = color;
            scene.commands.clear();
        } else {
            Self::fill_run(self.pixels, color);
        }
    }

    // Slice copies use the platform memory routine even in an unoptimized
    // development build. Doubling a known solid run avoids one Rust iteration
    // per framebuffer pixel, while retaining safe, bounded slice operations.
    pub(super) fn fill_run(pixels: &mut [u32], color: u32) {
        let Some(first) = pixels.first_mut() else {
            return;
        };
        *first = color;
        let mut filled = 1;
        while filled < pixels.len() {
            let count = filled.min(pixels.len() - filled);
            let (source, destination) = pixels.split_at_mut(filled);
            destination[..count].copy_from_slice(&source[..count]);
            filled += count;
        }
    }

    /// Nearest-neighbor RGBA drawing. A larger world_size repeats the image as terrain.
    /// Work is bounded by visible framebuffer pixels even for a large map or zoom.
    pub(super) fn image(
        &mut self,
        image: &'image Image,
        origin: [f64; 2],
        world_size: [u32; 2],
        zoom: f64,
    ) {
        self.image_mirrored(image, origin, world_size, zoom, false);
    }

    pub(super) fn image_mirrored(
        &mut self,
        image: &'image Image,
        origin: [f64; 2],
        world_size: [u32; 2],
        zoom: f64,
        flip_x: bool,
    ) {
        self.blit(
            image,
            origin,
            world_size,
            zoom,
            [0, 0, image.width, image.height],
            flip_x,
        );
    }

    pub(super) fn image_cloaked(
        &mut self,
        image: &'image Image,
        origin: [f64; 2],
        world_size: [u32; 2],
        zoom: f64,
        flip_x: bool,
    ) {
        self.blit_faded(
            image,
            origin,
            world_size,
            zoom,
            [0, 0, image.width, image.height],
            flip_x,
            110,
        );
    }

    pub(super) fn image_region(
        &mut self,
        image: &'image Image,
        origin: [f64; 2],
        world_size: [u32; 2],
        zoom: f64,
        source_rect: [u32; 4],
    ) {
        self.blit(image, origin, world_size, zoom, source_rect, false);
    }

    /// HUD panels may scale independently along each axis. The same GPU image
    /// command already supports this; software sampling is only the test oracle.
    pub(super) fn image_stretched(&mut self, image: &'image Image, rect: [f64; 4], color: u32) {
        if rect[2] <= 0.0 || rect[3] <= 0.0 {
            return;
        }
        let [left, top, width, height] = rect.map(|value| value * self.scale);
        if let Some(scene) = &mut self.scene {
            scene.commands.push(crate::gpu::Draw::Image {
                image,
                rect: [left as f32, top as f32, width as f32, height as f32],
                world_size: [image.width, image.height],
                source_rect: [0, 0, image.width, image.height],
                flip_x: false,
                color,
            });
            return;
        }
        let x0 = (left - 0.5).ceil().clamp(0.0, self.width as f64) as usize;
        let y0 = (top - 0.5).ceil().clamp(0.0, self.height as f64) as usize;
        let x1 = (left + width - 0.5).ceil().clamp(0.0, self.width as f64) as usize;
        let y1 = (top + height - 0.5).ceil().clamp(0.0, self.height as f64) as usize;
        for y in y0..y1 {
            let sy = (((y as f64 + 0.5 - top) / height * f64::from(image.height)) as u32)
                .min(image.height - 1);
            for x in x0..x1 {
                let sx = (((x as f64 + 0.5 - left) / width * f64::from(image.width)) as u32)
                    .min(image.width - 1);
                let source = ((sy * image.width + sx) * 4) as usize;
                let rgba = &image.rgba[source..source + 4];
                let alpha = u32::from(rgba[3]);
                let destination = &mut self.pixels[y * self.width + x];
                let channel = |shift: u32, source: u8| {
                    let tinted = (u32::from(source) * ((color >> shift) & 255) + 127) / 255;
                    (tinted * alpha + ((*destination >> shift) & 255) * (255 - alpha) + 127) / 255
                };
                *destination =
                    (channel(16, rgba[0]) << 16) | (channel(8, rgba[1]) << 8) | channel(0, rgba[2]);
            }
        }
    }

    pub(super) fn blit(
        &mut self,
        image: &'image Image,
        origin: [f64; 2],
        world_size: [u32; 2],
        zoom: f64,
        source_rect: [u32; 4],
        flip_x: bool,
    ) {
        self.blit_faded(image, origin, world_size, zoom, source_rect, flip_x, 255);
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn blit_faded(
        &mut self,
        image: &'image Image,
        origin: [f64; 2],
        world_size: [u32; 2],
        zoom: f64,
        source_rect: [u32; 4],
        flip_x: bool,
        opacity: u8,
    ) {
        let pixel_scale = zoom * self.scale;
        let left = origin[0] * self.scale;
        let top = origin[1] * self.scale;
        let x0 = (left - 0.5).ceil().clamp(0.0, self.width as f64) as usize;
        let y0 = (top - 0.5).ceil().clamp(0.0, self.height as f64) as usize;
        let x1 = (left + f64::from(world_size[0]) * pixel_scale - 0.5)
            .ceil()
            .clamp(0.0, self.width as f64) as usize;
        let y1 = (top + f64::from(world_size[1]) * pixel_scale - 0.5)
            .ceil()
            .clamp(0.0, self.height as f64) as usize;
        if x0 >= x1 || y0 >= y1 {
            return;
        }
        if let Some(scene) = &mut self.scene {
            scene.commands.push(crate::gpu::Draw::Image {
                image,
                rect: [
                    left as f32,
                    top as f32,
                    (f64::from(world_size[0]) * pixel_scale) as f32,
                    (f64::from(world_size[1]) * pixel_scale) as f32,
                ],
                world_size,
                source_rect,
                flip_x,
                color: (u32::from(opacity) << 24) | 0xffffff,
            });
            return;
        }
        // The horizontal sample is identical on every scanline. In particular,
        // do not perform a float divide/floor/modulo for every terrain pixel.
        let source_x: Vec<_> = (x0..x1)
            .map(|x| {
                let sample = (((x as f64 + 0.5 - left) / pixel_scale).floor() as usize)
                    % source_rect[2] as usize;
                let sample = if flip_x {
                    source_rect[2] as usize - 1 - sample
                } else {
                    sample
                };
                (source_rect[0] as usize + sample) * 4
            })
            .collect();
        // Repeated terrain and enlarged sprites reuse source rows. Cache only
        // fully opaque rows, whose result is independent of the destination.
        // Storage remains bounded by visible pixels; ordinary 1:1 sprites do
        // not allocate a scanline cache.
        let repeats = y1 - y0 > source_rect[3] as usize;
        let mut rows: Vec<Option<Vec<u32>>> = if repeats {
            (0..source_rect[3]).map(|_| None).collect()
        } else {
            Vec::new()
        };
        for y in y0..y1 {
            let source_y =
                (((y as f64 + 0.5 - top) / pixel_scale).floor() as usize) % source_rect[3] as usize;
            let destination = &mut self.pixels[y * self.width + x0..y * self.width + x1];
            if repeats && let Some(row) = &rows[source_y] {
                destination.copy_from_slice(row);
                continue;
            }
            let source_row = (source_rect[1] as usize + source_y) * image.width as usize * 4;
            let mut opaque = true;
            for (destination, source_x) in destination.iter_mut().zip(&source_x) {
                let source = source_row + source_x;
                let rgba = &image.rgba[source..source + 4];
                let alpha = (u32::from(rgba[3]) * u32::from(opacity) + 127) / 255;
                opaque &= alpha == 255;
                if alpha == 0 {
                    continue;
                }
                let red = u32::from(rgba[0]);
                let green = u32::from(rgba[1]);
                let blue = u32::from(rgba[2]);
                if alpha == 255 {
                    *destination = red << 16 | green << 8 | blue;
                } else {
                    let inverse = 255 - alpha;
                    let red = (red * alpha + ((*destination >> 16) & 255) * inverse + 127) / 255;
                    let green = (green * alpha + ((*destination >> 8) & 255) * inverse + 127) / 255;
                    let blue = (blue * alpha + (*destination & 255) * inverse + 127) / 255;
                    *destination = red << 16 | green << 8 | blue;
                }
            }
            if repeats && opaque {
                rows[source_y] = Some(destination.to_vec());
            }
        }
    }

    pub(super) fn rect(&mut self, x: f64, y: f64, w: f64, h: f64, color: u32) {
        self.rect_bounds(
            [
                (x * self.scale).floor(),
                (y * self.scale).floor(),
                ((x + w) * self.scale).ceil(),
                ((y + h) * self.scale).ceil(),
            ],
            color,
        );
    }

    /// Adjacent pixel-art strips share rounded boundaries instead of each
    /// expanding outward and painting over their one-pixel separators.
    pub(super) fn rect_snapped(&mut self, x: f64, y: f64, w: f64, h: f64, color: u32) {
        self.rect_bounds(
            [
                (x * self.scale).round(),
                (y * self.scale).round(),
                ((x + w) * self.scale).round(),
                ((y + h) * self.scale).round(),
            ],
            color,
        );
    }

    fn rect_bounds(&mut self, bounds: [f64; 4], color: u32) {
        let [x0, y0, x1, y1] = bounds;
        let x0 = x0.clamp(0.0, self.width as f64) as usize;
        let y0 = y0.clamp(0.0, self.height as f64) as usize;
        let x1 = x1.clamp(0.0, self.width as f64) as usize;
        let y1 = y1.clamp(0.0, self.height as f64) as usize;
        if x0 >= x1 || y0 >= y1 {
            return;
        }
        if let Some(scene) = &mut self.scene {
            scene.commands.push(crate::gpu::Draw::Rect {
                rect: [x0 as f32, y0 as f32, (x1 - x0) as f32, (y1 - y0) as f32],
                color,
            });
            return;
        }
        let first = y0 * self.width + x0;
        let count = x1 - x0;
        Self::fill_run(&mut self.pixels[first..first + count], color);
        for row in y0 + 1..y1 {
            self.pixels
                .copy_within(first..first + count, row * self.width + x0);
        }
    }

    pub(super) fn outline(&mut self, x: f64, y: f64, w: f64, h: f64, color: u32) {
        self.rect(x, y, w, 1.0, color);
        self.rect(x, y + h - 1.0, w, 1.0, color);
        self.rect(x, y, 1.0, h, color);
        self.rect(x + w - 1.0, y, 1.0, h, color);
    }

    pub(super) fn selection_circle(&mut self, center: [f64; 2], radii: [f64; 2], color: u32) {
        let steps = (std::f64::consts::TAU * radii[0].max(radii[1]))
            .ceil()
            .clamp(24.0, 2048.0) as usize;
        for step in 0..steps {
            let angle = step as f64 * std::f64::consts::TAU / steps as f64;
            self.rect(
                center[0] + angle.cos() * radii[0],
                center[1] + angle.sin() * radii[1],
                1.5,
                1.5,
                color,
            );
        }
    }

    pub(super) fn text(&mut self, text: &str, x: f64, y: f64, size: f64, color: u32) {
        if let Some(scene) = &mut self.scene {
            let image = font_atlas();
            for (index, character) in text.chars().enumerate() {
                if character as u32 >= 128
                    || font8x8::BASIC_FONTS
                        .get(character)
                        .is_none_or(|glyph| glyph.iter().all(|row| *row == 0))
                {
                    continue;
                }
                let left = (x + index as f64 * 8.0 * size) * self.scale;
                let top = y * self.scale;
                let extent = 8.0 * size * self.scale;
                if left + extent <= 0.0
                    || top + extent <= 0.0
                    || left >= self.width as f64
                    || top >= self.height as f64
                {
                    continue;
                }
                scene.commands.push(crate::gpu::Draw::Image {
                    image,
                    rect: [left as f32, top as f32, extent as f32, extent as f32],
                    world_size: [8, 8],
                    source_rect: [character as u32 % 16 * 8, character as u32 / 16 * 8, 8, 8],
                    flip_x: false,
                    color,
                });
            }
            return;
        }

        for (index, character) in text.chars().enumerate() {
            if let Some(glyph) = font8x8::BASIC_FONTS.get(character) {
                for (row, bits) in glyph.iter().enumerate() {
                    for column in 0..8 {
                        if bits & (1 << column) != 0 {
                            self.rect(
                                x + (index * 8 + column) as f64 * size,
                                y + row as f64 * size,
                                size,
                                size,
                                color,
                            );
                        }
                    }
                }
            }
        }
    }
}

/// A persistent atlas for the already-used font8x8 glyphs. Recording one quad
/// per glyph avoids thousands of tiny pixel rectangles in every HUD frame.
pub(super) fn font_atlas() -> &'static Image {
    static FONT: std::sync::OnceLock<Image> = std::sync::OnceLock::new();
    FONT.get_or_init(|| {
        let mut image = Image {
            width: 128,
            height: 64,
            rgba: vec![0; 128 * 64 * 4],
        };
        for character in 0_u8..128 {
            if let Some(glyph) = font8x8::BASIC_FONTS.get(char::from(character)) {
                for (row, bits) in glyph.iter().enumerate() {
                    for column in 0..8 {
                        if bits & (1 << column) != 0 {
                            let x = usize::from(character % 16) * 8 + column;
                            let y = usize::from(character / 16) * 8 + row;
                            image.rgba[(y * 128 + x) * 4..(y * 128 + x + 1) * 4].fill(255);
                        }
                    }
                }
            }
        }
        image
    })
}
