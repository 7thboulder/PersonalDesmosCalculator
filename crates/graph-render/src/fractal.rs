//! Fractal rows drawn by the GPU: escape-time sets (Mandelbrot and Julia)
//! and bifurcation diagrams.
//!
//! The GPU only has f32, which runs out of digits once the view is about
//! 1e-5 wide. Escape-time sets are therefore drawn by perturbation: the CPU
//! iterates one reference point in f64 and the shader iterates only each
//! pixel's small offset from it, which f32 holds to any depth the f64 view
//! can reach. When the offset stops being small (the pixel's orbit passes
//! nearer 0 than the reference's does, or the reference escapes), the
//! pixel moves onto the orbit of 0 instead and carries on ("rebasing"), so
//! one reference serves the whole view.
//!
//! A bifurcation diagram is worked out by a compute pass: each thread runs
//! x → f(r, x) for one r and counts where the orbit lands in that column,
//! and a full-area pass turns the counts into ink.

use std::collections::HashMap;
use wgpu::util::DeviceExt;

/// One fractal row for the GPU to draw this frame.
#[derive(Clone, Debug)]
pub enum Fractal {
    Escape(Escape),
    Bifurcation(Bifurcation),
}

/// A Mandelbrot or Julia set, by perturbation around a reference orbit.
#[derive(Clone, Debug)]
pub struct Escape {
    /// The orbit of the reference point (Julia sets only), then the orbit
    /// of 0, each as far as it goes before escaping.
    pub orbits: Vec<[f32; 2]>,
    /// How many of `orbits` belong to the reference point; 0 for the
    /// Mandelbrot set, whose pixels all start on the orbit of 0.
    pub start_len: u32,
    /// For a Julia set each pixel starts at its own z with the same c;
    /// for the Mandelbrot set at z = 0 with its own c.
    pub julia: bool,
    /// The target pixel the reference point sits at.
    pub reference_px: [f32; 2],
    /// World units per physical pixel.
    pub units_per_px: f32,
    pub iterations: u32,
    /// Straight (not premultiplied) colour.
    pub color: [f32; 4],
}

/// Where x → f(r, x) settles for each r across the view.
#[derive(Clone, Debug)]
pub struct Bifurcation {
    /// WGSL body of `fn relation(x: f32, y: f32) -> f32`, reading r as x
    /// and the orbit's value as y, as graph-expr writes it.
    pub body: String,
    pub constants: Vec<f32>,
    /// World coordinates of the target's pixel (0, 0).
    pub origin: [f32; 2],
    pub units_per_px: f32,
    /// Where every orbit starts.
    pub start: f32,
    pub color: [f32; 4],
}

