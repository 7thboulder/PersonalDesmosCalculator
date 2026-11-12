//! Partial differential equations in two variables, with constant
//! coefficients: first-order linear equations by characteristics, and the
//! heat, wave and Laplace equations by separating the variables. Boundary
//! and initial conditions come from other rows, as u(0,t)=0 and
//! u(x,0)=f(x), and turn the separated solutions into a Fourier series.
//! Every answer is put back into the equation before it is shown.

use super::convert::{to_sym, Defs};
use super::integrate::antiderivative;
use super::latex::{latex, name};
use super::sym::*;
use super::{nice, poly, Steps};
use crate::ast::{Expr, Relation, Statement};

type Done = Result<(String, Option<String>), String>;

/// Letters that are independent variables, never the unknown.
const INDEPENDENT: [&str; 8] = ["x", "y", "z", "t", "r", "theta", "s", "n"];

/// Prefix of the stand-in names for u and its derivatives.
const MARK: char = '\u{2}';

/// A linear PDE: Σ coefficient · ∂^key u = 0.
pub struct Pde {
    pub unknown: String,
    /// The two independent variables, alphabetically.
    vars: Vec<String>,
    /// (variables differentiated by, sorted; coefficient). An empty key is u.
    terms: Vec<(Vec<String>, Sym)>,
    defs: Defs,
}

/// A row such as u(0,t)=0, u(x,0)=f(x) or u_t(x,0)=g(x).
#[derive(Clone)]
struct Cond {
    /// The variable held fixed, and where.
    fixed: String,
    at: Sym,
    /// u_x(…) differentiates first.
    deriv: Option<String>,
    value: Sym,
    shown: String,
}

fn independent(v: &str) -> bool {
    INDEPENDENT.contains(&v)
}

fn mark(key: &[String]) -> String {
    format!("{MARK}{}", key.join(","))
}

/// u_xt as (u, [x, t]) when the letters after _ are variables.
fn subscripted(v: &str) -> Option<(&str, Vec<String>)> {
    let (base, sub) = v.split_once('_')?;
    if base.is_empty() || sub.is_empty() || independent(base) {
        return None;
    }
    let vars: Vec<String> = sub.chars().map(|c| c.to_string()).collect();
    vars.iter()
        .all(|c| independent(c) && c != "n")
        .then_some((base, vars))
}

/// The unknown and the variables of a chain ∂/∂x ∂/∂t … u.
fn chain(e: &Expr) -> Option<(String, Vec<String>)> {
    let mut vars = Vec::new();
    let mut e = e;
    while let Expr::Derivative(v, body) = e {
        vars.push(v.clone());
        e = body;
    }
    match e {
        Expr::Var(u) if !vars.is_empty() => Some((u.clone(), vars)),
        Expr::Apply(u, args)
            if !vars.is_empty() && args.iter().all(|a| matches!(a, Expr::Var(_))) =>
        {
            Some((u.clone(), vars))
        }
        _ => None,
    }
}

/// The PDE `source` is, when it is one: None when it is something else
/// (an ordinary differential equation, or a derivative of a defined
/// function), and Err when it is a PDE Solve can't read.
pub fn read(source: &str, others: &[String]) -> Option<Result<Pde, String>> {
    let Ok(Statement::Relation(l, Relation::Eq, r)) = crate::parse(source) else {
        return None;
    };
    let all = Defs::from_rows(others);
    let mut found: Vec<(String, Vec<String>)> = Vec::new();
    let mut look = |e: &Expr| match e {
        Expr::Derivative(..) => found.extend(chain(e)),
        Expr::Var(v) => found.extend(subscripted(v).map(|(b, vs)| (b.to_owned(), vs))),
        _ => {}
    };
    l.walk(&mut look);
    r.walk(&mut look);
    found.retain(|(u, _)| !independent(u) && !all.is_var(u));
    let unknown = found.first()?.0.clone();
    let mut vars: Vec<String> = found.iter().flat_map(|(_, v)| v.clone()).collect();
    vars.sort();
    vars.dedup();
    if vars.len() < 2 {
        return None;
    }
    // A row u(x,t) = … defines u, so this differentiates it.
    if let Some((params, _)) = all.function(&unknown) {
        if params.iter().all(|p| vars.contains(p)) {
            return None;
        }
    }
    Some(build(&l, &r, unknown, vars, &found, others))
}

fn build(
    l: &Expr,
    r: &Expr,
    unknown: String,
    vars: Vec<String>,
    found: &[(String, Vec<String>)],
    others: &[String],
) -> Result<Pde, String> {
    if found.iter().any(|(u, _)| *u != unknown) {
        return Err("Solve can only solve a PDE in one unknown function".into());
    }
    if vars.len() > 2 {
        return Err("Solve can only solve PDEs in two variables".into());
    }
    // Rows that set conditions on u mustn't define it as a function.
    let rows: Vec<String> = others
        .iter()
        .filter(|row| !matches!(crate::parse(row), Ok(Statement::Relation(Expr::Apply(f, _), _, _)) if *f == unknown || f.strip_prefix(unknown.as_str()).is_some_and(|s| s.starts_with('_'))))
        .cloned()
        .collect();
    let defs = Defs::from_rows(&rows);
    let mut keys: Vec<Vec<String>> = vec![Vec::new()];
    let mut swap = |e: Expr| {
        let key = match &e {
            Expr::Derivative(..) => chain(&e).filter(|(u, _)| *u == unknown).map(|(_, v)| v),
            Expr::Var(v) if *v == unknown => Some(Vec::new()),
            Expr::Apply(f, args)
                if *f == unknown && args.iter().all(|a| matches!(a, Expr::Var(_))) =>
            {
                Some(Vec::new())
            }
            Expr::Var(v) => subscripted(v)
                .filter(|(b, _)| *b == unknown)
                .map(|(_, v)| v),
            _ => None,
        };
        match key {
            Some(mut k) => {
                k.sort();
                let m = mark(&k);
                if !keys.contains(&k) {
                    keys.push(k);
                }
                Ok(Expr::Var(m))
            }
            None => Err(e),
        }
    };
    let l = crate::units::rewrite(l.clone(), &mut swap);
    let r = crate::units::rewrite(r.clone(), &mut swap);
    let mut rest = expand(&sub(to_sym(&l, &defs)?, to_sym(&r, &defs)?));
    let nonlinear = || {
        "Solve can only solve linear PDEs: u and its derivatives can't be multiplied together or put inside functions".to_owned()
    };
    let mut terms = Vec::new();
    for k in keys {
        let c = poly::coeffs(&rest, &mark(&k)).ok_or_else(nonlinear)?;
        if c.len() > 2 {
            return Err(nonlinear());
        }
        if c.len() == 2 {
            let coef = nice(&c[1]);
            if coef.names().iter().any(|n| n.starts_with(MARK)) {
                return Err(nonlinear());
            }
            if vars.iter().any(|v| coef.has(v)) {
                return Err(
                    "Solve can only solve PDEs whose coefficients are constants, not functions of the variables".into(),
                );
            }
            if !coef.is_zero() {
                terms.push((k, coef));
            }
        }
        rest = c[0].clone();
    }
    if rest.names().iter().any(|n| n.starts_with(MARK)) {
        return Err(nonlinear());
    }
    if !nice(&rest).is_zero() {
        return Err(format!(
            "Solve can only solve PDEs where every term has {} or a derivative of it in it (homogeneous ones)",
            name(&unknown)
        ));
    }
    Ok(Pde {
        unknown,
        vars,
        terms,
        defs,
    })
}

