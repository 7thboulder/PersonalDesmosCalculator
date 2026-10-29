//! Differential equations solved exactly, by the methods they are taught
//! with: direct integration, separating the variables, integrating
//! factors, Bernoulli and homogeneous substitutions, exact equations,
//! characteristic equations with undetermined coefficients or variation of
//! parameters, Cauchy–Euler equations, reduction of order, and linear
//! systems by eigenvalues. Every answer is checked by putting it back into
//! the equation before it is shown.

use super::diff::{derivative, paren};
use super::integrate::{antiderivative, ii};
use super::latex::{latex, name};
use super::poly;
use super::solve::{self, Outcome};
use super::sym::*;
use super::{nice, out_of_time, Steps};

/// y(x₀) = values[0], y'(x₀) = values[1], …
#[derive(Clone, Debug)]
pub struct Conditions {
    pub x0: Sym,
    pub values: Vec<Sym>,
}

/// A solution: y = f (one or more branches), or an equation in x and y.
#[derive(Clone, Debug)]
pub enum Sol {
    Explicit(Vec<Sym>),
    Implicit(Sym, Sym),
}

/// The constants of integration handed out so far.
struct Ctx {
    numbered: bool,
    made: Vec<String>,
    avoid: Vec<String>,
}

impl Ctx {
    fn constant(&mut self) -> String {
        let mut k = self.made.len() + 1;
        loop {
            let c = if self.numbered {
                format!("C_{k}")
            } else if k == 1 {
                "C".to_owned()
            } else {
                format!("C_{k}")
            };
            if !self.avoid.contains(&c) && !self.made.contains(&c) {
                self.made.push(c.clone());
                return c;
            }
            k += 1;
        }
    }
}

pub fn dname(dep: &str, k: usize) -> String {
    format!("{dep}{}", "'".repeat(k))
}

fn is_dep(dep: &str, n: &str) -> bool {
    n.strip_prefix(dep)
        .is_some_and(|rest| rest.chars().all(|c| c == '\''))
}

fn free_of_dep(s: &Sym, dep: &str) -> bool {
    !s.names().iter().any(|n| is_dep(dep, n))
}

fn same(a: &Sym, b: &Sym) -> bool {
    a == b || nice(&expand(&sub(a.clone(), b.clone()))).is_zero()
}

/// `+ s` or `- s` to follow another term, or nothing for zero.
fn signed(s: &Sym) -> String {
    if s.is_zero() {
        String::new()
    } else if s.is_negative() {
        latex(s)
    } else {
        format!("+{}", latex(s))
    }
}

fn eq_latex(l: &Sym, r: &Sym) -> String {
    format!("{}={}", latex(l), latex(r))
}

/// Whether `s` is zero for every value of its letters, tested at points.
fn zero_everywhere(s: &Sym) -> bool {
    if nice(s).is_zero() {
        return true;
    }
    let names = s.names();
    let mut finite = 0;
    for trial in 0..6 {
        let env = |n: &str| {
            names
                .iter()
                .position(|m| m == n)
                .map(|i| 0.37 + 0.61 * ((i * 7 + trial * 3) % 11) as f64 / 3.0)
        };
        let v = s.eval(&env);
        if v.is_finite() {
            if v.abs() > 1e-8 {
                return false;
            }
            finite += 1;
        }
    }
    finite >= 3
}

// ---- Tidying ------------------------------------------------------------------------

/// e^(a + k ln u) = uᵏ e^a, so integrating factors and solutions read
/// simply. `drop_abs` writes e^(ln|x|) as x rather than |x|.
pub fn tidy_exp(s: &Sym, drop_abs: bool) -> Sym {
    match s {
        Sym::Pow(b, e) if **b == var("e") => {
            let e = tidy_exp(e, drop_abs);
            let mut keep = Vec::new();
            let mut out = Vec::new();
            for t in expand(&e).terms() {
                let (c, rest) = split_coef(&t);
                match &rest {
                    Sym::Fn(Func::Ln, u) if c.is_num() => {
                        let u = match &**u {
                            Sym::Fn(Func::Abs, inner) if drop_abs => (**inner).clone(),
                            u => u.clone(),
                        };
                        out.push(pow(u, c));
                    }
                    _ => keep.push(t),
                }
            }
            if out.is_empty() {
                return exp(e);
            }
            out.push(exp(add(keep)));
            mul(out)
        }
        Sym::Add(xs) => add(xs.iter().map(|x| tidy_exp(x, drop_abs)).collect()),
        Sym::Mul(xs) => mul(xs.iter().map(|x| tidy_exp(x, drop_abs)).collect()),
        Sym::Pow(a, b) => pow(tidy_exp(a, drop_abs), tidy_exp(b, drop_abs)),
        Sym::Fn(f, a) => func(*f, tidy_exp(a, drop_abs)),
        other => other.clone(),
    }
}

/// ln|u| as ln u, where any one antiderivative will do.
fn log_abs_dropped(s: &Sym) -> Sym {
    match s {
        Sym::Fn(Func::Ln, u) => match &**u {
            Sym::Fn(Func::Abs, inner) => func(Func::Ln, log_abs_dropped(inner)),
            u => func(Func::Ln, log_abs_dropped(u)),
        },
        Sym::Add(xs) => add(xs.iter().map(log_abs_dropped).collect()),
        Sym::Mul(xs) => mul(xs.iter().map(log_abs_dropped).collect()),
        Sym::Pow(a, b) => pow(log_abs_dropped(a), log_abs_dropped(b)),
        Sym::Fn(f, a) => func(*f, log_abs_dropped(a)),
        other => other.clone(),
    }
}

/// Folds what the constant `c` is multiplied by, shifted by or raised to
/// into `c` itself: ±e^(H + C) is C e^H, and 2C + 3 is C.
fn absorb(s: &Sym, c: &str, scale: bool) -> Sym {
    if solve::occurrences(s, c) != 1 {
        return s.clone();
    }
    let cv = var(c);
    let is_constant_multiple = |t: &Sym| {
        let k = div(t.clone(), cv.clone());
        !k.has(c) && k.is_constant() && !k.is_zero()
    };
    match s {
        Sym::Add(ts) => {
            let mut out = Vec::new();
            let mut plain = false;
            for t in ts {
                if !t.has(c) {
                    out.push(t.clone());
                } else if is_constant_multiple(t) {
                    plain = true;
                } else {
                    let a = absorb(t, c, scale);
                    if a == cv {
                        plain = true;
                    } else {
                        out.push(a);
                    }
                }
            }
            if plain {
                out.retain(|t| !t.is_constant());
                out.push(cv);
            }
            add(out)
        }
        Sym::Mul(fs) => {
            let mut out = Vec::new();
            for f in fs {
                if !f.has(c) {
                    out.push(f.clone());
                    continue;
                }
                match f {
                    Sym::Pow(b, e) if **b == var("e") => {
                        let e2 = absorb(e, c, scale);
                        if e2.terms().contains(&cv) {
                            out.push(cv.clone());
                            out.push(exp(sub(e2, cv.clone())));
                        } else {
                            out.push(exp(e2));
                        }
                    }
                    f => out.push(absorb(f, c, scale)),
                }
            }
            if out.contains(&cv) {
                out.retain(|f| !f.is_constant());
            }
            mul(out)
        }
        Sym::Pow(b, _) if **b == var("e") => absorb(&Sym::Mul(vec![s.clone()]), c, scale),
        // √(x²/2 + 2C) = √(1/2)·√(x² + 4C), so the C can take in the 4.
        Sym::Pow(b, e) if matches!(**b, Sym::Add(_)) && !e.has(c) => {
            let lead = b.terms().into_iter().find(|t| !t.has(c));
            match lead.map(|t| split_coef(&t).0) {
                Some(k) if scale && k.is_num() && !k.is_one() && !k.is_zero() => {
                    let inner = absorb(&expand(&div((**b).clone(), k.clone())), c, scale);
                    mul2(pow(k, (**e).clone()), pow(inner, (**e).clone()))
                }
                _ => pow(absorb(b, c, scale), (**e).clone()),
            }
        }
        Sym::Pow(b, e) => pow(absorb(b, c, scale), absorb(e, c, scale)),
        Sym::Fn(f, a) => func(*f, absorb(a, c, scale)),
        other => other.clone(),
    }
}

fn absorb_all(s: &Sym, consts: &[String]) -> Sym {
    let mut out = s.clone();
    for c in consts {
        let (a, b) = (absorb(&out, c, false), absorb(&out, c, true));
        for cand in [a, b] {
            if cand != out {
                let cand = flat_nice(&cand);
                if latex(&cand).len() < latex(&out).len() {
                    out = cand;
                }
            }
        }
        // -C is as good a constant as C.
        if out.has(c) && out.size() < 80 {
            let flipped = flat_nice(&out.subs_var(c, &neg(var(c))));
            if latex(&flipped).len() < latex(&out).len() {
                out = flipped;
            }
        }
    }
    out
}

/// The shorter of an expression and its multiplied-out form, without
/// putting it over one denominator: C₁eˣ + C₂e⁻ˣ reads better as it is.
fn flat_nice(s: &Sym) -> Sym {
    let x = expand(s);
    if latex(&x).len() < latex(s).len() {
        x
    } else {
        s.clone()
    }
}

// ---- Checking -----------------------------------------------------------------------

/// A made-up value for each letter other than x and the unknown function.
fn env_value(names: &[String], n: &str, trial: usize) -> Option<f64> {
    names
        .iter()
        .position(|m| m == n)
        .map(|i| 0.55 + 0.37 * ((i + 2 * trial) % 7) as f64)
}

/// Whether y = f solves `eq` = 0, tested at points with made-up values for
/// the constants.
fn verify(eq: &Sym, dep: &str, x: &str, order: usize, f: &Sym) -> bool {
    if f.has("i") || out_of_time() {
        return false;
    }
    // Multiplied out at each step, derivatives of e^(ax)cos(bx) stay small.
    let mut ds = vec![expand(f)];
    for _ in 0..order {
        if out_of_time() {
            return false;
        }
        let d = expand(&derivative(ds.last().unwrap(), x));
        ds.push(d);
    }
    let mut names: Vec<String> = eq.names();
    names.extend(f.names());
    names.retain(|n| n != x && !is_dep(dep, n));
    names.sort();
    names.dedup();
    let mut passed = 0;
    for (trial, x0) in [0.31, 0.87, 1.43, 2.11, -0.6, 3.3, -1.7]
        .into_iter()
        .enumerate()
    {
        let base = |n: &str| {
            if n == x {
                Some(x0)
            } else {
                env_value(&names, n, trial % 2)
            }
        };
        let dv: Vec<f64> = ds.iter().map(|d| d.eval(&base)).collect();
        if dv.iter().any(|v| !v.is_finite()) {
            continue;
        }
        let r = eq.eval(&|n: &str| {
            if is_dep(dep, n) {
                dv.get(n.len() - dep.len()).copied()
            } else {
                base(n)
            }
        });
        if !r.is_finite() {
            continue;
        }
        let scale = dv.iter().fold(1.0f64, |m, v| m.max(v.abs()));
        if r.abs() > 1e-6 * scale {
            return false;
        }
        passed += 1;
    }
    passed >= 2
}

