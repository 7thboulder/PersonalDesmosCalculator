//! Trigonometric integrals: powers and products of sines and cosines with
//! the identities and reduction formulas they are taught with,
//! trigonometric substitution for square roots of quadratics, and the
//! tangent half-angle substitution for anything else rational in sin and
//! cos.

use super::diff::derivative;
use super::integrate::{ii, integ, rational_in};
use super::latex::{latex, name};
use super::poly;
use super::sym::*;
use super::{nice, Steps};

/// tan, sec, csc and cot written with sin and cos.
pub fn to_sin_cos(s: &Sym) -> Sym {
    match s {
        Sym::Fn(f, a) => {
            let a = to_sin_cos(a);
            let (sn, cs) = (func(Func::Sin, a.clone()), func(Func::Cos, a.clone()));
            match f {
                Func::Tan => div(sn, cs),
                Func::Sec => div(n(1), cs),
                Func::Csc => div(n(1), sn),
                Func::Cot => div(cs, sn),
                f => func(*f, a),
            }
        }
        Sym::Add(xs) => add(xs.iter().map(to_sin_cos).collect()),
        Sym::Mul(xs) => mul(xs.iter().map(to_sin_cos).collect()),
        Sym::Pow(a, b) => pow(to_sin_cos(a), to_sin_cos(b)),
        other => other.clone(),
    }
}

fn is_trig(f: Func) -> bool {
    matches!(
        f,
        Func::Sin | Func::Cos | Func::Tan | Func::Sec | Func::Csc | Func::Cot
    )
}

/// c·sinᵐx·cosⁿx as (c, m, n), when that is what `e` is.
fn monomial(e: &Sym, v: &str) -> Option<(Sym, i128, i128)> {
    let x = var(v);
    let e = to_sin_cos(e);
    let (mut c, mut m, mut k) = (Vec::new(), 0i128, 0i128);
    for f in e.factors() {
        if !f.has(v) {
            c.push(f);
            continue;
        }
        let (b, p) = match &f {
            Sym::Pow(b, p) => ((**b).clone(), p.as_q()?),
            other => (other.clone(), Q::ONE),
        };
        if !p.is_int() {
            return None;
        }
        match b {
            Sym::Fn(Func::Sin, a) if *a == x => m += p.n,
            Sym::Fn(Func::Cos, a) if *a == x => k += p.n,
            _ => return None,
        }
    }
    if m == 0 && k == 0 {
        return None;
    }
    Some((mul(c), m, k))
}

fn sin_cos(v: &str) -> (Sym, Sym) {
    (func(Func::Sin, var(v)), func(Func::Cos, var(v)))
}

fn fresh(e: &Sym) -> &'static str {
    if e.has("u") {
        "w"
    } else {
        "u"
    }
}

/// Integrates `w` in `u` and puts `back` in for u.
fn in_u(
    w: &Sym,
    u: &str,
    back: &Sym,
    e: &Sym,
    v: &str,
    st: &mut Steps,
    depth: usize,
) -> Option<Sym> {
    let r = integ(&expand(w), u, st, depth + 1)?;
    let out = nice(&r.subs_var(u, back));
    st.push(
        format!("Put ${} = {}$ back", name(u), latex(back)),
        format!("{}={}", ii(e, v), latex(&out)),
    );
    Some(out)
}

/// sinᵐx cosⁿx for whole numbers m and n of any sign.
pub fn trig_power(e: &Sym, v: &str, st: &mut Steps, depth: usize) -> Option<Sym> {
    let (c, m, k) = monomial(e, v)?;
    if !c.is_one() || m.abs() > 12 || k.abs() > 12 {
        return None;
    }
    let mark = st.mark();
    let r = trig_power_steps(e, m, k, v, st, depth);
    if r.is_none() {
        st.undo(mark);
    }
    r
}

