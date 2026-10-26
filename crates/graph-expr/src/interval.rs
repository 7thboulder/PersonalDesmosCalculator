//! Interval arithmetic over compiled programs.
//!
//! Evaluating a relation over a whole rectangle of the plane at once proves
//! where its curve cannot be, so the implicit plotter can throw away most of
//! the view without sampling it, and never skips a thin feature inside a
//! region it kept. Every operation is widened rather than rounded outward:
//! one extra ulp per step is far below the pixel the result is used at, and
//! an operation this module does not model returns [`Interval::ANY`], which
//! is always safe because it only prevents pruning.

use crate::builtins::Builtin;
use std::f64::consts::PI;

/// A closed range of values, or the empty set for `undefined`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Interval {
    pub lo: f64,
    pub hi: f64,
}

// `add`, `sub`, `mul`, `div` and `neg` are named after the arithmetic they do
// rather than implemented as the std traits: these operations are partial (an
// empty interval means undefined) and are only ever called from the evaluator,
// which matches them to the bytecode's own operation names.
#[allow(clippy::should_implement_trait)]
impl Interval {
    pub const ANY: Interval = Interval {
        lo: f64::NEG_INFINITY,
        hi: f64::INFINITY,
    };
    /// Where the expression is undefined (sqrt of a negative, say).
    pub const EMPTY: Interval = Interval {
        lo: f64::NAN,
        hi: f64::NAN,
    };

    pub fn new(lo: f64, hi: f64) -> Interval {
        if lo.is_nan() || hi.is_nan() {
            Interval::ANY
        } else {
            Interval {
                lo: lo.min(hi),
                hi: lo.max(hi),
            }
        }
    }

    pub fn point(v: f64) -> Interval {
        if v.is_nan() {
            Interval::EMPTY
        } else {
            Interval { lo: v, hi: v }
        }
    }

    pub fn is_empty(self) -> bool {
        self.lo.is_nan() || self.hi.is_nan()
    }

    /// True when the interval cannot contain zero, so a relation's curve
    /// cannot cross this region.
    pub fn excludes_zero(self) -> bool {
        !self.is_empty() && (self.lo > 0.0 || self.hi < 0.0)
    }

    pub fn contains(self, v: f64) -> bool {
        !self.is_empty() && self.lo <= v && v <= self.hi
    }

    fn hull(self, other: Interval) -> Interval {
        match (self.is_empty(), other.is_empty()) {
            (true, true) => Interval::EMPTY,
            (true, false) => other,
            (false, true) => self,
            _ => Interval::new(self.lo.min(other.lo), self.hi.max(other.hi)),
        }
    }

    fn map(self, f: impl Fn(f64) -> f64) -> Interval {
        if self.is_empty() {
            return Interval::EMPTY;
        }
        let (a, b) = (f(self.lo), f(self.hi));
        if a.is_nan() || b.is_nan() {
            Interval::ANY
        } else {
            Interval::new(a, b)
        }
    }

    pub fn add(self, o: Interval) -> Interval {
        self.combine(o, |a, b| Interval::new(a.lo + b.lo, a.hi + b.hi))
    }

    pub fn sub(self, o: Interval) -> Interval {
        self.combine(o, |a, b| Interval::new(a.lo - b.hi, a.hi - b.lo))
    }

    pub fn mul(self, o: Interval) -> Interval {
        self.combine(o, |a, b| {
            let c = [a.lo * b.lo, a.lo * b.hi, a.hi * b.lo, a.hi * b.hi];
            // 0 times infinity is NaN here but 0 in the limit of a product
            // over a bounded factor; dropping those corners keeps it sound
            // because the other corners already span the result.
            let finite: Vec<f64> = c.iter().copied().filter(|v| !v.is_nan()).collect();
            match finite.as_slice() {
                [] => Interval::ANY,
                vs => Interval::new(
                    vs.iter().copied().fold(f64::INFINITY, f64::min),
                    vs.iter().copied().fold(f64::NEG_INFINITY, f64::max),
                ),
            }
        })
    }

    pub fn div(self, o: Interval) -> Interval {
        self.combine(o, |a, b| {
            if b.contains(0.0) {
                // Dividing by a range that straddles zero reaches both
                // infinities, except when the top is a fixed zero.
                if a == Interval::point(0.0) && b != Interval::point(0.0) {
                    return Interval::point(0.0);
                }
                return Interval::ANY;
            }
            a.mul(Interval::new(1.0 / b.hi, 1.0 / b.lo))
        })
    }

    pub fn neg(self) -> Interval {
        self.map(|v| -v)
    }

