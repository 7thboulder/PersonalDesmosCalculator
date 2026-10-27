//! Symbolic algebra for the Solve button: simplify, expand, factor, solve,
//! differentiate and integrate one row exactly, writing out each step.
//! Nothing here runs while graphing; it only runs when asked.

mod convert;
pub mod diff;
pub mod integrate;
pub mod latex;
pub mod poly;
pub mod solve;
pub mod sym;

use std::cell::Cell;
use std::time::{Duration, Instant};

use crate::ast::{Expr, Relation, Statement};
use convert::{infinity, to_sym, Defs};
use latex::{latex, name};
use solve::Outcome;
use sym::*;

/// One line of working: what was done, and the result.
#[derive(Clone, Debug, PartialEq)]
pub struct Step {
    /// Plain words, with any maths between `$` signs.
    pub note: String,
    pub latex: String,
}

/// Another thing Solve can do with the same row.
#[derive(Clone, Debug, PartialEq)]
pub struct Choice {
    pub op: String,
    /// Plain words, with any maths between `$` signs.
    pub label: String,
}

#[derive(Clone, Debug, Default)]
pub struct Report {
    /// The operation that ran.
    pub op: String,
    pub choices: Vec<Choice>,
    pub steps: Vec<Step>,
    /// The final answer in words and maths, as the last line shows it.
    pub result: Option<String>,
    /// The answer as a row that can be added to the graph.
    pub answer: Option<String>,
    pub error: Option<String>,
}

/// The working being written down, or thrown away when `quiet`.
pub struct Steps {
    list: Vec<Step>,
    quiet: bool,
}

impl Steps {
    pub fn new() -> Steps {
        Steps {
            list: Vec::new(),
            quiet: false,
        }
    }

    pub fn quiet() -> Steps {
        Steps {
            list: Vec::new(),
            quiet: true,
        }
    }

    pub fn on(&self) -> bool {
        !self.quiet
    }

    pub fn push(&mut self, note: impl Into<String>, latex: impl Into<String>) {
        if !self.quiet {
            self.list.push(Step {
                note: note.into(),
                latex: latex.into(),
            });
        }
    }

    /// Where the working is now, to go back to if an attempt fails.
    pub fn mark(&self) -> usize {
        self.list.len()
    }

    pub fn undo(&mut self, mark: usize) {
        self.list.truncate(mark);
    }
}

impl Default for Steps {
    fn default() -> Steps {
        Steps::new()
    }
}

thread_local! {
    static DEADLINE: Cell<Option<Instant>> = const { Cell::new(None) };
}

/// How long one press of Solve may take before it gives up.
const BUDGET: Duration = Duration::from_millis(2000);

pub(crate) fn out_of_time() -> bool {
    DEADLINE.with(|d| d.get().is_some_and(|t| Instant::now() > t))
}

/// The tidiest of an expression's equivalent forms: as it is, multiplied
/// out, or over one denominator.
pub fn nice(s: &Sym) -> Sym {
    let mut best = s.clone();
    let mut best_len = latex(s).len();
    for c in [expand(s), together(&expand(s))] {
        let l = latex(&c).len();
        if l < best_len {
            best = c;
            best_len = l;
        }
    }
    best
}

fn normalized(latex: &str) -> String {
    latex
        .replace("\\left", "")
        .replace("\\right", "")
        .replace("\\cdot", "*")
        .replace(['{', '}', ' '], "")
}

/// A decimal for a constant that is not already a plain number.
fn approx(s: &Sym) -> Option<String> {
    if !s.is_constant() || s.as_q().is_some_and(Q::is_int) {
        return None;
    }
    let v = s.value();
    if !v.is_finite() {
        return None;
    }
    if let Sym::Real(_) = s {
        return None;
    }
    Some(crate::format::value_latex(v))
}

fn with_approx(s: &Sym) -> String {
    match approx(s) {
        Some(a) => format!("{}\\approx {a}", latex(s)),
        None => latex(s),
    }
}

