//! Works out what each expression row means, the way Desmos does: which rows
//! define variables or functions, which are curves, points or regions, which
//! just show a value, and which names still need defining (offered as
//! sliders).
//!
//! Lists and points are handled here, not in compiled programs: a list
//! becomes one program per element, as Desmos draws one curve per element,
//! and a point becomes a pair of programs.

use crate::ast::{Condition, Coord, Expr, Relation, Statement};
use crate::builtins::Builtin;
use crate::compile::{Arg, BinOp, Low, Program};
use crate::parser::parse;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug, Default)]
pub struct RowInput {
    pub id: u64,
    pub latex: String,
    /// The LaTeX of a parametric or polar row's domain bounds, when the
    /// person has edited them; otherwise Desmos's defaults apply.
    pub domain: Option<[String; 2]>,
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

/// A curve traced by a parameter: `(cos t, sin t)` or `r = 1 + cos θ`.
/// Both programs read the parameter as their argument.
#[derive(Clone, Debug, PartialEq)]
pub struct Parametric {
    pub x: Program,
    pub y: Program,
    pub domain: (f64, f64),
}

/// How a relation compares its two sides.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Compare {
    Eq,
    Lt,
    Le,
    Gt,
    Ge,
}

impl Compare {
    /// Whether `v`, the left side minus the right, satisfies the relation.
    pub fn holds(self, v: f64) -> bool {
        match self {
            Compare::Eq => v == 0.0,
            Compare::Lt => v < 0.0,
            Compare::Le => v <= 0.0,
            Compare::Gt => v > 0.0,
            Compare::Ge => v >= 0.0,
        }
    }

    /// True for `<` and `>`, which Desmos draws with a dashed boundary.
    pub fn is_strict(self) -> bool {
        matches!(self, Compare::Lt | Compare::Gt)
    }

    /// True for everything but `=`, which shades a region.
    pub fn shades(self) -> bool {
        self != Compare::Eq
    }
}

/// A relation between x and y that is not a function of either: an implicit
/// equation such as `x^2 + y^2 = 25`, or an inequality to shade. `program`
/// is the left side minus the right, so the relation holds where
/// `compare.holds(program)`.
#[derive(Clone, Debug, PartialEq)]
pub struct Region {
    pub program: Program,
    pub compare: Compare,
}

/// A computed result to show under a row.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Number(f64),
    Point(f64, f64),
    Numbers(Vec<f64>),
    Points(Vec<[f64; 2]>),
}

/// The parameter a parametric or polar row runs over.
#[derive(Clone, Debug, PartialEq)]
pub struct Parameter {
    /// `t` or `theta`.
    pub name: &'static str,
    pub domain: (f64, f64),
    /// What is wrong with an edited bound, which is then ignored.
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum RowKind {
    Empty,
    Variable(String),
    Function(String),
    Curve,
    Value,
    /// Valid math this version cannot graph yet, such as a derivative.
    NotYet,
    Error,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RowAnalysis {
    pub id: u64,
    pub kind: RowKind,
    pub error: Option<String>,
    /// A computed value to show under the row, as Desmos does for `2+3` or `b = 2a`.
    pub value: Option<Value>,
    /// Undefined names that could be added as sliders.
    pub missing: Vec<String>,
    /// The row is `name = number`, so the panel shows a slider for it.
    pub slider: Option<f64>,
    /// Explicit curves, one per list element.
    pub curves: Vec<Curve>,
    pub parametrics: Vec<Parametric>,
    /// Implicit equations and inequalities to plot over the plane.
    pub regions: Vec<Region>,
    /// Fixed points to draw.
    pub points: Vec<[f64; 2]>,
    /// Set for parametric and polar rows, whose domain the panel shows.
    pub parameter: Option<Parameter>,
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
            curves: Vec::new(),
            parametrics: Vec::new(),
            regions: Vec::new(),
            points: Vec::new(),
            parameter: None,
        }
    }

    fn error(id: u64, message: impl Into<String>) -> Self {
        Self {
            error: Some(message.into()),
            ..Self::new(id, RowKind::Error)
        }
    }

    /// The first explicit curve, for rows that have one.
    pub fn curve(&self) -> Option<&Curve> {
        self.curves.first()
    }

    /// The first region, for rows that have one.
    pub fn region(&self) -> Option<&Region> {
        self.regions.first()
    }

    /// The value when it is a single number.
    pub fn number(&self) -> Option<f64> {
        match self.value {
            Some(Value::Number(v)) => Some(v),
            _ => None,
        }
    }

