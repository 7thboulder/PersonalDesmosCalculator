//! Turns parsed rows into symbolic expressions, expanding the functions
//! other rows define.

use std::collections::HashMap;

use super::sym::*;
use crate::ast::{Expr, Relation, Repeat, Statement};
use crate::builtins::Builtin;

/// Functions defined in other rows, as `f(x) = x^2` defines `f`.
#[derive(Default)]
pub struct Defs {
    funcs: HashMap<String, (Vec<String>, Expr)>,
}

impl Defs {
    pub fn from_rows(rows: &[String]) -> Defs {
        let mut defs = Defs::default();
        for latex in rows {
            if let Ok(Statement::Relation(Expr::Apply(name, params), Relation::Eq, body)) =
                crate::parse(latex)
            {
                let names: Option<Vec<String>> = params
                    .iter()
                    .map(|p| match p {
                        Expr::Var(v) => Some(v.clone()),
                        _ => None,
                    })
                    .collect();
                if let Some(names) = names {
                    defs.funcs.insert(name, (names, body));
                }
            }
        }
        defs
    }

    pub fn function(&self, name: &str) -> Option<&(Vec<String>, Expr)> {
        self.funcs.get(name)
    }
}

const UNSUPPORTED: &str =
    "Solve works on single expressions and equations, not on lists, points or piecewise definitions";

/// Whether an expression is ∞ or -∞, for integral bounds.
pub fn infinity(e: &Expr) -> Option<f64> {
    match e {
        Expr::Var(v) if v == "infty" => Some(f64::INFINITY),
        Expr::Neg(inner) => infinity(inner).map(|v| -v),
        _ => None,
    }
}

pub fn to_sym(e: &Expr, defs: &Defs) -> Result<Sym, String> {
    convert(e, defs, 0)
}

fn convert(e: &Expr, defs: &Defs, depth: usize) -> Result<Sym, String> {
    if depth > 40 {
        return Err("Functions here call each other too deeply".into());
    }
    let c = |x: &Expr| convert(x, defs, depth);
    Ok(match e {
        Expr::Num(v) => num_f64(*v),
        Expr::Var(v) if v == "infty" => return Err("Solve can't use ∞ here".into()),
        Expr::Var(v) => var(v),
        Expr::Neg(a) => neg(c(a)?),
        Expr::Add(a, b) => add2(c(a)?, c(b)?),
        Expr::Sub(a, b) => sub(c(a)?, c(b)?),
        Expr::Mul(a, b) => mul2(c(a)?, c(b)?),
        Expr::Div(a, b) => div(c(a)?, c(b)?),
        Expr::Pow(a, b) => pow(c(a)?, c(b)?),
        Expr::Call(f, args) => {
            let args: Vec<Sym> = args.iter().map(c).collect::<Result<_, _>>()?;
            call(*f, args)?
        }
        Expr::Apply(name, args) => {
            let args: Vec<Sym> = args.iter().map(c).collect::<Result<_, _>>()?;
            match defs.function(name) {
                Some((params, body)) => {
                    if params.len() != args.len() {
                        return Err(format!("{name} takes {} arguments", params.len()));
                    }
                    let body = convert(body, defs, depth + 1)?;
                    substitute(&body, params, &args)
                }
                // a(x+1) is a times x+1 when a is not a function.
                None if args.len() == 1 => mul2(var(name), args.into_iter().next().unwrap()),
                None => return Err(format!("{name} isn't defined as a function")),
            }
        }
        Expr::Factorial(a) => {
            let v = c(a)?;
            match v.as_q() {
                Some(q) if q.is_int() && (0..=30).contains(&q.n) => {
                    n((1..=q.n).product::<i128>().max(1))
                }
                _ => return Err("Solve only takes factorials of whole numbers up to 30".into()),
            }
        }
        Expr::Derivative(v, body) => super::diff::derivative(&c(body)?, v),
        Expr::Prime(name, k, args) => {
            let Some((params, body)) = defs.function(name) else {
                return Err(format!("{name} isn't defined as a function"));
            };
            if params.len() != 1 || args.len() != 1 {
                return Err("Primes work on functions of one variable".into());
            }
            let mut body = convert(body, defs, depth + 1)?;
            for _ in 0..*k {
                body = super::diff::derivative(&body, &params[0]);
            }
            substitute(&body, params, &[c(&args[0])?])
        }
        Expr::Integral(v, lo, hi, body) => {
            if infinity(lo).is_some() || infinity(hi).is_some() {
                return Err("Solve can't use an integral to ∞ inside another expression".into());
            }
            let (lo, hi, body) = (c(lo)?, c(hi)?, c(body)?);
            let mut quiet = super::Steps::quiet();
            match super::integrate::antiderivative(&body, v, &mut quiet) {
                Some(f) => sub(f.subs_var(v, &hi), f.subs_var(v, &lo)),
                None if lo.is_constant() && hi.is_constant() && body.names() == [v.clone()] => {
                    Sym::Real(crate::calculus::integrate(
                        &mut |t| body.at(v, t),
                        lo.value(),
                        hi.value(),
                    ))
                }
                None => return Err("This integral has no antiderivative Solve can find".into()),
            }
        }
        Expr::Repeat(kind, v, lo, hi, body) => {
            let (lo, hi) = (c(lo)?, c(hi)?);
            let (Some(a), Some(b)) = (lo.as_q(), hi.as_q()) else {
                return Err("Solve needs whole-number bounds on a sum".into());
            };
            if !a.is_int() || !b.is_int() || b.n - a.n > 500 {
                return Err("Solve only writes out sums of up to 500 terms".into());
            }
            let body = c(body)?;
            let items: Vec<Sym> = (a.n..=b.n).map(|k| body.subs_var(v, &n(k))).collect();
            match kind {
                Repeat::Sum => add(items),
                Repeat::Product => mul(items),
            }
        }
        _ => return Err(UNSUPPORTED.into()),
    })
}

