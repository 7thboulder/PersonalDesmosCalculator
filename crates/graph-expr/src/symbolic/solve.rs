//! Solving equations and inequalities for one variable, the way it is
//! done by hand: undo each operation in turn, clear fractions, factor,
//! use the quadratic formula, and check answers that squaring may add.

use super::diff::paren;
use super::latex::{latex, name};
use super::poly;
use super::sym::*;
use super::{nice, out_of_time, Steps};
use crate::ast::Relation;

pub enum Outcome {
    /// These values, which may contain `k` for "any integer".
    Roots(Vec<Sym>),
    /// Every value works.
    All,
    /// Nothing found by any method.
    Fail,
}

/// The name used for "any integer" in general solutions.
pub fn integer_name(v: &str) -> &'static str {
    if v == "k" {
        "n"
    } else {
        "k"
    }
}

fn eq(l: &Sym, r: &Sym) -> String {
    format!("{}={}", latex(l), latex(r))
}

fn either(l: &Sym, rs: &[Sym]) -> String {
    rs.iter()
        .map(|r| eq(l, r))
        .collect::<Vec<_>>()
        .join("\\quad\\text{or}\\quad ")
}

/// How many times `v` itself appears.
pub fn occurrences(s: &Sym, v: &str) -> usize {
    match s {
        Sym::Var(x) => (x == v) as usize,
        Sym::Add(xs) | Sym::Mul(xs) => xs.iter().map(|x| occurrences(x, v)).sum(),
        Sym::Pow(a, b) => occurrences(a, v) + occurrences(b, v),
        Sym::Fn(_, a) => occurrences(a, v),
        _ => 0,
    }
}

pub fn solve(lhs: Sym, rhs: Sym, v: &str, st: &mut Steps, depth: usize) -> Outcome {
    if depth > 6 || out_of_time() {
        return Outcome::Fail;
    }
    let x = var(v);
    if !lhs.has(v) && !rhs.has(v) {
        let d = sub(lhs, rhs);
        if d.is_zero() {
            st.push(
                format!("Both sides are always equal, so every ${}$ works", name(v)),
                "",
            );
            return Outcome::All;
        }
        if d.is_constant() {
            st.push(
                format!(
                    "${}$ is gone and the sides differ, so there is no solution",
                    name(v)
                ),
                "",
            );
            return Outcome::Roots(Vec::new());
        }
        return Outcome::Fail;
    }
    let (l, r) = if !lhs.has(v) {
        st.push("Swap the sides", eq(&rhs, &lhs));
        (rhs, lhs)
    } else {
        (lhs, rhs)
    };
    if l == x && !r.has(v) {
        return Outcome::Roots(vec![r]);
    }
    if !r.has(v) && occurrences(&l, v) == 1 {
        if let Some(out) = isolate(l.clone(), vec![r.clone()], v, st) {
            return out;
        }
    }
    let f = sub(l, r.clone());
    let multiplied = expand(&f);
    let f = if latex(&multiplied).len() <= latex(&f).len() {
        multiplied
    } else {
        f
    };
    if f.is_zero() {
        st.push(
            format!("Both sides are the same, so every ${}$ works", name(v)),
            "",
        );
        return Outcome::All;
    }
    if !r.is_zero() {
        st.push("Move everything to one side", eq(&f, &n(0)));
    }
    let (top, bottom) = num_den(&f);
    let f = if !bottom.is_one() {
        let top = expand(&top);
        if bottom.has(v) {
            st.push(
                "Write it as one fraction; a fraction is zero where its top is, as long as its bottom isn't",
                format!("\\frac{{{}}}{{{}}}=0", latex(&top), latex(&bottom)),
            );
            st.push("Set the top to zero", eq(&top, &n(0)));
        } else {
            st.push(
                format!(
                    "Multiply both sides by ${}$ to clear the fractions",
                    latex(&bottom)
                ),
                eq(&top, &n(0)),
            );
        }
        top
    } else {
        f
    };
    zero(f, v, st, depth)
}

/// Solves f = 0.
fn zero(f: Sym, v: &str, st: &mut Steps, depth: usize) -> Outcome {
    if out_of_time() {
        return Outcome::Fail;
    }
    if let Some(c) = poly::coeffs(&f, v) {
        if c.len() >= 2 {
            let expanded = poly::build(&c, v);
            if expanded != f {
                st.push("Multiply out and collect like terms", eq(&expanded, &n(0)));
            }
            return polynomial(c, v, st);
        }
    }
    if occurrences(&f, v) == 1 {
        if let Some(out) = isolate(f.clone(), vec![n(0)], v, st) {
            return out;
        }
    }
    if let Sym::Mul(fs) = &f {
        let varying: Vec<Sym> = fs.iter().filter(|g| g.has(v)).cloned().collect();
        if varying.len() >= 2 || varying.len() < fs.len() {
            return product(varying, v, st, depth);
        }
    }
    let common = common_factor(&f);
    if let Sym::Mul(fs) = &common {
        if fs.iter().filter(|g| g.has(v)).count() >= 2 {
            st.push("Take out the common factor", eq(&common, &n(0)));
            let varying = fs.iter().filter(|g| g.has(v)).cloned().collect();
            return product(varying, v, st, depth);
        }
    }
    if let Some(out) = substitute(&f, v, st, depth) {
        return out;
    }
    if let Some(out) = radical(&f, v, st, depth) {
        return out;
    }
    numeric(&f, v, st)
}

