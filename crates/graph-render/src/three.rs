//! 3D graphs: lit surfaces and the box's lines, drawn with a depth buffer
//! so nearer things hide farther ones. Everything arrives in box
//! coordinates (-1 to 1 on each axis) with the camera as a matrix, so
//! turning the box only rewrites a uniform.

use graph_core::three::{LineVertex, Mesh, Vertex3};

const SHADER: &str = r#"
struct Uniforms {
    m: array<vec4<f32>, 4>,
    /// Toward the camera, in box coordinates.
    toward: vec4<f32>,
    /// Surface grid lines: spacing in box units (xy), offset (zw).
    grid: vec4<f32>,
    /// Graph area in pixels (xy), whether to linearize colours (z).
    size: vec4<f32>,
};
@group(0) @binding(0) var<uniform> u: Uniforms;

fn clip(p: vec3<f32>) -> vec4<f32> {
    let q = vec4<f32>(p, 1.0);
    return vec4<f32>(dot(u.m[0], q), dot(u.m[1], q), dot(u.m[2], q), dot(u.m[3], q));
}

fn to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((c + 0.055) / 1.055, vec3<f32>(2.4));
    return select(hi, lo, c <= vec3<f32>(0.04045));
}

fn output(c: vec4<f32>) -> vec4<f32> {
    if (u.size.z > 0.5) {
        return vec4<f32>(to_linear(c.rgb), c.a);
    }
    return c;
}

struct SurfaceOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) at: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec4<f32>,
};

@vertex
fn surface_vs(@location(0) p: vec3<f32>, @location(1) n: vec3<f32>, @location(2) c: vec4<f32>) -> SurfaceOut {
    var o: SurfaceOut;
    o.pos = clip(p);
    o.at = p;
    o.normal = n;
    o.color = c;
    return o;
}

@fragment
fn surface_fs(i: SurfaceOut) -> @location(0) vec4<f32> {
    // The box trims everything outside it.
    if (any(abs(i.at) > vec3<f32>(1.0001))) {
        discard;
    }
    var n = normalize(i.normal);
    var tint = 1.0;
    // The underside is lit like the top, a little darker.
    if (dot(n, u.toward.xyz) < 0.0) {
        n = -n;
        tint = 0.92;
    }
    // Lit mostly from the camera, a little from above, so whatever faces
    // the viewer reads clearly, inside or out.
    let light = normalize(u.toward.xyz + vec3<f32>(0.0, 0.0, 0.25));
    let diffuse = max(dot(n, light), 0.0);
    let half_way = normalize(light + u.toward.xyz);
    let shine = pow(max(dot(n, half_way), 0.0), 40.0) * 0.12;
    var rgb = i.color.rgb * (0.55 + 0.45 * diffuse) * tint + vec3<f32>(shine);
    // Faint grid lines at the box's grid steps help show the shape.
    let g = (i.at.xy - u.grid.zw) / u.grid.xy;
    let d = abs(fract(g + 0.5) - 0.5) / max(fwidth(g), vec2<f32>(1e-6));
    let line = 1.0 - min(min(d.x, d.y), 1.0);
    rgb = mix(rgb, rgb * 0.75, line * 0.6);
    return output(vec4<f32>(min(rgb, vec3<f32>(1.0)), i.color.a));
}

struct LineOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) color: vec4<f32>,
    /// Where in the quad, for dots: -1 to 1 each way.
    @location(1) uv: vec2<f32>,
    @location(2) round: f32,
    /// Box coordinates, so curves are trimmed to the box.
    @location(3) at: vec3<f32>,
};

@vertex
fn line_vs(
    @location(0) a: vec3<f32>, @location(1) end: f32,
    @location(2) b: vec3<f32>, @location(3) side: f32,
    @location(4) c: vec4<f32>, @location(5) width: f32, @location(6) round: f32,
) -> LineOut {
    let half_size = u.size.xy / 2.0;
    let ca = clip(a);
    let cb = clip(b);
    let pa = ca.xy / ca.w * half_size;
    let pb = cb.xy / cb.w * half_size;
    var dir = pb - pa;
    if (length(dir) < 1e-6) {
        dir = vec2<f32>(1.0, 0.0);
    }
    dir = normalize(dir);
    var offset = vec2<f32>(-dir.y, dir.x) * side * width / 2.0;
    if (round > 0.5) {
        // A dot's quad reaches along as far as across.
        offset = offset + dir * (end * 2.0 - 1.0) * width / 2.0;
    }
    var o: LineOut;
    var p = select(ca, cb, end > 0.5);
    p = vec4<f32>(p.xy + offset / half_size * p.w, p.z, p.w);
    o.uv = vec2<f32>(end * 2.0 - 1.0, side);
    o.round = round;
    o.at = select(a, b, end > 0.5);
    // Lines win against a surface at the same depth, such as the grid
    // under z = 0.
    p.z = p.z - 0.002 * p.w;
    o.pos = p;
    o.color = c;
    return o;
}