/// Puts arguments in for parameters all at once, so f(y, x) swaps them.
pub fn substitute(body: &Sym, params: &[String], args: &[Sym]) -> Sym {
    let temps: Vec<String> = (0..params.len()).map(|i| format!("\u{1}{i}")).collect();
    let mut out = body.clone();
    for (p, t) in params.iter().zip(&temps) {
        out = out.subs_var(p, &var(t));
    }
    for (t, a) in temps.iter().zip(args) {
        out = out.subs_var(t, a);
    }
    out
}

fn call(f: Builtin, mut args: Vec<Sym>) -> Result<Sym, String> {
    use Builtin as B;
    let one = |args: &mut Vec<Sym>| -> Result<Sym, String> {
        if args.len() == 1 {
            Ok(args.pop().unwrap())
        } else {
            Err(format!("{} takes one argument", f.name()))
        }
    };
    let unary = |g: Func, args: &mut Vec<Sym>| -> Result<Sym, String> { Ok(func(g, one(args)?)) };
    let recip = |g: Func, args: &mut Vec<Sym>| -> Result<Sym, String> {
        Ok(div(n(1), func(g, one(args)?)))
    };
    let of_recip = |g: Func, args: &mut Vec<Sym>| -> Result<Sym, String> {
        Ok(func(g, div(n(1), one(args)?)))
    };
    match f {
        B::Sin => unary(Func::Sin, &mut args),
        B::Cos => unary(Func::Cos, &mut args),
        B::Tan => unary(Func::Tan, &mut args),
        B::Csc => unary(Func::Csc, &mut args),
        B::Sec => unary(Func::Sec, &mut args),
        B::Cot => unary(Func::Cot, &mut args),
        B::Arcsin => unary(Func::Asin, &mut args),
        B::Arccos => unary(Func::Acos, &mut args),
        B::Arctan if args.len() == 1 => unary(Func::Atan, &mut args),
        B::Arccsc => of_recip(Func::Asin, &mut args),
        B::Arcsec => of_recip(Func::Acos, &mut args),
        B::Arccot => of_recip(Func::Atan, &mut args),
        B::Sinh => unary(Func::Sinh, &mut args),
        B::Cosh => unary(Func::Cosh, &mut args),
        B::Tanh => unary(Func::Tanh, &mut args),
        B::Csch => recip(Func::Sinh, &mut args),
        B::Sech => recip(Func::Cosh, &mut args),
        B::Coth => recip(Func::Tanh, &mut args),
        B::Arcsinh => unary(Func::Asinh, &mut args),
        B::Arccosh => unary(Func::Acosh, &mut args),
        B::Arctanh => unary(Func::Atanh, &mut args),
        B::Ln => unary(Func::Ln, &mut args),
        B::Log => unary(Func::Log, &mut args),
        B::LogBase if args.len() == 2 => {
            let x = args.pop().unwrap();
            let base = args.pop().unwrap();
            if base.is_int(10) {
                Ok(func(Func::Log, x))
            } else {
                Ok(div(func(Func::Ln, x), func(Func::Ln, base)))
            }
        }
        B::Exp => Ok(exp(one(&mut args)?)),
        B::Sqrt => Ok(sqrt(one(&mut args)?)),
        B::Root if args.len() == 2 => {
            let x = args.pop().unwrap();
            let k = args.pop().unwrap();
            Ok(pow(x, div(n(1), k)))
        }
        B::Abs => unary(Func::Abs, &mut args),
        other => {
            // Whole-number functions of numbers still work: gcd(12, 18).
            if args.iter().all(Sym::is_constant) {
                let values: Vec<f64> = args.iter().map(Sym::value).collect();
                let v = other.eval(&values);
                if v.is_finite() {
                    return Ok(num_f64(v));
                }
            }
            Err(format!("Solve can't do algebra with {} yet", other.name()))
        }
    }
}
