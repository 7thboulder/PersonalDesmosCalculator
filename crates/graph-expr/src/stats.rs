//! Statistics, as Desmos has them and a little more: summaries of lists
//! (standard deviation, quartiles, correlation, …), probability
//! distributions with their densities, cumulative probabilities, quantiles
//! and random samples, hypothesis tests and confidence intervals, and the
//! numbers behind histograms, dot plots and box plots.
//!
//! Everything here works on plain numbers; `analyze` decides what a row
//! means and calls in.

use crate::special::{erfc, ln_gamma};
use std::f64::consts::{PI, SQRT_2};

// ---------------------------------------------------------------------------
// Summaries of lists

pub fn mean(a: &[f64]) -> f64 {
    a.iter().sum::<f64>() / a.len() as f64
}

/// The sum of squared distances from the mean.
fn squares(a: &[f64]) -> f64 {
    let m = mean(a);
    a.iter().map(|x| (x - m) * (x - m)).sum()
}

/// The sample variance, dividing by n − 1, as Desmos's `var`.
pub fn var(a: &[f64]) -> f64 {
    if a.len() < 2 {
        return f64::NAN;
    }
    squares(a) / (a.len() - 1) as f64
}

/// The population variance, dividing by n, as Desmos's `varp`.
pub fn varp(a: &[f64]) -> f64 {
    if a.is_empty() {
        return f64::NAN;
    }
    squares(a) / a.len() as f64
}

/// The mean absolute deviation from the mean, Desmos's `mad`.
pub fn mad(a: &[f64]) -> f64 {
    let m = mean(a);
    a.iter().map(|x| (x - m).abs()).sum::<f64>() / a.len() as f64
}

fn sorted(a: &[f64]) -> Vec<f64> {
    let mut v = a.to_vec();
    v.sort_by(f64::total_cmp);
    v
}

pub fn median(a: &[f64]) -> f64 {
    median_sorted(&sorted(a))
}

fn median_sorted(v: &[f64]) -> f64 {
    if v.is_empty() {
        return f64::NAN;
    }
    let mid = v.len() / 2;
    if v.len() % 2 == 1 {
        v[mid]
    } else {
        (v[mid - 1] + v[mid]) / 2.0
    }
}

/// The p quantile, 0 ≤ p ≤ 1, interpolating between the sorted values
/// (Excel's PERCENTILE.INC), as Desmos's `quantile`.
pub fn quantile(a: &[f64], p: f64) -> f64 {
    if a.is_empty() || !(0.0..=1.0).contains(&p) {
        return f64::NAN;
    }
    let v = sorted(a);
    let h = (v.len() - 1) as f64 * p;
    let lo = h.floor() as usize;
    let hi = (lo + 1).min(v.len() - 1);
    v[lo] + (h - lo as f64) * (v[hi] - v[lo])
}

/// The q-th quartile, q = 0 to 4: Q1 and Q3 are the medians of the halves
/// below and above the median, leaving the median itself out when the
/// count is odd (the TI-84's method, which Desmos and box plots use).
pub fn quartile(a: &[f64], q: f64) -> f64 {
    let v = sorted(a);
    if v.is_empty() || q.fract() != 0.0 {
        return f64::NAN;
    }
    let n = v.len();
    match q as i64 {
        0 => v[0],
        1 if n == 1 => v[0],
        1 => median_sorted(&v[..n / 2]),
        2 => median_sorted(&v),
        3 if n == 1 => v[0],
        3 => median_sorted(&v[n.div_ceil(2)..]),
        4 => v[n - 1],
        _ => f64::NAN,
    }
}

/// The sample covariance of two lists of the same length.
pub fn cov(a: &[f64], b: &[f64]) -> f64 {
    if a.len() != b.len() || a.len() < 2 {
        return f64::NAN;
    }
    co_sum(a, b) / (a.len() - 1) as f64
}

/// The population covariance.
pub fn covp(a: &[f64], b: &[f64]) -> f64 {
    if a.len() != b.len() || a.is_empty() {
        return f64::NAN;
    }
    co_sum(a, b) / a.len() as f64
}

fn co_sum(a: &[f64], b: &[f64]) -> f64 {
    let (ma, mb) = (mean(a), mean(b));
    a.iter().zip(b).map(|(x, y)| (x - ma) * (y - mb)).sum()
}

/// Pearson's correlation coefficient r.
pub fn corr(a: &[f64], b: &[f64]) -> f64 {
    if a.len() != b.len() || a.len() < 2 {
        return f64::NAN;
    }
    co_sum(a, b) / (squares(a) * squares(b)).sqrt()
}

/// Spearman's rank correlation: Pearson's r of the ranks, with tied
/// values sharing the average of their ranks.
pub fn spearman(a: &[f64], b: &[f64]) -> f64 {
    if a.len() != b.len() {
        return f64::NAN;
    }
    corr(&ranks(a), &ranks(b))
}

fn ranks(a: &[f64]) -> Vec<f64> {
    let mut order: Vec<usize> = (0..a.len()).collect();
    order.sort_by(|&i, &j| a[i].total_cmp(&a[j]));
    let mut out = vec![0.0; a.len()];
    let mut i = 0;
    while i < order.len() {
        let mut j = i;
        while j + 1 < order.len() && a[order[j + 1]] == a[order[i]] {
            j += 1;
        }
        let rank = (i + j) as f64 / 2.0 + 1.0;
        for k in &order[i..=j] {
            out[*k] = rank;
        }
        i = j + 1;
    }
    out
}

/// Each value repeated as often as its frequency says: `freq([1, 2], [3,
/// 1])` is [1, 1, 1, 2]. Frequencies must be whole numbers, 0 or more.
pub fn expand(values: &[f64], counts: &[f64]) -> Result<Vec<f64>, String> {
    if values.len() != counts.len() {
        return Err("freq(L, F) needs one frequency for each value".into());
    }
    let total: f64 = counts.iter().sum();
    if counts.iter().any(|c| *c < 0.0 || c.fract() != 0.0) {
        return Err("Frequencies must be whole numbers, 0 or more".into());
    }
    if total > 10_000_000.0 {
        return Err("Those frequencies add up to too many values".into());
    }
    let mut out = Vec::with_capacity(total as usize);
    for (v, c) in values.iter().zip(counts) {
        out.extend(std::iter::repeat_n(*v, *c as usize));
    }
    Ok(out)
}

/// The five-number summary and the rest of what `stats(L)` shows.
pub fn summary(a: &[f64]) -> Vec<(String, f64)> {
    let n = a.len() as f64;
    let q1 = quartile(a, 1.0);
    let q3 = quartile(a, 3.0);
    let iqr = q3 - q1;
    let outliers = a
        .iter()
        .filter(|x| **x < q1 - 1.5 * iqr || **x > q3 + 1.5 * iqr)
        .count();
    let min = a.iter().copied().fold(f64::INFINITY, f64::min);
    let max = a.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    vec![
        ("n".into(), n),
        ("\\text{mean}".into(), mean(a)),
        ("s".into(), var(a).sqrt()),
        ("\\sigma".into(), varp(a).sqrt()),
        ("\\min".into(), min),
        ("Q_{1}".into(), q1),
        ("\\text{median}".into(), median(a)),
        ("Q_{3}".into(), q3),
        ("\\max".into(), max),
        ("\\text{IQR}".into(), iqr),
        ("\\text{range}".into(), max - min),
        ("\\text{outliers}".into(), outliers as f64),
        ("\\text{total}".into(), a.iter().sum()),
    ]
}

