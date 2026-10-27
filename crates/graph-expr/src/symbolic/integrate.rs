//! Antiderivatives, found the way they are taught: a table of standard
//! integrals, linearity, substitution, partial fractions and integration
//! by parts, with each move written into the steps.

use super::diff::{derivative, paren};
use super::latex::{latex, name};
use super::poly;
use super::sym::*;
use super::{out_of_time, Steps};

/// `\int u\,dx`.
pub fn ii(u: &Sym, v: &str) -> String {
    let body = if matches!(u, Sym::Add(_)) {
        format!("\\left({}\\right)", latex(u))
    } else {
        latex(u)
    };
    format!("\\int {body}\\ d{}", name(v))
}

/// An antiderivative of `e` in `v` (without the + C), if one is found.
pub fn antiderivative(e: &Sym, v: &str, st: &mut Steps) -> Option<Sym> {
    integ(e, v, st, 0)
}

fn integ(e: &Sym, v: &str, st: &mut Steps, depth: usize) -> Option<Sym> {
    if depth > 8 || out_of_time() {
        return None;
    }
    if !e.has(v) {
        let r = mul2(e.clone(), var(v));
        st.push(
            "The integral of a constant c is cx",
            format!("{}={}", ii(e, v), latex(&r)),
        );
        return Some(r);
    }
    if let Some((r, note)) = table(e, v) {
        st.push(note, format!("{}={}", ii(e, v), latex(&r)));
        return Some(r);
    }
    match e {
        Sym::Add(ts) => {
            let mark = st.mark();
            let parts: Vec<String> = ts.iter().map(|t| ii(t, v)).collect();
            st.push(
                "Integrate term by term",
                format!("{}={}", ii(e, v), parts.join("+")),
            );
            let mut out = Vec::new();
            for t in ts {
                match integ(t, v, st, depth + 1) {
                    Some(r) => out.push(r),
                    None => {
                        st.undo(mark);
                        return rational(e, v, st, depth);
                    }
                }
            }
            return Some(add(out));
        }
        Sym::Mul(fs) if fs.iter().any(|f| !f.has(v)) => {
            let (constant, varying): (Vec<Sym>, Vec<Sym>) =
                fs.iter().cloned().partition(|f| !f.has(v));
            let c = mul(constant);
            let rest = mul(varying);
            let mark = st.mark();
            if c.is_int(-1) {
                st.push(
                    "Take the minus sign outside the integral",
                    format!("{}=-{}", ii(e, v), ii(&rest, v)),
                );
            } else {
                st.push(
                    format!("Take the constant ${}$ outside the integral", latex(&c)),
                    format!("{}={}{}", ii(e, v), paren(&c), ii(&rest, v)),
                );
            }
            if let Some(r) = integ(&rest, v, st, depth + 1) {
                return Some(mul2(c, r));
            }
            st.undo(mark);
            return None;
        }
        _ => {}
    }
    // Products of polynomials multiply out first.
    if poly::coeffs(e, v).is_some() {
        let x = expand(e);
        if x != *e {
            let mark = st.mark();
            st.push("Multiply out", format!("{}={}", ii(e, v), ii(&x, v)));
            if let Some(r) = integ(&x, v, st, depth + 1) {
                return Some(r);
            }
            st.undo(mark);
        }
    }
    if let Some(r) = odd_power(e, v, st, depth) {
        return Some(r);
    }
    if let Some(r) = exp_trig(e, v, st) {
        return Some(r);
    }
    if let Some(r) = substitution(e, v, st, depth) {
        return Some(r);
    }
    if let Some(r) = rational(e, v, st, depth) {
        return Some(r);
    }
    if let Some(r) = parts(e, v, st, depth) {
        return Some(r);
    }
    let x = expand(e);
    if x != *e && x.size() <= 4 * e.size() + 8 {
        let mark = st.mark();
        st.push("Multiply out", format!("{}={}", ii(e, v), ii(&x, v)));
        if let Some(r) = integ(&x, v, st, depth + 1) {
            return Some(r);
        }
        st.undo(mark);
    }
    None
}