/// A product is zero where one of its factors is.
fn product(factors: Vec<Sym>, v: &str, st: &mut Steps, depth: usize) -> Outcome {
    let mut zeros = Vec::new();
    for g in &factors {
        match g {
            Sym::Pow(b, e) if e.is_negative() => {
                let _ = b;
            }
            Sym::Pow(b, _) if **b == var("e") => {}
            Sym::Pow(b, e) if e.as_q().is_some_and(|q| q.n > 0) => zeros.push((**b).clone()),
            other => zeros.push(other.clone()),
        }
    }
    if zeros.len() > 1 {
        let shown: Vec<String> = zeros.iter().map(|z| eq(z, &n(0))).collect();
        st.push(
            "A product is zero when one of its factors is",
            shown.join("\\quad\\text{or}\\quad "),
        );
    }
    let mut roots = Vec::new();
    for z in zeros {
        match solve(z, n(0), v, st, depth + 1) {
            Outcome::Roots(r) => roots.extend(r),
            Outcome::All => return Outcome::All,
            Outcome::Fail => return Outcome::Fail,
        }
    }
    Outcome::Roots(roots)
}

/// The factors every term shares: x²y + xy² is xy(x + y).
pub fn common_factor(f: &Sym) -> Sym {
    let Sym::Add(ts) = f else { return f.clone() };
    let mut shared: Option<Vec<(Sym, Q)>> = None;
    let mut numeric: Option<i128> = None;
    for t in ts {
        let (c, rest) = split_coef(t);
        numeric = match (numeric, c.as_q()) {
            (None, Some(q)) if q.is_int() => Some(q.n.abs()),
            (Some(g), Some(q)) if q.is_int() => Some(gcd(g, q.n)),
            _ => Some(1),
        };
        let mine: Vec<(Sym, Q)> = rest
            .factors()
            .into_iter()
            .filter(|x| !x.is_one())
            .map(|x| match x {
                Sym::Pow(b, e) if e.as_q().is_some_and(|q| q.n > 0) => (*b, e.as_q().unwrap()),
                other => (other, Q::ONE),
            })
            .filter(|(b, _)| !b.is_num())
            .collect();
        shared = Some(match shared {
            None => mine,
            Some(prev) => prev
                .into_iter()
                .filter_map(|(b, e)| {
                    let (_, e2) = mine.iter().find(|(b2, _)| *b2 == b)?;
                    Some((b, if *e2 < e { *e2 } else { e }))
                })
                .collect(),
        });
    }
    let mut g: Vec<Sym> = shared
        .unwrap_or_default()
        .into_iter()
        .map(|(b, e)| pow(b, Sym::Num(e)))
        .collect();
    let numeric = numeric.unwrap_or(1).max(1);
    // Keep the leading term positive inside the brackets.
    let sign = if ts[0].is_negative() { -1 } else { 1 };
    g.push(n(sign * numeric));
    let g = mul(g);
    if g.is_one() {
        return f.clone();
    }
    let inside = add(ts.iter().map(|t| div(t.clone(), g.clone())).collect());
    Sym::Mul(vec![g, inside]).resimplify()
}

impl Sym {
    /// A product built by hand, put back in canonical order without
    /// multiplying out.
    fn resimplify(self) -> Sym {
        match self {
            Sym::Mul(xs) => mul(xs),
            other => other,
        }
    }
}