/// What a row is, for deciding what Solve can do with it.
enum Form {
    /// A bare expression, or the right side of `a = ...`.
    Expr(Sym),
    /// `f(x) = ...` or `y = ...`: an expression in `var` that can have roots.
    Function { body: Sym, var: String },
    /// An equation or inequality to solve.
    Relation(Sym, Relation, Sym),
    /// `d/dx ...` as the whole row.
    Derivative { var: String, body: Sym, times: u32 },
    /// An integral as the whole row; no bounds means indefinite.
    Integral {
        var: String,
        body: Sym,
        bounds: Option<(Bound, Bound)>,
    },
}

#[derive(Clone)]
enum Bound {
    At(Sym),
    Infinite(f64),
}

/// Reads `\int x^2 dx`, which the graphing parser (like Desmos) rejects
/// for having no bounds.
fn indefinite(latex: &str, defs: &Defs) -> Option<Result<Form, String>> {
    let t = latex.trim();
    let mut rest = t.strip_prefix("\\int")?;
    // Typing int in the editor gives empty bounds: \int_{ }^{ }x dx.
    let compact: String = rest.chars().filter(|c| *c != ' ').collect();
    if compact.starts_with("_{}^{}") {
        let after_sub = &rest[rest.find('}')? + 1..];
        rest = &after_sub[after_sub.find('}')? + 1..];
    } else if rest.trim_start().starts_with('_') {
        return None;
    }
    let parsed = crate::parse(&format!("\\int_{{0}}^{{1}}{rest}")).ok()?;
    let Statement::Expr(Expr::Integral(v, _, _, body)) = parsed else {
        return None;
    };
    Some(to_sym(&body, defs).map(|body| Form::Integral {
        var: v,
        body,
        bounds: None,
    }))
}

fn read(latex: &str, defs: &Defs) -> Result<Form, String> {
    if let Some(form) = indefinite(latex, defs) {
        return form;
    }
    let statement = crate::parse(latex)?;
    Ok(match statement {
        Statement::Expr(Expr::Derivative(v, body)) => {
            // d/dx d/dx f counts as a second derivative.
            let mut times = 1;
            let mut body = *body;
            while let Expr::Derivative(w, inner) = &body {
                if *w != v {
                    break;
                }
                times += 1;
                body = (**inner).clone();
            }
            Form::Derivative {
                var: v,
                body: to_sym(&body, defs)?,
                times,
            }
        }
        Statement::Expr(Expr::Prime(f, k, args)) => {
            let (params, body) = defs
                .function(&f)
                .ok_or_else(|| format!("{f} isn't defined as a function"))?;
            if params.len() != 1 || args.len() != 1 {
                return Err("Primes work on functions of one variable".into());
            }
            let p = params[0].clone();
            let body = to_sym(body, defs)?;
            let arg = to_sym(&args[0], defs)?;
            let body = if arg == var(&p) {
                body
            } else {
                body.subs_var(&p, &arg)
            };
            let v = arg.names().first().cloned().unwrap_or(p);
            Form::Derivative {
                var: v,
                body,
                times: k,
            }
        }
        Statement::Expr(Expr::Integral(v, lo, hi, body)) => {
            let bound = |e: &Expr| -> Result<Bound, String> {
                Ok(match infinity(e) {
                    Some(inf) => Bound::Infinite(inf),
                    None => Bound::At(to_sym(e, defs)?),
                })
            };
            Form::Integral {
                bounds: Some((bound(&lo)?, bound(&hi)?)),
                body: to_sym(&body, defs)?,
                var: v,
            }
        }
        Statement::Expr(e) => Form::Expr(to_sym(&e, defs)?),
        // f(x) = ... defines f, unless another row already does.
        Statement::Relation(Expr::Apply(f, params), Relation::Eq, body)
            if params.len() == 1
                && matches!(&params[0], Expr::Var(_))
                && defs.function(&f).is_none() =>
        {
            let Expr::Var(p) = &params[0] else {
                unreachable!()
            };
            Form::Function {
                body: to_sym(&body, defs)?,
                var: p.clone(),
            }
        }
        Statement::Relation(Expr::Var(y), Relation::Eq, body)
            if (y == "y" && !body.mentions("y")) || (y == "r" && !body.mentions("r")) =>
        {
            let body = to_sym(&body, defs)?;
            let v =
                main_var(&body)
                    .unwrap_or_else(|| if y == "y" { "x".into() } else { "theta".into() });
            if body.is_constant() {
                Form::Expr(body)
            } else {
                Form::Function { body, var: v }
            }
        }
        Statement::Relation(Expr::Var(a), Relation::Eq, body)
            if !body.mentions(&a) && !matches!(a.as_str(), "x" | "y" | "z") =>
        {
            // a = 3 or a = 2b + 1: a definition, so simplify the right side.
            Form::Expr(to_sym(&body, defs)?)
        }
        Statement::Relation(l, rel, r) => Form::Relation(to_sym(&l, defs)?, rel, to_sym(&r, defs)?),
        Statement::Chain(_) => {
            return Err(
                "Solve can't do chained inequalities yet; solve each side on its own".into(),
            )
        }
        Statement::Regression(..) => {
            return Err("Regressions are fitted as you type; Solve has nothing to add".into())
        }
    })
}

