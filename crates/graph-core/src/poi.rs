//! Points of interest on explicit curves, the grey dots Desmos shows on a
//! selected curve: where it crosses the axes, its turning points, and where
//! it meets other curves. Found by sampling once per pixel column and
//! refining each candidate to full precision.

use crate::Viewport;

/// What makes a point interesting. Desmos labels them all the same way,
/// but the kind decides which ones survive deduplication first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PoiKind {
    /// The curve crosses or touches its own axis (y = 0 for y = f(x)).
    Zero,
    /// Where the curve crosses the other axis.
    Intercept,
    Maximum,
    Minimum,
    /// Where two curves meet.
    Intersection,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Poi {
    /// In world coordinates.
    pub at: [f64; 2],
    pub kind: PoiKind,
}

/// Most points one search returns, so a wild curve cannot flood the view.
const MAX_POIS: usize = 200;
const BISECT_STEPS: usize = 64;
const GOLDEN_STEPS: usize = 80;

/// Samples `f` once per pixel across `lo..hi`.
fn samples(f: &dyn Fn(f64) -> f64, lo: f64, hi: f64, n: usize) -> Vec<(f64, f64)> {
    (0..=n)
        .map(|i| {
            let t = lo + (hi - lo) * i as f64 / n as f64;
            (t, f(t))
        })
        .collect()
}

/// Where `f` crosses zero between samples, excluding jumps across a pole
/// (tan x), where `f` grows instead of shrinking as the gap narrows.
fn crossings(f: &dyn Fn(f64) -> f64, s: &[(f64, f64)]) -> Vec<f64> {
    let mut out = Vec::new();
    for w in s.windows(2) {
        let ((a, fa), (b, fb)) = (w[0], w[1]);
        if !(fa.is_finite() && fb.is_finite()) {
            continue;
        }
        if fa == 0.0 {
            out.push(a);
            continue;
        }
        if fa.signum() == fb.signum() || fb == 0.0 {
            continue;
        }
        let (mut lo, mut hi, mut flo) = (a, b, fa);
        for _ in 0..BISECT_STEPS {
            let mid = (lo + hi) / 2.0;
            let fm = f(mid);
            if !fm.is_finite() {
                break;
            }
            if fm.signum() == flo.signum() {
                lo = mid;
                flo = fm;
            } else {
                hi = mid;
            }
        }
        let root = (lo + hi) / 2.0;
        let fr = f(root);
        if fr.is_finite() && fr.abs() <= fa.abs().max(fb.abs()) {
            out.push(root);
        }
    }
    out
}

/// Golden-section search for the largest `sign * f` in `a..b`.
fn golden(f: &dyn Fn(f64) -> f64, mut a: f64, mut b: f64, sign: f64) -> f64 {
    let r = (5f64.sqrt() - 1.0) / 2.0;
    let g = |t: f64| sign * f(t);
    let (mut c, mut d) = (b - r * (b - a), a + r * (b - a));
    let (mut gc, mut gd) = (g(c), g(d));
    for _ in 0..GOLDEN_STEPS {
        if gc > gd {
            b = d;
            d = c;
            gd = gc;
            c = b - r * (b - a);
            gc = g(c);
        } else {
            a = c;
            c = d;
            gc = gd;
            d = a + r * (b - a);
            gd = g(d);
        }
    }
    (a + b) / 2.0
}

/// Turning points: where the sampled slope changes sign.
fn turning_points(f: &dyn Fn(f64) -> f64, s: &[(f64, f64)]) -> Vec<(f64, PoiKind)> {
    let mut out = Vec::new();
    for w in s.windows(3) {
        let [(a, fa), (m, fm), (b, fb)] = [w[0], w[1], w[2]];
        if !(fa.is_finite() && fm.is_finite() && fb.is_finite()) {
            continue;
        }
        let kind = if fm > fa && fm >= fb {
            PoiKind::Maximum
        } else if fm < fa && fm <= fb {
            PoiKind::Minimum
        } else {
            continue;
        };
        let sign = if kind == PoiKind::Maximum { 1.0 } else { -1.0 };
        let t = golden(f, a, b, sign);
        let ft = f(t);
        // A smooth turning point rises past the middle sample by less than
        // the neighbours differ from it; a spike towards a pole does not.
        let rise = sign * (ft - fm);
        let ok = ft.is_finite()
            && rise >= -1e-12 * (1.0 + fm.abs())
            && rise <= (fm - fa).abs() + (fm - fb).abs();
        if ok && (t - m).abs() <= (b - a) {
            out.push((t, kind));
        }
    }
    out
}

/// Points of interest on `y = f(x)` across the viewport. For `x = f(y)`
/// pass `swap = true`: `f` then runs along y and the results are swapped.
pub fn curve_pois(f: &dyn Fn(f64) -> f64, vp: &Viewport, swap: bool) -> Vec<Poi> {
    let ((lo, hi), n) = if swap {
        (vp.y_range(), vp.height_px)
    } else {
        (vp.x_range(), vp.width_px)
    };
    let n = n.ceil().max(2.0) as usize;
    let s = samples(f, lo, hi, n);
    let point = |t: f64, kind| Poi {
        at: if swap { [f(t), t] } else { [t, f(t)] },
        kind,
    };
    let mut out: Vec<Poi> = Vec::new();
    for t in crossings(f, &s) {
        out.push(point(t, PoiKind::Zero));
    }
    if (lo..=hi).contains(&0.0) && f(0.0).is_finite() {
        out.push(point(0.0, PoiKind::Intercept));
    }
    for (t, kind) in turning_points(f, &s) {
        out.push(point(t, kind));
    }
    dedupe(out, vp)
}