// ---------------------------------------------------------------------------
// Incomplete gamma and beta functions

/// The regularized lower incomplete gamma function P(a, x).
pub fn gamma_p(a: f64, x: f64) -> f64 {
    if x.is_nan() || a.is_nan() || a <= 0.0 {
        return f64::NAN;
    }
    if x <= 0.0 {
        return 0.0;
    }
    if x.is_infinite() {
        return 1.0;
    }
    if x < a + 1.0 {
        gamma_series(a, x)
    } else {
        1.0 - gamma_fraction(a, x)
    }
}

/// The regularized upper incomplete gamma function Q(a, x) = 1 − P(a, x),
/// accurate when it is tiny.
pub fn gamma_q(a: f64, x: f64) -> f64 {
    if x.is_nan() || a.is_nan() || a <= 0.0 {
        return f64::NAN;
    }
    if x <= 0.0 {
        return 1.0;
    }
    if x.is_infinite() {
        return 0.0;
    }
    if x < a + 1.0 {
        1.0 - gamma_series(a, x)
    } else {
        gamma_fraction(a, x)
    }
}

fn gamma_series(a: f64, x: f64) -> f64 {
    let mut ap = a;
    let mut sum = 1.0 / a;
    let mut del = sum;
    for _ in 0..10_000 {
        ap += 1.0;
        del *= x / ap;
        sum += del;
        if del.abs() < sum.abs() * 1e-16 {
            break;
        }
    }
    sum * (-x + a * x.ln() - ln_gamma(a)).exp()
}

/// Q(a, x) by Lentz's continued fraction.
fn gamma_fraction(a: f64, x: f64) -> f64 {
    let tiny = 1e-300;
    let mut b = x + 1.0 - a;
    let mut c = 1.0 / tiny;
    let mut d = 1.0 / b;
    let mut h = d;
    for i in 1..10_000 {
        let an = -(i as f64) * (i as f64 - a);
        b += 2.0;
        d = an * d + b;
        if d.abs() < tiny {
            d = tiny;
        }
        c = b + an / c;
        if c.abs() < tiny {
            c = tiny;
        }
        d = 1.0 / d;
        let del = d * c;
        h *= del;
        if (del - 1.0).abs() < 1e-16 {
            break;
        }
    }
    (-x + a * x.ln() - ln_gamma(a)).exp() * h
}

/// The regularized incomplete beta function I_x(a, b).
pub fn beta_i(x: f64, a: f64, b: f64) -> f64 {
    if x.is_nan() || a <= 0.0 || b <= 0.0 {
        return f64::NAN;
    }
    if x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }
    let front =
        (ln_gamma(a + b) - ln_gamma(a) - ln_gamma(b) + a * x.ln() + b * (1.0 - x).ln()).exp();
    if x < (a + 1.0) / (a + b + 2.0) {
        front * beta_fraction(x, a, b) / a
    } else {
        1.0 - front * beta_fraction(1.0 - x, b, a) / b
    }
}

fn beta_fraction(x: f64, a: f64, b: f64) -> f64 {
    let tiny = 1e-300;
    let (qab, qap, qam) = (a + b, a + 1.0, a - 1.0);
    let mut c = 1.0;
    let mut d = 1.0 - qab * x / qap;
    if d.abs() < tiny {
        d = tiny;
    }
    d = 1.0 / d;
    let mut h = d;
    for m in 1..10_000 {
        let m = m as f64;
        let m2 = 2.0 * m;
        let aa = m * (b - m) * x / ((qam + m2) * (a + m2));
        d = 1.0 + aa * d;
        if d.abs() < tiny {
            d = tiny;
        }
        c = 1.0 + aa / c;
        if c.abs() < tiny {
            c = tiny;
        }
        d = 1.0 / d;
        h *= d * c;
        let aa = -(a + m) * (qab + m) * x / ((a + m2) * (qap + m2));
        d = 1.0 + aa * d;
        if d.abs() < tiny {
            d = tiny;
        }
        c = 1.0 + aa / c;
        if c.abs() < tiny {
            c = tiny;
        }
        d = 1.0 / d;
        let del = d * c;
        h *= del;
        if (del - 1.0).abs() < 1e-16 {
            break;
        }
    }
    h
}

/// ln C(n, k), for real n and k.
fn ln_choose(n: f64, k: f64) -> f64 {
    ln_gamma(n + 1.0) - ln_gamma(k + 1.0) - ln_gamma(n - k + 1.0)
}

// ---------------------------------------------------------------------------
// Distributions

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DistKind {
    Normal,
    T,
    Binomial,
    Poisson,
    Uniform,
    ChiSquared,
    F,
    Exponential,
    Geometric,
    Gamma,
    Beta,
}

/// What can be asked of a distribution: `D.pdf(x)`, `D.cdf(x)` and
/// `D.cdf(a, b)`, and the summaries `mean(D)`, `var(D)`, `quantile(D, p)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DistOp {
    Pdf,
    Cdf,
    /// The probability between two values.
    CdfBetween,
    Quantile,
    Mean,
    Var,
}

pub const DIST_NAMES: &[(&str, DistKind)] = &[
    ("normaldist", DistKind::Normal),
    ("tdist", DistKind::T),
    ("binomialdist", DistKind::Binomial),
    ("poissondist", DistKind::Poisson),
    ("uniformdist", DistKind::Uniform),
    ("chisqdist", DistKind::ChiSquared),
    ("fdist", DistKind::F),
    ("expdist", DistKind::Exponential),
    ("geodist", DistKind::Geometric),
    ("gammadist", DistKind::Gamma),
    ("betadist", DistKind::Beta),
];

impl DistKind {
    pub fn from_name(name: &str) -> Option<DistKind> {
        DIST_NAMES.iter().find(|(n, _)| *n == name).map(|(_, k)| *k)
    }

