//! Infinite sums and products worked out numerically, with a check that
//! they converge. The partial sums are taken at 1, 2, 4, … terms; the tail
//! is estimated by extrapolating them (Wynn's epsilon algorithm, which
//! takes p-series and geometric tails in its stride) or by averaging the
//! last few (the Euler transform's trick for alternating series), and the
//! estimate is trusted only when two independent ways of making it agree.
//! A series whose terms don't shrink to zero, or whose blocks of terms
//! stop shrinking (as 1/n's do), is reported as diverging.

/// What an infinite series does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Outcome {
    /// It converges to `value`, known to about `error`.
    Converges { value: f64, error: f64 },
    /// It grows without bound, to +∞ (1) or -∞ (-1).
    Infinite(i8),
    /// Its terms don't go to zero (a product's: to one), so it never
    /// settles down.
    TermsDontVanish,
    /// The terms shrink but the partial sums keep wandering, or
    /// settle too slowly to be sure of.
    Unsure,
    /// A term is undefined.
    Undefined,
}

impl Outcome {
    /// The number a calculator shows: the value, ±∞, or NaN (undefined).
    pub fn number(self) -> f64 {
        match self {
            Outcome::Converges { value, error } => rounded(value, error),
            Outcome::Infinite(s) => f64::INFINITY * s as f64,
            _ => f64::NAN,
        }
    }
}

/// `value` without the digits `error` makes doubtful.
pub fn rounded(value: f64, error: f64) -> f64 {
    if error.is_nan() || error <= 0.0 || !value.is_finite() || error < 1e-13 * value.abs() {
        return value;
    }
    let step = 10f64.powf((error * 10.0).log10().ceil());
    (value / step).round() * step
}

thread_local! {
    static LAST: std::cell::Cell<Option<Outcome>> = const { std::cell::Cell::new(None) };
}

/// Notes what the engine's last infinite sum or product did, so Solve can
/// say why a series diverges and not only that it is undefined.
pub(crate) fn record(o: Outcome) {
    LAST.with(|l| l.set(Some(o)));
}

/// What the infinite sum or product written as `latex` does, worked out
/// by the engine exactly as the calculator would, with `others` the other
/// rows. None when it isn't one or a term isn't a real number.
pub fn outcome_of(latex: &str, others: &[String]) -> Option<Outcome> {
    let mut rows: Vec<crate::RowInput> = others
        .iter()
        .enumerate()
        .map(|(i, l)| crate::RowInput {
            id: i as u64 + 1,
            latex: l.clone(),
            ..Default::default()
        })
        .collect();
    rows.push(crate::RowInput {
        id: 0,
        latex: latex.to_owned(),
        ..Default::default()
    });
    LAST.with(|l| l.set(None));
    std::panic::catch_unwind(|| crate::analyze(&rows)).ok()?;
    LAST.with(|l| l.take())
}

/// Most terms looked at (2^20, about a million).
pub const MAX_TERMS: usize = 1 << 20;

/// `\sum_{k=lo}^{hi} term(k)` where either bound may be infinite. None when
/// a term can't be worked out as a real number.
pub fn sum(
    term: &mut dyn FnMut(f64) -> Option<f64>,
    lo: f64,
    hi: f64,
    budget: usize,
) -> Option<Outcome> {
    if lo.is_nan() || hi.is_nan() {
        return Some(Outcome::Undefined);
    }
    match (lo.is_infinite(), hi.is_infinite()) {
        (false, false) => {
            let mut total = 0.0;
            let mut k = lo.round();
            while k <= hi.round() {
                total += term(k)?;
                k += 1.0;
            }
            Some(Outcome::Converges {
                value: total,
                error: 0.0,
            })
        }
        // An empty range: from +∞ up, or down to -∞.
        _ if lo == f64::INFINITY || hi == f64::NEG_INFINITY => Some(Outcome::Converges {
            value: 0.0,
            error: 0.0,
        }),
        (false, true) => tail(term, lo.round(), 1.0, budget),
        (true, false) => tail(term, hi.round(), -1.0, budget),
        (true, true) => {
            // From 0 up and from -1 down.
            let up = tail(term, 0.0, 1.0, budget)?;
            let down = tail(term, -1.0, -1.0, budget)?;
            Some(combine(up, down))
        }
    }
}

