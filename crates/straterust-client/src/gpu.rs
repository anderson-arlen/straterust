//! Native textured-quad rendering. The CPU records geometry; the GPU samples,
//! repeats, mirrors, blends and presents native images. No CPU framebuffer is uploaded.
use std::{
    collections::HashMap,
    sync::{Arc, mpsc},
};

use anyhow::{Context, Result, ensure};
use straterust_engine::assets::{ColorRemap, Image};
use winit::window::Window;

pub struct Scene<'a> {
    pub width: u32,
    pub height: u32,
    pub clear: u32,
    pub commands: Vec<Draw<'a>>,
}

pub enum Draw<'a> {
    Rect {
        rect: [f32; 4],
        color: u32,
    },
    Image {
        image: &'a Image,
        colors: Option<&'a ColorRemap>,
        rect: [f32; 4],
        world_size: [u32; 2],
        source_rect: [u32; 4],
        flip_x: bool,
        color: u32,
    },
}

const INSTANCE_BYTES: u64 = 60;
const MAX_QUADS: usize = 200_000;
const ATTRIBUTES: [wgpu::VertexAttribute; 5] = wgpu::vertex_attr_array![
    0 => Float32x4, 1 => Float32x2, 2 => Float32x4, 3 => Uint32x4, 4 => Uint32
];

const QUAD_SHADER: &str = r#"
@group(0) @binding(0) var picture: texture_2d<f32>;
struct Vertex {
    @builtin(position) position: vec4<f32>,
    @location(0) texel: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) @interpolate(flat) region: vec4<u32>,
    @location(3) @interpolate(flat) flags: u32,
};
@vertex fn vertex(
    @builtin(vertex_index) index: u32,
    @location(0) rect: vec4<f32>,
    @location(1) extent: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) region: vec4<u32>,
    @location(4) flags: u32,
) -> Vertex {
    let corners = array<vec2<f32>,6>(vec2<f32>(0.0,0.0),vec2<f32>(0.0,1.0),vec2<f32>(1.0,0.0),vec2<f32>(1.0,0.0),vec2<f32>(0.0,1.0),vec2<f32>(1.0,1.0));
    let corner = corners[index];
    return Vertex(vec4<f32>(rect.xy + corner * rect.zw, 0.0, 1.0), corner * extent, color, region, flags);
}
@fragment fn fragment(input: Vertex) -> @location(0) vec4<f32> {
    // Integer textureLoad is nearest-neighbor sampling. Modulo stays within
    // the selected atlas rectangle when terrain repeats.
    var sample = vec2<u32>(floor(max(input.texel, vec2<f32>(0.0)))) % input.region.zw;
    if ((input.flags & 1u) != 0u) { sample.x = input.region.z - 1u - sample.x; }
    return textureLoad(picture, vec2<i32>(input.region.xy + sample), 0) * input.color;
}
"#;

struct Batch {
    texture: (usize, usize),
    start: u32,
    end: u32,
}

pub struct Renderer {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    texture_layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
    present_pipeline: wgpu::RenderPipeline,
    output: wgpu::Texture,
    output_view: wgpu::TextureView,
    output_binding: wgpu::BindGroup,
    instance_buffer: wgpu::Buffer,
    instance_capacity: u64,
    instances: Vec<u8>,
    batches: Vec<Batch>,
    // AssetPack images remain alive until reset_assets on a package transition.
    // Key zero is the renderer's permanent white texture for solid primitives.
    textures: HashMap<(usize, usize), wgpu::BindGroup>,
    adapter_name: String,
    rendered_frames: u64,
}

impl Renderer {
    /// Texture keys use allocation identity, so discard them before replacing
    /// the pack on a campaign transition. Keep the device and window surface.
    pub fn reset_assets(&mut self) {
        self.textures.retain(|&key, _| key == (0, 0));
    }

    pub fn new(window: Arc<Window>) -> Result<Self> {
        pollster::block_on(Self::initialize(window))
    }