    pub fn name(self) -> &'static str {
        DIST_NAMES
            .iter()
            .find(|(_, k)| *k == self)
            .map(|(n, _)| *n)
            .unwrap_or("?")
    }

    /// Bars at the whole numbers rather than a curve.
    pub fn discrete(self) -> bool {
        matches!(
            self,
            DistKind::Binomial | DistKind::Poisson | DistKind::Geometric
        )
    }

    /// The values parameters take when left out, and how many must be
    /// given.
    fn defaults(self) -> (usize, [f64; 2]) {
        match self {
            DistKind::Normal => (0, [0.0, 1.0]),
            DistKind::Uniform => (0, [0.0, 1.0]),
            DistKind::Exponential => (0, [1.0, 0.0]),
            DistKind::T | DistKind::ChiSquared | DistKind::Poisson | DistKind::Geometric => {
                (1, [f64::NAN, 0.0])
            }
            DistKind::Binomial => (1, [f64::NAN, 0.5]),
            DistKind::Gamma => (1, [f64::NAN, 1.0]),
            DistKind::F | DistKind::Beta => (2, [f64::NAN; 2]),
        }
    }

    /// How many parameters it takes.
    pub fn params(self) -> usize {
        match self {
            DistKind::Normal
            | DistKind::Uniform
            | DistKind::Binomial
            | DistKind::Gamma
            | DistKind::F
            | DistKind::Beta => 2,
            _ => 1,
        }
    }

    /// How to write it, for errors.
    pub fn usage(self) -> &'static str {
        match self {
            DistKind::Normal => "normaldist(μ, σ), with σ > 0",
            DistKind::T => "tdist(ν), with ν > 0 degrees of freedom",
            DistKind::Binomial => "binomialdist(n, p), with n trials and 0 ≤ p ≤ 1",
            DistKind::Poisson => "poissondist(λ), with mean λ > 0",
            DistKind::Uniform => "uniformdist(a, b), with a < b",
            DistKind::ChiSquared => "chisqdist(k), with k > 0 degrees of freedom",
            DistKind::F => "fdist(d_1, d_2), with both degrees of freedom > 0",
            DistKind::Exponential => "expdist(λ), with rate λ > 0",
            DistKind::Geometric => "geodist(p), the trial of the first success, with 0 < p ≤ 1",
            DistKind::Gamma => "gammadist(k, θ), with shape k > 0 and scale θ > 0",
            DistKind::Beta => "betadist(α, β), with α > 0 and β > 0",
        }
    }
}

/// A distribution with its parameters.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Dist {
    pub kind: DistKind,
    pub p: [f64; 2],
}

impl Dist {
    /// Fills in left-out parameters and checks them.
    pub fn new(kind: DistKind, given: &[f64]) -> Result<Dist, String> {
        let (needed, mut p) = kind.defaults();
        if given.len() < needed || given.len() > kind.params() {
            return Err(format!("Try {}", kind.usage()));
        }
        for (slot, v) in p.iter_mut().zip(given) {
            *slot = *v;
        }
        let d = Dist { kind, p };
        if d.valid() {
            Ok(d)
        } else {
            Err(format!("Try {}", kind.usage()))
        }
    }

    /// From parameters as a call passes them, already filled in.
    pub fn from_args(kind: DistKind, a: &[f64]) -> Option<Dist> {
        let d = Dist {
            kind,
            p: [
                a.first().copied().unwrap_or(f64::NAN),
                a.get(1).copied().unwrap_or(0.0),
            ],
        };
        d.valid().then_some(d)
    }

    fn valid(&self) -> bool {
        let [a, b] = self.p;
        if !a.is_finite() || !b.is_finite() {
            return false;
        }
        match self.kind {
            DistKind::Normal => b > 0.0,
            DistKind::T | DistKind::ChiSquared | DistKind::Poisson | DistKind::Exponential => {
                a > 0.0
            }
            DistKind::Binomial => a >= 0.0 && a.fract() == 0.0 && (0.0..=1.0).contains(&b),
            DistKind::Uniform => a < b,
            DistKind::F | DistKind::Gamma | DistKind::Beta => a > 0.0 && b > 0.0,
            DistKind::Geometric => a > 0.0 && a <= 1.0,
        }
    }

    /// The density, or for a discrete distribution the probability of
    /// exactly `x`.
    pub fn pdf(&self, x: f64) -> f64 {
        let [a, b] = self.p;
        if x.is_nan() {
            return f64::NAN;
        }
        match self.kind {
            DistKind::Normal => {
                let z = (x - a) / b;
                (-0.5 * z * z).exp() / (b * (2.0 * PI).sqrt())
            }
            DistKind::T => {
                let v = a;
                (ln_gamma((v + 1.0) / 2.0)
                    - ln_gamma(v / 2.0)
                    - 0.5 * (v * PI).ln()
                    - (v + 1.0) / 2.0 * (1.0 + x * x / v).ln())
                .exp()
            }
            DistKind::Uniform => {
                if x >= a && x <= b {
                    1.0 / (b - a)
                } else {
                    0.0
                }
            }
            DistKind::ChiSquared => gamma_pdf(x, a / 2.0, 2.0),
            DistKind::Gamma => gamma_pdf(x, a, b),
            DistKind::Exponential => {
                if x < 0.0 {
                    0.0
                } else {
                    a * (-a * x).exp()
                }
            }
            DistKind::F => {
                if x < 0.0 {
                    return 0.0;
                }
                if x == 0.0 {
                    return match a {
                        _ if a < 2.0 => f64::INFINITY,
                        _ if a == 2.0 => 1.0,
                        _ => 0.0,
                    };
                }
                let (d1, d2) = (a, b);
                (0.5 * (d1 * (d1 * x).ln() + d2 * d2.ln() - (d1 + d2) * (d1 * x + d2).ln())
                    - x.ln()
                    - (ln_gamma(d1 / 2.0) + ln_gamma(d2 / 2.0) - ln_gamma((d1 + d2) / 2.0)))
                .exp()
            }
            DistKind::Beta => {
                if !(0.0..=1.0).contains(&x) {
                    return 0.0;
                }
                if (x == 0.0 && a < 1.0) || (x == 1.0 && b < 1.0) {
                    return f64::INFINITY;
                }
                if (x == 0.0 && a > 1.0) || (x == 1.0 && b > 1.0) {
                    return 0.0;
                }
                let ln_b = ln_gamma(a) + ln_gamma(b) - ln_gamma(a + b);
                let part = |v: f64, e: f64| if e == 0.0 { 0.0 } else { e * v.ln() };
                (part(x, a - 1.0) + part(1.0 - x, b - 1.0) - ln_b).exp()
            }
            DistKind::Binomial => {
                if x.fract() != 0.0 || x < 0.0 || x > a {
                    return 0.0;
                }
                if b == 0.0 {
                    return if x == 0.0 { 1.0 } else { 0.0 };
                }
                if b == 1.0 {
                    return if x == a { 1.0 } else { 0.0 };
                }
                (ln_choose(a, x) + x * b.ln() + (a - x) * (1.0 - b).ln()).exp()
            }
            DistKind::Poisson => {
                if x.fract() != 0.0 || x < 0.0 {
                    return 0.0;
                }
                (x * a.ln() - a - ln_gamma(x + 1.0)).exp()
            }
            DistKind::Geometric => {
                if x.fract() != 0.0 || x < 1.0 {
                    return 0.0;
                }
                if a == 1.0 {
                    return if x == 1.0 { 1.0 } else { 0.0 };
                }
                ((x - 1.0) * (1.0 - a).ln()).exp() * a
            }
        }
    }