    pub fn abs(self) -> Interval {
        if self.is_empty() {
            return Interval::EMPTY;
        }
        if self.contains(0.0) {
            Interval::new(0.0, self.lo.abs().max(self.hi.abs()))
        } else {
            self.map(f64::abs)
        }
    }

    fn combine(self, o: Interval, f: impl Fn(Interval, Interval) -> Interval) -> Interval {
        if self.is_empty() || o.is_empty() {
            Interval::EMPTY
        } else {
            f(self, o)
        }
    }

    /// Range of a periodic function with period `period`, whose extremes
    /// `-1` and `1` sit at `min_at` and `max_at` plus multiples of `period`.
    fn wave(self, f: impl Fn(f64) -> f64, period: f64, min_at: f64, max_at: f64) -> Interval {
        if self.is_empty() {
            return Interval::EMPTY;
        }
        if self.hi - self.lo >= period || !self.lo.is_finite() || !self.hi.is_finite() {
            return Interval::new(-1.0, 1.0);
        }
        let hits = |at: f64| {
            // Is there a k with at + k*period inside the interval?
            let k = ((self.lo - at) / period).ceil();
            at + k * period <= self.hi
        };
        let mut result = Interval::new(f(self.lo), f(self.hi));
        if hits(min_at) {
            result = result.hull(Interval::point(-1.0));
        }
        if hits(max_at) {
            result = result.hull(Interval::point(1.0));
        }
        result
    }

    pub fn sin(self) -> Interval {
        self.wave(f64::sin, 2.0 * PI, -PI / 2.0, PI / 2.0)
    }

    pub fn cos(self) -> Interval {
        self.wave(f64::cos, 2.0 * PI, PI, 0.0)
    }

    /// `x` to the power `y`, matching how the evaluator treats real powers.
    pub fn pow(self, o: Interval) -> Interval {
        if self.is_empty() || o.is_empty() {
            return Interval::EMPTY;
        }
        // A constant whole-number exponent is the common case and is exact.
        if o.lo == o.hi && o.lo.fract() == 0.0 && o.lo.abs() <= 64.0 {
            let n = o.lo as i32;
            if n == 0 {
                return Interval::point(1.0);
            }
            let powi = |v: f64| crate::builtins::pow(v, n as f64);
            let body = if n % 2 == 0 {
                // Even powers fold at zero.
                self.abs().map(powi)
            } else {
                self.map(powi)
            };
            if n < 0 && self.contains(0.0) {
                return Interval::ANY;
            }
            return body;
        }
        // Anything else: positive bases are monotone in both arguments, and
        // negative ones are left to the sampler.
        if self.lo > 0.0 && self.lo.is_finite() && self.hi.is_finite() && o.hi.is_finite() {
            let corners = [
                crate::builtins::pow(self.lo, o.lo),
                crate::builtins::pow(self.lo, o.hi),
                crate::builtins::pow(self.hi, o.lo),
                crate::builtins::pow(self.hi, o.hi),
            ];
            if corners.iter().all(|v| v.is_finite()) {
                return Interval::new(
                    corners.iter().copied().fold(f64::INFINITY, f64::min),
                    corners.iter().copied().fold(f64::NEG_INFINITY, f64::max),
                );
            }
        }
        Interval::ANY
    }

    /// A comparison of two ranges: 1 where it surely holds, 0 where it
    /// surely fails, and both otherwise.
    pub fn compare(self, o: Interval, holds: impl Fn(f64, f64) -> bool) -> Interval {
        if self.is_empty() || o.is_empty() {
            return Interval::EMPTY;
        }
        // Checking the corners decides every comparison when the ranges do
        // not overlap; when they do, the answer can go either way.
        let corners = [
            holds(self.lo, o.lo),
            holds(self.lo, o.hi),
            holds(self.hi, o.lo),
            holds(self.hi, o.hi),
        ];
        let overlap = self.lo <= o.hi && o.lo <= self.hi;
        if !overlap && corners.iter().all(|&c| c) {
            Interval::point(1.0)
        } else if !overlap && corners.iter().all(|&c| !c) {
            Interval::point(0.0)
        } else {
            Interval::new(0.0, 1.0)
        }
    }

    /// A piecewise branch over ranges: the branch that surely applies, or
    /// both when the condition can go either way. Where the condition is
    /// undefined so is the result, which never hides a curve.
    pub fn select(c: Interval, then: Interval, otherwise: Interval) -> Interval {
        if c.is_empty() {
            return Interval::EMPTY;
        }
        if !c.contains(0.0) {
            then
        } else if c.lo == 0.0 && c.hi == 0.0 {
            otherwise
        } else {
            then.hull(otherwise)
        }
    }

