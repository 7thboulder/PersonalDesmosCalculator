//! Giac, the computer algebra system behind Solve's hardest problems: what
//! to ask it, written in its own language, and its answers back in LaTeX,
//! each checked numerically by this crate's own evaluator.
//!
//! Nothing here runs Giac. The app sends `Plan::setup` and `Plan::command`
//! to its Giac worker and hands the answer to `finish`.

use std::collections::{HashMap, HashSet};

use crate::ast::{Expr, Relation, Repeat, Statement};
use crate::builtins::{self, Builtin};
use crate::symbolic::{Choice, Report, Step};
use crate::{analyze, RowInput, Value};

/// How names in the problem are written for Giac. Every name the person
/// wrote gets a `g_` in front, so `D`, `E` or `beta` can't clash with one of
/// Giac's own commands; Giac's answers have it taken off again.
#[derive(Clone, Debug, Default)]
pub struct Names {
    back: HashMap<String, String>,
}

impl Names {
    fn var(&mut self, name: &str) -> String {
        match name {
            "pi" => return "pi".into(),
            "e" => return "e".into(),
            "i" => return "i".into(),
            "infty" | "infinity" => return "inf".into(),
            _ => {}
        }
        let giac: String = format!(
            "g_{}",
            name.chars()
                .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
                .collect::<String>()
        );
        self.back.insert(giac.clone(), name.to_owned());
        giac
    }

    fn original(&self, giac: &str) -> Option<&str> {
        self.back.get(giac).map(String::as_str)
    }
}

/// What a plan asks, so its answer can be written out and checked.
#[derive(Clone, Debug)]
enum Kind {
    Antiderivative {
        var: String,
        body: String,
    },
    Definite {
        question: String,
    },
    Derivative,
    Sum {
        question: String,
    },
    Limit {
        var: String,
        body: String,
        point: Option<f64>,
        dir: i8,
    },
    Solve {
        var: String,
        lhs: String,
        rhs: String,
    },
    Ode {
        dep: String,
    },
    Rewrite {
        var: Option<String>,
        original: String,
    },
    Series,
    Laplace,
}

/// What to ask Giac for one problem.
#[derive(Clone, Debug)]
pub struct Plan {
    /// Definitions from the other rows or live lines, run first.
    pub setup: Vec<String>,
    /// The command whose answer is the result.
    pub command: String,
    pub op: String,
    pub choices: Vec<Choice>,
    names: Names,
    kind: Kind,
    /// The problem as written, for the working.
    question: String,
    /// The other rows, for the numeric check.
    others: Vec<String>,
}

// ---------------------------------------------------------------------------
// LaTeX to Giac

struct Writer<'a> {
    names: &'a mut Names,
    /// Functions the other rows define.
    funcs: &'a HashSet<String>,
}

impl Writer<'_> {
    /// `e` written the way answers are, for the numeric checks.
    fn latex(&mut self, e: &Expr) -> Result<String, String> {
        let g = self.expr(e)?;
        latex_of(&g, self.names)
    }
}

/// A decimal written exactly, so Giac keeps exact arithmetic: 0.25 is 25/100.
fn exact(v: f64) -> String {
    if v.is_infinite() {
        return if v > 0.0 { "inf".into() } else { "-inf".into() };
    }
    if v.fract() == 0.0 && v.abs() < 1e15 {
        return format!("{}", v as i64);
    }
    let text = format!("{v}");
    match text.split_once('.') {
        Some((whole, frac)) if !text.contains('e') => {
            let digits = format!("{}{}", whole.trim_start_matches('-'), frac);
            let digits = digits.trim_start_matches('0');
            let sign = if v < 0.0 { "-" } else { "" };
            format!(
                "({sign}{}/1{})",
                if digits.is_empty() { "0" } else { digits },
                "0".repeat(frac.len())
            )
        }
        _ => format!("({text})"),
    }
}

impl Writer<'_> {
    fn expr(&mut self, e: &Expr) -> Result<String, String> {
        Ok(match e {
            Expr::Num(v) => exact(*v),
            Expr::Var(name) => {
                // y' and y'' in a differential equation.
                let primes = name.chars().rev().take_while(|c| *c == '\'').count();
                if primes > 0 {
                    let base = &name[..name.len() - primes];
                    format!("{}{}", self.names.var(base), "'".repeat(primes))
                } else {
                    self.names.var(name)
                }
            }
            Expr::Neg(a) => format!("(-{})", self.expr(a)?),
            Expr::Add(a, b) => format!("({}+{})", self.expr(a)?, self.expr(b)?),
            Expr::Sub(a, b) => format!("({}-{})", self.expr(a)?, self.expr(b)?),
            Expr::Mul(a, b) => format!("({}*{})", self.expr(a)?, self.expr(b)?),
            Expr::Div(a, b) => format!("({}/{})", self.expr(a)?, self.expr(b)?),
            Expr::Pow(a, b) => {
                if **a == Expr::Var("e".into()) {
                    format!("exp({})", self.expr(b)?)
                } else {
                    format!("({}^{})", self.expr(a)?, self.expr(b)?)
                }
            }
            Expr::Factorial(a) => format!("factorial({})", self.expr(a)?),
            Expr::Call(f, args) => self.call(*f, args)?,
            Expr::Apply(name, args) => {
                if self.funcs.contains(name) {
                    let args = self.list(args)?;
                    format!("{}({args})", self.names.var(name))
                } else if let Some(b) = builtins::from_letter(name, args.len()) {
                    self.call(b, args)?
                } else if let Some((b, order)) = name
                    .split_once('_')
                    .and_then(|(letter, sub)| Some((builtins::bessel(letter)?, sub)))
                    .filter(|_| args.len() == 1)
                {
                    let order = match order.parse::<f64>() {
                        Ok(v) => exact(v),
                        Err(_) => self.names.var(order),
                    };
                    let mut all = vec![];
                    all.push(order);
                    all.push(self.expr(&args[0])?);
                    format!("{}({})", giac_bessel(b), all.join(","))
                } else if args.len() == 1 {
                    // a(x+1) is a product when a isn't a function.
                    format!("({}*{})", self.names.var(name), self.expr(&args[0])?)
                } else {
                    return Err(format!("'{name}' is not defined"));
                }
            }
            Expr::Prime(name, k, args) => {
                format!(
                    "{}{}({})",
                    self.names.var(name),
                    "'".repeat(*k as usize),
                    self.list(args)?
                )
            }
            Expr::Integral(v, lo, hi, body) => {
                let var = self.names.var(v);
                let body = self.expr(body)?;
                if matches!(&**lo, Expr::Var(n) if n == crate::ast::INDEFINITE) {
                    format!("integrate({body},{var})")
                } else {
                    format!(
                        "integrate({body},{var},{},{})",
                        self.expr(lo)?,
                        self.expr(hi)?
                    )
                }
            }
            Expr::Derivative(v, body) => {
                format!("diff({},{})", self.expr(body)?, self.names.var(v))
            }
            Expr::Repeat(kind, v, lo, hi, body) => format!(
                "{}({},{},{},{})",
                if *kind == Repeat::Sum {
                    "sum"
                } else {
                    "product"
                },
                self.expr(body)?,
                self.names.var(v),
                self.expr(lo)?,
                self.expr(hi)?
            ),
            Expr::List(items) => format!("[{}]", self.list(items)?),
            _ => return Err("Giac can't take this kind of expression yet".into()),
        })
    }

    fn list(&mut self, items: &[Expr]) -> Result<String, String> {
        Ok(items
            .iter()
            .map(|a| self.expr(a))
            .collect::<Result<Vec<_>, _>>()?
            .join(","))
    }

    fn call(&mut self, f: Builtin, args: &[Expr]) -> Result<String, String> {
        use Builtin::*;
        let a = self.list(args)?;
        let one = |name: &str| format!("{name}({a})");
        Ok(match f {
            Sin | Cos | Tan | Sinh | Cosh | Tanh | Exp | Sqrt | Abs | Floor | Ceil | Sign | Ln
            | Erf | Erfc | Si | Ci | Ei | Min | Max => one(&format!("{f:?}").to_lowercase()),
            Csc => format!("(1/sin({a}))"),
            Sec => format!("(1/cos({a}))"),
            Cot => format!("(1/tan({a}))"),
            Arcsin => one("asin"),
            Arccos => one("acos"),
            Arctan if args.len() == 1 => one("atan"),
            Arctan => one("atan2"),
            Arcsinh => one("asinh"),
            Arccosh => one("acosh"),
            Arctanh => one("atanh"),
            Log => one("log10"),
            LogBase => {
                let base = self.expr(&args[0])?;
                let x = self.expr(&args[1])?;
                format!("(ln({x})/ln({base}))")
            }
            Root => {
                let n = self.expr(&args[0])?;
                let x = self.expr(&args[1])?;
                format!("surd({x},{n})")
            }
            Factorial => one("factorial"),
            NCr => one("comb"),
            NPr => one("perm"),
            Gamma => one("Gamma"),
            Digamma => one("Psi"),
            Beta => one("Beta"),
            Zeta => one("Zeta"),
            Erfi => format!("(-i*erf(i*({a})))"),
            Li => one("Li"),
            LambertW => one("LambertW"),
            AiryAi => one("Airy_Ai"),
            AiryBi => one("Airy_Bi"),
            BesselJ | BesselY | BesselI | BesselK => format!("{}({a})", giac_bessel(f)),
            other => return Err(format!("Giac can't work with {} yet", other.name())),
        })
    }
}

