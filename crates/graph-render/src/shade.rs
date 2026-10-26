//! Implicit relations drawn by the GPU: the shading inside an inequality
//! and, where the style allows it, the curve along its boundary.
//!
//! Each region arrives as the WGSL body of its relation (built by
//! graph-expr from the compiled expression). That body is pasted into a
//! fragment shader which turns the pixel into world coordinates and
//! evaluates the relation there. Dividing the value by the length of its
//! screen-space gradient turns it into a signed distance in pixels, which
//! both smooths the edge of the shading and draws the boundary at a chosen
//! thickness, however densely the curve folds. The pipeline for a given
//! body is compiled once and reused while the expression stays the same, so
//! dragging a slider only rewrites a uniform.

use std::collections::HashMap;

/// Which side of the relation to shade.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Test {
    /// f(x, y) < 0, with a boundary that is not part of the region.
    Less,
    LessOrEqual,
    Greater,
    GreaterOrEqual,
}

impl Test {
    /// Whether a boundary pixel belongs to the region. A strict inequality
    /// still covers its boundary: Desmos draws the line there regardless.
    pub fn includes_boundary(self) -> bool {
        matches!(self, Test::LessOrEqual | Test::GreaterOrEqual)
    }

    fn sense(self) -> f32 {
        match self {
            Test::Less | Test::LessOrEqual => -1.0,
            Test::Greater | Test::GreaterOrEqual => 1.0,
        }
    }
}

/// One relation for the GPU to draw this frame.
#[derive(Clone, Debug)]
pub struct Shading {
    /// WGSL body of `fn f(x: f32, y: f32) -> f32`, including `return`.
    pub body: String,
    /// Which side to fill, or `None` for an equation, which has no inside.
    pub test: Option<Test>,
    /// Straight (not premultiplied) colour.
    pub color: [f32; 4],
    /// How much colour the filled side gets; Desmos uses 0.4.
    pub fill_opacity: f32,
    /// Thickness of the boundary curve in physical pixels, or 0 to leave it
    /// to the CPU tracer (dashed and dotted styles, which a per-pixel
    /// shader cannot follow).
    pub line_px: f32,
    /// World coordinates of the target's pixel (0, 0).
    pub origin: [f32; 2],
    /// World units per physical pixel.
    pub units_per_px: f32,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    color: [f32; 4],
    origin: [f32; 2],
    units_per_px: f32,
    sense: f32,
    fill_opacity: f32,
    line_px: f32,
    _pad: [f32; 2],
}

/// Uniform buffers are written at 256-byte aligned offsets, which is the
/// strictest alignment wgpu's downlevel defaults require.
const UNIFORM_STRIDE: u64 = 256;
/// How many regions one frame can shade before the buffer is grown.
const INITIAL_REGIONS: u64 = 8;

struct Pipeline {
    pipeline: wgpu::RenderPipeline,
    /// Frame counter when this pipeline was last used, for eviction.
    last_used: u64,
}

pub struct ShadingRenderer {
    layout: wgpu::BindGroupLayout,
    pipelines: HashMap<String, Pipeline>,
    uniforms: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    capacity: u64,
    frame: u64,
}

/// Pipelines unused for this many frames are dropped.
const PIPELINE_LIFETIME: u64 = 600;

