//! Polynomials in one variable: reading them out of expressions, exact
//! arithmetic on rational coefficients, rational roots and factoring.

use super::sym::*;

/// The coefficients of `s` as a polynomial in `v`, lowest power first,
/// when it is one. Coefficients may contain other letters.
pub fn coeffs(s: &Sym, v: &str) -> Option<Vec<Sym>> {
    let s = expand(s);
    let mut out: Vec<Vec<Sym>> = Vec::new();
    for t in s.terms() {
        let mut k = 0usize;
        let mut rest = Vec::new();
        for f in t.factors() {
            match &f {
                Sym::Var(name) if name == v => k += 1,
                Sym::Pow(b, e) if matches!(&**b, Sym::Var(name) if name == v) => {
                    let q = e.as_q()?;
                    if !q.is_int() || q.n < 0 || q.n > 200 {
                        return None;
                    }
                    k += q.n as usize;
                }
                other if other.has(v) => return None,
                other => rest.push(other.clone()),
            }
        }
        if out.len() <= k {
            out.resize(k + 1, Vec::new());
        }
        out[k].push(mul(rest));
    }
    let mut c: Vec<Sym> = out.into_iter().map(add).collect();
    while c.len() > 1 && c.last().is_some_and(Sym::is_zero) {
        c.pop();
    }
    if c.is_empty() {
        c.push(n(0));
    }
    Some(c)
}

/// Builds Σ cₖ vᵏ.
pub fn build(c: &[Sym], v: &str) -> Sym {
    add(c
        .iter()
        .enumerate()
        .map(|(k, a)| mul2(a.clone(), powi(var(v), k as i128)))
        .collect())
}

/// A polynomial with exact rational coefficients, lowest power first.
pub type P = Vec<Q>;

pub fn rational(c: &[Sym]) -> Option<P> {
    c.iter().map(Sym::as_q).collect()
}

pub fn to_sym(p: &[Q]) -> Vec<Sym> {
    p.iter().map(|q| Sym::Num(*q)).collect()
}

pub fn trim(mut p: P) -> P {
    while p.len() > 1 && p.last().is_some_and(|q| q.is_zero()) {
        p.pop();
    }
    p
}

pub fn deg(p: &[Q]) -> usize {
    p.len().saturating_sub(1)
}

pub fn eval(p: &[Q], x: Q) -> Option<Q> {
    let mut acc = Q::ZERO;
    for c in p.iter().rev() {
        acc = acc.mul(x)?.add(*c)?;
    }
    Some(acc)
}

/// Quotient and remainder of a ÷ b.
pub fn divmod(a: &[Q], b: &[Q]) -> Option<(P, P)> {
    let b = trim(b.to_vec());
    let lead = *b.last()?;
    if lead.is_zero() {
        return None;
    }
    let mut r = trim(a.to_vec());
    if r.len() < b.len() {
        return Some((vec![Q::ZERO], r));
    }
    let mut q = vec![Q::ZERO; r.len() - b.len() + 1];
    while r.len() >= b.len() && !(r.len() == 1 && r[0].is_zero()) {
        let k = r.len() - b.len();
        let c = r.last()?.div(lead)?;
        q[k] = c;
        for (i, bi) in b.iter().enumerate() {
            r[i + k] = r[i + k].sub(c.mul(*bi)?)?;
        }
        r.pop();
        r = trim(r);
        if r.len() < b.len() {
            break;
        }
    }
    Some((trim(q), r))
}

/// The monic greatest common divisor.
pub fn gcd_poly(a: &[Q], b: &[Q]) -> Option<P> {
    let mut a = trim(a.to_vec());
    let mut b = trim(b.to_vec());
    while !(b.len() == 1 && b[0].is_zero()) {
        let (_, r) = divmod(&a, &b)?;
        a = b;
        b = r;
    }
    let lead = *a.last()?;
    a.iter().map(|c| c.div(lead)).collect()
}

/// Scales to whole-number coefficients with no common factor, positive
/// leading one; returns the scale taken out.
pub fn primitive(p: &[Q]) -> Option<(Q, Vec<i128>)> {
    let mut l = 1i128;
    for c in p {
        l = (l / gcd(l, c.d)).checked_mul(c.d)?;
    }
    let ints: Vec<i128> = p
        .iter()
        .map(|c| c.mul(Q::int(l)).map(|q| q.n))
        .collect::<Option<_>>()?;
    let mut g = 0i128;
    for &c in &ints {
        g = gcd(g, c);
    }
    if g == 0 {
        return None;
    }
    if ints.last().is_some_and(|&c| c < 0) {
        g = -g;
    }
    let ints: Vec<i128> = ints.iter().map(|c| c / g).collect();
    Some((Q::new(g, l)?, ints))
}

