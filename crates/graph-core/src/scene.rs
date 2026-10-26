use crate::grid::{grid_spacing, line_indices};
use crate::implicit::{trace_implicit, Field};
use crate::sample::{sample_curve, Axis};
use crate::tess::{fill_rect, stroke_polyline, stroke_styled, LineStyle, Vertex};
use crate::text::TextAtlas;
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
const LABEL: [f32; 4] = rgb(0x1a, 0x1a, 0x1a);
/// Axis number size and halo radius, in logical pixels.
const LABEL_SIZE: f32 = 14.0;
const LABEL_HALO: f32 = 2.0;
/// Gap between an axis and its numbers, in logical pixels.
const LABEL_GAP: f32 = 4.0;
/// Height of a digit as a fraction of the font size (Arial's cap height).
const DIGIT_HEIGHT: f32 = 0.716;
const MINUS: char = '\u{2212}';

/// Logical-pixel distance aimed for between major grid lines.
const MAJOR_SPACING_LOGICAL: f64 = 100.0;
/// Desmos's default line thickness, in logical pixels.
pub const DEFAULT_CURVE_WIDTH: f32 = 2.5;

const fn rgb(r: u8, g: u8, b: u8) -> [f32; 4] {
    [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0]
}

/// A curve to draw: y = f(x), or x = f(y) when `axis` is `Axis::Y`.
pub struct Curve<'a> {
    pub f: &'a (dyn Fn(f64) -> f64 + Sync),
    pub axis: Axis,
    pub color: [f32; 4],
    /// Thickness in logical pixels.
    pub width: f32,
    pub style: LineStyle,
}

impl<'a> Curve<'a> {
    /// A solid y = f(x) curve of the default thickness.
    pub fn new(f: &'a (dyn Fn(f64) -> f64 + Sync), color: [f32; 4]) -> Self {
        Curve {
            f,
            axis: Axis::X,
            color,
            width: DEFAULT_CURVE_WIDTH,
            style: LineStyle::Solid,
        }
    }
}

/// An implicit curve f(x, y) = 0, including the boundary of a shaded
/// inequality. The shading itself is drawn by the renderer, not here.
pub struct Implicit<'a> {
    pub field: &'a dyn Field,
    pub color: [f32; 4],
    /// Thickness in logical pixels.
    pub width: f32,
    pub style: LineStyle,
}

/// One frame's triangles, in two parts: the renderer draws `vertices` up to
/// `shade_after` (paper, grid and axes), then any inequality shading, then
/// the rest (axis numbers and curves), so shading sits over the grid and
/// under everything that has to stay readable.
pub struct Scene {
    pub vertices: Vec<Vertex>,
    pub shade_after: usize,
}

/// Builds the triangles for one frame: paper, grid, axes, axis numbers, then
/// curves. `scale` is the display's DPI scale (1.0 at 100%, 1.5 at 150%).
pub fn build_scene(
    vp: &Viewport,
    scale: f32,
    curves: &[Curve],
    implicits: &[Implicit],
    text: &mut TextAtlas,
) -> Scene {
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

    let shade_after = out.len();
    labels(vp, scale, spacing.major, text, &mut out);

    // Sampling and tracing are the expensive parts, so every curve runs on
    // its own thread.
    let (sampled, traced) = std::thread::scope(|scope| {
        let curve_jobs: Vec<_> = curves
            .iter()
            .map(|c| scope.spawn(move || sample_curve(c.f, vp, c.axis)))
            .collect();
        let implicit_jobs: Vec<_> = implicits
            .iter()
            .map(|i| scope.spawn(move || trace_implicit(i.field, vp)))
            .collect();
        let finish = |jobs: Vec<std::thread::ScopedJoinHandle<Vec<Vec<[f32; 2]>>>>| {
            jobs.into_iter()
                .map(|j| j.join().unwrap_or_default())
                .collect::<Vec<_>>()
        };
        (finish(curve_jobs), finish(implicit_jobs))
    });
    let stroke = |lines: Vec<Vec<[f32; 2]>>, width: f32, style, color, out: &mut Vec<Vertex>| {
        for line in lines {
            stroke_styled(&line, width * scale, style, color, out);
        }
    };
    for (c, lines) in curves.iter().zip(sampled) {
        stroke(lines, c.width, c.style, c.color, &mut out);
    }
    for (i, lines) in implicits.iter().zip(traced) {
        stroke(lines, i.width, i.style, i.color, &mut out);
    }
    Scene {
        vertices: out,
        shade_after,
    }
}