/// Whether ψ(x, y) = C solves y' = F: ψₓ + ψ_y F = 0 at points.
fn verify_implicit(psi: &Sym, f: &Sym, dep: &str, x: &str) -> bool {
    let check = add2(derivative(psi, x), mul2(derivative(psi, dep), f.clone()));
    let mut names = check.names();
    names.retain(|n| n != x && n != dep);
    let mut passed = 0;
    for (x0, y0) in [
        (0.31, 0.53),
        (0.87, 1.21),
        (1.43, -0.7),
        (2.11, 0.37),
        (-0.6, 0.8),
    ] {
        let v = check.eval(&|n: &str| {
            if n == x {
                Some(x0)
            } else if n == dep {
                Some(y0)
            } else {
                env_value(&names, n, 0)
            }
        });
        if v.is_finite() {
            if v.abs() > 1e-6 {
                return false;
            }
            passed += 1;
        }
    }
    passed >= 2
}

// ---- Integration with working ----------------------------------------------------

/// ∫ f d`v`, with the working written down under a heading.
fn integrate(f: &Sym, v: &str, heading: Option<String>, st: &mut Steps) -> Option<Sym> {
    let mark = st.mark();
    if let Some(h) = heading {
        st.push(h, ii(f, v));
    }
    match antiderivative(f, v, st) {
        Some(r) => Some(nice(&r)),
        None => {
            st.undo(mark);
            None
        }
    }
}

// ---- Entry -------------------------------------------------------------------------

/// Solves `eq` = 0, a differential equation of order `order` in `dep`(x).
/// Returns the solution, the constants in it, and the steps.
pub fn solve_ode(
    eq: &Sym,
    order: usize,
    conditions: Option<&Conditions>,
    st: &mut Steps,
) -> Result<(String, Option<String>), String> {
    let (dep, x) = ("y", "x");
    let mut avoid = eq.names();
    avoid.retain(|n| !is_dep(dep, n));
    let mut ctx = Ctx {
        numbered: order > 1,
        made: Vec::new(),
        avoid,
    };
    let sol = match order {
        1 => first_order(eq, dep, x, &mut ctx, st),
        _ => higher_order(eq, order, dep, x, &mut ctx, st),
    };
    let Some(sol) = sol else {
        if out_of_time() {
            return Err("This took too long, so Solve stopped".into());
        }
        return Err(
            "Solve couldn't find an exact solution for this differential equation; the graph still draws it numerically"
                .into(),
        );
    };
    let yv = var(dep);
    let tidy = |s: &Sym| absorb_all(s, &ctx.made);
    let folded = match &sol {
        Sol::Explicit(fs) => Sol::Explicit(fs.iter().map(tidy).collect()),
        Sol::Implicit(l, r) => Sol::Implicit(tidy(l), tidy(r)),
    };
    let changed = match (&sol, &folded) {
        (Sol::Explicit(a), Sol::Explicit(b)) => a != b,
        (Sol::Implicit(a, b), Sol::Implicit(c, d)) => a != c || b != d,
        _ => false,
    };
    let sol = if changed && valid(&folded, eq, dep, x, order) {
        folded
    } else {
        sol
    };
    let general = match &sol {
        Sol::Explicit(fs) => fs
            .iter()
            .map(|f| eq_latex(&yv, f))
            .collect::<Vec<_>>()
            .join("\\quad\\text{or}\\quad "),
        Sol::Implicit(l, r) => eq_latex(l, r),
    };
    let consts: Vec<String> = ctx
        .made
        .iter()
        .filter(|c| match &sol {
            Sol::Explicit(fs) => fs.iter().any(|f| f.has(c)),
            Sol::Implicit(l, r) => l.has(c) || r.has(c),
        })
        .cloned()
        .collect();
    let list = consts
        .iter()
        .map(|c| format!("${}$", name(c)))
        .collect::<Vec<_>>()
        .join(", ");
    st.push(
        match consts.len() {
            0 => "So the solution is".to_owned(),
            1 => format!("So the general solution is, for any constant {list}"),
            _ => format!("So the general solution is, for any constants {list}"),
        },
        general.clone(),
    );
    let answer_of = |sol: &Sol| match sol {
        Sol::Explicit(fs) if fs.len() == 1 => Some(eq_latex(&yv, &fs[0])),
        // y = ±f graphs as y² = f².
        Sol::Explicit(fs) if fs.len() == 2 && same(&fs[0], &neg(fs[1].clone())) => Some(eq_latex(
            &powi(yv.clone(), 2),
            &nice(&expand(&powi(fs[0].clone(), 2))),
        )),
        Sol::Explicit(_) => None,
        Sol::Implicit(l, r) => Some(eq_latex(l, r)),
    };
    if let Some(cond) = conditions {
        if !consts.is_empty() {
            if let Some(p) = apply_conditions(&sol, &consts, cond, dep, x, st) {
                let shown = match &p {
                    Sol::Explicit(fs) => fs
                        .iter()
                        .map(|f| eq_latex(&yv, f))
                        .collect::<Vec<_>>()
                        .join("\\quad\\text{or}\\quad "),
                    Sol::Implicit(l, r) => eq_latex(l, r),
                };
                return Ok((shown, answer_of(&p)));
            }
        }
    }
    Ok((general, answer_of(&sol)))
}

/// Whether a tidied solution still solves the equation.
fn valid(sol: &Sol, eq: &Sym, dep: &str, x: &str, order: usize) -> bool {
    match sol {
        Sol::Explicit(fs) => fs.iter().all(|f| verify(eq, dep, x, order, f)),
        Sol::Implicit(..) => true,
    }
}

/// Finds the constants that make the solution pass through the initial
/// point with the given initial derivatives.
fn apply_conditions(
    sol: &Sol,
    consts: &[String],
    cond: &Conditions,
    dep: &str,
    x: &str,
    st: &mut Steps,
) -> Option<Sol> {
    let mark = st.mark();
    let n = consts.len();
    if cond.values.len() < n {
        return None;
    }
    let x0 = &cond.x0;
    let shown: Vec<String> = (0..n)
        .map(|k| {
            format!(
                "{}\\left({}\\right)={}",
                name(&dname(dep, k)),
                latex(x0),
                latex(&cond.values[k])
            )
        })
        .collect();
    st.push(
        if n == 1 {
            "Use the initial condition to find the constant"
        } else {
            "Use the initial conditions to find the constants"
        },
        shown.join(",\\ "),
    );
    let values: Vec<Sym> = match sol {
        Sol::Implicit(l, r) => {
            if n != 1 {
                st.undo(mark);
                return None;
            }
            let at = |s: &Sym| s.subs_var(x, x0).subs_var(dep, &cond.values[0]);
            let (l0, r0) = (nice(&at(l)), nice(&at(r)));
            st.push("Put the point in", eq_latex(&l0, &r0));
            let c = pick_root(solve::solve(l0, r0, &consts[0], &mut Steps::quiet(), 0))?;
            vec![c]
        }
        Sol::Explicit(fs) => {
            // The branch that can pass through the point.
            let f = fs
                .iter()
                .find(|f| {
                    let at = nice(&f.subs_var(x, x0));
                    n > 1 || {
                        let c = pick_root(solve::solve(
                            at,
                            cond.values[0].clone(),
                            &consts[0],
                            &mut Steps::quiet(),
                            0,
                        ));
                        c.is_some_and(|c| {
                            let back = f.subs_var(&consts[0], &c).subs_var(x, x0);
                            let (a, b) = (back.value(), cond.values[0].value());
                            !(a.is_finite() && b.is_finite())
                                || (a - b).abs() < 1e-7 * b.abs().max(1.0)
                        })
                    }
                })
                .cloned()
                .or_else(|| fs.first().cloned())?;
            let mut eqs: Vec<(Sym, Sym)> = Vec::new();
            let mut d = f.clone();
            for k in 0..n {
                if k > 0 {
                    d = expand(&derivative(&d, x));
                }
                eqs.push((nice(&d.subs_var(x, x0)), cond.values[k].clone()));
            }
            st.push(
                "Put the point in",
                eqs.iter()
                    .map(|(l, r)| eq_latex(l, r))
                    .collect::<Vec<_>>()
                    .join(",\\ "),
            );
            let values = if n == 1 {
                vec![pick_root(solve::solve(
                    eqs[0].0.clone(),
                    eqs[0].1.clone(),
                    &consts[0],
                    &mut Steps::quiet(),
                    0,
                ))?]
            } else {
                // The constants appear linearly in a linear equation's solution.
                let rows: Vec<Vec<Sym>> = eqs
                    .iter()
                    .map(|(l, r)| {
                        let mut row: Vec<Sym> =
                            consts.iter().map(|c| nice(&derivative(l, c))).collect();
                        let rest = consts.iter().fold(l.clone(), |s, c| s.subs_var(c, &n0()));
                        row.push(nice(&sub(r.clone(), rest)));
                        row
                    })
                    .collect();
                solve_linear_sym(rows, n)?
            };
            let sol = Sol::Explicit(vec![f]);
            return finish_conditions(sol, consts, values, cond, dep, x, st, mark);
        }
    };
    finish_conditions(sol.clone(), consts, values, cond, dep, x, st, mark)
}

fn n0() -> Sym {
    n(0)
}

#[allow(clippy::too_many_arguments)]
fn finish_conditions(
    sol: Sol,
    consts: &[String],
    values: Vec<Sym>,
    cond: &Conditions,
    dep: &str,
    x: &str,
    st: &mut Steps,
    mark: usize,
) -> Option<Sol> {
    let values: Vec<Sym> = values.iter().map(nice).collect();
    if values
        .iter()
        .any(|v| v.has("i") || matches!(v, Sym::Real(r) if !r.is_finite()))
    {
        st.undo(mark);
        return None;
    }
    st.push(
        "So",
        consts
            .iter()
            .zip(&values)
            .map(|(c, v)| format!("{}={}", name(c), latex(v)))
            .collect::<Vec<_>>()
            .join(",\\ "),
    );
    let put = |s: &Sym| {
        let mut out = s.clone();
        for (c, v) in consts.iter().zip(&values) {
            out = out.subs_var(c, v);
        }
        nice(&out)
    };
    let out = match sol {
        Sol::Explicit(fs) => Sol::Explicit(fs.iter().map(put).collect()),
        Sol::Implicit(l, r) => Sol::Implicit(put(&l), put(&r)),
    };
    let shown = match &out {
        Sol::Explicit(fs) => eq_latex(&var(dep), &fs[0]),
        Sol::Implicit(l, r) => eq_latex(l, r),
    };
    st.push(
        format!(
            "Put the constants back: the solution through $\\left({},{}\\right)$ is",
            latex(&cond.x0),
            latex(&cond.values[0])
        ),
        shown,
    );
    let _ = x;
    Some(out)
}

