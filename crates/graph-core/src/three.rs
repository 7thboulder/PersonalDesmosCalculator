//! 3D graphs, as Desmos 3D draws them: a cube (the box) showing a range of
//! x, y and z, seen from a camera that orbits it. Everything is built in box
//! coordinates, where the box runs from -1 to 1 along each axis, so turning
//! the camera only changes the matrix and nothing has to be rebuilt.

use crate::grid::{grid_spacing, line_indices};
use crate::scene::axis_label;
use crate::tess::Vertex;
use crate::text::TextAtlas;
use bytemuck::{Pod, Zeroable};
use std::collections::HashMap;
use std::hash::BuildHasherDefault;

/// How far the camera sits from the centre of the box, in box units. Far
/// enough that the perspective is gentle.
const EYE_DISTANCE: f64 = 7.0;
/// The box's half width on screen, as a share of the smaller side of the
/// graph area, at the depth of its centre.
const BOX_SCREEN: f64 = 0.29;
/// Desmos 3D's starting view: x toward the viewer and to the left, y to the
/// right, z up, seen from a little above.
const DEFAULT_YAW: f64 = 0.62;
const DEFAULT_PITCH: f64 = 0.42;
/// Turn per pixel dragged, in radians.
const TURN_PER_PX: f64 = 0.008;

const EDGE: [f32; 4] = rgb(0xb4, 0xb4, 0xb4);
const GRID: [f32; 4] = rgb(0xdd, 0xdd, 0xdd);
const AXIS: [f32; 4] = rgb(0x55, 0x55, 0x55);
const LABEL: [f32; 4] = rgb(0x1a, 0x1a, 0x1a);
const LABEL_SIZE: f32 = 13.0;
const NAME_SIZE: f32 = 17.0;
const LABEL_HALO: f32 = 2.0;

const fn rgb(r: u8, g: u8, b: u8) -> [f32; 4] {
    [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0]
}

/// What a 3D graph shows and from where.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View3 {
    /// The point at the centre of the box.
    pub center: [f64; 3],
    /// Half the box's extent along each axis, in graph units.
    pub half: [f64; 3],
    /// The camera's turn about the z axis, measured from the x axis.
    pub yaw: f64,
    /// The camera's height above the xy plane, as an angle.
    pub pitch: f64,
    pub width_px: f64,
    pub height_px: f64,
}

impl View3 {
    /// Desmos 3D's starting view: -10 to 10 on every axis.
    pub fn new(width_px: f64, height_px: f64) -> Self {
        Self {
            center: [0.0; 3],
            half: [10.0; 3],
            yaw: DEFAULT_YAW,
            pitch: DEFAULT_PITCH,
            width_px,
            height_px,
        }
    }

    pub fn resize(&mut self, width_px: f64, height_px: f64) {
        self.width_px = width_px;
        self.height_px = height_px;
    }

    /// A graph point in box coordinates.
    pub fn to_box(&self, p: [f64; 3]) -> [f64; 3] {
        [0, 1, 2].map(|i| (p[i] - self.center[i]) / self.half[i])
    }

    /// The camera's axes in box coordinates: screen right, screen up, and
    /// toward the camera.
    fn axes(&self) -> [[f64; 3]; 3] {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        let toward = [cp * cy, cp * sy, sp];
        let right = [-sy, cy, 0.0];
        let up = cross(toward, right);
        [right, up, toward]
    }

    /// The direction toward the camera, in box coordinates.
    pub fn toward(&self) -> [f64; 3] {
        self.axes()[2]
    }

    /// Pixels per box unit at the depth of the box's centre.
    fn pixels_per_unit(&self) -> f64 {
        BOX_SCREEN * self.width_px.min(self.height_px)
    }

    /// The matrix from box coordinates to clip space, by rows. Depth runs
    /// from 0 just in front of the box to 1 just behind it.
    pub fn matrix(&self) -> [[f32; 4]; 4] {
        let [right, up, toward] = self.axes();
        let focal = self.pixels_per_unit() * EYE_DISTANCE;
        let sx = focal / (self.width_px.max(1.0) / 2.0);
        let sy = focal / (self.height_px.max(1.0) / 2.0);
        let (near, far) = (EYE_DISTANCE - 2.0, EYE_DISTANCE + 2.0);
        let a = far / (far - near);
        let b = -far * near / (far - near);
        let row = |v: [f64; 3], k: f64, w: f64| {
            [
                (v[0] * k) as f32,
                (v[1] * k) as f32,
                (v[2] * k) as f32,
                w as f32,
            ]
        };
        [
            row(right, sx, 0.0),
            row(up, sy, 0.0),
            row(toward, -a, a * EYE_DISTANCE + b),
            row(toward, -1.0, EYE_DISTANCE),
        ]
    }

    /// Where a box point lands in graph-area pixels.
    pub fn project(&self, b: [f64; 3]) -> [f64; 2] {
        let [right, up, toward] = self.axes();
        let depth = EYE_DISTANCE - dot(b, toward);
        let k = self.pixels_per_unit() * EYE_DISTANCE / depth;
        [
            self.width_px / 2.0 + dot(b, right) * k,
            self.height_px / 2.0 - dot(b, up) * k,
        ]
    }

    /// Turns the camera for a drag of (`dx`, `dy`) pixels, as Desmos does:
    /// sideways turns about the z axis and up and down tilts, stopping at
    /// straight overhead and straight below.
    pub fn turn_px(&mut self, dx: f64, dy: f64) {
        self.yaw = (self.yaw - dx * TURN_PER_PX).rem_euclid(std::f64::consts::TAU);
        let limit = std::f64::consts::FRAC_PI_2;
        self.pitch = (self.pitch + dy * TURN_PER_PX).clamp(-limit, limit);
    }

    /// Slides the box's contents with the mouse, across the screen.
    pub fn pan_px(&mut self, dx: f64, dy: f64) {
        let [right, up, _] = self.axes();
        let k = 1.0 / self.pixels_per_unit();
        for i in 0..3 {
            self.center[i] -= (right[i] * dx - up[i] * dy) * k * self.half[i];
        }
    }

