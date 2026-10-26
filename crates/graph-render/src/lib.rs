//! Draws graph-core scenes with wgpu. On Windows wgpu picks DirectX 12 (with
//! Vulkan or software fallbacks), so nothing extra needs installing.

use graph_core::text::{ATLAS_CHANNELS, ATLAS_SIZE};
use graph_core::{Scene, TextAtlas, Vertex};
use wgpu::util::DeviceExt;

mod shade;
pub use shade::{Shading, ShadingRenderer, Test, MAX_CONSTANTS};

const SHADER: &str = r#"
struct Uniforms {
    size: vec2<f32>,
    linearize: f32,
    _pad: f32,
};
@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var atlas: texture_2d<f32>;
@group(0) @binding(2) var atlas_sampler: sampler;

struct VOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
};

fn to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((c + 0.055) / 1.055, vec3<f32>(2.4));
    return select(hi, lo, c <= vec3<f32>(0.04045));
}

@vertex
fn vs(@location(0) p: vec2<f32>, @location(1) uv: vec2<f32>, @location(2) c: vec4<f32>) -> VOut {
    var o: VOut;
    o.pos = vec4<f32>(p.x / u.size.x * 2.0 - 1.0, 1.0 - p.y / u.size.y * 2.0, 0.0, 1.0);
    o.uv = uv;
    o.color = c;
    if (u.linearize > 0.5) {
        o.color = vec4<f32>(to_linear(c.rgb), c.a);
    }
    return o;
}

@fragment
fn fs(i: VOut) -> @location(0) vec4<f32> {
    // Red is ink coverage and green the halo around it; shapes sample a
    // patch where both are 1, so they come out as their plain color.
    let t = textureSample(atlas, atlas_sampler, i.uv).rg;
    let rgb = mix(vec3<f32>(1.0), i.color.rgb, t.r);
    return vec4<f32>(rgb, i.color.a * t.g);
}
"#;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    size: [f32; 2],
    linearize: f32,
    _pad: f32,
}

/// The part of the render target the graph occupies, in physical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Area {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// The adapter, device and queue shared by everything that draws.
pub struct Gpu {
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
}

pub fn new_instance() -> wgpu::Instance {
    wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env())
}

impl Gpu {
    /// Prefers a real GPU and falls back to a software adapter (WARP on
    /// Windows), so the app still runs on machines without working drivers.
    pub fn new(
        instance: &wgpu::Instance,
        surface: Option<&wgpu::Surface<'_>>,
    ) -> Result<Self, String> {
        let request = |force_fallback_adapter| {
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter,
                compatible_surface: surface,
                ..Default::default()
            }))
        };
        let adapter = request(false)
            .or_else(|_| request(true))
            .map_err(|e| format!("no graphics adapter available: {e}"))?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("graph device"),
            required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
            ..Default::default()
        }))
        .map_err(|e| format!("could not open graphics device: {e}"))?;
        Ok(Self {
            adapter,
            device,
            queue,
        })
    }

    /// Human-readable adapter name and backend, for diagnostics.
    pub fn describe(&self) -> String {
        let info = self.adapter.get_info();
        format!("{} ({:?})", info.name, info.backend)
    }
}

/// Draws a list of colored triangles into a sub-rectangle of a target, with
/// 4x MSAA when the format supports it.
pub struct GraphRenderer {
    pipeline: wgpu::RenderPipeline,
    uniforms: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    vertices: wgpu::Buffer,
    vertex_capacity: u64,
    atlas: wgpu::Texture,
    atlas_generation: u64,
    format: wgpu::TextureFormat,
    samples: u32,
    msaa: Option<(wgpu::TextureView, u32, u32)>,
}

