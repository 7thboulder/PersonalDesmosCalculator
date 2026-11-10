//! Limits with working: substitution, factoring and cancelling,
//! L'Hôpital's rule, and dividing by the highest power at infinity. Each
//! answer is checked against the engine's numeric limit before it is
//! shown; anything this can't do goes to Giac.

use super::convert::{to_sym, Defs};
use super::diff::derivative;
use super::latex::{latex, name};
use super::sym::*;
use super::{cancel, nice, poly, Steps};
use crate::ast::{Expr, Statement};

/// Where the variable goes.
#[derive(Clone)]
enum Point {
    At(Sym),
    /// +∞ (1) or -∞ (-1).
    Infinity(i8),
}

/// The pieces of `\lim_{x\to a^{+}} body`: variable, point, side, body.
pub(crate) fn split(latex: &str) -> Option<(String, String, i8, String)> {
    crate::cas::split_limit(latex)
}

/// How sure an answer is: a value, ±∞, or none.
#[derive(Clone, Debug, PartialEq)]
enum Answer {
    Value(Sym),
    Infinite(i8),
}

fn answer_latex(a: &Answer) -> String {
    match a {
        Answer::Value(s) => latex(s),
        Answer::Infinite(1) => "\\infty".into(),
        Answer::Infinite(_) => "-\\infty".into(),
    }
}

/// The limit with working, or Err when this can't do it (and Giac should).
pub fn solve(
    source: &str,
    others: &[String],
    defs: &Defs,
    st: &mut Steps,
) -> Result<String, String> {
    let (v_latex, point_latex, side, body_latex) =
        split(source).ok_or("This isn't a limit Solve can read")?;
    let v = match crate::parse(&v_latex) {
        Ok(Statement::Expr(Expr::Var(v))) => v,
        _ => return Err("A limit needs a variable".into()),
    };
    let body = match crate::parse(&body_latex)? {
        Statement::Expr(e) => to_sym(&e, defs)?,
        _ => return Err("Take the limit of an expression".into()),
    };
    let point = match super::convert::infinity(&match crate::parse(&point_latex)? {
        Statement::Expr(e) => e,
        _ => return Err("A limit needs a point".into()),
    }) {
        Some(inf) => Point::Infinity(if inf > 0.0 { 1 } else { -1 }),
        None => match crate::parse(&point_latex)? {
            Statement::Expr(e) => {
                let p = to_sym(&e, defs)?;
                if !p.is_constant() {
                    return Err("The point must be a number".into());
                }
                Point::At(p)
            }
            _ => return Err("A limit needs a point".into()),
        },
    };
    let mark = st.mark();
    let found = match &point {
        Point::At(a) => at_point(&body, &v, a, side, st),
        Point::Infinity(s) => at_infinity(&body, &v, *s, st),
    };
    let Some(answer) = found else {
        st.undo(mark);
        return Err("Solve can't take this limit with steps".into());
    };
    // The engine's own numeric limit must agree.
    if !agrees(source, &answer, others) {
        st.undo(mark);
        return Err("Solve's working didn't match the numeric limit".into());
    }
    let shown = answer_latex(&answer);
    st.push("So", format!("{source}={shown}"));
    st.push("Checked numerically against the limit itself", "");
    Ok(shown)
}

fn agrees(source: &str, answer: &Answer, others: &[String]) -> bool {
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
        latex: source.to_owned(),
        ..Default::default()
    });
    let out = std::panic::catch_unwind(|| crate::analyze(&rows)).unwrap_or_default();
    let Some(crate::Value::Number(n)) = out.last().and_then(|r| r.value.clone()) else {
        return false;
    };
    match answer {
        Answer::Value(s) => {
            let want = s.value();
            want.is_finite() && (n - want).abs() <= 1e-6 * (1.0 + want.abs())
        }
        Answer::Infinite(sign) => n.is_infinite() && (n > 0.0) == (*sign > 0),
    }
}

/// The value of `s` at x = a, when it is a finite number.
fn value_at(s: &Sym, v: &str, a: &Sym) -> Option<Sym> {
    let t = nice(&s.subs_var(v, a));
    let x = t.value();
    x.is_finite().then_some(t)
}