    /// Shows `factor` times closer (more than 1 zooms in): the box keeps
    /// its size and shows a smaller range.
    pub fn zoom(&mut self, factor: f64) {
        if factor.is_finite() && factor > 0.0 {
            for h in &mut self.half {
                *h = (*h / factor).clamp(1e-12, 1e12);
            }
        }
    }
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// One vertex of a surface, in box coordinates.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct Vertex3 {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
    pub color: [f32; 4],
}

/// Triangles sharing vertices.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Mesh {
    pub vertices: Vec<Vertex3>,
    pub indices: Vec<u32>,
}

/// Samples along each side of the box for a surface.
pub const SURFACE_SAMPLES: usize = 160;
/// How far past the box a surface's height is kept before it is cut off;
/// the GPU trims everything outside the box.
const HEIGHT_LIMIT: f64 = 4.0;

/// The surface `z = f(x, y)` over the box, as a grid of `n` by `n` cells.
/// Cells where f is undefined are left out.
pub fn surface_mesh(
    view: &View3,
    f: &(dyn Fn(f64, f64) -> f64 + Sync),
    color: [f32; 4],
    n: usize,
) -> Mesh {
    let n = n.max(2);
    let side = n + 1;
    let step = 2.0 / n as f64;
    // The height in box units over a box point, NaN where undefined.
    let height = |bx: f64, by: f64| {
        let x = view.center[0] + bx * view.half[0];
        let y = view.center[1] + by * view.half[1];
        let z = (f(x, y) - view.center[2]) / view.half[2];
        if z.is_finite() {
            z.clamp(-HEIGHT_LIMIT, HEIGHT_LIMIT)
        } else {
            f64::NAN
        }
    };
    let height = &height;
    let mut heights = vec![f64::NAN; side * side];
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
    let rows_each = side.div_ceil(threads.clamp(1, 16));
    std::thread::scope(|scope| {
        for (chunk, rows) in heights.chunks_mut(rows_each * side).enumerate() {
            scope.spawn(move || {
                for (k, h) in rows.iter_mut().enumerate() {
                    let (j, i) = (chunk * rows_each + k / side, k % side);
                    *h = height(-1.0 + i as f64 * step, -1.0 + j as f64 * step);
                }
            });
        }
    });

    let at = |i: usize, j: usize| heights[j * side + i];
    // The slope from neighbours on both sides where they are defined.
    let slope = |i: usize, j: usize, di: usize, dj: usize| -> f64 {
        let here = at(i, j);
        let before = (i >= di && j >= dj).then(|| at(i - di, j - dj));
        let after = (i + di < side && j + dj < side).then(|| at(i + di, j + dj));
        match (
            before.filter(|v| v.is_finite()),
            after.filter(|v| v.is_finite()),
        ) {
            (Some(b), Some(a)) => (a - b) / (2.0 * step),
            (Some(b), None) => (here - b) / step,
            (None, Some(a)) => (a - here) / step,
            (None, None) => 0.0,
        }
    };
    let mut mesh = Mesh::default();
    let mut index = vec![u32::MAX; side * side];
    for j in 0..side {
        for i in 0..side {
            let z = at(i, j);
            if !z.is_finite() {
                continue;
            }
            let (dx, dy) = (slope(i, j, 1, 0), slope(i, j, 0, 1));
            let len = (dx * dx + dy * dy + 1.0).sqrt();
            index[j * side + i] = mesh.vertices.len() as u32;
            mesh.vertices.push(Vertex3 {
                pos: [
                    (-1.0 + i as f64 * step) as f32,
                    (-1.0 + j as f64 * step) as f32,
                    z as f32,
                ],
                normal: [(-dx / len) as f32, (-dy / len) as f32, (1.0 / len) as f32],
                color,
            });
        }
    }
    for j in 0..n {
        for i in 0..n {
            // Corners in order around the cell.
            let corners = [(i, j), (i + 1, j), (i + 1, j + 1), (i, j + 1)];
            let ids = corners.map(|(i, j)| index[j * side + i]);
            let defined = ids.iter().filter(|&&v| v != u32::MAX).count();
            if defined == 4 {
                mesh.indices
                    .extend([ids[0], ids[1], ids[3], ids[1], ids[2], ids[3]]);
                continue;
            }
            if defined == 0 {
                continue;
            }
            // Where the surface stops partway across the cell, as it does
            // at the rim of a hemisphere, find where along each edge it
            // stops, so the rim is smooth rather than stepped.
            let mut polygon: Vec<u32> = Vec::with_capacity(6);
            for k in 0..4 {
                let (here, next) = (ids[k], ids[(k + 1) % 4]);
                if here != u32::MAX {
                    polygon.push(here);
                }
                if (here == u32::MAX) == (next == u32::MAX) {
                    continue;
                }
                let (inside, outside) = if here != u32::MAX {
                    (corners[k], corners[(k + 1) % 4])
                } else {
                    (corners[(k + 1) % 4], corners[k])
                };
                let point =
                    |c: (usize, usize)| [-1.0 + c.0 as f64 * step, -1.0 + c.1 as f64 * step];
                let (mut good, mut bad) = (point(inside), point(outside));
                let mut z = at(inside.0, inside.1);
                for _ in 0..10 {
                    let mid = [(good[0] + bad[0]) / 2.0, (good[1] + bad[1]) / 2.0];
                    let h = height(mid[0], mid[1]);
                    if h.is_finite() {
                        good = mid;
                        z = h;
                    } else {
                        bad = mid;
                    }
                }
                let near = mesh.vertices[index[inside.1 * side + inside.0] as usize];
                polygon.push(mesh.vertices.len() as u32);
                mesh.vertices.push(Vertex3 {
                    pos: [good[0] as f32, good[1] as f32, z as f32],
                    ..near
                });
            }
            for k in 1..polygon.len().saturating_sub(1) {
                mesh.indices
                    .extend([polygon[0], polygon[k], polygon[k + 1]]);
            }
        }
    }
    mesh
}

