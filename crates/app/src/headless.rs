use crate::engine::Engine;
use crate::interact::Interaction;
use graph_core::scene::DESMOS_COLORS;
use graph_core::{build_scene, build_scene_cached, LineCache, TextAtlas, View3, Viewport};
use graph_expr::Mode;
use graph_render::{
    new_instance, render_offscreen, render_offscreen_3d, Area, Gpu, GraphRenderer, ShadingRenderer,
};
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
///
/// `--select=N` among the rows selects row N (from 0), so its points of
/// interest show, and `--hover=X,Y` puts the mouse at that world point.
fn write_png(path: &str, width: u32, height: u32, pixels: &[u8]) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|e| format!("{path}: {e}"))?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
    writer.write_image_data(pixels).map_err(|e| e.to_string())
}

pub fn render_png(path: &str, width: u32, height: u32, latex: &[String]) -> Result<(), String> {
    let gpu = Gpu::new(&new_instance(), None)?;
    let vp = Viewport::new(width as f64, height as f64);
    let flag = |name: &str| {
        latex
            .iter()
            .find_map(|l| l.strip_prefix(name).map(str::to_owned))
    };
    let select = flag("--select=").and_then(|n| n.parse::<u64>().ok());
    let hover = flag("--hover=").and_then(|xy| {
        let (x, y) = xy.split_once(',')?;
        Some([x.parse::<f64>().ok()?, y.parse::<f64>().ok()?])
    });
    let latex: Vec<String> = latex
        .iter()
        .filter(|l| !l.starts_with("--"))
        .cloned()
        .collect();
    let latex = &latex[..];
    let default = ["y=\\sin\\left(x\\right)".to_owned()];
    let latex = if latex.is_empty() {
        &default[..]
    } else {
        latex
    };
    let three_d = flag("--3d").is_some();
    let mut engine = Engine::default();
    if three_d {
        engine.mode = Mode::ThreeD;
    }
    let results = engine.update(&rows(latex));
    for (l, r) in latex.iter().zip(results.as_array().into_iter().flatten()) {
        if let Some(e) = r["error"].as_str() {
            println!("{l}: {e}");
        }
    }
    let mut text = TextAtlas::system();
    if three_d {
        let view = View3::new(width as f64, height as f64);
        let start = Instant::now();
        let mut build = Duration::ZERO;
        let pixels = engine.with_frame_3d(&view, 1.0, &mut text, |frame, overlay, text| {
            build = start.elapsed();
            render_offscreen_3d(&gpu, width, height, frame, overlay, text)
        });
        write_png(path, width, height, &pixels)?;
        println!(
            "wrote {path} ({width}x{height}) on {}: surfaces built in {:.2} ms",
            gpu.describe(),
            build.as_secs_f64() * 1000.0
        );
        return Ok(());
    }

    let start = Instant::now();
    engine.selected = select;
    let mut interact = Interaction::default();
    if let Some([x, y]) = hover {
        interact.moved(&engine, &vp, vp.to_screen(x, y));
    }
    let marks = interact.marks(&engine, &vp);
    let scene = engine.with_plot_marks(&marks, |plot| build_scene(&vp, 1.0, plot, &mut text));
    let build = start.elapsed();
    let shadings = engine.shadings(vp.to_world(0.0, 0.0), vp.units_per_px, 1.0);
    let pixels = render_offscreen(&gpu, width, height, &scene, &text, &shadings);

    write_png(path, width, height, &pixels)?;

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
    if software {
        // The software rasterizer shares the CPU with the app, so analysis
        // and building are timed again on their own, with no frames being
        // drawn alongside them. Shared CI runners stall now and then, so each
        // frame is timed in three fresh passes and keeps its fastest time: a
        // frame that is really slow is slow every time.
        let mut best = vec![Duration::MAX; frames as usize];
        for _ in 0..3 {
            let mut engine = Engine::default();
            for (i, best) in best.iter_mut().enumerate() {
                let a = -10.0 + 20.0 * i as f64 / frames.max(2) as f64;
                latex[0] = format!("a={a:.2}");
                let start = Instant::now();
                engine.update(&rows(&latex));
                let scene = engine.with_plot(|plot| build_scene(&vp, 1.0, plot, &mut text));
                *best = (*best).min(start.elapsed());
                std::hint::black_box(scene);
            }
        }
        cpu_times = best;
        cpu_times.sort();
    }
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

/// Opens a graph (a saved graph or a Desmos graph state), then runs its
/// ticker `ticks` times as the panel would: each run's updates rewrite their
/// rows, and the next run sees the new values. Times the app's own work per
/// tick (analysis and scene building) against the ticker's step.
pub fn bench_ticker(path: &str, ticks: u32) -> Result<(), String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    let (rows, view, _) = crate::files::open_any(&text)?;
    let mut rows = rows.as_array().cloned().unwrap_or_default();
    for (i, r) in rows.iter_mut().enumerate() {
        r["id"] = json!(i + 1);
    }
    let ticker = rows
        .iter()
        .position(|r| r.get("ticker").is_some())
        .ok_or("this graph has no ticker")?;
    let step: f64 = rows[ticker]["ticker"]["step"]
        .as_str()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.0);
    let mut vp = Viewport::new(1600.0, 1000.0);
    if let Some(view) = view {
        view.apply(&mut vp);
    }
    let mut text = TextAtlas::system();
    let mut engine = Engine::default();
    let mut times = Vec::with_capacity(ticks as usize);
    let mut analysis = Duration::ZERO;
    for _ in 0..ticks {
        let start = Instant::now();
        let reply = engine.update(&serde_json::Value::Array(rows.clone()));
        analysis += start.elapsed();
        engine.with_plot(|plot| build_scene(&vp, 1.0, plot, &mut text));
        times.push(start.elapsed());
        if let Some(e) = reply[ticker]["error"].as_str() {
            return Err(format!("the ticker failed: {e}"));
        }
        for u in reply[ticker]["updates"].as_array().into_iter().flatten() {
            let Some(r) = rows.iter_mut().find(|r| r["id"] == u["row"]) else {
                continue;
            };
            let latex = r["latex"].as_str().unwrap_or("");
            if let Some(eq) = latex.find('=') {
                r["latex"] = json!(format!(
                    "{}={}",
                    &latex[..eq],
                    u["latex"].as_str().unwrap_or("")
                ));
            }
        }
    }
    let ms = |d: Duration| d.as_secs_f64() * 1000.0;
    let first = times.first().copied().unwrap_or_default();
    let mut rest: Vec<Duration> = times.iter().skip(1).copied().collect();
    rest.sort();
    let avg = rest.iter().sum::<Duration>() / rest.len().max(1) as u32;
    let max = rest.last().copied().unwrap_or_default();
    println!(
        "{ticks} ticks: first {:.1} ms, then average {:.1} ms, worst {:.1} ms (step {step} ms); \
         analysis {:.1} ms a tick on average",
        ms(first),
        ms(avg),
        ms(max),
        ms(analysis) / f64::from(ticks.max(1)),
    );
    Ok(())
}