/// Undoes the operations around the only `v`, one at a time.
fn isolate(mut lhs: Sym, mut rs: Vec<Sym>, v: &str, st: &mut Steps) -> Option<Outcome> {
    let k = var(integer_name(v));
    let positive = |r: &Sym| !(r.is_constant() && r.value() < 0.0);
    let mut guard = 0;
    while lhs != var(v) {
        guard += 1;
        if guard > 40 {
            return None;
        }
        let before = st.mark();
        let next: Sym;
        match lhs.clone() {
            Sym::Add(ts) => {
                let (with, without): (Vec<Sym>, Vec<Sym>) = ts.into_iter().partition(|t| t.has(v));
                let c = add(without);
                rs = rs.into_iter().map(|r| sub(r, c.clone())).collect();
                next = with.into_iter().next()?;
                let note = if c.is_negative() {
                    format!("Add ${}$ to both sides", latex(&neg(c)))
                } else {
                    format!("Subtract ${}$ from both sides", latex(&c))
                };
                st.push(note, either(&next, &rs));
            }
            Sym::Mul(fs) => {
                let (with, without): (Vec<Sym>, Vec<Sym>) = fs.into_iter().partition(|t| t.has(v));
                let c = mul(without);
                rs = rs.into_iter().map(|r| div(r, c.clone())).collect();
                next = with.into_iter().next()?;
                let inverse = div(n(1), c.clone());
                let note = if c.is_int(-1) {
                    "Multiply both sides by -1".to_owned()
                } else if inverse.as_q().is_some_and(Q::is_int) {
                    format!("Multiply both sides by ${}$", latex(&inverse))
                } else {
                    format!("Divide both sides by ${}$", latex(&c))
                };
                st.push(note, either(&next, &rs));
            }
            Sym::Pow(b, e) if b.has(v) && !e.has(v) => {
                next = *b;
                let q = e.as_q();
                if e.is_int(-1) {
                    rs.retain(|r| !r.is_zero());
                    rs = rs.into_iter().map(|r| div(n(1), r)).collect();
                    st.push("Flip both sides over", either(&next, &rs));
                } else if q.is_some_and(|q| q.n == 1 && q.d % 2 == 0) {
                    let kth = q.unwrap().d;
                    let dropped = rs.len();
                    rs.retain(positive);
                    let what = if kth == 2 { "square root" } else { "even root" };
                    if rs.len() < dropped {
                        st.push(
                            format!("A {what} is never negative, so that case has no solution"),
                            "",
                        );
                    }
                    rs = rs.into_iter().map(|r| powi(r, kth)).collect();
                    let note = if kth == 2 {
                        "Square both sides".to_owned()
                    } else {
                        format!("Raise both sides to the power {kth}")
                    };
                    st.push(note, either(&next, &rs));
                } else if q.is_some_and(|q| q.is_int() && q.n % 2 == 0) {
                    let kth = q.unwrap().n;
                    let dropped = rs.len();
                    rs.retain(positive);
                    if rs.len() < dropped {
                        st.push(
                            "An even power is never negative, so that case has no solution",
                            "",
                        );
                    }
                    let root = frac(1, kth);
                    let mut out = Vec::new();
                    for r in rs {
                        let p = nice(&pow(r, root.clone()));
                        if p.is_zero() {
                            out.push(p);
                        } else {
                            out.push(p.clone());
                            out.push(neg(p));
                        }
                    }
                    rs = out;
                    let note = if kth == 2 {
                        "Take the square root of both sides, keeping both signs".to_owned()
                    } else {
                        format!(
                            "Take the {} root of both sides, keeping both signs",
                            ordinal(kth)
                        )
                    };
                    st.push(note, either(&next, &rs));
                } else if q.is_some_and(|q| q.is_int() && q.n > 0) {
                    let kth = q.unwrap().n;
                    rs = rs
                        .into_iter()
                        .map(|r| nice(&pow(r, frac(1, kth))))
                        .collect();
                    let note = if kth == 3 {
                        "Take the cube root of both sides".to_owned()
                    } else {
                        format!("Take the {} root of both sides", ordinal(kth))
                    };
                    st.push(note, either(&next, &rs));
                } else {
                    rs = rs
                        .into_iter()
                        .map(|r| pow(r, div(n(1), (*e).clone())))
                        .collect();
                    st.push(
                        format!("Raise both sides to the power ${}$", latex(&div(n(1), *e))),
                        either(&next, &rs),
                    );
                }
            }
            Sym::Pow(b, e) if !b.has(v) => {
                next = *e;
                let dropped = rs.len();
                rs.retain(|r| !(r.is_constant() && r.value() <= 0.0));
                if rs.len() < dropped {
                    st.push(
                        format!(
                            "${}$ to any power is positive, so that case has no solution",
                            latex(&b)
                        ),
                        "",
                    );
                }
                if *b == var("e") {
                    rs = rs.into_iter().map(|r| func(Func::Ln, r)).collect();
                    st.push("Take the natural log of both sides", either(&next, &rs));
                } else if b.is_int(10) {
                    rs = rs.into_iter().map(|r| func(Func::Log, r)).collect();
                    st.push("Take log base 10 of both sides", either(&next, &rs));
                } else {
                    rs = rs.into_iter().map(|r| log_base(&b, r)).collect();
                    st.push(
                        format!("Take the log of both sides, base ${}$", latex(&b)),
                        either(&next, &rs),
                    );
                }
            }
            Sym::Fn(f, u) => {
                next = *u;
                let in_range = |r: &Sym, lo: f64, hi: f64| {
                    !(r.is_constant() && !(lo - 1e-12..=hi + 1e-12).contains(&r.value()))
                };
                let periodic = |base: Sym, period: Sym| add2(base, mul2(period, k.clone()));
                let two_pi = mul2(n(2), pi());
                let mut out = Vec::new();
                let note: String;
                match f {
                    Func::Ln => {
                        out = rs.iter().map(|r| exp(r.clone())).collect();
                        note = "Raise e to the power of each side".into();
                    }
                    Func::Log => {
                        out = rs.iter().map(|r| pow(n(10), r.clone())).collect();
                        note = "Raise 10 to the power of each side".into();
                    }
                    Func::Abs => {
                        for r in rs.iter().filter(|r| positive(r)) {
                            out.push(r.clone());
                            if !r.is_zero() {
                                out.push(neg(r.clone()));
                            }
                        }
                        note = "An absolute value is this number when the inside is it or its negative".into();
                    }
                    Func::Sin => {
                        for r in rs.iter().filter(|r| in_range(r, -1.0, 1.0)) {
                            let a = func(Func::Asin, r.clone());
                            out.push(periodic(a.clone(), two_pi.clone()));
                            if !(r.is_int(1) || r.is_int(-1)) {
                                out.push(periodic(sub(pi(), a), two_pi.clone()));
                            }
                        }
                        note = format!(
                            "Take arcsin of both sides. Sine repeats every $2\\pi$ and sin(π - θ) = sin θ, so each value gives two families (${}$ is any integer)",
                            name(integer_name(v))
                        );
                    }
                    Func::Cos => {
                        for r in rs.iter().filter(|r| in_range(r, -1.0, 1.0)) {
                            let a = func(Func::Acos, r.clone());
                            out.push(periodic(a.clone(), two_pi.clone()));
                            if !(r.is_int(1) || r.is_int(-1)) {
                                out.push(periodic(neg(a), two_pi.clone()));
                            }
                        }
                        note = format!(
                            "Take arccos of both sides. Cosine repeats every $2\\pi$ and cos(-θ) = cos θ, so each value gives two families (${}$ is any integer)",
                            name(integer_name(v))
                        );
                    }
                    Func::Tan => {
                        out = rs
                            .iter()
                            .map(|r| periodic(func(Func::Atan, r.clone()), pi()))
                            .collect();
                        note = format!(
                            "Take arctan of both sides. Tangent repeats every $\\pi$ (${}$ is any integer)",
                            name(integer_name(v))
                        );
                    }
                    Func::Csc | Func::Sec | Func::Cot => {
                        let (g, word) = match f {
                            Func::Csc => (Func::Sin, "sin"),
                            Func::Sec => (Func::Cos, "cos"),
                            _ => (Func::Tan, "tan"),
                        };
                        if f == Func::Cot && rs.iter().any(Sym::is_zero) {
                            // cot u = 0 where u = π/2 + kπ.
                            lhs = next;
                            rs = vec![periodic(div(pi(), n(2)), pi())];
                            st.push("Cotangent is zero where cosine is", either(&lhs, &rs));
                            continue;
                        }
                        rs = rs
                            .into_iter()
                            .filter(|r| !r.is_zero())
                            .map(|r| div(n(1), r))
                            .collect();
                        lhs = func(g, next);
                        st.push(
                            format!("Write it with {word}, flipping both sides over"),
                            either(&lhs, &rs),
                        );
                        continue;
                    }
                    Func::Asin => {
                        out = rs
                            .iter()
                            .filter(|r| in_range(r, -PI2, PI2))
                            .map(|r| func(Func::Sin, r.clone()))
                            .collect();
                        note = "Take sin of both sides".into();
                    }
                    Func::Acos => {
                        out = rs
                            .iter()
                            .filter(|r| in_range(r, 0.0, std::f64::consts::PI))
                            .map(|r| func(Func::Cos, r.clone()))
                            .collect();
                        note = "Take cos of both sides".into();
                    }
                    Func::Atan => {
                        out = rs
                            .iter()
                            .filter(|r| in_range(r, -PI2, PI2))
                            .map(|r| func(Func::Tan, r.clone()))
                            .collect();
                        note = "Take tan of both sides".into();
                    }
                    Func::Sinh => {
                        out = rs.iter().map(|r| func(Func::Asinh, r.clone())).collect();
                        note = "Take arcsinh of both sides".into();
                    }
                    Func::Cosh => {
                        for r in rs.iter().filter(|r| in_range(r, 1.0, f64::INFINITY)) {
                            let a = func(Func::Acosh, r.clone());
                            out.push(a.clone());
                            if !r.is_one() {
                                out.push(neg(a));
                            }
                        }
                        note = "Take arccosh of both sides, keeping both signs".into();
                    }
                    Func::Tanh => {
                        out = rs
                            .iter()
                            .filter(|r| in_range(r, -1.0, 1.0))
                            .map(|r| func(Func::Atanh, r.clone()))
                            .collect();
                        note = "Take arctanh of both sides".into();
                    }
                    Func::Asinh => {
                        out = rs.iter().map(|r| func(Func::Sinh, r.clone())).collect();
                        note = "Take sinh of both sides".into();
                    }
                    Func::Acosh => {
                        out = rs
                            .iter()
                            .filter(|r| in_range(r, 0.0, f64::INFINITY))
                            .map(|r| func(Func::Cosh, r.clone()))
                            .collect();
                        note = "Take cosh of both sides".into();
                    }
                    Func::Atanh => {
                        out = rs.iter().map(|r| func(Func::Tanh, r.clone())).collect();
                        note = "Take tanh of both sides".into();
                    }
                }
                if out.len() < rs.len()
                    && !matches!(f, Func::Abs | Func::Sin | Func::Cos | Func::Cosh)
                {
                    st.push(
                        "That value is out of this function's range, so that case has no solution",
                        "",
                    );
                }
                rs = out;
                st.push(note, either(&next, &rs));
            }
            _ => {
                st.undo(before);
                return None;
            }
        }
        lhs = next;
        if rs.is_empty() {
            st.push("No case is left, so there is no solution", "");
            return Some(Outcome::Roots(Vec::new()));
        }
    }
    Some(Outcome::Roots(rs.into_iter().map(|r| nice(&r)).collect()))
}

