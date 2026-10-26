use crate::engine::Engine;
use graph_core::scene::DESMOS_COLORS;
use graph_core::{build_scene, TextAtlas, Viewport};
use graph_render::{new_instance, render_offscreen, Area, Gpu, GraphRenderer};
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
    let vertices = engine.with_curves(|curves| build_scene(&vp, 1.0, curves, &mut text));
    let build = start.elapsed();
    let pixels = render_offscreen(&gpu, width, height, &vertices, &text);

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
        vertices.len(),
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

/// Simulates dragging the slider `a` across its range, one frame per step:
/// each frame re-analyzes every row (as a drag does), rebuilds the scene and
/// renders it on the GPU, waiting for the GPU to finish. Passes when 95% of
/// frames fit in the 60 fps budget.
pub fn bench_sliders(width: u32, height: u32, frames: u32) -> Result<(), String> {
    let gpu = Gpu::new(&new_instance(), None)?;
    let format = wgpu::TextureFormat::Bgra8UnormSrgb;
    let mut renderer = GraphRenderer::new(&gpu, format);
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
        let vertices = engine.with_curves(|curves| build_scene(&vp, 1.0, curves, &mut text));
        let built = start.elapsed();
        renderer.draw(&gpu, &view, (width, height), area, &vertices, &text);
        gpu.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|e| e.to_string())?;
        let total = start.elapsed();
        cpu += built;
        cpu_times.push(built);
        gpu_time += total - built;
        times.push(total);
        vertices_seen = vertices_seen.max(vertices.len());
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
