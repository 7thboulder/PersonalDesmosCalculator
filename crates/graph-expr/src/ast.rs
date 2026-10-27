use crate::builtins::Builtin;
use crate::units::Quantity;

#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    Num(f64),
    /// A variable such as `x`, `a_1` or `theta`.
    Var(String),
    Neg(Box<Expr>),
    Add(Box<Expr>, Box<Expr>),
    Sub(Box<Expr>, Box<Expr>),
    Mul(Box<Expr>, Box<Expr>),
    Div(Box<Expr>, Box<Expr>),
    Pow(Box<Expr>, Box<Expr>),
    Call(Builtin, Vec<Expr>),
    /// `f(a, b)`: a call when `f` is a defined function, otherwise a product
    /// (`a(x+1)` means a times x+1). Decided once all rows are known.
    Apply(String, Vec<Expr>),
    Factorial(Box<Expr>),
    /// `(a, b)`: a point, or a parametric curve when it reads `t`.
    Point(Box<Expr>, Box<Expr>),
    /// `(a, b, c)`: a point in 3D, or a 3D curve when it reads `t`.
    Point3(Box<Expr>, Box<Expr>, Box<Expr>),
    /// `[1, 2, 3]`.
    List(Vec<Expr>),
    /// `[1...10]` or `[1, 3, ..., 11]`: the start, an optional second
    /// element that sets the step, and the end.
    Range(Box<Expr>, Option<Box<Expr>>, Box<Expr>),
    /// `L[2]`: an element or sublist of a list. When the base turns out not
    /// to be a list this is a product, as `a[1, 2]` is in Desmos.
    Index(Box<Expr>, Box<Expr>),
    /// `L[L > 0]`: the elements of a list where a condition holds. The
    /// second part is the condition as `{L > 0: 1}`, so it is 1 where the
    /// element stays and undefined where it goes.
    Filter(Box<Expr>, Box<Expr>),
    /// `p.x`, `p.y` and `p.z`.
    Coord(Box<Expr>, Coord),
    /// `[n^2 for n = [1...5]]`: the body and each variable with its list.
    Comprehension(Box<Expr>, Vec<(String, Expr)>),
    /// `{x < 0: -x, x}`: conditions with their values, then an optional
    /// value for everything else. A branch without a value is worth 1, so
    /// `{x > 0}` restricts whatever it multiplies.
    Piecewise(Vec<(Condition, Expr)>, Option<Box<Expr>>),
    /// `\frac{d}{dx} body`: the derivative of `body` with respect to `x`,
    /// at the current value of `x`.
    Derivative(String, Box<Expr>),
    /// `f''(a)`: function `f` differentiated twice, at `a`.
    Prime(String, u32, Vec<Expr>),
    /// `\sum_{n=1}^{10} body` or `\prod`: the variable, the bounds and the body.
    Repeat(Repeat, String, Box<Expr>, Box<Expr>, Box<Expr>),
    /// `\int_a^b body dt`: the variable, the bounds and the body.
    Integral(String, Box<Expr>, Box<Expr>, Box<Expr>),
    /// A function that works on whole lists or builds shapes rather than
    /// numbers: `polygon(...)`, `join(...)`, `sort(...)`.
    Shape(ShapeFn, Vec<Expr>),
    /// `a \to a + 1`: an action that sets `a` when it runs.
    Update(String, Box<Expr>),
    /// Several actions run together: `a \to 1, b \to 2`.
    Actions(Vec<Expr>),
    /// A unit or constant written as text, as in `5\text{m/s}`. Rows are
    /// worked out with units replaced by their SI factors.
    Unit(Box<Quantity>),
    /// `… \text{in km}`: the answer shown in another unit.
    Convert(Box<Expr>, Box<Quantity>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShapeFn {
    Polygon,
    Join,
    Sort,
    Unique,
    Distance,
    Midpoint,
    Real,
    Imag,
    Conj,
    Arg,
    /// Rows that draw iteration: the Mandelbrot and Julia sets, a cobweb
    /// diagram and a bifurcation diagram. Each is a whole row.
    Mandelbrot,
    Julia,
    Cobweb,
    Bifurcation,
}

impl ShapeFn {
    pub fn from_name(name: &str) -> Option<ShapeFn> {
        Some(match name {
            "polygon" => ShapeFn::Polygon,
            "join" => ShapeFn::Join,
            "sort" => ShapeFn::Sort,
            "unique" => ShapeFn::Unique,
            "distance" => ShapeFn::Distance,
            "midpoint" => ShapeFn::Midpoint,
            "real" => ShapeFn::Real,
            "imag" => ShapeFn::Imag,
            "conj" => ShapeFn::Conj,
            "arg" => ShapeFn::Arg,
            "mandelbrot" => ShapeFn::Mandelbrot,
            "julia" => ShapeFn::Julia,
            "cobweb" => ShapeFn::Cobweb,
            "bifurcation" => ShapeFn::Bifurcation,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Repeat {
    Sum,
    Product,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Coord {
    X,
    Y,
    Z,
}

/// A comparison, possibly chained as in `0 < x < 2`: `operands` has one
/// more entry than `relations`.
#[derive(Clone, Debug, PartialEq)]
pub struct Condition {
    pub operands: Vec<Expr>,
    pub relations: Vec<Relation>,
}

impl Condition {
    fn visit_names(&self, f: &mut dyn FnMut(&str, bool)) {
        self.operands.iter().for_each(|e| e.visit_names(f));
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Relation {
    Eq,
    Lt,
    Gt,
    Le,
    Ge,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Statement {
    Expr(Expr),
    Relation(Expr, Relation, Expr),
    /// A chained inequality such as `-1 < y < x`, shaded where every
    /// comparison holds.
    Chain(Condition),
    /// `y_1 \sim m x_1 + b`: fit the undefined names on the right so the
    /// two sides agree as closely as they can.
    Regression(Expr, Expr),
    /// `y' = f(x, y)` or `\frac{dy}{dx} = f(x, y)`: a differential
    /// equation, drawn as a slope field with solutions through chosen
    /// points.
    Ode(Expr),
}

impl Expr {
    pub fn bin(op: fn(Box<Expr>, Box<Expr>) -> Expr, a: Expr, b: Expr) -> Expr {
        op(Box::new(a), Box::new(b))
    }

    /// Calls `f` on every name this expression reads, including the names
    /// of applied functions.
    pub fn visit_names(&self, f: &mut dyn FnMut(&str, bool)) {
        match self {
            Expr::Num(_) | Expr::Unit(_) => {}
            Expr::Var(name) => f(name, false),
            Expr::Neg(a) | Expr::Factorial(a) | Expr::Coord(a, _) | Expr::Convert(a, _) => {
                a.visit_names(f)
            }
            Expr::Add(a, b)
            | Expr::Sub(a, b)
            | Expr::Mul(a, b)
            | Expr::Div(a, b)
            | Expr::Pow(a, b)
            | Expr::Point(a, b)
            | Expr::Index(a, b)
            | Expr::Filter(a, b) => {
                a.visit_names(f);
                b.visit_names(f);
            }
            Expr::List(items) => items.iter().for_each(|a| a.visit_names(f)),
            Expr::Point3(a, b, c) => {
                a.visit_names(f);
                b.visit_names(f);
                c.visit_names(f);
            }
            Expr::Range(a, b, c) => {
                a.visit_names(f);
                if let Some(b) = b {
                    b.visit_names(f);
                }
                c.visit_names(f);
            }
            Expr::Derivative(_, body) => body.visit_names(f),
            Expr::Prime(name, _, args) => {
                f(name, true);
                args.iter().for_each(|a| a.visit_names(f));
            }
            Expr::Repeat(_, var, lo, hi, body) | Expr::Integral(var, lo, hi, body) => {
                lo.visit_names(f);
                hi.visit_names(f);
                // The variable is bound inside the body.
                body.visit_names(&mut |n, applied| {
                    if applied || n != var {
                        f(n, applied)
                    }
                });
            }
            Expr::Comprehension(body, generators) => {
                // The comprehension's own variables are not free names.
                let bound: Vec<&str> = generators.iter().map(|(n, _)| n.as_str()).collect();
                body.visit_names(&mut |n, applied| {
                    if applied || !bound.contains(&n) {
                        f(n, applied)
                    }
                });
                generators.iter().for_each(|(_, list)| list.visit_names(f));
            }
            Expr::Piecewise(branches, otherwise) => {
                for (condition, value) in branches {
                    condition.visit_names(f);
                    value.visit_names(f);
                }
                if let Some(e) = otherwise {
                    e.visit_names(f);
                }
            }
            Expr::Call(_, args) | Expr::Shape(_, args) | Expr::Actions(args) => {
                args.iter().for_each(|a| a.visit_names(f))
            }
            Expr::Update(name, value) => {
                f(name, false);
                value.visit_names(f);
            }
            Expr::Apply(name, args) => {
                f(name, true);
                args.iter().for_each(|a| a.visit_names(f));
            }
        }
    }

    /// Calls `f` on this expression and every expression inside it.
    pub fn walk(&self, f: &mut dyn FnMut(&Expr)) {
        f(self);
        match self {
            Expr::Num(_) | Expr::Var(_) | Expr::Unit(_) => {}
            Expr::Neg(a)
            | Expr::Factorial(a)
            | Expr::Coord(a, _)
            | Expr::Derivative(_, a)
            | Expr::Convert(a, _) => a.walk(f),
            Expr::Add(a, b)
            | Expr::Sub(a, b)
            | Expr::Mul(a, b)
            | Expr::Div(a, b)
            | Expr::Pow(a, b)
            | Expr::Point(a, b)
            | Expr::Index(a, b)
            | Expr::Filter(a, b) => {
                a.walk(f);
                b.walk(f);
            }
            Expr::Update(_, a) => a.walk(f),
            Expr::List(items)
            | Expr::Call(_, items)
            | Expr::Shape(_, items)
            | Expr::Actions(items)
            | Expr::Apply(_, items)
            | Expr::Prime(_, _, items) => items.iter().for_each(|a| a.walk(f)),
            Expr::Point3(a, b, c) => {
                a.walk(f);
                b.walk(f);
                c.walk(f);
            }
            Expr::Range(a, b, c) => {
                a.walk(f);
                if let Some(b) = b {
                    b.walk(f);
                }
                c.walk(f);
            }
            Expr::Repeat(_, _, a, b, c) | Expr::Integral(_, a, b, c) => {
                a.walk(f);
                b.walk(f);
                c.walk(f);
            }
            Expr::Comprehension(body, generators) => {
                body.walk(f);
                generators.iter().for_each(|(_, l)| l.walk(f));
            }
            Expr::Piecewise(branches, otherwise) => {
                for (condition, value) in branches {
                    condition.operands.iter().for_each(|a| a.walk(f));
                    value.walk(f);
                }
                if let Some(e) = otherwise {
                    e.walk(f);
                }
            }
        }
    }

    /// Whether this is an action by its form: `a \to 1`, several of them,
    /// or a piecewise choice between them. A name that refers to an action
    /// is only known once every row is.
    pub fn is_action(&self) -> bool {
        match self {
            Expr::Update(..) | Expr::Actions(_) => true,
            Expr::Piecewise(branches, otherwise) => {
                branches.iter().any(|(_, v)| v.is_action())
                    || otherwise.as_ref().is_some_and(|o| o.is_action())
            }
            _ => false,
        }
    }

    pub fn mentions(&self, name: &str) -> bool {
        let mut found = false;
        self.visit_names(&mut |n, _| found |= n == name);
        found
    }

    /// The literal number this expression spells, if it is one (`3`, `-2.5`).
    pub fn as_literal(&self) -> Option<f64> {
        match self {
            Expr::Num(v) => Some(*v),
            Expr::Neg(inner) => match **inner {
                Expr::Num(v) => Some(-v),
                _ => None,
            },
            _ => None,
        }
    }
}