/// log_b(r), exactly when r is a rational power of b: log₂ 8 = 3.
fn log_base(b: &Sym, r: Sym) -> Sym {
    if b.is_constant() && r.is_constant() {
        let ratio = r.value().ln() / b.value().ln();
        for d in 1..=6i128 {
            let k = (ratio * d as f64).round();
            if (ratio * d as f64 - k).abs() < 1e-9 {
                let q = frac(k as i128, d);
                if pow(b.clone(), q.clone()) == r {
                    return q;
                }
            }
        }
    }
    div(func(Func::Ln, r), func(Func::Ln, b.clone()))
}

const PI2: f64 = std::f64::consts::FRAC_PI_2;

fn ordinal(k: i128) -> String {
    match k {
        2 => "square".into(),
        3 => "cube".into(),
        k => format!("{k}th"),
    }
}

/// Polynomial equations: factor out x, then by degree.
fn polynomial(mut c: Vec<Sym>, v: &str, st: &mut Steps) -> Outcome {
    let x = var(v);
    let mut roots = Vec::new();
    let lowest = c.iter().position(|a| !a.is_zero()).unwrap_or(0);
    let degree = c.len() - 1;
    if lowest > 0 {
        let rest = poly::build(&c[lowest..], v);
        let xm = powi(x.clone(), lowest as i128);
        if degree == lowest {
            st.push(
                format!("Divide both sides by ${}$", latex(&c[lowest])),
                eq(&xm, &n(0)),
            );
            return Outcome::Roots(vec![n(0)]);
        }
        st.push(
            format!("Take out ${}$", latex(&xm)),
            format!("{}\\left({}\\right)=0", latex(&xm), latex(&rest)),
        );
        st.push(
            "A product is zero when one of its factors is",
            format!(
                "{}=0\\quad\\text{{or}}\\quad {}=0",
                latex(&xm),
                latex(&rest)
            ),
        );
        roots.push(n(0));
        c = c[lowest..].to_vec();
    }
    match polynomial_nonzero(c, v, st) {
        Outcome::Roots(r) => roots.extend(r),
        other => return other,
    }
    Outcome::Roots(roots)
}