/// Steps run before an orbit is counted, so it has settled.
const TRANSIENT: u32 = 1000;
/// Steps counted per orbit.
const KEPT: u32 = 1000;
/// Orbits per pixel column, at r spread across the column.
const SAMPLES: u32 = 16;
const WORKGROUP: u32 = 64;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct EscapeUniforms {
    color: [f32; 4],
    reference_px: [f32; 2],
    units_per_px: f32,
    iterations: u32,
    start_len: u32,
    crit_offset: u32,
    crit_len: u32,
    julia: u32,
    linearize: u32,
    _pad: [u32; 3],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct BifurcationUniforms {
    color: [f32; 4],
    origin: [f32; 2],
    units_per_px: f32,
    start: f32,
    width: u32,
    height: u32,
    samples: u32,
    transient: u32,
    kept: u32,
    linearize: u32,
    _pad: [u32; 2],
    k: [[f32; 4]; crate::MAX_CONSTANTS / 4],
}

struct BifurcationPipelines {
    compute: wgpu::ComputePipeline,
    render: wgpu::RenderPipeline,
    last_used: u64,
}

/// What a prepared fractal draws with.
enum Prepared {
    Escape(wgpu::BindGroup),
    Bifurcation(String, wgpu::BindGroup),
}

pub struct FractalRenderer {
    escape_layout: wgpu::BindGroupLayout,
    escape: Option<(wgpu::TextureFormat, u32, wgpu::RenderPipeline)>,
    count_layout: wgpu::BindGroupLayout,
    ink_layout: wgpu::BindGroupLayout,
    bifurcations: HashMap<String, BifurcationPipelines>,
    /// One count buffer per bifurcation row, kept between frames.
    counts: Vec<wgpu::Buffer>,
    prepared: Vec<Prepared>,
    frame: u64,
}

/// Bifurcation pipelines unused for this many frames are dropped.
const PIPELINE_LIFETIME: u64 = 600;

impl FractalRenderer {
    pub fn new(gpu: &crate::Gpu) -> Self {
        let entry = |binding, visibility, ty| wgpu::BindGroupLayoutEntry {
            binding,
            visibility,
            ty,
            count: None,
        };
        let uniform = wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        };
        let storage = |read_only| wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only },
            has_dynamic_offset: false,
            min_binding_size: None,
        };
        let layout = |label, entries: &[wgpu::BindGroupLayoutEntry]| {
            gpu.device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some(label),
                    entries,
                })
        };
        let fragment = wgpu::ShaderStages::FRAGMENT;
        let compute = wgpu::ShaderStages::COMPUTE;
        Self {
            escape_layout: layout(
                "escape",
                &[
                    entry(0, fragment, uniform),
                    entry(1, fragment, storage(true)),
                ],
            ),
            escape: None,
            count_layout: layout(
                "bifurcation count",
                &[
                    entry(0, compute, uniform),
                    entry(1, compute, storage(false)),
                ],
            ),
            ink_layout: layout(
                "bifurcation ink",
                &[
                    entry(0, fragment, uniform),
                    entry(1, fragment, storage(true)),
                ],
            ),
            bifurcations: HashMap::new(),
            counts: Vec::new(),
            prepared: Vec::new(),
            frame: 0,
        }
    }

    /// Compiles what this frame needs, uploads each fractal's data and runs
    /// the bifurcation counts, ahead of the pass that draws them.
    pub fn prepare(
        &mut self,
        gpu: &crate::Gpu,
        format: wgpu::TextureFormat,
        samples: u32,
        target_size: (u32, u32),
        fractals: &[Fractal],
    ) {
        self.frame += 1;
        self.prepared.clear();
        let device = &gpu.device;
        let linearize = format.is_srgb() as u32;
        let mut encoder = device.create_command_encoder(&Default::default());
        let mut bifurcation = 0;
        for fractal in fractals {
            match fractal {
                Fractal::Escape(e) => {
                    if !matches!(&self.escape, Some((f, s, _)) if *f == format && *s == samples) {
                        let pipeline = self.render_pipeline(
                            gpu,
                            format,
                            samples,
                            &self.escape_layout,
                            &escape_source(),
                        );
                        self.escape = Some((format, samples, pipeline));
                    }
                    let crit_len = e.orbits.len() as u32 - e.start_len;
                    let uniforms = EscapeUniforms {
                        color: e.color,
                        reference_px: e.reference_px,
                        units_per_px: e.units_per_px,
                        iterations: e.iterations,
                        start_len: if e.julia { e.start_len } else { crit_len },
                        crit_offset: e.start_len,
                        crit_len,
                        julia: e.julia as u32,
                        linearize,
                        _pad: [0; 3],
                    };
                    let orbits = if e.orbits.is_empty() {
                        &[[0.0, 0.0]][..]
                    } else {
                        &e.orbits[..]
                    };
                    let bind = self.bind(
                        gpu,
                        &self.escape_layout,
                        bytemuck::bytes_of(&uniforms),
                        &device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                            label: Some("escape orbits"),
                            contents: bytemuck::cast_slice(orbits),
                            usage: wgpu::BufferUsages::STORAGE,
                        }),
                    );
                    self.prepared.push(Prepared::Escape(bind));
                }
                Fractal::Bifurcation(b) => {
                    if !self.bifurcations.contains_key(&b.body) {
                        let source = bifurcation_source(&b.body);
                        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                            label: Some("bifurcation"),
                            source: wgpu::ShaderSource::Wgsl(source.clone().into()),
                        });
                        let layout =
                            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                                label: Some("bifurcation count"),
                                bind_group_layouts: &[Some(&self.count_layout)],
                                immediate_size: 0,
                            });
                        let compute =
                            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                                label: Some("bifurcation count"),
                                layout: Some(&layout),
                                module: &module,
                                entry_point: Some("count"),
                                compilation_options: Default::default(),
                                cache: None,
                            });
                        let render = self.render_pipeline(
                            gpu,
                            format,
                            samples,
                            &self.ink_layout,
                            &ink_source(),
                        );
                        self.bifurcations.insert(
                            b.body.clone(),
                            BifurcationPipelines {
                                compute,
                                render,
                                last_used: self.frame,
                            },
                        );
                    }
                    let pipelines = self.bifurcations.get_mut(&b.body).expect("inserted");
                    pipelines.last_used = self.frame;
                    let (w, h) = (target_size.0.max(1), target_size.1.max(1));
                    let size = w as u64 * h as u64 * 4;
                    if self.counts.len() <= bifurcation || self.counts[bifurcation].size() != size {
                        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                            label: Some("bifurcation counts"),
                            size,
                            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                            mapped_at_creation: false,
                        });
                        if self.counts.len() <= bifurcation {
                            self.counts.push(buffer);
                        } else {
                            self.counts[bifurcation] = buffer;
                        }
                    }
                    let counts = &self.counts[bifurcation];
                    bifurcation += 1;
                    let mut k = [[0.0; 4]; crate::MAX_CONSTANTS / 4];
                    for (i, &v) in b.constants.iter().take(crate::MAX_CONSTANTS).enumerate() {
                        k[i / 4][i % 4] = v;
                    }
                    let uniforms = BifurcationUniforms {
                        color: b.color,
                        origin: b.origin,
                        units_per_px: b.units_per_px,
                        start: b.start,
                        width: w,
                        height: h,
                        samples: SAMPLES,
                        transient: TRANSIENT,
                        kept: KEPT,
                        linearize,
                        _pad: [0; 2],
                        k,
                    };
                    let bytes = bytemuck::bytes_of(&uniforms);
                    let count_bind = self.bind(gpu, &self.count_layout, bytes, counts);
                    encoder.clear_buffer(counts, 0, None);
                    {
                        let mut pass = encoder.begin_compute_pass(&Default::default());
                        let pipelines = &self.bifurcations[&b.body];
                        pass.set_pipeline(&pipelines.compute);
                        pass.set_bind_group(0, &count_bind, &[]);
                        pass.dispatch_workgroups((w * SAMPLES).div_ceil(WORKGROUP), 1, 1);
                    }
                    let ink_bind = self.bind(gpu, &self.ink_layout, bytes, counts);
                    self.prepared
                        .push(Prepared::Bifurcation(b.body.clone(), ink_bind));
                }
            }
        }
        gpu.queue.submit(Some(encoder.finish()));
        let frame = self.frame;
        self.bifurcations
            .retain(|_, p| frame - p.last_used < PIPELINE_LIFETIME);
        self.counts.truncate(bifurcation);
    }

    /// Draws every fractal prepared this frame, one full-area triangle each.
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        for prepared in &self.prepared {
            let (pipeline, bind) = match prepared {
                Prepared::Escape(bind) => match &self.escape {
                    Some((_, _, pipeline)) => (pipeline, bind),
                    None => continue,
                },
                Prepared::Bifurcation(body, bind) => match self.bifurcations.get(body) {
                    Some(p) => (&p.render, bind),
                    None => continue,
                },
            };
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, bind, &[]);
            pass.draw(0..3, 0..1);
        }
    }

    pub fn has_work(&self) -> bool {
        !self.prepared.is_empty()
    }

    fn bind(
        &self,
        gpu: &crate::Gpu,
        layout: &wgpu::BindGroupLayout,
        uniforms: &[u8],
        storage: &wgpu::Buffer,
    ) -> wgpu::BindGroup {
        let uniforms = gpu
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("fractal uniforms"),
                contents: uniforms,
                usage: wgpu::BufferUsages::UNIFORM,
            });
        gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("fractal"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniforms.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: storage.as_entire_binding(),
                },
            ],
        })
    }

    fn render_pipeline(
        &self,
        gpu: &crate::Gpu,
        format: wgpu::TextureFormat,
        samples: u32,
        bind_layout: &wgpu::BindGroupLayout,
        source: &str,
    ) -> wgpu::RenderPipeline {
        let device = &gpu.device;
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("fractal"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("fractal"),
            bind_group_layouts: &[Some(bind_layout)],
            immediate_size: 0,
        });
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("fractal"),
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