impl Pde {
    fn coef(&self, key: &[&str]) -> Sym {
        let mut k: Vec<String> = key.iter().map(|s| s.to_string()).collect();
        k.sort();
        self.terms
            .iter()
            .find(|(t, _)| *t == k)
            .map_or(n(0), |(_, c)| c.clone())
    }

    fn only(&self, keys: &[&[&str]]) -> bool {
        self.terms.iter().all(|(t, _)| {
            keys.iter().any(|k| {
                let mut k: Vec<String> = k.iter().map(|s| s.to_string()).collect();
                k.sort();
                *t == k
            })
        })
    }

    fn has(&self, key: &[&str]) -> bool {
        !self.coef(key).is_zero()
    }

    /// Whether `sol` makes the equation hold, at a few points.
    fn holds(&self, sol: &Sym) -> bool {
        (0..3).all(|trial| {
            let env = |v: &str| Some(sample(v, trial));
            let mut total = 0.0;
            let mut scale = 0.0;
            for (key, c) in &self.terms {
                let mut d = sol.clone();
                for v in key {
                    d = super::diff::derivative(&d, v);
                }
                let term = c.eval(&env) * d.eval(&env);
                total += term;
                scale += term.abs();
            }
            total.is_finite() && total.abs() <= 1e-7 * (1.0 + scale)
        })
    }

    /// The conditions other rows set on the unknown.
    fn conditions(&self, others: &[String]) -> Vec<Cond> {
        let mut out = Vec::new();
        for row in others {
            let Ok(Statement::Relation(Expr::Apply(f, args), Relation::Eq, rhs)) =
                crate::parse(row)
            else {
                continue;
            };
            let deriv = if f == self.unknown {
                None
            } else {
                match f
                    .strip_prefix(self.unknown.as_str())
                    .and_then(|s| s.strip_prefix('_'))
                {
                    Some(v) if self.vars.iter().any(|w| w == v) => Some(v.to_owned()),
                    _ => continue,
                }
            };
            if args.len() != 2 {
                continue;
            }
            let is_var = |a: &Expr| matches!(a, Expr::Var(v) if self.vars.contains(v));
            let (free, at) = match (is_var(&args[0]), is_var(&args[1])) {
                (true, false) => (&args[0], &args[1]),
                (false, true) => (&args[1], &args[0]),
                _ => continue,
            };
            let Expr::Var(free) = free else { continue };
            let Some(fixed) = self.vars.iter().find(|v| *v != free).cloned() else {
                continue;
            };
            let (Ok(at), Ok(value)) = (to_sym(at, &self.defs), to_sym(&rhs, &self.defs)) else {
                continue;
            };
            if self.vars.iter().any(|v| at.has(v)) || value.has(&fixed) {
                continue;
            }
            out.push(Cond {
                fixed,
                at,
                deriv,
                value,
                shown: row.clone(),
            });
        }
        out
    }

    fn u(&self, first: &str, second: &str) -> String {
        format!("{}({},{})", name(&self.unknown), name(first), name(second))
    }
}

/// A made-up value for a letter, to check an answer numerically.
fn sample(v: &str, trial: u32) -> f64 {
    let h = v
        .bytes()
        .fold(7u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32));
    0.45 + ((h.wrapping_add(trial * 7919)) % 89) as f64 / 89.0 * 0.9
}

fn at0(v: &str) -> Option<f64> {
    Some(sample(v, 0))
}

/// `k` written in front of something: nothing for 1, `-` for -1.
fn times(k: &Sym) -> String {
    if k.is_one() {
        String::new()
    } else if neg(k.clone()).is_one() {
        "-".into()
    } else if matches!(k, Sym::Add(_)) {
        format!("\\left({}\\right)", latex(k))
    } else {
        latex(k)
    }
}

fn paren(s: &Sym) -> String {
    if matches!(s, Sym::Add(_)) {
        format!("\\left({}\\right)", latex(s))
    } else {
        latex(s)
    }
}

/// A capital letter for the part of the solution in `v`.
fn part(v: &str, avoid: &str) -> String {
    let c = match v {
        _ if v.len() == 1 => v.to_uppercase(),
        "theta" => "\\Theta".into(),
        _ => "X".into(),
    };
    if c == avoid {
        format!("{c}_1")
    } else {
        c
    }
}

fn positive(s: &Sym) -> bool {
    let v = s.eval(&at0);
    v.is_nan() || v > 0.0
}

/// Solves the PDE. `general` ignores the conditions in other rows.
pub fn solve(p: &Pde, others: &[String], general: bool, st: &mut Steps) -> Done {
    let conds = if general {
        Vec::new()
    } else {
        p.conditions(others)
    };
    let (a, b) = (p.vars[0].clone(), p.vars[1].clone());
    let order = p.terms.iter().map(|(k, _)| k.len()).max().unwrap_or(0);
    if order == 0 {
        return Err("There's no derivative here to solve for".into());
    }
    if order == 1 {
        return first_order(p, &conds, st);
    }
    // One variable to the first order and the other to the second: heat.
    for (t, x) in [(&a, &b), (&b, &a)] {
        if p.only(&[&[t], &[x, x], &[]]) && p.has(&[t]) && p.has(&[x, x]) {
            return heat(p, t, x, &conds, st);
        }
    }
    if p.only(&[&[&a, &a], &[&b, &b]]) && p.has(&[&a, &a]) && p.has(&[&b, &b]) {
        // u_tt = c² u_xx, or Laplace's equation when both have one sign.
        let (t, x) = if a == "t" || (b != "t" && a == "y" && b != "x") {
            (&a, &b)
        } else {
            (&b, &a)
        };
        let c2 = nice(&neg(div(p.coef(&[x, x]), p.coef(&[t, t]))));
        return if positive(&c2) {
            wave(p, t, x, c2, &conds, st)
        } else {
            laplace(p, x, t, st)
        };
    }
    Err("Solve can solve first-order linear PDEs, the heat equation, the wave equation and Laplace's equation (with constant coefficients); this PDE is beyond it".into())
}

// ---- First order ---------------------------------------------------------------

