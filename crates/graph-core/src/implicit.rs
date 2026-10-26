//! Implicit curves: everything of the form f(x, y) = 0 that is not a
//! function of x or of y alone.
//!
//! The view is split into a quadtree. Interval arithmetic proves which cells
//! cannot contain the curve, and those are thrown away whole; what remains is
//! subdivided to pixel size and traced with marching squares, so thin loops
//! and near-tangent branches are never skipped the way plain sampling skips
//! them.

use crate::Viewport;
use std::collections::HashMap;

/// A relation's left side minus its right, over the plane.
pub trait Field: Sync {
    fn value(&self, x: f64, y: f64) -> f64;

    /// A range that is guaranteed to contain every value the field takes on
    /// the rectangle. `None` means "could be anything", which is always safe
    /// and only costs subdivision.
    fn range(&self, x: (f64, f64), y: (f64, f64)) -> Option<(f64, f64)>;
}

impl<F: Fn(f64, f64) -> f64 + Sync> Field for F {
    fn value(&self, x: f64, y: f64) -> f64 {
        self(x, y)
    }

    fn range(&self, _x: (f64, f64), _y: (f64, f64)) -> Option<(f64, f64)> {
        None
    }
}

/// Side length, in pixels, at which a cell stops being subdivided and is
/// traced. One pixel is as fine as the screen can show; curvature inside a
/// cell is below what MSAA resolves.
const LEAF_PX: f64 = 1.0;
/// Side length of the first cells, in pixels. Large enough that interval
/// arithmetic can throw away most of an empty view in a few tests.
const ROOT_PX: f64 = 64.0;
/// Bisection steps used to place a crossing along a cell edge.
const REFINE_STEPS: u32 = 8;
/// How far a simplified line may stray from the traced one, in pixels.
const SIMPLIFY_PX: f32 = 0.15;
/// Segment ends closer together than this, in pixels, are the same point.
/// Wider than the error in a crossing, narrower than a leaf cell.
const WELD_PX: f32 = 0.1;

/// Traces f(x, y) = 0 across the viewport, returning screen-space polylines.
pub fn trace_implicit(field: &dyn Field, vp: &Viewport) -> Vec<Vec<[f32; 2]>> {
    let mut tracer = Tracer {
        field,
        vp,
        leaf: vp.units_per_px * LEAF_PX,
        segments: Vec::new(),
    };
    let (x0, x1) = vp.x_range();
    let (y0, y1) = vp.y_range();
    let root = vp.units_per_px * ROOT_PX;
    let cells_x = ((x1 - x0) / root).ceil().max(1.0) as i64;
    let cells_y = ((y1 - y0) / root).ceil().max(1.0) as i64;
    // Whole root cells, so the grid does not shift as the view is panned.
    let column = |i: i64, tracer: &mut Tracer| {
        let (ax, bx) = (x0 + i as f64 * root, x0 + (i + 1) as f64 * root);
        for j in 0..cells_y {
            let (ay, by) = (y0 + j as f64 * root, y0 + (j + 1) as f64 * root);
            let corners = [
                field.value(ax, ay),
                field.value(bx, ay),
                field.value(ax, by),
                field.value(bx, by),
            ];
            tracer.cell(ax, bx, ay, by, corners);
        }
    };
    let threads = std::thread::available_parallelism()
        .map_or(1, |n| n.get())
        .min(cells_x.max(1) as usize);
    let segments = if threads > 1 {
        // Columns of root cells are independent, so they go out to a
        // thread each and their segments are joined afterwards.
        std::thread::scope(|scope| {
            let jobs: Vec<_> = (0..threads)
                .map(|t| {
                    let column = &column;
                    scope.spawn(move || {
                        let mut local = Tracer {
                            field,
                            vp,
                            leaf: vp.units_per_px * LEAF_PX,
                            segments: Vec::new(),
                        };
                        for i in (t as i64..cells_x).step_by(threads) {
                            column(i, &mut local);
                        }
                        local.segments
                    })
                })
                .collect();
            jobs.into_iter()
                .flat_map(|j| j.join().unwrap_or_default())
                .collect()
        })
    } else {
        for i in 0..cells_x {
            column(i, &mut tracer);
        }
        tracer.segments
    };
    join(segments)
        .into_iter()
        .map(|line| crate::sample::simplify(&line, SIMPLIFY_PX))
        .collect()
}