    /// P(X ≤ x).
    pub fn cdf(&self, x: f64) -> f64 {
        let [a, b] = self.p;
        if x.is_nan() {
            return f64::NAN;
        }
        match self.kind {
            DistKind::Normal => 0.5 * erfc(-(x - a) / (b * SQRT_2)),
            DistKind::T => {
                if x.is_infinite() {
                    return if x > 0.0 { 1.0 } else { 0.0 };
                }
                let tail = 0.5 * beta_i(a / (a + x * x), a / 2.0, 0.5);
                if x > 0.0 {
                    1.0 - tail
                } else {
                    tail
                }
            }
            DistKind::Uniform => ((x - a) / (b - a)).clamp(0.0, 1.0),
            DistKind::ChiSquared => gamma_p(a / 2.0, x.max(0.0) / 2.0),
            DistKind::Gamma => gamma_p(a, x.max(0.0) / b),
            DistKind::Exponential => {
                if x <= 0.0 {
                    0.0
                } else {
                    -(-a * x).exp_m1()
                }
            }
            DistKind::F => {
                if x <= 0.0 {
                    0.0
                } else if x.is_infinite() {
                    1.0
                } else {
                    beta_i(a * x / (a * x + b), a / 2.0, b / 2.0)
                }
            }
            DistKind::Beta => beta_i(x, a, b),
            DistKind::Binomial => {
                let k = x.floor();
                if k < 0.0 {
                    0.0
                } else if k >= a {
                    1.0
                } else if b == 0.0 {
                    1.0
                } else if b == 1.0 {
                    0.0
                } else {
                    beta_i(1.0 - b, a - k, k + 1.0)
                }
            }
            DistKind::Poisson => {
                let k = x.floor();
                if k < 0.0 {
                    0.0
                } else {
                    gamma_q(k + 1.0, a)
                }
            }
            DistKind::Geometric => {
                let k = x.floor();
                if k < 1.0 {
                    0.0
                } else {
                    1.0 - (k * (1.0 - a).ln()).exp()
                }
            }
        }
    }

    /// P(a ≤ X ≤ b). For a discrete distribution both ends count.
    pub fn between(&self, lo: f64, hi: f64) -> f64 {
        let (lo, hi) = if lo <= hi { (lo, hi) } else { (hi, lo) };
        if self.kind.discrete() {
            let below = lo.ceil() - 1.0;
            return (self.cdf(hi) - self.cdf(below)).max(0.0);
        }
        // Far in the upper tail, 1 − cdf loses everything to rounding.
        if lo > self.median() {
            let upper = |x: f64| self.upper(x);
            return (upper(lo) - upper(hi)).max(0.0);
        }
        (self.cdf(hi) - self.cdf(lo)).max(0.0)
    }

    /// P(X > x), accurate in the far tail.
    fn upper(&self, x: f64) -> f64 {
        let [a, b] = self.p;
        match self.kind {
            DistKind::Normal => 0.5 * erfc((x - a) / (b * SQRT_2)),
            DistKind::T if x > 0.0 && x.is_finite() => 0.5 * beta_i(a / (a + x * x), a / 2.0, 0.5),
            DistKind::ChiSquared => gamma_q(a / 2.0, x.max(0.0) / 2.0),
            DistKind::Gamma => gamma_q(a, x.max(0.0) / b),
            DistKind::Exponential if x > 0.0 => (-a * x).exp(),
            DistKind::F if x > 0.0 && x.is_finite() => beta_i(b / (b + a * x), b / 2.0, a / 2.0),
            _ => 1.0 - self.cdf(x),
        }
    }

    pub fn mean(&self) -> f64 {
        let [a, b] = self.p;
        match self.kind {
            DistKind::Normal => a,
            DistKind::T => {
                if a > 1.0 {
                    0.0
                } else {
                    f64::NAN
                }
            }
            DistKind::Binomial => a * b,
            DistKind::Poisson => a,
            DistKind::Uniform => (a + b) / 2.0,
            DistKind::ChiSquared => a,
            DistKind::F => {
                if b > 2.0 {
                    b / (b - 2.0)
                } else {
                    f64::NAN
                }
            }
            DistKind::Exponential => 1.0 / a,
            DistKind::Geometric => 1.0 / a,
            DistKind::Gamma => a * b,
            DistKind::Beta => a / (a + b),
        }
    }

    pub fn var(&self) -> f64 {
        let [a, b] = self.p;
        match self.kind {
            DistKind::Normal => b * b,
            DistKind::T => {
                if a > 2.0 {
                    a / (a - 2.0)
                } else if a > 1.0 {
                    f64::INFINITY
                } else {
                    f64::NAN
                }
            }
            DistKind::Binomial => a * b * (1.0 - b),
            DistKind::Poisson => a,
            DistKind::Uniform => (b - a) * (b - a) / 12.0,
            DistKind::ChiSquared => 2.0 * a,
            DistKind::F => {
                if b > 4.0 {
                    2.0 * b * b * (a + b - 2.0) / (a * (b - 2.0) * (b - 2.0) * (b - 4.0))
                } else if b > 2.0 {
                    f64::INFINITY
                } else {
                    f64::NAN
                }
            }
            DistKind::Exponential => 1.0 / (a * a),
            DistKind::Geometric => (1.0 - a) / (a * a),
            DistKind::Gamma => a * b * b,
            DistKind::Beta => a * b / ((a + b) * (a + b) * (a + b + 1.0)),
        }
    }

    pub fn median(&self) -> f64 {
        self.quantile(0.5)
    }