/// The standard integrals, for the variable on its own.
fn table(e: &Sym, v: &str) -> Option<(Sym, String)> {
    let x = var(v);
    let is_x = |s: &Sym| *s == x;
    let ln_abs = |s: Sym| func(Func::Ln, func(Func::Abs, s));
    let f = |g: Func| func(g, x.clone());
    let out = |r: Sym, note: &str| Some((r, note.to_owned()));
    match e {
        Sym::Var(_) => out(
            div(powi(x.clone(), 2), n(2)),
            "Power rule: raise the power by one and divide by the new power",
        ),
        Sym::Pow(b, k) if is_x(b) && !k.has(v) => {
            if k.is_int(-1) {
                out(ln_abs(x.clone()), "The integral of 1/x is ln|x|")
            } else {
                let k1 = add2((**k).clone(), n(1));
                out(
                    div(pow(x.clone(), k1.clone()), k1),
                    "Power rule: raise the power by one and divide by the new power",
                )
            }
        }
        Sym::Pow(b, k) if is_x(k) && !b.has(v) => {
            if **b == var("e") {
                out(e.clone(), "The integral of eˣ is eˣ")
            } else {
                out(
                    div(e.clone(), func(Func::Ln, (**b).clone())),
                    "The integral of aˣ is aˣ/ln a",
                )
            }
        }
        Sym::Fn(g, a) if is_x(a) => match g {
            Func::Sin => out(neg(f(Func::Cos)), "The integral of sin x is -cos x"),
            Func::Cos => out(f(Func::Sin), "The integral of cos x is sin x"),
            Func::Tan => out(
                neg(ln_abs(f(Func::Cos))),
                "The integral of tan x is -ln|cos x|",
            ),
            Func::Cot => out(ln_abs(f(Func::Sin)), "The integral of cot x is ln|sin x|"),
            Func::Sec => out(
                ln_abs(add2(f(Func::Sec), f(Func::Tan))),
                "The integral of sec x is ln|sec x + tan x|",
            ),
            Func::Csc => out(
                neg(ln_abs(add2(f(Func::Csc), f(Func::Cot)))),
                "The integral of csc x is -ln|csc x + cot x|",
            ),
            Func::Sinh => out(f(Func::Cosh), "The integral of sinh x is cosh x"),
            Func::Cosh => out(f(Func::Sinh), "The integral of cosh x is sinh x"),
            Func::Tanh => out(
                func(Func::Ln, f(Func::Cosh)),
                "The integral of tanh x is ln(cosh x)",
            ),
            _ => None,
        },
        Sym::Pow(b, k) => {
            let Sym::Fn(g, a) = &**b else {
                return quadratic_roots(e, v);
            };
            if !is_x(a) {
                return None;
            }
            match (g, k.as_q()?.n, k.as_q()?.d) {
                (Func::Cos, -2, 1) | (Func::Sec, 2, 1) => {
                    out(f(Func::Tan), "The integral of sec²x is tan x")
                }
                (Func::Sin, -2, 1) | (Func::Csc, 2, 1) => {
                    out(neg(f(Func::Cot)), "The integral of csc²x is -cot x")
                }
                (Func::Cosh, -2, 1) => out(f(Func::Tanh), "The integral of sech²x is tanh x"),
                (Func::Sin, 2, 1) => out(
                    sub(
                        div(x.clone(), n(2)),
                        div(func(Func::Sin, mul2(n(2), x.clone())), n(4)),
                    ),
                    "Use sin²x = (1 - cos 2x)/2, then integrate each part",
                ),
                (Func::Cos, 2, 1) => out(
                    add2(
                        div(x.clone(), n(2)),
                        div(func(Func::Sin, mul2(n(2), x.clone())), n(4)),
                    ),
                    "Use cos²x = (1 + cos 2x)/2, then integrate each part",
                ),
                (Func::Tan, 2, 1) => out(
                    sub(f(Func::Tan), x.clone()),
                    "Use tan²x = sec²x - 1, then integrate each part",
                ),
                _ => None,
            }
        }
        Sym::Mul(fs) if fs.len() == 2 => {
            let has = |g: Func| fs.contains(&f(g));
            if has(Func::Sec) && has(Func::Tan) {
                out(f(Func::Sec), "The integral of sec x tan x is sec x")
            } else if has(Func::Csc) && has(Func::Cot) {
                out(neg(f(Func::Csc)), "The integral of csc x cot x is -csc x")
            } else {
                None
            }
        }
        _ => None,
    }
}

