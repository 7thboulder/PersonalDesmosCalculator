//! Recursive-descent parser following Desmos's reading of math:
//! implicit multiplication, `\sin x` without brackets, `\sin^2 x`,
//! `\sin^{-1}x`, `\log_2 x`, subscripted names and so on.

use crate::ast::{Expr, Relation, Statement};
use crate::builtins::{self, Builtin};
use crate::lexer::{tokenize, Tok};

pub fn parse(latex: &str) -> Result<Statement, String> {
    let toks = tokenize(latex)?;
    let mut p = Parser {
        toks,
        pos: 0,
        pipe_depth: 0,
    };
    let statement = p.statement()?;
    match p.peek() {
        None => Ok(statement),
        Some(tok) => Err(unexpected(tok)),
    }
}

const LATER_LISTS: &str = "Lists arrive in a later version";
const LATER_POINTS: &str = "Points arrive in a later version";
const LATER_PIECEWISE: &str = "Piecewise definitions arrive in a later version";
const LATER_CALCULUS: &str = "Sums, products, integrals and derivatives arrive in a later version";

struct Parser {
    toks: Vec<Tok>,
    pos: usize,
    /// How many bare `|` absolute values are open, so a closing `|` is not
    /// mistaken for the start of another factor.
    pipe_depth: usize,
}

fn unexpected(tok: &Tok) -> String {
    match tok {
        Tok::RParen | Tok::RBrace | Tok::AbsClose | Tok::RBracket | Tok::RCurly => {
            "Unbalanced brackets".into()
        }
        Tok::Comma => LATER_POINTS.into(),
        Tok::Eq | Tok::Lt | Tok::Gt | Tok::Le | Tok::Ge => {
            "Chained comparisons arrive in a later version".into()
        }
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
        let lhs = self.expr()?;
        let rel = match self.peek() {
            Some(Tok::Eq) => Relation::Eq,
            Some(Tok::Lt) => Relation::Lt,
            Some(Tok::Gt) => Relation::Gt,
            Some(Tok::Le) => Relation::Le,
            Some(Tok::Ge) => Relation::Ge,
            _ => return Ok(Statement::Expr(lhs)),
        };
        self.pos += 1;
        if self.peek().is_none() {
            return Err("Incomplete expression".into());
        }
        let rhs = self.expr()?;
        Ok(Statement::Relation(lhs, rel, rhs))
    }

    fn expr(&mut self) -> Result<Expr, String> {
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

    fn starts_factor(&self) -> bool {
        match self.peek() {
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
            } else if self.peek() == Some(&Tok::Prime) {
                return Err(LATER_CALCULUS.into());
            } else {
                return Ok(e);
            }
        }
    }

    fn primary(&mut self) -> Result<Expr, String> {
        let tok = self.next().ok_or("Incomplete expression")?;
        match tok {
            Tok::Num(v) => Ok(Expr::Num(v)),
            Tok::Letter(letter) => {
                let name = self.subscripted(letter)?;
                if self.peek() == Some(&Tok::Prime) {
                    return Err(LATER_CALCULUS.into());
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
                if is_derivative(&num, &den) {
                    return Err(LATER_CALCULUS.into());
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
                if self.peek() == Some(&Tok::Comma) {
                    return Err(LATER_POINTS.into());
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
            Tok::LBracket => Err(LATER_LISTS.into()),
            Tok::LCurly => Err(LATER_PIECEWISE.into()),
            Tok::Unsupported(name) => match name.as_str() {
                "sum" | "prod" | "int" => Err(LATER_CALCULUS.into()),
                _ => Err(format!("'\\{name}' isn't supported yet")),
            },
            other => Err(unexpected(&other)),
        }
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

fn is_derivative(num: &Expr, den: &Expr) -> bool {
    num == &Expr::Var("d".into())
        && matches!(den, Expr::Mul(a, b) if **a == Expr::Var("d".into()) && matches!(**b, Expr::Var(_)))
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
    fn roots_fractions_abs() {
        assert_eq!(e("\\sqrt[3]{x}"), call(Root, vec![n(3.0), v("x")]));
        assert_eq!(e("\\frac{1}{x}"), Expr::bin(Expr::Div, n(1.0), v("x")));
        assert_eq!(e("\\left|x\\right|"), call(Abs, vec![v("x")]));
        assert_eq!(e("|x|"), call(Abs, vec![v("x")]));
    }

    #[test]
    fn friendly_errors() {
        assert_eq!(parse("\\left(1,2\\right)").unwrap_err(), LATER_POINTS);
        assert_eq!(parse("\\left[1,2\\right]").unwrap_err(), LATER_LISTS);
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