fn pick_root(o: Outcome) -> Option<Sym> {
    match o {
        Outcome::Roots(rs) => rs.into_iter().find(|r| !r.has("k") && !r.has("n")),
        _ => None,
    }
}

/// Gaussian elimination over symbolic expressions; `rows` are augmented
/// with the right-hand side and may be more than `n`.
fn solve_linear_sym(mut rows: Vec<Vec<Sym>>, n: usize) -> Option<Vec<Sym>> {
    let zero = |s: &Sym| zero_everywhere(s);
    let mut r = 0;
    let mut pivots = Vec::new();
    for col in 0..n {
        let Some(p) = (r..rows.len()).find(|&i| !zero(&rows[i][col])) else {
            return None;
        };
        rows.swap(r, p);
        let pv = rows[r][col].clone();
        for k in col..=n {
            rows[r][k] = nice(&div(rows[r][k].clone(), pv.clone()));
        }
        for i in 0..rows.len() {
            if i != r && !zero(&rows[i][col]) {
                let f = rows[i][col].clone();
                for k in col..=n {
                    let v = sub(rows[i][k].clone(), mul2(f.clone(), rows[r][k].clone()));
                    rows[i][k] = nice(&expand(&v));
                }
            }
        }
        pivots.push(r);
        r += 1;
        if out_of_time() {
            return None;
        }
    }
    // Leftover rows must say 0 = 0.
    if rows[r..].iter().any(|row| !zero(&row[n])) {
        return None;
    }
    Some(pivots.iter().map(|&i| rows[i][n].clone()).collect())
}

// ---- First order -------------------------------------------------------------------

/// Tries one method, keeping its working only when it finds a solution.
fn attempt(st: &mut Steps, f: impl FnOnce(&mut Steps) -> Option<Sol>) -> Option<Sol> {
    let mark = st.mark();
    let r = f(st);
    if r.is_none() {
        st.undo(mark);
    }
    r
}

fn first_order(eq: &Sym, dep: &str, x: &str, ctx: &mut Ctx, st: &mut Steps) -> Option<Sol> {
    let yp = dname(dep, 1);
    let c = poly::coeffs(eq, &yp);
    let (f, mn) = match c.as_deref() {
        Some([c0, c1]) if !c1.has(&yp) && !c1.is_zero() => (
            nice(&div(neg(c0.clone()), c1.clone())),
            Some((c0.clone(), c1.clone())),
        ),
        _ => match solve::solve(eq.clone(), n(0), &yp, &mut Steps::quiet(), 0) {
            Outcome::Roots(rs) if rs.len() == 1 && !rs[0].has(&yp) => (nice(&rs[0]), None),
            _ => return None,
        },
    };
    let ypv = var(&yp);
    let shown = eq_latex(&ypv, &f);
    if !same(eq, &sub(ypv.clone(), f.clone())) {
        st.push(format!("Solve for ${}$", name(&yp)), shown.clone());
    }
    let check = |sol: &Sol| match sol {
        Sol::Explicit(fs) => fs.iter().all(|g| verify(eq, dep, x, 1, g)),
        Sol::Implicit(l, r) => verify_implicit(&sub(l.clone(), r.clone()), &f, dep, x),
    };
    let made = ctx.made.len();
    let mut tries: Vec<Box<dyn FnMut(&mut Steps, &mut Ctx) -> Option<Sol>>> = vec![
        Box::new(|st, ctx| direct(&f, dep, x, ctx, st)),
        Box::new(|st, ctx| linear_first(&f, dep, x, ctx, st)),
        Box::new(|st, ctx| separable(&f, dep, x, ctx, st)),
        Box::new(|st, ctx| bernoulli(&f, dep, x, ctx, st)),
        Box::new(|st, ctx| {
            let (m, nn) = mn.clone().unwrap_or((neg(f.clone()), n(1)));
            exact(&m, &nn, dep, x, ctx, st)
        }),
        Box::new(|st, ctx| homogeneous(&f, dep, x, ctx, st)),
    ];
    for t in tries.iter_mut() {
        if out_of_time() {
            return None;
        }
        ctx.made.truncate(made);
        if let Some(sol) = attempt(st, |st| t(st, ctx).filter(check)) {
            return Some(sol);
        }
    }
    ctx.made.truncate(made);
    None
}

/// y' = f(x): integrate both sides.
fn direct(f: &Sym, dep: &str, x: &str, ctx: &mut Ctx, st: &mut Steps) -> Option<Sol> {
    if !free_of_dep(f, dep) {
        return None;
    }
    let r = integrate(
        f,
        x,
        Some(format!(
            "The right side has no ${}$, so integrate both sides",
            name(dep)
        )),
        st,
    )?;
    let c = ctx.constant();
    let y = add2(r, var(&c));
    Some(Sol::Explicit(vec![y]))
}

/// The coefficients P, Q when f = P·y + Q with P, Q free of y.
fn linear_in(f: &Sym, dep: &str) -> Option<(Sym, Sym)> {
    let c = poly::coeffs(f, dep)?;
    if c.len() != 2 || c.iter().any(|k| !free_of_dep(k, dep)) {
        return None;
    }
    Some((c[1].clone(), c[0].clone()))
}

/// y' + P y = Q, by the integrating factor e^(∫P dx).
fn linear_first(f: &Sym, dep: &str, x: &str, ctx: &mut Ctx, st: &mut Steps) -> Option<Sol> {
    let (a, q) = linear_in(f, dep)?;
    let p = nice(&neg(a));
    let yv = var(dep);
    st.push(
        format!(
            "This is linear in ${}$: write it as ${}+P\\left({}\\right){}=Q\\left({}\\right)$",
            name(dep),
            name(&dname(dep, 1)),
            name(x),
            name(dep),
            name(x)
        ),
        format!(
            "{}{}={}",
            name(&dname(dep, 1)),
            signed(&mul2(p.clone(), yv.clone())),
            latex(&q)
        ),
    );
    let int_p = nice(&antiderivative(&p, x, &mut Steps::quiet())?);
    let mu = nice(&tidy_exp(&exp(log_abs_dropped(&int_p)), true));
    let shown_mu = if exp(int_p.clone()) == mu {
        latex(&mu)
    } else {
        format!("e^{{{}}}={}", latex(&int_p), latex(&mu))
    };
    st.push(
        format!(
            "The integrating factor is $\\mu=e^{{\\int P\\ d{}}}$ (any one will do, so absolute values can go)",
            name(x)
        ),
        format!("\\mu={shown_mu}"),
    );
    let rhs = nice(&mul2(mu.clone(), q.clone()));
    st.push(
        format!(
            "Multiply both sides by $\\mu$: the left side is then the derivative of $\\mu {}$",
            name(dep)
        ),
        format!(
            "\\frac{{d}}{{d{}}}\\left[{}\\right]={}",
            name(x),
            latex(&mul2(mu.clone(), yv.clone())),
            latex(&rhs)
        ),
    );
    let r = if rhs.is_zero() {
        n(0)
    } else {
        integrate(&rhs, x, Some("Integrate both sides".into()), st)?
    };
    let c = ctx.constant();
    let total = add2(r, var(&c));
    st.push(
        "With the constant of integration",
        eq_latex(&mul2(mu.clone(), yv.clone()), &total),
    );
    let y = nice(&expand(&div(total, mu.clone())));
    let y = nice(&y);
    st.push(format!("Divide by ${}$", latex(&mu)), eq_latex(&yv, &y));
    Some(Sol::Explicit(vec![y]))
}

/// A product's factors, with e^(a + b) as e^a and e^b so they can be told
/// apart.
fn split_exp(s: &Sym) -> Vec<Sym> {
    s.factors()
        .into_iter()
        .flat_map(|f| match &f {
            Sym::Pow(b, e) if **b == var("e") && matches!(**e, Sym::Add(_)) => {
                e.terms().into_iter().map(exp).collect()
            }
            _ => vec![f],
        })
        .collect()
}

/// f(x, y) as f(x)·g(y), when it splits.
fn separate(f: &Sym, dep: &str, x: &str) -> Option<(Sym, Sym)> {
    let try_factors = |s: &Sym| -> Option<(Sym, Sym)> {
        let (mut fx, mut gy) = (Vec::new(), Vec::new());
        for k in split_exp(s) {
            match (k.has(x), !free_of_dep(&k, dep)) {
                (true, true) => return None,
                (_, true) => gy.push(k),
                _ => fx.push(k),
            }
        }
        Some((mul(fx), mul(gy)))
    };
    for form in [
        f.clone(),
        solve::common_factor(&expand(f)),
        together(f),
        solve::common_factor(&together(f)),
    ] {
        if let Some(p) = try_factors(&form) {
            return Some(p);
        }
    }
    // f(x, y) f(x₀, y₀) = f(x, y₀) f(x₀, y) for every x and y when it splits.
    for (x0, y0) in [(1, 1), (0, 0), (1, 0), (0, 1), (2, 1), (1, 2), (2, 3)] {
        let at = f.subs_var(x, &n(x0)).subs_var(dep, &n(y0));
        if zero_everywhere(&at) || !at.eval(&|_| Some(0.7)).is_finite() {
            continue;
        }
        let g = nice(&f.subs_var(x, &n(x0)));
        let h = nice(&div(f.subs_var(dep, &n(y0)), at));
        if zero_everywhere(&sub(mul2(g.clone(), h.clone()), f.clone())) {
            return Some((h, g));
        }
        break;
    }
    None
}

/// y' = f(x) g(y): ∫ dy/g(y) = ∫ f(x) dx.
fn separable(f: &Sym, dep: &str, x: &str, ctx: &mut Ctx, st: &mut Steps) -> Option<Sol> {
    let (fx, gy) = separate(f, dep, x)?;
    if free_of_dep(&gy, dep) {
        return None;
    }
    let left = nice(&div(n(1), gy.clone()));
    let yv = var(dep);
    st.push(
        "Separate the variables: everything with $y$ on the left, everything with $x$ on the right"
            .replace("$y$", &format!("${}$", name(dep)))
            .replace("$x$", &format!("${}$", name(x))),
        format!(
            "{}\\ d{}={}\\ d{}",
            paren(&left),
            name(dep),
            paren(&fx),
            name(x)
        ),
    );
    let gl = integrate(&left, dep, Some("Integrate the left side".into()), st)?;
    let hr = integrate(&fx, x, Some("Integrate the right side".into()), st)?;
    let c = ctx.constant();
    let rhs = add2(hr, var(&c));
    st.push(
        "Put the sides together, with one constant for both",
        eq_latex(&gl, &rhs),
    );
    Some(explicit(gl, rhs, dep, &[c], st).unwrap_or_else(|(l, r)| Sol::Implicit(l, r))).map(|s| {
        let _ = &yv;
        s
    })
}

