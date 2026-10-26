use crate::builtins::Builtin;

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
    /// `[1, 2, 3]`.
    List(Vec<Expr>),
    /// `[1...10]` or `[1, 3, ..., 11]`: the start, an optional second
    /// element that sets the step, and the end.
    Range(Box<Expr>, Option<Box<Expr>>, Box<Expr>),
    /// `L[2]`: an element or sublist of a list. When the base turns out not
    /// to be a list this is a product, as `a[1, 2]` is in Desmos.
    Index(Box<Expr>, Box<Expr>),
    /// `p.x` and `p.y`.
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
}

impl Expr {
    pub fn bin(op: fn(Box<Expr>, Box<Expr>) -> Expr, a: Expr, b: Expr) -> Expr {
        op(Box::new(a), Box::new(b))
    }

    /// Calls `f` on every name this expression reads, including the names
    /// of applied functions.
    pub fn visit_names(&self, f: &mut dyn FnMut(&str, bool)) {
        match self {
            Expr::Num(_) => {}
            Expr::Var(name) => f(name, false),
            Expr::Neg(a) | Expr::Factorial(a) | Expr::Coord(a, _) => a.visit_names(f),
            Expr::Add(a, b)
            | Expr::Sub(a, b)
            | Expr::Mul(a, b)
            | Expr::Div(a, b)
            | Expr::Pow(a, b)
            | Expr::Point(a, b)
            | Expr::Index(a, b) => {
                a.visit_names(f);
                b.visit_names(f);
            }
            Expr::List(items) => items.iter().for_each(|a| a.visit_names(f)),
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
            Expr::Call(_, args) => args.iter().for_each(|a| a.visit_names(f)),
            Expr::Apply(name, args) => {
                f(name, true);
                args.iter().for_each(|a| a.visit_names(f));
            }
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