impl ShadingRenderer {
    pub fn new(gpu: &crate::Gpu) -> Self {
        let layout = gpu
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("shading uniforms"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: wgpu::BufferSize::new(
                            std::mem::size_of::<Uniforms>() as u64
                        ),
                    },
                    count: None,
                }],
            });
        let (uniforms, bind_group) = Self::buffer(gpu, &layout, INITIAL_REGIONS);
        Self {
            layout,
            pipelines: HashMap::new(),
            uniforms,
            bind_group,
            capacity: INITIAL_REGIONS,
            frame: 0,
        }
    }

    fn buffer(
        gpu: &crate::Gpu,
        layout: &wgpu::BindGroupLayout,
        regions: u64,
    ) -> (wgpu::Buffer, wgpu::BindGroup) {
        let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("shading uniforms"),
            size: regions * UNIFORM_STRIDE,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("shading uniforms"),
            layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &buffer,
                    offset: 0,
                    size: wgpu::BufferSize::new(std::mem::size_of::<Uniforms>() as u64),
                }),
            }],
        });
        (buffer, bind_group)
    }

    /// Compiles any pipeline this frame needs and writes the uniforms.
    pub fn prepare(
        &mut self,
        gpu: &crate::Gpu,
        format: wgpu::TextureFormat,
        samples: u32,
        shadings: &[Shading],
    ) {
        self.frame += 1;
        if shadings.len() as u64 > self.capacity {
            self.capacity = (shadings.len() as u64).next_power_of_two();
            let (buffer, bind_group) = Self::buffer(gpu, &self.layout, self.capacity);
            self.uniforms = buffer;
            self.bind_group = bind_group;
        }
        for (i, s) in shadings.iter().enumerate() {
            if !self.pipelines.contains_key(&s.body) {
                let pipeline = self.compile(gpu, format, samples, &s.body);
                self.pipelines.insert(
                    s.body.clone(),
                    Pipeline {
                        pipeline,
                        last_used: self.frame,
                    },
                );
            }
            if let Some(p) = self.pipelines.get_mut(&s.body) {
                p.last_used = self.frame;
            }
            let uniforms = Uniforms {
                color: s.color,
                origin: s.origin,
                units_per_px: s.units_per_px,
                sense: s.test.map_or(0.0, Test::sense),
                fill_opacity: if s.test.is_some() {
                    s.fill_opacity
                } else {
                    0.0
                },
                line_px: s.line_px,
                _pad: [0.0; 2],
            };
            gpu.queue.write_buffer(
                &self.uniforms,
                i as u64 * UNIFORM_STRIDE,
                bytemuck::bytes_of(&uniforms),
            );
        }
        let frame = self.frame;
        self.pipelines
            .retain(|_, p| frame - p.last_used < PIPELINE_LIFETIME);
    }

    /// Draws every region prepared this frame, one full-area triangle each.
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>, shadings: &[Shading]) {
        for (i, s) in shadings.iter().enumerate() {
            let Some(p) = self.pipelines.get(&s.body) else {
                continue;
            };
            pass.set_pipeline(&p.pipeline);
            pass.set_bind_group(0, &self.bind_group, &[(i as u64 * UNIFORM_STRIDE) as u32]);
            pass.draw(0..3, 0..1);
        }
    }

    fn compile(
        &self,
        gpu: &crate::Gpu,
        format: wgpu::TextureFormat,
        samples: u32,
        body: &str,
    ) -> wgpu::RenderPipeline {
        let source = shader_source(body, format.is_srgb());
        let module = gpu
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("shading"),
                source: wgpu::ShaderSource::Wgsl(source.into()),
            });
        let layout = gpu
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("shading"),
                bind_group_layouts: &[Some(&self.layout)],
                immediate_size: 0,
            });
        gpu.device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("shading"),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vs"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &module,
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
            })
    }
}

fn shader_source(body: &str, srgb: bool) -> String {
    format!(
        r#"
struct Uniforms {{
    color: vec4<f32>,
    origin: vec2<f32>,
    units_per_px: f32,
    sense: f32,
    fill_opacity: f32,
    line_px: f32,
    pad: vec2<f32>,
}};
@group(0) @binding(0) var<uniform> u: Uniforms;

{prelude}

fn relation(x: f32, y: f32) -> f32 {{
{body}}}

fn to_linear(c: vec3<f32>) -> vec3<f32> {{
    let lo = c / 12.92;
    let hi = pow((c + 0.055) / 1.055, vec3<f32>(2.4));
    return select(hi, lo, c <= vec3<f32>(0.04045));
}}

@vertex
fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {{
    // One triangle that covers the whole viewport.
    let p = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -3.0), vec2<f32>(-1.0, 1.0), vec2<f32>(3.0, 1.0));
    return vec4<f32>(p[i], 0.0, 1.0);
}}

@fragment
fn fs(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {{
    // Screen pixels to world coordinates; y grows upwards on the graph.
    let x = u.origin.x + pos.x * u.units_per_px;
    let y = u.origin.y - pos.y * u.units_per_px;
    let v = relation(x, y);
    if (v != v) {{
        discard; // Undefined here: neither inside the region nor on it.
    }}
    // How fast the relation changes across one pixel turns its value into a
    // signed distance in pixels, which is what both the fill edge and the
    // boundary thickness are measured in.
    let slope = max(length(vec2<f32>(dpdx(v), dpdy(v))), 1e-30);
    let d = v / slope;
    var alpha = 0.0;
    if (u.fill_opacity > 0.0) {{
        alpha = clamp(u.sense * d + 0.5, 0.0, 1.0) * u.fill_opacity;
    }}
    if (u.line_px > 0.0) {{
        // A line of its own width, centred on the curve.
        alpha = max(alpha, clamp(u.line_px * 0.5 + 0.5 - abs(d), 0.0, 1.0));
    }}
    if (alpha <= 0.0) {{
        discard;
    }}
    var rgb = u.color.rgb;
    if ({linearize}) {{
        rgb = to_linear(rgb);
    }}
    return vec4<f32>(rgb, u.color.a * alpha);
}}
"#,
        prelude = graph_core::shader::PRELUDE,
        body = body,
        linearize = if srgb { "true" } else { "false" },
    )
}