fn trig_power_steps(
    e: &Sym,
    m: i128,
    k: i128,
    v: &str,
    st: &mut Steps,
    depth: usize,
) -> Option<Sym> {
    let (s, c) = sin_cos(v);
    let x = name(v);
    let u = fresh(e);
    let uv = var(u);
    let one_minus = |t: &Sym| sub(n(1), powi(t.clone(), 2));
    // An odd power of sin: keep one for du = -sin x dx, the rest is cos.
    if m > 0 && m % 2 == 1 && !(k > 0 && k % 2 == 1 && k < m) {
        let w = neg(mul2(powi(one_minus(&uv), (m - 1) / 2), powi(uv.clone(), k)));
        st.push(
            format!(
                "Keep one $\\sin {x}$ and write the rest with $\\sin^2{x}=1-\\cos^2{x}$; then substitute ${u}=\\cos {x}$, $d{u}=-\\sin {x}\\ d{x}$"
            ),
            format!("{}={}", ii(e, v), ii(&w, u)),
        );
        return in_u(&w, u, &c, e, v, st, depth);
    }
    if k > 0 && k % 2 == 1 {
        let w = mul2(powi(one_minus(&uv), (k - 1) / 2), powi(uv.clone(), m));
        st.push(
            format!(
                "Keep one $\\cos {x}$ and write the rest with $\\cos^2{x}=1-\\sin^2{x}$; then substitute ${u}=\\sin {x}$, $d{u}=\\cos {x}\\ d{x}$"
            ),
            format!("{}={}", ii(e, v), ii(&w, u)),
        );
        return in_u(&w, u, &s, e, v, st, depth);
    }
    // Even powers: the half-angle identities.
    if m >= 0 && k >= 0 {
        let two = mul2(n(2), var(v));
        let c2 = func(Func::Cos, two);
        let w = mul2(
            powi(div(sub(n(1), c2.clone()), n(2)), m / 2),
            powi(div(add2(n(1), c2), n(2)), k / 2),
        );
        let w = expand(&w);
        st.push(
            format!(
                "Use $\\sin^2{x}=\\frac{{1-\\cos 2{x}}}{{2}}$ and $\\cos^2{x}=\\frac{{1+\\cos 2{x}}}{{2}}$ and multiply out"
            ),
            format!("{}={}", ii(e, v), ii(&w, v)),
        );
        return integ(&w, v, st, depth + 1);
    }
    let (t, se) = (func(Func::Tan, var(v)), func(Func::Sec, var(v)));
    let (ct, cs) = (func(Func::Cot, var(v)), func(Func::Csc, var(v)));
    // sec x, sec²x, csc x and csc²x are in the table.
    if (m == 0 && (k == -1 || k == -2)) || (k == 0 && (m == -1 || m == -2)) {
        let f = if m == 0 { se.clone() } else { cs.clone() };
        let w = powi(f, -(m + k));
        if w != *e {
            st.push(
                format!("This is ${}$", latex(&w)),
                format!("{}={}", ii(e, v), ii(&w, v)),
            );
        }
        return integ(&w, v, st, depth + 1);
    }
    if m >= 0 && k < 0 {
        let total = -(m + k);
        if total >= 0 && total % 2 == 0 {
            if total >= 2 {
                // tanᵐ secˢ with s even: u = tan x, du = sec²x dx.
                let w = mul2(
                    powi(uv.clone(), m),
                    powi(add2(n(1), powi(uv.clone(), 2)), (total - 2) / 2),
                );
                st.push(
                    format!(
                        "This is $\\tan^{{{m}}}{x}\\sec^{{{total}}}{x}$: keep $\\sec^2{x}$ for $d{u}$ and write the rest with $\\sec^2{x}=1+\\tan^2{x}$; then substitute ${u}=\\tan {x}$"
                    ),
                    format!("{}={}", ii(e, v), ii(&w, u)),
                );
                return in_u(&w, u, &t, e, v, st, depth);
            }
            if m <= 2 {
                let w = powi(t.clone(), m);
                st.push(
                    format!("This is ${}$", latex(&w)),
                    format!("{}={}", ii(e, v), ii(&w, v)),
                );
                return integ(&w, v, st, depth + 1);
            }
            // tanᵐx = tanᵐ⁻²x (sec²x - 1).
            if m >= 3 {
                let first = div(powi(t.clone(), m - 1), n(m - 1));
                let rest = powi(t.clone(), m - 2);
                st.push(
                    format!("Write $\\tan^{{{m}}}{x}=\\tan^{{{}}}{x}\\left(\\sec^2{x}-1\\right)$; the first part integrates to $\\frac{{\\tan^{{{}}}{x}}}{{{}}}$", m - 2, m - 1, m - 1),
                    format!("{}={}-{}", ii(e, v), latex(&first), ii(&rest, v)),
                );
                let r = integ(&rest, v, st, depth + 1)?;
                return Some(sub(first, r));
            }
            return None;
        }
        if m == 0 && -k >= 3 {
            // ∫secᵏ = secᵏ⁻² tan/(k-1) + (k-2)/(k-1) ∫secᵏ⁻².
            let p = -k;
            let first = div(mul2(powi(se.clone(), p - 2), t.clone()), n(p - 1));
            let rest = powi(se.clone(), p - 2);
            let f = frac(p - 2, p - 1);
            st.push(
                "Use the reduction formula $\\int\\sec^{k}x\\ dx=\\frac{\\sec^{k-2}x\\tan x}{k-1}+\\frac{k-2}{k-1}\\int\\sec^{k-2}x\\ dx$ (from integrating by parts)",
                format!("{}={}+{}{}", ii(e, v), latex(&first), paren_q(&f), ii(&rest, v)),
            );
            let r = integ(&rest, v, st, depth + 1)?;
            return Some(add2(first, mul2(f, r)));
        }
        if m >= 2 {
            // sinᵐ = (1 - cos²)^(m/2), then each power of cos on its own.
            let w = expand(&mul2(
                powi(sub(n(1), powi(c.clone(), 2)), m / 2),
                powi(c.clone(), k),
            ));
            st.push(
                format!("Write $\\sin^2{x}=1-\\cos^2{x}$ and multiply out"),
                format!("{}={}", ii(e, v), ii(&w, v)),
            );
            return integ(&w, v, st, depth + 1);
        }
        return None;
    }
    if k >= 0 && m < 0 {
        let total = -(m + k);
        if total >= 0 && total % 2 == 0 {
            if total >= 2 {
                // cotⁿ cscˢ: u = cot x, du = -csc²x dx.
                let w = neg(mul2(
                    powi(uv.clone(), k),
                    powi(add2(n(1), powi(uv.clone(), 2)), (total - 2) / 2),
                ));
                st.push(
                    format!(
                        "This is $\\cot^{{{k}}}{x}\\csc^{{{total}}}{x}$: keep $\\csc^2{x}$ for $d{u}$ and write the rest with $\\csc^2{x}=1+\\cot^2{x}$; then substitute ${u}=\\cot {x}$, $d{u}=-\\csc^2{x}\\ d{x}$"
                    ),
                    format!("{}={}", ii(e, v), ii(&w, u)),
                );
                return in_u(&w, u, &ct, e, v, st, depth);
            }
            if k <= 2 {
                let w = powi(ct.clone(), k);
                st.push(
                    format!("This is ${}$", latex(&w)),
                    format!("{}={}", ii(e, v), ii(&w, v)),
                );
                return integ(&w, v, st, depth + 1);
            }
            if k >= 3 {
                let first = neg(div(powi(ct.clone(), k - 1), n(k - 1)));
                let rest = powi(ct.clone(), k - 2);
                st.push(
                    format!("Write $\\cot^{{{k}}}{x}=\\cot^{{{}}}{x}\\left(\\csc^2{x}-1\\right)$; the first part integrates to $-\\frac{{\\cot^{{{}}}{x}}}{{{}}}$", k - 2, k - 1, k - 1),
                    format!("{}={}-{}", ii(e, v), latex(&first), ii(&rest, v)),
                );
                let r = integ(&rest, v, st, depth + 1)?;
                return Some(sub(first, r));
            }
            return None;
        }
        if k == 0 && -m >= 3 {
            let p = -m;
            let first = neg(div(mul2(powi(cs.clone(), p - 2), ct.clone()), n(p - 1)));
            let rest = powi(cs.clone(), p - 2);
            let f = frac(p - 2, p - 1);
            st.push(
                "Use the reduction formula $\\int\\csc^{k}x\\ dx=-\\frac{\\csc^{k-2}x\\cot x}{k-1}+\\frac{k-2}{k-1}\\int\\csc^{k-2}x\\ dx$ (from integrating by parts)",
                format!("{}={}+{}{}", ii(e, v), latex(&first), paren_q(&f), ii(&rest, v)),
            );
            let r = integ(&rest, v, st, depth + 1)?;
            return Some(add2(first, mul2(f, r)));
        }
        if k >= 2 {
            let w = expand(&mul2(
                powi(sub(n(1), powi(s.clone(), 2)), k / 2),
                powi(s.clone(), m),
            ));
            st.push(
                format!("Write $\\cos^2{x}=1-\\sin^2{x}$ and multiply out"),
                format!("{}={}", ii(e, v), ii(&w, v)),
            );
            return integ(&w, v, st, depth + 1);
        }
        return None;
    }
    // Both below: 1 = sin²x + cos²x splits it into simpler pieces.
    let w = add2(
        mul2(powi(s.clone(), m + 2), powi(c.clone(), k)),
        mul2(powi(s.clone(), m), powi(c.clone(), k + 2)),
    );
    st.push(
        format!("Write the 1 on top as $\\sin^2{x}+\\cos^2{x}$ and split"),
        format!("{}={}", ii(e, v), ii(&w, v)),
    );
    integ(&w, v, st, depth + 1)
}