    /// The smallest x with P(X ≤ x) ≥ p: the inverse of the cdf.
    pub fn quantile(&self, p: f64) -> f64 {
        if !(0.0..=1.0).contains(&p) {
            return f64::NAN;
        }
        let [a, b] = self.p;
        let (lo_end, hi_end) = self.support();
        if p == 0.0 {
            return lo_end;
        }
        if p == 1.0 {
            return hi_end;
        }
        match self.kind {
            DistKind::Uniform => return a + p * (b - a),
            DistKind::Exponential => return -(-p).ln_1p() / a,
            DistKind::Geometric if a == 1.0 => return 1.0,
            DistKind::Geometric => {
                let k = ((1.0 - p).ln() / (1.0 - a).ln()).ceil().max(1.0);
                // Rounding can land one off.
                return if k > 1.0 && self.cdf(k - 1.0) >= p {
                    k - 1.0
                } else {
                    k
                };
            }
            _ => {}
        }
        // Bracket the answer, then halve the bracket.
        let (m, s) = (self.mean(), self.var().sqrt());
        let (m, s) = if m.is_finite() && s.is_finite() && s > 0.0 {
            (m, s)
        } else {
            (0.0, 1.0)
        };
        let mut lo = (m - s).max(lo_end);
        let mut hi = (m + s).min(hi_end);
        let mut step = s;
        while self.cdf(lo) > p && lo > lo_end {
            step *= 2.0;
            lo = (lo - step).max(lo_end);
            if step > 1e300 {
                break;
            }
        }
        step = s;
        while self.cdf(hi) < p && hi < hi_end {
            step *= 2.0;
            hi = (hi + step).min(hi_end);
            if step > 1e300 {
                break;
            }
        }
        if self.kind.discrete() {
            let (mut lo, mut hi) = (lo.floor(), hi.ceil());
            // cdf(lo) < p ≤ cdf(hi), unless lo is already the answer.
            if self.cdf(lo) >= p {
                return lo;
            }
            while hi - lo > 1.0 {
                let mid = ((lo + hi) / 2.0).floor();
                if self.cdf(mid) >= p {
                    hi = mid;
                } else {
                    lo = mid;
                }
            }
            return hi;
        }
        for _ in 0..200 {
            let mid = 0.5 * (lo + hi);
            if mid == lo || mid == hi {
                break;
            }
            if self.cdf(mid) < p {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        0.5 * (lo + hi)
    }

    /// Where the distribution lives.
    pub fn support(&self) -> (f64, f64) {
        let [a, b] = self.p;
        match self.kind {
            DistKind::Normal | DistKind::T => (f64::NEG_INFINITY, f64::INFINITY),
            DistKind::Uniform => (a, b),
            DistKind::Binomial => (0.0, a),
            DistKind::Poisson
            | DistKind::ChiSquared
            | DistKind::F
            | DistKind::Exponential
            | DistKind::Gamma => (0.0, f64::INFINITY),
            DistKind::Geometric => (1.0, f64::INFINITY),
            DistKind::Beta => (0.0, 1.0),
        }
    }

    /// `n` draws, from the random numbers `rng` gives.
    pub fn sample(&self, n: usize, rng: &mut Rng) -> Vec<f64> {
        (0..n)
            .map(|_| match self.kind {
                DistKind::Normal => self.p[0] + self.p[1] * rng.normal(),
                _ => {
                    // Open at both ends, so the quantile is finite.
                    let mut u = rng.next();
                    while u == 0.0 {
                        u = rng.next();
                    }
                    self.quantile(u)
                }
            })
            .collect()
    }

    /// How it reads, as LaTeX: `\operatorname{normaldist}(0, 1)`.
    pub fn latex(&self) -> String {
        let n = self.kind.params();
        let params: Vec<String> = self.p[..n]
            .iter()
            .map(|v| crate::format::value_latex(*v))
            .collect();
        format!(
            "\\operatorname{{{}}}\\left({}\\right)",
            self.kind.name(),
            params.join(",")
        )
    }

    /// What a call to `op` with these parameters works out to, given its
    /// arguments after the parameters.
    pub fn eval(&self, op: DistOp, args: &[f64]) -> f64 {
        let x = args.first().copied().unwrap_or(f64::NAN);
        match op {
            DistOp::Pdf => self.pdf(x),
            DistOp::Cdf => self.cdf(x),
            DistOp::CdfBetween => self.between(x, args.get(1).copied().unwrap_or(f64::NAN)),
            DistOp::Quantile => self.quantile(x),
            DistOp::Mean => self.mean(),
            DistOp::Var => self.var(),
        }
    }
}

/// The gamma density with shape `k` and scale `theta`.
fn gamma_pdf(x: f64, k: f64, theta: f64) -> f64 {
    if x < 0.0 {
        return 0.0;
    }
    if x == 0.0 {
        return match k {
            _ if k < 1.0 => f64::INFINITY,
            _ if k == 1.0 => 1.0 / theta,
            _ => 0.0,
        };
    }
    ((k - 1.0) * x.ln() - x / theta - ln_gamma(k) - k * theta.ln()).exp()
}

// ---------------------------------------------------------------------------
// Random numbers

/// SplitMix64: small, fast and good enough for sampling.
#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed ^ 0x9e37_79b9_7f4a_7c15)
    }

    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// Uniform on [0, 1).
    pub fn next(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// A whole number from 0 to n − 1.
    pub fn below(&mut self, n: usize) -> usize {
        ((self.next() * n as f64) as usize).min(n.saturating_sub(1))
    }

    /// Standard normal, by Box–Muller.
    pub fn normal(&mut self) -> f64 {
        let mut u = self.next();
        while u == 0.0 {
            u = self.next();
        }
        let v = self.next();
        (-2.0 * u.ln()).sqrt() * (2.0 * PI * v).cos()
    }
}

/// A seed from anything hashable, mixed with the graph's seed.
pub fn seed_of(parts: &[u64]) -> u64 {
    let mut rng = Rng::new(0);
    let mut h = 0u64;
    for p in parts {
        rng.0 ^= p.wrapping_mul(0x2545_f491_4f6c_dd1d);
        h ^= rng.next_u64();
    }
    h
}

// ---------------------------------------------------------------------------
// Tests and intervals

/// Which side a test's alternative hypothesis is on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Tail {
    Less,
    #[default]
    NotEqual,
    Greater,
}

/// What a test, interval or summary row shows: named values in order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Table {
    /// Each value with its name as LaTeX.
    pub rows: Vec<(String, f64)>,
    /// Whether the tail picker applies.
    pub tails: bool,
    /// For a test: the hypotheses, as LaTeX.
    pub hypotheses: Option<[String; 2]>,
}

/// The p-value for a statistic with distribution `d` under the null
/// hypothesis.
fn p_value(d: &Dist, stat: f64, tail: Tail) -> f64 {
    match tail {
        Tail::Less => d.cdf(stat),
        Tail::Greater => d.upper(stat),
        Tail::NotEqual => (2.0 * d.cdf(stat).min(d.upper(stat))).min(1.0),
    }
}

fn relation(tail: Tail) -> &'static str {
    match tail {
        Tail::Less => "<",
        Tail::NotEqual => "\\ne",
        Tail::Greater => ">",
    }
}

fn hypotheses(param: &str, null: String, tail: Tail) -> Option<[String; 2]> {
    Some([
        format!("H_{{0}}:{param}={null}"),
        format!("H_{{a}}:{param}{}{null}", relation(tail)),
    ])
}

fn standard_normal() -> Dist {
    Dist {
        kind: DistKind::Normal,
        p: [0.0, 1.0],
    }
}

fn t_dist(df: f64) -> Dist {
    Dist {
        kind: DistKind::T,
        p: [df, 0.0],
    }
}

fn need(a: &[f64], n: usize, what: &str) -> Result<(), String> {
    if a.len() < n {
        return Err(format!("{what} needs at least {n} values"));
    }
    Ok(())
}

/// Desmos's `tscore(L, μ₀)`: how many standard errors the mean is from μ₀.
pub fn tscore(a: &[f64], mu0: f64) -> f64 {
    (mean(a) - mu0) / (var(a).sqrt() / (a.len() as f64).sqrt())
}

/// One-sample t-test of H₀: μ = μ₀.
pub fn ttest(a: &[f64], mu0: f64, tail: Tail) -> Result<Table, String> {
    need(a, 2, "A t-test")?;
    let n = a.len() as f64;
    let s = var(a).sqrt();
    let t = tscore(a, mu0);
    let df = n - 1.0;
    Ok(Table {
        rows: vec![
            ("t".into(), t),
            ("p".into(), p_value(&t_dist(df), t, tail)),
            ("\\text{df}".into(), df),
            ("\\bar{x}".into(), mean(a)),
            ("s".into(), s),
            ("\\text{SE}".into(), s / n.sqrt()),
            ("n".into(), n),
        ],
        tails: true,
        hypotheses: hypotheses("\\mu", crate::format::value_latex(mu0), tail),
    })
}