impl GraphRenderer {
    pub fn new(gpu: &Gpu, format: wgpu::TextureFormat) -> Self {
        let device = &gpu.device;
        let samples = if gpu
            .adapter
            .get_texture_format_features(format)
            .flags
            .sample_count_supported(4)
        {
            4
        } else {
            1
        };
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("graph shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("graph uniforms"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("graph layout"),
            bind_group_layouts: &[Some(&bind_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("graph pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Float32x4],
                })],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: samples,
                ..Default::default()
            },
            multiview_mask: None,
            cache: None,
        });
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("graph uniforms"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let atlas = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("text atlas"),
            size: wgpu::Extent3d {
                width: ATLAS_SIZE,
                height: ATLAS_SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rg8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("text atlas"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let atlas_view = atlas.create_view(&Default::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("graph bindings"),
            layout: &bind_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniforms.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&atlas_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });
        let vertex_capacity = 1 << 16;
        let vertices = Self::vertex_buffer(device, vertex_capacity);
        Self {
            pipeline,
            uniforms,
            bind_group,
            vertices,
            vertex_capacity,
            atlas,
            atlas_generation: 0,
            format,
            samples,
            msaa: None,
        }
    }

    pub fn sample_count(&self) -> u32 {
        self.samples
    }

    fn vertex_buffer(device: &wgpu::Device, count: u64) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("graph vertices"),
            size: count * std::mem::size_of::<Vertex>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    /// Clears the whole target to white and draws `vertices` (in graph-area
    /// pixels) into `area`, uploading the text atlas first if it changed.
    pub fn draw(
        &mut self,
        gpu: &Gpu,
        target: &wgpu::TextureView,
        target_size: (u32, u32),
        area: Area,
        vertices: &[Vertex],
        text: &TextAtlas,
    ) {
        self.draw_parts(gpu, target, target_size, area, vertices, text, &[], None);
    }

    /// Draws one frame: the scene's background, then each shaded region on
    /// top of it, then the rest of the scene.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_scene(
        &mut self,
        gpu: &Gpu,
        target: &wgpu::TextureView,
        target_size: (u32, u32),
        area: Area,
        scene: &Scene,
        text: &TextAtlas,
        shadings: &[Shading],
        shader: &mut ShadingRenderer,
    ) {
        self.draw_parts(
            gpu,
            target,
            target_size,
            area,
            &scene.vertices,
            text,
            shadings,
            Some((scene.shade_after, shader)),
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_parts(
        &mut self,
        gpu: &Gpu,
        target: &wgpu::TextureView,
        target_size: (u32, u32),
        area: Area,
        vertices: &[Vertex],
        text: &TextAtlas,
        shadings: &[Shading],
        split: Option<(usize, &mut ShadingRenderer)>,
    ) {
        let (device, queue) = (&gpu.device, &gpu.queue);
        if text.generation != self.atlas_generation {
            self.atlas_generation = text.generation;
            queue.write_texture(
                self.atlas.as_image_copy(),
                &text.pixels,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(ATLAS_SIZE * ATLAS_CHANNELS as u32),
                    rows_per_image: Some(ATLAS_SIZE),
                },
                wgpu::Extent3d {
                    width: ATLAS_SIZE,
                    height: ATLAS_SIZE,
                    depth_or_array_layers: 1,
                },
            );
        }
        let count = vertices.len() as u64;
        if count > self.vertex_capacity {
            self.vertex_capacity = count.next_power_of_two();
            self.vertices = Self::vertex_buffer(device, self.vertex_capacity);
        }
        queue.write_buffer(&self.vertices, 0, bytemuck::cast_slice(vertices));
        let uniforms = Uniforms {
            size: [area.width.max(1) as f32, area.height.max(1) as f32],
            linearize: if self.format.is_srgb() { 1.0 } else { 0.0 },
            _pad: 0.0,
        };
        queue.write_buffer(&self.uniforms, 0, bytemuck::bytes_of(&uniforms));

        let (w, h) = target_size;
        if self.samples > 1 && !matches!(self.msaa, Some((_, mw, mh)) if mw == w && mh == h) {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("graph msaa"),
                size: wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: self.samples,
                dimension: wgpu::TextureDimension::D2,
                format: self.format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
            self.msaa = Some((texture.create_view(&Default::default()), w, h));
        }
        let (view, resolve_target) = match &self.msaa {
            Some((msaa, _, _)) if self.samples > 1 => (msaa, Some(target)),
            _ => (target, None),
        };

        let ax = area.x.min(w);
        let ay = area.y.min(h);
        let aw = area.width.min(w - ax);
        let ah = area.height.min(h - ay);
        if aw == 0 || ah == 0 {
            return;
        }
        let (shade_after, shader) = match split {
            Some((at, shader)) => (at.min(vertices.len()), Some(shader)),
            None => (vertices.len(), None),
        };
        let mut encoder = device.create_command_encoder(&Default::default());
        // Up to three passes over the same attachment: only the first
        // clears, so shading lands between the scene's background and its
        // foreground.
        let mut first = true;
        macro_rules! begin_pass {
            () => {{
                let load = if first {
                    wgpu::LoadOp::Clear(wgpu::Color::WHITE)
                } else {
                    wgpu::LoadOp::Load
                };
                first = false;
                let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("graph pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view,
                        depth_slice: None,
                        resolve_target,
                        ops: wgpu::Operations {
                            load,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                rp.set_viewport(
                    ax as f32,
                    ay as f32,
                    area.width as f32,
                    area.height as f32,
                    0.0,
                    1.0,
                );
                rp.set_scissor_rect(ax, ay, aw, ah);
                rp
            }};
        }
        let stride = std::mem::size_of::<Vertex>() as u64;
        macro_rules! draw_scene_part {
            ($range:expr) => {{
                let range: std::ops::Range<u32> = $range;
                if !range.is_empty() {
                    let mut rp = begin_pass!();
                    rp.set_pipeline(&self.pipeline);
                    rp.set_bind_group(0, &self.bind_group, &[]);
                    rp.set_vertex_buffer(0, self.vertices.slice(..count * stride));
                    rp.draw(range, 0..1);
                }
            }};
        }
        draw_scene_part!(0..shade_after as u32);
        if let Some(shader) = shader {
            if !shadings.is_empty() {
                shader.prepare(gpu, self.format, self.samples, shadings);
                let mut rp = begin_pass!();
                shader.draw(&mut rp, shadings);
            }
        }
        draw_scene_part!(shade_after as u32..count as u32);
        if first {
            // Nothing to draw at all; the target is still cleared.
            let _ = begin_pass!();
        }
        let _ = first;
        queue.submit(Some(encoder.finish()));
    }
}

/// Renders a scene without a window and returns tightly packed RGBA8 pixels.
/// Used by the `--render-png` self-test and by CI.
pub fn render_offscreen(
    gpu: &Gpu,
    width: u32,
    height: u32,
    scene: &Scene,
    text: &TextAtlas,
    shadings: &[Shading],
) -> Vec<u8> {
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut renderer = GraphRenderer::new(gpu, format);
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("offscreen target"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());
    let area = Area {
        x: 0,
        y: 0,
        width,
        height,
    };
    let mut shader = ShadingRenderer::new(gpu);
    renderer.draw_scene(
        gpu,
        &view,
        (width, height),
        area,
        scene,
        text,
        shadings,
        &mut shader,
    );

    let padded_row = (width * 4).div_ceil(256) * 256;
    let readback = gpu
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("offscreen readback"),
            contents: &vec![0u8; (padded_row * height) as usize],
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        });
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_row),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    gpu.queue.submit(Some(encoder.finish()));
    let slice = readback.slice(..);
    slice.map_async(wgpu::MapMode::Read, |r| r.expect("map readback buffer"));
    gpu.device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("wait for GPU");
    let data = slice.get_mapped_range().expect("read mapped buffer");
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for row in data.chunks(padded_row as usize) {
        pixels.extend_from_slice(&row[..(width * 4) as usize]);
    }
    pixels
}