fn paren_q(q: &Sym) -> String {
    latex(q)
}

/// sin A cos B, sin A sin B and cos A cos B with different angles: the
/// product-to-sum identities.
pub fn trig_products(e: &Sym, v: &str, st: &mut Steps, depth: usize) -> Option<Sym> {
    let fs = e.factors();
    let trig: Vec<(Func, Sym)> = fs
        .iter()
        .filter_map(|f| match f {
            Sym::Fn(g @ (Func::Sin | Func::Cos), a) => Some((*g, (**a).clone())),
            _ => None,
        })
        .collect();
    let rest: Vec<Sym> = fs
        .iter()
        .filter(|f| !matches!(f, Sym::Fn(Func::Sin | Func::Cos, _)))
        .cloned()
        .collect();
    if trig.len() != 2 || rest.iter().any(|f| f.has(v)) {
        return None;
    }
    let ((f, a), (g, b)) = (trig[0].clone(), trig[1].clone());
    for t in [&a, &b] {
        let c = poly::coeffs(t, v)?;
        if c.len() != 2 {
            return None;
        }
    }
    if a == b {
        return None;
    }
    let (sum, diff) = (add2(a.clone(), b.clone()), sub(a.clone(), b.clone()));
    let (w, note) = match (f, g) {
        (Func::Sin, Func::Cos) => (
            add2(func(Func::Sin, sum), func(Func::Sin, diff)),
            "\\sin A\\cos B=\\frac{\\sin(A+B)+\\sin(A-B)}{2}",
        ),
        (Func::Cos, Func::Sin) => (
            sub(func(Func::Sin, sum), func(Func::Sin, diff)),
            "\\cos A\\sin B=\\frac{\\sin(A+B)-\\sin(A-B)}{2}",
        ),
        (Func::Sin, Func::Sin) => (
            sub(func(Func::Cos, diff), func(Func::Cos, sum)),
            "\\sin A\\sin B=\\frac{\\cos(A-B)-\\cos(A+B)}{2}",
        ),
        _ => (
            add2(func(Func::Cos, diff), func(Func::Cos, sum)),
            "\\cos A\\cos B=\\frac{\\cos(A-B)+\\cos(A+B)}{2}",
        ),
    };
    let w = mul(vec![mul(rest), frac(1, 2), w]);
    let mark = st.mark();
    st.push(
        format!("Use ${note}$"),
        format!("{}={}", ii(e, v), ii(&w, v)),
    );
    match integ(&expand(&w), v, st, depth + 1) {
        Some(r) => Some(r),
        None => {
            st.undo(mark);
            None
        }
    }
}