fn first_order(p: &Pde, conds: &[Cond], st: &mut Steps) -> Done {
    let (v0, v1) = (p.vars[0].clone(), p.vars[1].clone());
    let cond = conds.iter().find(|c| c.deriv.is_none());
    // a is the variable the characteristics are followed along: t when
    // there is one, else the one a condition holds fixed.
    let mut a = if v0 == "t" || v1 == "t" {
        "t".to_owned()
    } else {
        v1.clone()
    };
    if let Some(c) = cond {
        a = c.fixed.clone();
    }
    if !p.has(&[&a]) {
        a = if a == v0 { v1.clone() } else { v0.clone() };
        if cond.is_some_and(|c| c.fixed != a) {
            return Err(format!(
                "A condition on ${}$ at fixed ${}$ can't be met here, as nothing changes along ${}$",
                name(&p.unknown),
                name(&cond.unwrap().fixed),
                name(&cond.unwrap().fixed)
            ));
        }
    }
    let b = if a == v0 { v1.clone() } else { v0.clone() };
    let ca = p.coef(&[&a]);
    let s = nice(&div(p.coef(&[&b]), ca.clone()));
    let q = nice(&div(p.coef(&[]), ca.clone()));
    let (va, vb) = (var(&a), var(&b));
    let (la, lb) = (name(&a), name(&b));
    let u = name(&p.unknown);
    let shown = if a == "t" || b != "t" {
        p.u(&b, &a)
    } else {
        p.u(&a, &b)
    };
    let xi = nice(&sub(vb.clone(), mul2(s.clone(), va.clone())));
    st.push(
        if ca.is_one() {
            "This is a first-order linear PDE".to_owned()
        } else {
            format!(
                "This is a first-order linear PDE. Divide by ${}$",
                latex(&ca)
            )
        },
        format!(
            "{u}_{{{la}}}{}{u}_{{{lb}}}{}=0",
            signed_times(&s),
            if q.is_zero() {
                String::new()
            } else {
                format!("{}{u}", signed_times(&q))
            }
        ),
    );
    st.push(
        format!(
            "Along the lines where ${}$ stays constant (the characteristics, with $\\frac{{d{lb}}}{{d{la}}}={}$), the chain rule turns the left side into $\\frac{{d{u}}}{{d{la}}}$",
            lead(&xi, &b),
            latex(&s)
        ),
        if q.is_zero() {
            format!("\\frac{{d{u}}}{{d{la}}}=0")
        } else {
            format!("\\frac{{d{u}}}{{d{la}}}={}{u}", times(&neg(q.clone())))
        },
    );
    let growth = exp(neg(mul2(q.clone(), va.clone())));
    if let Some(c) = cond {
        // u = f along a = a₀ fixes F.
        let a0 = c.at.clone();
        let back = sub(va.clone(), a0.clone());
        let along = c
            .value
            .subs_var(&b, &nice(&sub(vb.clone(), mul2(s.clone(), back.clone()))));
        let sol = if q.is_zero() {
            along
        } else {
            mul2(exp(nice(&neg(mul2(q.clone(), back.clone())))), along)
        };
        if !p.holds(&sol) || !meets(&sol, &a, &a0, &c.value) {
            return Err("Couldn't check the solution, so Solve won't show it".into());
        }
        st.push(
            format!(
                "Each line starts where ${la}={}$, and there ${u}$ is given by the condition",
                latex(&a0)
            ),
            c.shown.clone(),
        );
        let result = format!("{shown}={}", tidy(&latex(&sol), &[(&xi, &b)]));
        st.push("Follow each line from there", result.clone());
        return Ok((result.clone(), Some(result)));
    }
    let test = mul2(
        growth.clone(),
        add2(func(Func::Sin, xi.clone()), div(powi(xi.clone(), 2), n(3))),
    );
    if !p.holds(&test) {
        return Err("Couldn't check the solution, so Solve won't show it".into());
    }
    let f = format!("F\\left({}\\right)", lead(&xi, &b));
    let result = if q.is_zero() {
        format!("{shown}={f}")
    } else {
        format!("{shown}={}{f}", paren(&nice(&growth)))
    };
    st.push(
        format!("Solve that along each line: the constant can be different on each one, so it is any function $F$ of ${}$", lead(&xi, &b)),
        result.clone(),
    );
    st.push(
        format!(
            "Add a row such as ${}({},0)=f({})$ to pick out one solution",
            u, lb, lb
        ),
        "",
    );
    Ok((result, None))
}

/// Multiplies numbers into sums at the top, so like terms combine,
/// leaving what is inside functions as it is.
fn spread(s: &Sym) -> Sym {
    let mut out = Vec::new();
    for t in s.terms() {
        let (c, rest) = split_coef(&t);
        match rest {
            Sym::Add(xs) => out.extend(xs.iter().map(|x| mul2(c.clone(), x.clone()))),
            _ => out.push(t),
        }
    }
    add(out)
}

/// x - ct written with x first.
fn lead(s: &Sym, v: &str) -> String {
    let terms = s.terms();
    let Some(i) = terms.iter().position(|t| *t == var(v)) else {
        return latex(s);
    };
    let mut rest = terms.clone();
    rest.remove(i);
    let r = latex(&add(rest));
    if r.starts_with('-') {
        format!("{}{r}", name(v))
    } else {
        format!("{}+{r}", name(v))
    }
}

/// Writes each of `parts` with its variable first wherever it shows.
fn tidy(text: &str, parts: &[(&Sym, &str)]) -> String {
    let mut out = text.to_owned();
    for (s, v) in parts {
        if s.terms().len() > 1 {
            out = out.replace(&latex(s), &lead(s, v));
        }
    }
    out
}

/// `+ k` or `- k` before a term, with 1 left out.
fn signed_times(k: &Sym) -> String {
    let t = times(k);
    if t.starts_with('-') {
        t
    } else {
        format!("+{t}")
    }
}

/// Whether sol at v = v0 equals `want` at a few points.
fn meets(sol: &Sym, v: &str, v0: &Sym, want: &Sym) -> bool {
    let s = sol.subs_var(v, v0);
    (0..3).all(|trial| {
        let env = |w: &str| Some(sample(w, trial));
        let (x, y) = (s.eval(&env), want.eval(&env));
        x.is_finite() && (x - y).abs() <= 1e-7 * (1.0 + y.abs())
    })
}

// ---- Fourier coefficients ----------------------------------------------------

/// A Fourier coefficient c_n, n = 1, 2, …
enum Coef {
    /// Only these n have one.
    Finite(Vec<(i128, Sym)>),
    /// A formula in n.
    Formula(Sym),
    /// Left as its integral, written out.
    Integral(String),
}

/// The interval [a, a+L] and whether the modes are cosines.
struct Basis {
    x: String,
    a: Sym,
    l: Sym,
    cosine: bool,
}

impl Basis {
    fn phase(&self, k: Sym) -> Sym {
        let shifted = if self.a.is_zero() {
            var(&self.x)
        } else {
            sub(var(&self.x), self.a.clone())
        };
        div(mul(vec![k, pi(), shifted]), self.l.clone())
    }

    fn mode(&self, k: Sym) -> Sym {
        let f = if self.cosine { Func::Cos } else { Func::Sin };
        func(f, self.phase(k))
    }

    /// nπ/L.
    fn rate(&self, k: Sym) -> Sym {
        div(mul2(k, pi()), self.l.clone())
    }

    fn end(&self) -> Sym {
        nice(&add2(self.a.clone(), self.l.clone()))
    }

    /// (2/L)∫ f·mode_k over the interval, by Gauss–Legendre.
    fn numeric(&self, f: &Sym, k: f64) -> f64 {
        let (a, l) = (self.a.eval(&at0), self.l.eval(&at0));
        let g = mul2(f.clone(), self.mode(var("\u{2}k")));
        let env = |v: &str, xv: f64| {
            if v == self.x {
                Some(xv)
            } else if v == "\u{2}k" {
                Some(k)
            } else {
                at0(v)
            }
        };
        const X: [f64; 5] = [
            -0.906_179_845_938_664,
            -0.538_469_310_105_683,
            0.0,
            0.538_469_310_105_683,
            0.906_179_845_938_664,
        ];
        const W: [f64; 5] = [
            0.236_926_885_056_189,
            0.478_628_670_499_366,
            0.568_888_888_888_889,
            0.478_628_670_499_366,
            0.236_926_885_056_189,
        ];
        let panels = 400;
        let h = l / panels as f64;
        let mut total = 0.0;
        for p in 0..panels {
            let mid = a + h * (p as f64 + 0.5);
            for (x, w) in X.iter().zip(W) {
                let xv = mid + x * h / 2.0;
                total += w * g.eval(&|v| env(v, xv));
            }
        }
        total * h / 2.0 * 2.0 / l
    }