/// 1/(x² + c), 1/√(c - x²) and 1/√(x² + c).
fn quadratic_roots(e: &Sym, v: &str) -> Option<(Sym, String)> {
    let Sym::Pow(b, k) = e else { return None };
    let c = poly::coeffs(b, v)?;
    if c.len() != 3 || !c[1].is_zero() {
        return None;
    }
    let x = var(v);
    let (c0, c2) = (c[0].as_q()?, c[2].as_q()?);
    let k = k.as_q()?;
    if k == Q::int(-1) && c2 == Q::ONE && c0.n > 0 {
        let r = sqrt(Sym::Num(c0));
        return Some((
            div(func(Func::Atan, div(x, r.clone())), r),
            "The integral of 1/(x² + a²) is (1/a) arctan(x/a)".into(),
        ));
    }
    if k == Q::new(-1, 2).unwrap() && c2 == Q::int(-1) && c0.n > 0 {
        let r = sqrt(Sym::Num(c0));
        return Some((
            func(Func::Asin, div(x, r)),
            "The integral of 1/√(a² - x²) is arcsin(x/a)".into(),
        ));
    }
    if k == Q::new(-1, 2).unwrap() && c2 == Q::ONE {
        return Some((
            func(Func::Ln, func(Func::Abs, add2(x, (**b).clone().pow_half()))),
            "The integral of 1/√(x² + c) is ln|x + √(x² + c)|".into(),
        ));
    }
    None
}

impl Sym {
    fn pow_half(self) -> Sym {
        sqrt(self)
    }
}

/// The coefficients a, b when `s` is ax + b.
fn linear(s: &Sym, v: &str) -> Option<(Sym, Sym)> {
    let c = poly::coeffs(s, v)?;
    (c.len() == 2).then(|| (c[1].clone(), c[0].clone()))
}

/// sin³x and other odd powers: keep one sin x for du and write the rest
/// with sin²x = 1 - cos²x, then substitute u = cos x.
fn odd_power(e: &Sym, v: &str, st: &mut Steps, depth: usize) -> Option<Sym> {
    let Sym::Pow(b, k) = e else { return None };
    let Sym::Fn(g @ (Func::Sin | Func::Cos), a) = &**b else {
        return None;
    };
    let k = k.as_q()?;
    if !k.is_int() || k.n < 3 || k.n % 2 == 0 || k.n > 15 {
        return None;
    }
    let other = func(
        if *g == Func::Sin {
            Func::Cos
        } else {
            Func::Sin
        },
        (**a).clone(),
    );
    let rewritten = mul2(
        (**b).clone(),
        powi(sub(n(1), powi(other.clone(), 2)), (k.n - 1) / 2),
    );
    let (s, c) = if *g == Func::Sin {
        ("sin", "cos")
    } else {
        ("cos", "sin")
    };
    let mark = st.mark();
    st.push(
        format!("Keep one {s} for the substitution and write the rest with {s}² = 1 - {c}²"),
        format!("{}={}", ii(e, v), ii(&rewritten, v)),
    );
    match integ(&expand(&rewritten), v, st, depth + 1) {
        Some(r) => Some(r),
        None => {
            st.undo(mark);
            None
        }
    }
}

/// e^(ax) sin(bx) and e^(ax) cos(bx): by parts twice, solving for the
/// integral when it comes back.
fn exp_trig(e: &Sym, v: &str, st: &mut Steps) -> Option<Sym> {
    let Sym::Mul(fs) = e else { return None };
    if fs.len() != 2 {
        return None;
    }
    let (expo, trig) = match (&fs[0], &fs[1]) {
        (t @ Sym::Fn(..), x @ Sym::Pow(..)) | (x @ Sym::Pow(..), t @ Sym::Fn(..)) => (x, t),
        _ => return None,
    };
    let (Sym::Pow(base, p), Sym::Fn(g, q)) = (expo, trig) else {
        return None;
    };
    if **base != var("e") || !matches!(g, Func::Sin | Func::Cos) {
        return None;
    }
    let (a, _) = linear(p, v)?;
    let (b, _) = linear(q, v)?;
    let s = func(Func::Sin, (**q).clone());
    let c = func(Func::Cos, (**q).clone());
    let top = if *g == Func::Sin {
        sub(mul2(a.clone(), s), mul2(b.clone(), c))
    } else {
        add2(mul2(a.clone(), c), mul2(b.clone(), s))
    };
    let r = div(mul2(expo.clone(), top), add2(powi(a, 2), powi(b, 2)));
    st.push(
        "Integrate by parts twice: the original integral comes back, so solve for it",
        format!("{}={}", ii(e, v), latex(&r)),
    );
    Some(r)
}