/// The vertex stage and colour helpers every fractal pass shares.
const COMMON: &str = r#"
fn to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((c + 0.055) / 1.055, vec3<f32>(2.4));
    return select(hi, lo, c <= vec3<f32>(0.04045));
}

@vertex
fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    // One triangle that covers the whole viewport.
    let p = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -3.0), vec2<f32>(-1.0, 1.0), vec2<f32>(3.0, 1.0));
    return vec4<f32>(p[i], 0.0, 1.0);
}

fn ink(color: vec4<f32>, alpha: f32, linearize: u32) -> vec4<f32> {
    var rgb = color.rgb;
    if (linearize != 0u) {
        rgb = to_linear(rgb);
    }
    return vec4<f32>(rgb, color.a * alpha);
}
"#;

fn escape_source() -> String {
    format!(
        r#"
struct Uniforms {{
    color: vec4<f32>,
    reference_px: vec2<f32>,
    units_per_px: f32,
    iterations: u32,
    start_len: u32,
    crit_offset: u32,
    crit_len: u32,
    julia: u32,
    linearize: u32,
}};
@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var<storage, read> orbits: array<vec2<f32>>;
{COMMON}

fn mul(a: vec2<f32>, b: vec2<f32>) -> vec2<f32> {{
    return vec2<f32>(a.x * b.x - a.y * b.y, a.x * b.y + a.y * b.x);
}}

const BAILOUT: f32 = 4096.0;

@fragment
fn fs(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {{
    // This pixel's offset from the reference point, in world units; y
    // grows upwards on the graph.
    let d = (pos.xy - u.reference_px) * vec2<f32>(u.units_per_px, -u.units_per_px);
    var dz = select(vec2<f32>(0.0), d, u.julia != 0u);
    let dc = select(d, vec2<f32>(0.0), u.julia != 0u);
    // Which orbit the pixel follows (it starts on the reference's and may
    // move to the orbit of 0), and how far along it is.
    var base = 0u;
    var len = u.start_len;
    var m = 0u;
    var z = orbits[0] + dz;
    var n = 0u;
    var r2 = dot(z, z);
    // dz/dc (or dz/dz₀ for a Julia set), for the distance to the set.
    var der = select(vec2<f32>(0.0), vec2<f32>(1.0, 0.0), u.julia != 0u);
    let one = select(vec2<f32>(1.0, 0.0), vec2<f32>(0.0), u.julia != 0u);
    loop {{
        if (n >= u.iterations || r2 > BAILOUT) {{
            break;
        }}
        der = 2.0 * mul(z, der) + one;
        // (Z + dz)² + c = Z² + C + (2Z + dz) dz + dc, and Z² + C is the
        // next step of the orbit.
        dz = mul(2.0 * orbits[base + m] + dz, dz) + dc;
        m += 1u;
        z = orbits[base + m] + dz;
        r2 = dot(z, z);
        n += 1u;
        if (r2 < dot(dz, dz) || m + 1u >= len) {{
            // The pixel is nearer 0 than to the orbit it follows, or that
            // orbit ends: carry on along the orbit of 0, which starts there.
            dz = z;
            base = u.crit_offset;
            len = u.crit_len;
            m = 0u;
        }}
    }}
    if (r2 <= BAILOUT) {{
        return ink(u.color, 1.0, u.linearize); // Never escaped: in the set.
    }}
    // How far the pixel is from the set, in pixels: dark on its thinnest
    // filaments however deep the view, fading away from it.
    let distance = sqrt(r2) * log(r2) * 0.5 / length(der) / u.units_per_px;
    let edge = select(1.0, 1.0 / (1.0 + 4.0 * distance * distance), distance == distance);
    // Escape time, smoothed so it doesn't step, gives bands between, darker
    // nearer the set.
    let mu = f32(n) + 1.0 - log2(max(log2(r2) * 0.5, 1.0));
    let wave = 0.5 - 0.5 * cos(log2(mu + 1.0) * 3.0);
    let glow = 1.0 / (1.0 + distance * 0.05);
    // Never quite solid, so the set itself stands out from its filaments.
    return ink(u.color, max(0.9 * edge, 0.05 + (0.1 + 0.25 * wave) * glow), u.linearize);
}}
"#
    )
}

fn bifurcation_source(body: &str) -> String {
    format!(
        r#"
struct Uniforms {{
    color: vec4<f32>,
    origin: vec2<f32>,
    units_per_px: f32,
    start: f32,
    width: u32,
    height: u32,
    samples: u32,
    transient: u32,
    kept: u32,
    linearize: u32,
    k: array<vec4<f32>, {constants}>,
}};
@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var<storage, read_write> counts: array<atomic<u32>>;

{prelude}

fn relation(x: f32, y: f32) -> f32 {{
{body}}}

@compute @workgroup_size({WORKGROUP})
fn count(@builtin(global_invocation_id) id: vec3<u32>) {{
    let column = id.x / u.samples;
    if (column >= u.width) {{
        return;
    }}
    let sample = id.x % u.samples;
    let r = u.origin.x + (f32(column) + (f32(sample) + 0.5) / f32(u.samples)) * u.units_per_px;
    var x = u.start;
    for (var i = 0u; i < u.transient; i += 1u) {{
        x = relation(r, x);
    }}
    for (var i = 0u; i < u.kept; i += 1u) {{
        x = relation(r, x);
        if (x != x || abs(x) > 3.0e38) {{
            return;
        }}
        let row = floor((u.origin.y - x) / u.units_per_px);
        if (row >= 0.0 && row < f32(u.height)) {{
            atomicAdd(&counts[u32(row) * u.width + column], 1u);
        }}
    }}
}}
"#,
        prelude = graph_core::shader::PRELUDE,
        constants = crate::MAX_CONSTANTS / 4,
    )
}

fn ink_source() -> String {
    format!(
        r#"
struct Uniforms {{
    color: vec4<f32>,
    origin: vec2<f32>,
    units_per_px: f32,
    start: f32,
    width: u32,
    height: u32,
    samples: u32,
    transient: u32,
    kept: u32,
    linearize: u32,
}};
@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var<storage, read> counts: array<u32>;
{COMMON}

@fragment
fn fs(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {{
    let p = vec2<u32>(pos.xy);
    let hits = counts[p.y * u.width + p.x];
    if (hits == 0u) {{
        discard;
    }}
    // As a share of a column's hits, times the column's height: 1 where
    // the orbit spreads evenly down the whole view, far more on a point it
    // settles on.
    let density = f32(hits) * f32(u.height) / f32(u.samples * u.kept);
    return ink(u.color, 1.0 - exp(-0.6 * density - 0.15), u.linearize);
}}
"#
    )
}
