//! Symbolic algebra for the Solve button: simplify, expand, factor, solve,
//! differentiate and integrate one row exactly, writing out each step.
//! Nothing here runs while graphing; it only runs when asked.

pub(crate) mod convert;
pub mod diff;
pub(crate) mod infinite;
pub mod integrate;
pub mod latex;
pub mod limits;
mod nfold;
pub mod ode;
pub mod pde;
pub mod poly;
pub mod solve;
pub mod sym;
pub(crate) mod trigint;

use std::cell::Cell;
use std::time::{Duration, Instant};

use crate::ast::{Expr, Relation, Statement};
use convert::{infinity, to_sym, Defs};

/// Replaces each integral written without bounds in a row with its
/// antiderivative (taking the constant as 0), so the graph can draw it.
/// `rows` are the rows' LaTeX, for functions they define.
pub fn resolve_indefinite(statement: Statement, rows: &[String]) -> Result<Statement, String> {
    let mut found = false;
    let mut look =
        |e: &Expr| found |= matches!(e, Expr::Integral(_, lo, _, _) if convert::is_indefinite(lo));
    match &statement {
        Statement::Expr(e) => e.walk(&mut look),
        Statement::Relation(a, _, b) | Statement::Regression(a, b) => {
            a.walk(&mut look);
            b.walk(&mut look);
        }
        Statement::Chain(c) => c.operands.iter().for_each(|e| e.walk(&mut look)),
    }
    if !found {
        return Ok(statement);
    }
    let defs = Defs::from_rows(rows);
    let mut error = None;
    let mut f = |e: Expr| match e {
        Expr::Integral(v, lo, hi, body) if convert::is_indefinite(&lo) => {
            let whole = Expr::Integral(v, lo, hi, body);
            DEADLINE.with(|d| d.set(Some(Instant::now() + Duration::from_millis(500))));
            let found = to_sym(&whole, &defs);
            DEADLINE.with(|d| d.set(None));
            match found.and_then(|s| match crate::parse(&latex(&s))? {
                Statement::Expr(e) => Ok(e),
                _ => Err("Couldn't graph this antiderivative".into()),
            }) {
                Ok(e) => Ok(e),
                Err(m) => {
                    error.get_or_insert(m);
                    Ok(Expr::Num(f64::NAN))
                }
            }
        }
        e => Err(e),
    };
    let mut r = |e: Expr| crate::units::rewrite(e, &mut f);
    let out = match statement {
        Statement::Expr(e) => Statement::Expr(r(e)),
        Statement::Relation(a, rel, b) => {
            let a = r(a);
            Statement::Relation(a, rel, r(b))
        }
        Statement::Regression(a, b) => {
            let a = r(a);
            Statement::Regression(a, r(b))
        }
        Statement::Chain(c) => Statement::Chain(crate::ast::Condition {
            operands: c.operands.into_iter().map(&mut r).collect(),
            relations: c.relations,
        }),
    };
    match error {
        Some(m) => Err(m),
        None => Ok(out),
    }
}
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
    // Over one denominator is rarely tidier for big expressions, and slow.
    let x = expand(s);
    let mut inexact = false;
    trigint::walk(&x, &mut |t| inexact |= matches!(t, Sym::Real(_)));
    let forms = if x.size() > 120 || inexact || out_of_time() {
        vec![x]
    } else {
        let t = together(&x);
        vec![x, t]
    };
    for c in forms {
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
    /// `d/dx ...` as the whole row: `vars` in the order they are applied,
    /// so ∂²/∂x∂y gives y then x.
    Derivative { vars: Vec<String>, body: Sym },
    /// `∇f`: the partial derivatives of `body` in each of `vars`.
    Gradient { vars: Vec<String>, body: Sym },
    /// A differential equation `eq` = 0 in y(x).
    Ode { order: usize, eq: Sym },
    /// A system x' = f, y' = g in t.
    System { f: Sym, g: Sym },
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
    if let Some(order) = crate::analyze::ode_order(&statement) {
        let Statement::Relation(l, Relation::Eq, r) = &statement else {
            return Err("Write a differential equation with =, like y' = x - y".into());
        };
        let is_system = |e: &Expr| matches!(e, Expr::Point(a, b) if matches!((&**a, &**b), (Expr::Var(p), Expr::Var(q)) if p == "x'" && q == "y'"));
        if is_system(l) || is_system(r) {
            let Expr::Point(f, g) = (if is_system(l) { r } else { l }) else {
                return Err("A system needs a point of slopes, like (y, -x)".into());
            };
            return Ok(Form::System {
                f: to_sym(f, defs)?,
                g: to_sym(g, defs)?,
            });
        }
        return Ok(Form::Ode {
            order,
            eq: sub(to_sym(l, defs)?, to_sym(r, defs)?),
        });
    }
    Ok(match statement {
        Statement::Expr(Expr::Derivative(v, body)) => {
            // d/dx d/dx f counts as a second derivative, and ∂/∂x ∂/∂y f
            // as a mixed one.
            let mut vars = vec![v];
            let mut body = *body;
            while let Expr::Derivative(w, inner) = &body {
                vars.push(w.clone());
                body = (**inner).clone();
            }
            vars.reverse();
            Form::Derivative {
                vars,
                body: to_sym(&convert::bare_function(&body, defs)?, defs)?,
            }
        }
        Statement::Expr(ref e @ (Expr::Point(..) | Expr::Point3(..))) => {
            // ∇f, as the parser writes it: one partial derivative of the same
            // body in each slot.
            let slots: Vec<&Expr> = match e {
                Expr::Point(a, b) => vec![&**a, &**b],
                Expr::Point3(a, b, c) => vec![&**a, &**b, &**c],
                _ => unreachable!(),
            };
            let mut vars = Vec::new();
            let mut body = None;
            for slot in slots {
                match slot {
                    Expr::Derivative(v, b) if body.is_none_or(|f| f == &**b) => {
                        vars.push(v.clone());
                        body = Some(&**b);
                    }
                    _ => {
                        return Err(
                            "Solve works on single expressions and equations, not on lists, points or piecewise definitions"
                                .into(),
                        )
                    }
                }
            }
            Form::Gradient {
                vars,
                body: to_sym(body.unwrap(), defs)?,
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
                vars: vec![v; k as usize],
                body,
            }
        }
        Statement::Expr(Expr::Integral(v, lo, _, body)) if convert::is_indefinite(&lo) => {
            Form::Integral {
                body: to_sym(&body, defs)?,
                var: v,
                bounds: None,
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
            let names = ordered_names(s);
            if names.len() > 1 {
                // Several variables: a partial derivative in each, the
                // gradient and the total differential.
                for w in &names {
                    let y = name(w);
                    let y = y.trim_start_matches('\\').trim();
                    let y = if y == "theta" { "θ" } else { y };
                    out.push(choice(&format!("diff:{w}"), &format!("∂/∂{y}")));
                }
                out.push(choice("grad", "Gradient ∇"));
                out.push(choice("differential", "Total differential"));
            } else {
                out.push(choice(&format!("diff:{v}"), &format!("d/d{x}")));
            }
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
        Form::Ode { .. } => {
            out.push(choice("ode", "Solve the equation"));
            out.push(choice("ode:general", "General solution"));
        }
        Form::System { .. } => {
            out.push(choice("ode", "Solve the system"));
            out.push(choice("ode:general", "General solution"));
        }
        Form::Derivative { .. } => out.push(choice("calc", "Differentiate")),
        Form::Gradient { .. } => out.push(choice("calc", "Find the gradient")),
        Form::Integral { .. } => out.push(choice("calc", "Integrate")),
    }
    out
}

/// Runs Solve on one row. `others` are the other rows' LaTeX, so functions
/// they define can be used; `op` picks the operation, or the most likely
/// one when it is None.
pub fn run(latex_in: &str, others: &[String], op: Option<&str>) -> Report {
    run_row(latex_in, others, op, &Initial::default())
}

/// A differential equation row's initial point and derivatives, as typed.
#[derive(Clone, Debug, Default)]
pub struct Initial {
    pub seeds: Option<String>,
    pub derivs: Vec<String>,
}

/// Like `run`, with the row's initial conditions for differential equations.
pub fn run_row(latex_in: &str, others: &[String], op: Option<&str>, initial: &Initial) -> Report {
    DEADLINE.with(|d| d.set(Some(Instant::now() + BUDGET)));
    let report = run_inner(latex_in, others, op, initial);
    DEADLINE.with(|d| d.set(None));
    report
}

/// The single initial point of a differential equation, with y'(x₀), … from
/// the derivative boxes (0 when empty, as the graph takes them).
fn initial_point(initial: &Initial, defs: &Defs, order: usize) -> Option<(Sym, Vec<Sym>)> {
    let seeds = initial.seeds.as_deref()?.trim();
    let Ok(Statement::Expr(Expr::Point(a, b))) = crate::parse(seeds) else {
        return None;
    };
    let x0 = to_sym(&a, defs).ok()?;
    let mut values = vec![to_sym(&b, defs).ok()?];
    for k in 0..order.saturating_sub(1) {
        let l = initial.derivs.get(k).map_or("", |s| s.trim());
        values.push(if l.is_empty() {
            n(0)
        } else {
            match crate::parse(l) {
                Ok(Statement::Expr(e)) => to_sym(&e, defs).ok()?,
                _ => return None,
            }
        });
    }
    Some((x0, values))
}

fn run_inner(source: &str, others: &[String], op: Option<&str>, initial: &Initial) -> Report {
    let defs = Defs::from_rows(others);
    let mut report = Report::default();
    if source.trim().is_empty() {
        report.error = Some("Type an expression or equation first".into());
        return report;
    }
    // The engine works limits out numerically; Giac finds them exactly.
    if source.contains("\\lim") {
        report.op = "limit".into();
        report.choices = vec![Choice {
            op: "limit".into(),
            label: "Take the limit".into(),
        }];
        let mut st = Steps::new();
        st.push("Take the limit", source.to_owned());
        match limits::solve(source, others, &defs, &mut st) {
            Ok(result) => {
                report.steps = st.list;
                report.result = Some(result);
            }
            Err(e) => report.error = Some(e),
        }
        return report;
    }
    if crate::dots::is_symbolic(source) {
        report.op = "nfold".into();
        report.choices = vec![Choice {
            op: "nfold".into(),
            label: "Work out the integral".into(),
        }];
        let mut st = Steps::new();
        st.push("Start with", source.to_owned());
        let outcome = nfold::solve(source, others, &defs, &mut st);
        report.steps = st.list;
        match outcome {
            Ok((result, answer)) => {
                report.result = Some(result);
                report.answer = answer;
            }
            Err(e) => report.error = Some(e),
        }
        return report;
    }
    // Partial differential equations, with conditions from other rows.
    if let Some(pde) = pde::read(source, others) {
        report.op = "pde".into();
        let general = op == Some("pde:general");
        let pde = match pde {
            Ok(p) => p,
            Err(e) => {
                report.error = Some(e);
                return report;
            }
        };
        report.choices = vec![choice("pde", "Solve the PDE")];
        if pde::has_conditions(&pde, others) {
            report
                .choices
                .push(choice("pde:general", "General solution"));
            if general {
                report.op = "pde:general".into();
            }
        }
        let mut st = Steps::new();
        st.push("Solve the partial differential equation", source.to_owned());
        match pde::solve(&pde, others, general, &mut st) {
            Ok((result, answer)) => {
                report.result = Some(result);
                report.answer = answer;
            }
            Err(e) => report.error = Some(e),
        }
        report.steps = st.list;
        return report;
    }
    // Sums and products out to ∞: numerically here, exactly by Giac.
    if let Some(series) = infinite::read(source) {
        let what = if series.product { "product" } else { "sum" };
        report.op = "calc".into();
        report.choices = vec![choice("calc", &format!("Work out the {what}"))];
        let mut st = Steps::new();
        match infinite::solve(source, others, &series, &mut st) {
            Ok((result, answer)) => {
                report.result = Some(result);
                report.answer = answer;
            }
            Err(e) => report.error = Some(e),
        }
        report.steps = st.list;
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
                "diff" => derivative_op(body, std::slice::from_ref(&v), &mut st),
                "grad" => gradient_op(body, &ordered_names(body), &mut st),
                "differential" => differential_op(body, &ordered_names(body), &mut st),
                "int" => integral_op(body, &v, None, &mut st),
                _ => Err("Unknown operation".into()),
            }
        }
        (Form::Ode { order, eq }, _) => {
            st.push("Solve the differential equation", source.to_owned());
            let conditions = (op == "ode")
                .then(|| initial_point(initial, &defs, *order))
                .flatten()
                .map(|(x0, values)| ode::Conditions { x0, values });
            ode::solve_ode(eq, *order, conditions.as_ref(), &mut st)
        }
        (Form::System { f, g }, _) => {
            st.push("Solve the system", source.to_owned());
            let start = (op == "ode")
                .then(|| initial_point(initial, &defs, 1))
                .flatten()
                .map(|(x0, v)| (x0, v[0].clone()));
            ode::solve_system(f, g, start, &mut st)
        }
        (Form::Derivative { vars, body }, _) => derivative_op(body, vars, &mut st),
        (Form::Gradient { vars, body }, _) => gradient_op(body, vars, &mut st),
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

/// Differentiates `s` in each of `vars` in turn. With other variables
/// about, these are partial derivatives: each of the others is held
/// constant, and the working is written with ∂.
fn derivative_op(s: &Sym, vars: &[String], st: &mut Steps) -> Done {
    let d = partial_derivative(s, vars, st)?;
    Ok((latex(&d), Some(latex(&d))))
}

fn partial_derivative(s: &Sym, vars: &[String], st: &mut Steps) -> Result<Sym, String> {
    let mut others: Vec<String> = s.names();
    others.retain(|n| !vars.contains(n));
    let mut distinct: Vec<&String> = Vec::new();
    for v in vars {
        if !distinct.contains(&v) {
            distinct.push(v);
        }
    }
    let partial = !others.is_empty() || distinct.len() > 1;
    let mut cur = s.clone();
    let times = vars.len();
    for (k, v) in vars.iter().enumerate() {
        let start = st.mark();
        if partial {
            let mut held: Vec<String> = ordered_names(s);
            held.retain(|n| n != v);
            if held.is_empty() {
                st.push(format!("Differentiate in ${}$", name(v)), "");
            } else {
                let held: Vec<String> = held.iter().map(|n| format!("${}$", name(n))).collect();
                st.push(
                    format!(
                        "Partial derivative in ${}$: hold {} constant",
                        name(v),
                        held.join(" and ")
                    ),
                    "",
                );
            }
        }
        if times > 1 {
            st.push(format!("Derivative number {}", k + 1), diff::dd(&cur, v));
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
        if partial {
            for step in &mut st.list[start..] {
                step.latex = to_partial(&step.latex);
            }
        }
        cur = shown;
    }
    if out_of_time() {
        return Err("This took too long, so Solve stopped".into());
    }
    Ok(cur)
}

/// The variables `s` reads, x, y and z first.
fn ordered_names(s: &Sym) -> Vec<String> {
    let mut names = s.names();
    names.sort_by_key(|n| match n.as_str() {
        "x" => 0,
        "y" => 1,
        "z" => 2,
        _ => 3,
    });
    names
}

/// `\frac{d}{dx}` written as `\frac{\partial}{\partial x}`.
fn to_partial(latex: &str) -> String {
    latex.replace("\\frac{d}{d", "\\frac{\\partial}{\\partial ")
}

/// ∇f: each partial derivative, then the vector of them.
fn gradient_op(s: &Sym, vars: &[String], st: &mut Steps) -> Done {
    if vars.len() < 2 {
        return Err("The gradient needs a function of two or three variables".into());
    }
    let mut parts = Vec::new();
    for v in vars {
        parts.push(partial_derivative(s, std::slice::from_ref(v), st)?);
    }
    let shown: Vec<String> = parts.iter().map(latex).collect();
    let names: Vec<String> = vars
        .iter()
        .map(|v| format!("\\frac{{\\partial f}}{{\\partial {}}}", name(v)))
        .collect();
    let vector = format!("\\left({}\\right)", shown.join(","));
    st.push(
        "The gradient is the vector of the partial derivatives",
        format!("\\nabla f=\\left({}\\right)={vector}", names.join(",")),
    );
    Ok((vector.clone(), Some(vector)))
}

/// df = f_x dx + f_y dy + …: how much f changes for small changes in each
/// variable.
fn differential_op(s: &Sym, vars: &[String], st: &mut Steps) -> Done {
    if vars.is_empty() {
        return Err("A differential needs a variable".into());
    }
    let mut terms = Vec::new();
    for v in vars {
        let d = partial_derivative(s, std::slice::from_ref(v), st)?;
        if d.is_zero() {
            continue;
        }
        let dv = format!("d{}", name(v));
        terms.push(if matches!(d, Sym::Add(_)) {
            format!("\\left({}\\right){dv}", latex(&d))
        } else if d == n(1) {
            dv
        } else if d == n(-1) {
            format!("-{dv}")
        } else {
            format!("{}\\ {dv}", latex(&d))
        });
    }
    let sum = if terms.is_empty() {
        "0".to_owned()
    } else {
        terms.join("+").replace("+-", "-")
    };
    st.push(
        "The total differential adds each partial derivative times a small change in its variable",
        format!("df={sum}"),
    );
    let result = format!("df={sum}");
    Ok((result.clone(), Some(sum)))
}

fn integral_op(s: &Sym, v: &str, bounds: Option<(Bound, Bound)>, st: &mut Steps) -> Done {
    let f = integrate::antiderivative(s, v, st);
    let Some((lo, hi)) = bounds else {
        let f = f.ok_or(if trigint::non_elementary(s, v) {
            "This has no antiderivative made of elementary functions; Solve can still find definite integrals of it numerically"
        } else {
            "Solve couldn't find an antiderivative for this"
        })?;
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
    let infinite_bound = matches!(lo, Bound::Infinite(_)) || matches!(hi, Bound::Infinite(_));
    let singular = match (numeric_bounds(&lo), numeric_bounds(&hi), only_v) {
        (Some(a), Some(b), true) => singularity(s, v, a, b),
        _ => None,
    };
    if infinite_bound || singular.is_some() {
        return improper(v, f, &lo, &hi, singular, numeric, st);
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
        (_, _, _) => {
            let num = numeric.ok_or("Solve couldn't find an antiderivative for this")?;
            st.push(
                "Solve couldn't find an antiderivative, so it adds up the area numerically",
                "",
            );
            finish_numeric(num, st)
        }
    }
}

/// Where an improper integral's pieces start and end.
#[derive(Clone)]
enum At {
    Point(Sym),
    Infinity(i8),
}

/// An integral over an infinite range, or across a point where the
/// integrand is undefined: the antiderivative's limits at those ends, each
/// piece on its own. It diverges when one of them is infinite, or doesn't
/// exist because the antiderivative keeps oscillating.
fn improper(
    v: &str,
    f: Option<Sym>,
    lo: &Bound,
    hi: &Bound,
    singular: Option<Sym>,
    numeric: Option<f64>,
    st: &mut Steps,
) -> Done {
    let x = name(v);
    let at = |b: &Bound| match b {
        Bound::At(s) => At::Point(s.clone()),
        Bound::Infinite(i) => At::Infinity(if *i > 0.0 { 1 } else { -1 }),
    };
    let shown = |p: &At| match p {
        At::Point(s) => latex(s),
        At::Infinity(1) => "\\infty".into(),
        At::Infinity(_) => "-\\infty".into(),
    };
    match &singular {
        Some(c) => st.push(
            format!(
                "The integrand is undefined at ${x}={}$, so this is an improper integral: split it there and take limits of the antiderivative",
                latex(c)
            ),
            "",
        ),
        None => st.push(
            "A bound is infinite, so this is an improper integral: take the limit of the antiderivative there",
            "",
        ),
    }
    let Some(f) = f.map(|f| nice(&f)) else {
        let num = numeric.filter(|n| n.is_finite()).ok_or(
            "Solve couldn't find an antiderivative, and the area doesn't settle when added up numerically",
        )?;
        st.push(
            "Solve couldn't find an antiderivative, so it adds up the area numerically",
            "",
        );
        return finish_numeric(num, st);
    };
    let fl = latex(&f);
    let mut points = vec![at(lo)];
    // A bad point strictly inside splits the range; at an end it is
    // approached from inside.
    let same = |p: &At, c: &Sym| matches!(p, At::Point(a) if (a.value() - c.value()).abs() < 1e-12);
    if let Some(c) = &singular {
        if !same(&at(lo), c) && !same(&at(hi), c) {
            points.push(At::Point(c.clone()));
        }
    }
    points.push(at(hi));
    // Reversed bounds are worked out the right way round, then negated.
    let flip = match (&points[0], &points[points.len() - 1]) {
        (At::Point(a), At::Point(b)) => a.value() > b.value(),
        (At::Infinity(1), _) | (_, At::Infinity(-1)) => true,
        _ => false,
    };
    if flip {
        points.reverse();
    }
    // Each piece's top end from below, minus its bottom end from above.
    let mut exact: Option<Sym> = Some(n(0));
    let mut total = 0.0;
    let mut runaway: Vec<i8> = Vec::new();
    for pair in points.windows(2) {
        for (p, side, sign) in [(&pair[1], -1i8, 1i8), (&pair[0], 1, -1)] {
            let is_singular = singular.as_ref().is_some_and(|c| same(p, c));
            let end = match p {
                At::Point(a) if !is_singular => {
                    let val = nice(&f.subs_var(v, a));
                    if val.value().is_finite() {
                        limits::End::Exact(val)
                    } else {
                        limits::end_value(&f, v, Some(a), 0, side)
                    }
                }
                At::Point(a) => limits::end_value(&f, v, Some(a), 0, side),
                At::Infinity(s) => limits::end_value(&f, v, None, *s, 0),
            };
            let limit_latex = match p {
                At::Point(a) if is_singular => format!(
                    "\\lim_{{{x}\\to {}^{{{}}}}}\\left({fl}\\right)",
                    latex(a),
                    if side > 0 { "+" } else { "-" }
                ),
                At::Infinity(_) => {
                    format!("\\lim_{{{x}\\to {}}}\\left({fl}\\right)", shown(p))
                }
                At::Point(_) => String::new(),
            };
            match end {
                limits::End::Exact(val) => {
                    if !matches!(p, At::Point(_)) || is_singular {
                        st.push(
                            format!("Take the limit of the antiderivative ${fl}$"),
                            format!("{limit_latex}={}", latex(&val)),
                        );
                    }
                    total += sign as f64 * val.value();
                    exact = exact.map(|e| add2(e, mul2(n(sign as i128), val)));
                }
                limits::End::Number(num) => {
                    st.push(
                        format!("Take the limit of the antiderivative ${fl}$ numerically"),
                        format!("{limit_latex}\\approx {}", crate::format::value_latex(num)),
                    );
                    total += sign as f64 * num;
                    exact = None;
                }
                limits::End::Infinite(s) => {
                    st.push(
                        format!("The antiderivative ${fl}$ grows without bound there"),
                        format!(
                            "{limit_latex}={}",
                            if s > 0 { "\\infty" } else { "-\\infty" }
                        ),
                    );
                    runaway.push(s * sign);
                }
                limits::End::None => {
                    st.push(
                        format!(
                            "The antiderivative ${fl}$ keeps oscillating as ${x}\\to {}$ and never settles, so the integral diverges",
                            shown(p)
                        ),
                        format!("{limit_latex}\\ \\text{{does not exist}}"),
                    );
                    return Ok(("\\text{diverges}".into(), None));
                }
            }
        }
    }
    if !runaway.is_empty() {
        let all = runaway[0];
        let flip_sign = if flip { -1 } else { 1 };
        if runaway.iter().all(|s| *s == all) {
            let inf = if all * flip_sign > 0 {
                "\\infty"
            } else {
                "-\\infty"
            };
            st.push(
                format!("So the area grows without bound: the integral diverges to ${inf}$"),
                "",
            );
            return Ok((format!("\\text{{diverges to }}{inf}"), None));
        }
        st.push(
            "The pieces run off to ∞ and -∞, and each must converge on its own, so the integral diverges",
            "",
        );
        return Ok(("\\text{diverges}".into(), None));
    }
    if flip {
        total = -total;
        exact = exact.map(|e| mul2(n(-1), e));
    }
    // The antiderivative must not jump in between: compare with the area
    // added up numerically.
    if let Some(num) = numeric.filter(|n| n.is_finite()) {
        if (num - total).abs() > 1e-6 * num.abs().max(1.0) {
            st.push(
                "That doesn't match adding up the area numerically, so the antiderivative must jump inside the interval (the integrand isn't continuous there). The numerical value is",
                crate::format::value_latex(num),
            );
            let shown = crate::format::value_latex(num);
            return Ok((format!("\\approx {shown}"), Some(shown)));
        }
    }
    match exact.map(|e| nice(&e)) {
        Some(value) => {
            let result = with_approx(&value);
            st.push("So", result.clone());
            Ok((result, Some(latex(&value))))
        }
        None => finish_numeric(total, st),
    }
}

/// A numerical value of an integral, recognized as a simple exact number
/// when it is one, or reported as diverging when it runs away.
fn finish_numeric(num: f64, st: &mut Steps) -> Done {
    if num.is_nan() {
        return Err("The area doesn't settle when added up numerically".into());
    }
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
pub(crate) fn recognize(v: f64) -> Option<Sym> {
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
        // A root found numerically is only close: ≈, not =.
        .map(|r| match r {
            Sym::Real(_) => format!("{xs}\\approx {}", latex(r)),
            _ => format!("{xs}={}", with_approx(r)),
        })
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