/// Samples along each side of the box for an equation in space.
pub const IMPLICIT_SAMPLES: usize = 72;
/// Samples along u and along v for a parametric surface.
pub const PARAMETRIC_SAMPLES: usize = 120;

/// Fills `values` (laid out `[k][j][i]`, `side` to a side) with `f` at each
/// grid point, one slab of k per thread.
fn sample_grid(values: &mut [f64], side: usize, f: &(dyn Fn(usize, usize, usize) -> f64 + Sync)) {
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
    let slabs_each = side.div_ceil(threads.clamp(1, 16));
    std::thread::scope(|scope| {
        for (chunk, slab) in values.chunks_mut(slabs_each * side * side).enumerate() {
            scope.spawn(move || {
                for (n, v) in slab.iter_mut().enumerate() {
                    let k = chunk * slabs_each + n / (side * side);
                    let (j, i) = ((n / side) % side, n % side);
                    *v = f(i, j, k);
                }
            });
        }
    });
}

/// The outside of the solid where `f(x, y, z) < 0` within the box: the
/// surface `f = 0` closed off by the box's walls where the solid meets
/// them, as Desmos 3D shades an inequality.
pub fn solid_mesh(
    view: &View3,
    f: &(dyn Fn(f64, f64, f64) -> f64 + Sync),
    color: [f32; 4],
    n: usize,
) -> Mesh {
    let mut mesh = implicit_mesh(view, f, color, n);
    let n = n.max(2);
    let side = n + 1;
    let coord = |i: usize| -1.0 + 2.0 * i as f64 / n as f64;
    // Each wall: the axis it is across, and which end.
    for axis in 0..3 {
        for end in [-1.0, 1.0] {
            let (a, b) = ((axis + 1) % 3, (axis + 2) % 3);
            let at = |i: usize, j: usize| {
                let mut p = [0.0; 3];
                p[axis] = end;
                p[a] = coord(i);
                p[b] = coord(j);
                p
            };
            let graph = |p: [f64; 3]| [0, 1, 2].map(|d| view.center[d] + p[d] * view.half[d]);
            let mut values = vec![f64::NAN; side * side];
            for j in 0..side {
                for i in 0..side {
                    let [x, y, z] = graph(at(i, j));
                    values[j * side + i] = f(x, y, z);
                }
            }
            let mut normal = [0f32; 3];
            normal[axis] = end as f32;
            for j in 0..n {
                for i in 0..n {
                    let corners = [(i, j), (i + 1, j), (i + 1, j + 1), (i, j + 1)];
                    let v = corners.map(|(ci, cj)| values[cj * side + ci]);
                    if v.iter().any(|x| !x.is_finite()) || v.iter().all(|&x| x >= 0.0) {
                        continue;
                    }
                    // The part of the cell inside the solid, corner by
                    // corner around it.
                    let mut polygon: Vec<[f64; 3]> = Vec::with_capacity(6);
                    for k in 0..4 {
                        let (p, q) = (corners[k], corners[(k + 1) % 4]);
                        let (vp, vq) = (v[k], v[(k + 1) % 4]);
                        let (pp, pq) = (at(p.0, p.1), at(q.0, q.1));
                        if vp < 0.0 {
                            polygon.push(pp);
                        }
                        if (vp < 0.0) != (vq < 0.0) {
                            let t = vp / (vp - vq);
                            polygon.push([0, 1, 2].map(|d| pp[d] + (pq[d] - pp[d]) * t));
                        }
                    }
                    let base = mesh.vertices.len() as u32;
                    mesh.vertices.extend(polygon.iter().map(|p| Vertex3 {
                        pos: p.map(|c| c as f32),
                        normal,
                        color,
                    }));
                    for k in 1..polygon.len().saturating_sub(1) as u32 {
                        mesh.indices.extend([base, base + k, base + k + 1]);
                    }
                }
            }
        }
    }
    mesh
}

/// A quick hash for grid edges, which are already well spread.
#[derive(Default)]
struct EdgeHasher(u64);

impl std::hash::Hasher for EdgeHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.write_u64(u64::from(*b));
        }
    }

    fn write_usize(&mut self, n: usize) {
        self.write_u64(n as u64);
    }

    fn write_u64(&mut self, n: u64) {
        self.0 = (self.0.rotate_left(29) ^ n).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    }
}