fn polynomial_nonzero(c: Vec<Sym>, v: &str, st: &mut Steps) -> Outcome {
    let x = var(v);
    let assume = |a: &Sym| {
        if a.is_constant() {
            String::new()
        } else {
            format!(" (assuming ${}\\ne0$)", latex(a))
        }
    };
    match c.len() - 1 {
        0 => Outcome::Roots(Vec::new()),
        1 => {
            let (b, a) = (c[0].clone(), c[1].clone());
            let ax = mul2(a.clone(), x.clone());
            if !b.is_zero() {
                let note = if b.is_negative() {
                    format!("Add ${}$ to both sides", latex(&neg(b.clone())))
                } else {
                    format!("Subtract ${}$ from both sides", latex(&b))
                };
                st.push(note, eq(&ax, &neg(b.clone())));
            }
            let r = nice(&div(neg(b), a.clone()));
            if !a.is_one() {
                st.push(
                    format!("Divide both sides by ${}${}", latex(&a), assume(&a)),
                    eq(&x, &r),
                );
            }
            Outcome::Roots(vec![r])
        }
        2 => quadratic(&c[0], &c[1], &c[2], v, st),
        _ => higher(c, v, st),
    }
}

fn quadratic(c: &Sym, b: &Sym, a: &Sym, v: &str, st: &mut Steps) -> Outcome {
    let x = var(v);
    let xs = name(v);
    if b.is_zero() {
        let r = nice(&div(neg(c.clone()), a.clone()));
        let x2 = powi(x.clone(), 2);
        st.push("Get the square on its own", eq(&x2, &r));
        if r.is_constant() && r.value() < 0.0 {
            let root = nice(&sqrt(neg(r)));
            st.push(
                "A square is never negative, so there are no real solutions",
                format!("{xs}=\\pm {}i\\ \\text{{(complex)}}", paren(&root)),
            );
            return Outcome::Roots(Vec::new());
        }
        let root = nice(&sqrt(r));
        st.push(
            "Take the square root of both sides, keeping both signs",
            format!("{xs}=\\pm {}", latex(&root)),
        );
        return if root.is_zero() {
            Outcome::Roots(vec![root])
        } else {
            Outcome::Roots(vec![neg(root.clone()), root])
        };
    }
    // Factor when the roots are rational.
    if let (Some(qa), Some(qb), Some(qc)) = (a.as_q(), b.as_q(), c.as_q()) {
        let disc = qb.mul(qb).and_then(|b2| {
            qa.mul(qc)
                .and_then(|ac| ac.mul(Q::int(4)))
                .and_then(|ac4| b2.sub(ac4))
        });
        if let Some(disc) = disc {
            if poly::rational_sqrt(disc).is_some() {
                if let Some(mut f) = poly::factor(&[qc, qb, qa]) {
                    f.factors.sort_by(|a, b| {
                        (-a.0[0] as f64 / a.0[1] as f64)
                            .total_cmp(&(-b.0[0] as f64 / b.0[1] as f64))
                    });
                    let fs = f.to_sym(v);
                    st.push("Factor", eq(&fs, &n(0)));
                    let lin: Vec<Sym> = f
                        .factors
                        .iter()
                        .map(|(p, _)| poly::build(&[n(p[0]), n(p[1])], v))
                        .collect();
                    if lin.len() > 1 {
                        st.push(
                            "Set each factor to zero",
                            lin.iter()
                                .map(|l| eq(l, &n(0)))
                                .collect::<Vec<_>>()
                                .join("\\quad\\text{or}\\quad "),
                        );
                    }
                    let roots: Vec<Sym> =
                        f.factors.iter().map(|(p, _)| frac(-p[0], p[1])).collect();
                    if roots.iter().any(|r| !r.is_zero()) || lin.len() > 1 {
                        st.push("Solve each one", either(&x, &roots));
                    }
                    return Outcome::Roots(roots);
                }
            }
        }
    }
    st.push(
        format!(
            "Use the quadratic formula with $a={}$, $b={}$, $c={}$",
            latex(a),
            latex(b),
            latex(c)
        ),
        format!("{xs}=\\frac{{-b\\pm\\sqrt{{b^{{2}}-4ac}}}}{{2a}}"),
    );
    st.push(
        "Put the numbers in",
        format!(
            "{xs}=\\frac{{-{}\\pm\\sqrt{{{}^{{2}}-4\\cdot {}\\cdot {}}}}}{{2\\cdot {}}}",
            paren(b),
            paren(b),
            paren(a),
            paren(c),
            paren(a)
        ),
    );
    let disc = expand(&sub(
        powi(b.clone(), 2),
        mul(vec![n(4), a.clone(), c.clone()]),
    ));
    let minus_b = neg(b.clone());
    let two_a = mul2(n(2), a.clone());
    st.push(
        "Simplify",
        format!(
            "{xs}=\\frac{{{}\\pm\\sqrt{{{}}}}}{{{}}}",
            latex(&minus_b),
            latex(&disc),
            latex(&two_a)
        ),
    );
    if disc.is_constant() && disc.value() < 0.0 {
        let re = nice(&div(minus_b, two_a.clone()));
        let im = nice(&div(sqrt(neg(disc)), two_a));
        st.push(
            "The number under the root is negative, so there are no real solutions. The complex ones are",
            format!("{xs}={}\\pm {}i", latex(&re), paren(&im)),
        );
        return Outcome::Roots(Vec::new());
    }
    if disc.is_zero() {
        let r = nice(&div(minus_b, two_a));
        st.push(
            "The root is zero, so there is just one solution",
            eq(&x, &r),
        );
        return Outcome::Roots(vec![r]);
    }
    let root = sqrt(disc);
    let r1 = nice(&div(sub(minus_b.clone(), root.clone()), two_a.clone()));
    let r2 = nice(&div(add2(minus_b, root), two_a));
    Outcome::Roots(vec![r1, r2])
}