    async fn initialize(window: Arc<Window>) -> Result<Self> {
        let size = window.inner_size();
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::VULKAN | wgpu::Backends::GL,
            ..Default::default()
        });
        let surface = instance
            .create_surface(window.clone())
            .context("create native GPU surface")?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .context("find a native graphics adapter")?;
        let info = adapter.get_info();
        let adapter_name = format!("{} ({:?}, {:?})", info.name, info.device_type, info.backend);
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("StrateRust renderer"),
                required_limits: wgpu::Limits::downlevel_defaults()
                    .using_resolution(adapter.limits()),
                ..Default::default()
            })
            .await
            .context("initialize native graphics device")?;
        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|format| !format.is_srgb())
            .or_else(|| caps.formats.first().copied())
            .context("surface has no pixel format")?;
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            desired_maximum_frame_latency: 2,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
        };
        ensure!(
            config.width <= device.limits().max_texture_dimension_2d
                && config.height <= device.limits().max_texture_dimension_2d,
            "window exceeds GPU texture limits"
        );
        surface.configure(&device, &config);
        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("nearest image texture"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("native quad layout"),
            bind_group_layouts: &[&texture_layout],
            push_constant_ranges: &[],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("native textured quads"),
            source: wgpu::ShaderSource::Wgsl(QUAD_SHADER.into()),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("native image and HUD quads"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vertex"),
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: INSTANCE_BYTES,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &ATTRIBUTES,
                }],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fragment"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview: None,
            cache: None,
        });
        // Palette bytes are already display-encoded. Prefer an unorm surface;
        // on sRGB-only surfaces decode once here so automatic encoding preserves
        // the original palette instead of applying gamma twice.
        let present_source = format!(
            r#"
@group(0) @binding(0) var picture: texture_2d<f32>;
@vertex fn vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {{
    let points = array<vec2<f32>,3>(vec2<f32>(-1.0,-1.0), vec2<f32>(3.0,-1.0), vec2<f32>(-1.0,3.0));
    return vec4<f32>(points[index],0.0,1.0);
}}
@fragment fn fragment(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {{
    let pixel = textureLoad(picture,vec2<i32>(position.xy),0);
    if ({}) {{
        let linear = select(pixel.rgb / 12.92, pow((pixel.rgb + 0.055) / 1.055, vec3<f32>(2.4)), pixel.rgb > vec3<f32>(0.04045));
        return vec4<f32>(linear,pixel.a);
    }}
    return pixel;
}}
"#,
            format.is_srgb()
        );
        let present_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("GPU presentation"),
            source: wgpu::ShaderSource::Wgsl(present_source.into()),
        });
        let present_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("GPU surface presentation"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &present_shader,
                entry_point: Some("vertex"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &present_shader,
                entry_point: Some("fragment"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview: None,
            cache: None,
        });
        let (output, output_view, output_binding) =
            output_target(&device, &texture_layout, config.width, config.height);
        let instance_capacity = 4096;
        let instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("quad instances"),
            size: instance_capacity,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut textures = HashMap::new();
        textures.insert(
            (0, 0),
            upload_image(
                &device,
                &queue,
                &texture_layout,
                &Image {
                    width: 1,
                    height: 1,
                    rgba: vec![255; 4],
                },
            ),
        );
        log::info!("Native GPU renderer: {adapter_name}; surface={format:?}");
        Ok(Self {
            window,
            surface,
            device,
            queue,
            config,
            texture_layout,
            pipeline,
            present_pipeline,
            output,
            output_view,
            output_binding,
            instance_buffer,
            instance_capacity,
            instances: Vec::new(),
            batches: Vec::new(),
            textures,
            adapter_name,
            rendered_frames: 0,
        })
    }

    pub fn adapter_name(&self) -> &str {
        &self.adapter_name
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 || (width == self.config.width && height == self.config.height)
        {
            return;
        }
        let limit = self.device.limits().max_texture_dimension_2d;
        self.config.width = width.min(limit);
        self.config.height = height.min(limit);
        self.surface.configure(&self.device, &self.config);
        (self.output, self.output_view, self.output_binding) = output_target(
            &self.device,
            &self.texture_layout,
            self.config.width,
            self.config.height,
        );
    }

    pub fn render(&mut self, scene: &Scene<'_>) -> Result<()> {
        let measure = log::log_enabled!(log::Level::Debug)
            && (self.rendered_frames < 5 || self.rendered_frames.is_multiple_of(60));
        let start_time = measure.then(std::time::Instant::now);
        ensure!(
            scene.width == self.config.width && scene.height == self.config.height,
            "scene dimensions exceed or differ from the GPU surface"
        );
        ensure!(
            scene.commands.len() <= MAX_QUADS,
            "too many visible drawing primitives"
        );
        self.instances.clear();
        self.batches.clear();
        for command in &scene.commands {
            let (texture, rect, world_size, source_rect, color, flip_x) = match command {
                Draw::Rect { rect, color } => ((0, 0), *rect, [1, 1], [0, 0, 1, 1], *color, false),
                Draw::Image {
                    image,
                    colors,
                    rect,
                    world_size,
                    source_rect,
                    flip_x,
                    color,
                } => {
                    let key = (
                        image.rgba.as_ptr() as usize,
                        colors.map_or(0, |c| c.colors.as_ptr() as usize),
                    );
                    if !self.textures.contains_key(&key) {
                        ensure!(
                            image.width <= self.device.limits().max_texture_dimension_2d
                                && image.height <= self.device.limits().max_texture_dimension_2d,
                            "native image exceeds GPU texture limits"
                        );
                        let mapped = colors.map(|palette| palette.image(image));
                        self.textures.insert(
                            key,
                            upload_image(
                                &self.device,
                                &self.queue,
                                &self.texture_layout,
                                mapped.as_ref().unwrap_or(image),
                            ),
                        );
                    }
                    (key, *rect, *world_size, *source_rect, *color, *flip_x)
                }
            };
            let start = (self.instances.len() as u64 / INSTANCE_BYTES) as u32;
            append_quad(
                &mut self.instances,
                [scene.width, scene.height],
                rect,
                world_size,
                source_rect,
                color,
                flip_x,
            );
            if let Some(batch) = self
                .batches
                .last_mut()
                .filter(|batch| batch.texture == texture)
            {
                batch.end = start + 1;
            } else {
                self.batches.push(Batch {
                    texture,
                    start,
                    end: start + 1,
                });
            }
        }
        let required = self.instances.len() as u64;
        if required > self.instance_capacity {
            self.instance_capacity = required.next_power_of_two();
            self.instance_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("quad instances"),
                size: self.instance_capacity,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        if !self.instances.is_empty() {
            self.queue
                .write_buffer(&self.instance_buffer, 0, &self.instances);
        }
        let data_ready = measure.then(std::time::Instant::now);
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("native frame"),
            });
        {
            let attachment = Some(wgpu::RenderPassColorAttachment {
                view: &self.output_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: f64::from((scene.clear >> 16) & 255) / 255.0,
                        g: f64::from((scene.clear >> 8) & 255) / 255.0,
                        b: f64::from(scene.clear & 255) / 255.0,
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            });
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("native terrain, sprites, and HUD"),
                color_attachments: &[attachment],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_vertex_buffer(0, self.instance_buffer.slice(..));
            for batch in &self.batches {
                pass.set_bind_group(0, &self.textures[&batch.texture], &[]);
                pass.draw(0..6, batch.start..batch.end);
            }
        }
        let encoded = measure.then(std::time::Instant::now);
        let frame = match self.surface.get_current_texture() {
            Ok(frame) => Some(frame),
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                self.surface.configure(&self.device, &self.config);
                Some(
                    self.surface
                        .get_current_texture()
                        .context("restore GPU surface")?,
                )
            }
            Err(wgpu::SurfaceError::Timeout) => None,
            Err(error) => return Err(error).context("acquire GPU surface"),
        };
        let acquired = measure.then(std::time::Instant::now);
        if let Some(frame) = &frame {
            let view = frame.texture.create_view(&Default::default());
            let attachment = Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            });
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("present GPU frame"),
                color_attachments: &[attachment],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.present_pipeline);
            pass.set_bind_group(0, &self.output_binding, &[]);
            pass.draw(0..3, 0..1);
        }
        self.queue.submit([encoder.finish()]);
        let submitted = measure.then(std::time::Instant::now);
        if let Some(frame) = frame {
            // Wayland's frame callback must be attached immediately before
            // the actual swapchain commit, after acquisition and submission.
            self.window.pre_present_notify();
            frame.present();
        }
        if let (
            Some(start_time),
            Some(data_ready),
            Some(encoded),
            Some(acquired),
            Some(submitted),
        ) = (start_time, data_ready, encoded, acquired, submitted)
        {
            let ms = |duration: std::time::Duration| duration.as_secs_f64() * 1000.0;
            log::debug!(
                "GPU phase frame{}: instances={:.2}ms encode={:.2}ms acquire={:.2}ms submit={:.2}ms present={:.2}ms batches={}",
                self.rendered_frames,
                ms(data_ready - start_time),
                ms(encoded - data_ready),
                ms(acquired - encoded),
                ms(submitted - acquired),
                ms(submitted.elapsed()),
                self.batches.len()
            );
        }
        self.rendered_frames = self.rendered_frames.saturating_add(1);
        Ok(())
    }

    /// Read the actual last GPU color target only when a screenshot is requested.
    pub fn readback(&self) -> Result<Vec<u32>> {
        let bytes_per_row = (self.config.width * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
            * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("GPU screenshot"),
            size: u64::from(bytes_per_row) * u64::from(self.config.height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("copy GPU screenshot"),
            });
        encoder.copy_texture_to_buffer(
            self.output.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(bytes_per_row),
                    rows_per_image: Some(self.config.height),
                },
            },
            wgpu::Extent3d {
                width: self.config.width,
                height: self.config.height,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit([encoder.finish()]);
        let (sender, receiver) = mpsc::channel();
        buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let _ = sender.send(result);
            });
        self.device
            .poll(wgpu::PollType::Wait)
            .context("wait for GPU screenshot")?;
        receiver
            .recv()
            .context("GPU screenshot callback")?
            .context("map GPU screenshot")?;
        let data = buffer.slice(..).get_mapped_range();
        let mut pixels = Vec::with_capacity((self.config.width * self.config.height) as usize);
        for row in data.chunks_exact(bytes_per_row as usize) {
            for rgba in row[..self.config.width as usize * 4].as_chunks::<4>().0 {
                pixels
                    .push(u32::from(rgba[0]) << 16 | u32::from(rgba[1]) << 8 | u32::from(rgba[2]));
            }
        }
        drop(data);
        buffer.unmap();
        Ok(pixels)
    }
}