/// sin(kθ) and cos(kθ) for whole k as powers of sin θ and cos θ.
fn expand_angles(s: &Sym, t: &str) -> Sym {
    match s {
        Sym::Fn(f @ (Func::Sin | Func::Cos), a) => {
            let a = expand_angles(a, t);
            let k = div(a.clone(), var(t)).as_q();
            match k {
                Some(k) if k.is_int() && k.n.abs() >= 2 && k.n.abs() <= 8 => {
                    let (s1, c1) = (func(Func::Sin, var(t)), func(Func::Cos, var(t)));
                    let (mut sk, mut ck) = (s1.clone(), c1.clone());
                    for _ in 1..k.n.abs() {
                        let ns = add2(mul2(sk.clone(), c1.clone()), mul2(ck.clone(), s1.clone()));
                        let nc = sub(mul2(ck.clone(), c1.clone()), mul2(sk.clone(), s1.clone()));
                        sk = expand(&ns);
                        ck = expand(&nc);
                    }
                    let sign = if k.n < 0 { -1 } else { 1 };
                    match f {
                        Func::Sin => mul2(n(sign), sk),
                        _ => ck,
                    }
                }
                _ => func(*f, a),
            }
        }
        Sym::Fn(f, a) => func(*f, expand_angles(a, t)),
        Sym::Add(xs) => add(xs.iter().map(|x| expand_angles(x, t)).collect()),
        Sym::Mul(xs) => mul(xs.iter().map(|x| expand_angles(x, t)).collect()),
        Sym::Pow(a, b) => pow(expand_angles(a, t), expand_angles(b, t)),
        other => other.clone(),
    }
}

