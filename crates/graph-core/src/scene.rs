use crate::grid::{grid_spacing, line_indices};
use crate::implicit::{trace_implicit, Field};
use crate::sample::{sample_curve, sample_parametric, Axis};
use crate::tess::{
    disc, fill_polygon, fill_rect, stroke_polyline, stroke_styled, LineStyle, Vertex,
};
use crate::text::TextAtlas;
use crate::Viewport;
use std::collections::HashMap;

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
/// Desmos's default point size: the diameter in logical pixels.
pub const DEFAULT_POINT_SIZE: f32 = 9.0;

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
    /// Changes whenever `f` does, so a [`LineCache`] can keep its samples.
    /// None means sample it afresh every frame.
    pub key: Option<u64>,
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
            key: None,
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
    /// As [`Curve::key`].
    pub key: Option<u64>,
}

/// A parametric curve (x(t), y(t)) for t across `domain`; polar curves are
/// drawn as these too.
pub struct Parametric<'a> {
    pub x: &'a (dyn Fn(f64) -> f64 + Sync),
    pub y: &'a (dyn Fn(f64) -> f64 + Sync),
    pub domain: (f64, f64),
    pub color: [f32; 4],
    /// Thickness in logical pixels.
    pub width: f32,
    pub style: LineStyle,
    /// As [`Curve::key`].
    pub key: Option<u64>,
}

/// How a point is marked, matching Desmos's point styles.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PointStyle {
    #[default]
    Filled,
    Open,
    Cross,
}

/// Points in world coordinates, all drawn alike.
pub struct Points<'a> {
    pub points: &'a [[f64; 2]],
    pub color: [f32; 4],
    /// Diameter in logical pixels.
    pub size: f32,
    pub style: PointStyle,
    /// Whether the points themselves are drawn.
    pub dots: bool,
    /// A line joining the points in order, as a table's "lines" option
    /// draws: its width and pattern.
    pub line: Option<(f32, LineStyle)>,
}

/// A filled polygon with its outline, as `polygon(...)` draws.
pub struct Polygon<'a> {
    pub points: &'a [[f64; 2]],
    pub color: [f32; 4],
    /// The fill's opacity; Desmos's default is 0.4.
    pub fill: f32,
    /// Outline thickness in logical pixels; 0 draws no outline.
    pub width: f32,
    pub style: LineStyle,
}

/// How a mark on top of the graph looks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkKind {
    /// A grey point of interest on the selected curve.
    Poi,
    /// A point of interest under the mouse or clicked: larger and labelled.
    Hot,
    /// The point following the mouse along a curve while tracing.
    Trace,
}

/// A point drawn over everything else, with an optional coordinate label.
#[derive(Clone, Debug, PartialEq)]
pub struct Mark {
    pub at: [f64; 2],
    /// The curve's colour, used by trace points.
    pub color: [f32; 4],
    pub kind: MarkKind,
    pub label: Option<String>,
}

const POI: [f32; 4] = rgb(0x80, 0x80, 0x80);
const LABEL_BORDER: [f32; 4] = rgb(0xcc, 0xcc, 0xcc);

/// Everything to plot in one frame.
#[derive(Default)]
pub struct Plot<'a> {
    pub curves: &'a [Curve<'a>],
    pub parametrics: &'a [Parametric<'a>],
    pub implicits: &'a [Implicit<'a>],
    pub polygons: &'a [Polygon<'a>],
    pub points: &'a [Points<'a>],
    pub marks: &'a [Mark],
}

/// One frame's triangles, in two parts: the renderer draws `vertices` up to
/// `shade_after` (paper, grid and axes), then any inequality shading, then
/// the rest (axis numbers and curves), so shading sits over the grid and
/// under everything that has to stay readable.
pub struct Scene {
    pub vertices: Vec<Vertex>,
    pub shade_after: usize,
}

/// Builds the triangles for one frame: paper, grid, axes, axis numbers,
/// curves, then points on top. `scale` is the display's DPI scale (1.0 at
/// 100%, 1.5 at 150%).
pub fn build_scene(vp: &Viewport, scale: f32, plot: &Plot, text: &mut TextAtlas) -> Scene {
    build_scene_cached(vp, scale, plot, text, &mut LineCache::default())
}