/// Text of the axis number at `i` major steps, as Desmos writes it: plain
/// decimals with as many places as the step needs, and `m×10ⁿ` (returned as
/// the mantissa part and the exponent) when numbers get very large or small.
pub fn axis_label(i: i64, major: f64) -> (String, Option<String>) {
    let value = i as f64 * major;
    let step_exp = major.log10().floor() as i32;
    let (text, exponent) = if !(-4..6).contains(&step_exp) && value != 0.0 {
        let exp = value.abs().log10().floor() as i32;
        let mantissa = value / 10f64.powi(exp);
        let m = trim_zeros(format!("{mantissa:.3}"));
        (format!("{m}\u{d7}10"), Some(exp.to_string()))
    } else {
        let places = (-step_exp).max(0) as usize;
        (trim_zeros(format!("{value:.places$}")), None)
    };
    let minus = |s: String| s.replace('-', &MINUS.to_string());
    (minus(text), exponent.map(minus))
}

/// `1.50` becomes `1.5` and `2.0` becomes `2`.
fn trim_zeros(mut s: String) -> String {
    if s.contains('.') {
        while s.ends_with('0') {
            s.pop();
        }
        if s.ends_with('.') {
            s.pop();
        }
    }
    s
}

/// Width of a label, including its raised exponent.
fn label_width(text: &TextAtlas, label: &(String, Option<String>), size: f32) -> f32 {
    text.measure(&label.0, size)
        + label
            .1
            .as_ref()
            .map_or(0.0, |e| text.measure(e, size * 0.75))
}

fn draw_label(
    text: &mut TextAtlas,
    label: &(String, Option<String>),
    size: f32,
    halo: f32,
    x: f32,
    baseline: f32,
    out: &mut Vec<Vertex>,
) {
    let pen = text.draw(&label.0, size, halo, x, baseline, LABEL, out);
    if let Some(exp) = &label.1 {
        text.draw(
            exp,
            size * 0.75,
            halo,
            pen,
            baseline - size * 0.4,
            LABEL,
            out,
        );
    }
}