/// Puts `with` in for √R wherever a power of R appears: R^(k/2) becomes
/// withᵏ.
fn replace_root(s: &Sym, r: &Sym, with: &Sym) -> Sym {
    if s == r {
        return powi(with.clone(), 2);
    }
    match s {
        Sym::Pow(b, p) if **b == *r => match p.as_q() {
            Some(q) => pow(with.clone(), Sym::Num(q.mul(Q::int(2)).unwrap_or(q))),
            None => s.clone(),
        },
        Sym::Add(xs) => add(xs.iter().map(|x| replace_root(x, r, with)).collect()),
        Sym::Mul(xs) => mul(xs.iter().map(|x| replace_root(x, r, with)).collect()),
        Sym::Pow(a, b) => pow(replace_root(a, r, with), replace_root(b, r, with)),
        Sym::Fn(f, a) => func(*f, replace_root(a, r, with)),
        other => other.clone(),
    }
}

/// The quadratic R with a power R^(k/2) (k odd) or R^k (k ≤ -2, R with no
/// real roots) in `e`, for trigonometric substitution.
fn find_quadratic(e: &Sym, v: &str) -> Option<Sym> {
    let mut found = None;
    let mut look = |s: &Sym| {
        if found.is_some() {
            return;
        }
        if let Sym::Pow(b, p) = s {
            let (Some(q), Some(c)) = (p.as_q(), poly::coeffs(b, v)) else {
                return;
            };
            if c.len() != 3 || poly::rational(&c).is_none() {
                return;
            }
            let half = q.d == 2;
            let whole = q.is_int() && q.n <= -2 && {
                let c = poly::rational(&c).unwrap();
                c[1].mul(c[1])
                    .and_then(|b2| {
                        c[0].mul(c[2])
                            .and_then(|ac| ac.mul(Q::int(4)))
                            .and_then(|f| b2.sub(f))
                    })
                    .is_some_and(|d| d.n < 0)
            };
            if half || whole {
                found = Some((**b).clone());
            }
        }
    };
    walk(e, &mut look);
    found
}

pub fn walk(s: &Sym, f: &mut dyn FnMut(&Sym)) {
    f(s);
    match s {
        Sym::Add(xs) | Sym::Mul(xs) => xs.iter().for_each(|x| walk(x, f)),
        Sym::Pow(a, b) => {
            walk(a, f);
            walk(b, f);
        }
        Sym::Fn(_, a) => walk(a, f),
        _ => {}
    }
}