/// The variable to work in: x if it is there, otherwise the first one.
fn main_var(s: &Sym) -> Option<String> {
    let names = s.names();
    for preferred in ["x", "t", "theta", "y"] {
        if names.iter().any(|n| n == preferred) {
            return Some(preferred.into());
        }
    }
    names.into_iter().next()
}

fn choice(op: &str, label: &str) -> Choice {
    Choice {
        op: op.into(),
        label: label.into(),
    }
}

fn choices(form: &Form) -> Vec<Choice> {
    let mut out = Vec::new();
    let expr_ops = |s: &Sym, v: Option<&str>, out: &mut Vec<Choice>| {
        out.push(choice("simplify", "Simplify"));
        if let Some(v) = v {
            if expand(s) != *s {
                out.push(choice("expand", "Expand"));
            }
            out.push(choice("factor", "Factor"));
            let x = name(v);
            let x = x.trim_start_matches('\\').trim();
            let x = if x == "theta" { "θ" } else { x };
            out.push(choice(&format!("diff:{v}"), &format!("d/d{x}")));
            out.push(choice(&format!("int:{v}"), &format!("∫ d{x}")));
        }
    };
    match form {
        Form::Expr(s) => {
            let v = main_var(s);
            expr_ops(s, v.as_deref(), &mut out);
            if let Some(v) = v {
                out.insert(1, choice(&format!("roots:{v}"), "Roots"));
            }
        }
        Form::Function { body, var: v, .. } => {
            out.push(choice(&format!("roots:{v}"), "Roots"));
            expr_ops(body, Some(v), &mut out);
        }
        Form::Relation(l, _, r) => {
            let mut names = l.names();
            for n in r.names() {
                if !names.contains(&n) {
                    names.push(n);
                }
            }
            names.sort_by_key(|n| match n.as_str() {
                "x" => 0,
                "y" => 1,
                _ => 2,
            });
            for v in names {
                out.push(choice(
                    &format!("solve:{v}"),
                    &format!("Solve for ${}$", name(&v)),
                ));
            }
        }
        Form::Derivative { .. } => out.push(choice("calc", "Differentiate")),
        Form::Integral { .. } => out.push(choice("calc", "Integrate")),
    }
    out
}

/// Runs Solve on one row. `others` are the other rows' LaTeX, so functions
/// they define can be used; `op` picks the operation, or the most likely
/// one when it is None.
pub fn run(latex_in: &str, others: &[String], op: Option<&str>) -> Report {
    DEADLINE.with(|d| d.set(Some(Instant::now() + BUDGET)));
    let report = run_inner(latex_in, others, op);
    DEADLINE.with(|d| d.set(None));
    report
}