/// Cubics and beyond: rational roots, then x² as a new variable, then
/// numbers.
fn higher(c: Vec<Sym>, v: &str, st: &mut Steps) -> Outcome {
    let x = var(v);
    let Some(p) = poly::rational(&c) else {
        st.push(
            "Solve can't solve polynomials of this degree with letters in them",
            "",
        );
        return Outcome::Fail;
    };
    if let Some(r) = poly::rational_root(&p) {
        let Some(q) = poly::deflate(&p, r) else {
            return Outcome::Fail;
        };
        let qs = poly::build(&poly::to_sym(&q), v);
        let factor = sub(x.clone(), Sym::Num(r));
        st.push(
            format!(
                "Try the rational roots (factors of the constant over factors of the leading coefficient): ${}={}$ makes it zero",
                name(v),
                latex(&Sym::Num(r))
            ),
            "",
        );
        st.push(
            format!("So ${}$ is a factor; divide it out", latex(&factor)),
            format!("{}\\left({}\\right)=0", paren(&factor), latex(&qs)),
        );
        let rest = polynomial_nonzero(poly::to_sym(&q), v, st);
        return match rest {
            Outcome::Roots(mut rs) => {
                rs.insert(0, Sym::Num(r));
                Outcome::Roots(rs)
            }
            other => other,
        };
    }
    // x⁴ - 5x² + 4: a quadratic in x².
    let even = p.iter().enumerate().all(|(k, a)| k % 2 == 0 || a.is_zero());
    if even && poly::deg(&p) == 4 {
        let u = if v == "u" { "w" } else { "u" };
        let half: Vec<Sym> = p.iter().step_by(2).map(|q| Sym::Num(*q)).collect();
        st.push(
            format!(
                "Only even powers appear, so let ${} = {}^{{2}}$",
                name(u),
                name(v)
            ),
            eq(&poly::build(&half, u), &n(0)),
        );
        if let Outcome::Roots(us) = quadratic(&half[0], &half[1], &half[2], u, st) {
            let mut roots = Vec::new();
            let mut shown = Vec::new();
            for r in us {
                if r.is_constant() && r.value() < 0.0 {
                    continue;
                }
                let s = nice(&sqrt(r.clone()));
                shown.push(format!("{}^{{2}}={}", name(v), latex(&r)));
                if s.is_zero() {
                    roots.push(s);
                } else {
                    roots.push(neg(s.clone()));
                    roots.push(s);
                }
            }
            st.push(
                format!("Put ${}^{{2}}$ back and take square roots (negative values of ${}$ give no real ${}$)", name(v), name(u), name(v)),
                shown.join("\\quad\\text{or}\\quad "),
            );
            return Outcome::Roots(roots);
        }
    }
    numeric_poly(&p, v, st)
}

fn numeric_poly(p: &[Q], v: &str, st: &mut Steps) -> Outcome {
    let real: Vec<f64> = poly::numeric_roots(&p.iter().map(|q| q.to_f64()).collect::<Vec<_>>())
        .into_iter()
        .filter(|z| z.1 == 0.0)
        .map(|z| z.0)
        .collect();
    let mut real = real;
    real.sort_by(f64::total_cmp);
    real.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
    st.push(
        "There are no rational roots and no formula Solve can use, so these are found numerically",
        "",
    );
    let _ = v;
    Outcome::Roots(real.into_iter().map(Sym::Real).collect())
}

/// Equations that are polynomials in something else: e^(2x) - 3eˣ + 2 = 0
/// with u = eˣ, or 2cos²x - cos x - 1 = 0 with u = cos x.
fn substitute(f: &Sym, v: &str, st: &mut Steps, depth: usize) -> Option<Outcome> {
    let mut cands = Vec::new();
    atoms(f, v, &mut cands);
    cands.sort_by_key(Sym::size);
    let u = if f.has("u") { "w" } else { "u" };
    for a in cands {
        let w = replace_power(f, &a, &var(u));
        if w.has(v) {
            continue;
        }
        let Some(c) = poly::coeffs(&w, u) else {
            continue;
        };
        if c.len() < 3 {
            continue;
        }
        st.push(
            format!(
                "This is a polynomial in ${}$; let ${} = {}$",
                latex(&a),
                name(u),
                latex(&a)
            ),
            eq(&poly::build(&c, u), &n(0)),
        );
        let Outcome::Roots(us) = polynomial(c, u, st) else {
            return Some(Outcome::Fail);
        };
        let mut roots = Vec::new();
        for r in us {
            st.push(
                format!("Now solve ${} = {}$", latex(&a), latex(&r)),
                eq(&a, &r),
            );
            match solve(a.clone(), r, v, st, depth + 1) {
                Outcome::Roots(rs) => roots.extend(rs),
                other => return Some(other),
            }
        }
        return Some(Outcome::Roots(roots));
    }
    None
}