fn bind_texture(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    view: &wgpu::TextureView,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("native image"),
        layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::TextureView(view),
        }],
    })
}
fn output_target(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    width: u32,
    height: u32,
) -> (wgpu::Texture, wgpu::TextureView, wgpu::BindGroup) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("GPU color target"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());
    let binding = bind_texture(device, layout, &view);
    (texture, view, binding)
}
fn upload_image(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layout: &wgpu::BindGroupLayout,
    image: &Image,
) -> wgpu::BindGroup {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("native RGBA image"),
        size: wgpu::Extent3d {
            width: image.width,
            height: image.height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        texture.as_image_copy(),
        &image.rgba,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(image.width * 4),
            rows_per_image: Some(image.height),
        },
        wgpu::Extent3d {
            width: image.width,
            height: image.height,
            depth_or_array_layers: 1,
        },
    );
    bind_texture(device, layout, &texture.create_view(&Default::default()))
}

fn append_quad(
    bytes: &mut Vec<u8>,
    size: [u32; 2],
    rect: [f32; 4],
    world_size: [u32; 2],
    source_rect: [u32; 4],
    color: u32,
    flip_x: bool,
) {
    // One instance per quad. The shader expands its six corners, so even a
    // detailed minimap does not require six CPU-encoded vertices per cell.
    let values = [
        rect[0] / size[0] as f32 * 2.0 - 1.0,
        1.0 - rect[1] / size[1] as f32 * 2.0,
        rect[2] / size[0] as f32 * 2.0,
        -rect[3] / size[1] as f32 * 2.0,
        world_size[0] as f32,
        world_size[1] as f32,
        ((color >> 16) & 255) as f32 / 255.0,
        ((color >> 8) & 255) as f32 / 255.0,
        (color & 255) as f32 / 255.0,
        if color >> 24 == 0 {
            1.0
        } else {
            (color >> 24) as f32 / 255.0
        },
    ];
    for value in values {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    for value in source_rect {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&u32::from(flip_x).to_le_bytes());
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use winit::{
        application::ApplicationHandler,
        dpi::PhysicalSize,
        event::WindowEvent,
        event_loop::{ActiveEventLoop, EventLoop},
        platform::wayland::EventLoopBuilderExtWayland,
        window::WindowId,
    };

    #[test]
    #[ignore = "requires a native display/GPU; opens one small window and reads GPU pixels"]
    fn native_gpu_readback_preserves_regions_repetition_mirror_blend_and_tint() {
        #[derive(Default)]
        struct Probe {
            window: Option<Arc<Window>>,
            renderer: Option<Renderer>,
            checked: bool,
        }
        impl ApplicationHandler for Probe {
            fn resumed(&mut self, event_loop: &ActiveEventLoop) {
                let size = PhysicalSize::new(64, 64);
                let window = Arc::new(
                    event_loop
                        .create_window(
                            Window::default_attributes()
                                .with_title("StrateRust GPU pixel check")
                                .with_inner_size(size)
                                .with_min_inner_size(size)
                                .with_max_inner_size(size),
                        )
                        .unwrap(),
                );
                self.renderer = Some(Renderer::new(window.clone()).unwrap());
                window.request_redraw();
                self.window = Some(window);
            }
            fn window_event(
                &mut self,
                event_loop: &ActiveEventLoop,
                _id: WindowId,
                event: WindowEvent,
            ) {
                if !matches!(event, WindowEvent::RedrawRequested) {
                    return;
                }
                let size = self.window.as_ref().unwrap().inner_size();
                let renderer = self.renderer.as_mut().unwrap();
                renderer.resize(size.width, size.height);
                let image = Image {
                    width: 2,
                    height: 2,
                    rgba: vec![
                        255, 0, 0, 255, 0, 255, 0, 128, 0, 0, 255, 0, 255, 255, 255, 255,
                    ],
                };
                let mut scene = Scene {
                    width: size.width,
                    height: size.height,
                    clear: 0x204060,
                    commands: vec![Draw::Rect {
                        rect: [2.0, 2.0, 4.0, 4.0],
                        color: 0xf08020,
                    }],
                };
                for (rect, world_size, source_rect, flip_x, color) in [
                    ([8.0, 8.0, 4.0, 4.0], [2, 2], [0, 0, 2, 2], false, 0xffffff),
                    ([16.0, 8.0, 4.0, 4.0], [2, 2], [0, 0, 2, 2], true, 0xffffff),
                    ([24.0, 8.0, 8.0, 4.0], [4, 2], [0, 0, 1, 2], false, 0xffffff),
                    ([2.0, 16.0, 4.0, 4.0], [1, 1], [1, 1, 1, 1], false, 0x72b8de),
                    (
                        [20.0, 16.0, 4.0, 4.0],
                        [2, 2],
                        [0, 0, 2, 2],
                        false,
                        0x6effffff,
                    ),
                    (
                        [10.0, 16.0, 8.0, 2.0],
                        [2, 2],
                        [0, 0, 2, 2],
                        false,
                        0x808080,
                    ),
                ] {
                    scene.commands.push(Draw::Image {
                        image: &image,
                        colors: None,
                        rect,
                        world_size,
                        source_rect,
                        flip_x,
                        color,
                    });
                }
                let colors = ColorRemap {
                    colors: vec![[[255, 0, 0], [12, 72, 204]]],
                };
                scene.commands.push(Draw::Image {
                    image: &image,
                    colors: Some(&colors),
                    rect: [32.0, 8.0, 4.0, 4.0],
                    world_size: [2, 2],
                    source_rect: [0, 0, 2, 2],
                    flip_x: false,
                    color: 0xffffff,
                });
                renderer.render(&scene).unwrap();
                let pixels = renderer.readback().unwrap();
                assert_eq!(
                    renderer.textures.len(),
                    3,
                    "one white texture, one image and one cached player palette variant"
                );
                // A campaign transition replaces image allocations, but the
                // first UI rectangle in the new mission still needs key zero.
                renderer.reset_assets();
                assert_eq!(renderer.textures.len(), 1);
                renderer.render(&scene).unwrap();
                assert_eq!(renderer.readback().unwrap(), pixels);
                for (x, y, expected) in [
                    (0, 0, 0x204060_u32),
                    (32, 8, 0x0c48cc),
                    (2, 2, 0xf08020),
                    (8, 8, 0xff0000),
                    (10, 8, 0x10a030),
                    (8, 10, 0x204060),
                    (10, 10, 0xffffff),
                    (16, 8, 0x10a030),
                    (18, 8, 0xff0000),
                    (16, 10, 0xffffff),
                    (18, 10, 0x204060),
                    (24, 8, 0xff0000),
                    (26, 8, 0xff0000),
                    (24, 10, 0x204060),
                    (2, 16, 0x72b8de),
                    (20, 16, 0x802437),
                    (22, 16, 0x19694b),
                    (20, 18, 0x204060),
                    (10, 16, 0x800000),
                    (14, 16, 0x106030),
                    (10, 17, 0x204060),
                    (14, 17, 0x808080),
                ] {
                    let actual = pixels[(y * size.width + x) as usize];
                    for shift in [0, 8, 16] {
                        assert!(
                            (((actual >> shift) & 255) as i32 - ((expected >> shift) & 255) as i32)
                                .abs()
                                <= 1,
                            "GPU pixel ({x},{y}):{actual:06x}, expected {expected:06x}"
                        );
                    }
                }
                eprintln!(
                    "Native GPU pixel/readback check passed on {}",
                    renderer.adapter_name()
                );
                self.checked = true;
                self.renderer = None;
                self.window = None;
                event_loop.exit();
            }
        }
        let mut builder = EventLoop::builder();
        builder.with_any_thread(true);
        let event_loop = builder.build().unwrap();
        let mut probe = Probe::default();
        event_loop.run_app(&mut probe).unwrap();
        assert!(probe.checked);
    }
}