/// Solves l = r for y when Solve can, folding constants together.
fn explicit(
    l: Sym,
    r: Sym,
    dep: &str,
    consts: &[String],
    st: &mut Steps,
) -> Result<Sol, (Sym, Sym)> {
    let (l, r) = unlog(l, r, consts, st);
    let roots = match solve::solve(l.clone(), r.clone(), dep, &mut Steps::quiet(), 0) {
        Outcome::Roots(rs) => rs,
        _ => return Err((l, r)),
    };
    if roots.is_empty()
        || roots.len() > 2
        || roots
            .iter()
            .any(|r| r.has("k") || r.has("n") || r.has(dep) || r.has("i"))
    {
        return Err((l, r));
    }
    let yv = var(dep);
    let raw: Vec<Sym> = roots.iter().map(nice).collect();
    st.push(
        format!("Solve for ${}$", name(dep)),
        raw.iter()
            .map(|f| eq_latex(&yv, f))
            .collect::<Vec<_>>()
            .join("\\quad\\text{or}\\quad "),
    );
    let mut out: Vec<Sym> = Vec::new();
    for f in &raw {
        let g = nice(&absorb_all(&tidy_exp(f, false), consts));
        if !out.iter().any(|h| same(h, &g)) {
            out.push(g);
        }
    }
    if out != raw {
        st.push(
            "A constant times, plus or raised to a number is just another constant, so rename it",
            out.iter()
                .map(|f| eq_latex(&yv, f))
                .collect::<Vec<_>>()
                .join("\\quad\\text{or}\\quad "),
        );
    }
    Ok(Sol::Explicit(out))
}

/// a ln|u| + b ln|v| = H + C becomes uᵃ vᵇ = C e^H: the logs combine, e
/// is raised to both sides, and ±e^C is just another constant.
fn unlog(l: Sym, r: Sym, consts: &[String], st: &mut Steps) -> (Sym, Sym) {
    let [c] = consts else { return (l, r) };
    let cv = var(c);
    if !r.terms().contains(&cv) || l.has(c) {
        return (l, r);
    }
    let mut parts = Vec::new();
    for t in l.terms() {
        let (k, rest) = split_coef(&t);
        let Sym::Fn(Func::Ln, u) = rest else {
            return (l, r);
        };
        if !k.is_num() {
            return (l, r);
        }
        let u = match *u {
            Sym::Fn(Func::Abs, inner) => *inner,
            u => u,
        };
        parts.push(pow(u, k));
    }
    let left = nice(&mul(parts));
    let right = nice(&mul2(cv.clone(), tidy_exp(&exp(sub(r, cv)), true)));
    st.push(
        format!(
            "Combine the logarithms and raise $e$ to both sides; $\\pm e^{{{0}}}$ is just another constant, so call it ${0}$ again",
            name(c)
        ),
        eq_latex(&left, &right),
    );
    (left, right)
}

/// The powers of y in f and their coefficients, when every term is a
/// coefficient free of y times a power of y.
fn powers_of(f: &Sym, dep: &str) -> Option<Vec<(Q, Sym)>> {
    let mut out: Vec<(Q, Sym)> = Vec::new();
    for t in expand(f).terms() {
        let mut k = Q::ZERO;
        let mut rest = Vec::new();
        for fac in t.factors() {
            match &fac {
                Sym::Var(v) if v == dep => k = k.add(Q::ONE)?,
                Sym::Pow(b, e) if matches!(&**b, Sym::Var(v) if v == dep) => {
                    k = k.add(e.as_q()?)?
                }
                other if !free_of_dep(other, dep) => return None,
                other => rest.push(other.clone()),
            }
        }
        match out.iter_mut().find(|(q, _)| *q == k) {
            Some(p) => p.1 = add2(p.1.clone(), mul(rest)),
            None => out.push((k, mul(rest))),
        }
    }
    Some(out)
}

/// y' = P y + Q yⁿ: v = y^(1-n) makes it linear.
fn bernoulli(f: &Sym, dep: &str, x: &str, ctx: &mut Ctx, st: &mut Steps) -> Option<Sol> {
    let ps = powers_of(f, dep)?;
    let a = ps
        .iter()
        .find(|(k, _)| *k == Q::ONE)
        .map(|(_, c)| c.clone())
        .unwrap_or(n(0));
    let others: Vec<&(Q, Sym)> = ps.iter().filter(|(k, _)| *k != Q::ONE).collect();
    let [(k, b)] = others.as_slice() else {
        return None;
    };
    if *k == Q::ZERO || b.is_zero() {
        return None;
    }
    let one_minus = Q::ONE.sub(*k)?;
    let v = if dep == "v" { "w" } else { "v" };
    let vv = var(v);
    let kk = Sym::Num(*k);
    st.push(
        format!(
            "This is a Bernoulli equation, ${}={}$: substitute ${}={}^{{{}}}$ to make it linear",
            name(&dname(dep, 1)),
            latex(&add2(
                mul2(a.clone(), var(dep)),
                mul2(b.clone(), pow(var(dep), kk))
            )),
            name(v),
            name(dep),
            latex(&Sym::Num(one_minus))
        ),
        eq_latex(
            &var(&dname(v, 1)),
            &mul(vec![
                Sym::Num(one_minus),
                pow(var(dep), Sym::Num(k.neg())),
                var(&dname(dep, 1)),
            ]),
        ),
    );
    let fv = nice(&mul2(
        Sym::Num(one_minus),
        add2(mul2(a.clone(), vv.clone()), b.clone()),
    ));
    st.push(
        format!(
            "Put in ${}'$ to get a linear equation in ${}$",
            name(dep),
            name(v)
        ),
        eq_latex(&var(&dname(v, 1)), &fv),
    );
    let Sol::Explicit(vs) = linear_first(&fv, v, x, ctx, st)? else {
        return None;
    };
    let back = nice(&pow(vs[0].clone(), Sym::Num(Q::ONE.div(one_minus)?)));
    st.push(
        format!(
            "Undo the substitution: ${}={}^{{{}}}$",
            name(dep),
            name(v),
            latex(&Sym::Num(Q::ONE.div(one_minus)?))
        ),
        eq_latex(&var(dep), &back),
    );
    Some(Sol::Explicit(vec![back]))
}

/// M + N y' = 0 with M_y = N_x, or made so by an integrating factor that
/// depends on x or y alone: the solution is ψ = C.
fn exact(m: &Sym, nn: &Sym, dep: &str, x: &str, ctx: &mut Ctx, st: &mut Steps) -> Option<Sol> {
    let (mut m, mut nn) = (nice(m), nice(nn));
    let gap = |m: &Sym, nn: &Sym| sub(derivative(m, dep), derivative(nn, x));
    let dyx = |m: &Sym, nn: &Sym| {
        format!(
            "\\frac{{\\partial M}}{{\\partial {}}}={},\\ \\frac{{\\partial N}}{{\\partial {}}}={}",
            name(dep),
            latex(&nice(&derivative(m, dep))),
            name(x),
            latex(&nice(&derivative(nn, x)))
        )
    };
    let form = |m: &Sym, nn: &Sym| {
        format!(
            "{}\\ d{}+{}\\ d{}=0",
            paren(m),
            name(x),
            paren(nn),
            name(dep)
        )
    };
    if !zero_everywhere(&gap(&m, &nn)) {
        // An integrating factor μ(x) or μ(y).
        let g = gap(&m, &nn);
        let px = nice(&div(g.clone(), nn.clone()));
        let py = nice(&div(neg(g), m.clone()));
        let mu = if !px.is_zero() && free_of_dep(&px, dep) {
            let i = antiderivative(&px, x, &mut Steps::quiet())?;
            (nice(&tidy_exp(&exp(log_abs_dropped(&i)), true)), x)
        } else if !py.is_zero() && !py.has(x) {
            let i = antiderivative(&py, dep, &mut Steps::quiet())?;
            (nice(&tidy_exp(&exp(log_abs_dropped(&i)), true)), dep)
        } else {
            return None;
        };
        st.push(
            format!(
                "Write it as $M\\ d{}+N\\ d{}=0$. It isn't exact, but multiplying by $\\mu\\left({}\\right)={}$ makes it so",
                name(x),
                name(dep),
                name(mu.1),
                latex(&mu.0)
            ),
            form(&m, &nn),
        );
        m = nice(&mul2(mu.0.clone(), m));
        nn = nice(&mul2(mu.0, nn));
        if !zero_everywhere(&gap(&m, &nn)) {
            return None;
        }
        st.push(
            "Multiplied through, the cross derivatives now match",
            format!("{},\\quad {}", form(&m, &nn), dyx(&m, &nn)),
        );
    } else {
        if free_of_dep(&m, dep) && nn.is_one() {
            return None;
        }
        st.push(
            format!(
                "Write it as $M\\ d{}+N\\ d{}=0$. The cross derivatives match, so it is exact",
                name(x),
                name(dep)
            ),
            format!("{},\\quad {}", form(&m, &nn), dyx(&m, &nn)),
        );
    }
    let psi_x = integrate(
        &m,
        x,
        Some(format!(
            "Find $\\psi$ with $\\psi_{{{}}}=M$: integrate $M$ with respect to ${}$",
            name(x),
            name(x)
        )),
        st,
    )?;
    let h_prime = nice(&expand(&sub(nn.clone(), derivative(&psi_x, dep))));
    if h_prime.has(x) && !zero_everywhere(&derivative(&h_prime, x)) {
        return None;
    }
    let h = if h_prime.is_zero() {
        n(0)
    } else {
        integrate(
            &h_prime,
            dep,
            Some(format!(
                "Add $h\\left({}\\right)$ so that $\\psi_{{{}}}=N$, which needs $h'={}$",
                name(dep),
                name(dep),
                latex(&h_prime)
            )),
            st,
        )?
    };
    let psi = nice(&add2(psi_x, h));
    let c = ctx.constant();
    st.push(
        "The solution is $\\psi = C$".replace('C', &name(&c)),
        eq_latex(&psi, &var(&c)),
    );
    Some(explicit(psi, var(&c), dep, &[c], st).unwrap_or_else(|(l, r)| Sol::Implicit(l, r)))
}

