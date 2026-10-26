//! Turns a resolved expression into a flat stack program that evaluates
//! quickly for many x values.

use crate::builtins::{self, Builtin};
use crate::calculus::integrate;
use crate::interval::Interval;

/// Which variable a leaf reads. Curves of one variable use only `Arg::X`,
/// whichever axis they run along; implicit relations use both. `Local`
/// slots hold the variable of an integral being evaluated, one slot per
/// level of nesting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arg {
    X,
    Y,
    Local(u8),
}

/// How deeply integrals may nest.
pub const MAX_LOCALS: usize = 4;

/// An expression with every name resolved: variables are numbers, functions
/// are inlined and the graph variables are `Arg`s.
#[derive(Clone, Debug, PartialEq)]
pub enum Low {
    Const(f64),
    Arg(Arg),
    Neg(Box<Low>),
    Bin(BinOp, Box<Low>, Box<Low>),
    Call(Builtin, Vec<Low>),
    /// The second value where the first is 1 (true), the third where it is
    /// 0 (false), and undefined where the condition is.
    Select(Box<Low>, Box<Low>, Box<Low>),
    /// A stand-in for a variable while an expression is differentiated or
    /// integrated over it. Replaced before compiling, except as the variable
    /// of an integral.
    Hole(u32),
    /// The integral of `body` over `Hole(hole)` from `lo` to `hi`.
    Integral {
        hole: u32,
        body: Box<Low>,
        lo: Box<Low>,
        hi: Box<Low>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Pow,
    /// Comparisons give 1 for true and 0 for false, or undefined when
    /// either side is.
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
}

impl BinOp {
    fn apply_interval(self, a: Interval, b: Interval) -> Interval {
        match self {
            BinOp::Add => a.add(b),
            BinOp::Sub => a.sub(b),
            BinOp::Mul => a.mul(b),
            BinOp::Div => a.div(b),
            BinOp::Pow => a.pow(b),
            BinOp::Lt => a.compare(b, |x, y| x < y),
            BinOp::Le => a.compare(b, |x, y| x <= y),
            BinOp::Gt => a.compare(b, |x, y| x > y),
            BinOp::Ge => a.compare(b, |x, y| x >= y),
            BinOp::Eq => a.compare(b, |x, y| x == y),
        }
    }

    pub fn apply(self, a: f64, b: f64) -> f64 {
        match self {
            BinOp::Add => a + b,
            BinOp::Sub => a - b,
            BinOp::Mul => a * b,
            BinOp::Div => a / b,
            BinOp::Pow => builtins::pow(a, b),
            BinOp::Lt => truth(a, b, a < b),
            BinOp::Le => truth(a, b, a <= b),
            BinOp::Gt => truth(a, b, a > b),
            BinOp::Ge => truth(a, b, a >= b),
            BinOp::Eq => truth(a, b, a == b),
        }
    }
}

fn truth(a: f64, b: f64, holds: bool) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if holds {
        1.0
    } else {
        0.0
    }
}

/// What a piecewise branch gives for condition `c`.
fn select(c: f64, then: f64, otherwise: f64) -> f64 {
    if c.is_nan() {
        f64::NAN
    } else if c != 0.0 {
        then
    } else {
        otherwise
    }
}

impl Low {
    pub fn uses(&self, arg: Arg) -> bool {
        self.any(&|l| matches!(l, Low::Arg(a) if *a == arg))
    }

    pub fn uses_arg(&self) -> bool {
        self.uses(Arg::X) || self.uses(Arg::Y)
    }

    pub fn uses_hole(&self, hole: u32) -> bool {
        self.any(&|l| matches!(l, Low::Hole(h) if *h == hole))
    }

    /// Whether any leaf satisfies `test`.
    fn any(&self, test: &dyn Fn(&Low) -> bool) -> bool {
        match self {
            Low::Const(_) | Low::Arg(_) | Low::Hole(_) => test(self),
            Low::Neg(a) => a.any(test),
            Low::Bin(_, a, b) => a.any(test) || b.any(test),
            Low::Call(_, args) => args.iter().any(|a| a.any(test)),
            Low::Select(c, a, b) => c.any(test) || a.any(test) || b.any(test),
            Low::Integral { body, lo, hi, .. } => body.any(test) || lo.any(test) || hi.any(test),
        }
    }