/// The pieces of `e` that a substitution u = ... could replace.
fn candidates(e: &Sym, v: &str, out: &mut Vec<Sym>) {
    let push = |s: &Sym, out: &mut Vec<Sym>| {
        if s.has(v) && *s != var(v) && !out.contains(s) {
            out.push(s.clone());
        }
    };
    match e {
        Sym::Fn(_, a) => {
            push(a, out);
            push(e, out);
            candidates(a, v, out);
        }
        Sym::Pow(b, x) => {
            push(b, out);
            push(x, out);
            candidates(b, v, out);
            candidates(x, v, out);
        }
        Sym::Add(xs) | Sym::Mul(xs) => {
            for x in xs {
                if matches!(x, Sym::Add(_)) {
                    push(x, out);
                }
                candidates(x, v, out);
            }
        }
        _ => {}
    }
}

fn substitution(e: &Sym, v: &str, st: &mut Steps, depth: usize) -> Option<Sym> {
    let mut cands = Vec::new();
    candidates(e, v, &mut cands);
    cands.sort_by_key(|c| std::cmp::Reverse(c.size()));
    let u_name = if e.has("u") { "w" } else { "u" };
    let u_var = var(u_name);
    for u in cands.into_iter().take(16) {
        if out_of_time() {
            return None;
        }
        let du = derivative(&u, v);
        if du.is_zero() {
            continue;
        }
        let q = div(e.clone(), du.clone());
        let mut w = replace_power(&q, &u, &u_var);
        if w.has(v) {
            // With u = ax + b, x itself is (u - b)/a.
            let Some((a, b)) = linear(&u, v) else {
                continue;
            };
            if a.has(v) || b.has(v) {
                continue;
            }
            w = expand(&w.subs_var(v, &div(sub(u_var.clone(), b), a)));
            if w.has(v) {
                continue;
            }
        }
        if w.size() > 3 * e.size() + 10 || w == *e {
            continue;
        }
        let mark = st.mark();
        st.push(
            format!(
                "Substitute ${} = {}$, so $d{} = {}\\ d{}$",
                name(u_name),
                latex(&u),
                name(u_name),
                latex(&du),
                name(v)
            ),
            format!("{}={}", ii(e, v), ii(&w, u_name)),
        );
        if let Some(r) = integ(&w, u_name, st, depth + 1) {
            let back = r.subs_var(u_name, &u);
            st.push(
                format!("Put ${} = {}$ back", name(u_name), latex(&u)),
                format!("{}={}", ii(e, v), latex(&back)),
            );
            return Some(back);
        }
        st.undo(mark);
    }
    None
}

/// Ratios of polynomials: long division, then partial fractions.
fn rational(e: &Sym, v: &str, st: &mut Steps, depth: usize) -> Option<Sym> {
    let mark = st.mark();
    let r = rational_steps(e, v, st, depth);
    if r.is_none() {
        st.undo(mark);
    }
    r
}