/// y' = F(y/x): y = vx makes it separable.
fn homogeneous(f: &Sym, dep: &str, x: &str, ctx: &mut Ctx, st: &mut Steps) -> Option<Sol> {
    if f.has(x) == false || free_of_dep(f, dep) {
        return None;
    }
    // F(tx, ty) = F(x, y).
    for t in [2, 3] {
        let scaled = f
            .subs_var(x, &mul2(n(t), var("\u{3}x")))
            .subs_var(dep, &mul2(n(t), var("\u{3}y")))
            .subs_var("\u{3}x", &var(x))
            .subs_var("\u{3}y", &var(dep));
        if !zero_everywhere(&sub(scaled, f.clone())) {
            return None;
        }
    }
    let v = if dep == "v" { "w" } else { "v" };
    let vv = var(v);
    let fv = nice(
        &f.subs_var(dep, &mul2(vv.clone(), var(x)))
            .subs_var(x, &n(1)),
    );
    let fv = if fv.has(x) { return None } else { fv };
    st.push(
        format!(
            "The right side only depends on ${}/{}$, so substitute ${}={}{}$, with ${}'={}+{}{}'$",
            name(dep),
            name(x),
            name(dep),
            name(v),
            name(x),
            name(dep),
            name(v),
            name(x),
            name(v)
        ),
        eq_latex(&add2(vv.clone(), mul2(var(x), var(&dname(v, 1)))), &fv),
    );
    let g = nice(&sub(fv, vv.clone()));
    if g.is_zero() {
        return None;
    }
    let left = nice(&div(n(1), g));
    st.push(
        "Separate the variables",
        format!(
            "{}\\ d{}=\\frac{{1}}{{{}}}\\ d{}",
            paren(&left),
            name(v),
            name(x),
            name(x)
        ),
    );
    let gl = integrate(&left, v, Some("Integrate the left side".into()), st)?;
    let hr = func(Func::Ln, func(Func::Abs, var(x)));
    let c = ctx.constant();
    let rhs = add2(hr, var(&c));
    st.push(
        "Integrate the right side and put them together",
        eq_latex(&gl, &rhs),
    );
    let ratio = div(var(dep), var(x));
    let l = nice(&gl.subs_var(v, &ratio));
    st.push(
        format!("Put ${}={}/{}$ back", name(v), name(dep), name(x)),
        eq_latex(&l, &rhs),
    );
    Some(explicit(l, rhs, dep, &[c], st).unwrap_or_else(|(l, r)| Sol::Implicit(l, r)))
}

// ---- Higher order ------------------------------------------------------------------

/// The coefficients a₀ … aₙ and right side g of a linear equation
/// aₙ y⁽ⁿ⁾ + … + a₀ y = g.
fn linear_coeffs(eq: &Sym, dep: &str, order: usize) -> Option<(Vec<Sym>, Sym)> {
    let mut rest = expand(eq);
    let mut a = vec![n(0); order + 1];
    for k in (0..=order).rev() {
        let c = poly::coeffs(&rest, &dname(dep, k))?;
        if c.len() > 2 {
            return None;
        }
        if c.len() == 2 {
            if !free_of_dep(&c[1], dep) {
                return None;
            }
            a[k] = nice(&c[1]);
        }
        rest = c[0].clone();
    }
    if !free_of_dep(&rest, dep) || a[order].is_zero() {
        return None;
    }
    Some((a, nice(&neg(rest))))
}

fn operator_latex(a: &[Sym], dep: &str) -> String {
    let terms: Vec<Sym> = a
        .iter()
        .enumerate()
        .rev()
        .map(|(k, c)| mul2(c.clone(), var(&dname(dep, k))))
        .collect();
    latex(&Sym::Add(terms.into_iter().filter(|t| !t.is_zero()).collect()).flat())
}

impl Sym {
    fn flat(self) -> Sym {
        match self {
            Sym::Add(mut xs) if xs.len() == 1 => xs.pop().unwrap(),
            Sym::Add(xs) if xs.is_empty() => n(0),
            other => other,
        }
    }
}

fn higher_order(
    eq: &Sym,
    order: usize,
    dep: &str,
    x: &str,
    ctx: &mut Ctx,
    st: &mut Steps,
) -> Option<Sol> {
    let check = |sol: &Sol| match sol {
        Sol::Explicit(fs) => fs.iter().all(|g| verify(eq, dep, x, order, g)),
        Sol::Implicit(..) => true,
    };
    let made = ctx.made.len();
    let linear = linear_coeffs(eq, dep, order);
    let mut tries: Vec<Box<dyn FnMut(&mut Steps, &mut Ctx) -> Option<Sol>>> = Vec::new();
    tries.push(Box::new(|st, ctx| {
        repeated_integration(eq, order, dep, x, ctx, st)
    }));
    if let Some((a, g)) = linear.clone() {
        let (a2, g2) = (a.clone(), g.clone());
        tries.push(Box::new(move |st, ctx| {
            constant_coefficients(&a, &g, dep, x, ctx, st)
        }));
        tries.push(Box::new(move |st, ctx| {
            cauchy_euler(&a2, &g2, dep, x, ctx, st)
        }));
    }
    tries.push(Box::new(|st, ctx| missing_dep(eq, order, dep, x, ctx, st)));
    tries.push(Box::new(|st, ctx| missing_x(eq, order, dep, x, ctx, st)));
    for t in tries.iter_mut() {
        if out_of_time() {
            return None;
        }
        ctx.made.truncate(made);
        if let Some(sol) = attempt(st, |st| t(st, ctx).filter(check)) {
            return Some(sol);
        }
    }
    ctx.made.truncate(made);
    None
}

/// y⁽ⁿ⁾ = f(x): integrate n times.
fn repeated_integration(
    eq: &Sym,
    order: usize,
    dep: &str,
    x: &str,
    ctx: &mut Ctx,
    st: &mut Steps,
) -> Option<Sol> {
    let top = dname(dep, order);
    let c = poly::coeffs(eq, &top)?;
    if c.len() != 2 || !free_of_dep(&c[0], dep) || !free_of_dep(&c[1], dep) {
        return None;
    }
    let mut cur = nice(&div(neg(c[0].clone()), c[1].clone()));
    st.push(
        format!(
            "Only ${}$ appears, so integrate {} times, adding a constant each time",
            name(&top),
            order
        ),
        eq_latex(&var(&top), &cur),
    );
    for k in (0..order).rev() {
        let r = if cur.is_zero() {
            n(0)
        } else {
            integrate(&cur, x, None, st)?
        };
        let c = ctx.constant();
        cur = nice(&add2(r, var(&c)));
        st.push("Integrate", eq_latex(&var(&dname(dep, k)), &cur));
    }
    Some(Sol::Explicit(vec![cur]))
}

#[derive(Clone, Debug)]
enum Root {
    Real(Sym, usize),
    /// α ± βi.
    Complex(Sym, Sym, usize),
}

fn strip_abs(s: &Sym) -> Sym {
    match s {
        Sym::Fn(Func::Abs, a) => strip_abs(a),
        Sym::Add(xs) => add(xs.iter().map(strip_abs).collect()),
        Sym::Mul(xs) => mul(xs.iter().map(strip_abs).collect()),
        Sym::Pow(a, b) => pow(strip_abs(a), strip_abs(b)),
        Sym::Fn(f, a) => func(*f, strip_abs(a)),
        other => other.clone(),
    }
}

/// The roots of Σ cₖ rᵏ with their multiplicities.
fn char_roots(c: &[Sym]) -> Option<Vec<Root>> {
    let mut c: Vec<Sym> = c.iter().map(nice).collect();
    while c.len() > 1 && c.last().is_some_and(Sym::is_zero) {
        c.pop();
    }
    let mut out = Vec::new();
    let zeros = c.iter().take_while(|k| k.is_zero()).count();
    if zeros > 0 {
        out.push(Root::Real(n(0), zeros));
        c.drain(..zeros);
    }
    if c.len() == 1 {
        return Some(out);
    }
    if let Some(p) = poly::rational(&c) {
        let f = poly::factor(&p)?;
        for (g, m) in &f.factors {
            let m = *m as usize;
            match g.len() {
                2 => out.push(Root::Real(frac(-g[0], g[1]), m)),
                3 => out.extend(quadratic_roots(&n(g[2]), &n(g[1]), &n(g[0]), m)),
                5 if g[1] == 0 && g[3] == 0 => {
                    out.extend(even_quartic_roots(&n(g[4]), &n(g[2]), &n(g[0]), m)?)
                }
                _ => {
                    // No exact roots: find them numerically.
                    let pf: Vec<f64> = g.iter().map(|&k| k as f64).collect();
                    for (re, im) in poly::numeric_roots(&pf) {
                        if im.abs() < 1e-12 {
                            out.push(Root::Real(Sym::Real(re), m));
                        } else if im > 0.0 {
                            out.push(Root::Complex(Sym::Real(re), Sym::Real(im), m));
                        }
                    }
                }
            }
        }
        return Some(out);
    }
    match c.len() {
        2 => out.push(Root::Real(nice(&div(neg(c[0].clone()), c[1].clone())), 1)),
        3 => out.extend(quadratic_roots(&c[2], &c[1], &c[0], 1)),
        5 if c[1].is_zero() && c[3].is_zero() => {
            out.extend(even_quartic_roots(&c[4], &c[2], &c[0], 1)?)
        }
        _ => return None,
    }
    Some(out)
}

/// a r⁴ + b r² + c, a quadratic in r².
fn even_quartic_roots(a: &Sym, b: &Sym, c: &Sym, m: usize) -> Option<Vec<Root>> {
    let mut out = Vec::new();
    for r in quadratic_roots(a, b, c, m) {
        match r {
            // r² = s.
            Root::Real(s, k) => {
                if s.is_negative() || (s.is_constant() && s.value() < 0.0) {
                    out.push(Root::Complex(n(0), nice(&strip_abs(&sqrt(neg(s)))), k));
                } else if s.is_zero() {
                    out.push(Root::Real(n(0), 2 * k));
                } else {
                    let q = nice(&strip_abs(&sqrt(s)));
                    out.push(Root::Real(q.clone(), k));
                    out.push(Root::Real(nice(&neg(q)), k));
                }
            }
            // No real r²: r⁴ + B r² + Q² = (r² + pr + Q)(r² - pr + Q) with
            // p = √(2Q - B).
            Root::Complex(..) => {
                let bb = div(b.clone(), a.clone());
                let q = nice(&sqrt(div(c.clone(), a.clone())));
                let p = nice(&sqrt(sub(mul2(n(2), q.clone()), bb)));
                out.extend(quadratic_roots(&n(1), &p, &q, m));
                out.extend(quadratic_roots(&n(1), &neg(p), &q, m));
                return Some(out);
            }
        }
    }
    Some(out)
}