    /// Whether the row draws anything.
    pub fn graphed(&self) -> bool {
        !(self.curves.is_empty()
            && self.parametrics.is_empty()
            && self.regions.is_empty()
            && self.points.is_empty())
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
/// Desmos's limit on list length.
const MAX_LIST: usize = 10_000;
/// Desmos's default domains: 0 ≤ t ≤ 1 and 0 ≤ θ ≤ 12π.
const T_DOMAIN: (f64, f64) = (0.0, 1.0);
const THETA_DOMAIN: (f64, f64) = (0.0, 12.0 * std::f64::consts::PI);

/// No graph variable is in scope (a plain value or a variable's definition).
const NO_FREE: &[(&str, Arg)] = &[];
/// Inside a function body, only its parameters are in scope.
const PARAM_ONLY: &[(&str, Arg)] = &[];

struct FuncDef {
    params: Vec<String>,
    body: Expr,
}

enum Def {
    Var(String, Expr),
    Func(String, FuncDef),
}

/// One element of a value: a number or a point, each still an expression
/// of the graph variables.
#[derive(Clone, Debug)]
enum Item {
    Num(Low),
    Point(Low, Low),
}

/// What an expression lowers to: a single item, or a list of them.
#[derive(Clone, Debug)]
struct Val {
    items: Vec<Item>,
    list: bool,
}

impl Val {
    fn num(low: Low) -> Val {
        Val {
            items: vec![Item::Num(low)],
            list: false,
        }
    }

    fn nan() -> Val {
        Val::num(Low::Const(f64::NAN))
    }

    fn list(items: Vec<Item>) -> Val {
        Val { items, list: true }
    }

    fn item(&self, i: usize) -> &Item {
        if self.list {
            &self.items[i]
        } else {
            &self.items[0]
        }
    }

    fn has_points(&self) -> bool {
        self.items.iter().any(|i| matches!(i, Item::Point(..)))
    }

    /// The value as constants, when nothing in it depends on a graph
    /// variable.
    fn constant(&self) -> Option<Value> {
        let num = |l: &Low| match l.clone().fold() {
            Low::Const(v) => Some(v),
            _ => None,
        };
        if self.has_points() {
            let points = self
                .items
                .iter()
                .map(|i| match i {
                    Item::Point(a, b) => Some([num(a)?, num(b)?]),
                    Item::Num(_) => None,
                })
                .collect::<Option<Vec<_>>>()?;
            return Some(if self.list {
                Value::Points(points)
            } else {
                Value::Point(points[0][0], points[0][1])
            });
        }
        let numbers = self
            .items
            .iter()
            .map(|i| match i {
                Item::Num(l) => num(l),
                Item::Point(..) => None,
            })
            .collect::<Option<Vec<_>>>()?;
        Some(if self.list {
            Value::Numbers(numbers)
        } else {
            Value::Number(numbers[0])
        })
    }

    fn from_value(v: &Value) -> Val {
        let c = |v: f64| Low::Const(v);
        match v {
            Value::Number(n) => Val::num(c(*n)),
            Value::Point(a, b) => Val {
                items: vec![Item::Point(c(*a), c(*b))],
                list: false,
            },
            Value::Numbers(ns) => Val::list(ns.iter().map(|n| Item::Num(c(*n))).collect()),
            Value::Points(ps) => {
                Val::list(ps.iter().map(|p| Item::Point(c(p[0]), c(p[1]))).collect())
            }
        }
    }
}

fn bin(op: BinOp, a: Low, b: Low) -> Low {
    Low::Bin(op, Box::new(a), Box::new(b))
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
                *def = Some(Err(format!("'{}' is defined more than once", name)));
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
            (Some(Ok(statement)), None) => env.other_row(row, &statement),
        })
        .collect()
}

fn def_name(def: &Def) -> &str {
    match def {
        Def::Var(n, _) | Def::Func(n, _) => n,
    }
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
                return Some(Err(format!("'{name}' is defined in terms of itself")));
            }
            Some(Ok(Def::Var(name.clone(), rhs.clone())))
        }
        Expr::Var(name) if CONSTANTS.iter().any(|(c, _)| c == name) => Some(Err(format!(
            "'{name}' is a built-in constant and can't be redefined"
        ))),
        // x and y can name functions too, as in `x(t) = 4 sin t`.
        Expr::Apply(name, args) if definable(name) || name == "x" || name == "y" => {
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
    /// Memoised variable values.
    values: HashMap<String, Result<Value, String>>,
}

/// Collected while resolving names.
#[derive(Default)]
struct Problems {
    missing: Vec<String>,
    errors: Vec<String>,
    /// Graph variables the expression read.
    used: Vec<&'static str>,
}

impl Problems {
    fn missing(&mut self, name: &str) {
        if !self.missing.iter().any(|m| m == name) {
            self.missing.push(name.to_owned());
        }
    }

    fn error(&mut self, message: impl Into<String>) {
        let message = message.into();
        if !self.errors.contains(&message) {
            self.errors.push(message);
        }
    }

    fn uses(&self, name: &str) -> bool {
        self.used.contains(&name)
    }