fn rational_steps(e: &Sym, v: &str, st: &mut Steps, depth: usize) -> Option<Sym> {
    let (top, bottom) = num_den(e);
    if !bottom.has(v) {
        return None;
    }
    let nt = poly::rational(&poly::coeffs(&top, v)?)?;
    let nb = poly::rational(&poly::coeffs(&bottom, v)?)?;
    let x = var(v);
    let mut parts: Vec<Sym> = Vec::new();
    let mark = st.mark();
    let (quot, rem) = poly::divmod(&nt, &nb)?;
    let bottom_sym = poly::build(&poly::to_sym(&nb), v);
    if poly::deg(&nt) >= poly::deg(&nb) {
        let q = poly::build(&poly::to_sym(&quot), v);
        let r = poly::build(&poly::to_sym(&rem), v);
        st.push(
            "Divide the polynomials first",
            format!(
                "{}={}",
                ii(e, v),
                ii(&add2(q.clone(), div(r, bottom_sym.clone())), v)
            ),
        );
        parts.push(q);
    }
    if rem.iter().all(|c| c.is_zero()) {
        return finish(parts, e, v, st, mark, depth);
    }
    let fb = poly::factor(&nb)?;
    // A quadratic that does not factor: complete the square.
    if fb.factors.len() == 1
        && fb.factors[0].0.len() == 3
        && fb.factors[0].1 == 1
        && poly::deg(&rem) <= 1
    {
        let term = div(poly::build(&poly::to_sym(&rem), v), bottom_sym);
        parts.push(term);
        return finish(parts, e, v, st, mark, depth);
    }
    // rem/nb = Σ cᵢ basisᵢ over the unknowns A/(x - r)ʲ and (Bx + C)/q,
    // each written over the product of the factors.
    let whole = product_of_factors(&fb);
    let mut basis: Vec<(Sym, poly::P)> = Vec::new();
    for (f, m) in &fb.factors {
        let fq: poly::P = f.iter().map(|&c| Q::int(c)).collect();
        let fs = poly::build(&poly::to_sym(&fq), v);
        let mut power = vec![Q::ONE];
        for j in 1..=*m {
            power = mul_poly(&power, &fq);
            let (rest, r) = poly::divmod(&whole, &power)?;
            if !r.iter().all(|c| c.is_zero()) {
                return None;
            }
            match f.len() {
                2 => basis.push((div(n(1), powi(fs.clone(), j as i128)), rest)),
                3 if *m == 1 => {
                    basis.push((
                        div(x.clone(), fs.clone()),
                        mul_poly(&rest, &[Q::ZERO, Q::ONE]),
                    ));
                    basis.push((div(n(1), fs.clone()), rest));
                }
                _ => {
                    st.undo(mark);
                    return None;
                }
            }
        }
    }
    let target: poly::P = rem.iter().map(|c| c.div(fb.unit)).collect::<Option<_>>()?;
    let size = basis.len();
    let mut rows: Vec<Vec<Q>> = (0..size)
        .map(|i| {
            let mut row: Vec<Q> = basis
                .iter()
                .map(|(_, p)| *p.get(i).unwrap_or(&Q::ZERO))
                .collect();
            row.push(*target.get(i).unwrap_or(&Q::ZERO));
            row
        })
        .collect();
    let coef = solve_linear(&mut rows)?;
    let terms: Vec<Sym> = basis
        .iter()
        .zip(&coef)
        .filter(|(_, c)| !c.is_zero())
        .map(|((b, _), c)| mul2(Sym::Num(*c), b.clone()))
        .collect();
    // x/q and 1/q terms go back together as one fraction over q.
    let shown = together_quadratics(&terms, v);
    st.push(
        "Split into partial fractions",
        format!(
            "\\frac{{{}}}{{{}}}={}",
            latex(&poly::build(&poly::to_sym(&rem), v)),
            latex(&bottom_sym),
            latex(&shown)
        ),
    );
    parts.extend(shown.terms());
    finish(parts, e, v, st, mark, depth)
}

/// The product of a factored polynomial's whole-number factors.
fn product_of_factors(fb: &poly::Factored) -> poly::P {
    let mut out = vec![Q::ONE];
    for (f, m) in &fb.factors {
        let fq: poly::P = f.iter().map(|&c| Q::int(c)).collect();
        for _ in 0..*m {
            out = mul_poly(&out, &fq);
        }
    }
    out
}

fn together_quadratics(terms: &[Sym], v: &str) -> Sym {
    let mut out: Vec<Sym> = Vec::new();
    let mut quad: Vec<(Sym, Sym)> = Vec::new();
    for t in terms {
        let (top, bottom) = num_den(t);
        if poly::coeffs(&bottom, v).is_some_and(|c| c.len() == 3) {
            match quad.iter_mut().find(|(b, _)| *b == bottom) {
                Some(q) => q.1 = add2(q.1.clone(), top),
                None => quad.push((bottom, top)),
            }
        } else {
            out.push(t.clone());
        }
    }
    for (b, t) in quad {
        out.push(Sym::Mul(vec![t, Sym::Pow(Box::new(b), Box::new(n(-1)))]));
    }
    Sym::Add(out).flatten_one()
}