    /// Whether the expression reads a graph variable or a hole that is not
    /// in `bound` or bound by an integral inside it.
    fn reads_outside(&self, bound: &[u32]) -> bool {
        match self {
            Low::Const(_) => false,
            Low::Arg(_) => true,
            Low::Hole(h) => !bound.contains(h),
            Low::Neg(a) => a.reads_outside(bound),
            Low::Bin(_, a, b) => a.reads_outside(bound) || b.reads_outside(bound),
            Low::Call(_, args) => args.iter().any(|a| a.reads_outside(bound)),
            Low::Select(c, a, b) => {
                c.reads_outside(bound) || a.reads_outside(bound) || b.reads_outside(bound)
            }
            Low::Integral { hole, body, lo, hi } => {
                let mut inner = bound.to_vec();
                inner.push(*hole);
                lo.reads_outside(bound) || hi.reads_outside(bound) || body.reads_outside(&inner)
            }
        }
    }

    /// A copy with every leaf for which `f` gives a replacement replaced.
    pub fn map_leaves(&self, f: &dyn Fn(&Low) -> Option<Low>) -> Low {
        let b = |l: &Low| Box::new(l.map_leaves(f));
        match self {
            Low::Const(_) | Low::Arg(_) | Low::Hole(_) => f(self).unwrap_or_else(|| self.clone()),
            Low::Neg(a) => Low::Neg(b(a)),
            Low::Bin(op, x, y) => Low::Bin(*op, b(x), b(y)),
            Low::Call(g, args) => Low::Call(*g, args.iter().map(|a| a.map_leaves(f)).collect()),
            Low::Select(c, x, y) => Low::Select(b(c), b(x), b(y)),
            Low::Integral { hole, body, lo, hi } => Low::Integral {
                hole: *hole,
                body: b(body),
                lo: b(lo),
                hi: b(hi),
            },
        }
    }

