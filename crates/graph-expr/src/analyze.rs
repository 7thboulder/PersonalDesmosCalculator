//! Works out what each expression row means, the way Desmos does: which rows
//! define variables or functions, which are curves, points or regions, which
//! just show a value, and which names still need defining (offered as
//! sliders).
//!
//! Lists and points are handled here, not in compiled programs: a list
//! becomes one program per element, as Desmos draws one curve per element,
//! and a point becomes a pair of programs.

use crate::ast::{Condition, Coord, Expr, Relation, Repeat, ShapeFn, Statement};
use crate::builtins::Builtin;
use crate::calculus::derivative;
use crate::compile::{Arg, BinOp, Low, Program};
use crate::parser::parse;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

#[derive(Clone, Debug, Default)]
pub struct RowInput {
    pub id: u64,
    pub latex: String,
    /// The LaTeX of a parametric or polar row's domain bounds, when the
    /// person has edited them; otherwise Desmos's defaults apply.
    pub domain: Option<[String; 2]>,
    /// The LaTeX of a differential equation's initial points, such as
    /// `(0, 1)` or `[(0, 1), (a, b)]`.
    pub seeds: Option<String>,
    /// The LaTeX of an equation of order n's initial derivatives, y' to
    /// y⁽ⁿ⁻¹⁾, each a number or a list with one per initial point.
    pub derivs: Vec<String>,
    /// Set when the row is a table instead of an expression.
    pub table: Option<Vec<ColumnInput>>,
}

/// One column of a table: its header and the LaTeX of each cell.
#[derive(Clone, Debug, Default)]
pub struct ColumnInput {
    pub header: String,
    pub cells: Vec<String>,
}

/// What a table column means and draws.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ColumnAnalysis {
    pub error: Option<String>,
    /// Undefined names in the header or cells, offered as sliders.
    pub missing: Vec<String>,
    /// For a column whose header is a formula (`x_1^2`), its value in
    /// each table row, NaN where undefined.
    pub computed: Option<Vec<f64>>,
    /// Points against the first column, skipping incomplete rows.
    pub points: Vec<[f64; 2]>,
    /// Cells that could not be read, by table row.
    pub cell_errors: Vec<(usize, String)>,
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

/// A surface `z = f(x, y)` in a 3D graph. The program reads x as
/// `Arg::X` and y as `Arg::Y`.
#[derive(Clone, Debug, PartialEq)]
pub struct Surface {
    pub program: Program,
}

/// A curve in 3D traced by t: `(cos t, sin t, t)`. All three programs
/// read t as their argument.
#[derive(Clone, Debug, PartialEq)]
pub struct Curve3 {
    pub x: Program,
    pub y: Program,
    pub z: Program,
    pub domain: (f64, f64),
}

/// An equation in x, y and z drawn as a surface, such as
/// `x^2 + y^2 + z^2 = 25`. The program is the left side minus the right,
/// reading x, y and z as `Arg::X`, `Arg::Y` and `Arg::Z`.
#[derive(Clone, Debug, PartialEq)]
pub struct Implicit3 {
    pub program: Program,
    /// For an inequality, how the program compares with zero inside the
    /// shaded solid: `x^2 + y^2 + z^2 < 25` is the program
    /// `x^2 + y^2 + z^2 - 25` with `Lt`.
    pub shade: Option<Compare>,
}

/// A surface traced by u and v: `(u, v, u v)`. All three programs read u
/// as `Arg::X` and v as `Arg::Y`.
#[derive(Clone, Debug, PartialEq)]
pub struct ParamSurface {
    pub x: Program,
    pub y: Program,
    pub z: Program,
    pub u: (f64, f64),
    pub v: (f64, f64),
}

/// A point a differential equation's solution passes through.
#[derive(Clone, Debug, PartialEq)]
pub struct Seed {
    pub at: [f64; 2],
    /// For a point written with slider variables, as in `(a, b)`: the
    /// variable each coordinate drags.
    pub drag: Option<[Option<String>; 2]>,
    /// For an equation of order n, y' to y⁽ⁿ⁻¹⁾ there.
    pub derivs: Vec<f64>,
}

/// A differential equation, as its row draws it.
#[derive(Clone, Debug, PartialEq)]
pub enum Ode {
    /// `y⁽ⁿ⁾ = f`, where f reads x as `Arg::X` and y, y', …, y⁽ⁿ⁻¹⁾ in
    /// order, as [`Program::eval_n`] passes them.
    Explicit { order: usize, f: Program },
    /// `F = 0` where F reads y⁽ⁿ⁾ too, so it has to be solved for it.
    Implicit { order: usize, f: Program },
    /// `(x', y') = (f, g)`, each reading x, y and t as `Arg::X`, `Arg::Y`
    /// and `Arg::Z`.
    System { f: Program, g: Program },
}

impl Ode {
    /// How many initial values past (x, y) a start needs, plus one.
    pub fn order(&self) -> usize {
        match self {
            Ode::Explicit { order, .. } | Ode::Implicit { order, .. } => *order,
            Ode::System { .. } => 1,
        }
    }
}

/// Highest order of differential equation drawn.
pub const MAX_ODE_ORDER: usize = 6;

/// What the graph variables of a differential equation in y(x) read: x,
/// then y and its derivatives.
const ODE_FREE: &[(&str, Arg)] = &[
    ("x", Arg::X),
    ("y", Arg::Y),
    ("y'", Arg::Z),
    ("y''", Arg::Extra(0)),
    ("y'''", Arg::Extra(1)),
    ("y''''", Arg::Extra(2)),
    ("y'''''", Arg::Extra(3)),
    ("y''''''", Arg::Extra(4)),
];
/// And of a system: x, y and t.
const SYSTEM_FREE: &[(&str, Arg)] = &[("x", Arg::X), ("y", Arg::Y), ("t", Arg::Z)];

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
    /// A point in 3D, and a list of them.
    Point3([f64; 3]),
    Points3(Vec<[f64; 3]>),
    Polygon(Vec<[f64; 2]>),
    Polygons(Vec<Vec<[f64; 2]>>),
    /// A polygon with corners in 3D, and a list of them.
    Polygon3(Vec<[f64; 3]>),
    Polygons3(Vec<Vec<[f64; 3]>>),
    /// A complex number, real part then imaginary part.
    Complex(f64, f64),
    Complexes(Vec<[f64; 2]>),
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
    Table,
    Value,
    /// An action, such as `a \to a + 1`, which runs when clicked.
    Action,
    /// A regression, such as `y_1 \sim m x_1 + b`.
    Regression,
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
    /// Surfaces over the xy plane, in a 3D graph.
    pub surfaces: Vec<Surface>,
    /// Equations in space and surfaces traced by u and v.
    pub implicits3: Vec<Implicit3>,
    pub param_surfaces: Vec<ParamSurface>,
    /// Points and curves in 3D.
    pub points3: Vec<[f64; 3]>,
    pub curves3: Vec<Curve3>,
    /// Fixed points to draw.
    pub points: Vec<[f64; 2]>,
    /// Filled polygons to draw, each a list of vertices.
    pub polygons: Vec<Vec<[f64; 2]>>,
    /// Polygons in a 3D graph.
    pub polygons3: Vec<Vec<[f64; 3]>>,
    /// Shaded boxes in a 3D graph, from inequalities that bound x, y and z
    /// by constants: the low and high end on each axis (infinite where
    /// unbounded).
    pub boxes3: Vec<[[f64; 2]; 3]>,
    /// For an action row: what each variable becomes when it runs, worked
    /// out from the current values.
    pub updates: Vec<(String, Value)>,
    /// Set for parametric and polar rows, whose domain the panel shows.
    pub parameter: Option<Parameter>,
    /// One entry per column for a table row.
    pub columns: Vec<ColumnAnalysis>,
    /// For a point whose coordinates are slider variables, as in
    /// `(a, b)`: the variable each coordinate drags.
    pub drag: Option<[Option<String>; 2]>,
    /// For a regression: the fitted parameters and how well they fit.
    pub fit: Option<Fit>,
    /// For a differential equation: one per list element.
    pub odes: Vec<Ode>,
    /// Set for a differential equation to its order, so the panel shows
    /// its initial points and the initial derivatives it needs.
    pub ode: Option<usize>,
    /// Where its solution curves start, with what is wrong with them and
    /// with the initial derivatives.
    pub seeds: Vec<Seed>,
    pub seeds_error: Option<String>,
    pub derivs_error: Option<String>,
    /// Whether every initial point is written as plain numbers, so
    /// dragging one can rewrite them.
    pub seeds_fixed: bool,
}

/// What a regression found.
#[derive(Clone, Debug, PartialEq)]
pub struct Fit {
    /// Each parameter with its best value, in the order they first appear.
    pub params: Vec<(String, f64)>,
    /// The share of the left side's variation the model explains, when
    /// the left side varies.
    pub r2: Option<f64>,
    /// The variable holding the residuals (left minus right side), e_1 for
    /// the first regression, as in Desmos.
    pub residuals: Option<String>,
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
            surfaces: Vec::new(),
            implicits3: Vec::new(),
            param_surfaces: Vec::new(),
            points3: Vec::new(),
            curves3: Vec::new(),
            points: Vec::new(),
            polygons: Vec::new(),
            polygons3: Vec::new(),
            boxes3: Vec::new(),
            updates: Vec::new(),
            parameter: None,
            columns: Vec::new(),
            drag: None,
            fit: None,
            odes: Vec::new(),
            ode: None,
            seeds: Vec::new(),
            seeds_error: None,
            derivs_error: None,
            seeds_fixed: false,
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
            && self.surfaces.is_empty()
            && self.implicits3.is_empty()
            && self.param_surfaces.is_empty()
            && self.points3.is_empty()
            && self.curves3.is_empty()
            && self.points.is_empty()
            && self.polygons.is_empty()
            && self.polygons3.is_empty()
            && self.boxes3.is_empty()
            && self.odes.is_empty()
            && self.columns.iter().all(|c| c.points.is_empty()))
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
/// Names that are coordinates in a 3D graph and so can't be defined there.
const SPACE_VARIABLES: &[&str] = &["z", "rho", "phi"];
/// Desmos 3D's domain for u and v in a parametric surface.
const UV_DOMAIN: (f64, f64) = (0.0, 1.0);
const MAX_INLINE_DEPTH: usize = 64;
/// Desmos's limit on list length.
const MAX_LIST: usize = 10_000;
/// Lists at least this long are broadcast through a compiled program.
const FAST_BROADCAST_LEN: usize = 64;
/// Sums with at least this many terms (less one) are compiled.
const FAST_SUM_TERMS: f64 = 7.0;

/// What a name means while compiling a sum: a local of the program, or a
/// value fixed for the whole sum (None when it has none).
#[derive(Clone)]
enum Bind {
    Slot(u16),
    Value(Option<Scalar>),
}

/// A sum being compiled, with the lists it reads by name.
struct FastCompile<'a> {
    prog: fastsum::Program,
    lists: Vec<String>,
    free: Free<'a>,
}
/// Desmos's default domains: 0 ≤ t ≤ 1 and 0 ≤ θ ≤ 12π.
const T_DOMAIN: (f64, f64) = (0.0, 1.0);
const THETA_DOMAIN: (f64, f64) = (0.0, 12.0 * std::f64::consts::PI);

/// No graph variable is in scope (a plain value or a variable's definition).
const NO_FREE: &[(&str, Arg)] = &[];
/// Inside a function body, only its parameters are in scope.
const PARAM_ONLY: &[(&str, Arg)] = &[];

#[derive(PartialEq)]
struct FuncDef {
    params: Vec<String>,
    body: Expr,
}

/// Variable values kept from one analysis to the next, each with what it
/// was worked out from, so a ticker or slider that changes one variable
/// does not make every other variable be worked out again.
#[derive(Default)]
pub struct Cache {
    vars: HashMap<String, Cached>,
    /// Function results, as `Env::calls` keeps them within one analysis.
    calls: FastMap<CallKey, CachedCall>,
    /// Function definitions, so an unchanged one keeps its pointer and the
    /// results above are checked against it without comparing bodies.
    funcs: FastMap<String, Rc<FuncDef>>,
}

/// A function's name and its constant arguments, then (when the result
/// read graph variables) which ones were in scope.
type CallKey = (String, Vec<u64>);

struct CachedCall {
    val: Val,
    used: Vec<&'static str>,
    reads: Vec<Read>,
    deps: Vec<Dep>,
}

struct Cached {
    expr: Expr,
    deps: Vec<Dep>,
    value: Result<Value, String>,
}

/// Something a value was worked out from.
#[derive(Clone)]
enum Dep {
    /// Another variable, with the value it had.
    Var(String, Rc<Result<Value, String>>),
    /// A function, with its definition at the time.
    Func(String, Rc<FuncDef>),
    /// A name that was not defined.
    Missing(String),
}

/// A name read while working a value out. Only the name is noted, since the
/// values read are already kept; they are looked up once the value is done.
#[derive(Clone)]
enum Read {
    Var(String),
    Func(String, Rc<FuncDef>),
    Missing(String),
}

/// Within one analysis a function has one definition, so reads of it are
/// told apart by name and pointer: comparing whole bodies made recording
/// reads a large share of a ticker's time.
impl PartialEq for Read {
    fn eq(&self, other: &Read) -> bool {
        match (self, other) {
            (Read::Var(a), Read::Var(b)) | (Read::Missing(a), Read::Missing(b)) => a == b,
            (Read::Func(a, f), Read::Func(b, g)) => Rc::ptr_eq(f, g) && a == b,
            _ => false,
        }
    }
}

/// Whether two values are the same, counting undefined as equal to itself.
fn same_value(a: &Result<Value, String>, b: &Result<Value, String>) -> bool {
    fn bits(v: &Value) -> Vec<u64> {
        let mut out = Vec::new();
        let mut push = |tag: u64, xs: &[f64]| {
            out.push(tag);
            out.extend(
                xs.iter()
                    .map(|x| if x.is_nan() { u64::MAX } else { x.to_bits() }),
            );
        };
        match v {
            Value::Number(n) => push(0, &[*n]),
            Value::Point(a, b) => push(1, &[*a, *b]),
            Value::Complex(a, b) => push(2, &[*a, *b]),
            Value::Numbers(ns) => push(3, ns),
            Value::Points(ps) => push(4, ps.as_flattened()),
            Value::Complexes(zs) => push(5, zs.as_flattened()),
            Value::Polygon(p) => push(6, p.as_flattened()),
            Value::Polygons(ps) => {
                for p in ps {
                    push(7, p.as_flattened());
                }
            }
            Value::Point3(p) => push(8, p),
            Value::Points3(ps) => push(9, ps.as_flattened()),
            Value::Polygon3(p) => push(10, p.as_flattened()),
            Value::Polygons3(ps) => {
                for p in ps {
                    push(11, p.as_flattened());
                }
            }
        }
        out
    }
    match (a, b) {
        (Ok(x), Ok(y)) => bits(x) == bits(y),
        (Err(x), Err(y)) => x == y,
        _ => false,
    }
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
    Point3(Low, Low, Low),
    /// A polygon's vertices, which are always constants.
    Polygon(Vec<[f64; 2]>),
    Polygon3(Vec<[f64; 3]>),
    /// A complex number with a nonzero imaginary part: real, imaginary.
    Complex(Low, Low),
}

impl Item {
    /// A complex number, or a real one when its imaginary part is zero.
    fn complex(re: Low, im: Low) -> Item {
        match im {
            Low::Const(0.0) => Item::Num(re),
            im => Item::Complex(re, im),
        }
    }

    /// The real and imaginary parts of a number.
    fn parts(&self) -> Option<(Low, Low)> {
        match self {
            Item::Num(l) => Some((l.clone(), Low::Const(0.0))),
            Item::Complex(a, b) => Some((a.clone(), b.clone())),
            _ => None,
        }
    }
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

    /// Whether any element is a point, in 2D or 3D.
    fn has_points(&self) -> bool {
        self.items
            .iter()
            .any(|i| matches!(i, Item::Point(..) | Item::Point3(..)))
    }

    fn has_points3(&self) -> bool {
        self.items.iter().any(|i| matches!(i, Item::Point3(..)))
    }

    fn has_polygons(&self) -> bool {
        self.items
            .iter()
            .any(|i| matches!(i, Item::Polygon(_) | Item::Polygon3(_)))
    }

    fn has_complex(&self) -> bool {
        self.items.iter().any(|i| matches!(i, Item::Complex(..)))
    }