fn giac_bessel(b: Builtin) -> &'static str {
    match b {
        Builtin::BesselJ => "BesselJ",
        Builtin::BesselY => "BesselY",
        Builtin::BesselI => "BesselI",
        _ => "BesselK",
    }
}

// ---------------------------------------------------------------------------
// Planning

fn choice(op: &str, label: &str) -> Choice {
    Choice {
        op: op.into(),
        label: label.into(),
    }
}

/// The names an expression reads, without constants.
fn free_names(e: &Expr) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    e.visit_names(&mut |n, function| {
        let n = n.trim_end_matches('\'');
        if !function
            && !matches!(n, "pi" | "e" | "i" | "infty")
            && n != crate::ast::INDEFINITE
            && !out.iter().any(|o| o == n)
        {
            out.push(n.to_owned());
        }
    });
    out
}

/// The variable to work in: x if it's there, then t, then the only one.
fn main_var(names: &[String]) -> String {
    for v in ["x", "t", "s", "n", "y", "z"] {
        if names.iter().any(|n| n == v) {
            return v.into();
        }
    }
    names.first().cloned().unwrap_or_else(|| "x".into())
}

/// `\lim_{x\to a}` at the start of `latex`: the variable, the point (with
/// ^{+} or ^{-} for one side) and the rest.
fn split_limit(latex: &str) -> Option<(String, String, i8, String)> {
    let rest = latex.trim_start().strip_prefix("\\lim_")?;
    let inner = rest.strip_prefix('{')?;
    let (sub, body) = {
        let mut depth = 1;
        let mut end = None;
        for (i, c) in inner.char_indices() {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(i);
                        break;
                    }
                }
                _ => {}
            }
        }
        let end = end?;
        (&inner[..end], &inner[end + 1..])
    };
    let at = sub.find("\\to")?;
    let var = sub[..at].trim().to_owned();
    let mut point = sub[at + 3..].trim().to_owned();
    let mut dir = 0;
    for (suffix, d) in [("^{+}", 1), ("^+", 1), ("^{-}", -1), ("^-", -1)] {
        if let Some(p) = point.strip_suffix(suffix) {
            point = p.trim().to_owned();
            dir = d;
            break;
        }
    }
    Some((var, point, dir, body.trim().to_owned()))
}

