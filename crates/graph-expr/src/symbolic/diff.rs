//! Derivatives, written out one rule at a time.

use super::latex::{latex, name};
use super::sym::*;
use super::Steps;

/// The derivative of `e` with respect to `v`, with no working shown.
pub fn derivative(e: &Sym, v: &str) -> Sym {
    d(e, v, &mut Steps::quiet())
}

/// The derivative, with each rule it uses written into `steps`.
pub fn derivative_steps(e: &Sym, v: &str, steps: &mut Steps) -> Sym {
    d(e, v, steps)
}

/// `\frac{d}{dx}\left[u\right]`.
pub fn dd(u: &Sym, v: &str) -> String {
    format!("\\frac{{d}}{{d{}}}\\left[{}\\right]", name(v), latex(u))
}

/// LaTeX with brackets when the expression is a sum or starts with a minus.
pub fn paren(s: &Sym) -> String {
    let l = latex(s);
    if matches!(s, Sym::Add(_)) || s.is_negative() {
        format!("\\left({l}\\right)")
    } else {
        l
    }
}

/// `a·b` in LaTeX, leaving out factors of 1.
fn times(parts: &[String]) -> String {
    let parts: Vec<&String> = parts.iter().filter(|p| p.as_str() != "1").collect();
    if parts.is_empty() {
        return "1".into();
    }
    let mut out = String::new();
    for (i, p) in parts.iter().enumerate() {
        // 2x and 2\frac{d}{dx}[...] read fine; 2·3 and x·(-1) need the dot.
        if i > 0
            && (p.starts_with(|c: char| c.is_ascii_digit() || c == '-')
                || p.starts_with("\\frac{1"))
        {
            out.push_str("\\cdot ");
        }
        out.push_str(p);
    }
    out
}

/// f'(u) for each function, in terms of u.
pub fn outer(f: Func, u: &Sym) -> Sym {
    let u = u.clone();
    let inv_sqrt = |s: Sym| pow(s, frac(-1, 2));
    match f {
        Func::Sin => func(Func::Cos, u),
        Func::Cos => neg(func(Func::Sin, u)),
        Func::Tan => powi(func(Func::Sec, u), 2),
        Func::Csc => neg(mul2(func(Func::Csc, u.clone()), func(Func::Cot, u))),
        Func::Sec => mul2(func(Func::Sec, u.clone()), func(Func::Tan, u)),
        Func::Cot => neg(powi(func(Func::Csc, u), 2)),
        Func::Asin => inv_sqrt(sub(n(1), powi(u, 2))),
        Func::Acos => neg(inv_sqrt(sub(n(1), powi(u, 2)))),
        Func::Atan => div(n(1), add2(n(1), powi(u, 2))),
        Func::Sinh => func(Func::Cosh, u),
        Func::Cosh => func(Func::Sinh, u),
        Func::Tanh => pow(func(Func::Cosh, u), n(-2)),
        Func::Asinh => inv_sqrt(add2(powi(u, 2), n(1))),
        Func::Acosh => inv_sqrt(sub(powi(u, 2), n(1))),
        Func::Atanh => div(n(1), sub(n(1), powi(u, 2))),
        Func::Ln => div(n(1), u),
        Func::Log => div(n(1), mul2(u, func(Func::Ln, n(10)))),
        Func::Abs => div(u.clone(), func(Func::Abs, u)),
    }
}