fn at_point(body: &Sym, v: &str, a: &Sym, side: i8, st: &mut Steps) -> Option<Answer> {
    let x = name(v);
    let mut f = body.clone();
    for round in 0..5 {
        // Continuous here: put the point in.
        let (top, bottom) = num_den(&f);
        if let Some(val) = value_at(&f, v, a) {
            let fine = [-1e-7, 1e-7].iter().all(|h| {
                (f.at(v, a.value() + h) - val.value()).abs() < 1e-4 * (1.0 + val.value().abs())
            });
            if fine {
                st.push(
                    if round == 0 {
                        format!(
                            "The function is continuous at ${x}={}$, so put the point in",
                            latex(a)
                        )
                    } else {
                        format!("Now put ${x}={}$ in", latex(a))
                    },
                    latex(&val),
                );
                return Some(Answer::Value(val));
            }
        }
        let tv = nice(&top.subs_var(v, a)).value();
        let bv = nice(&bottom.subs_var(v, a)).value();
        if bv == 0.0 && tv.is_finite() && tv != 0.0 {
            // c/0: the sides head off to ±∞.
            let near = |h: f64| f.at(v, a.value() + h);
            let (l, r) = (near(-1e-9), near(1e-9));
            let sign = |y: f64| if y > 0.0 { 1 } else { -1 };
            let one = match side {
                1 => Some(sign(r)),
                -1 => Some(sign(l)),
                _ => (sign(l) == sign(r)).then(|| sign(r)),
            };
            st.push(
                format!(
                    "Putting ${x}={}$ in gives ${}$ over $0$, so the function grows without bound",
                    latex(a),
                    crate::format::value_latex(tv)
                ),
                "",
            );
            return match one {
                Some(s) => {
                    st.push(
                        if s > 0 {
                            "It is positive on that side"
                        } else {
                            "It is negative on that side"
                        },
                        "",
                    );
                    Some(Answer::Infinite(s))
                }
                None => None,
            };
        }
        if !(tv == 0.0 && bv == 0.0) {
            return None;
        }
        // 0/0: cancel a common factor if there is one, else L'Hôpital.
        if let Some((c, g)) = cancel(&f, v) {
            st.push(
                format!(
                    "Putting ${x}={}$ in gives $\\frac{{0}}{{0}}$. Factor and cancel ${}$, which is zero there",
                    latex(a),
                    latex(&g)
                ),
                format!("{}={}", latex(&f), latex(&c)),
            );
            f = c;
            continue;
        }
        let (dt, db) = (nice(&derivative(&top, v)), nice(&derivative(&bottom, v)));
        st.push(
            format!(
                "Putting ${x}={}$ in gives $\\frac{{0}}{{0}}$, so use L'Hôpital's rule: differentiate the top and the bottom",
                latex(a)
            ),
            format!(
                "\\lim \\frac{{{}}}{{{}}}=\\lim \\frac{{{}}}{{{}}}",
                latex(&top),
                latex(&bottom),
                latex(&dt),
                latex(&db)
            ),
        );
        f = div(dt, db);
    }
    None
}

