use crate::engine::Engine;
use graph_core::scene::DESMOS_COLORS;
use graph_core::{build_scene, TextAtlas, Viewport};
use graph_render::{new_instance, render_offscreen, Area, Gpu, GraphRenderer, ShadingRenderer};
use serde_json::json;
use std::time::{Duration, Instant};

/// Rows as the panel would send them, cycling through Desmos's colors.
fn rows(latex: &[String]) -> serde_json::Value {
    let rows: Vec<_> = latex
        .iter()
        .enumerate()
        .map(|(i, l)| {
            let c = DESMOS_COLORS[i % DESMOS_COLORS.len()];
            let hex = format!(
                "#{:02x}{:02x}{:02x}",
                (c[0] * 255.0).round() as u8,
                (c[1] * 255.0).round() as u8,
                (c[2] * 255.0).round() as u8
            );
            json!({"id": i, "latex": l, "color": hex})
        })
        .collect();
    serde_json::Value::Array(rows)
}

/// Graphs the given expressions (y = sin x by default) offscreen and saves a
/// PNG. Proves the GPU path works on a machine without opening a window.
pub fn render_png(path: &str, width: u32, height: u32, latex: &[String]) -> Result<(), String> {
    let gpu = Gpu::new(&new_instance(), None)?;
    let vp = Viewport::new(width as f64, height as f64);
    let default = ["y=\\sin\\left(x\\right)".to_owned()];
    let latex = if latex.is_empty() {
        &default[..]
    } else {
        latex
    };
    let mut engine = Engine::default();
    let results = engine.update(&rows(latex));
    for (l, r) in latex.iter().zip(results.as_array().into_iter().flatten()) {
        if let Some(e) = r["error"].as_str() {
            println!("{l}: {e}");
        }
    }
    let mut text = TextAtlas::system();

    let start = Instant::now();
    let scene = engine.with_plot(|plot| build_scene(&vp, 1.0, plot, &mut text));
    let build = start.elapsed();
    let shadings = engine.shadings(vp.to_world(0.0, 0.0), vp.units_per_px, 1.0);
    let pixels = render_offscreen(&gpu, width, height, &scene, &text, &shadings);

    let file = std::fs::File::create(path).map_err(|e| format!("{path}: {e}"))?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
    writer
        .write_image_data(&pixels)
        .map_err(|e| e.to_string())?;

    println!(
        "wrote {path} ({width}x{height}) on {}: {} vertices built in {:.2} ms",
        gpu.describe(),
        scene.vertices.len(),
        build.as_secs_f64() * 1000.0
    );
    Ok(())
}

/// Expressions for the slider benchmark: a mix of smooth curves, asymptotes,
/// an x = g(y) curve and a function, all depending on the slider `a`.
const BENCH_ROWS: &[&str] = &[
    "a=1",
    "y=a\\sin\\left(x\\right)",
    "y=x^2-a",
    "y=\\frac{1}{x-a}",
    "y=\\tan\\left(ax\\right)",
    "x=\\cos\\left(ay\\right)+a",
    "f\\left(x\\right)=\\sqrt{\\left|x\\right|}\\cdot a",
    "y=f\\left(x\\right)-3",
    "y=\\operatorname{floor}\\left(x+a\\right)",
];

/// Frame budget at 60 frames per second.
const BUDGET: Duration = Duration::from_micros(16_667);

/// The implicit benchmark set: the graphs Phase 2 is judged on, from a
/// plain conic to curves that fold across the whole view. Each is timed on
/// its own, then all of them together.
const IMPLICIT_ROWS: &[(&str, &str)] = &[
    ("circle", "x^2+y^2=25"),
    ("rotated ellipse", "x^2+xy+y^2=9"),
    ("hyperbola", "x^2-y^2=4"),
    (
        "lemniscate",
        "\\left(x^2+y^2\\right)^2=50\\left(x^2-y^2\\right)",
    ),
    ("folium", "x^3+y^3=6xy"),
    ("diamond", "\\left|x\\right|+\\left|y\\right|=5"),
    (
        "dense waves",
        "\\sin\\left(xy\\right)=\\cos\\left(x\\right)",
    ),
    (
        "tangent grid",
        "\\tan\\left(x\\right)=\\tan\\left(y\\right)",
    ),
    ("shaded disc", "x^2+y^2<16"),
    ("shaded parabola", "y>x^2-4"),
    ("shaded band", "\\sin\\left(x\\right)>y"),
];

