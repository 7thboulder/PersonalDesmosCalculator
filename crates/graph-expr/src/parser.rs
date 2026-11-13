//! Recursive-descent parser following Desmos's reading of math:
//! implicit multiplication, `\sin x` without brackets, `\sin^2 x`,
//! `\sin^{-1}x`, `\log_2 x`, subscripted names and so on.

use crate::ast::{Condition, Coord, Expr, Relation, Repeat, ShapeFn, Statement};
use crate::builtins::{self, Builtin};
use crate::lexer::{tokenize, Tok};

pub fn parse(latex: &str) -> Result<Statement, String> {
    parse_with_units(latex).map(crate::units::lower_statement)
}

/// As `parse`, keeping units and conversions as written, for checking them.
pub fn parse_with_units(latex: &str) -> Result<Statement, String> {
    let toks = derivatives(crate::dots::rewrite(tokenize(latex)?)?);
    let mut p = Parser {
        toks,
        pos: 0,
        pipe_depth: 0,
        integral_depth: 0,
    };
    let mut statement = p.statement()?;
    // `a \to 1, b \to 2` runs both actions, as does `A, B` for actions
    // with names.
    if let Statement::Expr(first) = &statement {
        if p.peek() == Some(&Tok::Comma) {
            let mut actions = vec![first.clone()];
            while p.eat(&Tok::Comma) {
                actions.push(p.expr()?);
            }
            statement = Statement::Expr(Expr::Actions(actions));
        }
    }
    // `x' = f, y' = g` is a system, as `(x', y') = (f, g)`.
    if let Statement::Relation(Expr::Var(a), Relation::Eq, f) = &statement {
        if (a == "x'" || a == "y'") && p.eat(&Tok::Comma) {
            let Statement::Relation(Expr::Var(b), Relation::Eq, g) = p.statement()? else {
                return Err(SYSTEM_HINT.into());
            };
            let (f, g) = match (a.as_str(), b.as_str()) {
                ("x'", "y'") => (f.clone(), g),
                ("y'", "x'") => (g, f.clone()),
                _ => return Err(SYSTEM_HINT.into()),
            };
            let v = |n: &str| Expr::Var(n.into());
            statement = Statement::Relation(
                Expr::bin(Expr::Point, v("x'"), v("y'")),
                Relation::Eq,
                Expr::bin(Expr::Point, f, g),
            );
        }
    }
    // `5 m/s · 3 h \text{in km}` shows the answer in km.
    if let Some(Tok::UnitTarget(text)) = p.peek().cloned() {
        p.pos += 1;
        let target = Box::new(crate::units::parse_unit(&text)?);
        statement = match statement {
            Statement::Expr(e) => Statement::Expr(Expr::Convert(Box::new(e), target)),
            Statement::Relation(lhs, Relation::Eq, rhs) => {
                Statement::Relation(lhs, Relation::Eq, Expr::Convert(Box::new(rhs), target))
            }
            _ => return Err("Only a value can be converted, like 5 km in mi".into()),
        };
    }
    if let Some(tok) = p.peek() {
        return Err(unexpected(tok));
    }
    // ∂ belongs only in the fraction of a partial derivative.
    if format!("{statement:?}").contains("Var(\"partial\")") {
        return Err("Write a partial derivative as a fraction, like ∂/∂x (type partial)".into());
    }
    Ok(statement)
}

/// `\nabla f`: the partial derivatives of `f` as a point: in x and y (and
/// z when it reads z), or else in the two or three letters it reads.
fn gradient(body: Expr) -> Expr {
    let mut names: Vec<String> = Vec::new();
    body.visit_names(&mut |n, applied| {
        let constant = matches!(n, "e" | "pi" | "tau" | "infty" | "i");
        if !applied && !constant && !names.iter().any(|m| m == n) {
            names.push(n.to_owned());
        }
    });
    let has = |v: &str| names.iter().any(|n| n == v);
    let vars: Vec<String> = if has("z") {
        vec!["x".into(), "y".into(), "z".into()]
    } else if has("x") || has("y") || !(2..=3).contains(&names.len()) {
        vec!["x".into(), "y".into()]
    } else {
        names
    };
    let d = |v: &String| Box::new(Expr::Derivative(v.clone(), Box::new(body.clone())));
    match vars.as_slice() {
        [a, b, c] => Expr::Point3(d(a), d(b), d(c)),
        [a, b] => Expr::Point(d(a), d(b)),
        _ => unreachable!(),
    }
}

/// One expression from tokens already read, as Solve's n-fold integrals
/// rebuild their parts.
pub(crate) fn parse_tokens(toks: Vec<Tok>) -> Result<Expr, String> {
    let mut p = Parser {
        toks: derivatives(toks),
        pos: 0,
        pipe_depth: 0,
        integral_depth: 0,
    };
    let e = p.expr()?;
    if let Some(tok) = p.peek() {
        return Err(unexpected(tok));
    }
    match crate::units::lower_statement(Statement::Expr(e)) {
        Statement::Expr(e) => Ok(e),
        _ => Err("Expected an expression".into()),
    }
}

const SYSTEM_HINT: &str = "A system is written x'=f, y'=g or (x',y')=(f,g)";

/// Reads derivatives in differential equations as names of their own:
/// `y'`, `y''` and `\frac{dy}{dx}`, `\frac{d^2y}{dx^2}` become `y'` and
/// `y''`, and `x'` or `\frac{dx}{dt}` becomes `x'`. A prime followed by
/// brackets, as in `f'(x)`, is still a function's derivative.
fn derivatives(toks: Vec<Tok>) -> Vec<Tok> {
    derivatives_info(toks).0
}

/// As `derivatives`, saying too which Greek letter was read as y (see
/// [`greek_ode`]).
fn derivatives_info(toks: Vec<Tok>) -> (Vec<Tok>, Option<(String, bool)>) {
    let letter = |t: Option<&Tok>, names: &[&str]| match t {
        Some(Tok::Letter(l)) if names.contains(&l.as_str()) => Some(l.clone()),
        _ => None,
    };
    // `^2` or `^{2}` at `toks[i]`: the power and how many tokens it took.
    let power = |toks: &[Tok], i: usize| -> Option<(usize, usize)> {
        let whole = |v: &f64| (v.fract() == 0.0 && *v >= 1.0 && *v <= 9.0).then_some(*v as usize);
        match (
            toks.get(i),
            toks.get(i + 1),
            toks.get(i + 2),
            toks.get(i + 3),
        ) {
            (Some(Tok::Caret), Some(Tok::Num(v)), _, _) => Some((whole(v)?, 2)),
            (Some(Tok::Caret), Some(Tok::LBrace), Some(Tok::Num(v)), Some(Tok::RBrace)) => {
                Some((whole(v)?, 4))
            }
            _ => None,
        }
    };
    // A Greek letter can be the unknown too, as θ in θ'' = -sin θ.
    let unknown = |t: Option<&Tok>| match t {
        Some(Tok::Letter(l)) if l == "x" || l == "y" || greek_unknown(l) => Some(l.clone()),
        _ => None,
    };
    let mut out = Vec::with_capacity(toks.len());
    let mut i = 0;
    while i < toks.len() {
        if let Some(v) = unknown(toks.get(i)) {
            let primes = toks[i + 1..]
                .iter()
                .take_while(|t| **t == Tok::Prime)
                .count();
            if primes > 0 && toks.get(i + 1 + primes) != Some(&Tok::LParen) {
                out.push(Tok::Letter(format!("{v}{}", "'".repeat(primes))));
                i += 1 + primes;
                continue;
            }
        }
        // \frac{d^n v}{d u^n}
        if toks[i] == Tok::Frac && toks.get(i + 1) == Some(&Tok::LBrace) {
            let mut j = i + 2;
            let d = |j: usize| letter(toks.get(j), &["d"]).is_some();
            let leibniz = (|| {
                if !d(j) {
                    return None;
                }
                j += 1;
                let (top, used) = power(&toks, j).unwrap_or((1, 0));
                j += used;
                let v = unknown(toks.get(j))?;
                j += 1;
                if toks.get(j) != Some(&Tok::RBrace) || toks.get(j + 1) != Some(&Tok::LBrace) {
                    return None;
                }
                j += 2;
                if !d(j) {
                    return None;
                }
                let u = letter(toks.get(j + 1), &["x", "t"])?;
                j += 2;
                let (bottom, used) = power(&toks, j).unwrap_or((1, 0));
                j += used;
                if toks.get(j) != Some(&Tok::RBrace) || u == v || top != bottom {
                    return None;
                }
                Some((v, top, j + 1))
            })();
            if let Some((v, order, next)) = leibniz {
                out.push(Tok::Letter(format!("{v}{}", "'".repeat(order))));
                i = next;
                continue;
            }
        }
        out.push(toks[i].clone());
        i += 1;
    }
    greek_ode(out)
}

