//! Limits, `\lim_{x\to c} f(x)`, worked out numerically so a row, a live
//! line or a step check gets a value. (Solve asks Giac for the exact one.)
//!
//! A limit becomes an ordinary expression: the body is evaluated at points
//! closing in on `c` from each side, at distances 10⁻¹ to 10⁻⁴ (times
//! 1 + |c|), or at 10 to 10⁴ for a limit at ±∞. When the changes between
//! them shrink tenfold or faster, as for most bodies, Richardson's extrapolation is
//! the limit; when they shrink more slowly (√x, x ln x), Aitken's on three
//! points much closer in. When they keep growing the same way, the limit is
//! ±∞. Otherwise (or where the body is undefined) there is none. A two-sided
//! limit is the value both sides agree on.

use crate::ast::{Condition, Expr, Relation};
use crate::builtins::Builtin;

/// The expression for `\lim_{var \to point} body`, from above when `side`
/// is 1, from below when it is -1, and from both sides when it is 0.
pub fn numeric(var: &str, point: Expr, side: i8, body: Expr) -> Expr {
    let sign_of_infinity = |e: &Expr| match e {
        Expr::Num(v) if v.is_infinite() => Some(v.signum()),
        Expr::Var(n) if n == "infty" => Some(1.0),
        _ => None,
    };
    let infinite = match &point {
        Expr::Neg(inner) => sign_of_infinity(inner).map(|s| -s),
        e => sign_of_infinity(e),
    };
    if let Some(s) = infinite {
        // x = ±10, ±100, ... : the side doesn't matter at infinity.
        let at = |k: i32| Expr::Num(s * 10f64.powi(k));
        return settles(var, &body, s, one_side(var, &body, at));
    }
    let scale = add(num(1.0), abs(point.clone()));
    let near = |s: f64| {
        let point = point.clone();
        let scale = scale.clone();
        move |k: i32| add(point.clone(), mul(num(s * 10f64.powi(-k)), scale.clone()))
    };
    match side {
        1 => one_side(var, &body, near(1.0)),
        -1 => one_side(var, &body, near(-1.0)),
        _ => {
            let above = one_side(var, &body, near(1.0));
            let below = one_side(var, &body, near(-1.0));
            // Equal (so also both ∞ or both -∞), or close enough.
            let close = Condition {
                operands: vec![
                    abs(sub(above.clone(), below.clone())),
                    mul(num(1e-6), add(num(1.0), abs(above.clone()))),
                ],
                relations: vec![Relation::Le],
            };
            Expr::Piecewise(
                vec![
                    (
                        cond(above.clone(), Relation::Eq, below.clone()),
                        above.clone(),
                    ),
                    (close, mul(num(0.5), add(above, below))),
                ],
                None,
            )
        }
    }
}

/// `limit` (at ±∞, with `s` the sign) when the body really settles there:
/// at a dozen points spread through each of the decades from 10³ to 10⁴
/// and 10⁴ to 10⁵, the values' spread must shrink, as it doesn't for
/// sin x, whose values at powers of ten alone look like they converge.
/// An infinite limit must grow past the whole decade before.
fn settles(var: &str, body: &Expr, s: f64, limit: Expr) -> Expr {
    let decade = |from: f64| -> Vec<Expr> {
        (0..12)
            .map(|i| {
                let x = s * 10f64.powf(from + (i as f64 + 0.37) / 12.0);
                substitute(body.clone(), var, &Expr::Num(x))
            })
            .collect()
    };
    let (d1, d2) = (decade(3.0), decade(4.0));
    let max = |d: &[Expr]| Expr::Call(Builtin::Max, d.to_vec());
    let min = |d: &[Expr]| Expr::Call(Builtin::Min, d.to_vec());
    let spread = |d: &[Expr]| sub(max(d), min(d));
    let only = |c: Condition, then: Expr| Expr::Piecewise(vec![(c, then)], None);
    let finite = only(
        Condition {
            operands: vec![
                spread(&d2),
                add(
                    mul(num(0.8), spread(&d1)),
                    mul(num(1e-9), add(num(1.0), abs(limit.clone()))),
                ),
            ],
            relations: vec![Relation::Le],
        },
        limit.clone(),
    );
    Expr::Piecewise(
        vec![
            (
                cond(abs(limit.clone()), Relation::Lt, num(f64::INFINITY)),
                finite,
            ),
            (
                cond(limit.clone(), Relation::Gt, num(0.0)),
                only(cond(min(&d2), Relation::Gt, max(&d1)), limit.clone()),
            ),
            (
                cond(limit.clone(), Relation::Lt, num(0.0)),
                only(cond(max(&d2), Relation::Lt, min(&d1)), limit),
            ),
        ],
        None,
    )
}