/// What to ask Giac for `latex`, given the other rows and the operation
/// asked for (an `op` of `giac:…`), if any.
pub fn plan(latex: &str, others: &[String], op: Option<&str>) -> Result<Plan, String> {
    let mut names = Names::default();
    // Definitions in the other rows: values and functions.
    let defs: Vec<Statement> = others.iter().filter_map(|o| crate::parse(o).ok()).collect();
    let funcs: HashSet<String> = defs
        .iter()
        .filter_map(|d| match d {
            Statement::Relation(Expr::Apply(f, _), Relation::Eq, _) => Some(f.clone()),
            _ => None,
        })
        .collect();

    let mut question = latex.trim().to_owned();
    let kind;
    let command;
    let mut choices = Vec::new();
    let mut chosen = op.and_then(|o| o.strip_prefix("giac:")).map(str::to_owned);
    let used: Vec<String>;

    if let Some((var_latex, point_latex, dir, body_latex)) = split_limit(latex) {
        let var = match crate::parse(&var_latex) {
            Ok(Statement::Expr(Expr::Var(v))) => v,
            _ => return Err("A limit needs a variable, like \\lim_{x\\to 0}".into()),
        };
        let point = match crate::parse(&point_latex) {
            Ok(Statement::Expr(e)) => e,
            _ => return Err("A limit needs a point it goes to".into()),
        };
        let body = match crate::parse(&body_latex) {
            Ok(Statement::Expr(e)) => e,
            Ok(_) => return Err("Take the limit of an expression".into()),
            Err(e) => return Err(e),
        };
        used = free_names(&body);
        let mut w = Writer {
            names: &mut names,
            funcs: &funcs,
        };
        let v = w.names.var(&var);
        let p = w.expr(&point)?;
        let b = w.expr(&body)?;
        command = if dir == 0 {
            format!("limit({b},{v},{p})")
        } else {
            format!("limit({b},{v},{p},{dir})")
        };
        let point_value = number_of(&point_latex, others);
        kind = Kind::Limit {
            var,
            body: body_latex,
            point: point_value,
            dir,
        };
        chosen = Some("limit".into());
    } else {
        let statement = crate::parse(latex)?;
        match statement {
            Statement::Expr(e) => {
                used = free_names(&e);
                let var = main_var(&used);
                let mut w = Writer {
                    names: &mut names,
                    funcs: &funcs,
                };
                let g = w.expr(&e)?;
                let v = w.names.var(&var);
                match (&e, chosen.as_deref()) {
                    (Expr::Integral(iv, lo, ..), None) => {
                        if matches!(&**lo, Expr::Var(n) if n == crate::ast::INDEFINITE) {
                            let Expr::Integral(_, _, _, body) = &e else {
                                unreachable!()
                            };
                            kind = Kind::Antiderivative {
                                var: iv.clone(),
                                body: w.latex(body)?,
                            };
                        } else {
                            kind = Kind::Definite {
                                question: latex.into(),
                            };
                        }
                        command = g;
                        chosen = Some("integrate".into());
                    }
                    (Expr::Derivative(..), None) => {
                        kind = Kind::Derivative;
                        command = format!("simplify({g})");
                        chosen = Some("diff".into());
                    }
                    (Expr::Repeat(..), None) => {
                        kind = Kind::Sum {
                            question: latex.into(),
                        };
                        command = format!("simplify({g})");
                        chosen = Some("sum".into());
                    }
                    (_, what) => {
                        let what = what.unwrap_or("simplify").to_owned();
                        command = match what.as_str() {
                            "factor" => format!("factor({g})"),
                            "expand" => format!("expand({g})"),
                            "partfrac" => format!("partfrac({g},{v})"),
                            "series" => format!("series({g},{v},0,6)"),
                            "laplace" => format!("laplace({g},{v},{})", w.names.var("s")),
                            "ilaplace" => format!("ilaplace({g},{v},{})", w.names.var("t")),
                            "integrate" => format!("integrate({g},{v})"),
                            "diff" => format!("diff({g},{v})"),
                            _ => format!("simplify({g})"),
                        };
                        kind = match what.as_str() {
                            "series" => Kind::Series,
                            "laplace" | "ilaplace" => Kind::Laplace,
                            "integrate" => Kind::Antiderivative {
                                var: var.clone(),
                                body: latex.into(),
                            },
                            "diff" => Kind::Derivative,
                            _ => Kind::Rewrite {
                                var: (!used.is_empty()).then(|| var.clone()),
                                original: latex.into(),
                            },
                        };
                        question = match what.as_str() {
                            "integrate" => format!("\\int {latex}\\ d{}", name_latex(&var)),
                            "diff" => format!(
                                "\\frac{{d}}{{d{}}}\\left({latex}\\right)",
                                name_latex(&var)
                            ),
                            "laplace" => format!("\\mathcal{{L}}\\left\\{{{latex}\\right\\}}"),
                            _ => latex.into(),
                        };
                        chosen = Some(what);
                        let vl = name_latex(&var);
                        if !used.is_empty() {
                            choices = vec![
                                choice("giac:simplify", "Simplify"),
                                choice("giac:factor", "Factor"),
                                choice("giac:expand", "Expand"),
                                choice("giac:partfrac", "Partial fractions"),
                                choice("giac:series", &format!("Series in ${vl}$")),
                                choice("giac:integrate", &format!("Integrate in ${vl}$")),
                                choice("giac:diff", &format!("Differentiate in ${vl}$")),
                            ];
                            if used.iter().any(|n| n == "t") {
                                choices.push(choice("giac:laplace", "Laplace transform"));
                            }
                            if used.iter().any(|n| n == "s") {
                                choices.push(choice("giac:ilaplace", "Inverse Laplace transform"));
                            }
                        }
                    }
                }
            }
            Statement::Relation(Expr::Apply(..), Relation::Eq, _) => {
                return Err(
                    "That line defines a function; Solve works on the lines that use it".into(),
                );
            }
            Statement::Relation(a, Relation::Eq, b) => {
                let both = Expr::Sub(Box::new(a.clone()), Box::new(b.clone()));
                used = free_names(&both);
                let mut has_prime = None;
                both.visit_names(&mut |n, _| {
                    if n.ends_with('\'') {
                        has_prime = Some(n.trim_end_matches('\'').to_owned());
                    }
                });
                let mut w = Writer {
                    names: &mut names,
                    funcs: &funcs,
                };
                let (ga, gb) = (w.expr(&a)?, w.expr(&b)?);
                if let Some(dep) = has_prime {
                    let others: Vec<String> = used.iter().filter(|n| **n != dep).cloned().collect();
                    let var = main_var(&others);
                    let (gv, gd) = (w.names.var(&var), w.names.var(&dep));
                    command = format!("desolve({ga}={gb},{gv},{gd})");
                    kind = Kind::Ode { dep };
                    chosen = Some("desolve".into());
                } else {
                    let var = main_var(&used);
                    let gv = w.names.var(&var);
                    command = format!("solve({ga}={gb},{gv})");
                    kind = Kind::Solve {
                        var: var.clone(),
                        lhs: latex_of(&ga, w.names)?,
                        rhs: latex_of(&gb, w.names)?,
                    };
                    chosen = Some("solve".into());
                    choices = vec![choice(
                        "giac:solve",
                        &format!("Solve for ${}$", name_latex(&var)),
                    )];
                }
            }
            _ => return Err("Giac solves equations with =".into()),
        }
    }

    // Definitions the problem reads, in the order they were written.
    let mut setup = vec!["restart".to_owned()];
    for d in &defs {
        let mut w = Writer {
            names: &mut names,
            funcs: &funcs,
        };
        match d {
            Statement::Relation(Expr::Var(n), Relation::Eq, rhs)
                if !matches!(n.as_str(), "x" | "y" | "pi" | "e") && used.iter().any(|u| u == n) =>
            {
                if let Ok(r) = w.expr(rhs) {
                    let n = w.names.var(n);
                    setup.push(format!("{n}:={r}"));
                }
            }
            Statement::Relation(Expr::Apply(f, params), Relation::Eq, body) => {
                let params: Option<Vec<String>> = params
                    .iter()
                    .map(|p| match p {
                        Expr::Var(v) => Some(w.names.var(v)),
                        _ => None,
                    })
                    .collect();
                if let (Some(params), Ok(body)) = (params, w.expr(body)) {
                    let f = w.names.var(f);
                    setup.push(format!("{f}({}):={body}", params.join(",")));
                }
            }
            _ => {}
        }
    }

    Ok(Plan {
        setup,
        command,
        op: format!("giac:{}", chosen.unwrap_or_default()),
        choices,
        names,
        kind,
        question,
        others: others.to_vec(),
    })
}

fn name_latex(name: &str) -> String {
    let (base, sub) = match name.split_once('_') {
        Some((b, s)) => (b, Some(s)),
        None => (name, None),
    };
    let base = if base.chars().count() > 1 {
        format!("\\{base}")
    } else {
        base.to_owned()
    };
    match sub {
        Some(s) => format!("{base}_{{{s}}}"),
        None => base,
    }
}

// ---------------------------------------------------------------------------
// Giac's answers back to LaTeX

#[derive(Clone, Debug, PartialEq)]
enum G {
    Num(String),
    Sym(String),
    Neg(Box<G>),
    Add(Vec<G>),
    Mul(Vec<G>),
    Div(Box<G>, Box<G>),
    Pow(Box<G>, Box<G>),
    Call(String, Vec<G>),
    List(Vec<G>),
    Eq(Box<G>, Box<G>),
    Fact(Box<G>),
}