/// Greek letters that can be a differential equation's unknown: all but
/// the constants.
fn greek_unknown(name: &str) -> bool {
    crate::lexer::GREEK.contains(&name) && !matches!(name, "pi" | "tau")
}

/// An equation in a Greek letter and its derivatives, as θ'' = -sin θ, is
/// read as one in y: θ becomes y, θ' becomes y' and so on, and t (the
/// usual name of time) becomes x when x is not used. Returns the tokens,
/// and the letter and whether t was renamed when it is such an equation.
/// Anything that also uses y, or derivatives of two letters, is left as
/// it is.
fn greek_ode(toks: Vec<Tok>) -> (Vec<Tok>, Option<(String, bool)>) {
    let base = |name: &str| -> Option<(String, usize)> {
        let primes = name.len() - name.trim_end_matches('\'').len();
        let stem = &name[..name.len() - primes];
        (primes > 0).then(|| (stem.to_owned(), primes))
    };
    let mut greek: Option<String> = None;
    for t in &toks {
        let Tok::Letter(l) = t else { continue };
        if l == "y" {
            return (toks, None);
        }
        if let Some((stem, _)) = base(l) {
            if stem == "x" || stem == "y" {
                return (toks, None);
            }
            if greek_unknown(&stem) {
                match &greek {
                    Some(g) if *g != stem => return (toks, None),
                    _ => greek = Some(stem),
                }
            }
        }
    }
    let Some(g) = greek else {
        return (toks, None);
    };
    let has = |name: &str| {
        toks.iter()
            .any(|t| matches!(t, Tok::Letter(l) if l == name))
    };
    let time = has("t") && !has("x");
    let toks = toks
        .into_iter()
        .map(|t| match t {
            Tok::Letter(l) if l == g => Tok::Letter("y".into()),
            Tok::Letter(l) if time && l == "t" => Tok::Letter("x".into()),
            Tok::Letter(l) => match base(&l) {
                Some((stem, primes)) if stem == g => {
                    Tok::Letter(format!("y{}", "'".repeat(primes)))
                }
                _ => Tok::Letter(l),
            },
            t => t,
        })
        .collect();
    (toks, Some((g, time)))
}

/// For a differential equation written in a Greek letter, as
/// θ'' = -(g/L) sin θ: the letter (`theta`) and the name of the variable
/// it depends on (t, or x when x is used instead).
pub(crate) fn greek_unknown_of(latex: &str) -> Option<(String, String)> {
    let toks = crate::dots::rewrite(tokenize(latex).ok()?).ok()?;
    let (g, time) = derivatives_info(toks).1?;
    let toks = tokenize(latex).ok()?;
    let uses_x = toks.iter().any(|t| matches!(t, Tok::Letter(l) if l == "x"));
    Some((g, if time || !uses_x { "t" } else { "x" }.to_owned()))
}

struct Parser {
    toks: Vec<Tok>,
    pos: usize,
    /// How many bare `|` absolute values are open, so a closing `|` is not
    /// mistaken for the start of another factor.
    pipe_depth: usize,
    /// How many integrals are open, so the `dx` that ends one is not read
    /// as a product.
    integral_depth: usize,
}

fn unexpected(tok: &Tok) -> String {
    match tok {
        Tok::RParen | Tok::RBrace | Tok::AbsClose | Tok::RBracket | Tok::RCurly => {
            "Unbalanced brackets".into()
        }
        Tok::Comma => "Points are written with brackets, like (1, 2)".into(),
        Tok::Colon => "A ':' only belongs inside a piecewise definition like {x<0: 1}".into(),
        Tok::Dots => "A '...' only belongs inside a list like [1...10]".into(),
        Tok::MatrixClose | Tok::Amp | Tok::RowBreak => "This only belongs inside a matrix".into(),
        Tok::Eq | Tok::Lt | Tok::Gt | Tok::Le | Tok::Ge => "Unexpected comparison".into(),
        other => format!("Unexpected {}", describe(other)),
    }
}