fn atoms(s: &Sym, v: &str, out: &mut Vec<Sym>) {
    match s {
        Sym::Fn(_, a) => {
            if !out.contains(s) {
                out.push(s.clone());
            }
            atoms(a, v, out);
        }
        Sym::Pow(b, e) if e.has(v) && !b.has(v) => {
            // eˣ from e^(2x): the exponent's smallest whole-number part.
            let (c, rest) = split_coef(e);
            let base = if c.as_q().is_some_and(|q| q.is_int() && q.n > 1) {
                pow((**b).clone(), rest)
            } else {
                s.clone()
            };
            if !out.contains(&base) {
                out.push(base);
            }
        }
        Sym::Pow(b, e) => {
            atoms(b, v, out);
            atoms(e, v, out);
        }
        Sym::Add(xs) | Sym::Mul(xs) => xs.iter().for_each(|x| atoms(x, v, out)),
        _ => {}
    }
}

/// √(x + 3) = x - 3: get the root alone and square, then check answers.
fn radical(f: &Sym, v: &str, st: &mut Steps, depth: usize) -> Option<Outcome> {
    let ts = f.terms();
    let is_root = |t: &Sym| {
        t.factors()
            .iter()
            .any(|g| matches!(g, Sym::Pow(b, e) if b.has(v) && e.as_q().is_some_and(|q| q.d == 2)))
    };
    let (roots, rest): (Vec<Sym>, Vec<Sym>) = ts.into_iter().partition(is_root);
    if roots.len() != 1 {
        return None;
    }
    let lhs = roots[0].clone();
    let rhs = neg(add(rest));
    st.push("Get the square root on its own", eq(&lhs, &rhs));
    let l2 = expand(&powi(lhs, 2));
    let r2 = expand(&powi(rhs, 2));
    st.push(
        "Square both sides (squaring can add answers that don't work, so check them at the end)",
        eq(&l2, &r2),
    );
    Some(solve(l2, r2, v, st, depth + 1))
}

/// The last resort: look for sign changes and narrow each one down.
fn numeric(f: &Sym, v: &str, st: &mut Steps) -> Outcome {
    if f.names().iter().any(|n| n != v) {
        st.push(
            format!("Solve can't find ${}$ exactly here, and other letters stop it from finding numbers", name(v)),
            "",
        );
        return Outcome::Fail;
    }
    let g = |x: f64| f.at(v, x);
    let mut roots: Vec<f64> = Vec::new();
    let (lo, hi, steps) = (-100.0, 100.0, 40000);
    let h = (hi - lo) / steps as f64;
    let mut prev = g(lo);
    for i in 1..=steps {
        let x = lo + i as f64 * h;
        let y = g(x);
        if y == 0.0 {
            roots.push(x);
        } else if prev.is_finite() && y.is_finite() && prev.signum() != y.signum() && prev != 0.0 {
            let (mut a, mut b) = (x - h, x);
            for _ in 0..100 {
                let m = 0.5 * (a + b);
                if g(m).signum() == g(a).signum() {
                    a = m
                } else {
                    b = m
                }
            }
            let r = 0.5 * (a + b);
            // A jump across an asymptote is not a root.
            if g(r).abs() < 1e-6 {
                roots.push(r);
            }
        }
        prev = y;
        if roots.len() > 40 {
            break;
        }
    }
    roots.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
    if roots.is_empty() {
        st.push(
            "There's no exact method here, and no solutions turned up between -100 and 100",
            "",
        );
        return Outcome::Roots(Vec::new());
    }
    st.push(
        "There's no exact method for this one, so these solutions between -100 and 100 are found numerically",
        "",
    );
    Outcome::Roots(roots.into_iter().map(Sym::Real).collect())
}

// ---- Inequalities ---------------------------------------------------------------

pub fn flip(rel: Relation) -> Relation {
    match rel {
        Relation::Lt => Relation::Gt,
        Relation::Gt => Relation::Lt,
        Relation::Le => Relation::Ge,
        Relation::Ge => Relation::Le,
        Relation::Eq => Relation::Eq,
    }
}

pub fn rel_latex(rel: Relation) -> &'static str {
    match rel {
        Relation::Lt => "<",
        Relation::Gt => ">",
        Relation::Le => "\\le ",
        Relation::Ge => "\\ge ",
        Relation::Eq => "=",
    }
}

fn holds(rel: Relation, y: f64) -> bool {
    match rel {
        Relation::Lt => y < 0.0,
        Relation::Gt => y > 0.0,
        Relation::Le => y <= 0.0,
        Relation::Ge => y >= 0.0,
        Relation::Eq => y == 0.0,
    }
}