/// Where `y = f(x)` and `y = g(x)` meet across the viewport.
pub fn intersections(f: &dyn Fn(f64) -> f64, g: &dyn Fn(f64) -> f64, vp: &Viewport) -> Vec<Poi> {
    let (lo, hi) = vp.x_range();
    let n = vp.width_px.ceil().max(2.0) as usize;
    let h = |x: f64| f(x) - g(x);
    let s = samples(&h, lo, hi, n);
    let out = crossings(&h, &s)
        .into_iter()
        .map(|x| Poi {
            at: [x, f(x)],
            kind: PoiKind::Intersection,
        })
        .filter(|p| p.at[1].is_finite())
        .collect();
    dedupe(out, vp)
}

/// Drops points within half a pixel of an earlier one, and caps the count.
pub fn dedupe(points: Vec<Poi>, vp: &Viewport) -> Vec<Poi> {
    let close = vp.units_per_px / 2.0;
    let mut out: Vec<Poi> = Vec::new();
    for p in points {
        if !(p.at[0].is_finite() && p.at[1].is_finite()) {
            continue;
        }
        let dup = out
            .iter()
            .any(|q| (q.at[0] - p.at[0]).abs() < close && (q.at[1] - p.at[1]).abs() < close);
        if !dup {
            out.push(p);
        }
        if out.len() >= MAX_POIS {
            break;
        }
    }
    out
}

/// How Desmos writes a coordinate in a label: rounded to about a tenth of a
/// pixel at the current zoom, trailing zeros dropped, and a real minus sign.
pub fn coordinate_text(v: f64, units_per_px: f64) -> String {
    let places = (-units_per_px.log10()).ceil() as i32 + 1;
    let places = places.clamp(0, 12) as usize;
    let mut s = format!("{v:.places$}");
    if s.contains('.') {
        s = s.trim_end_matches('0').trim_end_matches('.').to_owned();
    }
    if s == "-0" {
        s = "0".into();
    }
    s.replace('-', "\u{2212}")
}

/// `(x, y)` as Desmos labels a point.
pub fn point_text(at: [f64; 2], units_per_px: f64) -> String {
    format!(
        "({}, {})",
        coordinate_text(at[0], units_per_px),
        coordinate_text(at[1], units_per_px)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn has(pois: &[Poi], x: f64, y: f64, kind: PoiKind) -> bool {
        pois.iter()
            .any(|p| p.kind == kind && (p.at[0] - x).abs() < 1e-9 && (p.at[1] - y).abs() < 1e-9)
    }

    #[test]
    fn finds_zeros_extrema_and_the_intercept() {
        let vp = Viewport::new(1000.0, 800.0);
        let f = |x: f64| x * x - 4.0;
        let pois = curve_pois(&f, &vp, false);
        assert!(has(&pois, 2.0, 0.0, PoiKind::Zero), "{pois:?}");
        assert!(has(&pois, -2.0, 0.0, PoiKind::Zero));
        // The minimum and the intercept coincide; one dot is kept.
        assert!(has(&pois, 0.0, -4.0, PoiKind::Intercept));
        assert_eq!(pois.len(), 3);

        let pois = curve_pois(&f64::sin, &vp, false);
        let half_pi = std::f64::consts::FRAC_PI_2;
        assert!(pois
            .iter()
            .any(|p| p.kind == PoiKind::Maximum && (p.at[0] - half_pi).abs() < 1e-6));
        assert!(pois
            .iter()
            .any(|p| p.kind == PoiKind::Zero && (p.at[0] - std::f64::consts::PI).abs() < 1e-12));
    }

    #[test]
    fn poles_are_not_zeros() {
        let vp = Viewport::new(1000.0, 800.0);
        let pois = curve_pois(&|x: f64| 1.0 / x, &vp, false);
        assert!(pois.is_empty(), "{pois:?}");
        let pois = curve_pois(&f64::tan, &vp, false);
        assert!(pois
            .iter()
            .all(|p| p.kind == PoiKind::Zero || p.kind == PoiKind::Intercept));
        assert!(pois.iter().all(|p| p.at[1].abs() < 1e-9));
    }

    #[test]
    fn finds_intersections() {
        let vp = Viewport::new(1000.0, 800.0);
        let pois = intersections(&|x: f64| x * x, &|x: f64| x + 2.0, &vp);
        assert_eq!(pois.len(), 2);
        assert!(has(&pois, 2.0, 4.0, PoiKind::Intersection));
        assert!(has(&pois, -1.0, 1.0, PoiKind::Intersection));
    }

    #[test]
    fn labels_round_to_the_zoom() {
        assert_eq!(point_text([std::f64::consts::FRAC_PI_2, 1.0], 0.02), "(1.571, 1)");
        assert_eq!(coordinate_text(-0.00001, 0.02), "0");
        assert_eq!(coordinate_text(-2.5, 0.02), "\u{2212}2.5");
        assert_eq!(coordinate_text(1234.5678, 2.0), "1234.6");
    }
}