/// `\prod_{k=lo}^{hi} term(k)` where either bound may be infinite, by the
/// sum of the logarithms of the terms.
pub fn product(
    term: &mut dyn FnMut(f64) -> Option<f64>,
    lo: f64,
    hi: f64,
    budget: usize,
) -> Option<Outcome> {
    if lo.is_nan() || hi.is_nan() {
        return Some(Outcome::Undefined);
    }
    if !lo.is_infinite() && !hi.is_infinite() {
        let mut total = 1.0;
        let mut k = lo.round();
        while k <= hi.round() {
            total *= term(k)?;
            k += 1.0;
        }
        return Some(Outcome::Converges {
            value: total,
            error: 0.0,
        });
    }
    // A zero term makes the whole product zero; a negative one flips its
    // sign. Both are found while the logarithms are summed.
    let mut zero = false;
    let mut negatives = 0usize;
    let mut last_negative = 0usize;
    let mut seen = 0usize;
    let mut logs = |k: f64| -> Option<f64> {
        let t = term(k)?;
        seen += 1;
        if t.is_nan() {
            return Some(f64::NAN);
        }
        if t == 0.0 {
            zero = true;
            return Some(0.0);
        }
        if t < 0.0 {
            negatives += 1;
            last_negative = seen;
        }
        Some(t.abs().ln())
    };
    let logged = sum(&mut logs, lo, hi, budget)?;
    if zero {
        return Some(Outcome::Converges {
            value: 0.0,
            error: 0.0,
        });
    }
    // Negative terms all the way out: the sign never settles.
    let flips_forever = last_negative > seen / 2 && negatives > 4;
    let sign = if negatives % 2 == 1 { -1.0 } else { 1.0 };
    Some(match logged {
        Outcome::Converges { value, error } if !flips_forever => {
            let v = sign * value.exp();
            Outcome::Converges {
                value: v,
                error: v.abs() * error,
            }
        }
        // The logarithms run off to -∞: the product shrinks to zero.
        Outcome::Infinite(-1) => Outcome::Converges {
            value: 0.0,
            error: 0.0,
        },
        Outcome::Infinite(_) if !flips_forever => Outcome::Infinite(sign as i8),
        Outcome::Converges { .. } | Outcome::Infinite(_) => Outcome::TermsDontVanish,
        other => other,
    })
}

fn combine(a: Outcome, b: Outcome) -> Outcome {
    use Outcome::*;
    match (a, b) {
        (Converges { value: x, error: e }, Converges { value: y, error: f }) => Converges {
            value: x + y,
            error: e + f,
        },
        (Undefined, _) | (_, Undefined) => Undefined,
        (TermsDontVanish, _) | (_, TermsDontVanish) => TermsDontVanish,
        (Infinite(s), Infinite(t)) if s != t => Unsure,
        (Infinite(s), _) | (_, Infinite(s)) => Infinite(s),
        _ => Unsure,
    }
}

/// How many partial sums are kept beside each checkpoint, for averaging.
const WINDOW: usize = 24;

/// `term(start) + term(start + step) + …` for ever.
fn tail(
    term: &mut dyn FnMut(f64) -> Option<f64>,
    start: f64,
    step: f64,
    budget: usize,
) -> Option<Outcome> {
    let budget = budget.clamp(64, MAX_TERMS);
    // Kahan summation, so a million terms don't lose digits.
    let (mut total, mut carry) = (0.0f64, 0.0f64);
    // Partial sums after 2^j terms, the largest |term| in each block of
    // terms 2^(j-1)+1 ..= 2^j, each block's sum and whether its terms all
    // have one sign.
    let mut checkpoints: Vec<f64> = Vec::new();
    let mut block_max: Vec<f64> = Vec::new();
    let mut block_sum: Vec<f64> = Vec::new();
    let mut block_sign: Vec<i8> = Vec::new();
    // The last WINDOW partial sums at each checkpoint.
    let mut windows: Vec<Vec<f64>> = Vec::new();
    let mut recent: std::collections::VecDeque<f64> = std::collections::VecDeque::new();
    let (mut bmax, mut bsum, mut bsign) = (0.0f64, 0.0f64, 0i8);
    let mut next_checkpoint = 1usize;
    let mut previous_abs = f64::NAN;
    let mut shrinking_run = 0usize;
    let mut k = 0usize;
    while k < budget {
        let t = term(start + step * k as f64)?;
        k += 1;
        if t.is_nan() {
            return Some(Outcome::Undefined);
        }
        if t.is_infinite() {
            return Some(Outcome::Infinite(t.signum() as i8));
        }
        let y = t - carry;
        let s = total + y;
        carry = (s - total) - y;
        total = s;
        recent.push_back(total);
        if recent.len() > WINDOW {
            recent.pop_front();
        }
        bmax = bmax.max(t.abs());
        bsum += t;
        let sg = if t > 0.0 {
            1
        } else if t < 0.0 {
            -1
        } else {
            0
        };
        bsign = match (bsign, sg) {
            (0, s) | (s, 0) => s,
            (a, b) if a == b => a,
            _ => 2,
        };
        if k == next_checkpoint {
            checkpoints.push(total);
            block_max.push(bmax);
            block_sum.push(bsum);
            block_sign.push(bsign);
            windows.push(recent.iter().copied().collect());
            bmax = 0.0;
            bsum = 0.0;
            bsign = 0;
            next_checkpoint *= 2;
        }
        // Terms far below the total that keep shrinking fast (geometric or
        // faster): the rest can't change the answer.
        let a = t.abs();
        if a <= 1e-18 * total.abs().max(1e-300) || a == 0.0 {
            if previous_abs.is_nan() || a <= 0.9 * previous_abs || a == 0.0 {
                shrinking_run += 1;
            } else {
                shrinking_run = 0;
            }
        } else {
            shrinking_run = 0;
        }
        previous_abs = a;
        if shrinking_run >= 32 && k >= 64 {
            return Some(Outcome::Converges {
                value: total,
                error: total.abs() * 1e-15,
            });
        }
        if !total.is_finite() {
            return Some(Outcome::Infinite(total.signum() as i8));
        }
    }
    Some(judge(
        &checkpoints,
        &block_max,
        &block_sum,
        &block_sign,
        &windows,
    ))
}