/// Times the implicit benchmark set: each graph on its own, then every one
/// at once, then a slider drag that moves all of them. Every measurement is
/// a whole frame, from re-reading the rows to the GPU finishing its work.
pub fn bench_implicit(width: u32, height: u32, frames: u32) -> Result<(), String> {
    let gpu = Gpu::new(&new_instance(), None)?;
    let mut frame = Frame::new(&gpu, width, height)?;
    let ms = |d: Duration| d.as_secs_f64() * 1000.0;
    // A software adapter rasterizes every pixel on the CPU, so its times
    // say nothing about a real graphics card; there only the app's own work
    // is judged, and the per-pixel times are printed for information.
    let software = gpu.adapter.get_info().device_type == wgpu::DeviceType::Cpu;
    println!(
        "implicit benchmark on {} at {width}x{height}, {frames} frames each{}",
        gpu.describe(),
        if software {
            "\nsoftware adapter: judging the app's own work, not the per-pixel shading"
        } else {
            ""
        }
    );

    let run = |name: &str, latex: &[String], frame: &mut Frame| -> Result<Duration, String> {
        let mut cpu = Vec::with_capacity(frames as usize);
        let mut total = Vec::with_capacity(frames as usize);
        for i in 0..frames {
            // Nudge the view every frame so nothing can be cached away.
            let nudge = 1.0 + 0.001 * (i % 2) as f64;
            let (c, t) = frame.draw(&gpu, &rows(latex), nudge)?;
            cpu.push(c);
            total.push(t);
        }
        cpu.sort();
        total.sort();
        let (c, t) = (cpu[cpu.len() / 2], total[total.len() / 2]);
        println!(
            "  {name:<16} {:>7.2} ms   (app {:>6.2} ms, drawing {:>7.2} ms)",
            ms(t),
            ms(c),
            ms(t - c)
        );
        Ok(if software { c } else { t })
    };

    // An empty graph first: everything below includes this much.
    run("empty graph", &[], &mut frame)?;
    let mut worst = Duration::ZERO;
    let mut worst_name = String::new();
    for (name, latex) in IMPLICIT_ROWS {
        let judged = run(name, &[latex.to_string()], &mut frame)?;
        if judged > worst {
            worst = judged;
            worst_name = name.to_string();
        }
    }
    let all: Vec<String> = IMPLICIT_ROWS.iter().map(|(_, l)| l.to_string()).collect();
    let together = run("all together", &all, &mut frame)?;

    // A slider every graph depends on, dragged across its range.
    let mut cpu = Vec::with_capacity(frames as usize);
    let mut total = Vec::with_capacity(frames as usize);
    let mut animated: Vec<String> = vec![String::new()];
    animated.extend(IMPLICIT_ROWS.iter().map(|(_, l)| format!("a{l}")));
    for i in 0..frames {
        animated[0] = format!("a={:.3}", 0.5 + 1.5 * i as f64 / frames.max(2) as f64);
        let (c, t) = frame.draw(&gpu, &rows(&animated), 1.0)?;
        cpu.push(c);
        total.push(t);
    }
    cpu.sort();
    total.sort();
    let (drag_cpu, drag_total) = (cpu[cpu.len() / 2], total[total.len() / 2]);
    println!(
        "  {:<16} {:>7.2} ms   (app {:>6.2} ms, drawing {:>7.2} ms)",
        "slider drag",
        ms(drag_total),
        ms(drag_cpu),
        ms(drag_total - drag_cpu)
    );
    let drag = if software { drag_cpu } else { drag_total };

    let slowest = worst.max(together).max(drag);
    let pass = slowest <= BUDGET;
    println!(
        "slowest single graph: {worst_name} at {:.2} ms; all {} at once {:.2} ms; \
         slider drag {:.2} ms; budget {:.2} ms\n{}",
        ms(worst),
        IMPLICIT_ROWS.len(),
        ms(together),
        ms(drag),
        ms(BUDGET),
        if pass {
            "BENCHMARK PASSED"
        } else {
            "BENCHMARK FAILED"
        }
    );
    if pass {
        Ok(())
    } else {
        Err(format!(
            "the implicit set misses 60 fps ({:.2} ms)",
            ms(slowest)
        ))
    }
}

/// An offscreen target that draws one frame of given rows, the way the
/// window does, and waits for the GPU to finish.
struct Frame {
    renderer: GraphRenderer,
    shader: ShadingRenderer,
    view: wgpu::TextureView,
    text: TextAtlas,
    engine: Engine,
    size: (u32, u32),
}