fn describe(tok: &Tok) -> String {
    match tok {
        Tok::Num(v) => format!("number {v}"),
        Tok::Letter(l) => format!("'{l}'"),
        Tok::OpName(n) => format!("'{n}'"),
        Tok::Unsupported(n) => format!("'\\{n}'"),
        Tok::Unit(t) => format!("'{t}'"),
        Tok::UnitTarget(t) => format!("'in {t}': a conversion goes at the end, like 5 km in mi"),
        other => format!("{other:?}").to_lowercase(),
    }
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }

    fn next(&mut self) -> Option<Tok> {
        let tok = self.toks.get(self.pos).cloned();
        self.pos += 1;
        tok
    }

    fn eat(&mut self, tok: &Tok) -> bool {
        if self.peek() == Some(tok) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, tok: &Tok) -> Result<(), String> {
        match self.next() {
            Some(ref t) if t == tok => Ok(()),
            Some(t) => Err(unexpected(&t)),
            None => Err("Incomplete expression".into()),
        }
    }

    fn statement(&mut self) -> Result<Statement, String> {
        let mut condition = self.comparison()?;
        match condition.relations.len() {
            0 if self.eat(&Tok::Tilde) => {
                let lhs = condition.operands.remove(0);
                Ok(Statement::Regression(lhs, self.expr()?))
            }
            0 => Ok(Statement::Expr(condition.operands.remove(0))),
            1 => {
                let rhs = condition.operands.pop().expect("two operands");
                let lhs = condition.operands.pop().expect("two operands");
                Ok(Statement::Relation(lhs, condition.relations[0], rhs))
            }
            _ => {
                if condition.relations.contains(&Relation::Eq) {
                    return Err("Equations can't be chained; try an inequality like 0<y<x".into());
                }
                Ok(Statement::Chain(condition))
            }
        }
    }

    fn relation(&self) -> Option<Relation> {
        Some(match self.peek()? {
            Tok::Eq => Relation::Eq,
            Tok::Lt => Relation::Lt,
            Tok::Gt => Relation::Gt,
            Tok::Le => Relation::Le,
            Tok::Ge => Relation::Ge,
            _ => return None,
        })
    }

    /// An expression followed by any number of comparisons: `x`, `x < 2`,
    /// `0 < x \le 2`.
    fn comparison(&mut self) -> Result<Condition, String> {
        let mut operands = vec![self.expr()?];
        let mut relations = Vec::new();
        while let Some(rel) = self.relation() {
            self.pos += 1;
            if self.peek().is_none() {
                return Err("Incomplete expression".into());
            }
            relations.push(rel);
            operands.push(self.expr()?);
        }
        Ok(Condition {
            operands,
            relations,
        })
    }

    fn expr(&mut self) -> Result<Expr, String> {
        let e = self.sum()?;
        // `n^2 for n = [1...5]`, with or without brackets around it.
        if matches!(self.peek(), Some(Tok::OpName(n)) if n == "for") {
            self.pos += 1;
            return Ok(Expr::Comprehension(Box::new(e), self.generators()?));
        }
        if !self.eat(&Tok::Arrow) {
            return Ok(e);
        }
        let Expr::Var(name) = e else {
            return Err("Only a variable can be updated by an action, as in a \\to a+1".into());
        };
        if self.peek().is_none() {
            return Err("Incomplete expression".into());
        }
        Ok(Expr::Update(name, Box::new(self.sum()?)))
    }

    fn sum(&mut self) -> Result<Expr, String> {
        let mut e = self.term()?;
        loop {
            if self.eat(&Tok::Plus) {
                e = Expr::bin(Expr::Add, e, self.term()?);
            } else if self.eat(&Tok::Minus) {
                e = Expr::bin(Expr::Sub, e, self.term()?);
            } else {
                return Ok(e);
            }
        }
    }

    fn term(&mut self) -> Result<Expr, String> {
        let mut e = self.unary()?;
        loop {
            if self.eat(&Tok::Mul) {
                e = Expr::bin(Expr::Mul, e, self.unary()?);
            } else if self.eat(&Tok::Div) {
                e = Expr::bin(Expr::Div, e, self.unary()?);
            } else if self.starts_factor() {
                e = Expr::bin(Expr::Mul, e, self.power()?);
            } else {
                return Ok(e);
            }
        }
    }

    /// Whether the next tokens are the `dx` that closes an integral.
    fn at_differential(&self) -> bool {
        self.integral_depth > 0
            && matches!(self.toks.get(self.pos), Some(Tok::Letter(d)) if d == "d")
            && matches!(self.toks.get(self.pos + 1), Some(Tok::Letter(_)))
    }

    fn starts_factor(&self) -> bool {
        if self.at_differential() {
            return false;
        }
        match self.peek() {
            // `for` ends the body of a list comprehension.
            Some(Tok::OpName(n)) if n == "for" => false,
            Some(
                Tok::Num(_)
                | Tok::Letter(_)
                | Tok::OpName(_)
                | Tok::Frac
                | Tok::Sqrt
                | Tok::LParen
                | Tok::AbsOpen
                | Tok::LBrace
                | Tok::LBracket
                | Tok::MatrixOpen
                | Tok::LCurly
                | Tok::Sum
                | Tok::Prod
                | Tok::Int
                | Tok::Lim
                | Tok::Unit(_)
                | Tok::Unsupported(_),
            ) => true,
            Some(Tok::Pipe) => self.pipe_depth == 0,
            _ => false,
        }
    }

    fn unary(&mut self) -> Result<Expr, String> {
        if self.eat(&Tok::Minus) {
            return Ok(Expr::Neg(Box::new(self.unary()?)));
        }
        if self.eat(&Tok::Plus) {
            return self.unary();
        }
        self.power()
    }

    fn power(&mut self) -> Result<Expr, String> {
        let base = self.postfix()?;
        if self.eat(&Tok::Caret) {
            let exponent = self.script()?;
            let mut e = Expr::bin(Expr::Pow, base, exponent);
            while self.eat(&Tok::Bang) {
                e = Expr::Factorial(Box::new(e));
            }
            return Ok(e);
        }
        Ok(base)
    }

    /// The contents of a superscript or subscript: `{...}` or one token.
    fn script(&mut self) -> Result<Expr, String> {
        if self.eat(&Tok::LBrace) {
            let e = self.expr()?;
            self.expect(&Tok::RBrace)?;
            return Ok(e);
        }
        match self.next() {
            Some(Tok::Num(v)) => Ok(Expr::Num(v)),
            Some(Tok::Letter(l)) => Ok(Expr::Var(l)),
            Some(Tok::Minus) => Ok(Expr::Neg(Box::new(self.script()?))),
            Some(t) => Err(unexpected(&t)),
            None => Err("Incomplete expression".into()),
        }
    }

    fn postfix(&mut self) -> Result<Expr, String> {
        let mut e = self.primary()?;
        loop {
            if self.eat(&Tok::Bang) {
                e = Expr::Factorial(Box::new(e));
            } else if self.peek() == Some(&Tok::LBracket) && indexable(&e) {
                self.pos += 1;
                if let Some(mask) = self.filter() {
                    e = Expr::Filter(Box::new(e), Box::new(mask));
                    continue;
                }
                let index = match self.list()? {
                    Expr::List(mut items) if items.len() == 1 => items.remove(0),
                    other => other,
                };
                e = Expr::Index(Box::new(e), Box::new(index));
            } else if self.eat(&Tok::Dot) {
                e = match self.next() {
                    Some(Tok::Letter(l)) if l == "x" => Expr::Coord(Box::new(e), Coord::X),
                    Some(Tok::Letter(l)) if l == "y" => Expr::Coord(Box::new(e), Coord::Y),
                    Some(Tok::Letter(l)) if l == "z" => Expr::Coord(Box::new(e), Coord::Z),
                    Some(Tok::OpName(n)) if n == "length" => Expr::Call(Builtin::Length, vec![e]),
                    _ => return Err("Only .x, .y, .z and .length can follow a '.'".into()),
                };
            } else if self.peek() == Some(&Tok::Prime) {
                return Err("Only functions can be differentiated with ', like f'(x)".into());
            } else {
                return Ok(e);
            }
        }
    }

    /// The condition in `L[L > 0]`, as `{L > 0: 1}`, with the closing
    /// bracket eaten. None (and nothing eaten) when the brackets hold an
    /// index instead.
    fn filter(&mut self) -> Option<Expr> {
        let start = self.pos;
        match self.comparison() {
            Ok(c) if !c.relations.is_empty() && self.eat(&Tok::RBracket) => {
                Some(Expr::Piecewise(vec![(c, Expr::Num(1.0))], None))
            }
            _ => {
                self.pos = start;
                None
            }
        }
    }

    fn primary(&mut self) -> Result<Expr, String> {
        let tok = self.next().ok_or("Incomplete expression")?;
        match tok {
            Tok::Num(v) => Ok(Expr::Num(v)),
            Tok::Unit(text) => Ok(Expr::Unit(Box::new(crate::units::parse_unit(&text)?))),
            Tok::Letter(letter) if letter == "nabla" => {
                // `\nabla f`: the gradient, a point of partial derivatives.
                if !(self.starts_factor() || self.peek() == Some(&Tok::Minus)) {
                    return Err("∇ needs something to take the gradient of".into());
                }
                let body = self.term()?;
                Ok(gradient(body))
            }
            // Only meaningful in the fraction of a partial derivative; a
            // stray one is caught once the whole row is read.
            Tok::Letter(letter) if letter == "partial" => Ok(Expr::Var(letter)),
            Tok::Letter(letter) => {
                // `H_{2n-1}`: a subscript holding an expression.
                if let Some(index) = self.subscript_expr()? {
                    let name = format!("{letter}_");
                    // `J_{n+1}(x)`: a Bessel function of order n+1.
                    if crate::builtins::bessel(&letter).is_some()
                        && self.peek() == Some(&Tok::LParen)
                    {
                        let mut args = vec![index];
                        args.extend(self.paren_args()?);
                        return Ok(Expr::Apply(name, args));
                    }
                    return Ok(Expr::Apply(name, vec![index]));
                }
                let name = self.subscripted(letter)?;
                let mut primes = 0;
                while self.eat(&Tok::Prime) {
                    primes += 1;
                }
                if primes > 0 {
                    if self.peek() != Some(&Tok::LParen) {
                        return Err(format!(
                            "{name}{} needs an argument, like {name}{}(x)",
                            "'".repeat(primes as usize),
                            "'".repeat(primes as usize)
                        ));
                    }
                    let args = self.paren_args()?;
                    return Ok(Expr::Prime(name, primes, args));
                }
                if self.peek() == Some(&Tok::LParen) {
                    let args = self.paren_args()?;
                    return Ok(Expr::Apply(name, args));
                }
                Ok(Expr::Var(name))
            }
            Tok::OpName(name) => self.function(&name),
            Tok::Frac => {
                self.expect(&Tok::LBrace)?;
                let num = self.expr()?;
                self.expect(&Tok::RBrace)?;
                self.expect(&Tok::LBrace)?;
                let den = self.expr()?;
                self.expect(&Tok::RBrace)?;
                // `\frac{\partial f}{\partial x}`: what to differentiate is on
                // top, after the ∂.
                if let Some((d, body)) = leading_d(&num) {
                    if let Some(vars) = derivative_vars(&d, &den)? {
                        return Ok(vars
                            .into_iter()
                            .rev()
                            .fold(body, |b, v| Expr::Derivative(v, Box::new(b))));
                    }
                }
                if let Some(vars) = derivative_vars(&num, &den)? {
                    if self.peek().is_none()
                        || !(self.starts_factor() || self.peek() == Some(&Tok::Minus))
                    {
                        return Err(format!("d/d{} needs something to differentiate", vars[0]));
                    }
                    let body = self.term()?;
                    // The variable next to the body is the first one used.
                    return Ok(vars
                        .into_iter()
                        .rev()
                        .fold(body, |b, v| Expr::Derivative(v, Box::new(b))));
                }
                Ok(Expr::bin(Expr::Div, num, den))
            }
            Tok::Sqrt => {
                let index = if self.eat(&Tok::LBracket) {
                    let index = self.expr()?;
                    self.expect(&Tok::RBracket)?;
                    Some(index)
                } else {
                    None
                };
                self.expect(&Tok::LBrace)?;
                let radicand = self.expr()?;
                self.expect(&Tok::RBrace)?;
                Ok(match index {
                    Some(n) => Expr::Call(Builtin::Root, vec![n, radicand]),
                    None => Expr::Call(Builtin::Sqrt, vec![radicand]),
                })
            }
            Tok::LParen => {
                let e = self.expr()?;
                if e.is_action() {
                    // `(a \to 1, b \to 2)` groups actions.
                    let mut actions = vec![e];
                    while self.eat(&Tok::Comma) {
                        actions.push(self.expr()?);
                    }
                    self.expect(&Tok::RParen)?;
                    return Ok(Expr::Actions(actions));
                }
                if self.eat(&Tok::Comma) {
                    let y = self.expr()?;
                    if self.eat(&Tok::Comma) {
                        let z = self.expr()?;
                        if self.peek() == Some(&Tok::Comma) {
                            return Err("Points have at most three coordinates".into());
                        }
                        self.expect(&Tok::RParen)?;
                        return Ok(Expr::Point3(Box::new(e), Box::new(y), Box::new(z)));
                    }
                    self.expect(&Tok::RParen)?;
                    return Ok(Expr::Point(Box::new(e), Box::new(y)));
                }
                self.expect(&Tok::RParen)?;
                Ok(e)
            }
            Tok::AbsOpen => {
                let e = self.expr()?;
                self.expect(&Tok::AbsClose)?;
                Ok(Expr::Call(Builtin::Abs, vec![e]))
            }
            Tok::Pipe => {
                self.pipe_depth += 1;
                let e = self.expr();
                self.pipe_depth -= 1;
                let e = e?;
                self.expect(&Tok::Pipe)?;
                Ok(Expr::Call(Builtin::Abs, vec![e]))
            }
            Tok::LBrace => {
                let e = self.expr()?;
                self.expect(&Tok::RBrace)?;
                Ok(e)
            }
            Tok::LBracket => self.list(),
            Tok::MatrixOpen => self.matrix(),
            Tok::LCurly => self.piecewise(),
            Tok::Sum => self.repeat(Repeat::Sum),
            Tok::Prod => self.repeat(Repeat::Product),
            Tok::Int => self.integral(),
            Tok::Lim => self.limit(),
            Tok::Unsupported(name) => Err(format!("'\\{name}' isn't supported yet")),
            other => Err(unexpected(&other)),
        }
    }

    /// `\sum_{n=1}^{10} body` after the `\sum`. The body runs over the
    /// product that follows, stopping at a `+` or `-`, as in Desmos.
    fn repeat(&mut self, kind: Repeat) -> Result<Expr, String> {
        let what = if kind == Repeat::Sum {
            "sum"
        } else {
            "product"
        };
        let bounds_error = || format!("A {what} needs bounds, like n=1 below and 10 above");
        if !self.eat(&Tok::Underscore) || !self.eat(&Tok::LBrace) {
            return Err(bounds_error());
        }
        let var = match self.next() {
            Some(Tok::Letter(l)) => self.subscripted(l)?,
            _ => return Err(bounds_error()),
        };
        if !self.eat(&Tok::Eq) {
            return Err(bounds_error());
        }
        let lo = self.expr()?;
        self.expect(&Tok::RBrace)?;
        if !self.eat(&Tok::Caret) {
            return Err(bounds_error());
        }
        let hi = self.script()?;
        if self.peek().is_none() || !(self.starts_factor() || self.peek() == Some(&Tok::Minus)) {
            return Err(format!("This {what} needs something to add up"));
        }
        let body = self.term()?;
        Ok(Expr::Repeat(
            kind,
            var,
            Box::new(lo),
            Box::new(hi),
            Box::new(body),
        ))
    }

    /// `\lim_{x\to c} body` after the `\lim`, with `c^+` or `c^-` for one
    /// side. The body runs over the following term, as a sum's does. It is
    /// worked out numerically: see [`crate::limit`].
    fn limit(&mut self) -> Result<Expr, String> {
        let form = || "A limit needs a variable and a point, like \\lim_{x\\to 0}".to_string();
        if !self.eat(&Tok::Underscore) || self.peek() != Some(&Tok::LBrace) {
            return Err(form());
        }
        let open = self.pos;
        let mut depth = 0;
        let mut close = None;
        for (i, t) in self.toks.iter().enumerate().skip(open) {
            match t {
                Tok::LBrace => depth += 1,
                Tok::RBrace => {
                    depth -= 1;
                    if depth == 0 {
                        close = Some(i);
                        break;
                    }
                }
                _ => {}
            }
        }
        let close = close.ok_or_else(form)?;
        // The side: c^+ or c^{+} from above, c^- or c^{-} from below.
        let (side, cut) = match &self.toks[open + 1..close] {
            [.., Tok::Caret, Tok::Plus] => (1, 2),
            [.., Tok::Caret, Tok::Minus] => (-1, 2),
            [.., Tok::Caret, Tok::LBrace, Tok::Plus, Tok::RBrace] => (1, 4),
            [.., Tok::Caret, Tok::LBrace, Tok::Minus, Tok::RBrace] => (-1, 4),
            _ => (0, 0),
        };
        self.toks.drain(close - cut..close);
        self.pos = open + 1;
        let var = match self.next() {
            Some(Tok::Letter(l)) => self.subscripted(l)?,
            _ => return Err(form()),
        };
        if !self.eat(&Tok::Arrow) {
            return Err(form());
        }
        if self.peek() == Some(&Tok::RBrace) {
            return Err("A limit needs the point it goes to".into());
        }
        let point = self.expr()?;
        self.expect(&Tok::RBrace)?;
        if self.peek().is_none() || !(self.starts_factor() || self.peek() == Some(&Tok::Minus)) {
            return Err("This limit needs something to take the limit of".into());
        }
        let body = self.term()?;
        Ok(crate::limit::numeric(&var, point, side, body))
    }

    /// `\int_a^b body dx` after the `\int`. The body runs to the `dx`.
    fn integral(&mut self) -> Result<Expr, String> {
        let bounds_error = || "An integral needs bounds, like 0 below and 1 above".to_string();
        let empty = |p: &Parser, at: usize| {
            matches!(
                (p.toks.get(at), p.toks.get(at + 1)),
                (Some(Tok::LBrace), Some(Tok::RBrace))
            )
        };
        // No bounds, or the empty ones the editor leaves: \int_{ }^{ }.
        let indefinite = || Expr::Var(crate::ast::INDEFINITE.into());
        let (lo, hi) = if self.peek() != Some(&Tok::Underscore) {
            (indefinite(), indefinite())
        } else if empty(self, self.pos + 1)
            && self.toks.get(self.pos + 3) == Some(&Tok::Caret)
            && empty(self, self.pos + 4)
        {
            self.pos += 6;
            (indefinite(), indefinite())
        } else {
            self.pos += 1;
            let lo = self.script()?;
            if !self.eat(&Tok::Caret) {
                return Err(bounds_error());
            }
            (lo, self.script()?)
        };
        self.integral_depth += 1;
        let body = if self.at_differential() {
            Ok(Expr::Num(1.0))
        } else {
            self.expr()
        };
        self.integral_depth -= 1;
        let body = body?;
        let var = match (self.next(), self.next()) {
            (Some(Tok::Letter(d)), Some(Tok::Letter(v))) if d == "d" => self.subscripted(v)?,
            _ => return Err("An integral ends with the variable, like dx".into()),
        };
        Ok(Expr::Integral(
            var,
            Box::new(lo),
            Box::new(hi),
            Box::new(body),
        ))
    }

    /// The rest of a list after its `[`: `]`, `1, 2]`, `1...10]`,
    /// `1, 3, ..., 11]` or `n^2 for n = [1...5]]`.
    /// The `n = [1...5], m = [1, 2]` after `for`.
    fn generators(&mut self) -> Result<Vec<(String, Expr)>, String> {
        let mut generators = Vec::new();
        loop {
            let name = match self.next() {
                Some(Tok::Letter(l)) => self.subscripted(l)?,
                _ => return Err("'for' needs a variable, like n=[1...5]".into()),
            };
            self.expect(&Tok::Eq)?;
            generators.push((name, self.sum()?));
            // Another generator follows a comma only as `name =`.
            let more = self.peek() == Some(&Tok::Comma)
                && matches!(self.toks.get(self.pos + 1), Some(Tok::Letter(_)))
                && self.toks[self.pos + 2..]
                    .iter()
                    .take_while(|t| !matches!(t, Tok::Comma | Tok::RBracket | Tok::RParen))
                    .any(|t| *t == Tok::Eq);
            if !more {
                break;
            }
            self.pos += 1;
        }
        Ok(generators)
    }

    /// A matrix typed in place, after its `\begin{bmatrix}`: entries split
    /// by `&` and rows by `\\`. It reads as the list of its rows, as
    /// `[[1, 2], [3, 4]]` does, and an empty entry is 0, as in a matrix row.
    fn matrix(&mut self) -> Result<Expr, String> {
        let mut rows = vec![Vec::new()];
        loop {
            let entry = match self.peek() {
                Some(Tok::Amp | Tok::RowBreak | Tok::MatrixClose) => Expr::Num(0.0),
                _ => self.expr()?,
            };
            rows.last_mut().expect("a row").push(entry);
            match self.next() {
                Some(Tok::Amp) => {}
                Some(Tok::RowBreak) => rows.push(Vec::new()),
                Some(Tok::MatrixClose) => break,
                _ => return Err("This matrix isn't finished".into()),
            }
        }
        if rows.iter().any(|r| r.len() != rows[0].len()) {
            return Err("Every row of a matrix needs the same number of entries".into());
        }
        Ok(Expr::List(rows.into_iter().map(Expr::List).collect()))
    }

    fn list(&mut self) -> Result<Expr, String> {
        if self.eat(&Tok::RBracket) {
            return Ok(Expr::List(Vec::new()));
        }
        let mut items = vec![self.expr()?];
        if self.eat(&Tok::Dots) {
            let end = self.expr()?;
            self.expect(&Tok::RBracket)?;
            return Ok(Expr::Range(Box::new(items.remove(0)), None, Box::new(end)));
        }
        if matches!(items[0], Expr::Comprehension(..)) && self.eat(&Tok::RBracket) {
            return Ok(items.remove(0));
        }
        while self.eat(&Tok::Comma) {
            if self.eat(&Tok::Dots) {
                // [1, 3, ..., 11]: the first two elements set the step.
                self.eat(&Tok::Comma);
                let end = self.expr()?;
                self.expect(&Tok::RBracket)?;
                if items.len() != 2 {
                    return Err("A range like [1, 3, ..., 11] starts with two elements".into());
                }
                let second = items.pop().expect("two items");
                let first = items.pop().expect("two items");
                return Ok(Expr::Range(
                    Box::new(first),
                    Some(Box::new(second)),
                    Box::new(end),
                ));
            }
            items.push(self.expr()?);
            if self.eat(&Tok::Dots) {
                // [1, 3...11]
                let end = self.expr()?;
                self.expect(&Tok::RBracket)?;
                if items.len() != 2 {
                    return Err("A range like [1, 3, ..., 11] starts with two elements".into());
                }
                let second = items.pop().expect("two items");
                let first = items.pop().expect("two items");
                return Ok(Expr::Range(
                    Box::new(first),
                    Some(Box::new(second)),
                    Box::new(end),
                ));
            }
        }
        self.expect(&Tok::RBracket)?;
        Ok(Expr::List(items))
    }

    /// The rest of a piecewise definition after its `{`.
    fn piecewise(&mut self) -> Result<Expr, String> {
        let mut branches = Vec::new();
        let mut otherwise = None;
        if self.peek() == Some(&Tok::RCurly) {
            return Err("Empty piecewise definition".into());
        }
        loop {
            if otherwise.is_some() {
                return Err(
                    "The value for everything else comes last in a piecewise definition".into(),
                );
            }
            let mut c = self.comparison()?;
            if c.relations.is_empty() {
                otherwise = Some(Box::new(c.operands.remove(0)));
            } else {
                let value = if self.eat(&Tok::Colon) {
                    self.expr()?
                } else {
                    Expr::Num(1.0)
                };
                branches.push((c, value));
            }
            if !self.eat(&Tok::Comma) {
                break;
            }
        }
        self.expect(&Tok::RCurly)?;
        if branches.is_empty() {
            return Err("A piecewise definition needs a condition, like {x<0: 1}".into());
        }
        Ok(Expr::Piecewise(branches, otherwise))
    }

    /// A subscript that holds more than letters and digits, as in
    /// `H_{2n-1}` or `a_{(n+1)}`, read as an expression. None (and nothing
    /// eaten) when there is no subscript or it is a plain name like `a_{12}`.
    fn subscript_expr(&mut self) -> Result<Option<Expr>, String> {
        if self.peek() != Some(&Tok::Underscore)
            || self.toks.get(self.pos + 1) != Some(&Tok::LBrace)
        {
            return Ok(None);
        }
        let mut i = self.pos + 2;
        let mut plain = true;
        loop {
            match self.toks.get(i) {
                Some(Tok::RBrace) => break,
                Some(Tok::Letter(l)) if l.len() == 1 => {}
                Some(Tok::Num(v)) if v.fract() == 0.0 && *v >= 0.0 => {}
                Some(_) => {
                    plain = false;
                    break;
                }
                None => break,
            }
            i += 1;
        }
        if plain {
            return Ok(None);
        }
        self.pos += 2;
        let e = self.expr()?;
        self.expect(&Tok::RBrace)?;
        Ok(Some(e))
    }

    /// Reads an optional subscript after a letter: `a_1`, `x_{12}`, `a_{bc}`.
    fn subscripted(&mut self, letter: String) -> Result<String, String> {
        if !self.eat(&Tok::Underscore) {
            return Ok(letter);
        }
        let mut sub = String::new();
        let mut push = |tok: Tok| -> Result<(), String> {
            match tok {
                Tok::Letter(l) if l.len() == 1 => sub.push_str(&l),
                Tok::Num(v) if v.fract() == 0.0 && v >= 0.0 => {
                    sub.push_str(&format!("{}", v as u64))
                }
                _ => return Err("Subscripts can only contain letters and digits".into()),
            }
            Ok(())
        };
        if self.eat(&Tok::LBrace) {
            loop {
                match self.next() {
                    Some(Tok::RBrace) => break,
                    Some(tok) => push(tok)?,
                    None => return Err("Incomplete expression".into()),
                }
            }
        } else {
            push(self.next().ok_or("Incomplete expression")?)?;
        }
        if sub.is_empty() {
            return Err("Empty subscript".into());
        }
        Ok(format!("{letter}_{sub}"))
    }

    fn paren_args(&mut self) -> Result<Vec<Expr>, String> {
        self.expect(&Tok::LParen)?;
        let mut args = vec![self.expr()?];
        while self.eat(&Tok::Comma) {
            args.push(self.expr()?);
        }
        self.expect(&Tok::RParen)?;
        Ok(args)
    }

    fn function(&mut self, name: &str) -> Result<Expr, String> {
        if let Some(shape) = ShapeFn::from_name(name) {
            if self.peek() != Some(&Tok::LParen) {
                return Err(format!("Function '{name}' needs parentheses"));
            }
            // `mandelbrot()` needs nothing more.
            if shape == ShapeFn::Mandelbrot && self.toks.get(self.pos + 1) == Some(&Tok::RParen) {
                self.pos += 2;
                return Ok(Expr::Shape(shape, Vec::new()));
            }
            return Ok(Expr::Shape(shape, self.paren_args()?));
        }
        // Matrix functions are worked out before anything else, by name.
        if crate::matrix::is_function(name) {
            if self.peek() != Some(&Tok::LParen) {
                return Err(format!(
                    "Function '{name}' needs parentheses, like {name}(A)"
                ));
            }
            return Ok(Expr::Apply(name.to_owned(), self.paren_args()?));
        }
        let mut builtin =
            builtins::from_name(name).ok_or_else(|| format!("'{name}' isn't supported yet"))?;
        let mut power = None;
        let mut base = None;
        if self.eat(&Tok::Caret) {
            let exponent = self.script()?;
            match (exponent.as_literal(), builtin.inverse()) {
                (Some(-1.0), Some(inverse)) => builtin = inverse,
                _ => power = Some(exponent),
            }
        }
        if builtin == Builtin::Log && self.eat(&Tok::Underscore) {
            base = Some(self.script()?);
            builtin = Builtin::LogBase;
        }
        // Li_s(x): the order is the subscript.
        if builtin == Builtin::Polylog {
            if !self.eat(&Tok::Underscore) {
                return Err("Li needs its order as a subscript, like Li_2(x)".into());
            }
            base = Some(self.script()?);
        }
        let mut args = if self.peek() == Some(&Tok::LParen) {
            self.paren_args()?
        } else {
            vec![self.bare_argument(name)?]
        };
        if let Some(base) = base {
            if args.len() != 1 {
                return Err(format!("Function '{name}' takes 1 argument"));
            }
            args.insert(0, base);
        }
        let arity = builtin.arity();
        if args.len() < arity.min || args.len() > arity.max {
            return Err(match (arity.min, arity.max) {
                (1, 1) => format!("Function '{name}' takes 1 argument"),
                (min, max) if min == max => format!("Function '{name}' takes {min} arguments"),
                (min, usize::MAX) => format!("Function '{name}' needs at least {min} argument"),
                (min, max) => format!("Function '{name}' takes {min} to {max} arguments"),
            });
        }
        let call = Expr::Call(builtin, args);
        Ok(match power {
            Some(p) => Expr::bin(Expr::Pow, call, p),
            None => call,
        })
    }

    /// `\sin 2x` takes the product that follows, stopping at another named
    /// function, so `\sin x\cos x` is sin(x)·cos(x).
    fn bare_argument(&mut self, name: &str) -> Result<Expr, String> {
        if self.peek().is_none() || !(self.starts_factor() || self.peek() == Some(&Tok::Minus)) {
            return Err(format!("Function '{name}' needs an argument"));
        }
        let mut e = if self.eat(&Tok::Minus) {
            Expr::Neg(Box::new(self.power()?))
        } else {
            self.power()?
        };
        while self.starts_factor() && !matches!(self.peek(), Some(Tok::OpName(_))) {
            e = Expr::bin(Expr::Mul, e, self.power()?);
        }
        Ok(e)
    }
}