/// The sampled lines of keyed curves from earlier frames. A curve whose key
/// and scale are unchanged is not sampled again: panning moves its lines,
/// and a slider that moves one curve leaves the others alone. When panning
/// leaves the area sampled, the curve is sampled over three times the view
/// each way, so the next stretch of panning is free too.
#[derive(Default)]
pub struct LineCache {
    curves: HashMap<u64, Sampled>,
    parametrics: HashMap<u64, Sampled>,
    implicits: HashMap<u64, Sampled>,
}

/// Lines in the screen coordinates of the view they were sampled in.
struct Sampled {
    vp: Viewport,
    lines: Vec<Vec<[f32; 2]>>,
}

/// What to do for one curve this frame.
enum Plan {
    Reuse(Vec<Vec<[f32; 2]>>),
    Sample(Viewport),
}

/// Extra screen space kept when clipping reused lines, so thick lines and
/// round caps at the edge still draw.
const CLIP_MARGIN_PX: f32 = 32.0;

fn plan(cached: Option<&Sampled>, vp: &Viewport, widen: bool) -> Plan {
    let Some(c) = cached.filter(|c| c.vp.units_per_px == vp.units_per_px) else {
        return Plan::Sample(*vp);
    };
    let ((x0, x1), (y0, y1)) = (vp.x_range(), vp.y_range());
    let ((cx0, cx1), (cy0, cy1)) = (c.vp.x_range(), c.vp.y_range());
    if cx0 <= x0 && x1 <= cx1 && cy0 <= y0 && y1 <= cy1 {
        return Plan::Reuse(moved(&c.lines, &c.vp, vp));
    }
    if !widen {
        return Plan::Sample(*vp);
    }
    // Panning: sample a wider area so the next frames can reuse it.
    let mut wide = *vp;
    wide.width_px *= 3.0;
    wide.height_px *= 3.0;
    Plan::Sample(wide)
}

/// `lines` sampled in view `from`, moved into view `to` (same scale) and
/// cut down to what can be seen there.
fn moved(lines: &[Vec<[f32; 2]>], from: &Viewport, to: &Viewport) -> Vec<Vec<[f32; 2]>> {
    if from == to {
        return lines.to_vec();
    }
    let at = to.to_screen(from.center[0], from.center[1]);
    let dx = (at[0] - from.width_px / 2.0) as f32;
    let dy = (at[1] - from.height_px / 2.0) as f32;
    let (w, h, m) = (to.width_px as f32, to.height_px as f32, CLIP_MARGIN_PX);
    let seen = |p: [f32; 2], q: [f32; 2]| {
        p[0].max(q[0]) >= -m
            && p[0].min(q[0]) <= w + m
            && p[1].max(q[1]) >= -m
            && p[1].min(q[1]) <= h + m
    };
    let mut out = Vec::new();
    for line in lines {
        let mut piece: Vec<[f32; 2]> = Vec::new();
        for pair in line.windows(2) {
            let p = [pair[0][0] + dx, pair[0][1] + dy];
            let q = [pair[1][0] + dx, pair[1][1] + dy];
            if seen(p, q) {
                if piece.is_empty() {
                    piece.push(p);
                }
                piece.push(q);
            } else if !piece.is_empty() {
                out.push(std::mem::take(&mut piece));
            }
        }
        if let [p] = line[..] {
            let p = [p[0] + dx, p[1] + dy];
            if seen(p, p) {
                piece.push(p);
            }
        }
        if !piece.is_empty() {
            out.push(piece);
        }
    }
    out
}