/// Whether the partial sums converge, and to what.
fn judge(
    checkpoints: &[f64],
    block_max: &[f64],
    block_sum: &[f64],
    block_sign: &[i8],
    windows: &[Vec<f64>],
) -> Outcome {
    let j = checkpoints.len();
    if j < 8 {
        return Outcome::Unsure;
    }
    let last = checkpoints[j - 1];
    let scale = 1.0 + last.abs();
    // The terms don't shrink: the latest blocks' largest terms are as big
    // as before.
    let (m1, m2, m3) = (block_max[j - 1], block_max[j - 2], block_max[j - 3]);
    if m1 > 1e-9 * scale && m1 >= 0.98 * m2 && m2 >= 0.98 * m3 {
        // Terms of one sign that don't shrink add up without bound.
        let one_sign = block_sign[j - 1].abs() == 1 && block_sign[j - 2] == block_sign[j - 1];
        return if one_sign {
            Outcome::Infinite(block_sign[j - 1])
        } else {
            Outcome::TermsDontVanish
        };
    }
    // Terms of one sign: compare the sums of successive blocks, each
    // twice as long as the one before. For terms like 1/n^p they shrink by
    // 2^(1-p) a block, so a ratio near 1 means p ≤ 1 and divergence.
    let one_sign =
        (j - 4..j).all(|i| block_sign[i] == block_sign[j - 1]) && block_sign[j - 1].abs() == 1;
    if one_sign {
        let r: Vec<f64> = (j - 3..j)
            .map(|i| block_sum[i] / block_sum[i - 1])
            .collect();
        if r.iter().all(|r| *r >= 0.96) && block_sum[j - 1].abs() > 1e-12 * scale {
            return Outcome::Infinite(block_sign[j - 1]);
        }
    }
    // Estimates of the limit, each with a rough error.
    let mut candidates: Vec<(f64, f64)> = Vec::new();
    // The plain partial sum.
    candidates.push((last, (last - checkpoints[j - 2]).abs()));
    // Wynn's epsilon over the partial sums at 2^4, 2^5, … terms, against the
    // same without the last.
    let from = 4.min(j - 6);
    if let (Some(a), Some(b)) = (
        epsilon(&checkpoints[from..j]),
        epsilon(&checkpoints[from..j - 1]),
    ) {
        candidates.push((a, (a - b).abs()));
    }
    // Repeated averaging of the last partial sums, against the same at the
    // checkpoint before.
    if let (Some(a), Some(b)) = (average(&windows[j - 1]), average(&windows[j - 2])) {
        candidates.push((a, (a - b).abs()));
    }
    let (value, error) = candidates
        .into_iter()
        .filter(|(v, e)| v.is_finite() && e.is_finite())
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .unwrap_or((f64::NAN, f64::INFINITY));
    if error <= 1e-6 * (1.0 + value.abs()) {
        Outcome::Converges { value, error }
    } else {
        Outcome::Unsure
    }
}

