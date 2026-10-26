use crate::Viewport;

/// Deepest bisection of a one-pixel step (1/1024 px), used near steep parts
/// and asymptotes.
const MAX_DEPTH: u32 = 10;
/// Segments steeper than this on screen get bisected.
const MAX_STEP_PX: f64 = 1.0;
/// Keeps far-off-screen values from overflowing f32 vertex positions.
const SCREEN_CLAMP_PX: f64 = 1.0e5;

/// Samples y = f(x) across the viewport and returns screen-space polylines.
///
/// Starts with one sample per pixel column, bisects wherever the curve moves
/// more than a pixel vertically, and breaks the line at undefined points and
/// at jumps that survive full refinement (for example tan x or floor x), so
/// asymptotes are not joined by false vertical strokes.
pub fn sample_explicit(f: &dyn Fn(f64) -> f64, vp: &Viewport) -> Vec<Vec<[f32; 2]>> {
    let mut s = Sampler {
        f,
        vp,
        lines: Vec::new(),
        current: Vec::new(),
    };
    let columns = vp.width_px.ceil().max(1.0) as usize;
    let mut prev = s.eval_at_px(0.0);
    for i in 1..=columns {
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
    lines: Vec<Vec<[f32; 2]>>,
    current: Vec<[f32; 2]>,
}

#[derive(Clone, Copy)]
struct Sample {
    x: f64,
    y: f64,
    /// Screen y, meaningful only when `y` is finite.
    sy: f64,
}

impl Sample {
    fn ok(&self) -> bool {
        self.y.is_finite()
    }
}

impl Sampler<'_> {
    fn eval_at_px(&self, px: f64) -> Sample {
        self.eval(self.vp.to_world(px, 0.0)[0])
    }

    fn eval(&self, x: f64) -> Sample {
        let y = (self.f)(x);
        let sy = self.vp.to_screen(x, y)[1];
        Sample { x, y, sy }
    }

    fn screen(&self, s: Sample) -> [f32; 2] {
        let sx = self.vp.to_screen(s.x, 0.0)[0];
        [
            sx as f32,
            s.sy.clamp(-SCREEN_CLAMP_PX, SCREEN_CLAMP_PX) as f32,
        ]
    }

    fn segment(&mut self, a: Sample, b: Sample, depth: u32) {
        if !a.ok() && !b.ok() {
            return;
        }
        let h = self.vp.height_px;
        let both_ok = a.ok() && b.ok();
        // Off the same edge of the screen: nothing visible to refine.
        let hidden = both_ok && ((a.sy < 0.0 && b.sy < 0.0) || (a.sy > h && b.sy > h));
        if hidden {
            self.extend(a);
            self.extend(b);
            return;
        }
        let steep = both_ok && (b.sy - a.sy).abs() > MAX_STEP_PX;
        if depth < MAX_DEPTH && (steep || a.ok() != b.ok()) {
            let mid = self.eval((a.x + b.x) / 2.0);
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
        if (b.sy - a.sy).abs() > h {
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
            self.lines.push(std::mem::take(&mut self.current));
        } else {
            self.current.clear();
        }
    }
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
    fn sqrt_starts_at_origin() {
        let vp = Viewport::new(1000.0, 600.0);
        let lines = sample_explicit(&f64::sqrt, &vp);
        assert_eq!(lines.len(), 1);
        let start = lines[0][0];
        // Bisection homes in on x = 0 (screen x 500) from the undefined side.
        assert!((start[0] - 500.0).abs() < 0.01, "start = {start:?}");
    }
}
