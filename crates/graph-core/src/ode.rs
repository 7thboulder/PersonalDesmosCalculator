//! Differential equations y' = f(x, y): the slope field drawn over the
//! view, and solution curves through chosen points, integrated with an
//! adaptive Runge-Kutta method (Dormand-Prince 5(4)).

use crate::grid::{grid_spacing, line_indices};
use crate::Viewport;

/// Logical pixels aimed for between neighbouring slope marks.
pub const FIELD_SPACING_LOGICAL: f64 = 28.0;
/// A slope mark's length as a share of the spacing between marks.
const MARK_LENGTH: f64 = 0.6;

/// The short line segments of the slope field over the view, in world
/// coordinates. Marks sit on a 1-2-5 lattice so they line up with the
/// grid and stay put while the view pans. Where the slope is undefined
/// there is no mark; an infinite slope is a vertical mark.
pub fn slope_marks(
    slope: &dyn Fn(f64, f64) -> f64,
    vp: &Viewport,
    scale: f64,
) -> Vec<[[f64; 2]; 2]> {
    let step = grid_spacing(vp.units_per_px, FIELD_SPACING_LOGICAL * scale.max(0.5)).major;
    let half = step * MARK_LENGTH / 2.0;
    let (x0, x1) = vp.x_range();
    let (y0, y1) = vp.y_range();
    let mut marks = Vec::new();
    for i in line_indices(x0, x1, step) {
        let x = i as f64 * step;
        for j in line_indices(y0, y1, step) {
            let y = j as f64 * step;
            let m = slope(x, y);
            let (dx, dy) = if m.is_finite() {
                let n = m.hypot(1.0);
                (half / n, half * m / n)
            } else if m.is_infinite() {
                (0.0, half)
            } else {
                continue;
            };
            marks.push([[x - dx, y - dy], [x + dx, y + dy]]);
        }
    }
    marks
}