fn at_infinity(body: &Sym, v: &str, sign: i8, st: &mut Steps) -> Option<Answer> {
    let x = name(v);
    let (top, bottom) = num_den(body);
    let far = sign as f64 * 1e8;
    // A rational function: compare the highest powers.
    if let (Some(pt), Some(pb)) = (poly::coeffs(&top, v), poly::coeffs(&bottom, v)) {
        let (m, n) = (pt.len() - 1, pb.len() - 1);
        let (lt, lb) = (pt[m].clone(), pb[n].clone());
        if !lt.is_constant() || !lb.is_constant() {
            return None;
        }
        let power = pow(var(v), Sym::Num(super::sym::Q::new(n as i128, 1)?));
        let lead = nice(&div(lt.clone(), lb.clone()));
        if n > 0 {
            st.push(
                format!(
                    "Divide the top and the bottom by ${}$, the highest power on the bottom",
                    latex(&power)
                ),
                "",
            );
        }
        if m < n {
            st.push(
                format!("Every term on top then has ${x}$ in the bottom, so the top goes to $0$ while the bottom goes to ${}$", latex(&lb)),
                "",
            );
            return Some(Answer::Value(n_zero()));
        }
        if m == n {
            st.push(
                "Only the highest powers' coefficients are left",
                format!(
                    "\\frac{{{}}}{{{}}}={}",
                    latex(&mul2(
                        lt.clone(),
                        pow(var(v), Sym::Num(super::sym::Q::new(m as i128, 1)?))
                    )),
                    latex(&mul2(lb.clone(), power.clone())),
                    latex(&lead)
                ),
            );
            return Some(Answer::Value(lead));
        }
        // m > n: grows without bound, with the sign of lead·sign^(m-n).
        let s = lead.value().signum() * if (m - n) % 2 == 1 { sign as f64 } else { 1.0 };
        st.push(
            "The top has the higher power, so it grows without bound",
            "",
        );
        return Some(Answer::Infinite(if s > 0.0 { 1 } else { -1 }));
    }
    // ∞/∞ beyond polynomials: L'Hôpital, a few times.
    let mut f = body.clone();
    for _ in 0..4 {
        let (t, b) = num_den(&f);
        let (tv, bv) = (t.at(v, far), b.at(v, far));
        if b.has(v) && tv.abs() > 1e6 && bv.abs() > 1e6 {
            let (dt, db) = (nice(&derivative(&t, v)), nice(&derivative(&b, v)));
            st.push(
                "Both the top and the bottom grow without bound ($\\frac{\\infty}{\\infty}$), so use L'Hôpital's rule",
                format!(
                    "\\lim \\frac{{{}}}{{{}}}=\\lim \\frac{{{}}}{{{}}}",
                    latex(&t),
                    latex(&b),
                    latex(&dt),
                    latex(&db)
                ),
            );
            f = div(dt, db);
            continue;
        }
        // A constant over something growing goes to 0.
        if !t.has(v) && bv.abs() > 1e6 {
            st.push(
                "The bottom grows without bound while the top stays fixed",
                "",
            );
            return Some(Answer::Value(n_zero()));
        }
        return None;
    }
    None
}

fn n_zero() -> Sym {
    n(0)
}

/// What an antiderivative does at one end of an improper integral.
pub(crate) enum End {
    /// An exact value, from the working.
    Exact(Sym),
    /// Only a numeric value.
    Number(f64),
    /// ±∞.
    Infinite(i8),
    /// No limit at all: it keeps oscillating.
    None,
}

/// The limit of `f` as `v` goes to `at` (or to ±∞ by `sign` when `at` is
/// None) from `side`: exactly when the working finds it and the engine's
/// numeric limit agrees, else the engine's number.
pub(crate) fn end_value(f: &Sym, v: &str, at: Option<&Sym>, sign: i8, side: i8) -> End {
    let point = match at {
        Some(a) => format!("{}^{{{}}}", latex(a), if side > 0 { "+" } else { "-" }),
        None if sign > 0 => "\\infty".into(),
        None => "-\\infty".into(),
    };
    let source = format!(
        "\\lim_{{{}\\to {point}}}\\left({}\\right)",
        name(v),
        latex(f)
    );
    let rows = [crate::RowInput {
        id: 0,
        latex: source,
        ..Default::default()
    }];
    let numeric = std::panic::catch_unwind(|| crate::analyze(&rows))
        .ok()
        .and_then(|out| match out.last().and_then(|r| r.value.clone()) {
            Some(crate::Value::Number(n)) => Some(n),
            _ => None,
        })
        .unwrap_or(f64::NAN);
    let mut quiet = Steps::quiet();
    let exact = match at {
        Some(a) => at_point(f, v, a, side, &mut quiet),
        None => at_infinity(f, v, sign, &mut quiet),
    };
    match exact {
        Some(Answer::Value(s))
            if numeric.is_finite()
                && (s.value() - numeric).abs() <= 1e-6 * (1.0 + numeric.abs()) =>
        {
            End::Exact(s)
        }
        Some(Answer::Infinite(s)) if numeric.is_infinite() && (numeric > 0.0) == (s > 0) => {
            End::Infinite(s)
        }
        _ if numeric.is_infinite() => End::Infinite(if numeric > 0.0 { 1 } else { -1 }),
        // A simple number within rounding, as √x gives 10⁻²⁰ for 0.
        _ if numeric.abs() < 1e-12 => End::Exact(n(0)),
        _ if numeric.is_finite() => match super::recognize(numeric) {
            Some(r) => End::Exact(r),
            None => End::Number(numeric),
        },
        _ => End::None,
    }
}