/// Welch's two-sample t-test of H₀: μ₁ = μ₂, not assuming equal spreads,
/// as Desmos's `ittest`.
pub fn ittest(a: &[f64], b: &[f64], tail: Tail) -> Result<Table, String> {
    need(a, 2, "Each list in a t-test")?;
    need(b, 2, "Each list in a t-test")?;
    let (n1, n2) = (a.len() as f64, b.len() as f64);
    let (v1, v2) = (var(a) / n1, var(b) / n2);
    let se = (v1 + v2).sqrt();
    let t = (mean(a) - mean(b)) / se;
    let df = (v1 + v2).powi(2) / (v1 * v1 / (n1 - 1.0) + v2 * v2 / (n2 - 1.0));
    Ok(Table {
        rows: vec![
            ("t".into(), t),
            ("p".into(), p_value(&t_dist(df), t, tail)),
            ("\\text{df}".into(), df),
            ("\\bar{x}_{1}".into(), mean(a)),
            ("\\bar{x}_{2}".into(), mean(b)),
            ("s_{1}".into(), var(a).sqrt()),
            ("s_{2}".into(), var(b).sqrt()),
            ("\\text{SE}".into(), se),
            ("n_{1}".into(), n1),
            ("n_{2}".into(), n2),
        ],
        tails: true,
        hypotheses: hypotheses("\\mu_{1}", "\\mu_{2}".into(), tail),
    })
}

/// One-sample z-test of H₀: μ = μ₀ with σ known.
pub fn ztest(a: &[f64], mu0: f64, sigma: f64, tail: Tail) -> Result<Table, String> {
    need(a, 1, "A z-test")?;
    if !(sigma > 0.0) {
        return Err("σ must be above 0".into());
    }
    let n = a.len() as f64;
    let se = sigma / n.sqrt();
    let z = (mean(a) - mu0) / se;
    Ok(Table {
        rows: vec![
            ("z".into(), z),
            ("p".into(), p_value(&standard_normal(), z, tail)),
            ("\\bar{x}".into(), mean(a)),
            ("\\text{SE}".into(), se),
            ("n".into(), n),
        ],
        tails: true,
        hypotheses: hypotheses("\\mu", crate::format::value_latex(mu0), tail),
    })
}

fn check_count(x: f64, n: f64) -> Result<(), String> {
    if !(n >= 1.0 && n.fract() == 0.0 && x >= 0.0 && x <= n && x.fract() == 0.0) {
        return Err("Successes and trials must be whole numbers, with successes ≤ trials".into());
    }
    Ok(())
}

/// One-proportion z-test of H₀: p = p₀, from x successes in n trials.
pub fn proptest(x: f64, n: f64, p0: f64, tail: Tail) -> Result<Table, String> {
    check_count(x, n)?;
    if !(p0 > 0.0 && p0 < 1.0) {
        return Err("p₀ must be between 0 and 1".into());
    }
    let p_hat = x / n;
    let se = (p0 * (1.0 - p0) / n).sqrt();
    let z = (p_hat - p0) / se;
    Ok(Table {
        rows: vec![
            ("z".into(), z),
            ("p".into(), p_value(&standard_normal(), z, tail)),
            ("\\hat{p}".into(), p_hat),
            ("\\text{SE}".into(), se),
            ("n".into(), n),
        ],
        tails: true,
        hypotheses: hypotheses("p", crate::format::value_latex(p0), tail),
    })
}

/// Two-proportion z-test of H₀: p₁ = p₂, with the pooled proportion.
pub fn twoproptest(x1: f64, n1: f64, x2: f64, n2: f64, tail: Tail) -> Result<Table, String> {
    check_count(x1, n1)?;
    check_count(x2, n2)?;
    let (p1, p2) = (x1 / n1, x2 / n2);
    let pooled = (x1 + x2) / (n1 + n2);
    let se = (pooled * (1.0 - pooled) * (1.0 / n1 + 1.0 / n2)).sqrt();
    let z = (p1 - p2) / se;
    Ok(Table {
        rows: vec![
            ("z".into(), z),
            ("p".into(), p_value(&standard_normal(), z, tail)),
            ("\\hat{p}_{1}".into(), p1),
            ("\\hat{p}_{2}".into(), p2),
            ("\\hat{p}".into(), pooled),
            ("\\text{SE}".into(), se),
        ],
        tails: true,
        hypotheses: hypotheses("p_{1}", "p_{2}".into(), tail),
    })
}

/// χ² goodness-of-fit test of observed counts against expected ones.
pub fn chisq_gof(observed: &[f64], expected: &[f64]) -> Result<Table, String> {
    if observed.len() != expected.len() || observed.len() < 2 {
        return Err(
            "chisqgof(O, E) needs observed and expected counts, at least 2 of each, one for one"
                .into(),
        );
    }
    if expected.iter().any(|e| !(*e > 0.0)) {
        return Err("Expected counts must be above 0".into());
    }
    let chi2: f64 = observed
        .iter()
        .zip(expected)
        .map(|(o, e)| (o - e) * (o - e) / e)
        .sum();
    let df = observed.len() as f64 - 1.0;
    let d = Dist {
        kind: DistKind::ChiSquared,
        p: [df, 0.0],
    };
    Ok(Table {
        rows: vec![
            ("\\chi^{2}".into(), chi2),
            ("p".into(), d.upper(chi2)),
            ("\\text{df}".into(), df),
        ],
        tails: false,
        hypotheses: None,
    })
}

/// χ² test of independence, each list one row of the table of counts.
pub fn chisq_test(table: &[Vec<f64>]) -> Result<Table, String> {
    let r = table.len();
    let c = table.first().map_or(0, Vec::len);
    if r < 2 || c < 2 || table.iter().any(|row| row.len() != c) {
        return Err(
            "chisqtest(R_1, R_2, …) needs 2 or more rows of counts, all the same length".into(),
        );
    }
    let row_sums: Vec<f64> = table.iter().map(|row| row.iter().sum()).collect();
    let col_sums: Vec<f64> = (0..c)
        .map(|j| table.iter().map(|row| row[j]).sum())
        .collect();
    let total: f64 = row_sums.iter().sum();
    let mut chi2 = 0.0;
    for (i, row) in table.iter().enumerate() {
        for (j, o) in row.iter().enumerate() {
            let e = row_sums[i] * col_sums[j] / total;
            if !(e > 0.0) {
                return Err("Every row and column needs a count above 0".into());
            }
            chi2 += (o - e) * (o - e) / e;
        }
    }
    let df = ((r - 1) * (c - 1)) as f64;
    let d = Dist {
        kind: DistKind::ChiSquared,
        p: [df, 0.0],
    };
    Ok(Table {
        rows: vec![
            ("\\chi^{2}".into(), chi2),
            ("p".into(), d.upper(chi2)),
            ("\\text{df}".into(), df),
        ],
        tails: false,
        hypotheses: None,
    })
}

/// A confidence level as a fraction: 95 and 0.95 both mean 95%.
pub fn level(c: f64) -> Result<f64, String> {
    let c = if c > 1.0 && c < 100.0 { c / 100.0 } else { c };
    if c > 0.0 && c < 1.0 {
        Ok(c)
    } else {
        Err("The confidence level must be between 0 and 1, such as 0.95".into())
    }
}

fn interval(centre: f64, margin: f64, mut rest: Vec<(String, f64)>) -> Table {
    let mut rows = vec![
        ("\\text{lower}".into(), centre - margin),
        ("\\text{upper}".into(), centre + margin),
        ("\\text{margin}".into(), margin),
    ];
    rows.append(&mut rest);
    Table {
        rows,
        tails: false,
        hypotheses: None,
    }
}