fn d(e: &Sym, v: &str, st: &mut Steps) -> Sym {
    if !e.has(v) {
        return n(0);
    }
    match e {
        Sym::Var(_) => n(1),
        Sym::Add(ts) => {
            if st.on() {
                let parts: Vec<String> = ts.iter().filter(|t| t.has(v)).map(|t| dd(t, v)).collect();
                let note = if ts.iter().any(|t| !t.has(v)) {
                    "Sum rule: differentiate each term (constants give 0)"
                } else {
                    "Sum rule: differentiate each term"
                };
                st.push(note, format!("{}={}", dd(e, v), parts.join("+")));
            }
            add(ts.iter().map(|t| d(t, v, st)).collect())
        }
        Sym::Mul(fs) => {
            let (constant, varying): (Vec<Sym>, Vec<Sym>) =
                fs.iter().cloned().partition(|f| !f.has(v));
            if !constant.is_empty() {
                let c = mul(constant);
                let rest = mul(varying);
                st.push(
                    format!(
                        "Constant multiple rule: keep ${}$ and differentiate the rest",
                        latex(&c)
                    ),
                    format!("{}={}", dd(e, v), times(&[paren(&c), dd(&rest, v)])),
                );
                return mul2(c, d(&rest, v, st));
            }
            let (top, bottom): (Vec<Sym>, Vec<Sym>) = varying
                .iter()
                .cloned()
                .partition(|f| !matches!(f, Sym::Pow(_, x) if x.is_negative()));
            if !top.is_empty() && !bottom.is_empty() {
                let u = mul(top);
                let w = div(n(1), mul(bottom));
                st.push(
                    "Quotient rule: (u/w)' = (u'w - uw')/w²",
                    format!(
                        "{}=\\frac{{{}-{}}}{{{}}}",
                        dd(e, v),
                        times(&[dd(&u, v), paren(&w)]),
                        times(&[paren(&u), dd(&w, v)]),
                        latex(&powi(w.clone(), 2))
                    ),
                );
                let du = d(&u, v, st);
                let dw = d(&w, v, st);
                return div(sub(mul2(du, w.clone()), mul2(u, dw)), powi(w, 2));
            }
            let first = varying[0].clone();
            let rest = mul(varying[1..].to_vec());
            st.push(
                "Product rule: (uw)' = u'w + uw'",
                format!(
                    "{}={}+{}",
                    dd(e, v),
                    times(&[dd(&first, v), paren(&rest)]),
                    times(&[paren(&first), dd(&rest, v)])
                ),
            );
            let a = d(&first, v, st);
            let b = d(&rest, v, st);
            add2(mul2(a, rest), mul2(first, b))
        }
        Sym::Pow(b, x) if !x.has(v) => {
            let bare = **b == var(v);
            let lowered = pow((**b).clone(), sub((**x).clone(), n(1)));
            let note = match (bare, x.as_q()) {
                (true, Some(q)) if q == Q::new(1, 2).unwrap() => {
                    "Power rule, writing the root as a power of 1/2".to_owned()
                }
                (true, _) => "Power rule: bring the power down and lower it by one".to_owned(),
                (false, _) => {
                    "Power rule with the chain rule: also multiply by the inside's derivative"
                        .to_owned()
                }
            };
            let mut parts = vec![paren(x), paren(&lowered)];
            if !bare {
                parts.push(dd(b, v));
            }
            st.push(note, format!("{}={}", dd(e, v), times(&parts)));
            let inner = d(b, v, st);
            mul(vec![(**x).clone(), lowered, inner])
        }
        Sym::Pow(b, x) if !b.has(v) => {
            let bare = **x == var(v);
            let natural = **b == var("e");
            let mut parts = vec![latex(e)];
            if !natural {
                parts.push(format!("\\ln\\left({}\\right)", latex(b)));
            }
            if !bare {
                parts.push(dd(x, v));
            }
            let note = match (natural, bare) {
                (true, true) => "The derivative of eˣ is itself",
                (true, false) => "Exponential rule with the chain rule",
                (false, true) => "Exponential rule: the derivative of aˣ is aˣ ln a",
                (false, false) => "Exponential rule (aᵘ)' = aᵘ ln a · u' with the chain rule",
            };
            st.push(note, format!("{}={}", dd(e, v), times(&parts)));
            let inner = d(x, v, st);
            mul(vec![e.clone(), func(Func::Ln, (**b).clone()), inner])
        }
        Sym::Pow(b, x) => {
            st.push(
                "Logarithmic differentiation: (fᵍ)' = fᵍ(g' ln f + g f'/f)",
                format!(
                    "{}={}\\left({}\\ln\\left({}\\right)+\\frac{{{}}}{{{}}}\\right)",
                    dd(e, v),
                    latex(e),
                    dd(x, v),
                    latex(b),
                    times(&[paren(x), dd(b, v)]),
                    latex(b)
                ),
            );
            let dx = d(x, v, st);
            let db = d(b, v, st);
            mul2(
                e.clone(),
                add2(
                    mul2(dx, func(Func::Ln, (**b).clone())),
                    div(mul2((**x).clone(), db), (**b).clone()),
                ),
            )
        }
        Sym::Fn(f, u) => {
            let bare = **u == var(v);
            let o = outer(*f, u);
            let general = Sym::Fn(*f, Box::new(var("u")));
            let rule = format!(
                "the derivative of ${}$ is ${}$",
                latex(&general),
                latex(&outer(*f, &var("u")))
            );
            let note = if bare {
                capitalize(&rule)
            } else {
                format!("Chain rule: {rule}, times the inside's derivative")
            };
            let mut parts = vec![paren(&o)];
            if !bare {
                parts.push(dd(u, v));
            }
            st.push(note, format!("{}={}", dd(e, v), times(&parts)));
            let inner = d(u, v, st);
            mul2(o, inner)
        }
        _ => n(0),
    }
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rules() {
        let x = var("x");
        assert_eq!(
            derivative(&powi(x.clone(), 3), "x"),
            mul2(n(3), powi(x.clone(), 2))
        );
        let s = func(Func::Sin, powi(x.clone(), 2));
        assert_eq!(
            derivative(&s, "x"),
            mul(vec![n(2), x.clone(), func(Func::Cos, powi(x.clone(), 2))])
        );
        assert_eq!(derivative(&exp(x.clone()), "x"), exp(x.clone()));
        assert_eq!(derivative(&func(Func::Ln, x.clone()), "x"), div(n(1), x));
    }
}