    /// (2/L)∫ f·mode_k exactly, for a whole number or n.
    fn exact(&self, f: &Sym, k: Sym) -> Option<Sym> {
        let g = expand(&mul2(f.clone(), self.mode(k)));
        let big = antiderivative(&g, &self.x, &mut Steps::quiet())?;
        let whole = sub(
            big.subs_var(&self.x, &self.end()),
            big.subs_var(&self.x, &self.a),
        );
        let c = mul2(div(n(2), self.l.clone()), reduce(&expand(&whole)));
        Some(nice(&reduce(&nice(&c))))
    }

    fn integral(&self, f: &Sym) -> String {
        format!(
            "{}\\int_{{{}}}^{{{}}}{}\\,d{}",
            times(&nice(&div(n(2), self.l.clone()))),
            latex(&self.a),
            latex(&self.end()),
            format!(
                "{}{}",
                if f.is_negative() {
                    format!("\\left({}\\right)", latex(f))
                } else {
                    paren(f)
                },
                show(&self.mode(var("n")))
            ),
            name(&self.x)
        )
    }

    fn coefficients(&self, f: &Sym) -> Coef {
        if f.is_zero() {
            return Coef::Finite(Vec::new());
        }
        let nums: Vec<f64> = (1..=40).map(|k| self.numeric(f, k as f64)).collect();
        if nums.iter().any(|v| !v.is_finite()) {
            return Coef::Integral(self.integral(f));
        }
        let big = nums.iter().fold(0.0f64, |m, v| m.max(v.abs()));
        let tiny = 1e-9 * (1.0 + big);
        if nums[16..].iter().all(|v| v.abs() < tiny) {
            let mut list = Vec::new();
            for (i, v) in nums[..16].iter().enumerate() {
                if v.abs() < tiny {
                    continue;
                }
                let k = i as i128 + 1;
                let c = self
                    .exact(f, n(k))
                    .filter(|c| close(c.eval(&at0), *v))
                    .or_else(|| {
                        super::recognize(*v).filter(|_| f.names().iter().all(|w| w == &self.x))
                    })
                    .unwrap_or(Sym::Real(*v));
                list.push((k, c));
            }
            return Coef::Finite(list);
        }
        if let Some(c) = self.exact(f, var("n")) {
            let ok = (1..=12).all(|k| {
                close(
                    c.eval(&|v| if v == "n" { Some(k as f64) } else { at0(v) }),
                    nums[k - 1],
                )
            });
            if ok {
                return Coef::Formula(c);
            }
        }
        Coef::Integral(self.integral(f))
    }

    /// (1/L)∫ f, the constant term of a cosine series.
    fn mean(&self, f: &Sym) -> Sym {
        if let Some(big) = antiderivative(f, &self.x, &mut Steps::quiet()) {
            let m = nice(&div(
                sub(
                    big.subs_var(&self.x, &self.end()),
                    big.subs_var(&self.x, &self.a),
                ),
                self.l.clone(),
            ));
            if close(m.eval(&at0), self.numeric_mean(f)) {
                return m;
            }
        }
        Sym::Real(self.numeric_mean(f))
    }

    fn numeric_mean(&self, f: &Sym) -> f64 {
        // cos(0·…) = 1, and (2/L)∫ f/2 = (1/L)∫ f.
        let half = Basis {
            x: self.x.clone(),
            a: self.a.clone(),
            l: self.l.clone(),
            cosine: true,
        };
        half.numeric(&div(f.clone(), n(2)), 0.0)
    }
}

fn close(a: f64, b: f64) -> bool {
    a.is_finite() && (a - b).abs() <= 1e-7 * (1.0 + b.abs())
}

/// sin(knπ + c) = (-1)^{kn} sin c and the same for cos, n a whole number.
fn reduce(s: &Sym) -> Sym {
    match s {
        Sym::Add(xs) => add(xs.iter().map(reduce).collect()),
        Sym::Mul(xs) => mul(xs.iter().map(reduce).collect()),
        Sym::Pow(a, b) => pow(reduce(a), reduce(b)),
        Sym::Fn(f @ (Func::Sin | Func::Cos), a) => {
            let a = reduce(a);
            let mut k = 0i128;
            let mut rest = Vec::new();
            for t in expand(&a).terms() {
                match expand(&div(t.clone(), mul2(var("n"), pi()))).as_q() {
                    Some(q) if q.is_int() => k += q.n,
                    _ => rest.push(t),
                }
            }
            if k == 0 {
                return func(*f, a);
            }
            let sign = if k % 2 == 0 {
                n(1)
            } else {
                pow(n(-1), var("n"))
            };
            mul2(sign, func(*f, add(rest)))
        }
        Sym::Fn(f, a) => func(*f, reduce(a)),
        _ => s.clone(),
    }
}

/// A Fourier series being put together: terms written out, the term
/// summed over n = 1, 2, …, and coefficients left as integrals. Each term
/// keeps how it is shown: coefficient, then e^{…}, then the modes.
#[derive(Default)]
struct Series {
    explicit: Vec<(Sym, String)>,
    summed: Vec<(Sym, String)>,
    wheres: Vec<String>,
}

/// One factor of a term as it is shown: e^{-…} stays a power of e.
fn show(s: &Sym) -> String {
    match s {
        Sym::Pow(b, e) if matches!(&**b, Sym::Var(v) if v == "e") => {
            format!("e^{{{}}}", latex(&nice(e)))
        }
        Sym::Fn(f, a) => format!(
            "\\{}\\left({}\\right)",
            super::latex::func_name(*f).trim_start_matches('\\'),
            mode_arg(a)
        ),
        _ => paren(s),
    }
}

/// nπx/L written in that order.
fn mode_arg(a: &Sym) -> String {
    let (top, bottom) = num_den(a);
    let mut front = Vec::new();
    let mut back = Vec::new();
    for f in top.factors() {
        match &f {
            Sym::Num(_) => front.insert(0, latex(&f)),
            Sym::Var(v) if v == "n" => front.push(latex(&f)),
            Sym::Var(v) if v == "pi" => {}
            _ => back.push(paren(&f)),
        }
    }
    if !top
        .factors()
        .iter()
        .any(|f| matches!(f, Sym::Var(v) if v == "pi"))
    {
        return latex(a);
    }
    let lead = front.join("");
    let lead = if lead == "1" { String::new() } else { lead };
    let upper = format!("{lead}\\pi {}", back.join(""));
    if bottom.is_one() {
        upper
    } else {
        format!("\\frac{{{upper}}}{{{}}}", latex(&bottom))
    }
}