fn divisors(v: i128) -> Vec<i128> {
    let v = v.abs();
    let mut out = Vec::new();
    if v == 0 || v > 1_000_000_000_000 {
        return out;
    }
    let mut i = 1;
    while i * i <= v {
        if v % i == 0 {
            out.push(i);
            if i != v / i {
                out.push(v / i);
            }
        }
        i += 1;
    }
    out.sort();
    out
}

/// A rational root by the rational root theorem: p divides the constant
/// term and q the leading coefficient.
pub fn rational_root(p: &[Q]) -> Option<Q> {
    let (_, ints) = primitive(p)?;
    if ints[0] == 0 {
        return Some(Q::ZERO);
    }
    let lead = *ints.last()?;
    for q in divisors(lead) {
        for num in divisors(ints[0]) {
            for s in [1, -1] {
                let r = Q::new(s * num, q)?;
                if eval(p, r) == Some(Q::ZERO) {
                    return Some(r);
                }
            }
        }
    }
    None
}

/// Divides by (x - r), as synthetic division does.
pub fn deflate(p: &[Q], r: Q) -> Option<P> {
    let (q, rem) = divmod(p, &[r.neg(), Q::ONE])?;
    (rem.iter().all(|c| c.is_zero())).then_some(q)
}

/// Whether a positive rational is a perfect square, with its root.
pub fn rational_sqrt(q: Q) -> Option<Q> {
    if q.n < 0 {
        return None;
    }
    let r = |v: i128| {
        let s = (v as f64).sqrt().round() as i128;
        (s * s == v).then_some(s)
    };
    Q::new(r(q.n)?, r(q.d)?)
}

/// The factors of a rational polynomial: a number in front, then each
/// irreducible factor (as whole numbers, lowest power first) with its
/// multiplicity.
pub struct Factored {
    pub unit: Q,
    pub factors: Vec<(Vec<i128>, u32)>,
}

pub fn factor(p: &[Q]) -> Option<Factored> {
    let p = trim(p.to_vec());
    let (unit, mut rest) = primitive(&p)?;
    let mut factors: Vec<(Vec<i128>, u32)> = Vec::new();
    let push = |f: Vec<i128>, factors: &mut Vec<(Vec<i128>, u32)>| match factors
        .iter_mut()
        .find(|(g, _)| *g == f)
    {
        Some(e) => e.1 += 1,
        None => factors.push((f, 1)),
    };
    // Powers of x first.
    while rest.len() > 1 && rest[0] == 0 {
        rest.remove(0);
        push(vec![0, 1], &mut factors);
    }
    let mut cur: P = rest.iter().map(|&c| Q::int(c)).collect();
    while deg(&cur) >= 2 {
        let Some(r) = rational_root(&cur) else { break };
        cur = deflate(&cur, r)?;
        // (q x - p) for the root p/q.
        push(vec![-r.n, r.d], &mut factors);
        let (_, ints) = primitive(&cur)?;
        cur = ints.iter().map(|&c| Q::int(c)).collect();
    }
    if deg(&cur) == 1 {
        let (_, ints) = primitive(&cur)?;
        push(ints, &mut factors);
    } else if deg(&cur) >= 2 {
        // x⁴ - 4 = (x² - 2)(x² + 2): a polynomial in x² factored as one.
        let even = cur
            .iter()
            .enumerate()
            .all(|(k, c)| k % 2 == 0 || c.is_zero());
        let halves: P = cur.iter().step_by(2).copied().collect();
        match even.then(|| factor(&halves)).flatten() {
            Some(inner) if inner.factors.len() > 1 || inner.factors.iter().any(|f| f.1 > 1) => {
                for (f, m) in inner.factors {
                    let mut spread = vec![0i128; 2 * f.len() - 1];
                    for (k, c) in f.iter().enumerate() {
                        spread[2 * k] = *c;
                    }
                    for _ in 0..m {
                        push(spread.clone(), &mut factors);
                    }
                }
            }
            _ => {
                let (_, ints) = primitive(&cur)?;
                push(ints, &mut factors);
            }
        }
    }
    // Order as written: lowest degree, then by root.
    factors.sort_by(|a, b| a.0.len().cmp(&b.0.len()).then(a.0.cmp(&b.0)));
    Some(Factored { unit, factors })
}

