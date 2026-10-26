use bytemuck::{Pod, Zeroable};

/// One vertex of a filled triangle, in graph-area pixels. `uv` points into
/// the text atlas; shapes use [`SOLID_UV`], a patch of the atlas that is
/// always fully covered.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct Vertex {
    pub pos: [f32; 2],
    pub uv: [f32; 2],
    pub color: [f32; 4],
}

pub use crate::text::SOLID_UV;

impl Vertex {
    pub const fn solid(pos: [f32; 2], color: [f32; 4]) -> Self {
        Vertex {
            pos,
            uv: SOLID_UV,
            color,
        }
    }
}

/// How a curve is stroked, matching Desmos's three line styles.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LineStyle {
    #[default]
    Solid,
    Dashed,
    Dotted,
}

/// Strokes a polyline in the given style. Dash and dot spacing scale with the
/// width, and the pattern runs on across vertices so it does not restart at
/// every sample point.
pub fn stroke_styled(
    points: &[[f32; 2]],
    width: f32,
    style: LineStyle,
    color: [f32; 4],
    out: &mut Vec<Vertex>,
) {
    match style {
        LineStyle::Solid => stroke_polyline(points, width, color, out),
        LineStyle::Dashed => {
            let (dash, gap) = (4.0 * width, 2.6 * width);
            let mut piece: Vec<[f32; 2]> = Vec::new();
            walk(points, dash, dash + gap, |pos, phase, _| {
                if phase < dash {
                    piece.push(pos);
                } else if !piece.is_empty() {
                    piece.push(pos);
                    stroke_polyline(&piece, width, color, out);
                    piece.clear();
                }
            });
            if piece.len() > 1 {
                stroke_polyline(&piece, width, color, out);
            }
        }
        LineStyle::Dotted => {
            let spacing = 2.2 * width;
            walk(points, spacing, spacing, |pos, _, start| {
                if start {
                    disc(pos, width * 0.55, color, out);
                }
            });
        }
    }
}

/// Visits every vertex of the polyline plus the points where the repeating
/// pattern of length `period` restarts or reaches `dash_end`. The callback
/// gets the position, the phase within the period, and whether this point
/// starts a new period.
fn walk(
    points: &[[f32; 2]],
    dash_end: f32,
    period: f32,
    mut visit: impl FnMut([f32; 2], f32, bool),
) {
    let Some(&first) = points.first() else { return };
    let mut phase = 0.0f32;
    visit(first, 0.0, true);
    for pair in points.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let len = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt();
        if len <= f32::EPSILON {
            continue;
        }
        let at = |d: f32| {
            [
                a[0] + (b[0] - a[0]) * d / len,
                a[1] + (b[1] - a[1]) * d / len,
            ]
        };
        let mut done = 0.0f32;
        loop {
            // The next event: the dash end or the period end, whichever is ahead.
            let next = if phase < dash_end { dash_end } else { period };
            let step = next - phase;
            if done + step > len {
                phase += len - done;
                visit(b, phase, false);
                break;
            }
            done += step;
            if next == period {
                phase = 0.0;
                visit(at(done), 0.0, true);
            } else {
                phase = next;
                visit(at(done), phase, false);
            }
        }
    }
}

/// Turns a polyline into triangles of the given width, with round joins so
/// bends stay smooth. MSAA in the renderer smooths the edges.
pub fn stroke_polyline(points: &[[f32; 2]], width: f32, color: [f32; 4], out: &mut Vec<Vertex>) {
    let half = width / 2.0;
    for pair in points.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let len = (dx * dx + dy * dy).sqrt();
        if len <= f32::EPSILON {
            continue;
        }
        let (nx, ny) = (-dy / len * half, dx / len * half);
        let quad = [
            [a[0] + nx, a[1] + ny],
            [a[0] - nx, a[1] - ny],
            [b[0] + nx, b[1] + ny],
            [b[0] - nx, b[1] - ny],
        ];
        for i in [0, 1, 2, 2, 1, 3] {
            out.push(Vertex::solid(quad[i], color));
        }
    }
    for triple in points.windows(3) {
        join(triple, half, color, out);
    }
}