fn term(c: Sym, parts: Vec<Sym>) -> (Sym, String) {
    let c = nice(&c);
    let mut shown = times(&c);
    for p in &parts {
        if !p.is_one() {
            shown.push_str(&show(p));
        }
    }
    if shown.is_empty() || shown == "-" {
        shown.push('1');
    }
    (mul2(c, mul(parts)), shown)
}

impl Series {
    /// Adds Σ c_n·shape(n) for one set of coefficients called `letter`.
    fn add(
        &mut self,
        c: &Coef,
        letter: &str,
        shape: &dyn Fn(Sym) -> Vec<Sym>,
        scale: &dyn Fn(Sym) -> Sym,
    ) {
        match c {
            Coef::Finite(list) => {
                for (k, v) in list {
                    self.explicit
                        .push(term(mul2(v.clone(), scale(n(*k))), shape(n(*k))));
                }
            }
            Coef::Formula(v) => self
                .summed
                .push(term(mul2(v.clone(), scale(var("n"))), shape(var("n")))),
            Coef::Integral(i) => {
                let cn = format!("{letter}_n");
                let s = scale(var("n"));
                self.summed.push(term(var(&cn), shape(var("n"))));
                if s.is_one() {
                    self.wheres.push(format!("{letter}_{{n}}={i}"));
                } else {
                    self.wheres
                        .push(format!("{letter}_{{n}}={}{i}", times(&nice(&s))));
                }
            }
        }
    }

    fn push(&mut self, s: Sym) {
        let shown = latex(&s);
        self.explicit.push((s, shown));
    }

    fn latex(&self) -> String {
        let join = |list: &[(Sym, String)]| {
            list.iter()
                .map(|(_, s)| s.clone())
                .collect::<Vec<_>>()
                .join("+")
                .replace("+-", "-")
        };
        let mut parts = Vec::new();
        if !self.explicit.is_empty() || self.summed.is_empty() {
            parts.push(if self.explicit.is_empty() {
                "0".to_owned()
            } else {
                join(&self.explicit)
            });
        }
        if !self.summed.is_empty() {
            let inner = join(&self.summed);
            parts.push(if self.summed.len() > 1 {
                format!("\\sum_{{n=1}}^{{\\infty}}\\left({inner}\\right)")
            } else {
                format!("\\sum_{{n=1}}^{{\\infty}}{inner}")
            });
        }
        let mut s = parts.join("+").replace("+-", "-");
        for w in &self.wheres {
            s.push_str(&format!(",\\quad {w}"));
        }
        s
    }

    fn explicit_sum(&self) -> Sym {
        add(self.explicit.iter().map(|(s, _)| s.clone()).collect())
    }

    /// One whole solution to check, with n = 3 in any sum.
    fn sample(&self) -> Sym {
        let summed = add(self.summed.iter().map(|(s, _)| s.clone()).collect());
        add2(self.explicit_sum(), summed.subs_var("n", &n(3)))
    }
}

// ---- Heat ---------------------------------------------------------------------

/// The ends of the interval the boundary rows give, and what they hold.
struct Ends {
    basis: Basis,
    /// The values held at each end (Dirichlet), or None when insulated.
    held: Option<(Sym, Sym)>,
    shown: Vec<String>,
}

fn ends(p: &Pde, x: &str, conds: &[Cond]) -> Result<Option<Ends>, String> {
    let bcs: Vec<&Cond> = conds.iter().filter(|c| c.fixed == x).collect();
    if bcs.is_empty() {
        return Ok(None);
    }
    let u = name(&p.unknown);
    if bcs.len() != 2 {
        return Err(format!(
            "Give a condition at each end, as ${u}(0,t)=0$ and ${u}(L,t)=0$"
        ));
    }
    let (mut c0, mut c1) = (bcs[0], bcs[1]);
    let (v0, v1) = (c0.at.eval(&|_| None), c1.at.eval(&|_| None));
    if v0.is_finite() && v1.is_finite() {
        if v0 > v1 {
            std::mem::swap(&mut c0, &mut c1);
        }
    } else if c1.at.is_zero() {
        std::mem::swap(&mut c0, &mut c1);
    } else if !c0.at.is_zero() {
        return Err(format!("Put one end at a number, as in ${u}(0,t)=0$"));
    }
    let l = nice(&sub(c1.at.clone(), c0.at.clone()));
    if l.is_zero() {
        return Err("The two ends are at the same place".into());
    }
    let insulated = match (&c0.deriv, &c1.deriv) {
        (None, None) => false,
        (Some(a), Some(b)) if a == x && b == x => true,
        _ => {
            return Err(format!(
                "Solve can solve with both ends held ($u=…$) or both insulated ($u_{{{}}}=0$), not one of each yet",
                name(x)
            ))
        }
    };
    let held = if insulated {
        if !c0.value.is_zero() || !c1.value.is_zero() {
            return Err(format!(
                "Solve can only solve with insulated ends where $u_{{{}}}=0$",
                name(x)
            ));
        }
        None
    } else {
        if p.vars.iter().any(|v| c0.value.has(v) || c1.value.has(v)) {
            return Err("Solve can only hold the ends at constant values".into());
        }
        Some((c0.value.clone(), c1.value.clone()))
    };
    Ok(Some(Ends {
        basis: Basis {
            x: x.to_owned(),
            a: c0.at.clone(),
            l,
            cosine: insulated,
        },
        held,
        shown: vec![c0.shown.clone(), c1.shown.clone()],
    }))
}

/// The initial rows: u(x,t₀) and, for the wave equation, u_t(x,t₀).
fn initial<'a>(conds: &'a [Cond], t: &str) -> Result<(Option<&'a Cond>, Option<&'a Cond>), String> {
    let mut start = None;
    let mut speed = None;
    for c in conds.iter().filter(|c| c.fixed == t) {
        if !c.at.is_zero() {
            return Err(format!("Put the initial condition at ${}=0$", name(t)));
        }
        match &c.deriv {
            None => start = Some(c),
            Some(v) if v == t => speed = Some(c),
            _ => {}
        }
    }
    Ok((start, speed))
}