impl Factored {
    pub fn to_sym(&self, v: &str) -> Sym {
        let mut parts = vec![Sym::Num(self.unit)];
        for (f, m) in &self.factors {
            let c: Vec<Sym> = f.iter().map(|&k| n(k)).collect();
            parts.push(powi(build(&c, v), *m as i128));
        }
        mul(parts)
    }
}

/// Every root, real and complex, found numerically (Durand-Kerner).
pub fn numeric_roots(p: &[f64]) -> Vec<(f64, f64)> {
    let mut p = p.to_vec();
    while p.len() > 1 && p.last().is_some_and(|c| *c == 0.0) {
        p.pop();
    }
    let d = p.len() - 1;
    if d == 0 {
        return Vec::new();
    }
    let lead = p[d];
    let a: Vec<f64> = p.iter().map(|c| c / lead).collect();
    type C = (f64, f64);
    let mulc = |x: C, y: C| (x.0 * y.0 - x.1 * y.1, x.0 * y.1 + x.1 * y.0);
    let divc = |x: C, y: C| {
        let m = y.0 * y.0 + y.1 * y.1;
        ((x.0 * y.0 + x.1 * y.1) / m, (x.1 * y.0 - x.0 * y.1) / m)
    };
    let evalc = |z: C| {
        let mut acc: C = (0.0, 0.0);
        for c in a.iter().rev() {
            acc = mulc(acc, z);
            acc.0 += c;
        }
        acc
    };
    let mut z: Vec<C> = (0..d)
        .map(|k| {
            let t = 0.4 + 0.9 * k as f64;
            let r = 1.0 + a.iter().map(|c| c.abs()).fold(0.0, f64::max);
            (r * t.cos() * 0.5, r * t.sin() * 0.5)
        })
        .collect();
    for _ in 0..500 {
        let mut moved = 0.0f64;
        for i in 0..d {
            let mut den: C = (1.0, 0.0);
            for j in 0..d {
                if i != j {
                    den = mulc(den, (z[i].0 - z[j].0, z[i].1 - z[j].1));
                }
            }
            let step = divc(evalc(z[i]), den);
            z[i] = (z[i].0 - step.0, z[i].1 - step.1);
            moved = moved.max(step.0.abs() + step.1.abs());
        }
        if moved < 1e-15 {
            break;
        }
    }
    // Polish each root with Newton's method on the real line where it is real.
    z.into_iter()
        .map(|(re, im)| {
            if im.abs() < 1e-7 {
                let mut x = re;
                for _ in 0..20 {
                    let (mut f, mut df) = (0.0, 0.0);
                    for c in a.iter().rev() {
                        df = df * x + f;
                        f = f * x + c;
                    }
                    if df == 0.0 {
                        break;
                    }
                    x -= f / df;
                }
                (x, 0.0)
            } else {
                (re, im)
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(v: &[i128]) -> P {
        v.iter().map(|&c| Q::int(c)).collect()
    }

    #[test]
    fn factors_over_the_rationals() {
        // 2x² - 8 = 2(x - 2)(x + 2).
        let f = factor(&p(&[-8, 0, 2])).unwrap();
        assert_eq!(f.unit, Q::int(2));
        assert_eq!(f.factors, vec![(vec![-2, 1], 1), (vec![2, 1], 1)]);
        // x⁴ - 4 = (x² - 2)(x² + 2).
        let f = factor(&p(&[-4, 0, 0, 0, 1])).unwrap();
        assert_eq!(f.factors.len(), 2);
        // (x - 1)² x.
        let f = factor(&p(&[0, 1, -2, 1])).unwrap();
        assert_eq!(f.factors, vec![(vec![-1, 1], 2), (vec![0, 1], 1)]);
    }

    #[test]
    fn numeric_roots_of_a_cubic() {
        let mut r: Vec<f64> = numeric_roots(&[-2.0, 0.0, 0.0, 1.0])
            .into_iter()
            .filter(|z| z.1 == 0.0)
            .map(|z| z.0)
            .collect();
        r.sort_by(f64::total_cmp);
        assert_eq!(r.len(), 1);
        assert!((r[0] - 2f64.cbrt()).abs() < 1e-12);
    }
}