@fragment
fn line_fs(i: LineOut) -> @location(0) vec4<f32> {
    if (i.round > 0.5 && dot(i.uv, i.uv) > 1.0) {
        discard;
    }
    if (any(abs(i.at) > vec3<f32>(1.001))) {
        discard;
    }
    return output(i.color);
}
"#;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    m: [[f32; 4]; 4],
    toward: [f32; 4],
    grid: [f32; 4],
    size: [f32; 4],
}

/// What a 3D frame draws.
pub struct Frame3<'a> {
    /// Box coordinates to clip space, by rows (`View3::matrix`).
    pub matrix: [[f32; 4]; 4],
    /// The direction toward the camera, in box coordinates.
    pub toward: [f32; 3],
    /// The surfaces' grid lines: spacing and offset in box units.
    pub grid: [f32; 4],
    pub surfaces: &'a [&'a Mesh],
    /// Changes whenever `surfaces` do, so unchanged ones are not uploaded
    /// again while the box turns.
    pub surfaces_key: u64,
    /// The box's lines, which change as it turns.
    pub lines: &'a [LineVertex],
    /// Points and curves, which stay put while the box turns.
    pub marks: &'a [LineVertex],
    /// Changes whenever `marks` do, as `surfaces_key`.
    pub marks_key: u64,
}

pub struct Renderer3 {
    surface_pipeline: wgpu::RenderPipeline,
    /// Translucent surfaces (shaded solids): drawn last, without hiding
    /// what is behind them.
    see_through_pipeline: wgpu::RenderPipeline,
    line_pipeline: wgpu::RenderPipeline,
    uniforms: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    vertices: Grown,
    indices: Grown,
    lines: Grown,
    marks: Grown,
    marks_uploaded: Option<u64>,
    index_count: u32,
    /// Where the translucent surfaces' indices start.
    opaque_count: u32,
    uploaded: Option<u64>,
    depth: Option<(wgpu::TextureView, u32, u32)>,
    samples: u32,
}

/// A buffer that grows to fit what is written to it.
struct Grown {
    buffer: wgpu::Buffer,
    usage: wgpu::BufferUsages,
    label: &'static str,
}

impl Grown {
    fn new(device: &wgpu::Device, label: &'static str, usage: wgpu::BufferUsages) -> Self {
        let usage = usage | wgpu::BufferUsages::COPY_DST;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: 1 << 16,
            usage,
            mapped_at_creation: false,
        });
        Self {
            buffer,
            usage,
            label,
        }
    }

    fn write(&mut self, gpu: &crate::Gpu, bytes: &[u8]) {
        if bytes.len() as u64 > self.buffer.size() {
            self.buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(self.label),
                size: (bytes.len() as u64).next_power_of_two(),
                usage: self.usage,
                mapped_at_creation: false,
            });
        }
        if !bytes.is_empty() {
            gpu.queue.write_buffer(&self.buffer, 0, bytes);
        }
    }
}

const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