fn heat(p: &Pde, t: &str, x: &str, conds: &[Cond], st: &mut Steps) -> Done {
    let ct = p.coef(&[t]);
    let k = nice(&neg(div(p.coef(&[x, x]), ct.clone())));
    let r = nice(&neg(div(p.coef(&[]), ct)));
    let u = name(&p.unknown);
    let (lx, lt) = (name(x), name(t));
    let shown = p.u(x, t);
    let (big_x, big_t) = (part(x, &p.unknown), part(t, &p.unknown));
    let growth = if r.is_zero() {
        String::new()
    } else {
        format!("{}{u}", signed_times(&r))
    };
    st.push(
        if r.is_zero() {
            format!(
                "This is the heat equation, with diffusion constant ${}$",
                latex(&k)
            )
        } else {
            format!(
                "This is the heat equation with diffusion constant ${}$ and a growth term",
                latex(&k)
            )
        },
        format!("{u}_{{{lt}}}={}{u}_{{{lx}{lx}}}{growth}", times(&k)),
    );
    st.push(
        format!("Separate the variables: look for solutions ${u}={big_x}({lx}){big_t}({lt})$"),
        format!(
            "{big_x}({lx}){big_t}'({lt})={}{big_x}''({lx}){big_t}({lt}){}",
            times(&k),
            if r.is_zero() {
                String::new()
            } else {
                format!("{}{big_x}({lx}){big_t}({lt})", signed_times(&r))
            }
        ),
    );
    let top = if r.is_zero() {
        format!("{big_t}'")
    } else {
        format!("{big_t}'{}{big_t}", signed_times(&neg(r.clone())))
    };
    let kt = if k.is_one() {
        big_t.clone()
    } else {
        format!("{}{big_t}", paren(&k))
    };
    st.push(
        format!("Divide by ${}{big_x}{big_t}$. The left side depends only on ${lt}$ and the right only on ${lx}$, so both equal one constant, called $-\\lambda$", times(&k)),
        format!("\\frac{{{top}}}{{{kt}}}=\\frac{{{big_x}''}}{{{big_x}}}=-\\lambda"),
    );
    let (start, _) = initial(conds, t)?;
    let ends = ends(p, x, conds)?;
    let decay = |rate2: Sym| exp(mul2(nice(&sub(r.clone(), mul2(k.clone(), rate2))), var(t)));

    let Some(ends) = ends else {
        if let Some(f) = start {
            // An infinite rod: the heat kernel spreads f out.
            let s = if f.value.has("s") { "xi" } else { "s" };
            let fs = f.value.subs_var(x, &var(s));
            let ls = name(s);
            let four_k = nice(&mul2(n(4), k.clone()));
            let front = if r.is_zero() {
                String::new()
            } else {
                latex(&exp(mul2(r.clone(), var(t))))
            };
            let top = if front.is_empty() {
                "1".to_owned()
            } else {
                front
            };
            let result = format!(
                "{shown}=\\frac{{{top}}}{{\\sqrt{{{}\\pi {lt}}}}}\\int_{{-\\infty}}^{{\\infty}}{}e^{{-\\frac{{\\left({lx}-{ls}\\right)^{{2}}}}{{{}{lt}}}}}\\,d{ls}",
                latex(&four_k),
                paren(&fs),
                latex(&four_k)
            );
            st.push(
                "With no rows for the ends, take the rod to be infinitely long. Then the initial temperature spreads out by the heat kernel",
                f.shown.clone(),
            );
            st.push("So", result.clone());
            return Ok((result, None));
        }
        let lam = var("lambda");
        let root = sqrt(lam.clone());
        let tpart = decay(lam.clone());
        let xpart = add2(
            mul2(var("A"), func(Func::Cos, mul2(root.clone(), var(x)))),
            mul2(var("B"), func(Func::Sin, mul2(root, var(x)))),
        );
        let sol = mul2(tpart.clone(), xpart.clone());
        if !p.holds(&sol) {
            return Err("Couldn't check the solution, so Solve won't show it".into());
        }
        st.push(
            "Solve the two ordinary differential equations, for $\\lambda>0$",
            format!(
                "{big_t}({lt})={},\\quad {big_x}({lx})={}",
                latex(&tpart),
                latex(&xpart)
            ),
        );
        let result = format!("{shown}={}{}", paren(&tpart), paren(&xpart));
        st.push(
            "So the separated solutions are, for any constants $A$, $B$ and $\\lambda>0$",
            result.clone(),
        );
        st.push(
            format!(
                "$\\lambda=0$ adds $A+B{lx}$, and any sum of these solutions is one too. For the one solution that meets your conditions, add rows for both ends and the start, as ${u}(0,{lt})=0$, ${u}(L,{lt})=0$ and ${u}({lx},0)=f({lx})$"
            ),
            "",
        );
        return Ok((result, None));
    };

    let Some(start) = start else {
        return Err(format!(
            "Add the initial temperature as a row, as ${u}({lx},0)=f({lx})$"
        ));
    };
    if ends
        .held
        .as_ref()
        .is_some_and(|(a, b)| !(a.is_zero() && b.is_zero()))
        && !r.is_zero()
    {
        return Err("Solve can only hold the ends at 0 when there is a growth term".into());
    }
    let basis = &ends.basis;
    st.push("The rows for the ends and the start", {
        let mut all = ends.shown.clone();
        all.push(start.shown.clone());
        all.join(",\\quad ")
    });
    let steady = steady_state(&ends, x);
    let mut f = start.value.clone();
    if !steady.is_zero() {
        st.push(
            format!("Take off the steady temperature $w({lx})$ that meets the end values: then $v={u}-w$ is 0 at both ends and solves the same equation"),
            format!("w({lx})={}", latex(&steady)),
        );
        f = nice(&sub(f, steady.clone()));
    }
    let (lam, modes) = eigen_step(basis, &big_x, &lx);
    st.push(lam, modes);
    let shape = |k: Sym| vec![decay(powi(basis.rate(k.clone()), 2)), basis.mode(k)];
    let mut series = Series::default();
    if !steady.is_zero() {
        series.push(steady.clone());
    }
    let what = if basis.cosine { "cosine" } else { "sine" };
    if basis.cosine {
        let c0 = basis.mean(&f);
        if !c0.is_zero() {
            series
                .explicit
                .push(term(c0, vec![exp(mul2(r.clone(), var(t)))]));
        }
    }
    let coefs = basis.coefficients(&f);
    series.add(&coefs, "b", &shape, &|_| n(1));
    st.push(
        format!(
            "Add up these solutions. At ${lt}=0$ the sum must be the initial temperature{}, so the $b_n$ are its Fourier {what} coefficients",
            if steady.is_zero() { "" } else { " less $w$" }
        ),
        format!("b_{{n}}={}", basis.integral(&f)),
    );
    coef_step(&coefs, "b", st);
    check_series(p, &series, &ends, t, &start.value, &steady, None)?;
    let result = format!("{shown}={}", series.latex());
    st.push("So the solution is", result.clone());
    let answer = series.wheres.is_empty().then(|| result.clone());
    Ok((result, answer))
}

/// The straight line between the values held at the ends.
fn steady_state(ends: &Ends, x: &str) -> Sym {
    let Some((h0, h1)) = &ends.held else {
        return n(0);
    };
    let b = &ends.basis;
    let shifted = sub(var(x), b.a.clone());
    nice(&add2(
        h0.clone(),
        mul2(div(sub(h1.clone(), h0.clone()), b.l.clone()), shifted),
    ))
}

/// Which λ the ends allow, and the modes they give.
fn eigen_step(basis: &Basis, big_x: &str, lx: &str) -> (String, String) {
    let rate = latex(&nice(&powi(basis.rate(var("n")), 2)));
    let (a, b) = (latex(&basis.a), latex(&basis.end()));
    let mode = show(&basis.mode(var("n")));
    if basis.cosine {
        (
            format!("The insulated ends need ${big_x}'({a})={big_x}'({b})=0$. That leaves only $\\lambda={rate}$ for $n=0,1,2,\\dots$"),
            format!("{big_x}_{{n}}({lx})={mode}"),
        )
    } else {
        (
            format!("Both ends need ${big_x}({a})={big_x}({b})=0$. Only $\\lambda={rate}$ for $n=1,2,3,\\dots$ leaves ${big_x}\\neq0$"),
            format!("{big_x}_{{n}}({lx})={mode}"),
        )
    }
}