/// Samples (or reuses) one kind of curve: `key` and `sample` for each.
fn sample_all<T: Sync>(
    items: &[T],
    cache: &mut HashMap<u64, Sampled>,
    vp: &Viewport,
    widen: bool,
    key: impl Fn(&T) -> Option<u64>,
    sample: impl Fn(&T, &Viewport) -> Vec<Vec<[f32; 2]>> + Sync,
) -> Vec<Vec<Vec<[f32; 2]>>> {
    let plans: Vec<Plan> = items
        .iter()
        .map(|item| match key(item) {
            Some(k) => plan(cache.get(&k), vp, widen),
            None => Plan::Sample(*vp),
        })
        .collect();
    let sample = &sample;
    let results: Vec<Option<Vec<Vec<[f32; 2]>>>> = std::thread::scope(|scope| {
        let jobs: Vec<_> = items
            .iter()
            .zip(&plans)
            .map(|(item, plan)| match plan {
                Plan::Sample(v) => Some(scope.spawn(move || sample(item, v))),
                Plan::Reuse(_) => None,
            })
            .collect();
        jobs.into_iter()
            .map(|j| j.map(|j| j.join().unwrap_or_default()))
            .collect()
    });
    let mut used = HashMap::new();
    let mut out = Vec::with_capacity(items.len());
    for ((item, plan), result) in items.iter().zip(plans).zip(results) {
        let k = key(item);
        match (plan, result) {
            (Plan::Reuse(lines), _) => {
                if let Some(k) = k {
                    if let Some(c) = cache.remove(&k) {
                        used.insert(k, c);
                    }
                }
                out.push(lines);
            }
            (Plan::Sample(v), Some(lines)) => {
                let shown = moved(&lines, &v, vp);
                if let Some(k) = k {
                    used.insert(k, Sampled { vp: v, lines });
                }
                out.push(shown);
            }
            (Plan::Sample(_), None) => out.push(Vec::new()),
        }
    }
    // Only what this frame drew is kept.
    *cache = used;
    out
}

/// Lines to stroke: the lines, width in pixels, style and colour.
type Stroke = (Vec<Vec<[f32; 2]>>, f32, LineStyle, [f32; 4]);

/// Strokes every line, in order, splitting the work across threads when
/// there is enough of it.
fn stroke_all(strokes: &[Stroke], out: &mut Vec<Vertex>) {
    let stroke = |strokes: &[Stroke], out: &mut Vec<Vertex>| {
        for (lines, width, style, color) in strokes {
            for line in lines {
                stroke_styled(line, *width, *style, *color, out);
            }
        }
    };
    let points: usize = strokes.iter().flat_map(|s| &s.0).map(Vec::len).sum();
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
    // Below this a thread costs more than it saves.
    const POINTS_PER_THREAD: usize = 4096;
    let threads = threads.min(points / POINTS_PER_THREAD).min(strokes.len());
    if threads <= 1 {
        stroke(strokes, out);
        return;
    }
    // Contiguous runs of about equal points, so the order is kept.
    let mut runs = Vec::with_capacity(threads);
    let (mut start, mut seen) = (0, 0);
    for (i, s) in strokes.iter().enumerate() {
        seen += s.0.iter().map(Vec::len).sum::<usize>();
        if seen * threads >= points * (runs.len() + 1) {
            runs.push(&strokes[start..=i]);
            start = i + 1;
        }
    }
    if start < strokes.len() {
        runs.push(&strokes[start..]);
    }
    let parts: Vec<Vec<Vertex>> = std::thread::scope(|scope| {
        let jobs: Vec<_> = runs
            .iter()
            .map(|run| {
                scope.spawn(move || {
                    let mut part = Vec::new();
                    stroke(run, &mut part);
                    part
                })
            })
            .collect();
        jobs.into_iter()
            .map(|j| j.join().unwrap_or_default())
            .collect()
    });
    out.reserve(parts.iter().map(Vec::len).sum());
    for part in parts {
        out.extend_from_slice(&part);
    }
}