/// Graphs for comparing with Desmos, kept as Desmos states so the same file
/// opens in both. Each has a slider `a` (id "a") from 0 to 3.
pub const SUITE: &[(&str, &str)] = &[
    ("curves", include_str!("../../../bench/curves.json")),
    ("sums", include_str!("../../../bench/sums.json")),
    ("implicit", include_str!("../../../bench/implicit.json")),
    ("shading", include_str!("../../../bench/shading.json")),
    ("points", include_str!("../../../bench/points.json")),
    ("parametric", include_str!("../../../bench/parametric.json")),
    ("polar", include_str!("../../../bench/polar.json")),
];

/// Times each suite graph as bench/desmos-bench.js times it in Desmos:
/// opening it, then `frames` frames each of moving slider `a`, panning and
/// zooming. A frame is the app's own work (analysis and scene building)
/// plus drawing on the GPU and waiting for it.
pub fn bench_suite(frames: u32) -> Result<(), String> {
    // GRAPHCALC_SIZE=260x540 matches the graph size Desmos printed.
    let size = std::env::var("GRAPHCALC_SIZE").unwrap_or_default();
    let (width, height) = size
        .split_once('x')
        .and_then(|(w, h)| Some((w.trim().parse().ok()?, h.trim().parse().ok()?)))
        .filter(|&(w, h): &(u32, u32)| w > 0 && h > 0)
        .unwrap_or((1600, 1000));
    let gpu = Gpu::new(&new_instance(), None)?;
    let format = wgpu::TextureFormat::Bgra8UnormSrgb;
    let mut renderer = GraphRenderer::new(&gpu, format);
    let mut shader = ShadingRenderer::new(&gpu);
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("suite target"),
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
    let target = texture.create_view(&Default::default());
    let area = Area {
        x: 0,
        y: 0,
        width,
        height,
    };
    let mut text = TextAtlas::system();
    let ms = |d: Duration| d.as_secs_f64() * 1000.0;
    let software = gpu.adapter.get_info().device_type == wgpu::DeviceType::Cpu;
    println!(
        "suite on {} at {width}x{height}, {frames} frames each (ms; cpu = analysis and scene building)",
        gpu.describe()
    );
    println!(
        "{:<11} {:>8} {:>13} {:>13} {:>13}",
        "graph", "open", "slider", "pan", "zoom"
    );
    // GRAPHCALC_SUITE=curves,points runs only those graphs.
    let only = std::env::var("GRAPHCALC_SUITE").unwrap_or_default();
    let mut like_desmos = Vec::new();
    for (name, state) in SUITE {
        if !only.is_empty() && !only.split(',').any(|o| o.trim() == *name) {
            continue;
        }
        let (rows, view, _) = crate::files::open_any(state)?;
        let mut rows = rows.as_array().cloned().unwrap_or_default();
        for (i, r) in rows.iter_mut().enumerate() {
            r["id"] = json!(i + 1);
        }
        let slider = rows
            .iter()
            .position(|r| r["latex"].as_str().is_some_and(|l| l.starts_with("a=")))
            .ok_or(format!("{name} has no slider a"))?;
        let fresh_view = || {
            let mut vp = Viewport::new(width as f64, height as f64);
            if let Some(v) = &view {
                v.apply(&mut vp);
            }
            vp
        };

        // Opening: a fresh engine analyzes every row and builds the scene.
        let mut open = Duration::MAX;
        for _ in 0..3 {
            let vp = fresh_view();
            let start = Instant::now();
            let mut engine = Engine::default();
            engine.update(&serde_json::Value::Array(rows.clone()));
            std::hint::black_box(engine.with_plot(|plot| build_scene(&vp, 1.0, plot, &mut text)));
            open = open.min(start.elapsed());
        }

        let mut engine = Engine::default();
        engine.update(&serde_json::Value::Array(rows.clone()));
        // As in the app, lines carry over from frame to frame.
        let mut lines = LineCache::default();
        let mut frame =
            |engine: &mut Engine, vp: &Viewport, update: Option<&[serde_json::Value]>| {
                let start = Instant::now();
                if let Some(rows) = update {
                    engine.update(&serde_json::Value::Array(rows.to_vec()));
                }
                let scene = engine
                    .with_plot(|plot| build_scene_cached(vp, 1.0, plot, &mut text, &mut lines));
                let shadings = engine.shadings(vp.to_world(0.0, 0.0), vp.units_per_px, 1.0);
                let cpu = start.elapsed();
                renderer.draw_scene(
                    &gpu,
                    &target,
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
                Ok::<_, String>((cpu, start.elapsed()))
            };
        let mut run = |engine: &mut Engine,
                       each: &mut dyn FnMut(
            u32,
            &mut Viewport,
            &mut Vec<serde_json::Value>,
        ) -> bool| {
            let mut vp = fresh_view();
            let mut rows = rows.clone();
            let (mut cpu, mut total) = (Duration::ZERO, Duration::ZERO);
            for i in 0..frames {
                let changed = each(i, &mut vp, &mut rows);
                let (c, t) = frame(engine, &vp, changed.then_some(&rows[..]))?;
                cpu += c;
                total += t;
            }
            let n = frames.max(1);
            Ok::<_, String>((cpu / n, total / n))
        };
        // An unchanged frame, which bench/desmos-bench.js takes off every
        // Desmos time (as the cost of a screenshot of an unchanged graph).
        let still_t = run(&mut engine, &mut |_, _, _| false)?;
        let slider_t = run(&mut engine, &mut |i, _, rows| {
            let a = 0.5 + 2.0 * f64::from(i) / f64::from(frames.max(1));
            rows[slider]["latex"] = json!(format!("a={a:.4}"));
            true
        })?;
        engine.update(&serde_json::Value::Array(rows.clone()));
        let pan_t = run(&mut engine, &mut |_, vp, _| {
            vp.pan_px(-4.0, 0.0);
            false
        })?;
        let zoom_t = run(&mut engine, &mut |_, vp, _| {
            vp.zoom_at(width as f64 / 2.0, height as f64 / 2.0, 1.0 / 1.01);
            false
        })?;
        let cell = |(cpu, total): (Duration, Duration)| {
            if software {
                format!("{:.1} cpu", ms(cpu))
            } else {
                format!("{:.1} ({:.1})", ms(total), ms(cpu))
            }
        };
        println!(
            "{:<11} {:>8.1} {:>13} {:>13} {:>13}",
            name,
            ms(open),
            cell(slider_t),
            cell(pan_t),
            cell(zoom_t)
        );
        let less = |(_, total): (Duration, Duration)| ms(total.saturating_sub(still_t.1));
        like_desmos.push(format!(
            "{:<11} {:>8.1} {:>13.1} {:>13.1} {:>13.1}   (unchanged frame {:.1})",
            name,
            ms(open),
            less(slider_t),
            less(pan_t),
            less(zoom_t),
            ms(still_t.1)
        ));
    }
    if !software {
        println!("\nless an unchanged frame, as bench/desmos-bench.js measures Desmos:");
        for line in &like_desmos {
            println!("{line}");
        }
    }
    if software {
        println!("software adapter: only the app's own (cpu) time is shown");
    } else {
        println!("frame times are total (cpu in brackets)");
    }
    Ok(())
}

/// The 3D graphs for comparing with Desmos 3D, kept as its states. Each
/// has a slider `a` (id "a") from 0 to 3.
pub const SUITE_3D: &[(&str, &str)] = &[
    ("surface", include_str!("../../../bench/3d/surface.json")),
    ("implicit", include_str!("../../../bench/3d/implicit.json")),
    (
        "uvsurface",
        include_str!("../../../bench/3d/uvsurface.json"),
    ),
    ("solids", include_str!("../../../bench/3d/solids.json")),
    ("curves3d", include_str!("../../../bench/3d/curves3d.json")),
];

/// Times each 3D suite graph as bench/desmos-3d-bench.js times it in
/// Desmos 3D: opening it, then `frames` frames each of moving slider `a`,
/// turning the box and zooming. Moving the slider rebuilds every mesh;
/// turning and zooming in only reuses them.
pub fn bench_suite_3d(frames: u32) -> Result<(), String> {
    let (width, height) = (1600u32, 1000u32);
    let gpu = Gpu::new(&new_instance(), None)?;
    let format = wgpu::TextureFormat::Bgra8UnormSrgb;
    let mut renderer = GraphRenderer::new(&gpu, format);
    let mut three = renderer.renderer_3d(&gpu);
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("3d suite target"),
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
    let target = texture.create_view(&Default::default());
    let area = Area {
        x: 0,
        y: 0,
        width,
        height,
    };
    let mut text = TextAtlas::system();
    let ms = |d: Duration| d.as_secs_f64() * 1000.0;
    let software = gpu.adapter.get_info().device_type == wgpu::DeviceType::Cpu;
    println!(
        "3d suite on {} at {width}x{height}, {frames} frames each (ms; cpu = analysis and meshes)",
        gpu.describe()
    );
    println!(
        "{:<11} {:>8} {:>13} {:>13} {:>13}",
        "graph", "open", "slider", "turn", "zoom"
    );
    let only = std::env::var("GRAPHCALC_SUITE").unwrap_or_default();
    for (name, state) in SUITE_3D {
        if !only.is_empty() && !only.split(',').any(|o| o.trim() == *name) {
            continue;
        }
        let (rows, _, _) = crate::files::open_any(state)?;
        let mut rows = rows.as_array().cloned().unwrap_or_default();
        for (i, r) in rows.iter_mut().enumerate() {
            r["id"] = json!(i + 1);
        }
        let slider = rows
            .iter()
            .position(|r| r["latex"].as_str().is_some_and(|l| l.starts_with("a=")))
            .ok_or(format!("{name} has no slider a"))?;
        let fresh_view = || View3::new(width as f64, height as f64);
        let new_engine = || {
            let mut engine = Engine::default();
            engine.mode = Mode::ThreeD;
            engine
        };
        let mut frame =
            |engine: &mut Engine, view: &View3, update: Option<&[serde_json::Value]>| {
                let start = Instant::now();
                if let Some(rows) = update {
                    engine.update(&serde_json::Value::Array(rows.to_vec()));
                }
                let mut cpu = Duration::ZERO;
                engine.with_frame_3d(view, 1.0, &mut text, |frame, overlay, text| {
                    cpu = start.elapsed();
                    renderer.draw_3d(
                        &gpu,
                        &target,
                        (width, height),
                        area,
                        frame,
                        overlay,
                        text,
                        &mut three,
                    );
                });
                gpu.device
                    .poll(wgpu::PollType::wait_indefinitely())
                    .map_err(|e| e.to_string())?;
                Ok::<_, String>((cpu, start.elapsed()))
            };
        // Opening: a fresh engine analyzes every row and builds every mesh.
        let mut open = Duration::MAX;
        for _ in 0..3 {
            let mut engine = new_engine();
            let rows = rows.clone();
            open = open.min(frame(&mut engine, &fresh_view(), Some(&rows))?.1);
        }
        let mut engine = new_engine();
        engine.update(&serde_json::Value::Array(rows.clone()));
        let mut run = |engine: &mut Engine,
                       each: &mut dyn FnMut(
            u32,
            &mut View3,
            &mut Vec<serde_json::Value>,
        ) -> bool| {
            let mut view = fresh_view();
            let mut rows = rows.clone();
            let (mut cpu, mut total) = (Duration::ZERO, Duration::ZERO);
            for i in 0..frames {
                let changed = each(i, &mut view, &mut rows);
                let (c, t) = frame(engine, &view, changed.then_some(&rows[..]))?;
                cpu += c;
                total += t;
            }
            let n = frames.max(1);
            Ok::<_, String>((cpu / n, total / n))
        };
        let slider_t = run(&mut engine, &mut |i, _, rows| {
            let a = 0.5 + 2.0 * f64::from(i) / f64::from(frames.max(1));
            rows[slider]["latex"] = json!(format!("a={a:.4}"));
            true
        })?;
        engine.update(&serde_json::Value::Array(rows.clone()));
        let turn_t = run(&mut engine, &mut |_, view, _| {
            view.turn_px(4.0, 0.0);
            false
        })?;
        let zoom_t = run(&mut engine, &mut |_, view, _| {
            view.zoom(1.0 / 1.01);
            false
        })?;
        let cell = |(cpu, total): (Duration, Duration)| {
            if software {
                format!("{:.1} cpu", ms(cpu))
            } else {
                format!("{:.1} ({:.1})", ms(total), ms(cpu))
            }
        };
        println!(
            "{:<11} {:>8.1} {:>13} {:>13} {:>13}",
            name,
            ms(open),
            cell(slider_t),
            cell(turn_t),
            cell(zoom_t)
        );
    }
    if software {
        println!("software adapter: only the app's own (cpu) time is shown");
    } else {
        println!("frame times are total (cpu in brackets)");
    }
    Ok(())
}

/// The console script that times the 3D suite graphs in Desmos 3D, for
/// [`bench_suite_3d`]. Kept in the repository as bench/desmos-3d-bench.js.
pub fn desmos_script_3d() -> String {
    let graphs: Vec<String> = SUITE_3D
        .iter()
        .map(|(name, state)| {
            let compact: serde_json::Value = serde_json::from_str(state).expect("suite JSON");
            format!("    {name}: {compact},")
        })
        .collect();
    format!(
        r#"// Times the graphcalc 3D comparison graphs in Desmos 3D. Open
// https://www.desmos.com/3d in Chrome, press F12, paste all of this into the
// Console and press Enter. It replaces the graph on screen, takes about a
// minute, and prints a line to compare with `graphcalc --bench-suite-3d`.
//
// Each step waits until Desmos has finished computing and drawing (through
// asyncScreenshot when it has one, else two animation frames), less what
// waiting on an unchanged graph costs. Desmos 3D has no API for turning
// the box, so only opening and the slider are timed.
// Generated by graphcalc (headless.rs, desmos_script_3d); do not edit by hand.
(async () => {{
  const FRAMES = 60;
  const GRAPHS = {{
{}
  }};
  const frames = () => new Promise((done) =>
    requestAnimationFrame(() => requestAnimationFrame(() => done())));
  const settle = () => typeof Calc.asyncScreenshot === 'function'
    ? new Promise((done) => Calc.asyncScreenshot({{ targetPixelRatio: 1 }}, () => done()))
    : frames();
  const results = [];
  for (const [graph, state] of Object.entries(GRAPHS)) {{
    let t = performance.now();
    Calc.setState(state);
    await settle();
    const open = performance.now() - t;
    t = performance.now();
    for (let i = 0; i < 10; i++) await settle();
    const base = (performance.now() - t) / 10;
    t = performance.now();
    for (let i = 0; i < FRAMES; i++) {{
      Calc.setExpression({{ id: 'a', latex: 'a=' + (0.5 + (2 * i) / FRAMES).toFixed(4) }});
      await settle();
    }}
    const slider = Math.max(0, (performance.now() - t) / FRAMES - base);
    results.push({{ graph, open: +open.toFixed(1), slider: +slider.toFixed(1) }});
  }}
  console.table(results);
  console.log('Desmos 3D times in ms. Copy this line into the chat:\n' + JSON.stringify(results));
}})();
"#,
        graphs.join("\n")
    )
}

/// The console script that times the suite graphs in Desmos, the same way
/// [`bench_suite`] times them here. Kept in the repository as
/// bench/desmos-bench.js; a test checks the two agree.
pub fn desmos_script() -> String {
    let graphs: Vec<String> = SUITE
        .iter()
        .map(|(name, state)| {
            let compact: serde_json::Value = serde_json::from_str(state).expect("suite JSON");
            format!("    {name}: {compact},")
        })
        .collect();
    format!(
        r#"// Times the graphcalc comparison graphs in Desmos. Open
// https://www.desmos.com/calculator in Chrome, press F12, paste all of this
// into the Console and press Enter. It replaces the graph on screen, takes
// about a minute, and prints a table to compare with `graphcalc --bench-suite`.
//
// Each step waits until Desmos has finished computing and drawing (through
// asyncScreenshot), less what a screenshot of an unchanged graph costs.
// Generated by graphcalc (headless.rs, desmos_script); do not edit by hand.
(async () => {{
  const FRAMES = 60;
  const GRAPHS = {{
{}
  }};
  const size = () => Calc.graphpaperBounds.pixelCoordinates;
  const settle = () => new Promise((done) => {{
    const px = size();
    Calc.asyncScreenshot({{ width: px.width, height: px.height, targetPixelRatio: 1 }}, () => done());
  }});
  const results = [];
  for (const [graph, state] of Object.entries(GRAPHS)) {{
    let t = performance.now();
    Calc.setState(state);
    await settle();
    const open = performance.now() - t;
    t = performance.now();
    for (let i = 0; i < 10; i++) await settle();
    const base = (performance.now() - t) / 10;
    const steps = async (change) => {{
      const t0 = performance.now();
      for (let i = 0; i < FRAMES; i++) {{
        change(i);
        await settle();
      }}
      return Math.max(0, (performance.now() - t0) / FRAMES - base);
    }};
    const b = Calc.graphpaperBounds.mathCoordinates;
    const bounds = (dx, k) => {{
      const cx = (b.left + b.right) / 2 + dx, cy = (b.bottom + b.top) / 2;
      const hw = (b.right - b.left) / 2 / k, hh = (b.top - b.bottom) / 2 / k;
      Calc.setMathBounds({{ left: cx - hw, right: cx + hw, bottom: cy - hh, top: cy + hh }});
    }};
    const slider = await steps((i) =>
      Calc.setExpression({{ id: 'a', latex: 'a=' + (0.5 + (2 * i) / FRAMES).toFixed(4) }}));
    Calc.setExpression({{ id: 'a', latex: 'a=1' }});
    const pan = await steps((i) => bounds((b.right - b.left) * 0.0025 * (i + 1), 1));
    const zoom = await steps((i) => bounds(0, Math.pow(1.01, i + 1)));
    const px = size();
    results.push({{
      graph,
      open: +open.toFixed(1),
      slider: +slider.toFixed(1),
      pan: +pan.toFixed(1),
      zoom: +zoom.toFixed(1),
      size: px.width + 'x' + px.height,
    }});
  }}
  console.table(results);
  console.log('Desmos times in ms. Copy this line into the chat:\n' + JSON.stringify(results));
}})();
"#,
        graphs.join("\n")
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn suite_3d_graphs_open_in_3d_and_the_desmos_script_is_current() {
        for (name, state) in super::SUITE_3D {
            assert!(crate::files::space_of(state).three_d, "{name}");
            let (rows, _, _) = crate::files::open_any(state).unwrap();
            let mut engine = crate::engine::Engine::default();
            engine.mode = graph_expr::Mode::ThreeD;
            let reply = engine.update(&rows);
            for r in reply.as_array().unwrap() {
                assert!(r["error"].is_null(), "{name}: {r}");
            }
        }
        let script = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../bench/desmos-3d-bench.js"
        ))
        .unwrap_or_default()
        .replace("\r\n", "\n");
        assert!(
            script == super::desmos_script_3d().replace("\r\n", "\n"),
            "bench/desmos-3d-bench.js is out of date: run `graphcalc --desmos-script-3d bench/desmos-3d-bench.js`"
        );
    }

    #[test]
    fn suite_graphs_open_and_the_desmos_script_is_current() {
        for (name, state) in super::SUITE {
            let (rows, _, imported) = crate::files::open_any(state).unwrap();
            assert!(imported, "{name}");
            let reply = crate::engine::Engine::default().update(&rows);
            for r in reply.as_array().unwrap() {
                assert!(r["error"].is_null(), "{name}: {r}");
            }
        }
        let script = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../bench/desmos-bench.js"
        ))
        .unwrap_or_default()
        // A Windows checkout may turn line endings into CRLF.
        .replace("\r\n", "\n");
        assert!(
            script == super::desmos_script().replace("\r\n", "\n"),
            "bench/desmos-bench.js is out of date: run `graphcalc --desmos-script bench/desmos-bench.js`"
        );
    }
}