struct Reader<'a> {
    s: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn skip(&mut self) {
        while self.at < self.s.len() && self.s[self.at] == b' ' {
            self.at += 1;
        }
    }
    fn peek(&mut self) -> Option<u8> {
        self.skip();
        self.s.get(self.at).copied()
    }
    fn eat(&mut self, c: u8) -> bool {
        if self.peek() == Some(c) {
            self.at += 1;
            true
        } else {
            false
        }
    }
    fn top(&mut self) -> Result<G, String> {
        let a = self.sum()?;
        if self.eat(b'=') {
            let b = self.sum()?;
            return Ok(G::Eq(Box::new(a), Box::new(b)));
        }
        Ok(a)
    }
    fn sum(&mut self) -> Result<G, String> {
        let mut terms = vec![self.product()?];
        loop {
            if self.eat(b'+') {
                terms.push(self.product()?);
            } else if self.peek() == Some(b'-') {
                self.at += 1;
                terms.push(G::Neg(Box::new(self.product()?)));
            } else {
                break;
            }
        }
        Ok(if terms.len() == 1 {
            terms.pop().unwrap()
        } else {
            G::Add(terms)
        })
    }
    fn product(&mut self) -> Result<G, String> {
        let mut acc = self.unary()?;
        loop {
            if self.eat(b'*') {
                let next = self.unary()?;
                acc = match acc {
                    G::Mul(mut v) => {
                        v.push(next);
                        G::Mul(v)
                    }
                    other => G::Mul(vec![other, next]),
                };
            } else if self.eat(b'/') {
                let next = self.unary()?;
                acc = G::Div(Box::new(acc), Box::new(next));
            } else {
                break;
            }
        }
        Ok(acc)
    }
    fn unary(&mut self) -> Result<G, String> {
        if self.eat(b'-') {
            return Ok(G::Neg(Box::new(self.unary()?)));
        }
        if self.eat(b'+') {
            return self.unary();
        }
        self.power()
    }
    fn power(&mut self) -> Result<G, String> {
        let mut base = self.atom()?;
        while self.eat(b'!') {
            base = G::Fact(Box::new(base));
        }
        if self.eat(b'^') {
            let exp = self.unary()?;
            return Ok(G::Pow(Box::new(base), Box::new(exp)));
        }
        Ok(base)
    }
    fn atom(&mut self) -> Result<G, String> {
        match self.peek() {
            Some(b'(') => {
                self.at += 1;
                let e = self.top()?;
                if !self.eat(b')') {
                    return Err("unbalanced ( in Giac's answer".into());
                }
                Ok(e)
            }
            Some(b'[') => {
                self.at += 1;
                Ok(G::List(self.items(b']')?))
            }
            Some(c) if c.is_ascii_digit() || c == b'.' => {
                let start = self.at;
                while self.at < self.s.len()
                    && (self.s[self.at].is_ascii_digit() || self.s[self.at] == b'.')
                {
                    self.at += 1;
                }
                if self.at < self.s.len() && (self.s[self.at] == b'e' || self.s[self.at] == b'E') {
                    let mark = self.at;
                    self.at += 1;
                    if self.at < self.s.len()
                        && (self.s[self.at] == b'-' || self.s[self.at] == b'+')
                    {
                        self.at += 1;
                    }
                    if self.at < self.s.len() && self.s[self.at].is_ascii_digit() {
                        while self.at < self.s.len() && self.s[self.at].is_ascii_digit() {
                            self.at += 1;
                        }
                    } else {
                        self.at = mark;
                    }
                }
                Ok(G::Num(
                    String::from_utf8_lossy(&self.s[start..self.at]).into(),
                ))
            }
            Some(c) if c.is_ascii_alphabetic() || c == b'_' => {
                let start = self.at;
                while self.at < self.s.len()
                    && (self.s[self.at].is_ascii_alphanumeric() || self.s[self.at] == b'_')
                {
                    self.at += 1;
                }
                let name = String::from_utf8_lossy(&self.s[start..self.at]).into_owned();
                // list[1,2] and set[1,2] are lists.
                if matches!(name.as_str(), "list" | "set" | "seq" | "poly1")
                    && self.peek() == Some(b'[')
                {
                    self.at += 1;
                    return Ok(G::List(self.items(b']')?));
                }
                if self.peek() == Some(b'(') {
                    self.at += 1;
                    return Ok(G::Call(name, self.items(b')')?));
                }
                Ok(G::Sym(name))
            }
            _ => Err("Giac's answer couldn't be read".into()),
        }
    }
    fn items(&mut self, close: u8) -> Result<Vec<G>, String> {
        let mut out = Vec::new();
        if self.eat(close) {
            return Ok(out);
        }
        loop {
            out.push(self.top()?);
            if self.eat(b',') {
                continue;
            }
            if self.eat(close) {
                return Ok(out);
            }
            return Err("Giac's answer couldn't be read".into());
        }
    }
}

fn read_giac(text: &str) -> Result<G, String> {
    let mut r = Reader {
        s: text.trim().as_bytes(),
        at: 0,
    };
    let g = r.top()?;
    r.skip();
    if r.at != r.s.len() {
        return Err("Giac's answer couldn't be read".into());
    }
    Ok(g)
}

const GREEK: &[&str] = &[
    "alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta", "theta", "iota", "kappa",
    "lambda", "mu", "nu", "xi", "pi", "rho", "sigma", "tau", "upsilon", "phi", "chi", "psi",
    "omega", "Gamma", "Delta", "Theta", "Lambda", "Xi", "Pi", "Sigma", "Upsilon", "Phi", "Psi",
    "Omega",
];

struct Printer<'a> {
    names: &'a Names,
}