/// Solves `lhs rel rhs` with a sign chart. Returns the answer as LaTeX, and
/// the same as a row the graph can shade when it is one interval.
pub fn inequality(
    lhs: Sym,
    rel: Relation,
    rhs: Sym,
    v: &str,
    st: &mut Steps,
) -> Result<(String, Option<String>), String> {
    let xs = name(v);
    let f = sub(lhs, rhs.clone());
    if !rhs.is_zero() {
        st.push(
            "Move everything to one side",
            format!("{}{}0", latex(&f), rel_latex(rel)),
        );
    }
    if f.names().iter().any(|n| n != v) {
        return Err(format!(
            "Solve can only do inequalities where ${xs}$ is the only letter"
        ));
    }
    let (top, bottom) = num_den(&f);
    let mut points: Vec<(f64, Sym, bool)> = Vec::new();
    let mut quiet = Steps::quiet();
    for (part, is_zero) in [(top, true), (bottom, false)] {
        if !part.has(v) {
            continue;
        }
        match solve(expand(&part), n(0), v, &mut quiet, 0) {
            Outcome::Roots(rs) => {
                for r in rs {
                    if r.has(integer_name(v)) {
                        return Err(
                            "Solve can't do inequalities with infinitely many boundary points"
                                .into(),
                        );
                    }
                    let x = r.value();
                    if x.is_finite() && !points.iter().any(|p| (p.0 - x).abs() < 1e-12) {
                        points.push((x, r, is_zero));
                    }
                }
            }
            Outcome::All => return Err("Solve can't find the boundary points here".into()),
            Outcome::Fail => return Err("Solve can't find where the two sides are equal".into()),
        }
    }
    points.sort_by(|a, b| a.0.total_cmp(&b.0));
    if points.is_empty() {
        st.push(format!("The left side is never zero or undefined, so its sign is the same for every ${xs}$"), "");
    } else {
        let zeros: Vec<&(f64, Sym, bool)> = points.iter().filter(|p| p.2).collect();
        let poles: Vec<&(f64, Sym, bool)> = points.iter().filter(|p| !p.2).collect();
        let list = |ps: &[&(f64, Sym, bool)]| {
            ps.iter()
                .map(|p| latex(&p.1))
                .collect::<Vec<_>>()
                .join(",\\ ")
        };
        if !zeros.is_empty() {
            st.push(
                "Find where the left side is zero",
                format!("{xs}={}", list(&zeros)),
            );
        }
        if !poles.is_empty() {
            st.push(
                "and where it is undefined",
                format!("{xs}={}", list(&poles)),
            );
        }
    }
    // Test a point in each interval.
    let mut tests = Vec::new();
    let xs_pts: Vec<f64> = points.iter().map(|p| p.0).collect();
    if xs_pts.is_empty() {
        tests.push(0.0);
    } else {
        tests.push(xs_pts[0] - 1.0);
        for w in xs_pts.windows(2) {
            tests.push(0.5 * (w[0] + w[1]));
        }
        tests.push(xs_pts[xs_pts.len() - 1] + 1.0);
    }
    let good: Vec<bool> = tests.iter().map(|&t| holds(rel, f.at(v, t))).collect();
    let mut chart = Vec::new();
    for (t, ok) in tests.iter().zip(&good) {
        let y = f.at(v, *t);
        chart.push(format!(
            "{xs}={}:\\ {}{}0\\ {}",
            crate::format::value_latex(round_test(*t)),
            crate::format::value_latex(y),
            if y > 0.0 {
                ">"
            } else if y < 0.0 {
                "<"
            } else {
                "="
            },
            if *ok { "\\checkmark" } else { "\\times" }
        ));
    }
    st.push("Test one value in each interval", chart.join("\\qquad "));
    // Walk the line left to right: interval, point, interval, ..., merging
    // whatever satisfies it into runs.
    let strict = matches!(rel, Relation::Lt | Relation::Gt);
    let m = points.len();
    let mut items: Vec<(bool, bool, usize)> = Vec::new(); // (is point, in, index)
    for k in 0..=m {
        items.push((false, good[k], k));
        if k < m {
            items.push((true, !strict && points[k].2, k));
        }
    }
    let mut pieces: Vec<String> = Vec::new();
    let mut i = 0;
    while i < items.len() {
        if !items[i].1 {
            i += 1;
            continue;
        }
        let first = items[i];
        while i + 1 < items.len() && items[i + 1].1 {
            i += 1;
        }
        let last = items[i];
        i += 1;
        let lo = match first {
            (true, _, k) => Some((points[k].1.clone(), true)),
            (false, _, 0) => None,
            (false, _, k) => Some((points[k - 1].1.clone(), false)),
        };
        let hi = match last {
            (true, _, k) => Some((points[k].1.clone(), true)),
            (false, _, k) if k == m => None,
            (false, _, k) => Some((points[k].1.clone(), false)),
        };
        let below = |closed: bool| if closed { "\\le " } else { "<" };
        pieces.push(match (lo, hi) {
            (None, None) => format!("\\text{{every }}{xs}"),
            (None, Some((b, c))) => format!("{xs}{}{}", below(c), latex(&b)),
            (Some((a, c)), None) => format!("{xs}{}{}", if c { "\\ge " } else { ">" }, latex(&a)),
            (Some((a, _)), Some((b, _))) if first.0 && last.0 && first.2 == last.2 => {
                let _ = b;
                format!("{xs}={}", latex(&a))
            }
            (Some((a, c1)), Some((b, c2))) => {
                format!("{}{}{xs}{}{}", latex(&a), below(c1), below(c2), latex(&b))
            }
        });
    }
    if pieces.is_empty() {
        return Ok(("\\text{no solution}".into(), None));
    }
    let row = (pieces.len() == 1 && !pieces[0].contains("text")).then(|| pieces[0].clone());
    let result = pieces.join("\\quad\\text{or}\\quad ");
    st.push("So", result.clone());
    Ok((result, row))
}

fn round_test(t: f64) -> f64 {
    let r = (t * 100.0).round() / 100.0;
    if r == 0.0 {
        0.0
    } else {
        r
    }
}