    fn first_message(&self) -> Option<String> {
        if let Some(e) = self.errors.first() {
            return Some(e.clone());
        }
        match self.missing.as_slice() {
            [] => None,
            [one] => Some(format!("'{one}' is not defined")),
            many => Some(format!(
                "{} are not defined",
                many.iter()
                    .map(|m| format!("'{m}'"))
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
        }
    }
}

/// Graph variables in scope, each with the program argument it reads.
type Free<'a> = &'a [(&'static str, Arg)];

impl Env {
    fn variable_row(&mut self, id: u64, name: &str, expr: &Expr) -> RowAnalysis {
        let mut row = RowAnalysis::new(id, RowKind::Variable(name.to_owned()));
        match self.value_of(name, &mut HashSet::new()) {
            Ok(value) => {
                match (&value, expr.as_literal()) {
                    (Value::Number(_), Some(literal)) => row.slider = Some(literal),
                    // A list or point written out in full needs no echo.
                    (_, _) if is_literal(expr) => {}
                    _ => row.value = Some(value.clone()),
                }
                // A point variable is drawn, as in Desmos.
                row.points = points_of(&value);
            }
            Err(e) => {
                row.error = Some(e);
                row.missing = self.missing_in(expr);
            }
        }
        row
    }

    fn function_row(&mut self, id: u64, name: &str, f: &FuncDef) -> RowAnalysis {
        let mut row = RowAnalysis::new(id, RowKind::Function(name.to_owned()));
        let graph_param = f.params.len() == 1 && f.params[0] == "x";
        let mut problems = Problems::default();
        let bindings: HashMap<String, Val> = f
            .params
            .iter()
            .map(|p| {
                (
                    p.clone(),
                    Val::num(if graph_param {
                        Low::Arg(Arg::X)
                    } else {
                        Low::Const(1.0)
                    }),
                )
            })
            .collect();
        // x is free inside a function only when it is the parameter.
        let val = self.lower(&f.body, &bindings, PARAM_ONLY, &mut problems);
        row.missing = slider_names(&problems.missing);
        row.error = problems.first_message();
        if row.error.is_none() && graph_param && !val.has_points() {
            row.curves = compile_all(&val, &mut row.error, |low| Curve {
                axis: Axis::X,
                program: low,
            });
        }
        row
    }

    fn other_row(&mut self, input: &RowInput, statement: &Statement) -> RowAnalysis {
        let id = input.id;
        match statement {
            Statement::Relation(lhs, Relation::Eq, rhs) => {
                let (lhs_x, lhs_y) = (lhs.mentions("x"), lhs.mentions("y"));
                let (rhs_x, rhs_y) = (rhs.mentions("x"), rhs.mentions("y"));
                let xy = |e: &Expr| e.mentions("x") || e.mentions("y") || e.mentions("r");
                if is_var(lhs, "y") && !rhs_y {
                    self.curve_row(id, rhs, Axis::X)
                } else if is_var(rhs, "y") && !lhs_y {
                    self.curve_row(id, lhs, Axis::X)
                } else if is_var(lhs, "x") && !rhs_x {
                    self.curve_row(id, rhs, Axis::Y)
                } else if is_var(rhs, "x") && !lhs_x {
                    self.curve_row(id, lhs, Axis::Y)
                } else if is_var(lhs, "r") && !xy(rhs) {
                    self.polar_row(input, rhs)
                } else if is_var(rhs, "r") && !xy(lhs) {
                    self.polar_row(input, lhs)
                } else {
                    // Implicit equations, and constant ones such as 1 = 2.
                    self.region_row(id, lhs, rhs, Compare::Eq)
                }
            }
            Statement::Relation(lhs, rel, rhs) => {
                let compare = match rel {
                    Relation::Lt => Compare::Lt,
                    Relation::Le => Compare::Le,
                    Relation::Gt => Compare::Gt,
                    Relation::Ge => Compare::Ge,
                    Relation::Eq => Compare::Eq,
                };
                self.region_row(id, lhs, rhs, compare)
            }
            Statement::Chain(condition) => self.chain_row(id, condition),
            Statement::Expr(e) => self.expression_row(input, e),
        }
    }

    /// A bare expression: a curve in x, a point or list of points, a
    /// parametric curve in t, or a value.
    fn expression_row(&mut self, input: &RowInput, e: &Expr) -> RowAnalysis {
        let id = input.id;
        // First see what the expression is and which graph variables it
        // reads, with t free unless a row defines it.
        let t_free = !self.vars.contains_key("t");
        let mut probe_free = vec![("x", Arg::X), ("y", Arg::Y)];
        if t_free {
            probe_free.push(("t", Arg::X));
        }
        let mut probe = Problems::default();
        let val = self.lower(e, &HashMap::new(), &probe_free, &mut probe);
        if val.has_points() && probe.errors.is_empty() {
            if probe.uses("x") || probe.uses("y") {
                return RowAnalysis::error(id, "Points can't depend on x or y; try t for a curve");
            }
            if probe.uses("t") {
                return self.parametric_row(input, e);
            }
        }
        if !val.has_points() && probe.uses("y") && !probe.uses("x") {
            return RowAnalysis::error(id, "Try writing this as x = …");
        }
        if probe.uses("x") || probe.uses("y") {
            return self.curve_row(id, e, Axis::X);
        }
        self.value_row(id, e)
    }

    fn curve_row(&mut self, id: u64, expr: &Expr, axis: Axis) -> RowAnalysis {
        let free: Free = match axis {
            Axis::X => &[("x", Arg::X)],
            Axis::Y => &[("y", Arg::X)],
        };
        let mut row = RowAnalysis::new(id, RowKind::Curve);
        let mut problems = Problems::default();
        let val = self.lower(expr, &HashMap::new(), free, &mut problems);
        row.missing = slider_names(&problems.missing);
        row.error = problems.first_message();
        if row.error.is_none() && val.has_points() {
            row.error = Some(format!(
                "{} can't equal a point",
                if axis == Axis::X { "y" } else { "x" }
            ));
        }
        if row.error.is_none() {
            row.curves = compile_all(&val, &mut row.error, |program| Curve { axis, program });
        }
        row
    }

    /// `(f(t), g(t))`, one curve per point in a list.
    fn parametric_row(&mut self, input: &RowInput, e: &Expr) -> RowAnalysis {
        let mut row = RowAnalysis::new(input.id, RowKind::Curve);
        let mut problems = Problems::default();
        let val = self.lower(e, &HashMap::new(), &[("t", Arg::X)], &mut problems);
        row.missing = slider_names(&problems.missing);
        row.error = problems.first_message();
        let parameter = self.parameter(input, "t", T_DOMAIN);
        if row.error.is_none() {
            for item in &val.items {
                let Item::Point(a, b) = item else {
                    row.error = Some("A list mixing points and numbers can't be graphed".into());
                    break;
                };
                match (Program::compile(a), Program::compile(b)) {
                    (Ok(x), Ok(y)) => row.parametrics.push(Parametric {
                        x,
                        y,
                        domain: parameter.domain,
                    }),
                    (Err(e), _) | (_, Err(e)) => row.error = Some(e),
                }
            }
        }
        row.parameter = Some(parameter);
        row
    }

    /// `r = f(θ)`, traced as the parametric curve (f(θ)cos θ, f(θ)sin θ).
    fn polar_row(&mut self, input: &RowInput, rhs: &Expr) -> RowAnalysis {
        let mut row = RowAnalysis::new(input.id, RowKind::Curve);
        let mut problems = Problems::default();
        let val = self.lower(rhs, &HashMap::new(), &[("theta", Arg::X)], &mut problems);
        row.missing = slider_names(&problems.missing);
        row.error = problems.first_message();
        if row.error.is_none() && val.has_points() {
            row.error = Some("r can't equal a point".into());
        }
        let parameter = self.parameter(input, "theta", THETA_DOMAIN);
        if row.error.is_none() {
            let theta = || Low::Arg(Arg::X);
            for item in &val.items {
                let Item::Num(radius) = item else { continue };
                let x = bin(
                    BinOp::Mul,
                    radius.clone(),
                    Low::Call(Builtin::Cos, vec![theta()]),
                );
                let y = bin(
                    BinOp::Mul,
                    radius.clone(),
                    Low::Call(Builtin::Sin, vec![theta()]),
                );
                match (Program::compile(&x), Program::compile(&y)) {
                    (Ok(x), Ok(y)) => row.parametrics.push(Parametric {
                        x,
                        y,
                        domain: parameter.domain,
                    }),
                    (Err(e), _) | (_, Err(e)) => row.error = Some(e),
                }
            }
        }
        row.parameter = Some(parameter);
        row
    }

    /// The domain a parametric or polar row runs over: the person's bounds
    /// when they are numbers, otherwise Desmos's defaults.
    fn parameter(
        &mut self,
        input: &RowInput,
        name: &'static str,
        default: (f64, f64),
    ) -> Parameter {
        let mut parameter = Parameter {
            name,
            domain: default,
            error: None,
        };
        let Some([lo, hi]) = &input.domain else {
            return parameter;
        };
        let mut bound = |latex: &str, fallback: f64| -> Result<f64, String> {
            if latex.trim().is_empty() {
                return Ok(fallback);
            }
            let Statement::Expr(e) = parse(latex)? else {
                return Err("A domain bound must be a number".into());
            };
            let mut problems = Problems::default();
            let val = self.lower(&e, &HashMap::new(), NO_FREE, &mut problems);
            if let Some(message) = problems.first_message() {
                return Err(message);
            }
            match val.constant() {
                Some(Value::Number(v)) if v.is_finite() => Ok(v),
                _ => Err("A domain bound must be a number".into()),
            }
        };
        match (bound(lo, default.0), bound(hi, default.1)) {
            (Ok(a), Ok(b)) => parameter.domain = (a, b),
            (Err(e), _) | (_, Err(e)) => parameter.error = Some(e),
        }
        parameter
    }

    /// An implicit equation or an inequality: both sides are lowered over x
    /// and y (and r and θ, measured from the origin), and what is plotted
    /// is the left minus the right.
    fn region_row(&mut self, id: u64, lhs: &Expr, rhs: &Expr, compare: Compare) -> RowAnalysis {
        let mut problems = Problems::default();
        let left = self.lower_plane(lhs, &mut problems);
        let right = self.lower_plane(rhs, &mut problems);
        let difference = self.broadcast(&[left, right], &mut problems, |items| {
            match (items[0], items[1]) {
                (Item::Num(a), Item::Num(b)) => {
                    Ok(Item::Num(bin(BinOp::Sub, a.clone(), b.clone())))
                }
                _ => Err("Points can't be compared".into()),
            }
        });
        self.region_from(id, difference, compare, problems)
    }

    /// `a < b < c` is shaded where both comparisons hold, which is where
    /// the larger of `a - b` and `b - c` is negative. The boundary of that
    /// is exactly the boundary of the shaded region.
    fn chain_row(&mut self, id: u64, c: &Condition) -> RowAnalysis {
        let ascending = c
            .relations
            .iter()
            .all(|r| matches!(r, Relation::Lt | Relation::Le));
        let descending = c
            .relations
            .iter()
            .all(|r| matches!(r, Relation::Gt | Relation::Ge));
        if !ascending && !descending {
            return RowAnalysis::error(id, "A chain of inequalities must all point the same way");
        }
        let strict = c
            .relations
            .iter()
            .all(|r| matches!(r, Relation::Lt | Relation::Gt));
        let mut problems = Problems::default();
        let operands: Vec<Val> = c
            .operands
            .iter()
            .map(|e| self.lower_plane(e, &mut problems))
            .collect();
        let gaps: Vec<Val> = operands
            .windows(2)
            .map(|pair| {
                let (lo, hi) = if ascending { (0, 1) } else { (1, 0) };
                self.broadcast(
                    &[pair[lo].clone(), pair[hi].clone()],
                    &mut problems,
                    |items| match (items[0], items[1]) {
                        (Item::Num(a), Item::Num(b)) => {
                            Ok(Item::Num(bin(BinOp::Sub, a.clone(), b.clone())))
                        }
                        _ => Err("Points can't be compared".into()),
                    },
                )
            })
            .collect();
        let largest = self.broadcast(&gaps, &mut problems, |items| {
            let mut args = Vec::new();
            for item in items {
                match item {
                    Item::Num(l) => args.push(l.clone()),
                    Item::Point(..) => return Err("Points can't be compared".into()),
                }
            }
            Ok(Item::Num(Low::Call(Builtin::Max, args)))
        });
        let compare = if strict { Compare::Lt } else { Compare::Le };
        self.region_from(id, largest, compare, problems)
    }

    fn region_from(
        &mut self,
        id: u64,
        val: Val,
        compare: Compare,
        problems: Problems,
    ) -> RowAnalysis {
        let mut row = RowAnalysis::new(id, RowKind::Curve);
        row.missing = slider_names(&problems.missing);
        row.error = problems.first_message();
        if row.error.is_some() {
            return row;
        }
        let mut constant = None;
        for item in &val.items {
            let Item::Num(low) = item else { continue };
            match Program::compile(low) {
                Ok(program) if !program.uses_arg() => constant = program.constant(),
                Ok(program) => row.regions.push(Region { program, compare }),
                Err(e) => row.error = Some(e),
            }
        }
        if row.regions.is_empty() && row.error.is_none() {
            // Something like 1 = 2 or 3 < 4: true or false everywhere.
            row.kind = RowKind::Value;
            row.value = constant.map(Value::Number);
        }
        row
    }

    /// Lowers one side of a relation over the plane: x and y, plus r and θ
    /// measured from the origin.
    fn lower_plane(&mut self, e: &Expr, problems: &mut Problems) -> Val {
        let x = || Low::Arg(Arg::X);
        let y = || Low::Arg(Arg::Y);
        let r = Low::Call(
            Builtin::Sqrt,
            vec![bin(
                BinOp::Add,
                bin(BinOp::Mul, x(), x()),
                bin(BinOp::Mul, y(), y()),
            )],
        );
        // atan2 gives θ between -π and π; Desmos uses 0 to 2π for one turn.
        let theta = Low::Call(
            Builtin::Mod,
            vec![
                Low::Call(Builtin::Arctan, vec![y(), x()]),
                Low::Const(std::f64::consts::TAU),
            ],
        );
        let bindings: HashMap<String, Val> = [
            ("r".to_owned(), Val::num(r)),
            ("theta".to_owned(), Val::num(theta)),
        ]
        .into_iter()
        .collect();
        self.lower(e, &bindings, &[("x", Arg::X), ("y", Arg::Y)], problems)
    }

    fn value_row(&mut self, id: u64, expr: &Expr) -> RowAnalysis {
        let mut row = RowAnalysis::new(id, RowKind::Value);
        let mut problems = Problems::default();
        let val = self.lower(expr, &HashMap::new(), NO_FREE, &mut problems);
        row.missing = slider_names(&problems.missing);
        row.error = problems.first_message();
        if row.error.is_none() {
            row.value = val.constant();
            if let Some(v) = &row.value {
                row.points = points_of(v);
                if !row.points.is_empty() || matches!(v, Value::Point(..)) {
                    // Points are shown on the graph, not echoed under the row.
                    row.kind = RowKind::Curve;
                    row.value = None;
                }
            }
        }
        row
    }

    fn missing_in(&mut self, expr: &Expr) -> Vec<String> {
        let mut problems = Problems::default();
        self.lower(expr, &HashMap::new(), NO_FREE, &mut problems);
        slider_names(&problems.missing)
    }

    /// The value of a variable, evaluated once and remembered.
    fn value_of(&mut self, name: &str, visiting: &mut HashSet<String>) -> Result<Value, String> {
        if let Some(v) = self.values.get(name) {
            return v.clone();
        }
        if !visiting.insert(name.to_owned()) {
            return Err(format!("'{name}' is defined in terms of itself"));
        }
        let expr = self
            .vars
            .get(name)
            .cloned()
            .expect("caller checked the variable exists");
        let mut problems = Problems::default();
        let val = self.lower_with(&expr, &HashMap::new(), NO_FREE, 0, &mut problems, visiting);
        visiting.remove(name);
        let result = match (problems.first_message(), val.constant()) {
            (Some(e), _) => Err(e),
            (None, Some(v)) => Ok(v),
            (None, None) => Err("Variables can't depend on x or y".into()),
        };
        self.values.insert(name.to_owned(), result.clone());
        result
    }

    fn lower(
        &mut self,
        e: &Expr,
        bindings: &HashMap<String, Val>,
        free: Free,
        problems: &mut Problems,
    ) -> Val {
        self.lower_with(e, bindings, free, 0, problems, &mut HashSet::new())
    }

    /// Applies `f` to matching items of each value: element by element
    /// when any is a list (to the shortest list's length, as in Desmos),
    /// or once when none is.
    fn broadcast(
        &self,
        vals: &[Val],
        problems: &mut Problems,
        f: impl Fn(&[&Item]) -> Result<Item, String>,
    ) -> Val {
        let list = vals.iter().any(|v| v.list);
        let len = vals
            .iter()
            .filter(|v| v.list)
            .map(|v| v.items.len())
            .min()
            .unwrap_or(1);
        let items = (0..len)
            .map(|i| {
                let args: Vec<&Item> = vals.iter().map(|v| v.item(i)).collect();
                f(&args).unwrap_or_else(|e| {
                    problems.error(e);
                    Item::Num(Low::Const(f64::NAN))
                })
            })
            .collect();
        Val { items, list }
    }

    fn arithmetic(&self, op: BinOp, a: Val, b: Val, problems: &mut Problems) -> Val {
        self.broadcast(&[a, b], problems, |items| {
            use Item::*;
            let op2 = |a: &Low, b: &Low| bin(op, a.clone(), b.clone());
            Ok(match (op, items[0], items[1]) {
                (_, Num(a), Num(b)) => Num(op2(a, b)),
                (BinOp::Add | BinOp::Sub, Point(a, b), Point(c, d)) => Point(op2(a, c), op2(b, d)),
                (BinOp::Mul, Point(a, b), Num(k)) | (BinOp::Div, Point(a, b), Num(k)) => {
                    Point(op2(a, k), op2(b, k))
                }
                (BinOp::Mul, Num(k), Point(a, b)) => Point(op2(k, a), op2(k, b)),
                (BinOp::Add | BinOp::Sub, _, _) => {
                    return Err("Points can't be added to numbers".into())
                }
                (BinOp::Mul, _, _) => return Err("Points can't be multiplied together".into()),
                (BinOp::Div, _, _) => return Err("Can't divide by a point".into()),
                (BinOp::Pow, _, _) => return Err("Points can't be raised to powers".into()),
                _ => return Err("Points can't be compared".into()),
            })
        })
    }

    /// Resolves every name in `e`: function parameters, then the graph
    /// variables, then constants, then variables. Records anything undefined.
    fn lower_with(
        &mut self,
        e: &Expr,
        bindings: &HashMap<String, Val>,
        free: Free,
        depth: usize,
        problems: &mut Problems,
        visiting: &mut HashSet<String>,
    ) -> Val {
        macro_rules! go {
            ($e:expr) => {
                self.lower_with($e, bindings, free, depth, problems, visiting)
            };
        }
        match e {
            Expr::Num(v) => Val::num(Low::Const(*v)),
            Expr::Var(name) => self.resolve_name(name, bindings, free, problems, visiting),
            Expr::Neg(a) => {
                let a = go!(a);
                self.broadcast(&[a], problems, |items| {
                    let neg = |l: &Low| Low::Neg(Box::new(l.clone()));
                    Ok(match items[0] {
                        Item::Num(a) => Item::Num(neg(a)),
                        Item::Point(a, b) => Item::Point(neg(a), neg(b)),
                    })
                })
            }
            Expr::Add(a, b) => {
                let (a, b) = (go!(a), go!(b));
                self.arithmetic(BinOp::Add, a, b, problems)
            }
            Expr::Sub(a, b) => {
                let (a, b) = (go!(a), go!(b));
                self.arithmetic(BinOp::Sub, a, b, problems)
            }
            Expr::Mul(a, b) => {
                let (a, b) = (go!(a), go!(b));
                self.arithmetic(BinOp::Mul, a, b, problems)
            }
            Expr::Div(a, b) => {
                let (a, b) = (go!(a), go!(b));
                self.arithmetic(BinOp::Div, a, b, problems)
            }
            Expr::Pow(a, b) => {
                let (a, b) = (go!(a), go!(b));
                self.arithmetic(BinOp::Pow, a, b, problems)
            }
            Expr::Factorial(a) => {
                let a = go!(a);
                self.call(Builtin::Factorial, vec![a], problems)
            }
            Expr::Call(f, args) => {
                let args: Vec<Val> = args.iter().map(|a| go!(a)).collect();
                self.call(*f, args, problems)
            }
            Expr::Point(a, b) => {
                let (a, b) = (go!(a), go!(b));
                self.broadcast(&[a, b], problems, |items| match (items[0], items[1]) {
                    (Item::Num(a), Item::Num(b)) => Ok(Item::Point(a.clone(), b.clone())),
                    _ => Err("A point's coordinates must be numbers".into()),
                })
            }
            Expr::Coord(a, coord) => {
                let a = go!(a);
                self.broadcast(&[a], problems, |items| match items[0] {
                    Item::Point(x, y) => Ok(Item::Num(match coord {
                        Coord::X => x.clone(),
                        Coord::Y => y.clone(),
                    })),
                    Item::Num(_) => Err("Only points have .x and .y".into()),
                })
            }
            Expr::List(items) => {
                let mut out = Vec::new();
                for item in items {
                    let v = go!(item);
                    if v.list {
                        problems.error("Lists can't contain lists");
                        return Val::nan();
                    }
                    out.extend(v.items);
                }
                Val::list(out)
            }
            Expr::Range(start, second, end) => {
                let mut number = |e: &Expr, problems: &mut Problems| -> Option<f64> {
                    let v = self.lower_with(e, bindings, free, depth, problems, visiting);
                    match v.constant() {
                        Some(Value::Number(n)) if n.is_finite() => Some(n),
                        _ => None,
                    }
                };
                let (Some(a), Some(z)) = (number(start, problems), number(end, problems)) else {
                    problems.error("A range's ends must be numbers");
                    return Val::nan();
                };
                let step = match second {
                    Some(s) => match number(s, problems) {
                        Some(b) => b - a,
                        None => {
                            problems.error("A range's elements must be numbers");
                            return Val::nan();
                        }
                    },
                    None if z >= a => 1.0,
                    None => -1.0,
                };
                if step == 0.0 || (z - a) / step < 0.0 {
                    return Val::list(vec![Item::Num(Low::Const(a))]);
                }
                let count = ((z - a) / step + 1e-9).floor() as usize + 1;
                if count > MAX_LIST {
                    problems.error("Lists can have at most 10,000 elements");
                    return Val::nan();
                }
                Val::list(
                    (0..count)
                        .map(|i| Item::Num(Low::Const(a + step * i as f64)))
                        .collect(),
                )
            }
            Expr::Index(base, index) => {
                let base = go!(base);
                let index = go!(index);
                if !base.list {
                    // `a[1, 2]` with `a` a number is a product.
                    return self.arithmetic(BinOp::Mul, base, index, problems);
                }
                let pick = |i: &Item| -> Result<Item, String> {
                    let Item::Num(low) = i else {
                        return Err("List indices must be numbers".into());
                    };
                    match low.clone().fold() {
                        Low::Const(n) => {
                            let n = n.round();
                            if n >= 1.0 && (n as usize) <= base.items.len() {
                                Ok(base.items[n as usize - 1].clone())
                            } else {
                                Ok(Item::Num(Low::Const(f64::NAN)))
                            }
                        }
                        _ => Err("List indices can't depend on x or y yet".into()),
                    }
                };
                let mut out = Vec::new();
                for i in &index.items {
                    match pick(i) {
                        Ok(item) => out.push(item),
                        Err(e) => {
                            problems.error(e);
                            return Val::nan();
                        }
                    }
                }
                Val {
                    items: out,
                    list: index.list,
                }
            }
            Expr::Comprehension(body, generators) => {
                let mut lists = Vec::new();
                for (name, list) in generators {
                    let v = go!(list);
                    if !v.list {
                        problems.error(format!("'{name}' must range over a list"));
                        return Val::nan();
                    }
                    lists.push((name.clone(), v));
                }
                let total: usize = lists.iter().map(|(_, v)| v.items.len()).product();
                if total > MAX_LIST {
                    problems.error("Lists can have at most 10,000 elements");
                    return Val::nan();
                }
                let mut out = Vec::with_capacity(total);
                for k in 0..total {
                    let mut inner = bindings.clone();
                    let mut rest = k;
                    // The last variable varies fastest.
                    for (name, v) in lists.iter().rev() {
                        let n = v.items.len();
                        inner.insert(
                            name.clone(),
                            Val {
                                items: vec![v.items[rest % n].clone()],
                                list: false,
                            },
                        );
                        rest /= n;
                    }
                    let item = self.lower_with(body, &inner, free, depth, problems, visiting);
                    if item.list {
                        problems.error("Lists can't contain lists");
                        return Val::nan();
                    }
                    out.extend(item.items);
                }
                Val::list(out)
            }
            Expr::Piecewise(branches, otherwise) => {
                let mut result = match otherwise {
                    Some(e) => go!(e),
                    None => Val::nan(),
                };
                for (condition, value) in branches.iter().rev() {
                    let c = self.condition(condition, bindings, free, depth, problems, visiting);
                    let v = go!(value);
                    result = self.broadcast(&[c, v, result], problems, |items| {
                        let sel = |c: &Low, a: &Low, b: &Low| {
                            Low::Select(
                                Box::new(c.clone()),
                                Box::new(a.clone()),
                                Box::new(b.clone()),
                            )
                        };
                        let nan = Low::Const(f64::NAN);
                        Ok(match (items[0], items[1], items[2]) {
                            (Item::Num(c), Item::Num(a), Item::Num(b)) => Item::Num(sel(c, a, b)),
                            (Item::Num(c), Item::Point(a, b), Item::Point(p, q)) => {
                                Item::Point(sel(c, a, p), sel(c, b, q))
                            }
                            // No else: a point that is undefined elsewhere.
                            (Item::Num(c), Item::Point(a, b), Item::Num(_)) => {
                                Item::Point(sel(c, a, &nan), sel(c, b, &nan))
                            }
                            _ => {
                                return Err(
                                    "A piecewise definition can't mix points and numbers".into()
                                )
                            }
                        })
                    });
                }
                result
            }
            Expr::Apply(name, args) => {
                let is_function = self.funcs.contains_key(name) && !bindings.contains_key(name);
                if is_function {
                    let (params, body) = {
                        let f = &self.funcs[name];
                        (f.params.clone(), f.body.clone())
                    };
                    if params.len() != args.len() {
                        problems.error(format!(
                            "Function '{}' takes {} argument{}",
                            name,
                            params.len(),
                            if params.len() == 1 { "" } else { "s" }
                        ));
                        return Val::nan();
                    }
                    if depth >= MAX_INLINE_DEPTH {
                        problems.error(format!("'{name}' is defined in terms of itself"));
                        return Val::nan();
                    }
                    let mut lowered: HashMap<String, Val> = HashMap::new();
                    for (p, a) in params.iter().zip(args) {
                        let value = go!(a);
                        lowered.insert(p.clone(), value);
                    }
                    // Inside the function body only its parameters (and the
                    // caller's graph variables) are in scope.
                    self.lower_with(&body, &lowered, free, depth + 1, problems, visiting)
                } else if args.len() <= 2 {
                    // `a(x+1)` with `a` not a function is multiplication, and
                    // `a(1, 2)` scales a point.
                    let factor = self.resolve_name(name, bindings, free, problems, visiting);
                    let operand = if args.len() == 1 {
                        go!(&args[0])
                    } else {
                        go!(&Expr::Point(
                            Box::new(args[0].clone()),
                            Box::new(args[1].clone())
                        ))
                    };
                    self.arithmetic(BinOp::Mul, factor, operand, problems)
                } else {
                    problems.error(format!("Function '{name}' is not defined"));
                    Val::nan()
                }
            }
        }
    }

    /// A condition such as `0 < x \le 2` as a value that is 1 where every
    /// comparison holds and 0 where one fails.
    fn condition(
        &mut self,
        c: &Condition,
        bindings: &HashMap<String, Val>,
        free: Free,
        depth: usize,
        problems: &mut Problems,
        visiting: &mut HashSet<String>,
    ) -> Val {
        let operands: Vec<Val> = c
            .operands
            .iter()
            .map(|e| self.lower_with(e, bindings, free, depth, problems, visiting))
            .collect();
        let mut result: Option<Val> = None;
        for (i, rel) in c.relations.iter().enumerate() {
            let op = match rel {
                Relation::Lt => BinOp::Lt,
                Relation::Le => BinOp::Le,
                Relation::Gt => BinOp::Gt,
                Relation::Ge => BinOp::Ge,
                Relation::Eq => BinOp::Eq,
            };
            let cmp = self.arithmetic(op, operands[i].clone(), operands[i + 1].clone(), problems);
            result = Some(match result {
                None => cmp,
                // Both must hold: the second comparison where the first
                // does, false where it does not.
                Some(acc) => {
                    self.broadcast(&[acc, cmp], problems, |items| match (items[0], items[1]) {
                        (Item::Num(a), Item::Num(b)) => Ok(Item::Num(Low::Select(
                            Box::new(a.clone()),
                            Box::new(b.clone()),
                            Box::new(Low::Const(0.0)),
                        ))),
                        _ => Err("Points can't be compared".into()),
                    })
                }
            });
        }
        result.unwrap_or_else(Val::nan)
    }

    fn call(&self, f: Builtin, args: Vec<Val>, problems: &mut Problems) -> Val {
        let point_error = || format!("Function '{}' can't take a point", f.name());
        if f.takes_list() {
            // total([1, 2, 3]) and max(a, b) both reduce every element.
            let mut flat = Vec::new();
            for v in &args {
                for item in &v.items {
                    match item {
                        Item::Num(l) => flat.push(l.clone()),
                        Item::Point(..) => {
                            problems.error(point_error());
                            return Val::nan();
                        }
                    }
                }
            }
            if flat.is_empty() {
                return Val::num(Low::Const(f.eval(&[])));
            }
            return Val::num(Low::Call(f, flat));
        }
        self.broadcast(&args, problems, |items| {
            let mut lows = Vec::with_capacity(items.len());
            for item in items {
                match item {
                    Item::Num(l) => lows.push(l.clone()),
                    Item::Point(..) => return Err(point_error()),
                }
            }
            Ok(Item::Num(Low::Call(f, lows)))
        })
    }

    fn resolve_name(
        &mut self,
        name: &str,
        bindings: &HashMap<String, Val>,
        free: Free,
        problems: &mut Problems,
        visiting: &mut HashSet<String>,
    ) -> Val {
        if let Some(v) = bindings.get(name) {
            // r and θ in a relation stand for expressions of x and y; a
            // function's parameter named x holding a number does not count.
            let reads_plane = v.items.iter().any(|i| match i {
                Item::Num(l) => l.uses_arg(),
                Item::Point(a, b) => a.uses_arg() || b.uses_arg(),
            });
            if let Some(g) = GRAPH_VARIABLES.iter().find(|g| **g == name) {
                if reads_plane {
                    problems.used.push(g);
                }
            }
            return v.clone();
        }
        if let Some((n, arg)) = free.iter().find(|(n, _)| *n == name) {
            problems.used.push(n);
            return Val::num(Low::Arg(*arg));
        }
        if let Some((_, v)) = CONSTANTS.iter().find(|(c, _)| *c == name) {
            return Val::num(Low::Const(*v));
        }
        if self.vars.contains_key(name) {
            return match self.value_of(name, visiting) {
                Ok(v) => Val::from_value(&v),
                Err(e) => {
                    problems.error(e);
                    Val::nan()
                }
            };
        }
        if self.funcs.contains_key(name) {
            problems.error(format!("'{name}' is a function; give it an argument"));
            return Val::nan();
        }
        if GRAPH_VARIABLES.contains(&name) {
            problems.error(match name {
                "x" | "y" => "Variables can't depend on x or y".to_owned(),
                _ => format!("'{name}' can only be used in polar graphs, like r = 2θ"),
            });
            return Val::nan();
        }
        problems.missing(name);
        Val::nan()
    }
}

/// Compiles each number in `val`, recording the first failure.
fn compile_all<T>(val: &Val, error: &mut Option<String>, make: impl Fn(Program) -> T) -> Vec<T> {
    let mut out = Vec::new();
    for item in &val.items {
        if let Item::Num(low) = item {
            match Program::compile(low) {
                Ok(p) => out.push(make(p)),
                Err(e) => {
                    *error = Some(e);
                    return Vec::new();
                }
            }
        }
    }
    out
}

fn points_of(v: &Value) -> Vec<[f64; 2]> {
    let finite = |p: &[f64; 2]| p[0].is_finite() && p[1].is_finite();
    match v {
        Value::Point(a, b) => [[*a, *b]].into_iter().filter(finite).collect(),
        Value::Points(ps) => ps.iter().copied().filter(finite).collect(),
        _ => Vec::new(),
    }
}

/// A list or point of plain numbers, which shows no value under its row.
fn is_literal(e: &Expr) -> bool {
    match e {
        Expr::List(items) => items.iter().all(is_literal),
        Expr::Point(a, b) => a.as_literal().is_some() && b.as_literal().is_some(),
        other => other.as_literal().is_some(),
    }
}

fn is_var(e: &Expr, name: &str) -> bool {
    matches!(e, Expr::Var(n) if n == name)
}

/// Undefined names that Desmos would offer as sliders.
fn slider_names(missing: &[String]) -> Vec<String> {
    missing.iter().filter(|m| definable(m)).cloned().collect()
}
