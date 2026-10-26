use bytemuck::{Pod, Zeroable};

/// One vertex of a filled triangle, in graph-area pixels.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct Vertex {
    pub pos: [f32; 2],
    pub color: [f32; 4],
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
            out.push(Vertex {
                pos: quad[i],
                color,
            });
        }
    }
    if points.len() > 2 {
        for triple in points.windows(3) {
            if bends(triple) {
                disc(triple[1], half, color, out);
            }
        }
    }
}

/// True when the turn at the middle point is sharp enough for a visible gap.
fn bends(t: &[[f32; 2]]) -> bool {
    let (ax, ay) = (t[1][0] - t[0][0], t[1][1] - t[0][1]);
    let (bx, by) = (t[2][0] - t[1][0], t[2][1] - t[1][1]);
    let cross = ax * by - ay * bx;
    let dot = ax * bx + ay * by;
    // About 8 degrees.
    cross.abs() > 0.14 * dot.abs() || dot < 0.0
}

fn disc(c: [f32; 2], r: f32, color: [f32; 4], out: &mut Vec<Vertex>) {
    const SEGMENTS: usize = 10;
    let point = |i: usize| {
        let t = i as f32 / SEGMENTS as f32 * std::f32::consts::TAU;
        [c[0] + r * t.cos(), c[1] + r * t.sin()]
    };
    for i in 0..SEGMENTS {
        out.push(Vertex { pos: c, color });
        out.push(Vertex {
            pos: point(i),
            color,
        });
        out.push(Vertex {
            pos: point(i + 1),
            color,
        });
    }
}

pub fn fill_rect(x0: f32, y0: f32, x1: f32, y1: f32, color: [f32; 4], out: &mut Vec<Vertex>) {
    let quad = [[x0, y0], [x1, y0], [x0, y1], [x1, y1]];
    for i in [0, 1, 2, 2, 1, 3] {
        out.push(Vertex {
            pos: quad[i],
            color,
        });
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
        assert_eq!(v.len(), 12 + 30);
    }
}
