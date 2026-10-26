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
            Expr::Neg(a) | Expr::Factorial(a) => a.visit_names(f),
            Expr::Add(a, b)
            | Expr::Sub(a, b)
            | Expr::Mul(a, b)
            | Expr::Div(a, b)
            | Expr::Pow(a, b) => {
                a.visit_names(f);
                b.visit_names(f);
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
