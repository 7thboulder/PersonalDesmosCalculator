//! Recursive-descent parser following Desmos's reading of math:
//! implicit multiplication, `\sin x` without brackets, `\sin^2 x`,
//! `\sin^{-1}x`, `\log_2 x`, subscripted names and so on.

use crate::ast::{Condition, Coord, Expr, Relation, Repeat, ShapeFn, Statement};
use crate::builtins::{self, Builtin};
use crate::lexer::{tokenize, Tok};

pub fn parse(latex: &str) -> Result<Statement, String> {
    let toks = tokenize(latex)?;
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
    match p.peek() {
        None => Ok(statement),
        Some(tok) => Err(unexpected(tok)),
    }
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
        if self.ode_start() {
            if self.peek().is_none() {
                return Err("Incomplete expression".into());
            }
            return Ok(Statement::Ode(self.expr()?));
        }
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

    /// Eats `y' =` or `\frac{dy}{dx} =`, which start a differential
    /// equation, and says whether it did.
    fn ode_start(&mut self) -> bool {
        let letter = |l: &str| Tok::Letter(l.into());
        let starts = [
            vec![letter("y"), Tok::Prime, Tok::Eq],
            vec![
                Tok::Frac,
                Tok::LBrace,
                letter("d"),
                letter("y"),
                Tok::RBrace,
                Tok::LBrace,
                letter("d"),
                letter("x"),
                Tok::RBrace,
                Tok::Eq,
            ],
        ];
        for start in starts {
            if self.toks.get(self.pos..self.pos + start.len()) == Some(&start[..]) {
                self.pos += start.len();
                return true;
            }
        }
        false
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
                | Tok::LCurly
                | Tok::Sum
                | Tok::Prod
                | Tok::Int
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
            Tok::Letter(letter) => {
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
                if let Some(var) = derivative_variable(&num, &den) {
                    if self.peek().is_none()
                        || !(self.starts_factor() || self.peek() == Some(&Tok::Minus))
                    {
                        return Err(format!("d/d{var} needs something to differentiate"));
                    }
                    let body = self.term()?;
                    return Ok(Expr::Derivative(var, Box::new(body)));
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
            Tok::LCurly => self.piecewise(),
            Tok::Sum => self.repeat(Repeat::Sum),
            Tok::Prod => self.repeat(Repeat::Product),
            Tok::Int => self.integral(),
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

    /// `\int_a^b body dx` after the `\int`. The body runs to the `dx`.
    fn integral(&mut self) -> Result<Expr, String> {
        let bounds_error = || "An integral needs bounds, like 0 below and 1 above".to_string();
        if !self.eat(&Tok::Underscore) {
            return Err(bounds_error());
        }
        let lo = self.script()?;
        if !self.eat(&Tok::Caret) {
            return Err(bounds_error());
        }
        let hi = self.script()?;
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

/// The variable of `\frac{d}{dx}`, if this fraction is one.
fn derivative_variable(num: &Expr, den: &Expr) -> Option<String> {
    if num != &Expr::Var("d".into()) {
        return None;
    }
    match den {
        Expr::Mul(a, b) if **a == Expr::Var("d".into()) => match &**b {
            Expr::Var(v) => Some(v.clone()),
            _ => None,
        },
        _ => None,
    }
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
            Statement::Ode(e) => e,
            other => panic!("not a differential equation: {other:?}"),
        };
        let x_minus_y = Expr::bin(Expr::Sub, v("x"), v("y"));
        assert_eq!(ode("y'=x-y"), x_minus_y);
        assert_eq!(ode("\\frac{dy}{dx}=x-y"), x_minus_y);
        assert_eq!(parse("y'="), Err("Incomplete expression".into()));
        // f'(x) is still a derivative.
        assert!(matches!(parse("f'(x)=2"), Ok(Statement::Relation(..))));
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