fn quadratic_roots(a: &Sym, b: &Sym, c: &Sym, m: usize) -> Vec<Root> {
    let d = nice(&expand(&sub(
        powi(b.clone(), 2),
        mul(vec![n(4), a.clone(), c.clone()]),
    )));
    let two_a = mul2(n(2), a.clone());
    let alpha = nice(&div(neg(b.clone()), two_a.clone()));
    if d.is_zero() {
        return vec![Root::Real(alpha, 2 * m)];
    }
    let negative = if d.is_constant() {
        d.value() < 0.0
    } else {
        d.is_negative()
    };
    if negative {
        let beta = nice(&strip_abs(&div(sqrt(neg(d)), two_a)));
        vec![Root::Complex(alpha, beta, m)]
    } else {
        let s = sqrt(d);
        vec![
            Root::Real(nice(&div(sub(neg(b.clone()), s.clone()), two_a.clone())), m),
            Root::Real(nice(&div(add2(neg(b.clone()), s), two_a)), m),
        ]
    }
}

fn roots_latex(r: &str, roots: &[Root]) -> String {
    roots
        .iter()
        .map(|root| match root {
            Root::Real(s, m) => {
                let t = if *m > 1 {
                    format!("\\ \\left(\\text{{{} times}}\\right)", m)
                } else {
                    String::new()
                };
                format!("{r}={}{t}", latex(s))
            }
            Root::Complex(a, b, m) => {
                let bi = if b.is_one() {
                    "i".to_owned()
                } else {
                    format!("{}i", paren(b))
                };
                let t = if *m > 1 {
                    format!("\\ \\left(\\text{{{} times}}\\right)", m)
                } else {
                    String::new()
                };
                if a.is_zero() {
                    format!("{r}=\\pm {bi}{t}")
                } else {
                    format!("{r}={}\\pm {bi}{t}", latex(a))
                }
            }
        })
        .collect::<Vec<_>>()
        .join(",\\quad ")
}

/// The solutions a set of roots gives: `power(r)` is e^(rx) for constant
/// coefficients or x^r for Cauchy–Euler, `angle` is x or ln x, and
/// `extra` multiplies repeated roots (x or ln x).
fn basis(roots: &[Root], euler: bool, x: &str) -> (Vec<Sym>, Vec<String>) {
    let xv = var(x);
    let t = if euler {
        func(Func::Ln, xv.clone())
    } else {
        xv.clone()
    };
    let grow = |r: &Sym| -> Sym {
        if euler {
            pow(xv.clone(), r.clone())
        } else {
            exp(mul2(r.clone(), xv.clone()))
        }
    };
    let mut out = Vec::new();
    let mut notes = Vec::new();
    let tl = if euler {
        "\\ln x".replace('x', &name(x))
    } else {
        name(x)
    };
    for root in roots {
        match root {
            Root::Real(r, m) => {
                for j in 0..*m {
                    out.push(nice(&mul2(powi(t.clone(), j as i128), grow(r))));
                }
                let g = latex(&grow(r));
                notes.push(if *m == 1 {
                    format!("${}$ gives ${g}$", latex(r))
                } else {
                    format!(
                        "${}$, repeated {m} times, gives ${g}$ times $1,\\ {tl},\\ \\ldots,\\ \\left({tl}\\right)^{{{}}}$",
                        latex(r),
                        m - 1
                    )
                });
            }
            Root::Complex(a, b, m) => {
                let arg = mul2(b.clone(), t.clone());
                for j in 0..*m {
                    let k = powi(t.clone(), j as i128);
                    out.push(nice(&mul(vec![
                        k.clone(),
                        grow(a),
                        func(Func::Cos, arg.clone()),
                    ])));
                    out.push(nice(&mul(vec![k, grow(a), func(Func::Sin, arg.clone())])));
                }
                let pair = roots_latex("", &[Root::Complex(a.clone(), b.clone(), 1)]);
                notes.push(format!(
                    "the pair ${}$ gives ${}$ and ${}$",
                    pair.trim_start_matches('='),
                    latex(&nice(&mul2(grow(a), func(Func::Cos, arg.clone())))),
                    latex(&nice(&mul2(grow(a), func(Func::Sin, arg.clone()))))
                ));
            }
        }
    }
    (out, notes)
}

fn homogeneous_part(ys: &[Sym], ctx: &mut Ctx) -> Sym {
    add(ys
        .iter()
        .map(|y| mul2(var(&ctx.constant()), y.clone()))
        .collect())
}

/// Constant coefficients: the characteristic equation for the homogeneous
/// part, then undetermined coefficients or variation of parameters.
fn constant_coefficients(
    a: &[Sym],
    g: &Sym,
    dep: &str,
    x: &str,
    ctx: &mut Ctx,
    st: &mut Steps,
) -> Option<Sol> {
    if a.iter().any(|c| c.has(x)) {
        return None;
    }
    let order = a.len() - 1;
    let r = if ctx.avoid.iter().any(|n| n == "r") {
        "m"
    } else {
        "r"
    };
    st.push(
        "This is linear with constant coefficients. First solve it with the right side set to zero",
        format!("{}=0", operator_latex(a, dep)),
    );
    let char_poly = poly::build(a, r);
    st.push(
        format!(
            "Try ${}=e^{{{}{}}}$: each derivative brings down a factor ${}$, which gives the characteristic equation",
            name(dep),
            r,
            name(x),
            r
        ),
        format!("{}=0", latex(&char_poly)),
    );
    let roots = char_roots(a)?;
    st.push("Its roots are", roots_latex(r, &roots));
    let (ys, notes) = basis(&roots, false, x);
    if ys.len() != order {
        return None;
    }
    let yh = homogeneous_part(&ys, ctx);
    st.push(
        format!("Each root gives a solution: {}. So", notes.join("; ")),
        format!("{}_{{h}}={}", name(dep), latex(&yh)),
    );
    finish_linear(a, g, &roots, &ys, yh, false, dep, x, st)
}

#[allow(clippy::too_many_arguments)]
fn finish_linear(
    a: &[Sym],
    g: &Sym,
    roots: &[Root],
    ys: &[Sym],
    yh: Sym,
    euler: bool,
    dep: &str,
    x: &str,
    st: &mut Steps,
) -> Option<Sol> {
    if g.is_zero() {
        return Some(Sol::Explicit(vec![yh]));
    }
    let yp = (!euler)
        .then(|| attempt_sym(st, |st| undetermined(a, g, roots, dep, x, st)))
        .flatten()
        .or_else(|| {
            if ys.len() == 2 {
                let std = nice(&div(g.clone(), a[2].clone()));
                attempt_sym(st, |st| variation(&ys[0], &ys[1], &std, dep, x, st))
            } else {
                None
            }
        })?;
    let y = add2(yh.clone(), yp.clone());
    st.push(
        "The general solution is the homogeneous part plus the particular one",
        format!(
            "{}={}_{{h}}+{}_{{p}}={}",
            name(dep),
            name(dep),
            name(dep),
            latex(&y)
        ),
    );
    Some(Sol::Explicit(vec![y]))
}

fn attempt_sym(st: &mut Steps, f: impl FnOnce(&mut Steps) -> Option<Sym>) -> Option<Sym> {
    let mark = st.mark();
    let r = f(st);
    if r.is_none() {
        st.undo(mark);
    }
    r
}

/// sinh and cosh as exponentials, so they can be matched by e^(ax).
fn hyperbolic_to_exp(s: &Sym) -> Sym {
    match s {
        Sym::Fn(f @ (Func::Sinh | Func::Cosh), u) => {
            let u = hyperbolic_to_exp(u);
            let (p, m) = (exp(u.clone()), exp(neg(u)));
            let sign = if *f == Func::Sinh { neg(m) } else { m };
            div(add2(p, sign), n(2))
        }
        Sym::Add(xs) => add(xs.iter().map(hyperbolic_to_exp).collect()),
        Sym::Mul(xs) => mul(xs.iter().map(hyperbolic_to_exp).collect()),
        Sym::Pow(a, b) => pow(hyperbolic_to_exp(a), hyperbolic_to_exp(b)),
        Sym::Fn(f, a) => func(*f, hyperbolic_to_exp(a)),
        other => other.clone(),
    }
}

/// a term c·xᵐ·e^(αx)·[cos or sin](βx): (m, α, β).
fn uc_shape(t: &Sym, x: &str) -> Option<(usize, Sym, Sym)> {
    let mut m = 0usize;
    let mut alpha = n(0);
    let mut beta: Option<Sym> = None;
    for f in t.factors() {
        if !f.has(x) {
            continue;
        }
        match &f {
            Sym::Var(v) if v == x => m += 1,
            Sym::Pow(b, e) if matches!(&**b, Sym::Var(v) if v == x) => {
                let q = e.as_q()?;
                if !q.is_int() || q.n < 0 || q.n > 12 {
                    return None;
                }
                m += q.n as usize;
            }
            Sym::Pow(b, e) if **b == var("e") => {
                let c = poly::coeffs(e, x)?;
                if c.len() != 2 || c[1].has(x) {
                    return None;
                }
                alpha = add2(alpha, c[1].clone());
            }
            Sym::Fn(Func::Sin | Func::Cos, u) if beta.is_none() => {
                let c = poly::coeffs(u, x)?;
                if c.len() != 2 || !c[0].is_zero() {
                    return None;
                }
                beta = Some(c[1].clone());
            }
            _ => return None,
        }
    }
    Some((m, nice(&alpha), beta.map(|b| nice(&b)).unwrap_or(n(0))))
}

const UNKNOWNS: [&str; 12] = ["A", "B", "D", "E", "F", "G", "H", "J", "K", "L", "M", "N"];