    /// The value as constants, when nothing in it depends on a graph
    /// variable.
    fn constant(&self) -> Option<Value> {
        let num = |l: &Low| match l.clone().fold() {
            Low::Const(v) => Some(v),
            _ => None,
        };
        if self.items.iter().any(|i| matches!(i, Item::Polygon3(_))) {
            let polygons = self
                .items
                .iter()
                .map(|i| match i {
                    Item::Polygon3(p) => Some(p.clone()),
                    _ => None,
                })
                .collect::<Option<Vec<_>>>()?;
            return Some(if self.list {
                Value::Polygons3(polygons)
            } else {
                Value::Polygon3(polygons.into_iter().next()?)
            });
        }
        if self.has_polygons() {
            let polygons = self
                .items
                .iter()
                .map(|i| match i {
                    Item::Polygon(p) => Some(p.clone()),
                    _ => None,
                })
                .collect::<Option<Vec<_>>>()?;
            return Some(if self.list {
                Value::Polygons(polygons)
            } else {
                Value::Polygon(polygons.into_iter().next()?)
            });
        }
        if self.has_complex() {
            let numbers = self
                .items
                .iter()
                .map(|i| {
                    let (a, b) = i.parts()?;
                    Some([num(&a)?, num(&b)?])
                })
                .collect::<Option<Vec<_>>>()?;
            return Some(if self.list {
                Value::Complexes(numbers)
            } else {
                Value::Complex(numbers[0][0], numbers[0][1])
            });
        }
        if self.has_points3() {
            let points = self
                .items
                .iter()
                .map(|i| match i {
                    Item::Point3(a, b, c) => Some([num(a)?, num(b)?, num(c)?]),
                    _ => None,
                })
                .collect::<Option<Vec<_>>>()?;
            return Some(if self.list {
                Value::Points3(points)
            } else {
                Value::Point3(points[0])
            });
        }
        if self.has_points() {
            let points = self
                .items
                .iter()
                .map(|i| match i {
                    Item::Point(a, b) => Some([num(a)?, num(b)?]),
                    _ => None,
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
                _ => None,
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
            Value::Point3(p) => Val {
                items: vec![Item::Point3(c(p[0]), c(p[1]), c(p[2]))],
                list: false,
            },
            Value::Points3(ps) => Val::list(
                ps.iter()
                    .map(|p| Item::Point3(c(p[0]), c(p[1]), c(p[2])))
                    .collect(),
            ),
            Value::Polygon(p) => Val {
                items: vec![Item::Polygon(p.clone())],
                list: false,
            },
            Value::Polygons(ps) => Val::list(ps.iter().cloned().map(Item::Polygon).collect()),
            Value::Polygon3(p) => Val {
                items: vec![Item::Polygon3(p.clone())],
                list: false,
            },
            Value::Polygons3(ps) => Val::list(ps.iter().cloned().map(Item::Polygon3).collect()),
            Value::Complex(a, b) => Val {
                items: vec![Item::complex(c(*a), c(*b))],
                list: false,
            },
            Value::Complexes(zs) => {
                Val::list(zs.iter().map(|z| Item::complex(c(z[0]), c(z[1]))).collect())
            }
        }
    }
}

fn bin(op: BinOp, a: Low, b: Low) -> Low {
    match (&a, &b) {
        (Low::Const(x), Low::Const(y)) => Low::Const(op.apply(*x, *y)),
        _ => Low::Bin(op, Box::new(a), Box::new(b)),
    }
}

/// Works out a node at once when its operands are already numbers, so
/// sums of thousands of constant terms stay numbers instead of growing
/// into trees.
fn call1(f: Builtin, a: Low) -> Low {
    shallow(Low::Call(f, vec![a]))
}

/// e to the power of a complex number.
fn complex_exp(a: Low, b: Low) -> Item {
    let scale = call1(Builtin::Exp, a);
    Item::complex(
        bin(BinOp::Mul, scale.clone(), call1(Builtin::Cos, b.clone())),
        bin(BinOp::Mul, scale, call1(Builtin::Sin, b)),
    )
}

/// The arithmetic of complex numbers, on real and imaginary parts.
fn complex_op(op: BinOp, (a, b): (Low, Low), (c, d): (Low, Low)) -> Result<Item, String> {
    let (add, sub, mul, div) = (
        |x: Low, y: Low| bin(BinOp::Add, x, y),
        |x: Low, y: Low| bin(BinOp::Sub, x, y),
        |x: Low, y: Low| bin(BinOp::Mul, x, y),
        |x: Low, y: Low| bin(BinOp::Div, x, y),
    );
    Ok(match op {
        BinOp::Add => Item::complex(add(a, c), add(b, d)),
        BinOp::Sub => Item::complex(sub(a, c), sub(b, d)),
        BinOp::Mul => Item::complex(
            sub(mul(a.clone(), c.clone()), mul(b.clone(), d.clone())),
            add(mul(a, d), mul(b, c)),
        ),
        BinOp::Div => {
            let den = add(mul(c.clone(), c.clone()), mul(d.clone(), d.clone()));
            Item::complex(
                div(
                    add(mul(a.clone(), c.clone()), mul(b.clone(), d.clone())),
                    den.clone(),
                ),
                div(sub(mul(b, c), mul(a, d)), den),
            )
        }
        BinOp::Pow => {
            // Whole powers multiply out exactly, as z^2 should.
            if let (Low::Const(n), Low::Const(m)) = (&c, &d) {
                if *m == 0.0 && n.fract() == 0.0 && n.abs() <= 64.0 {
                    let mut result = Item::Num(Low::Const(1.0));
                    for _ in 0..n.abs() as u32 {
                        result = complex_op(
                            BinOp::Mul,
                            result.parts().expect("number"),
                            (a.clone(), b.clone()),
                        )?;
                    }
                    if *n < 0.0 {
                        result = complex_op(
                            BinOp::Div,
                            (Low::Const(1.0), Low::Const(0.0)),
                            result.parts().expect("number"),
                        )?;
                    }
                    return Ok(result);
                }
            }
            // e^w directly, and z^w as e^(w ln z) otherwise.
            if matches!((&a, &b), (Low::Const(e), Low::Const(z)) if *e == std::f64::consts::E && *z == 0.0)
            {
                return Ok(complex_exp(c, d));
            }
            let modulus = call1(
                Builtin::Sqrt,
                add(mul(a.clone(), a.clone()), mul(b.clone(), b.clone())),
            );
            let ln_r = call1(Builtin::Ln, modulus);
            let theta = shallow(Low::Call(Builtin::Arctan, vec![b, a]));
            let p = sub(mul(c.clone(), ln_r.clone()), mul(d.clone(), theta.clone()));
            let q = add(mul(c, theta), mul(d, ln_r));
            complex_exp(p, q)
        }
        _ => return Err("Complex numbers can't be compared".into()),
    })
}

/// A hash map keyed by names, with a hash much quicker than the standard
/// one: names come from the user's own rows, so resisting crafted keys does
/// not matter, and heavy graphs look names up millions of times.
type FastMap<K, V> = HashMap<K, V, std::hash::BuildHasherDefault<FxHasher>>;

/// The Fx hash (from Firefox and rustc).
#[derive(Default)]
struct FxHasher(u64);

impl std::hash::Hasher for FxHasher {
    fn write(&mut self, bytes: &[u8]) {
        for chunk in bytes.chunks(8) {
            let mut word = [0u8; 8];
            word[..chunk.len()].copy_from_slice(chunk);
            self.write_u64(u64::from_le_bytes(word));
        }
    }

    fn write_u64(&mut self, v: u64) {
        self.0 = (self.0.rotate_left(5) ^ v).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
    }

    fn finish(&self) -> u64 {
        self.0
    }
}

/// Notes in the innermost recording that the value being worked out read all
/// of `reads`.
fn record_into(recording: &mut [Vec<Read>], reads: &[Read]) {
    if let Some(frame) = recording.last_mut() {
        for r in reads {
            if !frame.contains(r) {
                frame.push(r.clone());
            }
        }
    }
}

/// A plain number as its real and imaginary parts (0 for a real number), for
/// [`Env::quick`].
use crate::fastsum::{self, scalar_op, Scalar};

/// The number `v` holds, when it is one constant real or complex number.
fn scalar_of(v: &Val) -> Option<Scalar> {
    if v.list || v.items.len() != 1 {
        return None;
    }
    scalar_item(&v.items[0])
}

fn scalar_item(item: &Item) -> Option<Scalar> {
    match item {
        Item::Num(Low::Const(a)) => Some((*a, 0.0)),
        Item::Complex(Low::Const(a), Low::Const(b)) => Some((*a, *b)),
        _ => None,
    }
}

/// A built-in function of plain numbers, as `Env::call` works it out: real
/// arguments directly, and a complex one for abs, exp and sqrt.
pub(crate) fn scalar_call(f: Builtin, parts: &[Scalar]) -> Option<Scalar> {
    if f == Builtin::Length || parts.is_empty() {
        return None;
    }
    let k = Low::Const;
    if parts.iter().any(|p| p.1 != 0.0) {
        let [(a, b)] = parts[..] else {
            return None;
        };
        return scalar_item(&match f {
            Builtin::Abs => Item::Num(call1(
                Builtin::Sqrt,
                bin(
                    BinOp::Add,
                    bin(BinOp::Mul, k(a), k(a)),
                    bin(BinOp::Mul, k(b), k(b)),
                ),
            )),
            Builtin::Exp => complex_exp(k(a), k(b)),
            Builtin::Sqrt => complex_op(BinOp::Pow, (k(a), k(b)), (k(0.5), k(0.0))).ok()?,
            _ => return None,
        });
    }
    if let [(a, _)] = parts {
        return Some((f.eval(&[*a]), 0.0));
    }
    let values: Vec<f64> = parts.iter().map(|p| p.0).collect();
    Some((f.eval(&values), 0.0))
}

fn shallow(l: Low) -> Low {
    match l {
        Low::Neg(a) => match *a {
            Low::Const(v) => Low::Const(-v),
            a => Low::Neg(Box::new(a)),
        },
        Low::Call(f, args) if args.iter().all(|a| matches!(a, Low::Const(_))) => {
            let values: Vec<f64> = args
                .iter()
                .map(|a| match a {
                    Low::Const(v) => *v,
                    _ => f64::NAN,
                })
                .collect();
            Low::Const(f.eval(&values))
        }
        Low::Select(c, a, b) => match *c {
            Low::Const(v) if v.is_nan() => Low::Const(f64::NAN),
            Low::Const(v) if v != 0.0 => *a,
            Low::Const(_) => *b,
            c => Low::Select(Box::new(c), a, b),
        },
        other => other,
    }
}

pub fn analyze(rows: &[RowInput]) -> Vec<RowAnalysis> {
    analyze_cached(rows, &mut Cache::default())
}

/// Whether rows are read for a flat graph or a 3D one, where z is a graph
/// variable and `z = f(x, y)` is a surface.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Flat,
    ThreeD,
}

/// As `analyze`, reusing variable values from `cache` where nothing they
/// depend on has changed, and leaving this analysis's values in it.
pub fn analyze_cached(rows: &[RowInput], cache: &mut Cache) -> Vec<RowAnalysis> {
    analyze_mode(rows, cache, Mode::Flat)
}

/// As `analyze_cached`, for a graph in the given mode.
pub fn analyze_mode(rows: &[RowInput], cache: &mut Cache, mode: Mode) -> Vec<RowAnalysis> {
    let parsed: Vec<Option<Result<Statement, String>>> = rows
        .iter()
        .map(|r| (r.table.is_none() && !r.latex.trim().is_empty()).then(|| parse(&r.latex)))
        .collect();
    let mut tables: Vec<Option<Table>> = rows
        .iter()
        .map(|r| r.table.as_deref().map(Table::read))
        .collect();

    // Collect definitions, refusing duplicates and reserved names.
    let mut defs: Vec<Option<Result<Def, String>>> = parsed
        .iter()
        .map(|p| match p {
            // In 3D, z = … is a surface rather than a variable, and ρ and φ
            // are spherical coordinates.
            Some(Ok(statement)) => definition(statement).filter(|d| {
                !(mode == Mode::ThreeD
                    && matches!(d, Ok(Def::Var(n, _)) if SPACE_VARIABLES.contains(&n.as_str())))
            }),
            _ => None,
        })
        .collect();
    let mut counts: HashMap<String, usize> = HashMap::new();
    for def in defs.iter().flatten().flatten() {
        *counts.entry(def_name(def).to_owned()).or_default() += 1;
    }
    for table in tables.iter().flatten() {
        for name in table.defined_names() {
            *counts.entry(name.to_owned()).or_default() += 1;
        }
    }
    let twice = |name: &str| format!("'{}' is defined more than once", name);
    for def in defs.iter_mut() {
        if let Some(Ok(d)) = def {
            let name = def_name(d).to_owned();
            if counts[&name] > 1 {
                *def = Some(Err(twice(&name)));
            }
        }
    }
    for table in tables.iter_mut().flatten() {
        for column in &mut table.columns {
            if let Header::Defines(name) = &column.header {
                if counts[name] > 1 {
                    column.header = Header::Bad(twice(name));
                }
            }
        }
    }

    let mut env = Env {
        vars: FastMap::default(),
        funcs: FastMap::default(),
        values: FastMap::default(),
        fast_lists: FastMap::default(),
        next_hole: 0,
        actions: HashSet::new(),
        calls: FastMap::default(),
        snapshots: FastMap::default(),
        checked: FastMap::default(),
        old: std::mem::take(cache),
        new: Cache::default(),
        recording: Vec::new(),
        mode,
    };
    for table in tables.iter().flatten() {
        for column in &table.columns {
            if let Header::Defines(name) = &column.header {
                env.vars
                    .insert(name.clone(), Expr::List(column.cells.clone()));
            }
        }
    }
    for def in defs.iter().flatten().flatten() {
        match def {
            Def::Var(name, expr) => {
                env.vars.insert(name.clone(), expr.clone());
            }
            Def::Func(name, f) => {
                let def = match env.old.funcs.get(name) {
                    Some(old) if old.params == f.params && old.body == f.body => Rc::clone(old),
                    _ => Rc::new(FuncDef {
                        params: f.params.clone(),
                        body: f.body.clone(),
                    }),
                };
                env.funcs.insert(name.clone(), def);
            }
        }
    }

    // Names of actions, including names for other names of actions.
    loop {
        let found: Vec<String> = env
            .vars
            .iter()
            .filter(|(n, e)| !env.actions.contains(*n) && env.is_action(e))
            .map(|(n, _)| n.clone())
            .collect();
        if found.is_empty() {
            break;
        }
        env.actions.extend(found);
    }

    // Regressions define their parameters and residuals for every other
    // row, so they are fitted before any row is worked out.
    let mut fits: HashMap<u64, Result<Fit, String>> = HashMap::new();
    for (row, statement) in rows.iter().zip(&parsed) {
        if let Some(Ok(Statement::Regression(lhs, rhs))) = statement {
            let fit = env.fit(lhs, rhs);
            if let Ok((fit, residuals)) = &fit {
                for (name, v) in &fit.params {
                    env.vars.insert(name.clone(), Expr::Num(*v));
                }
                if let Some(name) = &fit.residuals {
                    let cells = residuals.iter().map(|r| Expr::Num(*r)).collect();
                    env.vars.insert(name.clone(), Expr::List(cells));
                }
            }
            fits.insert(row.id, fit.map(|(fit, _)| fit));
        }
    }

    let results = rows
        .iter()
        .zip(parsed)
        .zip(defs)
        .zip(tables)
        .map(|(((row, parsed), def), table)| {
            let mut point = None;
            let mut out = match (parsed, def) {
                _ if table.is_some() => env.table_row(row.id, &table.expect("checked")),
                (None, _) => RowAnalysis::new(row.id, RowKind::Empty),
                (Some(Err(e)), _) => RowAnalysis::error(row.id, e),
                (Some(Ok(_)), Some(Err(e))) => RowAnalysis::error(row.id, e),
                (Some(Ok(_)), Some(Ok(Def::Var(name, _)))) if env.actions.contains(&name) => {
                    let expr = env.vars[&name].clone();
                    env.action_row(row.id, &expr)
                }
                (Some(Ok(Statement::Expr(e))), None) if env.is_action(&e) => {
                    env.action_row(row.id, &e)
                }
                (Some(Ok(_)), Some(Ok(Def::Var(name, expr)))) => {
                    point = Some(expr.clone());
                    env.variable_row(row.id, &name, &expr)
                }
                (Some(Ok(_)), Some(Ok(Def::Func(name, f)))) => env.function_row(row.id, &name, &f),
                (Some(Ok(Statement::Regression(_, rhs))), None) => {
                    let fit = fits.remove(&row.id).expect("fitted above");
                    env.regression_row(row.id, &rhs, fit)
                }
                (Some(Ok(statement)), None) => {
                    if let Statement::Expr(e) = &statement {
                        point = Some(e.clone());
                    }
                    env.other_row(row, &statement)
                }
            };
            if out.error.is_none() && out.points.len() == 1 {
                out.drag = point.and_then(|e| env.drag_of(&e));
            }
            match mode {
                Mode::ThreeD => into_3d(&mut out),
                Mode::Flat => flat(&mut out),
            }
            out
        })
        .collect::<Vec<_>>();
    env.new.funcs = std::mem::take(&mut env.funcs);
    *cache = std::mem::take(&mut env.new);
    results
}

/// In a 3D graph, flat points and parametric curves sit on the xy plane,
/// as in Desmos 3D. What only a flat graph can draw (curves of x, shading,
/// polygons, tables) is dropped and the row says so.
fn into_3d(row: &mut RowAnalysis) {
    for p in std::mem::take(&mut row.points) {
        row.points3.push([p[0], p[1], 0.0]);
    }
    if !row.parametrics.is_empty() {
        let zero = Program::compile(&Low::Const(0.0)).expect("a constant compiles");
        for p in std::mem::take(&mut row.parametrics) {
            row.curves3.push(Curve3 {
                x: p.x,
                y: p.y,
                z: zero.clone(),
                domain: p.domain,
            });
        }
    }
    for p in std::mem::take(&mut row.polygons) {
        row.polygons3
            .push(p.iter().map(|q| [q[0], q[1], 0.0]).collect());
    }
    row.drag = None;
    let flat = !(row.curves.is_empty()
        && row.odes.is_empty()
        && row.regions.is_empty()
        && row.polygons.is_empty()
        && row.columns.iter().all(|c| c.points.is_empty()));
    if !flat {
        return;
    }
    row.curves.clear();
    row.odes.clear();
    row.ode = None;
    row.seeds.clear();
    row.regions.clear();
    row.polygons.clear();
    row.parameter = None;
    for column in &mut row.columns {
        column.points.clear();
    }
    if row.error.is_none() {
        row.error = Some("3D graphs can't draw this yet".into());
    }
}

/// In a flat graph, 3D points and curves can't be drawn.
fn flat(row: &mut RowAnalysis) {
    if row.points3.is_empty()
        && row.curves3.is_empty()
        && row.param_surfaces.is_empty()
        && row.polygons3.is_empty()
    {
        return;
    }
    row.points3.clear();
    row.polygons3.clear();
    row.curves3.clear();
    row.param_surfaces.clear();
    row.parameter = None;
    if row.error.is_none() {
        row.error = Some("Points with three coordinates need a 3D graph".into());
    }
}

fn def_name(def: &Def) -> &str {
    match def {
        Def::Var(n, _) | Def::Func(n, _) => n,
    }
}

fn definable(name: &str) -> bool {
    // `y'` is a derivative in a differential equation, never a variable.
    !name.contains('\'')
        && !GRAPH_VARIABLES.contains(&name)
        && !CONSTANTS.iter().any(|(c, _)| *c == name)
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

/// What a table column's header says.
enum Header {
    /// No header yet.
    Blank,
    /// A name such as `y_1`, which the column's cells define as a list.
    Defines(String),
    /// `x` or `y`: the cells are plotted but define nothing.
    Plain(String),
    /// A formula of the first column, such as `x_1^2`, filled in by the
    /// engine.
    Formula(Expr),
    Bad(String),
}

struct TableColumn {
    header: Header,
    /// One expression per table row; blank cells are NaN.
    cells: Vec<Expr>,
    cell_errors: Vec<(usize, String)>,
    /// Whether any cell has something typed in it.
    filled: bool,
}

/// A table with its headers and cells parsed.
struct Table {
    columns: Vec<TableColumn>,
    /// Rows up to the last one with anything in it.
    rows: usize,
}

impl Table {
    fn read(columns: &[ColumnInput]) -> Table {
        let blank = |s: &str| s.trim().is_empty();
        let rows = columns
            .iter()
            .filter_map(|c| c.cells.iter().rposition(|cell| !blank(cell)))
            .max()
            .map_or(0, |last| last + 1);
        let columns = columns
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let header = if blank(&c.header) {
                    Header::Blank
                } else {
                    match parse(&c.header) {
                        Err(e) => Header::Bad(e),
                        Ok(Statement::Expr(Expr::Var(name))) if definable(&name) => {
                            Header::Defines(name)
                        }
                        Ok(Statement::Expr(Expr::Var(name))) if name == "x" || name == "y" => {
                            Header::Plain(name)
                        }
                        Ok(Statement::Expr(Expr::Var(name)))
                            if CONSTANTS.iter().any(|(c, _)| *c == name) =>
                        {
                            Header::Bad(format!(
                                "'{name}' is a built-in constant and can't be redefined"
                            ))
                        }
                        Ok(Statement::Expr(_)) if i == 0 => {
                            Header::Bad("The first column needs a name, like x_1".into())
                        }
                        Ok(Statement::Expr(e)) => Header::Formula(e),
                        Ok(_) => Header::Bad(
                            "A column header is a name like y_1 or a formula like x_1^2".into(),
                        ),
                    }
                };
                let mut cell_errors = Vec::new();
                let cells = (0..rows)
                    .map(|r| {
                        let latex = c.cells.get(r).map_or("", String::as_str);
                        if blank(latex) {
                            return Expr::Num(f64::NAN);
                        }
                        match parse(latex) {
                            Ok(Statement::Expr(e)) => e,
                            Ok(_) => {
                                cell_errors.push((r, "A cell holds a number or expression".into()));
                                Expr::Num(f64::NAN)
                            }
                            Err(e) => {
                                cell_errors.push((r, e));
                                Expr::Num(f64::NAN)
                            }
                        }
                    })
                    .collect();
                TableColumn {
                    header,
                    cells,
                    cell_errors,
                    filled: c.cells.iter().any(|cell| !blank(cell)),
                }
            })
            .collect();
        Table { columns, rows }
    }