/// Whether `[...]` straight after `e` indexes it (`L[2]`) rather than
/// starting a new factor.
fn indexable(e: &Expr) -> bool {
    matches!(
        e,
        Expr::Var(_)
            | Expr::List(_)
            | Expr::Range(..)
            | Expr::Comprehension(..)
            | Expr::Index(..)
            | Expr::Filter(..)
            | Expr::Apply(..)
    )
}

/// The variables of a derivative written as a fraction, in the order they
/// are written: `\frac{d}{dx}` gives x, `\frac{d^2}{dx^2}` gives x, x and
/// `\frac{\partial^2}{\partial x\partial y}` gives x, y. None when the
/// fraction isn't a derivative; Err when it looks like one whose orders
/// don't add up.
/// `∂f`, `∂²u` or `d f`: the d (with its power) and what follows it.
fn leading_d(num: &Expr) -> Option<(Expr, Expr)> {
    fn factors(e: &Expr, out: &mut Vec<Expr>) {
        match e {
            Expr::Mul(a, b) => {
                factors(a, out);
                factors(b, out);
            }
            other => out.push(other.clone()),
        }
    }
    let mut fs = Vec::new();
    factors(num, &mut fs);
    let is_d = |e: &Expr| matches!(e, Expr::Var(d) if d == "d" || d == "partial");
    let d = fs.first()?.clone();
    let ok = is_d(&d) || matches!(&d, Expr::Pow(b, _) if is_d(b));
    if !ok || fs.len() < 2 {
        return None;
    }
    let body = fs[1..]
        .iter()
        .cloned()
        .reduce(|a, b| Expr::bin(Expr::Mul, a, b))?;
    Some((d, body))
}