/// Trigonometric substitution for √(a² - x²), √(a² + x²), √(x² - a²) and
/// powers of them, completing the square first when there is an x term.
pub fn trig_sub(e: &Sym, v: &str, st: &mut Steps, depth: usize) -> Option<Sym> {
    if depth > 5 {
        return None;
    }
    let r = find_quadratic(e, v)?;
    let mark = st.mark();
    let out = trig_sub_steps(e, &r, v, st, depth);
    match out {
        Some(f) if same_derivative(&f, e, v) => Some(f),
        _ => {
            st.undo(mark);
            None
        }
    }
}

fn trig_sub_steps(e: &Sym, r: &Sym, v: &str, st: &mut Steps, depth: usize) -> Option<Sym> {
    let c = poly::rational(&poly::coeffs(r, v)?)?;
    let (cc, b, a) = (c[0], c[1], c[2]);
    let x = var(v);
    if !b.is_zero() {
        // a(x + h)² + k.
        let h = b.div(Q::int(2).mul(a)?)?;
        let k = cc.sub(b.mul(b)?.div(Q::int(4).mul(a)?)?)?;
        let t = if e.has("t") { "s" } else { "t" };
        let tv = var(t);
        let shifted = add2(
            mul2(Sym::Num(a), powi(add2(x.clone(), Sym::Num(h)), 2)),
            Sym::Num(k),
        );
        let back = add2(x.clone(), Sym::Num(h));
        let r2 = add2(mul2(Sym::Num(a), powi(tv.clone(), 2)), Sym::Num(k));
        let w = replace_root(e, r, &sqrt(r2.clone())).subs_var(v, &sub(tv.clone(), Sym::Num(h)));
        st.push(
            format!(
                "Complete the square, ${}={}$, and substitute ${t}={}$",
                latex(r),
                latex(&shifted),
                latex(&back)
            ),
            format!("{}={}", ii(e, v), ii(&w, t)),
        );
        let f = trig_sub_steps(&w, &r2, t, st, depth + 1)?;
        let out = nice(&f.subs_var(t, &back));
        st.push(
            format!("Put ${t}={}$ back", latex(&back)),
            format!("{}={}", ii(e, v), latex(&out)),
        );
        return Some(out);
    }
    let th = "theta";
    let tv = var(th);
    let (s, co) = (func(Func::Sin, tv.clone()), func(Func::Cos, tv.clone()));
    let (ta, se) = (func(Func::Tan, tv.clone()), func(Func::Sec, tv.clone()));
    // x = scale·trig(θ), √R = root·trig'(θ), dx = d(θ) dθ.
    let (scale, root, xs, rs, note, inv, sin_back, cos_back);
    let sq = |q: Q| sqrt(Sym::Num(q));
    let rv = sqrt(r.clone());
    if a.n < 0 && cc.n > 0 {
        scale = sq(cc.div(a.neg())?);
        root = sq(cc);
        xs = mul2(scale.clone(), s.clone());
        rs = mul2(root.clone(), co.clone());
        note = "sin";
        inv = func(Func::Asin, div(x.clone(), scale.clone()));
        sin_back = div(x.clone(), scale.clone());
        cos_back = div(rv.clone(), root.clone());
    } else if a.n > 0 && cc.n > 0 {
        scale = sq(cc.div(a)?);
        root = sq(cc);
        xs = mul2(scale.clone(), ta.clone());
        rs = mul2(root.clone(), se.clone());
        note = "tan";
        inv = func(Func::Atan, div(x.clone(), scale.clone()));
        cos_back = div(root.clone(), rv.clone());
        sin_back = mul2(div(x.clone(), scale.clone()), cos_back.clone());
    } else if a.n > 0 && cc.n < 0 {
        scale = sq(cc.neg().div(a)?);
        root = sq(cc.neg());
        xs = mul2(scale.clone(), se.clone());
        rs = mul2(root.clone(), ta.clone());
        note = "sec";
        inv = func(Func::Acos, div(scale.clone(), x.clone()));
        cos_back = div(scale.clone(), x.clone());
        sin_back = mul2(div(rv.clone(), root.clone()), cos_back.clone());
    } else {
        return None;
    }
    let dx = derivative(&xs, th);
    let w = replace_root(e, r, &rs).subs_var(v, &xs);
    let w = nice(&expand(&to_sin_cos(&mul2(w, dx.clone()))));
    let trig_word = match note {
        "sin" => "\\sin",
        "tan" => "\\tan",
        _ => "\\sec",
    };
    let identity = match note {
        "sin" => "1-\\sin^2\\theta=\\cos^2\\theta",
        "tan" => "1+\\tan^2\\theta=\\sec^2\\theta",
        _ => "\\sec^2\\theta-1=\\tan^2\\theta",
    };
    st.push(
        format!(
            "Substitute ${}={}$ so that ${identity}$ clears the root: $\\sqrt{{{}}}={}$ and $d{}={}\\ d\\theta$",
            name(v),
            latex(&xs),
            latex(r),
            latex(&rs),
            name(v),
            latex(&dx)
        ),
        format!("{}={}", ii(e, v), ii(&w, th)),
    );
    let f = integ(&w, th, st, depth + 1)?;
    // Back to x with the triangle: sin θ and cos θ in terms of x.
    let tan_back = nice(&div(sin_back.clone(), cos_back.clone()));
    let mut f = expand_angles(&f, th);
    for (g, b) in [
        (Func::Tan, tan_back.clone()),
        (Func::Sec, nice(&div(n(1), cos_back.clone()))),
        (Func::Csc, nice(&div(n(1), sin_back.clone()))),
        (Func::Cot, nice(&div(n(1), tan_back))),
    ] {
        f = f.subs(&func(g, tv.clone()), &b);
    }
    let f = expand_angles(&to_sin_cos(&f), th);
    let back = f.subs(&s, &sin_back).subs(&co, &cos_back).subs(&tv, &inv);
    let back = nice(&back);
    if back.has(th) {
        return None;
    }
    st.push(
        format!(
            "Go back to ${}$ with a right triangle for ${}={trig_word}\\theta$",
            name(v),
            latex(&div(x.clone(), scale))
        ),
        format!("{}={}", ii(e, v), latex(&back)),
    );
    Some(back)
}