impl Printer<'_> {
    fn sym(&self, s: &str) -> Result<String, String> {
        Ok(match s {
            "pi" => "\\pi".into(),
            "e" => "e".into(),
            "i" => "i".into(),
            "inf" | "infinity" | "plus_inf" => "\\infty".into(),
            "minus_inf" => "-\\infty".into(),
            "undef" => return Err("Giac's answer is undefined".into()),
            "euler_gamma" => "\\gamma".into(),
            _ => {
                let name = self.names.original(s).unwrap_or(s);
                if let Some(rest) = name.strip_prefix("g_") {
                    return Ok(name_latex(rest));
                }
                let (base, sub) = match name.split_once('_') {
                    Some((b, sub)) => (b, Some(sub)),
                    None => (name, None),
                };
                let base = if GREEK.contains(&base) {
                    format!("\\{base}")
                } else if base.chars().count() > 1 {
                    format!("\\operatorname{{{base}}}")
                } else {
                    base.to_owned()
                };
                match sub {
                    Some(sub) => format!("{base}_{{{sub}}}"),
                    None => base,
                }
            }
        })
    }

    fn paren(&self, g: &G) -> Result<String, String> {
        Ok(format!("\\left({}\\right)", self.latex(g)?))
    }

    /// Whether `g` needs brackets as a factor of a product.
    fn loose(g: &G) -> bool {
        matches!(g, G::Add(_) | G::Neg(_) | G::Eq(..))
    }

    fn latex(&self, g: &G) -> Result<String, String> {
        Ok(match g {
            G::Num(n) => n.clone(),
            G::Sym(s) => self.sym(s)?,
            G::Neg(a) => match &**a {
                G::Add(_) => format!("-{}", self.paren(a)?),
                _ => format!("-{}", self.latex(a)?),
            },
            G::Add(terms) => {
                let mut out = String::new();
                for (k, t) in terms.iter().enumerate() {
                    let piece = self.latex(t)?;
                    if k > 0 && !piece.starts_with('-') {
                        out.push('+');
                    }
                    out.push_str(&piece);
                }
                out
            }
            G::Mul(factors) => {
                // A leading -1 is a minus sign.
                let mut out = String::new();
                let mut prev_num = false;
                for (k, f) in factors.iter().enumerate() {
                    let piece = if Self::loose(f) || (k > 0 && matches!(f, G::Num(_)) && false) {
                        self.paren(f)?
                    } else {
                        self.latex(f)?
                    };
                    let is_num = matches!(f, G::Num(_))
                        || matches!(f, G::Div(a, _) if matches!(**a, G::Num(_)));
                    if k > 0
                        && (is_num || prev_num && piece.starts_with(|c: char| c.is_ascii_digit()))
                    {
                        out.push_str("\\cdot ");
                    }
                    out.push_str(&piece);
                    prev_num = is_num;
                }
                out
            }
            G::Div(a, b) => format!("\\frac{{{}}}{{{}}}", self.latex(a)?, self.latex(b)?),
            G::Pow(a, b) => {
                if let (G::Call(f, args), true) = (&**a, true) {
                    // sin(x)^2 is \sin^2(x).
                    if args.len() == 1
                        && matches!(
                            f.as_str(),
                            "sin" | "cos" | "tan" | "sinh" | "cosh" | "tanh" | "ln"
                        )
                    {
                        if let G::Num(n) = &**b {
                            return Ok(format!(
                                "\\{f}^{{{n}}}\\left({}\\right)",
                                self.latex(&args[0])?
                            ));
                        }
                    }
                }
                let base = match &**a {
                    G::Num(n) if !n.contains('.') => n.clone(),
                    G::Sym(_) => self.latex(a)?,
                    G::Call(..) => self.latex(a)?,
                    _ => self.paren(a)?,
                };
                // x^(1/2) is √x.
                if let G::Div(n, d) = &**b {
                    if **n == G::Num("1".into()) && **d == G::Num("2".into()) {
                        return Ok(format!("\\sqrt{{{}}}", self.latex(a)?));
                    }
                }
                format!("{base}^{{{}}}", self.latex(b)?)
            }
            G::Fact(a) => match &**a {
                G::Num(_) | G::Sym(_) => format!("{}!", self.latex(a)?),
                _ => format!("{}!", self.paren(a)?),
            },
            G::Eq(a, b) => format!("{}={}", self.latex(a)?, self.latex(b)?),
            G::List(items) => format!(
                "\\left[{}\\right]",
                items
                    .iter()
                    .map(|i| self.latex(i))
                    .collect::<Result<Vec<_>, _>>()?
                    .join(",")
            ),
            G::Call(f, args) => self.call(f, args)?,
        })
    }

    fn call(&self, f: &str, args: &[G]) -> Result<String, String> {
        let arg = |k: usize| -> Result<String, String> {
            args.get(k)
                .map_or(Err("missing argument".into()), |a| self.latex(a))
        };
        let all = || -> Result<String, String> {
            Ok(args
                .iter()
                .map(|a| self.latex(a))
                .collect::<Result<Vec<_>, _>>()?
                .join(","))
        };
        let wrap = |name: &str| -> Result<String, String> {
            Ok(format!("{name}\\left({}\\right)", all()?))
        };
        Ok(match f {
            "sqrt" => format!("\\sqrt{{{}}}", arg(0)?),
            "exp" => format!("e^{{{}}}", arg(0)?),
            "abs" => format!("\\left|{}\\right|", arg(0)?),
            "sin" | "cos" | "tan" | "sinh" | "cosh" | "tanh" | "ln" | "sec" | "csc" | "cot" => {
                wrap(&format!("\\{f}"))?
            }
            "asin" | "acos" | "atan" => wrap(&format!("\\arc{}", &f[1..]))?,
            "asinh" | "acosh" | "atanh" => wrap(&format!("\\operatorname{{ar{}}}", &f[1..]))?,
            "log10" => wrap("\\log")?,
            "Gamma" => wrap("\\Gamma")?,
            "Psi" => wrap("\\psi")?,
            "Beta" => wrap("B")?,
            "Zeta" => wrap("\\zeta")?,
            "erf" | "erfc" | "Si" | "Ci" | "Ei" | "sign" | "floor" | "ceil" | "max" | "min" => {
                wrap(&format!("\\operatorname{{{f}}}"))?
            }
            "Li" => wrap("\\operatorname{li}")?,
            "LambertW" => wrap("W")?,
            "Airy_Ai" => wrap("\\operatorname{Ai}")?,
            "Airy_Bi" => wrap("\\operatorname{Bi}")?,
            "BesselJ" | "BesselY" | "BesselI" | "BesselK" if args.len() == 2 => {
                format!("{}_{{{}}}\\left({}\\right)", &f[6..], arg(0)?, arg(1)?)
            }
            "factorial" => self.latex(&G::Fact(Box::new(args[0].clone())))?,
            "comb" => format!("\\binom{{{}}}{{{}}}", arg(0)?, arg(1)?),
            "Heaviside" => wrap("\\operatorname{H}")?,
            "Dirac" => wrap("\\delta")?,
            "order_size" => return Err("order_size".into()),
            "rootof" | "piecewise" | "when" | "ifte" | "integrate" | "sum" | "limit" => {
                return Err(format!("Giac answered with {f}, which can't be shown yet"))
            }
            other => {
                // A function the person defined.
                if let Some(name) = self.names.original(other) {
                    format!("{}\\left({}\\right)", name_latex(name), all()?)
                } else {
                    wrap(&format!("\\operatorname{{{other}}}"))?
                }
            }
        })
    }
}

/// A series: the terms, and O(x^n) for the `x^n*order_size(x)` term.
fn series_latex(g: &G, p: &Printer) -> Result<String, String> {
    let is_order = |t: &G| match t {
        G::Mul(fs) => fs
            .iter()
            .any(|f| matches!(f, G::Call(n, _) if n == "order_size")),
        _ => false,
    };
    let terms: Vec<G> = match g {
        G::Add(ts) => ts.clone(),
        other => vec![other.clone()],
    };
    let mut out = String::new();
    for t in &terms {
        let piece = if is_order(t) {
            let G::Mul(fs) = t else { unreachable!() };
            let rest: Vec<G> = fs
                .iter()
                .filter(|f| !matches!(f, G::Call(n, _) if n == "order_size"))
                .cloned()
                .collect();
            let inner = if rest.len() == 1 {
                rest[0].clone()
            } else {
                G::Mul(rest)
            };
            format!("O\\left({}\\right)", p.latex(&inner)?)
        } else {
            p.latex(t)?
        };
        if !out.is_empty() && !piece.starts_with('-') {
            out.push('+');
        }
        out.push_str(&piece);
    }
    Ok(out)
}