/// The solution of y' = f(x, y) through `seed`, followed both ways until
/// it leaves the view (with some room to spare), stops being defined or
/// runs off to infinity. Points run from left to right; NaN entries break
/// the line where the slope was undefined.
pub fn solution(slope: &dyn Fn(f64, f64) -> f64, seed: [f64; 2], vp: &Viewport) -> Vec<[f64; 2]> {
    if !(seed[0].is_finite() && seed[1].is_finite()) || !slope(seed[0], seed[1]).is_finite() {
        return Vec::new();
    }
    let mut left = integrate(slope, seed, vp, -1.0);
    let right = integrate(slope, seed, vp, 1.0);
    left.reverse();
    left.push(seed);
    left.extend(right);
    left
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

/// Steps from `seed` in the direction `dir` (1 right, -1 left), returning
/// the points after the seed.
fn integrate(
    slope: &dyn Fn(f64, f64) -> f64,
    seed: [f64; 2],
    vp: &Viewport,
    dir: f64,
) -> Vec<[f64; 2]> {
    let unit = vp.units_per_px;
    let (x0, x1) = vp.x_range();
    let (y0, y1) = vp.y_range();
    // A view's width of room past each edge, so the line runs off the
    // paper rather than stopping at its edge.
    let reach_x = x1 - x0;
    let end = if dir > 0.0 {
        x1 + reach_x
    } else {
        x0 - reach_x
    };
    let (lo_y, hi_y) = (y0 - 10.0 * (y1 - y0), y1 + 10.0 * (y1 - y0));
    // A tenth of a pixel is as close as anyone can see; steps are no
    // longer than two pixels across so the drawn line stays smooth.
    let tol = 0.1 * unit;
    let max_h = 2.0 * unit;
    let min_h = 1e-9 * unit;

    let mut out = Vec::new();
    let (mut x, mut y) = (seed[0], seed[1]);
    if (end - x) * dir <= 0.0 {
        return out;
    }
    let mut h = max_h;
    let mut k = [0.0; 7];
    k[0] = slope(x, y);
    let mut steps = 0;
    while (end - x) * dir > 0.0 && steps < MAX_STEPS {
        steps += 1;
        // Also no more than about two pixels up or down, so steep stretches
        // are drawn as smoothly as flat ones.
        h = h
            .min(max_h / k[0].abs().max(1.0))
            .min((end - x).abs())
            .max(min_h);
        let hs = h * dir;
        let mut ok = true;
        for s in 1..7 {
            let dy: f64 = (0..s).map(|j| A[s][j] * k[j]).sum();
            k[s] = slope(x + C[s] * hs, y + hs * dy);
            if !k[s].is_finite() {
                ok = false;
                break;
            }
        }
        // A step may turn the line by only so much, which keeps it from
        // leaping across a vertical tangent where the slope flips sign.
        let turn = (k[6].atan() - k[0].atan()).abs();
        // Error per unit step: tiny steps near a vertical tangent must be
        // tinier still in error, or thousands of them add up.
        let tol = tol * (h / max_h).min(1.0);
        let err = if ok && turn <= MAX_TURN {
            (hs * (0..7).map(|j| E[j] * k[j]).sum::<f64>()).abs()
        } else {
            f64::INFINITY
        };
        if err <= tol {
            // The fifth-order solution is the last stage's input (FSAL).
            let dy: f64 = (0..6).map(|j| A[6][j] * k[j]).sum();
            x += hs;
            y += hs * dy;
            k[0] = k[6];
            if !(y.is_finite() && (lo_y..=hi_y).contains(&y)) {
                // Ran off the paper: draw up to where it leaves.
                if y.is_finite() {
                    out.push([x, y]);
                }
                break;
            }
            out.push([x, y]);
            let grow = if err == 0.0 {
                5.0
            } else {
                (0.9 * (tol / err).powf(0.2)).clamp(0.2, 5.0)
            };
            h = (h * grow).min(max_h);
        } else if h <= min_h {
            // The slope is undefined or the solution turns vertical here,
            // as y' = -x/y does where its circles meet the x-axis.
            break;
        } else {
            h = if err.is_finite() {
                h * (0.9 * (tol / err).powf(0.2)).clamp(0.1, 0.5)
            } else {
                h * 0.25
            };
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

    #[test]
    fn exponential_growth_matches_e_to_the_x() {
        let vp = view();
        let f = |_x: f64, y: f64| y;
        let line = solution(&f, [0.0, 1.0], &vp);
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
        let f = |x: f64, y: f64| x - y;
        for p in solution(&f, [0.0, 1.0], &vp) {
            let exact = p[0] - 1.0 + 2.0 * (-p[0]).exp();
            if exact.abs() < 20.0 {
                assert!((p[1] - exact).abs() < 0.2 * vp.units_per_px, "{p:?}");
            }
        }
        // y' = y (1 - y) through (0, 0.5) is the logistic curve.
        let g = |_x: f64, y: f64| y * (1.0 - y);
        for p in solution(&g, [0.0, 0.5], &vp) {
            let exact = 1.0 / (1.0 + (-p[0]).exp());
            assert!((p[1] - exact).abs() < 0.2 * vp.units_per_px, "{p:?}");
        }
    }

    #[test]
    fn a_circle_stops_where_it_turns_vertical() {
        let vp = view();
        // y' = -x / y through (0, 3) is the top half of the circle of
        // radius 3, which ends at (±3, 0).
        let f = |x: f64, y: f64| -x / y;
        let line = solution(&f, [0.0, 3.0], &vp);
        let (first, last) = (line[0], *line.last().unwrap());
        assert!(first[0] < -2.9 && first[0] > -3.0001, "{first:?}");
        assert!(last[0] > 2.9 && last[0] < 3.0001, "{last:?}");
        for p in &line {
            assert!((p[0].hypot(p[1]) - 3.0).abs() < 0.01, "{p:?}");
        }
    }

    #[test]
    fn undefined_seeds_draw_nothing() {
        let f = |x: f64, _y: f64| x.ln();
        assert!(solution(&f, [-1.0, 0.0], &view()).is_empty());
        assert!(solution(&f, [f64::NAN, 0.0], &view()).is_empty());
    }

    #[test]
    fn marks_follow_the_slope_on_a_grid_lattice() {
        let vp = view();
        let f = |x: f64, _y: f64| x;
        let marks = slope_marks(&f, &vp, 1.0);
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
        let g = |x: f64, _y: f64| 1.0 / x;
        let marks = slope_marks(&g, &vp, 1.0);
        assert!(marks.iter().any(|[a, b]| a[0] == 0.0 && b[0] == 0.0));
        let h = |x: f64, _y: f64| x.sqrt();
        assert!(slope_marks(&h, &vp, 1.0).iter().all(|[a, _]| a[0] >= -0.5));
    }
}