fn run_inner(source: &str, others: &[String], op: Option<&str>) -> Report {
    let defs = Defs::from_rows(others);
    let mut report = Report::default();
    if source.trim().is_empty() {
        report.error = Some("Type an expression or equation first".into());
        return report;
    }
    let form = match read(source, &defs) {
        Ok(f) => f,
        Err(e) => {
            report.error = Some(e);
            return report;
        }
    };
    report.choices = choices(&form);
    let Some(default) = report.choices.first().map(|c| c.op.clone()) else {
        report.error = Some("There's nothing here for Solve to work on".into());
        return report;
    };
    let op = op
        .filter(|o| report.choices.iter().any(|c| c.op == *o))
        .map(str::to_owned)
        .unwrap_or(default);
    report.op = op.clone();
    let mut st = Steps::new();
    let (kind, v) = match op.split_once(':') {
        Some((k, v)) => (k.to_owned(), v.to_owned()),
        None => (op.clone(), String::new()),
    };
    let outcome: Result<(String, Option<String>), String> = match (&form, kind.as_str()) {
        (Form::Relation(l, rel, r), "solve") => {
            st.push(format!("Solve for ${}$", name(&v)), source.to_owned());
            if *rel == Relation::Eq {
                equation(l.clone(), r.clone(), &v, &mut st)
            } else {
                solve::inequality(l.clone(), *rel, r.clone(), &v, &mut st)
            }
        }
        (Form::Function { body, .. }, "roots") | (Form::Expr(body), "roots") => {
            let shown = format!("{}=0", latex(body));
            st.push(
                format!("Find where it equals zero: solve for ${}$", name(&v)),
                shown,
            );
            roots(body.clone(), &v, &mut st)
        }
        (Form::Function { body, .. }, k) | (Form::Expr(body), k) => {
            let original = source.split_once('=').map(|(_, r)| r).unwrap_or(source);
            let v = match (&form, v.is_empty()) {
                (Form::Function { var, .. }, true) => var.clone(),
                (_, true) => main_var(body).unwrap_or_default(),
                _ => v.clone(),
            };
            match k {
                "simplify" => simplify(body, original, &mut st),
                "expand" => expand_op(body, &mut st),
                "factor" => factor_op(body, &v, &mut st),
                "diff" => derivative_op(body, &v, 1, &mut st),
                "int" => integral_op(body, &v, None, &mut st),
                _ => Err("Unknown operation".into()),
            }
        }
        (
            Form::Derivative {
                var: v,
                body,
                times,
            },
            _,
        ) => derivative_op(body, v, *times, &mut st),
        (
            Form::Integral {
                var: v,
                body,
                bounds,
            },
            _,
        ) => integral_op(body, v, bounds.clone(), &mut st),
        _ => Err("Unknown operation".into()),
    };
    if out_of_time() {
        report.steps = st.list;
        report.error = Some("This took too long, so Solve stopped".into());
        return report;
    }
    report.steps = st.list;
    match outcome {
        Ok((result, answer)) => {
            report.result = Some(result);
            report.answer = answer;
        }
        Err(e) => report.error = Some(e),
    }
    report
}

// ---- Operations ------------------------------------------------------------------

type Done = Result<(String, Option<String>), String>;

fn simplify(s: &Sym, original: &str, st: &mut Steps) -> Done {
    st.push("Start with", original.to_owned());
    let mut best = s.clone();
    if normalized(&latex(&best)) != normalized(original) {
        st.push("Combine like terms and work out the numbers", latex(&best));
    }
    let x = expand(&best);
    if latex(&x).len() < latex(&best).len() {
        st.push("Multiply out and collect like terms", latex(&x));
        best = x;
    }
    let t = together(&best);
    if latex(&t).len() < latex(&best).len() {
        st.push("Combine into one fraction", latex(&t));
        best = t;
    }
    if let Some(v) = main_var(&best) {
        if let Some((c, g)) = cancel(&best, &v) {
            st.push(
                format!(
                    "Cancel the common factor ${}$ (the original is undefined where it is zero)",
                    latex(&g)
                ),
                latex(&c),
            );
            best = c;
        }
    }
    let result = with_approx(&best);
    if result != latex(&best) {
        st.push("As a decimal", result.clone());
    }
    let answer = (!best.has("i")).then(|| latex(&best));
    Ok((result, answer))
}