/// As [`build_scene`], reusing lines from `cache` where it can.
pub fn build_scene_cached(
    vp: &Viewport,
    scale: f32,
    plot: &Plot,
    text: &mut TextAtlas,
    cache: &mut LineCache,
) -> Scene {
    let Plot {
        curves,
        parametrics,
        implicits,
        polygons,
        points,
        marks,
    } = *plot;
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
    // its own thread, unless its lines from an earlier frame still serve.
    let sampled = sample_all(
        curves,
        &mut cache.curves,
        vp,
        true,
        |c| c.key,
        |c, v| sample_curve(c.f, v, c.axis),
    );
    let swept = sample_all(
        parametrics,
        &mut cache.parametrics,
        vp,
        true,
        |p| p.key,
        |p, v| sample_parametric(p.x, p.y, p.domain, v),
    );
    // Tracing three times the view each way costs too much to do ahead.
    let traced = sample_all(
        implicits,
        &mut cache.implicits,
        vp,
        false,
        |i| i.key,
        |i, v| trace_implicit(i.field, v),
    );
    for polygon in polygons {
        draw_polygon(vp, scale, polygon, &mut out);
    }
    let strokes: Vec<Stroke> = (curves.iter().zip(sampled))
        .map(|(c, lines)| (lines, c.width * scale, c.style, c.color))
        .chain((parametrics.iter().zip(swept)).map(|(p, l)| (l, p.width * scale, p.style, p.color)))
        .chain((implicits.iter().zip(traced)).map(|(i, l)| (l, i.width * scale, i.style, i.color)))
        .collect();
    stroke_all(&strokes, &mut out);
    for set in points {
        draw_points(vp, scale, set, &mut out);
    }
    // Labels go after every dot so none is hidden under a neighbour.
    for mark in marks {
        draw_mark(vp, scale, mark, &mut out);
    }
    for mark in marks {
        if let Some(label) = &mark.label {
            draw_mark_label(vp, scale, mark.at, label, text, &mut out);
        }
    }
    Scene {
        vertices: out,
        shade_after,
    }
}

fn draw_polygon(vp: &Viewport, scale: f32, polygon: &Polygon, out: &mut Vec<Vertex>) {
    // Far-off vertices are clamped so f32 vertices stay accurate.
    const CLAMP: f64 = 1.0e5;
    let ring: Vec<[f32; 2]> = polygon
        .points
        .iter()
        .map(|p| vp.to_screen(p[0], p[1]))
        .filter(|s| s[0].is_finite() && s[1].is_finite())
        .map(|s| {
            [
                s[0].clamp(-CLAMP, CLAMP) as f32,
                s[1].clamp(-CLAMP, CLAMP) as f32,
            ]
        })
        .collect();
    let [r, g, b, a] = polygon.color;
    if polygon.fill > 0.0 {
        fill_polygon(&ring, [r, g, b, a * polygon.fill], out);
    }
    if polygon.width > 0.0 && ring.len() >= 2 {
        let mut closed = ring.clone();
        closed.push(ring[0]);
        stroke_styled(
            &closed,
            polygon.width * scale,
            polygon.style,
            polygon.color,
            out,
        );
    }
}

fn draw_points(vp: &Viewport, scale: f32, set: &Points, out: &mut Vec<Vertex>) {
    if let Some((width, style)) = set.line {
        // Far-off points are clamped so f32 vertices stay accurate.
        const CLAMP: f64 = 1.0e5;
        let mut run: Vec<[f32; 2]> = Vec::new();
        for p in set
            .points
            .iter()
            .chain(std::iter::once(&[f64::NAN, f64::NAN]))
        {
            let [sx, sy] = vp.to_screen(p[0], p[1]);
            if sx.is_finite() && sy.is_finite() {
                run.push([
                    sx.clamp(-CLAMP, CLAMP) as f32,
                    sy.clamp(-CLAMP, CLAMP) as f32,
                ]);
            } else {
                if run.len() >= 2 {
                    stroke_styled(&run, width * scale, style, set.color, out);
                }
                run.clear();
            }
        }
    }
    if !set.dots {
        return;
    }
    let r = set.size * scale / 2.0;
    let (w, h) = (vp.width_px as f32, vp.height_px as f32);
    for p in set.points {
        let [sx, sy] = vp.to_screen(p[0], p[1]);
        let c = [sx as f32, sy as f32];
        let visible = (-r..=w + r).contains(&c[0]) && (-r..=h + r).contains(&c[1]);
        if !visible {
            continue;
        }
        match set.style {
            PointStyle::Filled => disc(c, r, set.color, out),
            PointStyle::Open => {
                // A white centre inside a ring of the point's colour.
                disc(c, r, set.color, out);
                disc(c, r * 0.55, BACKGROUND, out);
            }
            PointStyle::Cross => {
                let (a, k) = (r * 0.8, (r * 0.45).max(1.0));
                stroke_polyline(
                    &[[c[0] - a, c[1] - a], [c[0] + a, c[1] + a]],
                    k,
                    set.color,
                    out,
                );
                stroke_polyline(
                    &[[c[0] - a, c[1] + a], [c[0] + a, c[1] - a]],
                    k,
                    set.color,
                    out,
                );
            }
        }
    }
}