impl Frame {
    fn new(gpu: &Gpu, width: u32, height: u32) -> Result<Frame, String> {
        let format = wgpu::TextureFormat::Bgra8UnormSrgb;
        let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("bench target"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        Ok(Frame {
            renderer: GraphRenderer::new(gpu, format),
            shader: ShadingRenderer::new(gpu),
            view: texture.create_view(&Default::default()),
            text: TextAtlas::system(),
            engine: Engine::default(),
            size: (width, height),
        })
    }

    /// Analyses the rows, builds the scene and renders it. Returns the time
    /// the app itself took and the time the whole frame took, GPU included.
    fn draw(
        &mut self,
        gpu: &Gpu,
        rows: &serde_json::Value,
        zoom: f64,
    ) -> Result<(Duration, Duration), String> {
        let (width, height) = self.size;
        let mut vp = Viewport::new(width as f64, height as f64);
        vp.zoom_at(width as f64 / 2.0, height as f64 / 2.0, zoom);
        let start = Instant::now();
        self.engine.update(rows);
        let text = &mut self.text;
        let scene = self
            .engine
            .with_plot(|plot| build_scene(&vp, 1.0, plot, text));
        let shadings = self
            .engine
            .shadings(vp.to_world(0.0, 0.0), vp.units_per_px, 1.0);
        let cpu = start.elapsed();
        self.renderer.draw_scene(
            gpu,
            &self.view,
            self.size,
            Area {
                x: 0,
                y: 0,
                width,
                height,
            },
            &scene,
            &self.text,
            &shadings,
            &mut self.shader,
        );
        gpu.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|e| e.to_string())?;
        Ok((cpu, start.elapsed()))
    }
}

/// Simulates dragging the slider `a` across its range, one frame per step:
/// each frame re-analyzes every row (as a drag does), rebuilds the scene and
/// renders it on the GPU, waiting for the GPU to finish. Passes when 95% of
/// frames fit in the 60 fps budget.
pub fn bench_sliders(width: u32, height: u32, frames: u32) -> Result<(), String> {
    let gpu = Gpu::new(&new_instance(), None)?;
    let format = wgpu::TextureFormat::Bgra8UnormSrgb;
    let mut renderer = GraphRenderer::new(&gpu, format);
    let mut shader = ShadingRenderer::new(&gpu);
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("bench target"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());
    let area = Area {
        x: 0,
        y: 0,
        width,
        height,
    };
    let vp = Viewport::new(width as f64, height as f64);
    let mut text = TextAtlas::system();
    let mut engine = Engine::default();
    let mut latex: Vec<String> = BENCH_ROWS.iter().map(|s| s.to_string()).collect();

    let mut times = Vec::with_capacity(frames as usize);
    let (mut cpu, mut gpu_time) = (Duration::ZERO, Duration::ZERO);
    let mut cpu_times = Vec::with_capacity(frames as usize);
    let mut vertices_seen = 0;
    for i in 0..frames {
        let a = -10.0 + 20.0 * i as f64 / frames.max(2) as f64;
        latex[0] = format!("a={a:.2}");
        let start = Instant::now();
        engine.update(&rows(&latex));
        let scene = engine.with_plot(|plot| build_scene(&vp, 1.0, plot, &mut text));
        let built = start.elapsed();
        let shadings = engine.shadings(vp.to_world(0.0, 0.0), vp.units_per_px, 1.0);
        renderer.draw_scene(
            &gpu,
            &view,
            (width, height),
            area,
            &scene,
            &text,
            &shadings,
            &mut shader,
        );
        gpu.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|e| e.to_string())?;
        let total = start.elapsed();
        cpu += built;
        cpu_times.push(built);
        gpu_time += total - built;
        times.push(total);
        vertices_seen = vertices_seen.max(scene.vertices.len());
    }

    times.sort();
    cpu_times.sort();
    let ms = |d: Duration| d.as_secs_f64() * 1000.0;
    let avg = times.iter().sum::<Duration>() / times.len().max(1) as u32;
    let p95 = times[(times.len() * 95 / 100).min(times.len() - 1)];
    let max = *times.last().unwrap_or(&Duration::ZERO);
    // A software adapter (WARP on CI runners, llvmpipe) rasterizes on the
    // CPU, so its GPU time says nothing about a real graphics card. There the
    // check is on the app's own work: analysis and scene building.
    let software = gpu.adapter.get_info().device_type == wgpu::DeviceType::Cpu;
    let cpu_p95 = cpu_times[(cpu_times.len() * 95 / 100).min(cpu_times.len() - 1)];
    let judged = if software { cpu_p95 } else { p95 };
    let pass = judged <= BUDGET;
    println!(
        "slider benchmark on {}: {frames} frames at {width}x{height}, {} rows, up to {vertices_seen} vertices\n\
         average {:.2} ms (analyze and build {:.2} ms, GPU {:.2} ms), 95th percentile {:.2} ms, \
         worst {:.2} ms (budget {:.2} ms)\n{}{}",
        gpu.describe(),
        BENCH_ROWS.len(),
        ms(avg),
        ms(cpu / frames.max(1)),
        ms(gpu_time / frames.max(1)),
        ms(p95),
        ms(max),
        ms(BUDGET),
        if software {
            format!(
                "software adapter: judging analyze and build only (95th percentile {:.2} ms)\n",
                ms(cpu_p95)
            )
        } else {
            String::new()
        },
        if pass { "BENCHMARK PASSED" } else { "BENCHMARK FAILED" }
    );
    if pass {
        Ok(())
    } else {
        Err("slider drag misses 60 fps".into())
    }
}