/// Giac's answer (in its own syntax) in LaTeX that this crate can read.
pub fn latex_of(text: &str, names: &Names) -> Result<String, String> {
    let g = read_giac(text)?;
    let p = Printer { names };
    p.latex(&g).or_else(|e| {
        if e == "order_size" {
            series_latex(&g, &p)
        } else {
            Err(e)
        }
    })
}

/// Giac's own LaTeX, tidied for MathQuill, for answers `latex_of` can't
/// write.
fn tidy_tex(tex: &str, names: &Names) -> String {
    let mut s = tex.trim().trim_matches('$').trim().to_owned();
    for (from, to) in [
        ("\\mathrm{", "\\operatorname{"),
        ("\\mbox{", "\\text{"),
        ("\\displaystyle", ""),
        ("\\,", " "),
    ] {
        s = s.replace(from, to);
    }
    let mut names: Vec<(&String, &String)> = names.back.iter().collect();
    names.sort_by_key(|(g, _)| std::cmp::Reverse(g.len()));
    for (g, original) in names {
        s = s.replace(g.as_str(), &name_latex(original));
    }
    s
}

// ---------------------------------------------------------------------------
// Numeric checks

/// The values of `rows` (LaTeX), worked out together; the last `n` are
/// read.
fn values(rows: &[String]) -> Vec<Option<(f64, f64)>> {
    let input: Vec<RowInput> = rows
        .iter()
        .enumerate()
        .map(|(i, l)| RowInput {
            id: i as u64,
            latex: l.clone(),
            ..Default::default()
        })
        .collect();
    let out = std::panic::catch_unwind(|| analyze(&input)).unwrap_or_default();
    out.iter()
        .map(|r| match &r.value {
            Some(Value::Number(v)) if v.is_finite() => Some((*v, 0.0)),
            Some(Value::Complex(a, b)) if a.is_finite() && b.is_finite() => Some((*a, *b)),
            _ => None,
        })
        .collect()
}

fn number_of(latex: &str, others: &[String]) -> Option<f64> {
    let t = latex.trim();
    if t == "\\infty" || t == "+\\infty" {
        return Some(f64::INFINITY);
    }
    if t == "-\\infty" {
        return Some(f64::NEG_INFINITY);
    }
    let mut rows = others.to_vec();
    rows.push(latex.to_owned());
    values(&rows).last().copied().flatten().map(|v| v.0)
}

fn close(a: (f64, f64), b: (f64, f64), tol: f64) -> bool {
    let d = ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt();
    let m = (b.0.powi(2) + b.1.powi(2)).sqrt();
    d <= tol * (1.0 + m)
}

#[derive(Debug, PartialEq)]
enum Check {
    Passed(usize),
    Failed,
    NotDone,
}

const POINTS: [f64; 6] = [0.37, 1.13, 2.41, -0.58, 0.83, 3.7];

/// Compares `f` and `g` (LaTeX in `var`) at a few points.
fn same_function(f: &str, g: &str, var: &str, others: &[String]) -> Check {
    let v = name_latex(var);
    let mut rows = others.to_vec();
    rows.push(format!("F_{{chk}}\\left({v}\\right)={f}"));
    rows.push(format!("G_{{chk}}\\left({v}\\right)={g}"));
    let base = rows.len();
    for p in POINTS {
        rows.push(format!("F_{{chk}}\\left({p}\\right)"));
        rows.push(format!("G_{{chk}}\\left({p}\\right)"));
    }
    let vals = values(&rows);
    let mut passed = 0;
    for k in 0..POINTS.len() {
        if let (Some(a), Some(b)) = (vals[base + 2 * k], vals[base + 2 * k + 1]) {
            if !close(a, b, 1e-6) {
                return Check::Failed;
            }
            passed += 1;
        }
    }
    if passed >= 2 {
        Check::Passed(passed)
    } else {
        Check::NotDone
    }
}

/// Checks F' = f at a few points, F' by central differences.
fn antiderivative_check(body: &str, result: &str, var: &str, others: &[String]) -> Check {
    let v = name_latex(var);
    let mut rows = others.to_vec();
    rows.push(format!("F_{{chk}}\\left({v}\\right)={result}"));
    rows.push(format!("G_{{chk}}\\left({v}\\right)={body}"));
    let base = rows.len();
    let h = 1e-5;
    for p in POINTS {
        rows.push(format!("F_{{chk}}\\left({}\\right)", p + h));
        rows.push(format!("F_{{chk}}\\left({}\\right)", p - h));
        rows.push(format!("G_{{chk}}\\left({p}\\right)"));
    }
    let vals = values(&rows);
    let mut passed = 0;
    for k in 0..POINTS.len() {
        if let (Some(a), Some(b), Some(f)) = (
            vals[base + 3 * k],
            vals[base + 3 * k + 1],
            vals[base + 3 * k + 2],
        ) {
            let d = ((a.0 - b.0) / (2.0 * h), (a.1 - b.1) / (2.0 * h));
            if !close(d, f, 1e-4) {
                return Check::Failed;
            }
            passed += 1;
        }
    }
    if passed >= 2 {
        Check::Passed(passed)
    } else {
        Check::NotDone
    }
}

fn value_check(question: &str, result: &str, others: &[String], tol: f64) -> Check {
    let mut rows = others.to_vec();
    rows.push(question.to_owned());
    rows.push(result.to_owned());
    let vals = values(&rows);
    match (vals[vals.len() - 2], vals[vals.len() - 1]) {
        (Some(a), Some(b)) if close(a, b, tol) => Check::Passed(1),
        (Some(_), Some(_)) => Check::Failed,
        _ => Check::NotDone,
    }
}

// ---------------------------------------------------------------------------
// The report

fn decimal(latex: &str, others: &[String]) -> Option<String> {
    let mut rows = others.to_vec();
    rows.push(latex.to_owned());
    let v = values(&rows).last().copied().flatten()?;
    if v.1 != 0.0 {
        return None;
    }
    let shown = crate::format::value_latex(v.0);
    (shown != latex).then_some(shown)
}