struct Tracer<'a> {
    field: &'a dyn Field,
    vp: &'a Viewport,
    /// Cell side, in world units, at which to stop subdividing.
    leaf: f64,
    segments: Vec<[[f32; 2]; 2]>,
}

/// Corner values, in the order (x0,y0), (x1,y0), (x0,y1), (x1,y1).
type Corners = [f64; 4];

impl Tracer<'_> {
    fn cell(&mut self, x0: f64, x1: f64, y0: f64, y1: f64, c: Corners) {
        if let Some((lo, hi)) = self.field.range((x0, x1), (y0, y1)) {
            if lo > 0.0 || hi < 0.0 {
                return; // Proven empty.
            }
        }
        let defined = c.iter().filter(|v| v.is_finite()).count();
        if x1 - x0 <= self.leaf {
            if defined == 4 {
                self.trace_leaf(x0, x1, y0, y1, c);
            }
            return;
        }
        // All four corners on the same side, with no interval to contradict
        // it, still needs splitting when the field is undefined anywhere in
        // the cell or the cell is big enough to hide a loop.
        let (mx, my) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
        let f = |x, y| self.field.value(x, y);
        let (n, s) = (f(mx, y0), f(mx, y1));
        let (w, e) = (f(x0, my), f(x1, my));
        let m = f(mx, my);
        self.cell(x0, mx, y0, my, [c[0], n, w, m]);
        self.cell(mx, x1, y0, my, [n, c[1], m, e]);
        self.cell(x0, mx, my, y1, [w, m, c[2], s]);
        self.cell(mx, x1, my, y1, [m, e, s, c[3]]);
    }

    /// Marching squares over one leaf cell.
    fn trace_leaf(&mut self, x0: f64, x1: f64, y0: f64, y1: f64, c: Corners) {
        let bit = |v: f64| if v >= 0.0 { 1u8 } else { 0 };
        let case = bit(c[0]) | bit(c[1]) << 1 | bit(c[2]) << 2 | bit(c[3]) << 3;
        if case == 0 || case == 15 {
            return;
        }
        // Crossings on the four edges, named by the side they sit on. Only
        // the sides this case uses are worked out.
        let bottom = self.on_x(x0, x1, y0, c[0], c[1]);
        let top = self.on_x(x0, x1, y1, c[2], c[3]);
        let left = self.on_y(x0, y0, y1, c[0], c[2]);
        let right = self.on_y(x1, y0, y1, c[1], c[3]);
        match case {
            1 | 14 => self.segment(left, bottom),
            2 | 13 => self.segment(bottom, right),
            3 | 12 => self.segment(left, right),
            4 | 11 => self.segment(left, top),
            5 | 10 => self.segment(bottom, top),
            7 | 8 => self.segment(top, right),
            // Saddles: the two corners of one diagonal are on one side of
            // zero and the other diagonal's on the other, so the centre
            // decides which pair of corners each branch cuts off.
            6 | 9 => {
                let centre = self.field.value((x0 + x1) / 2.0, (y0 + y1) / 2.0);
                // Case 9 has its positive corners on the (x0,y0)-(x1,y1)
                // diagonal. The corners cut off are the positive pair when
                // the centre is negative, and the negative pair otherwise.
                let cut_off_main_diagonal = (centre >= 0.0) != (case == 9);
                if cut_off_main_diagonal {
                    self.segment(left, bottom);
                    self.segment(top, right);
                } else {
                    self.segment(bottom, right);
                    self.segment(left, top);
                }
            }
            _ => {}
        }
    }

    /// The crossing on a horizontal edge at height `y`.
    fn on_x(&self, x0: f64, x1: f64, y: f64, a: f64, b: f64) -> [f32; 2] {
        let x = self.refine(a, b, |t| self.field.value(x0 + (x1 - x0) * t, y));
        self.screen(x0 + (x1 - x0) * x, y)
    }

    /// The crossing on a vertical edge at `x`.
    fn on_y(&self, x: f64, y0: f64, y1: f64, a: f64, b: f64) -> [f32; 2] {
        let t = self.refine(a, b, |t| self.field.value(x, y0 + (y1 - y0) * t));
        self.screen(x, y0 + (y1 - y0) * t)
    }

    /// Where the field crosses zero along an edge, as a fraction of it:
    /// linear interpolation from the corner values, then bisection.
    fn refine(&self, a: f64, b: f64, at: impl Fn(f64) -> f64) -> f64 {
        let mut lo = 0.0;
        let mut hi = 1.0;
        let (mut flo, mut fhi) = (a, b);
        let mut guess = if (b - a).abs() > f64::MIN_POSITIVE {
            (a / (a - b)).clamp(0.0, 1.0)
        } else {
            0.5
        };
        for _ in 0..REFINE_STEPS {
            let v = at(guess);
            if !v.is_finite() {
                break;
            }
            if (v >= 0.0) == (flo >= 0.0) {
                lo = guess;
                flo = v;
            } else {
                hi = guess;
                fhi = v;
            }
            guess = if (fhi - flo).abs() > f64::MIN_POSITIVE {
                let secant = lo + (hi - lo) * (flo / (flo - fhi));
                // Keep the step inside the bracket, so it cannot run away.
                secant.clamp(lo + (hi - lo) * 0.05, hi - (hi - lo) * 0.05)
            } else {
                (lo + hi) / 2.0
            };
        }
        guess
    }

    fn screen(&self, x: f64, y: f64) -> [f32; 2] {
        let p = self.vp.to_screen(x, y);
        [p[0] as f32, p[1] as f32]
    }

    fn segment(&mut self, a: [f32; 2], b: [f32; 2]) {
        // Segments shorter than a weld are slivers cut where the curve
        // grazes a cell corner; joining welds the gap they leave behind.
        if (a[0] - b[0]).hypot(a[1] - b[1]) > WELD_PX / 2.0 {
            self.segments.push([a, b]);
        }
    }
}