/// Whether `f` can be checked by its values: no unknowns but `v`, the
/// constants and made-up parameters.
pub fn checkable(f: &Sym, _v: &str) -> bool {
    !f.has("i")
}

/// Whether d/dx F = f at points where f is defined.
pub fn same_derivative(big_f: &Sym, f: &Sym, v: &str) -> bool {
    let d = derivative(big_f, v);
    let names: Vec<String> = f.names().into_iter().filter(|n| n != v).collect();
    let env_for = |x: f64| {
        let names = names.clone();
        let v = v.to_owned();
        move |n: &str| {
            if n == v {
                Some(x)
            } else {
                names
                    .iter()
                    .position(|m| m == n)
                    .map(|i| 0.7 + 0.3 * i as f64)
            }
        }
    };
    let mut passed = 0;
    for x in [0.23, 0.61, 1.17, 1.93, 2.71, 3.6, 4.4, 5.3] {
        let env = env_for(x);
        let (a, b) = (d.eval(&env), f.eval(&env));
        if !a.is_finite() || !b.is_finite() {
            continue;
        }
        if (a - b).abs() > 1e-7 * b.abs().max(1.0) {
            return false;
        }
        passed += 1;
    }
    passed >= 2
}

/// Anything else rational in sin x and cos x: t = tan(x/2) makes it a
/// rational function of t.
pub fn weierstrass(e: &Sym, v: &str, st: &mut Steps, depth: usize) -> Option<Sym> {
    if depth > 3 {
        return None;
    }
    let w = to_sin_cos(e);
    let (s, c) = sin_cos(v);
    let ps = var("\u{6}s");
    let pc = var("\u{6}c");
    let marked = w.subs(&s, &ps).subs(&c, &pc);
    if marked.has(v) || !(marked.has("\u{6}s") || marked.has("\u{6}c")) {
        return None;
    }
    let t = if e.has("t") { "s" } else { "t" };
    let tv = var(t);
    let den = add2(n(1), powi(tv.clone(), 2));
    let in_t = mul2(
        marked
            .subs(&ps, &div(mul2(n(2), tv.clone()), den.clone()))
            .subs(&pc, &div(sub(n(1), powi(tv.clone(), 2)), den.clone())),
        div(n(2), den),
    );
    let in_t = together(&in_t);
    let (top, bottom) = num_den(&in_t);
    let in_t = div(expand(&top), expand(&bottom));
    if !rational_in(&in_t, t) {
        return None;
    }
    let mark = st.mark();
    st.push(
        format!(
            "Substitute ${t}=\\tan\\frac{{{x}}}{{2}}$, so $\\sin {x}=\\frac{{2{t}}}{{1+{t}^2}}$, $\\cos {x}=\\frac{{1-{t}^2}}{{1+{t}^2}}$ and $d{x}=\\frac{{2\\ d{t}}}{{1+{t}^2}}$",
            x = name(v)
        ),
        format!("{}={}", ii(e, v), ii(&in_t, t)),
    );
    let Some(r) = integ(&in_t, t, st, depth + 1) else {
        st.undo(mark);
        return None;
    };
    let back = func(Func::Tan, div(var(v), n(2)));
    let out = nice(&r.subs_var(t, &back));
    st.push(
        format!("Put ${t}=\\tan\\frac{{{}}}{{2}}$ back", name(v)),
        format!("{}={}", ii(e, v), latex(&out)),
    );
    Some(out)
}

