//! Turns a resolved expression into a flat stack program that evaluates
//! quickly for many x values.

use crate::builtins::{self, Builtin};

/// An expression with every name resolved: variables are numbers, functions
/// are inlined and the graph variable is `Arg`.
#[derive(Clone, Debug, PartialEq)]
pub enum Low {
    Const(f64),
    Arg,
    Neg(Box<Low>),
    Bin(BinOp, Box<Low>, Box<Low>),
    Call(Builtin, Vec<Low>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Pow,
}

impl BinOp {
    fn apply(self, a: f64, b: f64) -> f64 {
        match self {
            BinOp::Add => a + b,
            BinOp::Sub => a - b,
            BinOp::Mul => a * b,
            BinOp::Div => a / b,
            BinOp::Pow => builtins::pow(a, b),
        }
    }
}

impl Low {
    pub fn uses_arg(&self) -> bool {
        match self {
            Low::Const(_) => false,
            Low::Arg => true,
            Low::Neg(a) => a.uses_arg(),
            Low::Bin(_, a, b) => a.uses_arg() || b.uses_arg(),
            Low::Call(_, args) => args.iter().any(Low::uses_arg),
        }
    }

    /// Folds every part that does not depend on the argument into a number.
    pub fn fold(self) -> Low {
        match self {
            Low::Neg(a) => match a.fold() {
                Low::Const(v) => Low::Const(-v),
                a => Low::Neg(Box::new(a)),
            },
            Low::Bin(op, a, b) => match (a.fold(), b.fold()) {
                (Low::Const(x), Low::Const(y)) => Low::Const(op.apply(x, y)),
                (a, b) => Low::Bin(op, Box::new(a), Box::new(b)),
            },
            Low::Call(f, args) => {
                let args: Vec<Low> = args.into_iter().map(Low::fold).collect();
                if args.iter().all(|a| matches!(a, Low::Const(_))) {
                    let values: Vec<f64> = args
                        .iter()
                        .map(|a| match a {
                            Low::Const(v) => *v,
                            _ => unreachable!(),
                        })
                        .collect();
                    Low::Const(f.eval(&values))
                } else {
                    Low::Call(f, args)
                }
            }
            other => other,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Op {
    Const(f64),
    Arg,
    Neg,
    Bin(BinOp),
    Call(Builtin, u8),
}

/// Deepest evaluation stack a program may need.
const MAX_STACK: usize = 128;

/// A compiled expression of one variable.
#[derive(Clone, Debug, PartialEq)]
pub struct Program {
    ops: Vec<Op>,
}

impl Program {
    pub fn compile(expr: &Low) -> Result<Program, String> {
        let folded = expr.clone().fold();
        let mut ops = Vec::new();
        let depth = emit(&folded, &mut ops);
        if depth > MAX_STACK {
            return Err("Expression is too deeply nested".into());
        }
        Ok(Program { ops })
    }

    /// The value when the program does not depend on its argument.
    pub fn constant(&self) -> Option<f64> {
        match self.ops.as_slice() {
            [Op::Const(v)] => Some(*v),
            _ => None,
        }
    }

    pub fn eval(&self, x: f64) -> f64 {
        let mut stack = [0.0f64; MAX_STACK];
        let mut top = 0usize;
        for op in &self.ops {
            match *op {
                Op::Const(v) => {
                    stack[top] = v;
                    top += 1;
                }
                Op::Arg => {
                    stack[top] = x;
                    top += 1;
                }
                Op::Neg => stack[top - 1] = -stack[top - 1],
                Op::Bin(op) => {
                    top -= 1;
                    stack[top - 1] = op.apply(stack[top - 1], stack[top]);
                }
                Op::Call(f, n) => {
                    let n = n as usize;
                    let value = f.eval(&stack[top - n..top]);
                    top -= n - 1;
                    stack[top - 1] = value;
                }
            }
        }
        stack[0]
    }
}

/// Appends ops for `e` and returns the stack depth they need.
fn emit(e: &Low, ops: &mut Vec<Op>) -> usize {
    match e {
        Low::Const(v) => {
            ops.push(Op::Const(*v));
            1
        }
        Low::Arg => {
            ops.push(Op::Arg);
            1
        }
        Low::Neg(a) => {
            let d = emit(a, ops);
            ops.push(Op::Neg);
            d
        }
        Low::Bin(op, a, b) => {
            let da = emit(a, ops);
            let db = emit(b, ops);
            ops.push(Op::Bin(*op));
            da.max(db + 1)
        }
        Low::Call(f, args) => {
            let mut depth = 0;
            for (i, a) in args.iter().enumerate() {
                depth = depth.max(emit(a, ops) + i);
            }
            ops.push(Op::Call(*f, args.len() as u8));
            depth.max(1)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evaluates_and_folds() {
        // 2*3 + x^2
        let e = Low::Bin(
            BinOp::Add,
            Box::new(Low::Bin(
                BinOp::Mul,
                Box::new(Low::Const(2.0)),
                Box::new(Low::Const(3.0)),
            )),
            Box::new(Low::Bin(
                BinOp::Pow,
                Box::new(Low::Arg),
                Box::new(Low::Const(2.0)),
            )),
        );
        let p = Program::compile(&e).unwrap();
        assert_eq!(p.eval(4.0), 22.0);
        assert_eq!(p.ops.len(), 5, "2*3 should fold to one constant");
        assert_eq!(
            Program::compile(&Low::Const(5.0)).unwrap().constant(),
            Some(5.0)
        );
    }

    #[test]
    fn multi_argument_calls() {
        let e = Low::Call(
            Builtin::Max,
            vec![Low::Arg, Low::Const(1.0), Low::Neg(Box::new(Low::Arg))],
        );
        let p = Program::compile(&e).unwrap();
        assert_eq!(p.eval(-3.0), 3.0);
        assert_eq!(p.eval(0.5), 1.0);
    }
}