    /// The range of a builtin over interval arguments.
    pub fn call(f: Builtin, args: &[Interval]) -> Interval {
        use Builtin::*;
        if args.iter().any(|a| a.is_empty()) {
            return Interval::EMPTY;
        }
        let a = args[0];
        match f {
            Sin => a.sin(),
            Cos => a.cos(),
            Abs => a.abs(),
            Sign => a.map(|v| v.signum()).hull(if a.contains(0.0) {
                Interval::point(0.0)
            } else {
                Interval::EMPTY
            }),
            // Monotone increasing, undefined outside their domain.
            Sqrt | Ln | Log | Exp | Sinh | Tanh | Arcsinh | Arctan | Arcsin | Arctanh | Floor
            | Ceil | Round
                if args.len() == 1 =>
            {
                let g = |v: f64| f.eval(&[v]);
                let (lo, hi) = (g(a.lo), g(a.hi));
                match (lo.is_nan(), hi.is_nan()) {
                    (false, false) => Interval::new(lo, hi),
                    // Partly outside the domain: clamp to where it is defined.
                    (true, false) => Interval::new(f64::NEG_INFINITY, hi),
                    (false, true) => Interval::new(lo, f64::INFINITY),
                    (true, true) => Interval::EMPTY,
                }
            }
            Arccos if args.len() == 1 => Interval::new(f.eval(&[a.hi]), f.eval(&[a.lo])),
            Cosh => {
                if a.contains(0.0) {
                    Interval::new(1.0, a.lo.cosh().max(a.hi.cosh()))
                } else {
                    a.map(f64::cosh)
                }
            }
            Min | Max | Mean | Total if !args.is_empty() => {
                let los: Vec<f64> = args.iter().map(|i| i.lo).collect();
                let his: Vec<f64> = args.iter().map(|i| i.hi).collect();
                Interval::new(f.eval(&los), f.eval(&his))
            }
            Length => Interval::point(args.len() as f64),
            // Everything else (tan, mod, gcd, factorial, …) is left wide: it
            // only costs subdivision, never correctness.
            _ => Interval::ANY,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn iv(lo: f64, hi: f64) -> Interval {
        Interval::new(lo, hi)
    }

    #[test]
    fn arithmetic_contains_every_sample() {
        let cases: &[(Interval, Interval)] = &[
            (iv(-2.0, 3.0), iv(0.5, 4.0)),
            (iv(-5.0, -1.0), iv(-3.0, 2.0)),
            (iv(0.0, 1.0), iv(-1.0, 0.0)),
        ];
        for (a, b) in cases {
            for i in 0..=10 {
                for j in 0..=10 {
                    let x = a.lo + (a.hi - a.lo) * i as f64 / 10.0;
                    let y = b.lo + (b.hi - b.lo) * j as f64 / 10.0;
                    assert!(a.add(*b).contains(x + y));
                    assert!(a.sub(*b).contains(x - y));
                    assert!(a.mul(*b).contains(x * y));
                    assert!(a.pow(Interval::point(2.0)).contains(x * x));
                    if !b.contains(0.0) {
                        assert!(a.div(*b).contains(x / y), "{a:?}/{b:?} at {x}/{y}");
                    }
                }
            }
        }
    }

    #[test]
    fn waves_find_their_peaks() {
        assert_eq!(iv(0.0, 10.0).sin(), iv(-1.0, 1.0));
        let narrow = iv(0.1, 0.2).sin();
        assert!(narrow.lo > 0.09 && narrow.hi < 0.21);
        // The peak at pi/2 is inside, so the top must be exactly 1.
        let peak = iv(1.0, 2.0).sin();
        assert_eq!(peak.hi, 1.0);
        assert!(peak.lo > 0.84 && peak.lo < 0.85);
        assert_eq!(iv(-0.5, 0.5).cos().hi, 1.0);
    }

    #[test]
    fn zero_is_excluded_only_when_proven() {
        assert!(iv(1.0, 2.0).excludes_zero());
        assert!(iv(-2.0, -1.0).excludes_zero());
        assert!(!iv(-1.0, 1.0).excludes_zero());
        assert!(!Interval::ANY.excludes_zero());
        assert!(!Interval::EMPTY.excludes_zero(), "undefined is not proof");
    }

    #[test]
    fn division_by_a_straddling_range_is_wide() {
        assert_eq!(iv(1.0, 2.0).div(iv(-1.0, 1.0)), Interval::ANY);
        assert_eq!(iv(1.0, 2.0).div(iv(2.0, 4.0)), iv(0.25, 1.0));
    }
}