/// The Report for `plan`, given Giac's answer: its own syntax and its
/// LaTeX, or what went wrong.
pub fn finish(plan: &Plan, answer: Result<(String, String), String>) -> Report {
    let mut report = Report {
        op: plan.op.clone(),
        choices: plan.choices.clone(),
        ..Default::default()
    };
    let (text, tex) = match answer {
        Ok(a) => a,
        Err(e) => {
            report.error = Some(format!("Giac couldn't do this: {e}"));
            return report;
        }
    };
    let others = &plan.others;
    let ours = latex_of(&text, &plan.names);
    let shown = match &ours {
        Ok(l) => l.clone(),
        Err(_) => tidy_tex(&tex, &plan.names),
    };
    let readable = ours.is_ok();
    let q = &plan.question;
    let mut check = Check::NotDone;
    let mut note = "Giac works it out".to_owned();
    match &plan.kind {
        Kind::Antiderivative { var, body } => {
            if text.contains("integrate(") {
                report.error =
                    Some("Giac found no antiderivative in terms of the functions it knows".into());
                return report;
            }
            report.steps.push(Step {
                note: "Giac integrates it".into(),
                latex: format!("{q}={shown}+C"),
            });
            report.result = Some(format!("{shown}+C"));
            if readable {
                report.answer = Some(shown.clone());
                check = antiderivative_check(body, &shown, var, others);
            }
        }
        Kind::Definite { question } => {
            if text.contains("integrate(") {
                report.error = Some("Giac couldn't find this integral exactly".into());
                return report;
            }
            let mut result = shown.clone();
            if readable {
                if let Some(d) = decimal(&shown, others) {
                    result = format!("{shown}\\approx {d}");
                }
                check = value_check(question, &shown, others, 1e-6);
            }
            report.steps.push(Step {
                note: "Giac evaluates the integral".into(),
                latex: format!("{q}={shown}"),
            });
            report.result = Some(result);
        }
        Kind::Sum { question } => {
            let mut result = shown.clone();
            if readable {
                if let Some(d) = decimal(&shown, others) {
                    result = format!("{shown}\\approx {d}");
                }
                if !question.contains("\\infty") {
                    check = value_check(question, &shown, others, 1e-9);
                }
            }
            report.steps.push(Step {
                note: "Giac finds the sum".into(),
                latex: format!("{q}={shown}"),
            });
            report.result = Some(result);
        }
        Kind::Limit {
            var,
            body,
            point,
            dir,
        } => {
            note = "Giac takes the limit".into();
            report.steps.push(Step {
                note: note.clone(),
                latex: format!("{q}={shown}"),
            });
            report.result = Some(shown.clone());
            if readable {
                check = limit_check(var, body, *point, *dir, &shown, others);
            }
        }
        Kind::Solve { var, lhs, rhs } => {
            let v = name_latex(var);
            let items: Vec<G> = match read_giac(&text) {
                Ok(G::List(items)) => items,
                Ok(other) => vec![other],
                Err(_) => Vec::new(),
            };
            let p = Printer { names: &plan.names };
            let sols: Vec<String> = items.iter().filter_map(|g| p.latex(g).ok()).collect();
            if readable && sols.is_empty() {
                report.steps.push(Step {
                    note: "Giac solves it".into(),
                    latex: q.clone(),
                });
                report.result = Some(format!("\\text{{No solutions for }}{v}"));
                return report;
            }
            let joined = if readable {
                sols.iter()
                    .map(|s| format!("{v}={s}"))
                    .collect::<Vec<_>>()
                    .join("\\quad\\text{or}\\quad ")
            } else {
                format!("{v}\\in {shown}")
            };
            report.steps.push(Step {
                note: "Giac solves it".into(),
                latex: joined.clone(),
            });
            report.result = Some(joined);
            if readable {
                report.answer = Some(format!("{v}=\\left[{}\\right]", sols.join(",")));
                let mut all = Check::NotDone;
                for s in &sols {
                    match value_check(
                        &format!("L_{{chk}}\\left({s}\\right)"),
                        "0",
                        &[
                            others.clone(),
                            vec![format!(
                                "L_{{chk}}\\left({v}\\right)={lhs}-\\left({rhs}\\right)"
                            )],
                        ]
                        .concat(),
                        1e-6,
                    ) {
                        Check::Failed => {
                            all = Check::Failed;
                            break;
                        }
                        Check::Passed(_) => {
                            all = Check::Passed(match all {
                                Check::Passed(n) => n + 1,
                                _ => 1,
                            })
                        }
                        Check::NotDone => {}
                    }
                }
                check = all;
            }
        }
        Kind::Ode { dep } => {
            let d = name_latex(dep);
            let items: Vec<String> = match read_giac(&text) {
                Ok(G::List(items)) => {
                    let p = Printer { names: &plan.names };
                    items.iter().filter_map(|g| p.latex(g).ok()).collect()
                }
                _ => vec![shown.clone()],
            };
            let joined = items
                .iter()
                .map(|s| format!("{d}={s}"))
                .collect::<Vec<_>>()
                .join("\\quad\\text{or}\\quad ");
            report.steps.push(Step {
                note: "Giac solves the differential equation; $c_{0}$, $c_{1}$, … are constants"
                    .into(),
                latex: joined.clone(),
            });
            report.result = Some(joined);
        }
        Kind::Derivative => {
            report.steps.push(Step {
                note: "Giac differentiates it".into(),
                latex: format!("{q}={shown}"),
            });
            report.result = Some(shown.clone());
        }
        Kind::Rewrite { var, original } => {
            report.steps.push(Step {
                note: "Giac rewrites it".into(),
                latex: format!("{q}={shown}"),
            });
            report.result = Some(shown.clone());
            if let (Some(var), true) = (var, readable) {
                check = same_function(original, &shown, var, others);
            } else if readable {
                check = value_check(original, &shown, others, 1e-9);
            }
        }
        Kind::Series => {
            report.steps.push(Step {
                note: "Giac's series, to order 6".into(),
                latex: format!("{q}={shown}"),
            });
            report.result = Some(shown.clone());
        }
        Kind::Laplace => {
            report.steps.push(Step {
                note: "Giac transforms it".into(),
                latex: format!("{q}={shown}"),
            });
            report.result = Some(shown.clone());
        }
    }
    let _ = note;
    report.steps.push(Step {
        note: match check {
            Check::Passed(n) if n > 1 => format!("Checked numerically at {n} points"),
            Check::Passed(_) => "Checked numerically".into(),
            Check::Failed => {
                "The numeric check doesn't agree with this answer, so treat it with care".into()
            }
            Check::NotDone => "This answer couldn't be checked numerically".into(),
        },
        latex: String::new(),
    });
    report
}