/// t interval for a mean.
pub fn tinterval(a: &[f64], c: f64) -> Result<Table, String> {
    need(a, 2, "A t interval")?;
    let c = level(c)?;
    let n = a.len() as f64;
    let se = var(a).sqrt() / n.sqrt();
    let star = t_dist(n - 1.0).quantile((1.0 + c) / 2.0);
    Ok(interval(
        mean(a),
        star * se,
        vec![
            ("\\bar{x}".into(), mean(a)),
            ("t^{*}".into(), star),
            ("\\text{df}".into(), n - 1.0),
            ("\\text{SE}".into(), se),
        ],
    ))
}

/// Welch's t interval for the difference of two means.
pub fn ittinterval(a: &[f64], b: &[f64], c: f64) -> Result<Table, String> {
    need(a, 2, "Each list in a t interval")?;
    need(b, 2, "Each list in a t interval")?;
    let c = level(c)?;
    let (n1, n2) = (a.len() as f64, b.len() as f64);
    let (v1, v2) = (var(a) / n1, var(b) / n2);
    let se = (v1 + v2).sqrt();
    let df = (v1 + v2).powi(2) / (v1 * v1 / (n1 - 1.0) + v2 * v2 / (n2 - 1.0));
    let star = t_dist(df).quantile((1.0 + c) / 2.0);
    let diff = mean(a) - mean(b);
    Ok(interval(
        diff,
        star * se,
        vec![
            ("\\bar{x}_{1}-\\bar{x}_{2}".into(), diff),
            ("t^{*}".into(), star),
            ("\\text{df}".into(), df),
            ("\\text{SE}".into(), se),
        ],
    ))
}

/// z interval for a mean with σ known.
pub fn zinterval(a: &[f64], sigma: f64, c: f64) -> Result<Table, String> {
    need(a, 1, "A z interval")?;
    if !(sigma > 0.0) {
        return Err("σ must be above 0".into());
    }
    let c = level(c)?;
    let se = sigma / (a.len() as f64).sqrt();
    let star = standard_normal().quantile((1.0 + c) / 2.0);
    Ok(interval(
        mean(a),
        star * se,
        vec![
            ("\\bar{x}".into(), mean(a)),
            ("z^{*}".into(), star),
            ("\\text{SE}".into(), se),
        ],
    ))
}

/// z interval for a proportion.
pub fn propinterval(x: f64, n: f64, c: f64) -> Result<Table, String> {
    check_count(x, n)?;
    let c = level(c)?;
    let p = x / n;
    let se = (p * (1.0 - p) / n).sqrt();
    let star = standard_normal().quantile((1.0 + c) / 2.0);
    Ok(interval(
        p,
        star * se,
        vec![
            ("\\hat{p}".into(), p),
            ("z^{*}".into(), star),
            ("\\text{SE}".into(), se),
        ],
    ))
}

/// z interval for the difference of two proportions.
pub fn twopropinterval(x1: f64, n1: f64, x2: f64, n2: f64, c: f64) -> Result<Table, String> {
    check_count(x1, n1)?;
    check_count(x2, n2)?;
    let c = level(c)?;
    let (p1, p2) = (x1 / n1, x2 / n2);
    let se = (p1 * (1.0 - p1) / n1 + p2 * (1.0 - p2) / n2).sqrt();
    let star = standard_normal().quantile((1.0 + c) / 2.0);
    Ok(interval(
        p1 - p2,
        star * se,
        vec![
            ("\\hat{p}_{1}-\\hat{p}_{2}".into(), p1 - p2),
            ("z^{*}".into(), star),
            ("\\text{SE}".into(), se),
        ],
    ))
}

// ---------------------------------------------------------------------------
// Plots

/// How a histogram's bins line up with its bin width.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum BinAlign {
    /// Bins start at multiples of the width: [0, w), [w, 2w), …
    #[default]
    Left,
    /// Bins are centred on multiples of the width.
    Center,
}

/// What a histogram's bars measure.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum BarHeight {
    #[default]
    Count,
    /// The share of all values.
    Relative,
    /// The share per unit across, so the bars' area is 1.
    Density,
}

/// The most bins a histogram draws.
pub const MAX_BINS: usize = 10_000;

/// Each nonempty bin's left edge and how many values fall in it.
pub fn bins(values: &[f64], width: f64, align: BinAlign) -> Result<Vec<(f64, usize)>, String> {
    if !(width > 0.0) {
        return Err("The bin width must be above 0".into());
    }
    let shift = match align {
        BinAlign::Left => 0.0,
        BinAlign::Center => 0.5,
    };
    let mut counts: std::collections::BTreeMap<i64, usize> = Default::default();
    for v in values.iter().filter(|v| v.is_finite()) {
        let k = (v / width + shift).floor();
        if k.abs() > 1e15 {
            continue;
        }
        *counts.entry(k as i64).or_default() += 1;
    }
    if let (Some((lo, _)), Some((hi, _))) = (counts.first_key_value(), counts.last_key_value()) {
        if (hi - lo) as usize > MAX_BINS {
            return Err("That bin width makes too many bins; try a wider one".into());
        }
    }
    Ok(counts
        .into_iter()
        .map(|(k, n)| ((k as f64 - shift) * width, n))
        .collect())
}

/// A bar's height for `count` of `total` values in a bin `width` wide.
pub fn bar_height(count: usize, total: usize, width: f64, height: BarHeight) -> f64 {
    match height {
        BarHeight::Count => count as f64,
        BarHeight::Relative => count as f64 / total as f64,
        BarHeight::Density => count as f64 / (total as f64 * width),
    }
}

/// Where each dot of a dot plot goes: values with the same bin (or, with
/// no width, the same value) stack up from 1.
pub fn dots(values: &[f64], width: Option<f64>, align: BinAlign) -> Result<Vec<[f64; 2]>, String> {
    let mut out = Vec::new();
    match width {
        Some(w) => {
            if !(w > 0.0) {
                return Err("The bin width must be above 0".into());
            }
            // Dots sit in the middle of their bin.
            let mut stacks: std::collections::BTreeMap<i64, usize> = Default::default();
            for v in values.iter().filter(|v| v.is_finite()) {
                let (k, centre) = match align {
                    BinAlign::Left => {
                        let k = (v / w).floor();
                        (k, (k + 0.5) * w)
                    }
                    BinAlign::Center => {
                        let k = (v / w + 0.5).floor();
                        (k, k * w)
                    }
                };
                let n = stacks.entry(k as i64).or_default();
                *n += 1;
                out.push([centre, *n as f64]);
            }
        }
        None => {
            let mut stacks: Vec<(f64, usize)> = Vec::new();
            for v in values.iter().filter(|v| v.is_finite()) {
                let n = match stacks.iter_mut().find(|(x, _)| x == v) {
                    Some((_, n)) => {
                        *n += 1;
                        *n
                    }
                    None => {
                        stacks.push((*v, 1));
                        1
                    }
                };
                out.push([*v, n as f64]);
            }
        }
    }
    Ok(out)
}

/// What a box plot draws.
#[derive(Clone, Debug, PartialEq)]
pub struct BoxPlot {
    /// The box's two halves, either side of the median.
    pub boxes: [[f64; 2]; 2],
    /// The whiskers' far ends.
    pub whiskers: [f64; 2],
    pub outliers: Vec<f64>,
}