/// The limit from one side, with `at(k)` the point 10⁻ᵏ of the way in
/// (or 10ᵏ out, for ∞).
fn one_side(var: &str, body: &Expr, at: impl Fn(i32) -> Expr) -> Expr {
    let f = |k: i32| substitute(body.clone(), var, &at(k));
    let (f2, f3, f4) = (f(2), f(3), f(4));
    let f1 = f(1);
    let d2 = sub(f3.clone(), f2);
    let d3 = sub(f4.clone(), f3.clone());
    // Changes shrinking tenfold or more, as when the error goes as h or h²:
    // Richardson's extrapolation, rid of the h and h² terms.
    let tenfold = cond(
        abs(d3.clone()),
        Relation::Le,
        mul(num(0.12), abs(d2.clone())),
    );
    let r34 = mul(num(1.0 / 9.0), sub(mul(num(10.0), f4.clone()), f3.clone()));
    let r23 = mul(num(1.0 / 9.0), sub(mul(num(10.0), f3.clone()), f(2)));
    let richardson = mul(num(1.0 / 99.0), sub(mul(num(100.0), r34), r23));
    // Otherwise Aitken's, for slower rates such as √h or h ln h, on three
    // points much closer in (10⁻⁶ to 10⁻⁸): g8 - e8² / (e8 - e7), or g8 when
    // the changes are equal.
    let (g6, g7, g8) = (f(6), f(7), f(8));
    let e7 = sub(g7.clone(), g6);
    let e8 = sub(g8.clone(), g7);
    let bend = sub(e8.clone(), e7);
    let aitken = Expr::Piecewise(
        vec![(
            cond(abs(bend.clone()), Relation::Gt, num(0.0)),
            sub(
                g8.clone(),
                Expr::Div(
                    Box::new(Expr::Pow(Box::new(e8), Box::new(num(2.0)))),
                    Box::new(bend),
                ),
            ),
        )],
        Some(Box::new(g8)),
    );
    let d1 = sub(f(2), f1);
    let settled = cond(
        abs(d3.clone()),
        Relation::Le,
        mul(num(1e-12), add(num(1.0), abs(f4.clone()))),
    );
    let shrinking = cond(
        abs(d3.clone()),
        Relation::Lt,
        mul(num(0.9), abs(d2.clone())),
    );
    // Growing the same way each step: ±∞.
    let infinity = Expr::Piecewise(
        vec![(
            cond(mul(d2.clone(), d3.clone()), Relation::Gt, num(0.0)),
            Expr::Piecewise(
                vec![(
                    cond(mul(d1, d2), Relation::Gt, num(0.0)),
                    Expr::Piecewise(
                        vec![(
                            cond(abs(f4.clone()), Relation::Gt, abs(f3)),
                            mul(Expr::Call(Builtin::Sign, vec![d3]), num(f64::INFINITY)),
                        )],
                        None,
                    ),
                )],
                None,
            ),
        )],
        None,
    );
    Expr::Piecewise(
        vec![(settled, f4), (tenfold, richardson), (shrinking, aitken)],
        Some(Box::new(infinity)),
    )
}

/// `body` with `var` replaced by `value`.
fn substitute(body: Expr, var: &str, value: &Expr) -> Expr {
    crate::units::rewrite(body, &mut |e| match e {
        Expr::Var(ref n) if n == var => Ok(value.clone()),
        e => Err(e),
    })
}

fn num(v: f64) -> Expr {
    Expr::Num(v)
}
fn add(a: Expr, b: Expr) -> Expr {
    Expr::Add(Box::new(a), Box::new(b))
}
fn sub(a: Expr, b: Expr) -> Expr {
    Expr::Sub(Box::new(a), Box::new(b))
}
fn mul(a: Expr, b: Expr) -> Expr {
    Expr::Mul(Box::new(a), Box::new(b))
}
fn abs(a: Expr) -> Expr {
    Expr::Call(Builtin::Abs, vec![a])
}
fn cond(a: Expr, r: Relation, b: Expr) -> Condition {
    Condition {
        operands: vec![a, b],
        relations: vec![r],
    }
}

#[cfg(test)]
mod tests {
    fn value(latex: &str) -> Option<f64> {
        value_with(latex, &[])
    }

    fn value_with(latex: &str, others: &[&str]) -> Option<f64> {
        let mut rows: Vec<crate::analyze::RowInput> = others
            .iter()
            .enumerate()
            .map(|(i, l)| crate::analyze::RowInput {
                id: i as u64 + 1,
                latex: (*l).to_owned(),
                ..Default::default()
            })
            .collect();
        rows.push(crate::analyze::RowInput {
            id: 99,
            latex: latex.to_owned(),
            ..Default::default()
        });
        let out = crate::analyze::analyze(&rows);
        let last = out.last().unwrap();
        assert!(last.error.is_none(), "{latex}: {:?}", last.error);
        match &last.value {
            Some(crate::analyze::Value::Number(v)) if !v.is_nan() => Some(*v),
            _ => None,
        }
    }

    fn near(latex: &str, want: f64) {
        let got = value(latex).unwrap_or_else(|| panic!("{latex}: no value"));
        assert!(
            (got - want).abs() <= 1e-6 * (1.0 + want.abs()),
            "{latex}: {got}, not {want}"
        );
    }