/// Wynn's epsilon algorithm: the limit of `s`, extrapolated.
fn epsilon(s: &[f64]) -> Option<f64> {
    let n = s.len();
    if n < 3 {
        return None;
    }
    // e[k] holds column k; columns of odd index are auxiliary.
    let mut prev: Vec<f64> = vec![0.0; n + 1];
    let mut cur: Vec<f64> = s.to_vec();
    let mut best = *s.last()?;
    let mut col = 0;
    while cur.len() > 1 {
        let mut next = Vec::with_capacity(cur.len() - 1);
        for i in 0..cur.len() - 1 {
            let d = cur[i + 1] - cur[i];
            if d == 0.0 || !d.is_finite() {
                // The sequence has settled exactly at this column.
                return if col % 2 == 0 {
                    Some(cur[i + 1])
                } else {
                    Some(best)
                };
            }
            next.push(prev[i + 1] + 1.0 / d);
        }
        prev = cur;
        cur = next;
        col += 1;
        if col % 2 == 0 {
            if let Some(v) = cur.last() {
                if v.is_finite() {
                    best = *v;
                }
            }
        }
    }
    Some(best)
}

/// The last partial sums averaged pairwise again and again, which cancels
/// the back-and-forth of an alternating series.
fn average(w: &[f64]) -> Option<f64> {
    if w.len() < 4 {
        return None;
    }
    let mut cur = w.to_vec();
    while cur.len() > 1 {
        cur = cur.windows(2).map(|p| 0.5 * (p[0] + p[1])).collect();
    }
    cur.first().copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(f: fn(f64) -> f64, lo: f64, hi: f64) -> Outcome {
        sum(&mut |k| Some(f(k)), lo, hi, MAX_TERMS).unwrap()
    }
    fn p(f: fn(f64) -> f64, lo: f64, hi: f64) -> Outcome {
        product(&mut |k| Some(f(k)), lo, hi, MAX_TERMS).unwrap()
    }
    fn near(o: Outcome, want: f64, tol: f64) {
        match o {
            Outcome::Converges { value, .. } => {
                assert!((value - want).abs() <= tol, "{value} vs {want}")
            }
            other => panic!("{other:?}, wanted {want}"),
        }
    }

    #[test]
    fn convergent_sums() {
        let inf = f64::INFINITY;
        near(
            s(|n| 1.0 / (n * n), 1.0, inf),
            std::f64::consts::PI.powi(2) / 6.0,
            1e-10,
        );
        near(s(|n| 0.5f64.powf(n), 0.0, inf), 2.0, 1e-14);
        near(s(|n| (-1f64).powf(n + 1.0) / n, 1.0, inf), 2f64.ln(), 1e-10);
        near(s(|n| 1.0 / n.powi(3), 1.0, inf), 1.2020569031595942, 1e-10);
        near(
            s(|n| (-(n * n)).exp(), -inf, inf),
            1.7726372048266521,
            1e-12,
        );
        near(
            s(|n| 1.0 / (n * n), -inf, -1.0),
            std::f64::consts::PI.powi(2) / 6.0,
            1e-10,
        );
        // Leibniz: π/4, slowly by hand.
        near(
            s(|n| (-1f64).powf(n) / (2.0 * n + 1.0), 0.0, inf),
            std::f64::consts::FRAC_PI_4,
            1e-10,
        );
        near(s(|n| n.sin() / (n * n), 1.0, inf), 1.0139591323607684, 1e-6);
    }

    #[test]
    fn divergent_sums() {
        let inf = f64::INFINITY;
        assert_eq!(s(|n| 1.0 / n, 1.0, inf), Outcome::Infinite(1));
        assert_eq!(s(|n| -1.0 / n.sqrt(), 1.0, inf), Outcome::Infinite(-1));
        assert_eq!(s(|n| n, 1.0, inf), Outcome::Infinite(1));
        assert_eq!(s(|n| (-1f64).powf(n), 0.0, inf), Outcome::TermsDontVanish);
        assert_eq!(s(|n| n.sin(), 0.0, inf), Outcome::TermsDontVanish);
        assert!(matches!(
            s(|n| 2f64.powf(n), 0.0, inf),
            Outcome::Infinite(1)
        ));
    }

    #[test]
    fn products() {
        let inf = f64::INFINITY;
        near(p(|n| 1.0 - 1.0 / (n * n), 2.0, inf), 0.5, 1e-9);
        // Wallis: π/2.
        near(
            p(|n| 4.0 * n * n / (4.0 * n * n - 1.0), 1.0, inf),
            std::f64::consts::FRAC_PI_2,
            1e-9,
        );
        near(
            p(|n| 1.0 + 1.0 / (n * n), 1.0, inf),
            3.676077910374978,
            1e-9,
        );
        near(p(|n| 1.0 - 1.0 / n, 2.0, inf), 0.0, 1e-12);
        assert_eq!(p(|n| 1.0 + 1.0 / n, 1.0, inf), Outcome::Infinite(1));
        assert_eq!(p(|_| 2.0, 1.0, inf), Outcome::Infinite(1));
        assert_eq!(p(|_| -1.0, 1.0, inf), Outcome::TermsDontVanish);
    }
}