/// Undetermined coefficients: guess a particular solution shaped like the
/// right side, put it in, and match the coefficients.
fn undetermined(
    a: &[Sym],
    g: &Sym,
    roots: &[Root],
    dep: &str,
    x: &str,
    st: &mut Steps,
) -> Option<Sym> {
    let g = expand(&hyperbolic_to_exp(g));
    // (α, β) → highest power of x.
    let mut groups: Vec<(Sym, Sym, usize)> = Vec::new();
    for t in g.terms() {
        let (m, al, be) = uc_shape(&t, x)?;
        match groups
            .iter_mut()
            .find(|(a2, b2, _)| same(a2, &al) && same(b2, &be))
        {
            Some(gr) => gr.2 = gr.2.max(m),
            None => groups.push((al, be, m)),
        }
    }
    let xv = var(x);
    let mut used: Vec<String> = a.iter().chain([&g]).flat_map(|s| s.names()).collect();
    used.push(dep.into());
    let mut names = UNKNOWNS
        .iter()
        .filter(|u| !used.iter().any(|n| n == *u))
        .map(|s| s.to_string());
    let mut unknowns: Vec<String> = Vec::new();
    let mut guess = Vec::new();
    let mut bumped = Vec::new();
    for (al, be, m) in &groups {
        let mult: usize = roots
            .iter()
            .map(|r| match r {
                Root::Real(r, k) if be.is_zero() && same(r, al) => *k,
                Root::Complex(ra, rb, k)
                    if !be.is_zero()
                        && same(ra, al)
                        && (same(rb, be) || same(rb, &neg(be.clone()))) =>
                {
                    *k
                }
                _ => 0,
            })
            .sum();
        let e = exp(mul2(al.clone(), xv.clone()));
        let kinds: Vec<Sym> = if be.is_zero() {
            vec![n(1)]
        } else {
            let arg = mul2(be.clone(), xv.clone());
            vec![func(Func::Cos, arg.clone()), func(Func::Sin, arg)]
        };
        let mut part = Vec::new();
        for kind in &kinds {
            let mut poly_terms = Vec::new();
            for j in 0..=*m {
                let u = names.next()?;
                poly_terms.push(mul2(var(&u), powi(xv.clone(), j as i128)));
                unknowns.push(u);
            }
            part.push(mul(vec![add(poly_terms), e.clone(), kind.clone()]));
        }
        let piece = mul2(powi(xv.clone(), mult as i128), add(part));
        if mult > 0 {
            bumped.push((mult, latex(&mul2(e, kinds[0].clone()))));
        }
        guess.push(piece);
    }
    let guess = add(guess);
    let mut note =
        "Guess a particular solution shaped like the right side, with unknown coefficients"
            .to_owned();
    for (k, f) in &bumped {
        note.push_str(&format!(
            ". Because ${f}$ already solves the equation with zero on the right, multiply its part by ${}$",
            latex(&powi(xv.clone(), *k as i128))
        ));
    }
    st.push(note, format!("{}_{{p}}={}", name(dep), latex(&guess)));
    // L[guess] - g, collected by the kind of term.
    let mut lhs = Vec::new();
    let mut d = guess.clone();
    for (k, c) in a.iter().enumerate() {
        if k > 0 {
            d = derivative(&d, x);
        }
        if !c.is_zero() {
            lhs.push(mul2(c.clone(), d.clone()));
        }
    }
    let residual = expand(&sub(add(lhs), g.clone()));
    let mut keyed: Vec<(Sym, Sym)> = Vec::new();
    for t in residual.terms() {
        let (with_x, without): (Vec<Sym>, Vec<Sym>) =
            t.factors().into_iter().partition(|f| f.has(x));
        let key = mul(with_x);
        let coef = mul(without);
        match keyed.iter_mut().find(|(k, _)| *k == key) {
            Some(p) => p.1 = add2(p.1.clone(), coef),
            None => keyed.push((key, coef)),
        }
    }
    let zero_args: Vec<(String, Sym)> = unknowns.iter().map(|u| (u.clone(), n(0))).collect();
    let mut rows = Vec::new();
    let mut shown = Vec::new();
    for (_, c) in &keyed {
        let c = nice(c);
        if c.is_zero() {
            continue;
        }
        let mut row: Vec<Sym> = unknowns.iter().map(|u| nice(&derivative(&c, u))).collect();
        let constant = zero_args
            .iter()
            .fold(c.clone(), |s, (u, z)| s.subs_var(u, z));
        let rest = nice(&sub(c.clone(), constant.clone()));
        shown.push(eq_latex(&rest, &nice(&neg(constant.clone()))));
        row.push(nice(&neg(constant)));
        rows.push(row);
    }
    st.push(
        "Put the guess into the equation and match the coefficients of each kind of term",
        shown.join(",\\quad "),
    );
    let values = solve_linear_sym(rows, unknowns.len())?;
    st.push(
        "Solve for the unknowns",
        unknowns
            .iter()
            .zip(&values)
            .map(|(u, v)| format!("{u}={}", latex(v)))
            .collect::<Vec<_>>()
            .join(",\\ "),
    );
    let mut yp = guess;
    for (u, v) in unknowns.iter().zip(&values) {
        yp = yp.subs_var(u, v);
    }
    let yp = nice(&yp);
    st.push("So", format!("{}_{{p}}={}", name(dep), latex(&yp)));
    Some(yp)
}

/// Variation of parameters for y'' + p y' + q y = g with known y₁, y₂.
fn variation(y1: &Sym, y2: &Sym, g: &Sym, dep: &str, x: &str, st: &mut Steps) -> Option<Sym> {
    let w = nice(&expand(&sub(
        mul2(y1.clone(), derivative(y2, x)),
        mul2(derivative(y1, x), y2.clone()),
    )));
    if zero_everywhere(&w) {
        return None;
    }
    st.push(
        format!(
            "Find a particular solution by variation of parameters. Divide through by the coefficient of ${}''$ so the right side is $g={}$; the Wronskian of $y_1={}$ and $y_2={}$ is",
            name(dep),
            latex(g),
            latex(y1),
            latex(y2)
        )
        .replace("y_1", &format!("{}_1", name(dep)))
        .replace("y_2", &format!("{}_2", name(dep))),
        format!("W={}{}'-{}'{}={}", paren(y1), paren(y2), paren(y1), paren(y2), latex(&w)),
    );
    let i1 = nice(&div(mul2(y2.clone(), g.clone()), w.clone()));
    let i2 = nice(&div(mul2(y1.clone(), g.clone()), w.clone()));
    let u1 = integrate(
        &i1,
        x,
        Some("Integrate $y_2 g/W$".replace('y', &name(dep))),
        st,
    )?;
    let u2 = integrate(
        &i2,
        x,
        Some("Integrate $y_1 g/W$".replace('y', &name(dep))),
        st,
    )?;
    let yp = nice(&expand(&sub(mul2(u2, y2.clone()), mul2(u1, y1.clone()))));
    // sec x sin²x - sin x tan x is 0 once written with sin and cos.
    let plain = nice(&expand(&super::trigint::to_sin_cos(&yp)));
    let yp = if latex(&plain).len() < latex(&yp).len() {
        plain
    } else {
        yp
    };
    st.push(
        format!(
            "Then ${}_{{p}}=-{}_1\\int\\frac{{{}_2 g}}{{W}}+{}_2\\int\\frac{{{}_1 g}}{{W}}$",
            name(dep),
            name(dep),
            name(dep),
            name(dep),
            name(dep)
        ),
        format!("{}_{{p}}={}", name(dep), latex(&yp)),
    );
    Some(yp)
}

/// aₖ = cₖ xᵏ: try y = x^r.
fn cauchy_euler(
    a: &[Sym],
    g: &Sym,
    dep: &str,
    x: &str,
    ctx: &mut Ctx,
    st: &mut Steps,
) -> Option<Sol> {
    let xv = var(x);
    let c: Vec<Sym> = a
        .iter()
        .enumerate()
        .map(|(k, ak)| nice(&div(ak.clone(), powi(xv.clone(), k as i128))))
        .collect();
    if c.iter().any(|k| k.has(x)) || a.iter().all(|k| !k.has(x)) {
        return None;
    }
    let r = if ctx.avoid.iter().any(|n| n == "r") {
        "m"
    } else {
        "r"
    };
    let rv = var(r);
    // Σ cₖ r(r-1)…(r-k+1).
    let mut char_poly = Vec::new();
    for (k, ck) in c.iter().enumerate() {
        let falling = mul((0..k).map(|j| sub(rv.clone(), n(j as i128))).collect());
        char_poly.push(mul2(ck.clone(), falling));
    }
    let char_poly = expand(&add(char_poly));
    let cc = poly::coeffs(&char_poly, r)?;
    st.push(
        format!(
            "Each ${}^{{(k)}}$ is multiplied by ${}^k$, so this is a Cauchy–Euler equation. Try ${}={}^{{{}}}$, which gives",
            name(dep),
            name(x),
            name(dep),
            name(x),
            r
        ),
        format!("{}=0", latex(&char_poly)),
    );
    let roots = char_roots(&cc)?;
    st.push("Its roots are", roots_latex(r, &roots));
    let (ys, notes) = basis(&roots, true, x);
    if ys.len() != a.len() - 1 {
        return None;
    }
    let yh = homogeneous_part(&ys, ctx);
    st.push(
        format!("Each root gives a solution: {}. So", notes.join("; ")),
        format!("{}_{{h}}={}", name(dep), latex(&yh)),
    );
    finish_linear(a, g, &roots, &ys, yh, true, dep, x, st)
}

/// Renames the derivatives of `dep` by `shift`: y'' → p' for shift 1.
fn rename_derivs(eq: &Sym, dep: &str, to: &str, shift: usize, order: usize) -> Sym {
    let mut out = eq.clone();
    for k in shift..=order {
        out = out.subs_var(&dname(dep, k), &var(&format!("\u{4}{}", k - shift)));
    }
    for k in 0..=order - shift {
        out = out.subs_var(&format!("\u{4}{k}"), &var(&dname(to, k)));
    }
    out
}

/// No y itself: p = y⁽ᵏ⁾ lowers the order.
fn missing_dep(
    eq: &Sym,
    order: usize,
    dep: &str,
    x: &str,
    ctx: &mut Ctx,
    st: &mut Steps,
) -> Option<Sol> {
    let lowest = (0..=order).find(|&k| eq.has(&dname(dep, k)))?;
    if lowest == 0 || lowest == order {
        return None;
    }
    let p = if dep == "p" { "q" } else { "p" };
    let reduced = rename_derivs(eq, dep, p, lowest, order);
    let new_order = order - lowest;
    st.push(
        format!(
            "There's no ${}$ itself, so let ${}={}$. That lowers the order to {}",
            name(dep),
            name(p),
            name(&dname(dep, lowest)),
            new_order
        ),
        format!("{}=0", latex(&reduced)),
    );
    let sol = if new_order == 1 {
        first_order(&reduced, p, x, ctx, st)?
    } else {
        higher_order(&reduced, new_order, p, x, ctx, st)?
    };
    let Sol::Explicit(ps) = sol else { return None };
    let mut cur = ps.into_iter().next()?;
    st.push(
        format!(
            "So ${}={}$; integrate {} times to get ${}$",
            name(&dname(dep, lowest)),
            latex(&cur),
            lowest,
            name(dep)
        ),
        "",
    );
    for k in (0..lowest).rev() {
        let r = integrate(&cur, x, None, st)?;
        let c = ctx.constant();
        cur = nice(&add2(r, var(&c)));
        st.push("Integrate", eq_latex(&var(&dname(dep, k)), &cur));
    }
    Some(Sol::Explicit(vec![cur]))
}