    /// Replaces `Hole(hole)` with `with`.
    pub fn fill(&self, hole: u32, with: &Low) -> Low {
        self.map_leaves(&|l| match l {
            Low::Hole(h) if *h == hole => Some(with.clone()),
            _ => None,
        })
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
            Low::Select(c, a, b) => match c.fold() {
                Low::Const(v) if v.is_nan() => Low::Const(f64::NAN),
                Low::Const(v) if v != 0.0 => a.fold(),
                Low::Const(_) => b.fold(),
                c => Low::Select(Box::new(c), Box::new(a.fold()), Box::new(b.fold())),
            },
            Low::Integral { hole, body, lo, hi } => {
                let (body, lo, hi) = (body.fold(), lo.fold(), hi.fold());
                // A definite integral of numbers alone is a number.
                let open = body.reads_outside(&[hole]);
                if let (Low::Const(a), Low::Const(b), false) = (&lo, &hi, open) {
                    let inner = body.fill(hole, &Low::Arg(Arg::Local(0))).fold();
                    // Slot 0 holds this integral's variable, so integrals
                    // inside it start at slot 1.
                    if let Ok(p) = Program::compile_at(&inner, 1) {
                        let mut locals = [f64::NAN; MAX_LOCALS];
                        return Low::Const(integrate(
                            &mut |t| {
                                locals[0] = t;
                                p.run(f64::NAN, f64::NAN, &mut locals)
                            },
                            *a,
                            *b,
                        ));
                    }
                }
                Low::Integral {
                    hole,
                    body: Box::new(body),
                    lo: Box::new(lo),
                    hi: Box::new(hi),
                }
            }
            other => other,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Op {
    Const(f64),
    Arg(Arg),
    Neg,
    Bin(BinOp),
    Call(Builtin, u8),
    /// Pops the else value, the then value and the condition.
    Select,
    /// Pops the upper and lower bounds and integrates sub-program `sub`
    /// between them, its variable held in local slot `slot`.
    Integral {
        sub: u16,
        slot: u8,
    },
}

/// Deepest evaluation stack a program may need.
const MAX_STACK: usize = 128;
/// The stack most programs fit in.
const SMALL_STACK: usize = 16;

/// A compiled expression of one variable.
#[derive(Clone, Debug, PartialEq)]
pub struct Program {
    ops: Vec<Op>,
    /// The bodies of integrals, evaluated by `Op::Integral`.
    subs: Vec<Program>,
    /// Deepest the stack gets, so most programs run on a small one.
    depth: usize,
}

impl Program {
    pub fn compile(expr: &Low) -> Result<Program, String> {
        let folded = expr.clone().fold();
        Program::compile_at(&folded, 0)
    }

    /// Compiles with `slot` the first local slot free for integrals.
    fn compile_at(expr: &Low, slot: u8) -> Result<Program, String> {
        let mut program = Program {
            ops: Vec::new(),
            subs: Vec::new(),
            depth: 0,
        };
        let depth = program.emit(expr, slot)?;
        if depth > MAX_STACK {
            return Err("Expression is too deeply nested".into());
        }
        program.depth = depth;
        Ok(program)
    }

    /// The value when the program does not depend on its argument.
    pub fn constant(&self) -> Option<f64> {
        match self.ops.as_slice() {
            [Op::Const(v)] => Some(*v),
            _ => None,
        }
    }

    /// Evaluates a program of one variable.
    pub fn eval(&self, x: f64) -> f64 {
        self.eval2(x, f64::NAN)
    }

    /// A hash that changes whenever the program does, so drawn lines can
    /// be kept while it stays the same. Only stable within one run.
    pub fn fingerprint(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        // Debug writes every float exactly, and says which op is which.
        format!("{self:?}").hash(&mut h);
        h.finish()
    }

    /// The program's instructions, for backends that compile them further.
    pub fn ops(&self) -> &[Op] {
        &self.ops
    }

    /// True when the program reads either graph variable.
    pub fn uses_arg(&self) -> bool {
        self.reads(&|a| matches!(a, Arg::X | Arg::Y))
    }

    /// True when the program reads y, so it needs a two-dimensional plot.
    pub fn uses_y(&self) -> bool {
        self.reads(&|a| a == Arg::Y)
    }

    fn reads(&self, test: &dyn Fn(Arg) -> bool) -> bool {
        self.ops
            .iter()
            .any(|op| matches!(op, Op::Arg(a) if test(*a)))
            || self.subs.iter().any(|s| s.reads(test))
    }

    /// The range the program can take over a rectangle of the plane. Never
    /// narrower than the true range, so an interval that excludes zero
    /// proves the relation has no solution inside the rectangle.
    pub fn eval_interval(&self, x: Interval, y: Interval) -> Interval {
        let mut stack = [Interval::ANY; MAX_STACK];
        let mut top = 0usize;
        for op in &self.ops {
            match *op {
                Op::Const(v) => {
                    stack[top] = Interval::point(v);
                    top += 1;
                }
                Op::Arg(a) => {
                    stack[top] = match a {
                        Arg::X => x,
                        Arg::Y => y,
                        Arg::Local(_) => Interval::ANY,
                    };
                    top += 1;
                }
                Op::Neg => stack[top - 1] = stack[top - 1].neg(),
                Op::Bin(op) => {
                    top -= 1;
                    stack[top - 1] = op.apply_interval(stack[top - 1], stack[top]);
                }
                Op::Call(f, n) => {
                    let n = n as usize;
                    let value = Interval::call(f, &stack[top - n..top]);
                    top -= n - 1;
                    stack[top - 1] = value;
                }
                Op::Select => {
                    top -= 2;
                    stack[top - 1] = Interval::select(stack[top - 1], stack[top], stack[top + 1]);
                }
                Op::Integral { .. } => {
                    top -= 1;
                    stack[top - 1] = Interval::ANY;
                }
            }
        }
        stack[0]
    }

    /// Evaluates a program of x and y.
    pub fn eval2(&self, x: f64, y: f64) -> f64 {
        let mut locals = [f64::NAN; MAX_LOCALS];
        self.run(x, y, &mut locals)
    }

    fn run(&self, x: f64, y: f64, locals: &mut [f64; MAX_LOCALS]) -> f64 {
        // Clearing a full-size stack on every call cost as much as running
        // a short program.
        if self.depth <= SMALL_STACK {
            self.run_on::<SMALL_STACK>(x, y, locals)
        } else {
            self.run_on::<MAX_STACK>(x, y, locals)
        }
    }

    fn run_on<const N: usize>(&self, x: f64, y: f64, locals: &mut [f64; MAX_LOCALS]) -> f64 {
        let mut stack = [0.0f64; N];
        let mut top = 0usize;
        for op in &self.ops {
            match *op {
                Op::Const(v) => {
                    stack[top] = v;
                    top += 1;
                }
                Op::Arg(a) => {
                    stack[top] = match a {
                        Arg::X => x,
                        Arg::Y => y,
                        Arg::Local(s) => locals[s as usize],
                    };
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
                Op::Select => {
                    top -= 2;
                    stack[top - 1] = select(stack[top - 1], stack[top], stack[top + 1]);
                }
                Op::Integral { sub, slot } => {
                    top -= 1;
                    let (lo, hi) = (stack[top - 1], stack[top]);
                    let body = &self.subs[sub as usize];
                    let slot = slot as usize;
                    let saved = locals[slot];
                    let value = integrate(
                        &mut |t| {
                            locals[slot] = t;
                            body.run(x, y, locals)
                        },
                        lo,
                        hi,
                    );
                    locals[slot] = saved;
                    stack[top - 1] = value;
                }
            }
        }
        stack[0]
    }

    /// Appends ops for `e` and returns the stack depth they need. `slot` is
    /// the next free local slot for an integral's variable.
    fn emit(&mut self, e: &Low, slot: u8) -> Result<usize, String> {
        Ok(match e {
            Low::Const(v) => {
                self.ops.push(Op::Const(*v));
                1
            }
            Low::Arg(a) => {
                self.ops.push(Op::Arg(*a));
                1
            }
            Low::Hole(_) => {
                // Every hole is filled before compiling; one left over is a
                // variable with no value here.
                self.ops.push(Op::Const(f64::NAN));
                1
            }
            Low::Neg(a) => {
                let d = self.emit(a, slot)?;
                self.ops.push(Op::Neg);
                d
            }
            Low::Bin(op, a, b) => {
                let da = self.emit(a, slot)?;
                let db = self.emit(b, slot)?;
                self.ops.push(Op::Bin(*op));
                da.max(db + 1)
            }
            Low::Call(f, args) => {
                let mut depth = 0;
                for (i, a) in args.iter().enumerate() {
                    depth = depth.max(self.emit(a, slot)? + i);
                }
                self.ops.push(Op::Call(*f, args.len() as u8));
                depth.max(1)
            }
            Low::Select(c, a, b) => {
                let dc = self.emit(c, slot)?;
                let da = self.emit(a, slot)?;
                let db = self.emit(b, slot)?;
                self.ops.push(Op::Select);
                dc.max(da + 1).max(db + 2)
            }
            Low::Integral { hole, body, lo, hi } => {
                if slot as usize >= MAX_LOCALS {
                    return Err("Integrals can nest at most 4 deep".into());
                }
                let dl = self.emit(lo, slot)?;
                let dh = self.emit(hi, slot)?;
                let inner = body.fill(*hole, &Low::Arg(Arg::Local(slot)));
                self.subs.push(Program::compile_at(&inner, slot + 1)?);
                self.ops.push(Op::Integral {
                    sub: (self.subs.len() - 1) as u16,
                    slot,
                });
                dl.max(dh + 1)
            }
        })
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
                Box::new(Low::Arg(Arg::X)),
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
    fn intervals_bound_every_sample() {
        // (x - 1)^2 + y^2 - 4
        let e = Low::Bin(
            BinOp::Sub,
            Box::new(Low::Bin(
                BinOp::Add,
                Box::new(Low::Bin(
                    BinOp::Pow,
                    Box::new(Low::Bin(
                        BinOp::Sub,
                        Box::new(Low::Arg(Arg::X)),
                        Box::new(Low::Const(1.0)),
                    )),
                    Box::new(Low::Const(2.0)),
                )),
                Box::new(Low::Bin(
                    BinOp::Pow,
                    Box::new(Low::Arg(Arg::Y)),
                    Box::new(Low::Const(2.0)),
                )),
            )),
            Box::new(Low::Const(4.0)),
        );
        let p = Program::compile(&e).unwrap();
        assert!(p.uses_y());
        assert_eq!(p.eval2(3.0, 0.0), 0.0);
        let (x, y) = (Interval::new(-1.0, 0.5), Interval::new(2.0, 3.0));
        let bound = p.eval_interval(x, y);
        for i in 0..=8 {
            for j in 0..=8 {
                let sx = -1.0 + 1.5 * i as f64 / 8.0;
                let sy = 2.0 + j as f64 / 8.0;
                assert!(bound.contains(p.eval2(sx, sy)), "{bound:?}");
            }
        }
        // That corner of the plane is outside the circle, provably.
        assert!(bound.excludes_zero());
        assert!(!p
            .eval_interval(Interval::new(0.0, 3.0), Interval::new(-2.0, 2.0))
            .excludes_zero());
    }

    #[test]
    fn piecewise_selects_and_restricts() {
        // {x < 0: -x, x}
        let x = || Box::new(Low::Arg(Arg::X));
        let e = Low::Select(
            Box::new(Low::Bin(BinOp::Lt, x(), Box::new(Low::Const(0.0)))),
            Box::new(Low::Neg(x())),
            x(),
        );
        let p = Program::compile(&e).unwrap();
        assert_eq!(p.eval(-3.0), 3.0);
        assert_eq!(p.eval(2.0), 2.0);
        assert!(p.eval(f64::NAN).is_nan());
        let bound = p.eval_interval(Interval::new(1.0, 2.0), Interval::ANY);
        assert_eq!((bound.lo, bound.hi), (1.0, 2.0), "only the else branch");
        let both = p.eval_interval(Interval::new(-1.0, 2.0), Interval::ANY);
        assert!(both.contains(1.0) && both.contains(2.0));
    }

    #[test]
    fn multi_argument_calls() {
        let e = Low::Call(
            Builtin::Max,
            vec![
                Low::Arg(Arg::X),
                Low::Const(1.0),
                Low::Neg(Box::new(Low::Arg(Arg::X))),
            ],
        );
        let p = Program::compile(&e).unwrap();
        assert_eq!(p.eval(-3.0), 3.0);
        assert_eq!(p.eval(0.5), 1.0);
    }
}