/// Numbers along both axes at every major grid line. When an axis is off
/// screen its numbers stay pinned to the nearest edge, as in Desmos.
fn labels(vp: &Viewport, scale: f32, major: f64, text: &mut TextAtlas, out: &mut Vec<Vertex>) {
    if !text.has_font() {
        return;
    }
    let size = (LABEL_SIZE * scale).round();
    let halo = LABEL_HALO * scale;
    let gap = LABEL_GAP * scale;
    let digit = size * DIGIT_HEIGHT;
    let (w, h) = (vp.width_px as f32, vp.height_px as f32);
    let [ox, oy] = vp.to_screen(0.0, 0.0);
    let (ox, oy) = (ox as f32, oy as f32);
    let (x0, x1) = vp.x_range();
    let (y0, y1) = vp.y_range();
    let major_px = (major / vp.units_per_px) as f32;
    let minus_w = text.measure(&MINUS.to_string(), size);

    // x axis: centred under each line, skipping every other one if crowded.
    let xs = line_indices(x0, x1, major);
    let widest = [*xs.start(), *xs.end()]
        .iter()
        .map(|&i| label_width(text, &axis_label(i, major), size))
        .fold(0.0, f32::max);
    let every = if widest + 2.0 * gap > major_px { 2 } else { 1 };
    let baseline = (oy + gap + digit).clamp(gap + digit, h - gap);
    for i in xs {
        if i == 0 || i % every != 0 {
            continue;
        }
        let label = axis_label(i, major);
        let width = label_width(text, &label, size);
        // Centre the digits, letting a minus sign hang to the left.
        let sign = if i < 0 { minus_w } else { 0.0 };
        let sx = vp.to_screen(i as f64 * major, 0.0)[0] as f32;
        draw_label(
            text,
            &label,
            size,
            halo,
            sx - (width - sign) / 2.0 - sign,
            baseline,
            out,
        );
    }

    // y axis: right-aligned left of the axis, centred on each line.
    for i in line_indices(y0, y1, major) {
        if i == 0 {
            continue;
        }
        let label = axis_label(i, major);
        let width = label_width(text, &label, size);
        let right = (ox - gap).min(w - gap);
        let left = (right - width).max(gap);
        let sy = vp.to_screen(0.0, i as f64 * major)[1] as f32;
        draw_label(text, &label, size, halo, left, sy + digit / 2.0, out);
    }

    // A single 0 sits below and left of the origin.
    if (0.0..=w).contains(&ox) && (0.0..=h).contains(&oy) {
        let zero = ("0".to_string(), None);
        let width = label_width(text, &zero, size);
        draw_label(text, &zero, size, halo, ox - gap - width, baseline, out);
    }
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
        let curve = Curve::new(&f64::sin, DESMOS_COLORS[0]);
        let verts = build_scene(&vp, 1.0, &[curve], &[], &mut TextAtlas::empty()).vertices;
        assert!(verts.len() > 1000);
        assert!(verts.iter().any(|v| v.color == DESMOS_COLORS[0]));
        assert!(verts.iter().any(|v| v.color == AXIS));
        assert!(verts.iter().any(|v| v.color == MINOR_GRID));
    }

    #[test]
    fn an_implicit_circle_reaches_the_scene() {
        let vp = Viewport::new(600.0, 600.0);
        let field = |x: f64, y: f64| x * x + y * y - 9.0;
        let implicit = Implicit {
            field: &field,
            color: DESMOS_COLORS[1],
            width: DEFAULT_CURVE_WIDTH,
            style: LineStyle::Solid,
        };
        let scene = build_scene(&vp, 1.0, &[], &[implicit], &mut TextAtlas::empty());
        let verts = scene.vertices;
        assert!(verts.iter().any(|v| v.color == DESMOS_COLORS[1]));
    }

    #[test]
    fn labels_follow_desmos_formatting() {
        let t = |i, major| axis_label(i, major);
        assert_eq!(t(2, 2.0), ("4".into(), None));
        assert_eq!(t(-3, 2.0), ("\u{2212}6".into(), None));
        assert_eq!(t(3, 0.5), ("1.5".into(), None));
        assert_eq!(t(4, 0.5), ("2".into(), None));
        assert_eq!(t(1, 0.05), ("0.05".into(), None));
        assert_eq!(t(3, 1e6), ("3\u{d7}10".into(), Some("6".into())));
        assert_eq!(t(15, 1e6), ("1.5\u{d7}10".into(), Some("7".into())));
        assert_eq!(t(2, 1e-5), ("2\u{d7}10".into(), Some("\u{2212}5".into())));
        assert_eq!(t(1, 1000.0), ("1000".into(), None));
    }

    #[test]
    fn scene_draws_labels_when_a_font_exists() {
        let mut text = TextAtlas::system();
        if !text.has_font() {
            return;
        }
        let vp = Viewport::new(1000.0, 600.0);
        let verts = build_scene(&vp, 1.0, &[], &[], &mut text).vertices;
        assert!(verts.iter().any(|v| v.color == LABEL));
    }
}