/// No x, second order: p(y) = y' turns y'' into p dp/dy.
fn missing_x(
    eq: &Sym,
    order: usize,
    dep: &str,
    x: &str,
    ctx: &mut Ctx,
    st: &mut Steps,
) -> Option<Sol> {
    if order != 2 || eq.has(x) || !eq.has(dep) {
        return None;
    }
    let p = if dep == "p" { "q" } else { "p" };
    let pv = var(p);
    let reduced = eq
        .subs_var(&dname(dep, 2), &mul2(pv.clone(), var(&dname(p, 1))))
        .subs_var(&dname(dep, 1), &pv);
    let reduced = nice(&reduced);
    st.push(
        format!(
            "There's no ${}$, so let ${}={}'$ as a function of ${}$: then ${}''={}\\frac{{d{}}}{{d{}}}$",
            name(x),
            name(p),
            name(dep),
            name(dep),
            name(dep),
            name(p),
            name(p),
            name(dep)
        ),
        format!("{}=0", latex(&reduced)),
    );
    let Sol::Explicit(ps) = first_order(&reduced, p, dep, ctx, st)? else {
        return None;
    };
    let pf = ps.into_iter().next()?;
    st.push(format!("Now solve ${}'={}$", name(dep), latex(&pf)), "");
    let next = sub(var(&dname(dep, 1)), pf);
    first_order(&next, dep, x, ctx, st)
}

// ---- Systems -----------------------------------------------------------------------

/// x' = ax + by, y' = cx + dy, by the eigenvalues of [[a, b], [c, d]].
pub fn solve_system(
    f: &Sym,
    g: &Sym,
    start: Option<(Sym, Sym)>,
    st: &mut Steps,
) -> Result<(String, Option<String>), String> {
    let fail = || {
        "Solve only solves systems that are linear with constant coefficients, like x' = x - y, y' = x + y".to_owned()
    };
    let lin = |e: &Sym| -> Option<(Sym, Sym)> {
        let cx = poly::coeffs(e, "x")?;
        if cx.len() > 2 {
            return None;
        }
        let rest = cx[0].clone();
        let cy = poly::coeffs(&rest, "y")?;
        if cy.len() > 2 || !cy[0].is_zero() {
            return None;
        }
        let a = cx.get(1).cloned().unwrap_or(n(0));
        let b = cy.get(1).cloned().unwrap_or(n(0));
        if a.has("x") || a.has("y") || a.has("t") || b.has("x") || b.has("y") || b.has("t") {
            return None;
        }
        Some((nice(&a), nice(&b)))
    };
    let (a, b) = lin(f).ok_or_else(fail)?;
    let (c, d) = lin(g).ok_or_else(fail)?;
    let t = var("t");
    st.push(
        "This is a linear system with constant coefficients: find the eigenvalues of its matrix",
        format!(
            "\\left(x',y'\\right)=\\left({},{}\\right)",
            latex(&add2(mul2(a.clone(), var("x")), mul2(b.clone(), var("y")))),
            latex(&add2(mul2(c.clone(), var("x")), mul2(d.clone(), var("y"))))
        ),
    );
    let tr = nice(&add2(a.clone(), d.clone()));
    let det = nice(&sub(mul2(a.clone(), d.clone()), mul2(b.clone(), c.clone())));
    let lv = var("lambda");
    let char_poly = add(vec![
        powi(lv.clone(), 2),
        neg(mul2(tr.clone(), lv)),
        det.clone(),
    ]);
    st.push(
        "The characteristic equation is $\\lambda^2-(a+d)\\lambda+(ad-bc)=0$",
        format!("{}=0", latex(&char_poly)),
    );
    let roots = quadratic_roots(&n(1), &neg(tr), &det, 1);
    st.push("Its roots are", roots_latex("\\lambda", &roots));
    let c1 = var("C_1");
    let c2 = var("C_2");
    // An eigenvector for λ: (b, λ - a), or (λ - d, c).
    let vector = |l: &Sym| -> (Sym, Sym) {
        if !b.is_zero() {
            (b.clone(), nice(&sub(l.clone(), a.clone())))
        } else if !c.is_zero() {
            (nice(&sub(l.clone(), d.clone())), c.clone())
        } else if same(l, &a) {
            (n(1), n(0))
        } else {
            (n(0), n(1))
        }
    };
    let vl = |v: &(Sym, Sym)| format!("\\left({},{}\\right)", latex(&v.0), latex(&v.1));
    let (xs, ys) = match roots.as_slice() {
        [Root::Real(l1, 1), Root::Real(l2, 1)] => {
            let (v1, v2) = (vector(l1), vector(l2));
            let (v1, v2) = if b.is_zero() && c.is_zero() {
                (
                    if same(l1, &a) {
                        (n(1), n(0))
                    } else {
                        (n(0), n(1))
                    },
                    if same(l2, &a) {
                        (n(1), n(0))
                    } else {
                        (n(0), n(1))
                    },
                )
            } else {
                (v1, v2)
            };
            st.push(
                "Each eigenvalue $\\lambda$ has an eigenvector $v$, and $e^{\\lambda t}v$ is a solution",
                format!("\\lambda={}:\\ v={},\\quad \\lambda={}:\\ v={}", latex(l1), vl(&v1), latex(l2), vl(&v2)),
            );
            let e1 = exp(mul2(l1.clone(), t.clone()));
            let e2 = exp(mul2(l2.clone(), t.clone()));
            (
                add2(
                    mul(vec![c1.clone(), v1.0, e1.clone()]),
                    mul(vec![c2.clone(), v2.0, e2.clone()]),
                ),
                add2(
                    mul(vec![c1.clone(), v1.1, e1]),
                    mul(vec![c2.clone(), v2.1, e2]),
                ),
            )
        }
        [Root::Real(l, 2)] => {
            let e = exp(mul2(l.clone(), t.clone()));
            if b.is_zero() && c.is_zero() {
                st.push(
                    "The matrix is a multiple of the identity, so each part grows on its own",
                    "",
                );
                (mul2(c1.clone(), e.clone()), mul2(c2.clone(), e))
            } else {
                let v = vector(l);
                // (A - λI) w = v.
                let w = if !b.is_zero() {
                    (n(0), n(1))
                } else {
                    (n(1), n(0))
                };
                let scale = if !b.is_zero() { b.clone() } else { c.clone() };
                let w = (div(w.0, scale.clone()), div(w.1, scale));
                let v = (v.0, v.1);
                let (w0, w1) = (nice(&w.0), nice(&w.1));
                st.push(
                    "The eigenvalue is repeated: with eigenvector $v$ and $w$ solving $(A-\\lambda I)w=v$, the solutions are $e^{\\lambda t}v$ and $e^{\\lambda t}(tv+w)$",
                    format!("v={},\\ w={}", vl(&v), vl(&(w0.clone(), w1.clone()))),
                );
                (
                    mul2(
                        e.clone(),
                        add2(
                            mul2(c1.clone(), v.0.clone()),
                            mul2(c2.clone(), add2(mul2(t.clone(), v.0), w0)),
                        ),
                    ),
                    mul2(
                        e,
                        add2(
                            mul2(c1.clone(), v.1.clone()),
                            mul2(c2.clone(), add2(mul2(t.clone(), v.1), w1)),
                        ),
                    ),
                )
            }
        }
        [Root::Complex(al, be, 1)] => {
            // v = (b, α - a + βi) = p + qi.
            let (p, q) = if !b.is_zero() {
                (
                    (b.clone(), nice(&sub(al.clone(), a.clone()))),
                    (n(0), be.clone()),
                )
            } else {
                (
                    (nice(&sub(al.clone(), d.clone())), c.clone()),
                    (be.clone(), n(0)),
                )
            };
            st.push(
                "The eigenvalues are complex, $\\alpha\\pm\\beta i$, with eigenvector $p+qi$; the real solutions are $e^{\\alpha t}(p\\cos\\beta t-q\\sin\\beta t)$ and $e^{\\alpha t}(p\\sin\\beta t+q\\cos\\beta t)$",
                format!("p={},\\ q={}", vl(&p), vl(&q)),
            );
            let e = exp(mul2(al.clone(), t.clone()));
            let co = func(Func::Cos, mul2(be.clone(), t.clone()));
            let si = func(Func::Sin, mul2(be.clone(), t.clone()));
            let part = |pk: &Sym, qk: &Sym| {
                mul2(
                    e.clone(),
                    add2(
                        mul2(
                            c1.clone(),
                            sub(mul2(pk.clone(), co.clone()), mul2(qk.clone(), si.clone())),
                        ),
                        mul2(
                            c2.clone(),
                            add2(mul2(pk.clone(), si.clone()), mul2(qk.clone(), co.clone())),
                        ),
                    ),
                )
            };
            (part(&p.0, &q.0), part(&p.1, &q.1))
        }
        _ => return Err(fail()),
    };
    let (mut xs, mut ys) = (nice(&expand(&xs)), nice(&expand(&ys)));
    // Check: x' = f(x, y), y' = g(x, y).
    for (lhs, rhs) in [(derivative(&xs, "t"), f), (derivative(&ys, "t"), g)] {
        let r = sub(
            lhs,
            rhs.subs_var("x", &var("\u{5}"))
                .subs_var("y", &ys)
                .subs_var("\u{5}", &xs),
        );
        if !zero_everywhere(&r) {
            return Err(fail());
        }
    }
    let pair = |xs: &Sym, ys: &Sym| format!("\\left({},{}\\right)", latex(xs), latex(ys));
    st.push(
        "So the general solution is, for any constants $C_1$, $C_2$",
        format!("\\left(x,y\\right)={}", pair(&xs, &ys)),
    );
    if let Some((x0, y0)) = start {
        let at0 = |s: &Sym| nice(&s.subs_var("t", &n(0)));
        let (ex, ey) = (at0(&xs), at0(&ys));
        st.push(
            "Start at the initial point when $t=0$",
            format!(
                "{}={},\\ {}={}",
                latex(&ex),
                latex(&x0),
                latex(&ey),
                latex(&y0)
            ),
        );
        let row = |e: &Sym, v: &Sym| {
            vec![
                nice(&derivative(e, "C_1")),
                nice(&derivative(e, "C_2")),
                nice(v),
            ]
        };
        if let Some(vals) = solve_linear_sym(vec![row(&ex, &x0), row(&ey, &y0)], 2) {
            st.push(
                "So",
                format!("C_{{1}}={},\\ C_{{2}}={}", latex(&vals[0]), latex(&vals[1])),
            );
            let put = |s: &Sym| {
                nice(&expand(
                    &s.subs_var("C_1", &vals[0]).subs_var("C_2", &vals[1]),
                ))
            };
            xs = put(&xs);
            ys = put(&ys);
            st.push(
                format!(
                    "The trajectory from $\\left({},{}\\right)$ is",
                    latex(&x0),
                    latex(&y0)
                ),
                pair(&xs, &ys),
            );
        }
    }
    let shown = pair(&xs, &ys);
    Ok((format!("\\left(x,y\\right)={shown}"), Some(shown)))
}