/// Cancels a polynomial factor shared by the top and bottom.
fn cancel(s: &Sym, v: &str) -> Option<(Sym, Sym)> {
    let (top, bottom) = num_den(s);
    if !bottom.has(v) {
        return None;
    }
    let pt = poly::rational(&poly::coeffs(&top, v)?)?;
    let pb = poly::rational(&poly::coeffs(&bottom, v)?)?;
    let g = poly::gcd_poly(&pt, &pb)?;
    if poly::deg(&g) == 0 {
        return None;
    }
    let (qt, _) = poly::divmod(&pt, &g)?;
    let (qb, _) = poly::divmod(&pb, &g)?;
    let gs = poly::factor(&g)
        .map(|f| f.to_sym(v))
        .unwrap_or_else(|| poly::build(&poly::to_sym(&g), v));
    let out = div(
        poly::build(&poly::to_sym(&qt), v),
        poly::build(&poly::to_sym(&qb), v),
    );
    Some((out, gs))
}

fn expand_op(s: &Sym, st: &mut Steps) -> Done {
    st.push("Start with", latex(s));
    let x = expand(s);
    st.push(
        "Multiply out every bracket and collect like terms",
        latex(&x),
    );
    Ok((latex(&x), Some(latex(&x))))
}

fn factor_op(s: &Sym, v: &str, st: &mut Steps) -> Done {
    st.push("Start with", latex(s));
    let x = var(v);
    if let Some(p) = poly::coeffs(s, v).and_then(|c| poly::rational(&c)) {
        if poly::deg(&p) == 0 {
            return Err("A number has no factors to take out".into());
        }
        let expanded = poly::build(&poly::to_sym(&p), v);
        if expanded != *s {
            st.push("Multiply out first", latex(&expanded));
        }
        let f = poly::factor(&p).ok_or("This is too big to factor")?;
        if f.unit != Q::ONE {
            let inside: Vec<Sym> = poly::to_sym(&p)
                .into_iter()
                .map(|c| div(c, Sym::Num(f.unit)))
                .collect();
            st.push(
                format!("Take out ${}$", latex(&Sym::Num(f.unit))),
                format!(
                    "{}\\left({}\\right)",
                    latex(&Sym::Num(f.unit)),
                    latex(&poly::build(&inside, v))
                ),
            );
        }
        let mut linear: Vec<&(Vec<i128>, u32)> =
            f.factors.iter().filter(|(g, _)| g.len() == 2).collect();
        linear.sort_by(|a, b| {
            let root = |g: &Vec<i128>| -g[0] as f64 / g[1] as f64;
            (root(&a.0) != 0.0)
                .cmp(&(root(&b.0) != 0.0))
                .then(root(&a.0).total_cmp(&root(&b.0)))
        });
        for (g, m) in linear {
            {
                let root = frac(-g[0], g[1]);
                let factor = poly::build(&[n(g[0]), n(g[1])], v);
                let times = if *m > 1 {
                    format!(" (it divides {m} times)")
                } else {
                    String::new()
                };
                if root.is_zero() {
                    st.push(
                        format!("Every term has ${}$, so take it out{times}", name(v)),
                        "",
                    );
                } else {
                    st.push(
                        format!(
                            "${}={}$ makes it zero, so ${}$ is a factor{times}",
                            name(v),
                            latex(&root),
                            latex(&factor)
                        ),
                        "",
                    );
                }
            }
        }
        let out = f.to_sym(v);
        if f.factors.len() == 1 && f.factors[0].1 == 1 && f.unit == Q::ONE {
            st.push(
                "This doesn't factor any further over the rational numbers",
                latex(&out),
            );
        } else {
            let leftover = f.factors.iter().any(|(g, _)| g.len() > 2);
            st.push(
                if leftover {
                    "Put the factors together (what is left has no rational roots)"
                } else {
                    "Put the factors together"
                },
                latex(&out),
            );
        }
        let _ = x;
        return Ok((latex(&out), Some(latex(&out))));
    }
    let c = solve::common_factor(&expand(s));
    if c != expand(s) {
        st.push("Take out the common factor", latex(&c));
        return Ok((latex(&c), Some(latex(&c))));
    }
    Err("Solve can't find anything to factor out".into())
}