fn coef_step(c: &Coef, letter: &str, st: &mut Steps) {
    match c {
        Coef::Finite(list) if list.is_empty() => {
            st.push(format!("Every ${letter}_n$ is 0"), "")
        }
        Coef::Finite(list) => st.push(
            format!("The initial values are already a sum of these modes, so only a few ${letter}_n$ aren't 0"),
            list.iter()
                .map(|(k, v)| format!("{letter}_{{{k}}}={}", latex(v)))
                .collect::<Vec<_>>()
                .join(",\\quad "),
        ),
        Coef::Formula(v) => st.push(
            "Work out the integral, using $\\sin(n\\pi)=0$ and $\\cos(n\\pi)=(-1)^{n}$",
            format!("{letter}_{{n}}={}", latex(v)),
        ),
        Coef::Integral(_) => st.push(
            format!("This integral has no simple formula in $n$, so ${letter}_n$ is left as it is"),
            "",
        ),
    }
}

/// Puts a series solution back into the PDE and its conditions.
fn check_series(
    p: &Pde,
    series: &Series,
    ends: &Ends,
    t: &str,
    start: &Sym,
    steady: &Sym,
    speed: Option<&Sym>,
) -> Result<(), String> {
    let fail = || Err("Couldn't check the solution, so Solve won't show it".to_owned());
    let one = series.sample();
    if !p.holds(&one) {
        return fail();
    }
    let b = &ends.basis;
    let edge = if b.cosine {
        super::diff::derivative(&one, &b.x)
    } else {
        sub(one.clone(), steady.clone())
    };
    let zero = n(0);
    for end in [b.a.clone(), b.end()] {
        if !meets(&edge, &b.x, &end, &zero) && !meets(&edge.subs_var(t, &zero), &b.x, &end, &zero) {
            return fail();
        }
    }
    // When it is all written out, it must start as the rows say.
    if series.summed.is_empty() {
        let whole = series.explicit_sum();
        if !meets(&whole, t, &n(0), start) {
            return fail();
        }
        if let Some(g) = speed {
            if !meets(&super::diff::derivative(&whole, t), t, &n(0), g) {
                return fail();
            }
        }
    }
    Ok(())
}

// ---- Wave ---------------------------------------------------------------------

fn wave(p: &Pde, t: &str, x: &str, c2: Sym, conds: &[Cond], st: &mut Steps) -> Done {
    // √(c²) is c: a speed is taken positive.
    let c = match &c2 {
        Sym::Pow(b, e) if e.is_int(2) => (**b).clone(),
        _ => nice(&sqrt(c2.clone())),
    };
    let u = name(&p.unknown);
    let (lx, lt) = (name(x), name(t));
    let shown = p.u(x, t);
    st.push(
        format!(
            "This is the wave equation, with wave speed $c={}$",
            latex(&c)
        ),
        format!("{u}_{{{lt}{lt}}}={}{u}_{{{lx}{lx}}}", times(&c2)),
    );
    let (start, speed) = initial(conds, t)?;
    let ends = ends(p, x, conds)?;
    let left = nice(&sub(var(x), mul2(c.clone(), var(t))));
    let right = nice(&add2(var(x), mul2(c.clone(), var(t))));

    let Some(ends) = ends else {
        let Some(f) = start else {
            let test = add2(func(Func::Sin, left.clone()), exp(div(right.clone(), n(3))));
            if !p.holds(&test) {
                return Err("Couldn't check the solution, so Solve won't show it".into());
            }
            st.push(
                format!("Change variables to $\\xi={}$ and $\\eta={}$: the equation becomes ${u}_{{\\xi\\eta}}=0$", lead(&left, x), lead(&right, x)),
                format!("{u}_{{\\xi\\eta}}=0"),
            );
            let result = format!(
                "{shown}=F\\left({}\\right)+G\\left({}\\right)",
                lead(&left, x),
                lead(&right, x)
            );
            st.push(
                "Integrate once in each: the solution is any wave moving right plus any moving left (d'Alembert)",
                result.clone(),
            );
            st.push(
                format!("Add rows for the start, as ${u}({lx},0)=f({lx})$ and ${u}_{{{lt}}}({lx},0)=g({lx})$, and for the ends if there are any, to pick out one solution"),
                "",
            );
            return Ok((result, None));
        };
        // d'Alembert's formula on the whole line.
        let g = speed.map_or(n(0), |s| s.value.clone());
        let fx = |at: &Sym| f.value.subs_var(x, at);
        let mut sol = div(add2(fx(&left), fx(&right)), n(2));
        let mut integral = None;
        if !g.is_zero() {
            match antiderivative(&g, x, &mut Steps::quiet()) {
                Some(big) => {
                    sol = add2(
                        sol,
                        div(
                            sub(big.subs_var(x, &right), big.subs_var(x, &left)),
                            mul2(n(2), c.clone()),
                        ),
                    )
                }
                None => {
                    integral = Some(format!(
                        "\\frac{{1}}{{{}}}\\int_{{{}}}^{{{}}}{}\\,ds",
                        latex(&nice(&mul2(n(2), c.clone()))),
                        lead(&left, x),
                        lead(&right, x),
                        paren(&g.subs_var(x, &var("s")))
                    ))
                }
            }
        }
        let sol = {
            let (a, b) = (spread(&sol), nice(&sol));
            if latex(&b).len() * 5 < latex(&a).len() * 3 {
                b
            } else {
                a
            }
        };
        if integral.is_none()
            && (!p.holds(&sol)
                || !meets(&sol, t, &n(0), &f.value)
                || !meets(&super::diff::derivative(&sol, t), t, &n(0), &g))
        {
            return Err("Couldn't check the solution, so Solve won't show it".into());
        }
        st.push(
            if speed.is_some() {
                "With no rows for the ends, the string is infinitely long. These rows give its start"
            } else {
                "With no rows for the ends, the string is infinitely long. It starts at rest, as no row gives its starting speed"
            },
            conds
                .iter()
                .filter(|c| c.fixed == t)
                .map(|c| c.shown.clone())
                .collect::<Vec<_>>()
                .join(",\\quad "),
        );
        st.push(
            format!("d'Alembert's formula: ${shown}=\\frac{{f({lx}-ct)+f({lx}+ct)}}{{2}}+\\frac{{1}}{{2c}}\\int_{{{lx}-ct}}^{{{lx}+ct}}g(s)\\,ds$"),
            "",
        );
        let result = match &integral {
            Some(i) => format!(
                "{shown}={}+{i}",
                tidy(&latex(&sol), &[(&left, x), (&right, x)])
            ),
            None => format!("{shown}={}", tidy(&latex(&sol), &[(&left, x), (&right, x)])),
        };
        st.push("So the solution is", result.clone());
        let answer = integral.is_none().then(|| result.clone());
        return Ok((result, answer));
    };

    let Some(start) = start else {
        return Err(format!(
            "Add the starting shape as a row, as ${u}({lx},0)=f({lx})$, and its starting speed as ${u}_{{{lt}}}({lx},0)=g({lx})$"
        ));
    };
    let g = speed.map_or(n(0), |s| s.value.clone());
    let basis = &ends.basis;
    let mut shown_rows = ends.shown.clone();
    shown_rows.push(start.shown.clone());
    shown_rows.extend(speed.map(|s| s.shown.clone()));
    st.push(
        if speed.is_some() {
            "The rows for the ends and the start".to_owned()
        } else {
            "The rows for the ends and the start (no row gives the starting speed, so it starts at rest)".to_owned()
        },
        shown_rows.join(",\\quad "),
    );
    let steady = steady_state(&ends, x);
    let mut f = start.value.clone();
    if !steady.is_zero() {
        st.push(
            format!("Take off the straight line $w({lx})$ between the end values: then $v={u}-w$ is 0 at both ends and solves the same equation"),
            format!("w({lx})={}", latex(&steady)),
        );
        f = nice(&sub(f, steady.clone()));
    }
    let (big_x, big_t) = (part(x, &p.unknown), part(t, &p.unknown));
    st.push(
        format!("Separate the variables, ${u}={big_x}({lx}){big_t}({lt})$: then $\\frac{{{big_t}''}}{{{}{big_t}}}=\\frac{{{big_x}''}}{{{big_x}}}=-\\lambda$", if c2.is_one() { String::new() } else { paren(&c2) }),
        "",
    );
    let (lam, modes) = eigen_step(basis, &big_x, &lx);
    st.push(lam, modes);
    let omega = |k: Sym| mul2(c.clone(), basis.rate(k));
    let cos_shape = |k: Sym| {
        vec![
            func(Func::Cos, mul2(omega(k.clone()), var(t))),
            basis.mode(k),
        ]
    };
    let sin_shape = |k: Sym| {
        vec![
            func(Func::Sin, mul2(omega(k.clone()), var(t))),
            basis.mode(k),
        ]
    };
    let mut series = Series::default();
    if !steady.is_zero() {
        series.push(steady.clone());
    }
    if basis.cosine {
        let (a0, b0) = (basis.mean(&f), basis.mean(&g));
        series.push(nice(&add2(a0, mul2(b0, var(t)))));
    }
    let a_coefs = basis.coefficients(&f);
    let b_coefs = basis.coefficients(&g);
    series.add(&a_coefs, "a", &cos_shape, &|_| n(1));
    series.add(&b_coefs, "b", &sin_shape, &|k| div(n(1), omega(k)));
    let what = if basis.cosine { "cosine" } else { "sine" };
    st.push(
        format!("Each mode swings in time as $\\cos$ and $\\sin$ of $c\\sqrt{{\\lambda}}{lt}$. Add them up: at ${lt}=0$ the shape gives $a_n$ and the speed gives $b_n$, by Fourier {what} coefficients"),
        format!(
            "{shown}=\\sum\\left(a_{{n}}{}+b_{{n}}{}\\right){}",
            latex(&func(Func::Cos, mul2(omega(var("n")), var(t)))),
            latex(&func(Func::Sin, mul2(omega(var("n")), var(t)))),
            latex(&basis.mode(var("n")))
        ),
    );
    coef_step(&a_coefs, "a", st);
    if !g.is_zero() {
        coef_step(&b_coefs, "b", st);
    }
    check_series(p, &series, &ends, t, &start.value, &steady, Some(&g))?;
    let result = format!("{shown}={}", series.latex());
    st.push("So the solution is", result.clone());
    let answer = series.wheres.is_empty().then(|| result.clone());
    Ok((result, answer))
}

