//! Differential equations: the slope or direction field drawn over the
//! view, and solution curves through chosen starting points, integrated
//! with an adaptive Runge-Kutta method (Dormand-Prince 5(4)).
//!
//! Three kinds are drawn:
//! - `y⁽ⁿ⁾ = f(x, y, y', …, y⁽ⁿ⁻¹⁾)`, of any order;
//! - `F(x, y, y', …, y⁽ⁿ⁾) = 0`, not solved for its highest derivative,
//!   which is found numerically wherever it is needed, one curve for each
//!   root at the starting point;
//! - systems `x' = f(t, x, y)`, `y' = g(t, x, y)`, drawn as trajectories in
//!   the plane starting at t = 0.

use crate::grid::{grid_spacing, line_indices};
use crate::Viewport;

/// Logical pixels aimed for between neighbouring slope marks.
pub const FIELD_SPACING_LOGICAL: f64 = 28.0;
/// A slope mark's length as a share of the spacing between marks.
const MARK_LENGTH: f64 = 0.6;
/// A direction arrow's head, as a share of the arrow's length.
const HEAD_LENGTH: f64 = 0.3;

/// Highest order of equation drawn.
pub const MAX_ORDER: usize = 6;
/// Most roots followed at one starting point of an implicit equation.
const MAX_ROOTS: usize = 4;

type Fn1<'a> = &'a (dyn Fn(f64, &[f64]) -> f64 + Sync);
type Fn3<'a> = &'a (dyn Fn(f64, f64, f64) -> f64 + Sync);

/// A differential equation to draw.
#[derive(Clone, Copy)]
pub enum Equation<'a> {
    /// `y⁽ⁿ⁾ = f(x, [y, y', …, y⁽ⁿ⁻¹⁾])`.
    Explicit { order: usize, f: Fn1<'a> },
    /// `F(x, [y, y', …, y⁽ⁿ⁾]) = 0`.
    Implicit { order: usize, f: Fn1<'a> },
    /// `x' = f(t, x, y)`, `y' = g(t, x, y)`. The field is drawn only when
    /// the system is `autonomous` (does not read t), since it would
    /// otherwise change along every trajectory.
    System {
        f: Fn3<'a>,
        g: Fn3<'a>,
        autonomous: bool,
    },
}

impl Equation<'_> {
    /// How many numbers a starting point needs: x and y, then for an
    /// equation of order n the derivatives y' to y⁽ⁿ⁻¹⁾.
    pub fn start_len(&self) -> usize {
        match *self {
            Equation::Explicit { order, .. } | Equation::Implicit { order, .. } => order + 1,
            Equation::System { .. } => 2,
        }
    }
}