/// A box plot of `values`. Without `exclude_outliers` the whiskers reach
/// the minimum and maximum; with it they stop at the furthest values
/// within 1.5 IQR of the box and the rest are drawn as dots.
pub fn box_plot(values: &[f64], exclude_outliers: bool) -> Option<BoxPlot> {
    let v: Vec<f64> = values.iter().copied().filter(|v| v.is_finite()).collect();
    if v.is_empty() {
        return None;
    }
    let (q1, med, q3) = (quartile(&v, 1.0), median(&v), quartile(&v, 3.0));
    let iqr = q3 - q1;
    let (lo_fence, hi_fence) = if exclude_outliers {
        (q1 - 1.5 * iqr, q3 + 1.5 * iqr)
    } else {
        (f64::NEG_INFINITY, f64::INFINITY)
    };
    let inside: Vec<f64> = v
        .iter()
        .copied()
        .filter(|x| *x >= lo_fence && *x <= hi_fence)
        .collect();
    let outliers = v
        .iter()
        .copied()
        .filter(|x| *x < lo_fence || *x > hi_fence)
        .collect();
    let lo = inside.iter().copied().fold(f64::INFINITY, f64::min).min(q1);
    let hi = inside
        .iter()
        .copied()
        .fold(f64::NEG_INFINITY, f64::max)
        .max(q3);
    Some(BoxPlot {
        boxes: [[q1, med], [med, q3]],
        whiskers: [lo, hi],
        outliers,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol * (1.0 + b.abs())
    }

    #[test]
    fn summaries() {
        let a = [2.0, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0];
        assert!(close(varp(&a).sqrt(), 2.0, 1e-12));
        assert!(close(var(&a), 32.0 / 7.0, 1e-12));
        assert!(close(mad(&a), 1.5, 1e-12));
        assert_eq!(quantile(&[1.0, 2.0, 3.0, 4.0], 0.25), 1.75);
        // TI-84 quartiles: 1, 2, 3, 4, 5, 6, 7 has Q1 = 2, Q3 = 6.
        let b = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0];
        assert_eq!(quartile(&b, 1.0), 2.0);
        assert_eq!(quartile(&b, 3.0), 6.0);
        let c = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0];
        assert_eq!(quartile(&c, 1.0), 2.0);
        assert_eq!(quartile(&c, 3.0), 5.0);
        assert!(close(
            corr(&[1.0, 2.0, 3.0], &[2.0, 4.0, 7.0]),
            0.993_399,
            1e-5
        ));
        assert!(close(
            spearman(&[1.0, 2.0, 3.0], &[1.0, 8.0, 27.0]),
            1.0,
            1e-12
        ));
        assert!(close(cov(&[1.0, 2.0, 3.0], &[1.0, 2.0, 3.0]), 1.0, 1e-12));
    }

    #[test]
    fn distributions() {
        let n = Dist::new(DistKind::Normal, &[]).unwrap();
        assert!(close(n.cdf(1.96), 0.975_002, 1e-5));
        assert!(close(n.quantile(0.975), 1.959_964, 1e-6));
        assert!(close(n.between(-1.0, 1.0), 0.682_689, 1e-5));
        let t = Dist::new(DistKind::T, &[10.0]).unwrap();
        assert!(close(t.cdf(2.228), 0.975, 1e-4));
        assert!(close(t.quantile(0.975), 2.228_139, 1e-6));
        let b = Dist::new(DistKind::Binomial, &[10.0, 0.3]).unwrap();
        assert!(close(b.pdf(3.0), 0.266_828, 1e-5));
        assert!(close(b.cdf(3.0), 0.649_611, 1e-5));
        assert!(close(b.between(2.0, 4.0), 0.700_423, 1e-5));
        assert_eq!(b.quantile(0.5), 3.0);
        let p = Dist::new(DistKind::Poisson, &[4.0]).unwrap();
        assert!(close(p.pdf(2.0), 0.146_525, 1e-5));
        assert!(close(p.cdf(2.0), 0.238_103, 1e-5));
        let c = Dist::new(DistKind::ChiSquared, &[3.0]).unwrap();
        assert!(close(c.cdf(7.815), 0.95, 1e-4));
        let f = Dist::new(DistKind::F, &[5.0, 10.0]).unwrap();
        assert!(close(f.cdf(3.326), 0.95, 1e-4));
        let g = Dist::new(DistKind::Geometric, &[0.25]).unwrap();
        assert!(close(g.cdf(3.0), 1.0 - 0.75f64.powi(3), 1e-12));
        assert_eq!(g.quantile(0.5), 3.0);
        let be = Dist::new(DistKind::Beta, &[2.0, 3.0]).unwrap();
        assert!(close(be.cdf(0.5), 0.6875, 1e-10));
        let ga = Dist::new(DistKind::Gamma, &[2.0, 2.0]).unwrap();
        assert!(close(ga.cdf(4.0), 1.0 - 3.0 * (-2.0f64).exp(), 1e-10));
        assert!(Dist::new(DistKind::Normal, &[0.0, -1.0]).is_err());
        assert!(Dist::new(DistKind::T, &[]).is_err());
        // Means and variances of samples land near the distribution's.
        let mut rng = Rng::new(7);
        let s = ga.sample(20_000, &mut rng);
        assert!(close(mean(&s), 4.0, 0.03));
    }

    #[test]
    fn tests_and_intervals() {
        let a = [5.1, 4.9, 5.6, 5.8, 6.0, 5.2, 4.7, 5.5];
        let t = ttest(&a, 5.0, Tail::NotEqual).unwrap();
        assert!(close(t.rows[0].1, 2.197_86, 1e-4));
        assert!(close(t.rows[1].1, 0.063_98, 2e-3));
        let iv = tinterval(&a, 0.95).unwrap();
        assert!(iv.rows[0].1 < mean(&a) && iv.rows[1].1 > mean(&a));
        let z = proptest(60.0, 100.0, 0.5, Tail::Greater).unwrap();
        assert!(close(z.rows[0].1, 2.0, 1e-12));
        assert!(close(z.rows[1].1, 0.022_75, 1e-3));
        let g = chisq_gof(&[50.0, 30.0, 20.0], &[40.0, 40.0, 20.0]).unwrap();
        assert!(close(g.rows[0].1, 5.0, 1e-12));
        let ind = chisq_test(&[vec![10.0, 20.0], vec![20.0, 10.0]]).unwrap();
        assert!(close(ind.rows[0].1, 6.666_667, 1e-5));
    }

    #[test]
    fn plots() {
        let b = bins(&[0.5, 1.2, 1.7, 3.0], 1.0, BinAlign::Left).unwrap();
        assert_eq!(b, vec![(0.0, 1), (1.0, 2), (3.0, 1)]);
        let c = bins(&[0.4, 0.6, 1.4], 1.0, BinAlign::Center).unwrap();
        assert_eq!(c, vec![(-0.5, 1), (0.5, 2)]);
        let d = dots(&[1.0, 2.0, 1.0], None, BinAlign::Left).unwrap();
        assert_eq!(d, vec![[1.0, 1.0], [2.0, 1.0], [1.0, 2.0]]);
        let bp = box_plot(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 100.0], true).unwrap();
        assert_eq!(bp.outliers, vec![100.0]);
        assert_eq!(bp.whiskers, [1.0, 7.0]);
    }
}