/// The surface where `f(x, y, z) = 0` inside the box, found by marching
/// tetrahedra over a grid of `n` cells a side. Points where f is undefined
/// break the surface there.
pub fn implicit_mesh(
    view: &View3,
    f: &(dyn Fn(f64, f64, f64) -> f64 + Sync),
    color: [f32; 4],
    n: usize,
) -> Mesh {
    let n = n.max(2);
    let side = n + 1;
    let step = 2.0 / n as f64;
    let coord = |i: usize| -1.0 + i as f64 * step;
    let mut values = vec![f64::NAN; side * side * side];
    sample_grid(&mut values, side, &|i, j, k| {
        let x = view.center[0] + coord(i) * view.half[0];
        let y = view.center[1] + coord(j) * view.half[1];
        let z = view.center[2] + coord(k) * view.half[2];
        f(x, y, z)
    });
    let at = |i: usize, j: usize, k: usize| values[(k * side + j) * side + i];
    // The field's gradient at a grid point, in box units, from its
    // neighbours.
    let gradient = |p: [usize; 3]| -> [f64; 3] {
        [0, 1, 2].map(|axis| {
            let mut lo = p;
            let mut hi = p;
            if lo[axis] > 0 {
                lo[axis] -= 1;
            }
            if hi[axis] + 1 < side {
                hi[axis] += 1;
            }
            let span = (hi[axis] - lo[axis]) as f64 * step;
            (at(hi[0], hi[1], hi[2]) - at(lo[0], lo[1], lo[2])) / span
        })
    };
    const CORNERS: [[usize; 3]; 8] = [
        [0, 0, 0],
        [1, 0, 0],
        [0, 1, 0],
        [1, 1, 0],
        [0, 0, 1],
        [1, 0, 1],
        [0, 1, 1],
        [1, 1, 1],
    ];
    // Six tetrahedra around the cube's diagonal from corner 0 to corner 7.
    const TETS: [[usize; 4]; 6] = [
        [0, 1, 3, 7],
        [0, 3, 2, 7],
        [0, 2, 6, 7],
        [0, 6, 4, 7],
        [0, 4, 5, 7],
        [0, 5, 1, 7],
    ];
    // Slabs of k on separate threads, joined in order.
    let threads = std::thread::available_parallelism().map_or(1, |t| t.get());
    let per = n.div_ceil(threads.clamp(1, 16));
    let slab = |ks: std::ops::Range<usize>| {
        let mut mesh = Mesh::default();
        // Each crossing's vertex, by the grid edge it is on, so the
        // tetrahedra around an edge share it.
        let mut made: HashMap<(usize, usize), u32, BuildHasherDefault<EdgeHasher>> =
            HashMap::default();
        for k in ks {
            for j in 0..n {
                for i in 0..n {
                    let grid = CORNERS.map(|c| [i + c[0], j + c[1], k + c[2]]);
                    let v = grid.map(|g| at(g[0], g[1], g[2]));
                    if v.iter().any(|x| !x.is_finite()) {
                        continue;
                    }
                    if v.iter().all(|&x| x < 0.0) || v.iter().all(|&x| x >= 0.0) {
                        continue;
                    }
                    for tet in TETS {
                        // The tetrahedron's corners, inside ones first.
                        let mut order = tet;
                        order.sort_by_key(|&c| v[c] >= 0.0);
                        let count = order.iter().filter(|&&c| v[c] < 0.0).count();
                        let (inside, outside) = order.split_at(count);
                        // Where the field crosses zero along the edge a-b.
                        let mut cross = |a: usize, b: usize| -> u32 {
                            let (ga, gb) = (grid[a], grid[b]);
                            let index = |g: [usize; 3]| (g[2] * side + g[1]) * side + g[0];
                            let edge = (index(ga).min(index(gb)), index(ga).max(index(gb)));
                            if let Some(&known) = made.get(&edge) {
                                return known;
                            }
                            let t = v[a] / (v[a] - v[b]);
                            let pos = [0, 1, 2]
                                .map(|d| (coord(ga[d]) + (coord(gb[d]) - coord(ga[d])) * t) as f32);
                            let (na, nb) = (gradient(ga), gradient(gb));
                            let mut normal = [0, 1, 2].map(|d| na[d] + (nb[d] - na[d]) * t);
                            let len = normal.iter().map(|c| c * c).sum::<f64>().sqrt();
                            if len.is_finite() && len > 0.0 {
                                normal = normal.map(|c| c / len);
                            } else {
                                normal = [0.0, 0.0, 1.0];
                            }
                            mesh.vertices.push(Vertex3 {
                                pos,
                                normal: normal.map(|c| c as f32),
                                color,
                            });
                            let new = mesh.vertices.len() as u32 - 1;
                            made.insert(edge, new);
                            new
                        };
                        match (inside, outside) {
                            ([a], [b, c, d]) | ([b, c, d], [a]) => {
                                let t = [cross(*a, *b), cross(*a, *c), cross(*a, *d)];
                                mesh.indices.extend(t);
                            }
                            ([a, b], [c, d]) => {
                                let q =
                                    [cross(*a, *c), cross(*a, *d), cross(*b, *d), cross(*b, *c)];
                                mesh.indices.extend([q[0], q[1], q[2], q[0], q[2], q[3]]);
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
        mesh
    };
    let parts: Vec<Mesh> = std::thread::scope(|scope| {
        let slab = &slab;
        let handles: Vec<_> = (0..n)
            .step_by(per)
            .map(|k0| scope.spawn(move || slab(k0..(k0 + per).min(n))))
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().expect("mesh thread"))
            .collect()
    });
    let mut mesh = Mesh::default();
    for part in parts {
        let base = mesh.vertices.len() as u32;
        mesh.vertices.extend(part.vertices);
        mesh.indices.extend(part.indices.iter().map(|i| i + base));
    }
    mesh
}

/// The surface traced by `f(u, v)` for u and v over their domains, as a
/// grid of `n` by `n` cells. Cells where f is undefined are left out.
pub fn parametric_mesh(
    view: &View3,
    f: &(dyn Fn(f64, f64) -> [f64; 3] + Sync),
    u: (f64, f64),
    v: (f64, f64),
    color: [f32; 4],
    n: usize,
) -> Mesh {
    let n = n.max(2);
    let side = n + 1;
    let mut points = vec![[f64::NAN; 3]; side * side];
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
    let rows_each = side.div_ceil(threads.clamp(1, 16));
    std::thread::scope(|scope| {
        for (chunk, rows) in points.chunks_mut(rows_each * side).enumerate() {
            scope.spawn(move || {
                for (m, p) in rows.iter_mut().enumerate() {
                    let (j, i) = (chunk * rows_each + m / side, m % side);
                    let uu = u.0 + (u.1 - u.0) * i as f64 / n as f64;
                    let vv = v.0 + (v.1 - v.0) * j as f64 / n as f64;
                    let b = view.to_box(f(uu, vv));
                    *p = if b.iter().all(|c| c.is_finite()) {
                        b.map(|c| c.clamp(-HEIGHT_LIMIT, HEIGHT_LIMIT))
                    } else {
                        [f64::NAN; 3]
                    };
                }
            });
        }
    });
    let at = |i: usize, j: usize| points[j * side + i];
    let ok = |p: [f64; 3]| p[0].is_finite();
    // The change across a grid point along u (or v), from whichever
    // neighbours are defined.
    let along = |i: usize, j: usize, di: usize, dj: usize| -> Option<[f64; 3]> {
        let before = (i >= di && j >= dj)
            .then(|| at(i - di, j - dj))
            .filter(|p| ok(*p));
        let after = (i + di < side && j + dj < side)
            .then(|| at(i + di, j + dj))
            .filter(|p| ok(*p));
        let (a, b) = match (before, after) {
            (Some(a), Some(b)) => (a, b),
            (Some(a), None) => (a, at(i, j)),
            (None, Some(b)) => (at(i, j), b),
            (None, None) => return None,
        };
        Some([b[0] - a[0], b[1] - a[1], b[2] - a[2]])
    };
    let mut mesh = Mesh::default();
    let mut index = vec![u32::MAX; side * side];
    for j in 0..side {
        for i in 0..side {
            let p = at(i, j);
            if !ok(p) {
                continue;
            }
            let normal = match (along(i, j, 1, 0), along(i, j, 0, 1)) {
                (Some(du), Some(dv)) => {
                    let c = cross(du, dv);
                    let len = dot(c, c).sqrt();
                    if len > 1e-12 {
                        c.map(|x| x / len)
                    } else {
                        [0.0, 0.0, 1.0]
                    }
                }
                _ => [0.0, 0.0, 1.0],
            };
            index[j * side + i] = mesh.vertices.len() as u32;
            mesh.vertices.push(Vertex3 {
                pos: p.map(|c| c as f32),
                normal: normal.map(|c| c as f32),
                color,
            });
        }
    }
    for j in 0..n {
        for i in 0..n {
            let a = index[j * side + i];
            let b = index[j * side + i + 1];
            let c = index[(j + 1) * side + i];
            let d = index[(j + 1) * side + i + 1];
            for t in [[a, b, c], [b, d, c]] {
                if t.iter().all(|&x| x != u32::MAX) {
                    mesh.indices.extend(t);
                }
            }
        }
    }
    mesh
}

/// One corner of a line segment's quad. The GPU projects both ends and
/// pushes this corner out sideways by half the width, so lines keep their
/// width in pixels however the box is turned.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct LineVertex {
    pub a: [f32; 3],
    /// Which end this corner belongs to: 0 for `a`, 1 for `b`.
    pub end: f32,
    pub b: [f32; 3],
    /// Which side of the line: -1 or 1.
    pub side: f32,
    pub color: [f32; 4],
    pub width: f32,
    /// 1 for a round dot (both ends the same point, `width` across), 0
    /// for a line.
    pub round: f32,
    pub _pad: [f32; 2],
}

fn segment(a: [f64; 3], b: [f64; 3], width: f32, color: [f32; 4], out: &mut Vec<LineVertex>) {
    quad(a, b, width, color, 0.0, out);
}

/// A round dot `size` pixels across, drawn facing the camera.
fn disc(at: [f64; 3], size: f32, color: [f32; 4], out: &mut Vec<LineVertex>) {
    quad(at, at, size, color, 1.0, out);
}

fn quad(
    a: [f64; 3],
    b: [f64; 3],
    width: f32,
    color: [f32; 4],
    round: f32,
    out: &mut Vec<LineVertex>,
) {
    let a = a.map(|v| v as f32);
    let b = b.map(|v| v as f32);
    for (end, side) in [
        (0.0, -1.0),
        (0.0, 1.0),
        (1.0, -1.0),
        (1.0, -1.0),
        (0.0, 1.0),
        (1.0, 1.0),
    ] {
        out.push(LineVertex {
            a,
            end,
            b,
            side,
            color,
            width,
            round,
            _pad: [0.0; 2],
        });
    }
}

/// Points in 3D, as dots `size` pixels across. Points outside the box are
/// A flat polygon's fill, fanned out from its first corner. Its normal is
/// Newell's average, so a slightly bent polygon still lights evenly.
pub fn polygon_mesh(view: &View3, corners: &[[f64; 3]], color: [f32; 4]) -> Mesh {
    let mut mesh = Mesh::default();
    let b: Vec<[f64; 3]> = corners.iter().map(|p| view.to_box(*p)).collect();
    if b.len() < 3 || b.iter().flatten().any(|c| !c.is_finite()) {
        return mesh;
    }
    let mut n = [0.0; 3];
    for (k, p) in b.iter().enumerate() {
        let q = b[(k + 1) % b.len()];
        n[0] += (p[1] - q[1]) * (p[2] + q[2]);
        n[1] += (p[2] - q[2]) * (p[0] + q[0]);
        n[2] += (p[0] - q[0]) * (p[1] + q[1]);
    }
    let len = dot(n, n).sqrt();
    let normal = if len > 1e-12 {
        n.map(|c| (c / len) as f32)
    } else {
        [0.0, 0.0, 1.0]
    };
    mesh.vertices = b
        .iter()
        .map(|p| Vertex3 {
            pos: p.map(|c| c.clamp(-HEIGHT_LIMIT, HEIGHT_LIMIT) as f32),
            normal,
            color,
        })
        .collect();
    for k in 1..b.len() as u32 - 1 {
        mesh.indices.extend([0, k, k + 1]);
    }
    mesh
}

/// The six faces of an axis-aligned box, its low and high end on each
/// axis in graph units, cut to the view's box.
pub fn box_mesh(view: &View3, bounds: [[f64; 2]; 3], color: [f32; 4]) -> Mesh {
    let mut mesh = Mesh::default();
    let mut b = [[0.0; 2]; 3];
    for axis in 0..3 {
        let to_box = |v: f64| ((v - view.center[axis]) / view.half[axis]).clamp(-1.0, 1.0);
        b[axis] = [to_box(bounds[axis][0]), to_box(bounds[axis][1])];
        if b[axis][0].partial_cmp(&b[axis][1]) != Some(std::cmp::Ordering::Less) {
            return mesh;
        }
    }
    for axis in 0..3 {
        let (u, w) = ((axis + 1) % 3, (axis + 2) % 3);
        for side in 0..2 {
            let mut normal = [0f32; 3];
            normal[axis] = if side == 0 { -1.0 } else { 1.0 };
            let base = mesh.vertices.len() as u32;
            for (i, j) in [(0, 0), (1, 0), (1, 1), (0, 1)] {
                let mut p = [0f32; 3];
                p[axis] = b[axis][side] as f32;
                p[u] = b[u][i] as f32;
                p[w] = b[w][j] as f32;
                mesh.vertices.push(Vertex3 {
                    pos: p,
                    normal,
                    color,
                });
            }
            mesh.indices
                .extend([base, base + 1, base + 2, base, base + 2, base + 3]);
        }
    }
    mesh
}

/// A polygon's closed outline, `width` pixels wide.
pub fn polygon_outline_3d(
    view: &View3,
    corners: &[[f64; 3]],
    width: f32,
    color: [f32; 4],
    out: &mut Vec<LineVertex>,
) {
    let b: Vec<[f64; 3]> = corners.iter().map(|p| view.to_box(*p)).collect();
    if b.len() < 2 || b.iter().flatten().any(|c| !c.is_finite()) {
        return;
    }
    for (k, p) in b.iter().enumerate() {
        segment(*p, b[(k + 1) % b.len()], width, color, out);
        disc(*p, width, color, out);
    }
}

/// left out, as Desmos does.
pub fn points_3d(
    view: &View3,
    points: &[[f64; 3]],
    size: f32,
    color: [f32; 4],
    out: &mut Vec<LineVertex>,
) {
    for p in points {
        let b = view.to_box(*p);
        if b.iter().all(|v| v.abs() <= 1.0) {
            disc(b, size, color, out);
        }
    }
}

/// Samples along a curve in 3D.
const CURVE_SAMPLES: usize = 2000;

/// The curve `t -> (x(t), y(t), z(t))` for t over `domain`, `width`
/// pixels wide. The GPU trims it to the box; stretches far outside the box
/// or where the curve is undefined are left out.
#[allow(clippy::too_many_arguments)]
pub fn curve_3d(
    view: &View3,
    x: &dyn Fn(f64) -> f64,
    y: &dyn Fn(f64) -> f64,
    z: &dyn Fn(f64) -> f64,
    domain: (f64, f64),
    width: f32,
    color: [f32; 4],
    out: &mut Vec<LineVertex>,
) {
    let (lo, hi) = domain;
    if !(lo.is_finite() && hi.is_finite()) {
        return;
    }
    let near = |b: &[f64; 3]| b.iter().all(|v| v.abs() <= HEIGHT_LIMIT);
    let mut last: Option<[f64; 3]> = None;
    for k in 0..=CURVE_SAMPLES {
        let t = lo + (hi - lo) * k as f64 / CURVE_SAMPLES as f64;
        let b = view.to_box([x(t), y(t), z(t)]);
        let here = (b.iter().all(|v| v.is_finite()) && near(&b)).then_some(b);
        if let (Some(a), Some(b)) = (last, here) {
            segment(a, b, width, color, out);
            // Round joints keep a thick curve whole where it bends.
            if k < CURVE_SAMPLES {
                disc(b, width, color, out);
            }
        }
        last = here;
    }
}

/// Grid steps along each axis, in graph units: a 1-2-5 step giving about
/// six numbers across the box.
pub fn steps(view: &View3) -> [f64; 3] {
    view.half.map(|h| grid_spacing(2.0 * h / 6.0, 1.0).major)
}

/// The box's frame: its twelve edges, a grid over the xy plane, and the
/// three axes through the origin, where they pass through the box.
pub fn box_lines(view: &View3, scale: f32) -> Vec<LineVertex> {
    let mut out = Vec::new();
    let s = |v: f64| if v < 0.5 { -1.0 } else { 1.0 };
    for k in 0..12 {
        // Edges along axis k / 4, at the other two coordinates' corners.
        let (axis, corner) = (k / 4, k % 4);
        let (u, v) = (s((corner & 1) as f64), s((corner >> 1) as f64));
        let mut a = [0.0; 3];
        let (p, q) = ((axis + 1) % 3, (axis + 2) % 3);
        a[p] = u;
        a[q] = v;
        let mut b = a;
        a[axis] = -1.0;
        b[axis] = 1.0;
        segment(a, b, 1.0 * scale, EDGE, &mut out);
    }
    let origin = view.to_box([0.0; 3]);
    let inside = |v: f64| (-1.0..=1.0).contains(&v);
    let step = steps(view);
    if inside(origin[2]) {
        for (axis, other) in [(0, 1), (1, 0)] {
            let lo = view.center[axis] - view.half[axis];
            let hi = view.center[axis] + view.half[axis];
            for i in line_indices(lo, hi, step[axis]) {
                let mut a = [0.0, 0.0, origin[2]];
                a[axis] = (i as f64 * step[axis] - view.center[axis]) / view.half[axis];
                let mut b = a;
                a[other] = -1.0;
                b[other] = 1.0;
                segment(a, b, 1.0 * scale, GRID, &mut out);
            }
        }
    }
    for axis in 0..3 {
        let (p, q) = ((axis + 1) % 3, (axis + 2) % 3);
        if inside(origin[p]) && inside(origin[q]) {
            let mut a = origin;
            let mut b = origin;
            a[axis] = -1.0;
            b[axis] = 1.0;
            segment(a, b, 1.5 * scale, AXIS, &mut out);
        }
    }
    out
}

/// Numbers along each axis and the axes' names, as 2D triangles over the
/// graph area. They are placed by projecting, so they are rebuilt whenever
/// the camera turns.
pub fn box_labels(view: &View3, scale: f32, text: &mut TextAtlas) -> Vec<Vertex> {
    let mut out = Vec::new();
    if !text.has_font() {
        return out;
    }
    let origin = view.to_box([0.0; 3]);
    let inside = |v: f64| (-1.0..=1.0).contains(&v);
    let size = (LABEL_SIZE * scale).round();
    let halo = LABEL_HALO * scale;
    let step = steps(view);
    let centre = |text: &mut TextAtlas, s: &str, size: f32, at: [f64; 2], out: &mut Vec<Vertex>| {
        let w = text.measure(s, size);
        let x = at[0] as f32 - w / 2.0;
        let baseline = at[1] as f32 + size * 0.36;
        text.draw(s, size, halo, x, baseline, LABEL, out);
    };
    for axis in 0..3 {
        let (p, q) = ((axis + 1) % 3, (axis + 2) % 3);
        // Along the axis when it is in the box, otherwise along the edge
        // of the box nearest to where it would be.
        let mut base = origin;
        base[p] = base[p].clamp(-1.0, 1.0);
        base[q] = base[q].clamp(-1.0, 1.0);
        let lo = view.center[axis] - view.half[axis];
        let hi = view.center[axis] + view.half[axis];
        // Leave room at the ends for the axis's name.
        for i in line_indices(lo, hi, step[axis]) {
            if i == 0 && inside(origin[p]) && inside(origin[q]) {
                continue;
            }
            let mut b = base;
            b[axis] = (i as f64 * step[axis] - view.center[axis]) / view.half[axis];
            if b[axis].abs() > 0.97 {
                continue;
            }
            let (label, exponent) = axis_label(i, step[axis]);
            let label = match exponent {
                Some(e) => format!("{label}^{e}"),
                None => label,
            };
            let at = view.project(b);
            let offset = 10.0 * scale as f64;
            centre(
                text,
                &label,
                size,
                [at[0] + offset, at[1] + offset],
                &mut out,
            );
        }
        let mut end = base;
        end[axis] = 1.12;
        let name = ["x", "y", "z"][axis];
        centre(
            text,
            name,
            (NAME_SIZE * scale).round(),
            view.project(end),
            &mut out,
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clip(m: &[[f32; 4]; 4], b: [f64; 3]) -> [f64; 4] {
        m.map(|r| r[0] as f64 * b[0] + r[1] as f64 * b[1] + r[2] as f64 * b[2] + r[3] as f64)
    }

    #[test]
    fn matrix_and_projection_agree() {
        let mut v = View3::new(800.0, 600.0);
        v.turn_px(37.0, -12.0);
        let m = v.matrix();
        for b in [[0.0, 0.0, 0.0], [1.0, -1.0, 1.0], [-1.0, 0.5, -1.0]] {
            let c = clip(&m, b);
            let px = [
                (c[0] / c[3] + 1.0) / 2.0 * 800.0,
                (1.0 - c[1] / c[3]) / 2.0 * 600.0,
            ];
            let p = v.project(b);
            assert!((px[0] - p[0]).abs() < 1e-3 && (px[1] - p[1]).abs() < 1e-3);
            let depth = c[2] / c[3];
            assert!((0.0..=1.0).contains(&depth), "depth {depth} for {b:?}");
        }
        // The centre of the box is in the middle of the graph area.
        assert_eq!(v.project([0.0; 3]), [400.0, 300.0]);
    }

    #[test]
    fn default_view_has_x_toward_the_viewer_and_z_up() {
        let v = View3::new(600.0, 600.0);
        let [ox, oy] = v.project([0.0; 3]);
        let x = v.project([1.0, 0.0, 0.0]);
        let y = v.project([0.0, 1.0, 0.0]);
        let z = v.project([0.0, 0.0, 1.0]);
        assert!(x[0] < ox && x[1] > oy, "x comes down and to the left");
        assert!(y[0] > ox, "y goes right");
        assert!(z[1] < oy && (z[0] - ox).abs() < 1e-9, "z goes straight up");
    }

    #[test]
    fn zoom_and_pan_change_the_range_not_the_box() {
        let mut v = View3::new(600.0, 600.0);
        v.zoom(2.0);
        assert_eq!(v.half, [5.0; 3]);
        // Dragging right moves the contents right: the centre moves the
        // other way along screen right. A point at the depth of the box's
        // centre keeps up with the mouse exactly.
        let before = v.project(v.to_box([0.0, 0.0, 0.0]));
        v.pan_px(30.0, 0.0);
        let after = v.project(v.to_box([0.0, 0.0, 0.0]));
        assert!(
            (after[0] - before[0] - 30.0).abs() < 2.0,
            "{before:?} {after:?}"
        );
    }

    #[test]
    fn surface_mesh_follows_the_function() {
        let v = View3::new(600.0, 600.0);
        let f = |x: f64, y: f64| x * y / 10.0;
        let mesh = surface_mesh(&v, &f, [1.0; 4], 10);
        assert_eq!(mesh.vertices.len(), 121);
        assert_eq!(mesh.indices.len(), 10 * 10 * 6);
        // The corner (10, 10) has z = 10, the top of the box.
        let corner = mesh.vertices.last().unwrap();
        assert_eq!(corner.pos, [1.0, 1.0, 1.0]);
        // Normals point up and are unit length.
        for p in &mesh.vertices {
            let n = p.normal;
            assert!(n[2] > 0.0);
            assert!(((n[0] * n[0] + n[1] * n[1] + n[2] * n[2]) - 1.0).abs() < 1e-5);
        }
        // Holes where f is undefined: sqrt of a negative. The column of
        // cells left of x = 0 is filled up to the edge of the hole.
        let g = |x: f64, _: f64| x.sqrt();
        let half = surface_mesh(&v, &g, [1.0; 4], 11);
        let defined = half.vertices.iter().filter(|p| p.pos[0] >= 0.0).count();
        // Each cell crossing the edge adds a point on its top and bottom.
        assert_eq!(defined, 6 * 12 + 2 * 11);
        let left = half.vertices.iter().map(|p| p.pos[0]).fold(1.0, f32::min);
        assert!(left.abs() < 1e-3, "filled to x = 0: {left}");
    }

    #[test]
    fn points_and_curves_stay_in_the_box() {
        let v = View3::new(600.0, 600.0);
        let mut out = Vec::new();
        points_3d(
            &v,
            &[[1.0, 2.0, 3.0], [0.0, 0.0, 30.0]],
            9.0,
            [1.0; 4],
            &mut out,
        );
        assert_eq!(out.len(), 6, "only the point inside the box");
        assert!(out.iter().all(|c| c.round == 1.0 && c.a == c.b));
        out.clear();
        // A helix rising out of the top of the box: its far end is left out.
        curve_3d(
            &v,
            &|t| t.cos(),
            &|t| t.sin(),
            &|t| t,
            (0.0, 100.0),
            2.5,
            [1.0; 4],
            &mut out,
        );
        let tops = out
            .iter()
            .map(|c| c.a[2].max(c.b[2]))
            .fold(f32::MIN, f32::max);
        assert!(tops <= HEIGHT_LIMIT as f32 && !out.is_empty());
    }

    #[test]
    fn implicit_sphere_lies_on_its_radius() {
        let v = View3::new(600.0, 600.0);
        let sphere = |x: f64, y: f64, z: f64| x * x + y * y + z * z - 25.0;
        let mesh = implicit_mesh(&v, &sphere, [1.0; 4], 24);
        assert!(mesh.indices.len() > 300, "{} indices", mesh.indices.len());
        for p in &mesh.vertices {
            // Back to graph units: the box is ±10 on every axis.
            let r = p.pos.iter().map(|c| (c * 10.0).powi(2)).sum::<f32>().sqrt();
            assert!((r - 5.0).abs() < 0.2, "a vertex at radius {r}");
            // The normal points along the radius, one way or the other.
            let along = p
                .pos
                .iter()
                .zip(p.normal)
                .map(|(c, n)| c * 10.0 / r * n)
                .sum::<f32>();
            assert!(along.abs() > 0.95, "normal {:?} at {:?}", p.normal, p.pos);
        }
    }

    #[test]
    fn solid_below_a_plane_is_closed_by_the_walls() {
        let v = View3::new(600.0, 600.0);
        let below = |_: f64, _: f64, z: f64| z - 2.0;
        let mesh = solid_mesh(&v, &below, [1.0; 4], 20);
        let on_top = |p: &Vertex3| (p.pos[2] - 0.2).abs() < 0.02;
        let on_wall = |p: &Vertex3| p.pos.iter().any(|c| (c.abs() - 1.0).abs() < 0.01);
        assert!(
            mesh.vertices.iter().any(on_top),
            "the plane z = 2 is its top"
        );
        assert!(
            mesh.vertices.iter().all(|p| on_top(p) || on_wall(p)),
            "everything else lies on the walls"
        );
        assert!(
            mesh.vertices.iter().any(|p| p.pos[2] < -0.98),
            "the floor closes it"
        );
    }

    #[test]
    fn polygon_fill_is_a_fan_facing_its_normal() {
        let v = View3::new(600.0, 600.0);
        let square = [
            [0.0, 0.0, 5.0],
            [5.0, 0.0, 5.0],
            [5.0, 5.0, 5.0],
            [0.0, 5.0, 5.0],
        ];
        let mesh = polygon_mesh(&v, &square, [1.0; 4]);
        assert_eq!(mesh.indices, vec![0, 1, 2, 0, 2, 3]);
        assert_eq!(mesh.vertices[0].normal, [0.0, 0.0, 1.0]);
        assert_eq!(mesh.vertices[2].pos, [0.5, 0.5, 0.5]);
        let mut lines = Vec::new();
        polygon_outline_3d(&v, &square, 2.0, [1.0; 4], &mut lines);
        assert_eq!(lines.len(), 4 * 12, "a side and a joint at each corner");
    }

    #[test]
    fn box_mesh_is_cut_to_the_view() {
        let v = View3::new(600.0, 600.0);
        let m = box_mesh(
            &v,
            [[0.0, 5.0], [-20.0, 5.0], [0.0, f64::INFINITY]],
            [1.0; 4],
        );
        assert_eq!((m.vertices.len(), m.indices.len()), (24, 36));
        let lo = m
            .vertices
            .iter()
            .fold([9f32; 3], |a, p| [0, 1, 2].map(|i| a[i].min(p.pos[i])));
        let hi = m
            .vertices
            .iter()
            .fold([-9f32; 3], |a, p| [0, 1, 2].map(|i| a[i].max(p.pos[i])));
        assert_eq!((lo, hi), ([0.0, -1.0, 0.0], [0.5, 0.5, 1.0]));
        let empty = box_mesh(&v, [[1.0, 1.0], [0.0, 1.0], [0.0, 1.0]], [1.0; 4]);
        assert!(empty.vertices.is_empty());
    }

    #[test]
    fn parametric_cylinder_wraps_around() {
        let v = View3::new(600.0, 600.0);
        let tau = std::f64::consts::TAU;
        let f = |u: f64, w: f64| [5.0 * (tau * u).cos(), 5.0 * (tau * u).sin(), 10.0 * w - 5.0];
        let mesh = parametric_mesh(&v, &f, (0.0, 1.0), (0.0, 1.0), [1.0; 4], 12);
        assert_eq!(mesh.vertices.len(), 13 * 13);
        assert_eq!(mesh.indices.len(), 12 * 12 * 6);
        for p in &mesh.vertices {
            let r = (p.pos[0].powi(2) + p.pos[1].powi(2)).sqrt();
            assert!((r - 0.5).abs() < 1e-5);
            assert!(p.normal[2].abs() < 1e-3, "normals point out from the axis");
        }
    }

    #[test]
    fn frame_has_edges_grid_and_axes() {
        let v = View3::new(600.0, 600.0);
        let lines = box_lines(&v, 1.0);
        // 12 edges, 5 grid lines each way at step 5 (-10 to 10), 3 axes.
        assert_eq!(lines.len(), (12 + 10 + 3) * 6);
        let mut text = TextAtlas::system();
        let labels = box_labels(&v, 1.0, &mut text);
        assert!(!text.has_font() || !labels.is_empty());
    }
}