fn limit_check(
    var: &str,
    body: &str,
    point: Option<f64>,
    dir: i8,
    result: &str,
    others: &[String],
) -> Check {
    let Some(point) = point else {
        return Check::NotDone;
    };
    let mut rows = others.to_vec();
    let v = name_latex(var);
    rows.push(format!("L_{{chk}}\\left({v}\\right)={body}"));
    rows.push(result.to_owned());
    let base = rows.len();
    let near: Vec<f64> = if point.is_infinite() {
        vec![1e6 * point.signum(), 1e7 * point.signum()]
    } else {
        match dir {
            1 => vec![point + 1e-6, point + 1e-7],
            -1 => vec![point - 1e-6, point - 1e-7],
            _ => vec![point + 1e-6, point - 1e-6],
        }
    };
    for p in &near {
        rows.push(format!(
            "L_{{chk}}\\left({}\\right)",
            crate::format::value_latex(*p)
        ));
    }
    let vals = values(&rows);
    let target = vals[base - 1];
    let Some(target) = target else {
        // An infinite limit: the values should grow.
        return Check::NotDone;
    };
    let mut passed = 0;
    for k in 0..near.len() {
        if let Some(v) = vals[base + k] {
            if !close(v, target, 1e-3) {
                return Check::Failed;
            }
            passed += 1;
        }
    }
    if passed > 0 {
        Check::Passed(passed)
    } else {
        Check::NotDone
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn giac_of(latex: &str) -> String {
        plan(latex, &[], None).unwrap().command
    }

    #[test]
    fn writes_problems_for_giac() {
        assert_eq!(
            giac_of("\\int_{ }^{ }x^2e^xdx"),
            "integrate(((g_x^2)*exp(g_x)),g_x)"
        );
        assert_eq!(
            giac_of("\\int_0^{\\infty}e^{-x^2}dx"),
            "integrate(exp((-(g_x^2))),g_x,0,inf)"
        );
        assert_eq!(giac_of("x^2-5x+6=0"), "solve((((g_x^2)-(5*g_x))+6)=0,g_x)");
        assert_eq!(
            giac_of("y''+y=\\sin\\left(x\\right)"),
            "desolve((g_y''+g_y)=sin(g_x),g_x,g_y)"
        );
        assert_eq!(
            giac_of("\\lim_{x\\to0}\\frac{\\sin x}{x}"),
            "limit((sin(g_x)/g_x),g_x,0)"
        );
        assert_eq!(
            giac_of("\\lim_{x\\to 0^{+}}x\\ln x"),
            "limit((g_x*ln(g_x)),g_x,0,1)"
        );
        assert_eq!(
            giac_of("\\sum_{n=1}^{\\infty}\\frac{1}{n^2}"),
            "simplify(sum((1/(g_n^2)),g_n,1,inf))"
        );
        assert_eq!(giac_of("0.25x"), "simplify(((25/100)*g_x))");
        assert_eq!(giac_of("\\Gamma\\left(x\\right)"), "simplify(Gamma(g_x))");
        assert_eq!(giac_of("J_{0}\\left(x\\right)"), "simplify(BesselJ(0,g_x))");
        let p = plan("\\frac{1}{x^2-1}", &[], Some("giac:partfrac")).unwrap();
        assert_eq!(p.command, "partfrac((1/((g_x^2)-1)),g_x)");
    }

    #[test]
    fn uses_definitions_from_other_rows() {
        let p = plan(
            "\\int_0^af\\left(x\\right)dx",
            &["a=2".into(), "f\\left(t\\right)=t^2".into(), "b=5".into()],
            None,
        )
        .unwrap();
        assert_eq!(p.setup, vec!["restart", "g_a:=2", "g_f(g_t):=(g_t^2)"]);
        assert_eq!(p.command, "integrate(g_f(g_x),g_x,0,g_a)");
    }

    #[test]
    fn reads_giac_answers() {
        let mut names = Names::default();
        names.var("x");
        let l = |t: &str| latex_of(t, &names).unwrap();
        assert_eq!(
            l("1/2*sqrt(pi)*erf(g_x)"),
            "\\frac{1}{2}\\sqrt{\\pi}\\operatorname{erf}\\left(x\\right)"
        );
        assert_eq!(
            l("g_x^2*exp(g_x)-2*g_x*exp(g_x)+2*exp(g_x)"),
            "x^{2}e^{x}-2\\cdot x e^{x}+2\\cdot e^{x}"
                .replace("\\cdot x e", "xe")
                .replace("\\cdot e", "e")
        );
        assert_eq!(l("[2,3]"), "\\left[2,3\\right]");
        assert_eq!(l("list[2,3]"), "\\left[2,3\\right]");
        assert_eq!(l("-(g_x+1)"), "-\\left(x+1\\right)");
        assert_eq!(l("sin(g_x)^2"), "\\sin^{2}\\left(x\\right)");
        assert_eq!(l("Gamma(1/3)"), "\\Gamma\\left(\\frac{1}{3}\\right)");
        assert_eq!(l("BesselJ(0,g_x)"), "J_{0}\\left(x\\right)");
        assert_eq!(l("c_0*cos(g_x)"), "c_{0}\\cos\\left(x\\right)");
        assert_eq!(l("pi^2/6"), "\\frac{\\pi^{2}}{6}");
        assert_eq!(l("+infinity"), "\\infty");
        assert_eq!(
            l("g_x+g_x^2+g_x^3/3+g_x^6*order_size(g_x)"),
            "x+x^{2}+\\frac{x^{3}}{3}+O\\left(x^{6}\\right)"
        );
        assert!(latex_of("rootof([1,0,-2],[1,0,1])", &names).is_err());
    }

    #[test]
    fn finishes_and_checks_answers() {
        // ∫x² eˣ dx, as Giac answers it.
        let p = plan("\\int_{ }^{ }x^2e^xdx", &[], None).unwrap();
        let r = finish(&p, Ok(("(g_x^2-2*g_x+2)*exp(g_x)".into(), String::new())));
        assert_eq!(
            r.result.as_deref(),
            Some("\\left(x^{2}-2x+2\\right)e^{x}+C"),
            "{r:?}"
        );
        assert_eq!(
            r.steps.last().unwrap().note,
            "Checked numerically at 6 points"
        );
        // A wrong answer is caught.
        let r = finish(&p, Ok(("(g_x^2+2)*exp(g_x)".into(), String::new())));
        assert!(
            r.steps.last().unwrap().note.contains("doesn't agree"),
            "{r:?}"
        );
        // A definite integral, with its decimal.
        let p = plan("\\int_0^{\\infty}x^2e^{-x^2}dx", &[], None).unwrap();
        let r = finish(&p, Ok(("sqrt(pi)/4".into(), String::new())));
        assert_eq!(
            r.result.as_deref(),
            Some("\\frac{\\sqrt{\\pi}}{4}\\approx 0.443113462726"),
            "{r:?}"
        );
        assert!(r.steps.last().unwrap().note.starts_with("Checked"), "{r:?}");
        // Equations.
        let p = plan("x^3-6x^2+11x-6=0", &[], None).unwrap();
        let r = finish(&p, Ok(("list[1,2,3]".into(), String::new())));
        assert_eq!(
            r.result.as_deref(),
            Some("x=1\\quad\\text{or}\\quad x=2\\quad\\text{or}\\quad x=3")
        );
        assert_eq!(
            r.steps.last().unwrap().note,
            "Checked numerically at 3 points",
            "{r:?}"
        );
        // Limits.
        let p = plan("\\lim_{x\\to0}\\frac{\\sin x}{x}", &[], None).unwrap();
        let r = finish(&p, Ok(("1".into(), String::new())));
        assert!(r.steps.last().unwrap().note.starts_with("Checked"), "{r:?}");
        // Special functions in the answer are checked too.
        let p = plan("\\int_{ }^{ }e^{-x^2}dx", &[], None).unwrap();
        let r = finish(&p, Ok(("1/2*sqrt(pi)*erf(g_x)".into(), String::new())));
        assert!(r.steps.last().unwrap().note.starts_with("Checked"), "{r:?}");
        // Rewrites.
        let p = plan("x^4-1", &[], Some("giac:factor")).unwrap();
        let r = finish(&p, Ok(("(g_x-1)*(g_x+1)*(g_x^2+1)".into(), String::new())));
        assert!(r.steps.last().unwrap().note.starts_with("Checked"), "{r:?}");
        // Errors.
        let r = finish(&p, Err("Bad argument".into()));
        assert!(r.error.is_some());
    }
}