/// The marks of the field over the view, in world coordinates: short
/// segments showing the slope of a first-order equation (one per root for
/// an implicit one), or arrows showing which way an autonomous system
/// flows. Higher-order equations have no field in the plane. Marks sit on a
/// 1-2-5 lattice so they line up with the grid and stay put while the view
/// pans.
pub fn field_marks(eq: &Equation, vp: &Viewport, scale: f64) -> Vec<[[f64; 2]; 2]> {
    let step = grid_spacing(vp.units_per_px, FIELD_SPACING_LOGICAL * scale.max(0.5)).major;
    let half = step * MARK_LENGTH / 2.0;
    let (x0, x1) = vp.x_range();
    let (y0, y1) = vp.y_range();
    let mut marks = Vec::new();
    let slope_mark = |x: f64, y: f64, m: f64, marks: &mut Vec<[[f64; 2]; 2]>| {
        let (dx, dy) = if m.is_finite() {
            let n = m.hypot(1.0);
            (half / n, half * m / n)
        } else if m.is_infinite() {
            (0.0, half)
        } else {
            return;
        };
        marks.push([[x - dx, y - dy], [x + dx, y + dy]]);
    };
    let lattice = || {
        line_indices(x0, x1, step).flat_map(move |i| {
            line_indices(y0, y1, step).map(move |j| (i as f64 * step, j as f64 * step))
        })
    };
    match *eq {
        Equation::Explicit { order: 1, f } => {
            for (x, y) in lattice() {
                slope_mark(x, y, f(x, &[y]), &mut marks);
            }
        }
        Equation::Implicit { order: 1, f } => {
            for (x, y) in lattice() {
                for m in roots(&|m| f(x, &[y, m])) {
                    slope_mark(x, y, m, &mut marks);
                }
            }
        }
        Equation::System {
            f,
            g,
            autonomous: true,
        } => {
            for (x, y) in lattice() {
                let (u, v) = (f(0.0, x, y), g(0.0, x, y));
                let n = u.hypot(v);
                if !(n.is_finite() && n > 0.0) {
                    continue;
                }
                let (dx, dy) = (half * u / n, half * v / n);
                let tip = [x + dx, y + dy];
                marks.push([[x - dx, y - dy], tip]);
                // The head: two short strokes back from the tip.
                let (hx, hy) = (dx * 2.0 * HEAD_LENGTH, dy * 2.0 * HEAD_LENGTH);
                for side in [1.0, -1.0] {
                    let back = [
                        tip[0] - hx * 0.87 - side * hy * 0.5,
                        tip[1] - hy * 0.87 + side * hx * 0.5,
                    ];
                    marks.push([tip, back]);
                }
            }
        }
        _ => {}
    }
    marks
}

/// The real roots of `f` over all slopes, found by sampling the angle
/// whose tangent is the slope and refining each sign change. Up to
/// [`MAX_ROOTS`], in increasing order.
pub fn roots(f: &dyn Fn(f64) -> f64) -> Vec<f64> {
    const SAMPLES: usize = 64;
    let half_turn = std::f64::consts::FRAC_PI_2;
    let at =
        |i: usize| (-half_turn + (i as f64 + 0.5) * std::f64::consts::PI / SAMPLES as f64).tan();
    let mut found = Vec::new();
    let mut prev = (at(0), f(at(0)));
    for i in 1..SAMPLES {
        let m = at(i);
        let v = f(m);
        if prev.1 == 0.0 {
            found.push(prev.0);
        } else if prev.1.is_finite() && v.is_finite() && (prev.1 < 0.0) != (v < 0.0) && v != 0.0 {
            // Bisect between the two slopes.
            let (mut lo, mut hi, mut flo) = (prev.0, m, prev.1);
            for _ in 0..60 {
                let mid = 0.5 * (lo + hi);
                let fm = f(mid);
                if !fm.is_finite() {
                    break;
                }
                if (fm < 0.0) == (flo < 0.0) {
                    lo = mid;
                    flo = fm;
                } else {
                    hi = mid;
                }
            }
            let root = 0.5 * (lo + hi);
            // A sign change across a pole, as 1/m has at 0, is no root.
            if f(root).abs() < 1e-6 * (1.0 + root.abs()) {
                found.push(root);
            }
        }
        if found.len() >= MAX_ROOTS {
            break;
        }
        prev = (m, v);
    }
    if prev.1 == 0.0 && found.len() < MAX_ROOTS {
        found.push(prev.0);
    }
    found
}

/// The root of `f` nearest `guess`, by Newton's method, if it converges.
fn newton(f: &dyn Fn(f64) -> f64, guess: f64) -> Option<f64> {
    let mut m = guess;
    for _ in 0..30 {
        let v = f(m);
        if !v.is_finite() {
            return None;
        }
        let h = 1e-7 * (1.0 + m.abs());
        let d = (f(m + h) - f(m - h)) / (2.0 * h);
        if !(d.is_finite() && d != 0.0) {
            return None;
        }
        let step = v / d;
        m -= step;
        if step.abs() <= 1e-12 * (1.0 + m.abs()) {
            return Some(m);
        }
    }
    (f(m).abs() < 1e-9 * (1.0 + m.abs())).then_some(m)
}