fn derivative_vars(num: &Expr, den: &Expr) -> Result<Option<Vec<String>>, String> {
    let is_d = |e: &Expr| matches!(e, Expr::Var(d) if d == "d" || d == "partial");
    let whole = |e: &Expr| match e {
        Expr::Num(k) if k.fract() == 0.0 && *k >= 1.0 && *k <= 20.0 => Some(*k as usize),
        _ => None,
    };
    let order = match num {
        e if is_d(e) => 1,
        Expr::Pow(d, k) if is_d(d) => match whole(k) {
            Some(k) => k,
            None => return Ok(None),
        },
        _ => return Ok(None),
    };
    // The bottom as a list of factors, left to right.
    fn factors(e: &Expr, out: &mut Vec<Expr>) {
        match e {
            Expr::Mul(a, b) => {
                factors(a, out);
                factors(b, out);
            }
            other => out.push(other.clone()),
        }
    }
    let mut fs = Vec::new();
    factors(den, &mut fs);
    if fs.len() < 2 || !is_d(&fs[0]) {
        return Ok(None);
    }
    let partial = matches!(num, Expr::Var(p) if p == "partial")
        || matches!(num, Expr::Pow(p, _) if **p == Expr::Var("partial".into()));
    let mut vars = Vec::new();
    let mut i = 0;
    while i < fs.len() {
        if !is_d(&fs[i]) {
            return Ok(None);
        }
        let (v, k) = match fs.get(i + 1) {
            Some(Expr::Var(v)) if !is_d(&Expr::Var(v.clone())) => (v.clone(), 1),
            Some(Expr::Pow(b, k)) => match (&**b, whole(k)) {
                (Expr::Var(v), Some(k)) if v != "d" && v != "partial" => (v.clone(), k),
                _ => return Ok(None),
            },
            _ => return Ok(None),
        };
        vars.extend(std::iter::repeat_n(v, k));
        i += 2;
    }
    if vars.len() != order {
        let d = if partial { "∂" } else { "d" };
        return Err(format!(
            "The orders don't match: {d}^{order} on top needs {order} variable{} below",
            if order == 1 { "" } else { "s" }
        ));
    }
    let distinct: std::collections::HashSet<&String> = vars.iter().collect();
    if !partial && distinct.len() > 1 {
        return Err("Use ∂ for a derivative in more than one variable".into());
    }
    Ok(Some(vars))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtins::Builtin::*;

    fn e(src: &str) -> Expr {
        match parse(src).unwrap() {
            Statement::Expr(e) => e,
            other => panic!("not an expression: {other:?}"),
        }
    }
    fn v(n: &str) -> Expr {
        Expr::Var(n.into())
    }
    fn n(x: f64) -> Expr {
        Expr::Num(x)
    }
    fn mul(a: Expr, b: Expr) -> Expr {
        Expr::bin(Expr::Mul, a, b)
    }
    fn pow(a: Expr, b: Expr) -> Expr {
        Expr::bin(Expr::Pow, a, b)
    }
    fn call(b: Builtin, args: Vec<Expr>) -> Expr {
        Expr::Call(b, args)
    }

    #[test]
    fn implicit_multiplication() {
        assert_eq!(e("2x"), mul(n(2.0), v("x")));
        assert_eq!(e("ab"), mul(v("a"), v("b")));
        assert_eq!(e("2\\pi r"), mul(mul(n(2.0), v("pi")), v("r")));
    }

    #[test]
    fn unary_minus_binds_looser_than_power() {
        assert_eq!(e("-x^2"), Expr::Neg(Box::new(pow(v("x"), n(2.0)))));
    }

    #[test]
    fn functions_without_brackets() {
        assert_eq!(e("\\sin x"), call(Sin, vec![v("x")]));
        assert_eq!(e("\\sin 2x"), call(Sin, vec![mul(n(2.0), v("x"))]));
        assert_eq!(
            e("\\sin x\\cos x"),
            mul(call(Sin, vec![v("x")]), call(Cos, vec![v("x")]))
        );
    }

    #[test]
    fn function_powers_and_inverses() {
        assert_eq!(e("\\sin^2x"), pow(call(Sin, vec![v("x")]), n(2.0)));
        assert_eq!(e("\\sin^{-1}x"), call(Arcsin, vec![v("x")]));
        assert_eq!(e("\\log_2x"), call(LogBase, vec![n(2.0), v("x")]));
    }

    #[test]
    fn subscripts_and_apply() {
        assert_eq!(e("a_1"), v("a_1"));
        assert_eq!(e("x_{12}"), v("x_12"));
        assert_eq!(
            e("f\\left(x+1\\right)"),
            Expr::Apply("f".into(), vec![Expr::bin(Expr::Add, v("x"), n(1.0))])
        );
    }

    #[test]
    fn differential_equations() {
        let ode = |src: &str| match parse(src).unwrap() {
            Statement::Relation(lhs, Relation::Eq, rhs) => (lhs, rhs),
            other => panic!("not an equation: {other:?}"),
        };
        let x_minus_y = Expr::bin(Expr::Sub, v("x"), v("y"));
        assert_eq!(ode("y'=x-y"), (v("y'"), x_minus_y.clone()));
        assert_eq!(ode("\\frac{dy}{dx}=x-y"), (v("y'"), x_minus_y.clone()));
        assert_eq!(ode("\\frac{d^2y}{dx^2}=-y").0, v("y''"));
        assert_eq!(ode("\\frac{d^{3}y}{dx^{3}}=y").0, v("y'''"));
        assert_eq!(ode("y''=-y'").1, Expr::Neg(Box::new(v("y'"))));
        assert_eq!(parse("y'="), Err("Incomplete expression".into()));
        // f'(x) is still a derivative, and d/dx still differentiates.
        assert!(matches!(ode("f'(x)=2").0, Expr::Prime(..)));
        assert!(matches!(e("\\frac{d}{dx}x^2"), Expr::Derivative(..)));
        // Partial and higher derivatives: the variable written last is
        // applied first.
        let d = |v: &str, b: Expr| Expr::Derivative(v.into(), Box::new(b));
        let x2y = Expr::bin(Expr::Mul, pow(v("x"), n(2.0)), v("y"));
        assert_eq!(e("\\frac{\\partial}{\\partial y}x^2y"), d("y", x2y.clone()));
        assert_eq!(e("\\frac{∂}{∂y}x^2y"), d("y", x2y.clone()));
        assert_eq!(
            e("\\frac{\\partial^2}{\\partial x\\partial y}x^2y"),
            d("x", d("y", x2y.clone()))
        );
        assert_eq!(
            e("\\frac{\\partial^{3}}{\\partial x^{2}\\partial y}x^2y"),
            d("x", d("x", d("y", x2y.clone())))
        );
        assert_eq!(e("\\frac{d^2}{dx^2}x^2y"), d("x", d("x", x2y.clone())));
        assert_eq!(
            e("\\nabla\\left(x^2y\\right)"),
            Expr::bin(Expr::Point, d("x", x2y.clone()), d("y", x2y))
        );
        assert!(parse("\\frac{d^2}{dx}x").is_err());
        assert!(parse("\\frac{d^2}{dxdy}x").is_err());
        assert!(parse("\\partial x").is_err());
        // Leibniz form, with what is differentiated on top.
        assert_eq!(e("\\frac{\\partial f}{\\partial x}"), d("x", v("f")));
        assert_eq!(
            e("\\frac{\\partial^{2}u}{\\partial x^{2}}"),
            d("x", d("x", v("u")))
        );
        assert_eq!(
            e("\\frac{\\partial^2f}{\\partial x\\partial y}"),
            d("x", d("y", v("f")))
        );
        assert_eq!(e("\\frac{df}{dt}"), d("t", v("f")));
        // Systems, either way round.
        let point = |a: Expr, b: Expr| Expr::bin(Expr::Point, a, b);
        let system = (
            point(v("x'"), v("y'")),
            point(v("y"), Expr::Neg(Box::new(v("x")))),
        );
        assert_eq!(ode("x'=y,y'=-x"), system);
        assert_eq!(ode("y'=-x,x'=y"), system);
        assert_eq!(ode("\\frac{dx}{dt}=y,\\frac{dy}{dt}=-x"), system);
        assert_eq!(ode("\\left(x',y'\\right)=\\left(y,-x\\right)"), system);
        assert!(parse("x'=y,x'=1").is_err());
    }

    #[test]
    fn roots_fractions_abs() {
        assert_eq!(e("\\sqrt[3]{x}"), call(Root, vec![n(3.0), v("x")]));
        assert_eq!(e("\\frac{1}{x}"), Expr::bin(Expr::Div, n(1.0), v("x")));
        assert_eq!(e("\\left|x\\right|"), call(Abs, vec![v("x")]));
        assert_eq!(e("|x|"), call(Abs, vec![v("x")]));
    }

    #[test]
    fn points_lists_and_ranges() {
        assert_eq!(
            e("\\left(1,a\\right)"),
            Expr::Point(Box::new(n(1.0)), Box::new(v("a")))
        );
        assert_eq!(e("\\left[1,2\\right]"), Expr::List(vec![n(1.0), n(2.0)]));
        assert_eq!(
            e("\\left[1...10\\right]"),
            Expr::Range(Box::new(n(1.0)), None, Box::new(n(10.0)))
        );
        assert_eq!(
            e("\\left[1,3,...,11\\right]"),
            Expr::Range(Box::new(n(1.0)), Some(Box::new(n(3.0))), Box::new(n(11.0)))
        );
        assert_eq!(
            e("L\\left[2\\right]"),
            Expr::Index(Box::new(v("L")), Box::new(n(2.0)))
        );
        assert_eq!(e("p.x"), Expr::Coord(Box::new(v("p")), Coord::X));
        assert!(matches!(
            e("\\left[n^2\\operatorname{for}n=\\left[1...5\\right]\\right]"),
            Expr::Comprehension(_, ref g) if g[0].0 == "n"
        ));
    }

    #[test]
    fn piecewise_and_chains() {
        let Expr::Piecewise(branches, otherwise) = e("\\left\\{x<0:-x,x\\right\\}") else {
            panic!()
        };
        assert_eq!(branches.len(), 1);
        assert_eq!(otherwise, Some(Box::new(v("x"))));
        // A restriction is worth 1 where it holds.
        let Expr::Mul(_, restriction) = e("x^2\\left\\{0<x<2\\right\\}") else {
            panic!()
        };
        let Expr::Piecewise(branches, None) = *restriction else {
            panic!()
        };
        assert_eq!(branches[0].0.relations, vec![Relation::Lt, Relation::Lt]);
        assert_eq!(branches[0].1, n(1.0));
        assert!(matches!(parse("-1<y<x").unwrap(), Statement::Chain(_)));
    }

    #[test]
    fn calculus() {
        assert_eq!(
            e("\\frac{d}{dx}x^2"),
            Expr::Derivative("x".into(), Box::new(pow(v("x"), n(2.0))))
        );
        assert_eq!(
            e("f''\\left(x\\right)"),
            Expr::Prime("f".into(), 2, vec![v("x")])
        );
        assert_eq!(
            e("\\sum_{n=1}^{10}n^2"),
            Expr::Repeat(
                Repeat::Sum,
                "n".into(),
                Box::new(n(1.0)),
                Box::new(n(10.0)),
                Box::new(pow(v("n"), n(2.0)))
            )
        );
        // The body of a sum stops at +.
        assert!(matches!(e("\\sum_{n=1}^{3}n+1"), Expr::Add(..)));
        // The body of an integral runs to the dx, + and all.
        let Expr::Integral(var, _, _, body) = e("\\int_{0}^{x}t^2+1dt") else {
            panic!()
        };
        assert_eq!(var, "t");
        assert!(matches!(*body, Expr::Add(..)));
        assert!(parse("\\int_{0}^{1}t^2").is_err(), "no dt");
    }

    #[test]
    fn matrices_typed_in_place() {
        let n = |v: f64| Expr::Num(v);
        let row = |items: Vec<Expr>| Expr::List(items);
        assert_eq!(
            parse("2\\begin{bmatrix}1&\\\\x&4\\end{bmatrix}").unwrap(),
            Statement::Expr(Expr::bin(
                Expr::Mul,
                n(2.0),
                row(vec![
                    row(vec![n(1.0), n(0.0)]),
                    row(vec![Expr::Var("x".into()), n(4.0)])
                ])
            ))
        );
        assert_eq!(
            parse("\\begin{bmatrix}1&2\\\\3\\end{bmatrix}").unwrap_err(),
            "Every row of a matrix needs the same number of entries"
        );
    }

    #[test]
    fn friendly_errors() {
        assert_eq!(parse("y=").unwrap_err(), "Incomplete expression");
        assert_eq!(
            parse("\\sin").unwrap_err(),
            "Function 'sin' needs an argument"
        );
        assert_eq!(
            parse("\\operatorname{mod}\\left(3\\right)").unwrap_err(),
            "Function 'mod' takes 2 arguments"
        );
    }
}