fn draw_mark(vp: &Viewport, scale: f32, mark: &Mark, out: &mut Vec<Vertex>) {
    let [sx, sy] = vp.to_screen(mark.at[0], mark.at[1]);
    if !(sx.is_finite() && sy.is_finite()) {
        return;
    }
    let c = [sx as f32, sy as f32];
    match mark.kind {
        MarkKind::Poi => disc(c, 4.0 * scale, POI, out),
        MarkKind::Hot => {
            disc(c, 6.5 * scale, BACKGROUND, out);
            disc(c, 5.0 * scale, POI, out);
        }
        MarkKind::Trace => {
            disc(c, 7.5 * scale, BACKGROUND, out);
            disc(c, 6.0 * scale, mark.color, out);
        }
    }
}

/// A coordinate label in a white box up and to the right of the point,
/// flipped to stay on screen.
fn draw_mark_label(
    vp: &Viewport,
    scale: f32,
    at: [f64; 2],
    label: &str,
    text: &mut TextAtlas,
    out: &mut Vec<Vertex>,
) {
    if !text.has_font() {
        return;
    }
    let [sx, sy] = vp.to_screen(at[0], at[1]);
    let (sx, sy) = (sx as f32, sy as f32);
    let size = (15.0 * scale).round();
    let pad = 5.0 * scale;
    let width = text.measure(label, size) + 2.0 * pad;
    let height = size + 2.0 * pad;
    let gap = 10.0 * scale;
    let (w, h) = (vp.width_px as f32, vp.height_px as f32);
    let mut x0 = sx + gap;
    if x0 + width > w {
        x0 = sx - gap - width;
    }
    let mut y0 = sy - gap - height;
    if y0 < 0.0 {
        y0 = sy + gap;
    }
    let (x0, y0) = (
        x0.clamp(0.0, (w - width).max(0.0)),
        y0.clamp(0.0, (h - height).max(0.0)),
    );
    let border = scale.max(1.0);
    fill_rect(
        x0 - border,
        y0 - border,
        x0 + width + border,
        y0 + height + border,
        LABEL_BORDER,
        out,
    );
    fill_rect(x0, y0, x0 + width, y0 + height, BACKGROUND, out);
    text.draw(
        label,
        size,
        0.0,
        x0 + pad,
        y0 + pad + size * 0.78,
        LABEL,
        out,
    );
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
    fn panned_lines_come_from_the_cache_and_stay_on_the_curve() {
        let sin = |x: f64| x.sin();
        let mut curve = Curve::new(&sin, DESMOS_COLORS[0]);
        curve.key = Some(7);
        let mut cache = HashMap::new();
        let calls = std::sync::atomic::AtomicUsize::new(0);
        let mut draw = |vp: &Viewport| {
            sample_all(
                &[&curve],
                &mut cache,
                vp,
                true,
                |c| c.key,
                |c, v| {
                    calls.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    sample_curve(c.f, v, c.axis)
                },
            )
            .remove(0)
        };
        let mut vp = Viewport::new(800.0, 600.0);
        draw(&vp);
        let mut lines = Vec::new();
        for _ in 0..5 {
            vp.pan_px(37.0, -11.0);
            lines = draw(&vp);
        }
        // The first pan samples three times the view; the rest reuse it.
        assert_eq!(calls.load(std::sync::atomic::Ordering::Relaxed), 2);
        let (w, h) = (vp.width_px as f32, vp.height_px as f32);
        let mut seen = 0;
        for p in lines.iter().flatten() {
            assert!(p[0] > -100.0 && p[0] < w + 100.0 && p[1] > -100.0 && p[1] < h + 100.0);
            let [x, y] = vp.to_world(p[0] as f64, p[1] as f64);
            assert!(
                (y - x.sin()).abs() < vp.units_per_px,
                "{p:?} is off the curve"
            );
            seen += 1;
        }
        assert!(seen > 20);
        // Zooming samples again.
        vp.zoom_at(400.0, 300.0, 1.1);
        draw(&vp);
        assert_eq!(calls.load(std::sync::atomic::Ordering::Relaxed), 3);
    }

    #[test]
    fn scene_has_paper_grid_axes_and_curve() {
        let vp = Viewport::new(1000.0, 600.0);
        let curve = Curve::new(&f64::sin, DESMOS_COLORS[0]);
        let plot = Plot {
            curves: &[curve],
            ..Plot::default()
        };
        let verts = build_scene(&vp, 1.0, &plot, &mut TextAtlas::empty()).vertices;
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
            key: None,
        };
        let plot = Plot {
            implicits: &[implicit],
            ..Plot::default()
        };
        let scene = build_scene(&vp, 1.0, &plot, &mut TextAtlas::empty());
        let verts = scene.vertices;
        assert!(verts.iter().any(|v| v.color == DESMOS_COLORS[1]));
    }

    #[test]
    fn parametrics_and_points_reach_the_scene() {
        let vp = Viewport::new(600.0, 600.0);
        let (cos, sin) = (f64::cos, f64::sin);
        let parametric = Parametric {
            x: &cos,
            y: &sin,
            domain: (0.0, std::f64::consts::TAU),
            color: DESMOS_COLORS[2],
            width: DEFAULT_CURVE_WIDTH,
            style: LineStyle::Solid,
            key: None,
        };
        let points = Points {
            points: &[[1.0, 2.0], [f64::NAN, 0.0], [1e9, 0.0]],
            color: DESMOS_COLORS[3],
            size: DEFAULT_POINT_SIZE,
            style: PointStyle::Filled,
            dots: true,
            line: None,
        };
        let plot = Plot {
            parametrics: &[parametric],
            points: &[points],
            ..Plot::default()
        };
        let verts = build_scene(&vp, 1.0, &plot, &mut TextAtlas::empty()).vertices;
        assert!(verts.iter().any(|v| v.color == DESMOS_COLORS[2]));
        let dots: Vec<_> = verts
            .iter()
            .filter(|v| v.color == DESMOS_COLORS[3])
            .collect();
        assert!(!dots.is_empty(), "the visible point is drawn");
        let centre = vp.to_screen(1.0, 2.0);
        assert!(dots
            .iter()
            .all(|v| (v.pos[0] as f64 - centre[0]).abs() < 6.0));
    }

    #[test]
    fn points_can_be_joined_without_dots() {
        let vp = Viewport::new(600.0, 600.0);
        let points = Points {
            points: &[[-2.0, -2.0], [0.0, 1.0], [2.0, -2.0]],
            color: DESMOS_COLORS[4],
            size: DEFAULT_POINT_SIZE,
            style: PointStyle::Filled,
            dots: false,
            line: Some((DEFAULT_CURVE_WIDTH, LineStyle::Solid)),
        };
        let plot = Plot {
            points: &[points],
            ..Plot::default()
        };
        let verts = build_scene(&vp, 1.0, &plot, &mut TextAtlas::empty()).vertices;
        let line: Vec<_> = verts
            .iter()
            .filter(|v| v.color == DESMOS_COLORS[4])
            .collect();
        // The line reaches every point, and with no dots nothing sticks
        // out further than the line's half-width plus a joint.
        for p in [[-2.0, -2.0], [0.0, 1.0], [2.0, -2.0]] {
            let c = vp.to_screen(p[0], p[1]);
            let near = |v: &&Vertex| {
                (v.pos[0] as f64 - c[0]).abs() < 3.0 && (v.pos[1] as f64 - c[1]).abs() < 3.0
            };
            assert!(line.iter().any(near), "line reaches {p:?}");
        }
        let left = vp.to_screen(-2.0, -2.0)[0];
        assert!(line.iter().all(|v| v.pos[0] as f64 > left - 3.0));
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
        let verts = build_scene(&vp, 1.0, &Plot::default(), &mut text).vertices;
        assert!(verts.iter().any(|v| v.color == LABEL));
    }
}