#[cfg(test)]
mod tests {
    use super::*;
    use graph_core::scene::DESMOS_COLORS;
    use graph_core::{build_scene, Curve, Plot, Viewport};

    #[test]
    fn shaded_inequality_fills_its_side() {
        let Ok(gpu) = Gpu::new(&new_instance(), None) else {
            return;
        };
        let (w, h) = (200, 200);
        let vp = Viewport::new(w as f64, h as f64);
        let mut text = graph_core::TextAtlas::empty();
        let scene = build_scene(&vp, 1.0, &Plot::default(), &mut text);
        // y > 0, in blue at Desmos's shading opacity.
        let shading = Shading {
            body: "    return y - u.k[0][1];\n".into(),
            constants: vec![7.0, 0.0],
            test: Some(Test::Greater),
            color: [0.176, 0.439, 0.702, 1.0],
            fill_opacity: 0.4,
            line_px: 2.5,
            origin: [
                vp.to_world(0.0, 0.0)[0] as f32,
                vp.to_world(0.0, 0.0)[1] as f32,
            ],
            units_per_px: vp.units_per_px as f32,
        };
        let px = render_offscreen(&gpu, w, h, &scene, &text, &[shading]);
        let at = |x: u32, y: u32| &px[((y * w + x) * 4) as usize..][..4];
        // Away from the axes and grid lines, which have colours of their own.
        let above = at(37, 43);
        let below = at(37, 157);
        assert!(above[2] > above[0] + 20, "shaded above: {above:?}");
        assert!(below[2] <= below[0], "nothing shaded below: {below:?}");
    }

    #[test]
    fn offscreen_sine_has_red_curve_on_white() {
        let gpu = match Gpu::new(&new_instance(), None) {
            Ok(gpu) => gpu,
            Err(e) => {
                eprintln!("skipping: {e}");
                return;
            }
        };
        let (w, h) = (400, 240);
        let vp = Viewport::new(w as f64, h as f64);
        let curve = Curve::new(&f64::sin, DESMOS_COLORS[0]);
        let mut text = graph_core::TextAtlas::system();
        let plot = Plot {
            curves: &[curve],
            ..Plot::default()
        };
        let scene = build_scene(&vp, 1.0, &plot, &mut text);
        let px = render_offscreen(&gpu, w, h, &scene, &text, &[]);
        let at = |x: u32, y: u32| &px[((y * w + x) * 4) as usize..][..4];
        // Corner is white paper.
        assert_eq!(at(3, 3), &[255, 255, 255, 255]);
        // sin(pi/2) = 1 at x = pi/2: column 231, row 120 - 20 = 100.
        let red = at(231, 100);
        assert!(red[0] > 150 && red[1] < 120 && red[2] < 120, "got {red:?}");
        if text.has_font() {
            // The "5" under x = 5 (column 300) is drawn in dark ink.
            let dark = (124..142).any(|y| (292..308).any(|x| at(x, y)[0] < 80));
            assert!(dark, "no axis number under x = 5");
        }
    }
}