impl Sym {
    fn flatten_one(self) -> Sym {
        match self {
            Sym::Add(mut xs) if xs.len() == 1 => xs.pop().unwrap(),
            other => other,
        }
    }
}

fn mul_poly(a: &[Q], b: &[Q]) -> poly::P {
    let mut out = vec![Q::ZERO; a.len() + b.len() - 1];
    for (i, x) in a.iter().enumerate() {
        for (j, y) in b.iter().enumerate() {
            out[i + j] = out[i + j]
                .add(x.mul(*y).unwrap_or(Q::ZERO))
                .unwrap_or(Q::ZERO);
        }
    }
    out
}

/// Gaussian elimination on an augmented matrix of fractions.
fn solve_linear(rows: &mut [Vec<Q>]) -> Option<Vec<Q>> {
    let size = rows.len();
    for col in 0..size {
        let pivot = (col..size).find(|&r| !rows[r][col].is_zero())?;
        rows.swap(col, pivot);
        let p = rows[col][col];
        for k in col..=size {
            rows[col][k] = rows[col][k].div(p)?;
        }
        for r in 0..size {
            if r != col && !rows[r][col].is_zero() {
                let f = rows[r][col];
                for k in col..=size {
                    rows[r][k] = rows[r][k].sub(f.mul(rows[col][k])?)?;
                }
            }
        }
    }
    Some(rows.iter().map(|r| r[size]).collect())
}

/// Integrates each piece of a split-up fraction.
fn finish(
    parts: Vec<Sym>,
    e: &Sym,
    v: &str,
    st: &mut Steps,
    mark: usize,
    depth: usize,
) -> Option<Sym> {
    let mut out = Vec::new();
    for p in parts {
        let r = match quadratic_fraction(&p, v, st) {
            Some(r) => Some(r),
            None => integ(&p, v, st, depth + 1),
        };
        match r {
            Some(r) => out.push(r),
            None => {
                st.undo(mark);
                return None;
            }
        }
    }
    let r = add(out);
    st.push("Add the pieces", format!("{}={}", ii(e, v), latex(&r)));
    Some(r)
}

/// (Bx + C)/(ax² + bx + c) with no real roots below: a log for the part
/// that is the bottom's derivative, and an arctangent for the rest.
fn quadratic_fraction(t: &Sym, v: &str, st: &mut Steps) -> Option<Sym> {
    let (top, bottom) = num_den(t);
    let qb = poly::rational(&poly::coeffs(&bottom, v)?)?;
    let qt = poly::rational(&poly::coeffs(&top, v)?)?;
    if qb.len() != 3 || qt.len() > 2 {
        return None;
    }
    let (c, b, a) = (qb[0], qb[1], qb[2]);
    let disc = b.mul(b)?.sub(Q::int(4).mul(a)?.mul(c)?)?;
    if disc.n >= 0 {
        return None;
    }
    let big_b = *qt.get(1).unwrap_or(&Q::ZERO);
    let big_c = qt[0];
    let x = var(v);
    let quad = poly::build(&poly::to_sym(&qb), v);
    // Bx + C = (B/2a)(2ax + b) + (C - Bb/2a).
    let k = big_b.div(Q::int(2).mul(a)?)?;
    let rest = big_c.sub(k.mul(b)?)?;
    let root = sqrt(Sym::Num(disc.neg()));
    let log_part = mul2(Sym::Num(k), func(Func::Ln, func(Func::Abs, quad.clone())));
    let atan_part = mul(vec![
        Sym::Num(rest),
        n(2),
        pow(root.clone(), n(-1)),
        func(
            Func::Atan,
            div(
                add2(mul2(Sym::Num(Q::int(2).mul(a)?), x), Sym::Num(b)),
                root,
            ),
        ),
    ]);
    let r = add2(log_part, atan_part);
    st.push(
        "The bottom has no real roots: the part of the top that is its derivative gives a log, and completing the square gives an arctangent",
        format!("{}={}", ii(t, v), latex(&r)),
    );
    Some(r)
}