// ---- Laplace ------------------------------------------------------------------

fn laplace(p: &Pde, x: &str, y: &str, st: &mut Steps) -> Done {
    // u_xx + m u_yy = 0 with m > 0.
    let m = nice(&div(p.coef(&[y, y]), p.coef(&[x, x])));
    let u = name(&p.unknown);
    let (lx, ly) = (name(x), name(y));
    let (big_x, big_y) = (part(x, &p.unknown), part(y, &p.unknown));
    let shown = p.u(x, y);
    st.push(
        if m.is_one() {
            "This is Laplace's equation".to_owned()
        } else {
            "This is Laplace's equation, with the variables scaled".to_owned()
        },
        format!("{u}_{{{lx}{lx}}}+{}{u}_{{{ly}{ly}}}=0", times(&m)),
    );
    st.push(
        format!("Separate the variables, ${u}={big_x}({lx}){big_y}({ly})$, and divide by ${big_x}{big_y}$: each side is one constant, called $-k^2$"),
        format!(
            "\\frac{{{big_x}''}}{{{big_x}}}=-{}\\frac{{{big_y}''}}{{{big_y}}}=-k^{{2}}",
            times(&m)
        ),
    );
    let kk = var("k");
    let grow = nice(&div(mul2(kk.clone(), var(y)), sqrt(m.clone())));
    let xpart = add2(
        mul2(var("A"), func(Func::Cos, mul2(kk.clone(), var(x)))),
        mul2(var("B"), func(Func::Sin, mul2(kk, var(x)))),
    );
    let ypart = add2(
        mul2(var("C"), exp(grow.clone())),
        mul2(var("D"), exp(neg(grow.clone()))),
    );
    let sol = mul2(xpart.clone(), ypart.clone());
    if !p.holds(&sol) {
        return Err("Couldn't check the solution, so Solve won't show it".into());
    }
    let g = latex(&grow);
    let shown_y = format!("Ce^{{{g}}}+De^{{-{g}}}");
    st.push(
        "Solve each ordinary differential equation",
        format!(
            "{big_x}({lx})={},\\quad {big_y}({ly})={shown_y}",
            latex(&xpart),
        ),
    );
    let result = format!("{shown}={}\\left({shown_y}\\right)", paren(&xpart));
    st.push(
        "So the separated solutions are, for any constants",
        result.clone(),
    );
    st.push(
        format!("$k=0$ adds $(A+B{lx})(C+D{ly})$, swapping the roles of ${lx}$ and ${ly}$ gives more, and sums of solutions are solutions. Solve can't fit boundary conditions to Laplace's equation yet"),
        "",
    );
    Ok((result, None))
}

/// Whether other rows set conditions on the unknown.
pub fn has_conditions(p: &Pde, others: &[String]) -> bool {
    !p.conditions(others).is_empty()
}

/// Notes for the graph's rows: a PDE row and the rows that set its
/// conditions can't be drawn, and Solve works with them.
pub fn graph_notes(rows: &[String]) -> Vec<Option<String>> {
    let mut notes = vec![None; rows.len()];
    let mut unknowns = Vec::new();
    for (i, row) in rows.iter().enumerate() {
        if !row.contains("partial") && !row.contains('_') {
            continue;
        }
        let others: Vec<String> = rows
            .iter()
            .enumerate()
            .filter(|(j, _)| *j != i)
            .map(|(_, r)| r.clone())
            .collect();
        if let Some(Ok(p)) = read(row, &others) {
            notes[i] = Some("A partial differential equation: press Solve to solve it".to_owned());
            unknowns.push(p.unknown);
        }
    }
    if unknowns.is_empty() {
        return notes;
    }
    for (i, row) in rows.iter().enumerate() {
        if notes[i].is_some() {
            continue;
        }
        if let Ok(Statement::Relation(Expr::Apply(f, args), Relation::Eq, _)) = crate::parse(row) {
            let base = f.split_once('_').map_or(f.as_str(), |(b, _)| b);
            if args.len() == 2 && unknowns.iter().any(|u| u == base) {
                notes[i] = Some(
                    "A condition for the partial differential equation: Solve uses it".to_owned(),
                );
            }
        }
    }
    notes
}