/// The solution curves through `start` ([x, y] then any initial
/// derivatives; missing ones are 0), followed both ways until they leave
/// the view (with some room to spare), stop being defined or run off to
/// infinity. An implicit equation has one curve for each root at the
/// starting point.
pub fn solutions(eq: &Equation, start: &[f64], vp: &Viewport) -> Vec<Vec<[f64; 2]>> {
    if start.len() < 2 || !start.iter().all(|v| v.is_finite()) {
        return Vec::new();
    }
    match *eq {
        Equation::Explicit { order, f } => {
            let order = order.clamp(1, MAX_ORDER);
            let state = initial(start, order);
            let rhs = |x: f64, s: &[f64], out: &mut [f64]| {
                out[..order - 1].copy_from_slice(&s[1..order]);
                out[order - 1] = f(x, s);
                out[order - 1].is_finite()
            };
            curve_both_ways(&rhs, start[0], &state, vp)
                .into_iter()
                .collect()
        }
        Equation::Implicit { order, f } => {
            let order = order.clamp(1, MAX_ORDER);
            let state = initial(start, order);
            let x0 = start[0];
            let with = |s: &[f64], m: f64| {
                let mut all = [0.0; MAX_ORDER + 1];
                all[..order].copy_from_slice(s);
                all[order] = m;
                all
            };
            roots(&|m| f(x0, &with(&state, m)[..=order]))
                .into_iter()
                .filter_map(|root| {
                    // Each branch remembers its last root and stays on it.
                    let guess = std::cell::Cell::new(root);
                    let rhs = |x: f64, s: &[f64], out: &mut [f64]| {
                        let g = |m: f64| f(x, &with(s, m)[..=order]);
                        let Some(m) = newton(&g, guess.get()) else {
                            return false;
                        };
                        guess.set(m);
                        out[..order - 1].copy_from_slice(&s[1..order]);
                        out[order - 1] = m;
                        true
                    };
                    let line = curve_both_ways(&rhs, x0, &state, vp);
                    guess.set(root);
                    line
                })
                .collect()
        }
        Equation::System { f, g, .. } => {
            let (u, v) = (f(0.0, start[0], start[1]), g(0.0, start[0], start[1]));
            if !(u.is_finite() && v.is_finite()) {
                return Vec::new();
            }
            if u == 0.0 && v == 0.0 {
                // A resting point stays put.
                return Vec::new();
            }
            let rhs = |t: f64, s: &[f64], out: &mut [f64]| {
                out[0] = f(t, s[0], s[1]);
                out[1] = g(t, s[0], s[1]);
                out[0].is_finite() && out[1].is_finite()
            };
            let seed = [start[0], start[1]];
            let mut back = trajectory(&rhs, seed, vp, -1.0);
            let ahead = trajectory(&rhs, seed, vp, 1.0);
            back.reverse();
            back.push(seed);
            back.extend(ahead);
            vec![back]
        }
    }
}

/// y and its first `order - 1` derivatives at the starting point.
fn initial(start: &[f64], order: usize) -> Vec<f64> {
    (0..order)
        .map(|i| start.get(i + 1).copied().unwrap_or(0.0))
        .collect()
}

type Rhs<'a> = dyn Fn(f64, &[f64], &mut [f64]) -> bool + 'a;

/// A solution y(x) through `state` at `x0`, left to right.
fn curve_both_ways(rhs: &Rhs, x0: f64, state: &[f64], vp: &Viewport) -> Option<Vec<[f64; 2]>> {
    let mut d = vec![0.0; state.len()];
    if !rhs(x0, state, &mut d) {
        return None;
    }
    let mut left = curve(rhs, x0, state, vp, -1.0);
    let right = curve(rhs, x0, state, vp, 1.0);
    left.reverse();
    left.push([x0, state[0]]);
    left.extend(right);
    Some(left)
}