fn derivative_op(s: &Sym, v: &str, times: u32, st: &mut Steps) -> Done {
    let mut cur = s.clone();
    for k in 1..=times {
        if times > 1 {
            st.push(format!("Derivative number {k}"), diff::dd(&cur, v));
        }
        let d = diff::derivative_steps(&cur, v, st);
        let shown = nice(&d);
        let together = format!("{}={}", diff::dd(&cur, v), latex(&d));
        if st.list.last().map(|s| &s.latex) != Some(&together) {
            st.push("Put it together", together);
        }
        if shown != d {
            st.push("Simplify", latex(&shown));
        }
        cur = shown;
    }
    Ok((latex(&cur), Some(latex(&cur))))
}

fn integral_op(s: &Sym, v: &str, bounds: Option<(Bound, Bound)>, st: &mut Steps) -> Done {
    let f = integrate::antiderivative(s, v, st);
    let Some((lo, hi)) = bounds else {
        let f = f.ok_or("Solve couldn't find an antiderivative for this")?;
        let f = nice(&f);
        st.push(
            "So, adding the constant of integration",
            format!("{}={}+C", integrate::ii(s, v), latex(&f)),
        );
        return Ok((format!("{}+C", latex(&f)), Some(latex(&f))));
    };
    let numeric_bounds = |b: &Bound| match b {
        Bound::At(x) if x.is_constant() => Some(x.value()),
        Bound::Infinite(i) => Some(*i),
        _ => None,
    };
    let only_v = s.names().iter().all(|n| n == v);
    let numeric = match (numeric_bounds(&lo), numeric_bounds(&hi)) {
        (Some(a), Some(b)) if only_v => Some(crate::calculus::integrate(&mut |t| s.at(v, t), a, b)),
        _ => None,
    };
    let finite = |b: &Bound| match b {
        Bound::At(x) => Some(x.clone()),
        Bound::Infinite(_) => None,
    };
    let bound_latex = |b: &Bound| match b {
        Bound::At(x) => latex(x),
        Bound::Infinite(i) if *i > 0.0 => "\\infty".into(),
        Bound::Infinite(_) => "-\\infty".into(),
    };
    if let (Some(a), Some(b), true) = (numeric_bounds(&lo), numeric_bounds(&hi), only_v) {
        if let Some(bad) = singularity(s, v, a, b) {
            st.push(
                format!(
                    "The integrand is undefined at ${}={}$, in the interval, so this is an improper integral and the antiderivative can't just be evaluated at the ends",
                    name(v),
                    latex(&bad)
                ),
                "",
            );
            return finish_numeric(numeric.unwrap_or(f64::NAN), st);
        }
    }
    match (f, finite(&lo), finite(&hi)) {
        (Some(f), Some(a), Some(b)) => {
            let f = nice(&f);
            let at_b = f.subs_var(v, &b);
            let at_a = f.subs_var(v, &a);
            st.push(
                "Evaluate the antiderivative at the top bound minus the bottom bound",
                format!(
                    "\\left[{}\\right]_{{{}}}^{{{}}}={}-{}",
                    latex(&f),
                    latex(&a),
                    latex(&b),
                    diff::paren(&at_b),
                    diff::paren(&at_a)
                ),
            );
            let value = nice(&sub(at_b, at_a));
            if let Some(num) = numeric {
                let exact = value.value();
                if value.is_constant() && !((exact - num).abs() <= 1e-6 * num.abs().max(1.0)) {
                    st.push(
                        "That doesn't match adding up the area numerically, so the antiderivative must jump inside the interval (the integrand isn't continuous there). The numerical value is",
                        crate::format::value_latex(num),
                    );
                    return Ok((
                        format!("\\approx {}", crate::format::value_latex(num)),
                        Some(crate::format::value_latex(num)),
                    ));
                }
            }
            let result = with_approx(&value);
            st.push("So", result.clone());
            Ok((result, Some(latex(&value))))
        }
        (f, _, _) => {
            let num = numeric.ok_or("Solve couldn't find an antiderivative for this")?;
            match &f {
                Some(f) => st.push(
                    "A bound is infinite, so take the limit of the antiderivative there",
                    format!(
                        "\\left[{}\\right]_{{{}}}^{{{}}}",
                        latex(&nice(f)),
                        bound_latex(&lo),
                        bound_latex(&hi)
                    ),
                ),
                None => st.push(
                    "Solve couldn't find an antiderivative, so it adds up the area numerically",
                    "",
                ),
            }
            finish_numeric(num, st)
        }
    }
}

