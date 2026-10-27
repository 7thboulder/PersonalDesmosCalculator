//! Writes symbolic expressions as LaTeX that reads back in the calculator,
//! so a result can be added to the graph as a row.

use super::sym::{neg, num_den, Func, Sym, Q};

/// How tightly something binds, so brackets go only where they must.
#[derive(Clone, Copy, PartialEq, PartialOrd)]
enum Level {
    Sum,
    Product,
    Power,
    Atom,
}

pub fn latex(s: &Sym) -> String {
    write(s).0
}

/// The LaTeX for a name: `a_1` as a_{1}, `theta` as \theta.
pub fn name(v: &str) -> String {
    let (base, sub) = match v.split_once('_') {
        Some((b, s)) => (b, Some(s)),
        None => (v, None),
    };
    let base = if base.chars().count() > 1 {
        format!("\\{base} ")
    } else {
        base.to_owned()
    };
    match sub {
        Some(s) => format!("{}_{{{s}}}", base.trim_end()),
        None => base,
    }
}

fn number(q: Q) -> (String, Level) {
    if q.is_int() {
        let level = if q.n < 0 { Level::Sum } else { Level::Atom };
        (q.n.to_string(), level)
    } else if q.n < 0 {
        (format!("-\\frac{{{}}}{{{}}}", -q.n, q.d), Level::Sum)
    } else {
        (format!("\\frac{{{}}}{{{}}}", q.n, q.d), Level::Atom)
    }
}

fn real(r: f64) -> (String, Level) {
    let s = crate::format::value_latex(r);
    let level = if r < 0.0 { Level::Sum } else { Level::Atom };
    (s, level)
}

pub fn func_name(f: Func) -> &'static str {
    match f {
        Func::Sin => "\\sin",
        Func::Cos => "\\cos",
        Func::Tan => "\\tan",
        Func::Csc => "\\csc",
        Func::Sec => "\\sec",
        Func::Cot => "\\cot",
        Func::Asin => "\\arcsin",
        Func::Acos => "\\arccos",
        Func::Atan => "\\arctan",
        Func::Sinh => "\\sinh",
        Func::Cosh => "\\cosh",
        Func::Tanh => "\\tanh",
        Func::Asinh => "\\operatorname{arcsinh}",
        Func::Acosh => "\\operatorname{arccosh}",
        Func::Atanh => "\\operatorname{arctanh}",
        Func::Ln => "\\ln",
        Func::Log => "\\log",
        Func::Abs => "",
    }
}

fn wrap(s: (String, Level), at_least: Level) -> String {
    if s.1 < at_least {
        format!("\\left({}\\right)", s.0)
    } else {
        s.0
    }
}

fn write(s: &Sym) -> (String, Level) {
    match s {
        Sym::Num(q) => number(*q),
        Sym::Real(r) => real(*r),
        Sym::Var(v) => (name(v), Level::Atom),
        Sym::Add(xs) => {
            let mut out = String::new();
            for (i, t) in xs.iter().enumerate() {
                if i == 0 {
                    out.push_str(&write(t).0);
                } else if t.is_negative() {
                    out.push('-');
                    out.push_str(&wrap(write(&neg(t.clone())), Level::Product));
                } else {
                    out.push('+');
                    out.push_str(&wrap(write(t), Level::Product));
                }
            }
            (out, Level::Sum)
        }
        Sym::Mul(_) | Sym::Pow(..) if s.is_negative() => {
            let inner = write(&neg(s.clone()));
            (format!("-{}", wrap(inner, Level::Product)), Level::Sum)
        }
        Sym::Mul(xs) => {
            let (top, bottom) = split(xs);
            if bottom.is_empty() {
                (juxtapose(&top), Level::Product)
            } else {
                let t = if top.is_empty() {
                    "1".to_owned()
                } else {
                    juxtapose(&top)
                };
                (
                    format!("\\frac{{{t}}}{{{}}}", juxtapose(&bottom)),
                    Level::Atom,
                )
            }
        }
        Sym::Pow(b, e) => power(b, e),
        Sym::Fn(Func::Abs, a) => (format!("\\left|{}\\right|", write(a).0), Level::Atom),
        Sym::Fn(f, a) => (
            format!("{}\\left({}\\right)", func_name(*f), write(a).0),
            Level::Atom,
        ),
    }
}

