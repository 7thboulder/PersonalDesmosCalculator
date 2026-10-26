use crate::grid::{grid_spacing, line_indices};
use crate::sample::sample_explicit;
use crate::tess::{fill_rect, stroke_polyline, Vertex};
use crate::Viewport;

/// Desmos's default curve colors, in the order new expressions receive them.
pub const DESMOS_COLORS: [[f32; 4]; 6] = [
    rgb(0xc7, 0x44, 0x40), // red
    rgb(0x2d, 0x70, 0xb3), // blue
    rgb(0x38, 0x8c, 0x46), // green
    rgb(0x60, 0x42, 0xa6), // purple
    rgb(0xfa, 0x7e, 0x19), // orange
    rgb(0x00, 0x00, 0x00), // black
];

const BACKGROUND: [f32; 4] = rgb(0xff, 0xff, 0xff);
const MINOR_GRID: [f32; 4] = rgb(0xee, 0xee, 0xee);
const MAJOR_GRID: [f32; 4] = rgb(0xcc, 0xcc, 0xcc);
const AXIS: [f32; 4] = rgb(0x2a, 0x2a, 0x2a);

/// Logical-pixel distance aimed for between major grid lines.
const MAJOR_SPACING_LOGICAL: f64 = 100.0;
/// Desmos's default line thickness, in logical pixels.
pub const DEFAULT_CURVE_WIDTH: f32 = 2.5;

const fn rgb(r: u8, g: u8, b: u8) -> [f32; 4] {
    [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0]
}

/// An explicit curve y = f(x) to draw.
pub struct Curve<'a> {
    pub f: &'a dyn Fn(f64) -> f64,
    pub color: [f32; 4],
    /// Thickness in logical pixels.
    pub width: f32,
}

/// Builds the triangles for one frame: paper, grid, axes, then curves.
/// `scale` is the display's DPI scale (1.0 at 100%, 1.5 at 150%).
pub fn build_scene(vp: &Viewport, scale: f32, curves: &[Curve]) -> Vec<Vertex> {
    let mut out = Vec::with_capacity(64 * 1024);
    let (w, h) = (vp.width_px as f32, vp.height_px as f32);
    fill_rect(0.0, 0.0, w, h, BACKGROUND, &mut out);

    let spacing = grid_spacing(vp.units_per_px, MAJOR_SPACING_LOGICAL * scale as f64);
    let (x0, x1) = vp.x_range();
    let (y0, y1) = vp.y_range();
    let ratio = (spacing.major / spacing.minor).round() as i64;
    let thin = scale.max(1.0).round();

    // Minor lines first so major lines and axes draw over them.
    for pass in [Pass::Minor, Pass::Major] {
        let (step, color) = match pass {
            Pass::Minor => (spacing.minor, MINOR_GRID),
            Pass::Major => (spacing.major, MAJOR_GRID),
        };
        for i in line_indices(x0, x1, step) {
            if skip(pass, i, ratio) {
                continue;
            }
            let sx = snap(vp.to_screen(i as f64 * step, 0.0)[0] as f32, thin);
            stroke_polyline(&[[sx, 0.0], [sx, h]], thin, color, &mut out);
        }
        for i in line_indices(y0, y1, step) {
            if skip(pass, i, ratio) {
                continue;
            }
            let sy = snap(vp.to_screen(0.0, i as f64 * step)[1] as f32, thin);
            stroke_polyline(&[[0.0, sy], [w, sy]], thin, color, &mut out);
        }
    }

    let axis_w = (1.5 * scale).round().max(1.0);
    let [ox, oy] = vp.to_screen(0.0, 0.0);
    let (ox, oy) = (snap(ox as f32, axis_w), snap(oy as f32, axis_w));
    if (0.0..=w).contains(&ox) {
        stroke_polyline(&[[ox, 0.0], [ox, h]], axis_w, AXIS, &mut out);
    }
    if (0.0..=h).contains(&oy) {
        stroke_polyline(&[[0.0, oy], [w, oy]], axis_w, AXIS, &mut out);
    }

    for curve in curves {
        for line in sample_explicit(curve.f, vp) {
            stroke_polyline(&line, curve.width * scale, curve.color, &mut out);
        }
    }
    out
}

#[derive(Clone, Copy, PartialEq)]
enum Pass {
    Minor,
    Major,
}

/// Minor lines under a major line, and major lines on an axis, are skipped.
fn skip(pass: Pass, i: i64, ratio: i64) -> bool {
    match pass {
        Pass::Minor => i % ratio == 0,
        Pass::Major => i == 0,
    }
}

/// Centres a line of `width` pixels on the pixel grid so 1 px lines stay crisp.
fn snap(v: f32, width: f32) -> f32 {
    (v - width / 2.0).round() + width / 2.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scene_has_paper_grid_axes_and_curve() {
        let vp = Viewport::new(1000.0, 600.0);
        let curve = Curve {
            f: &f64::sin,
            color: DESMOS_COLORS[0],
            width: DEFAULT_CURVE_WIDTH,
        };
        let verts = build_scene(&vp, 1.0, &[curve]);
        assert!(verts.len() > 1000);
        assert!(verts.iter().any(|v| v.color == DESMOS_COLORS[0]));
        assert!(verts.iter().any(|v| v.color == AXIS));
        assert!(verts.iter().any(|v| v.color == MINOR_GRID));
    }
}