/// A numerical value of an integral, recognized as a simple exact number
/// when it is one, or reported as diverging when it runs away.
fn finish_numeric(num: f64, st: &mut Steps) -> Done {
    if !num.is_finite() || num.abs() > 1e9 {
        st.push("The area grows without bound, so the integral diverges", "");
        return Ok(("\\text{diverges}".into(), None));
    }
    if let Some(exact) = recognize(num) {
        let result = with_approx(&exact);
        st.push("So", result.clone());
        return Ok((result, Some(latex(&exact))));
    }
    let shown = crate::format::value_latex(num);
    st.push("So", format!("\\approx {shown}"));
    Ok((format!("\\approx {shown}"), Some(shown)))
}

/// p/q or p/q·π for small q, when `v` is within rounding of one.
fn recognize(v: f64) -> Option<Sym> {
    for d in 1..=12i128 {
        for (unit, scale) in [(n(1), 1.0), (pi(), std::f64::consts::PI)] {
            let k = v * d as f64 / scale;
            if (k - k.round()).abs() < 1e-9 && k.abs() < 1e6 {
                return Some(mul2(frac(k.round() as i128, d), unit));
            }
        }
    }
    None
}

/// Where an integrand is undefined between a and b: a zero of its
/// denominator.
fn singularity(s: &Sym, v: &str, a: f64, b: f64) -> Option<Sym> {
    let (_, bottom) = num_den(s);
    if !bottom.has(v) {
        return None;
    }
    let (lo, hi) = if a < b { (a, b) } else { (b, a) };
    match solve::solve(expand(&bottom), n(0), v, &mut Steps::quiet(), 0) {
        Outcome::Roots(rs) => rs
            .into_iter()
            .find(|r| r.is_constant() && (lo - 1e-12..=hi + 1e-12).contains(&r.value())),
        _ => None,
    }
}

/// Solves f = 0 for the roots of a function or curve.
fn roots(f: Sym, v: &str, st: &mut Steps) -> Done {
    let (result, answer) = equation(f, n(0), v, st)?;
    // Roots of a curve are points on the x-axis, which the graph can show.
    let answer = answer.and_then(|a| {
        let values = a.split_once('=')?.1.to_owned();
        let list: Vec<String> = match values
            .strip_prefix("\\left[")
            .and_then(|s| s.strip_suffix("\\right]"))
        {
            Some(inner) => split_top(inner),
            None => vec![values],
        };
        let points: Vec<String> = list
            .iter()
            .map(|x| format!("\\left({x},0\\right)"))
            .collect();
        Some(if points.len() == 1 {
            points[0].clone()
        } else {
            format!("\\left[{}\\right]", points.join(","))
        })
    });
    Ok((result, answer))
}

/// Splits on commas that are not inside braces or brackets.
fn split_top(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut cur = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' | '(' | '[' => depth += 1,
            '}' | ')' | ']' => depth -= 1,
            ',' if depth == 0 => {
                out.push(std::mem::take(&mut cur));
                continue;
            }
            _ => {}
        }
        cur.push(c);
    }
    out.push(cur);
    out
}