/// Chains the loose segments into as few polylines as possible, so curves
/// stroke with proper joins and dashes run along them.
///
/// Neighbouring cells compute the crossing on their shared edge identically,
/// so most ends meet exactly; ends that only nearly meet (where the curve
/// grazes a cell corner) are welded within [`WELD_PX`].
fn join(segments: Vec<[[f32; 2]; 2]>) -> Vec<Vec<[f32; 2]>> {
    // Segment ends are indexed by a grid one weld wide, so a tip only has to
    // look at the nine buckets around it.
    let bucket = |p: [f32; 2]| {
        [
            (p[0] / WELD_PX).floor() as i64,
            (p[1] / WELD_PX).floor() as i64,
        ]
    };
    let mut ends: HashMap<[i64; 2], Vec<usize>> = HashMap::new();
    for (i, s) in segments.iter().enumerate() {
        for p in s {
            ends.entry(bucket(*p)).or_default().push(i);
        }
    }
    let near = |a: [f32; 2], b: [f32; 2]| (a[0] - b[0]).hypot(a[1] - b[1]) <= WELD_PX;
    let mut used = vec![false; segments.len()];
    let mut lines = Vec::new();
    for start in 0..segments.len() {
        if used[start] {
            continue;
        }
        used[start] = true;
        let mut line = vec![segments[start][0], segments[start][1]];
        // Grow from the end, then from the start.
        for from_end in [true, false] {
            loop {
                let tip = if from_end {
                    *line.last().unwrap()
                } else {
                    line[0]
                };
                let home = bucket(tip);
                let next = (-1..=1)
                    .flat_map(|dx| (-1..=1).map(move |dy| [home[0] + dx, home[1] + dy]))
                    .filter_map(|b| ends.get(&b))
                    .flatten()
                    .copied()
                    .find(|&i| {
                        !used[i] && (near(segments[i][0], tip) || near(segments[i][1], tip))
                    });
                let Some(i) = next else { break };
                used[i] = true;
                let s = segments[i];
                let far = if near(s[0], tip) { s[1] } else { s[0] };
                if from_end {
                    line.push(far);
                } else {
                    line.insert(0, far);
                }
            }
        }
        lines.push(line);
    }
    // Anything left under a weld long is a duplicate of a point the chain
    // already passes through, too short to draw.
    lines.retain(|l| l.len() > 2 || (l[0][0] - l[1][0]).hypot(l[0][1] - l[1][1]) > WELD_PX);
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A field with no interval information, the worst case for pruning.
    struct Plain<F>(F);
    impl<F: Fn(f64, f64) -> f64 + Sync> Field for Plain<F> {
        fn value(&self, x: f64, y: f64) -> f64 {
            (self.0)(x, y)
        }
        fn range(&self, _x: (f64, f64), _y: (f64, f64)) -> Option<(f64, f64)> {
            None
        }
    }

    fn circle(r: f64) -> impl Fn(f64, f64) -> f64 {
        move |x, y| x * x + y * y - r * r
    }

    #[test]
    fn a_circle_comes_back_as_one_closed_loop() {
        let vp = Viewport::new(600.0, 600.0);
        let lines = trace_implicit(&Plain(circle(5.0)), &vp);
        assert_eq!(lines.len(), 1, "one loop, not {} pieces", lines.len());
        let line = &lines[0];
        // A 150 px circle needs about 60 points to stay within the 0.2 px
        // simplification tolerance.
        assert!(line.len() > 40, "{} points", line.len());
        // Every point is on the circle, to within a fraction of a pixel.
        for p in line {
            let w = vp.to_world(p[0] as f64, p[1] as f64);
            let r = (w[0] * w[0] + w[1] * w[1]).sqrt();
            assert!((r - 5.0).abs() < 0.02, "radius {r} at {p:?}");
        }
        // It closes: the two ends meet.
        let (a, b) = (line[0], *line.last().unwrap());
        assert!((a[0] - b[0]).hypot(a[1] - b[1]) < 1.0, "{a:?} to {b:?}");
    }

    #[test]
    fn hyperbola_gives_two_branches() {
        let vp = Viewport::new(600.0, 600.0);
        let lines = trace_implicit(&Plain(|x: f64, y: f64| x * x - y * y - 1.0), &vp);
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn intervals_prune_without_losing_the_curve() {
        // The same circle, with a sound interval bound supplied.
        struct Bounded;
        impl Field for Bounded {
            fn value(&self, x: f64, y: f64) -> f64 {
                x * x + y * y - 25.0
            }
            fn range(&self, x: (f64, f64), y: (f64, f64)) -> Option<(f64, f64)> {
                let sq = |(a, b): (f64, f64)| {
                    if a <= 0.0 && b >= 0.0 {
                        (0.0, (a * a).max(b * b))
                    } else {
                        ((a * a).min(b * b), (a * a).max(b * b))
                    }
                };
                let (xl, xh) = sq(x);
                let (yl, yh) = sq(y);
                Some((xl + yl - 25.0, xh + yh - 25.0))
            }
        }
        let vp = Viewport::new(600.0, 600.0);
        let pruned = trace_implicit(&Bounded, &vp);
        let plain = trace_implicit(&Plain(circle(5.0)), &vp);
        assert_eq!(pruned.len(), 1);
        let count = |ls: &[Vec<[f32; 2]>]| ls.iter().map(Vec::len).sum::<usize>();
        let (a, b) = (count(&pruned), count(&plain));
        assert!(a.abs_diff(b) * 20 < b, "{a} points pruned vs {b} plain");
    }

    #[test]
    fn a_tiny_loop_is_not_missed() {
        // A circle of radius a third of a pixel at this zoom.
        let vp = Viewport::new(600.0, 600.0);
        let r = vp.units_per_px / 3.0;
        let lines = trace_implicit(&Plain(move |x: f64, y: f64| x * x + y * y - r * r), &vp);
        assert!(!lines.is_empty(), "the loop was skipped");
    }

    #[test]
    fn an_empty_relation_draws_nothing() {
        let vp = Viewport::new(600.0, 600.0);
        assert!(trace_implicit(&Plain(|x: f64, y: f64| x * x + y * y + 1.0), &vp).is_empty());
    }
}