/// sinh, cosh and tanh as exponentials.
pub fn hyperbolic(e: &Sym, v: &str, st: &mut Steps, depth: usize) -> Option<Sym> {
    fn rewrite(s: &Sym) -> Sym {
        match s {
            Sym::Fn(f @ (Func::Sinh | Func::Cosh | Func::Tanh), u) => {
                let u = rewrite(u);
                let (p, m) = (exp(u.clone()), exp(neg(u)));
                match f {
                    Func::Sinh => div(sub(p, m), n(2)),
                    Func::Cosh => div(add2(p, m), n(2)),
                    _ => div(sub(p.clone(), m.clone()), add2(p, m)),
                }
            }
            Sym::Add(xs) => add(xs.iter().map(rewrite).collect()),
            Sym::Mul(xs) => mul(xs.iter().map(rewrite).collect()),
            Sym::Pow(a, b) => pow(rewrite(a), rewrite(b)),
            Sym::Fn(f, a) => func(*f, rewrite(a)),
            other => other.clone(),
        }
    }
    let mut found = false;
    walk(e, &mut |s| {
        found |= matches!(s, Sym::Fn(Func::Sinh | Func::Cosh | Func::Tanh, _))
    });
    if !found || depth > 4 {
        return None;
    }
    let w = expand(&rewrite(e));
    let mark = st.mark();
    st.push(
        "Write the hyperbolic functions with exponentials and multiply out",
        format!("{}={}", ii(e, v), ii(&w, v)),
    );
    match integ(&w, v, st, depth + 1) {
        Some(r) => Some(r),
        None => {
            st.undo(mark);
            None
        }
    }
}

/// Whether `e` has no antiderivative made of elementary functions, for the
/// well-known ones: e^(x²), sin(x)/x, eˣ/x, 1/ln x, sin(x²).
pub fn non_elementary(e: &Sym, v: &str) -> bool {
    let x = var(v);
    let quadratic = |s: &Sym| {
        poly::coeffs(s, v).is_some_and(|c| c.len() == 3 && c[1].is_zero() && c[0].is_zero())
    };
    match e {
        Sym::Pow(b, p) if **b == var("e") => quadratic(p),
        Sym::Fn(Func::Sin | Func::Cos, a) => quadratic(a),
        Sym::Pow(b, p) if p.is_int(-1) => matches!(&**b, Sym::Fn(Func::Ln, a) if **a == x),
        Sym::Mul(fs) if fs.len() == 2 && fs.contains(&pow(x.clone(), n(-1))) => {
            fs.iter().any(|f| {
                matches!(f, Sym::Fn(Func::Sin | Func::Cos, a) if **a == x)
                    || matches!(f, Sym::Pow(b, p) if **b == var("e") && **p == x)
            })
        }
        _ => false,
    }
}

pub fn has_trig(e: &Sym) -> bool {
    let mut found = false;
    walk(e, &mut |s| {
        found |= matches!(s, Sym::Fn(f, _) if is_trig(*f))
    });
    found
}