fn equation(l: Sym, r: Sym, v: &str, st: &mut Steps) -> Done {
    let x = var(v);
    let xs = name(v);
    let outcome = solve::solve(l.clone(), r.clone(), v, st, 0);
    let found = match outcome {
        Outcome::All => return Ok((format!("\\text{{every }}{xs}"), None)),
        Outcome::Fail => return Err(format!("Solve couldn't find ${xs}$ here")),
        Outcome::Roots(r) => r,
    };
    // Drop repeats, and answers that don't work in the original equation.
    let k = solve::integer_name(v);
    let checkable = l.names().iter().chain(r.names().iter()).all(|n| n == v);
    let mut roots: Vec<Sym> = Vec::new();
    for root in found {
        let root = match root {
            Sym::Real(x) => Sym::Real(x),
            other => nice(&other),
        };
        let same = |a: &Sym| {
            *a == root
                || (a.is_constant()
                    && root.is_constant()
                    && (a.value() - root.value()).abs() < 1e-9)
        };
        if roots.iter().any(same) {
            continue;
        }
        if checkable && root.is_constant() && !root.has(k) {
            let t = root.value();
            let (a, b) = (l.at(v, t), r.at(v, t));
            let scale = a.abs().max(b.abs()).max(1.0);
            let exact = !matches!(root, Sym::Real(_));
            let tolerance = if exact { 1e-7 } else { 1e-5 };
            if !(a.is_finite() && b.is_finite() && (a - b).abs() <= tolerance * scale) {
                let why = if a.is_finite() && b.is_finite() {
                    "it doesn't work in the original equation"
                } else {
                    "the original equation is undefined there"
                };
                st.push(
                    format!("Check ${xs}={}$: {why}, so drop it", latex(&root)),
                    "",
                );
                continue;
            }
        }
        roots.push(root);
    }
    if roots.iter().all(Sym::is_constant) {
        roots.sort_by(|a, b| a.value().total_cmp(&b.value()));
    }
    if roots.is_empty() {
        st.push("So there is no real solution", "");
        return Ok(("\\text{no real solution}".into(), None));
    }
    let shown: Vec<String> = roots
        .iter()
        .map(|r| format!("{xs}={}", with_approx(r)))
        .collect();
    let result = shown.join("\\quad\\text{or}\\quad ");
    let general = roots.iter().any(|r| r.has(k));
    if general {
        st.push(format!("So, for any integer ${}$", name(k)), result.clone());
        // The ones in one turn, for a feel of where they are.
        let mut within: Vec<Sym> = Vec::new();
        for r in &roots {
            for j in -8..=8 {
                let s = nice(&r.subs_var(k, &n(j)));
                if s.is_constant() {
                    let t = s.value();
                    let two_pi = 2.0 * std::f64::consts::PI;
                    if t > -1e-12
                        && t < two_pi - 1e-12
                        && !within.iter().any(|w| (w.value() - t).abs() < 1e-9)
                    {
                        within.push(s);
                    }
                }
            }
        }
        within.sort_by(|a, b| a.value().total_cmp(&b.value()));
        if !within.is_empty() {
            let list: Vec<String> = within.iter().map(|w| with_approx(w)).collect();
            st.push(
                "The solutions from $0$ up to $2\\pi$ are".to_owned(),
                format!("{xs}={}", list.join(",\\ ")),
            );
        }
        return Ok((result, None));
    }
    st.push("So", result.clone());
    let answer =
        (roots.iter().all(Sym::is_constant) && !roots.iter().any(|r| r.has("i"))).then(|| {
            let values: Vec<String> = roots.iter().map(latex).collect();
            if values.len() == 1 {
                format!("{}={}", latex(&x), values[0])
            } else {
                format!("{}=\\left[{}\\right]", latex(&x), values.join(","))
            }
        });
    Ok((result, answer))
}

#[cfg(test)]
mod tests;