/// Fills the gap on the outside of a bend: a bevel triangle on each side for
/// gentle bends, which is indistinguishable from round at curve widths, and a
/// disc for sharp corners.
fn join(t: &[[f32; 2]], half: f32, color: [f32; 4], out: &mut Vec<Vertex>) {
    let (ax, ay) = (t[1][0] - t[0][0], t[1][1] - t[0][1]);
    let (bx, by) = (t[2][0] - t[1][0], t[2][1] - t[1][1]);
    let (la, lb) = ((ax * ax + ay * ay).sqrt(), (bx * bx + by * by).sqrt());
    if la <= f32::EPSILON || lb <= f32::EPSILON {
        return;
    }
    let cross = ax * by - ay * bx;
    let dot = ax * bx + ay * by;
    // Under about one degree the gap is invisible.
    if cross.abs() < 0.017 * dot.abs() && dot > 0.0 {
        return;
    }
    if dot < 0.5 * la * lb {
        disc(t[1], half, color, out);
        return;
    }
    let c = t[1];
    let n1 = [-ay / la * half, ax / la * half];
    let n2 = [-by / lb * half, bx / lb * half];
    for s in [1.0, -1.0] {
        out.push(Vertex::solid(c, color));
        out.push(Vertex::solid([c[0] + s * n1[0], c[1] + s * n1[1]], color));
        out.push(Vertex::solid([c[0] + s * n2[0], c[1] + s * n2[1]], color));
    }
}

pub(crate) fn disc(c: [f32; 2], r: f32, color: [f32; 4], out: &mut Vec<Vertex>) {
    // Enough segments that the polygon edge stays within a tenth of a pixel.
    let segments = ((std::f32::consts::PI / (1.0 - 0.1 / r.max(0.2)).clamp(0.0, 1.0).acos()).ceil()
        as usize)
        .clamp(6, 32);
    let point = |i: usize| {
        let t = i as f32 / segments as f32 * std::f32::consts::TAU;
        [c[0] + r * t.cos(), c[1] + r * t.sin()]
    };
    for i in 0..segments {
        out.push(Vertex::solid(c, color));
        out.push(Vertex::solid(point(i), color));
        out.push(Vertex::solid(point(i + 1), color));
    }
}

pub fn fill_rect(x0: f32, y0: f32, x1: f32, y1: f32, color: [f32; 4], out: &mut Vec<Vertex>) {
    let quad = [[x0, y0], [x1, y0], [x0, y1], [x1, y1]];
    for i in [0, 1, 2, 2, 1, 3] {
        out.push(Vertex::solid(quad[i], color));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn straight_line_is_two_triangles_per_segment() {
        let mut v = Vec::new();
        stroke_polyline(
            &[[0.0, 0.0], [10.0, 0.0], [20.0, 0.0]],
            2.0,
            [0.0; 4],
            &mut v,
        );
        assert_eq!(v.len(), 12);
        assert!(v.iter().all(|p| p.pos[1].abs() <= 1.0 + 1e-6));
    }

    #[test]
    fn sharp_bend_gets_round_join() {
        let mut v = Vec::new();
        stroke_polyline(
            &[[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]],
            2.0,
            [0.0; 4],
            &mut v,
        );
        // Two segments plus a disc at the right angle.
        assert!(v.len() > 12 + 18, "{}", v.len());
    }

    #[test]
    fn dashes_and_dots_follow_the_pattern() {
        let line = [[0.0, 0.0], [33.0, 0.0], [66.0, 0.0]];
        let mut dashed = Vec::new();
        // Width 1: dashes of 4 px every 6.6 px, so 10 dashes over 66 px.
        stroke_styled(&line, 1.0, LineStyle::Dashed, [0.0; 4], &mut dashed);
        let xs: Vec<f32> = dashed.iter().map(|v| v.pos[0]).collect();
        assert!(xs.iter().all(|x| (0.0..=66.0).contains(x)));
        assert!(
            !dashed.iter().any(|v| (v.pos[0] - 5.0).abs() < 1.0),
            "no ink in the first gap"
        );
        let mut dotted = Vec::new();
        stroke_styled(&line, 1.0, LineStyle::Dotted, [0.0; 4], &mut dotted);
        // One dot every 2.2 px: 30 or 31 equal dots, depending on rounding
        // at the far end.
        let per_dot = dotted.len() / 30;
        assert!(dotted.len() % 30 == 0 || dotted.len() % 31 == 0);
        assert!(per_dot >= 18, "dots have at least 6 segments");
    }
}