    fn defined_names(&self) -> impl Iterator<Item = &str> {
        self.columns.iter().filter_map(|c| match &c.header {
            Header::Defines(name) => Some(name.as_str()),
            _ => None,
        })
    }
}

struct Env {
    vars: FastMap<String, Expr>,
    /// Shared, so inlining a call does not copy the function's body.
    funcs: FastMap<String, Rc<FuncDef>>,
    /// Memoised variable values.
    values: FastMap<String, Result<Value, String>>,
    /// List values as compiled sums read them, made once per analysis.
    fast_lists: FastMap<String, Rc<[Scalar]>>,
    /// The next unused placeholder for a variable being differentiated or
    /// integrated over.
    next_hole: u32,
    /// Variables that name actions, as in `A = a \to a + 1`.
    actions: HashSet<String>,
    /// Results of function calls with constant arguments, so a function
    /// called thousands of times with the same arguments is worked out once.
    calls: FastMap<CallKey, (Val, Vec<&'static str>, Vec<Read>)>,
    /// The value each variable had when a remembered value read it, shared
    /// by everything remembered in this analysis.
    snapshots: FastMap<String, Rc<Result<Value, String>>>,
    /// Variables already compared with the last analysis's values.
    checked: FastMap<String, bool>,
    /// Values from the last analysis, and this one's, for the next.
    old: Cache,
    new: Cache,
    /// What each value being worked out has read so far, innermost last.
    recording: Vec<Vec<Read>>,
    mode: Mode,
}

/// A value's constants as bits, to key remembered function calls; None
/// when it depends on a graph variable.
fn constant_key(v: &Val, key: &mut Vec<u64>) -> Option<()> {
    let bits = |l: &Low| match l {
        Low::Const(c) => Some(c.to_bits()),
        _ => None,
    };
    key.push(if v.list { v.items.len() as u64 + 1 } else { 0 });
    for item in &v.items {
        match item {
            Item::Num(a) => key.extend([0, bits(a)?]),
            Item::Point(a, b) => key.extend([1, bits(a)?, bits(b)?]),
            Item::Complex(a, b) => key.extend([2, bits(a)?, bits(b)?]),
            Item::Point3(a, b, c) => key.extend([3, bits(a)?, bits(b)?, bits(c)?]),
            Item::Polygon(_) | Item::Polygon3(_) => return None,
        }
    }
    Some(())
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
    /// The slider variables a point's coordinates are, when it is written
    /// as `(a, b)` or `(a, 3)` with `a` and `b` defined as plain numbers.
    fn drag_of(&self, e: &Expr) -> Option<[Option<String>; 2]> {
        let Expr::Point(x, y) = e else { return None };
        let slider = |c: &Expr| match c {
            Expr::Var(n) if self.vars.get(n).and_then(Expr::as_literal).is_some() => {
                Some(n.clone())
            }
            _ => None,
        };
        let drag = [slider(x), slider(y)];
        (drag[0].is_some() || drag[1].is_some()).then_some(drag)
    }

    /// Whether `e` is an action, by its form or because it names one.
    fn is_action(&self, e: &Expr) -> bool {
        match e {
            Expr::Var(n) => self.actions.contains(n),
            Expr::Piecewise(branches, otherwise) => {
                branches.iter().any(|(_, v)| self.is_action(v))
                    || otherwise.as_ref().is_some_and(|o| self.is_action(o))
            }
            other => other.is_action(),
        }
    }

    fn action_row(&mut self, id: u64, e: &Expr) -> RowAnalysis {
        let mut row = RowAnalysis::new(id, RowKind::Action);
        let mut problems = Problems::default();
        match self.run(e, &mut problems, &mut HashSet::new()) {
            Some(updates) if problems.errors.is_empty() && problems.missing.is_empty() => {
                row.updates = updates
            }
            _ => {
                row.error = problems.first_message();
                row.missing = slider_names(&problems.missing);
            }
        }
        row
    }

    /// What running the action `e` would set each variable to, from the
    /// current values. Every new value is worked out before any is set, as
    /// in Desmos, so `a \to b, b \to a` swaps them.
    fn run(
        &mut self,
        e: &Expr,
        problems: &mut Problems,
        visiting: &mut HashSet<String>,
    ) -> Option<Vec<(String, Value)>> {
        match e {
            Expr::Update(name, value) => {
                if !self.vars.contains_key(name) || self.actions.contains(name) {
                    if self.actions.contains(name) || self.funcs.contains_key(name) {
                        problems.error(format!("'{name}' can't be updated by an action"));
                    } else if definable(name) {
                        problems.missing(name);
                    } else {
                        problems.error(format!("'{name}' can't be updated by an action"));
                    }
                    return None;
                }
                let val = self.lower_with(value, &HashMap::new(), NO_FREE, 0, problems, visiting);
                if !problems.errors.is_empty() || !problems.missing.is_empty() {
                    return None;
                }
                match val.constant() {
                    Some(v) => Some(vec![(name.clone(), v)]),
                    None => {
                        problems.error("An action's new value can't depend on x or y");
                        None
                    }
                }
            }
            Expr::Actions(items) => {
                let mut all: Vec<(String, Value)> = Vec::new();
                for item in items {
                    for (name, v) in self.run(item, problems, visiting)? {
                        if all.iter().any(|(n, _)| *n == name) {
                            problems.error(format!("'{name}' is updated twice by one action"));
                            return None;
                        }
                        all.push((name, v));
                    }
                }
                Some(all)
            }
            Expr::Var(name) if self.actions.contains(name) => {
                if !visiting.insert(name.clone()) {
                    problems.error(format!("'{name}' is defined in terms of itself"));
                    return None;
                }
                let body = self.vars[name].clone();
                let out = self.run(&body, problems, visiting);
                visiting.remove(name);
                out
            }
            Expr::Piecewise(branches, otherwise) => {
                for (condition, value) in branches {
                    let c =
                        self.condition(condition, &HashMap::new(), NO_FREE, 0, problems, visiting);
                    let holds = match c.constant() {
                        Some(Value::Number(v)) => v != 0.0 && !v.is_nan(),
                        _ => {
                            problems.error("An action's conditions can't depend on x or y");
                            return None;
                        }
                    };
                    if holds {
                        return self.run(value, problems, visiting);
                    }
                }
                match otherwise {
                    Some(o) => self.run(o, problems, visiting),
                    None => Some(Vec::new()),
                }
            }
            _ => {
                problems.error("This isn't an action; an action looks like a \\to a+1");
                None
            }
        }
    }

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
                // A point or polygon variable is drawn, as in Desmos.
                row.points = points_of(&value);
                row.points3 = points3_of(&value);
                row.polygons = polygons_of(&value);
                row.polygons3 = polygons3_of(&value);
                if !row.polygons.is_empty() || !row.polygons3.is_empty() {
                    row.value = None;
                }
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
        // In a 3D graph, f(x, y) = … is also drawn as the surface z = f(x, y).
        let surface = self.mode == Mode::ThreeD && f.params == ["x", "y"];
        if surface {
            let bindings: HashMap<String, Val> = [
                ("x".to_owned(), Val::num(Low::Arg(Arg::X))),
                ("y".to_owned(), Val::num(Low::Arg(Arg::Y))),
            ]
            .into_iter()
            .collect();
            let mut problems = Problems::default();
            let val = self.lower(&f.body, &bindings, PARAM_ONLY, &mut problems);
            row.missing = slider_names(&problems.missing);
            row.error = problems.first_message();
            if row.error.is_none() && !val.has_points() {
                row.surfaces = compile_all(&val, &mut row.error, |program| Surface { program });
            }
            return row;
        }
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
        if let Some(order) = ode_order(statement) {
            if self.mode == Mode::ThreeD {
                return RowAnalysis::error(id, "Differential equations are drawn in 2D graphs");
            }
            return self.ode_row(input, statement, order);
        }
        if self.mode == Mode::ThreeD {
            return self.other_row_3d(input, statement);
        }
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
            Statement::Regression(..) => unreachable!("regressions have their own rows"),
            Statement::Expr(e) => self.expression_row(input, e),
        }
    }

    /// A differential equation: `y⁽ⁿ⁾ = f(x, y, …)`, an equation in x, y
    /// and its derivatives not solved for the highest, or a system
    /// `(x', y') = (f, g)`. `order` is the highest derivative of y, or 0
    /// for a system.
    fn ode_row(&mut self, input: &RowInput, statement: &Statement, order: usize) -> RowAnalysis {
        let id = input.id;
        let Statement::Relation(lhs, rel, rhs) = statement else {
            return RowAnalysis::error(id, "Write a differential equation like y' = x - y");
        };
        if *rel != Relation::Eq {
            return RowAnalysis::error(id, "Differential equations are written with =");
        }
        let mut row = RowAnalysis::new(id, RowKind::Curve);
        let mut problems = Problems::default();
        let is_system =
            |e: &Expr| matches!(e, Expr::Point(a, b) if is_var(a, "x'") && is_var(b, "y'"));
        if is_system(lhs) || is_system(rhs) {
            let f = if is_system(lhs) { rhs } else { lhs };
            if ode_order(&Statement::Expr(f.clone())).is_some() {
                return RowAnalysis::error(id, "x' and y' can't depend on x' or y'");
            }
            row.ode = Some(1);
            let val = self.lower(f, &HashMap::new(), SYSTEM_FREE, &mut problems);
            row.missing = slider_names(&problems.missing);
            row.error = problems.first_message();
            for item in &val.items {
                if row.error.is_some() {
                    break;
                }
                let Item::Point(a, b) = item else {
                    row.error = Some("A system needs a point of slopes, like (y, -x)".into());
                    break;
                };
                match (Program::compile(a), Program::compile(b)) {
                    (Ok(f), Ok(g)) => row.odes.push(Ode::System { f, g }),
                    (Err(e), _) | (_, Err(e)) => row.error = Some(e),
                }
            }
        } else {
            let mentions_x = |e: &Expr| {
                let mut found = false;
                e.visit_names(&mut |n, _| found |= n.starts_with("x'"));
                found
            };
            if mentions_x(lhs) || mentions_x(rhs) {
                return RowAnalysis::error(id, "x' belongs in a system, like (x', y') = (y, -x)");
            }
            if order > MAX_ODE_ORDER {
                return RowAnalysis::error(
                    id,
                    format!("Differential equations go up to order {MAX_ODE_ORDER}"),
                );
            }
            row.ode = Some(order);
            let top = format!("y{}", "'".repeat(order));
            let explicit = if is_var(lhs, &top) && !rhs.mentions(&top) {
                Some(rhs)
            } else if is_var(rhs, &top) && !lhs.mentions(&top) {
                Some(lhs)
            } else {
                None
            };
            let (e, free) = match explicit {
                Some(f) => (f.clone(), &ODE_FREE[..order + 1]),
                None => (
                    Expr::bin(Expr::Sub, lhs.clone(), rhs.clone()),
                    &ODE_FREE[..order + 2],
                ),
            };
            let val = self.lower(&e, &HashMap::new(), free, &mut problems);
            row.missing = slider_names(&problems.missing);
            row.error = problems.first_message();
            if row.error.is_none() && val.has_points() {
                row.error = Some(format!("{top} can't equal a point"));
            }
            if row.error.is_none() {
                row.odes = compile_all(&val, &mut row.error, |f| match explicit {
                    Some(_) => Ode::Explicit { order, f },
                    None => Ode::Implicit { order, f },
                });
            }
        }
        self.read_seeds(input, &mut row);
        row
    }

    /// A differential equation's initial points and derivatives.
    fn read_seeds(&mut self, input: &RowInput, row: &mut RowAnalysis) {
        let order = row.ode.unwrap_or(1);
        let latex = input.seeds.as_deref().unwrap_or("").trim();
        if latex.is_empty() {
            row.seeds_fixed = true;
            return;
        }
        let mut missing = Vec::new();
        let mut value_of = |env: &mut Env, latex: &str| -> Result<(Expr, Option<Value>), String> {
            let e = match parse(latex)? {
                Statement::Expr(e) => e,
                _ => return Err(SEEDS_HINT.into()),
            };
            let mut problems = Problems::default();
            let val = env.lower(&e, &HashMap::new(), NO_FREE, &mut problems);
            missing.extend(slider_names(&problems.missing));
            match problems.first_message() {
                Some(m) => Err(m),
                None => Ok((e, val.constant())),
            }
        };
        let (seeds, at) = match value_of(self, latex) {
            Ok((e, Some(Value::Point(x, y)))) => (e, vec![[x, y]]),
            Ok((e, Some(Value::Points(ps)))) => (e, ps),
            Ok((e, Some(Value::Numbers(ns)))) if ns.is_empty() => (e, Vec::new()),
            Ok(_) => {
                row.seeds_error = Some(SEEDS_HINT.into());
                (Expr::List(Vec::new()), Vec::new())
            }
            Err(e) => {
                row.seeds_error = Some(e);
                (Expr::List(Vec::new()), Vec::new())
            }
        };
        // y'(x₀), y''(x₀), …: a number for every point, or a list with one
        // for each.
        let mut derivs: Vec<Vec<f64>> = Vec::new();
        for i in 0..order.saturating_sub(1) {
            let latex = input.derivs.get(i).map_or("", |l| l.trim());
            let values = if latex.is_empty() {
                Ok(vec![0.0])
            } else {
                match value_of(self, latex) {
                    Ok((_, Some(Value::Number(v)))) => Ok(vec![v]),
                    Ok((_, Some(Value::Numbers(vs)))) if !vs.is_empty() => Ok(vs),
                    Ok(_) => Err("Initial derivatives are numbers, or a list of them".into()),
                    Err(e) => Err(e),
                }
            };
            match values {
                Ok(vs) => derivs.push(vs),
                Err(e) => {
                    row.derivs_error.get_or_insert(e);
                    derivs.push(vec![f64::NAN]);
                }
            }
        }
        for name in missing {
            if !row.missing.contains(&name) {
                row.missing.push(name);
            }
        }
        let drags: Vec<Option<[Option<String>; 2]>> = match &seeds {
            Expr::List(items) if items.len() == at.len() => {
                items.iter().map(|e| self.drag_of(e)).collect()
            }
            e if at.len() == 1 => vec![self.drag_of(e)],
            _ => vec![None; at.len()],
        };
        row.seeds_fixed = row.seeds_error.is_none() && is_literal(&seeds);
        row.seeds = at
            .into_iter()
            .zip(drags)
            .enumerate()
            .map(|(i, (at, drag))| Seed {
                at,
                drag,
                derivs: derivs
                    .iter()
                    .map(|vs| match vs.len() {
                        1 => vs[0],
                        _ => vs.get(i).copied().unwrap_or(f64::NAN),
                    })
                    .collect(),
            })
            .collect();
    }

    /// A row of a 3D graph that defines nothing: `z = f(x, y)` (either way
    /// round) or a bare expression in x and y is a surface, and anything
    /// without a graph variable is a value or points as in 2D.
    fn other_row_3d(&mut self, input: &RowInput, statement: &Statement) -> RowAnalysis {
        let id = input.id;
        let spherical = |e: &Expr| e.mentions("rho") || e.mentions("phi");
        let spatial = |e: &Expr| ["x", "y", "z", "r", "theta"].iter().any(|v| e.mentions(v));
        match statement {
            Statement::Relation(lhs, Relation::Eq, rhs)
                if is_var(lhs, "z") && !rhs.mentions("z") && !spherical(rhs) =>
            {
                self.surface_row(id, rhs)
            }
            Statement::Relation(lhs, Relation::Eq, rhs)
                if is_var(rhs, "z") && !lhs.mentions("z") && !spherical(lhs) =>
            {
                self.surface_row(id, lhs)
            }
            Statement::Relation(lhs, Relation::Eq, rhs) => self.implicit3_row(id, lhs, rhs),
            Statement::Expr(e) if e.mentions("z") || spherical(e) => RowAnalysis {
                kind: RowKind::NotYet,
                ..RowAnalysis::error(id, "Try writing this as an equation, such as z = …")
            },
            Statement::Expr(e) if self.uses_uv(e) => self.param_surface_row(id, e),
            Statement::Expr(e) if spatial(e) && !self.is_parametric(e) => self.surface_row(id, e),
            Statement::Expr(e) => self.expression_row(input, e),
            Statement::Relation(lhs, rel, rhs) => {
                let condition = Condition {
                    operands: vec![lhs.clone(), rhs.clone()],
                    relations: vec![*rel],
                };
                self.solid_row(id, &condition)
            }
            Statement::Chain(condition) => self.solid_row(id, condition),
            Statement::Regression(..) => unreachable!("regressions have their own rows"),
        }
    }

    /// Lowers one side of an equation in space: x, y and z, with r and θ
    /// measured around the z axis and ρ and φ from the origin and the z
    /// axis, as in Desmos 3D.
    fn lower_space(&mut self, e: &Expr, problems: &mut Problems) -> Val {
        let x = || Low::Arg(Arg::X);
        let y = || Low::Arg(Arg::Y);
        let z = || Low::Arg(Arg::Z);
        let sq = |a: Low| bin(BinOp::Mul, a.clone(), a);
        let r = Low::Call(Builtin::Sqrt, vec![bin(BinOp::Add, sq(x()), sq(y()))]);
        let rho = Low::Call(
            Builtin::Sqrt,
            vec![bin(BinOp::Add, bin(BinOp::Add, sq(x()), sq(y())), sq(z()))],
        );
        let theta = Low::Call(
            Builtin::Mod,
            vec![
                Low::Call(Builtin::Arctan, vec![y(), x()]),
                Low::Const(std::f64::consts::TAU),
            ],
        );
        let phi = Low::Call(Builtin::Arccos, vec![bin(BinOp::Div, z(), rho.clone())]);
        let bindings: HashMap<String, Val> = [
            ("r".to_owned(), Val::num(r)),
            ("theta".to_owned(), Val::num(theta)),
            ("rho".to_owned(), Val::num(rho)),
            ("phi".to_owned(), Val::num(phi)),
        ]
        .into_iter()
        .collect();
        self.lower(
            e,
            &bindings,
            &[("x", Arg::X), ("y", Arg::Y), ("z", Arg::Z)],
            problems,
        )
    }

    /// An equation in space, drawn where the left side equals the right.
    fn implicit3_row(&mut self, id: u64, lhs: &Expr, rhs: &Expr) -> RowAnalysis {
        let mut problems = Problems::default();
        let left = self.lower_space(lhs, &mut problems);
        let right = self.lower_space(rhs, &mut problems);
        let difference = self.broadcast(&[left, right], &mut problems, |items| {
            match (items[0], items[1]) {
                (Item::Num(a), Item::Num(b)) => {
                    Ok(Item::Num(bin(BinOp::Sub, a.clone(), b.clone())))
                }
                _ => Err("Points can't be compared".into()),
            }
        });
        let mut row = RowAnalysis::new(id, RowKind::Curve);
        row.missing = slider_names(&problems.missing);
        row.error = problems.first_message();
        if row.error.is_some() {
            return row;
        }
        let mut constant = None;
        for item in &difference.items {
            let Item::Num(low) = item else { continue };
            match Program::compile(low) {
                Ok(program) if !program.uses_arg() => constant = program.constant(),
                Ok(program) => row.implicits3.push(Implicit3 {
                    program,
                    shade: None,
                }),
                Err(e) => row.error = Some(e),
            }
        }
        if row.implicits3.is_empty() && row.error.is_none() {
            row.kind = RowKind::Value;
            row.value = constant.map(Value::Number);
        }
        row
    }

    /// Whether a bare expression is a 3D point that reads u or v (unless a
    /// row defines them), which makes it a surface.
    fn uses_uv(&mut self, e: &Expr) -> bool {
        if !(e.mentions("u") || e.mentions("v")) {
            return false;
        }
        let mut free = Vec::new();
        for (name, arg) in [("u", Arg::X), ("v", Arg::Y)] {
            if !self.vars.contains_key(name) {
                free.push((name, arg));
            }
        }
        let mut probe = Problems::default();
        let val = self.lower(e, &HashMap::new(), &free, &mut probe);
        val.has_points3() && (probe.uses("u") || probe.uses("v"))
    }

    /// `(f(u, v), g(u, v), h(u, v))`, one surface per point in a list,
    /// over Desmos 3D's 0 ≤ u ≤ 1 and 0 ≤ v ≤ 1.
    fn param_surface_row(&mut self, id: u64, e: &Expr) -> RowAnalysis {
        let mut row = RowAnalysis::new(id, RowKind::Curve);
        let mut problems = Problems::default();
        let mut free = Vec::new();
        for (name, arg) in [("u", Arg::X), ("v", Arg::Y)] {
            if !self.vars.contains_key(name) {
                free.push((name, arg));
            }
        }
        let val = self.lower(e, &HashMap::new(), &free, &mut problems);
        row.missing = slider_names(&problems.missing);
        row.error = problems.first_message();
        if row.error.is_some() {
            return row;
        }
        for item in &val.items {
            let compiled = match item {
                Item::Point3(a, b, c) => (|| {
                    let compile = |l: &Low| Program::compile(l);
                    row.param_surfaces.push(ParamSurface {
                        x: compile(a)?,
                        y: compile(b)?,
                        z: compile(c)?,
                        u: UV_DOMAIN,
                        v: UV_DOMAIN,
                    });
                    Ok(())
                })(),
                _ => Err("A surface needs points with three coordinates".to_string()),
            };
            if let Err(e) = compiled {
                row.error = Some(e);
                row.param_surfaces.clear();
                break;
            }
        }
        row
    }

    /// Whether a bare expression is a point that moves with t.
    fn is_parametric(&mut self, e: &Expr) -> bool {
        let mut probe = Problems::default();
        let val = self.lower(e, &HashMap::new(), &[("t", Arg::X)], &mut probe);
        val.has_points()
    }

    /// `z = f(x, y)`, one surface per list element. r and θ are measured
    /// around the z axis, so `z = r` is a cone.
    fn surface_row(&mut self, id: u64, expr: &Expr) -> RowAnalysis {
        let mut row = RowAnalysis::new(id, RowKind::Curve);
        let mut problems = Problems::default();
        let val = self.lower_plane(expr, &mut problems);
        row.missing = slider_names(&problems.missing);
        row.error = problems.first_message();
        if row.error.is_none() && val.has_points() {
            row.error = Some("z can't equal a point".into());
        }
        if row.error.is_none() {
            row.surfaces = compile_all(&val, &mut row.error, |program| Surface { program });
        }
        row
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
                let compile = |l: &Low| Program::compile(l);
                let compiled = match item {
                    Item::Point(a, b) => (|| {
                        let (x, y) = (compile(a)?, compile(b)?);
                        row.parametrics.push(Parametric {
                            x,
                            y,
                            domain: parameter.domain,
                        });
                        Ok(())
                    })(),
                    Item::Point3(a, b, c) => (|| {
                        let (x, y, z) = (compile(a)?, compile(b)?, compile(c)?);
                        row.curves3.push(Curve3 {
                            x,
                            y,
                            z,
                            domain: parameter.domain,
                        });
                        Ok(())
                    })(),
                    _ => Err("A list mixing points and numbers can't be graphed".into()),
                };
                if let Err(e) = compiled {
                    row.error = Some(e);
                    break;
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
        let left = self.lower_side(lhs, &mut problems);
        let right = self.lower_side(rhs, &mut problems);
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
        self.chains_row(id, std::slice::from_ref(c))
    }

    /// Where every chain in `conditions` holds, as [`Self::chain_row`].
    fn chains_row(&mut self, id: u64, conditions: &[Condition]) -> RowAnalysis {
        let mut problems = Problems::default();
        let mut gaps: Vec<Val> = Vec::new();
        let mut strict = true;
        for c in conditions {
            let Some(ascending) = direction(c) else {
                return RowAnalysis::error(
                    id,
                    "A chain of inequalities must all point the same way",
                );
            };
            strict &= c
                .relations
                .iter()
                .all(|r| matches!(r, Relation::Lt | Relation::Gt));
            let operands: Vec<Val> = c
                .operands
                .iter()
                .map(|e| self.lower_side(e, &mut problems))
                .collect();
            for pair in operands.windows(2) {
                let (lo, hi) = if ascending { (0, 1) } else { (1, 0) };
                gaps.push(self.broadcast(
                    &[pair[lo].clone(), pair[hi].clone()],
                    &mut problems,
                    |items| match (items[0], items[1]) {
                        (Item::Num(a), Item::Num(b)) => {
                            Ok(Item::Num(bin(BinOp::Sub, a.clone(), b.clone())))
                        }
                        _ => Err("Points can't be compared".into()),
                    },
                ));
            }
        }
        let largest = self.broadcast(&gaps, &mut problems, |items| {
            let mut args = Vec::new();
            for item in items {
                match item {
                    Item::Num(l) => args.push(l.clone()),
                    _ => return Err("Points can't be compared".into()),
                }
            }
            // max skips undefined values, but a restriction on any operand
            // must leave the whole chain undefined there, so add 0 times
            // each gap.
            let undefined_where_any_is = args.iter().fold(Low::Const(0.0), |acc, a| {
                bin(BinOp::Add, acc, bin(BinOp::Mul, Low::Const(0.0), a.clone()))
            });
            Ok(Item::Num(shallow(bin(
                BinOp::Add,
                Low::Call(Builtin::Max, args),
                undefined_where_any_is,
            ))))
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
    /// An inequality or chain in a 3D graph: a see-through solid, or
    /// exact boxes when every comparison bounds x, y or z by constants
    /// (as a 3D Riemann sum does). Restrictions such as
    /// `\{0 \le x \le 1\}` are part of the solid.
    fn solid_row(&mut self, id: u64, condition: &Condition) -> RowAnalysis {
        let mut conditions = Vec::new();
        let main = Condition {
            operands: condition
                .operands
                .iter()
                .map(|e| peel_restrictions(e, &mut conditions))
                .collect(),
            relations: condition.relations.clone(),
        };
        conditions.insert(0, main);
        if let Some(row) = self.box_row(id, &conditions) {
            return row;
        }
        solids(self.chains_row(id, &conditions))
    }

    /// Boxes, when each condition is `a ≤ v ≤ b` (or one side of it) for v
    /// one of x, y and z, each at most once, and a and b don't depend on
    /// where in space. A list among the bounds makes one box per element.
    fn box_row(&mut self, id: u64, conditions: &[Condition]) -> Option<RowAnalysis> {
        const SPATIAL: [&str; 7] = ["x", "y", "z", "r", "theta", "rho", "phi"];
        let mut bounds: [[Option<Val>; 2]; 3] = Default::default();
        let mut problems = Problems::default();
        for c in conditions {
            let ascending = direction(c)?;
            if c.operands.len() > 3 {
                return None;
            }
            let axes: Vec<(usize, usize)> = c
                .operands
                .iter()
                .enumerate()
                .filter_map(|(i, e)| {
                    ["x", "y", "z"]
                        .iter()
                        .position(|v| is_var(e, v))
                        .map(|a| (i, a))
                })
                .collect();
            let &[(at, axis)] = axes.as_slice() else {
                return None;
            };
            if bounds[axis].iter().any(Option::is_some) {
                return None;
            }
            for (i, e) in c.operands.iter().enumerate() {
                if i == at {
                    continue;
                }
                if SPATIAL.iter().any(|v| e.mentions(v)) {
                    return None;
                }
                let below = (i < at) == ascending;
                let side = if below { 0 } else { 1 };
                if bounds[axis][side].is_some() {
                    return None;
                }
                bounds[axis][side] = Some(self.lower_side(e, &mut problems));
            }
        }
        if !problems.errors.is_empty() || !problems.missing.is_empty() {
            return None;
        }
        let vals: Vec<&Val> = bounds.iter().flatten().flatten().collect();
        let n = vals
            .iter()
            .filter(|v| v.list)
            .map(|v| v.items.len())
            .min()
            .unwrap_or(1);
        let constant = |v: &Val, k: usize| match v.item(k) {
            Item::Num(low) => match low.clone().fold() {
                Low::Const(c) => Some(c),
                _ => None,
            },
            _ => None,
        };
        let mut row = RowAnalysis::new(id, RowKind::Curve);
        for k in 0..n {
            let mut b = [[f64::NEG_INFINITY, f64::INFINITY]; 3];
            for (axis, ends) in bounds.iter().enumerate() {
                for (side, end) in ends.iter().enumerate() {
                    if let Some(v) = end {
                        b[axis][side] = constant(v, k)?;
                    }
                }
            }
            if b.iter().all(|[lo, hi]| lo <= hi) {
                row.boxes3.push(b);
            }
        }
        Some(row)
    }

    /// One side of an inequality: in the plane, or in space in a 3D graph.
    fn lower_side(&mut self, e: &Expr, problems: &mut Problems) -> Val {
        if self.mode == Mode::ThreeD {
            self.lower_space(e, problems)
        } else {
            self.lower_plane(e, problems)
        }
    }

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

    /// A table: each column after the first is drawn as points against
    /// the first, and columns headed by a name define it as a list.
    fn table_row(&mut self, id: u64, table: &Table) -> RowAnalysis {
        let mut row = RowAnalysis::new(id, RowKind::Table);
        let mut columns: Vec<ColumnAnalysis> = Vec::new();
        let mut values: Vec<Option<Vec<f64>>> = Vec::new();
        for (i, column) in table.columns.iter().enumerate() {
            let mut out = ColumnAnalysis {
                cell_errors: column.cell_errors.clone(),
                ..ColumnAnalysis::default()
            };
            let mut problems = Problems::default();
            let numbers = match &column.header {
                Header::Blank => {
                    if column.filled {
                        out.error = Some("Give this column a header, like y_1".into());
                    }
                    None
                }
                Header::Bad(e) => {
                    out.error = Some(e.clone());
                    None
                }
                Header::Defines(name) => match self.value_of(name, &mut HashSet::new()) {
                    Ok(v) => numbers_of(&v, table.rows, &mut out.error),
                    Err(e) => {
                        out.error = Some(e);
                        out.missing = self.missing_in(&Expr::List(column.cells.clone()));
                        None
                    }
                },
                Header::Plain(_) => {
                    let val = self.lower(
                        &Expr::List(column.cells.clone()),
                        &HashMap::new(),
                        NO_FREE,
                        &mut problems,
                    );
                    self.column_numbers(&val, table.rows, &problems, &mut out)
                }
                Header::Formula(e) => {
                    // The formula reads the first column's name as its list,
                    // even when that name is x.
                    let mut bindings = HashMap::new();
                    if let (Some(first), Some(Some(xs))) = (table.columns.first(), values.first()) {
                        if let Header::Defines(n) | Header::Plain(n) = &first.header {
                            bindings
                                .insert(n.clone(), Val::from_value(&Value::Numbers(xs.clone())));
                        }
                    }
                    let val = self.lower(e, &bindings, NO_FREE, &mut problems);
                    let numbers = self.column_numbers(&val, table.rows, &problems, &mut out);
                    out.computed = numbers.clone();
                    numbers
                }
            };
            if i > 0 {
                if let (Some(Some(xs)), Some(ys)) = (values.first(), &numbers) {
                    out.points = xs
                        .iter()
                        .zip(ys)
                        .map(|(x, y)| [*x, *y])
                        .filter(|p| p[0].is_finite() && p[1].is_finite())
                        .collect();
                }
            }
            for m in &out.missing {
                if !row.missing.contains(m) {
                    row.missing.push(m.clone());
                }
            }
            values.push(numbers);
            columns.push(out);
        }
        row.columns = columns;
        row
    }

    /// A column's numbers from its lowered value, padded with NaN to the
    /// table's length, or None with the reason recorded.
    fn column_numbers(
        &self,
        val: &Val,
        rows: usize,
        problems: &Problems,
        out: &mut ColumnAnalysis,
    ) -> Option<Vec<f64>> {
        out.missing = slider_names(&problems.missing);
        if let Some(e) = problems.first_message() {
            out.error = Some(e);
            return None;
        }
        match val.constant() {
            Some(v) => numbers_of(&v, rows, &mut out.error),
            None => {
                out.error = Some("Variables can't depend on x or y".into());
                None
            }
        }
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
                row.points3 = points3_of(v);
                row.polygons = polygons_of(v);
                row.polygons3 = polygons3_of(v);
                if !row.points.is_empty()
                    || !row.points3.is_empty()
                    || !row.polygons.is_empty()
                    || !row.polygons3.is_empty()
                    || matches!(v, Value::Point(..) | Value::Point3(_))
                {
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
        self.value_of_ref(name, visiting)?;
        self.values[name].clone()
    }

    /// Makes sure the variable's value is worked out and remembered.
    fn value_of_ref(&mut self, name: &str, visiting: &mut HashSet<String>) -> Result<(), String> {
        if let Some(v) = self.values.get(name) {
            return v.as_ref().map(|_| ()).map_err(Clone::clone);
        }
        if self.actions.contains(name) {
            return Err(format!("'{name}' is an action; it can only be run"));
        }
        if !visiting.insert(name.to_owned()) {
            return Err(format!("'{name}' is defined in terms of itself"));
        }
        let expr = self
            .vars
            .get(name)
            .cloned()
            .expect("caller checked the variable exists");
        // Reuse the last analysis's value if the definition and everything
        // it read are unchanged.
        let reused = match self.old.vars.remove(name) {
            Some(c) if c.expr == expr && self.still_valid(&c.deps, visiting) => Some(c),
            _ => None,
        };
        let (result, deps) = match reused {
            Some(c) => (c.value, c.deps),
            None => {
                self.recording.push(Vec::new());
                let mut problems = Problems::default();
                let val =
                    self.lower_with(&expr, &HashMap::new(), NO_FREE, 0, &mut problems, visiting);
                let reads = self.recording.pop().unwrap_or_default();
                let deps = self.deps_of(&reads);
                let result = match (problems.first_message(), val.constant()) {
                    (Some(e), _) => Err(e),
                    (None, Some(v)) => Ok(v),
                    (None, None) => Err("Variables can't depend on x or y".into()),
                };
                (result, deps)
            }
        };
        visiting.remove(name);
        self.new.vars.insert(
            name.to_owned(),
            Cached {
                expr,
                deps,
                value: result.clone(),
            },
        );
        let out = result.as_ref().map(|_| ()).map_err(Clone::clone);
        self.values.insert(name.to_owned(), result);
        out
    }

    /// The reads of a value just worked out, with the values they saw, to
    /// remember it by.
    fn deps_of(&mut self, reads: &[Read]) -> Vec<Dep> {
        reads
            .iter()
            .map(|r| match r {
                Read::Var(n) => {
                    if !self.snapshots.contains_key(n) {
                        let v = self.values.get(n).cloned();
                        let v = Rc::new(v.unwrap_or_else(|| Err(String::new())));
                        self.snapshots.insert(n.clone(), v);
                    }
                    Dep::Var(n.clone(), Rc::clone(&self.snapshots[n]))
                }
                Read::Func(n, f) => Dep::Func(n.clone(), Rc::clone(f)),
                Read::Missing(n) => Dep::Missing(n.clone()),
            })
            .collect()
    }

    /// Whether everything a remembered value read is as it was.
    fn still_valid(&mut self, deps: &[Dep], visiting: &mut HashSet<String>) -> bool {
        deps.iter().all(|d| match d {
            Dep::Var(n, v) => {
                // Everything remembered saw the same value of each variable
                // (the last analysis's), so each is compared only once.
                if let Some(&same) = self.checked.get(n) {
                    return same;
                }
                if !self.vars.contains_key(n) {
                    return false;
                }
                match self.value_of_ref(n, visiting) {
                    Ok(()) => {
                        let same = same_value(&self.values[n], v);
                        self.checked.insert(n.clone(), same);
                        same
                    }
                    Err(e) => matches!(&**v, Err(old) if *old == e),
                }
            }
            Dep::Func(n, f) => self
                .funcs
                .get(n)
                .is_some_and(|g| Rc::ptr_eq(g, f) || g == f),
            Dep::Missing(n) => !self.vars.contains_key(n) && !self.funcs.contains_key(n),
        })
    }

    /// Notes that the value being worked out read `dep`.
    fn record(&mut self, read: Read) {
        if let Some(frame) = self.recording.last_mut() {
            if !frame.contains(&read) {
                frame.push(read);
            }
        }
    }

    /// Notes that the value being worked out read all of `reads`.
    fn record_all(&mut self, reads: &[Read]) {
        record_into(&mut self.recording, reads);
    }

    /// Works out `e` straight to a number when everything it uses is a
    /// plain constant number: sums and function calls over numbers, the
    /// common case in heavy graphs such as Fourier drawings. None means some
    /// part needs the general path (a list, a point, x or y, an error), which
    /// then lowers `e` as usual and gives the same result, only slower.
    /// `scope` holds the names bound here, None for any that is not a
    /// constant number.
    fn quick(
        &mut self,
        e: &Expr,
        scope: &[(&str, Option<Scalar>)],
        free: Free,
        depth: usize,
        visiting: &mut HashSet<String>,
    ) -> Option<Scalar> {
        macro_rules! go {
            ($e:expr) => {
                self.quick($e, scope, free, depth, visiting)?
            };
        }
        let real = |(a, b): Scalar| (b == 0.0).then_some(a);
        Some(match e {
            Expr::Num(v) => (*v, 0.0),
            Expr::Var(name) => {
                if let Some((_, v)) = scope.iter().rev().find(|(n, _)| n == name) {
                    return *v;
                }
                if free.iter().any(|(n, _)| n == name) {
                    return None;
                }
                if let Some((_, v)) = CONSTANTS.iter().find(|(c, _)| c == name) {
                    return Some((*v, 0.0));
                }
                if !self.values.contains_key(name) && self.vars.contains_key(name) {
                    self.value_of_ref(name, visiting).ok()?;
                }
                if let Some(v) = self.values.get(name) {
                    let v = match v.as_ref().ok()? {
                        Value::Number(n) => (*n, 0.0),
                        Value::Complex(a, b) => (*a, *b),
                        _ => return None,
                    };
                    self.note_var(name);
                    return Some(v);
                }
                if name == "i" && !self.vars.contains_key(name) && !self.funcs.contains_key(name) {
                    self.note_missing(name);
                    return Some((0.0, 1.0));
                }
                return None;
            }
            Expr::Neg(a) => match go!(a) {
                (a, 0.0) => (-a, 0.0),
                (a, b) => (-a, -b),
            },
            Expr::Add(a, b) => scalar_op(BinOp::Add, go!(a), go!(b))?,
            Expr::Sub(a, b) => scalar_op(BinOp::Sub, go!(a), go!(b))?,
            Expr::Mul(a, b) => scalar_op(BinOp::Mul, go!(a), go!(b))?,
            Expr::Div(a, b) => scalar_op(BinOp::Div, go!(a), go!(b))?,
            Expr::Pow(a, b) => scalar_op(BinOp::Pow, go!(a), go!(b))?,
            Expr::Factorial(a) => {
                let a = real(go!(a))?;
                scalar_item(&Item::Num(call1(Builtin::Factorial, Low::Const(a))))?
            }
            Expr::Call(f, args) => {
                if *f == Builtin::Length {
                    return None;
                }
                let mut parts = Vec::with_capacity(args.len());
                for a in args {
                    parts.push(go!(a));
                }
                scalar_call(*f, &parts)?
            }
            Expr::Index(base, index) => {
                let Expr::Var(name) = &**base else {
                    return None;
                };
                let global = !scope.iter().any(|(n, _)| n == name)
                    && !free.iter().any(|(n, _)| n == name)
                    && self.vars.contains_key(name);
                if !global {
                    return None;
                }
                let n = real(go!(index))?.round();
                self.value_of_ref(name, visiting).ok()?;
                self.note_var(name);
                let value = self.values.get(name)?.as_ref().ok()?;
                let len = match value {
                    Value::Numbers(v) => v.len(),
                    Value::Complexes(v) => v.len(),
                    _ => return None,
                };
                if n < 1.0 || n as usize > len {
                    return Some((f64::NAN, 0.0));
                }
                let k = n as usize - 1;
                match value {
                    Value::Numbers(v) => (v[k], 0.0),
                    Value::Complexes(v) => {
                        scalar_item(&Item::complex(Low::Const(v[k][0]), Low::Const(v[k][1])))?
                    }
                    _ => return None,
                }
            }
            Expr::Shape(
                f @ (ShapeFn::Real | ShapeFn::Imag | ShapeFn::Conj | ShapeFn::Arg),
                args,
            ) => {
                let [a] = &args[..] else {
                    return None;
                };
                let (a, b) = go!(a);
                match f {
                    ShapeFn::Real => (a, 0.0),
                    ShapeFn::Imag => (b, 0.0),
                    ShapeFn::Conj if b == 0.0 => (a, 0.0),
                    ShapeFn::Conj => (a, -b),
                    _ => scalar_item(&Item::Num(shallow(Low::Call(
                        Builtin::Arctan,
                        vec![Low::Const(b), Low::Const(a)],
                    ))))?,
                }
            }
            Expr::Apply(name, args) => {
                if scope.iter().any(|(n, _)| n == name) || depth >= MAX_INLINE_DEPTH {
                    return None;
                }
                let f = Rc::clone(self.funcs.get(name)?);
                if f.params.len() != args.len() {
                    return None;
                }
                self.note_func(name, &f);
                let mut inner = Vec::with_capacity(args.len());
                // The key Env::lower_with gives these arguments, so results
                // are shared with it.
                let mut key = Vec::with_capacity(3 * args.len());
                for (p, a) in f.params.iter().zip(args) {
                    let (re, im) = go!(a);
                    key.push(0);
                    if im == 0.0 {
                        key.extend([0, re.to_bits()]);
                    } else {
                        key.extend([2, re.to_bits(), im.to_bits()]);
                    }
                    inner.push((p.as_str(), Some((re, im))));
                }
                let key = (name.clone(), key);
                if let Some((v, used)) = self.remembered_call([Some(&key), None], free, visiting) {
                    if !used.is_empty() {
                        return None;
                    }
                    return scalar_of(&v);
                }
                // Inside the body only its parameters are in scope.
                self.recording.push(Vec::new());
                self.note_func(name, &f);
                let result = self.quick(&f.body, &inner, free, depth + 1, visiting);
                let reads = self.recording.pop().unwrap_or_default();
                self.record_all(&reads);
                let (re, im) = result?;
                let v = Val {
                    items: vec![Item::complex(Low::Const(re), Low::Const(im))],
                    list: false,
                };
                self.remember_call(key, v, Vec::new(), reads);
                (re, im)
            }
            Expr::Repeat(kind, var, lo, hi, body) => {
                let bound = |v: Scalar| real(v).filter(|n| n.is_finite()).map(f64::round);
                let a = bound(go!(lo))?;
                let b = bound(go!(hi))?;
                let (op, empty) = match kind {
                    Repeat::Sum => (BinOp::Add, 0.0),
                    Repeat::Product => (BinOp::Mul, 1.0),
                };
                if b < a {
                    return Some((empty, 0.0));
                }
                if b - a >= MAX_LIST as f64 {
                    return None;
                }
                // A long sum is compiled once rather than read for every term.
                if b - a >= FAST_SUM_TERMS {
                    if let Some(v) =
                        self.fast_repeat(*kind, var, a, b, body, scope, free, depth, visiting)
                    {
                        return Some(v);
                    }
                }
                let mut inner = scope.to_vec();
                inner.push((var.as_str(), None));
                let last = inner.len() - 1;
                let mut total: Option<Scalar> = None;
                let mut k = a;
                while k <= b {
                    inner[last].1 = Some((k, 0.0));
                    let term = self.quick(body, &inner, free, depth, visiting)?;
                    total = Some(match total {
                        None => term,
                        Some(acc) => scalar_op(op, acc, term)?,
                    });
                    k += 1.0;
                }
                total.unwrap_or((empty, 0.0))
            }
            _ => return None,
        })
    }

    /// `\sum_{var=a}^{b} body` (or a product) through a compiled program,
    /// with what it reads recorded as `quick` would. None when the body
    /// holds something the program cannot do, or a term fails; the caller
    /// then works the sum out term by term.
    #[allow(clippy::too_many_arguments)]
    fn fast_repeat(
        &mut self,
        kind: Repeat,
        var: &str,
        a: f64,
        b: f64,
        body: &Expr,
        scope: &[(&str, Option<Scalar>)],
        free: Free,
        depth: usize,
        visiting: &mut HashSet<String>,
    ) -> Option<Scalar> {
        let mut names: Vec<(String, Bind)> = scope
            .iter()
            .map(|(n, v)| (n.to_string(), Bind::Value(*v)))
            .collect();
        names.push((var.to_owned(), Bind::Slot(0)));
        let mut c = FastCompile {
            prog: fastsum::Program {
                slots: 1,
                ..Default::default()
            },
            lists: Vec::new(),
            free,
        };
        let mut ops = Vec::new();
        self.compile_fast(body, &names, &mut ops, &mut c, depth, visiting)?;
        c.prog.ops = ops;
        c.prog.repeat(kind == Repeat::Product, 0, a, b)
    }

    /// Arithmetic on long lists of numbers, such as `(n/500 cos(an), ...)`
    /// for `n` in `[1...5000]`, worked out element by element by a compiled
    /// program rather than as one symbolic item per element. Gives the same
    /// numbers as the general path; None when the shortcut does not apply
    /// (no long list, points or complex numbers in play, or anything the
    /// compiler cannot express), and the general path takes over.
    fn fast_broadcast(
        &mut self,
        e: &Expr,
        free: Free,
        depth: usize,
        visiting: &mut HashSet<String>,
    ) -> Option<Val> {
        let parts: Vec<&Expr> = match e {
            Expr::Point(a, b) => vec![a, b],
            Expr::Neg(_)
            | Expr::Add(..)
            | Expr::Sub(..)
            | Expr::Mul(..)
            | Expr::Div(..)
            | Expr::Pow(..)
            | Expr::Call(..)
            | Expr::Apply(..) => vec![e],
            _ => return None,
        };
        // Functions of whole lists, such as total(n), would see one element.
        if self.reads_whole_lists(e, 0) {
            return None;
        }
        let mut names: Vec<String> = Vec::new();
        e.visit_names(&mut |n, applied| {
            if !applied && !names.iter().any(|m| m == n) {
                names.push(n.to_owned());
            }
        });
        // `i` brings in complex numbers, whose items the general path keeps.
        if names.iter().any(|n| n == "i" && !self.vars.contains_key(n)) {
            return None;
        }
        let mut binds: Vec<(String, Bind)> = Vec::new();
        let mut columns: Vec<Rc<[Scalar]>> = Vec::new();
        for name in names {
            if free.iter().any(|(n, _)| *n == name) {
                return None;
            }
            if !self.vars.contains_key(&name) {
                continue;
            }
            self.value_of_ref(&name, visiting).ok()?;
            let Value::Numbers(v) = self.values.get(&name)?.as_ref().ok()? else {
                continue;
            };
            let column: Rc<[Scalar]> = match self.fast_lists.get(&name) {
                Some(list) => Rc::clone(list),
                None => {
                    let list: Rc<[Scalar]> = v.iter().map(|x| (*x, 0.0)).collect();
                    self.fast_lists.insert(name.clone(), Rc::clone(&list));
                    list
                }
            };
            binds.push((name, Bind::Slot(u16::try_from(columns.len()).ok()?)));
            columns.push(column);
        }
        let len = columns.iter().map(|c| c.len()).min()?;
        if len < FAST_BROADCAST_LEN {
            return None;
        }
        let mut c = FastCompile {
            prog: fastsum::Program {
                slots: columns.len(),
                ..Default::default()
            },
            lists: Vec::new(),
            free,
        };
        let mut programs = Vec::new();
        for part in &parts {
            let mut ops = Vec::new();
            self.compile_fast(part, &binds, &mut ops, &mut c, depth, visiting)?;
            programs.push(ops);
        }
        for (name, _) in &binds {
            self.note_var(name);
        }
        let values = c.prog.broadcast(&programs, &columns, len)?;
        let real = |v: &Scalar| (v.1 == 0.0).then_some(Low::Const(v.0));
        let items = match &values[..] {
            [xs, ys] => xs
                .iter()
                .zip(ys)
                .map(|(x, y)| Some(Item::Point(real(x)?, real(y)?)))
                .collect::<Option<Vec<_>>>()?,
            [vs] => vs
                .iter()
                .map(|v| real(v).map(Item::Num))
                .collect::<Option<Vec<_>>>()?,
            _ => return None,
        };
        Some(Val::list(items))
    }

    /// Whether `e`, or a function it calls, uses a function of whole lists.
    fn reads_whole_lists(&self, e: &Expr, depth: usize) -> bool {
        if depth > MAX_INLINE_DEPTH {
            return true;
        }
        let mut found = false;
        let mut visit = |e: &Expr| match e {
            Expr::Call(f, _) => found |= f.takes_list(),
            Expr::Apply(name, _) | Expr::Prime(name, ..) => {
                found |= self
                    .funcs
                    .get(name)
                    .is_some_and(|f| self.reads_whole_lists(&f.body, depth + 1))
            }
            _ => {}
        };
        e.walk(&mut visit);
        found
    }

    /// Appends ops for `e` to `ops`, returning which locals it reads as a
    /// bit mask (bit 63 standing for every local past 62).
    fn compile_fast(
        &mut self,
        e: &Expr,
        names: &[(String, Bind)],
        ops: &mut Vec<fastsum::Op>,
        c: &mut FastCompile,
        depth: usize,
        visiting: &mut HashSet<String>,
    ) -> Option<u64> {
        use fastsum::Op;
        if ops.len() + c.prog.len() > fastsum::MAX_OPS {
            return None;
        }
        let bit = |s: u16| 1u64 << (s as u32).min(63);
        macro_rules! go {
            ($e:expr) => {
                self.compile_fast($e, names, ops, c, depth, visiting)?
            };
        }
        Some(match e {
            Expr::Num(v) => {
                ops.push(Op::Const((*v, 0.0)));
                0
            }
            Expr::Var(name) => {
                if let Some((_, bind)) = names.iter().rev().find(|(n, _)| n == name) {
                    return match bind {
                        Bind::Slot(s) => {
                            ops.push(Op::Slot(*s));
                            Some(bit(*s))
                        }
                        Bind::Value(v) => {
                            ops.push(Op::Const((*v)?));
                            Some(0)
                        }
                    };
                }
                let v = self.quick(e, &[], c.free, depth, visiting)?;
                ops.push(Op::Const(v));
                0
            }
            Expr::Neg(a) => {
                let m = go!(a);
                ops.push(Op::Neg);
                m
            }
            Expr::Add(a, b)
            | Expr::Sub(a, b)
            | Expr::Mul(a, b)
            | Expr::Div(a, b)
            | Expr::Pow(a, b) => {
                let m = go!(a) | go!(b);
                ops.push(Op::Bin(match e {
                    Expr::Add(..) => BinOp::Add,
                    Expr::Sub(..) => BinOp::Sub,
                    Expr::Mul(..) => BinOp::Mul,
                    Expr::Div(..) => BinOp::Div,
                    _ => BinOp::Pow,
                }));
                m
            }
            Expr::Call(f, args) => {
                if *f == Builtin::Length || args.is_empty() || args.len() > 255 {
                    return None;
                }
                let mut m = 0;
                for a in args {
                    m |= go!(a);
                }
                ops.push(Op::Call(*f, args.len() as u8));
                m
            }
            Expr::Shape(
                f @ (ShapeFn::Real | ShapeFn::Imag | ShapeFn::Conj | ShapeFn::Arg),
                args,
            ) => {
                let [a] = &args[..] else {
                    return None;
                };
                let m = go!(a);
                ops.push(match f {
                    ShapeFn::Real => Op::Real,
                    ShapeFn::Imag => Op::Imag,
                    ShapeFn::Conj => Op::Conj,
                    _ => Op::Arg,
                });
                m
            }
            Expr::Index(base, index) => {
                let Expr::Var(name) = &**base else {
                    return None;
                };
                let global = !names.iter().any(|(n, _)| n == name)
                    && !c.free.iter().any(|(n, _)| n == name)
                    && self.vars.contains_key(name);
                if !global {
                    return None;
                }
                let m = go!(index);
                let id = match c.lists.iter().position(|n| n == name) {
                    Some(id) => id,
                    None => {
                        self.value_of_ref(name, visiting).ok()?;
                        self.note_var(name);
                        let list = match self.fast_lists.get(name) {
                            Some(list) => Rc::clone(list),
                            None => {
                                let list: Rc<[Scalar]> =
                                    match self.values.get(name)?.as_ref().ok()? {
                                        Value::Numbers(v) => v.iter().map(|x| (*x, 0.0)).collect(),
                                        Value::Complexes(v) => v
                                            .iter()
                                            .map(|[re, im]| {
                                                scalar_item(&Item::complex(
                                                    Low::Const(*re),
                                                    Low::Const(*im),
                                                ))
                                            })
                                            .collect::<Option<_>>()?,
                                        _ => return None,
                                    };
                                self.fast_lists.insert(name.clone(), Rc::clone(&list));
                                list
                            }
                        };
                        c.lists.push(name.clone());
                        c.prog.lists.push(list);
                        c.lists.len() - 1
                    }
                };
                ops.push(Op::Index(u16::try_from(id).ok()?));
                m
            }
            Expr::Apply(name, args) => {
                if names.iter().any(|(n, _)| n == name) || depth >= MAX_INLINE_DEPTH {
                    return None;
                }
                let f = Rc::clone(self.funcs.get(name)?);
                if f.params.len() != args.len() {
                    return None;
                }
                let start = ops.len();
                let mut m = 0;
                for a in args {
                    m |= go!(a);
                }
                if m == 0 {
                    // Constant arguments: worked out (and remembered) once,
                    // as quick would.
                    ops.truncate(start);
                    let scope: Vec<(&str, Option<Scalar>)> = names
                        .iter()
                        .filter_map(|(n, b)| match b {
                            Bind::Value(v) => Some((n.as_str(), *v)),
                            Bind::Slot(_) => None,
                        })
                        .collect();
                    let v = self.quick(e, &scope, c.free, depth, visiting)?;
                    ops.push(Op::Const(v));
                    return Some(0);
                }
                self.note_func(name, &f);
                // Inside the body only its parameters are in scope.
                let first = u16::try_from(c.prog.slots).ok()?;
                c.prog.slots += f.params.len();
                let inner: Vec<(String, Bind)> = f
                    .params
                    .iter()
                    .enumerate()
                    .map(|(i, p)| (p.clone(), Bind::Slot(first + i as u16)))
                    .collect();
                for i in (0..f.params.len()).rev() {
                    ops.push(Op::Store(first + i as u16));
                }
                self.compile_fast(&f.body, &inner, ops, c, depth + 1, visiting)?;
                m
            }
            Expr::Repeat(kind, var, lo, hi, body) => {
                let m = go!(lo) | go!(hi);
                let slot = u16::try_from(c.prog.slots).ok()?;
                c.prog.slots += 1;
                let mut inner = names.to_vec();
                inner.push((var.clone(), Bind::Slot(slot)));
                let mut sub = Vec::new();
                let mb = self.compile_fast(body, &inner, &mut sub, c, depth, visiting)?;
                c.prog.subs.push(sub);
                ops.push(Op::Repeat {
                    product: *kind == Repeat::Product,
                    slot,
                    sub: u16::try_from(c.prog.subs.len() - 1).ok()?,
                });
                m | (mb & !bit(slot))
            }
            _ => return None,
        })
    }

    /// A function result already worked out, in this analysis or (if
    /// nothing it read has changed) the last, under the first of `keys` that
    /// has one: with what it read recorded, and the graph variables it used.
    /// `keys` are the key for any scope, which serves only if the result
    /// read none of the graph variables in `free`, and the key for `free`.
    fn remembered_call(
        &mut self,
        keys: [Option<&CallKey>; 2],
        free: Free,
        visiting: &mut HashSet<String>,
    ) -> Option<(Val, Vec<&'static str>)> {
        for k in keys.into_iter().flatten() {
            if self.calls.contains_key(k) {
                continue;
            }
            let Some(c) = self.old.calls.remove(k) else {
                continue;
            };
            if self.still_valid(&c.deps, visiting) {
                let kept = (c.val.clone(), c.used.clone(), c.reads.clone());
                self.calls.insert(k.clone(), kept);
                self.new.calls.insert(k.clone(), c);
            }
        }
        let reads_free = |reads: &[Read]| {
            reads.iter().any(|d| match d {
                Read::Var(n) | Read::Missing(n) => free.iter().any(|(f, _)| f == n),
                Read::Func(..) => false,
            })
        };
        let [any, key] = keys;
        let (v, used, reads) = any
            .and_then(|k| self.calls.get(k))
            .filter(|(_, _, reads)| !reads_free(reads))
            .or_else(|| key.and_then(|k| self.calls.get(k)))?;
        record_into(&mut self.recording, reads);
        Some((v.clone(), used.clone()))
    }

    /// Keeps a function result for the rest of this analysis, and when it
    /// is constant, for the next.
    fn remember_call(&mut self, k: CallKey, v: Val, used: Vec<&'static str>, reads: Vec<Read>) {
        // Anything not constant may hold this analysis's numbered holes.
        if v.constant().is_some() {
            let call = CachedCall {
                val: v.clone(),
                used: used.clone(),
                deps: self.deps_of(&reads),
                reads: reads.clone(),
            };
            self.new.calls.insert(k.clone(), call);
        }
        self.calls.insert(k, (v, used, reads));
    }

    /// Records a read of variable `name`, without allocating when it was
    /// already recorded.
    fn note_var(&mut self, name: &str) {
        if let Some(frame) = self.recording.last_mut() {
            if !frame.iter().any(|r| matches!(r, Read::Var(n) if n == name)) {
                frame.push(Read::Var(name.to_owned()));
            }
        }
    }

    fn note_missing(&mut self, name: &str) {
        if let Some(frame) = self.recording.last_mut() {
            if !frame
                .iter()
                .any(|r| matches!(r, Read::Missing(n) if n == name))
            {
                frame.push(Read::Missing(name.to_owned()));
            }
        }
    }

    fn note_func(&mut self, name: &str, f: &Rc<FuncDef>) {
        if let Some(frame) = self.recording.last_mut() {
            let seen = frame
                .iter()
                .any(|r| matches!(r, Read::Func(n, g) if n == name && Rc::ptr_eq(g, f)));
            if !seen {
                frame.push(Read::Func(name.to_owned(), Rc::clone(f)));
            }
        }
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
            let polygon = |i: &Item| matches!(i, Polygon(_) | Polygon3(_));
            if polygon(items[0]) || polygon(items[1]) {
                return Err("Polygons can't be used in arithmetic".into());
            }
            if matches!(items[0], Complex(..)) || matches!(items[1], Complex(..)) {
                return match (items[0].parts(), items[1].parts()) {
                    (Some(z), Some(w)) => complex_op(op, z, w),
                    _ => Err("Points and complex numbers can't be combined".into()),
                };
            }
            Ok(match (op, items[0], items[1]) {
                (_, Num(a), Num(b)) => Num(op2(a, b)),
                (BinOp::Add | BinOp::Sub, Point(a, b), Point(c, d)) => Point(op2(a, c), op2(b, d)),
                (BinOp::Mul, Point(a, b), Num(k)) | (BinOp::Div, Point(a, b), Num(k)) => {
                    Point(op2(a, k), op2(b, k))
                }
                (BinOp::Mul, Num(k), Point(a, b)) => Point(op2(k, a), op2(k, b)),
                (BinOp::Add | BinOp::Sub, Point3(a, b, c), Point3(d, e, f)) => {
                    Point3(op2(a, d), op2(b, e), op2(c, f))
                }
                (BinOp::Mul | BinOp::Div, Point3(a, b, c), Num(k)) => {
                    Point3(op2(a, k), op2(b, k), op2(c, k))
                }
                (BinOp::Mul, Num(k), Point3(a, b, c)) => Point3(op2(k, a), op2(k, b), op2(k, c)),
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
        if bindings.is_empty() {
            if let Some(v) = self.fast_broadcast(e, free, depth, visiting) {
                return v;
            }
        }
        match e {
            Expr::Num(v) => Val::num(Low::Const(*v)),
            Expr::Var(name) => self.resolve_name(name, bindings, free, problems, visiting),
            Expr::Neg(a) => {
                let a = go!(a);
                self.broadcast(&[a], problems, |items| {
                    let neg = |l: &Low| shallow(Low::Neg(Box::new(l.clone())));
                    Ok(match items[0] {
                        Item::Num(a) => Item::Num(neg(a)),
                        Item::Point(a, b) => Item::Point(neg(a), neg(b)),
                        Item::Point3(a, b, c) => Item::Point3(neg(a), neg(b), neg(c)),
                        Item::Polygon(_) | Item::Polygon3(_) => {
                            return Err("Polygons can't be negated".into())
                        }
                        Item::Complex(a, b) => Item::Complex(neg(a), neg(b)),
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
            Expr::Point3(a, b, c) => {
                let (a, b, c) = (go!(a), go!(b), go!(c));
                self.broadcast(&[a, b, c], problems, |items| {
                    match (items[0], items[1], items[2]) {
                        (Item::Num(a), Item::Num(b), Item::Num(c)) => {
                            Ok(Item::Point3(a.clone(), b.clone(), c.clone()))
                        }
                        _ => Err("A point's coordinates must be numbers".into()),
                    }
                })
            }
            Expr::Coord(a, coord) => {
                let a = go!(a);
                self.broadcast(&[a], problems, |items| match (items[0], coord) {
                    (Item::Point(x, _) | Item::Point3(x, _, _), Coord::X) => {
                        Ok(Item::Num(x.clone()))
                    }
                    (Item::Point(_, y) | Item::Point3(_, y, _), Coord::Y) => {
                        Ok(Item::Num(y.clone()))
                    }
                    (Item::Point3(_, _, z), Coord::Z) => Ok(Item::Num(z.clone())),
                    (Item::Point(..), Coord::Z) => Err("Only 3D points have .z".into()),
                    _ => Err("Only points have .x and .y".into()),
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
                // `L[n]` with L a variable reads the one element straight from
                // L's remembered value, without copying the whole list.
                if let Expr::Var(name) = &**base {
                    let global = !bindings.contains_key(name)
                        && !free.iter().any(|(n, _)| n == name)
                        && self.vars.contains_key(name);
                    if global {
                        let index_val = go!(index);
                        if let Some(v) = self.element_of(name, &index_val, problems, visiting) {
                            return v;
                        }
                        let base = self.resolve_name(name, bindings, free, problems, visiting);
                        return self.index(base, index_val, problems);
                    }
                }
                let base = go!(base);
                let index = go!(index);
                self.index(base, index, problems)
            }
            Expr::Filter(base, mask) => {
                let base = go!(base);
                let mask = go!(mask);
                self.filter(base, mask, problems)
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
                            shallow(Low::Select(
                                Box::new(c.clone()),
                                Box::new(a.clone()),
                                Box::new(b.clone()),
                            ))
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
                            (
                                Item::Num(c),
                                a @ (Item::Num(_) | Item::Complex(..)),
                                b @ (Item::Num(_) | Item::Complex(..)),
                            ) => {
                                let ((ar, ai), (br, bi)) =
                                    (a.parts().expect("number"), b.parts().expect("number"));
                                Item::complex(sel(c, &ar, &br), sel(c, &ai, &bi))
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
            Expr::Derivative(var, body) => {
                // Differentiate with the variable as a placeholder, then
                // put the variable's own value back in.
                let hole = self.hole();
                let mut inner = bindings.clone();
                inner.insert(var.clone(), Val::num(Low::Hole(hole)));
                let body = self.lower_with(body, &inner, free, depth, problems, visiting);
                let at = self.resolve_name(var, bindings, free, problems, visiting);
                self.broadcast(&[body, at], problems, |items| {
                    let slope = |l: &Low, a: &Low| derivative(l, &Low::Hole(hole)).fill(hole, a);
                    match (items[0], items[1]) {
                        (Item::Num(l), Item::Num(a)) => Ok(Item::Num(slope(l, a))),
                        (Item::Point(p, q), Item::Num(a)) => {
                            Ok(Item::Point(slope(p, a), slope(q, a)))
                        }
                        _ => Err("Can't differentiate with respect to a point".into()),
                    }
                })
            }
            Expr::Prime(name, order, args) => {
                let Some(f) = self
                    .funcs
                    .get(name)
                    .filter(|_| !bindings.contains_key(name))
                    .cloned()
                else {
                    problems.error(format!(
                        "'{name}' is not a function, so {name}' has no meaning"
                    ));
                    return Val::nan();
                };
                self.record(Read::Func(name.clone(), Rc::clone(&f)));
                if f.params.len() != 1 || args.len() != 1 {
                    problems.error(format!("{name}' needs a function of one variable"));
                    return Val::nan();
                }
                if depth >= MAX_INLINE_DEPTH {
                    problems.error(format!("'{name}' is defined in terms of itself"));
                    return Val::nan();
                }
                let (param, body) = (f.params[0].clone(), f.body.clone());
                let hole = self.hole();
                let inner: HashMap<String, Val> =
                    [(param, Val::num(Low::Hole(hole)))].into_iter().collect();
                let body = self.lower_with(&body, &inner, free, depth + 1, problems, visiting);
                let at = go!(&args[0]);
                let order = *order;
                self.broadcast(&[body, at], problems, |items| {
                    let slope = |l: &Low, a: &Low| {
                        let mut d = l.clone();
                        for _ in 0..order {
                            d = derivative(&d, &Low::Hole(hole));
                        }
                        d.fill(hole, a)
                    };
                    match (items[0], items[1]) {
                        (Item::Num(l), Item::Num(a)) => Ok(Item::Num(slope(l, a))),
                        (Item::Point(p, q), Item::Num(a)) => {
                            Ok(Item::Point(slope(p, a), slope(q, a)))
                        }
                        _ => Err(format!("{name}' takes a number")),
                    }
                })
            }
            Expr::Repeat(kind, var, lo, hi, body) => {
                let scope: Vec<(&str, Option<Scalar>)> = bindings
                    .iter()
                    .map(|(n, v)| (n.as_str(), scalar_of(v)))
                    .collect();
                if let Some((re, im)) = self.quick(e, &scope, free, depth, visiting) {
                    return Val {
                        items: vec![Item::complex(Low::Const(re), Low::Const(im))],
                        list: false,
                    };
                }
                let what = if *kind == Repeat::Sum {
                    "sum"
                } else {
                    "product"
                };
                let mut bound = |e: &Expr, problems: &mut Problems| -> Option<f64> {
                    let v = self.lower_with(e, bindings, free, depth, problems, visiting);
                    match v.constant() {
                        Some(Value::Number(n)) if n.is_finite() => Some(n.round()),
                        _ => None,
                    }
                };
                let (Some(a), Some(b)) = (bound(lo, problems), bound(hi, problems)) else {
                    problems.error(format!("A {what}'s bounds must be finite numbers"));
                    return Val::nan();
                };
                let (op, empty) = match kind {
                    Repeat::Sum => (BinOp::Add, 0.0),
                    Repeat::Product => (BinOp::Mul, 1.0),
                };
                if b < a {
                    return Val::num(Low::Const(empty));
                }
                if b - a >= MAX_LIST as f64 {
                    problems.error(format!("A {what} can have at most 10,000 terms"));
                    return Val::nan();
                }
                let mut total: Option<Val> = None;
                let mut k = a;
                while k <= b {
                    let mut inner = bindings.clone();
                    inner.insert(var.clone(), Val::num(Low::Const(k)));
                    let term = self.lower_with(body, &inner, free, depth, problems, visiting);
                    total = Some(match total {
                        None => term,
                        Some(acc) => self.arithmetic(op, acc, term, problems),
                    });
                    k += 1.0;
                }
                total.unwrap_or_else(|| Val::num(Low::Const(empty)))
            }
            Expr::Integral(var, lo, hi, body) => {
                let lo = go!(lo);
                let hi = go!(hi);
                let hole = self.hole();
                let mut inner = bindings.clone();
                inner.insert(var.clone(), Val::num(Low::Hole(hole)));
                let body = self.lower_with(body, &inner, free, depth, problems, visiting);
                self.broadcast(&[lo, hi, body], problems, |items| {
                    match (items[0], items[1], items[2]) {
                        (Item::Num(a), Item::Num(b), Item::Num(f)) => {
                            Ok(Item::Num(Low::Integral {
                                hole,
                                body: Box::new(f.clone()),
                                lo: Box::new(a.clone()),
                                hi: Box::new(b.clone()),
                            }))
                        }
                        _ => Err("Integrals of points aren't supported".into()),
                    }
                })
            }
            Expr::Shape(f, args) => {
                let args: Vec<Val> = args.iter().map(|a| go!(a)).collect();
                self.shape(*f, args, problems)
            }
            Expr::Update(..) | Expr::Actions(_) => {
                problems.error("An action can only be run, not used in a calculation");
                Val::nan()
            }
            Expr::Apply(name, args) => {
                let is_function = self.funcs.contains_key(name) && !bindings.contains_key(name);
                if is_function {
                    let f = Rc::clone(&self.funcs[name]);
                    self.record(Read::Func(name.clone(), Rc::clone(&f)));
                    let (params, body) = (&f.params, &f.body);
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
                    let mut key = Some(Vec::new());
                    for (p, a) in params.iter().zip(args) {
                        let value = go!(a);
                        if let Some(k) = key.as_mut() {
                            if constant_key(&value, k).is_none() {
                                key = None;
                            }
                        }
                        lowered.insert(p.clone(), value);
                    }
                    // With constant arguments the result depends only on
                    // which graph variables are in scope, so it is kept. A
                    // result that read none of them is kept for every scope
                    // (under `any`), as long as no name it read is one of
                    // the graph variables where it is used again.
                    let any = key.clone().map(|k| (name.clone(), k));
                    let key = key.map(|mut k| {
                        k.push(u64::MAX);
                        k.extend(
                            free.iter()
                                .flat_map(|(n, _)| n.bytes().map(u64::from).chain([0])),
                        );
                        (name.clone(), k)
                    });
                    if let Some((v, used)) =
                        self.remembered_call([any.as_ref(), key.as_ref()], free, visiting)
                    {
                        problems.used.extend(used);
                        return v;
                    }
                    let before = (problems.errors.len(), problems.missing.len());
                    let used_before = problems.used.len();
                    self.recording.push(Vec::new());
                    // The result is remembered by what it read, its own
                    // definition included.
                    self.record(Read::Func(name.clone(), Rc::clone(&f)));
                    // Inside the function body only its parameters (and the
                    // caller's graph variables) are in scope.
                    let v = self.lower_with(body, &lowered, free, depth + 1, problems, visiting);
                    let deps = self.recording.pop().unwrap_or_default();
                    self.record_all(&deps);
                    if let (Some(k), Some(any)) = (key, any) {
                        if (problems.errors.len(), problems.missing.len()) == before {
                            let used = problems.used[used_before..].to_vec();
                            let k = if used.is_empty() { any } else { k };
                            self.remember_call(k, v.clone(), used, deps);
                        }
                    }
                    v
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

    /// `base[index]`: elements of a list, or a product when `base` is not
    /// one.
    fn index(&self, base: Val, index: Val, problems: &mut Problems) -> Val {
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

    /// The elements of `base` where `mask`, a condition worth 1 where it
    /// holds, is defined. A single condition keeps all or nothing; lists of
    /// different lengths stop at the shorter, as in Desmos.
    fn filter(&self, base: Val, mask: Val, problems: &mut Problems) -> Val {
        if !base.list {
            problems.error("Only lists can be filtered, like L[L > 0]");
            return Val::nan();
        }
        let n = if mask.list {
            base.items.len().min(mask.items.len())
        } else {
            base.items.len()
        };
        let mut out = Vec::new();
        for (i, item) in base.items.iter().take(n).enumerate() {
            let m = &mask.items[if mask.list { i } else { 0 }];
            let Item::Num(low) = m else {
                problems.error("A filter needs a condition, like L[L > 0]");
                return Val::nan();
            };
            match low.clone().fold() {
                Low::Const(v) if v.is_nan() => {}
                Low::Const(_) => out.push(item.clone()),
                _ => {
                    problems.error("Filters can't depend on x or y yet");
                    return Val::nan();
                }
            }
        }
        Val::list(out)
    }

    /// The elements of variable `name` at constant `index` positions, read
    /// without copying the variable's whole value. None when that shortcut
    /// does not apply.
    fn element_of(
        &mut self,
        name: &str,
        index: &Val,
        problems: &mut Problems,
        visiting: &mut HashSet<String>,
    ) -> Option<Val> {
        let positions = index
            .items
            .iter()
            .map(|i| match i {
                Item::Num(Low::Const(n)) => Some(n.round()),
                _ => None,
            })
            .collect::<Option<Vec<f64>>>()?;
        let read = self.value_of_ref(name, visiting);
        self.record(Read::Var(name.to_owned()));
        if let Err(e) = read {
            problems.error(e);
            return Some(Val::nan());
        }
        let value = self.values.get(name)?.as_ref().ok()?;
        let c = |v: f64| Low::Const(v);
        let len = match value {
            Value::Numbers(v) => v.len(),
            Value::Points(v) => v.len(),
            Value::Complexes(v) => v.len(),
            Value::Polygons(v) => v.len(),
            Value::Polygons3(v) => v.len(),
            _ => return None,
        };
        let items = positions
            .iter()
            .map(|&n| {
                if n < 1.0 || n as usize > len {
                    return Item::Num(c(f64::NAN));
                }
                let k = n as usize - 1;
                match value {
                    Value::Numbers(v) => Item::Num(c(v[k])),
                    Value::Points(v) => Item::Point(c(v[k][0]), c(v[k][1])),
                    Value::Complexes(v) => Item::complex(c(v[k][0]), c(v[k][1])),
                    Value::Polygons(v) => Item::Polygon(v[k].clone()),
                    Value::Polygons3(v) => Item::Polygon3(v[k].clone()),
                    _ => unreachable!("checked above"),
                }
            })
            .collect();
        Some(Val {
            items,
            list: index.list,
        })
    }

    /// `polygon`, `join`, `sort` and the other functions of whole lists
    /// and points.
    fn shape(&self, f: ShapeFn, args: Vec<Val>, problems: &mut Problems) -> Val {
        let constant = |l: &Low| match l.clone().fold() {
            Low::Const(v) => Some(v),
            _ => None,
        };
        let point_of = |item: &Item| match item {
            Item::Point(a, b) => Some([constant(a)?, constant(b)?]),
            _ => None,
        };
        let point3_of = |item: &Item| match item {
            Item::Point3(a, b, c) => Some([constant(a)?, constant(b)?, constant(c)?]),
            _ => None,
        };
        // A polygon from 3D points has 3D corners.
        let polygon_of = |items: &[&Item]| -> Option<Item> {
            if items.iter().any(|i| matches!(i, Item::Point3(..))) {
                items
                    .iter()
                    .map(|i| point3_of(i))
                    .collect::<Option<Vec<_>>>()
                    .map(Item::Polygon3)
            } else {
                items
                    .iter()
                    .map(|i| point_of(i))
                    .collect::<Option<Vec<_>>>()
                    .map(Item::Polygon)
            }
        };
        match f {
            ShapeFn::Join => {
                let items = args.into_iter().flat_map(|v| v.items).collect::<Vec<_>>();
                if items.len() > MAX_LIST {
                    problems.error("Lists can have at most 10,000 elements");
                    return Val::nan();
                }
                Val::list(items)
            }
            ShapeFn::Polygon => {
                let polygon_error =
                    "A polygon's vertices must be points that don't depend on x, y or t";
                // polygon(xs, ys) pairs up two lists of numbers.
                let numbers = |v: &Val| v.items.iter().all(|i| matches!(i, Item::Num(_)));
                if args.len() == 2 && numbers(&args[0]) && numbers(&args[1]) {
                    let paired =
                        self.broadcast(&args, problems, |items| match (items[0], items[1]) {
                            (Item::Num(a), Item::Num(b)) => Ok(Item::Point(a.clone(), b.clone())),
                            _ => Err(polygon_error.into()),
                        });
                    return self.shape(ShapeFn::Polygon, vec![paired], problems);
                }
                // A single list is the vertices of one polygon.
                if args.len() == 1 {
                    let vertices: Vec<&Item> = args[0].items.iter().collect();
                    return match polygon_of(&vertices) {
                        Some(polygon) => Val {
                            items: vec![polygon],
                            list: false,
                        },
                        None => {
                            problems.error(polygon_error);
                            Val::nan()
                        }
                    };
                }
                // Several points, any of them lists, make one polygon per
                // element.
                self.broadcast(&args, problems, |items| {
                    polygon_of(items).ok_or_else(|| polygon_error.to_owned())
                })
            }
            ShapeFn::Sort | ShapeFn::Unique => {
                let name = if f == ShapeFn::Sort { "sort" } else { "unique" };
                let keys_given = f == ShapeFn::Sort && args.len() == 2;
                if args.is_empty() || args.len() > if f == ShapeFn::Sort { 2 } else { 1 } {
                    problems.error(format!("Function '{name}' takes a list"));
                    return Val::nan();
                }
                let list = &args[0];
                if f == ShapeFn::Unique {
                    let mut seen: Vec<Item> = Vec::new();
                    for item in &list.items {
                        let key = |i: &Item| match i {
                            Item::Num(l) => constant(l).map(|v| vec![v]),
                            Item::Point(..) => point_of(i).map(|p| p.to_vec()),
                            Item::Point3(a, b, c) => {
                                Some(vec![constant(a)?, constant(b)?, constant(c)?])
                            }
                            Item::Complex(a, b) => Some(vec![constant(a)?, constant(b)?]),
                            Item::Polygon(_) | Item::Polygon3(_) => None,
                        };
                        let Some(k) = key(item) else {
                            problems.error("Function 'unique' needs numbers or points that don't depend on x or y");
                            return Val::nan();
                        };
                        let same = |a: &[f64], b: &[f64]| {
                            a.iter()
                                .zip(b)
                                .all(|(x, y)| x == y || (x.is_nan() && y.is_nan()))
                        };
                        if !seen.iter().any(|s| key(s).is_some_and(|sk| same(&sk, &k))) {
                            seen.push(item.clone());
                        }
                    }
                    return Val::list(seen);
                }
                let keys_val = if keys_given { &args[1] } else { list };
                let keys = keys_val
                    .items
                    .iter()
                    .map(|i| match i {
                        Item::Num(l) => constant(l),
                        _ => None,
                    })
                    .collect::<Option<Vec<f64>>>();
                let Some(keys) = keys else {
                    problems.error("Function 'sort' needs numbers that don't depend on x or y");
                    return Val::nan();
                };
                let n = list.items.len().min(keys.len());
                let mut order: Vec<usize> = (0..n).collect();
                // Undefined keys go last, and equal keys keep their order.
                order.sort_by(|&a, &b| match (keys[a].is_nan(), keys[b].is_nan()) {
                    (false, false) => keys[a].total_cmp(&keys[b]),
                    (x, y) => x.cmp(&y),
                });
                Val::list(order.into_iter().map(|i| list.items[i].clone()).collect())
            }
            ShapeFn::Real | ShapeFn::Imag | ShapeFn::Conj | ShapeFn::Arg => {
                if args.len() != 1 {
                    problems.error("This function takes 1 argument");
                    return Val::nan();
                }
                self.broadcast(&args, problems, |items| {
                    let (a, b) = items[0]
                        .parts()
                        .ok_or_else(|| "This function takes a number".to_owned())?;
                    Ok(match f {
                        ShapeFn::Real => Item::Num(a),
                        ShapeFn::Imag => Item::Num(b),
                        ShapeFn::Conj => Item::complex(a, shallow(Low::Neg(Box::new(b)))),
                        _ => Item::Num(shallow(Low::Call(Builtin::Arctan, vec![b, a]))),
                    })
                })
            }
            ShapeFn::Distance | ShapeFn::Midpoint => {
                let name = if f == ShapeFn::Distance {
                    "distance"
                } else {
                    "midpoint"
                };
                if args.len() != 2 {
                    problems.error(format!("Function '{name}' takes 2 points"));
                    return Val::nan();
                }
                self.broadcast(&args, problems, |items| match (items[0], items[1]) {
                    (Item::Point(a, b), Item::Point(c, d)) => Ok(if f == ShapeFn::Distance {
                        let dx = bin(BinOp::Sub, a.clone(), c.clone());
                        let dy = bin(BinOp::Sub, b.clone(), d.clone());
                        let sq = |l: Low| bin(BinOp::Mul, l.clone(), l);
                        Item::Num(shallow(Low::Call(
                            Builtin::Sqrt,
                            vec![bin(BinOp::Add, sq(dx), sq(dy))],
                        )))
                    } else {
                        let half = |p: &Low, q: &Low| {
                            bin(
                                BinOp::Div,
                                bin(BinOp::Add, p.clone(), q.clone()),
                                Low::Const(2.0),
                            )
                        };
                        Item::Point(half(a, c), half(b, d))
                    }),
                    _ => Err(format!("Function '{name}' takes 2 points")),
                })
            }
        }
    }

    /// Fits `lhs \sim rhs`: the names nothing defines become parameters,
    /// set by least squares (Levenberg-Marquardt) so the two sides agree as
    /// closely as they can. Also gives the residuals.
    fn fit(&mut self, lhs: &Expr, rhs: &Expr) -> Result<(Fit, Vec<f64>), String> {
        let mut params: Vec<String> = Vec::new();
        for side in [lhs, rhs] {
            side.visit_names(&mut |n, applied| {
                let free = !self.vars.contains_key(n)
                    && !self.funcs.contains_key(n)
                    && definable(n)
                    && !(applied && self.funcs.contains_key(n))
                    && n != "i"
                    && !params.iter().any(|p| p == n);
                if free {
                    params.push(n.to_owned());
                }
            });
        }
        let holes: Vec<u32> = params.iter().map(|_| self.hole()).collect();
        let bindings: HashMap<String, Val> = params
            .iter()
            .zip(&holes)
            .map(|(p, h)| (p.clone(), Val::num(Low::Hole(*h))))
            .collect();
        let mut problems = Problems::default();
        let observed = self.lower(lhs, &HashMap::new(), NO_FREE, &mut problems);
        let diff = Expr::Sub(Box::new(lhs.clone()), Box::new(rhs.clone()));
        let diff = self.lower(&diff, &bindings, NO_FREE, &mut problems);
        if let Some(e) = problems.first_message() {
            return Err(e);
        }
        let terms: Vec<Low> = diff
            .items
            .iter()
            .map(|i| match i {
                Item::Num(l) => Ok(l.clone()),
                _ => Err("Regressions fit numbers, not points".to_owned()),
            })
            .collect::<Result<_, _>>()?;
        let residuals = |p: &[f64]| -> Vec<f64> {
            terms
                .iter()
                .map(|t| {
                    let filled = t.map_leaves(&|leaf| match leaf {
                        Low::Hole(h) => holes.iter().position(|k| k == h).map(|k| Low::Const(p[k])),
                        _ => None,
                    });
                    match filled.fold() {
                        Low::Const(v) => v,
                        _ => f64::NAN,
                    }
                })
                .collect()
        };
        let values = least_squares(&residuals, params.len())
            .ok_or("These points can't be fitted to this model")?;
        let left_over = residuals(&values);
        let observed: Vec<f64> = observed
            .constant()
            .map(|v| match v {
                Value::Number(n) => vec![n],
                Value::Numbers(ns) => ns,
                _ => Vec::new(),
            })
            .unwrap_or_default();
        let r2 = (observed.len() == left_over.len() && observed.len() > 1)
            .then(|| {
                let mean = observed.iter().sum::<f64>() / observed.len() as f64;
                let total: f64 = observed.iter().map(|v| (v - mean).powi(2)).sum();
                let unexplained: f64 = left_over.iter().map(|r| r * r).sum();
                (total > 0.0).then(|| 1.0 - unexplained / total)
            })
            .flatten();
        let residuals = (1..)
            .map(|n| format!("e_{n}"))
            .find(|n| !self.vars.contains_key(n) && !self.funcs.contains_key(n));
        let fit = Fit {
            params: params.into_iter().zip(values).collect(),
            r2,
            residuals: diff.list.then_some(residuals).flatten(),
        };
        Ok((fit, left_over))
    }

    /// A regression's row: its fit, and the model drawn as a curve when the
    /// right side reads one list, which then stands for x (y_1 \sim m x_1 + b
    /// draws y = m x + b).
    fn regression_row(&mut self, id: u64, rhs: &Expr, fit: Result<Fit, String>) -> RowAnalysis {
        let mut row = RowAnalysis::new(id, RowKind::Regression);
        let fit = match fit {
            Ok(fit) => fit,
            Err(e) => {
                row.error = Some(e);
                return row;
            }
        };
        let mut lists: Vec<String> = Vec::new();
        rhs.visit_names(&mut |n, applied| {
            if !applied && !lists.iter().any(|l| l == n) && self.vars.contains_key(n) {
                lists.push(n.to_owned());
            }
        });
        let mut visiting = HashSet::new();
        lists.retain(|n| {
            self.value_of_ref(n, &mut visiting).is_ok()
                && matches!(self.values.get(n), Some(Ok(Value::Numbers(_))))
        });
        if let [list] = &lists[..] {
            let mut bindings = HashMap::new();
            bindings.insert(list.clone(), Val::num(Low::Arg(Arg::X)));
            let mut problems = Problems::default();
            let val = self.lower(rhs, &bindings, &[("x", Arg::X)], &mut problems);
            if problems.first_message().is_none() && !val.list && !val.has_points() {
                let mut error = None;
                row.curves = compile_all(&val, &mut error, |program| Curve {
                    axis: Axis::X,
                    program,
                });
            }
        }
        row.fit = Some(fit);
        row
    }

    fn hole(&mut self) -> u32 {
        self.next_hole += 1;
        self.next_hole
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
                        (Item::Num(a), Item::Num(b)) => Ok(Item::Num(shallow(Low::Select(
                            Box::new(a.clone()),
                            Box::new(b.clone()),
                            Box::new(Low::Const(0.0)),
                        )))),
                        _ => Err("Points can't be compared".into()),
                    })
                }
            });
        }
        result.unwrap_or_else(Val::nan)
    }

    fn call(&self, f: Builtin, args: Vec<Val>, problems: &mut Problems) -> Val {
        let point_error = || format!("Function '{}' can't take a point", f.name());
        if f == Builtin::Length {
            // Counts points and polygons as well as numbers.
            let n: usize = args.iter().map(|v| v.items.len()).sum();
            return Val::num(Low::Const(n as f64));
        }
        let complex = args.iter().any(Val::has_complex);
        if complex && matches!(f, Builtin::Total | Builtin::Mean) {
            // Sums and means of complex numbers add up each part.
            let (mut res, mut ims) = (Vec::new(), Vec::new());
            for item in args.iter().flat_map(|v| &v.items) {
                match item.parts() {
                    Some((a, b)) => {
                        res.push(a);
                        ims.push(b);
                    }
                    None => {
                        problems.error(point_error());
                        return Val::nan();
                    }
                }
            }
            return Val {
                items: vec![Item::complex(
                    shallow(Low::Call(f, res)),
                    shallow(Low::Call(f, ims)),
                )],
                list: false,
            };
        }
        if complex && matches!(f, Builtin::Abs | Builtin::Exp | Builtin::Sqrt) {
            return self.broadcast(&args, problems, |items| {
                let (a, b) = items[0].parts().ok_or_else(point_error)?;
                Ok(match f {
                    Builtin::Abs => Item::Num(call1(
                        Builtin::Sqrt,
                        bin(
                            BinOp::Add,
                            bin(BinOp::Mul, a.clone(), a),
                            bin(BinOp::Mul, b.clone(), b),
                        ),
                    )),
                    Builtin::Exp => complex_exp(a, b),
                    _ => complex_op(BinOp::Pow, (a, b), (Low::Const(0.5), Low::Const(0.0)))?,
                })
            });
        }
        if complex {
            problems.error(format!(
                "Function '{}' doesn't take complex numbers yet",
                f.name()
            ));
            return Val::nan();
        }
        if f.takes_list() {
            // total([1, 2, 3]) and max(a, b) both reduce every element.
            let mut flat = Vec::new();
            for v in &args {
                for item in &v.items {
                    match item {
                        Item::Num(l) => flat.push(l.clone()),
                        _ => {
                            problems.error(point_error());
                            return Val::nan();
                        }
                    }
                }
            }
            if flat.is_empty() {
                return Val::num(Low::Const(f.eval(&[])));
            }
            return Val::num(shallow(Low::Call(f, flat)));
        }
        self.broadcast(&args, problems, |items| {
            let mut lows = Vec::with_capacity(items.len());
            for item in items {
                match item {
                    Item::Num(l) => lows.push(l.clone()),
                    _ => return Err(point_error()),
                }
            }
            Ok(Item::Num(shallow(Low::Call(f, lows))))
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
                Item::Point(a, b) | Item::Complex(a, b) => a.uses_arg() || b.uses_arg(),
                Item::Point3(a, b, c) => a.uses_arg() || b.uses_arg() || c.uses_arg(),
                Item::Polygon(_) | Item::Polygon3(_) => false,
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
            let value = self.value_of(name, visiting);
            self.record(Read::Var(name.to_owned()));
            return match value {
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
        self.record(Read::Missing(name.to_owned()));
        // i is the imaginary unit unless a row defines it.
        if name == "i" {
            return Val {
                items: vec![Item::Complex(Low::Const(0.0), Low::Const(1.0))],
                list: false,
            };
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

/// A table column's numbers: a list padded or cut to the table's length,
/// or one number repeated down it.
fn numbers_of(v: &Value, rows: usize, error: &mut Option<String>) -> Option<Vec<f64>> {
    match v {
        Value::Number(n) => Some(vec![*n; rows]),
        Value::Numbers(ns) => {
            let mut ns = ns.clone();
            ns.resize(rows, f64::NAN);
            Some(ns)
        }
        Value::Point(..) | Value::Points(_) | Value::Point3(_) | Value::Points3(_) => {
            *error = Some("Table cells hold numbers, not points".into());
            None
        }
        Value::Polygon(_) | Value::Polygons(_) | Value::Polygon3(_) | Value::Polygons3(_) => {
            *error = Some("Table cells hold numbers, not polygons".into());
            None
        }
        Value::Complex(..) | Value::Complexes(_) => {
            *error = Some("Table cells hold real numbers".into());
            None
        }
    }
}

fn points_of(v: &Value) -> Vec<[f64; 2]> {
    let finite = |p: &[f64; 2]| p[0].is_finite() && p[1].is_finite();
    match v {
        Value::Point(a, b) => [[*a, *b]].into_iter().filter(finite).collect(),
        Value::Points(ps) => ps.iter().copied().filter(finite).collect(),
        _ => Vec::new(),
    }
}

fn points3_of(v: &Value) -> Vec<[f64; 3]> {
    let finite = |p: &[f64; 3]| p.iter().all(|c| c.is_finite());
    match v {
        Value::Point3(p) => [*p].into_iter().filter(finite).collect(),
        Value::Points3(ps) => ps.iter().copied().filter(finite).collect(),
        _ => Vec::new(),
    }
}

fn polygons3_of(v: &Value) -> Vec<Vec<[f64; 3]>> {
    match v {
        Value::Polygon3(p) => vec![p.clone()],
        Value::Polygons3(ps) => ps.clone(),
        _ => Vec::new(),
    }
}

fn polygons_of(v: &Value) -> Vec<Vec<[f64; 2]>> {
    match v {
        Value::Polygon(p) => vec![p.clone()],
        Value::Polygons(ps) => ps.clone(),
        _ => Vec::new(),
    }
}

/// A list or point of plain numbers, which shows no value under its row.
fn is_literal(e: &Expr) -> bool {
    match e {
        Expr::List(items) => items.iter().all(is_literal),
        Expr::Point(a, b) => a.as_literal().is_some() && b.as_literal().is_some(),
        Expr::Point3(a, b, c) => [a, b, c].iter().all(|e| e.as_literal().is_some()),
        other => other.as_literal().is_some(),
    }
}

/// For a statement with derivatives of y in it, as `y'' = -y`, the
/// highest; 0 when it has only x' and y' as in a system. None when it is no
/// differential equation.
fn ode_order(statement: &Statement) -> Option<usize> {
    let mut order: Option<usize> = None;
    let mut see = |name: &str, _: bool| {
        if let Some(primes) = name.strip_prefix(['x', 'y']) {
            if !primes.is_empty() && primes.chars().all(|c| c == '\'') {
                let n = if name.starts_with('y') {
                    primes.len()
                } else {
                    0
                };
                order = Some(order.map_or(n, |o| o.max(n)));
            }
        }
    };
    match statement {
        Statement::Expr(e) => e.visit_names(&mut see),
        Statement::Relation(a, _, b) | Statement::Regression(a, b) => {
            a.visit_names(&mut see);
            b.visit_names(&mut see);
        }
        Statement::Chain(c) => c.operands.iter().for_each(|e| e.visit_names(&mut see)),
    }
    // A system's y' counts as order 0, as its x' does.
    order.map(|o| if is_system_statement(statement) { 0 } else { o })
}

fn is_system_statement(statement: &Statement) -> bool {
    let Statement::Relation(a, _, b) = statement else {
        return false;
    };
    let is = |e: &Expr| matches!(e, Expr::Point(p, q) if is_var(p, "x'") && is_var(q, "y'"));
    is(a) || is(b)
}

fn is_var(e: &Expr, name: &str) -> bool {
    matches!(e, Expr::Var(n) if n == name)
}

const SEEDS_HINT: &str = "Initial points are written like (0, 1) or [(0, 1), (2, -1)]";

/// Undefined names that Desmos would offer as sliders.
/// The values of `n` parameters that make the residuals smallest in the
/// least-squares sense, by Levenberg-Marquardt from a few starting points.
/// None when no start gets anywhere finite.
fn least_squares(residuals: &dyn Fn(&[f64]) -> Vec<f64>, n: usize) -> Option<Vec<f64>> {
    let cost = |p: &[f64]| -> f64 {
        let c: f64 = residuals(p).iter().map(|r| r * r).sum();
        if c.is_nan() {
            f64::INFINITY
        } else {
            c
        }
    };
    // The normal equations at `p`: J^T J and J^T r, with the Jacobian by
    // differences of step `h(p_k)`, forward or central.
    let normal = |p: &[f64], central: bool| -> (Vec<Vec<f64>>, Vec<f64>) {
        let r = residuals(p);
        let jacobian: Vec<Vec<f64>> = (0..n)
            .map(|k| {
                let h = if central { 1e-5 } else { 1e-7 } * p[k].abs().max(1.0);
                let mut q = p.to_vec();
                q[k] += h;
                let up = residuals(&q);
                let down = if central {
                    q[k] -= 2.0 * h;
                    residuals(&q)
                } else {
                    r.clone()
                };
                let width = if central { 2.0 * h } else { h };
                up.iter().zip(&down).map(|(a, b)| (a - b) / width).collect()
            })
            .collect();
        let mut a = vec![vec![0.0; n]; n];
        let mut g = vec![0.0; n];
        for i in 0..n {
            for j in 0..n {
                a[i][j] = jacobian[i]
                    .iter()
                    .zip(&jacobian[j])
                    .map(|(x, y)| x * y)
                    .sum();
            }
            g[i] = jacobian[i].iter().zip(&r).map(|(x, y)| x * y).sum();
        }
        (a, g)
    };
    let solve = |start: f64| -> (Vec<f64>, f64) {
        let mut p = vec![start; n];
        let mut c = cost(&p);
        let mut lambda = 1e-3;
        for _ in 0..200 {
            let (a, g) = normal(&p, false);
            let mut improved = false;
            for _ in 0..30 {
                let mut damped = a.clone();
                for (i, row) in damped.iter_mut().enumerate() {
                    row[i] += lambda * a[i][i].max(1e-12);
                }
                let Some(step) = gauss(damped, g.iter().map(|v| -v).collect()) else {
                    lambda *= 10.0;
                    continue;
                };
                let q: Vec<f64> = p.iter().zip(&step).map(|(a, b)| a + b).collect();
                let cq = cost(&q);
                if cq < c {
                    let gain = c - cq;
                    p = q;
                    c = cq;
                    lambda = (lambda / 3.0).max(1e-12);
                    improved = gain > 1e-15 * c.max(1e-300);
                    break;
                }
                lambda *= 4.0;
            }
            if !improved {
                break;
            }
        }
        (p, c)
    };
    let mut best: Option<(Vec<f64>, f64)> = None;
    for start in [1.0, 0.1, -1.0, 10.0] {
        let (p, c) = solve(start);
        if c.is_finite() && best.as_ref().is_none_or(|b| c < b.1) {
            best = Some((p, c));
        }
        // A near-perfect fit needs no other start.
        if best.as_ref().is_some_and(|b| b.1 < 1e-20) {
            break;
        }
    }
    // Finish with plain Gauss-Newton steps on a central-difference Jacobian,
    // which land a model linear in its parameters on the exact answer.
    let (mut p, mut c) = best?;
    for _ in 0..8 {
        let (a, g) = normal(&p, true);
        let Some(step) = gauss(a, g.iter().map(|v| -v).collect()) else {
            break;
        };
        let q: Vec<f64> = p.iter().zip(&step).map(|(a, b)| a + b).collect();
        let cq = cost(&q);
        // Near the minimum the cost is flat to rounding, so a step that
        // leaves it the same to 12 digits still counts.
        if cq.is_nan() || cq > c * (1.0 + 1e-12) {
            break;
        }
        let settled = q == p;
        (p, c) = (q, cq);
        if settled {
            break;
        }
    }
    Some(p)
}

/// Solves `a x = b` by Gaussian elimination with partial pivoting.
fn gauss(mut a: Vec<Vec<f64>>, mut b: Vec<f64>) -> Option<Vec<f64>> {
    let n = b.len();
    for col in 0..n {
        let pivot = (col..n).max_by(|&i, &j| a[i][col].abs().total_cmp(&a[j][col].abs()))?;
        if a[pivot][col].abs().is_nan() || a[pivot][col].abs() <= 1e-300 {
            return None;
        }
        a.swap(col, pivot);
        b.swap(col, pivot);
        for row in col + 1..n {
            let f = a[row][col] / a[col][col];
            let (top, rest) = a.split_at_mut(row);
            for (x, &y) in rest[0][col..].iter_mut().zip(&top[col][col..]) {
                *x -= f * y;
            }
            b[row] -= f * b[col];
        }
    }
    let mut x = vec![0.0; n];
    for i in (0..n).rev() {
        let s: f64 = (i + 1..n).map(|k| a[i][k] * x[k]).sum();
        x[i] = (b[i] - s) / a[i][i];
    }
    x.iter().all(|v| v.is_finite()).then_some(x)
}

fn slider_names(missing: &[String]) -> Vec<String> {
    missing.iter().filter(|m| definable(m)).cloned().collect()
}

/// Turns the shaded regions of an inequality analysed in space into
/// translucent solids.
fn solids(mut row: RowAnalysis) -> RowAnalysis {
    for region in std::mem::take(&mut row.regions) {
        row.implicits3.push(Implicit3 {
            program: region.program,
            shade: Some(region.compare),
        });
    }
    row
}

/// Whether a chain of comparisons runs upward (`<`, `≤`), downward, or
/// (None) both ways.
fn direction(c: &Condition) -> Option<bool> {
    if c.relations
        .iter()
        .all(|r| matches!(r, Relation::Lt | Relation::Le))
    {
        Some(true)
    } else if c
        .relations
        .iter()
        .all(|r| matches!(r, Relation::Gt | Relation::Ge))
    {
        Some(false)
    } else {
        None
    }
}

/// Splits restrictions off an expression: `f\{0 \le x \le 1\}` is `f`
/// where `0 ≤ x ≤ 1`.
fn peel_restrictions(e: &Expr, conditions: &mut Vec<Condition>) -> Expr {
    // The parser writes `f\{c\}` as f times `\{c: 1\}`.
    let restriction = |e: &Expr| match e {
        Expr::Piecewise(branches, None)
            if branches.len() == 1 && matches!(branches[0].1, Expr::Num(n) if n == 1.0) =>
        {
            Some(branches[0].0.clone())
        }
        _ => None,
    };
    match e {
        Expr::Mul(a, b) if restriction(b).is_some() => {
            conditions.push(restriction(b).expect("checked"));
            peel_restrictions(a, conditions)
        }
        // Written last, a restriction binds to the last term, but in Desmos
        // it restricts the whole row.
        Expr::Add(a, b) => Expr::Add(a.clone(), Box::new(peel_restrictions(b, conditions))),
        Expr::Sub(a, b) => Expr::Sub(a.clone(), Box::new(peel_restrictions(b, conditions))),
        Expr::Piecewise(branches, None) if branches.len() == 1 => {
            conditions.push(branches[0].0.clone());
            peel_restrictions(&branches[0].1, conditions)
        }
        other => other.clone(),
    }
}
