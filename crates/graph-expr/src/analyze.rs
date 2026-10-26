//! Works out what each expression row means, the way Desmos does: which rows
//! define variables or functions, which are curves, which just show a value,
//! and which names still need defining (offered as sliders).

use crate::ast::{Expr, Relation, Statement};
use crate::compile::{BinOp, Low, Program};
use crate::parser::parse;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug)]
pub struct RowInput {
    pub id: u64,
    pub latex: String,
}

/// Which variable a curve is a function of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    /// y = f(x)
    X,
    /// x = g(y)
    Y,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Curve {
    pub axis: Axis,
    pub program: Program,
}

#[derive(Clone, Debug, PartialEq)]
pub enum RowKind {
    Empty,
    Variable(String),
    Function(String),
    Curve,
    Value,
    /// Valid math this version cannot graph yet, such as an inequality.
    NotYet,
    Error,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RowAnalysis {
    pub id: u64,
    pub kind: RowKind,
    pub error: Option<String>,
    /// A computed value to show under the row, as Desmos does for `2+3` or `b = 2a`.
    pub value: Option<f64>,
    /// Undefined names that could be added as sliders.
    pub missing: Vec<String>,
    /// The row is `name = number`, so the panel shows a slider for it.
    pub slider: Option<f64>,
    pub curve: Option<Curve>,
}

impl RowAnalysis {
    fn new(id: u64, kind: RowKind) -> Self {
        Self {
            id,
            kind,
            error: None,
            value: None,
            missing: Vec::new(),
            slider: None,
            curve: None,
        }
    }

    fn error(id: u64, message: impl Into<String>) -> Self {
        Self {
            error: Some(message.into()),
            ..Self::new(id, RowKind::Error)
        }
    }
}

/// Names with fixed meanings that rows cannot redefine.
const CONSTANTS: &[(&str, f64)] = &[
    ("pi", std::f64::consts::PI),
    ("tau", std::f64::consts::TAU),
    ("e", std::f64::consts::E),
    ("infty", f64::INFINITY),
];
const GRAPH_VARIABLES: &[&str] = &["x", "y", "r", "theta"];
const MAX_INLINE_DEPTH: usize = 64;

struct FuncDef {
    params: Vec<String>,
    body: Expr,
}

enum Def {
    Var(String, Expr),
    Func(String, FuncDef),
}

pub fn analyze(rows: &[RowInput]) -> Vec<RowAnalysis> {
    let parsed: Vec<Option<Result<Statement, String>>> = rows
        .iter()
        .map(|r| (!r.latex.trim().is_empty()).then(|| parse(&r.latex)))
        .collect();

    // Collect definitions, refusing duplicates and reserved names.
    let mut defs: Vec<Option<Result<Def, String>>> = parsed
        .iter()
        .map(|p| match p {
            Some(Ok(statement)) => definition(statement),
            _ => None,
        })
        .collect();
    let mut counts: HashMap<String, usize> = HashMap::new();
    for def in defs.iter().flatten().flatten() {
        *counts.entry(def_name(def).to_owned()).or_default() += 1;
    }
    for def in defs.iter_mut() {
        if let Some(Ok(d)) = def {
            let name = def_name(d).to_owned();
            if counts[&name] > 1 {
                *def = Some(Err(format!(
                    "'{}' is defined more than once",
                    pretty(&name)
                )));
            }
        }
    }

    let mut env = Env {
        vars: HashMap::new(),
        funcs: HashMap::new(),
        values: HashMap::new(),
    };
    for def in defs.iter().flatten().flatten() {
        match def {
            Def::Var(name, expr) => {
                env.vars.insert(name.clone(), expr.clone());
            }
            Def::Func(name, f) => {
                env.funcs.insert(
                    name.clone(),
                    FuncDef {
                        params: f.params.clone(),
                        body: f.body.clone(),
                    },
                );
            }
        }
    }

    rows.iter()
        .zip(parsed)
        .zip(defs)
        .map(|((row, parsed), def)| match (parsed, def) {
            (None, _) => RowAnalysis::new(row.id, RowKind::Empty),
            (Some(Err(e)), _) => RowAnalysis::error(row.id, e),
            (Some(Ok(_)), Some(Err(e))) => RowAnalysis::error(row.id, e),
            (Some(Ok(_)), Some(Ok(Def::Var(name, expr)))) => env.variable_row(row.id, &name, &expr),
            (Some(Ok(_)), Some(Ok(Def::Func(name, f)))) => env.function_row(row.id, &name, &f),
            (Some(Ok(statement)), None) => env.other_row(row.id, &statement),
        })
        .collect()
}

fn def_name(def: &Def) -> &str {
    match def {
        Def::Var(n, _) | Def::Func(n, _) => n,
    }
}

/// `a_1` is shown to people as written, without our internal underscore form.
fn pretty(name: &str) -> String {
    name.to_owned()
}

fn definable(name: &str) -> bool {
    !GRAPH_VARIABLES.contains(&name) && !CONSTANTS.iter().any(|(c, _)| *c == name)
}

fn definition(statement: &Statement) -> Option<Result<Def, String>> {
    let Statement::Relation(lhs, Relation::Eq, rhs) = statement else {
        return None;
    };
    match lhs {
        Expr::Var(name) if definable(name) => {
            if rhs.mentions(name) {
                return Some(Err(format!(
                    "'{}' is defined in terms of itself",
                    pretty(name)
                )));
            }
            Some(Ok(Def::Var(name.clone(), rhs.clone())))
        }
        Expr::Var(name) if CONSTANTS.iter().any(|(c, _)| c == name) => Some(Err(format!(
            "'{name}' is a built-in constant and can't be redefined"
        ))),
        Expr::Apply(name, args) if definable(name) => {
            let mut params = Vec::new();
            for arg in args {
                match arg {
                    Expr::Var(p)
                        if !params.contains(p) && !CONSTANTS.iter().any(|(c, _)| c == p) =>
                    {
                        params.push(p.clone())
                    }
                    // Not a definition: `a(x+1) = 3` is an equation in x.
                    _ => return None,
                }
            }
            Some(Ok(Def::Func(
                name.clone(),
                FuncDef {
                    params,
                    body: rhs.clone(),
                },
            )))
        }
        _ => None,
    }
}

struct Env {
    vars: HashMap<String, Expr>,
    funcs: HashMap<String, FuncDef>,
    /// Memoised variable values: Ok(value) or Err(message).
    values: HashMap<String, Result<f64, String>>,
}

/// Collected while resolving names.
#[derive(Default)]
struct Problems {
    missing: Vec<String>,
    errors: Vec<String>,
}

impl Problems {
    fn missing(&mut self, name: &str) {
        if !self.missing.iter().any(|m| m == name) {
            self.missing.push(name.to_owned());
        }
    }

