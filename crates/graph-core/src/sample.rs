use crate::Viewport;

/// Deepest bisection of a one-pixel step (1/1024 px), used near steep parts
/// and asymptotes.
const MAX_DEPTH: u32 = 10;
/// Segments steeper than this on screen get bisected.
const MAX_STEP_PX: f64 = 1.0;
/// Keeps far-off-screen values from overflowing f32 vertex positions.
const SCREEN_CLAMP_PX: f64 = 1.0e5;
/// A jump bigger than this that survives full refinement is a discontinuity
/// (floor, round, mod, an asymptote), so the line breaks instead of joining.
const JUMP_PX: f64 = 8.0;
/// Points closer than this to the line through their neighbours are dropped.
const SIMPLIFY_PX: f32 = 0.2;

/// Which world variable a curve is a function of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    /// y = f(x), sampled along screen columns.
    X,
    /// x = f(y), sampled along screen rows.
    Y,
}

/// Samples y = f(x) across the viewport and returns screen-space polylines.
///
/// Starts with one sample per pixel column, bisects wherever the curve moves
/// more than a pixel vertically, and breaks the line at undefined points and
/// at jumps that survive full refinement (for example tan x or floor x), so
/// asymptotes are not joined by false vertical strokes.
pub fn sample_explicit(f: &dyn Fn(f64) -> f64, vp: &Viewport) -> Vec<Vec<[f32; 2]>> {
    sample_curve(f, vp, Axis::X)
}

/// Samples a curve that is a function of x or of y. For `Axis::Y` the same
/// algorithm runs along screen rows instead of columns.
pub fn sample_curve(f: &dyn Fn(f64) -> f64, vp: &Viewport, axis: Axis) -> Vec<Vec<[f32; 2]>> {
    let steps = match axis {
        Axis::X => vp.width_px,
        Axis::Y => vp.height_px,
    };
    let mut s = Sampler {
        f,
        vp,
        axis,
        lines: Vec::new(),
        current: Vec::new(),
    };
    let steps = steps.ceil().max(1.0) as usize;
    let mut prev = s.eval_at_px(0.0);
    for i in 1..=steps {
        let next = s.eval_at_px(i as f64);
        s.segment(prev, next, 0);
        prev = next;
    }
    s.flush();
    s.lines
}

struct Sampler<'a> {
    f: &'a dyn Fn(f64) -> f64,
    vp: &'a Viewport,
    axis: Axis,
    lines: Vec<Vec<[f32; 2]>>,
    current: Vec<[f32; 2]>,
}

/// One evaluation: `t` is the input (x for `Axis::X`), `v` the output, and
/// `sv` the output's screen coordinate.
#[derive(Clone, Copy)]
struct Sample {
    t: f64,
    v: f64,
    /// Screen coordinate of `v`, meaningful only when `v` is finite.
    sv: f64,
}

impl Sample {
    fn ok(&self) -> bool {
        self.v.is_finite()
    }
}

impl Sampler<'_> {
    /// Screen extent along the output direction.
    fn value_extent(&self) -> f64 {
        match self.axis {
            Axis::X => self.vp.height_px,
            Axis::Y => self.vp.width_px,
        }
    }

    fn eval_at_px(&self, px: f64) -> Sample {
        let t = match self.axis {
            Axis::X => self.vp.to_world(px, 0.0)[0],
            Axis::Y => self.vp.to_world(0.0, px)[1],
        };
        self.eval(t)
    }

    fn eval(&self, t: f64) -> Sample {
        let v = (self.f)(t);
        let sv = match self.axis {
            Axis::X => self.vp.to_screen(t, v)[1],
            Axis::Y => self.vp.to_screen(v, t)[0],
        };
        Sample { t, v, sv }
    }

    fn screen(&self, s: Sample) -> [f32; 2] {
        let sv = s.sv.clamp(-SCREEN_CLAMP_PX, SCREEN_CLAMP_PX) as f32;
        match self.axis {
            Axis::X => [self.vp.to_screen(s.t, 0.0)[0] as f32, sv],
            Axis::Y => [sv, self.vp.to_screen(0.0, s.t)[1] as f32],
        }
    }

    fn segment(&mut self, a: Sample, b: Sample, depth: u32) {
        if !a.ok() && !b.ok() {
            return;
        }
        let h = self.value_extent();
        let both_ok = a.ok() && b.ok();
        // Off the same edge of the screen: nothing visible to refine.
        let hidden = both_ok && ((a.sv < 0.0 && b.sv < 0.0) || (a.sv > h && b.sv > h));
        if hidden {
            self.extend(a);
            self.extend(b);
            return;
        }
        let steep = both_ok && (b.sv - a.sv).abs() > MAX_STEP_PX;
        if depth < MAX_DEPTH && (steep || a.ok() != b.ok()) {
            let mid = self.eval((a.t + b.t) / 2.0);
            self.segment(a, mid, depth + 1);
            self.segment(mid, b, depth + 1);
            return;
        }
        if !b.ok() {
            // The curve stops being defined here: end the line at `a`.
            self.extend(a);
            self.flush();
            return;
        }
        if !a.ok() {
            self.flush();
            self.extend(b);
            return;
        }
        if (b.sv - a.sv).abs() > JUMP_PX {
            // Still a huge jump after full refinement: a discontinuity.
            self.extend(a);
            self.flush();
            self.extend(b);
            return;
        }
        self.extend(a);
        self.extend(b);
    }

    fn extend(&mut self, s: Sample) {
        let p = self.screen(s);
        if self.current.last() != Some(&p) {
            self.current.push(p);
        }
    }

    fn flush(&mut self) {
        if self.current.len() >= 2 {
            let line = simplify(&self.current, SIMPLIFY_PX);
            self.lines.push(line);
        }
        self.current.clear();
    }
}