// Dormand-Prince 5(4) coefficients.
const C: [f64; 7] = [0.0, 1.0 / 5.0, 3.0 / 10.0, 4.0 / 5.0, 8.0 / 9.0, 1.0, 1.0];
const A: [[f64; 6]; 7] = [
    [0.0; 6],
    [1.0 / 5.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    [3.0 / 40.0, 9.0 / 40.0, 0.0, 0.0, 0.0, 0.0],
    [44.0 / 45.0, -56.0 / 15.0, 32.0 / 9.0, 0.0, 0.0, 0.0],
    [
        19372.0 / 6561.0,
        -25360.0 / 2187.0,
        64448.0 / 6561.0,
        -212.0 / 729.0,
        0.0,
        0.0,
    ],
    [
        9017.0 / 3168.0,
        -355.0 / 33.0,
        46732.0 / 5247.0,
        49.0 / 176.0,
        -5103.0 / 18656.0,
        0.0,
    ],
    [
        35.0 / 384.0,
        0.0,
        500.0 / 1113.0,
        125.0 / 192.0,
        -2187.0 / 6784.0,
        11.0 / 84.0,
    ],
];
/// Fifth-order weights minus fourth-order weights: the error estimate.
const E: [f64; 7] = [
    71.0 / 57600.0,
    0.0,
    -71.0 / 16695.0,
    71.0 / 1920.0,
    -17253.0 / 339200.0,
    22.0 / 525.0,
    -1.0 / 40.0,
];

/// The most a step may turn the line, in radians.
const MAX_TURN: f64 = 0.3;

/// Most steps taken one way from a seed, so a stiff equation cannot hang
/// the frame.
const MAX_STEPS: usize = 20_000;

/// One Dormand-Prince step of size `h` from (s, y), whose derivative is
/// `k[0]`. Fills the stages and returns the new state and the largest
/// error estimate, or None where the equation is undefined.
fn step(rhs: &Rhs, s: f64, y: &[f64], h: f64, k: &mut [Vec<f64>; 7]) -> Option<(Vec<f64>, f64)> {
    let n = y.len();
    let mut tmp = vec![0.0; n];
    for st in 1..7 {
        for i in 0..n {
            let dy: f64 = (0..st).map(|j| A[st][j] * k[j][i]).sum();
            tmp[i] = y[i] + h * dy;
        }
        let (_, tail) = k.split_at_mut(st);
        if !rhs(s + C[st] * h, &tmp, &mut tail[0]) {
            return None;
        }
    }
    // The last stage's input is the fifth-order solution (FSAL).
    let err = (0..n)
        .map(|i| (h * (0..7).map(|j| E[j] * k[j][i]).sum::<f64>()).abs())
        .fold(0.0, f64::max);
    Some((tmp, err))
}

/// Grows or shrinks a step after an error estimate.
fn resize(h: f64, err: f64, tol: f64) -> f64 {
    if err == 0.0 {
        h * 5.0
    } else if err <= tol {
        h * (0.9 * (tol / err).powf(0.2)).clamp(0.2, 5.0)
    } else if err.is_finite() {
        h * (0.9 * (tol / err).powf(0.2)).clamp(0.1, 0.5)
    } else {
        h * 0.25
    }
}

/// Steps a solution y(x) from x0 in the direction `dir` (1 right, -1 left),
/// returning the points after the start.
fn curve(rhs: &Rhs, x0: f64, state: &[f64], vp: &Viewport, dir: f64) -> Vec<[f64; 2]> {
    let unit = vp.units_per_px;
    let (vx0, vx1) = vp.x_range();
    let (vy0, vy1) = vp.y_range();
    // A view's width of room past each edge, so the line runs off the
    // paper rather than stopping at its edge.
    let reach_x = vx1 - vx0;
    let end = if dir > 0.0 {
        vx1 + reach_x
    } else {
        vx0 - reach_x
    };
    let (lo_y, hi_y) = (vy0 - 10.0 * (vy1 - vy0), vy1 + 10.0 * (vy1 - vy0));
    // A tenth of a pixel is as close as anyone can see; steps are no
    // longer than two pixels across so the drawn line stays smooth.
    let tol = 0.1 * unit;
    let max_h = 2.0 * unit;
    let min_h = 1e-9 * unit;

    let mut out = Vec::new();
    let (mut x, mut y) = (x0, state.to_vec());
    if (end - x) * dir <= 0.0 {
        return out;
    }
    let n = y.len();
    let mut k: [Vec<f64>; 7] = std::array::from_fn(|_| vec![0.0; n]);
    if !rhs(x, &y, &mut k[0]) {
        return out;
    }
    let mut h = max_h;
    let mut steps = 0;
    while (end - x) * dir > 0.0 && steps < MAX_STEPS {
        steps += 1;
        // Also no more than about two pixels up or down, so steep stretches
        // are drawn as smoothly as flat ones.
        h = h
            .min(max_h / k[0][0].abs().max(1.0))
            .min((end - x).abs())
            .max(min_h);
        let hs = h * dir;
        // Error per unit step: tiny steps near a vertical tangent must be
        // tinier still in error, or thousands of them add up.
        let tol_h = tol * (h / max_h).min(1.0);
        let result = step(rhs, x, &y, hs, &mut k);
        // A step may turn the line by only so much, which keeps it from
        // leaping across a vertical tangent where the slope flips sign.
        let err = match &result {
            Some((_, err)) if (k[6][0].atan() - k[0][0].atan()).abs() <= MAX_TURN => *err,
            _ => f64::INFINITY,
        };
        if err <= tol_h {
            let (next, _) = result.expect("accepted");
            x += hs;
            y = next;
            let last = k[6].clone();
            k[0] = last;
            if !(y[0].is_finite() && (lo_y..=hi_y).contains(&y[0])) {
                // Ran off the paper: draw up to where it leaves.
                if y[0].is_finite() {
                    out.push([x, y[0]]);
                }
                break;
            }
            out.push([x, y[0]]);
            h = resize(h, err, tol_h).min(max_h);
        } else if h <= min_h {
            // The slope is undefined or the solution turns vertical here,
            // as y' = -x/y does where its circles meet the x-axis.
            break;
        } else {
            h = resize(h, err, tol_h);
        }
    }
    out
}

/// Steps a system's trajectory from `seed` at t = 0, forward in time when
/// `dir` is 1 and backward when -1, until it leaves the view by a wide
/// margin, comes back round to its start, settles or runs out of steps.
fn trajectory(rhs: &Rhs, seed: [f64; 2], vp: &Viewport, dir: f64) -> Vec<[f64; 2]> {
    let unit = vp.units_per_px;
    let (vx0, vx1) = vp.x_range();
    let (vy0, vy1) = vp.y_range();
    let (wx, wy) = (vx1 - vx0, vy1 - vy0);
    let inside =
        |p: &[f64]| (vx0 - wx..=vx1 + wx).contains(&p[0]) && (vy0 - wy..=vy1 + wy).contains(&p[1]);
    let tol = 0.1 * unit;
    let max_len = 2.0 * unit;
    const MAX_T: f64 = 1e4;

    let mut out = Vec::new();
    let (mut t, mut y) = (0.0, seed.to_vec());
    let mut k: [Vec<f64>; 7] = std::array::from_fn(|_| vec![0.0; 2]);
    if !rhs(t, &y, &mut k[0]) {
        return out;
    }
    let speed = |d: &[f64]| d[0].hypot(d[1]);
    let mut h = max_len / speed(&k[0]).max(1e-12);
    let mut travelled = 0.0;
    let mut steps = 0;
    while steps < MAX_STEPS && t.abs() < MAX_T {
        steps += 1;
        let v = speed(&k[0]);
        if v.is_nan() || v <= 0.0 {
            break; // Reached a resting point.
        }
        h = h.min(max_len / v).min(MAX_T);
        let hs = h * dir;
        let tol_h = tol * (h * v / max_len).min(1.0);
        let result = step(rhs, t, &y, hs, &mut k);
        let turn = {
            let a = k[0][1].atan2(k[0][0]);
            let b = k[6][1].atan2(k[6][0]);
            let d = (b - a).abs();
            d.min(std::f64::consts::TAU - d)
        };
        let err = match &result {
            Some((_, err)) if turn <= MAX_TURN => *err,
            _ => f64::INFINITY,
        };
        if err <= tol_h {
            let (next, _) = result.expect("accepted");
            travelled += (next[0] - y[0]).hypot(next[1] - y[1]);
            t += hs;
            y = next;
            let last = k[6].clone();
            k[0] = last;
            if !(y[0].is_finite() && y[1].is_finite()) {
                break;
            }
            out.push([y[0], y[1]]);
            if !inside(&y) {
                break;
            }
            // A closed orbit: back within half a pixel of the start.
            if travelled > 20.0 * unit && (y[0] - seed[0]).hypot(y[1] - seed[1]) < 0.5 * unit {
                out.push(seed);
                break;
            }
            h = resize(h, err, tol_h);
        } else if h * v <= 1e-9 * unit {
            break;
        } else {
            h = resize(h, err, tol_h);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view() -> Viewport {
        Viewport::new(1000.0, 600.0)
    }

    fn first_order(f: &(dyn Fn(f64, &[f64]) -> f64 + Sync)) -> Equation<'_> {
        Equation::Explicit { order: 1, f }
    }

    fn one(eq: &Equation, start: &[f64]) -> Vec<[f64; 2]> {
        let mut lines = solutions(eq, start, &view());
        assert_eq!(lines.len(), 1);
        lines.remove(0)
    }

    #[test]
    fn exponential_growth_matches_e_to_the_x() {
        let vp = view();
        let f = |_x: f64, s: &[f64]| s[0];
        let line = one(&first_order(&f), &[0.0, 1.0]);
        assert!(line.len() > 100);
        assert!(line.windows(2).all(|w| w[0][0] < w[1][0]), "left to right");
        for p in &line {
            if p[1] < 20.0 {
                assert!(
                    (p[1] - p[0].exp()).abs() < 0.2 * vp.units_per_px,
                    "{p:?} is off e^x"
                );
            }
        }
        // It runs off the top of the paper on the right and goes on past
        // the left edge.
        assert!(line.first().unwrap()[0] < vp.x_range().0);
        assert!(line.last().unwrap()[1] > vp.y_range().1);
    }

    #[test]
    fn logistic_and_linear_equations_stay_accurate() {
        let vp = view();
        // y' = x - y through (0, 1) is y = x - 1 + 2e^{-x}.
        let f = |x: f64, s: &[f64]| x - s[0];
        for p in one(&first_order(&f), &[0.0, 1.0]) {
            let exact = p[0] - 1.0 + 2.0 * (-p[0]).exp();
            if exact.abs() < 20.0 {
                assert!((p[1] - exact).abs() < 0.2 * vp.units_per_px, "{p:?}");
            }
        }
        // y' = y (1 - y) through (0, 0.5) is the logistic curve.
        let g = |_x: f64, s: &[f64]| s[0] * (1.0 - s[0]);
        for p in one(&first_order(&g), &[0.0, 0.5]) {
            let exact = 1.0 / (1.0 + (-p[0]).exp());
            assert!((p[1] - exact).abs() < 0.2 * vp.units_per_px, "{p:?}");
        }
    }

    #[test]
    fn a_circle_stops_where_it_turns_vertical() {
        // y' = -x / y through (0, 3) is the top half of the circle of
        // radius 3, which ends at (±3, 0).
        let f = |x: f64, s: &[f64]| -x / s[0];
        let line = one(&first_order(&f), &[0.0, 3.0]);
        let (first, last) = (line[0], *line.last().unwrap());
        assert!(first[0] < -2.9 && first[0] > -3.0001, "{first:?}");
        assert!(last[0] > 2.9 && last[0] < 3.0001, "{last:?}");
        for p in &line {
            assert!((p[0].hypot(p[1]) - 3.0).abs() < 0.01, "{p:?}");
        }
    }

    #[test]
    fn undefined_starts_draw_nothing() {
        let f = |x: f64, _s: &[f64]| x.ln();
        assert!(solutions(&first_order(&f), &[-1.0, 0.0], &view()).is_empty());
        assert!(solutions(&first_order(&f), &[f64::NAN, 0.0], &view()).is_empty());
    }

    #[test]
    fn second_and_third_order_equations() {
        let vp = view();
        // y'' = -y with y(0) = 1, y'(0) = 0 is cos x.
        let f = |_x: f64, s: &[f64]| -s[0];
        let eq = Equation::Explicit { order: 2, f: &f };
        let line = one(&eq, &[0.0, 1.0, 0.0]);
        assert!(line.first().unwrap()[0] < vp.x_range().0);
        assert!(line.last().unwrap()[0] > vp.x_range().1);
        for p in &line {
            assert!((p[1] - p[0].cos()).abs() < 0.2 * vp.units_per_px, "{p:?}");
        }
        // The initial slope defaults to 0 when left out.
        assert_eq!(one(&eq, &[0.0, 1.0]), line);
        // y''' = y' with y = 0, y' = 1, y'' = 0 at 0 is sinh x.
        let g = |_x: f64, s: &[f64]| s[1];
        let eq = Equation::Explicit { order: 3, f: &g };
        for p in one(&eq, &[0.0, 0.0, 1.0, 0.0]) {
            if p[1].abs() < 20.0 {
                assert!((p[1] - p[0].sinh()).abs() < 0.2 * vp.units_per_px, "{p:?}");
            }
        }
    }

    #[test]
    fn implicit_equations_follow_every_root() {
        let vp = view();
        // (y')^2 = 1 has slopes ±1 everywhere: two lines through each
        // starting point, and two marks at each lattice point.
        let f = |_x: f64, s: &[f64]| s[1] * s[1] - 1.0;
        let eq = Equation::Implicit { order: 1, f: &f };
        let lines = solutions(&eq, &[0.0, 0.0], &vp);
        assert_eq!(lines.len(), 2);
        for (line, slope) in lines.iter().zip([-1.0, 1.0]) {
            for p in line {
                assert!((p[1] - slope * p[0]).abs() < 1e-6, "{p:?}");
            }
        }
        let marks = field_marks(&eq, &vp, 1.0);
        let lattice = field_marks(
            &Equation::Explicit {
                order: 1,
                f: &|_, _| 0.0,
            },
            &vp,
            1.0,
        );
        assert_eq!(marks.len(), 2 * lattice.len());
        // x y' + y = 0 is y = c / x: not solved for y', but linear in it.
        let g = |x: f64, s: &[f64]| x * s[1] + s[0];
        let eq = Equation::Implicit { order: 1, f: &g };
        let line = &solutions(&eq, &[1.0, 2.0], &vp)[0];
        for p in line {
            if p[1].abs() < 20.0 {
                assert!((p[1] - 2.0 / p[0]).abs() < 0.2 * vp.units_per_px, "{p:?}");
            }
        }
        // y'' + y = 0, written the other way round, is cos x again.
        let h = |_x: f64, s: &[f64]| s[2] + s[0];
        let eq = Equation::Implicit { order: 2, f: &h };
        for p in &solutions(&eq, &[0.0, 1.0, 0.0], &vp)[0] {
            assert!((p[1] - p[0].cos()).abs() < 0.2 * vp.units_per_px, "{p:?}");
        }
    }

    #[test]
    fn systems_trace_closed_orbits_and_spirals() {
        let vp = view();
        // x' = -y, y' = x: circles, closed after one turn.
        let (f, g) = (|_t: f64, _x: f64, y: f64| -y, |_t: f64, x: f64, _y: f64| x);
        let eq = Equation::System {
            f: &f,
            g: &g,
            autonomous: true,
        };
        let line = one(&eq, &[2.0, 0.0]);
        for p in &line {
            assert!(
                (p[0].hypot(p[1]) - 2.0).abs() < 0.2 * vp.units_per_px,
                "{p:?}"
            );
        }
        assert!(line.len() < 4000, "stops after a turn: {}", line.len());
        // A resting point draws nothing.
        assert!(solutions(&eq, &[0.0, 0.0], &vp).is_empty());
        // Arrows: a shaft and two strokes of head at each lattice point.
        let marks = field_marks(&eq, &vp, 1.0);
        assert_eq!(marks.len() % 3, 0);
        assert!(marks.len() > 1000);
        // x' = -x/5 - y, y' = x - y/5 spirals in, and back out the other
        // way until it leaves the paper.
        let (f, g) = (
            |_t: f64, x: f64, y: f64| -x / 5.0 - y,
            |_t: f64, x: f64, y: f64| x - y / 5.0,
        );
        let eq = Equation::System {
            f: &f,
            g: &g,
            autonomous: true,
        };
        let line = one(&eq, &[3.0, 0.0]);
        let r = |p: &[f64; 2]| p[0].hypot(p[1]);
        assert!(r(line.last().unwrap()) < 0.5);
        assert!(r(&line[0]) > 10.0);
        // A system that reads t draws trajectories but no field.
        let (f, g) = (
            |t: f64, _x: f64, _y: f64| t,
            |_t: f64, _x: f64, _y: f64| 1.0,
        );
        let eq = Equation::System {
            f: &f,
            g: &g,
            autonomous: false,
        };
        assert!(field_marks(&eq, &vp, 1.0).is_empty());
        // x = t²/2, y = t from (0, 0): the parabola x = y²/2.
        for p in one(&eq, &[0.0, 0.0]) {
            assert!(
                (p[0] - p[1] * p[1] / 2.0).abs() < 0.2 * vp.units_per_px,
                "{p:?}"
            );
        }
    }

    #[test]
    fn marks_follow_the_slope_on_a_grid_lattice() {
        let vp = view();
        let f = |x: f64, _s: &[f64]| x;
        let marks = field_marks(&first_order(&f), &vp, 1.0);
        // 20 units across 1000 px, so a mark about every 0.5 units.
        assert!(marks.len() > 500, "{}", marks.len());
        for [a, b] in &marks {
            let (cx, cy) = ((a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0);
            assert!((cx / 0.5 - (cx / 0.5).round()).abs() < 1e-9);
            assert!((cy / 0.5 - (cy / 0.5).round()).abs() < 1e-9);
            let m = (b[1] - a[1]) / (b[0] - a[0]);
            assert!((m - cx).abs() < 1e-9);
        }
        // Infinite slopes stand upright, undefined ones are left out.
        let g = |x: f64, _s: &[f64]| 1.0 / x;
        let marks = field_marks(&first_order(&g), &vp, 1.0);
        assert!(marks.iter().any(|[a, b]| a[0] == 0.0 && b[0] == 0.0));
        let h = |x: f64, _s: &[f64]| x.sqrt();
        assert!(field_marks(&first_order(&h), &vp, 1.0)
            .iter()
            .all(|[a, _]| a[0] >= -0.5));
        // Higher orders have no field in the plane.
        let eq = Equation::Explicit { order: 2, f: &f };
        assert!(field_marks(&eq, &vp, 1.0).is_empty());
    }
}