/// The factors of a product that go above and below a fraction bar.
fn split(xs: &[Sym]) -> (Vec<Sym>, Vec<Sym>) {
    let mut top = Vec::new();
    let mut bottom = Vec::new();
    for x in xs {
        match x {
            Sym::Num(q) => {
                if q.n != 1 {
                    top.push(Sym::Num(Q::int(q.n)));
                }
                if q.d != 1 {
                    bottom.push(Sym::Num(Q::int(q.d)));
                }
            }
            Sym::Pow(..) if x.is_negative_power() => {
                let (_, d) = num_den(x);
                bottom.push(d);
            }
            other => top.push(other.clone()),
        }
    }
    (top, bottom)
}

impl Sym {
    fn is_negative_power(&self) -> bool {
        matches!(self, Sym::Pow(_, e) if e.is_negative())
    }
}

/// Factors written side by side, with a dot where two numbers would run
/// together.
fn juxtapose(fs: &[Sym]) -> String {
    let mut out = String::new();
    for (i, f) in fs.iter().enumerate() {
        let piece = write(f);
        let level = piece.1;
        let text = if level < Level::Power || (i > 0 && level == Level::Product) {
            if fs.len() == 1 {
                piece.0
            } else {
                format!("\\left({}\\right)", piece.0)
            }
        } else {
            piece.0
        };
        let starts_with_digit =
            text.starts_with(|c: char| c.is_ascii_digit()) || text.starts_with("\\frac");
        if i > 0 && starts_with_digit {
            out.push_str("\\cdot ");
        }
        out.push_str(&text);
    }
    out
}

fn power(b: &Sym, e: &Sym) -> (String, Level) {
    if let Some(q) = e.as_q() {
        if q.n < 0 {
            let (_, d) = num_den(&Sym::Pow(Box::new(b.clone()), Box::new(e.clone())));
            return (format!("\\frac{{1}}{{{}}}", write(&d).0), Level::Atom);
        }
        if q.n == 1 && q.d == 2 {
            return (format!("\\sqrt{{{}}}", write(b).0), Level::Atom);
        }
        if q.n == 1 {
            return (format!("\\sqrt[{}]{{{}}}", q.d, write(b).0), Level::Atom);
        }
        // sin²x, as Desmos writes it.
        if let Sym::Fn(f, a) = b {
            if q.is_int() && *f != Func::Abs {
                return (
                    format!("{}^{{{}}}\\left({}\\right)", func_name(*f), q.n, write(a).0),
                    Level::Atom,
                );
            }
        }
    }
    let base = write(b);
    let base = match b {
        Sym::Fn(..) | Sym::Pow(..) => format!("\\left({}\\right)", base.0),
        Sym::Num(q) if !q.is_int() => format!("\\left({}\\right)", base.0),
        _ => wrap(base, Level::Atom),
    };
    (format!("{base}^{{{}}}", write(e).0), Level::Power)
}

#[cfg(test)]
mod tests {
    use super::super::sym::*;
    use super::latex;

    #[test]
    fn reads_like_desmos() {
        let x = var("x");
        let p = add(vec![powi(x.clone(), 2), mul2(n(-3), x.clone()), n(2)]);
        assert_eq!(latex(&p), "x^{2}-3x+2");
        assert_eq!(latex(&div(x.clone(), n(2))), "\\frac{x}{2}");
        assert_eq!(latex(&div(n(1), x.clone())), "\\frac{1}{x}");
        assert_eq!(latex(&sqrt(n(12))), "2\\sqrt{3}");
        assert_eq!(
            latex(&mul2(n(2), func(Func::Sin, x.clone()))),
            "2\\sin\\left(x\\right)"
        );
        assert_eq!(
            latex(&powi(func(Func::Sin, x.clone()), 2)),
            "\\sin^{2}\\left(x\\right)"
        );
        assert_eq!(
            latex(&mul2(n(2), add2(x.clone(), n(1)))),
            "2\\left(x+1\\right)"
        );
        assert_eq!(latex(&exp(mul2(n(2), x.clone()))), "e^{2x}");
        assert_eq!(latex(&neg(x.clone())), "-x");
        assert_eq!(latex(&mul2(var("theta"), x)), "\\theta x");
    }
}