/// Where a factor ranks as the u in integration by parts (LIATE).
fn rank(f: &Sym, v: &str) -> u8 {
    match f {
        Sym::Fn(Func::Ln | Func::Log, _) => 5,
        Sym::Fn(
            Func::Asin | Func::Acos | Func::Atan | Func::Asinh | Func::Acosh | Func::Atanh,
            _,
        ) => 4,
        _ if poly::coeffs(f, v).is_some() => 3,
        Sym::Fn(..) => 2,
        Sym::Pow(_, x) if x.has(v) => 1,
        _ => 0,
    }
}

fn parts(e: &Sym, v: &str, st: &mut Steps, depth: usize) -> Option<Sym> {
    if depth > 5 {
        return None;
    }
    let fs = e.factors();
    let (u, dv) = if fs.len() == 1 {
        if rank(e, v) < 4 {
            return None;
        }
        (e.clone(), n(1))
    } else {
        let best = (0..fs.len()).max_by_key(|&i| rank(&fs[i], v))?;
        if rank(&fs[best], v) == 0 {
            return None;
        }
        let rest: Vec<Sym> = fs
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != best)
            .map(|(_, f)| f.clone())
            .collect();
        (fs[best].clone(), mul(rest))
    };
    if rank(&dv, v) >= 3 && rank(&u, v) == 3 {
        return None;
    }
    let mut quiet = Steps::quiet();
    let w = integ(&dv, v, &mut quiet, depth + 1)?;
    let du = derivative(&u, v);
    let inner = mul2(w.clone(), du.clone());
    let mark = st.mark();
    st.push(
        format!(
            "Integrate by parts, ∫u dv = uv - ∫v du, with $u = {}$ and $dv = {}\\ d{}$, so $du = {}\\ d{}$ and $v = {}$",
            latex(&u),
            latex(&dv),
            name(v),
            latex(&du),
            name(v),
            latex(&w)
        ),
        format!("{}={}-{}", ii(e, v), latex(&mul2(u.clone(), w.clone())), ii(&inner, v)),
    );
    match integ(&inner, v, st, depth + 1) {
        Some(r) => {
            let out = sub(mul2(u, w), r);
            st.push("Put it together", format!("{}={}", ii(e, v), latex(&out)));
            Some(out)
        }
        None => {
            st.undo(mark);
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(e: Sym) {
        let f = antiderivative(&e, "x", &mut Steps::quiet())
            .unwrap_or_else(|| panic!("no antiderivative for {e:?}"));
        // The derivative of the answer is the integrand, checked at points.
        let d = derivative(&f, "x");
        for x in [0.3, 0.7, 1.9, 2.6] {
            let (a, b) = (d.at("x", x), e.at("x", x));
            assert!(
                (a - b).abs() < 1e-9 * b.abs().max(1.0),
                "{e:?} at {x}: {a} vs {b}"
            );
        }
    }

    #[test]
    fn standard_integrals() {
        let x = var("x");
        check(powi(x.clone(), 3));
        check(mul2(x.clone(), exp(powi(x.clone(), 2))));
        check(mul2(x.clone(), func(Func::Sin, x.clone())));
        check(func(Func::Ln, x.clone()));
        check(div(n(1), add2(powi(x.clone(), 2), n(1))));
        check(div(n(1), sub(powi(x.clone(), 2), n(1))));
        check(div(
            add2(x.clone(), n(3)),
            add(vec![powi(x.clone(), 2), x.clone(), n(1)]),
        ));
        check(mul2(exp(x.clone()), func(Func::Cos, x.clone())));
        check(mul2(x.clone(), sqrt(add2(x.clone(), n(1)))));
        check(powi(add2(mul2(n(2), x.clone()), n(1)), 5));
        check(mul2(func(Func::Sin, x.clone()), func(Func::Cos, x.clone())));
        check(div(powi(x.clone(), 3), sub(powi(x.clone(), 2), n(4))));
        check(div(n(1), mul2(x.clone(), powi(add2(x.clone(), n(1)), 2))));
    }
}
