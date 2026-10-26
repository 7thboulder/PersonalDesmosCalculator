use graph_core::scene::{DEFAULT_CURVE_WIDTH, DESMOS_COLORS};
use graph_core::{build_scene, Curve, Viewport};
use graph_render::{new_instance, render_offscreen, Gpu};
use std::time::Instant;

/// Draws the Phase 0 graph (y = sin x) offscreen and saves it as a PNG.
/// Proves the GPU path works on a machine without opening a window.
pub fn render_png(path: &str, width: u32, height: u32) -> Result<(), String> {
    let gpu = Gpu::new(&new_instance(), None)?;
    let vp = Viewport::new(width as f64, height as f64);
    let curve = Curve {
        f: &f64::sin,
        color: DESMOS_COLORS[0],
        width: DEFAULT_CURVE_WIDTH,
    };

    let start = Instant::now();
    let vertices = build_scene(&vp, 1.0, &[curve]);
    let build = start.elapsed();
    let pixels = render_offscreen(&gpu, width, height, &vertices);

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