    #[test]
    fn finite_limits() {
        near("\\lim_{x\\to0}\\frac{\\sin x}{x}", 1.0);
        near("\\lim_{x\\to0}\\frac{1-\\cos x}{x^2}", 0.5);
        near("\\lim_{h\\to0}\\frac{\\left(2+h\\right)^2-4}{h}", 4.0);
        near(
            "\\lim_{x\\to\\infty}\\left(1+\\frac{1}{x}\\right)^x",
            std::f64::consts::E,
        );
        near("\\lim_{x\\to\\infty}\\frac{3x^2+1}{x^2-5}", 3.0);
        near("\\lim_{x\\to-\\infty}e^x", 0.0);
        // Oscillating forever: no limit, though sin at powers of ten alone
        // would suggest one.
        assert_eq!(value("\\lim_{x\\to\\infty}\\sin x"), None);
        assert_eq!(value("\\lim_{x\\to\\infty}x\\sin x"), None);
        near("\\lim_{x\\to\\infty}\\frac{\\sin x}{x}", 0.0);
        assert_eq!(value("\\lim_{x\\to\\infty}\\ln x"), Some(f64::INFINITY));
        // Infinite sums and products are numbers too.
        near(
            "\\sum_{n=1}^{\\infty}\\frac{1}{n^2}",
            std::f64::consts::PI.powi(2) / 6.0,
        );
        near("\\prod_{n=2}^{\\infty}\\left(1-\\frac{1}{n^2}\\right)", 0.5);
        near("\\sum_{n=-\\infty}^{\\infty}e^{-n^2}", 1.7726372048266521);
        near("\\sum_{n=1}^{\\infty}\\frac{1}{n(n+1)}", 1.0);
        assert_eq!(
            value("\\sum_{n=1}^{\\infty}\\frac{1}{n}"),
            Some(f64::INFINITY)
        );
        assert_eq!(value("\\sum_{n=0}^{\\infty}(-1)^n"), None);
        // ∂f/∂y of a defined function, through another function.
        assert_eq!(
            value_with(
                "g(1,2)",
                &["f(x,y)=x^2y^3", "g(x,y)=\\frac{\\partial f}{\\partial y}"]
            ),
            Some(12.0)
        );
        near("\\lim_{x\\to0^+}\\sqrt{x}", 0.0);
        near("\\lim_{x\\to0^{+}}x\\ln x", 0.0);
        near("\\lim_{x\\to2}x^2+1", 5.0);
    }

    #[test]
    fn one_sided_limits() {
        near("\\lim_{x\\to0^+}\\frac{\\left|x\\right|}{x}", 1.0);
        near("\\lim_{x\\to0^-}\\frac{\\left|x\\right|}{x}", -1.0);
        near("\\lim_{x\\to2^-}\\operatorname{floor}\\left(x\\right)", 1.0);
        near("\\lim_{x\\to2^+}\\operatorname{floor}\\left(x\\right)", 2.0);
        assert_eq!(value("\\lim_{x\\to0^+}\\frac{1}{x}"), Some(f64::INFINITY));
        assert_eq!(
            value("\\lim_{x\\to0^-}\\frac{1}{x}"),
            Some(f64::NEG_INFINITY)
        );
        assert_eq!(value("\\lim_{x\\to0^+}\\ln x"), Some(f64::NEG_INFINITY));
        assert_eq!(value("\\lim_{x\\to0^-}\\sqrt{x}"), None);
    }

    #[test]
    fn two_sides_must_agree() {
        assert_eq!(value("\\lim_{x\\to0}\\frac{\\left|x\\right|}{x}"), None);
        assert_eq!(value("\\lim_{x\\to0}\\frac{1}{x}"), None);
        assert_eq!(value("\\lim_{x\\to0}\\frac{1}{x^2}"), Some(f64::INFINITY));
        assert_eq!(
            value("\\lim_{x\\to2}\\operatorname{floor}\\left(x\\right)"),
            None
        );
    }

    #[test]
    fn steps_are_checked_from_the_right_side() {
        use crate::check::{check_group, Verdict};
        let check = |lines: &[&str]| {
            let lines: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
            check_group(&lines, &[])
        };
        let lim = "\\lim_{x\\to0^-}\\frac{\\left|x\\right|}{x}";
        assert_eq!(check(&[lim, "=-1"])[1], Verdict::Follows);
        assert!(matches!(check(&[lim, "=1"])[1], Verdict::Wrong(_)));
        let lim = "\\lim_{h\\to0}\\frac{\\left(x+h\\right)^2-x^2}{h}";
        assert_eq!(check(&[lim, "=2x"])[1], Verdict::Follows);
    }

    #[test]
    fn uses_other_rows() {
        let got = value_with(
            "\\lim_{x\\to a}\\frac{f\\left(x\\right)-f\\left(a\\right)}{x-a}",
            &["f\\left(x\\right)=x^3", "a=2"],
        );
        assert!((got.unwrap() - 12.0).abs() < 1e-6, "{got:?}");
    }
}