    fn first_message(&self) -> Option<String> {
        if let Some(e) = self.errors.first() {
            return Some(e.clone());
        }
        match self.missing.as_slice() {
            [] => None,
            [one] => Some(format!("'{}' is not defined", pretty(one))),
            many => Some(format!(
                "{} are not defined",
                many.iter()
                    .map(|m| format!("'{}'", pretty(m)))
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
        }
    }
}

impl Env {
    fn variable_row(&mut self, id: u64, name: &str, expr: &Expr) -> RowAnalysis {
        let mut row = RowAnalysis::new(id, RowKind::Variable(name.to_owned()));
        match self.value_of(name, &mut HashSet::new()) {
            Ok(v) => {
                if let Some(literal) = expr.as_literal() {
                    row.slider = Some(literal);
                } else {
                    row.value = Some(v);
                }
            }
            Err(e) => {
                row.error = Some(e);
                row.missing = self.missing_in(expr, &[]);
            }
        }
        row
    }

    fn function_row(&mut self, id: u64, name: &str, f: &FuncDef) -> RowAnalysis {
        let mut row = RowAnalysis::new(id, RowKind::Function(name.to_owned()));
        let graph_param = f.params.len() == 1 && f.params[0] == "x";
        let mut problems = Problems::default();
        let bindings: HashMap<String, Low> = f
            .params
            .iter()
            .map(|p| {
                (
                    p.clone(),
                    if graph_param {
                        Low::Arg
                    } else {
                        Low::Const(1.0)
                    },
                )
            })
            .collect();
        // x is free inside a function only when it is the parameter.
        let low = self.lower(&f.body, &bindings, None, 0, &mut problems);
        row.missing = slider_names(&problems.missing);
        row.error = problems.first_message();
        if row.error.is_none() && graph_param {
            match Program::compile(&low) {
                Ok(program) => {
                    row.curve = Some(Curve {
                        axis: Axis::X,
                        program,
                    })
                }
                Err(e) => row.error = Some(e),
            }
        }
        row
    }

    fn other_row(&mut self, id: u64, statement: &Statement) -> RowAnalysis {
        let not_yet = |message: &str| RowAnalysis {
            error: Some(message.to_owned()),
            ..RowAnalysis::new(id, RowKind::NotYet)
        };
        match statement {
            Statement::Relation(lhs, Relation::Eq, rhs) => {
                let (lhs_x, lhs_y) = (lhs.mentions("x"), lhs.mentions("y"));
                let (rhs_x, rhs_y) = (rhs.mentions("x"), rhs.mentions("y"));
                if is_var(lhs, "y") && !rhs_y {
                    self.curve_row(id, rhs, Axis::X)
                } else if is_var(rhs, "y") && !lhs_y {
                    self.curve_row(id, lhs, Axis::X)
                } else if is_var(lhs, "x") && !rhs_x {
                    self.curve_row(id, rhs, Axis::Y)
                } else if is_var(rhs, "x") && !lhs_x {
                    self.curve_row(id, lhs, Axis::Y)
                } else if is_var(lhs, "r") || is_var(lhs, "theta") {
                    not_yet("Polar graphs arrive in a later version")
                } else {
                    not_yet("Implicit equations arrive in the next version")
                }
            }
            Statement::Relation(..) => not_yet("Inequalities arrive in the next version"),
            Statement::Expr(e) => {
                if e.mentions("y") && !e.mentions("x") {
                    RowAnalysis::error(id, "Try writing this as x = …")
                } else if e.mentions("x") || e.mentions("y") {
                    self.curve_row(id, e, Axis::X)
                } else {
                    self.value_row(id, e)
                }
            }
        }
    }

    fn curve_row(&mut self, id: u64, expr: &Expr, axis: Axis) -> RowAnalysis {
        let free = match axis {
            Axis::X => "x",
            Axis::Y => "y",
        };
        let mut row = RowAnalysis::new(id, RowKind::Curve);
        let mut problems = Problems::default();
        let low = self.lower(expr, &HashMap::new(), Some(free), 0, &mut problems);
        row.missing = slider_names(&problems.missing);
        row.error = problems.first_message();
        if row.error.is_none() {
            match Program::compile(&low) {
                Ok(program) => row.curve = Some(Curve { axis, program }),
                Err(e) => row.error = Some(e),
            }
        }
        row
    }

    fn value_row(&mut self, id: u64, expr: &Expr) -> RowAnalysis {
        let mut row = RowAnalysis::new(id, RowKind::Value);
        let mut problems = Problems::default();
        let low = self
            .lower(expr, &HashMap::new(), None, 0, &mut problems)
            .fold();
        row.missing = slider_names(&problems.missing);
        row.error = problems.first_message();
        if row.error.is_none() {
            if let Low::Const(v) = low {
                row.value = Some(v);
            }
        }
        row
    }

    fn missing_in(&mut self, expr: &Expr, params: &[String]) -> Vec<String> {
        let mut problems = Problems::default();
        let bindings = params
            .iter()
            .map(|p| (p.clone(), Low::Const(1.0)))
            .collect();
        self.lower(expr, &bindings, None, 0, &mut problems);
        slider_names(&problems.missing)
    }

    /// The value of a variable, evaluated once and remembered.
    fn value_of(&mut self, name: &str, visiting: &mut HashSet<String>) -> Result<f64, String> {
        if let Some(v) = self.values.get(name) {
            return v.clone();
        }
        if !visiting.insert(name.to_owned()) {
            return Err(format!("'{}' is defined in terms of itself", pretty(name)));
        }
        let expr = self
            .vars
            .get(name)
            .cloned()
            .expect("caller checked the variable exists");
        let mut problems = Problems::default();
        let low = self
            .lower_with(&expr, &HashMap::new(), None, 0, &mut problems, visiting)
            .fold();
        visiting.remove(name);
        let result = match (problems.first_message(), low) {
            (Some(e), _) => Err(e),
            (None, Low::Const(v)) => Ok(v),
            (None, _) => Err("Variables can't depend on x or y".into()),
        };
        self.values.insert(name.to_owned(), result.clone());
        result
    }

    fn lower(
        &mut self,
        e: &Expr,
        bindings: &HashMap<String, Low>,
        free: Option<&str>,
        depth: usize,
        problems: &mut Problems,
    ) -> Low {
        self.lower_with(e, bindings, free, depth, problems, &mut HashSet::new())
    }

    /// Resolves every name in `e`: function parameters, then the graph
    /// variable, then constants, then variables. Records anything undefined.
    fn lower_with(
        &mut self,
        e: &Expr,
        bindings: &HashMap<String, Low>,
        free: Option<&str>,
        depth: usize,
        problems: &mut Problems,
        visiting: &mut HashSet<String>,
    ) -> Low {
        macro_rules! go {
            ($e:expr) => {
                self.lower_with($e, bindings, free, depth, problems, visiting)
            };
        }
        let bin = |op, a: Low, b: Low| Low::Bin(op, Box::new(a), Box::new(b));
        match e {
            Expr::Num(v) => Low::Const(*v),
            Expr::Var(name) => self.resolve_name(name, bindings, free, problems, visiting),
            Expr::Neg(a) => Low::Neg(Box::new(go!(a))),
            Expr::Add(a, b) => bin(BinOp::Add, go!(a), go!(b)),
            Expr::Sub(a, b) => bin(BinOp::Sub, go!(a), go!(b)),
            Expr::Mul(a, b) => bin(BinOp::Mul, go!(a), go!(b)),
            Expr::Div(a, b) => bin(BinOp::Div, go!(a), go!(b)),
            Expr::Pow(a, b) => bin(BinOp::Pow, go!(a), go!(b)),
            Expr::Factorial(a) => Low::Call(crate::builtins::Builtin::Factorial, vec![go!(a)]),
            Expr::Call(f, args) => Low::Call(*f, args.iter().map(|a| go!(a)).collect()),
            Expr::Apply(name, args) => {
                let is_function = self.funcs.contains_key(name) && !bindings.contains_key(name);
                if is_function {
                    let (params, body) = {
                        let f = &self.funcs[name];
                        (f.params.clone(), f.body.clone())
                    };
                    if params.len() != args.len() {
                        problems.errors.push(format!(
                            "Function '{}' takes {} argument{}",
                            pretty(name),
                            params.len(),
                            if params.len() == 1 { "" } else { "s" }
                        ));
                        return Low::Const(f64::NAN);
                    }
                    if depth >= MAX_INLINE_DEPTH {
                        problems
                            .errors
                            .push(format!("'{}' is defined in terms of itself", pretty(name)));
                        return Low::Const(f64::NAN);
                    }
                    let mut lowered: HashMap<String, Low> = HashMap::new();
                    for (p, a) in params.iter().zip(args) {
                        let value = go!(a);
                        lowered.insert(p.clone(), value);
                    }
                    // Inside the function body only its parameters (and x, if it
                    // is free in the caller) are in scope.
                    self.lower_with(&body, &lowered, free, depth + 1, problems, visiting)
                } else if args.len() == 1 {
                    // `a(x+1)` with `a` not a function is multiplication.
                    let factor = self.resolve_name(name, bindings, free, problems, visiting);
                    bin(BinOp::Mul, factor, go!(&args[0]))
                } else {
                    problems
                        .errors
                        .push(format!("Function '{}' is not defined", pretty(name)));
                    Low::Const(f64::NAN)
                }
            }
        }
    }

    fn resolve_name(
        &mut self,
        name: &str,
        bindings: &HashMap<String, Low>,
        free: Option<&str>,
        problems: &mut Problems,
        visiting: &mut HashSet<String>,
    ) -> Low {
        if let Some(low) = bindings.get(name) {
            return low.clone();
        }
        if free == Some(name) {
            return Low::Arg;
        }
        if let Some((_, v)) = CONSTANTS.iter().find(|(c, _)| *c == name) {
            return Low::Const(*v);
        }
        if self.vars.contains_key(name) {
            return match self.value_of(name, visiting) {
                Ok(v) => Low::Const(v),
                Err(e) => {
                    if !problems.errors.contains(&e) {
                        problems.errors.push(e);
                    }
                    Low::Const(f64::NAN)
                }
            };
        }
        if self.funcs.contains_key(name) {
            problems.errors.push(format!(
                "'{}' is a function; give it an argument",
                pretty(name)
            ));
            return Low::Const(f64::NAN);
        }
        if GRAPH_VARIABLES.contains(&name) {
            problems.errors.push(match name {
                "x" | "y" => "Variables can't depend on x or y".to_owned(),
                _ => format!(
                    "'{name}' can only be used in polar graphs, which arrive in a later version"
                ),
            });
            return Low::Const(f64::NAN);
        }
        problems.missing(name);
        Low::Const(f64::NAN)
    }
}

fn is_var(e: &Expr, name: &str) -> bool {
    matches!(e, Expr::Var(n) if n == name)
}

/// Undefined names that Desmos would offer as sliders.
fn slider_names(missing: &[String]) -> Vec<String> {
    missing.iter().filter(|m| definable(m)).cloned().collect()
}