impl Renderer3 {
    pub fn new(gpu: &crate::Gpu, format: wgpu::TextureFormat, samples: u32) -> Self {
        let device = &gpu.device;
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("3d shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("3d uniforms"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("3d layout"),
            bind_group_layouts: &[Some(&bind_layout)],
            immediate_size: 0,
        });
        let pipeline = |label, vs, fs, buffer: wgpu::VertexBufferLayout, write_depth: bool| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some(vs),
                    compilation_options: Default::default(),
                    buffers: &[Some(buffer)],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(write_depth),
                    depth_compare: Some(wgpu::CompareFunction::LessEqual),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: wgpu::MultisampleState {
                    count: samples,
                    ..Default::default()
                },
                multiview_mask: None,
                cache: None,
            })
        };
        const SURFACE_ATTRIBUTES: [wgpu::VertexAttribute; 3] =
            wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x4];
        let surface_layout = || wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex3>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &SURFACE_ATTRIBUTES,
        };
        let surface_pipeline = pipeline(
            "3d surfaces",
            "surface_vs",
            "surface_fs",
            surface_layout(),
            true,
        );
        let see_through_pipeline = pipeline(
            "3d see-through surfaces",
            "surface_vs",
            "surface_fs",
            surface_layout(),
            false,
        );
        let line_pipeline = pipeline(
            "3d lines",
            "line_vs",
            "line_fs",
            wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<LineVertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &wgpu::vertex_attr_array![
                    0 => Float32x3, 1 => Float32, 2 => Float32x3, 3 => Float32,
                    4 => Float32x4, 5 => Float32, 6 => Float32
                ],
            },
            true,
        );
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("3d uniforms"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("3d bindings"),
            layout: &bind_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniforms.as_entire_binding(),
            }],
        });
        Self {
            surface_pipeline,
            see_through_pipeline,
            line_pipeline,
            uniforms,
            bind_group,
            vertices: Grown::new(device, "3d vertices", wgpu::BufferUsages::VERTEX),
            indices: Grown::new(device, "3d indices", wgpu::BufferUsages::INDEX),
            lines: Grown::new(device, "3d lines", wgpu::BufferUsages::VERTEX),
            marks: Grown::new(device, "3d marks", wgpu::BufferUsages::VERTEX),
            marks_uploaded: None,
            index_count: 0,
            opaque_count: 0,
            uploaded: None,
            depth: None,
            samples,
        }
    }

    /// Uploads what changed and keeps a depth buffer the target's size.
    pub(crate) fn prepare(
        &mut self,
        gpu: &crate::Gpu,
        frame: &Frame3,
        target_size: (u32, u32),
        area: crate::Area,
        linearize: bool,
    ) {
        if self.uploaded != Some(frame.surfaces_key) {
            self.uploaded = Some(frame.surfaces_key);
            let mut vertices: Vec<Vertex3> = Vec::new();
            let mut indices: Vec<u32> = Vec::new();
            let see_through = |m: &Mesh| m.vertices.first().is_some_and(|v| v.color[3] < 0.999);
            // Opaque surfaces first, then the translucent ones.
            for translucent in [false, true] {
                if translucent {
                    self.opaque_count = indices.len() as u32;
                }
                for mesh in frame
                    .surfaces
                    .iter()
                    .filter(|m| see_through(m) == translucent)
                {
                    let base = vertices.len() as u32;
                    vertices.extend_from_slice(&mesh.vertices);
                    indices.extend(mesh.indices.iter().map(|i| i + base));
                }
            }
            self.vertices.write(gpu, bytemuck::cast_slice(&vertices));
            self.indices.write(gpu, bytemuck::cast_slice(&indices));
            self.index_count = indices.len() as u32;
        }
        self.lines.write(gpu, bytemuck::cast_slice(frame.lines));
        if self.marks_uploaded != Some(frame.marks_key) {
            self.marks_uploaded = Some(frame.marks_key);
            self.marks.write(gpu, bytemuck::cast_slice(frame.marks));
        }
        let t = frame.toward;
        let uniforms = Uniforms {
            m: frame.matrix,
            toward: [t[0], t[1], t[2], 0.0],
            grid: frame.grid,
            size: [
                area.width.max(1) as f32,
                area.height.max(1) as f32,
                if linearize { 1.0 } else { 0.0 },
                0.0,
            ],
        };
        gpu.queue
            .write_buffer(&self.uniforms, 0, bytemuck::bytes_of(&uniforms));
        let (w, h) = target_size;
        if !matches!(self.depth, Some((_, dw, dh)) if dw == w && dh == h) {
            let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("3d depth"),
                size: wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: self.samples,
                dimension: wgpu::TextureDimension::D2,
                format: DEPTH_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
            self.depth = Some((texture.create_view(&Default::default()), w, h));
        }
    }

    pub(crate) fn depth_view(&self) -> &wgpu::TextureView {
        &self.depth.as_ref().expect("prepared").0
    }

    pub(crate) fn draw(&self, rp: &mut wgpu::RenderPass<'_>, frame: &Frame3) {
        rp.set_bind_group(0, &self.bind_group, &[]);
        if self.opaque_count > 0 {
            rp.set_pipeline(&self.surface_pipeline);
            rp.set_vertex_buffer(0, self.vertices.buffer.slice(..));
            rp.set_index_buffer(self.indices.buffer.slice(..), wgpu::IndexFormat::Uint32);
            rp.draw_indexed(0..self.opaque_count, 0, 0..1);
        }
        for (buffer, count) in [
            (&self.lines, frame.lines.len()),
            (&self.marks, frame.marks.len()),
        ] {
            if count > 0 {
                rp.set_pipeline(&self.line_pipeline);
                rp.set_vertex_buffer(0, buffer.buffer.slice(..));
                rp.draw(0..count as u32, 0..1);
            }
        }
        if self.index_count > self.opaque_count {
            rp.set_pipeline(&self.see_through_pipeline);
            rp.set_vertex_buffer(0, self.vertices.buffer.slice(..));
            rp.set_index_buffer(self.indices.buffer.slice(..), wgpu::IndexFormat::Uint32);
            rp.draw_indexed(self.opaque_count..self.index_count, 0, 0..1);
        }
    }
}