/// Ramer-Douglas-Peucker: keeps only the points needed to stay within
/// `tolerance` pixels of the sampled line. Smooth curves sampled once per
/// pixel shrink to a small fraction of their points.
fn simplify(points: &[[f32; 2]], tolerance: f32) -> Vec<[f32; 2]> {
    let n = points.len();
    if n <= 2 {
        return points.to_vec();
    }
    let mut keep = vec![false; n];
    keep[0] = true;
    keep[n - 1] = true;
    let mut stack = vec![(0usize, n - 1)];
    while let Some((first, last)) = stack.pop() {
        if last <= first + 1 {
            continue;
        }
        let (a, b) = (points[first], points[last]);
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let len = (dx * dx + dy * dy).sqrt();
        let mut worst = (0.0f32, first);
        for (i, p) in points.iter().enumerate().take(last).skip(first + 1) {
            let d = if len <= f32::EPSILON {
                ((p[0] - a[0]).powi(2) + (p[1] - a[1]).powi(2)).sqrt()
            } else {
                ((p[0] - a[0]) * dy - (p[1] - a[1]) * dx).abs() / len
            };
            if d > worst.0 {
                worst = (d, i);
            }
        }
        if worst.0 > tolerance {
            keep[worst.1] = true;
            stack.push((first, worst.1));
            stack.push((worst.1, last));
        }
    }
    points
        .iter()
        .zip(keep)
        .filter_map(|(p, k)| k.then_some(*p))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sine_is_one_continuous_line() {
        let vp = Viewport::new(1000.0, 600.0);
        let lines = sample_explicit(&f64::sin, &vp);
        assert_eq!(lines.len(), 1);
        let line = &lines[0];
        assert!(line.first().unwrap()[0] <= 0.5);
        assert!(line.last().unwrap()[0] >= 999.5);
        // y = 1 sits 50 px above the centre line at this zoom.
        let top = line.iter().map(|p| p[1]).fold(f32::INFINITY, f32::min);
        assert!((top - 250.0).abs() < 0.5, "top = {top}");
    }

    #[test]
    fn tangent_breaks_at_asymptotes() {
        let vp = Viewport::new(1000.0, 600.0);
        let lines = sample_explicit(&f64::tan, &vp);
        // tan has asymptotes at ±π/2, ±3π/2, ±5π/2 inside [-10, 10]: 7 branches.
        assert_eq!(lines.len(), 7);
    }

    #[test]
    fn x_as_function_of_y_runs_down_the_rows() {
        let vp = Viewport::new(1000.0, 600.0);
        // x = y^2: at y = 2 (row 200), x = 4 (column 700).
        let lines = sample_curve(&|y| y * y, &vp, Axis::Y);
        assert_eq!(lines.len(), 1);
        let w = lines[0]
            .windows(2)
            .find(|w| w[0][1] <= 200.0 && w[1][1] >= 200.0)
            .expect("a segment crossing row 200");
        let t = (200.0 - w[0][1]) / (w[1][1] - w[0][1]);
        let x = w[0][0] + t * (w[1][0] - w[0][0]);
        assert!((x - 700.0).abs() < 0.5, "got {x}");
    }

    #[test]
    fn floor_steps_are_not_joined() {
        let vp = Viewport::new(1000.0, 600.0);
        let lines = sample_explicit(&f64::floor, &vp);
        // Every step on screen is its own flat piece, with no vertical risers.
        assert!(lines.len() >= 13);
        for line in &lines {
            for w in line.windows(2) {
                let on_screen = w.iter().all(|p| (0.0..=600.0).contains(&p[1]));
                assert!(!on_screen || (w[0][1] - w[1][1]).abs() < 1.0, "riser {w:?}");
            }
        }
    }

    #[test]
    fn simplification_keeps_curves_accurate() {
        let vp = Viewport::new(1000.0, 600.0);
        let lines = sample_explicit(&f64::sin, &vp);
        assert!(lines[0].len() < 300, "{} points", lines[0].len());
        for w in lines[0].windows(2) {
            // The midpoint of every kept segment is within tolerance of sin.
            let mx = (w[0][0] + w[1][0]) / 2.0;
            let my = (w[0][1] + w[1][1]) / 2.0;
            let x = vp.to_world(mx as f64, 0.0)[0];
            let exact = vp.to_screen(x, x.sin())[1] as f32;
            assert!((my - exact).abs() < 0.5, "at {mx}: {my} vs {exact}");
        }
    }

    #[test]
    fn sqrt_starts_at_origin() {
        let vp = Viewport::new(1000.0, 600.0);
        let lines = sample_explicit(&f64::sqrt, &vp);
        assert_eq!(lines.len(), 1);
        let start = lines[0][0];
        // Bisection homes in on x = 0 (screen x 500) from the undefined side.
        assert!((start[0] - 500.0).abs() < 0.01, "start = {start:?}");
    }
}
